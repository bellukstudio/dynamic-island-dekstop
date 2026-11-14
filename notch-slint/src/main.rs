//! Prototipe Notch dengan Slint. Target utama: Windows.
//! Di OS lain hanya untuk uji kompilasi (posisi kursor dan jam memakai fallback).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use notch_core::stats::StatsMonitor;
use slint::{ComponentHandle, Image, PhysicalPosition, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};

slint::include_modules!();

// Ukuran jendela = pil + bahu melengkung (14 px kiri/kanan) + sedikit ruang animasi.
const COLLAPSED: (u32, u32) = (152, 38);
const EXPANDED: (u32, u32) = (592, 202);
const PAGE_STATS: i32 = 3;
const POMO_SECS: u32 = 25 * 60;

mod platform {
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

    /// Lebar layar utama (piksel fisik).
    pub fn screen_width() -> i32 {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
            GetSystemMetrics(SM_CXSCREEN)
        }
        #[cfg(not(windows))]
        1920
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

    /// Hari sejak 1970-01-01 -> (tahun, bulan, tanggal), algoritma Howard Hinnant.
    #[cfg(not(windows))]
    fn civil_from_days(z: i64) -> (u32, u32, u32) {
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
}

/// Decode PNG kecil yang ditanam di binary menjadi Image Slint. Tanpa decoder gambar umum,
/// jadi binary dan RAM tetap kecil. Mengembalikan gambar kosong jika formatnya bukan RGBA8.
fn load_png(bytes: &'static [u8]) -> Image {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let Ok(mut reader) = decoder.read_info() else { return Image::default() };
    let mut buf = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut buf) else { return Image::default() };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Image::default();
    }
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &buf[..info.buffer_size()],
        info.width,
        info.height,
    ))
}

fn gib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

/// Kecepatan jaringan dalam B/s -> teks pendek (KB/s atau MB/s).
fn fmt_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB/s", bps / 1024.0)
    }
}

fn fmt_mmss(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Letakkan jendela di tengah atas layar untuk ukuran tertentu.
fn place(win: &NotchWindow, size: (u32, u32)) {
    let w = win.window();
    w.set_size(PhysicalSize::new(size.0, size.1));
    let x = (platform::screen_width() - size.0 as i32) / 2;
    w.set_position(PhysicalPosition::new(x, 0));
}

fn main() -> Result<(), slint::PlatformError> {
    let win = NotchWindow::new()?;
    place(&win, COLLAPSED);

    win.set_emoji_coffee(load_png(include_bytes!("../assets/emoji/2615.png")));
    win.set_emoji_tomato(load_png(include_bytes!("../assets/emoji/1f345.png")));
    win.set_emoji_moon(load_png(include_bytes!("../assets/emoji/1f319.png")));

    // --- Hover + halaman aktif ---
    // Stats (dari crate notch-core) hanya dibuat saat pertama dibutuhkan dan hanya disampel
    // selama notch melebar DAN halaman Stats terlihat, sama seperti versi Tauri.
    let shrink_timer = Rc::new(Timer::default());
    let stats_timer = Rc::new(Timer::default());
    let monitor: Rc<RefCell<Option<StatsMonitor>>> = Rc::new(RefCell::new(None));

    let sync_stats: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let stats_timer = stats_timer.clone();
        let monitor = monitor.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            let wanted = w.get_expanded() && w.get_page() == PAGE_STATS;
            if !wanted {
                stats_timer.stop();
                return;
            }
            if stats_timer.running() {
                return;
            }
            monitor.borrow_mut().get_or_insert_with(StatsMonitor::new).resume();
            let weak_s = weak.clone();
            let monitor = monitor.clone();
            stats_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
                let Some(w) = weak_s.upgrade() else { return };
                if let Some(m) = monitor.borrow_mut().as_mut() {
                    let s = m.sample();
                    w.set_cpu(s.cpu);
                    w.set_ram(s.ram);
                    w.set_disk(s.disk);
                    w.set_cpu_text(format!("{:.0}%", s.cpu).into());
                    w.set_ram_text(format!("{:.1} / {:.1} GiB", gib(s.ram_used), gib(s.ram_total)).into());
                    w.set_disk_text(
                        format!("{:.0} / {:.0} GiB", gib(s.disk_used), gib(s.disk_total)).into(),
                    );
                    w.set_net_text(
                        format!("\u{2193} {}   \u{2191} {}", fmt_rate(s.down), fmt_rate(s.up)).into(),
                    );
                    w.set_sys_text(format!("{} threads \u{00B7} {} proses", s.cpu_cores, s.processes).into());
                }
            });
        })
    };

    {
        let sync = sync_stats.clone();
        win.on_page_changed(move |_| sync());
    }
    {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let sync = sync_stats.clone();
        win.on_hover_changed(move |hovered| {
            let Some(w) = weak.upgrade() else { return };
            if hovered {
                shrink.stop();
                place(&w, EXPANDED);
                w.set_expanded(true);
                sync();
            } else {
                w.set_expanded(false);
                sync();
                let weak2 = weak.clone();
                shrink.start(TimerMode::SingleShot, Duration::from_millis(260), move || {
                    if let Some(w) = weak2.upgrade() {
                        if !w.get_expanded() {
                            place(&w, COLLAPSED);
                        }
                    }
                });
            }
        });
    }

    // --- Jam: cek tiap detik, tapi hanya set properti kalau teks berubah ---
    let clock_timer = Timer::default();
    {
        let weak = win.as_weak();
        let last = Rc::new(Cell::new((99u32, 99u32)));
        let tick = move || {
            let Some(w) = weak.upgrade() else { return };
            let (h, m, d, mo, y) = platform::local_time();
            if last.get() != (h, m) {
                last.set((h, m));
                w.set_clock_text(format!("{h:02}:{m:02}").into());
                w.set_date_text(format!("{d:02}/{mo:02}/{y}").into());
            }
        };
        tick();
        clock_timer.start(TimerMode::Repeated, Duration::from_secs(1), tick);
    }

    // --- Pomodoro ---
    let remaining = Rc::new(Cell::new(POMO_SECS));
    let running = Rc::new(Cell::new(false));
    let pomo_timer = Timer::default();
    {
        let weak = win.as_weak();
        let (rem, run) = (remaining.clone(), running.clone());
        pomo_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if !run.get() { return; }
            let Some(w) = weak.upgrade() else { return };
            let left = rem.get().saturating_sub(1);
            rem.set(left);
            if left == 0 {
                run.set(false);
                w.set_pomo_running(false);
            }
            w.set_pomo_text(fmt_mmss(left).into());
        });
    }
    {
        let weak = win.as_weak();
        let (rem, run) = (remaining.clone(), running.clone());
        win.on_toggle_pomo(move || {
            if rem.get() == 0 { rem.set(POMO_SECS); }
            run.set(!run.get());
            if let Some(w) = weak.upgrade() {
                w.set_pomo_running(run.get());
                w.set_pomo_text(fmt_mmss(rem.get()).into());
            }
        });
    }
    {
        let weak = win.as_weak();
        let (rem, run) = (remaining.clone(), running.clone());
        win.on_reset_pomo(move || {
            rem.set(POMO_SECS);
            run.set(false);
            if let Some(w) = weak.upgrade() {
                w.set_pomo_running(false);
                w.set_pomo_text(fmt_mmss(POMO_SECS).into());
            }
        });
    }

    // --- Mata mengikuti kursor (~20 fps, hanya set kalau berubah) ---
    let eye_timer = Timer::default();
    {
        let weak = win.as_weak();
        let last = Rc::new(Cell::new((0i32, 0i32)));
        eye_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            let Some(w) = weak.upgrade() else { return };
            let Some((cx, cy)) = platform::cursor_pos() else { return };
            if last.get() == (cx, cy) { return; }
            last.set((cx, cy));
            let mid_x = platform::screen_width() / 2;
            let dx = ((cx - mid_x) as f32 / 400.0).clamp(-1.0, 1.0);
            let dy = (cy as f32 / 300.0).clamp(0.0, 1.0) * 2.0 - 1.0;
            w.set_eye_dx(dx);
            w.set_eye_dy(dy);
        });
    }

    // --- Kedip tiap ~4 detik ---
    let blink_timer = Timer::default();
    {
        let weak = win.as_weak();
        let unblink = Rc::new(Timer::default());
        blink_timer.start(TimerMode::Repeated, Duration::from_millis(4000), move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_blink(true);
            let weak2 = weak.clone();
            unblink.start(TimerMode::SingleShot, Duration::from_millis(120), move || {
                if let Some(w) = weak2.upgrade() { w.set_blink(false); }
            });
        });
    }

    win.run()
}
