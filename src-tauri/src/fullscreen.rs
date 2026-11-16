use tauri::AppHandle;

// Sembunyikan notch saat ada aplikasi layar penuh (game, video, presentasi).
// Bisa dimatikan lewat config.toml: hide_fullscreen = false
pub fn start(app: AppHandle) {
    if !crate::config::get().hide_fullscreen {
        return;
    }
    imp::start(app);
}

// ======================= Windows =======================
#[cfg(target_os = "windows")]
mod imp {
    use std::time::Duration;
    use tauri::{AppHandle, Manager};
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE,
        QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    // BUSY = aplikasi layar penuh biasa (video, browser F11)
    // D3D_FULL_SCREEN = game eksklusif | PRESENTATION_MODE = mode presentasi
    fn fullscreen_now() -> bool {
        unsafe {
            match SHQueryUserNotificationState() {
                Ok(s) => {
                    s == QUNS_BUSY
                        || s == QUNS_RUNNING_D3D_FULL_SCREEN
                        || s == QUNS_PRESENTATION_MODE
                }
                Err(_) => false,
            }
        }
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            let mut hidden = false;
            loop {
                let want = fullscreen_now();
                if want != hidden {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = if want { w.hide() } else { w.show() };
                    }
                    hidden = want;
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
    }
}

// ======================= platform lain: belum didukung =======================
#[cfg(not(target_os = "windows"))]
mod imp {
    use tauri::AppHandle;
    pub fn start(_app: AppHandle) {}
}