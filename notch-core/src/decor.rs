//! Hiasan tema notch (cahaya, garis scan, rim, bibir) dirender ke gambar RGBA, meniru
//! `src/css/notch-themes.css`. Digambar sendiri (bukan lewat clip UI) karena renderer software Slint
//! tidak memotong anak elemen mengikuti radius sudut, sehingga cahaya bocor keluar lengkungan.
//!
//! Aturan seperti di CSS: hiasan dan cahaya hanya di SISI BEBAS; sisi yang menempel layar dipudarkan
//! ke warna dasar dan tanpa garis supaya menyatu dengan telinga yang polos. Dock bawah = bayangan
//! cermin vertikal dari dock atas. Melayang = garis tepi penuh, tanpa pudar tepi.

use crate::layout::Dock;
use crate::theme::Rgba;

/// Satu lapis hiasan. Koordinat/radius berupa pecahan lebar (x) dan tinggi (y) badan notch.
#[derive(Clone, Copy, Debug)]
enum Layer {
    /// `radial-gradient(rx ry at cx cy, color, transparent stop)` — elips dengan radius eksplisit.
    Radial { cx: f32, cy: f32, rx: f32, ry: f32, color: Rgba, stop: f32 },
    /// `linear-gradient(0deg, color, transparent stop)` — dari tepi bawah ke atas, `stop` pecahan tinggi.
    FromBottom { color: Rgba, stop: f32 },
    /// `linear-gradient(180deg, color, transparent stop)` — dari tepi atas ke bawah.
    FromTop { color: Rgba, stop: f32 },
    /// `repeating-linear-gradient(0deg, color 0 1px, transparent 1px 3px)` — garis scan.
    Scan { color: Rgba },
    /// `repeating-linear-gradient(45deg, color 0 6px, transparent 6px 12px)` — garis miring.
    Stripes { color: Rgba },
    /// Polkadot: tile 16 px, titik berradius 1,7 px (tepi lembut sampai 2,2 px).
    Dots { color: Rgba },
}

/// Bayangan dalam (`box-shadow: inset 0 -<off> <blur> <spread> color`), di sisi bebas.
#[derive(Clone, Copy, Debug)]
struct Glow {
    color: Rgba,
    off: f32,
    blur: f32,
    spread: f32,
}

#[derive(Clone, Debug)]
struct ThemeDecor {
    fade_y: f32,
    layers: Vec<Layer>,
    rim: (Rgba, f32),
    lip: (Rgba, f32),
    glow: Option<Glow>,
}

fn c(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    Rgba::rgb(r, g, b).with_alpha(a)
}

fn theme_decor(name: &str) -> Option<ThemeDecor> {
    Some(match name {
        "aurora" => ThemeDecor {
            fade_y: 7.0,
            layers: vec![
                Layer::Radial { cx: 0.12, cy: 1.55, rx: 0.85, ry: 1.40, color: c(82, 226, 200, 0.50), stop: 0.62 },
                Layer::Radial { cx: 0.92, cy: 1.65, rx: 0.85, ry: 1.40, color: c(139, 92, 246, 0.56), stop: 0.64 },
            ],
            rim: (c(130, 235, 255, 0.16), 1.0),
            lip: (c(130, 235, 255, 0.22), 1.0),
            glow: Some(Glow { color: c(82, 226, 200, 0.38), off: 10.0, blur: 18.0, spread: -12.0 }),
        },
        "obsidian" => ThemeDecor {
            fade_y: 7.0,
            layers: vec![
                Layer::FromBottom { color: c(255, 235, 190, 0.07), stop: 0.45 },
                Layer::Radial { cx: 0.5, cy: 1.4, rx: 1.2, ry: 0.9, color: c(240, 190, 90, 0.30), stop: 0.62 },
            ],
            rim: (c(240, 190, 90, 0.28), 1.0),
            lip: (c(255, 225, 150, 0.38), 1.0),
            glow: None,
        },
        "neon" => ThemeDecor {
            fade_y: 7.0,
            layers: vec![
                Layer::Radial { cx: 0.0, cy: 1.25, rx: 0.70, ry: 1.30, color: c(255, 43, 214, 0.46), stop: 0.62 },
                Layer::Radial { cx: 1.0, cy: 1.30, rx: 0.70, ry: 1.30, color: c(0, 229, 255, 0.36), stop: 0.62 },
                Layer::Scan { color: c(255, 255, 255, 0.03) },
            ],
            rim: (c(0, 229, 255, 0.45), 1.0),
            lip: (c(255, 43, 214, 0.55), 1.0),
            glow: Some(Glow { color: c(255, 43, 214, 0.45), off: 8.0, blur: 14.0, spread: -9.0 }),
        },
        "sakura" => ThemeDecor {
            fade_y: 0.0,
            layers: vec![
                Layer::Dots { color: c(255, 255, 255, 0.62) },
                Layer::Radial { cx: 0.5, cy: -0.3, rx: 1.2, ry: 1.6, color: c(255, 92, 150, 0.22), stop: 0.60 },
                Layer::FromTop { color: c(255, 255, 255, 0.5), stop: 0.5 },
            ],
            rim: (c(255, 120, 170, 0.55), 1.5),
            lip: (c(255, 120, 170, 0.36), 3.0),
            glow: None,
        },
        "matcha" => ThemeDecor {
            fade_y: 0.0,
            layers: vec![
                Layer::Stripes { color: c(255, 255, 255, 0.38) },
                Layer::Radial { cx: 0.5, cy: -0.3, rx: 1.2, ry: 1.6, color: c(38, 166, 91, 0.22), stop: 0.60 },
                Layer::FromTop { color: c(255, 255, 255, 0.45), stop: 0.5 },
            ],
            rim: (c(38, 166, 91, 0.50), 1.5),
            lip: (c(38, 166, 91, 0.30), 3.0),
            glow: None,
        },
        _ => return None,
    })
}

/// Apakah tema ini punya hiasan (tema "default" tidak).
pub fn has_decor(theme: &str) -> bool {
    theme_decor(theme).is_some()
}

/// Radius sudut badan: (kiri-atas, kanan-atas, kanan-bawah, kiri-bawah), dalam piksel gambar.
pub fn radii(dock: Dock, r: f32) -> [f32; 4] {
    match dock {
        Dock::Top => [0.0, 0.0, r, r],
        Dock::Bottom => [r, r, 0.0, 0.0],
        Dock::Floating | Dock::Left | Dock::Right => [r, r, r, r],
    }
}

/// Jarak bertanda ke persegi panjang bersudut bulat (negatif = di dalam). Piksel dengan koordinat (x, y)
/// terhadap pojok kiri-atas; `r` = radius [tl, tr, br, bl].
fn sdf(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> f32 {
    let (hx, hy) = (w / 2.0, h / 2.0);
    let (px, py) = (x - hx, y - hy);
    let rr = match (px < 0.0, py < 0.0) {
        (true, true) => r[0],
        (false, true) => r[1],
        (false, false) => r[2],
        (true, false) => r[3],
    };
    let qx = px.abs() - (hx - rr);
    let qy = py.abs() - (hy - rr);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rr
}

/// Sampel per sumbu untuk supersampling tepi (4 x 4 = 16 sampel per piksel tepi).
const SSN: usize = 4;


// Piksel dalam alfa-terpremultiplikasi (r, g, b, a) bernilai 0..1.
type Px = [f32; 4];

fn over(dst: &mut Px, color: Rgba, alpha: f32) {
    let a = (f32::from(color.a) / 255.0) * alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let src = [
        f32::from(color.r) / 255.0 * a,
        f32::from(color.g) / 255.0 * a,
        f32::from(color.b) / 255.0 * a,
        a,
    ];
    for i in 0..4 {
        dst[i] = src[i] + dst[i] * (1.0 - a);
    }
}

fn layer_alpha(l: &Layer, x: f32, y: f32, w: f32, h: f32, px: f32) -> (Rgba, f32) {
    match *l {
        Layer::Radial { cx, cy, rx, ry, color, stop } => {
            let (dx, dy) = ((x - cx * w) / (rx * w), (y - cy * h) / (ry * h));
            let t = dx.hypot(dy);
            (color, if t >= stop { 0.0 } else { 1.0 - t / stop })
        }
        Layer::FromBottom { color, stop } => {
            let t = (h - y) / (stop * h);
            (color, (1.0 - t).clamp(0.0, 1.0))
        }
        Layer::FromTop { color, stop } => {
            let t = y / (stop * h);
            (color, (1.0 - t).clamp(0.0, 1.0))
        }
        Layer::Scan { color } => {
            // 0deg = dari bawah ke atas: tiap periode 3 px, 1 px pertama berwarna
            let from_bottom = (h - y) / px;
            (color, if from_bottom.rem_euclid(3.0) < 1.0 { 1.0 } else { 0.0 })
        }
        Layer::Stripes { color } => {
            // 45deg: garis miring, periode 12 px, 6 px pertama berwarna
            let d = ((x + (h - y)) / px) * std::f32::consts::FRAC_1_SQRT_2;
            (color, if d.rem_euclid(12.0) < 6.0 { 1.0 } else { 0.0 })
        }
        Layer::Dots { color } => {
            let tile = 16.0 * px;
            let (tx, ty) = (x.rem_euclid(tile) - tile / 2.0, y.rem_euclid(tile) - tile / 2.0);
            let d = tx.hypot(ty) / px;
            (color, ((2.2 - d) / (2.2 - 1.7)).clamp(0.0, 1.0))
        }
    }
}

/// Render hiasan `theme` untuk badan berukuran `w` x `h` piksel gambar (`px` = piksel gambar per
/// piksel CSS, mis. faktor DPI). Mengembalikan RGBA8 *non-premultiplied* (`w*h*4` byte), atau `None`
/// untuk tema tanpa hiasan. Piksel di luar sudut bulat transparan penuh.
pub fn render(theme: &str, body: Rgba, dock: Dock, w: u32, h: u32, corner_r: f32, px: f32) -> Option<Vec<u8>> {
    let td = theme_decor(theme)?;
    let (wf, hf) = (w as f32, h as f32);
    let r = radii(dock, corner_r);
    // Dock bawah digambar sebagai dock atas lalu dicerminkan vertikal.
    let draw_r = if dock == Dock::Bottom { radii(Dock::Top, corner_r) } else { r };
    let floating = dock == Dock::Floating;
    let fade_x = 14.0 * px;
    let fade_y = td.fade_y * px;
    let (rim_c, rim_w) = (td.rim.0, td.rim.1 * px);
    let (lip_c, lip_w) = (td.lip.0, td.lip.1 * px);

    let mut out = vec![0u8; (w * h * 4) as usize];
    for yy in 0..h {
        for xx in 0..w {
            let (x, y) = (xx as f32 + 0.5, yy as f32 + 0.5);
            let d0 = sdf(x, y, wf, hf, draw_r);
            if d0 > 1.0 {
                continue; // jauh di luar bentuk
            }
            // Cakupan piksel terhadap badan (`inside`), bibir (`lip_rel`), dan rim (`rim_rel`; dua
            // terakhir relatif terhadap bagian yang di dalam). Dekat tepi dihitung dengan supersampling
            // 4x4 supaya lengkungan dan garis 1 px halus; di bagian dalam cukup satu sampel.
            let reach = lip_w.max(rim_w) + 1.5;
            let (inside, lip_rel, rim_rel) = if d0 < -reach {
                (1.0, 0.0, 0.0)
            } else {
                let (mut ins, mut lip, mut rim) = (0u32, 0u32, 0u32);
                for j in 0..SSN {
                    for i in 0..SSN {
                        let sx = xx as f32 + (i as f32 + 0.5) / SSN as f32;
                        let sy = yy as f32 + (j as f32 + 0.5) / SSN as f32;
                        let d = sdf(sx, sy, wf, hf, draw_r);
                        if d > 0.0 {
                            continue;
                        }
                        ins += 1;
                        if sdf(sx, sy + lip_w, wf, hf, draw_r) > 0.0 {
                            lip += 1;
                        }
                        let in_rim = if floating { d + rim_w > 0.0 } else { sdf(sx, sy + rim_w, wf, hf, draw_r) > 0.0 };
                        if in_rim {
                            rim += 1;
                        }
                    }
                }
                if ins == 0 {
                    continue;
                }
                (ins as f32 / (SSN * SSN) as f32, lip as f32 / ins as f32, rim as f32 / ins as f32)
            };
            let mut p: Px = [0.0; 4];

            // 1. hiasan latar, lalu pudar di sisi yang menempel layar (dock atas/bawah)
            let mut vis = 1.0;
            if !floating {
                let a_t = if fade_y > 0.0 { (1.0 - y / fade_y).clamp(0.0, 1.0) } else { 0.0 };
                let a_tl = (1.0 - x.hypot(y) / fade_x).clamp(0.0, 1.0);
                let a_tr = (1.0 - (wf - x).hypot(y) / fade_x).clamp(0.0, 1.0);
                vis = (1.0 - a_t) * (1.0 - a_tl) * (1.0 - a_tr);
            }
            for l in &td.layers {
                let (col, a) = layer_alpha(l, x, y, wf, hf, px);
                over(&mut p, col, a * vis);
            }

            // 2. bayangan dalam: cahaya (paling bawah), lalu bibir, lalu rim (paling atas)
            if let Some(g) = td.glow {
                let off = g.off * px;
                let spread = g.spread * px;
                let blur = (g.blur * px).max(1.0);
                // Inset shadow CSS: "lubang" = badan digeser ke atas `off` dan diperluas sebesar -spread
                // (spread negatif memperluas lubang, jadi cahaya hanya tersisa tipis di tepi bebas).
                // Piksel di luar lubang (s > 0) terwarnai, lalu dihaluskan dengan blur.
                let s = sdf(x, y + off, wf, hf, draw_r) + spread;
                over(&mut p, g.color, ((s + blur / 2.0) / blur).clamp(0.0, 1.0));
            }
            // lip: pita di sisi bebas; melayang: garis tepi penuh mengikuti lengkung badan (dihitung di atas)
            over(&mut p, lip_c, lip_rel);
            over(&mut p, rim_c, rim_rel);

            // 3. mask sudut bulat (antialias): alfa akhir = alfa lapisan x cakupan piksel terhadap badan.
            // (Sebelumnya cakupan ini saling membatalkan sehingga piksel tepi selalu penuh = tepi bertangga.)
            let pa = p[3];
            let a = (pa * inside).clamp(0.0, 1.0);
            let (oy, ox) = (if dock == Dock::Bottom { h - 1 - yy } else { yy }, xx);
            let o = ((oy * w + ox) * 4) as usize;
            if a > 0.0 {
                out[o] = (p[0] / pa * 255.0).round().clamp(0.0, 255.0) as u8;
                out[o + 1] = (p[1] / pa * 255.0).round().clamp(0.0, 255.0) as u8;
                out[o + 2] = (p[2] / pa * 255.0).round().clamp(0.0, 255.0) as u8;
                out[o + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    let _ = body; // warna dasar digambar oleh UI; disimpan di API untuk tema yang kelak butuh pencampuran
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Rgba = Rgba::rgb(10, 7, 20);
    const W: u32 = 300;
    const H: u32 = 120;

    fn alpha(img: &[u8], x: u32, y: u32) -> u8 {
        img[((y * W + x) * 4 + 3) as usize]
    }

    #[test]
    fn default_theme_has_no_decor() {
        assert!(render("default", BODY, Dock::Top, W, H, 28.0, 1.0).is_none());
        assert!(!has_decor("default"));
        assert!(has_decor("neon"));
    }

    #[test]
    fn nothing_leaks_outside_rounded_bottom_corners() {
        let img = render("neon", BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
        // pojok kiri-bawah dan kanan-bawah (di luar busur) harus transparan penuh
        for (x, y) in [(0, H - 1), (1, H - 2), (3, H - 4), (W - 1, H - 1), (W - 2, H - 2), (W - 4, H - 4)] {
            assert_eq!(alpha(&img, x, y), 0, "bocor di ({x},{y})");
        }
    }

    #[test]
    fn docked_top_corners_are_square_not_cut_by_mask() {
        // sakura punya isian di tepi atas. Titik (20, 0) sudah di luar zona pudar sudut (14 px) tetapi
        // masih di dalam lengkung 28 px: kalau mask membulatkan sudut atas, titik ini akan transparan.
        let img = render("sakura", BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
        assert!(alpha(&img, 20, 0) > 100, "pojok kiri-atas dipotong");
        assert!(alpha(&img, W - 21, 0) > 100, "pojok kanan-atas dipotong");
        // sebagai pembanding, dock melayang memang memotong titik yang sama
        let fl = render("sakura", BODY, Dock::Floating, W, H, 28.0, 1.0).unwrap();
        assert!(alpha(&fl, 20, 0) < 100);
    }

    #[test]
    fn floating_rounds_all_four_corners() {
        let img = render("neon", BODY, Dock::Floating, W, H, 28.0, 1.0).unwrap();
        for (x, y) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)] {
            assert_eq!(alpha(&img, x, y), 0, "({x},{y}) harus transparan");
        }
    }

    #[test]
    fn rim_only_on_free_side() {
        // dock atas: sisi bebas = bawah. Baris paling bawah punya garis, baris paling atas tidak.
        let img = render("neon", BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
        assert!(alpha(&img, W / 2, H - 1) > 100, "rim bawah harus tampak");
        let top_edge = alpha(&img, W / 2, 0);
        assert!(top_edge < 20, "tepi atas (menempel layar) tidak boleh bergaris, alfa={top_edge}");
        // tidak ada garis vertikal di sisi (rim hanya di bawah): tengah-sisi kiri nyaris transparan
        assert!(alpha(&img, 0, H / 3) < 60);
    }

    #[test]
    fn decor_fades_toward_docked_edge() {
        let img = render("neon", BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
        // cahaya menyala di bawah, memudar ke atas
        assert!(alpha(&img, 20, H - 20) > alpha(&img, 20, 2));
    }

    #[test]
    fn bottom_dock_is_vertical_mirror_of_top() {
        let top = render("aurora", BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
        let bot = render("aurora", BODY, Dock::Bottom, W, H, 28.0, 1.0).unwrap();
        for (x, y) in [(40, 5), (150, 60), (250, 100), (10, 110)] {
            let t = ((y * W + x) * 4) as usize;
            let b = (((H - 1 - y) * W + x) * 4) as usize;
            assert_eq!(top[t..t + 4], bot[b..b + 4]);
        }
    }

    #[test]
    fn tepi_lengkung_memudar_halus_bukan_bertangga() {
        // Piksel yang sebagian saja tercakup lengkungan harus beralfa sebagian dari tetangga di dalamnya
        // (antialias). Sebelum perbaikan mask, piksel tepi sama pekatnya dengan bagian dalam, sehingga
        // tepinya bertangga. Diukur di dock atas (tanpa rim di sisi lengkung dekat sisi tegak).
        let img = render("neon", BODY, Dock::Top, W, H, 32.0, 1.0).unwrap();
        let (cx, cy) = ((W - 32) as f32, (H - 32) as f32);
        let mut halus = 0;
        for k in 1..=40 {
            let th = (k as f32).to_radians(); // 1..40 derajat dari sisi tegak
            let edge = (cx + 32.0 * th.cos(), cy + 32.0 * th.sin());
            let inner = (cx + 27.0 * th.cos(), cy + 27.0 * th.sin());
            let (ae, ai) = (alpha(&img, edge.0 as u32, edge.1 as u32) as f32, alpha(&img, inner.0 as u32, inner.1 as u32) as f32);
            if ai > 0.0 && ae / ai > 0.15 && ae / ai < 0.85 {
                halus += 1;
            }
        }
        assert!(halus >= 6, "tepi lengkung harus memudar halus (antialias); piksel peralihan={halus}");
    }

    #[test]
    fn rim_menyusuri_lengkung_tanpa_putus() {
        // Menyusuri sudut kanan-bawah dari bawah ke sisi: tidak boleh ada piksel batas yang kosong
        // di tengah garis (sebelumnya garis terputus-putus).
        let img = render("neon", BODY, Dock::Floating, W, H, 28.0, 1.0).unwrap();
        // pusat busur kanan-bawah = (W - r, H - r); garis tepi 1 px berada di radius 27..28
        let (cx, cy) = ((W - 28) as f32, (H - 28) as f32);
        let mut gaps = 0;
        for k in 0..=45 {
            let th = (k as f32 * 2.0).to_radians(); // 0..90 derajat
            let (x, y) = (cx + 27.5 * th.cos(), cy + 27.5 * th.sin());
            if alpha(&img, x as u32, y as u32) < 40 {
                gaps += 1;
            }
        }
        assert_eq!(gaps, 0, "garis tepi melayang putus di {gaps} titik");
    }

    #[test]
    fn light_themes_render_too() {
        for t in ["sakura", "matcha", "obsidian", "aurora"] {
            let img = render(t, BODY, Dock::Top, W, H, 28.0, 1.0).unwrap();
            assert_eq!(img.len(), (W * H * 4) as usize);
            assert!(img.chunks(4).any(|p| p[3] > 0), "{t} kosong");
        }
    }
}
