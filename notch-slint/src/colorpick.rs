//! Native Windows screen eyedropper. Samples the pixel under the pointer on click
//! and copies its HEX value to the clipboard. Escape cancels the current session.

#[cfg(windows)]
pub fn pick_and_copy() -> Result<Option<String>, String> {
    use std::{thread, time::{Duration, Instant}};
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetDC, GetPixel, ReleaseDC},
        UI::{Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON}, WindowsAndMessaging::GetCursorPos},
    };

    static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    use std::sync::atomic::Ordering;
    if ACTIVE.swap(true, Ordering::SeqCst) { return Err("Color Picker sedang aktif".into()); }
    struct Guard;
    impl Drop for Guard { fn drop(&mut self) { ACTIVE.store(false, Ordering::SeqCst); } }
    let _guard = Guard;

    let down = |key: i32| unsafe { (GetAsyncKeyState(key) as u16 & 0x8000) != 0 };
    // Jangan salah membaca tombol klik yang mungkin masih tertahan saat shortcut ditekan.
    let release_start = Instant::now();
    while down(VK_LBUTTON.0 as i32) && release_start.elapsed() < Duration::from_millis(700) {
        thread::sleep(Duration::from_millis(8));
    }

    let screen = unsafe { GetDC(None) };
    if screen.is_invalid() { return Err("Tidak dapat mengakses layar".into()); }
    let result = (|| {
        let start = Instant::now();
        loop {
            if down(VK_ESCAPE.0 as i32) || start.elapsed() > Duration::from_secs(60) {
                return Ok(None);
            }
            if down(VK_LBUTTON.0 as i32) {
                let mut point = POINT::default();
                unsafe { GetCursorPos(&mut point) }.map_err(|e| e.to_string())?;
                let raw = unsafe { GetPixel(screen, point.x, point.y) }.0;
                if raw == 0xFFFF_FFFF { return Err("Warna piksel tidak dapat dibaca".into()); }
                let hex = format!("#{:02X}{:02X}{:02X}", raw & 0xFF, (raw >> 8) & 0xFF, (raw >> 16) & 0xFF);
                let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
                clipboard.set_text(hex.clone()).map_err(|e| e.to_string())?;
                while down(VK_LBUTTON.0 as i32) { thread::sleep(Duration::from_millis(8)); }
                return Ok(Some(hex));
            }
            thread::sleep(Duration::from_millis(8));
        }
    })();
    unsafe { ReleaseDC(None, screen); }
    result
}

#[cfg(not(windows))]
pub fn pick_and_copy() -> Result<Option<String>, String> {
    Err("Color Picker layar saat ini hanya tersedia di Windows".into())
}
