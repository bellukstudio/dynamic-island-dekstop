use image::{codecs::jpeg::JpegEncoder, ImageReader};
use std::path::{Path, PathBuf};

pub fn compress_jpeg(path: &Path) -> Result<(PathBuf, u64, u64), String> {
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" {
        return Err("Versi awal mendukung file JPG/JPEG saja.".into());
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let original_size = metadata.len();
    let image = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| format!("Gagal membaca gambar: {e}"))?;
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 72)
        .encode_image(&image.to_rgb8())
        .map_err(|e| format!("Gagal mengompres gambar: {e}"))?;
    let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("image");
    let output_path = path.with_file_name(format!("{stem}_compressed.jpg"));
    std::fs::write(&output_path, &output).map_err(|e| e.to_string())?;
    Ok((output_path, original_size, output.len() as u64))
}
