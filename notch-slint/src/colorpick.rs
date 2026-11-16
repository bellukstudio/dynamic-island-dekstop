//! Native Windows screen eyedropper. Click a pixel to copy HEX; Escape or the
//! registered shortcut again cancels the active sampling session.
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

struct ActiveSession;

impl Drop for ActiveSession {
    fn drop(&mut self) {
        CANCEL.store(false, Ordering::SeqCst);
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Start a picker session, or cancel the current one when invoked a second time.
/// Reserve the session before spawning so two rapid shortcut presses cannot
/// create competing picker threads.
pub fn toggle() {
    match ACTIVE.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => {
            CANCEL.store(false, Ordering::SeqCst);
            std::thread::spawn(|| {
                let _session = ActiveSession;
                let _ = run_picker();
            });
        }
        Err(_) => CANCEL.store(true, Ordering::SeqCst),
    }
}

/// Run one picker session synchronously. Returns the sampled HEX value, or
/// `None` when the user cancels or the session times out.
pub fn pick_and_copy() -> Result<Option<String>, String> {
    ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "Color Picker sedang aktif".to_string())?;
    let _session = ActiveSession;
    CANCEL.store(false, Ordering::SeqCst);
    run_picker()
}

#[cfg(windows)]
fn run_picker() -> Result<Option<String>, String> {
    use std::{thread, time::{Duration, Instant}};
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetDC, GetPixel, ReleaseDC},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON},
            WindowsAndMessaging::GetCursorPos,
        },
    };

    let down = |key: i32| unsafe { (GetAsyncKeyState(key) as u16 & 0x8000) != 0 };
    let release_start = Instant::now();
    while down(VK_LBUTTON.0 as i32) && release_start.elapsed() < Duration::from_millis(700) {
        if CANCEL.load(Ordering::SeqCst) { return Ok(None); }
        thread::sleep(Duration::from_millis(8));
    }

    let screen = unsafe { GetDC(None) };
    if screen.is_invalid() { return Err("Tidak dapat mengakses layar".into()); }
    let result = (|| {
        let start = Instant::now();
        loop {
            if CANCEL.load(Ordering::SeqCst)
                || down(VK_ESCAPE.0 as i32)
                || start.elapsed() > Duration::from_secs(60)
            {
                return Ok(None);
            }

            if down(VK_LBUTTON.0 as i32) {
                let mut point = POINT::default();
                unsafe { GetCursorPos(&mut point) }.map_err(|e| e.to_string())?;
                let raw = unsafe { GetPixel(screen, point.x, point.y) }.0;
                if raw == 0xFFFF_FFFF {
                    return Err("Warna piksel tidak dapat dibaca".into());
                }
                let rgb = ((raw & 0xFF) << 16) | (raw & 0xFF00) | ((raw >> 16) & 0xFF);
                let hex = format!("#{rgb:06X}");
                let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
                clipboard.set_text(hex.clone()).map_err(|e| e.to_string())?;
                while down(VK_LBUTTON.0 as i32) {
                    thread::sleep(Duration::from_millis(8));
                }
                return Ok(Some(hex));
            }
            thread::sleep(Duration::from_millis(8));
        }
    })();
    unsafe { ReleaseDC(None, screen); }
    result
}

#[cfg(not(windows))]
fn run_picker() -> Result<Option<String>, String> {
    Err("Color Picker layar saat ini hanya tersedia di Windows".into())
}
