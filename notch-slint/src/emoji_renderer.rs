//! Runtime color emoji renderer for the Slint software renderer.
//!
//! Uses the installed system emoji font (Segoe UI Emoji on Windows) and Swash's
//! OpenType shaper + COLR/CPAL/CBDT renderer. The original Unicode string is
//! never replaced: only its visual representation is rasterized and cached.

use std::collections::HashMap;
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use fontdb::Database;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use swash::{
    scale::{
        image::Content,
        Render, ScaleContext, Source, StrikeWith,
    },
    shape::ShapeContext,
    text::Script,
    FontRef,
};

const MAX_EMOJI_BYTES: usize = 128;
const MAX_EMOJI_CHARS: usize = 32;
const MAX_CACHE_ENTRIES: usize = 256;

static FONT_DATA: OnceLock<Option<(Arc<Vec<u8>>, usize)>> = OnceLock::new();
static IMAGE_CACHE: OnceLock<Mutex<HashMap<(String, u32), Option<CachedEmoji>>>> =
    OnceLock::new();

#[derive(Clone)]
struct CachedEmoji {
    size: u32,
    rgba: Arc<Vec<u8>>,
}

impl CachedEmoji {
    /// Convert cached bytes into a Slint Image on the caller's UI thread.
    fn to_slint_image(&self) -> Image {
        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            self.rgba.as_slice(),
            self.size,
            self.size,
        );
        Image::from_rgba8(buffer)
    }
}

#[derive(Debug)]
struct RasterGlyph {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[cfg(windows)]
fn windows_emoji_font() -> Option<(Arc<Vec<u8>>, usize)> {
    let windows_dir = std::env::var_os("WINDIR")?;
    let font_path = PathBuf::from(windows_dir).join("Fonts").join("seguiemj.ttf");
    let font_data = Arc::new(std::fs::read(font_path).ok()?);
    FontRef::from_index(font_data.as_slice(), 0)?;
    Some((font_data, 0))
}

fn system_emoji_font() -> Option<(Arc<Vec<u8>>, usize)> {
    FONT_DATA
        .get_or_init(|| {
            // Avoid scanning every installed font on Windows when the standard
            // Segoe UI Emoji font is present. Keep fontdb as a cross-platform fallback.
            #[cfg(windows)]
            if let Some(font) = windows_emoji_font() {
                return Some(font);
            }

            let mut database = Database::new();
            database.load_system_fonts();

            // Prefer Microsoft's color emoji font on Windows. The remaining names
            // make the same renderer usable on macOS and Linux development builds.
            let preferred = [
                "Segoe UI Emoji",
                "Noto Color Emoji",
                "Apple Color Emoji",
            ];

            let faces: Vec<_> = database.faces().collect();
            let selected = preferred
                .iter()
                .find_map(|wanted| {
                    faces.iter().find(|face| {
                        face.families
                            .iter()
                            .any(|(family, _)| family.eq_ignore_ascii_case(wanted))
                    })
                })
                .or_else(|| {
                    faces.iter().find(|face| {
                        face.families
                            .iter()
                            .any(|(family, _)| family.to_ascii_lowercase().contains("emoji"))
                    })
                })?;

            database.with_face_data(selected.id, |bytes, index| {
                (Arc::new(bytes.to_vec()), index as usize)
            })
        })
        .clone()
}

fn blend_over_premultiplied(destination: &mut [u8], source: [u8; 4]) {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    if source_alpha == 255 {
        destination.copy_from_slice(&source);
        return;
    }

    let inverse = 255 - source_alpha;
    for channel in 0..3 {
        destination[channel] = (u32::from(source[channel])
            + (u32::from(destination[channel]) * inverse + 127) / 255)
            .min(255) as u8;
    }
    destination[3] = (source_alpha
        + (u32::from(destination[3]) * inverse + 127) / 255)
        .min(255) as u8;
}

fn premultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for channel in 0..3 {
            pixel[channel] =
                ((u32::from(pixel[channel]) * alpha + 127) / 255).min(255) as u8;
        }
    }
}

fn unpremultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        for channel in 0..3 {
            pixel[channel] =
                ((u32::from(pixel[channel]) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

fn render_uncached(text: &str, target_size: u32) -> Option<CachedEmoji> {
    let (font_data, font_index) = system_emoji_font()?;
    let font = FontRef::from_index(font_data.as_slice(), font_index)?;

    // Shape the complete sequence so skin tones, flags, keycaps and ZWJ
    // combinations can become one ligature/color glyph where the font supports it.
    let mut shape_context = ShapeContext::new();
    let mut shaper = shape_context
        .builder(font)
        .script(Script::Common)
        .size((target_size as f32 * 0.88).max(24.0))
        .build();
    shaper.add_str(text);

    let mut shaped_glyphs = Vec::new();
    let mut has_missing_glyph = false;
    shaper.shape_with(|cluster| {
        for glyph in cluster.glyphs {
            if glyph.id == 0 {
                has_missing_glyph = true;
            } else {
                shaped_glyphs.push((glyph.id, glyph.x, glyph.y));
            }
        }
    });
    // Jangan menampilkan pecahan glyph saat font tidak mendukung satu bagian sequence
    // keluarga/ZWJ/skin-tone. Fallback teks akan lebih jujur daripada urutan yang rusak.
    if has_missing_glyph || shaped_glyphs.is_empty() {
        return None;
    }

    let render_size = (target_size as f32 * 0.88).max(24.0);
    let mut scale_context = ScaleContext::new();
    let mut scaler = scale_context
        .builder(font)
        .size(render_size)
        .hint(true)
        .build();
    let renderer = Render::new(&[
        // Keep color layers/bitmaps first. The outline fallback still gives a visible
        // glyph on systems whose emoji font lacks a color representation for a codepoint.
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ]);

    let mut rasters = Vec::with_capacity(shaped_glyphs.len());
    for (glyph_id, glyph_x, glyph_y) in shaped_glyphs {
        let Some(rendered) = renderer.render(&mut scaler, glyph_id) else {
            continue;
        };
        let width = rendered.placement.width;
        let height = rendered.placement.height;
        if width == 0 || height == 0 || width > 512 || height > 512 {
            continue;
        }

        let pixels = (width as usize).checked_mul(height as usize)?;
        let mut rgba = vec![0u8; pixels.checked_mul(4)?];

        match rendered.content {
            Content::Color => {
                if rendered.data.len() < rgba.len() {
                    continue;
                }
                let copy_len = rgba.len();
                rgba.copy_from_slice(&rendered.data[..copy_len]);
                // Swash emits COLR layered outlines as premultiplied RGBA, while
                // embedded color bitmap strikes (CBDT/sbix) are straight RGBA.
                if matches!(rendered.source, Source::ColorBitmap(_)) {
                    premultiply_rgba(&mut rgba);
                }
            }
            Content::Mask => {
                if rendered.data.len() < pixels {
                    continue;
                }
                for (index, alpha) in rendered.data.iter().take(pixels).enumerate() {
                    let offset = index * 4;
                    let alpha = u32::from(*alpha);
                    rgba[offset] = ((255 * alpha + 127) / 255) as u8;
                    rgba[offset + 1] = ((218 * alpha + 127) / 255) as u8;
                    rgba[offset + 2] = ((92 * alpha + 127) / 255) as u8;
                    rgba[offset + 3] = alpha as u8;
                }
            }
            Content::SubpixelMask => {
                if rendered.data.len() < rgba.len() {
                    continue;
                }
                for (index, pixel) in rendered.data.chunks_exact(4).take(pixels).enumerate() {
                    let offset = index * 4;
                    let alpha = u32::from(pixel[0].max(pixel[1]).max(pixel[2]).max(pixel[3]));
                    rgba[offset] = ((255 * alpha + 127) / 255) as u8;
                    rgba[offset + 1] = ((218 * alpha + 127) / 255) as u8;
                    rgba[offset + 2] = ((92 * alpha + 127) / 255) as u8;
                    rgba[offset + 3] = alpha as u8;
                }
            }
        }

        // Swash placement.top is a font-coordinate distance above the baseline;
        // screen coordinates grow downward, so its screen Y offset is negative.
        rasters.push(RasterGlyph {
            left: glyph_x.round() as i32 + rendered.placement.left,
            top: -(glyph_y.round() as i32) - rendered.placement.top,
            width,
            height,
            rgba,
        });
    }

    if rasters.is_empty() {
        return None;
    }

    let min_x = rasters.iter().map(|glyph| glyph.left).min()?;
    let min_y = rasters.iter().map(|glyph| glyph.top).min()?;
    let max_x = rasters
        .iter()
        .map(|glyph| glyph.left + glyph.width as i32)
        .max()?;
    let max_y = rasters
        .iter()
        .map(|glyph| glyph.top + glyph.height as i32)
        .max()?;

    let content_width = (max_x - min_x).max(1) as u32;
    let content_height = (max_y - min_y).max(1) as u32;
    if content_width > 512 || content_height > 512 {
        return None;
    }

    let padding = 2u32;
    let canvas_width = content_width + padding * 2;
    let canvas_height = content_height + padding * 2;
    let canvas_len = (canvas_width as usize)
        .checked_mul(canvas_height as usize)?
        .checked_mul(4)?;
    let mut canvas = vec![0u8; canvas_len];

    for glyph in rasters {
        let origin_x = (glyph.left - min_x) as u32 + padding;
        let origin_y = (glyph.top - min_y) as u32 + padding;
        for y in 0..glyph.height {
            for x in 0..glyph.width {
                let source_offset = ((y * glyph.width + x) * 4) as usize;
                let destination_offset =
                    (((origin_y + y) * canvas_width + origin_x + x) * 4) as usize;
                let source = [
                    glyph.rgba[source_offset],
                    glyph.rgba[source_offset + 1],
                    glyph.rgba[source_offset + 2],
                    glyph.rgba[source_offset + 3],
                ];
                blend_over_premultiplied(
                    &mut canvas[destination_offset..destination_offset + 4],
                    source,
                );
            }
        }
    }

    // Normalize the result into a square buffer. Slint then scales the same RGBA image
    // consistently for the settings field, notch and speech bubble.
    let output_size = target_size.clamp(24, 128);
    let max_content = output_size.saturating_sub(4).max(1);
    let fit_scale = (max_content as f32 / canvas_width as f32)
        .min(max_content as f32 / canvas_height as f32)
        .min(1.0);
    let resized_width = ((canvas_width as f32 * fit_scale).round() as u32).max(1);
    let resized_height = ((canvas_height as f32 * fit_scale).round() as u32).max(1);

    let source_image =
        ::image::RgbaImage::from_raw(canvas_width, canvas_height, canvas)?;
    let resized = ::image::imageops::resize(
        &source_image,
        resized_width,
        resized_height,
        ::image::imageops::FilterType::Lanczos3,
    );

    let mut output = vec![0u8; (output_size * output_size * 4) as usize];
    let offset_x = (output_size - resized_width) / 2;
    let offset_y = (output_size - resized_height) / 2;
    for y in 0..resized_height {
        let source_start = (y * resized_width * 4) as usize;
        let destination_start =
            (((offset_y + y) * output_size + offset_x) * 4) as usize;
        let row_len = (resized_width * 4) as usize;
        output[destination_start..destination_start + row_len]
            .copy_from_slice(&resized.as_raw()[source_start..source_start + row_len]);
    }

    // The canvas was composited in premultiplied form. Slint's stable from_rgba8
    // constructor expects straight RGBA, so convert once after resizing.
    unpremultiply_rgba(&mut output);
    Some(CachedEmoji {
        size: output_size,
        rgba: Arc::new(output),
    })
}

/// Render one emoji (or a Unicode emoji sequence) using the installed color emoji font.
/// Successful images are cached by the exact Unicode string and requested pixel size.
pub(crate) fn render_emoji(text: &str, target_size: u32) -> Option<Image> {
    if text.trim().is_empty()
        || text.len() > MAX_EMOJI_BYTES
        || text.chars().count() > MAX_EMOJI_CHARS
        || text.chars().any(char::is_control)
    {
        return None;
    }

    let key = (text.to_string(), target_size);
    let cache = IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = cache.lock().ok().and_then(|cache| cache.get(&key).cloned());

    let rendered = if let Some(cached) = cached {
        cached
    } else {
        let rendered = render_uncached(text, target_size);
        if let Ok(mut cache) = cache.lock() {
            if cache.len() >= MAX_CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(key, rendered.clone());
        }
        rendered
    };

    rendered.map(|cached| cached.to_slint_image())
}
