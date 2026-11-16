//! Jendela Convert (Ctrl+Alt+Shift+K = Compress, Ctrl+Alt+Shift+C = Convert).
//!
//! - Gambar (jpg/png/webp/bmp/gif) -> gambar lain atau PDF
//! - PDF -> PNG/JPG per halaman (Pdfium, lewat compress::for_each_pdf_page)
//! - DOCX/XLSX/PPTX -> PDF lewat mesin bawaan (officepdf, tanpa LibreOffice)
//! - Format Office lama/lain (doc/xls/ppt/odt/ods/odp/rtf/txt/csv) -> PDF lewat LibreOffice, bila terpasang;
//!   LibreOffice juga jadi cadangan bila mesin bawaan gagal pada sebuah dokumen.
use serde::Serialize;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

use crate::compress::{
    decode_image, encode_jpeg, encode_png, for_each_pdf_page, no_window, raster_ok, set_resource_dir,
};
use crate::officepdf;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------------
// Jenis file & format tujuan
// ---------------------------------------------------------------------------------------------
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

const IMAGE_TARGETS: [&str; 6] = ["png", "jpg", "webp", "bmp", "gif", "pdf"];

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn canon(ext: &str) -> String {
    if ext == "jpeg" { "jpg".into() } else { ext.to_string() }
}

fn kind_of(ext: &str) -> Kind {
    match ext {
        "jpg" | "jpeg" | "png" | "bmp" | "webp" | "gif" => Kind::Image,
        "pdf" => Kind::Pdf,
        "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "odp" | "rtf" | "txt" | "csv" => Kind::Office,
        _ => Kind::Other,
    }
}

/// Mesin yang akan dipakai untuk Office -> PDF: "bawaan" (officepdf) atau "LibreOffice".
fn office_engine(ext: &str) -> Option<&'static str> {
    if officepdf::supports(ext) {
        Some("bawaan")
    } else if soffice().is_some() {
        Some("LibreOffice")
    } else {
        None
    }
}

/// Format tujuan yang tersedia untuk file ini + catatan bila tidak ada (mis. alat pendukung belum ada).
fn targets_for(kind: Kind, ext: &str) -> (Vec<&'static str>, Option<String>) {
    match kind {
        Kind::Image => {
            let me = canon(ext);
            (IMAGE_TARGETS.iter().copied().filter(|t| *t != me.as_str()).collect(), None)
        }
        Kind::Pdf => {
            if raster_ok() {
                (vec!["png", "jpg"], None)
            } else {
                (Vec::new(), Some("Butuh pdfium.dll (taruh di folder pdfium)".into()))
            }
        }
        Kind::Office => {
            if office_engine(ext).is_some() {
                (vec!["pdf"], None)
            } else {
                (Vec::new(), Some(format!("Format .{ext} butuh LibreOffice terpasang")))
            }
        }
        Kind::Other => (Vec::new(), Some("Format ini belum didukung".into())),
    }
}

fn default_for(kind: Kind, ext: &str, targets: &[&'static str]) -> &'static str {
    let pref = match kind {
        Kind::Image => if ext == "png" || ext == "bmp" { "jpg" } else { "png" },
        Kind::Pdf => "png",
        Kind::Office => "pdf",
        Kind::Other => "",
    };
    targets.iter().copied().find(|t| *t == pref).or_else(|| targets.first().copied()).unwrap_or("")
}

// ---------------------------------------------------------------------------------------------
// Pengodean gambar
// ---------------------------------------------------------------------------------------------
/// Ratakan transparansi ke latar putih (JPG/BMP/PDF tidak punya alpha).
fn flatten_white(img: &image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    if !img.color().has_alpha() {
        return DynamicImage::ImageRgb8(img.to_rgb8());
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

/// Paksa 8-bit (WebP tidak menerima 16-bit / luma-alpha tertentu).
fn to8(img: &image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    }
}

fn write_fmt(img: &image::DynamicImage, fmt: image::ImageFormat) -> Result<Vec<u8>, String> {
    let mut cur = Cursor::new(Vec::new());
    img.write_to(&mut cur, fmt).map_err(err)?;
    Ok(cur.into_inner())
}

fn pdf_name(s: &str) -> lopdf::Object {
    lopdf::Object::Name(s.as_bytes().to_vec())
}

fn pdf_dict(items: Vec<(&str, lopdf::Object)>) -> lopdf::Dictionary {
    let mut d = lopdf::Dictionary::new();
    for (k, v) in items {
        d.set(k, v);
    }
    d
}

/// Satu gambar -> PDF satu halaman A4 (otomatis portrait/landscape, gambar dimuat utuh di tengah).
fn image_to_pdf(img: &image::DynamicImage, q: u8) -> Result<Vec<u8>, String> {
    use lopdf::Object;
    let flat = flatten_white(img);
    let (w, h) = (flat.width(), flat.height());
    let jpg = encode_jpeg(&flat, q)?;

    let (pw, ph) = if w >= h { (842i64, 595i64) } else { (595i64, 842i64) };
    let margin = 24.0f32;
    let s = ((pw as f32 - 2.0 * margin) / w as f32).min((ph as f32 - 2.0 * margin) / h as f32);
    let (dw, dh) = (w as f32 * s, h as f32 * s);
    let (x, y) = ((pw as f32 - dw) / 2.0, (ph as f32 - dh) / 2.0);

    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let img_id = doc.add_object(lopdf::Stream::new(
        pdf_dict(vec![
            ("Type", pdf_name("XObject")),
            ("Subtype", pdf_name("Image")),
            ("Width", Object::Integer(w as i64)),
            ("Height", Object::Integer(h as i64)),
            ("ColorSpace", pdf_name("DeviceRGB")),
            ("BitsPerComponent", Object::Integer(8)),
            ("Filter", pdf_name("DCTDecode")),
        ]),
        jpg,
    ));
    let content = format!("q {dw:.2} 0 0 {dh:.2} {x:.2} {y:.2} cm /Im0 Do Q");
    let content_id = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), content.into_bytes()));
    let xobjects = pdf_dict(vec![("Im0", Object::Reference(img_id))]);
    let resources = pdf_dict(vec![("XObject", Object::Dictionary(xobjects))]);
    let page_id = doc.add_object(pdf_dict(vec![
        ("Type", pdf_name("Page")),
        ("Parent", Object::Reference(pages_id)),
        (
            "MediaBox",
            Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Integer(pw), Object::Integer(ph)]),
        ),
        ("Contents", Object::Reference(content_id)),
        ("Resources", Object::Dictionary(resources)),
    ]));
    doc.objects.insert(
        pages_id,
        Object::Dictionary(pdf_dict(vec![
            ("Type", pdf_name("Pages")),
            ("Kids", Object::Array(vec![Object::Reference(page_id)])),
            ("Count", Object::Integer(1)),
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

fn encode_as(img: &image::DynamicImage, target: &str, q: u8) -> Result<Vec<u8>, String> {
    use image::ImageFormat;
    match target {
        "jpg" => encode_jpeg(&flatten_white(img), q),
        "png" => encode_png(img),
        "bmp" => write_fmt(&flatten_white(img), ImageFormat::Bmp),
        "webp" => write_fmt(&to8(img), ImageFormat::WebP),
        "gif" => write_fmt(&image::DynamicImage::ImageRgba8(img.to_rgba8()), ImageFormat::Gif),
        "pdf" => image_to_pdf(img, q),
        _ => Err(format!("Format tujuan tidak dikenal: {target}")),
    }
}

// ---------------------------------------------------------------------------------------------
// LibreOffice (cadangan / format Office lama)
// ---------------------------------------------------------------------------------------------
fn find_soffice() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SOFFICE_PATH") {
        c.push(PathBuf::from(p));
    }
    if cfg!(target_os = "windows") {
        for root in ["C:\\Program Files\\LibreOffice", "C:\\Program Files (x86)\\LibreOffice"] {
            c.push(Path::new(root).join("program").join("soffice.exe"));
        }
    } else {
        for p in [
            "/usr/bin/soffice",
            "/usr/bin/libreoffice",
            "/usr/local/bin/soffice",
            "/snap/bin/libreoffice",
            "/opt/libreoffice/program/soffice",
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        ] {
            c.push(PathBuf::from(p));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            for n in ["soffice.exe", "soffice", "libreoffice"] {
                c.push(d.join(n));
            }
        }
    }
    c.into_iter().find(|p| p.is_file())
}

fn soffice() -> Option<&'static PathBuf> {
    static S: OnceLock<Option<PathBuf>> = OnceLock::new();
    S.get_or_init(|| {
        let p = find_soffice();
        match &p {
            Some(p) => eprintln!("convert: LibreOffice ditemukan: {}", p.display()),
            None => eprintln!("convert: LibreOffice tidak ditemukan (hanya DOCX/XLSX/PPTX lewat mesin bawaan)"),
        }
        p
    })
    .as_ref()
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

// Profil LibreOffice yang sama tidak boleh dipakai dua proses sekaligus: serialkan.
static SOFFICE_LOCK: Mutex<()> = Mutex::new(());
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn office_to_pdf_lo(input: &Path) -> Result<Vec<u8>, String> {
    let so = soffice().ok_or_else(|| "LibreOffice tidak ditemukan".to_string())?;
    let _g = SOFFICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir();
    let outdir = tmp.join(format!("notch-conv-{}-{}", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&outdir).map_err(|e| format!("Gagal membuat folder sementara: {e}"))?;
    // profil terpisah supaya tidak bentrok dengan LibreOffice yang sedang dibuka pengguna
    let profile = tmp.join("notch-lo-profile");

    let mut cmd = std::process::Command::new(so);
    if std::env::var_os("APPDIR").is_some() {
        cmd.env_remove("LD_LIBRARY_PATH");
    }
    let res = no_window(&mut cmd)
        .arg(format!("-env:UserInstallation={}", file_url(&profile)))
        .args(["--headless", "--norestore", "--nolockcheck", "--convert-to", "pdf", "--outdir"])
        .arg(&outdir)
        .arg(input)
        .output()
        .map_err(|e| format!("Gagal menjalankan LibreOffice: {e}"));

    let result = (|| {
        let res = res?;
        let pdf = std::fs::read_dir(&outdir)
            .map_err(err)?
            .flatten()
            .map(|e| e.path())
            .find(|p| ext_of(p) == "pdf");
        match pdf {
            Some(p) => std::fs::read(&p).map_err(|e| format!("Hasil LibreOffice tidak terbaca: {e}")),
            None => {
                let msg = String::from_utf8_lossy(&res.stderr);
                let msg = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("tidak ada keluaran PDF");
                Err(format!("LibreOffice gagal: {msg}"))
            }
        }
    })();
    let _ = std::fs::remove_dir_all(&outdir);
    result
}

/// Office -> PDF. DOCX/XLSX/PPTX: mesin bawaan dulu; bila gagal dan LibreOffice ada, coba LibreOffice.
/// Mengembalikan (byte PDF, nama mesin yang berhasil).
fn office_to_pdf(input: &Path, ext: &str) -> Result<(Vec<u8>, &'static str), String> {
    if officepdf::supports(ext) {
        match officepdf::convert_to_pdf(input) {
            Ok(b) => return Ok((b, "bawaan")),
            Err(e) => {
                if soffice().is_none() {
                    return Err(e);
                }
                eprintln!("convert: mesin bawaan gagal ({e}), mencoba LibreOffice");
            }
        }
    }
    office_to_pdf_lo(input).map(|b| (b, "LibreOffice"))
}

// ---------------------------------------------------------------------------------------------
// Command Tauri
// ---------------------------------------------------------------------------------------------
#[derive(Serialize)]
pub struct ConvInfo {
    path: String,
    name: String,
    size: u64,
    kind: &'static str,
    ext: String,
    targets: Vec<&'static str>,
    default_target: &'static str,
    note: Option<String>,
    /// Mesin Office -> PDF ("bawaan" / "LibreOffice"); None untuk jenis file lain.
    engine: Option<&'static str>,
}

fn describe(p: String) -> Option<ConvInfo> {
    let pb = PathBuf::from(&p);
    let meta = std::fs::metadata(&pb).ok()?;
    if !meta.is_file() {
        return None; // folder tidak didukung
    }
    let ext = ext_of(&pb);
    let kind = kind_of(&ext);
    let (targets, note) = targets_for(kind, &ext);
    let default_target = default_for(kind, &ext, &targets);
    let engine = if kind == Kind::Office { office_engine(&ext) } else { None };
    Some(ConvInfo {
        name: pb.file_name()?.to_string_lossy().into_owned(),
        size: meta.len(),
        kind: kind.name(),
        ext,
        targets,
        default_target,
        note,
        engine,
        path: p,
    })
}

#[tauri::command]
pub async fn convert_file_info(paths: Vec<String>) -> Vec<ConvInfo> {
    // async + blocking pool: pengecekan Pdfium/LibreOffice pertama kali bisa agak lama
    tauri::async_runtime::spawn_blocking(move || paths.into_iter().filter_map(describe).collect())
        .await
        .unwrap_or_default()
}

#[derive(Serialize)]
pub struct ConvResult {
    path: String,
    out_path: String,
    count: usize,
    size: u64,
    /// Mesin yang dipakai untuk Office -> PDF; string kosong untuk jenis lain.
    engine: &'static str,
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut c = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while c.exists() {
        c = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    c
}

fn unique_dir(dir: &Path, stem: &str) -> PathBuf {
    let mut c = dir.join(format!("{stem} (halaman)"));
    let mut n = 2;
    while c.exists() {
        c = dir.join(format!("{stem} (halaman {n})"));
        n += 1;
    }
    c
}

fn write_out(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("Gagal menyimpan: {e}"))
}

fn run_blocking(path: String, target: String, quality: u8, dpi: u32) -> Result<ConvResult, String> {
    let pb = PathBuf::from(&path);
    let ext = ext_of(&pb);
    let kind = kind_of(&ext);
    let (targets, note) = targets_for(kind, &ext);
    if !targets.iter().any(|t| *t == target.as_str()) {
        return Err(note.unwrap_or_else(|| format!("Tidak bisa mengonversi .{ext} ke {target}")));
    }
    let dir = pb.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = pb.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let quality = quality.clamp(40, 100);

    match kind {
        Kind::Image => {
            let src = std::fs::read(&pb).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
            let img = decode_image(&src, true)?;
            let bytes = encode_as(&img, &target, quality)?;
            let out = unique_path(&dir, &stem, &target);
            write_out(&out, &bytes)?;
            Ok(ConvResult { path, out_path: out.to_string_lossy().into_owned(), count: 1, size: bytes.len() as u64, engine: "" })
        }
        Kind::Pdf => {
            let src = std::fs::read(&pb).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
            let dpi = dpi.clamp(36, 400);
            let mut out_dir: Option<PathBuf> = None;
            let mut first: Option<PathBuf> = None;
            let mut size = 0u64;
            let mut count = 0usize;
            for_each_pdf_page(&src, dpi, |i, total, img| {
                let bytes = encode_as(&img, &target, quality)?;
                let out = if total == 1 {
                    unique_path(&dir, &stem, &target)
                } else {
                    if out_dir.is_none() {
                        let d = unique_dir(&dir, &stem);
                        std::fs::create_dir_all(&d).map_err(|e| format!("Gagal membuat folder: {e}"))?;
                        out_dir = Some(d);
                    }
                    out_dir.as_ref().unwrap().join(format!("{stem}-{:03}.{target}", i + 1))
                };
                write_out(&out, &bytes)?;
                size += bytes.len() as u64;
                count += 1;
                if first.is_none() {
                    first = Some(out);
                }
                Ok(())
            })?;
            let out_path = out_dir.or(first).ok_or_else(|| "PDF tidak punya halaman".to_string())?;
            Ok(ConvResult { path, out_path: out_path.to_string_lossy().into_owned(), count, size, engine: "" })
        }
        Kind::Office => {
            let (bytes, engine) = office_to_pdf(&pb, &ext)?;
            let out = unique_path(&dir, &stem, "pdf");
            write_out(&out, &bytes)?;
            Ok(ConvResult { path, out_path: out.to_string_lossy().into_owned(), count: 1, size: bytes.len() as u64, engine })
        }
        Kind::Other => Err("Format ini belum didukung".into()),
    }
}

/// Konversi satu file ke `target`; hasil disimpan di samping file asli (tidak menimpa apa pun).
#[tauri::command]
pub async fn convert_run_one(path: String, target: String, quality: u8, dpi: u32) -> Result<ConvResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_blocking(path, target, quality, dpi))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn convert_pick_files() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Pilih file untuk dikonversi")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn convert_reveal(path: String) {
    crate::compress::compress_reveal(path);
}

// ---------------------------------------------------------------------------------------------
// Siklus hidup jendela
// ---------------------------------------------------------------------------------------------
pub fn close_convert_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("convert") {
        if let Err(error) = w.destroy() {
            eprintln!("convert: gagal menutup jendela: {error}");
            if let Err(hide_error) = w.hide() {
                eprintln!("convert: fallback menyembunyikan jendela juga gagal: {hide_error}");
            }
        }
    }
}

fn show_convert_window(w: &tauri::WebviewWindow) {
    crate::foreground::bring_to_front(w);
    let _ = w.eval("window.__convertShown && window.__convertShown()");
}

#[tauri::command]
pub fn convert_close(app: AppHandle) {
    close_convert_window(&app);
}

#[tauri::command]
pub fn convert_drag(window: tauri::WebviewWindow) {
    let _ = window.start_dragging();
}

/// Dipanggil shortcut global (Ctrl+Alt+Shift+C): tampilkan, atau sembunyikan bila sedang terlihat dan fokus.
pub fn toggle_window(app: &AppHandle) {
    set_resource_dir(app); // gs / pdfium
    officepdf::set_resource_dir(app); // font di resources/fonts
    if let Some(w) = app.get_webview_window("convert") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            close_convert_window(app);
        } else {
            show_convert_window(&w);
        }
        return;
    }
    let app = app.clone();
    // dibangun dari thread lain agar tidak deadlock dengan event loop (Windows)
    std::thread::spawn(move || {
        let built = (|| -> tauri::Result<()> {
            // Sama seperti Compress: dragDropEnabled hanya berlaku bila window dideklarasikan di
            // tauri.conf.json (app.windows) dengan "create": false dan dibangun lewat from_config.
            let full_cfg = app.config();
            let win = match full_cfg.app.windows.iter().find(|w| w.label == "convert") {
                Some(c) => tauri::WebviewWindowBuilder::from_config(&app, c)?,
                None => {
                    eprintln!(
                        "convert: entri \"convert\" belum ada di app.windows pada tauri.conf.json — \
                         drag & drop file TIDAK akan berfungsi sampai ditambahkan."
                    );
                    tauri::WebviewWindowBuilder::new(&app, "convert", tauri::WebviewUrl::App("convert.html".into()))
                }
            };
            let w = win
                .title("Convert")
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
            eprintln!("convert: gagal membuka jendela: {e}");
        }
    });
}