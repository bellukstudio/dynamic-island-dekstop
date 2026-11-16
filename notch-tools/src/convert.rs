use image::{ImageFormat, ImageReader};
use std::path::{Path, PathBuf};

/// Convert a supported raster image and write the result beside the input.
pub fn convert_image(path: &Path, target: &str) -> Result<PathBuf, String> {
    let format = match target.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        "bmp" => ImageFormat::Bmp,
        "gif" => ImageFormat::Gif,
        other => return Err(format!("Format tujuan tidak didukung: {other}")),
    };

    let image = ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| format!("Gagal membaca gambar: {error}"))?;

    let extension = match format {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Gif => "gif",
        _ => unreachable!(),
    };

    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
    let output = path.with_file_name(format!("{stem}_converted.{extension}"));

    image
        .save_with_format(&output, format)
        .map_err(|error| format!("Gagal menyimpan gambar: {error}"))?;

    Ok(output)
}
