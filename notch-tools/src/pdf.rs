//! Pdfium-backed PDF page rendering shared by the Slint and Tauri frontends.

use image::DynamicImage;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();
static PDFIUM_LOCK: Mutex<()> = Mutex::new(());

trait IntoDynamicImage {
    fn into_dynamic_image(self) -> Option<DynamicImage>;
}

impl IntoDynamicImage for DynamicImage {
    fn into_dynamic_image(self) -> Option<DynamicImage> { Some(self) }
}

impl<E> IntoDynamicImage for Result<DynamicImage, E> {
    fn into_dynamic_image(self) -> Option<DynamicImage> { self.ok() }
}

/// Configure the application's resource directory before rendering PDF pages.
pub fn set_resource_dir(path: PathBuf) {
    let _ = RESOURCE_DIR.set(path);
}

fn pdfium_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = std::env::var_os("NOTCH_PDFIUM_DIR") {
        dirs.push(PathBuf::from(path));
    }
    if let Some(resource_dir) = RESOURCE_DIR.get() {
        dirs.push(resource_dir.join("pdfium"));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|path| path.parent().map(PathBuf::from)) {
        dirs.push(exe_dir.join("pdfium"));
        dirs.push(exe_dir.join("resources").join("pdfium"));
        dirs.push(exe_dir);
    }
    dirs
}

fn bind_pdfium() -> Result<pdfium_render::prelude::Pdfium, String> {
    use pdfium_render::prelude::Pdfium;
    for directory in pdfium_dirs() {
        let library = Pdfium::pdfium_platform_library_name_at_path(&directory);
        if let Ok(binding) = Pdfium::bind_to_library(library) {
            return Ok(Pdfium::new(binding));
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|error| format!("Pdfium tidak ditemukan. Letakkan DLL di folder pdfium atau atur NOTCH_PDFIUM_DIR: {error}"))
}

/// Returns whether Pdfium can currently be loaded from the app bundle or system.
pub fn is_available() -> bool {
    let _guard = PDFIUM_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    bind_pdfium().is_ok()
}


/// Render one zero-based page without decoding the remaining pages.
pub fn render_page(source: &[u8], page_index: usize, dpi: u32) -> Result<(DynamicImage, usize), String> {
    use pdfium_render::prelude::PdfRenderConfig;
    let _guard = PDFIUM_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let pdfium = bind_pdfium()?;
    let document = pdfium.load_pdf_from_byte_slice(source, None)
        .map_err(|error| format!("PDF tidak dapat dibuka: {error}"))?;
    let pages = document.pages();
    let total = pages.len() as usize;
    if page_index >= total {
        return Err(format!("Halaman {} tidak tersedia (total {total})", page_index + 1));
    }
    let page = pages.get(page_index as u16).map_err(|error| error.to_string())?;
    let dpi = dpi.clamp(36, 600);
    let width = ((page.width().value / 72.0 * dpi as f32).round() as i32).max(1);
    let height = ((page.height().value / 72.0 * dpi as f32).round() as i32).max(1);
    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_size(width, height))
        .map_err(|error| error.to_string())?;
    let image = bitmap.as_image().into_dynamic_image()
        .ok_or_else(|| "Gagal membaca bitmap halaman PDF".to_string())?;
    Ok((image, total))
}

/// Render all pages sequentially. Pdfium access is serialized because its bindings are not
/// thread-safe. The callback receives a zero-based page index, total pages, and rendered image.
pub fn for_each_page<F>(source: &[u8], dpi: u32, mut callback: F) -> Result<usize, String>
where
    F: FnMut(usize, usize, DynamicImage) -> Result<(), String>,
{
    use pdfium_render::prelude::PdfRenderConfig;
    let _guard = PDFIUM_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let pdfium = bind_pdfium()?;
    let document = pdfium.load_pdf_from_byte_slice(source, None)
        .map_err(|error| format!("PDF tidak dapat dibuka: {error}"))?;
    let pages = document.pages();
    let total = pages.len() as usize;
    if total == 0 {
        return Err("PDF tidak punya halaman".into());
    }
    let dpi = dpi.clamp(36, 600);
    for index in 0..pages.len() {
        let page = pages.get(index).map_err(|error| error.to_string())?;
        let width = ((page.width().value / 72.0 * dpi as f32).round() as i32).max(1);
        let height = ((page.height().value / 72.0 * dpi as f32).round() as i32).max(1);
        let config = PdfRenderConfig::new().set_target_size(width, height);
        let bitmap = page.render_with_config(&config).map_err(|error| error.to_string())?;
        let image = bitmap.as_image().into_dynamic_image()
            .ok_or_else(|| "Gagal membaca bitmap halaman PDF".to_string())?;
        callback(index as usize, total, image)?;
    }
    Ok(total)
}


/** Render a representative sample of pages for fast size estimation. */
pub fn for_each_page_sample<F>(
    source: &[u8],
    dpi: u32,
    sample_count: usize,
    mut callback: F,
) -> Result<usize, String>
where
    F: FnMut(usize, usize, DynamicImage) -> Result<(), String>,
{
    use pdfium_render::prelude::PdfRenderConfig;
    let _guard = PDFIUM_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let pdfium = bind_pdfium()?;
    let document = pdfium.load_pdf_from_byte_slice(source, None)
        .map_err(|error| format!("PDF tidak dapat dibuka: {error}"))?;
    let pages = document.pages();
    let total = pages.len() as usize;
    if total == 0 {
        return Err("PDF tidak punya halaman".into());
    }
    let indices: Vec<u16> = if sample_count > 0 && total > sample_count {
        (0..sample_count).map(|index| (index * total / sample_count) as u16).collect()
    } else {
        (0..pages.len()).collect()
    };
    let dpi = dpi.clamp(36, 600);
    for index in indices {
        let page = pages.get(index).map_err(|error| error.to_string())?;
        let width = ((page.width().value / 72.0 * dpi as f32).round() as i32).max(1);
        let height = ((page.height().value / 72.0 * dpi as f32).round() as i32).max(1);
        let config = PdfRenderConfig::new().set_target_size(width, height);
        let bitmap = page.render_with_config(&config).map_err(|error| error.to_string())?;
        let image = bitmap.as_image().into_dynamic_image()
            .ok_or_else(|| "Gagal membaca bitmap halaman PDF".to_string())?;
        callback(index as usize, total, image)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dpi_is_clamped_to_safe_range() {
        assert_eq!(36u32.clamp(36, 600), 36);
        assert_eq!(900u32.clamp(36, 600), 600);
    }
}
