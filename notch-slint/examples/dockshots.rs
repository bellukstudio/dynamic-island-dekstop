//! Render dock (atas/bawah/kiri/kanan) ke PNG pada ukuran jendela sebenarnya.
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
    let out = std::env::args().nth(1).unwrap_or_else(|| "dock-out".into());
    std::fs::create_dir_all(&out).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let win = NotchWindow::new().unwrap();
    if let Some(f) = std::env::var("PREVIEW_SF").ok().and_then(|v| v.parse::<f32>().ok()) {
        SF_BITS.store(f.to_bits(), Ordering::SeqCst);
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: f });
    }
    let pages = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats",
        "reminder", "notes", "clipboard", "shelf", "privacy", "prayer"];
    win.set_tabs(ModelRc::from(Rc::new(VecModel::from(pages.iter().map(|p| SharedString::from(*p)).collect::<Vec<_>>()))));
    win.set_clock_text("14:28".into());
    win.set_sec_text("07".into());
    win.set_day_text("Selasa".into());
    win.set_date_text("17 November 2026".into());
    win.set_media_playing(true);
    win.set_media_progress(0.42);
    win.set_media_pos_text("1:24".into());
    win.set_media_dur_text("3:21".into());
    win.set_pomo_text("25:00".into());
    win.set_pomo_label("Fokus · ronde 1/4".into());
    win.set_pomo_progress(0.35);
    win.set_battery_percent(95);
    win.set_battery_charging(true);
    win.set_q_mic(0);
    win.set_q_wifi(1);
    win.set_q_bt(0);
    win.set_q_awake(0);
    win.set_q_dark(1);
    win.set_weather_temp("27°".into());
    win.set_weather_condition("Berawan sebagian".into());
    win.set_weather_feels("Terasa 29°".into());
    win.set_weather_humidity("Kelembapan 78%".into());
    win.set_weather_rain("Hujan 40%".into());
    win.set_weather_icon("M 12 2 V 4 M 12 20 V 22 M 4 12 H 2 M 22 12 H 20 M 12 8 A 4 4 0 1 0 12 16 A 4 4 0 1 0 12 8 Z".into());
    win.set_notes(ModelRc::from(Rc::new(VecModel::from(vec![
        NoteItem { id: "1".into(), text: "API DOC RAMCES".into(), preview: "API DOC RAMCES contoh catatan yang cukup panjang".into(), meta: "Baru saja · 62 kar".into() },
        NoteItem { id: "2".into(), text: "Belanja".into(), preview: "Belanja mingguan".into(), meta: "3 jam lalu · 16 kar".into() },
        NoteItem { id: "3".into(), text: "Ide".into(), preview: "Ide fitur baru".into(), meta: "2 hari lalu · 14 kar".into() },
    ]))));
    win.set_currency(CurrencyView {
        amount: "100.000".into(), code: "USD".into(), name: "Dolar AS".into(), result: "6,06 USD".into(),
        rate: "1 USD = Rp 16.500".into(), updated: "Diperbarui 14:28".into(), stock_period: "MTD · 1 Nov".into(),
        stock_status: "Pilihan saham IDX · data Yahoo Finance, bukan seluruh bursa".into(), error: "".into(), loading: false,
    });
    win.set_currency_amount("100.000".into());
    win.set_stock_gainers(ModelRc::from(Rc::new(VecModel::from(vec![
        StockRow { name: "ANTM".into(), price: "Rp 3.120".into(), change: "+8,4%".into(), positive: true },
        StockRow { name: "BBCA".into(), price: "Rp 9.125".into(), change: "+3,1%".into(), positive: true },
        StockRow { name: "TLKM".into(), price: "Rp 3.540".into(), change: "+1,2%".into(), positive: true },
    ]))));
    win.set_stock_losers(ModelRc::from(Rc::new(VecModel::from(vec![
        StockRow { name: "GOTO".into(), price: "Rp 62".into(), change: "-6,0%".into(), positive: false },
        StockRow { name: "MDKA".into(), price: "Rp 2.050".into(), change: "-3,3%".into(), positive: false },
        StockRow { name: "BRPT".into(), price: "Rp 1.020".into(), change: "-0,9%".into(), positive: false },
    ]))));
    win.set_stats(StatsView {
        host: "DESKTOP-NOTCH".into(), cpu: 34.0, ram: 54.0, swap: 12.0, disk: 91.0,
        cpu_v: "34%".into(), ram_v: "54%".into(), swap_v: "12%".into(), disk_v: "91%".into(),
        cpu_meta: "Ryzen 7 5700U · 16 thread · 1.8 GHz".into(), ram_meta: "8.6 / 15.9 GiB digunakan".into(),
        swap_meta: "1.2 / 9.9 GiB digunakan".into(), disk_meta: "290 / 476 GiB · C:".into(),
        down: "↓ 120 KB/s".into(), up: "↑ 14 KB/s".into(), processes: "312 proses".into(),
        uptime: "Aktif 5j 12m".into(), os: "Windows 11".into(), kernel: "26100".into(),
    });
    win.set_media_title("Judul Lagu".into());
    win.set_media_artist("Nama Artis".into());
    let a = Appearance::default();
    let pal = Palette::from_appearance(&a, false);
    apply_theme(&win, &pal);
    win.set_date_short("17 Nov".into());
    win.set_idle_clock(true);
    win.set_dock(0);
    set_decor(&win, "default", pal.body, Dock::Top, 1.0);
    win.set_expanded(false);
    snap(&window, (300, 140), &format!("{out}/idle_clock.png"));
    win.set_idle_clock(false);
    win.set_dock(0);
    set_decor(&win, "default", pal.body, Dock::Top, 1.0);
    win.set_expanded(true);
    win.set_page_id("notes".into());
    win.set_notes_expanded(true);
    snap(&window, (612, 338), &format!("{out}/notes_expanded.png"));
    win.set_notes_expanded(false);
    win.set_expanded(false);
    {
        let it=|n:&str,t:&str,st:i32| PrayerItem{name:n.into(),time:t.into(),state:st};
        win.set_prayer_items(ModelRc::from(Rc::new(VecModel::from(vec![it("Subuh","04:34",2),it("Dzuhur","11:56",2),it("Ashar","15:12",1),it("Maghrib","17:58",0),it("Isya","19:08",0)]))));
        win.set_prayer_imsakiyah(ModelRc::from(Rc::new(VecModel::from(vec![it("Imsak","04:24",0),it("Subuh","04:34",0),it("Terbit","05:50",0),it("Dzuhur","11:56",0),it("Ashar","15:12",0),it("Maghrib","17:58",0),it("Isya","19:08",0)]))));
        win.set_prayer_next_name("Ashar".into());
        win.set_prayer_next_time("15:12".into());
        win.set_prayer_when("Hari ini".into());
        win.set_prayer_countdown("00:42:17".into());
        win.set_prayer_meta("Imsak 04:24 · Terbit 05:50".into());
        win.set_prayer_city("Kota Bandung".into());
        win.set_prayer_mode("adzan".into());
        win.set_prayer_remind_before("true".into());
        win.set_prayer_remind_min("10".into());
        win.set_dhikr_arabic("أَسْتَغْفِرُ اللّٰهَ".into());
        win.set_dhikr_latin("Astaghfirullah".into());
        win.set_dhikr_meaning("Aku memohon ampun kepada Allah. Dibaca 3x".into());
        win.set_ramadan_date("Selasa, 17 November 2026".into());
        win.set_ramadan_imsak_countdown("08:21:09".into());
        win.set_ramadan_buka_countdown("03:12:40".into());
        win.set_ramadan_imsak_time("04:24".into());
        win.set_ramadan_buka_time("17:58".into());
        win.set_dock(0);
        set_decor(&win, "default", pal.body, Dock::Top, 1.0);
        win.set_expanded(true);
        win.set_page_id("prayer".into());
        win.set_ramadan_enabled(false);
        snap(&window, (612, 240), &format!("{out}/prayer_normal.png"));
        win.set_ramadan_enabled(true);
        snap(&window, (812, 578), &format!("{out}/prayer_ramadan.png"));
        win.set_ramadan_enabled(false);
        win.set_expanded(false);
    }
    // Semua halaman hub utama (dock atas) untuk dibandingkan dengan Tauri
    {
        win.set_dock(0);
        set_decor(&win, "default", pal.body, Dock::Top, 1.0);
        win.set_expanded(true);
        for page in ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "notes", "currency"] {
            win.set_page_id(page.into());
            snap(&window, (612, 240), &format!("{out}/page_{page}.png"));
        }
    }
    for (name, dock, d, collapsed) in [
        ("top", 0, Dock::Top, (300u32, 140u32)),
        ("bottom", 2, Dock::Bottom, (300, 140)),
        ("left", 3, Dock::Left, (300, 262)),
        ("right", 4, Dock::Right, (300, 262)),
    ] {
        win.set_dock(dock);
        set_decor(&win, "default", pal.body, d, 1.0);
        win.set_expanded(false);
        snap(&window, collapsed, &format!("{out}/{name}_collapsed.png"));
        win.set_expanded(true);
        win.set_page_id("media".into());
        snap(&window, (612, 240), &format!("{out}/{name}_hub.png"));
    }
}
