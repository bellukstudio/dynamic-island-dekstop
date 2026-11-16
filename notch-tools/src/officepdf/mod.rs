//! Shared Office document to PDF engine (DOCX / XLSX / PPTX), independent of UI frameworks.
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

/// Configure the optional application resource directory used to locate bundled fonts.
pub fn set_resource_dir(path: PathBuf) {
    let _ = RESOURCE_DIR.set(path);
}

fn resource_dir() -> Option<&'static PathBuf> {
    RESOURCE_DIR.get()
}

pub fn supports(ext: &str) -> bool {
    matches!(ext, "docx" | "xlsx" | "pptx")
}

/// Convert a DOCX, XLSX, or PPTX file to PDF bytes.
pub fn convert_to_pdf(path: &Path) -> Result<Vec<u8>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let src = std::fs::read(path).map_err(|e| format!("Tidak bisa membaca file: {e}"))?;
    convert_bytes(&ext, &src)
}

/// Convert Office document bytes to PDF bytes.
pub fn convert_bytes(ext: &str, src: &[u8]) -> Result<Vec<u8>, String> {
    if !supports(ext) {
        return Err(format!("Format .{ext} tidak didukung mesin PDF bawaan"));
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let fonts = font::Fonts::load()?;
        let pdf = pdf::Pdf::new(fonts);
        match ext {
            "docx" => docx::convert(src, pdf),
            "xlsx" => xlsx::convert(src, pdf),
            _ => pptx::convert(src, pdf),
        }
    }));
    match result {
        Ok(value) => value,
        Err(_) => Err("Konversi dokumen gagal (kesalahan internal)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{convert_bytes, supports};

    #[test]
    fn supports_only_the_three_open_xml_office_formats() {
        assert!(supports("docx"));
        assert!(supports("xlsx"));
        assert!(supports("pptx"));
        assert!(!supports("doc"));
        assert!(!supports("pdf"));
    }

    #[test]
    fn rejects_unsupported_format_before_parsing_input() {
        let error = convert_bytes("txt", b"not a document").unwrap_err();
        assert_eq!(error, "Format .txt tidak didukung mesin PDF bawaan");
    }
}
