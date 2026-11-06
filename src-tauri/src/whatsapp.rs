use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};
use tauri::{Emitter, Manager};

static ACCOUNT_LOCKED: [AtomicBool; 2] = [AtomicBool::new(true), AtomicBool::new(true)];
static ACCOUNT_CREATION_LOCK: Mutex<()> = Mutex::new(());
static MANAGER_CREATION_LOCK: Mutex<()> = Mutex::new(());

// Satu jendela "whatsapp" berisi: webview UI (tab + PIN + pengaturan) dan satu webview anak per akun.
// VIEW: 0 = webview UI memenuhi jendela (PIN / pengaturan); 1 atau 2 = webview Akun itu tampil di
// bawah bar tab (UI menyusut jadi TAB_H).
static VIEW: AtomicU8 = AtomicU8::new(0);
const MANAGER: &str = "whatsapp";
const UI_LABEL: &str = "whatsapp-ui";
const TAB_H: f64 = 48.0;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    notifications: bool,
    /// Jaga sesi kedua akun tetap hidup (tersembunyi) saat panel ditutup/terkunci supaya
    /// notifikasi tetap masuk. Memakai RAM ratusan MB per akun, jadi default-nya mati.
    background_sessions: bool,
    preview_messages: bool,
    blur_mode: String,
    pins: [Option<PinRecord>; 2],
}

impl Preferences {
    /// Webview akun dipertahankan di latar saat terkunci/ditutup?
    fn keeps_background(&self) -> bool {
        self.notifications && self.background_sessions
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            notifications: true,
            background_sessions: false,
            preview_messages: false,
            blur_mode: "all_hover".into(),
            pins: [None, None],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct PinRecord {
    salt: Vec<u8>,
    digest: Vec<u8>,
}

#[derive(Serialize)]
pub struct AccountStatus {
    pub configured: bool,
    pub locked: bool,
    pub open: bool,
}

#[derive(Serialize)]
pub struct WhatsappStatus {
    pub notifications: bool,
    pub background_sessions: bool,
    pub preview_messages: bool,
    pub blur_mode: String,
    pub view: u8,
    pub accounts: [AccountStatus; 2],
}

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("whatsapp").join("preferences.json"))
        .map_err(|error| format!("Gagal menentukan folder data WhatsApp: {error}"))
}

fn load_preferences(path: &Path) -> Result<Preferences, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Pengaturan WhatsApp rusak: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(format!("Gagal membaca pengaturan WhatsApp: {error}")),
    }
}

fn save_preferences(path: &Path, preferences: &Preferences) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Folder pengaturan WhatsApp tidak valid".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Gagal membuat folder pengaturan WhatsApp: {error}"))?;
    let bytes = serde_json::to_vec(preferences)
        .map_err(|error| format!("Gagal menyimpan pengaturan WhatsApp: {error}"))?;
    fs::write(path, bytes).map_err(|error| format!("Gagal menyimpan pengaturan WhatsApp: {error}"))
}

fn account_index(account: u8) -> Result<usize, String> {
    match account {
        1 => Ok(0),
        2 => Ok(1),
        _ => Err("Pilih akun WhatsApp 1 atau 2".into()),
    }
}

fn lock_flag(index: usize) -> &'static AtomicBool {
    &ACCOUNT_LOCKED[index]
}

fn pin_digest(pin: &str, salt: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(salt);
    digest.update(pin.as_bytes());
    let mut output: [u8; 32] = digest.finalize().into();
    for _ in 1..150_000 {
        let mut digest = Sha256::new();
        digest.update(output);
        digest.update(salt);
        digest.update(pin.as_bytes());
        output = digest.finalize().into();
    }
    output
}

fn validate_pin(pin: &str) -> Result<(), String> {
    if !(4..=12).contains(&pin.len()) || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("PIN harus terdiri dari 4 sampai 12 angka".into());
    }
    Ok(())
}

fn verify_pin(record: &PinRecord, pin: &str) -> bool {
    if record.salt.len() != 16 || record.digest.len() != 32 {
        return false;
    }
    let candidate = pin_digest(pin, &record.salt);
    candidate
        .iter()
        .zip(&record.digest)
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

// Selektor area yang disamarkan. Daftar dibuat longgar (testid + role + struktur) karena
// WhatsApp Web sering mengganti markup. Semua di-scope ke root panel (#main / #pane-side)
// atau padanan testid-nya.
const LIST_SELECTORS: &[&str] = &[
    "[data-testid=\"cell-frame-container\"]",
    ":is(#pane-side, [data-testid=\"chat-list\"], [data-testid=\"archived-chatlist\"]) [role=\"listitem\"]",
    ":is(#pane-side, [data-testid=\"chat-list\"], [data-testid=\"archived-chatlist\"]) [role=\"row\"]",
    "[data-testid*=\"yourself\" i]",
];
const HEADER_SELECTORS: &[&str] = &[
    ":is(#main, [data-testid=\"conversation-panel-wrapper\"]) > header",
    ":is(#main, [data-testid=\"conversation-panel-wrapper\"]) header",
    "[data-testid=\"conversation-header\"]",
    "[data-testid*=\"conversation-header\" i]",
    "[data-testid*=\"conversation-panel-header\" i]",
];
/// Bubble pesan: diblur dengan `filter` per bubble (cara yang terbukti tampil). Pembukaan saat
/// hover lewat atribut dari skrip dan ditahan selama scroll, jadi tidak ada hitung ulang `:hover`.
const MESSAGE_SELECTOR: &str =
    ":is(#main, [data-testid=\"conversation-panel-wrapper\"]) [data-testid=\"msg-container\"]";
/// Avatar daftar chat berupa SVG (<image xlink:href> di dalam cincin status), bukan <img>.
const STATUS_RING_SELECTOR: &str = "[data-testid*=\"status-ring\" i], svg:has(> g > image), svg:has(> image)";
/// Semua avatar di daftar chat, apa pun struktur barisnya.
const AVATAR_SELECTOR: &str = ":is(#pane-side, [data-testid=\"chat-list\"], [data-testid=\"archived-chatlist\"]) :is(img, canvas, [data-icon*=\"default\" i], [data-icon*=\"user\" i], [data-icon*=\"group\" i], [data-icon*=\"avatar\" i])";

/// Foto profil di mana pun (daftar chat, header, rail kiri, chat ke nomor sendiri), dikenali dari
/// sumbernya, bukan dari struktur baris. Foto di dalam bubble pesan dikecualikan (sudah ditutup
/// lapisan blur pesan) begitu pula dialog (penampil media).
const PROFILE_IMG_SELECTOR: &str = "img:is([src*=\"whatsapp.net\"], [src^=\"blob:\"], [src^=\"data:image/jpeg\"], [src^=\"data:image/webp\"])";

/// Atribut penanda elemen yang sedang dibuka (di-hover). Diatur oleh skrip, bukan `:hover`.
const REVEAL_ATTR: &str = "data-nw-reveal";

/// Daftar chat + header: teks disamarkan lewat text-shadow, gambar lewat filter.
fn text_selectors(mode: &str) -> Vec<&'static str> {
    match mode {
        "off" | "chat_hover" => Vec::new(),
        _ => LIST_SELECTORS.iter().chain(HEADER_SELECTORS).copied().collect(),
    }
}

/// Bubble pesan: diblur di semua mode kecuali "off".
fn message_selectors(mode: &str) -> Vec<&'static str> {
    if mode == "off" { Vec::new() } else { vec![MESSAGE_SELECTOR] }
}

fn reveals_on_hover(mode: &str) -> bool {
    !matches!(mode, "off" | "all_always")
}

/// Selektor elemen yang dibuka saat pointer di atasnya. Kosong bila blur selalu aktif / mati.
fn reveal_selector(mode: &str) -> String {
    if !reveals_on_hover(mode) {
        return String::new();
    }
    text_selectors(mode)
        .into_iter()
        .chain(message_selectors(mode))
        .collect::<Vec<_>>()
        .join(", ")
}

fn blur_css(mode: &str) -> String {
    let texts = text_selectors(mode);
    let messages = message_selectors(mode);
    if texts.is_empty() && messages.is_empty() {
        return String::new();
    }
    let hover = reveals_on_hover(mode);
    let guard = if hover { format!(":not([{REVEAL_ATTR}])") } else { String::new() };
    let join = |list: &[&str], tail: &str| {
        list.iter()
            .map(|selector| format!("{selector}{guard}{tail}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut css = String::new();
    // Bubble pesan: satu filter per bubble.
    css.push_str(&format!("{} {{ filter: blur(8px) !important; }} ", join(&messages, "")));
    if !texts.is_empty() {
        css.push_str(&format!(
            "{}, {} {{ color: transparent !important; text-shadow: 0 0 8px rgba(150,160,155,.9) !important; }} ",
            join(&texts, ""),
            join(&texts, " *")
        ));
        // Gambar/ikon di dalam baris + semua avatar (termasuk avatar SVG bercincin status).
        let inside = ":not(:is([data-testid=\"msg-container\"], [role=\"dialog\"]) *)";
        let avatar_guard = if hover {
            format!(":not([{REVEAL_ATTR}] *)")
        } else {
            String::new()
        };
        css.push_str(&format!(
            "{}, {}, {}, {}, {AVATAR_SELECTOR}{avatar_guard}, {PROFILE_IMG_SELECTOR}{avatar_guard}{inside}, \
             {STATUS_RING_SELECTOR}{avatar_guard}{inside} {{ filter: blur(8px) !important; }}",
            join(&texts, " img"),
            join(&texts, " video"),
            join(&texts, " canvas"),
            join(&texts, " [data-icon]"),
        ));
    }
    css
}

fn initialization_script(account: u8, preferences: &Preferences) -> String {
    let css =
        serde_json::to_string(&blur_css(&preferences.blur_mode)).unwrap_or_else(|_| "\"\"".into());
    let reveal = serde_json::to_string(&reveal_selector(&preferences.blur_mode))
        .unwrap_or_else(|_| "\"\"".into());
    let notify = preferences.notifications;
    let preview = preferences.preview_messages;
    let reveal_attr = REVEAL_ATTR;
    format!(
        r#"
(() => {{
  const account = {account};
  let notificationsEnabled = {notify};
  let previewEnabled = {preview};
  const styleId = "__notch_wa_blur";
  const revealAttr = "{reveal_attr}";
  let revealSelector = "";
  let revealed = [];
  let scrolling = false;
  let scrollTimer = 0;
  const clearReveal = () => {{
    for (const el of revealed) el.removeAttribute(revealAttr);
    revealed = [];
  }};
  window.__NOTCH_WA_SET_BLUR__ = (css, reveal) => {{
    let style = document.getElementById(styleId);
    if (!style) {{
      style = document.createElement("style");
      style.id = styleId;
      (document.head || document.documentElement).appendChild(style);
    }}
    style.textContent = css;
    revealSelector = reveal || "";
    clearReveal();
  }};
  // Selama menggulir, semuanya tetap blur dan hover diabaikan: tidak ada kerja ekstra per frame.
  document.addEventListener("scroll", () => {{
    if (!scrolling) clearReveal();
    scrolling = true;
    window.clearTimeout(scrollTimer);
    scrollTimer = window.setTimeout(() => {{ scrolling = false; }}, 150);
  }}, {{ capture: true, passive: true }});
  document.addEventListener("mouseover", (event) => {{
    if (scrolling) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (!revealSelector) return;
    const first = target.closest(revealSelector);
    if (first === (revealed[0] || null)) return;
    clearReveal();
    // Buka semua leluhur yang juga cocok, supaya blur induk tidak menutupi anak yang dibuka.
    for (let el = first; el; el = el.parentElement ? el.parentElement.closest(revealSelector) : null) {{
      el.setAttribute(revealAttr, "");
      revealed.push(el);
    }}
  }}, {{ passive: true }});
  document.addEventListener("mouseout", (event) => {{
    if (!event.relatedTarget) clearReveal();
  }}, {{ passive: true }});
  window.addEventListener("blur", clearReveal);
  window.__NOTCH_WA_SET_NOTIFICATIONS__ = (enabled) => {{
    notificationsEnabled = !!enabled;
    previousUnread = null;
  }};
  window.__NOTCH_WA_SET_PREVIEW__ = (enabled) => {{
    previewEnabled = !!enabled;
  }};
  const applyBlur = () => window.__NOTCH_WA_SET_BLUR__({css}, {reveal});
  if (document.documentElement) {{
    applyBlur();
  }} else {{
    document.addEventListener("DOMContentLoaded", applyBlur, {{ once: true }});
  }}
  const unreadRoot = () => document.querySelector('#pane-side') || document;
  const readUnread = () => {{
    const badges = unreadRoot().querySelectorAll('[data-testid*="unread" i], [aria-label*="unread" i]');
    let total = 0;
    const rows = new Set();
    for (const badge of badges) {{
      const row = badge.closest('[data-testid="cell-frame-container"]') || badge;
      if (rows.has(row)) continue;
      rows.add(row);
      const label = (badge.getAttribute("aria-label") || badge.textContent || "").trim();
      const count = Number.parseInt(label.match(/\d+/)?.[0] || "", 10);
      total += Number.isFinite(count) ? count : 1;
    }}
    const titleCount = document.title.match(/^\s*\((\d+)\)/);
    return Math.max(total, titleCount ? Number.parseInt(titleCount[1], 10) : 0);
  }};
  const readLatestUnreadPreview = () => {{
    const badges = unreadRoot().querySelectorAll('[data-testid*="unread" i], [aria-label*="unread" i]');
    const ignored = new Set(["archived", "diarsipkan", "unread", "belum dibaca"]);
    const textOf = (element) => element?.textContent?.replace(/\s+/g, " ").trim() || "";
    for (const badge of badges) {{
      const row = badge.closest('[data-testid="cell-frame-container"]');
      if (!row) continue;
      const title = row.querySelector('[data-testid="cell-frame-title"]') || row.querySelector('span[title]');
      const unreadLabel = /^\s*\d+\s+(?:unread messages?|pesan belum dibaca)\s*/i;
      const sender = (title?.getAttribute("title")?.trim() || textOf(title)).replace(unreadLabel, "");
      const secondary = row.querySelector('[data-testid="cell-frame-secondary"]');
      // Jangan memakai textContent seluruh baris: lencana "jumlah belum dibaca" (angka di kanan baris)
      // ikut tergabung ke ujung pesan, jadi muncul "Haduh 4". Pakai atribut title pesan terakhir,
      // atau salinan baris tanpa lencana/ikon.
      const clean = (t) => (t || "").replace(/[\u202a-\u202e\u2066-\u2069]/g, "").replace(/\s+/g, " ").trim();
      let preview = clean(secondary?.querySelector('[data-testid="last-msg-status"]')?.getAttribute("title"));
      if (!preview && secondary) {{
        const copy = secondary.cloneNode(true);
        copy.querySelectorAll('[data-testid="icon-unread-count"], [aria-label*="unread" i], [data-testid="chat-msg-symbol"], [aria-hidden="true"]').forEach((n) => n.remove());
        preview = clean(copy.textContent);
      }}
      if (!preview) {{
        const spans = [...row.querySelectorAll('span[dir="auto"]')];
        preview = spans.map(textOf).filter((text) => text && text !== sender).at(-1) || "";
      }}
      if (sender && preview && !ignored.has(sender.toLocaleLowerCase()) &&
          !ignored.has(preview.toLocaleLowerCase())) {{
        return {{ sender: sender.slice(0, 120), preview: preview.slice(0, 120) }};
      }}
    }}
    return {{ sender: "", preview: "" }};
  }};
  let previousUnread = null;
  window.setInterval(() => {{
    if (!document.body) return;
    if (!notificationsEnabled) return;
    const unread = readUnread();
    if (previousUnread === null) {{
      previousUnread = unread;
      return;
    }}
    if (notificationsEnabled && unread > previousUnread && window.__TAURI__?.core?.invoke) {{
      const details = previewEnabled ? readLatestUnreadPreview() : {{ sender: "", preview: "" }};
      window.__TAURI__.core.invoke("whatsapp_report_new_message", {{ account, ...details }})
        .catch((error) => console.error("[WhatsApp] Gagal melaporkan pesan baru:", error));
    }}
    previousUnread = unread;
  }}, 10000);
}})();
"#
    )
}

fn account_label(account: u8) -> String {
    format!("whatsapp-account-{account}")
}

/// Ukuran area isi jendela dalam piksel logis. Nol saat jendela di-minimize, jadi pemanggil harus cek.
fn logical_inner(window: &tauri::Window) -> tauri::Result<(f64, f64)> {
    let scale = window.scale_factor()?.max(0.1);
    let size = window.inner_size()?;
    Ok((size.width as f64 / scale, size.height as f64 / scale))
}

/// Mengatur ulang posisi/ukuran semua webview sesuai VIEW saat ini.
fn apply_layout(app: &tauri::AppHandle) {
    let Some(window) = app.get_window(MANAGER) else {
        return;
    };
    let Ok((w, h)) = logical_inner(&window) else {
        return;
    };
    if w < 1.0 || h < 1.0 {
        return; // sedang di-minimize
    }
    let view = VIEW.load(Ordering::SeqCst);
    if let Some(ui) = app.get_webview(UI_LABEL) {
        let ui_h = if view == 0 { h } else { TAB_H.min(h) };
        let _ = ui.set_position(tauri::LogicalPosition::new(0.0, 0.0));
        let _ = ui.set_size(tauri::LogicalSize::new(w, ui_h));
    }
    for account in 1u8..=2 {
        if let Some(webview) = app.get_webview(&account_label(account)) {
            if view == account {
                let _ = webview.set_position(tauri::LogicalPosition::new(0.0, TAB_H));
                let _ = webview.set_size(tauri::LogicalSize::new(w, (h - TAB_H).max(1.0)));
                let _ = webview.show();
            } else {
                let _ = webview.hide();
            }
        }
    }
}

/// Membuat jendela WhatsApp (tersembunyi) beserta webview UI-nya. Jangan panggil dari thread utama.
fn ensure_manager(app: &tauri::AppHandle) -> tauri::Result<tauri::Window> {
    let _guard = MANAGER_CREATION_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(window) = app.get_window(MANAGER) {
        return Ok(window);
    }
    // Sengaja TANPA skip_taskbar: jendela ini harus muncul di taskbar dan Alt+Tab.
    let window = tauri::window::WindowBuilder::new(app, MANAGER)
        .title("WhatsApp")
        .inner_size(1080.0, 760.0)
        .min_inner_size(640.0, 520.0)
        .center()
        .resizable(true)
        .visible(false)
        .build()?;
    let (w, h) = logical_inner(&window).unwrap_or((1080.0, 760.0));
    window.add_child(
        tauri::webview::WebviewBuilder::new(
            UI_LABEL,
            tauri::WebviewUrl::App("whatsapp.html".into()),
        ),
        tauri::LogicalPosition::new(0.0, 0.0),
        tauri::LogicalSize::new(w.max(1.0), h.max(1.0)),
    )?;
    let app_handle = app.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let app_handle = app_handle.clone();
            std::thread::spawn(move || {
                if let Err(error) = lock_all(&app_handle) {
                    eprintln!("whatsapp: gagal mengunci sesi: {error}");
                    return;
                }
                if let Some(window) = app_handle.get_window(MANAGER) {
                    if let Err(error) = window.hide() {
                        eprintln!("whatsapp: gagal menutup panel: {error}");
                    }
                }
            });
        }
        tauri::WindowEvent::Resized(_) => apply_layout(&app_handle),
        _ => {}
    });
    Ok(window)
}

/// Membuat webview anak untuk satu akun (profil browser terpisah). Jangan panggil dari thread utama.
fn create_account_webview(
    app: &tauri::AppHandle,
    account: u8,
) -> tauri::Result<tauri::Webview> {
    let _creation_guard = ACCOUNT_CREATION_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let label = account_label(account);
    if let Some(webview) = app.get_webview(&label) {
        return Ok(webview);
    }
    let window = ensure_manager(app)?;
    let prefs_path = preferences_path(app).map_err(std::io::Error::other)?;
    let preferences = load_preferences(&prefs_path).map_err(std::io::Error::other)?;
    let data_dir = app
        .path()
        .app_data_dir()?
        .join("whatsapp")
        .join(format!("account-{account}"));
    let (w, h) = match logical_inner(&window) {
        Ok((w, h)) if w >= 1.0 && h >= 1.0 => (w, h),
        _ => (1080.0, 760.0),
    };
    let builder = tauri::webview::WebviewBuilder::new(
        label,
        tauri::WebviewUrl::External(
            "https://web.whatsapp.com"
                .parse()
                .expect("valid WhatsApp URL"),
        ),
    )
    .data_directory(data_dir)
    .disable_drag_drop_handler()
    .initialization_script(initialization_script(account, &preferences))
    .on_navigation(|url| url.scheme() == "https" && url.host_str() == Some("web.whatsapp.com"));
    let webview = window.add_child(
        builder,
        tauri::LogicalPosition::new(0.0, TAB_H),
        tauri::LogicalSize::new(w, (h - TAB_H).max(1.0)),
    )?;
    apply_layout(app); // sembunyikan jika bukan akun yang sedang aktif
    Ok(webview)
}

pub fn start_background_accounts(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let preferences = match preferences_path(&app).and_then(|path| load_preferences(&path)) {
            Ok(preferences) => preferences,
            Err(error) => {
                eprintln!("whatsapp: gagal memuat sesi latar: {error}");
                return;
            }
        };
        if !preferences.keeps_background() {
            return; // tanpa sesi latar: tidak ada webview WhatsApp sebelum panel dibuka dan PIN dimasukkan
        }
        for account in 1u8..=2 {
            if preferences.pins[usize::from(account - 1)].is_some() {
                if let Err(error) = create_account_webview(&app, account) {
                    eprintln!("whatsapp: gagal memulihkan Akun {account} di latar: {error}");
                }
            }
        }
    });
}

fn show_manager(window: &tauri::Window) {
    if let Err(error) = window.show() {
        eprintln!("whatsapp: gagal menampilkan panel: {error}");
        return;
    }
    // Halaman UI dibuat/dirender saat jendela tersembunyi dan bisa menyimpan status lama
    // (mis. tampilan "akun terbuka" sebelum semua akun dikunci). Susun ulang dan minta UI memuat ulang.
    let app = window.app_handle().clone();
    apply_layout(&app);
    let _ = app.emit("whatsapp-state", ());
    crate::foreground::bring_window_to_front(window);
}

pub fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(MANAGER) {
        if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
            if let Err(error) = lock_all(app) {
                eprintln!("whatsapp: gagal mengunci sesi: {error}");
            } else if let Err(error) = window.hide() {
                eprintln!("whatsapp: gagal menyembunyikan panel: {error}");
            }
        } else {
            show_manager(&window);
        }
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || match ensure_manager(&app) {
        Ok(window) => show_manager(&window),
        Err(error) => eprintln!("whatsapp: gagal membuka panel: {error}"),
    });
}

/// Mengunci satu akun: webview disembunyikan (notifikasi aktif) atau ditutup (notifikasi mati).
fn lock_account(app: &tauri::AppHandle, account: u8) -> Result<(), String> {
    let index = account_index(account)?;
    let keep_background = load_preferences(&preferences_path(app)?)?.keeps_background();
    if !keep_background {
        if let Some(webview) = app.get_webview(&account_label(account)) {
            webview
                .close()
                .map_err(|error| format!("Gagal menutup sesi latar Akun {account}: {error}"))?;
        }
    }
    lock_flag(index).store(true, Ordering::SeqCst);
    if VIEW.load(Ordering::SeqCst) == account {
        VIEW.store(0, Ordering::SeqCst);
        if let Some(window) = app.get_window(MANAGER) {
            let _ = window.set_title("WhatsApp");
        }
    }
    apply_layout(app); // menyembunyikan webview yang tetap hidup
    let _ = app.emit("whatsapp-state", ());
    Ok(())
}

fn lock_all(app: &tauri::AppHandle) -> Result<(), String> {
    for account in 1u8..=2 {
        lock_account(app, account)?;
    }
    VIEW.store(0, Ordering::SeqCst);
    apply_layout(app);
    Ok(())
}

#[tauri::command]
pub fn whatsapp_status(app: tauri::AppHandle) -> Result<WhatsappStatus, String> {
    let preferences = load_preferences(&preferences_path(&app)?)?;
    let view = VIEW.load(Ordering::SeqCst);
    Ok(WhatsappStatus {
        notifications: preferences.notifications,
        background_sessions: preferences.background_sessions,
        preview_messages: preferences.preview_messages,
        blur_mode: preferences.blur_mode,
        view,
        accounts: std::array::from_fn(|index| {
            let account = (index + 1) as u8;
            AccountStatus {
                configured: preferences.pins[index].is_some(),
                locked: lock_flag(index).load(Ordering::SeqCst),
                open: view == account,
            }
        }),
    })
}

#[tauri::command]
pub fn whatsapp_report_new_message(
    app: tauri::AppHandle,
    account: u8,
    sender: Option<String>,
    preview: Option<String>,
) -> Result<(), String> {
    account_index(account)?;
    let preferences = load_preferences(&preferences_path(&app)?)?;
    if preferences.notifications {
        let show_preview = preferences.preview_messages;
        let clean = |value: Option<String>| -> Option<String> {
            value
                .map(|value| {
                    value
                        .chars()
                        .filter(|character| !character.is_control())
                        .take(120)
                        .collect::<String>()
                        .trim()
                        .to_string()
                })
                .filter(|value| !value.is_empty())
        };
        app.emit(
            "whatsapp-notification",
            serde_json::json!({
                "account": account,
                "sender": show_preview.then(|| clean(sender)).flatten(),
                "preview": show_preview.then(|| clean(preview)).flatten(),
            }),
        )
        .map_err(|error| format!("Gagal meneruskan notifikasi WhatsApp: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn whatsapp_set_pin(
    app: tauri::AppHandle,
    account: u8,
    pin: String,
) -> Result<(), String> {
    validate_pin(&pin)?;
    let index = account_index(account)?;
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    if preferences.pins[index].is_some() {
        return Err("PIN akun ini sudah dibuat. Gunakan PIN yang ada untuk membuka.".into());
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|error| format!("Gagal membuat salt PIN: {error}"))?;
    preferences.pins[index] = Some(PinRecord {
        digest: pin_digest(&pin, &salt).to_vec(),
        salt: salt.to_vec(),
    });
    save_preferences(&path, &preferences)?;
    reveal_account(app, account, index).await
}

#[tauri::command]
pub async fn whatsapp_change_pin(
    app: tauri::AppHandle,
    account: u8,
    old_pin: String,
    new_pin: String,
) -> Result<(), String> {
    validate_pin(&new_pin)?;
    let index = account_index(account)?;
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    let record = preferences.pins[index]
        .as_ref()
        .ok_or_else(|| "Akun ini belum punya PIN".to_string())?;
    if !verify_pin(record, &old_pin) {
        return Err("PIN lama salah".into());
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|error| format!("Gagal membuat salt PIN: {error}"))?;
    preferences.pins[index] = Some(PinRecord {
        digest: pin_digest(&new_pin, &salt).to_vec(),
        salt: salt.to_vec(),
    });
    save_preferences(&path, &preferences)
}

#[tauri::command]
pub async fn whatsapp_unlock(
    app: tauri::AppHandle,
    account: u8,
    pin: String,
) -> Result<(), String> {
    let index = account_index(account)?;
    let preferences = load_preferences(&preferences_path(&app)?)?;
    let record = preferences.pins[index]
        .as_ref()
        .ok_or_else(|| "Buat PIN untuk akun ini terlebih dahulu".to_string())?;
    if !verify_pin(record, &pin) {
        return Err("PIN salah".into());
    }
    reveal_account(app, account, index).await
}

async fn reveal_account(app: tauri::AppHandle, account: u8, index: usize) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        create_account_webview(&app, account).map_err(|error| error.to_string())?;
        lock_flag(index).store(false, Ordering::SeqCst);
        VIEW.store(account, Ordering::SeqCst);
        if let Some(window) = app.get_window(MANAGER) {
            let _ = window.set_title(&format!("WhatsApp — Akun {account}"));
        }
        apply_layout(&app);
        if let Some(webview) = app.get_webview(&account_label(account)) {
            if let Err(error) = webview.set_focus() {
                eprintln!("whatsapp: gagal memfokuskan Akun {account}: {error}");
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Gagal membuka Akun {account}: {error}"))?
}

/// Pindah tab: 0 = pengaturan/PIN (UI penuh), 1 atau 2 = tampilkan akun. Akun yang masih
/// terkunci tidak bisa ditampilkan; tampilan jatuh ke 0 supaya UI menampilkan form PIN.
#[tauri::command]
pub fn whatsapp_set_view(app: tauri::AppHandle, view: u8) -> Result<(), String> {
    let next = match view {
        0 => 0,
        1 | 2 => {
            let unlocked = !lock_flag(usize::from(view - 1)).load(Ordering::SeqCst);
            if unlocked && app.get_webview(&account_label(view)).is_some() {
                view
            } else {
                0
            }
        }
        _ => return Err("Tampilan WhatsApp tidak valid".into()),
    };
    VIEW.store(next, Ordering::SeqCst);
    if let Some(window) = app.get_window(MANAGER) {
        let title = if next == 0 {
            "WhatsApp".to_string()
        } else {
            format!("WhatsApp — Akun {next}")
        };
        let _ = window.set_title(&title);
    }
    apply_layout(&app);
    if next != 0 {
        if let Some(webview) = app.get_webview(&account_label(next)) {
            let _ = webview.set_focus();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn whatsapp_lock(app: tauri::AppHandle, account: u8) -> Result<(), String> {
    lock_account(&app, account)
}

#[tauri::command]
pub fn whatsapp_set_notifications(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    preferences.notifications = enabled;
    save_preferences(&path, &preferences)?;
    let mut update_errors = Vec::new();
    for account in 1u8..=2 {
        if let Some(webview) = app.get_webview(&account_label(account)) {
            let index = usize::from(account - 1);
            let is_locked = lock_flag(index).load(Ordering::SeqCst);
            let script = format!("window.__NOTCH_WA_SET_NOTIFICATIONS__?.({enabled});");
            let eval_error = webview.eval(&script).err();
            if !preferences.keeps_background() && is_locked {
                if let Err(error) = webview.close() {
                    return Err(format!("Gagal menutup sesi latar Akun {account}: {error}"));
                }
            } else if let Some(error) = eval_error {
                update_errors.push(format!("Akun {account}: {error}"));
            }
        }
    }
    if preferences.keeps_background() {
        start_background_accounts(&app);
    }
    if !update_errors.is_empty() {
        return Err(format!(
            "Preferensi tersimpan, tetapi pembaruan sesi gagal: {}",
            update_errors.join("; ")
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn whatsapp_set_background(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    preferences.background_sessions = enabled;
    save_preferences(&path, &preferences)?;
    if preferences.keeps_background() {
        start_background_accounts(&app);
        return Ok(());
    }
    // Dimatikan: tutup semua sesi latar yang terkunci sekarang juga supaya RAM langsung bebas.
    for account in 1u8..=2 {
        let index = usize::from(account - 1);
        if !lock_flag(index).load(Ordering::SeqCst) {
            continue; // akun yang sedang dipakai dibiarkan sampai dikunci
        }
        if let Some(webview) = app.get_webview(&account_label(account)) {
            webview
                .close()
                .map_err(|error| format!("Gagal menutup sesi latar Akun {account}: {error}"))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn whatsapp_set_preview(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    preferences.preview_messages = enabled;
    save_preferences(&path, &preferences)?;
    let script = format!("window.__NOTCH_WA_SET_PREVIEW__?.({enabled});");
    let mut update_errors = Vec::new();
    for account in 1u8..=2 {
        if let Some(webview) = app.get_webview(&account_label(account)) {
            if let Err(error) = webview.eval(&script) {
                update_errors.push(format!("Akun {account}: {error}"));
            }
        }
    }
    if !update_errors.is_empty() {
        return Err(format!(
            "Preferensi tersimpan, tetapi pembaruan sesi gagal: {}",
            update_errors.join("; ")
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn whatsapp_set_blur(app: tauri::AppHandle, mode: String) -> Result<(), String> {
    if !["all_hover", "all_always", "chat_hover", "off"].contains(&mode.as_str()) {
        return Err("Mode blur tidak valid".into());
    }
    let path = preferences_path(&app)?;
    let mut preferences = load_preferences(&path)?;
    preferences.blur_mode = mode.clone();
    save_preferences(&path, &preferences)?;
    let css = serde_json::to_string(&blur_css(&mode)).map_err(|error| error.to_string())?;
    let reveal =
        serde_json::to_string(&reveal_selector(&mode)).map_err(|error| error.to_string())?;
    let script = format!("window.__NOTCH_WA_SET_BLUR__?.({css}, {reveal});");
    let mut update_errors = Vec::new();
    for account in 1u8..=2 {
        if let Some(webview) = app.get_webview(&account_label(account)) {
            if let Err(error) = webview.eval(&script) {
                update_errors.push(format!("Akun {account}: {error}"));
            }
        }
    }
    if !update_errors.is_empty() {
        return Err(format!(
            "Mode blur tersimpan, tetapi pembaruan sesi gagal: {}",
            update_errors.join("; ")
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn whatsapp_close_manager(app: tauri::AppHandle) -> Result<(), String> {
    lock_all(&app)?;
    if let Some(window) = app.get_window(MANAGER) {
        window
            .hide()
            .map_err(|error| format!("Gagal menyembunyikan panel WhatsApp: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hash_verifies_only_the_matching_pin() {
        let salt = [7u8; 16];
        let record = PinRecord {
            salt: salt.to_vec(),
            digest: pin_digest("2468", &salt).to_vec(),
        };
        assert!(verify_pin(&record, "2468"));
        assert!(!verify_pin(&record, "2469"));
    }

    #[test]
    fn pin_requires_four_to_twelve_digits() {
        assert!(validate_pin("1234").is_ok());
        assert!(validate_pin("123").is_err());
        assert!(validate_pin("1234567890123").is_err());
        assert!(validate_pin("12a4").is_err());
    }

    #[test]
    fn blur_modes_map_to_expected_rules() {
        assert_eq!(blur_css("off"), "");
        let all_hover = blur_css("all_hover");
        assert!(all_hover.contains(":not([data-nw-reveal])"));
        assert!(all_hover.contains("#pane-side"));
        assert!(all_hover.contains("> header"));
        assert!(all_hover.contains("yourself"));
        assert!(all_hover.contains("msg-container"));
        assert!(all_hover.contains("status-ring"));
        assert!(all_hover.contains("whatsapp.net"));
        assert!(all_hover.contains("archived-chatlist"));
        assert!(all_hover.contains("[data-testid=\"cell-frame-container\"]"));
        assert!(!blur_css("all_always").contains("data-nw-reveal"));
        let chat_hover = blur_css("chat_hover");
        assert!(chat_hover.contains("msg-container"));
        assert!(!chat_hover.contains("#pane-side"));
        assert!(!chat_hover.contains("status-ring"));
        assert_eq!(reveal_selector("all_always"), "");
        assert_eq!(reveal_selector("off"), "");
        assert!(reveal_selector("all_hover").contains("#pane-side"));
        assert!(reveal_selector("chat_hover").contains("msg-container"));
    }

    #[test]
    fn background_notifications_can_be_disabled_in_the_injected_script() {
        let preferences = Preferences {
            notifications: false,
            ..Preferences::default()
        };
        let script = initialization_script(2, &preferences);
        assert!(script.contains("let notificationsEnabled = false"));
        assert!(script.contains("let previewEnabled = false"));
        assert!(script.contains("}, 10000);"));
        assert!(script.contains("whatsapp_report_new_message"));
    }

    #[test]
    fn message_preview_is_disabled_by_default() {
        assert!(!Preferences::default().preview_messages);
        assert!(!Preferences::default().background_sessions);
        let mut keep = Preferences::default();
        assert!(!keep.keeps_background());
        keep.background_sessions = true;
        assert!(keep.keeps_background());
        keep.notifications = false;
        assert!(!keep.keeps_background());
    }
}
