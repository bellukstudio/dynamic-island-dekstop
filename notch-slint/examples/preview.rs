//! Render UI ke PNG tanpa jendela (renderer software, tanpa GPU/OS windowing).
//! Berguna untuk memeriksa tata letak dan tema di mesin tanpa layar/Windows.
//!
//!   cargo run --example preview -- <folder-keluaran>
//!
//! Menghasilkan satu PNG per halaman/tema. Data contoh diisi manual (bukan dari sistem).

use std::rc::Rc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use notch_core::config::Appearance;
use notch_core::decor;
use notch_core::layout::Dock;
use notch_core::theme::{Palette, Rgba};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{Global, Image, ModelRc, PhysicalSize, PlatformError, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

slint::include_modules!();

static NOW_MS: AtomicU64 = AtomicU64::new(0);
// Faktor DPI untuk render (env PREVIEW_SF, mis. 1.25 atau 1.5), supaya masalah garis di skala pecahan bisa direproduksi.
static SF_BITS: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0

fn sf() -> f32 {
    f32::from_bits(SF_BITS.load(Ordering::SeqCst))
}

struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> Duration {
        Duration::from_millis(NOW_MS.load(Ordering::SeqCst))
    }
}

fn col(c: Rgba) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn apply_theme(w: &NotchWindow, p: &Palette) {
    let t = Theme::get(w);
    t.set_body(col(p.body));
    t.set_text(col(p.text));
    t.set_text_dim(col(p.text_dim));
    t.set_text_faint(col(p.text_faint));
    t.set_accent(col(p.accent));
    t.set_on_accent(col(p.on_accent));
    t.set_ok(col(p.ok));
    t.set_chip(col(p.chip));
    t.set_chip_hover(col(p.chip_hover));
    t.set_track(col(p.track));
}

/// Sama dengan `Shell::rebuild_decor` di main.rs (tanpa faktor DPI).
fn set_decor(win: &NotchWindow, theme: &str, body: Rgba, dock: Dock, scale: f32) {
    let f = sf();
    let (w, h) = ((580.0 * f).round() as u32, (202.0 * f).round() as u32);
    let r = (if dock == Dock::Floating { 28.0 } else { 32.0 }) * scale * f;
    match decor::render(theme, body, dock, w, h, r, f) {
        Some(buf) => {
            win.set_decor(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, w, h)));
            win.set_has_decor(true);
        }
        None => win.set_has_decor(false),
    }
}

fn snap(window: &MinimalSoftwareWindow, size: (u32, u32), path: &str) {
    // majukan waktu supaya animasi (lebar/tinggi/opacity) selesai
    NOW_MS.fetch_add(1500, Ordering::SeqCst);
    slint::platform::update_timers_and_animations();
    let size = ((size.0 as f32 * sf()).round() as u32, (size.1 as f32 * sf()).round() as u32);
    window.set_size(PhysicalSize::new(size.0, size.1));
    let (w, h) = (size.0 as usize, size.1 as usize);
    // Target ber-alpha supaya area transparan tetap transparan; dikomposit ke latar abu-abu "desktop".
    let mut buf = vec![PremultipliedRgbaColor { red: 0, green: 0, blue: 0, alpha: 0 }; w * h];
    window.draw_if_needed(|r| {
        r.render(&mut buf, w);
    });
    let bg = [70u32, 74, 86];
    let mut bytes = Vec::with_capacity(w * h * 4);
    for p in &buf {
        let inv = 255 - u32::from(p.alpha);
        let mix = |c: u8, b: u32| (u32::from(c) + b * inv / 255).min(255) as u8;
        bytes.extend_from_slice(&[mix(p.red, bg[0]), mix(p.green, bg[1]), mix(p.blue, bg[2]), 255]);
    }
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, size.0, size.1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&bytes).unwrap();
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "preview-out".into());
    std::fs::create_dir_all(&out).unwrap();

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let win = NotchWindow::new().unwrap();
    if let Some(f) = std::env::var("PREVIEW_SF").ok().and_then(|v| v.parse::<f32>().ok()) {
        SF_BITS.store(f.to_bits(), Ordering::SeqCst);
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: f });
    }

    let pages = [
        "media", "clock", "weather", "battery", "timer", "quick", "tools", "stats",
        "reminder", "notes", "clipboard", "shelf", "privacy", "prayer", "uji",
    ];
    win.set_tabs(ModelRc::from(Rc::new(VecModel::from(
        pages.iter().map(|p| SharedString::from(*p)).collect::<Vec<_>>(),
    ))));

    // data contoh
    win.set_clock_text("14:28".into());
    win.set_date_text("14/11/2026".into());
    win.set_pomo_text("25:00".into());
    win.set_pomo_label("Fokus · ronde 1/4".into());
    win.set_pomo_progress(0.35);
    win.set_cpu(34.0);
    win.set_ram(54.0);
    win.set_disk(61.0);
    win.set_cpu_text("34%".into());
    win.set_ram_text("8.6 / 15.9 GiB".into());
    win.set_disk_text("290 / 476 GiB".into());
    win.set_net_text("\u{2193} 120 KB/s   \u{2191} 14 KB/s".into());
    win.set_sys_text("16 threads \u{00B7} 312 proses".into());
    win.set_battery_percent(95);
    win.set_battery_charging(true);
    win.set_q_mic(0);
    win.set_q_wifi(1);
    win.set_q_bt(0);
    win.set_q_awake(0);
    win.set_q_dark(1);
    win.set_notes(ModelRc::from(Rc::new(VecModel::from(vec![
        NoteItem { id: "1".into(), text: "q*UYmN2!Di-KoB2C".into() },
        NoteItem { id: "2".into(), text: "API DOC RAMCES contoh catatan yang cukup panjang untuk dipotong".into() },
    ]))));

    let themes = ["default", "neon", "aurora", "obsidian", "sakura", "matcha"];
    let expanded = (612, 240);

    for theme in themes {
        let mut a = Appearance::default();
        a.notch_theme = theme.into();
        let pal = Palette::from_appearance(&a, false);
        apply_theme(&win, &pal);
        set_decor(&win, theme, pal.body, Dock::Top, 1.0);

        // menciut
        win.set_expanded(false);
        snap(&window, (152, 38), &format!("{out}/{theme}_collapsed.png"));

        win.set_expanded(true);
        let list: Vec<&str> = if theme == "default" {
            pages.to_vec()
        } else {
            vec!["media", "timer", "quick"]
        };
        for page in list {
            win.set_page_id(page.into());
            snap(&window, expanded, &format!("{out}/{theme}_{page}.png"));
        }
    }

    // Mode tepi: menempel atas / melayang / menempel bawah (tema neon, skala 1.0 seperti config referensi)
    let mut a = Appearance::default();
    a.notch_theme = "neon".into();
    let pal = Palette::from_appearance(&a, false);
    apply_theme(&win, &pal);
    win.set_ui_scale(1.0);
    for (name, dock, d) in [("top", 0, Dock::Top), ("floating", 1, Dock::Floating), ("bottom", 2, Dock::Bottom)] {
        win.set_dock(dock);
        set_decor(&win, "neon", pal.body, d, 1.0);
        win.set_expanded(false);
        snap(&window, (152, 38), &format!("{out}/dock_{name}_collapsed.png"));
        win.set_expanded(true);
        win.set_page_id("media".into());
        snap(&window, expanded, &format!("{out}/dock_{name}_media.png"));
    }

    // Panel lirik di dalam Media: keadaan kosong, lalu contoh baris (HANYA untuk memeriksa tata letak)
    {
        let pal = Palette::from_appearance(&Appearance::default(), false);
        apply_theme(&win, &pal);
        win.set_dock(0);
        set_decor(&win, "default", pal.body, Dock::Top, 1.0);
        win.set_expanded(true);
        win.set_page_id("media".into());
        win.set_lyrics_open(true);
        snap(&window, expanded, &format!("{out}/lyrics_empty.png"));
        let sample = ["baris contoh sebelumnya", "baris contoh yang sedang aktif", "baris contoh berikutnya"];
        win.set_lyric_lines(ModelRc::from(Rc::new(VecModel::from(sample.iter().map(|x| SharedString::from(*x)).collect::<Vec<_>>()))));
        win.set_lyric_active(1);
        snap(&window, expanded, &format!("{out}/lyrics_sample.png"));
        win.set_lyrics_open(false);
        win.set_lyric_lines(ModelRc::default());
    }

    // Kalender: data nyata dari notch_core::calendar, "hari ini" dikunci 15 Nov 2026 agar hasil stabil.
    {
        use notch_core::calendar::{self, Date, Holidays, View};
        let pal = Palette::from_appearance(&Appearance::default(), false);
        apply_theme(&win, &pal);
        win.set_dock(0);
        set_decor(&win, "default", pal.body, Dock::Top, 1.0);
        win.set_expanded(true);
        win.set_page_id("clock".into());
        win.set_cal_weekdays(ModelRc::from(Rc::new(VecModel::from(
            calendar::weekday_headers().iter().map(|x| SharedString::from(*x)).collect::<Vec<_>>(),
        ))));
        let today = Date::new(2026, 11, 15);
        let hol = Holidays::new();
        for (name, y, m, pick) in [("nov2026", 2026, 11, None), ("agu2026", 2026, 8, Some(Date::new(2026, 8, 17))), ("mar2026", 2026, 3, Some(Date::new(2026, 3, 20)))] {
            let mut v = View { year: y, month: m, selected: None };
            v.pick_default(today, &hol);
            if pick.is_some() {
                v.selected = pick;
            }
            let cells: Vec<CalCell> = calendar::cells_for(y, m)
                .iter()
                .map(|c| CalCell {
                    day: c.date.d as i32,
                    in_month: c.in_month,
                    sun: c.date.weekday() == 0,
                    today: c.date == today,
                    sel: v.selected == Some(c.date),
                    kind: hol.events_on(c.date).first().map_or(0, |e| e.kind.code()),
                })
                .collect();
            win.set_cal_cells(ModelRc::from(Rc::new(VecModel::from(cells))));
            win.set_cal_title(calendar::title(y, m).into());
            let (d, n, k) = v.footer(&hol);
            win.set_cal_foot_date(d.into());
            win.set_cal_foot_name(n.into());
            win.set_cal_foot_kind(k);
            win.set_cal_open(true);
            snap(&window, expanded, &format!("{out}/calendar_{name}.png"));
        }
        win.set_cal_open(false);
        snap(&window, expanded, &format!("{out}/clock_with_hint.png"));
    }
    println!("selesai: {out}");
}
