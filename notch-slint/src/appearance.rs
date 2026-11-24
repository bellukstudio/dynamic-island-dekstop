//! Tema dan aset visual: decode PNG tertanam, konversi warna, dan penerapan palet ke UI.

use crate::*;

/// Decode PNG kecil yang ditanam di binary menjadi Image Slint. Tanpa decoder gambar umum,
/// jadi binary dan RAM tetap kecil. Mengembalikan gambar kosong jika formatnya bukan RGBA8.
pub(crate) fn load_png(bytes: &'static [u8]) -> Image {
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

pub(crate) fn col(c: Rgba) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

pub(crate) fn apply_theme(w: &NotchWindow, p: &Palette) {
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
