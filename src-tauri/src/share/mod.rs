//! Jendela Share (Ctrl+Alt+Shift+S): kirim/terima file lewat jaringan lokal.
//!
//! PC menjalankan server web kecil (lihat server.rs). HP atau PC lain cukup memindai QR / membuka
//! alamatnya di browser: bisa mengunduh file yang dibagikan PC dan mengunggah file ke PC.
//! Server hanya hidup selama jendela Share terbuka; menutup jendela menghentikan server dan
//! mengosongkan daftar file yang dibagikan.
mod discovery;
pub mod lan;
mod peer;
mod server;

use serde::Serialize;
use server::{Emit, Event, Files, Running, SharedFile};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager};

static FILES: LazyLock<Files> = LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));
static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Folder tempat file masuk disimpan: <Unduhan>/Notch
pub(super) fn inbox_dir(app: &AppHandle) -> PathBuf {
    app.path().download_dir().unwrap_or_else(|_| std::env::temp_dir()).join("Notch")
}

#[derive(Serialize)]
pub struct ShareState {
    running: bool,
    /// Alamat utama (IP jaringan paling mungkin); `urls` berisi semua alternatif.
    url: Option<String>,
    urls: Vec<String>,
    /// QR untuk `url`, berupa SVG murni.
    qr: Option<String>,
    inbox: String,
    files: Vec<SharedFile>,
}

fn build_state(app: &AppHandle) -> ShareState {
    let files = FILES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let inbox = inbox_dir(app).to_string_lossy().into_owned();
    let guard = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(r) => {
            let urls: Vec<String> =
                server::lan_ips().iter().map(|ip| format!("http://{ip}:{}/s/{}/", r.port, r.token)).collect();
            let url = urls.first().cloned();
            let qr = url.as_deref().and_then(server::qr_svg);
            ShareState { running: true, url, urls, qr, inbox, files }
        }
        None => ShareState { running: false, url: None, urls: vec![], qr: None, inbox, files },
    }
}

#[tauri::command]
pub async fn share_start(app: AppHandle) -> Result<ShareState, String> {
    let already = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).is_some();
    if !already {
        let app2 = app.clone();
        let emit: Emit = Arc::new(move |e: Event| {
            let _ = app2.emit_to("share", "share-event", &e);
        });
        let running = server::start(FILES.clone(), inbox_dir(&app), emit).await?;
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(running);
    }
    Ok(build_state(&app))
}

fn stop_server() {
    // Running::drop mengirim sinyal shutdown
    let _ = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[tauri::command]
pub fn share_stop(app: AppHandle) -> ShareState {
    stop_server();
    build_state(&app)
}

#[tauri::command]
pub fn share_state(app: AppHandle) -> ShareState {
    build_state(&app)
}

/// Tambah file ke daftar "dibagikan dari PC" (folder diabaikan, file yang sama tidak digandakan).
#[tauri::command]
pub fn share_add(paths: Vec<String>) -> Vec<SharedFile> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    for p in paths {
        let pb = PathBuf::from(&p);
        let Ok(meta) = std::fs::metadata(&pb) else { continue };
        if !meta.is_file() || files.iter().any(|f| f.path == p) {
            continue;
        }
        let Some(name) = pb.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        files.push(SharedFile { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), name, size: meta.len(), path: p });
    }
    files.clone()
}

#[tauri::command]
pub fn share_remove(id: u32) -> Vec<SharedFile> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    files.retain(|f| f.id != id);
    files.clone()
}

#[tauri::command]
pub async fn share_pick_files() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Pilih file untuk dibagikan")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

/// Buka folder file masuk di File Explorer / file manager (dibuat dulu bila belum ada).
#[tauri::command]
pub fn share_open_inbox(app: AppHandle) -> Result<(), String> {
    let dir = inbox_dir(&app);
    std::fs::create_dir_all(&dir).map_err(err)?;
    open_dir(&dir)
}

fn open_dir(dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(dir).spawn();
    r.map(|_| ()).map_err(err)
}

/// Tampilkan satu file yang sudah diterima di File Explorer.
#[tauri::command]
pub fn share_reveal(path: String) {
    crate::compress::compress_reveal(path);
}

// ---------------------------------------------------------------------------------------------
// Siklus hidup jendela (di-hide, bukan di-destroy; menutup = hentikan server)
// ---------------------------------------------------------------------------------------------
fn close_share_window(app: &AppHandle) {
    stop_server();
    lan::reject_all_pending(); // penawaran masuk yang belum dijawab dianggap ditolak
    FILES.lock().unwrap_or_else(|e| e.into_inner()).clear();
    if let Some(w) = app.get_webview_window("share") {
        let _ = w.eval("window.__shareReset && window.__shareReset()");
        let _ = w.hide();
    }
}

fn show_share_window(w: &tauri::WebviewWindow) {
    crate::foreground::bring_to_front(w);
    let _ = w.eval("window.__shareShown && window.__shareShown()");
}

#[tauri::command]
pub fn share_close(app: AppHandle) {
    close_share_window(&app);
}

#[tauri::command]
pub fn share_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

/// Buat jendela Share (dari thread lain agar tidak deadlock dengan event loop di Windows).
fn build_window(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let built = (|| -> tauri::Result<()> {
            let full_cfg = app.config();
            let win = match full_cfg.app.windows.iter().find(|w| w.label == "share") {
                Some(c) => tauri::WebviewWindowBuilder::from_config(&app, c)?,
                None => {
                    eprintln!(
                        "share: entri \"share\" belum ada di app.windows pada tauri.conf.json — \
                         drag & drop file TIDAK akan berfungsi sampai ditambahkan."
                    );
                    tauri::WebviewWindowBuilder::new(&app, "share", tauri::WebviewUrl::App("share.html".into()))
                }
            };
            let w = win
                .title("Share")
                .inner_size(840.0, 660.0)
                .center()
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .skip_taskbar(false)
                .resizable(false)
                .focused(true)
                .build()?;
            crate::foreground::bring_to_front(&w);
            Ok(())
        })();
        if let Err(e) = built {
            eprintln!("share: gagal membuka jendela: {e}");
        }
    });
}

/// Tampilkan jendela Share (dibuat dulu bila belum ada). Dipakai juga saat ada penawaran file masuk.
pub fn show_window(app: &AppHandle) {
    match app.get_webview_window("share") {
        Some(w) => show_share_window(&w),
        None => build_window(app),
    }
}

/// Dipanggil shortcut global (Ctrl+Alt+Shift+S): tampilkan, atau tutup bila sedang terlihat dan fokus.
pub fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("share") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            close_share_window(app);
        } else {
            show_share_window(&w);
        }
        return;
    }
    build_window(app);
}
