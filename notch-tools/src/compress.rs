use image::{codecs::jpeg::JpegEncoder, ImageReader};
use std::io::Cursor;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct Profile {
    quality: u8,
    scale: f32,
}

const LADDER: [Profile; 14] = [
    Profile { quality: 92, scale: 1.0 },
    Profile { quality: 85, scale: 1.0 },
    Profile { quality: 78, scale: 1.0 },
    Profile { quality: 70, scale: 1.0 },
    Profile { quality: 62, scale: 1.0 },
    Profile { quality: 54, scale: 1.0 },
    Profile { quality: 46, scale: 1.0 },
    Profile { quality: 38, scale: 1.0 },
    Profile { quality: 50, scale: 0.8 },
    Profile { quality: 42, scale: 0.7 },
    Profile { quality: 40, scale: 0.6 },
    Profile { quality: 36, scale: 0.5 },
    Profile { quality: 32, scale: 0.4 },
    Profile { quality: 30, scale: 0.3 },
];

fn decode_image(src: &[u8]) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(Cursor::new(src))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Gagal membaca gambar: {error}"))?;
    decoded.apply_orientation(orientation);
    Ok(decoded)
}

fn scaled(image: &image::DynamicImage, scale: f32) -> image::DynamicImage {
    if scale >= 0.999 {
        return image.clone();
    }
    let width = ((image.width() as f32 * scale).round() as u32).max(1);
    let height = ((image.height() as f32 * scale).round() as u32).max(1);
    image.resize_exact(width, height, image::imageops::FilterType::Triangle)
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    use image::{ColorType, DynamicImage};
    let mut output = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut output, quality);
    let flattened = match image.color() {
        ColorType::L8 | ColorType::L16 | ColorType::La8 | ColorType::La16 => DynamicImage::ImageLuma8(image.to_luma8()),
        _ => DynamicImage::ImageRgb8(image.to_rgb8()),
    };
    flattened.write_with_encoder(encoder).map_err(|error| error.to_string())?;
    Ok(output)
}

fn encode_png(image: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::DynamicImage;
    let mut output = Vec::new();
    let encoder = PngEncoder::new_with_quality(&mut output, CompressionType::Best, FilterType::Adaptive);
    let flattened = if image.color().has_alpha() {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    };
    flattened.write_with_encoder(encoder).map_err(|error| error.to_string())?;
    Ok(output)
}

fn render(image: &image::DynamicImage, ext: &str, profile: Profile) -> Result<Vec<u8>, String> {
    let resized = scaled(image, profile.scale);
    if ext == "png" {
        encode_png(&resized)
    } else {
        encode_jpeg(&resized, profile.quality)
    }
}

fn unique_output(path: &Path, extension: &str) -> PathBuf {
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut output = parent.join(format!("{stem}_compressed.{extension}"));
    let mut index = 2u32;
    while output.exists() {
        output = parent.join(format!("{stem}_compressed_{index}.{extension}"));
        index += 1;
    }
    output
}

/// Compress JPEG, PNG, or BMP with a quality/resolution ladder.
///
/// `target_reduction_percent` is clamped to 1–95. If the requested reduction
/// cannot be reached, the strongest profile is still written and its actual size returned.
pub fn compress_image_file(path: &Path, target_reduction_percent: u8) -> Result<(PathBuf, u64, u64), String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp") {
        return Err(format!("Format kompresi tidak didukung: .{ext}. Gunakan JPG, PNG, atau BMP."));
    }

    let source = std::fs::read(path).map_err(|error| format!("Tidak bisa membaca file: {error}"))?;
    let original_size = source.len() as u64;
    if original_size == 0 {
        return Err("File kosong, tidak dapat dikompres.".into());
    }
    let image = decode_image(&source)?;
    let target_percent = target_reduction_percent.clamp(1, 95) as u64;
    let target_size = original_size * (100 - target_percent) / 100;

    let worst = render(&image, &ext, *LADDER.last().unwrap())?;
    let (chosen, output) = if worst.len() as u64 > target_size {
        (*LADDER.last().unwrap(), worst)
    } else {
        let (mut low, mut high) = (0usize, LADDER.len() - 1);
        while low < high {
            let middle = (low + high) / 2;
            let candidate = render(&image, &ext, LADDER[middle])?;
            if candidate.len() as u64 <= target_size {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        let profile = LADDER[low];
        (profile, render(&image, &ext, profile)?)
    };
    let _ = chosen;

    let output_extension = if ext == "bmp" { "jpg" } else { ext.as_str() };
    let output_path = unique_output(path, output_extension);
    let output_size = output.len() as u64;
    std::fs::write(&output_path, output).map_err(|error| format!("Gagal menyimpan hasil: {error}"))?;
    Ok((output_path, original_size, output_size))
}

/// Backward-compatible entry point used by the first Slint prototype.
pub fn compress_jpeg(path: &Path) -> Result<(PathBuf, u64, u64), String> {
    compress_image_file(path, 30)
}

#[cfg(test)]
mod tests {
    use super::{compress_image_file, compress_jpeg};
    use std::path::Path;

    #[test]
    fn rejects_unsupported_input_before_reading_file() {
        let error = compress_jpeg(Path::new("does-not-need-to-exist.webp")).unwrap_err();
        assert!(error.contains("Format kompresi tidak didukung"));
    }

    #[test]
    fn rejects_unsupported_input_for_general_compressor() {
        let error = compress_image_file(Path::new("missing.gif"), 30).unwrap_err();
        assert!(error.contains("JPG, PNG, atau BMP"));
    }
}
