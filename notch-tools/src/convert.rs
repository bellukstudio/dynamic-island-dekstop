use image::{codecs::jpeg::JpegEncoder, ImageFormat, ImageReader};
use std::path::{Path, PathBuf};

fn image_to_pdf_bytes(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    use lopdf::Object;

    let flat = if image.color().has_alpha() {
        let rgba = image.to_rgba8();
        let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
        for (x, y, pixel) in rgba.enumerate_pixels() {
            let alpha = pixel[3] as u32;
            let blend = |channel: u8| {
                ((channel as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            };
            rgb.put_pixel(x, y, image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]));
        }
        image::DynamicImage::ImageRgb8(rgb)
    } else {
        image::DynamicImage::ImageRgb8(image.to_rgb8())
    };

    let (width, height) = (flat.width(), flat.height());
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, quality.clamp(40, 100))
        .encode_image(&flat)
        .map_err(|error| format!("Gagal mengodekan gambar untuk PDF: {error}"))?;

    let (page_width, page_height) = if width >= height { (842i64, 595i64) } else { (595i64, 842i64) };
    let margin = 24.0f32;
    let scale = ((page_width as f32 - 2.0 * margin) / width as f32)
        .min((page_height as f32 - 2.0 * margin) / height as f32);
    let (draw_width, draw_height) = (width as f32 * scale, height as f32 * scale);
    let (x, y) = ((page_width as f32 - draw_width) / 2.0, (page_height as f32 - draw_height) / 2.0);

    let name = |value: &str| Object::Name(value.as_bytes().to_vec());
    let dictionary = |items: Vec<(&str, Object)>| {
        let mut value = lopdf::Dictionary::new();
        for (key, object) in items {
            value.set(key, object);
        }
        value
    };

    let mut document = lopdf::Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let image_id = document.add_object(lopdf::Stream::new(
        dictionary(vec![
            ("Type", name("XObject")),
            ("Subtype", name("Image")),
            ("Width", Object::Integer(width as i64)),
            ("Height", Object::Integer(height as i64)),
            ("ColorSpace", name("DeviceRGB")),
            ("BitsPerComponent", Object::Integer(8)),
            ("Filter", name("DCTDecode")),
        ]),
        jpeg,
    ));
    let content = format!("q {draw_width:.2} 0 0 {draw_height:.2} {x:.2} {y:.2} cm /Im0 Do Q");
    let content_id = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), content.into_bytes()));
    let xobjects = dictionary(vec![("Im0", Object::Reference(image_id))]);
    let resources = dictionary(vec![("XObject", Object::Dictionary(xobjects))]);
    let page_id = document.add_object(dictionary(vec![
        ("Type", name("Page")),
        ("Parent", Object::Reference(pages_id)),
        ("MediaBox", Object::Array(vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(page_width), Object::Integer(page_height),
        ])),
        ("Contents", Object::Reference(content_id)),
        ("Resources", Object::Dictionary(resources)),
    ]));
    document.objects.insert(pages_id, Object::Dictionary(dictionary(vec![
        ("Type", name("Pages")),
        ("Kids", Object::Array(vec![Object::Reference(page_id)])),
        ("Count", Object::Integer(1)),
    ])));
    let catalog_id = document.add_object(dictionary(vec![
        ("Type", name("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    document.trailer.set("Root", Object::Reference(catalog_id));
    document.compress();

    let mut output = Vec::new();
    document.save_to(&mut output).map_err(|error| format!("Gagal membuat PDF: {error}"))?;
    Ok(output)
}

/// Convert a supported raster image and write the result beside the input.
pub fn convert_image(path: &Path, target: &str) -> Result<PathBuf, String> {
    convert_image_with_quality(path, target, 85)
}

/// Convert a raster image with an explicit JPEG/PDF image quality setting.
pub fn convert_image_with_quality(
    path: &Path,
    target: &str,
    quality: u8,
) -> Result<PathBuf, String> {
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    let quality = quality.clamp(40, 100);
    if target == "pdf" {
        let image = decode_image(path)?;
        let bytes = image_to_pdf_bytes(&image, quality)?;
        return write_converted(path, "pdf", &bytes);
    }

    let format = match target.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        "bmp" => ImageFormat::Bmp,
        "gif" => ImageFormat::Gif,
        other => return Err(format!("Format tujuan tidak didukung: {other}")),
    };

    let image = decode_image(path)?;

    let extension = match format {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Gif => "gif",
        _ => unreachable!(),
    };

    let encoded = match format {
        ImageFormat::Jpeg => {
            let flattened = flatten_white(&image);
            let mut bytes = Vec::new();
            JpegEncoder::new_with_quality(&mut bytes, quality)
                .encode_image(&flattened)
                .map_err(|error| format!("Gagal mengodekan JPEG: {error}"))?;
            bytes
        }
        ImageFormat::Png => encode_image(&image, ImageFormat::Png)?,
        ImageFormat::WebP => encode_image(&to_8bit(&image), ImageFormat::WebP)?,
        ImageFormat::Bmp => encode_image(&flatten_white(&image), ImageFormat::Bmp)?,
        ImageFormat::Gif => encode_image(
            &image::DynamicImage::ImageRgba8(image.to_rgba8()),
            ImageFormat::Gif,
        )?,
        _ => unreachable!(),
    };
    write_converted(path, extension, &encoded)
}

fn flatten_white(image: &image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    if !image.color().has_alpha() {
        return DynamicImage::ImageRgb8(image.to_rgb8());
    }
    let rgba = image.to_rgba8();
    let mut output = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as u32;
        let blend = |channel: u8| {
            ((channel as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8
        };
        output.put_pixel(x, y, image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]));
    }
    DynamicImage::ImageRgb8(output)
}

fn to_8bit(image: &image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    if image.color().has_alpha() {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    }
}

fn encode_image(image: &image::DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, format)
        .map_err(|error| format!("Gagal mengodekan gambar: {error}"))?;
    Ok(output.into_inner())
}

fn decode_image(path: &Path) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;

    let reader = ImageReader::open(path)
        .map_err(|error| format!("Gagal membuka gambar: {error}"))?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut decoder = reader.into_decoder()
        .map_err(|error| format!("Gagal membaca gambar: {error}"))?;
    let orientation = decoder.orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Gagal membaca gambar: {error}"))?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn write_converted(source: &Path, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or("file");
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let mut output = parent.join(format!("{stem}_converted.{extension}"));
    let mut index = 2u32;
    while output.exists() {
        output = parent.join(format!("{stem}_converted_{index}.{extension}"));
        index += 1;
    }
    std::fs::write(&output, bytes).map_err(|error| format!("Gagal menyimpan hasil: {error}"))?;
    Ok(output)
}

// ---------------------------------------------------------------------------------------------
// LibreOffice fallback untuk format Office lama/lain dan dokumen yang gagal di mesin bawaan.
// Implementasi ini UI-independent agar Tauri dan Slint memakai perilaku konversi yang sama.
// ---------------------------------------------------------------------------------------------
const OFFICE_EXTENSIONS: [&str; 11] = [
    "docx", "xlsx", "pptx", "doc", "xls", "ppt", "odt", "ods", "odp", "rtf", "txt",
];
// CSV termasuk format Office yang dapat dibuka LibreOffice.
fn is_office_extension(ext: &str) -> bool {
    OFFICE_EXTENSIONS.contains(&ext) || ext == "csv"
}

fn find_soffice() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SOFFICE_PATH") {
        candidates.push(PathBuf::from(path));
    }

    if cfg!(target_os = "windows") {
        for root in ["C:\\Program Files\\LibreOffice", "C:\\Program Files (x86)\\LibreOffice"] {
            candidates.push(Path::new(root).join("program").join("soffice.exe"));
        }
    } else {
        for path in [
            "/usr/bin/soffice",
            "/usr/bin/libreoffice",
            "/usr/local/bin/soffice",
            "/snap/bin/libreoffice",
            "/opt/libreoffice/program/soffice",
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        ] {
            candidates.push(PathBuf::from(path));
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            for name in ["soffice.exe", "soffice", "libreoffice"] {
                candidates.push(directory.join(name));
            }
        }
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn soffice() -> Option<&'static PathBuf> {
    static SOFFICE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    SOFFICE.get_or_init(find_soffice).as_ref()
}

fn file_url(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    if value.starts_with('/') {
        format!("file://{value}")
    } else {
        format!("file:///{value}")
    }
}

fn configure_no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn office_to_pdf_with_libreoffice(input: &Path) -> Result<Vec<u8>, String> {
    let executable = soffice().ok_or_else(|| "LibreOffice tidak ditemukan".to_string())?;
    static TEMP_SEQUENCE: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let sequence = TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let temp = std::env::temp_dir();
    let job_id = format!("{}-{sequence}", std::process::id());
    let output_dir = temp.join(format!("notch-convert-{job_id}"));
    let profile = temp.join(format!("notch-lo-profile-{job_id}"));
    std::fs::create_dir_all(&output_dir)
        .map_err(|error| format!("Gagal membuat folder sementara: {error}"))?;

    let result = (|| {
        let mut command = std::process::Command::new(executable);
        if std::env::var_os("APPDIR").is_some() {
            command.env_remove("LD_LIBRARY_PATH");
        }
        let output = configure_no_window(&mut command)
            .arg(format!("-env:UserInstallation={}", file_url(&profile)))
            .args([
                "--headless",
                "--norestore",
                "--nolockcheck",
                "--convert-to",
                "pdf",
                "--outdir",
            ])
            .arg(&output_dir)
            .arg(input)
            .output()
            .map_err(|error| format!("Gagal menjalankan LibreOffice: {error}"))?;

        let pdf_path = std::fs::read_dir(&output_dir)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("pdf")));

        match pdf_path {
            Some(path) => std::fs::read(&path)
                .map_err(|error| format!("Hasil LibreOffice tidak terbaca: {error}")),
            None => {
                let message = String::from_utf8_lossy(&output.stderr);
                let message = message.lines().rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("tidak ada keluaran PDF");
                Err(format!("LibreOffice gagal: {message}"))
            }
        }
    })();

    let _ = std::fs::remove_dir_all(&output_dir);
    let _ = std::fs::remove_dir_all(&profile);
    result
}

fn office_to_pdf_bytes(path: &Path, extension: &str) -> Result<Vec<u8>, String> {
    if crate::officepdf::supports(extension) {
        match crate::officepdf::convert_to_pdf(path) {
            Ok(bytes) => return Ok(bytes),
            Err(native_error) => {
                if soffice().is_none() {
                    return Err(native_error);
                }
                eprintln!(
                    "convert: mesin Office bawaan gagal ({native_error}), mencoba LibreOffice"
                );
            }
        }
    }
    office_to_pdf_with_libreoffice(path)
}

/// Convert a DOCX, XLSX, or PPTX to PDF and save the result beside the source file.
pub fn convert_office_to_pdf(path: &Path) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !is_office_extension(&ext) {
        return Err(format!("Format Office tidak didukung: .{ext}"));
    }
    let bytes = office_to_pdf_bytes(path, &ext)?;
    write_converted(path, "pdf", &bytes)
}

/// Convert each page of a PDF to PNG or JPEG. Multi-page output is written to a new folder.
pub fn convert_pdf_to_images(
    path: &Path,
    target: &str,
    dpi: u32,
    quality: u8,
) -> Result<(PathBuf, usize, u64), String> {
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    let format = match target.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        other => return Err(format!("Format tujuan PDF tidak didukung: {other}. Gunakan PNG atau JPG.")),
    };
    let source = std::fs::read(path).map_err(|error| format!("Tidak bisa membaca PDF: {error}"))?;
    if source.is_empty() {
        return Err("File PDF kosong".into());
    }
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("document");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let extension = if format == ImageFormat::Jpeg { "jpg" } else { "png" };
    let quality = quality.clamp(40, 100);
    let mut output_dir: Option<PathBuf> = None;
    let mut first_output: Option<PathBuf> = None;
    let mut total_size = 0u64;

    let rendered = crate::pdf::for_each_page(&source, dpi, |index, total, image| {
        let bytes = if format == ImageFormat::Jpeg {
            let flattened = flatten_white(&image);
            let mut output = Vec::new();
            JpegEncoder::new_with_quality(&mut output, quality)
                .encode_image(&flattened)
                .map_err(|error| format!("Gagal mengodekan halaman {}: {error}", index + 1))?;
            output
        } else {
            encode_image(&image, ImageFormat::Png)?
        };
        let output_path = if total == 1 {
            unique_converted_path(path, extension)
        } else {
            if output_dir.is_none() {
                let mut candidate = parent.join(format!("{stem}_converted_pages"));
                let mut suffix = 2u32;
                while candidate.exists() {
                    candidate = parent.join(format!("{stem}_converted_pages_{suffix}"));
                    suffix += 1;
                }
                std::fs::create_dir_all(&candidate)
                    .map_err(|error| format!("Gagal membuat folder hasil: {error}"))?;
                output_dir = Some(candidate);
            }
            output_dir.as_ref().unwrap().join(format!("{stem}-{:03}.{extension}", index + 1))
        };
        std::fs::write(&output_path, &bytes)
            .map_err(|error| format!("Gagal menyimpan halaman {}: {error}", index + 1))?;
        total_size += bytes.len() as u64;
        if first_output.is_none() {
            first_output = Some(output_path);
        }
        Ok(())
    });

    let count = match rendered {
        Ok(count) => count,
        Err(error) => {
            if let Some(directory) = output_dir {
                let _ = std::fs::remove_dir_all(directory);
            }
            return Err(error);
        }
    };
    let output = output_dir.or(first_output)
        .ok_or_else(|| "PDF tidak punya halaman".to_string())?;
    Ok((output, count, total_size))
}

fn unique_converted_path(source: &Path, extension: &str) -> PathBuf {
    let stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or("file");
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let mut candidate = parent.join(format!("{stem}_converted.{extension}"));
    let mut suffix = 2u32;
    while candidate.exists() {
        candidate = parent.join(format!("{stem}_converted_{suffix}.{extension}"));
        suffix += 1;
    }
    candidate
}

/// Convert an image or supported Office document to PDF.
pub fn convert_to_pdf(path: &Path) -> Result<PathBuf, String> {
    convert_to_pdf_with_quality(path, 85)
}

/// Convert an image or Office document to PDF, applying quality to raster-image output.
pub fn convert_to_pdf_with_quality(path: &Path, quality: u8) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if is_office_extension(&ext) {
        convert_office_to_pdf(path)
    } else {
        convert_image_with_quality(path, "pdf", quality)
    }
}

#[cfg(test)]
mod office_format_tests {
    use super::is_office_extension;

    #[test]
    fn recognizes_modern_and_legacy_office_formats() {
        for extension in [
            "docx", "xlsx", "pptx", "doc", "xls", "ppt",
            "odt", "ods", "odp", "rtf", "txt", "csv",
        ] {
            assert!(is_office_extension(extension), "{extension}");
        }
    }

    #[test]
    fn does_not_treat_unrelated_extensions_as_office_documents() {
        for extension in ["pdf", "png", "jpg", "webp", "exe", ""] {
            assert!(!is_office_extension(extension), "{extension}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::convert_image;
    use std::path::Path;

    #[test]
    fn rejects_pdf_targets_other_than_png_and_jpg_before_reading() {
        let error = super::convert_pdf_to_images(
            Path::new("missing.pdf"),
            "webp",
            150,
            85,
        ).unwrap_err();
        assert!(error.contains("Gunakan PNG atau JPG"));
    }

    #[test]
    fn rejects_unsupported_target_before_accessing_the_file() {
        let error = convert_image(Path::new("does-not-need-to-exist.png"), "tiff").unwrap_err();
        assert_eq!(error, "Format tujuan tidak didukung: tiff");
    }

    #[test]
    fn creates_a_valid_pdf_header_from_a_small_image() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255])));
        let bytes = super::image_to_pdf_bytes(&image, 85).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn converts_transparent_png_to_jpeg_and_keeps_both_outputs() {
        let directory = std::env::temp_dir().join(format!("notch-convert-image-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("transparent.png");
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8, 8, image::Rgba([200, 40, 20, 0]),
        ));
        image.save(&source).unwrap();

        let first = super::convert_image(&source, "jpg").unwrap();
        let second = super::convert_image(&source, "jpg").unwrap();
        assert_ne!(first, second);
        assert!(std::fs::read(&first).unwrap().starts_with(&[0xff, 0xd8]));
        assert!(std::fs::read(&second).unwrap().starts_with(&[0xff, 0xd8]));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn converted_outputs_get_unique_names_instead_of_overwriting() {
        let directory = std::env::temp_dir().join(format!("notch-convert-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("sample.png");
        std::fs::write(&source, b"source").unwrap();

        let first = super::write_converted(&source, "jpg", b"first").unwrap();
        let second = super::write_converted(&source, "jpg", b"second").unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");

        let _ = std::fs::remove_dir_all(directory);
    }
}
