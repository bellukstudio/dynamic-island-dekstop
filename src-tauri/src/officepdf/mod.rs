#![allow(dead_code)]
//! Mesin PDF ringan untuk DOCX / XLSX / PPTX: murni Rust, tanpa LibreOffice.
//! Jalan di Windows dan Linux. Hanya butuh satu keluarga font TrueType (lihat font.rs).
mod docx;
mod font;
mod pdf;
mod pptx;
mod text;
mod util;
mod xlsx;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Panggil sekali (mis. dari convert::toggle_window) agar font di `resources/fonts` ikut dicari.
pub(crate) fn set_resource_dir(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Ok(d) = app.path().resource_dir() {
        let _ = RESOURCE_DIR.set(d);
    }
}
pub(crate) fn resource_dir() -> Option<&'static PathBuf> {
    RESOURCE_DIR.get()
}

pub(crate) fn supports(ext: &str) -> bool {
    matches!(ext, "docx" | "xlsx" | "pptx")
}

/// Konversi DOCX/XLSX/PPTX di `path` menjadi byte PDF.
pub(crate) fn convert_to_pdf(path: &Path) -> Result<Vec<u8>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let src = std::fs::read(path).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
    convert_bytes(&ext, &src)
}

pub(crate) fn convert_bytes(ext: &str, src: &[u8]) -> Result<Vec<u8>, String> {
    if !supports(ext) {
        return Err(format!("Format .{ext} tidak didukung mesin PDF bawaan"));
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let fonts = font::Fonts::load()?;
        let pdf = pdf::Pdf::new(fonts);
        match ext {
            "docx" => docx::convert(src, pdf),
            "xlsx" => xlsx::convert(src, pdf),
            _ => pptx::convert(src, pdf),
        }
    }));
    match r {
        Ok(v) => v,
        Err(_) => Err("Konversi dokumen gagal (kesalahan internal)".into()),
    }
}
