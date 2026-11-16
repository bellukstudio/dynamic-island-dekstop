//! Mata notch: 8 mood x 20 skin, dirender ke gambar RGBA oleh Rust.
//!
//! Meniru `src/css/eyes.css`, `eye-themes.css`, `notch-face.ts`, dan `mood.ts` dari versi Tauri.
//! Digambar sendiri (bukan lewat UI Slint) karena renderer software Slint tidak mendukung
//! bayangan/glow dan gradien berlapis. Pola yang sama dengan `decor.rs`.
//!
//! Alur: `Face::for_mood` (bentuk + warna target) -> `Morph` (transisi 250 ms) ->
//! `EyeRenderer::render` (piksel). `MoodState` mengurus mood dasar, tidur saat idle, dan
//! reaksi klik, tanpa UI.

use std::time::{Duration, Instant};

use crate::eyes_data::SKINS;

/// Jarak antar mata (`gap: 9px` di `.eyes`).
pub const GAP: f32 = 9.0;

// ---------------------------------------------------------------- data skin

#[derive(Clone, Copy, Debug)]
pub enum C {
    /// Warna skin mood saat ini, dengan alpha.
    Skin(f32),
    Rgb(u8, u8, u8, f32),
}

#[derive(Clone, Copy, Debug)]
pub struct Sh {
    pub inset: bool,
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: C,
}

#[derive(Clone, Copy, Debug)]
pub struct Outline {
    pub w: f32,
    pub color: C,
}

#[derive(Clone, Copy, Debug)]
pub struct PalEntry {
    pub rgb: [u8; 3],
    pub glow_alpha: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct SkinDef {
    pub name: &'static str,
    pub glow_extra: f32,
    pub glow_alpha: f32,
    pub glow_soft: f32,
    pub highlight: f32,
    pub shadows: &'static [Sh],
    pub outline: Option<Outline>,
    pub second: [u8; 3],
    pub pal: [PalEntry; 8],
}

/// Indeks skin dari nama; nama tak dikenal jatuh ke `aurora` (sama dengan `Config::sanitize`).
pub fn skin_index(name: &str) -> usize {
    SKINS.iter().position(|s| s.name == name).unwrap_or(0)
}

pub fn skin_names() -> impl Iterator<Item = &'static str> {
    SKINS.iter().map(|s| s.name)
}

// ---------------------------------------------------------------- mood

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Normal,
    Happy,
    Sleepy,
    Surprised,
    Angry,
    Love,
    Suspicious,
    Sad,
}

impl Mood {
    pub const ALL: [Mood; 8] = [
        Mood::Normal,
        Mood::Happy,
        Mood::Sleepy,
        Mood::Surprised,
        Mood::Angry,
        Mood::Love,
        Mood::Suspicious,
        Mood::Sad,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mood::Normal => "normal",
            Mood::Happy => "happy",
            Mood::Sleepy => "sleepy",
            Mood::Surprised => "surprised",
            Mood::Angry => "angry",
            Mood::Love => "love",
            Mood::Suspicious => "suspicious",
            Mood::Sad => "sad",
        }
    }

    pub fn parse(s: &str) -> Option<Mood> {
        Mood::ALL.into_iter().find(|m| m.name() == s)
    }

    fn idx(self) -> usize {
        self as usize
    }

    /// `--c` di eyes.css/nudge.css: dipakai "z" dan tetesan air mata (bukan badan mata).
    fn base_rgb(self) -> [f32; 3] {
        let c = match self {
            Mood::Normal => [120, 220, 255],
            Mood::Happy => [130, 255, 190],
            Mood::Sleepy => [150, 160, 200],
            Mood::Surprised => [255, 240, 140],
            Mood::Angry => [255, 90, 90],
            Mood::Love => [255, 105, 180],
            Mood::Suspicious => [255, 190, 90],
            Mood::Sad => [140, 170, 255],
        };
        [c[0] as f32, c[1] as f32, c[2] as f32]
    }
}

// ---------------------------------------------------------------- bentuk

fn mix(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

/// Satu mata: ukuran, radius sudut (rx, ry per sudut: kiri-atas, kanan-atas, kanan-bawah, kiri-bawah),
/// kelopak atas/bawah, kecerahan, kilau, dan warna.
#[derive(Clone, Copy, Debug)]
pub struct EyeP {
    w: f32,
    h: f32,
    rad: [(f32, f32); 4],
    lid_t: f32,
    lid_rot: f32,
    lid_dy: f32,
    lid_b: f32,
    lid_b_round: f32,
    bright: f32,
    shine_w: f32,
    shine_h: f32,
    rgb: [f32; 3],
    glow_a: f32,
}

impl EyeP {
    fn lerp(a: &EyeP, b: &EyeP, k: f32) -> EyeP {
        let mut rad = [(0.0, 0.0); 4];
        for (i, r) in rad.iter_mut().enumerate() {
            *r = (mix(a.rad[i].0, b.rad[i].0, k), mix(a.rad[i].1, b.rad[i].1, k));
        }
        EyeP {
            w: mix(a.w, b.w, k),
            h: mix(a.h, b.h, k),
            rad,
            lid_t: mix(a.lid_t, b.lid_t, k),
            lid_rot: mix(a.lid_rot, b.lid_rot, k),
            lid_dy: mix(a.lid_dy, b.lid_dy, k),
            lid_b: mix(a.lid_b, b.lid_b, k),
            lid_b_round: mix(a.lid_b_round, b.lid_b_round, k),
            bright: mix(a.bright, b.bright, k),
            shine_w: mix(a.shine_w, b.shine_w, k),
            shine_h: mix(a.shine_h, b.shine_h, k),
            rgb: [mix(a.rgb[0], b.rgb[0], k), mix(a.rgb[1], b.rgb[1], k), mix(a.rgb[2], b.rgb[2], k)],
            glow_a: mix(a.glow_a, b.glow_a, k),
        }
    }
}

/// `border-radius` berbentuk persen: rx = persen lebar, ry = persen tinggi.
fn pct(w: f32, h: f32, rx: [f32; 4], ry: [f32; 4]) -> [(f32, f32); 4] {
    [
        (rx[0] * w / 100.0, ry[0] * h / 100.0),
        (rx[1] * w / 100.0, ry[1] * h / 100.0),
        (rx[2] * w / 100.0, ry[2] * h / 100.0),
        (rx[3] * w / 100.0, ry[3] * h / 100.0),
    ]
}

fn mood_eye(m: Mood, right: bool, skin: &SkinDef) -> EyeP {
    let pal = skin.pal[m.idx()];
    let mut e = EyeP {
        w: 8.0,
        h: 13.0,
        rad: [(5.0, 5.0); 4],
        lid_t: 0.0,
        lid_rot: 0.0,
        lid_dy: 0.0,
        lid_b: 0.0,
        lid_b_round: 0.0,
        bright: 1.0,
        shine_w: 3.0,
        shine_h: 3.0,
        rgb: [pal.rgb[0] as f32, pal.rgb[1] as f32, pal.rgb[2] as f32],
        glow_a: pal.glow_alpha.unwrap_or(skin.glow_alpha),
    };
    let half = |e: &EyeP| (e.w / 2.0, e.h / 2.0);
    match m {
        Mood::Normal => {}
        Mood::Happy => {
            e.h = 10.0;
            let (rx, ry) = half(&e);
            e.rad = [(rx, ry), (rx, ry), (0.0, 0.0), (0.0, 0.0)];
            e.lid_b = 6.0;
            e.lid_b_round = 1.0;
        }
        Mood::Sleepy => {
            e.lid_t = 9.0;
            e.bright = 0.7;
        }
        Mood::Surprised => {
            e.w = 10.0;
            e.h = 17.0;
            let (rx, ry) = half(&e);
            e.rad = [(rx, ry); 4];
            e.shine_w = 4.0;
            e.shine_h = 4.0;
        }
        Mood::Angry => {
            e.h = 12.0;
            e.lid_t = 9.0;
            e.lid_rot = if right { -20.0 } else { 20.0 };
            e.lid_dy = -3.0;
        }
        Mood::Love => {
            e.w = 10.0;
            e.h = 19.0;
            let (rx, ry) = half(&e);
            e.rad = [(rx, ry); 4];
        }
        Mood::Suspicious => {
            e.lid_t = if right { 11.0 } else { 8.0 };
            e.lid_b = 3.0;
        }
        Mood::Sad => {
            // nudge.css (dimuat belakangan) menimpa eyes.css: tinggi 12, kelopak miring 16 derajat
            e.h = 12.0;
            e.bright = 0.9;
            e.lid_t = 6.0;
            e.lid_rot = if right { 16.0 } else { -16.0 };
            e.lid_dy = -1.0;
        }
    }
    // Bentuk khas skin. pixel & sakura berlaku untuk semua mood; skin baru hanya untuk mood normal
    // (aturan CSS: supaya bentuk mood lain tetap utuh).
    let (w, h) = (e.w, e.h);
    match skin.name {
        "pixel" => e.rad = [(1.0, 1.0); 4],
        "sakura" => e.rad = pct(w, h, [48.0, 52.0, 44.0, 56.0], [48.0, 52.0, 44.0, 56.0]),
        _ if m == Mood::Normal => match skin.name {
            "kawaii" | "boba" | "candy" => e.rad = pct(w, h, [50.0; 4], [50.0; 4]),
            "matcha" => e.rad = pct(w, h, [42.0; 4], [42.0; 4]),
            "synthwave" => e.rad = [(3.0, 3.0); 4],
            "slime" => e.rad = pct(w, h, [55.0, 45.0, 50.0, 50.0], [62.0, 62.0, 38.0, 38.0]),
            "cotton" => e.rad = pct(w, h, [46.0, 54.0, 48.0, 52.0], [46.0, 54.0, 48.0, 52.0]),
            "pumpkin" => e.rad = pct(w, h, [52.0, 52.0, 34.0, 34.0], [78.0, 78.0, 22.0, 22.0]),
            "frost" => e.rad = pct(w, h, [38.0; 4], [38.0; 4]),
            _ => {}
        },
        _ => {}
    }
    match skin.name {
        "pixel" => {
            e.shine_w = 0.0;
            e.shine_h = 0.0;
        }
        "kawaii" => {
            e.shine_w = 3.6;
            e.shine_h = 3.6;
        }
        "slime" => {
            e.shine_w = 4.0;
            e.shine_h = 2.4;
        }
        _ => {}
    }
    e
}

/// Wajah lengkap (dua mata + efek) untuk satu mood. Bisa di-interpolasi untuk transisi.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    left: EyeP,
    right: EyeP,
    /// Opasitas "z" tidur dan tetes air mata (0..1).
    z: f32,
    tear: f32,
    fx_rgb: [f32; 3],
}

impl Face {
    pub fn for_mood(m: Mood, skin: usize) -> Face {
        let s = &SKINS[skin.min(SKINS.len() - 1)];
        Face {
            left: mood_eye(m, false, s),
            right: mood_eye(m, true, s),
            z: if m == Mood::Sleepy { 1.0 } else { 0.0 },
            tear: if m == Mood::Sad { 1.0 } else { 0.0 },
            fx_rgb: m.base_rgb(),
        }
    }

    pub fn lerp(a: &Face, b: &Face, k: f32) -> Face {
        Face {
            left: EyeP::lerp(&a.left, &b.left, k),
            right: EyeP::lerp(&a.right, &b.right, k),
            z: mix(a.z, b.z, k),
            tear: mix(a.tear, b.tear, k),
            fx_rgb: [
                mix(a.fx_rgb[0], b.fx_rgb[0], k),
                mix(a.fx_rgb[1], b.fx_rgb[1], k),
                mix(a.fx_rgb[2], b.fx_rgb[2], k),
            ],
        }
    }
}

/// Transisi antar mood (transition .25s di CSS). `t` dalam detik pada jam yang sama dengan pemanggil.
pub struct Morph {
    from: Face,
    to: Face,
    start: f32,
}

const MORPH_S: f32 = 0.25;

impl Morph {
    pub fn new(face: Face) -> Morph {
        Morph { from: face, to: face, start: -10.0 }
    }

    pub fn set_target(&mut self, now: f32, target: Face) {
        self.from = self.current(now);
        self.to = target;
        self.start = now;
    }

    pub fn current(&self, now: f32) -> Face {
        let k = ((now - self.start) / MORPH_S).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k); // smoothstep
        Face::lerp(&self.from, &self.to, k)
    }

    pub fn settled(&self, now: f32) -> bool {
        now - self.start >= MORPH_S
    }
}

// ---------------------------------------------------------------- tatapan

/// Offset mata dari arah kursor (px logis). `dx`/`dy` = kursor relatif terhadap tengah notch.
/// Sama dengan `ctx.look` di notch-face.ts: maksimal 6 px, dibulatkan 0,25 px.
pub fn look_offset(dx: f32, dy: f32) -> (f32, f32) {
    let max = 6.0;
    let dist = dx.hypot(dy).max(1e-6);
    let k = (dist / 300.0).min(1.0);
    let lx = ((dx / dist) * max * k * 4.0).round() / 4.0;
    let ly = (((dy / dist) * max * k).clamp(-1.0, 1.0) * 4.0).round() / 4.0;
    (lx, ly)
}

// ---------------------------------------------------------------- isian (gradien) per skin

#[derive(Clone, Copy, Debug)]
enum Col {
    Skin(f32),
    Second(f32),
    Rgb(u8, u8, u8, f32),
    Clear,
}

#[derive(Clone, Debug)]
enum Layer {
    /// `radial-gradient(rx ry at cx cy, color, transparent stop)`; semua pecahan lebar/tinggi.
    Radial { cx: f32, cy: f32, rx: f32, ry: f32, stop: f32, col: Col },
    Linear { deg: f32, stops: Vec<(f32, Col)> },
    Stars(&'static [(f32, f32, f32)]),
    Scan,
}

const W: fn(f32) -> Col = |a| Col::Rgb(255, 255, 255, a);

fn radial(cx: f32, cy: f32, rx: f32, ry: f32, stop: f32, col: Col) -> Layer {
    Layer::Radial { cx, cy, rx, ry, stop, col }
}

fn linear(deg: f32, stops: &[(f32, Col)]) -> Layer {
    Layer::Linear { deg, stops: stops.to_vec() }
}

/// Lapisan isian dari atas ke bawah, di atas `background-color` skin.
fn fill_layers(s: &SkinDef) -> Vec<Layer> {
    let hl = s.highlight;
    match s.name {
        "holo" => vec![
            linear(135.0, &[(0.0, W(0.32)), (0.42, Col::Clear)]),
            linear(160.0, &[(0.0, Col::Skin(0.88)), (1.0, Col::Skin(0.58))]),
        ],
        "ember" => vec![
            radial(0.25, 0.15, 1.2, 1.0, 0.40, Col::Rgb(255, 245, 205, 0.76)),
            linear(155.0, &[(0.0, Col::Skin(0.98)), (1.0, Col::Rgb(120, 24, 24, 0.76))]),
        ],
        "ocean" => vec![
            radial(0.27, 0.15, 1.0, 0.85, 0.44, Col::Rgb(235, 255, 255, 0.76)),
            linear(160.0, &[(0.0, Col::Skin(0.92)), (1.0, Col::Rgb(10, 58, 130, 0.72))]),
        ],
        "sakura" => vec![
            radial(0.28, 0.15, 1.0, 0.90, 0.48, W(0.9)),
            linear(155.0, &[(0.0, Col::Skin(0.92)), (1.0, Col::Rgb(171, 84, 143, 0.58))]),
        ],
        "cyber" => {
            let m = Col::Rgb(255, 0, 210, 0.16);
            vec![
                linear(
                    115.0,
                    &[(0.0, W(0.42)), (0.10, W(0.42)), (0.10, Col::Clear), (0.42, Col::Clear), (0.42, m), (0.48, m), (0.48, Col::Clear)],
                ),
                linear(160.0, &[(0.0, Col::Skin(0.98)), (1.0, Col::Rgb(45, 34, 140, 0.8))]),
            ]
        }
        "monochrome" => vec![
            radial(0.26, 0.14, 1.0, 1.0, 0.46, W(0.88)),
            linear(160.0, &[(0.0, Col::Skin(0.88)), (1.0, Col::Rgb(92, 103, 126, 0.72))]),
        ],
        "kawaii" => vec![
            radial(0.30, 0.18, 0.70, 0.60, 0.50, W(0.98)),
            radial(0.74, 0.82, 0.40, 0.35, 0.60, W(0.55)),
            linear(160.0, &[(0.0, Col::Skin(0.95)), (1.0, Col::Second(1.0))]),
        ],
        "boba" => vec![
            radial(0.30, 0.22, 0.45, 0.38, 0.62, W(0.85)),
            radial(0.50, 1.05, 0.80, 0.80, 0.60, Col::Skin(0.55)),
            linear(165.0, &[(0.0, Col::Second(1.0)), (1.0, Col::Rgb(24, 12, 8, 1.0))]),
        ],
        "galaxy" => vec![
            Layer::Stars(&[(0.28, 0.30, 1.2), (0.68, 0.22, 1.0), (0.60, 0.62, 1.3), (0.30, 0.78, 0.9)]),
            radial(0.25, 0.12, 0.90, 0.70, 0.62, Col::Skin(0.9)),
            linear(170.0, &[(0.0, Col::Skin(0.7)), (1.0, Col::Second(1.0))]),
        ],
        "matcha" => vec![
            radial(0.28, 0.16, 0.90, 0.80, 0.50, Col::Rgb(255, 255, 235, 0.85)),
            linear(160.0, &[(0.0, Col::Skin(0.96)), (1.0, Col::Second(0.8))]),
        ],
        "candy" => vec![
            radial(0.28, 0.16, 0.60, 0.50, 0.55, W(0.95)),
            linear(
                155.0,
                &[(0.0, Col::Rgb(255, 120, 190, 1.0)), (0.48, Col::Rgb(255, 222, 110, 1.0)), (1.0, Col::Second(1.0))],
            ),
        ],
        "synthwave" => vec![
            Layer::Scan,
            linear(180.0, &[(0.0, Col::Skin(1.0)), (0.38, Col::Skin(1.0)), (1.0, Col::Second(1.0))]),
        ],
        "slime" => vec![
            radial(0.36, 0.18, 0.55, 0.28, 0.70, W(0.95)),
            radial(0.70, 0.88, 0.30, 0.22, 0.70, W(0.32)),
            linear(165.0, &[(0.0, Col::Skin(1.0)), (1.0, Col::Second(0.85))]),
        ],
        "cotton" => vec![
            radial(0.28, 0.14, 0.90, 0.70, 0.52, W(0.95)),
            linear(160.0, &[(0.0, Col::Skin(1.0)), (1.0, Col::Second(0.82))]),
        ],
        "pumpkin" => vec![
            radial(0.30, 0.14, 0.90, 0.70, 0.48, Col::Rgb(255, 240, 170, 0.8)),
            linear(170.0, &[(0.0, Col::Skin(1.0)), (0.40, Col::Skin(1.0)), (1.3, Col::Second(1.0))]),
        ],
        "frost" => vec![
            linear(
                118.0,
                &[(0.36, Col::Clear), (0.44, W(0.7)), (0.54, W(0.7)), (0.62, Col::Clear)],
            ),
            linear(170.0, &[(0.0, Col::Second(1.0)), (0.55, Col::Skin(1.0)), (1.0, Col::Skin(0.7))]),
        ],
        // aurora, neon, pastel, pixel: isian dasar
        _ => vec![
            radial(0.28, 0.16, 1.10, 1.20, 0.48, W(hl)),
            linear(160.0, &[(0.0, Col::Skin(0.9)), (1.0, Col::Skin(1.0))]),
        ],
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ShineKind {
    Round,
    Rect,
    Diamond,
}

/// Bentuk dan warna kilau per skin (`.shine` di eye-themes.css).
fn shine_style(name: &str) -> (ShineKind, [f32; 4]) {
    match name {
        "boba" => (ShineKind::Round, [255.0, 246.0, 230.0, 0.95]),
        "synthwave" => (ShineKind::Rect, [255.0, 255.0, 255.0, 0.85]),
        "cyber" => (ShineKind::Rect, [255.0, 255.0, 255.0, 0.94]),
        "pumpkin" => (ShineKind::Round, [255.0, 244.0, 190.0, 0.92]),
        "frost" => (ShineKind::Diamond, [255.0, 255.0, 255.0, 1.0]),
        "galaxy" => (ShineKind::Round, [255.0, 255.0, 255.0, 1.0]),
        _ => (ShineKind::Round, [255.0, 255.0, 255.0, 0.94]),
    }
}

// ---------------------------------------------------------------- animasi

/// Keadaan animasi saat satu frame digambar.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    /// Mood yang sedang tampil (menentukan jenis animasi: napas, denyut, muncul).
    pub mood: Mood,
    /// Detik sejak aplikasi mulai (untuk napas, denyut, "z", tetesan).
    pub t: f32,
    /// Detik sejak mood ini mulai (animasi "muncul" saat terkejut).
    pub t_mood: f32,
    /// Offset tatapan dalam px logis (dari `look_offset`).
    pub look: (f32, f32),
    pub blink: bool,
}

fn ease_cos(p: f32) -> f32 {
    0.5 - 0.5 * (std::f32::consts::TAU * p).cos()
}

/// Skala mata dan opasitas dari animasi mood (breathe, pulse, surprised-pop).
fn mood_anim(a: &Anim, skin: &SkinDef) -> (f32, f32) {
    let breathe = |t: f32| 0.88 + 0.12 * ease_cos(t / 3.6);
    match a.mood {
        Mood::Love => (1.0 + 0.25 * ease_cos(a.t / 0.8), 1.0),
        Mood::Surprised => {
            // surprised-pop 0.4 dtk sekali: 0.6 -> 1.18 (70%) -> 1
            let p = (a.t_mood / 0.4).clamp(0.0, 1.0);
            let s = if p < 0.7 { mix(0.6, 1.18, p / 0.7) } else { mix(1.18, 1.0, (p - 0.7) / 0.3) };
            (s, 1.0)
        }
        Mood::Sleepy | Mood::Sad => (1.0, 1.0),
        _ if skin.name == "pixel" => (1.0, 1.0),
        _ => (1.0, breathe(a.t)),
    }
}

/// Kunci frame: dua frame dengan kunci sama (dan tatapan/kedip sama) identik, jadi penggambaran
/// ulang dilewati. Animasi kontinu (transisi, denyut, "z", tetes, muncul) memakai waktu 15 fps;
/// selain itu hanya napas pelan yang berubah (opasitas 0,88..1).
pub fn frame_key(a: &Anim, skin_name: &str, morphing: bool) -> i64 {
    let continuous = morphing || matches!(a.mood, Mood::Love | Mood::Sleepy | Mood::Sad) || a.t_mood < 0.45;
    if continuous {
        (a.t * 15.0) as i64
    } else {
        let (_, op) = mood_anim(a, &SKINS[skin_index(skin_name)]);
        (op * 100.0).round() as i64
    }
}

// ---------------------------------------------------------------- penggambar

/// Canvas premultiplied RGBA (float, 0..255 untuk rgb, 0..1 alpha).
struct Canvas {
    w: usize,
    h: usize,
    d: Vec<f32>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, d: vec![0.0; w * h * 4] }
    }

    /// Letakkan warna lurus `rgb` dengan alpha `a` di piksel `i` (indeks piksel).
    #[inline]
    fn over(&mut self, i: usize, rgb: [f32; 3], a: f32) {
        if a <= 0.0 {
            return;
        }
        let p = &mut self.d[i * 4..i * 4 + 4];
        let inv = 1.0 - a;
        p[0] = rgb[0] * a + p[0] * inv;
        p[1] = rgb[1] * a + p[1] * inv;
        p[2] = rgb[2] * a + p[2] * inv;
        p[3] = a + p[3] * inv;
    }

    /// Gabungkan canvas premultiplied lain (setelah filter/opasitas) di atas ini.
    fn merge(&mut self, o: &Canvas, bright: f32, opacity: f32) {
        for (dst, src) in self.d.chunks_exact_mut(4).zip(o.d.chunks_exact(4)) {
            let sa = src[3] * opacity;
            if sa <= 0.0 {
                continue;
            }
            let inv = 1.0 - sa;
            let k = bright * opacity;
            dst[0] = (src[0] * k).min(255.0 * sa) + dst[0] * inv;
            dst[1] = (src[1] * k).min(255.0 * sa) + dst[1] * inv;
            dst[2] = (src[2] * k).min(255.0 * sa) + dst[2] * inv;
            dst[3] = sa + dst[3] * inv;
        }
    }

    fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 4];
        for (o, p) in out.chunks_exact_mut(4).zip(self.d.chunks_exact(4)) {
            let a = p[3];
            if a > 0.0 {
                o[0] = (p[0] / a).clamp(0.0, 255.0).round() as u8;
                o[1] = (p[1] / a).clamp(0.0, 255.0).round() as u8;
                o[2] = (p[2] / a).clamp(0.0, 255.0).round() as u8;
                o[3] = (a * 255.0).clamp(0.0, 255.0).round() as u8;
            }
        }
        out
    }
}

/// Sebaran Gaussian lewat tiga kali box blur (sigma dalam piksel), tepi dianggap kosong.
fn gauss(buf: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let n = 3.0f32;
    let wl = ((12.0 * sigma * sigma / n + 1.0).sqrt().floor() as i32).max(1);
    let wl = if wl % 2 == 0 { wl - 1 } else { wl }.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    let mut tmp = vec![0.0f32; buf.len()];
    for pass in 0..3 {
        let r = ((if pass < m { wl } else { wu }) as usize - 1) / 2;
        box_h(buf, &mut tmp, w, h, r);
        box_v(&tmp, buf, w, h, r);
    }
}

fn box_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let inv = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let out = &mut dst[y * w..(y + 1) * w];
        let mut acc: f32 = row.iter().take(r + 1).sum();
        for x in 0..w {
            out[x] = acc * inv;
            let add = x + r + 1;
            if add < w {
                acc += row[add];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
}

fn box_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let inv = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        let mut acc = 0.0;
        for y in 0..=r.min(h - 1) {
            acc += src[y * w + x];
        }
        for y in 0..h {
            dst[y * w + x] = acc * inv;
            let add = y + r + 1;
            if add < h {
                acc += src[add * w + x];
            }
            if y >= r {
                acc -= src[(y - r) * w + x];
            }
        }
    }
}

/// Radius sudut CSS: bila jumlah radius satu sisi melebihi panjang sisi, semuanya diskalakan.
fn css_radii(w: f32, h: f32, r: &[(f32, f32); 4]) -> [(f32, f32); 4] {
    let side = |a: f32, b: f32, len: f32| if a + b > len && a + b > 0.0 { len / (a + b) } else { 1.0 };
    let f = side(r[0].0, r[1].0, w)
        .min(side(r[3].0, r[2].0, w))
        .min(side(r[0].1, r[3].1, h))
        .min(side(r[1].1, r[2].1, h))
        .min(1.0);
    [(r[0].0 * f, r[0].1 * f), (r[1].0 * f, r[1].1 * f), (r[2].0 * f, r[2].1 * f), (r[3].0 * f, r[3].1 * f)]
}

/// Titik (x, y) di dalam persegi bersudut elips? `d` = berapa px bentuk diperkecil (untuk garis tepi).
fn rr_inside(x: f32, y: f32, w: f32, h: f32, r: &[(f32, f32); 4], d: f32) -> bool {
    let (x0, y0, x1, y1) = (d, d, w - d, h - d);
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let sh = |v: (f32, f32)| ((v.0 - d).max(0.0), (v.1 - d).max(0.0));
    let (tl, tr, br, bl) = (sh(r[0]), sh(r[1]), sh(r[2]), sh(r[3]));
    let ell = |dx: f32, dy: f32, rx: f32, ry: f32| (dx / rx).powi(2) + (dy / ry).powi(2) <= 1.0;
    if tl.0 > 0.0 && tl.1 > 0.0 && x < x0 + tl.0 && y < y0 + tl.1 {
        return ell(x0 + tl.0 - x, y0 + tl.1 - y, tl.0, tl.1);
    }
    if tr.0 > 0.0 && tr.1 > 0.0 && x > x1 - tr.0 && y < y0 + tr.1 {
        return ell(x - (x1 - tr.0), y0 + tr.1 - y, tr.0, tr.1);
    }
    if br.0 > 0.0 && br.1 > 0.0 && x > x1 - br.0 && y > y1 - br.1 {
        return ell(x - (x1 - br.0), y - (y1 - br.1), br.0, br.1);
    }
    if bl.0 > 0.0 && bl.1 > 0.0 && x < x0 + bl.0 && y > y1 - bl.1 {
        return ell(x0 + bl.0 - x, y - (y1 - bl.1), bl.0, bl.1);
    }
    true
}

/// Titik (koordinat lokal mata) tertutup kelopak atas/bawah?
fn lid_hit(e: &EyeP, x: f32, y: f32) -> bool {
    if e.lid_t > 0.05 {
        let (cx, cy) = (e.w / 2.0, -1.0 + e.lid_t / 2.0);
        let (s, c) = (-e.lid_rot.to_radians()).sin_cos();
        let (sx, sy) = (x - cx, y - cy);
        let px = cx + sx * c - sy * s;
        let py = cy + sx * s + sy * c - e.lid_dy;
        if px >= -2.0 && px <= e.w + 2.0 && py >= -1.0 && py <= -1.0 + e.lid_t {
            return true;
        }
    }
    if e.lid_b > 0.05 {
        let top = e.h + 1.0 - e.lid_b;
        if x >= -2.0 && x <= e.w + 2.0 && y >= top && y <= e.h + 1.0 {
            if e.lid_b_round > 0.5 {
                let rx = (e.w + 4.0) / 2.0;
                let ry = (e.lid_b / 2.0).max(0.01);
                if y < top + ry {
                    let (dx, dy) = ((x - e.w / 2.0) / rx, (y - (top + ry)) / ry);
                    return dx * dx + dy * dy <= 1.0;
                }
            }
            return true;
        }
    }
    false
}

/// Pemetaan koordinat canvas (px logis) ke koordinat lokal mata, termasuk skala animasi.
#[derive(Clone, Copy)]
struct Xf {
    cx: f32,
    cy: f32,
    sx: f32,
    sy: f32,
    w: f32,
    h: f32,
}

impl Xf {
    fn local(&self, lx: f32, ly: f32) -> (f32, f32) {
        ((lx - self.cx) / self.sx + self.w / 2.0, (ly - self.cy) / self.sy + self.h / 2.0)
    }
    fn shifted(&self, dx: f32, dy: f32) -> Xf {
        Xf { cx: self.cx + dx, cy: self.cy + dy, ..*self }
    }
}

const SS: usize = 3;

/// Mask cakupan (0..1) dengan supersampling untuk tepi halus, hanya di dalam `bbox` (px fisik).
fn coverage<F: Fn(f32, f32) -> bool>(cw: usize, ch: usize, sf: f32, bbox: (usize, usize, usize, usize), xf: &Xf, test: F) -> Vec<f32> {
    let mut m = vec![0.0f32; cw * ch];
    let (x0, y0, x1, y1) = bbox;
    for py in y0..y1 {
        for px in x0..x1 {
            let mut n = 0;
            for j in 0..SS {
                for i in 0..SS {
                    let lx = (px as f32 + (i as f32 + 0.5) / SS as f32) / sf;
                    let ly = (py as f32 + (j as f32 + 0.5) / SS as f32) / sf;
                    let (x, y) = xf.local(lx, ly);
                    if test(x, y) {
                        n += 1;
                    }
                }
            }
            m[py * cw + px] = n as f32 / (SS * SS) as f32;
        }
    }
    m
}

fn resolve_c(c: C, skin_rgb: [f32; 3]) -> ([f32; 3], f32) {
    match c {
        C::Skin(a) => (skin_rgb, a),
        C::Rgb(r, g, b, a) => ([r as f32, g as f32, b as f32], a),
    }
}

fn col_rgba(c: Col, skin: [f32; 3], second: [f32; 3]) -> [f32; 4] {
    match c {
        Col::Skin(a) => [skin[0], skin[1], skin[2], a],
        Col::Second(a) => [second[0], second[1], second[2], a],
        Col::Rgb(r, g, b, a) => [r as f32, g as f32, b as f32, a],
        Col::Clear => [0.0, 0.0, 0.0, 0.0],
    }
}

fn premul(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Warna (premultiplied) satu lapisan di titik lokal (x, y) pada mata berukuran w x h.
fn layer_at(l: &Layer, skin: [f32; 3], second: [f32; 3], w: f32, h: f32, x: f32, y: f32) -> [f32; 4] {
    match l {
        Layer::Radial { cx, cy, rx, ry, stop, col } => {
            let t = (((x / w - cx) / rx).powi(2) + ((y / h - cy) / ry).powi(2)).sqrt();
            let f = (1.0 - t / stop).clamp(0.0, 1.0);
            let c = col_rgba(*col, skin, second);
            premul([c[0], c[1], c[2], c[3] * f])
        }
        Layer::Linear { deg, stops } => {
            let th = deg.to_radians();
            let (dx, dy) = (th.sin(), -th.cos());
            let len = (w * th.sin()).abs() + (h * th.cos()).abs();
            let t = 0.5 + ((x - w / 2.0) * dx + (y - h / 2.0) * dy) / len.max(1e-3);
            let pc = |c: Col| premul(col_rgba(c, skin, second));
            if t <= stops[0].0 {
                return pc(stops[0].1);
            }
            let last = stops.len() - 1;
            if t >= stops[last].0 {
                return pc(stops[last].1);
            }
            let mut i = 0;
            for (k, s) in stops.iter().enumerate() {
                if s.0 <= t {
                    i = k;
                }
            }
            let (a, b) = (stops[i], stops[(i + 1).min(last)]);
            let f = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 1.0 };
            let (ca, cb) = (pc(a.1), pc(b.1));
            [mix(ca[0], cb[0], f), mix(ca[1], cb[1], f), mix(ca[2], cb[2], f), mix(ca[3], cb[3], f)]
        }
        Layer::Stars(pts) => {
            let mut a = 0.0f32;
            for (px, py, r) in pts.iter() {
                let d = ((x - px * w).powi(2) + (y - py * h).powi(2)).sqrt();
                a = a.max((r - d + 0.5).clamp(0.0, 1.0));
            }
            [255.0 * a, 255.0 * a, 255.0 * a, a]
        }
        Layer::Scan => {
            if (y.rem_euclid(3.0)) >= 2.0 {
                premul([10.0, 0.0, 40.0, 0.38])
            } else {
                [0.0; 4]
            }
        }
    }
}

/// Penggambar mata untuk satu skin dan ukuran canvas (px logis).
pub struct EyeRenderer {
    skin: usize,
    layers: Vec<Layer>,
    cw: f32,
    ch: f32,
    vertical: bool,
}

impl EyeRenderer {
    pub fn new(skin_name: &str, canvas_w: f32, canvas_h: f32) -> EyeRenderer {
        let skin = skin_index(skin_name);
        EyeRenderer { skin, layers: fill_layers(&SKINS[skin]), cw: canvas_w, ch: canvas_h, vertical: false }
    }

    /// Tata dua mata bertumpuk untuk notch yang menempel di sisi kiri/kanan layar.
    /// Ukuran canvas sebaiknya portrait (mis. 34 x 130 px).
    pub fn new_vertical(skin_name: &str, canvas_w: f32, canvas_h: f32) -> EyeRenderer {
        let mut renderer = Self::new(skin_name, canvas_w, canvas_h);
        renderer.vertical = true;
        renderer
    }

    pub fn skin_name(&self) -> &'static str {
        SKINS[self.skin].name
    }

    /// Ukuran gambar dalam piksel fisik untuk faktor DPI `sf`.
    pub fn size_px(&self, sf: f32) -> (u32, u32) {
        ((self.cw * sf).round().max(1.0) as u32, (self.ch * sf).round().max(1.0) as u32)
    }

    /// Gambar satu frame. Hasil: RGBA8 non-premultiplied berukuran `size_px(sf)`.
    pub fn render(&self, face: &Face, anim: &Anim, sf: f32) -> Vec<u8> {
        let skin = &SKINS[self.skin];
        let (wpx, hpx) = self.size_px(sf);
        let (cw, ch) = (wpx as usize, hpx as usize);
        let mut out = Canvas::new(cw, ch);

        // Dua orientasi memakai bentuk mata dan animasi yang sama; hanya sumbu susunannya berubah.
        let (wc, hc, ox, oy, eyes) = if self.vertical {
            let wc = face.left.w.max(face.right.w);
            let hc = face.left.h + GAP + face.right.h;
            let ox = (self.cw - wc) / 2.0 + anim.look.0;
            let oy = (self.ch - hc) / 2.0 + 1.0 + anim.look.1;
            (wc, hc, ox, oy, [( &face.left, oy ), ( &face.right, oy + face.left.h + GAP )])
        } else {
            let wc = face.left.w + GAP + face.right.w;
            let hc = face.left.h.max(face.right.h);
            let ox = (self.cw - wc) / 2.0 + anim.look.0;
            let oy = (self.ch - hc) / 2.0 + 1.0 + anim.look.1;
            (wc, hc, ox, oy, [( &face.left, ox ), ( &face.right, ox + face.left.w + GAP )])
        };
        let (pulse, opacity) = mood_anim(anim, skin);
        let blink_sy = if anim.blink && anim.mood != Mood::Love { 0.08 } else { 1.0 };

        for (e, offset) in eyes {
            let (cx, cy) = if self.vertical {
                (ox + e.w / 2.0, offset + e.h / 2.0)
            } else {
                (offset + e.w / 2.0, oy + e.h / 2.0)
            };
            let xf = Xf { cx, cy, sx: pulse, sy: pulse * blink_sy, w: e.w, h: e.h };
            let mut eye = Canvas::new(cw, ch);
            self.draw_eye(&mut eye, e, &xf, sf);
            out.merge(&eye, e.bright, opacity);
        }
        self.draw_fx(&mut out, face, anim, sf, ox, oy, wc, hc);
        out.to_rgba8()
    }

    fn draw_eye(&self, cv: &mut Canvas, e: &EyeP, xf: &Xf, sf: f32) {
        let skin = &SKINS[self.skin];
        let (cw, ch) = (cv.w, cv.h);
        let rad = css_radii(e.w, e.h, &e.rad);
        let (srgb, second) = (e.rgb, [skin.second[0] as f32, skin.second[1] as f32, skin.second[2] as f32]);

        // kotak batas (px fisik) tempat badan mata berada, lengkap dengan margin untuk tepi
        let ext = (e.w.max(e.h) * xf.sx.max(xf.sy) / 2.0 + 6.0) * 1.0;
        let bx = |v: f32, hi: usize| ((v * sf).floor().max(0.0) as usize).min(hi);
        let bbox = (
            bx(xf.cx - ext, cw),
            bx(xf.cy - ext, ch),
            bx(xf.cx + ext + 1.0, cw),
            bx(xf.cy + ext + 1.0, ch),
        );

        let shape = coverage(cw, ch, sf, bbox, xf, |x, y| rr_inside(x, y, e.w, e.h, &rad, 0.0));
        let body = coverage(cw, ch, sf, bbox, xf, |x, y| rr_inside(x, y, e.w, e.h, &rad, 0.0) && !lid_hit(e, x, y));

        // ---- bayangan luar (glow + offset), urutan CSS: yang pertama paling atas
        let mut outer: Vec<Sh> = vec![
            Sh { inset: false, dx: 0.0, dy: 0.0, blur: 6.0 + skin.glow_extra, color: C::Skin(e.glow_a) },
            Sh { inset: false, dx: 0.0, dy: 0.0, blur: 15.0 + skin.glow_extra, color: C::Skin(skin.glow_soft) },
        ];
        outer.extend(skin.shadows.iter().copied().filter(|s| !s.inset));
        for sh in outer.iter().rev() {
            let mut m = if sh.dx == 0.0 && sh.dy == 0.0 {
                shape.clone()
            } else {
                let xs = xf.shifted(sh.dx, sh.dy);
                coverage(cw, ch, sf, shifted_bbox(bbox, sh, sf, cw, ch), &xs, |x, y| rr_inside(x, y, e.w, e.h, &rad, 0.0))
            };
            gauss(&mut m, cw, ch, sh.blur / 2.0 * sf);
            let (rgb, a) = resolve_c(sh.color, srgb);
            for i in 0..cw * ch {
                if m[i] > 0.0 {
                    // bayangan luar tidak digambar di bawah badan elemen
                    cv.over(i, rgb, a * m[i] * (1.0 - shape[i]));
                }
            }
        }

        // ---- badan: background-color skin + lapisan gradien
        let (x0, y0, x1, y1) = bbox;
        for py in y0..y1 {
            for px in x0..x1 {
                let i = py * cw + px;
                if body[i] <= 0.0 {
                    continue;
                }
                let (x, y) = xf.local((px as f32 + 0.5) / sf, (py as f32 + 0.5) / sf);
                let mut c = premul([srgb[0], srgb[1], srgb[2], 1.0]);
                for l in self.layers.iter().rev() {
                    let s = layer_at(l, srgb, second, e.w, e.h, x, y);
                    let inv = 1.0 - s[3];
                    c = [s[0] + c[0] * inv, s[1] + c[1] * inv, s[2] + c[2] * inv, s[3] + c[3] * inv];
                }
                cv.over(i, [c[0], c[1], c[2]], body[i] * c[3]);
            }
        }

        // ---- bayangan dalam
        for sh in skin.shadows.iter().filter(|s| s.inset) {
            let xs = xf.shifted(sh.dx, sh.dy);
            let mut out_m = coverage(cw, ch, sf, (0, 0, cw, ch), &xs, |x, y| !rr_inside(x, y, e.w, e.h, &rad, 0.0));
            gauss(&mut out_m, cw, ch, sh.blur / 2.0 * sf);
            let (rgb, a) = resolve_c(sh.color, srgb);
            for i in 0..cw * ch {
                if body[i] > 0.0 {
                    cv.over(i, rgb, a * out_m[i] * body[i]);
                }
            }
        }

        // ---- garis tepi (border) di sisi dalam bentuk
        if let Some(o) = skin.outline {
            let inner = coverage(cw, ch, sf, bbox, xf, |x, y| rr_inside(x, y, e.w, e.h, &rad, o.w));
            let (rgb, a) = resolve_c(o.color, srgb);
            for i in 0..cw * ch {
                let ring = (shape[i] - inner[i]).max(0.0);
                if ring > 0.0 && body[i] > 0.0 {
                    cv.over(i, rgb, a * ring * body[i].min(1.0));
                }
            }
        }

        // ---- kilau
        if e.shine_w > 0.05 {
            let (kind, col) = shine_style(skin.name);
            let (sw, sh_) = (e.shine_w, e.shine_h);
            let c = (2.5 + sw / 2.0, 3.0 + sh_ / 2.0);
            let rot = if skin.name == "slime" { -24.0f32.to_radians() } else { 0.0 };
            let shine = coverage(cw, ch, sf, bbox, xf, |x, y| {
                if !rr_inside(x, y, e.w, e.h, &rad, 0.0) || lid_hit(e, x, y) {
                    return false;
                }
                let (dx, dy) = (x - c.0, y - c.1);
                let (s, co) = (-rot).sin_cos();
                let (u, v) = (dx * co - dy * s, dx * s + dy * co);
                match kind {
                    ShineKind::Round => (u / (sw / 2.0)).powi(2) + (v / (sh_ / 2.0)).powi(2) <= 1.0,
                    ShineKind::Rect => u.abs() <= sw / 2.0 && v.abs() <= sh_ / 2.0,
                    ShineKind::Diamond => (u.abs() + v.abs()) * std::f32::consts::FRAC_1_SQRT_2 * 1.0 <= sw / 2.0 * 1.0,
                }
            });
            for i in 0..cw * ch {
                if shine[i] > 0.0 {
                    cv.over(i, [col[0], col[1], col[2]], col[3] * shine[i]);
                }
            }
            if skin.name == "kawaii" {
                // titik kecil ::after di kanan-bawah
                let dc = (e.w * 0.78 - 0.9, e.h * 0.84 - 0.9);
                let dot = coverage(cw, ch, sf, bbox, xf, |x, y| (x - dc.0).powi(2) + (y - dc.1).powi(2) <= 0.81 && !lid_hit(e, x, y));
                for i in 0..cw * ch {
                    if dot[i] > 0.0 {
                        cv.over(i, [255.0, 255.0, 255.0], 0.9 * dot[i]);
                    }
                }
            }
        }
    }

    /// "z" tidur dan tetes air mata (di kontainer mata, bukan di dalam mata).
    #[allow(clippy::too_many_arguments)]
    fn draw_fx(&self, out: &mut Canvas, face: &Face, anim: &Anim, sf: f32, ox: f32, oy: f32, wc: f32, hc: f32) {
        let (cw, ch) = (out.w, out.h);
        let rgb = face.fx_rgb;
        if face.z > 0.01 {
            let p = (anim.t / 2.4).fract();
            let op = if p < 0.4 { p / 0.4 } else { 1.0 - (p - 0.4) / 0.6 };
            let sc = mix(0.7, 1.15, p);
            let base = (ox + wc + 11.0 - 5.5, oy - 9.0);
            let c = (base.0 + 2.75 + 5.0 * p, base.1 + 3.25 - 9.0 * p);
            let (w, h) = (5.0 * sc, 6.0 * sc);
            let pts = [(c.0 - w / 2.0, c.1 - h / 2.0), (c.0 + w / 2.0, c.1 - h / 2.0), (c.0 - w / 2.0, c.1 + h / 2.0), (c.0 + w / 2.0, c.1 + h / 2.0)];
            let segs = [(pts[0], pts[1]), (pts[1], pts[2]), (pts[2], pts[3])];
            let half = 0.75 * sc;
            for py in 0..ch {
                for px in 0..cw {
                    let (x, y) = ((px as f32 + 0.5) / sf, (py as f32 + 0.5) / sf);
                    let d = segs.iter().map(|(a, b)| dist_seg((x, y), *a, *b)).fold(f32::MAX, f32::min);
                    let cov = (half - d + 0.5 / sf).clamp(0.0, 1.0);
                    if cov > 0.0 {
                        out.over(py * cw + px, rgb, 0.85 * op * face.z * cov);
                    }
                }
            }
        }
        if face.tear > 0.01 {
            let p = (anim.t / 2.2).fract();
            let (op, ty) = if p < 0.15 {
                (p / 0.15, 0.0)
            } else if p < 0.8 {
                (mix(1.0, 0.4, (p - 0.15) / 0.65), 7.0 * (p - 0.15) / 0.65)
            } else {
                (mix(0.4, 0.0, (p - 0.8) / 0.2), mix(7.0, 8.0, (p - 0.8) / 0.2))
            };
            // kotak 3x4 di kanan-bawah kontainer, diputar 45 derajat, lalu digeser sepanjang sumbu lokal
            let c = (ox + wc - 1.0 - 1.5, oy + hc + 3.0 - 2.0);
            let th = 45.0f32.to_radians();
            let (s, co) = th.sin_cos();
            let (tx, ty2) = (-ty * s, ty * co);
            let rad = css_radii(3.0, 4.0, &[(0.0, 0.0), (1.5, 2.0), (1.5, 2.0), (1.5, 2.0)]);
            for py in 0..ch {
                for px in 0..cw {
                    let mut n = 0;
                    for j in 0..SS {
                        for i in 0..SS {
                            let lx = (px as f32 + (i as f32 + 0.5) / SS as f32) / sf - c.0 - tx;
                            let ly = (py as f32 + (j as f32 + 0.5) / SS as f32) / sf - c.1 - ty2;
                            let (u, v) = (lx * co + ly * s, -lx * s + ly * co);
                            if rr_inside(u + 1.5, v + 2.0, 3.0, 4.0, &rad, 0.0) {
                                n += 1;
                            }
                        }
                    }
                    if n > 0 {
                        out.over(py * cw + px, rgb, 0.85 * op * face.tear * n as f32 / (SS * SS) as f32);
                    }
                }
            }
        }
    }
}

fn shifted_bbox(b: (usize, usize, usize, usize), sh: &Sh, sf: f32, cw: usize, ch: usize) -> (usize, usize, usize, usize) {
    let (dx, dy) = ((sh.dx.abs() * sf).ceil() as usize + 1, (sh.dy.abs() * sf).ceil() as usize + 1);
    (b.0.saturating_sub(dx), b.1.saturating_sub(dy), (b.2 + dx).min(cw), (b.3 + dy).min(ch))
}

fn dist_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = (abx * abx + aby * aby).max(1e-6);
    let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0);
    ((p.0 - (a.0 + abx * t)).powi(2) + (p.1 - (a.1 + aby * t)).powi(2)).sqrt()
}

// ---------------------------------------------------------------- state mood

/// Mood dasar + sementara (flash), tidur saat idle, reaksi klik. Padanan `mood.ts` dan bagian klik
/// di `notch-face.ts`. Tidak menyentuh UI: pemanggil memberi waktu dan membaca `current`.
pub struct MoodState {
    default: Mood,
    base: Mood,
    temp: Option<(Mood, Instant)>,
    last_activity: Instant,
    sleeping: bool,
    clicks: Vec<Instant>,
    was_charging: Option<bool>,
    sleep_after: Duration,
    pub playing: bool,
    pub low_battery: bool,
    pub keep_awake: bool,
}

impl MoodState {
    pub fn new(default: Mood, sleep_after_s: u32, now: Instant) -> MoodState {
        MoodState {
            default,
            base: default,
            temp: None,
            last_activity: now,
            sleeping: false,
            clicks: Vec::new(),
            was_charging: None,
            sleep_after: Duration::from_secs(u64::from(sleep_after_s)),
            playing: false,
            low_battery: false,
            keep_awake: false,
        }
    }

    /// Mood yang harus tampil sekarang (mood sementara habis dengan sendirinya).
    pub fn current(&mut self, now: Instant) -> Mood {
        if let Some((m, until)) = self.temp {
            if now < until {
                return m;
            }
            self.temp = None;
        }
        self.base
    }

    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    /// Perbarui ambang idle tanpa mereset mood, klik, atau aktivitas yang sedang berjalan.
    pub fn set_sleep_after(&mut self, seconds: u32) {
        self.sleep_after = Duration::from_secs(u64::from(seconds));
    }

    pub fn flash(&mut self, m: Mood, ms: u64, now: Instant) {
        self.temp = Some((m, now + Duration::from_millis(ms)));
    }

    /// Dipanggil tiap ~10 dtk dan saat kondisi (musik, baterai) berubah. `hour` = jam lokal 0..23.
    pub fn update_base(&mut self, now: Instant, hour: u32) {
        let idle_long = now.saturating_duration_since(self.last_activity) > self.sleep_after;
        let night = hour < 5;
        let should_sleep = (idle_long || night) && !self.keep_awake;
        if self.sleeping && !should_sleep {
            self.flash(Mood::Surprised, 900, now);
        }
        self.sleeping = should_sleep;
        self.base = if should_sleep || self.low_battery {
            Mood::Sleepy
        } else if self.playing {
            Mood::Happy
        } else {
            self.default
        };
    }

    /// Ada gerakan kursor atau tombol. Bila sedang tidur, bangun segera.
    pub fn activity(&mut self, now: Instant, hour: u32) {
        self.last_activity = now;
        if self.sleeping {
            self.update_base(now, hour);
        }
    }

    /// Klik: happy 1,2 dtk; lima klik dalam 2 dtk membuat marah 2 dtk.
    pub fn click(&mut self, now: Instant) {
        self.clicks.retain(|t| now.saturating_duration_since(*t) < Duration::from_secs(2));
        self.clicks.push(now);
        if self.clicks.len() >= 5 {
            self.flash(Mood::Angry, 2000, now);
            self.clicks.clear();
        } else {
            self.flash(Mood::Happy, 1200, now);
        }
    }

    pub fn double_click(&mut self, now: Instant) {
        self.flash(Mood::Love, 2500, now);
    }

    /// Bacaan baterai baru (`handleBattery` di devices.ts): mulai mengisi -> love 3 dtk;
    /// lemah = 15% atau kurang dan tidak mengisi -> mood dasar mengantuk.
    pub fn on_battery(&mut self, percent: u8, charging: bool, now: Instant, hour: u32) {
        if self.was_charging == Some(false) && charging {
            self.flash(Mood::Love, 3000, now);
        }
        self.was_charging = Some(charging);
        let low = percent <= 15 && !charging;
        if low != self.low_battery {
            self.low_battery = low;
            self.update_base(now, hour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(m: Mood) -> Anim {
        Anim { mood: m, t: 1.3, t_mood: 1.0, look: (0.0, 0.0), blink: false }
    }

    #[test]
    fn semua_skin_dan_mood_bisa_digambar() {
        for name in skin_names() {
            let r = EyeRenderer::new(name, 120.0, 34.0);
            for m in Mood::ALL {
                let f = Face::for_mood(m, skin_index(name));
                let px = r.render(&f, &anim(m), 1.0);
                assert_eq!(px.len(), 120 * 34 * 4, "{name}/{}", m.name());
                assert!(px.chunks_exact(4).any(|p| p[3] > 128), "{name}/{} kosong", m.name());
            }
        }
    }

    #[test]
    fn renderer_vertikal_menumpuk_mata_dan_menghormati_ukuran_canvas() {
        for name in skin_names() {
            let r = EyeRenderer::new_vertical(name, 34.0, 120.0);
            assert_eq!(r.size_px(1.0), (34, 120));
            for mood in Mood::ALL {
                let frame = r.render(&Face::for_mood(mood, skin_index(name)), &anim(mood), 1.0);
                assert_eq!(frame.len(), 34 * 120 * 4, "{name}/{}", mood.name());
                assert!(frame.chunks_exact(4).any(|p| p[3] > 128), "{name}/{} kosong", mood.name());
            }
        }
    }

    #[test]
    fn kedip_menipiskan_mata() {
        let r = EyeRenderer::new("aurora", 120.0, 34.0);
        let f = Face::for_mood(Mood::Normal, 0);
        let opaque = |blink| {
            let mut a = anim(Mood::Normal);
            a.blink = blink;
            r.render(&f, &a, 1.0).chunks_exact(4).filter(|p| p[3] > 200).count()
        };
        assert!(opaque(true) * 4 < opaque(false));
    }

    #[test]
    fn frame_key_diam_tidak_berubah_tiap_frame() {
        let mut a = anim(Mood::Normal);
        a.t_mood = 5.0;
        let k1 = frame_key(&a, "aurora", false);
        a.t += 0.066;
        let k2 = frame_key(&a, "aurora", false);
        assert_eq!(k1, k2, "napas pelan jangan memicu gambar ulang tiap frame");
        let mut l = anim(Mood::Love);
        l.t_mood = 5.0;
        let (a1, mut a2) = (frame_key(&l, "aurora", false), l);
        a2.t += 0.07;
        assert_ne!(a1, frame_key(&a2, "aurora", false));
    }

    /// Ukur: `cargo test --release --lib ukur_waktu_gambar -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ukur_waktu_gambar() {
        for (skin, sf) in [("aurora", 1.0f32), ("aurora", 2.0), ("neon", 2.0), ("galaxy", 2.0)] {
            let r = EyeRenderer::new(skin, 120.0, 34.0);
            let f = Face::for_mood(Mood::Normal, skin_index(skin));
            let a = anim(Mood::Normal);
            let t = Instant::now();
            for _ in 0..50 {
                std::hint::black_box(r.render(&f, &a, sf));
            }
            println!("{skin} sf={sf}: {:.2} ms/frame", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
        }
    }

    #[test]
    fn dpi_menambah_piksel() {
        let r = EyeRenderer::new("neon", 120.0, 34.0);
        assert_eq!(r.size_px(2.0), (240, 68));
    }

    #[test]
    fn skin_tak_dikenal_jatuh_ke_aurora() {
        assert_eq!(skin_index("tidak-ada"), 0);
        assert_eq!(skin_index("frost"), 19);
    }

    #[test]
    fn tatapan_dibatasi_enam_px() {
        let (x, y) = look_offset(10_000.0, 0.0);
        assert_eq!((x, y), (6.0, 0.0));
        assert_eq!(look_offset(0.0, 0.0), (0.0, 0.0));
        let (_, y) = look_offset(0.0, 10_000.0);
        assert_eq!(y, 1.0);
    }

    #[test]
    fn mood_parse_dan_nama() {
        for m in Mood::ALL {
            assert_eq!(Mood::parse(m.name()), Some(m));
        }
        assert_eq!(Mood::parse("x"), None);
    }

    #[test]
    fn tidur_saat_idle_dan_bangun_terkejut() {
        let t0 = Instant::now();
        let mut s = MoodState::new(Mood::Normal, 60, t0);
        s.update_base(t0 + Duration::from_secs(61), 12);
        assert_eq!(s.current(t0 + Duration::from_secs(61)), Mood::Sleepy);
        let t1 = t0 + Duration::from_secs(70);
        s.activity(t1, 12);
        assert_eq!(s.current(t1), Mood::Surprised);
        assert_eq!(s.current(t1 + Duration::from_millis(901)), Mood::Normal);
    }

    #[test]
    fn keep_awake_mencegah_tidur_dan_malam_menidurkan() {
        let t0 = Instant::now();
        let mut s = MoodState::new(Mood::Normal, 60, t0);
        s.keep_awake = true;
        s.update_base(t0 + Duration::from_secs(600), 3);
        assert_eq!(s.current(t0 + Duration::from_secs(600)), Mood::Normal);
        s.keep_awake = false;
        s.update_base(t0 + Duration::from_secs(1), 3); // jam 03: malam
        assert_eq!(s.current(t0 + Duration::from_secs(1)), Mood::Sleepy);
    }

    #[test]
    fn musik_dan_baterai_lemah_menentukan_mood_dasar() {
        let t0 = Instant::now();
        let mut s = MoodState::new(Mood::Sad, 600, t0);
        s.playing = true;
        s.update_base(t0, 12);
        assert_eq!(s.current(t0), Mood::Happy);
        s.low_battery = true;
        s.update_base(t0, 12);
        assert_eq!(s.current(t0), Mood::Sleepy);
    }

    #[test]
    fn klik_happy_lima_klik_marah_dobel_klik_cinta() {
        let t0 = Instant::now();
        let mut s = MoodState::new(Mood::Normal, 600, t0);
        s.click(t0);
        assert_eq!(s.current(t0), Mood::Happy);
        assert_eq!(s.current(t0 + Duration::from_millis(1201)), Mood::Normal);
        for i in 1..5 {
            s.click(t0 + Duration::from_millis(100 * i));
        }
        assert_eq!(s.current(t0 + Duration::from_millis(500)), Mood::Angry);
        s.double_click(t0 + Duration::from_secs(5));
        assert_eq!(s.current(t0 + Duration::from_secs(5)), Mood::Love);
    }

    #[test]
    fn baterai_lemah_mengantuk_mengisi_membuat_cinta() {
        let t0 = Instant::now();
        let mut s = MoodState::new(Mood::Normal, 600, t0);
        s.on_battery(15, false, t0, 12);
        assert_eq!(s.current(t0), Mood::Sleepy);
        s.on_battery(16, false, t0, 12);
        assert_eq!(s.current(t0), Mood::Normal);
        s.on_battery(10, true, t0, 12); // mulai mengisi
        assert_eq!(s.current(t0), Mood::Love);
        assert_eq!(s.current(t0 + Duration::from_millis(3001)), Mood::Normal);
        // bacaan pertama yang sudah mengisi tidak memicu love
        let mut s2 = MoodState::new(Mood::Normal, 600, t0);
        s2.on_battery(50, true, t0, 12);
        assert_eq!(s2.current(t0), Mood::Normal);
    }

    #[test]
    fn morph_selesai_dalam_250_ms() {
        let mut m = Morph::new(Face::for_mood(Mood::Normal, 0));
        m.set_target(1.0, Face::for_mood(Mood::Surprised, 0));
        assert!(!m.settled(1.1));
        assert!(m.settled(1.26));
        let end = m.current(2.0);
        assert!((end.left.w - 10.0).abs() < 1e-4);
    }
}
