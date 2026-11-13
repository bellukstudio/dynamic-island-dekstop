use tauri::Emitter;
use tauri::{Manager, PhysicalPosition};

mod autostart;
mod battery;
mod bluetooth;
mod config;
mod devices;
mod hud;
mod media;
mod quick;
mod weather;
mod stats;
mod fullscreen;
mod clipboard;
mod colorpick;
mod compress;
mod convert;
mod privacy;
mod lyrics;
mod shelf;
mod notes;
mod reminder;
mod prayer;
mod currency;
mod open_url;
mod officepdf;
mod share;
mod android;
mod whatsapp;
mod foreground;
mod pip;

#[cfg(target_os = "linux")]
mod osd_control;
#[cfg(target_os = "windows")]
mod winps;
// area klik pil (x, lebar, tinggi) dalam piksel logis. Dipakai di Windows.
#[cfg(target_os = "windows")]
static HIT: std::sync::Mutex<(i32, i32, i32, i32)> = std::sync::Mutex::new((5, 0, 130, 24));

// Posisi kustom notch (center_x, top_y) dalam piksel fisik.
// Jika None, notch dipusatkan di tengah atas monitor.
static WIN_PHYS: std::sync::Mutex<(i32, i32)> = std::sync::Mutex::new((0, 0));
static NOTCH_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);
// Menandakan apakah notch sedang digeser oleh user
static IS_DRAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// true selama item shelf diseret KELUAR (loop OLE drag Windows sedang berjalan): jendela tidak boleh
// diubah gaya click-through-nya dari thread lain di tengah seretan
static DRAG_OUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn set_drag_out(window: tauri::WebviewWindow, active: bool) {
    DRAG_OUT.store(active, std::sync::atomic::Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        if active {
            let _ = window.set_ignore_cursor_events(false);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
    }
}
// Nomor generasi membatalkan animasi resize sebelumnya saat target mode berubah lagi.
// static SIZE_ANIMATION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Area pakai monitor dalam piksel fisik: (x, y, lebar, tinggi). Lebar = monitor penuh; y/tinggi = area kerja
// (di luar taskbar) dari rcWork GetMonitorInfoW. Platform lain memakai seluruh monitor.
fn monitor_work_area(m: &tauri::Monitor) -> (i32, i32, i32, i32) {
    let pos = m.position();
    let size = m.size();
    let full = (pos.x, pos.y, size.width as i32, size.height as i32);
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        unsafe {
            let hmon = MonitorFromPoint(
                POINT { x: pos.x + 1, y: pos.y + 1 },
                MONITOR_DEFAULTTONEAREST,
            );
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                let r = mi.rcWork;
                let h = r.bottom - r.top;
                if h > 0 {
                    // Horizontal tetap lebar monitor penuh (titik tengah layar tidak bergeser oleh taskbar
                    // di kiri/kanan); hanya vertikal yang mengikuti area kerja (di luar taskbar).
                    return (pos.x, r.top, size.width as i32, h);
                }
            }
        }
    }
    full
}

// Terapkan posisi notch (di tengah atas atau di posisi kustom) berdasarkan ukuran jendela saat ini
fn apply_notch_position_for(window: &tauri::WebviewWindow, width: i32, height: i32) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let cur = *NOTCH_POS.lock().unwrap();
        // Pakai area kerja (tanpa taskbar) agar notch tidak tertutup taskbar.
        let (wa_x, wa_y, wa_w, wa_h) = monitor_work_area(&m);
        let (x, y) = match cur {
            Some((cx, ty)) => {
                let half_w = width / 2;
                let min_x = wa_x + half_w;
                let max_x = (wa_x + wa_w - half_w).max(min_x);
                let min_y = wa_y;
                // ty sangat besar (BOTTOM_TY) => otomatis menempel di dasar area kerja (di atas taskbar)
                let max_y = (wa_y + wa_h - height).max(min_y);
                (cx.clamp(min_x, max_x) - half_w, ty.clamp(min_y, max_y))
            }
            None => (wa_x + (wa_w - width) / 2, wa_y),
        };
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn apply_notch_position(window: &tauri::WebviewWindow) {
    let (w, h) = *WIN_PHYS.lock().unwrap();
    if w > 0 && h > 0 {
        apply_notch_position_for(window, w, h);
    } else if let Ok(size) = window.outer_size() {
        apply_notch_position_for(window, size.width as i32, size.height as i32);
    }
}

// Satu thread pekerja tetap (dulu: thread baru TIAP set_size dipanggil, yang terjadi tiap
// notch membuka/menutup). Panggilan baru mengulang urutan dari awal.
static REPOS_TX: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<tauri::WebviewWindow>>> =
    std::sync::OnceLock::new();

// reposisikan beberapa kali karena compositor menerapkan ukuran dengan jeda
fn reposition_later(window: &tauri::WebviewWindow) {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    let tx = REPOS_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<tauri::WebviewWindow>();
        std::thread::spawn(move || {
            // jeda berurutan 60 + 90 + 150 ms = reposisi pada 60, 150, dan 300 ms
            const STEPS: [u64; 3] = [60, 90, 150];
            while let Ok(mut win) = rx.recv() {
                let mut i = 0;
                while i < STEPS.len() {
                    match rx.recv_timeout(Duration::from_millis(STEPS[i])) {
                        Ok(w) => {
                            win = w;
                            i = 0;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            apply_notch_position(&win);
                            i += 1;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        });
        std::sync::Mutex::new(tx)
    });
    if let Ok(g) = tx.lock() {
        let _ = g.send(window.clone());
    }
}

// Payload kursor adaptif: struct bertipe menghindari peta + String baru pada setiap event.
#[derive(serde::Serialize, Clone)]
struct CursorPos {
    dx: f64,
    dy: f64,
}

// jendela pengaturan (dibuat saat diminta, dipakai ulang kalau sudah ada)
fn open_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Notch — Pengaturan")
    .inner_size(440.0, 660.0)
    .center() // buka tepat di tengah layar
    .resizable(false)
    .build();
}

// jendela detail catatan (dibuat saat diminta, dipakai ulang kalau sudah ada)
#[tauri::command]
async fn open_note_detail(app: tauri::AppHandle, id: String) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("note-detail") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("note-open", &id);
        return Ok(());
    }
    let id_js = serde_json::to_string(&id).map_err(|e| e.to_string())?;
    tauri::WebviewWindowBuilder::new(
        &app,
        "note-detail",
        tauri::WebviewUrl::App("note-detail.html".into()),
    )
    .title("Catatan")
    .inner_size(680.0, 520.0)
    .min_inner_size(400.0, 300.0)
    .resizable(true)
    .focused(true)
    .initialization_script(&format!("window.__NOTE_ID__ = {};", id_js))
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

// ringkasan update: popup di tengah layar. Dua mode (pemicunya di updates.ts):
//  - "release" (bawaan): tampil sekali per versi, begitu versi terpasang == new_version.
//  - "reminder": tampil berkala saat ada update opsional yang belum dipasang, berisi catatan
//    rilis versi BARU itu + tombol "Perbarui sekarang" (update_url).
#[tauri::command]
async fn open_whats_new(
    app: tauri::AppHandle,
    version: String,
    notes: String,
    mode: Option<String>,
    update_url: Option<String>,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("whats-new") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let data = serde_json::json!({
        "version": version,
        "notes": notes,
        "mode": mode.unwrap_or_else(|| "release".into()),
        "updateUrl": update_url,
    });
    let script = format!(
        "window.__WHATS_NEW__ = {};",
        serde_json::to_string(&data).map_err(|e| e.to_string())?
    );
    tauri::WebviewWindowBuilder::new(
        &app,
        "whats-new",
        tauri::WebviewUrl::App("whats-new.html".into()),
    )
    .title("Ringkasan pembaruan")
    .inner_size(460.0, 560.0)
    .center()
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(true)
    .initialization_script(&script)
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_whats_new(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("whats-new") {
        let _ = w.close();
    }
}

// #[tauri::command]
// async fn set_size(window: tauri::WebviewWindow, w: f64, h: f64) {
//     use std::sync::atomic::Ordering;
//     let animation_id = SIZE_ANIMATION_ID.fetch_add(1, Ordering::SeqCst) + 1;

//     // Jalankan animasi native pada blocking pool. Fungsi command tetap async sehingga
//     // renderer bisa menyelesaikan animasi CSS sambil ukuran jendela berubah.
//     let _ = tauri::async_runtime::spawn_blocking(move || {
//         let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
//         let (start_w, start_h) = window
//             .inner_size()
//             .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
//             .unwrap_or((w, h));

//         // 20 titik x 16ms ~= 300ms, cocok dengan transition CSS.
//         const STEPS: u32 = 20;
//         for step in 1..=STEPS {
//             if SIZE_ANIMATION_ID.load(Ordering::SeqCst) != animation_id {
//                 return;
//             }
//             let t = step as f64 / STEPS as f64;
//             let eased = t * t * (3.0 - 2.0 * t); // smoothstep tanpa overshoot
//             let cur_w = start_w + (w - start_w) * eased;
//             let cur_h = start_h + (h - start_h) * eased;
//             let _ = window.set_size(tauri::LogicalSize::new(cur_w.max(1.0), cur_h.max(1.0)));
//             apply_notch_position_for(
//                 &window,
//                 (cur_w.max(1.0) * scale).round() as i32,
//                 (cur_h.max(1.0) * scale).round() as i32,
//             );
//             if step < STEPS {
//                 std::thread::sleep(std::time::Duration::from_millis(16));
//             }
//         }

//         if SIZE_ANIMATION_ID.load(Ordering::SeqCst) == animation_id {
//             let _ = window.set_size(tauri::LogicalSize::new(w, h));
//             apply_notch_position_for(&window, (w * scale).round() as i32, (h * scale).round() as i32);
//         }
//     })
//     .await;
// }
#[cfg(target_os = "windows")]
fn lbutton_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}
#[tauri::command]
fn notch_monitor(window: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    let m = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or("monitor tidak ditemukan")?;
    let s = window.scale_factor().unwrap_or(1.0);
    // Kirim area kerja (tanpa taskbar), bukan seluruh monitor.
    let (x, y, w, h) = monitor_work_area(&m);
    Ok(serde_json::json!({
        "x": x as f64 / s,
        "y": y as f64 / s,
        "w": w as f64 / s,
        "h": h as f64 / s,
    }))
}
#[tauri::command]
fn set_size(window: tauri::WebviewWindow, w: f64, h: f64) {
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    *WIN_PHYS.lock().unwrap() = ((w * scale).round() as i32, (h * scale).round() as i32);
    let _ = window.set_size(tauri::LogicalSize::new(w, h));
    apply_notch_position(&window);
    reposition_later(&window);
}
#[tauri::command]
fn notch_start_move(window: tauri::WebviewWindow) -> Result<(f64, f64), String> {
    IS_DRAGGING.store(true, std::sync::atomic::Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    let _ = window.set_ignore_cursor_events(false);

    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);

    let stored = *NOTCH_POS.lock().unwrap();
    let (cx, ty) = match stored {
        Some((cx, ty)) => (cx as f64 / scale, ty as f64 / scale),
        None => (
            (pos.x as f64 + size.width as f64 / 2.0) / scale,
            pos.y as f64 / scale,
        ),
    };
    Ok((cx, ty))
}

#[tauri::command]
fn notch_move_to(window: tauri::WebviewWindow, cx: f64, ty: f64) -> Result<(), String> {
    let scale = window.scale_factor().unwrap_or(1.0);
    let phys_cx = (cx * scale).round() as i32;
    let phys_ty = (ty * scale).round() as i32;

    *NOTCH_POS.lock().unwrap() = Some((phys_cx, phys_ty));
    apply_notch_position(&window);
    Ok(())
}

#[tauri::command]
fn notch_end_move(window: tauri::WebviewWindow, save: bool) -> Result<(), String> {
    IS_DRAGGING.store(false, std::sync::atomic::Ordering::SeqCst);

    if save {
        let cur = *NOTCH_POS.lock().unwrap();
        if let Some((cx, ty)) = cur {
            let app = window.app_handle().clone();
            std::thread::spawn(move || {
                let mut cfg = config::cfg_get();
                cfg.position.custom = true;
                cfg.position.x = cx;
                cfg.position.y = ty;
                let _ = config::cfg_save(app.clone(), cfg);
                let _ = app.emit("position-changed", serde_json::json!({
                    "custom": true,
                    "x": cx,
                    "y": ty
                }));
            });
        }
    }
    Ok(())
}

#[tauri::command]
fn notch_reset_position(app: tauri::AppHandle) -> Result<(), String> {
    *NOTCH_POS.lock().unwrap() = None;
    if let Some(win) = app.get_webview_window("main") {
        apply_notch_position(&win);
        reposition_later(&win);
    }
    let a = app.clone();
    std::thread::spawn(move || {
        let mut cfg = config::cfg_get();
        cfg.position.custom = false;
        let _ = config::cfg_save(a.clone(), cfg);
        let _ = a.emit("position-changed", serde_json::json!({
            "custom": false
        }));
    });
    Ok(())
}

#[tauri::command]
fn notch_get_position(window: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    let cur = *NOTCH_POS.lock().unwrap();
    let scale = window.scale_factor().unwrap_or(1.0);
    let (custom, cx, ty) = match cur {
        Some((x, y)) => (true, x as f64 / scale, y as f64 / scale),
        None => (false, 0.0, 0.0),
    };
    Ok(serde_json::json!({
        "custom": custom,
        "x": cx,
        "y": ty,
    }))
}

// Hanya area (x, 0, w, h) yang menerima klik. Sisanya tembus ke aplikasi di bawahnya.
#[tauri::command]
fn set_hit_area(window: tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let win = window.clone();
        let _ = window.run_on_main_thread(move || {
            if let Ok(gtk_win) = win.gtk_window() {
                if let Some(gdk_win) = gtk_win.window() {
                    let region = gtk::cairo::Region::create_rectangle(
                        &gtk::cairo::RectangleInt::new(x, y, w, h),
                    );
                    gdk_win.input_shape_combine_region(&region, 0, 0);
                }
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = window;
        *HIT.lock().unwrap() = (x, y, w, h);
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (window, x, y, w, h);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Windows: tekan aktivitas latar & memori WebView2. Harus diset SEBELUM webview pertama dibuat.
    // Daftar --disable-features sama dengan bawaan Tauri. Mau menimpa? set sendiri env var ini sebelum
    // menjalankan app. Sengaja TIDAK memakai --enable-low-end-device-mode: mode itu mematikan raster GPU dan
    // memperkecil tile sehingga scroll WhatsApp Web patah-patah (apalagi dengan efek blur).
    #[cfg(target_os = "windows")]
    {
        if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
            // SAFETY: dipanggil sekali di awal run(), sebelum thread lain dibuat.
            unsafe {
                std::env::set_var(
                    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
                     --autoplay-policy=no-user-gesture-required \
                     --disable-background-networking \
                     --disable-component-update \
                     --disable-domain-reliability \
                     --disable-breakpad \
                     --no-pings",
                );
            }
        }
    }
    // Wayland: default ke XWayland supaya posisi dan klik berfungsi.
    // Untuk mencoba mode native (layer-shell): NOTCH_NATIVE=1
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false);
        if wayland
            && std::env::var_os("NOTCH_NATIVE").is_none()
            && std::env::var_os("GDK_BACKEND").is_none()
        {
            unsafe { std::env::set_var("GDK_BACKEND", "x11") };
        }
    }

    config::init();

    // KDE Plasma: hide status-change OSD while Notch is running.
    #[cfg(target_os = "linux")]
    osd_control::activate();

    // konfigurasi dikirim ke frontend sebelum main.ts jalan (window.__CFG__)
    let script = format!(
        "window.__CFG__ = {};",
        serde_json::to_string(config::get()).unwrap_or_else(|_| "{}".into())
    );
    let cfg_plugin: tauri::plugin::TauriPlugin<tauri::Wry> =
        tauri::plugin::Builder::new("notch-config")
            .js_init_script(script)
            .build();

    tauri::Builder::default()
        .plugin(cfg_plugin)
        .plugin(tauri_plugin_drag::init()) // drag keluar: notch -> File Explorer
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        use tauri_plugin_global_shortcut::{Code, Modifiers};
                        if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyK,
                        ) {
                            compress::toggle_window(app); // Ctrl+Alt+Shift+K: jendela Compress
                        } else if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyC,
                        ) {
                            convert::toggle_window(app); // Ctrl+Alt+Shift+C: jendela Convert
                        } else if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyS,
                        ) {
                            share::toggle_window(app); // Ctrl+Alt+Shift+S: jendela Share (kirim/terima file)
                        } else if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyA,
                        ) {
                            android::toggle_window(app); // Ctrl+Alt+Shift+A: jendela Android (adb)
                        } else if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyP,
                        ) {
                            let _ = app.emit("color-picker-shortcut", ()); // Ctrl+Alt+Shift+P: color picker
                        } else if shortcut.matches(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                            Code::KeyW,
                        ) {
                            whatsapp::toggle_window(app); // Ctrl+Alt+Shift+W: dua akun WhatsApp
                        }
                        // kombinasi lain yang tidak terdaftar sengaja diabaikan (bukan lagi jatuh ke color picker)
                    }
                })
                .build(),
        )
        .setup(|app| {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            app.global_shortcut().register("Ctrl+Alt+Shift+P")?;
            // Shortcut Compress & Convert: bila sudah dipakai aplikasi lain, abaikan saja (jangan gagalkan startup).
            let _ = app.global_shortcut().register("Ctrl+Alt+Shift+K");
            let _ = app.global_shortcut().register("Ctrl+Alt+Shift+C");
            let _ = app.global_shortcut().register("Ctrl+Alt+Shift+S");
            let _ = app.global_shortcut().register("Ctrl+Alt+Shift+A");
            let _ = app.global_shortcut().register("Ctrl+Alt+Shift+W");
            let win = app.get_webview_window("main").unwrap();
            // Pantau area kerja monitor: berubah saat taskbar di-hide/ditampilkan, dipindah, atau resolusi berubah.
            // Frontend lalu memperbarui batas bawahnya sehingga notch bisa turun mentok / naik di atas taskbar.
            {
                let w = win.clone();
                std::thread::spawn(move || {
                    let mut last = (i32::MIN, i32::MIN, i32::MIN, i32::MIN);
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        let m = w
                            .current_monitor()
                            .ok()
                            .flatten()
                            .or_else(|| w.primary_monitor().ok().flatten());
                        let Some(m) = m else { continue };
                        let cur = monitor_work_area(&m);
                        if cur == last {
                            continue;
                        }
                        let first = last.0 == i32::MIN;
                        last = cur;
                        if first {
                            continue; // nilai awal sudah diambil frontend lewat notch_monitor
                        }
                        let s = w.scale_factor().unwrap_or(1.0).max(0.1);
                        let _ = w.emit(
                            "work-area-changed",
                            serde_json::json!({
                                "x": cur.0 as f64 / s,
                                "y": cur.1 as f64 / s,
                                "w": cur.2 as f64 / s,
                                "h": cur.3 as f64 / s,
                            }),
                        );
                        apply_notch_position(&w);
                    }
                });
            }
            // eprintln!("[dnd-rust] hook aktif");
            // if std::env::var_os("NOTCH_DND_TEST").is_some() {
            //     eprintln!("[dnd-rust] TES: click-through DIMATIKAN");
            // }
            // if std::env::var_os("NOTCH_NO_TOP").is_some() {
            //     let _ = win.set_always_on_top(false);
            //     eprintln!("[dnd-rust] TES: alwaysOnTop DIMATIKAN");
            // }

            #[allow(unused_mut)]
            let mut layer_ok = false;

            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::*;
                use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

                let gtk_win = win.gtk_window()?;
                let native = std::env::var_os("NOTCH_NATIVE").is_some();
                println!(
                    "layer-shell: didukung={} realized={} native_diminta={}",
                    gtk_layer_shell::is_supported(),
                    gtk_win.is_realized(),
                    native
                );

                if native && gtk_layer_shell::is_supported() && !gtk_win.is_realized() {
                    gtk_win.init_layer_shell();
                    gtk_win.set_layer(Layer::Overlay);
                    gtk_win.set_anchor(Edge::Top, true); // hanya atas = otomatis di tengah
                    gtk_win.set_keyboard_mode(KeyboardMode::None);
                    gtk_win.set_exclusive_zone(-1);
                    gtk_win.set_default_size(160, 40);
                    gtk_win.set_size_request(160, 40);
                    layer_ok = true;
                    println!("layer-shell: aktif");
                } else if native {
                    println!("layer-shell: GAGAL diaktifkan (lihat nilai di atas)");
                }
            }

            win.show()?;

            // posisi kursor untuk mata (~30 fps). Di Windows sekaligus mengatur click-through.
            #[cfg(target_os = "windows")]
            let mut last_inside = true;
            #[cfg(target_os = "windows")]
            let mut resync = false; // paksa terapkan ulang click-through setelah seretan keluar selesai
            let w = win.clone();
            std::thread::spawn(move || {
              // kirim posisi hanya saat kursor bergerak (plus denyut ~4x/detik saat diam,
              // karena frontend menghitung ulang hover saat ukuran notch berubah)
              let mut last_emit = (i32::MIN, i32::MIN);
              let mut last_beat = std::time::Instant::now();
              let mut busy_until = std::time::Instant::now();
              loop {
                if let (Ok(cur), Ok(pos), Ok(size), Ok(scale)) = (
                    w.cursor_position(),
                    w.outer_position(),
                    w.outer_size(),
                    w.scale_factor(),
                ) {
                    let cx = pos.x as f64 + size.width as f64 / 2.0;
                    let cy = pos.y as f64 + 12.0 * scale;
                    let (dx, dy) = ((cur.x - cx) / scale, (cur.y - cy) / scale);
                    let key = (dx.round() as i32, dy.round() as i32);
                    let send = if key != last_emit {
                        last_emit = key;
                        last_beat = std::time::Instant::now();
                        busy_until = last_beat + std::time::Duration::from_millis(1500);
                        true
                    } else if last_beat.elapsed() >= std::time::Duration::from_millis(250) {
                        last_beat = std::time::Instant::now();
                        true
                    } else {
                        false
                    };
                    if send {
                        let _ = w.emit("cursor", CursorPos { dx, dy });
                    }

                    #[cfg(target_os = "windows")]
                    {
                        if DRAG_OUT.load(std::sync::atomic::Ordering::Relaxed) {
                            resync = true;
                        } else if !IS_DRAGGING.load(std::sync::atomic::Ordering::Relaxed) {
                        let (hx, hy, hw, hh) = *HIT.lock().unwrap();
                        let grow = if lbutton_down() { 40.0 * scale } else { 0.0 }; // lebih longgar saat ada seretan
                        let left = pos.x as f64 + hx as f64 * scale - grow;
                        let top = pos.y as f64 + hy as f64 * scale - grow;
                        let inside = cur.x >= left
                                && cur.x <= left + hw as f64 * scale + grow * 2.0
                                && cur.y >= top
                                && cur.y <= top + hh as f64 * scale + grow * 2.0;
                            if inside != last_inside || resync {
                                resync = false;
                                last_inside = inside;
                                let _ = w.set_ignore_cursor_events(!inside);
                                // let _ = w.set_ignore_cursor_events(!inside && std::env::var_os("NOTCH_DND_TEST").is_none());
                            }
                        }
                    }
                }
                // Adaptif: rapat (33 ms) hanya saat kursor baru bergerak atau sedang menyeret notch.
                // Saat diam interval dilonggarkan supaya thread ini jarang membangunkan event loop utama
                // (outer_position/outer_size/scale_factor masing-masing = satu round-trip ke thread UI).
                #[cfg(target_os = "windows")]
                let hot = IS_DRAGGING.load(std::sync::atomic::Ordering::Relaxed)
                    || DRAG_OUT.load(std::sync::atomic::Ordering::Relaxed);
                #[cfg(not(target_os = "windows"))]
                let hot = false;
                let now = std::time::Instant::now();
                let ms: u64 = if hot || now < busy_until {
                    33
                } else if now.duration_since(busy_until) > std::time::Duration::from_secs(10) {
                    250
                } else {
                    120
                };
                std::thread::sleep(std::time::Duration::from_millis(ms));
                }
            });

            // Prototipe: jendela PiP Chrome/Edge yang dijatuhkan di notch menempel di bawah panel media.
            pip::start(win.clone());

            // Inisialisasi posisi kustom jika ada di konfigurasi
            let initial_cfg = config::get();
            if initial_cfg.position.custom {
                *NOTCH_POS.lock().unwrap() = Some((initial_cfg.position.x, initial_cfg.position.y));
            }

            // Fallback X11/XWayland dan Windows: posisikan manual
            if !layer_ok {
                let _ = win.set_size(tauri::LogicalSize::new(160.0, 40.0));
                apply_notch_position(&win);
                reposition_later(&win);
            }

            // ikon tray: Pengaturan + Pusatkan + Keluar (notch tidak punya taskbar).
            // Tidak fatal: saat dijalankan otomatis ketika login, tray kadang belum siap
            // (mis. GNOME tanpa AppIndicator); aplikasi tetap harus jalan.
            let tray_result: Result<(), Box<dyn std::error::Error>> = (|| {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::TrayIconBuilder;
                let mi_set = MenuItem::with_id(app, "settings", "Pengaturan", true, None::<&str>)?;
                let mi_center = MenuItem::with_id(app, "center", "Pusatkan Notch", true, None::<&str>)?;
                let mi_quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&mi_set, &mi_center, &mi_quit])?;
                let mut tray = TrayIconBuilder::new()
                    .menu(&menu)
                    .tooltip("Notch")
                    .on_menu_event(|app, e| match e.id.as_ref() {
                        "settings" => {
                            // thread terpisah: membuat window di event handler bisa deadlock di Windows
                            let a = app.clone();
                            std::thread::spawn(move || open_settings(&a));
                        }
                        "center" => {
                            let a = app.clone();
                            let _ = notch_reset_position(a);
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.build(app)?;
                Ok(())
            })();
            if let Err(e) = tray_result {
                eprintln!("tray gagal dibuat (pengaturan tidak bisa dibuka lewat tray): {e}");
            }

            bluetooth::start(app.handle().clone());
            battery::start(app.handle().clone());
            devices::start(app.handle().clone());
            hud::start(app.handle().clone());
            weather::start(app.handle().clone());
            fullscreen::start(app.handle().clone());
            media::start(app.handle().clone());
            stats::start(app.handle().clone()); 
            clipboard::start(app.handle().clone()); 
            privacy::start(app.handle().clone());
            shelf::start(app.handle().clone());
            reminder::start(app.handle().clone());
            share::lan::start_background(app.handle().clone()); // Share: terima dari Notch lain bila diaktifkan
            // setelah semua modul mendaftar: satu PowerShell bersama untuk perangkat/kecerahan/Bluetooth
            #[cfg(target_os = "windows")]
            winps::start();
            Ok(())
        })
        .on_window_event(|window, event| {
            // diagnostik drag-drop dari luar (Explorer dsb.): hapus blok ini kalau sudah beres
            if let tauri::WindowEvent::DragDrop(d) = event {
                if !matches!(d, tauri::DragDropEvent::Over { .. }) {
                    eprintln!("[dnd-rust] {} {:?}", window.label(), d);
                }
            }
            // Jendela alat dibuat ulang saat dibuka lagi, sehingga WebView tersembunyi tidak menahan RAM.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "compress" | "convert" | "share" | "android" => {
                        api.prevent_close();
                        match window.label() {
                            "compress" => compress::close_compress_window(window.app_handle()),
                            "convert" => convert::close_convert_window(window.app_handle()),
                            "share" => share::close_share_window(window.app_handle()),
                            "android" => android::close_android_window(window.app_handle()),
                            _ => unreachable!(),
                        }
                    }
                    _ => {}
                }
            }
            // jendela pengaturan ditutup tanpa simpan -> kembalikan tampilan notch ke isi file
            if window.label() == "settings" {
                if let tauri::WindowEvent::Destroyed = event {
                    let _ = window.app_handle().emit("config-changed", config::cfg_get());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            media::media_control,
            media::media_pause_all,
            media::media_resume,
            stats::stats_set_active,
            media::media_seek,
            set_size,
            set_hit_area,
            pip::pip_set_visible,
            set_drag_out,
            shelf::shelf_log,
            quick::quick_state,
            quick::quick_toggle,
            clipboard::clip_list,
            clipboard::clip_copy,
            clipboard::clip_clear,
            clipboard::color_copy,
            colorpick::color_pick_screen,
            colorpick::color_pick_cancel,
            compress::compress_file_info,
            compress::compress_estimate,
            compress::compress_run_one,
            compress::compress_pick_files,
            compress::compress_reveal,
            compress::compress_close,
            compress::compress_drag,
            convert::convert_file_info,
            convert::convert_run_one,
            convert::convert_pick_files,
            convert::convert_reveal,
            convert::convert_close,
            convert::convert_drag,
            share::share_start,
            share::share_stop,
            share::share_state,
            share::share_add,
            share::share_remove,
            share::share_pick_files,
            share::share_open_inbox,
            share::share_reveal,
            share::share_close,
            share::share_drag,
            share::lan::lan_state,
            share::lan::lan_set_enabled,
            share::lan::lan_set_name,
            share::lan::lan_set_auto_accept,
            share::lan::lan_untrust,
            share::lan::lan_respond,
            share::lan::lan_send,
            share::lan::lan_pick_folder,
            android::android_status,
            android::android_devices,
            android::android_ls,
            android::android_push,
            android::android_pull,
            android::android_pair,
            android::android_connect,
            android::android_disconnect,
            android::android_qr_start,
            android::android_qr_status,
            android::android_qr_stop,
            android::android_pick_files,
            android::android_reveal,
            android::android_close,
            android::android_drag,
            android::android_mirror_start,
            android::android_mirror_stop,
            android::android_toggle_fullscreen,
            android::android_escape,
            android::android_minimize,
            android::android_toggle_maximize,
            whatsapp::whatsapp_status,
            whatsapp::whatsapp_report_new_message,
            whatsapp::whatsapp_set_preview,
            whatsapp::whatsapp_set_pin,
            whatsapp::whatsapp_unlock,
            whatsapp::whatsapp_set_view,
            whatsapp::whatsapp_change_pin,
            whatsapp::whatsapp_lock,
            whatsapp::whatsapp_set_notifications,
            whatsapp::whatsapp_set_background,
            whatsapp::whatsapp_set_blur,
            whatsapp::whatsapp_close_manager,
            lyrics::lyrics_fetch,
            shelf::shelf_list,
            shelf::shelf_add,
            shelf::shelf_remove,
            shelf::shelf_clear,
            shelf::shelf_open,
            shelf::shelf_reveal,
            notes::notes_list,
            notes::notes_add,
            notes::notes_update,
            notes::notes_remove,
            notes::notes_clear,
            reminder::reminders_list,
            reminder::reminders_add,
            reminder::reminders_toggle,
            reminder::reminders_remove,
            reminder::reminders_clear_done,
            reminder::reminders_stop,
            reminder::reminders_snooze,            
            prayer::prayer_month,
            prayer::prayer_info,
            prayer::prayer_set_prefs,
            config::cfg_get,
            config::cfg_preview,
            config::cfg_save,
            config::cfg_restart,
            config::nudge_test,
            bluetooth::bt_list,
            autostart::autostart_get,
            autostart::autostart_set,
            notch_start_move,
            notch_move_to,
            notch_end_move,
            notch_reset_position,
            notch_get_position,
            notch_monitor,
            open_note_detail,
            open_whats_new,
            close_whats_new,
            currency::fetch_yahoo_chart,
            open_url::open_url
        ])
        .build(tauri::generate_context!())
        .expect("gagal membangun aplikasi")
        .run(|app_handle, event| match event {
            tauri::RunEvent::Ready => whatsapp::start_background_accounts(app_handle),
            tauri::RunEvent::Exit => {
                android::shutdown();
                #[cfg(target_os = "linux")]
                osd_control::restore();
            }
            _ => {}
        });
}