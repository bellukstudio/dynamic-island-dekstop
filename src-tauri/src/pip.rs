//! Prototipe: jendela Picture-in-Picture Chrome/Edge "masuk" ke notch.
//!
//! Alur (hanya Windows):
//! 1. Saat tombol kiri mouse dilepas di area notch, cari jendela PiP di bawah kursor: jendela
//!    Chromium (`Chrome_WidgetWin_1`) yang topmost, terlihat, dan berukuran kecil. Jendela itu
//!    "menempel": ukurannya dikunci dan Rust memberi tahu frontend lewat event `pip-dock`.
//! 2. Frontend memperbesar panel media notch supaya memuat PiP, lalu memanggil `pip_set_visible`
//!    setiap kali panel media tampil/hilang. PiP muncul bersama panel media dan disembunyikan
//!    (ShowWindow SW_HIDE) saat notch menutup. Audio tetap berjalan karena videonya tetap di tab.
//! 3. Karena PiP adalah jendela lain di atas notch, kursor di atas PiP membuat webview notch
//!    mengira pointer keluar. Rust mengirim event `pip-hover` supaya notch tetap terbuka.
//! 4. Menyeret PiP menjauh dan melepasnya di luar area notch melepas tempelannya; menutup PiP juga.
//!
//! Hemat sumber daya: saat tidak ada PiP yang menempel, thread hanya membaca status tombol mouse
//! tiap 100 ms. Matikan fitur dengan env `NOTCH_PIP_DOCK=0`.
//!
//! Batasan prototipe: semua jendela Chromium kecil yang topmost bisa terdeteksi (bukan hanya PiP);
//! urutan z antara notch dan PiP ditentukan Windows.

use std::sync::Mutex;

/// Permintaan dari frontend: (tampilkan?, x tengah-atas PiP, y sisi atas PiP, nomor urut yang
/// bertambah setiap kali frontend menjawab). x dan y adalah koordinat viewport webview notch
/// dalam piksel logis, jadi posisi PiP mengikuti tata letak halaman media yang sebenarnya.
static REQUEST: Mutex<(bool, f64, f64, u64)> = Mutex::new((false, 0.0, 0.0, 0));

/// Dipanggil frontend setiap kali panel media (dengan PiP) tampil atau hilang.
#[tauri::command]
pub fn pip_set_visible(visible: bool, x: f64, y: f64) {
    if let Ok(mut r) = REQUEST.lock() {
        *r = (visible, x, y, r.3.wrapping_add(1));
    }
}

#[cfg(not(target_os = "windows"))]
pub fn start(_win: tauri::WebviewWindow) {}

#[cfg(target_os = "windows")]
pub use imp::start;

#[cfg(target_os = "windows")]
mod imp {
    use std::time::{Duration, Instant};

    use tauri::Emitter;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, IsWindow, IsWindowVisible,
        SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        SW_HIDE, WS_EX_TOPMOST,
    };

    // ---- Penyetelan (piksel logis, dikalikan skala layar) ----
    /// Lebar PiP saat menempel.
    const PIP_W: f64 = 360.0;
    /// Tinggi maksimum PiP saat menempel.
    const PIP_MAX_H: f64 = 240.0;
    /// Margin di sekitar area klik notch untuk menjatuhkan PiP.
    const DROP_MARGIN: f64 = 60.0;
    /// Tinggi area jatuh di bawah area klik notch.
    const DROP_BELOW: f64 = 260.0;
    /// Selisih (px logis) sebelum PiP dianggap sengaja digeser pengguna.
    const MOVE_TOLERANCE: f64 = 12.0;
    /// Setelah menempel, tahan PiP tetap terlihat sampai frontend menjawab (membuka panel media),
    /// paling lama selama ini. Begitu frontend menjawab, PiP mengikuti notch persis.
    const GRACE: Duration = Duration::from_millis(1800);

    #[derive(Clone, Copy)]
    struct Geo {
        scale: f64,
        origin: (i32, i32),        // pojok kiri-atas viewport notch (piksel fisik)
        hit: (i32, i32, i32, i32), // l, t, r, b (piksel fisik)
    }

    #[derive(Clone, Copy)]
    struct Dock {
        hwnd: isize,
        w: i32,
        h: i32,
    }

    fn hwnd_of(raw: isize) -> HWND {
        HWND(raw as *mut core::ffi::c_void)
    }

    fn rect_of(hwnd: HWND) -> Option<RECT> {
        let mut r = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut r).ok()? };
        Some(r)
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    /// Jendela Chromium kecil yang topmost dan terlihat = kandidat PiP.
    fn pip_rect(hwnd: HWND) -> Option<RECT> {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return None;
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOPMOST.0 == 0 {
                return None;
            }
        }
        if class_name(hwnd) != "Chrome_WidgetWin_1" {
            return None;
        }
        let r = rect_of(hwnd)?;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        ((200..=1100).contains(&w) && (110..=800).contains(&h)).then_some(r)
    }

    struct Find {
        x: i32,
        y: i32,
        found: Option<(isize, RECT)>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let find = &mut *(lparam.0 as *mut Find);
        if let Some(r) = pip_rect(hwnd) {
            if find.x >= r.left && find.x <= r.right && find.y >= r.top && find.y <= r.bottom {
                find.found = Some((hwnd.0 as isize, r));
                return BOOL(0); // hentikan pencarian
            }
        }
        BOOL(1)
    }

    /// Jendela PiP yang berada di titik (x, y), dari yang paling atas.
    fn pip_at(x: i32, y: i32) -> Option<(isize, RECT)> {
        let mut find = Find { x, y, found: None };
        unsafe {
            let _ = EnumWindows(Some(enum_cb), LPARAM(&mut find as *mut Find as isize));
        }
        find.found
    }

    fn geometry(win: &tauri::WebviewWindow) -> Option<Geo> {
        let pos = win.outer_position().ok()?;
        let origin = win.inner_position().map(|p| (p.x, p.y)).unwrap_or((pos.x, pos.y));
        let scale = win.scale_factor().ok()?.max(0.1);
        let (hx, hy, hw, hh) = *crate::HIT.lock().ok()?;
        let l = pos.x + (hx as f64 * scale) as i32;
        let t = pos.y + (hy as f64 * scale) as i32;
        Some(Geo {
            scale,
            origin,
            hit: (l, t, l + (hw as f64 * scale) as i32, t + (hh as f64 * scale) as i32),
        })
    }

    /// Titik (x, y) berada di area tempat PiP boleh dijatuhkan?
    fn in_zone(g: &Geo, x: i32, y: i32) -> bool {
        let m = (DROP_MARGIN * g.scale) as i32;
        let below = (DROP_BELOW * g.scale) as i32;
        x >= g.hit.0 - m && x <= g.hit.2 + m && y >= g.hit.1 - m && y <= g.hit.3 + below
    }

    fn make_dock(hwnd: isize, r: &RECT, g: &Geo) -> Dock {
        let cur_w = (r.right - r.left).max(1) as f64;
        let ratio = ((r.bottom - r.top) as f64 / cur_w).clamp(0.3, 1.2);
        let max_h = PIP_MAX_H * g.scale;
        // Lebar mengikuti tinggi maksimum agar PiP yang tinggi (mis. 4:3) tetap muat di panel.
        let w = (PIP_W * g.scale).min(max_h / ratio).round() as i32;
        let h = ((w as f64) * ratio).round() as i32;
        Dock { hwnd, w, h }
    }

    /// Pojok kiri-atas PiP dari titik tengah-atas (x, y) dalam koordinat viewport notch (piksel logis).
    fn desired(d: &Dock, g: &Geo, x_logical: f64, y_logical: f64) -> (i32, i32) {
        (
            g.origin.0 + (x_logical * g.scale).round() as i32 - d.w / 2,
            g.origin.1 + (y_logical * g.scale).round() as i32,
        )
    }

    fn moved_away(r: &RECT, want: (i32, i32), scale: f64) -> bool {
        let tol = (MOVE_TOLERANCE * scale) as i32;
        (r.left - want.0).abs() > tol || (r.top - want.1).abs() > tol
    }

    fn place(d: &Dock, x: i32, y: i32, show: bool) {
        let flags = if show { SWP_NOACTIVATE | SWP_SHOWWINDOW } else { SWP_NOACTIVATE };
        unsafe {
            let _ = SetWindowPos(hwnd_of(d.hwnd), Some(HWND_TOPMOST), x, y, d.w, d.h, flags);
        }
    }

    fn hide(d: &Dock) {
        unsafe {
            let _ = ShowWindow(hwnd_of(d.hwnd), SW_HIDE);
        }
    }

    pub fn start(win: tauri::WebviewWindow) {
        if std::env::var("NOTCH_PIP_DOCK").map(|v| v == "0").unwrap_or(false) {
            return;
        }
        std::thread::spawn(move || {
            let mut docked: Option<Dock> = None;
            let mut shown = true; // jendela PiP sedang terlihat?
            let mut hovering = false;
            let mut grace_until = Instant::now();
            let mut dock_seq = 0u64;
            let mut was_down = false;

            let undock = |docked: &mut Option<Dock>, shown: &mut bool, hovering: &mut bool| {
                if let Some(d) = docked.take() {
                    if !*shown {
                        // jangan tinggalkan PiP dalam keadaan tersembunyi
                        if let Some(r) = rect_of(hwnd_of(d.hwnd)) {
                            place(&Dock { hwnd: d.hwnd, w: r.right - r.left, h: r.bottom - r.top }, r.left, r.top, true);
                        } else {
                            unsafe {
                                let _ = SetWindowPos(hwnd_of(d.hwnd), Some(HWND_TOPMOST), 0, 0, 0, 0,
                                    SWP_NOACTIVATE | SWP_SHOWWINDOW | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE);
                            }
                        }
                    }
                }
                *shown = true;
                *hovering = false;
            };

            loop {
                let idle = docked.is_none();
                std::thread::sleep(Duration::from_millis(if idle { 100 } else { 80 }));
                let down = crate::lbutton_down();
                let released = was_down && !down;
                was_down = down;
                if idle && !released {
                    continue; // tidak ada PiP menempel dan tidak ada tombol dilepas: tidak mengukur apa pun
                }
                let (Some(g), Ok(cur)) = (geometry(&win), win.cursor_position()) else {
                    continue;
                };
                let (cx, cy) = (cur.x as i32, cur.y as i32);
                let (want_visible, x_off, y_off, seq) = *super::REQUEST.lock().unwrap_or_else(|e| e.into_inner());

                if let Some(d) = docked {
                    if unsafe { !IsWindow(Some(hwnd_of(d.hwnd))).as_bool() } {
                        docked = None; // PiP ditutup
                        shown = true;
                        hovering = false;
                        let _ = win.emit("pip-dock", serde_json::json!({ "docked": false }));
                        let _ = win.emit("pip-hover", serde_json::json!({ "hover": false }));
                    }
                }

                if released {
                    let target = if in_zone(&g, cx, cy) { pip_at(cx, cy) } else { None };
                    match (target, docked) {
                        (Some((h, r)), prev) => {
                            let same = prev.map(|d| d.hwnd) == Some(h);
                            let still = prev
                                .map(|d| !moved_away(&r, desired(&d, &g, x_off, y_off), g.scale))
                                .unwrap_or(false);
                            if !(same && still) {
                                let d = make_dock(h, &r, &g);
                                // Kunci ukuran saja; posisi tetap di tempat PiP dijatuhkan sampai frontend
                                // mengirim posisi yang benar (jangan pakai permintaan lama).
                                place(&d, r.left, r.top, true);
                                docked = Some(d);
                                shown = true;
                                grace_until = Instant::now() + GRACE;
                                if let Ok(mut req) = super::REQUEST.lock() {
                                    req.0 = false; // buang permintaan "tampil" lama
                                    dock_seq = req.3;
                                }
                                let _ = win.emit(
                                    "pip-dock",
                                    serde_json::json!({
                                        "docked": true,
                                        "w": d.w as f64 / g.scale,
                                        "h": d.h as f64 / g.scale,
                                    }),
                                );
                            }
                        }
                        (None, Some(d)) => {
                            // Dilepas di luar area notch: lepas tempelan bila PiP memang digeser menjauh.
                            if shown {
                                if let Some(r) = rect_of(hwnd_of(d.hwnd)) {
                                    if moved_away(&r, desired(&d, &g, x_off, y_off), g.scale) {
                                        undock(&mut docked, &mut shown, &mut hovering);
                                        let _ = win.emit("pip-dock", serde_json::json!({ "docked": false }));
                                        let _ = win.emit("pip-hover", serde_json::json!({ "hover": false }));
                                    }
                                }
                            }
                        }
                        (None, None) => {}
                    }
                }

                let Some(d) = docked else { continue };
                // Masa tunggu berakhir begitu frontend menjawab (nomor urut berubah) atau waktunya habis.
                let in_grace = seq == dock_seq && Instant::now() < grace_until;
                let visible_now = want_visible || in_grace;

                if visible_now && in_grace && !want_visible {
                    // Menunggu frontend membuka panel dan mengirim posisi: jangan pindahkan PiP dulu.
                } else if visible_now {
                    let (x, y) = desired(&d, &g, x_off, y_off);
                    let cur_rect = if shown { rect_of(hwnd_of(d.hwnd)) } else { None };
                    let dragging_pip = down
                        && cur_rect
                            .map(|r| cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom)
                            .unwrap_or(false);
                    if !dragging_pip {
                        let in_place = cur_rect
                            .map(|r| r.left == x && r.top == y && r.right - r.left == d.w && r.bottom - r.top == d.h)
                            .unwrap_or(false);
                        if !shown || !in_place {
                            place(&d, x, y, !shown);
                            shown = true;
                        }
                    }
                } else if shown {
                    hide(&d);
                    shown = false;
                }

                // Kursor di atas PiP = notch dianggap masih di-hover (webview notch tidak melihatnya).
                let over = shown
                    && rect_of(hwnd_of(d.hwnd))
                        .map(|r| cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom)
                        .unwrap_or(false);
                if over != hovering {
                    hovering = over;
                    let _ = win.emit("pip-hover", serde_json::json!({ "hover": over }));
                }
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn geo() -> Geo {
            Geo { scale: 1.0, origin: (660, 0), hit: (680, 0, 1240, 242) }
        }

        #[test]
        fn zone_covers_notch_and_area_below() {
            let g = geo();
            assert!(in_zone(&g, 960, 100));
            assert!(in_zone(&g, 960, 400)); // di bawah panel, tempat PiP berada
            assert!(!in_zone(&g, 100, 100));
            assert!(!in_zone(&g, 960, 900));
        }

        #[test]
        fn dock_is_centered_below_panel() {
            let g = geo();
            let r = RECT { left: 0, top: 0, right: 400, bottom: 300 };
            let d = make_dock(1, &r, &g);
            assert!(d.h <= 240);
            let (x, y) = desired(&d, &g, 300.0, 208.0);
            assert_eq!(y, 208);
            assert_eq!(x, 660 + 300 - d.w / 2);
        }

        #[test]
        fn tall_pip_shrinks_width_to_fit() {
            let g = geo();
            let r = RECT { left: 0, top: 0, right: 400, bottom: 400 }; // 1:1
            let d = make_dock(1, &r, &g);
            assert!(d.h <= 240 && d.w <= 360);
        }

        #[test]
        fn small_nudges_do_not_undock() {
            let r = RECT { left: 772, top: 175, right: 1152, bottom: 400 };
            assert!(!moved_away(&r, (770, 172), 1.0));
            assert!(moved_away(&r, (400, 172), 1.0));
        }
    }
}
