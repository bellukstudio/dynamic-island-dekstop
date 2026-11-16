use image::{codecs::jpeg::JpegEncoder, ImageFormat, ImageReader};
use std::path::{Path, PathBuf};

fn image_to_pdf_bytes(image: &image::DynamicImage) -> Result<Vec<u8>, String> {
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
    JpegEncoder::new_with_quality(&mut jpeg, 85)
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
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    if target == "pdf" {
        let image = decode_image(path)?;
        let bytes = image_to_pdf_bytes(&image)?;
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
            JpegEncoder::new_with_quality(&mut bytes, 85)
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

/// Convert a DOCX, XLSX, or PPTX to PDF and save the result beside the source file.
pub fn convert_office_to_pdf(path: &Path) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !crate::officepdf::supports(&ext) {
        return Err(format!("Format Office tidak didukung: .{ext}"));
    }
    let bytes = crate::officepdf::convert_to_pdf(path)?;
    write_converted(path, "pdf", &bytes)
}

/// Convert an image or supported Office document to PDF.
pub fn convert_to_pdf(path: &Path) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if crate::officepdf::supports(&ext) {
        convert_office_to_pdf(path)
    } else {
        convert_image(path, "pdf")
    }
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

    #[test]
    fn creates_a_valid_pdf_header_from_a_small_image() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255])));
        let bytes = super::image_to_pdf_bytes(&image).unwrap();
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
