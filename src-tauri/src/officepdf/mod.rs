//! Tauri adapter for the shared, UI-independent OfficePDF engine.
use std::path::Path;

pub(crate) fn set_resource_dir(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Ok(path) = app.path().resource_dir() {
        notch_tools::officepdf::set_resource_dir(path);
    }
}

pub(crate) fn supports(ext: &str) -> bool {
    notch_tools::officepdf::supports(ext)
}

pub(crate) fn convert_to_pdf(path: &Path) -> Result<Vec<u8>, String> {
    notch_tools::officepdf::convert_to_pdf(path)
}

#[allow(dead_code)]
pub(crate) fn convert_bytes(ext: &str, src: &[u8]) -> Result<Vec<u8>, String> {
    notch_tools::officepdf::convert_bytes(ext, src)
}
