//! Jendela Android (Ctrl+Alt+A): jelajahi penyimpanan HP Android dan transfer file lewat USB
//! (atau Wi-Fi debugging) memakai `adb`. Tidak perlu aplikasi di HP.
//! adb dicari di folder resource `adb/` (dibundel), env ADB_PATH, PATH, atau Android SDK.
mod adb;
mod mirror;

use adb::{Adb, Device, Entry};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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
    tauri::async_runtime::spawn_blocking(|| get_adb().ok_or_else(|| NO_ADB.to_string())?.devices())
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn android_ls(serial: String, path: String) -> Result<Vec<Entry>, String> {
    tauri::async_runtime::spawn_blocking(move || get_adb().ok_or_else(|| NO_ADB.to_string())?.list_dir(&serial, &path))
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
        let a = get_adb().ok_or_else(|| NO_ADB.to_string())?;
        let mut out = Vec::new();
        for p in paths {
            let pb = PathBuf::from(&p);
            let name = pb.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            match a.push(&serial, &pb, &remote_dir) {
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
        let a = get_adb().ok_or_else(|| NO_ADB.to_string())?;
        std::fs::create_dir_all(&dest_dir).map_err(|e| format!("Folder tujuan tidak bisa dibuat: {e}"))?;
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
// Mirror layar (scrcpy-server lewat adb)
// ---------------------------------------------------------------------------------------------

/// Mulai mirror layar HP. Mengembalikan port WebSocket lokal + ukuran layar.
#[tauri::command]
pub async fn android_mirror_start(app: AppHandle, serial: String) -> Result<mirror::MirrorInfo, String> {
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
// Siklus hidup jendela (di-hide, bukan di-destroy)
// ---------------------------------------------------------------------------------------------
fn close_android_window(app: &AppHandle) {
    // hentikan mirror di thread terpisah supaya UI tidak menunggu adb
    std::thread::spawn(mirror::stop);
    if let Some(w) = app.get_webview_window("android") {
        let _ = w.eval("window.__androidReset && window.__androidReset()");
        let _ = w.hide();
    }
}

fn show_android_window(w: &tauri::WebviewWindow) {
    crate::foreground::bring_to_front(w);
    let _ = w.eval("window.__androidShown && window.__androidShown()");
}

#[tauri::command]
pub fn android_close(app: AppHandle) {
    close_android_window(&app);
}

#[tauri::command]
pub fn android_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

/// Ctrl+Alt+A sudah dipakai toggle buka/tutup; ini untuk dobel-klik header: keluar/masuk fullscreen.
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

/// Dipanggil shortcut global (Ctrl+Alt+A).
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
#[cfg(target_os = "windows")]
fn force_foreground(w: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, SetForegroundWindow};

    let Ok(h) = w.hwnd() else { return };
    let hwnd = HWND(h.0 as _);
    unsafe {
        // ketukan Alt palsu: membuka foreground lock untuk proses ini
        keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(not(target_os = "windows"))]
fn force_foreground(_w: &tauri::WebviewWindow) {}