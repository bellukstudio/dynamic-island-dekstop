//! Galeri mata: semua skin (baris) x semua mood (kolom), di atas latar hitam, ke satu PNG.
//!
//!   cargo run --release --example eyes_gallery -- <berkas.png> [skala=2] [skin1,skin2,...]
//!
//! Berguna untuk memeriksa tampilan mata tanpa Windows/layar.

use std::{fs::File, io::BufWriter};

use notch_core::eyes::{skin_index, skin_names, Anim, EyeRenderer, Face, Mood};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "eyes.png".into());
    let sf: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let only: Option<Vec<String>> = args.next().map(|s| s.split(',').map(str::to_owned).collect());
    let skins: Vec<&str> = skin_names().filter(|n| only.as_ref().map_or(true, |o| o.iter().any(|x| x == n))).collect();

    let (cw, ch) = (120.0f32, 34.0f32);
    let cell = ((cw * sf) as usize, (ch * sf) as usize);
    let (cols, rows) = (Mood::ALL.len(), skins.len());
    let (w, h) = (cell.0 * cols, cell.1 * rows);
    let mut img = vec![0u8; w * h * 4];
    for px in img.chunks_exact_mut(4) {
        px[3] = 255; // latar hitam
    }
    for (r, name) in skins.iter().enumerate() {
        let renderer = EyeRenderer::new(name, cw, ch);
        for (c, m) in Mood::ALL.iter().enumerate() {
            let face = Face::for_mood(*m, skin_index(name));
            let anim = Anim { mood: *m, t: 1.8, t_mood: 1.0, look: (0.0, 0.0), blink: false };
            let buf = renderer.render(&face, &anim, sf);
            for y in 0..cell.1 {
                for x in 0..cell.0 {
                    let s = (y * cell.0 + x) * 4;
                    let d = ((r * cell.1 + y) * w + c * cell.0 + x) * 4;
                    let a = buf[s + 3] as u32;
                    for k in 0..3 {
                        img[d + k] = ((buf[s + k] as u32 * a + img[d + k] as u32 * (255 - a)) / 255) as u8;
                    }
                }
            }
        }
    }
    let file = BufWriter::new(File::create(&out).expect("tidak bisa membuat berkas"));
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
    println!("{out}: {w}x{h} ({} skin x {} mood)", rows, cols);
}
