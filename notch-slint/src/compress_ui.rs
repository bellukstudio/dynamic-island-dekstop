//! Halaman Kompres: pilih file dan jalankan kompresi batch di thread latar.

use crate::*;

pub(crate) fn setup(win: &NotchWindow) {
let compress_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
{
    let weak = win.as_weak();
    win.on_compress_pick(move || {
        let selected = rfd::FileDialog::new()
            .add_filter("Gambar, PDF & Office", &["jpg", "jpeg", "png", "bmp", "pdf", "docx", "xlsx", "pptx"])
            .pick_files().unwrap_or_default();
        if selected.is_empty() { return; }
        let paths = selected.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>();
        if let Some(w) = weak.upgrade() {
            w.set_compress_status(format!("{} file dipilih untuk kompresi", paths.len()).into());
            w.set_compress_selected_path(paths.join("\n").into());
        }
    });
}
{
    let weak = win.as_weak();
    let running = compress_running.clone();
    win.on_compress_run(move |raw_path| {
        let mut seen = std::collections::HashSet::new();
        let paths = raw_path.split('\n').map(str::trim).filter(|path| !path.is_empty())
            .filter(|path| seen.insert(path.to_ascii_lowercase()))
            .map(str::to_string).collect::<Vec<_>>();
        if paths.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_compress_status("Pilih minimal satu file sebelum kompresi".into()); }
            return;
        }
        if running.swap(true, std::sync::atomic::Ordering::AcqRel) {
            if let Some(w) = weak.upgrade() { w.set_compress_status("Batch kompresi sedang berjalan".into()); }
            return;
        }
        let weak = weak.clone();
        let running_worker = running.clone();
        std::thread::spawn(move || {
            let total = paths.len();
            let mut success = 0usize;
            let mut failures = Vec::new();
            let mut summaries = Vec::new();
            for (index, path_text) in paths.iter().enumerate() {
                let result = compress::compress_file(std::path::Path::new(path_text), 30);
                match result {
                    Ok((output, before, after)) => {
                        success += 1;
                        let saved = before.saturating_sub(after);
                        summaries.push(format!("✓ {} · {} KB → {} KB · hemat {} KB",
                            std::path::Path::new(path_text).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path_text.clone()),
                            before / 1024, after / 1024, saved / 1024));
                        let _ = output;
                    }
                    Err(error) => failures.push(format!("{}: {error}",
                        std::path::Path::new(path_text).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path_text.clone()))),
                }
                let status = format!("Memproses {}/{} · {} berhasil · {} gagal",
                    index + 1, total, success, failures.len());
                let weak_status = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_status.upgrade() { w.set_compress_status(status.into()); }
                });
            }
            running_worker.store(false, std::sync::atomic::Ordering::Release);
            summaries.extend(failures.into_iter().map(|failure| format!("✕ {failure}")));
            let message = format!("Batch selesai: {success}/{total} berhasil\n{}", summaries.join("\n"));
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { w.set_compress_status(message.into()); }
            });
        });
    });
}
}
