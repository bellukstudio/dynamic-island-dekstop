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

#[cfg(test)]
mod tests {
    use super::convert_image;
    use std::path::Path;

    #[test]
    fn rejects_unsupported_target_before_accessing_the_file() {
        let error = convert_image(Path::new("does-not-need-to-exist.png"), "tiff").unwrap_err();
        assert_eq!(error, "Format tujuan tidak didukung: tiff");
    }

}

/// Convert DOCX, XLSX, or PPTX to PDF and save the result beside the source file.
pub fn convert_office_to_pdf(path: &Path) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !crate::officepdf::supports(&ext) {
        return Err(format!("Format Office tidak didukung: .{ext}"));
    }

    let bytes = crate::officepdf::convert_to_pdf(path)?;
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("document");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut output = parent.join(format!("{stem}_converted.pdf"));
    let mut index = 2u32;
    while output.exists() {
        output = parent.join(format!("{stem}_converted_{index}.pdf"));
        index += 1;
    }

    std::fs::write(&output, bytes).map_err(|error| format!("Gagal menyimpan PDF: {error}"))?;
    Ok(output)
}
