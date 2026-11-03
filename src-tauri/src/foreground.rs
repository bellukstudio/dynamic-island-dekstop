//! Membawa jendela ke depan dengan andal.
//!
//! Di Windows, SetForegroundWindow dari proses yang sedang tidak aktif (mis. dipicu shortcut global
//! saat aplikasi lain di depan) ditolak oleh "foreground lock": jendela sudah show() tapi tetap di
//! belakang sampai Alt+Tab / klik taskbar. Triknya: ketukan Alt palsu untuk membuka kunci itu, lalu
//! TOPMOST -> SetForegroundWindow -> NOTOPMOST secara berurutan (sinkron, tanpa thread tidur).

#[cfg(target_os = "windows")]
pub fn bring_to_front(w: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };

    let _ = w.unminimize();
    let _ = w.show();

    let Ok(h) = w.hwnd() else {
        let _ = w.set_focus();
        return;
    };
    let hwnd = HWND(h.0 as _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // ketukan Alt palsu: membuka foreground lock untuk proses ini
        keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);

        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        // lepas status "selalu di atas" langsung, jendela tetap berada di depan
        let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
    }
    let _ = w.set_focus();
}

#[cfg(not(target_os = "windows"))]
pub fn bring_to_front(w: &tauri::WebviewWindow) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_always_on_top(true);
    let _ = w.set_focus();
    let _ = w.set_always_on_top(false);
}

/// Sama seperti `bring_to_front`, tetapi untuk `tauri::Window` (jendela multi-webview).
#[cfg(target_os = "windows")]
pub fn bring_window_to_front(w: &tauri::Window) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };

    let _ = w.unminimize();
    let _ = w.show();

    let Ok(h) = w.hwnd() else {
        let _ = w.set_focus();
        return;
    };
    let hwnd = HWND(h.0 as _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);

        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
    }
    let _ = w.set_focus();
}

#[cfg(not(target_os = "windows"))]
pub fn bring_window_to_front(w: &tauri::Window) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_always_on_top(true);
    let _ = w.set_focus();
    let _ = w.set_always_on_top(false);
}