use image::{codecs::jpeg::JpegEncoder, ImageReader};
use std::path::{Path, PathBuf};

/// Compress a JPEG image using the initial shared quality profile (72).
///
/// The output is written beside the input as `<stem>_compressed.jpg`.
pub fn compress_jpeg(path: &Path) -> Result<(PathBuf, u64, u64), String> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext != "jpg" && ext != "jpeg" {
        return Err("Versi awal mendukung file JPG/JPEG saja.".into());
    }

    let original_size = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();

    let image = ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| format!("Gagal membaca gambar: {error}"))?;

    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 72)
        .encode_image(&image.to_rgb8())
        .map_err(|error| format!("Gagal mengompres gambar: {error}"))?;

    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
    let output_path = path.with_file_name(format!("{stem}_compressed.jpg"));
    let output_size = output.len() as u64;

    std::fs::write(&output_path, output).map_err(|error| error.to_string())?;
    Ok((output_path, original_size, output_size))
}
