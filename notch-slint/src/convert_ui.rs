//! Halaman Konversi: antrean file, preview dokumen, jeda/batal/ulang, riwayat.

use crate::*;

/// State Konversi yang juga dibutuhkan penangan drag-drop jendela (Shelf/Convert).
pub(crate) struct ConvertHandles {
    pub selected_paths: Rc<RefCell<Vec<String>>>,
    pub last_output: Arc<Mutex<Option<std::path::PathBuf>>>,
    pub running: Arc<AtomicBool>,
    pub preview_page: Rc<Cell<usize>>,
    pub preview_path: Rc<RefCell<String>>,
    pub preview_dpi: Rc<Cell<u32>>,
}

pub(crate) fn setup(win: &NotchWindow) -> ConvertHandles {
let convert_selected_paths = Rc::new(RefCell::new(Vec::<String>::new()));
let convert_preview_page = Rc::new(Cell::new(0usize));
let convert_preview_dpi = Rc::new(Cell::new(110u32));
let convert_preview_path = Rc::new(RefCell::new(String::new()));
let convert_last_output = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
let convert_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
let convert_cancel_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
let convert_pause_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
let convert_queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
let convert_failed_files = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
let convert_file_statuses = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
let convert_active_file = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
let convert_cancel_active_file = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
let (restored_queue, restored_statuses) = load_convert_queue_state();
*convert_queue.lock().unwrap_or_else(|error| error.into_inner()) = restored_queue;
*convert_file_statuses.lock().unwrap_or_else(|error| error.into_inner()) = restored_statuses;
let initial_output_dir = load_convert_output_dir();
let convert_output_dir = Rc::new(RefCell::new(initial_output_dir.clone()));
win.set_convert_output_dir_text(initial_output_dir.into());
win.set_convert_overrides_text(load_convert_overrides().into());
{
    let queue_text = format_convert_file_statuses(&convert_file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
    win.set_convert_queue_text(queue_text.into());
    let restored_paths = convert_queue.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect::<Vec<_>>();
    if let Some(first) = restored_paths.first() {
        *convert_selected_paths.borrow_mut() = restored_paths.clone();
        win.set_convert_selected_path(first.clone().into());
        win.set_convert_files_text(format_convert_selection(&restored_paths).into());
        win.set_convert_status(format!("{} file antrean dipulihkan dari sesi sebelumnya", restored_paths.len()).into());
    }
}
{
    let weak = win.as_weak();
    win.on_convert_history_export(move || {
        let Some(path) = rfd::FileDialog::new().set_file_name("riwayat-konversi.csv").save_file() else { return; };
        let result = std::fs::write(&path, convert_history_csv());
        if let Some(w) = weak.upgrade() {
            w.set_convert_status(match result {
                Ok(()) => format!("Riwayat berhasil diekspor: {}", path.display()).into(),
                Err(error) => format!("Gagal mengekspor riwayat: {error}").into(),
            });
        }
    });
}
{
    let weak = win.as_weak();
    let selected_paths = convert_selected_paths.clone();
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let running = convert_running.clone();
    win.on_convert_add_files(move || {
        let selected = rfd::FileDialog::new()
            .add_filter("Gambar, PDF & Office", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "pdf", "docx", "xlsx", "pptx", "doc", "xls", "ppt", "odt", "ods", "odp", "rtf", "txt", "csv"])
            .pick_files().unwrap_or_default().into_iter()
            .map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>();
        if selected.is_empty() { return; }
        let mut added = 0usize;
        {
            let mut pending = queue.lock().unwrap_or_else(|error| error.into_inner());
            let mut file_statuses = statuses.lock().unwrap_or_else(|error| error.into_inner());
            for path in &selected {
                if pending.contains(path) || file_statuses.iter().any(|(known, status)| known == path && (status == "Menunggu" || status == "Diproses")) { continue; }
                pending.push_back(path.clone());
                file_statuses.push((path.clone(), "Menunggu".to_string()));
                added += 1;
            }
        }
        if !running.load(std::sync::atomic::Ordering::Acquire) {
            *selected_paths.borrow_mut() = queue.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect();
        }
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            let all = selected_paths.borrow().clone();
            if let Some(first) = all.first() { w.set_convert_selected_path(first.clone().into()); }
            w.set_convert_files_text(format_convert_selection(&all).into());
            w.set_convert_queue_text(format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner())).into());
            w.set_convert_status(format!("{added} file baru ditambahkan ke antrean").into());
        }
    });
}
{
    let weak = win.as_weak();
    let selected_paths = convert_selected_paths.clone();
    let last_output = convert_last_output.clone();
    let preview_page = convert_preview_page.clone();
    let preview_dpi = convert_preview_dpi.clone();
    let preview_path = convert_preview_path.clone();
    let weak_preview = weak.clone();
    win.on_convert_pick(move || {
        let selected = rfd::FileDialog::new()
            .add_filter("Gambar, PDF & Office", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "pdf", "docx", "xlsx", "pptx", "doc", "xls", "ppt", "odt", "ods", "odp", "rtf", "txt", "csv"])
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if let (Some(first), Some(w)) = (selected.first(), weak.upgrade()) {
            *selected_paths.borrow_mut() = selected.clone();
            w.set_convert_history_text(format_convert_history().into());
            w.set_convert_files_text(format_convert_selection(&selected).into());
            preview_page.set(0);
            preview_dpi.set(110);
            w.set_convert_preview_zoom_text("110 DPI".into());
            *preview_path.borrow_mut() = first.clone();
            w.set_convert_preview_page_text("Memuat preview…".into());
            w.set_convert_preview_total(1);
            w.set_convert_preview_status("".into());
            request_convert_document_preview(weak_preview.clone(), first.clone(), 0, preview_dpi.get());
            w.set_convert_result_preview(Image::default());
            *last_output.lock().unwrap_or_else(|error| error.into_inner()) = None;
            w.set_convert_selected_path(first.clone().into());
            w.set_convert_progress_text("".into());
            w.set_convert_status(format!("Dipilih {} file · target auto menyesuaikan tiap file", selected.len()).into());
        }
    });
}
{
    let weak = win.as_weak();
    let selected_paths = convert_selected_paths.clone();
    let last_output = convert_last_output.clone();
    let running = convert_running.clone();
    let cancel_requested = convert_cancel_requested.clone();
    let pause_requested = convert_pause_requested.clone();
    let queue = convert_queue.clone();
    let failed_files = convert_failed_files.clone();
    let file_statuses = convert_file_statuses.clone();
    let active_file = convert_active_file.clone();
    let cancel_active_file = convert_cancel_active_file.clone();
    let output_dir = convert_output_dir.clone();
    win.on_convert_run(move |raw_path, raw_format, raw_quality, raw_dpi, raw_overrides| {
        let path = raw_path.to_string();
        let format = raw_format.to_string();
        let overrides = raw_overrides.to_string();
        save_convert_overrides(&overrides);
        let output_dir = output_dir.borrow().clone();
        let quality = raw_quality.to_string().parse::<u8>().unwrap_or(90).clamp(40, 100);
        let dpi = raw_dpi.to_string().parse::<u32>().unwrap_or(150).clamp(36, 400);
        let mut paths = selected_paths.borrow().clone();
        if paths.first().map(String::as_str) != Some(path.as_str()) {
            paths = if path.trim().is_empty() { Vec::new() } else { vec![path] };
        }
        if paths.is_empty() {
            if let Some(w) = weak.upgrade() {
                w.set_convert_status("Pilih file terlebih dahulu".into());
            }
            return;
        }
        if running.compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        ).is_err() {
            if let Some(w) = weak.upgrade() {
                w.set_convert_status("Konversi sedang berjalan".into());
            }
            return;
        }
        *last_output.lock().unwrap_or_else(|error| error.into_inner()) = None;
        cancel_requested.store(false, std::sync::atomic::Ordering::Release);
        pause_requested.store(false, std::sync::atomic::Ordering::Release);
        let total = paths.len();
        *queue.lock().unwrap_or_else(|error| error.into_inner()) = paths.iter().cloned().collect();
        *file_statuses.lock().unwrap_or_else(|error| error.into_inner()) = paths.iter().map(|path| (path.clone(), "Menunggu".to_string())).collect();
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
        failed_files.lock().unwrap_or_else(|error| error.into_inner()).clear();
        *active_file.lock().unwrap_or_else(|error| error.into_inner()) = None;
        cancel_active_file.store(false, std::sync::atomic::Ordering::Release);
        if let Some(w) = weak.upgrade() {
            w.set_convert_progress_text("0%".into());
            w.set_convert_progress_value(0.0);
            w.set_convert_eta_text("Menyiapkan antrean…".into());
            let queue_text = format_convert_file_statuses(&file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
            w.set_convert_queue_text(queue_text.into());
        }
        let weak = weak.clone();
        let cancel_requested = cancel_requested.clone();
        let pause_requested = pause_requested.clone();
        let queue = queue.clone();
        let failed_files = failed_files.clone();
        let file_statuses = file_statuses.clone();
        let active_file = active_file.clone();
        let cancel_active_file = cancel_active_file.clone();
        let last_output = last_output.clone();
        let running_worker = running.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let total = queue.lock().unwrap_or_else(|error| error.into_inner()).len();
            let mut processed = 0usize;
            let mut completed = 0usize;
            let mut failures = Vec::new();
            let mut outputs = Vec::new();
            loop {
                while pause_requested.load(std::sync::atomic::Ordering::Acquire)
                    && !cancel_requested.load(std::sync::atomic::Ordering::Acquire)
                    && !queue.lock().unwrap_or_else(|error| error.into_inner()).is_empty()
                {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if cancel_requested.load(std::sync::atomic::Ordering::Acquire) { break; }
                let next = queue.lock().unwrap_or_else(|error| error.into_inner()).pop_front();
                let Some(path) = next else { break; };
                cancel_active_file.store(false, std::sync::atomic::Ordering::Release);
                *active_file.lock().unwrap_or_else(|error| error.into_inner()) = Some(path.clone());
                {
                    let mut statuses = file_statuses.lock().unwrap_or_else(|error| error.into_inner());
                    if let Some(item) = statuses.iter_mut().find(|(item_path, _)| item_path == &path) {
                        item.1 = "Diproses".to_string();
                    }
                }
                save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
                let queue_text = format_convert_file_statuses(&file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
                let weak_queue = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak_queue.upgrade() {
                        window.set_convert_queue_text(queue_text.into());
                    }
                });
                let index = processed;
                processed += 1;
                let display_name = std::path::Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let progress = format!("Mengonversi {}/{}: {}", index + 1, total, display_name);
                let weak_progress = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_progress.upgrade() {
                        w.set_convert_status(progress.into());
                    }
                });
                let source = std::path::Path::new(&path);
                let file_quality = convert_quality_override(source, &overrides).unwrap_or(quality);
                let result: Result<(std::path::PathBuf, String), String> = (|| {
                    let target = if let Some(override_target) = convert_target_override(source, &overrides) {
                        override_target
                    } else if format.trim().is_empty() || format.eq_ignore_ascii_case("auto") {
                        convert::default_target(source)
                            .ok_or_else(|| format!("Format sumber tidak didukung: {}", source.display()))?
                            .to_string()
                    } else {
                        format.trim().trim_start_matches('.').to_ascii_lowercase()
                    };
                    let available = convert::available_targets(source);
                    if !available.iter().any(|candidate| *candidate == target.as_str()) {
                        let supported = if available.is_empty() {
                            "tidak ada format tujuan yang tersedia (periksa Pdfium/LibreOffice)"
                                .to_string()
                        } else {
                            available.join(", ")
                        };
                        return Err(format!(
                            "Format tujuan {target} tidak tersedia untuk {}. Pilihan: {supported}",
                            source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone())
                        ));
                    }
                    if source.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("pdf")).unwrap_or(false) {
                        let weak_page_progress = weak.clone();
                        let queue_page_progress = queue.clone();
                        let display_name_progress = display_name.clone();
                        convert::convert_pdf_to_images_with_progress(source, &target, dpi, file_quality, move |page, pages| {
                            let fraction = page as f32 / pages.max(1) as f32;
                            let remaining = queue_page_progress.lock().unwrap_or_else(|error| error.into_inner()).len();
                            let effective_total = (index + 1 + remaining).max(1);
                            let overall = (((index as f32 + fraction) / effective_total as f32) * 100.0).clamp(0.0, 100.0);
                            let progress_text = format!("{:.0}% · file {}/{} · halaman {}/{}", overall, index + 1, effective_total, page, pages);
                            let status_text = format!("Merender halaman PDF {page}/{pages}: {display_name_progress}");
                            let weak = weak_page_progress.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(window) = weak.upgrade() {
                                    window.set_convert_progress_value(overall);
                                    window.set_convert_progress_text(progress_text.into());
                                    window.set_convert_status(status_text.into());
                                }
                            });
                        }).map(|(output, pages, bytes)| (output, format!("{} · {pages} halaman · {} KB", target.to_ascii_uppercase(), bytes / 1024)))
                    } else if target == "pdf" {
                        convert::convert_to_pdf_with_quality(source, file_quality)
                            .map(|output| (output, "PDF".to_string()))
                    } else {
                        convert::convert_image_with_quality(source, &target, file_quality)
                            .map(|output| (output, target.to_ascii_uppercase()))
                    }
                })();
                let history_target = convert_target_override(source, &overrides).unwrap_or_else(|| {
                    if format.trim().is_empty() || format.eq_ignore_ascii_case("auto") {
                        convert::default_target(source).unwrap_or("unknown").to_string()
                    } else {
                        format.trim().trim_start_matches('.').to_ascii_lowercase()
                    }
                });
                if cancel_active_file.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    if let Ok((output, _)) = &result {
                        if output.is_dir() { let _ = std::fs::remove_dir_all(output); }
                        else { let _ = std::fs::remove_file(output); }
                    }
                    if let Some(item) = file_statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) {
                        item.1 = "Dibatalkan".to_string();
                    }
                } else {
                match result {
                    Ok((output, detail)) => {
                        let relocated = if output_dir.trim().is_empty() {
                            Ok(output.clone())
                        } else {
                            relocate_convert_output(&output, std::path::Path::new(&output_dir))
                        };
                        match relocated {
                            Ok(final_output) => {
                                completed += 1;
                                if let Some(item) = file_statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) { item.1 = "Berhasil".to_string(); }
                                record_convert_history(&path, Some(&final_output), &history_target, "OK");
                                *last_output.lock().unwrap_or_else(|error| error.into_inner()) = Some(final_output.clone());
                                let preview_path = final_output.clone();
                                let weak_preview = weak.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(w) = weak_preview.upgrade() {
                                        w.set_convert_result_preview(load_convert_preview(&preview_path));
                                    }
                                });
                                outputs.push(format!("{} ({detail})", final_output.display()));
                            }
                            Err(error) => {
                                record_convert_history(&path, None, &history_target, "GAGAL");
                                failed_files.lock().unwrap_or_else(|error| error.into_inner()).push(path.clone());
                                if let Some(item) = file_statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) { item.1 = "Gagal".to_string(); }
                                failures.push(format!("{}: hasil dibuat tetapi gagal dipindahkan: {error}", display_name));
                            }
                        }
                    }
                    Err(error) => {
                        record_convert_history(&path, None, &history_target, "GAGAL");
                        failed_files.lock().unwrap_or_else(|error| error.into_inner()).push(path.clone());
                        if let Some(item) = file_statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) { item.1 = "Gagal".to_string(); }
                        failures.push(format!("{}: {error}", source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone())));
                    }
                }
                }
                *active_file.lock().unwrap_or_else(|error| error.into_inner()) = None;
                save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
                let remaining = queue.lock().unwrap_or_else(|error| error.into_inner()).len();
                let effective_total = (processed + remaining).max(1);
                let percent = processed * 100 / effective_total;
                let progress_text = format!("{percent}% ({processed}/{effective_total})");
                let queue_text = format_convert_file_statuses(&file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
                let elapsed = started.elapsed().as_secs_f64();
                let eta_seconds = if processed > 0 {
                    (elapsed / processed as f64 * remaining as f64).ceil() as u64
                } else { 0 };
                let eta_text = if remaining == 0 { "Selesai".to_string() } else {
                    format!("Perkiraan tersisa {:02}:{:02}", eta_seconds / 60, eta_seconds % 60)
                };
                let weak_progress = weak.clone();
                let history_text = format_convert_history();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_progress.upgrade() {
                        w.set_convert_progress_text(progress_text.into());
                        w.set_convert_progress_value(percent as f32);
                        w.set_convert_eta_text(eta_text.into());
                        w.set_convert_queue_text(queue_text.into());
                        w.set_convert_history_text(history_text.into());
                    }
                });
            }
            let cancelled = cancel_requested.load(std::sync::atomic::Ordering::Acquire);
            let cancelled_files = file_statuses.lock().unwrap_or_else(|error| error.into_inner())
                .iter().filter(|(_, status)| status == "Dibatalkan").count();
            let queue_cleared = !cancelled && processed < total && cancelled_files == 0;
            cancel_requested.store(false, std::sync::atomic::Ordering::Release);
            let mut message = if cancelled {
                format!("Dibatalkan: {completed}/{total} file berhasil")
            } else if queue_cleared {
                format!("Antrean dikosongkan: {completed}/{total} file berhasil")
            } else {
                format!("Batch selesai: {completed}/{total} file berhasil · {cancelled_files} file dibatalkan")
            };
            if !outputs.is_empty() {
                message.push_str(" · ");
                message.push_str(&outputs.join(" | "));
            }
            if !failures.is_empty() {
                message.push_str(" · Gagal: ");
                message.push_str(&failures.join(" | "));
            }
            running_worker.store(false, std::sync::atomic::Ordering::Release);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    w.set_convert_status(message.into());
                    let status_text = format_convert_file_statuses(&file_statuses.lock().unwrap_or_else(|error| error.into_inner()));
                    w.set_convert_queue_text(status_text.into());
                    w.set_convert_eta_text("Selesai".into());
                    if !cancelled && !queue_cleared { w.set_convert_progress_value(100.0); }
                }
            });
        });
    });
}
{
    let weak = win.as_weak();
    let page = convert_preview_page.clone();
    let dpi = convert_preview_dpi.clone();
    let path = convert_preview_path.clone();
    let weak_preview = weak.clone();
    win.on_convert_preview_prev(move || {
        let current = page.get();
        if current == 0 { return; }
        let next = current - 1;
        page.set(next);
        let selected = path.borrow().clone();
        if !selected.is_empty() { request_convert_document_preview(weak_preview.clone(), selected, next, dpi.get()); }
    });
    let weak = win.as_weak();
    let page = convert_preview_page.clone();
    let dpi = convert_preview_dpi.clone();
    let path = convert_preview_path.clone();
    win.on_convert_preview_next(move || {
        let next = page.get().saturating_add(1);
        let Some(window) = weak.upgrade() else { return; };
        if next >= window.get_convert_preview_total().max(1) as usize { return; }
        page.set(next);
        let selected = path.borrow().clone();
        if !selected.is_empty() { request_convert_document_preview(weak.clone(), selected, next, dpi.get()); }
    });
}
{
    let weak = win.as_weak();
    let dpi = convert_preview_dpi.clone();
    let page = convert_preview_page.clone();
    let path = convert_preview_path.clone();
    let weak_preview = weak.clone();
    win.on_convert_preview_zoom_in(move || {
        let next = (dpi.get() + 20).min(300);
        dpi.set(next);
        let selected = path.borrow().clone();
        if let Some(window) = weak.upgrade() { window.set_convert_preview_zoom_text(format!("{next} DPI").into()); }
        if !selected.is_empty() { request_convert_document_preview(weak_preview.clone(), selected, page.get(), next); }
    });
    let weak = win.as_weak();
    let dpi = convert_preview_dpi.clone();
    let page = convert_preview_page.clone();
    let path = convert_preview_path.clone();
    let weak_preview = weak.clone();
    win.on_convert_preview_zoom_out(move || {
        let next = dpi.get().saturating_sub(20).max(60);
        dpi.set(next);
        let selected = path.borrow().clone();
        if let Some(window) = weak.upgrade() { window.set_convert_preview_zoom_text(format!("{next} DPI").into()); }
        if !selected.is_empty() { request_convert_document_preview(weak_preview.clone(), selected, page.get(), next); }
    });
}
{
    let weak = win.as_weak();
    let output_dir = convert_output_dir.clone();
    win.on_convert_output_dir_pick(move || {
        if let Some(path) = rfd::FileDialog::new().set_title("Pilih folder output Convert").pick_folder() {
            let value = path.to_string_lossy().to_string();
            *output_dir.borrow_mut() = value.clone();
            save_convert_output_dir(&value);
            if let Some(w) = weak.upgrade() {
                w.set_convert_output_dir_text(value.into());
                w.set_convert_status("Folder output berhasil diperbarui".into());
            }
        }
    });
}
{
    let weak = win.as_weak();
    win.on_convert_history_clear(move || {
        clear_convert_history();
        if let Some(w) = weak.upgrade() {
            w.set_convert_history_text("Belum ada riwayat konversi".into());
            w.set_convert_status("Riwayat konversi dibersihkan".into());
        }
    });
}
{
    let pause = convert_pause_requested.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_pause(move || {
        if running.load(std::sync::atomic::Ordering::Acquire) {
            pause.store(true, std::sync::atomic::Ordering::Release);
            if let Some(w) = weak.upgrade() { w.set_convert_status("Dijeda setelah file aktif selesai diproses".into()); }
        }
    });
}
{
    let pause = convert_pause_requested.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_resume(move || {
        if running.load(std::sync::atomic::Ordering::Acquire) {
            pause.store(false, std::sync::atomic::Ordering::Release);
            if let Some(w) = weak.upgrade() { w.set_convert_status("Antrean dilanjutkan".into()); }
        }
    });
}
{
    let failed = convert_failed_files.clone();
    let selected_paths = convert_selected_paths.clone();
    let weak = win.as_weak();
    win.on_convert_retry_failed(move || {
        let paths = failed.lock().unwrap_or_else(|error| error.into_inner()).clone();
        if paths.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_convert_status("Tidak ada file gagal yang perlu dicoba ulang".into()); }
            return;
        }
        *selected_paths.borrow_mut() = paths.clone();
        if let Some(w) = weak.upgrade() {
            w.set_convert_selected_path(paths[0].clone().into());
            w.set_convert_files_text(format_convert_selection(&paths).into());
            w.set_convert_status(format!("{} file gagal dimuat ulang. Tekan Konversi untuk mencoba kembali", paths.len()).into());
        }
    });
}
{
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let active_file = convert_active_file.clone();
    let cancel_active = convert_cancel_active_file.clone();
    let weak = win.as_weak();
    win.on_convert_cancel_one_file(move |raw_name| {
        let query = raw_name.to_string().trim().to_ascii_lowercase();
        if query.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_convert_status("Masukkan nama file yang ingin dibatalkan".into()); }
            return;
        }
        let matches_name = |path: &str| {
            let name = std::path::Path::new(path).file_name()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            name == query || path.to_ascii_lowercase() == query
        };
        let removed_path = {
            let mut pending = queue.lock().unwrap_or_else(|error| error.into_inner());
            let found = pending.iter().find(|path| matches_name(path)).cloned();
            if let Some(path) = &found { pending.retain(|item| item != path); }
            found
        };
        let message = if let Some(path) = removed_path {
            if let Some(item) = statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) {
                item.1 = "Dibatalkan".to_string();
            }
            format!("File {} dibatalkan dari antrean", std::path::Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()))
        } else {
            let active = active_file.lock().unwrap_or_else(|error| error.into_inner()).clone();
            if let Some(path) = active.filter(|path| matches_name(path)) {
                cancel_active.store(true, std::sync::atomic::Ordering::Release);
                if let Some(item) = statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == &path) {
                    item.1 = "Pembatalan diminta".to_string();
                }
                "Pembatalan diminta. File aktif dihentikan setelah operasi konversi aman selesai".to_string()
            } else {
                "File tidak ditemukan di antrean atau tidak sedang diproses".to_string()
            }
        };
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            let status_text = format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner()));
            w.set_convert_queue_text(status_text.into());
            w.set_convert_status(message.into());
        }
    });
}
{
    let failed = convert_failed_files.clone();
    let selected_paths = convert_selected_paths.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_retry_one_failed(move |raw_name| {
        if running.load(std::sync::atomic::Ordering::Acquire) {
            if let Some(w) = weak.upgrade() { w.set_convert_status("Tunggu hingga batch aktif selesai sebelum retry satu file".into()); }
            return;
        }
        let query = raw_name.to_string().trim().to_ascii_lowercase();
        if query.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_convert_status("Masukkan nama file yang gagal untuk diulang".into()); }
            return;
        }
        let failed_paths = failed.lock().unwrap_or_else(|error| error.into_inner());
        let matches = failed_paths.iter().filter(|path| {
            let name = std::path::Path::new(path.as_str()).file_name()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            name == query || path.to_ascii_lowercase() == query
        }).cloned().collect::<Vec<_>>();
        drop(failed_paths);
        if matches.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_convert_status("File gagal tidak ditemukan. Gunakan nama file lengkap".into()); }
            return;
        }
        *selected_paths.borrow_mut() = matches.clone();
        if let Some(w) = weak.upgrade() {
            w.set_convert_selected_path(matches[0].clone().into());
            w.set_convert_files_text(format_convert_selection(&matches).into());
            w.set_convert_status(format!("{} file cocok dimuat untuk retry. Tekan Konversi", matches.len()).into());
        }
    });
}
{
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let weak = win.as_weak();
    win.on_convert_move_queue_file(move |raw_name, direction| {
        let changed = move_convert_queue_item(&mut queue.lock().unwrap_or_else(|error| error.into_inner()), raw_name.as_str(), direction);
        sync_convert_status_order(&queue.lock().unwrap_or_else(|error| error.into_inner()), &mut statuses.lock().unwrap_or_else(|error| error.into_inner()));
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            w.set_convert_queue_text(format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner())).into());
            w.set_convert_status(if changed { "Urutan antrean diperbarui".into() } else { "File tidak ditemukan atau sudah di batas antrean".into() });
        }
    });
}
{
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let selected_paths = convert_selected_paths.clone();
    let weak = win.as_weak();
    win.on_convert_remove_finished(move || {
        let removed = remove_finished_convert_statuses(&mut statuses.lock().unwrap_or_else(|error| error.into_inner()));
        let pending = queue.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect::<Vec<_>>();
        selected_paths.borrow_mut().retain(|path| pending.contains(path));
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            w.set_convert_queue_text(format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner())).into());
            w.set_convert_status(format!("{removed} file selesai/dibatalkan dibersihkan dari daftar").into());
        }
    });
}
{
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_remove_queue_file(move |raw_name| {
        let query = raw_name.to_string().trim().to_ascii_lowercase();
        if query.is_empty() {
            if let Some(w) = weak.upgrade() { w.set_convert_status("Masukkan nama file atau path yang ingin dihapus".into()); }
            return;
        }
        let (removed, remaining) = {
            let mut pending = queue.lock().unwrap_or_else(|error| error.into_inner());
            let before = pending.len();
            let removed_paths = pending.iter().filter(|item| {
                let path = std::path::Path::new(item.as_str());
                let name = path.file_name().map(|v| v.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                name.contains(&query) || item.to_ascii_lowercase().contains(&query)
            }).cloned().collect::<Vec<_>>();
            pending.retain(|item| !removed_paths.contains(item));
            for removed_path in &removed_paths {
                if let Some(item) = statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut().find(|(item_path, _)| item_path == removed_path) {
                    item.1 = "Dihapus".to_string();
                }
            }
            (before - pending.len(), pending.len())
        };
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            let queue_text = format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner()));
            w.set_convert_queue_text(queue_text.into());
            w.set_convert_status(if removed > 0 {
                format!("{removed} file dihapus dari antrean")
            } else {
                "Tidak ada file antrean yang cocok".to_string()
            }.into());
        }
    });
}
{
    let queue = convert_queue.clone();
    let statuses = convert_file_statuses.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_clear_queue(move || {
        let removed = {
            let mut pending = queue.lock().unwrap_or_else(|error| error.into_inner());
            let count = pending.len();
            pending.clear();
            count
        };
        for item in statuses.lock().unwrap_or_else(|error| error.into_inner()).iter_mut() {
            if item.1 == "Menunggu" { item.1 = "Dihapus".to_string(); }
        }
        save_convert_queue_state(&queue.lock().unwrap_or_else(|error| error.into_inner()), &statuses.lock().unwrap_or_else(|error| error.into_inner()));
        if let Some(w) = weak.upgrade() {
            let queue_text = format_convert_file_statuses(&statuses.lock().unwrap_or_else(|error| error.into_inner()));
            w.set_convert_queue_text(queue_text.into());
            w.set_convert_status(if running.load(std::sync::atomic::Ordering::Acquire) {
                format!("{removed} file menunggu dihapus; file aktif tetap diselesaikan").into()
            } else { format!("{removed} file dihapus dari antrean").into() });
        }
    });
}
{
    let cancel_requested = convert_cancel_requested.clone();
    let running = convert_running.clone();
    let weak = win.as_weak();
    win.on_convert_cancel(move || {
        if running.load(std::sync::atomic::Ordering::Acquire) {
            cancel_requested.store(true, std::sync::atomic::Ordering::Release);
            if let Some(w) = weak.upgrade() {
                w.set_convert_status("Pembatalan diminta · file yang sedang diproses akan diselesaikan dahulu".into());
            }
        } else if let Some(w) = weak.upgrade() {
            w.set_convert_status("Tidak ada konversi yang sedang berjalan".into());
        }
    });
}
{
    let last_output = convert_last_output.clone();
    let weak = win.as_weak();
    win.on_convert_reveal(move || {
        let output = last_output.lock().unwrap_or_else(|error| error.into_inner()).clone()
            .or_else(latest_convert_output);
        if let Some(output) = output {
            let folder = if output.is_dir() {
                output.as_path()
            } else {
                output.parent().unwrap_or_else(|| std::path::Path::new("."))
            };
            if let Err(error) = transfer::open_inbox(folder) {
                if let Some(w) = weak.upgrade() {
                    w.set_convert_status(format!("Gagal membuka folder hasil: {error}").into());
                }
            } else if let Some(w) = weak.upgrade() {
                w.set_convert_status(format!("Membuka folder: {}", folder.display()).into());
            }
        } else if let Some(w) = weak.upgrade() {
            w.set_convert_status("Belum ada hasil konversi berhasil di riwayat".into());
        }
    });
}

    ConvertHandles {
        selected_paths: convert_selected_paths,
        last_output: convert_last_output,
        running: convert_running,
        preview_page: convert_preview_page,
        preview_path: convert_preview_path,
        preview_dpi: convert_preview_dpi,
    }
}
