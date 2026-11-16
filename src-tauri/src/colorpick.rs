// Color picker layar: dipicu shortcut global, langsung aktif tanpa panel/tombol.
// Windows: loupe (kaca pembesar bulat) mengikuti pointer seperti EyeDropper bawaan browser; klik kiri / Enter
// mengambil warna, Esc membatalkan. Loupe adalah jendela Win32 kecil yang tembus klik, digambar dengan GDI.
// Platform lain: mengembalikan "unsupported" supaya frontend memakai EyeDropper WebView.
use std::sync::atomic::{AtomicBool, Ordering};

static PICKING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Batalkan sesi pemilihan warna yang sedang berjalan (mis. shortcut ditekan lagi).
#[tauri::command]
pub fn color_pick_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Tunggu klik/Enter lalu kembalikan warna "#RRGGBB" di bawah kursor. None = dibatalkan / timeout.
#[tauri::command]
pub async fn color_pick_screen() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pick)
        .await
        .map_err(|e| format!("Color picker gagal: {e}"))?
}

#[cfg(target_os = "windows")]
mod loupe {
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Jumlah piksel layar yang ditampilkan per sisi (ganjil; piksel tengah = piksel yang diambil).
    pub const GRID: i32 = 11;
    const CELL: i32 = 12; // ukuran satu piksel layar setelah diperbesar
    const PAD: i32 = 4;
    const CIRCLE: i32 = GRID * CELL; // 132
    const W: i32 = CIRCLE + PAD * 2; // 140
    const LABEL_TOP: i32 = PAD + CIRCLE + 6;
    const LABEL_H: i32 = 28;
    const H: i32 = LABEL_TOP + LABEL_H + PAD;

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_NCHITTEST {
            return LRESULT(-1); // HTTRANSPARENT: klik diteruskan ke jendela di bawahnya
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    pub struct Loupe {
        hwnd: HWND,
        hinst: HINSTANCE,
        screen: HDC,
        // Snapshot seluruh layar yang diambil SEBELUM loupe tampil. Loupe tidak boleh membaca layar langsung,
        // karena jendelanya sendiri ikut tertangkap (hasilnya loupe memperbesar dirinya sendiri).
        snap: HDC,
        snap_bmp: HBITMAP,
        old_snap: HGDIOBJ,
        vx: i32,
        vy: i32,
        mem: HDC,
        bmp: HBITMAP,
        old_bmp: HGDIOBJ,
        font: HFONT,
        old_font: HGDIOBJ,
        shown: bool,
    }

    impl Loupe {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let screen = GetDC(None);
                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
                let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
                let snap = CreateCompatibleDC(Some(screen));
                let snap_bmp = CreateCompatibleBitmap(screen, vw, vh);
                let old_snap = SelectObject(snap, HGDIOBJ(snap_bmp.0));
                let _ = BitBlt(snap, 0, 0, vw, vh, Some(screen), vx, vy, SRCCOPY);

                let hmod = GetModuleHandleW(None).map_err(|e| format!("Modul tidak ditemukan: {e}"))?;
                let hinst = HINSTANCE(hmod.0);
                let class = w!("NotchColorLoupe");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinst,
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassW(&wc); // 0 bila sudah terdaftar: tidak masalah

                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    class,
                    w!(""),
                    WS_POPUP,
                    -10000,
                    -10000,
                    W,
                    H,
                    None,
                    None,
                    Some(hinst),
                    None,
                )
                .map_err(|e| format!("Jendela loupe gagal dibuat: {e}"))?;
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

                // Bentuk jendela: lingkaran (kaca pembesar) + pil kecil untuk label HEX.
                let circle = CreateEllipticRgn(PAD, PAD, PAD + CIRCLE, PAD + CIRCLE);
                let pill = CreateRoundRectRgn(PAD + 8, LABEL_TOP, W - PAD - 8, LABEL_TOP + LABEL_H, 14, 14);
                let rgn = CreateRectRgn(0, 0, 0, 0);
                CombineRgn(Some(rgn), Some(circle), Some(pill), RGN_OR);
                let _ = DeleteObject(HGDIOBJ(circle.0));
                let _ = DeleteObject(HGDIOBJ(pill.0));
                SetWindowRgn(hwnd, Some(rgn), false); // rgn kini dimiliki sistem

                let mem = CreateCompatibleDC(Some(screen));
                let bmp = CreateCompatibleBitmap(screen, W, H);
                let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));
                let font = CreateFontW(
                    -15,
                    0,
                    0,
                    0,
                    700,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe UI"),
                );
                let old_font = SelectObject(mem, HGDIOBJ(font.0));
                SetBkMode(mem, TRANSPARENT);
                SetStretchBltMode(mem, COLORONCOLOR); // piksel tajam, tanpa interpolasi

                Ok(Loupe { hwnd, hinst, screen, snap, snap_bmp, old_snap, vx, vy, mem, bmp, old_bmp, font, old_font, shown: false })
            }
        }

        /// Warna piksel layar (0xRRGGBB) dari snapshot, None bila di luar layar.
        pub fn pixel(&self, x: i32, y: i32) -> Option<u32> {
            let v = unsafe { GetPixel(self.snap, x - self.vx, y - self.vy) }.0;
            if v == 0xFFFF_FFFF {
                return None;
            }
            Some(((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF))
        }

        /// Kosongkan antrean pesan jendela loupe.
        pub fn pump(&self) {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }

        /// Pindahkan loupe ke pointer dan gambar ulang isinya.
        pub fn update(&mut self, p: POINT) {
            unsafe {
                let center = self.pixel(p.x, p.y);
                let (r, g, b) = match center {
                    Some(c) => ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF),
                    None => (0, 0, 0),
                };
                let rgb = |r: u32, g: u32, b: u32| COLORREF(r | (g << 8) | (b << 16));

                // latar
                let bg = CreateSolidBrush(rgb(24, 27, 36));
                FillRect(self.mem, &RECT { left: 0, top: 0, right: W, bottom: H }, bg);
                let _ = DeleteObject(HGDIOBJ(bg.0));

                // area layar yang diperbesar
                let half = GRID / 2;
                let _ = StretchBlt(
                    self.mem,
                    PAD,
                    PAD,
                    CIRCLE,
                    CIRCLE,
                    Some(self.snap),
                    p.x - half - self.vx,
                    p.y - half - self.vy,
                    GRID,
                    GRID,
                    SRCCOPY,
                );

                // bingkai lingkaran + kotak piksel tengah (hitam + putih agar terlihat di warna apa pun)
                let null_brush = GetStockObject(NULL_BRUSH);
                let old_brush = SelectObject(self.mem, null_brush);
                let ring = CreatePen(PS_SOLID, 4, rgb(255, 255, 255));
                let old_pen = SelectObject(self.mem, HGDIOBJ(ring.0));
                let _ = Ellipse(self.mem, PAD, PAD, PAD + CIRCLE, PAD + CIRCLE);
                let cx = PAD + half * CELL;
                let black = CreatePen(PS_SOLID, 2, rgb(0, 0, 0));
                SelectObject(self.mem, HGDIOBJ(black.0));
                let _ = Rectangle(self.mem, cx, cx, cx + CELL + 1, cx + CELL + 1);
                let white = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
                SelectObject(self.mem, HGDIOBJ(white.0));
                let _ = Rectangle(self.mem, cx + 2, cx + 2, cx + CELL - 1, cx + CELL - 1);
                SelectObject(self.mem, old_pen);
                SelectObject(self.mem, old_brush);
                let _ = DeleteObject(HGDIOBJ(ring.0));
                let _ = DeleteObject(HGDIOBJ(black.0));
                let _ = DeleteObject(HGDIOBJ(white.0));

                // label: swatch warna + HEX
                let sw = CreateSolidBrush(rgb(r, g, b));
                let sy = LABEL_TOP + (LABEL_H - 14) / 2;
                let sx = PAD + 18;
                FillRect(self.mem, &RECT { left: sx, top: sy, right: sx + 14, bottom: sy + 14 }, sw);
                let _ = DeleteObject(HGDIOBJ(sw.0));
                SetTextColor(self.mem, rgb(255, 255, 255));
                let mut text: Vec<u16> = format!("#{r:02X}{g:02X}{b:02X}").encode_utf16().collect();
                let mut rc = RECT { left: sx + 22, top: LABEL_TOP, right: W - PAD - 10, bottom: LABEL_TOP + LABEL_H };
                DrawTextW(self.mem, &mut text, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

                // posisikan: pusat lingkaran tepat di pointer
                let x = p.x - W / 2;
                let y = p.y - (PAD + CIRCLE / 2);
                let flags = if self.shown { SWP_NOSIZE | SWP_NOACTIVATE } else { SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW };
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, 0, 0, flags);
                self.shown = true;

                // tampilkan
                let wdc = GetDC(Some(self.hwnd));
                let _ = BitBlt(wdc, 0, 0, W, H, Some(self.mem), 0, 0, SRCCOPY);
                ReleaseDC(Some(self.hwnd), wdc);
            }
        }
    }

    impl Drop for Loupe {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
                let _ = UnregisterClassW(w!("NotchColorLoupe"), Some(self.hinst));
                SelectObject(self.snap, self.old_snap);
                let _ = DeleteObject(HGDIOBJ(self.snap_bmp.0));
                let _ = DeleteDC(self.snap);
                SelectObject(self.mem, self.old_font);
                SelectObject(self.mem, self.old_bmp);
                let _ = DeleteObject(HGDIOBJ(self.font.0));
                let _ = DeleteObject(HGDIOBJ(self.bmp.0));
                let _ = DeleteDC(self.mem);
                ReleaseDC(None, self.screen);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn pick() -> Result<Option<String>, String> {
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON, VK_RETURN};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    if PICKING.swap(true, Ordering::SeqCst) {
        return Err("busy".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            PICKING.store(false, Ordering::SeqCst);
        }
    }
    let _guard = Guard;
    CANCEL.store(false, Ordering::SeqCst);

    let down = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 };
    let (lb, esc, ret) = (VK_LBUTTON.0 as i32, VK_ESCAPE.0 as i32, VK_RETURN.0 as i32);
    let nap = || std::thread::sleep(Duration::from_millis(8));

    // Lepas dulu tombol yang mungkin masih tertekan saat shortcut dipicu.
    let t0 = Instant::now();
    while (down(lb) || down(esc) || down(ret)) && t0.elapsed() < Duration::from_millis(800) {
        nap();
    }

    let mut loupe = loupe::Loupe::new()?;
    let started = Instant::now();
    loop {
        loupe.pump();
        if CANCEL.load(Ordering::SeqCst) || down(esc) || started.elapsed() > Duration::from_secs(60) {
            return Ok(None);
        }
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.map_err(|e| format!("Posisi kursor tidak terbaca: {e}"))?;

        if down(lb) || down(ret) {
            let c = loupe.pixel(p.x, p.y).ok_or("Piksel tidak dapat dibaca")?;
            drop(loupe); // sembunyikan loupe sebelum klik diteruskan
            // tunggu klik dilepas supaya tidak terbaca dua kali
            let t1 = Instant::now();
            while (down(lb) || down(ret)) && t1.elapsed() < Duration::from_millis(500) {
                nap();
            }
            return Ok(Some(format!("#{:06X}", c & 0xFF_FFFF)));
        }
        loupe.update(p);
        nap();
    }
}

#[cfg(not(target_os = "windows"))]
fn pick() -> Result<Option<String>, String> {
    Err("unsupported".into())
}