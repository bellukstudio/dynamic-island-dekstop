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
    let d = NoteDetailWindow::new().unwrap();
    d.set_meta("17 Nov 2026, 14.28".into());
    d.set_text("API DOC RAMCES\nContoh catatan yang panjang dengan beberapa baris.\nBaris ketiga untuk uji hitung baris.".into());
    d.set_chars("108 karakter".into());
    d.set_lines("3 baris".into());
    d.set_dirty(true);
    d.set_can_save(true);
    d.set_toast("\u{2713} Disimpan".into());
    snap(&window, (680, 520), &format!("{out}/note_detail.png"));
    d.set_confirm_delete(true);
    d.set_dirty(false);
    d.set_can_save(false);
    d.set_toast("".into());
    snap(&window, (420, 320), &format!("{out}/note_detail_small.png"));
}
