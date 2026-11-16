use image::{codecs::jpeg::JpegEncoder, ImageReader};
use std::io::{Cursor, Read, Write};
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
    let output = if worst.len() as u64 > target_size {
        worst
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
        render(&image, &ext, profile)?
    };
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
    fn ghostscript_profiles_keep_quality_order() {
        assert_eq!(super::GS_LADDER.first().unwrap().dpi, 300);
        assert_eq!(super::GS_LADDER.last().unwrap().dpi, 50);
        assert!(super::GS_LADDER.windows(2).all(|pair| pair[0].dpi > pair[1].dpi));
        assert!(super::GS_LADDER.windows(2).all(|pair| pair[0].qfactor < pair[1].qfactor));
    }

    #[test]
    fn raster_profiles_progress_from_readable_to_aggressive() {
        assert_eq!(super::RASTER_LADDER.first().unwrap().dpi, 110);
        assert_eq!(super::RASTER_LADDER.last().unwrap().dpi, 30);
        assert!(super::RASTER_LADDER.windows(2).all(|pair| pair[0].dpi > pair[1].dpi));
        assert!(super::RASTER_LADDER.windows(2).all(|pair| pair[0].quality > pair[1].quality));
    }

    #[test]
    fn rejects_malformed_pdf_without_creating_an_output() {
        let path = std::env::temp_dir().join(format!("notch-tools-invalid-{}.pdf", std::process::id()));
        std::fs::write(&path, b"not a PDF").unwrap();
        let error = super::compress_pdf_file(&path, 30).unwrap_err();
        assert!(error.contains("PDF"));
        let _ = std::fs::remove_file(path);
    }

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

    #[test]
    fn compresses_png_and_writes_a_distinct_output_file() {
        let source = std::env::temp_dir().join(format!("notch-tools-test-{}.png", std::process::id()));
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            64, 64, image::Rgba([30, 120, 220, 180]),
        ));
        image.save(&source).unwrap();

        let (output, before, after) = compress_image_file(&source, 30).unwrap();
        assert!(output.exists());
        assert_eq!(output.extension().and_then(|value| value.to_str()), Some("png"));
        assert!(before > 0);
        assert!(after > 0);

        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn compresses_office_zip_and_keeps_its_package_entries() {
        use std::io::Write;

        let source = std::env::temp_dir().join(format!("notch-tools-test-{}.docx", std::process::id()));
        let file = std::fs::File::create(&source).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(&vec![b'x'; 4096]).unwrap();
        archive.finish().unwrap();

        let (output, before, after) = super::compress_office_file(&source, 30).unwrap();
        let file = std::fs::File::open(&output).unwrap();
        let mut result = zip::ZipArchive::new(file).unwrap();
        assert!(result.by_name("word/document.xml").is_ok());
        assert!(before > 0);
        assert!(after > 0);
        drop(result);

        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(output);
    }
}


fn recode_embedded_jpeg(source: &[u8], profile: Profile) -> Option<Vec<u8>> {
    let image = scaled(&decode_image(source).ok()?, profile.scale);
    let output = encode_jpeg(&image, profile.quality).ok()?;
    (output.len() < source.len()).then_some(output)
}

fn recode_embedded_png(source: &[u8], profile: Profile) -> Option<Vec<u8>> {
    let image = scaled(&decode_image(source).ok()?, profile.scale);
    let output = encode_png(&image).ok()?;
    (output.len() < source.len()).then_some(output)
}

fn compress_office_bytes(source: &[u8], profile: Profile, fast: bool) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(source))
        .map_err(|error| format!("Paket Office tidak valid: {error}"))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(if fast { 6 } else { 9 }));

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            writer.add_directory(name, options).map_err(|error| error.to_string())?;
            continue;
        }

        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data).map_err(|error| error.to_string())?;
        let lower_name = name.to_ascii_lowercase();
        if lower_name.contains("/media/") {
            if lower_name.ends_with(".jpg") || lower_name.ends_with(".jpeg") {
                if let Some(reencoded) = recode_embedded_jpeg(&data, profile) {
                    data = reencoded;
                }
            } else if lower_name.ends_with(".png") {
                if let Some(reencoded) = recode_embedded_png(&data, profile) {
                    data = reencoded;
                }
            }
        }

        writer.start_file(name, options).map_err(|error| error.to_string())?;
        writer.write_all(&data).map_err(|error| error.to_string())?;
    }

    writer.finish()
        .map_err(|error| error.to_string())
        .map(|cursor| cursor.into_inner())
}

/// Compress DOCX, XLSX, or PPTX packages, including embedded JPEG/PNG media.
pub fn compress_office_file(path: &Path, target_reduction_percent: u8) -> Result<(PathBuf, u64, u64), String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "docx" | "xlsx" | "pptx") {
        return Err(format!("Format Office tidak didukung: .{ext}"));
    }

    let source = std::fs::read(path).map_err(|error| format!("Tidak bisa membaca file: {error}"))?;
    let original_size = source.len() as u64;
    if original_size == 0 {
        return Err("File kosong, tidak dapat dikompres.".into());
    }
    let target = original_size * (100 - target_reduction_percent.clamp(1, 95) as u64) / 100;
    let last = LADDER.len() - 1;
    let worst = compress_office_bytes(&source, LADDER[last], true)?;
    let selected = if worst.len() as u64 > target {
        last
    } else {
        let (mut low, mut high) = (0usize, last);
        while low < high {
            let middle = (low + high) / 2;
            let candidate = compress_office_bytes(&source, LADDER[middle], true)?;
            if candidate.len() as u64 <= target {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        low
    };
    // Estimasi memakai ZIP level 6; tulis hasil final dengan level 9.
    let output = compress_office_bytes(&source, LADDER[selected], false)?;

    let output_path = unique_output(path, &ext);
    let output_size = output.len() as u64;
    std::fs::write(&output_path, output).map_err(|error| format!("Gagal menyimpan hasil: {error}"))?;
    Ok((output_path, original_size, output_size))
}


#[derive(Clone, Copy)]
struct PdfProfile {
    quality: u8,
    scale: f32,
    grayscale: bool,
}

const PDF_LADDER: [PdfProfile; 18] = [
    PdfProfile { quality: 92, scale: 1.0, grayscale: false },
    PdfProfile { quality: 85, scale: 1.0, grayscale: false },
    PdfProfile { quality: 78, scale: 1.0, grayscale: false },
    PdfProfile { quality: 70, scale: 1.0, grayscale: false },
    PdfProfile { quality: 62, scale: 1.0, grayscale: false },
    PdfProfile { quality: 54, scale: 1.0, grayscale: false },
    PdfProfile { quality: 46, scale: 1.0, grayscale: false },
    PdfProfile { quality: 38, scale: 1.0, grayscale: false },
    PdfProfile { quality: 50, scale: 0.8, grayscale: false },
    PdfProfile { quality: 42, scale: 0.7, grayscale: false },
    PdfProfile { quality: 40, scale: 0.6, grayscale: false },
    PdfProfile { quality: 36, scale: 0.5, grayscale: false },
    PdfProfile { quality: 32, scale: 0.4, grayscale: false },
    PdfProfile { quality: 30, scale: 0.3, grayscale: false },
    PdfProfile { quality: 35, scale: 0.4, grayscale: true },
    PdfProfile { quality: 30, scale: 0.3, grayscale: true },
    PdfProfile { quality: 25, scale: 0.25, grayscale: true },
    PdfProfile { quality: 20, scale: 0.2, grayscale: true },
];

fn pdf_is_image_xobject(stream: &lopdf::Stream) -> bool {
    matches!(stream.dict.get(b"Subtype"), Ok(lopdf::Object::Name(name)) if name.as_slice() == b"Image")
}

fn pdf_filter_names(stream: &lopdf::Stream) -> Vec<Vec<u8>> {
    use lopdf::Object;
    match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => vec![name.clone()],
        Ok(Object::Array(items)) => items.iter()
            .filter_map(|item| if let Object::Name(name) = item { Some(name.clone()) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn pdf_dict_i64(dict: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok()?.as_i64().ok()
}

fn pdf_components(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<usize> {
    use lopdf::Object;
    let color_space = dict.get(b"ColorSpace").ok()?;
    let color_space = match color_space {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        value => value,
    };
    match color_space {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" => Some(1),
            b"DeviceRGB" => Some(3),
            _ => None,
        },
        Object::Array(items) if items.len() >= 2 => {
            if !matches!(&items[0], Object::Name(name) if name.as_slice() == b"ICCBased") {
                return None;
            }
            let profile = match &items[1] {
                Object::Reference(id) => doc.get_object(*id).ok()?,
                value => value,
            };
            if let Object::Stream(stream) = profile {
                match pdf_dict_i64(&stream.dict, b"N")? {
                    1 => Some(1),
                    3 => Some(3),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn pdf_decode_params(doc: &lopdf::Document, stream: &lopdf::Stream) -> Option<lopdf::Dictionary> {
    use lopdf::Object;
    let value = stream.dict.get(b"DecodeParms").ok()?;
    let value = match value {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        value => value,
    };
    match value {
        Object::Dictionary(dict) => Some(dict.clone()),
        Object::Array(items) => match items.first()? {
            Object::Dictionary(dict) => Some(dict.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn pdf_paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (a as i32, b as i32, c as i32);
    let predictor = a + b - c;
    let (pa, pb, pc) = ((predictor - a).abs(), (predictor - b).abs(), (predictor - c).abs());
    if pa <= pb && pa <= pc { a as u8 } else if pb <= pc { b as u8 } else { c as u8 }
}

fn pdf_unpredict_png(data: &[u8], bytes_per_pixel: usize, row_bytes: usize) -> Option<Vec<u8>> {
    let stride = row_bytes.checked_add(1)?;
    if row_bytes == 0 || data.len() < stride {
        return None;
    }
    let rows = data.len() / stride;
    let mut output = Vec::with_capacity(rows.checked_mul(row_bytes)?);
    let mut previous = vec![0u8; row_bytes];
    for row in 0..rows {
        let filter = data[row * stride];
        let mut line = data[row * stride + 1..(row + 1) * stride].to_vec();
        match filter {
            0 => {}
            1 => for index in bytes_per_pixel..row_bytes {
                line[index] = line[index].wrapping_add(line[index - bytes_per_pixel]);
            },
            2 => for index in 0..row_bytes {
                line[index] = line[index].wrapping_add(previous[index]);
            },
            3 => for index in 0..row_bytes {
                let left = if index >= bytes_per_pixel { line[index - bytes_per_pixel] as u16 } else { 0 };
                line[index] = line[index].wrapping_add(((left + previous[index] as u16) / 2) as u8);
            },
            4 => for index in 0..row_bytes {
                let left = if index >= bytes_per_pixel { line[index - bytes_per_pixel] } else { 0 };
                let upper_left = if index >= bytes_per_pixel { previous[index - bytes_per_pixel] } else { 0 };
                line[index] = line[index].wrapping_add(pdf_paeth(left, previous[index], upper_left));
            },
            _ => return None,
        }
        output.extend_from_slice(&line);
        previous = line;
    }
    Some(output)
}

fn recode_pdf_image(
    document: &lopdf::Document,
    stream: &lopdf::Stream,
    profile: PdfProfile,
) -> Option<(Vec<u8>, u32, u32, bool)> {
    use lopdf::Object;
    // Resizing an image independently of its soft/color-key mask can invalidate PDF rendering.
    if matches!(stream.dict.get(b"ImageMask"), Ok(Object::Boolean(true)))
        || stream.dict.has(b"Mask")
        || stream.dict.has(b"SMask")
        || stream.dict.has(b"Decode")
    {
        return None;
    }
    let components = pdf_components(document, &stream.dict)?;
    let filters = pdf_filter_names(stream);
    let is_filter = |name: &[u8]| filters.len() == 1 && filters[0].as_slice() == name;

    let image = if is_filter(b"DCTDecode") {
        decode_image(&stream.content).ok()?
    } else if is_filter(b"FlateDecode") {
        let width = pdf_dict_i64(&stream.dict, b"Width")? as usize;
        let height = pdf_dict_i64(&stream.dict, b"Height")? as usize;
        if pdf_dict_i64(&stream.dict, b"BitsPerComponent")? != 8 || width == 0 || height == 0 {
            return None;
        }
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&stream.content[..]).read_to_end(&mut raw).ok()?;
        if let Some(params) = pdf_decode_params(document, stream) {
            let predictor = pdf_dict_i64(&params, b"Predictor").unwrap_or(1);
            if predictor >= 10 {
                let columns = pdf_dict_i64(&params, b"Columns").unwrap_or(1) as usize;
                let colors = pdf_dict_i64(&params, b"Colors").unwrap_or(1) as usize;
                let bits = pdf_dict_i64(&params, b"BitsPerComponent").unwrap_or(8);
                if bits != 8 || colors != components || columns != width {
                    return None;
                }
                raw = pdf_unpredict_png(&raw, components, width.checked_mul(components)?)?;
            } else if predictor == 2 {
                return None;
            }
        }
        let expected = width.checked_mul(height)?.checked_mul(components)?;
        if raw.len() < expected {
            return None;
        }
        raw.truncate(expected);
        if components == 1 {
            image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(width as u32, height as u32, raw)?)
        } else {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(width as u32, height as u32, raw)?)
        }
    } else {
        return None;
    };

    let mut image = scaled(&image, profile.scale);
    if profile.grayscale && image.color().has_color() {
        image = image::DynamicImage::ImageLuma8(image.to_luma8());
    }
    let (width, height) = (image.width(), image.height());
    let encoded = encode_jpeg(&image, profile.quality).ok()?;
    if encoded.len() >= stream.content.len() {
        return None;
    }
    Some((encoded, width, height, !image.color().has_color()))
}

fn compress_pdf_bytes(source: &[u8], profile: PdfProfile) -> Result<Vec<u8>, String> {
    use lopdf::Object;
    let mut document = lopdf::Document::load_mem(source)
        .map_err(|error| format!("PDF tidak valid atau tidak dapat dibuka: {error}"))?;
    let image_ids: Vec<lopdf::ObjectId> = document.objects.iter()
        .filter_map(|(id, object)| match object {
            Object::Stream(stream) if pdf_is_image_xobject(stream) => Some(*id),
            _ => None,
        })
        .collect();
    for id in image_ids {
        let replacement = match document.objects.get(&id) {
            Some(Object::Stream(stream)) => recode_pdf_image(&document, stream, profile),
            _ => None,
        };
        if let Some((bytes, width, height, grayscale)) = replacement {
            if let Some(Object::Stream(stream)) = document.objects.get_mut(&id) {
                stream.set_content(bytes);
                stream.dict.set("Width", Object::Integer(width as i64));
                stream.dict.set("Height", Object::Integer(height as i64));
                stream.dict.set("BitsPerComponent", Object::Integer(8));
                stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
                stream.dict.remove(b"DecodeParms");
                if grayscale {
                    stream.dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
                }
            }
        }
    }
    document.prune_objects();
    document.delete_zero_length_streams();
    document.compress();
    let mut output = Vec::new();
    document.save_to(&mut output)
        .map_err(|error| format!("Gagal menyimpan hasil kompresi PDF: {error}"))?;
    Ok(output)
}


#[derive(Clone, Copy)]
struct GsProfile {
    dpi: u32,
    qfactor: f32,
}

const GS_LADDER: [GsProfile; 11] = [
    GsProfile { dpi: 300, qfactor: 0.15 },
    GsProfile { dpi: 220, qfactor: 0.25 },
    GsProfile { dpi: 170, qfactor: 0.40 },
    GsProfile { dpi: 150, qfactor: 0.50 },
    GsProfile { dpi: 130, qfactor: 0.60 },
    GsProfile { dpi: 110, qfactor: 0.80 },
    GsProfile { dpi: 96, qfactor: 1.00 },
    GsProfile { dpi: 80, qfactor: 1.30 },
    GsProfile { dpi: 72, qfactor: 1.60 },
    GsProfile { dpi: 60, qfactor: 2.00 },
    GsProfile { dpi: 50, qfactor: 2.40 },
];

static GS_RESOURCE_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static GS_PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
static GS_TEMP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Configure the application resource directory used to locate a bundled Ghostscript runtime.
pub fn set_resource_dir(path: PathBuf) {
    let _ = GS_RESOURCE_DIR.set(path);
}

fn ghostscript_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn ghostscript_works(path: &Path) -> bool {
    ghostscript_command(path)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn find_ghostscript() -> Option<PathBuf> {
    let executable = if cfg!(target_os = "windows") { "gswin64c.exe" } else { "gs" };
    let mut candidates = Vec::new();

    if let Some(resource_dir) = GS_RESOURCE_DIR.get() {
        candidates.push(resource_dir.join("gs").join("bin").join(executable));
    }
    if let Some(path) = std::env::var_os("GS_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(directory) = std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)) {
        candidates.push(directory.join("gs").join("bin").join(executable));
        candidates.push(directory.join("resources").join("gs").join("bin").join(executable));
        candidates.push(directory.join(executable));
    }
    candidates.push(PathBuf::from(executable));

    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from("gswin32c.exe"));
        for root in ["C:\\Program Files\\gs", "C:\\Program Files (x86)\\gs"] {
            if let Ok(entries) = std::fs::read_dir(root) {
                let mut directories: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
                directories.sort();
                directories.reverse();
                candidates.extend(directories.into_iter().map(|directory| directory.join("bin").join("gswin64c.exe")));
            }
        }
    }
    #[cfg(target_os = "linux")]
    candidates.extend(["/usr/bin/gs", "/usr/local/bin/gs", "/snap/bin/gs"].into_iter().map(PathBuf::from));

    candidates.into_iter().find(|candidate| ghostscript_works(candidate))
}

fn ghostscript_path() -> Option<&'static PathBuf> {
    GS_PATH.get_or_init(|| {
        if std::env::var_os("NOTCH_NO_GS").is_some() {
            return None;
        }
        find_ghostscript()
    }).as_ref()
}

fn compress_pdf_with_ghostscript(
    executable: &Path,
    input: &Path,
    profile: GsProfile,
) -> Result<Vec<u8>, String> {
    let output = std::env::temp_dir().join(format!(
        "notch-gs-{}-{}.pdf",
        std::process::id(),
        GS_TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let dpi = profile.dpi.to_string();
    let image_parameters = format!(
        "<< /ColorImageDict << /QFactor {q:.2} /Blend 1 /HSamples [2 1 1 2] /VSamples [2 1 1 2] >> /GrayImageDict << /QFactor {q:.2} /Blend 1 /HSamples [1 1 1 1] /VSamples [1 1 1 1] >> >> setdistillerparams",
        q = profile.qfactor
    );

    let mut command = ghostscript_command(executable);
    if let Some(root) = executable.parent().and_then(Path::parent)
        .filter(|root| root.join("Resource").is_dir() && root.join("lib").is_dir())
    {
        let directories: Vec<PathBuf> = ["lib", "Resource/Init", "Resource/Font", "Resource/CMap", "Resource", "fonts"]
            .iter().map(|directory| root.join(directory)).filter(|directory| directory.is_dir()).collect();
        if !directories.is_empty() {
            if let Ok(joined) = std::env::join_paths(directories) {
                command.env("GS_LIB", joined);
            }
        }
    }
    if std::env::var_os("APPDIR").is_some() {
        command.env_remove("LD_LIBRARY_PATH");
    }

    let result = command
        .args(["-q", "-dSAFER", "-dBATCH", "-dNOPAUSE", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.5"])
        .args(["-dDetectDuplicateImages=true", "-dSubsetFonts=true", "-dCompressFonts=true", "-dEmbedAllFonts=true"])
        .arg("-dPassThroughJPEGImages=false")
        .args(["-dDownsampleColorImages=true", "-dDownsampleGrayImages=true", "-dDownsampleMonoImages=true"])
        .args(["-dColorImageDownsampleType=/Bicubic", "-dGrayImageDownsampleType=/Bicubic"])
        .arg("-dColorImageDownsampleThreshold=1.0")
        .arg("-dGrayImageDownsampleThreshold=1.0")
        .arg(format!("-dColorImageResolution={dpi}"))
        .arg(format!("-dGrayImageResolution={dpi}"))
        .arg(format!("-dMonoImageResolution={}", profile.dpi.max(150)))
        .args(["-dAutoFilterColorImages=false", "-dAutoFilterGrayImages=false"])
        .args(["-dColorImageFilter=/DCTEncode", "-dGrayImageFilter=/DCTEncode"])
        .arg(format!("-sOutputFile={}", output.display()))
        .arg("-c").arg(image_parameters).arg("-f").arg(input)
        .output()
        .map_err(|error| format!("Gagal menjalankan Ghostscript: {error}"))?;

    if !result.status.success() {
        let _ = std::fs::remove_file(&output);
        let message = String::from_utf8_lossy(&result.stderr);
        let detail = message.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("kode keluar bukan 0");
        return Err(format!("Ghostscript gagal: {detail}"));
    }
    let bytes = std::fs::read(&output).map_err(|error| format!("Hasil Ghostscript tidak terbaca: {error}"));
    let _ = std::fs::remove_file(output);
    bytes
}

fn compress_pdf_with_profiles(
    source: &[u8],
    path: &Path,
    target: u64,
    original_size: u64,
) -> Result<Vec<u8>, String> {
    if let Some(executable) = ghostscript_path() {
        let worst = compress_pdf_with_ghostscript(executable, path, *GS_LADDER.last().unwrap())?;
        let selected = if worst.len() as u64 > target {
            GS_LADDER.len() - 1
        } else {
            let (mut low, mut high) = (0usize, GS_LADDER.len() - 1);
            while low < high {
                let middle = (low + high) / 2;
                let candidate = compress_pdf_with_ghostscript(executable, path, GS_LADDER[middle])?;
                if candidate.len() as u64 <= target { high = middle; } else { low = middle + 1; }
            }
            low
        };
        let output = compress_pdf_with_ghostscript(executable, path, GS_LADDER[selected])?;
        if (output.len() as u64) < original_size {
            return Ok(output);
        }
    }

    let last = PDF_LADDER.len() - 1;
    let worst = compress_pdf_bytes(source, PDF_LADDER[last])?;
    let selected = if worst.len() as u64 > target {
        last
    } else {
        let (mut low, mut high) = (0usize, last);
        while low < high {
            let middle = (low + high) / 2;
            let candidate = compress_pdf_bytes(source, PDF_LADDER[middle])?;
            if candidate.len() as u64 <= target { high = middle; } else { low = middle + 1; }
        }
        low
    };
    compress_pdf_bytes(source, PDF_LADDER[selected])
}


#[derive(Clone, Copy)]
struct RasterProfile {
    dpi: u32,
    quality: u8,
}

const RASTER_LADDER: [RasterProfile; 7] = [
    RasterProfile { dpi: 110, quality: 45 },
    RasterProfile { dpi: 90, quality: 40 },
    RasterProfile { dpi: 72, quality: 35 },
    RasterProfile { dpi: 60, quality: 30 },
    RasterProfile { dpi: 50, quality: 25 },
    RasterProfile { dpi: 40, quality: 20 },
    RasterProfile { dpi: 30, quality: 15 },
];

fn pdf_name(value: &str) -> lopdf::Object {
    lopdf::Object::Name(value.as_bytes().to_vec())
}

fn pdf_dictionary(items: Vec<(&str, lopdf::Object)>) -> lopdf::Dictionary {
    let mut dictionary = lopdf::Dictionary::new();
    for (key, value) in items {
        dictionary.set(key, value);
    }
    dictionary
}

fn raster_pdf_bytes(source: &[u8], profile: RasterProfile) -> Result<Vec<u8>, String> {
    use lopdf::Object;
    let mut rendered_pages: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    let total = crate::pdf::for_each_page(source, profile.dpi, |_, _, image| {
        let gray = image::DynamicImage::ImageLuma8(image.to_luma8());
        let width = gray.width();
        let height = gray.height();
        let jpeg = encode_jpeg(&gray, profile.quality)?;
        rendered_pages.push((jpeg, width, height));
        Ok(())
    })?;
    if rendered_pages.len() != total || rendered_pages.is_empty() {
        return Err("Tidak semua halaman PDF berhasil dirasterisasi".into());
    }

    let mut document = lopdf::Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let mut kids: Vec<Object> = Vec::with_capacity(rendered_pages.len());
    for (jpeg, width, height) in rendered_pages {
        let width_points = (width as f64 * 72.0 / profile.dpi as f64).round().max(1.0) as i64;
        let height_points = (height as f64 * 72.0 / profile.dpi as f64).round().max(1.0) as i64;
        let image_id = document.add_object(lopdf::Stream::new(
            pdf_dictionary(vec![
                ("Type", pdf_name("XObject")),
                ("Subtype", pdf_name("Image")),
                ("Width", Object::Integer(width as i64)),
                ("Height", Object::Integer(height as i64)),
                ("ColorSpace", pdf_name("DeviceGray")),
                ("BitsPerComponent", Object::Integer(8)),
                ("Filter", pdf_name("DCTDecode")),
            ]),
            jpeg,
        ));
        let content_id = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            format!("q {width_points} 0 0 {height_points} 0 0 cm /Im0 Do Q").into_bytes(),
        ));
        let xobjects = pdf_dictionary(vec![("Im0", Object::Reference(image_id))]);
        let resources = pdf_dictionary(vec![("XObject", Object::Dictionary(xobjects))]);
        let page_id = document.add_object(pdf_dictionary(vec![
            ("Type", pdf_name("Page")),
            ("Parent", Object::Reference(pages_id)),
            ("MediaBox", Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(width_points), Object::Integer(height_points),
            ])),
            ("Contents", Object::Reference(content_id)),
            ("Resources", Object::Dictionary(resources)),
        ]));
        kids.push(Object::Reference(page_id));
    }
    document.objects.insert(pages_id, Object::Dictionary(pdf_dictionary(vec![
        ("Type", pdf_name("Pages")),
        ("Kids", Object::Array(kids)),
        ("Count", Object::Integer(total as i64)),
    ])));
    let catalog_id = document.add_object(pdf_dictionary(vec![
        ("Type", pdf_name("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    document.trailer.set("Root", Object::Reference(catalog_id));
    document.compress();
    let mut output = Vec::new();
    document.save_to(&mut output)
        .map_err(|error| format!("Gagal menyimpan PDF hasil rasterisasi: {error}"))?;
    Ok(output)
}

fn raster_pdf_estimate(source: &[u8], profile: RasterProfile) -> Result<u64, String> {
    let mut sample_bytes = 0u64;
    let mut sample_pages = 0usize;
    let total = crate::pdf::for_each_page_sample(source, profile.dpi, 12, |_, _, image| {
        let gray = image::DynamicImage::ImageLuma8(image.to_luma8());
        sample_bytes += encode_jpeg(&gray, profile.quality)?.len() as u64;
        sample_pages += 1;
        Ok(())
    })?;
    if sample_pages == 0 {
        return Err("PDF tidak punya halaman untuk estimasi".into());
    }
    let average = sample_bytes / sample_pages as u64;
    Ok(average * total as u64 + 250 * total as u64 + 700)
}

fn compress_pdf_raster(source: &[u8], target_size: u64) -> Result<Vec<u8>, String> {
    let last = RASTER_LADDER.len() - 1;
    let worst = raster_pdf_estimate(source, RASTER_LADDER[last])?;
    let selected = if worst > target_size {
        last
    } else {
        let (mut low, mut high) = (0usize, last);
        while low < high {
            let middle = (low + high) / 2;
            if raster_pdf_estimate(source, RASTER_LADDER[middle])? <= target_size {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        low
    };
    raster_pdf_bytes(source, RASTER_LADDER[selected])
}

/// Compress PDF content, preserving text/vector where possible and rasterizing only as a last resort.
pub fn compress_pdf_file(path: &Path, target_reduction_percent: u8) -> Result<(PathBuf, u64, u64), String> {
    if path.extension().and_then(|value| value.to_str()).unwrap_or("").eq_ignore_ascii_case("pdf") == false {
        return Err("File bukan PDF".into());
    }
    let source = std::fs::read(path).map_err(|error| format!("Tidak bisa membaca PDF: {error}"))?;
    if source.is_empty() {
        return Err("File PDF kosong".into());
    }
    let original_size = source.len() as u64;
    let target = original_size * (100 - target_reduction_percent.clamp(1, 95) as u64) / 100;
    let mut output = compress_pdf_with_profiles(&source, path, target, original_size)?;
    if output.len() as u64 > target && crate::pdf::is_available() {
        if let Ok(rasterized) = compress_pdf_raster(&source, target) {
            if rasterized.len() < output.len() {
                output = rasterized;
            }
        }
    }
    if output.len() as u64 >= original_size {
        return Err("PDF tidak dapat diperkecil dengan aman menggunakan profil kompresi yang tersedia.".into());
    }
    let output_path = unique_output(path, "pdf");
    let output_size = output.len() as u64;
    std::fs::write(&output_path, output)
        .map_err(|error| format!("Gagal menyimpan hasil: {error}"))?;
    Ok((output_path, original_size, output_size))
}


/// Compress a supported image, PDF, or Office Open XML document.
pub fn compress_file(path: &Path, target_reduction_percent: u8) -> Result<(PathBuf, u64, u64), String> {
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "pdf" {
        compress_pdf_file(path, target_reduction_percent)
    } else if matches!(ext.as_str(), "docx" | "xlsx" | "pptx") {
        compress_office_file(path, target_reduction_percent)
    } else {
        compress_image_file(path, target_reduction_percent)
    }
}
