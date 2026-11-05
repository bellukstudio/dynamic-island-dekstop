//! Jendela Android (Ctrl+Alt+Shift+A): jelajahi penyimpanan HP Android dan transfer file lewat USB
//! (atau Wi-Fi debugging) memakai `adb`. Tidak perlu aplikasi di HP.
//! adb dicari di folder resource `adb/` (dibundel), env ADB_PATH, PATH, atau Android SDK.
mod adb;
#[cfg(target_os = "windows")]
mod mtp;
mod mirror;

use adb::{Adb, Device, Entry};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();
static ADB: Mutex<Option<PathBuf>> = Mutex::new(None);

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn get_adb() -> Option<Adb> {
    let mut g = ADB.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = adb::find(RESOURCE_DIR.get().map(|p| p.as_path()));
        match &*g {
            Some(p) => eprintln!("android: adb dipakai: {}", p.display()),
            None => eprintln!("android: adb TIDAK ditemukan"),
        }
    }
    g.clone().map(Adb::new)
}

const NO_ADB: &str = "ADB_MISSING";

// ---- MTP (file saja, tanpa USB debugging; hanya Windows) ----
fn is_mtp(serial: &str) -> bool {
    serial.starts_with("mtp:")
}

#[cfg(target_os = "windows")]
fn mtp_devices() -> Vec<Device> {
    mtp::devices().unwrap_or_else(|err| {
        eprintln!("android: MTP tidak bisa dibaca: {err}");
        Vec::new()
    })
}
#[cfg(not(target_os = "windows"))]
fn mtp_devices() -> Vec<Device> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn mtp_list(serial: &str, path: &str) -> Result<Vec<Entry>, String> {
    mtp::list_dir(serial, path)
}
#[cfg(not(target_os = "windows"))]
fn mtp_list(_serial: &str, _path: &str) -> Result<Vec<Entry>, String> {
    Err("MTP hanya tersedia di Windows".into())
}

#[cfg(target_os = "windows")]
fn mtp_push(serial: &str, local: &Path, remote_dir: &str) -> Result<(), String> {
    mtp::push(serial, local, remote_dir)
}
#[cfg(not(target_os = "windows"))]
fn mtp_push(_serial: &str, _local: &Path, _remote_dir: &str) -> Result<(), String> {
    Err("MTP hanya tersedia di Windows".into())
}

#[cfg(target_os = "windows")]
fn mtp_pull(serial: &str, remote: &str, dest_dir: &Path) -> Result<PathBuf, String> {
    mtp::pull(serial, remote, dest_dir)
}
#[cfg(not(target_os = "windows"))]
fn mtp_pull(_serial: &str, _remote: &str, _dest_dir: &Path) -> Result<PathBuf, String> {
    Err("MTP hanya tersedia di Windows".into())
}

/// Nama model disamakan untuk mencocokkan HP yang sama di adb dan MTP.
fn norm_model(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

#[derive(Serialize)]
pub struct Status {
    found: bool,
    path: Option<String>,
}

/// Cek ulang keberadaan adb (dipanggil juga oleh tombol "Coba lagi").
#[tauri::command]
pub async fn android_status() -> Status {
    tauri::async_runtime::spawn_blocking(|| {
        *ADB.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let a = get_adb();
        Status { found: a.is_some(), path: a.map(|a| a.path.to_string_lossy().into_owned()) }
    })
    .await
    .unwrap_or(Status { found: false, path: None })
}

#[tauri::command]
pub async fn android_devices() -> Result<Vec<Device>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let adb_result = get_adb().ok_or_else(|| NO_ADB.to_string()).and_then(|a| a.devices());
        let mtp = mtp_devices();
        let mut devices = match adb_result {
            Ok(list) => list,
            // adb tidak ada/gagal: tetap tampilkan HP yang terbaca lewat MTP
            Err(_) if !mtp.is_empty() => Vec::new(),
            Err(e) => return Err(e),
        };
        // HP yang sama di adb dan MTP: pakai adb (lebih cepat dan bisa mirror), sembunyikan MTP-nya.
        let adb_models: Vec<String> = devices.iter().map(|d| norm_model(&d.model)).collect();
        devices.extend(mtp.into_iter().filter(|m| !adb_models.contains(&norm_model(&m.model))));
        Ok(devices)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn android_ls(serial: String, path: String) -> Result<Vec<Entry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if is_mtp(&serial) {
            return mtp_list(&serial, &path);
        }
        get_adb().ok_or_else(|| NO_ADB.to_string())?.list_dir(&serial, &path)
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct PushResult {
    name: String,
    ok: bool,
    error: Option<String>,
}

/// Kirim file/folder dari PC ke folder `remote_dir` di HP.
#[tauri::command]
pub async fn android_push(serial: String, paths: Vec<String>, remote_dir: String) -> Result<Vec<PushResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let over_mtp = is_mtp(&serial);
        let adb = if over_mtp { None } else { Some(get_adb().ok_or_else(|| NO_ADB.to_string())?) };
        let mut out = Vec::new();
        for p in paths {
            let pb = PathBuf::from(&p);
            let name = pb.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            let sent = match &adb {
                Some(a) => a.push(&serial, &pb, &remote_dir),
                None => mtp_push(&serial, &pb, &remote_dir),
            };
            match sent {
                Ok(()) => out.push(PushResult { name, ok: true, error: None }),
                Err(e) => out.push(PushResult { name, ok: false, error: Some(e) }),
            }
        }
        Ok(out)
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct PullResult {
    local_path: String,
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut c = dir.join(name);
    let mut n = 2;
    while c.exists() {
        c = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    c
}

/// Ambil file/folder dari HP ke <Unduhan>/Notch/Android.
#[tauri::command]
pub async fn android_pull(app: AppHandle, serial: String, remote: String) -> Result<PullResult, String> {
    let dest_dir = app.path().download_dir().unwrap_or_else(|_| std::env::temp_dir()).join("Notch").join("Android");
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&dest_dir).map_err(|e| format!("Folder tujuan tidak bisa dibuat: {e}"))?;
        if is_mtp(&serial) {
            let local = mtp_pull(&serial, &remote, &dest_dir)?;
            return Ok(PullResult { local_path: local.to_string_lossy().into_owned() });
        }
        let a = get_adb().ok_or_else(|| NO_ADB.to_string())?;
        let name = remote.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("file").to_string();
        // nama bersih untuk Windows (karakter terlarang)
        let name: String = name.chars().map(|c| if "<>:\"\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
        let dest = unique_path(&dest_dir, &name);
        a.pull(&serial, &remote, &dest)?;
        Ok(PullResult { local_path: dest.to_string_lossy().into_owned() })
    })
    .await
    .map_err(err)?
}

/// Pairing Wi-Fi debugging (Android 11+): alamat:port pairing + kode 6 digit dari HP.
#[tauri::command]
pub async fn android_pair(addr: String, code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || get_adb().ok_or_else(|| NO_ADB.to_string())?.pair(addr.trim(), code.trim()))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn android_connect(addr: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || get_adb().ok_or_else(|| NO_ADB.to_string())?.connect(addr.trim()))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn android_disconnect(addr: String) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(a) = get_adb() {
            a.disconnect(addr.trim());
        }
    })
    .await;
}

#[tauri::command]
pub async fn android_pick_files() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Pilih file untuk dikirim ke HP")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn android_reveal(path: String) {
    crate::compress::compress_reveal(path);
}

// ---------------------------------------------------------------------------------------------
// Pairing lewat QR (Debugging nirkabel, Android 11+)
//
// Alur: QR berisi "WIFI:T:ADB;S:<nama>;P:<sandi>;;". HP yang memindainya menyiarkan layanan mDNS
// `_adb-tls-pairing._tcp` bernama <nama>. Kita menunggu layanan itu muncul di `adb mdns services`,
// lalu `adb pair <ip:port> <sandi>`, lalu `adb connect` ke layanan `_adb-tls-connect` di IP yang sama.
// ---------------------------------------------------------------------------------------------
#[derive(Clone, Serialize)]
pub struct QrStatus {
    /// idle | waiting | pairing | connecting | done | error
    state: &'static str,
    message: String,
}

static QR_GEN: AtomicU64 = AtomicU64::new(0);
static QR_STATE: Mutex<QrStatus> = Mutex::new(QrStatus { state: "idle", message: String::new() });
const QR_SERVICE_NAME: &str = "ADBQR-connectPhoneOverWifi";

fn qr_alive(epoch: u64) -> bool {
    QR_GEN.load(Ordering::SeqCst) == epoch
}

fn set_qr(epoch: u64, state: &'static str, message: impl Into<String>) {
    if qr_alive(epoch) {
        *QR_STATE.lock().unwrap_or_else(|e| e.into_inner()) = QrStatus { state, message: message.into() };
    }
}

fn pairing_code() -> Result<String, String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|e| format!("Gagal membuat kode pairing QR: {e}"))?;
    Ok(format!("{:06}", u32::from_ne_bytes(bytes) % 1_000_000))
}

fn qr_worker(epoch: u64, name: String, pass: String) {
    let Some(a) = get_adb() else {
        set_qr(epoch, "error", "adb tidak ditemukan");
        return;
    };
    match a.mdns_check() {
        Ok(t) => {
            let l = t.to_lowercase();
            if l.contains("error") || l.contains("disabled") || l.contains("unknown") {
                set_qr(epoch, "error", format!("adb di sistem ini tidak mendukung mDNS: {}", t.trim()));
                return;
            }
        }
        Err(e) => {
            set_qr(epoch, "error", e);
            return;
        }
    }

    // 1) tunggu HP menyiarkan layanan pairing dengan nama yang sama dengan isi QR
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut last_note = String::new();
    let pair_addr = loop {
        if !qr_alive(epoch) {
            return;
        }
        if Instant::now() > deadline {
            set_qr(
                epoch,
                "error",
                "Waktu habis: QR tidak dipindai dari HP. Coba lagi dan pastikan HP dan PC satu jaringan Wi-Fi.",
            );
            return;
        }
        // laporkan apa yang dilihat adb supaya "tidak terjadi apa-apa" bisa didiagnosis dari UI
        let services = a.mdns_services();
        if !qr_alive(epoch) {
            return;
        }
        let note = match services {
            Ok(list) => {
                if let Some(s) = list.iter().find(|s| s.kind.contains("_adb-tls-pairing") && s.name == name) {
                    break s.addr.clone();
                }
                let pairing = list.iter().filter(|s| s.kind.contains("_adb-tls-pairing")).count();
                format!("Menunggu pindaian dari HP… (mDNS terlihat: {} layanan, {} pairing)", list.len(), pairing)
            }
            Err(e) => format!("Menunggu pindaian dari HP… (adb mdns services gagal: {e})"),
        };
        if note != last_note {
            set_qr(epoch, "waiting", note.clone());
            last_note = note;
        }
        std::thread::sleep(Duration::from_millis(1000));
    };

    // 2) pairing
    if !qr_alive(epoch) {
        return;
    }
    set_qr(epoch, "pairing", "Memasangkan dengan HP…");
    if let Err(e) = a.pair(&pair_addr, &pass) {
        set_qr(epoch, "error", format!("Pairing gagal: {e}"));
        return;
    }
    if !qr_alive(epoch) {
        return;
    }

    // 3) sambungkan: cari layanan connect dari IP yang sama
    set_qr(epoch, "connecting", "Pairing berhasil, menyambungkan…");
    let ip = pair_addr.rsplit_once(':').map(|x| x.0).unwrap_or(&pair_addr).to_string();
    let prefix = format!("{ip}:");
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if !qr_alive(epoch) {
            return;
        }
        let services = a.mdns_services();
        if !qr_alive(epoch) {
            return;
        }
        if let Ok(list) = services {
            if let Some(s) = list.iter().find(|s| s.kind.contains("_adb-tls-connect") && s.addr.starts_with(&prefix)) {
                if let Ok(m) = a.connect(&s.addr) {
                    if !qr_alive(epoch) {
                        return;
                    }
                    set_qr(epoch, "done", m);
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    set_qr(
        epoch,
        "error",
        "Pairing berhasil, tetapi HP belum tersambung. Isi \"Alamat sambung\" di bawah (alamat di layar utama Debugging nirkabel).",
    );
}

#[derive(Serialize)]
pub struct QrInfo {
    svg: Option<String>,
    name: String,
}

/// Buat QR pairing dan tunggu HP memindainya (berjalan di thread latar; status lewat android_qr_status).
#[tauri::command]
pub async fn android_qr_start() -> Result<QrInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if get_adb().is_none() {
            return Err(NO_ADB.to_string());
        }
        let name = QR_SERVICE_NAME.to_string();
        let pass = pairing_code()?;
        let payload = format!("WIFI:T:ADB;S:{name};P:{pass};;");
        let svg = crate::share::qr_svg(&payload)
            .ok_or_else(|| "QR pairing tidak dapat dibuat.".to_string())?;
        let epoch = QR_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        *QR_STATE.lock().unwrap_or_else(|e| e.into_inner()) =
            QrStatus { state: "waiting", message: "Menunggu pindaian dari HP…".into() };
        let n2 = name.clone();
        std::thread::spawn(move || qr_worker(epoch, n2, pass));
        Ok(QrInfo { svg: Some(svg), name })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn android_qr_status() -> QrStatus {
    QR_STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}


#[tauri::command]
pub fn android_qr_stop() {
    QR_GEN.fetch_add(1, Ordering::SeqCst);
    *QR_STATE.lock().unwrap_or_else(|e| e.into_inner()) = QrStatus { state: "idle", message: String::new() };
}

#[cfg(test)]
mod qr_tests {
    use super::*;

    #[test]
    fn pairing_code_matches_android_qr_format() {
        let code = pairing_code().expect("secure random source should be available");
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(
            format!("WIFI:T:ADB;S:{QR_SERVICE_NAME};P:{code};;"),
            format!("WIFI:T:ADB;S:ADBQR-connectPhoneOverWifi;P:{code};;")
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Mirror layar (scrcpy-server lewat adb)
// ---------------------------------------------------------------------------------------------

/// Mulai mirror layar HP. Mengembalikan port WebSocket lokal + ukuran layar.
#[tauri::command]
pub async fn android_mirror_start(app: AppHandle, serial: String) -> Result<mirror::MirrorInfo, String> {
    if is_mtp(&serial) {
        return Err("Mirror layar butuh USB debugging. Nyalakan USB debugging di HP, lalu sambungkan lagi.".into());
    }
    // pastikan RESOURCE_DIR terisi walau jendela tidak dibuka lewat shortcut
    if let Ok(d) = app.path().resource_dir() {
        let _ = RESOURCE_DIR.set(d);
    }
    let rd = RESOURCE_DIR.get().cloned();
    mirror::start(rd, serial).await
}

#[tauri::command]
pub async fn android_mirror_stop() {
    // stop() memanggil adb yang blocking (lewat Drop), jadi jangan dijalankan di thread async
    let _ = tauri::async_runtime::spawn_blocking(mirror::stop).await;
}

// ---------------------------------------------------------------------------------------------
// Siklus hidup jendela
// ---------------------------------------------------------------------------------------------
pub fn close_android_window(app: &AppHandle) {
    // hentikan mirror di thread terpisah supaya UI tidak menunggu adb
    std::thread::spawn(mirror::stop);
    android_qr_stop();
    if let Some(w) = app.get_webview_window("android") {
        if let Err(error) = w.destroy() {
            eprintln!("android: gagal menutup jendela: {error}");
            if let Err(hide_error) = w.hide() {
                eprintln!("android: fallback menyembunyikan jendela juga gagal: {hide_error}");
            }
        }
    }
}

fn show_android_window(w: &tauri::WebviewWindow) {
    crate::foreground::bring_to_front(w);
    let _ = w.eval("window.__androidShown && window.__androidShown()");
}

/// Dipanggil saat aplikasi keluar: hentikan pairing QR dan mirror supaya tidak ada proses adb yatim.
pub fn shutdown() {
    android_qr_stop();
    mirror::stop();
}

#[tauri::command]
pub fn android_close(app: AppHandle) {
    close_android_window(&app);
}

#[tauri::command]
pub fn android_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

/// Ctrl+Alt+Shift+A sudah dipakai toggle buka/tutup; ini untuk dobel-klik header: keluar/masuk fullscreen.
#[tauri::command]
pub fn android_toggle_fullscreen(window: tauri::WebviewWindow) {
    let is_fs = window.is_fullscreen().unwrap_or(false);
    let _ = window.set_fullscreen(!is_fs);
}

/// Esc: kalau sedang fullscreen, keluar dari fullscreen dulu; kalau tidak, baru tutup jendela.
#[tauri::command]
pub fn android_escape(app: AppHandle, window: tauri::WebviewWindow) {
    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    } else {
        close_android_window(&app);
    }
}

#[tauri::command]
pub fn android_minimize(window: tauri::WebviewWindow) {
    let _ = window.minimize();
}

static RESTORE: Mutex<Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>> = Mutex::new(None);

/// Maximize manual ke area kerja monitor (tanpa maximize OS yang bergeser di jendela tanpa dekorasi).
/// Mengembalikan true bila sekarang dalam keadaan penuh.
#[tauri::command]
pub fn android_toggle_maximize(window: tauri::WebviewWindow) -> bool {
    let mut g = RESTORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((pos, size)) = g.take() {
        let _ = window.set_size(size);
        let _ = window.set_position(pos);
        return false;
    }
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    }
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else { return false };
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return false };
    let (x, y, w, h) = crate::monitor_work_area(&m);
    *g = Some((pos, size));
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
    true
}

/// Dipanggil shortcut global (Ctrl+Alt+Shift+A).
pub fn toggle_window(app: &AppHandle) {
    if let Ok(d) = app.path().resource_dir() {
        let _ = RESOURCE_DIR.set(d);
    }
    if let Some(w) = app.get_webview_window("android") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            close_android_window(app);
        } else {
            show_android_window(&w);
        }
        return;
    }
    let app = app.clone();
    // dibangun dari thread lain agar tidak deadlock dengan event loop (Windows)
    std::thread::spawn(move || {
        let built = (|| -> tauri::Result<()> {
            let full_cfg = app.config();
            let win = match full_cfg.app.windows.iter().find(|w| w.label == "android") {
                Some(c) => tauri::WebviewWindowBuilder::from_config(&app, c)?,
                None => {
                    eprintln!(
                        "android: entri \"android\" belum ada di app.windows pada tauri.conf.json — \
                         drag & drop file TIDAK akan berfungsi sampai ditambahkan."
                    );
                    tauri::WebviewWindowBuilder::new(&app, "android", tauri::WebviewUrl::App("android.html".into()))
                }
            };
            let w = win
                .title("Android")
                .inner_size(840.0, 660.0)
                .center()
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .skip_taskbar(false)
                .resizable(true)
                .focused(true)
                .build()?;
            crate::foreground::bring_to_front(&w);
            Ok(())
        })();
        if let Err(e) = built {
            eprintln!("android: gagal membuka jendela: {e}");
        }
    });
}