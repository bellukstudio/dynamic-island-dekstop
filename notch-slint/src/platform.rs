//! Fungsi khusus OS (Win32 di Windows; fallback di OS lain): kursor, jam lokal, area kerja, region jendela.

use notch_core::layout::WorkArea;

/// Apakah Windows memakai tema aplikasi terang (untuk `theme = "auto"`). Di OS lain: gelap.
pub fn os_light() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::core::w;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        );
        r.is_ok() && data == 1
    }
    #[cfg(not(windows))]
    false
}

/// Posisi kursor global (piksel fisik).
pub fn cursor_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        Some((p.x, p.y))
    }
    #[cfg(not(windows))]
    None
}

/// Status tombol kiri mouse global untuk drag native (Windows).
pub fn left_mouse_down() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        return GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0;
    }
    #[cfg(not(windows))]
    false
}

/// Cegah Windows menggambar bingkai klasik (caption biru muda "Notch" + tombol x, atau kotak putih seukuran jendela)
/// di atas notch. winit selalu memberi jendela style WS_CAPTION (agar Aero Snap jalan) dan menyembunyikannya lewat
/// WM_NCCALCSIZE; tetapi pada jendela dengan region (SetWindowRgn) bingkai tidak lagi digambar DWM melainkan USER,
/// tepat di dalam persegi jendela, setiap kali ada WM_NCPAINT atau pergantian aktivasi (klik tray, buka Pengaturan).
/// Subclass ini menahan keduanya. Aman dipanggil berulang (id subclass sama hanya memperbarui yang lama).
#[cfg(windows)]
unsafe fn no_border(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    use windows::Win32::UI::Shell::SetWindowSubclass;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_BORDER_COLOR,
        &DWMWA_COLOR_NONE as *const u32 as *const core::ffi::c_void,
        core::mem::size_of::<u32>() as u32,
    );
    let _ = SetWindowSubclass(hwnd, Some(nc_guard), NC_GUARD_ID, 0);
}

#[cfg(windows)]
const NC_GUARD_ID: usize = 0x4E43; // "NC"

#[cfg(windows)]
unsafe extern "system" fn nc_guard(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{LPARAM, LRESULT};
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{WM_NCACTIVATE, WM_NCPAINT};
    match msg {
        // Jangan menggambar area non-client sama sekali.
        WM_NCPAINT => LRESULT(0),
        // lParam = -1: winit tetap menerima perubahan aktivasi, tetapi DefWindowProc tidak menggambar ulang bingkai.
        WM_NCACTIVATE => DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

/// Sembunyikan jendela dari taskbar Windows (dan Alt+Tab). Idempotent, aman dipanggil berulang.
/// Tiga lapis karena winit memasang WS_EX_APPWINDOW (memaksa tombol taskbar) dan menimpa style jendela
/// setiap `show()` ulang: (1) `set_skip_taskbar` (ITaskbarList::DeleteTab), (2) buang WS_EX_APPWINDOW dan
/// pasang WS_EX_TOOLWINDOW, (3) hide/show sesaat bila style berubah agar shell membaca style baru.
/// Jendela winit hanya bisa diakses saat event loop aktif (dari timer/callback); sebelum `win.run()` berjalan
/// pemanggilan ini tidak berefek.
pub fn hide_from_taskbar(window: &slint::Window) {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::platform::windows::WindowExtWindows;
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_HIDE, SW_SHOWNA,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        let raw = window
            .with_winit_window(|w| {
                w.set_skip_taskbar(true);
                match w.window_handle().map(|h| h.as_raw()) {
                    Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                    _ => None,
                }
            })
            .flatten();
        if let Some(raw) = raw {
            unsafe {
                let hwnd = HWND(raw as *mut core::ffi::c_void);
                no_border(hwnd);
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let want = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
                if want != ex {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    let _ = ShowWindow(hwnd, SW_SHOWNA);
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Blok sampai komposisi DWM berikutnya selesai (maksimal satu frame, ±16 ms). Dipakai agar perubahan ukuran
/// jendela jatuh tepat setelah vsync. Tidak melakukan apa pun di platform lain.
pub fn wait_vsync() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
}

/// Atur posisi DAN ukuran jendela dalam satu panggilan (piksel fisik). `set_size` lalu `set_position` terpisah
/// menyisakan satu frame saat jendela sudah berukuran baru tetapi masih di posisi lama, sehingga notch yang
/// ditengahkan di dalamnya terlihat melompat/berkedip saat hub membuka atau menutup. Mengembalikan false bila
/// jendela native belum ada (atau bukan Windows); pemanggil lalu memakai API Slint biasa.
pub fn set_bounds(window: &slint::Window, x: i32, y: i32, w: u32, h: u32) -> bool {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
        let raw = window
            .with_winit_window(|win| match win.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                _ => None,
            })
            .flatten();
        if let Some(raw) = raw {
            unsafe {
                let hwnd = HWND(raw as *mut core::ffi::c_void);
                return SetWindowPos(hwnd, None, x, y, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE).is_ok();
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = (window, x, y, w, h);
        false
    }
}

/// Batasi bagian jendela yang tampil dan menerima klik ke persegi `(x, y, lebar, tinggi)` (piksel fisik, relatif
/// ke sudut jendela), atau `None` untuk menampilkan seluruh jendela lagi. Dipakai agar jendela tidak perlu
/// diperkecil saat notch menciut. Mengembalikan false bila gagal (atau bukan Windows).
pub fn set_region(window: &slint::Window, rect: Option<(i32, i32, i32, i32)>) -> bool {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn};
        let raw = window
            .with_winit_window(|win| match win.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                _ => None,
            })
            .flatten();
        if let Some(raw) = raw {
            unsafe {
                let hwnd = HWND(raw as *mut core::ffi::c_void);
                // Pasang penahan bingkai SEBELUM region berubah: SetWindowRgn memicu repaint non-client.
                no_border(hwnd);
                let region = rect.map(|(x, y, w, h)| CreateRectRgn(x, y, x + w, y + h));
                let ok = SetWindowRgn(hwnd, region, true) != 0;
                // Bila berhasil, sistem memiliki region itu; bila gagal, kita yang harus membebaskannya.
                if !ok {
                    if let Some(region) = region {
                        let _ = DeleteObject(region.into());
                    }
                }
                return ok;
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = (window, rect);
        false
    }
}

/// Mulai drag jendela native dari area judul kustom. Dipakai jendela Pengaturan tanpa title bar Windows.
pub fn begin_window_drag(window: &slint::Window) -> bool {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_NCLBUTTONDOWN};
        let raw = window.with_winit_window(|w| match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => Some(handle.hwnd.get()),
            _ => None,
        }).flatten();
        if let Some(raw) = raw {
            unsafe {
                let hwnd = HWND(raw as *mut core::ffi::c_void);
                let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(2)), Some(LPARAM(0)));
            }
            return true;
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        false
    }
}

/// Area kerja monitor (di luar taskbar) tempat notch berada, dalam piksel fisik: lebar penuh monitor,
/// vertikal mengikuti area kerja (sama dengan `monitor_work_area` di src-tauri/src/lib.rs).
/// `near` = titik (mis. tengah posisi kustom) untuk memilih monitor; `None` = monitor utama.
pub fn work_area(near: Option<(i32, i32)>) -> WorkArea {
    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
        };
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};
        let (pt, flag) = match near {
            // y bisa 100000 (menempel di dasar layar): jepit supaya titik tetap dekat monitor
            Some((x, y)) => (POINT { x, y: y.min(50_000) }, MONITOR_DEFAULTTONEAREST),
            None => (POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
        };
        let hmon = MonitorFromPoint(pt, flag);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            let (m, r) = (mi.rcMonitor, mi.rcWork);
            if r.bottom - r.top > 0 {
                // rcWork adalah sumber utama. Periksa juga HWND taskbar agar posisi bawah
                // tetap aman pada konfigurasi Windows yang melaporkan rcWork penuh.
                let class_name: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
                let mut tray_rect = RECT::default();
                let work_bottom = match FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null()) {
                    Ok(tray)
                        if !tray.0.is_null()
                            && GetWindowRect(tray, &mut tray_rect).is_ok()
                            && tray_rect.left < m.right
                            && tray_rect.right > m.left
                            && tray_rect.top >= r.top
                            && tray_rect.top < m.bottom =>
                    {
                        r.bottom.min(tray_rect.top)
                    }
                    _ => r.bottom,
                };
                if work_bottom > r.top {
                    return WorkArea {
                        x: m.left,
                        y: r.top,
                        w: m.right - m.left,
                        h: work_bottom - r.top,
                    };
                }
            }
        }
    }
    let _ = near;
    WorkArea { x: 0, y: 0, w: 1920, h: 1080 }
}

/// Deteksi aplikasi foreground yang menutupi seluruh monitor (fullscreen/borderless).
/// Maximized biasa tidak terdeteksi karena masih menyisakan area taskbar.
pub fn foreground_is_fullscreen() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
        };

        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        // Jendela milik proses ini (menu tray, jendela Pengaturan) dan jendela shell Windows (taskbar, flyout tray,
        // Start, Alt+Tab) sering seukuran monitor atau dibuat transparan, tetapi bukan aplikasi fullscreen.
        // Tanpa pengecualian ini notch ikut disembunyikan lalu ditampilkan ulang (tampil putih) tiap tray dibuka.
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return false;
        }
        let mut class = [0u16; 64];
        let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..class_len.min(class.len())]);
        const SHELL_CLASSES: [&str; 14] = [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland",
            "XamlExplorerHostIslandWindow",
            "Windows.UI.Core.CoreWindow",
            "ForegroundStaging",
            "MultitaskingViewFrame",
            "TaskSwitcherWnd",
            "TaskListThumbnailWnd",
            "#32768",
            "tray_icon_app",
        ];
        if SHELL_CLASSES.contains(&class.as_str()) {
            return false;
        }
        let mut window_rect = RECT::default();
        if !GetWindowRect(hwnd, &mut window_rect).is_ok() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let bounds = info.rcMonitor;
        const TOLERANCE: i32 = 2;
        window_rect.left <= bounds.left + TOLERANCE
            && window_rect.top <= bounds.top + TOLERANCE
            && window_rect.right >= bounds.right - TOLERANCE
            && window_rect.bottom >= bounds.bottom - TOLERANCE
    }
    #[cfg(not(windows))]
    false
}

/// (jam, menit, hari, bulan, tahun) waktu lokal.
pub fn local_time() -> (u32, u32, u32, u32, u32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let t = GetLocalTime();
        (t.wHour as u32, t.wMinute as u32, t.wDay as u32, t.wMonth as u32, t.wYear as u32)
    }
    #[cfg(not(windows))]
    {
        // Fallback UTC tanpa dependensi tambahan.
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = s / 86_400;
        let (y, m, d) = civil_from_days(days as i64);
        (((s / 3600) % 24) as u32, ((s / 60) % 60) as u32, d, m, y)
    }
}

/// (detik, hari dalam pekan: 0 = Minggu).
pub fn local_sec_wday() -> (u32, u32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let t = GetLocalTime();
        (t.wSecond as u32, t.wDayOfWeek as u32)
    }
    #[cfg(not(windows))]
    {
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        ((s % 60) as u32, (((s / 86_400) + 4) % 7) as u32)
    }
}

/// Hari sejak 1970-01-01 -> (tahun, bulan, tanggal), algoritma Howard Hinnant.
#[allow(dead_code)]
pub fn civil_from_days(z: i64) -> (u32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400) as u32 + u32::from(m <= 2);
    (y, m, d)
}
