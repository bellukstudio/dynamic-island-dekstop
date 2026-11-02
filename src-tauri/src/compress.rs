use serde::Serialize;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use tauri::{AppHandle, Manager};

// ---------------------------------------------------------------------------------------------
// Profil kompresi: dari ringan (indeks 0) ke agresif. Pencarian biner memilih profil teringan
// yang masih memenuhi target ukuran.
// ---------------------------------------------------------------------------------------------
#[derive(Clone, Copy)]
struct Profile {
    q: u8,
    scale: f32,
    gray: bool, // ubah gambar berwarna jadi abu-abu (dipakai level PDF terakhir)
}

const LADDER: [Profile; 14] = [
    Profile { q: 92, scale: 1.0, gray: false },
    Profile { q: 85, scale: 1.0, gray: false },
    Profile { q: 78, scale: 1.0, gray: false },
    Profile { q: 70, scale: 1.0, gray: false },
    Profile { q: 62, scale: 1.0, gray: false },
    Profile { q: 54, scale: 1.0, gray: false },
    Profile { q: 46, scale: 1.0, gray: false },
    Profile { q: 38, scale: 1.0, gray: false },
    Profile { q: 50, scale: 0.8, gray: false },
    Profile { q: 42, scale: 0.7, gray: false },
    Profile { q: 40, scale: 0.6, gray: false },
    Profile { q: 36, scale: 0.5, gray: false },
    Profile { q: 32, scale: 0.4, gray: false },
    Profile { q: 30, scale: 0.3, gray: false },
];

/// Tangga khusus PDF (tanpa Ghostscript): sama seperti LADDER, ditambah level abu-abu.
const PDF_LADDER: [Profile; 18] = [
    Profile { q: 92, scale: 1.0, gray: false },
    Profile { q: 85, scale: 1.0, gray: false },
    Profile { q: 78, scale: 1.0, gray: false },
    Profile { q: 70, scale: 1.0, gray: false },
    Profile { q: 62, scale: 1.0, gray: false },
    Profile { q: 54, scale: 1.0, gray: false },
    Profile { q: 46, scale: 1.0, gray: false },
    Profile { q: 38, scale: 1.0, gray: false },
    Profile { q: 50, scale: 0.8, gray: false },
    Profile { q: 42, scale: 0.7, gray: false },
    Profile { q: 40, scale: 0.6, gray: false },
    Profile { q: 36, scale: 0.5, gray: false },
    Profile { q: 32, scale: 0.4, gray: false },
    Profile { q: 30, scale: 0.3, gray: false },
    // level terakhir: abu-abu + resolusi sangat rendah, untuk target ukuran ekstrem
    Profile { q: 35, scale: 0.4, gray: true },
    Profile { q: 30, scale: 0.3, gray: true },
    Profile { q: 25, scale: 0.25, gray: true },
    Profile { q: 20, scale: 0.2, gray: true },
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Image,
    Pdf,
    Office,
    Other,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Image => "image",
            Kind::Pdf => "pdf",
            Kind::Office => "office",
            Kind::Other => "other",
        }
    }
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn kind_of(ext: &str) -> Kind {
    match ext {
        "jpg" | "jpeg" | "png" | "bmp" => Kind::Image,
        "pdf" => Kind::Pdf,
        "docx" | "xlsx" | "pptx" => Kind::Office,
        _ => Kind::Other,
    }
}

/// Ekstensi file hasil: BMP menjadi JPG, file lain menjadi ZIP, sisanya tetap.
fn out_ext(ext: &str, kind: Kind) -> String {
    match (kind, ext) {
        (Kind::Image, "bmp") => "jpg".into(),
        (Kind::Other, _) => "zip".into(),
        _ => ext.to_string(),
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------------
// Gambar
// ---------------------------------------------------------------------------------------------
pub(crate) fn decode_image(src: &[u8], apply_orientation: bool) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(Cursor::new(src)).with_guessed_format().map_err(err)?;
    let mut dec = reader.into_decoder().map_err(err)?;
    let orientation = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(dec).map_err(err)?;
    if apply_orientation {
        img.apply_orientation(orientation);
    }
    Ok(img)
}

fn scaled(img: &image::DynamicImage, scale: f32) -> image::DynamicImage {
    if scale >= 0.999 {
        return img.clone();
    }
    let w = ((img.width() as f32 * scale).round() as u32).max(1);
    let h = ((img.height() as f32 * scale).round() as u32).max(1);
    img.resize_exact(w, h, image::imageops::FilterType::Triangle)
}

// Cache gambar yang sudah di-decode (belum di-scale), kunci (path, ukuran asli).
// Pencarian profil memanggil render() hingga ~4x per file (binary search) dan decode JPEG/PNG
// besar adalah bagian termahal — tanpa cache ini diulang setiap langkah, inilah sumber utama
// lag saat menggeser slider pada file gambar besar.
static IMG_CACHE: LazyLock<Mutex<HashMap<(String, u64), std::sync::Arc<image::DynamicImage>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn decode_cached(path: &Path, src: &[u8]) -> Result<std::sync::Arc<image::DynamicImage>, String> {
    let key = (path.to_string_lossy().into_owned(), src.len() as u64);
    if let Some(img) = IMG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(img.clone());
    }
    let img = std::sync::Arc::new(decode_image(src, true)?);
    IMG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(key, img.clone());
    Ok(img)
}

pub(crate) fn encode_jpeg(img: &image::DynamicImage, q: u8) -> Result<Vec<u8>, String> {
    use image::{ColorType, DynamicImage};
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
    let flat = match img.color() {
        ColorType::L8 | ColorType::L16 | ColorType::La8 | ColorType::La16 => DynamicImage::ImageLuma8(img.to_luma8()),
        _ => DynamicImage::ImageRgb8(img.to_rgb8()),
    };
    flat.write_with_encoder(enc).map_err(err)?;
    Ok(out)
}

pub(crate) fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::DynamicImage;
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive);
    let flat = if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    };
    flat.write_with_encoder(enc).map_err(err)?;
    Ok(out)
}

fn compress_image(path: &Path, src: &[u8], ext: &str, p: Profile) -> Result<Vec<u8>, String> {
    let base = decode_cached(path, src)?;
    let img = scaled(&base, p.scale);
    if ext == "png" {
        encode_png(&img)
    } else {
        encode_jpeg(&img, p.q) // jpg, jpeg, bmp
    }
}

/// Encode ulang JPEG yang tertanam (PDF/Office). None bila gagal atau tidak lebih kecil.
/// Gambar tertanam biasanya kecil dan hanya di-render sekali per dokumen per langkah, jadi
/// tidak memakai IMG_CACHE (kuncinya per path-file, bukan per gambar tertanam).
fn recode_jpeg(src: &[u8], p: Profile, apply_orientation: bool) -> Option<(Vec<u8>, u32, u32)> {
    let img = scaled(&decode_image(src, apply_orientation).ok()?, p.scale);
    let (w, h) = (img.width(), img.height());
    let out = encode_jpeg(&img, p.q).ok()?;
    if out.len() < src.len() { Some((out, w, h)) } else { None }
}

fn recode_png(src: &[u8], p: Profile) -> Option<Vec<u8>> {
    let img = scaled(&decode_image(src, true).ok()?, p.scale);
    let out = encode_png(&img).ok()?;
    if out.len() < src.len() { Some(out) } else { None }
}

// ---------------------------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------------------------
#[allow(dead_code)]
fn is_dct(s: &lopdf::Stream) -> bool {
    use lopdf::Object;
    match s.dict.get(b"Filter") {
        Ok(Object::Name(n)) => n.as_slice() == b"DCTDecode",
        Ok(Object::Array(a)) => a.len() == 1 && matches!(&a[0], Object::Name(n) if n.as_slice() == b"DCTDecode"),
        _ => false,
    }
}

fn is_image_xobject(s: &lopdf::Stream) -> bool {
    matches!(s.dict.get(b"Subtype"), Ok(lopdf::Object::Name(n)) if n.as_slice() == b"Image")
}

fn filter_names(s: &lopdf::Stream) -> Vec<Vec<u8>> {
    use lopdf::Object;
    match s.dict.get(b"Filter") {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(a)) => a
            .iter()
            .filter_map(|o| if let Object::Name(n) = o { Some(n.clone()) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn dict_i64(d: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    d.get(key).ok()?.as_i64().ok()
}

/// Jumlah komponen warna (1 = gray, 3 = RGB). None untuk CMYK / Indexed / dll (dilewati).
fn comps_of(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<usize> {
    use lopdf::Object;
    let cs = dict.get(b"ColorSpace").ok()?;
    let cs = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        o => o,
    };
    match cs {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" => Some(1),
            b"DeviceRGB" => Some(3),
            _ => None,
        },
        Object::Array(a) if a.len() >= 2 => {
            if !matches!(&a[0], Object::Name(n) if n.as_slice() == b"ICCBased") {
                return None;
            }
            let icc = match &a[1] {
                Object::Reference(id) => doc.get_object(*id).ok()?,
                o => o,
            };
            if let Object::Stream(st) = icc {
                match dict_i64(&st.dict, b"N")? {
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

fn decode_parms(doc: &lopdf::Document, s: &lopdf::Stream) -> Option<lopdf::Dictionary> {
    use lopdf::Object;
    let o = s.dict.get(b"DecodeParms").ok()?;
    let o = match o {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        o => o,
    };
    match o {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Array(a) => match a.first()? {
            Object::Dictionary(d) => Some(d.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i32, b as i32, c as i32);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
}

/// Balikkan filter prediktor PNG (Predictor >= 10) pada data Flate. bpp = byte per piksel.
fn unpredict_png(data: &[u8], bpp: usize, row_bytes: usize) -> Option<Vec<u8>> {
    let stride = row_bytes + 1;
    if row_bytes == 0 || data.len() < stride {
        return None;
    }
    let rows = data.len() / stride;
    let mut out = Vec::with_capacity(rows * row_bytes);
    let mut prev = vec![0u8; row_bytes];
    for r in 0..rows {
        let ft = data[r * stride];
        let mut line = data[r * stride + 1..(r + 1) * stride].to_vec();
        match ft {
            0 => {}
            1 => {
                for i in bpp..row_bytes {
                    line[i] = line[i].wrapping_add(line[i - bpp]);
                }
            }
            2 => {
                for i in 0..row_bytes {
                    line[i] = line[i].wrapping_add(prev[i]);
                }
            }
            3 => {
                for i in 0..row_bytes {
                    let left = if i >= bpp { line[i - bpp] as u16 } else { 0 };
                    line[i] = line[i].wrapping_add(((left + prev[i] as u16) / 2) as u8);
                }
            }
            4 => {
                for i in 0..row_bytes {
                    let a = if i >= bpp { line[i - bpp] } else { 0 };
                    let c = if i >= bpp { prev[i - bpp] } else { 0 };
                    line[i] = line[i].wrapping_add(paeth(a, prev[i], c));
                }
            }
            _ => return None,
        }
        out.extend_from_slice(&line);
        prev = line;
    }
    Some(out)
}

/// Encode ulang satu gambar PDF (JPEG atau Flate 8-bit Gray/RGB) menjadi JPEG yang lebih kecil.
/// Mengembalikan (bytes, lebar, tinggi, grayscale). None bila tidak didukung atau tidak lebih kecil.
fn recode_pdf_image(doc: &lopdf::Document, s: &lopdf::Stream, p: Profile) -> Option<(Vec<u8>, u32, u32, bool)> {
    use lopdf::Object;
    // stensil, color-key mask, dan Decode kustom bisa rusak bila di-JPEG-kan: lewati
    if matches!(s.dict.get(b"ImageMask"), Ok(Object::Boolean(true))) {
        return None;
    }
    if matches!(s.dict.get(b"Mask"), Ok(Object::Array(_))) || s.dict.has(b"Decode") {
        return None;
    }
    let n = comps_of(doc, &s.dict)?;
    let filters = filter_names(s);
    let is = |name: &[u8]| filters.len() == 1 && filters[0].as_slice() == name;

    let img = if is(b"DCTDecode") {
        decode_image(&s.content, false).ok()?
    } else if is(b"FlateDecode") {
        let (w, h) = (dict_i64(&s.dict, b"Width")? as usize, dict_i64(&s.dict, b"Height")? as usize);
        if dict_i64(&s.dict, b"BitsPerComponent")? != 8 || w == 0 || h == 0 {
            return None;
        }
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&s.content[..]).read_to_end(&mut raw).ok()?;
        if let Some(dp) = decode_parms(doc, s) {
            let pred = dict_i64(&dp, b"Predictor").unwrap_or(1);
            if pred >= 10 {
                let cols = dict_i64(&dp, b"Columns").unwrap_or(1) as usize;
                let colors = dict_i64(&dp, b"Colors").unwrap_or(1) as usize;
                let bpc = dict_i64(&dp, b"BitsPerComponent").unwrap_or(8);
                if bpc != 8 || colors != n || cols != w {
                    return None;
                }
                raw = unpredict_png(&raw, n, w * n)?;
            } else if pred == 2 {
                return None; // prediktor TIFF: tidak didukung
            }
        }
        if raw.len() < w * h * n {
            return None;
        }
        raw.truncate(w * h * n);
        if n == 1 {
            image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(w as u32, h as u32, raw)?)
        } else {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(w as u32, h as u32, raw)?)
        }
    } else {
        return None; // JBIG2 / CCITT / JPX / dll: dibiarkan
    };

    let mut img = scaled(&img, p.scale);
    if p.gray && img.color().has_color() {
        img = image::DynamicImage::ImageLuma8(img.to_luma8());
    }
    let (w, h) = (img.width(), img.height());
    let out = encode_jpeg(&img, p.q).ok()?;
    if out.len() >= s.content.len() {
        return None;
    }
    Some((out, w, h, !img.color().has_color()))
}

fn compress_pdf(src: &[u8], p: Profile) -> Result<Vec<u8>, String> {
    use lopdf::Object;
    let mut doc = lopdf::Document::load_mem(src).map_err(err)?;
    let ids: Vec<lopdf::ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, o)| match o {
            Object::Stream(s) if is_image_xobject(s) => Some(*id),
            _ => None,
        })
        .collect();
    for id in ids {
        let res = match doc.objects.get(&id) {
            Some(Object::Stream(s)) => recode_pdf_image(&doc, s, p),
            _ => None,
        };
        if let Some((bytes, w, h, luma)) = res {
            if let Some(Object::Stream(s)) = doc.objects.get_mut(&id) {
                s.set_content(bytes);
                s.dict.set("Width", Object::Integer(w as i64));
                s.dict.set("Height", Object::Integer(h as i64));
                s.dict.set("BitsPerComponent", Object::Integer(8));
                s.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
                s.dict.remove(b"DecodeParms");
                if luma {
                    s.dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
                }
            }
        }
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}

static REPORTED: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// Diagnostik (sekali per file): tampilkan ke terminal isi PDF per kategori, supaya jelas apa
/// yang membuat ukurannya besar dan apa yang bisa dikecilkan.
fn pdf_report(path: &Path, src: &[u8]) {
    use lopdf::Object;
    let key = path.to_string_lossy().into_owned();
    if !REPORTED.lock().unwrap_or_else(|e| e.into_inner()).insert(key) {
        return;
    }
    let Ok(doc) = lopdf::Document::load_mem(src) else { return };
    let pages = doc.get_pages().len();
    let (mut jpeg, mut flate, mut bilevel, mut jpx, mut img_other, mut font, mut rest) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut n_jpeg, mut n_flate) = (0, 0);
    for (_, o) in doc.objects.iter() {
        let Object::Stream(s) = o else { continue };
        let len = s.content.len() as u64;
        if is_image_xobject(s) {
            let f = filter_names(s);
            let first = f.first().map(|x| x.as_slice()).unwrap_or(b"");
            match first {
                b"DCTDecode" => { jpeg += len; n_jpeg += 1; }
                b"FlateDecode" | b"LZWDecode" => { flate += len; n_flate += 1; }
                b"JBIG2Decode" | b"CCITTFaxDecode" => bilevel += len,
                b"JPXDecode" => jpx += len,
                _ => img_other += len,
            }
        } else if s.dict.has(b"Length1") || matches!(s.dict.get(b"Subtype"), Ok(Object::Name(n)) if matches!(n.as_slice(), b"Type1C" | b"CIDFontType0C" | b"OpenType")) {
            font += len;
        } else {
            rest += len;
        }
    }
    let kb = |v: u64| format!("{:.0} KB", v as f64 / 1024.0);
    eprintln!(
        "pdf-report {}: {} halaman, {} total | JPEG {} ({}x) | Flate-gambar {} ({}x) | 1-bit(JBIG2/CCITT) {} | JPX {} | gambar-lain {} | font {} | konten/lain {}",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        pages, kb(src.len() as u64), kb(jpeg), n_jpeg, kb(flate), n_flate, kb(bilevel), kb(jpx), kb(img_other), kb(font), kb(rest)
    );
}

// ---------------------------------------------------------------------------------------------
// ZIP: Office (gambar di folder media di-encode ulang) dan pengemasan file biasa
// ---------------------------------------------------------------------------------------------
/// `fast=true` dipakai selama pencarian profil (binary search bisa memanggil ini ~4x per file):
/// level kompresi rendah tapi jauh lebih cepat, cukup akurat untuk estimasi. `fast=false` dipakai
/// sekali saja saat benar-benar menyimpan hasil akhir, dengan kompresi maksimal.
fn zip_opts(fast: bool) -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(if fast { 6 } else { 9 }))
}

fn compress_office(src: &[u8], p: Profile, fast: bool) -> Result<Vec<u8>, String> {
    let mut ar = zip::ZipArchive::new(Cursor::new(src)).map_err(err)?;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip_opts(fast);
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(err)?;
        let name = f.name().to_string();
        if f.is_dir() {
            w.add_directory(name, opts).map_err(err)?;
            continue;
        }
        let mut data = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut data).map_err(err)?;
        let lower = name.to_lowercase();
        if lower.contains("/media/") {
            if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                if let Some((b, _, _)) = recode_jpeg(&data, p, true) {
                    data = b;
                }
            } else if lower.ends_with(".png") {
                if let Some(b) = recode_png(&data, p) {
                    data = b;
                }
            }
        }
        w.start_file(name, opts).map_err(err)?;
        w.write_all(&data).map_err(err)?;
    }
    Ok(w.finish().map_err(err)?.into_inner())
}

fn zip_single(name: &str, src: &[u8], fast: bool) -> Result<Vec<u8>, String> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    w.start_file(name, zip_opts(fast)).map_err(err)?;
    w.write_all(src).map_err(err)?;
    Ok(w.finish().map_err(err)?.into_inner())
}

// ---------------------------------------------------------------------------------------------
// PDF lewat Ghostscript (seperti aplikasi kompres PDF lain)
//
// Ghostscript (pdfwrite) mengecilkan SEMUA jenis gambar (Flate/JPEG/1-bit) dengan downsample DPI,
// men-subset font, membuang objek tak terpakai, dan menyimpan struktur secara efisien. Ini yang
// membuat PDF 1,2 MB bisa turun ke ~500 KB sementara teks tetap bisa dipilih.
//
// Lokasi yang dicari (berurutan):
//   1. env GS_PATH (path penuh ke gswin64c.exe / gs)
//   2. folder aplikasi: <exe>/gs/bin/gswin64c.exe, <exe>/resources/gs/bin/gswin64c.exe,
//      <exe>/gswin64c.exe  (untuk dibundel bersama installer)
//   3. PATH sistem (gswin64c / gs)
//   4. C:\Program Files\gs\gs*\bin\gswin64c.exe (hasil instalasi Ghostscript resmi)
// Bila tidak ada, dipakai mode dasar (lopdf: hanya gambar JPEG), yang sering "tidak bisa lebih kecil".
// ---------------------------------------------------------------------------------------------
/// Folder resource aplikasi (dari Tauri): benar di installer Windows, .deb, AppImage, dan mode dev.
static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_resource_dir(app: &AppHandle) {
    if let Ok(d) = app.path().resource_dir() {
        let _ = RESOURCE_DIR.set(d);
    }
}

#[derive(Clone, Copy)]
struct GsProfile {
    dpi: u32,
    qfactor: f32, // QFactor JPEG Ghostscript: makin kecil makin berkualitas
    q: u8,        // perkiraan kualitas (hanya untuk label)
}

const GS_LADDER: [GsProfile; 11] = [
    GsProfile { dpi: 300, qfactor: 0.15, q: 92 },
    GsProfile { dpi: 220, qfactor: 0.25, q: 85 },
    GsProfile { dpi: 170, qfactor: 0.40, q: 78 },
    GsProfile { dpi: 150, qfactor: 0.50, q: 72 },
    GsProfile { dpi: 130, qfactor: 0.60, q: 66 },
    GsProfile { dpi: 110, qfactor: 0.80, q: 58 },
    GsProfile { dpi: 96, qfactor: 1.00, q: 50 },
    GsProfile { dpi: 80, qfactor: 1.30, q: 42 },
    GsProfile { dpi: 72, qfactor: 1.60, q: 36 },
    GsProfile { dpi: 60, qfactor: 2.00, q: 30 },
    GsProfile { dpi: 50, qfactor: 2.40, q: 25 },
];

pub(crate) fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: jangan kedip jendela konsol
    }
    cmd
}

fn gs_works(p: &Path) -> bool {
    no_window(std::process::Command::new(p).arg("--version"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn find_gs() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") { "gswin64c.exe" } else { "gs" };
    let mut cands: Vec<PathBuf> = Vec::new();

    if let Some(rd) = RESOURCE_DIR.get() {
        cands.push(rd.join("gs").join("bin").join(exe_name));
    }
    if let Some(p) = std::env::var_os("GS_PATH") {
        cands.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        cands.push(dir.join("gs").join("bin").join(exe_name));
        cands.push(dir.join("resources").join("gs").join("bin").join(exe_name));
        cands.push(dir.join(exe_name));
    }
    // PATH sistem
    cands.push(PathBuf::from(exe_name));
    if cfg!(target_os = "windows") {
        cands.push(PathBuf::from("gswin32c.exe"));
        for root in ["C:\\Program Files\\gs", "C:\\Program Files (x86)\\gs"] {
            if let Ok(rd) = std::fs::read_dir(root) {
                let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                dirs.sort();
                dirs.reverse(); // versi terbaru dulu
                for d in dirs {
                    cands.push(d.join("bin").join("gswin64c.exe"));
                }
            }
        }
    }
    if cfg!(target_os = "linux") {
        // Ghostscript sistem (apt/dnf/pacman install ghostscript). Biner Linux tidak portabel,
        // jadi tidak dibundel; bila tidak ada, dipakai mode bawaan + rasterisasi Pdfium.
        for p in ["/usr/bin/gs", "/usr/local/bin/gs", "/snap/bin/gs"] {
            cands.push(PathBuf::from(p));
        }
    }
    cands.into_iter().find(|p| gs_works(p))
}

static GS: OnceLock<Option<PathBuf>> = OnceLock::new();

fn gs_path() -> Option<&'static PathBuf> {
    GS.get_or_init(|| {
        let p = if std::env::var_os("NOTCH_NO_GS").is_some() { None } else { find_gs() };
        match &p {
            Some(p) => eprintln!("compress: Ghostscript dipakai: {}", p.display()),
            None => eprintln!("compress: Ghostscript TIDAK ditemukan, PDF memakai mode dasar (lopdf)"),
        }
        p
    })
    .as_ref()
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn compress_pdf_gs(gs: &Path, input: &Path, p: GsProfile) -> Result<Vec<u8>, String> {
    let out = std::env::temp_dir().join(format!(
        "notch-gs-{}-{}.pdf",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let dpi = p.dpi.to_string();
    let img_dict = format!(
        "<< /ColorImageDict << /QFactor {q:.2} /Blend 1 /HSamples [2 1 1 2] /VSamples [2 1 1 2] >> \
           /GrayImageDict << /QFactor {q:.2} /Blend 1 /HSamples [1 1 1 1] /VSamples [1 1 1 1] >> >> setdistillerparams",
        q = p.qfactor
    );

    let mut cmd = std::process::Command::new(gs);
    // Ghostscript yang dibundel: arahkan ke folder lib/Resource di sebelah bin/ supaya init file
    // dan font-nya ketemu tanpa instalasi.
    if let Some(root) = gs
        .parent()
        .and_then(Path::parent)
        .filter(|r| r.join("Resource").is_dir() && r.join("lib").is_dir())
    {
        let dirs: Vec<PathBuf> = ["lib", "Resource/Init", "Resource/Font", "Resource/CMap", "Resource", "fonts"]
            .iter()
            .map(|d| root.join(d))
            .filter(|d| d.is_dir())
            .collect();
        if !dirs.is_empty() {
            if let Ok(joined) = std::env::join_paths(dirs) {
                cmd.env("GS_LIB", joined);
            }
        }
    }
    // AppImage menimpa LD_LIBRARY_PATH dengan pustaka miliknya; Ghostscript sistem harus memakai pustaka sistem.
    if std::env::var_os("APPDIR").is_some() {
        cmd.env_remove("LD_LIBRARY_PATH");
    }
    no_window(&mut cmd)
        .args(["-q", "-dSAFER", "-dBATCH", "-dNOPAUSE", "-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.5"])
        .args(["-dDetectDuplicateImages=true", "-dSubsetFonts=true", "-dCompressFonts=true", "-dEmbedAllFonts=true"])
        // PENTING: tanpa ini JPEG yang sudah ada diteruskan apa adanya dan tidak dikecilkan
        .arg("-dPassThroughJPEGImages=false")
        .args(["-dDownsampleColorImages=true", "-dDownsampleGrayImages=true", "-dDownsampleMonoImages=true"])
        .args(["-dColorImageDownsampleType=/Bicubic", "-dGrayImageDownsampleType=/Bicubic"])
        .arg("-dColorImageDownsampleThreshold=1.0")
        .arg("-dGrayImageDownsampleThreshold=1.0")
        .arg(format!("-dColorImageResolution={dpi}"))
        .arg(format!("-dGrayImageResolution={dpi}"))
        .arg(format!("-dMonoImageResolution={}", p.dpi.max(150)))
        .args(["-dAutoFilterColorImages=false", "-dAutoFilterGrayImages=false"])
        .args(["-dColorImageFilter=/DCTEncode", "-dGrayImageFilter=/DCTEncode"])
        .arg(format!("-sOutputFile={}", out.display()))
        .arg("-c")
        .arg(img_dict)
        .arg("-f")
        .arg(input);

    let res = cmd.output().map_err(|e| format!("Gagal menjalankan Ghostscript: {e}"))?;
    if !res.status.success() {
        let _ = std::fs::remove_file(&out);
        let msg = String::from_utf8_lossy(&res.stderr);
        let msg = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("kode keluar bukan 0");
        return Err(format!("Ghostscript gagal: {msg}"));
    }
    let bytes = std::fs::read(&out).map_err(|e| format!("Hasil Ghostscript tidak terbaca: {e}"));
    let _ = std::fs::remove_file(&out);
    bytes
}

// ---------------------------------------------------------------------------------------------
// Rasterisasi PDF lewat Pdfium (jalan terakhir untuk target ukuran ekstrem)
//
// Setiap halaman dirender jadi gambar abu-abu ber-DPI rendah, di-JPEG-kan, lalu dirakit jadi PDF
// baru. Ukuran bisa ditekan jauh, tapi teks tidak bisa dipilih/dicari lagi dan tulisan jadi
// pecah di DPI rendah. Level ini hanya dipakai bila level biasa tidak mencapai target.
//
// Butuh pdfium.dll (win-x64) dari github.com/bblanchon/pdfium-binaries. Dicari di:
//   <exe>/, <exe>/pdfium/, <exe>/resources/pdfium/  lalu library sistem.
// Tanpa pdfium.dll level rasterisasi otomatis tidak ada (perilaku seperti sebelumnya).
// ---------------------------------------------------------------------------------------------
#[derive(Clone, Copy)]
struct RasterProfile {
    dpi: u32,
    q: u8,
}

const RASTER_LADDER: [RasterProfile; 7] = [
    RasterProfile { dpi: 110, q: 45 },
    RasterProfile { dpi: 90, q: 40 },
    RasterProfile { dpi: 72, q: 35 },
    RasterProfile { dpi: 60, q: 30 },
    RasterProfile { dpi: 50, q: 25 },
    RasterProfile { dpi: 40, q: 20 },
    RasterProfile { dpi: 30, q: 15 },
];

// Pdfium tidak thread-safe: semua pemakaiannya diserialkan.
static PDFIUM_LOCK: Mutex<()> = Mutex::new(());

trait IntoDyn {
    fn into_dyn(self) -> Option<image::DynamicImage>;
}
impl IntoDyn for image::DynamicImage {
    fn into_dyn(self) -> Option<image::DynamicImage> {
        Some(self)
    }
}
impl<E> IntoDyn for Result<image::DynamicImage, E> {
    fn into_dyn(self) -> Option<image::DynamicImage> {
        self.ok()
    }
}

fn pdfium_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(rd) = RESOURCE_DIR.get() {
        v.push(rd.join("pdfium"));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(dir.join("pdfium"));
        v.push(dir.join("resources").join("pdfium"));
        v.push(dir);
    }
    v
}

fn bind_pdfium() -> Result<pdfium_render::prelude::Pdfium, String> {
    use pdfium_render::prelude::*;
    for d in pdfium_dirs() {
        let lib = Pdfium::pdfium_platform_library_name_at_path(&d);
        if let Ok(b) = Pdfium::bind_to_library(lib) {
            return Ok(Pdfium::new(b));
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| format!("Pdfium tidak ditemukan: {e}"))
}

pub(crate) fn raster_ok() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let _g = PDFIUM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match bind_pdfium() {
            Ok(_) => {
                eprintln!("compress: Pdfium ditemukan, mode rasterisasi PDF tersedia");
                true
            }
            Err(e) => {
                eprintln!("compress: {e} — mode rasterisasi PDF tidak tersedia");
                false
            }
        }
    })
}

/// (jpeg, lebar_px, tinggi_px, lebar_pt, tinggi_pt) per halaman, plus jumlah halaman total.
/// `sample = Some(k)`: hanya k halaman yang tersebar merata (untuk estimasi cepat).
fn render_pages(src: &[u8], rp: RasterProfile, sample: Option<usize>) -> Result<(Vec<(Vec<u8>, u32, u32, f32, f32)>, usize), String> {
    use pdfium_render::prelude::*;
    let _g = PDFIUM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let pdfium = bind_pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(src, None).map_err(err)?;
    let pages = doc.pages();
    let total = pages.len() as usize;
    let idxs: Vec<u16> = match sample {
        Some(k) if k > 0 && total > k => (0..k).map(|j| (j * total / k) as u16).collect(),
        _ => (0..pages.len()).collect(),
    };
    let mut out = Vec::with_capacity(idxs.len());
    for i in idxs {
        let page = pages.get(i).map_err(err)?;
        let (wpt, hpt) = (page.width().value, page.height().value);
        let wpx = ((wpt / 72.0 * rp.dpi as f32).round() as i32).max(1);
        let hpx = ((hpt / 72.0 * rp.dpi as f32).round() as i32).max(1);
        let cfg = PdfRenderConfig::new().set_target_size(wpx, hpx);
        let bmp = page.render_with_config(&cfg).map_err(err)?;
        let img = bmp.as_image().into_dyn().ok_or_else(|| "Gagal membaca bitmap halaman".to_string())?;
        let gray = image::DynamicImage::ImageLuma8(img.to_luma8());
        let jpg = encode_jpeg(&gray, rp.q)?;
        out.push((jpg, gray.width(), gray.height(), wpt, hpt));
    }
    Ok((out, total))
}

/// Render tiap halaman PDF pada DPI tertentu dan serahkan satu per satu ke `f(indeks, total, gambar)`.
pub(crate) fn for_each_pdf_page<F>(src: &[u8], dpi: u32, mut f: F) -> Result<usize, String>
where
    F: FnMut(usize, usize, image::DynamicImage) -> Result<(), String>,
{
    use pdfium_render::prelude::*;
    let _g = PDFIUM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let pdfium = bind_pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(src, None).map_err(err)?;
    let pages = doc.pages();
    let total = pages.len() as usize;
    if total == 0 {
        return Err("PDF tidak punya halaman".into());
    }
    for i in 0..pages.len() {
        let page = pages.get(i).map_err(err)?;
        let wpx = ((page.width().value / 72.0 * dpi as f32).round() as i32).max(1);
        let hpx = ((page.height().value / 72.0 * dpi as f32).round() as i32).max(1);
        let cfg = PdfRenderConfig::new().set_target_size(wpx, hpx);
        let bmp = page.render_with_config(&cfg).map_err(err)?;
        let img = bmp.as_image().into_dyn().ok_or_else(|| "Gagal membaca bitmap halaman".to_string())?;
        f(i as usize, total, img)?;
    }
    Ok(total)
}

/// Perkiraan ukuran hasil rasterisasi dari contoh beberapa halaman (cepat; dipakai saat pencarian).
fn raster_estimate(src: &[u8], rp: RasterProfile) -> Result<u64, String> {
    let (pages, total) = render_pages(src, rp, Some(12))?;
    if pages.is_empty() {
        return Err("PDF tidak punya halaman".into());
    }
    let sum: u64 = pages.iter().map(|p| p.0.len() as u64).sum();
    let avg = sum / pages.len() as u64;
    Ok(avg * total as u64 + 250 * total as u64 + 700)
}

/// Objek Name PDF dari string.
fn pdf_name(s: &str) -> lopdf::Object {
    lopdf::Object::Name(s.as_bytes().to_vec())
}

/// Bangun Dictionary dari pasangan (kunci, nilai). Pengganti makro `lopdf::dictionary!`,
/// yang di lopdf 0.34 error bila dipakai dengan koma di akhir (rekursi tanpa `$crate::`).
fn pdf_dict(items: Vec<(&str, lopdf::Object)>) -> lopdf::Dictionary {
    let mut d = lopdf::Dictionary::new();
    for (k, v) in items {
        d.set(k, v);
    }
    d
}

/// Render semua halaman dan rakit PDF baru (hasil akhir yang disimpan).
fn raster_pdf_full(src: &[u8], rp: RasterProfile) -> Result<Vec<u8>, String> {
    use lopdf::Object;
    let (pages, total) = render_pages(src, rp, None)?;
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::with_capacity(pages.len());
    for (jpg, w, h, wpt, hpt) in pages {
        let (wp, hp) = (wpt.round().max(1.0) as i64, hpt.round().max(1.0) as i64);
        let img_id = doc.add_object(lopdf::Stream::new(
            pdf_dict(vec![
                ("Type", pdf_name("XObject")),
                ("Subtype", pdf_name("Image")),
                ("Width", Object::Integer(w as i64)),
                ("Height", Object::Integer(h as i64)),
                ("ColorSpace", pdf_name("DeviceGray")),
                ("BitsPerComponent", Object::Integer(8)),
                ("Filter", pdf_name("DCTDecode")),
            ]),
            jpg,
        ));
        let content_id = doc.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            format!("q {wp} 0 0 {hp} 0 0 cm /Im0 Do Q").into_bytes(),
        ));
        let xobjects = pdf_dict(vec![("Im0", Object::Reference(img_id))]);
        let resources = pdf_dict(vec![("XObject", Object::Dictionary(xobjects))]);
        let page_id = doc.add_object(pdf_dict(vec![
            ("Type", pdf_name("Page")),
            ("Parent", Object::Reference(pages_id)),
            (
                "MediaBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(wp),
                    Object::Integer(hp),
                ]),
            ),
            ("Contents", Object::Reference(content_id)),
            ("Resources", Object::Dictionary(resources)),
        ]));
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(pdf_dict(vec![
            ("Type", pdf_name("Pages")),
            ("Kids", Object::Array(kids)),
            ("Count", Object::Integer(total as i64)),
        ])),
    );
    let catalog_id = doc.add_object(pdf_dict(vec![
        ("Type", pdf_name("Catalog")),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Pencarian profil + cache ukuran
// ---------------------------------------------------------------------------------------------
/// Jumlah level PDF "biasa" (Ghostscript atau mode bawaan); level rasterisasi dimulai setelahnya.
fn pdf_base_len() -> usize {
    if gs_path().is_some() { GS_LADDER.len() } else { PDF_LADDER.len() }
}

/// Jumlah profil pada tangga kompresi untuk jenis file ini.
fn ladder_len(kind: Kind) -> usize {
    match kind {
        Kind::Pdf => pdf_base_len() + if raster_ok() { RASTER_LADDER.len() } else { 0 },
        _ => LADDER.len(),
    }
}

fn render(path: &Path, kind: Kind, src: &[u8], idx: usize, fast: bool) -> Result<Vec<u8>, String> {
    let ext = ext_of(path);
    match kind {
        Kind::Image => compress_image(path, src, &ext, LADDER[idx]),
        Kind::Pdf => {
            let base = pdf_base_len();
            if idx >= base {
                raster_pdf_full(src, RASTER_LADDER[idx - base])
            } else {
                match gs_path() {
                    Some(gs) => compress_pdf_gs(gs, path, GS_LADDER[idx]),
                    None => compress_pdf(src, PDF_LADDER[idx]),
                }
            }
        }
        Kind::Office => compress_office(src, LADDER[idx], fast),
        Kind::Other => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            zip_single(&name, src, fast)
        }
    }
}

// (path, ukuran asli, indeks profil) -> ukuran hasil. Hanya dipakai selama pencarian (fast=true),
// jadi tidak pernah bentrok dengan hasil kompresi final (fast=false) yang dihitung terpisah.
static CACHE: LazyLock<Mutex<HashMap<(String, u64, usize), u64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn size_at(path: &Path, kind: Kind, src: &[u8], idx: usize) -> Result<u64, String> {
    let key = (path.to_string_lossy().into_owned(), src.len() as u64, idx);
    if let Some(v) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(*v);
    }
    let base = pdf_base_len();
    let len = if kind == Kind::Pdf && idx >= base {
        raster_estimate(src, RASTER_LADDER[idx - base])?
    } else {
        render(path, kind, src, idx, true)?.len() as u64
    };
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(key, len);
    Ok(len)
}

struct Found {
    idx: usize,
    len: u64,
    reachable: bool,
}

/// Cari profil teringan yang membuat ukuran <= target. Jika tidak ada, pakai yang paling agresif.
fn search(path: &Path, kind: Kind, src: &[u8], percent: u8) -> Result<Found, String> {
    let orig = src.len() as u64;
    let percent = percent.clamp(1, 95) as u64;
    let target = orig * (100 - percent) / 100;

    if kind == Kind::Other {
        let len = size_at(path, kind, src, 0)?;
        return Ok(Found { idx: 0, len, reachable: len <= target });
    }
    let last = ladder_len(kind) - 1;
    let worst = size_at(path, kind, src, last)?;
    if worst > target {
        return Ok(Found { idx: last, len: worst, reachable: false });
    }
    let (mut lo, mut hi) = (0usize, last);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if size_at(path, kind, src, mid)? <= target {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(Found { idx: lo, len: size_at(path, kind, src, lo)?, reachable: true })
}

fn profile_label(kind: Kind, idx: usize) -> String {
    if kind == Kind::Other {
        return "Dikemas ZIP".into();
    }
    if kind == Kind::Pdf {
        let base = pdf_base_len();
        if idx >= base {
            let r = RASTER_LADDER[idx - base];
            return format!("Halaman dijadikan gambar · {} DPI · abu-abu (teks tidak bisa dipilih)", r.dpi);
        }
        if gs_path().is_some() {
            let p = GS_LADDER[idx];
            return format!("Ghostscript · gambar {} DPI · kualitas ~{}%", p.dpi, p.q);
        }
        let p = PDF_LADDER[idx];
        return format!(
            "Mode bawaan · kualitas {}% · resolusi {}%{}",
            p.q,
            (p.scale * 100.0).round() as u32,
            if p.gray { " · abu-abu" } else { "" }
        );
    }
    let p = LADDER[idx];
    if p.scale < 0.999 {
        format!("Kualitas {}% · resolusi {}%", p.q, (p.scale * 100.0).round() as u32)
    } else {
        format!("Kualitas {}%", p.q)
    }
}

// ---------------------------------------------------------------------------------------------
// Command Tauri
// ---------------------------------------------------------------------------------------------
#[derive(Serialize)]
pub struct FileInfo {
    path: String,
    name: String,
    size: u64,
    kind: &'static str,
    ext: String,
}

#[tauri::command]
pub fn compress_file_info(paths: Vec<String>) -> Vec<FileInfo> {
    paths
        .into_iter()
        .filter_map(|p| {
            let pb = PathBuf::from(&p);
            let meta = std::fs::metadata(&pb).ok()?;
            if !meta.is_file() {
                return None; // folder tidak didukung
            }
            let ext = ext_of(&pb);
            Some(FileInfo {
                name: pb.file_name()?.to_string_lossy().into_owned(),
                size: meta.len(),
                kind: kind_of(&ext).name(),
                ext,
                path: p,
            })
        })
        .collect()
}

#[derive(Serialize)]
pub struct Estimate {
    orig: u64,
    size: u64,
    saved_pct: f32,
    reachable: bool,
    no_gain: bool,
    label: String,
    out_ext: String,
}

fn estimate_blocking(path: String, percent: u8) -> Result<Estimate, String> {
    let pb = PathBuf::from(&path);
    let src = std::fs::read(&pb).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
    let ext = ext_of(&pb);
    let kind = kind_of(&ext);
    let orig = src.len() as u64;
    if orig == 0 {
        return Err("File kosong".into());
    }
    if kind == Kind::Pdf {
        pdf_report(&pb, &src);
    }
    let f = search(&pb, kind, &src, percent)?;
    let no_gain = f.len >= orig;
    let size = if no_gain { orig } else { f.len };
    Ok(Estimate {
        orig,
        size,
        saved_pct: (1.0 - size as f32 / orig as f32) * 100.0,
        reachable: f.reachable && !no_gain,
        no_gain,
        label: profile_label(kind, f.idx),
        out_ext: out_ext(&ext, kind),
    })
}

/// Estimasi ukuran hasil untuk target `percent` (pengurangan ukuran, 1-95).
#[tauri::command]
pub async fn compress_estimate(path: String, percent: u8) -> Result<Estimate, String> {
    tauri::async_runtime::spawn_blocking(move || estimate_blocking(path, percent))
        .await
        .map_err(err)?
}

#[derive(Serialize)]
pub struct RunResult {
    path: String,
    out_path: String,
    orig: u64,
    size: u64,
    saved_pct: f32,
    /// "ok" | "no_gain"
    status: &'static str,
}

fn unique_output(src: &Path, ext: &str) -> PathBuf {
    let dir = src.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let mut candidate = dir.join(format!("{stem}-compressed.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-compressed ({n}).{ext}"));
        n += 1;
    }
    candidate
}

fn run_blocking(path: String, percent: u8) -> Result<RunResult, String> {
    let pb = PathBuf::from(&path);
    let src = std::fs::read(&pb).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
    let ext = ext_of(&pb);
    let kind = kind_of(&ext);
    let orig = src.len() as u64;
    let f = search(&pb, kind, &src, percent)?;
    // fast=false: ini hasil akhir yang disimpan ke disk, pakai kompresi maksimal (hanya sekali,
    // bukan di dalam loop pencarian, jadi tidak menambah lag).
    let bytes = render(&pb, kind, &src, f.idx, false)?;
    if bytes.len() as u64 >= orig {
        return Ok(RunResult { path, out_path: String::new(), orig, size: orig, saved_pct: 0.0, status: "no_gain" });
    }
    let out = unique_output(&pb, &out_ext(&ext, kind));
    std::fs::write(&out, &bytes).map_err(|e| format!("Gagal menyimpan: {e}"))?;
    Ok(RunResult {
        path,
        out_path: out.to_string_lossy().into_owned(),
        orig,
        size: bytes.len() as u64,
        saved_pct: (1.0 - bytes.len() as f32 / orig as f32) * 100.0,
        status: "ok",
    })
}

/// Kompres satu file dan simpan di samping file asli.
#[tauri::command]
pub async fn compress_run_one(path: String, percent: u8) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_blocking(path, percent))
        .await
        .map_err(err)?
}

/// Dialog pilih file (alternatif drag & drop).
#[tauri::command]
pub async fn compress_pick_files() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Pilih file untuk dikompres")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

/// Buka File Explorer dengan file hasil terpilih.
#[tauri::command]
pub fn compress_reveal(path: String) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(dir) = Path::new(&path).parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Siklus hidup jendela
//
// Jendela Compress TIDAK dihancurkan saat ditutup, hanya disembunyikan (hide) lalu dipakai ulang.
// Alasannya:
//  - `close()` dari thread selain main di Windows kadang baru diproses saat event loop
//    "terpancing" (mis. saat Settings dibuka), sehingga label "compress" masih terdaftar dan
//    shortcut berikutnya salah mengira jendela masih ada.
//  - Jendela transparan tanpa dekorasi bisa meninggalkan "hantu" visual bila di-close langsung.
//  - Listener drag & drop di frontend cukup dipasang sekali.
//
// Shortcut Ctrl+Alt+K: bila jendela tidak terlihat ATAU tidak fokus (mis. tertutup setelah
// Alt+Tab) -> tampilkan dan bawa ke depan. Hanya bila terlihat dan fokus -> sembunyikan.
// ---------------------------------------------------------------------------------------------
fn clear_caches() {
    // Lepas cache gambar/ukuran: window ini dipakai sesekali, jangan biarkan memori menumpuk
    // di antara sesi kompres.
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    IMG_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Sembunyikan (bukan destroy) jendela Compress, lalu reset UI-nya.
fn close_compress_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("compress") {
        // eval tidak butuh capability, jadi tetap jalan walau listener event bermasalah
        let _ = w.eval("window.__compressReset && window.__compressReset()");
        let _ = w.hide();
    }
    clear_caches();
}

fn show_compress_window(w: &tauri::WebviewWindow) {
    crate::foreground::bring_to_front(w);
    let _ = w.eval("window.__compressShown && window.__compressShown()");
}

#[tauri::command]
pub fn compress_close(app: AppHandle) {
    close_compress_window(&app);
}

#[tauri::command]
pub fn compress_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

/// Dipanggil shortcut global: buka jendela Compress di tengah layar, tampilkan ke depan bila
/// tersembunyi/tertutup, atau sembunyikan bila sedang terlihat dan fokus.
pub fn toggle_window(app: &AppHandle) {
    set_resource_dir(app);
    if let Some(w) = app.get_webview_window("compress") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            close_compress_window(app);
        } else {
            show_compress_window(&w);
        }
        return;
    }
    let app = app.clone();
    // dibangun dari thread lain agar tidak deadlock dengan event loop (Windows)
    std::thread::spawn(move || {
        let built = (|| -> tauri::Result<()> {
            // PENTING soal drag & drop: `drag_drop_enabled` yang diset lewat builder Rust (mis.
            // `.drag_drop_enabled(true)`) DIABAIKAN oleh Tauri — bug yang sudah dilaporkan
            // (tauri-apps/tauri#13761). Nilainya hanya benar-benar dipakai kalau window ini
            // dideklarasikan di tauri.conf.json (app.windows) dan dibangun lewat `from_config`.
            // Window "main" sengaja dragDropEnabled:false (dibutuhkan tauri_plugin_drag untuk
            // drag-keluar di shelf); window Compress ini sebaliknya butuh dragDropEnabled:true.
            // Tambahkan ke app.windows di tauri.conf.json:
            //   { "label": "compress", "create": false, "url": "compress.html", "dragDropEnabled": true }
            //
            // Event ke JS (onDragDropEvent) juga butuh capability: pastikan "compress" ada di
            // "windows" pada src-tauri/capabilities/default.json.
            let full_cfg = app.config();
            let win = match full_cfg.app.windows.iter().find(|w| w.label == "compress") {
                Some(c) => tauri::WebviewWindowBuilder::from_config(&app, c)?,
                None => {
                    eprintln!(
                        "compress: entri \"compress\" belum ada di app.windows pada tauri.conf.json — \
                         drag & drop file TIDAK akan berfungsi sampai ditambahkan (lihat komentar di toggle_window())."
                    );
                    tauri::WebviewWindowBuilder::new(&app, "compress", tauri::WebviewUrl::App("compress.html".into()))
                }
            };
            let w = win
                .title("Compress")
                .inner_size(840.0, 660.0)
                .center()
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .skip_taskbar(false)
                .resizable(false)
                .focused(true)
                .build()?;
            crate::foreground::bring_to_front(&w);
            Ok(())
        })();
        if let Err(e) = built {
            eprintln!("compress: gagal membuka jendela: {e}");
        }
    });
}