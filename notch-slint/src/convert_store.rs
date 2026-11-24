//! Penyimpanan dan format data halaman Convert: folder output, antrean, riwayat, override, dan pratinjau.

use crate::*;

pub(crate) fn convert_output_dir_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-output-dir.txt")
}

pub(crate) fn load_convert_output_dir() -> String {
    std::fs::read_to_string(convert_output_dir_path()).unwrap_or_default().trim().to_string()
}

pub(crate) fn save_convert_output_dir(value: &str) {
    let path = convert_output_dir_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, value);
}


pub(crate) fn convert_queue_state_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-queue.json")
}

pub(crate) fn save_convert_queue_state(queue: &std::collections::VecDeque<String>, statuses: &[(String, String)]) {
    let value = serde_json::json!({
        "queue": queue.iter().collect::<Vec<_>>(),
        "statuses": statuses.iter().map(|(path, status)| serde_json::json!({"path": path, "status": status})).collect::<Vec<_>>()
    });
    let path = convert_queue_state_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(raw) = serde_json::to_vec_pretty(&value) { let _ = std::fs::write(path, raw); }
}

pub(crate) fn load_convert_queue_state() -> (std::collections::VecDeque<String>, Vec<(String, String)>) {
    let raw = match std::fs::read_to_string(convert_queue_state_path()) {
        Ok(raw) => raw,
        Err(_) => return (std::collections::VecDeque::new(), Vec::new()),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return (std::collections::VecDeque::new(), Vec::new()),
    };
    let statuses = value.get("statuses").and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(|item| {
            let path = item.get("path")?.as_str()?.to_string();
            let status = item.get("status")?.as_str()?.to_string();
            Some((path, if status == "Diproses" || status == "Pembatalan diminta" { "Menunggu".to_string() } else { status }))
        }).collect::<Vec<_>>()).unwrap_or_default();
    let mut restored = statuses;
    for (path, status) in &mut restored {
        if status == "Menunggu" && !std::path::Path::new(path).is_file() { *status = "File tidak ditemukan".to_string(); }
    }
    let status_by_path = restored.iter().map(|(path, status)| (path.clone(), status.clone())).collect::<std::collections::HashMap<_, _>>();
    let saved_order = value.get("queue").and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut queue = std::collections::VecDeque::new();
    for path in saved_order {
        if status_by_path.get(&path).map(|status| status == "Menunggu").unwrap_or(false)
            && std::path::Path::new(&path).is_file() && !queue.contains(&path) {
            queue.push_back(path);
        }
    }
    for (path, status) in &restored {
        if status == "Menunggu" && std::path::Path::new(path).is_file() && !queue.contains(path) {
            queue.push_back(path.clone());
        }
    }
    (queue, restored)
}

pub(crate) fn convert_history_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-history.json")
}

pub(crate) fn convert_overrides_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-overrides.txt")
}

pub(crate) fn load_convert_overrides() -> String {
    std::fs::read_to_string(convert_overrides_path()).unwrap_or_default()
}

pub(crate) fn save_convert_overrides(value: &str) {
    let path = convert_overrides_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, value);
}

pub(crate) fn load_convert_history() -> serde_json::Value {
    std::fs::read_to_string(convert_history_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(serde_json::Value::is_array)
        .unwrap_or_else(|| serde_json::json!([]))
}

pub(crate) fn latest_convert_output() -> Option<std::path::PathBuf> {
    load_convert_history()
        .as_array()?
        .iter()
        .rev()
        .find(|entry| entry.get("status").and_then(serde_json::Value::as_str) == Some("OK"))
        .and_then(|entry| entry.get("output").and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

pub(crate) fn format_convert_history() -> String {
    let history = load_convert_history();
    let Some(entries) = history.as_array() else { return "Belum ada riwayat konversi".into(); };
    if entries.is_empty() { return "Belum ada riwayat konversi".into(); }
    entries.iter().rev().take(8).map(|entry| {
        let name = entry.get("name").and_then(serde_json::Value::as_str).unwrap_or("File");
        let target = entry.get("target").and_then(serde_json::Value::as_str).unwrap_or("-");
        let status = entry.get("status").and_then(serde_json::Value::as_str).unwrap_or("?");
        let detail = entry.get("detail").and_then(serde_json::Value::as_str).unwrap_or("");
        format!("{name} → {target} · {status}{detail}")
    }).collect::<Vec<_>>().join("\n")
}

pub(crate) fn convert_quality_override(path: &std::path::Path, raw: &str) -> Option<u8> {
    let name = path.file_name()?.to_string_lossy();
    raw.split(';').filter_map(|entry| {
        let (key, value) = entry.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(&name) {
            value.split_once('@')?.1.trim().parse::<u8>().ok().map(|v| v.clamp(40, 100))
        } else { None }
    }).last()
}

pub(crate) fn convert_target_override(path: &std::path::Path, raw: &str) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    raw.split(';').filter_map(|entry| {
        let (key, value) = entry.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(&name) {
            let target = value.trim().split('@').next().unwrap_or(value).trim().trim_start_matches('.').to_ascii_lowercase();
            if target.is_empty() { None } else { Some(target) }
        } else {
            None
        }
    }).last()
}


pub(crate) fn move_convert_queue_item(queue: &mut std::collections::VecDeque<String>, raw_name: &str, direction: i32) -> bool {
    let query = raw_name.trim().to_ascii_lowercase();
    if query.is_empty() || direction == 0 { return false; }
    let index = queue.iter().position(|path| {
        let name = std::path::Path::new(path).file_name()
            .map(|value| value.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        name == query || path.to_ascii_lowercase() == query
    });
    let Some(index) = index else { return false; };
    let target = if direction < 0 { index.checked_sub(1) } else if index + 1 < queue.len() { Some(index + 1) } else { None };
    let Some(target) = target else { return false; };
    queue.swap(index, target);
    true
}

pub(crate) fn sync_convert_status_order(queue: &std::collections::VecDeque<String>, statuses: &mut Vec<(String, String)>) {
    let positions = queue.iter().enumerate().map(|(index, path)| (path.clone(), index)).collect::<std::collections::HashMap<_, _>>();
    statuses.sort_by_key(|(path, status)| {
        if status == "Menunggu" {
            positions.get(path).copied().unwrap_or(usize::MAX)
        } else {
            usize::MAX
        }
    });
}

pub(crate) fn remove_finished_convert_statuses(statuses: &mut Vec<(String, String)>) -> usize {
    let before = statuses.len();
    statuses.retain(|(_, status)| status != "Berhasil" && status != "Dibatalkan" && status != "Dihapus" && status != "File tidak ditemukan");
    before - statuses.len()
}

pub(crate) fn format_convert_file_statuses(statuses: &[(String, String)]) -> String {
    if statuses.is_empty() { return "Belum ada file dalam antrean".into(); }
    let lines = statuses.iter().take(10).map(|(path, status)| {
        let name = std::path::Path::new(path).file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        format!("[{status}] {name}")
    }).collect::<Vec<_>>();
    let suffix = if statuses.len() > lines.len() {
        format!("\n… dan {} file lainnya", statuses.len() - lines.len())
    } else { String::new() };
    format!("Status file ({}):\n{}{}", statuses.len(), lines.join("\n"), suffix)
}

pub(crate) fn format_convert_queue(queue: &std::collections::VecDeque<String>) -> String {
    if queue.is_empty() { return "Antrean kosong".into(); }
    let lines = queue.iter().take(8).map(|raw| {
        std::path::Path::new(raw).file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| raw.clone())
    }).collect::<Vec<_>>();
    let suffix = if queue.len() > lines.len() {
        format!("\n… dan {} file lainnya", queue.len() - lines.len())
    } else { String::new() };
    format!("Antrean ({}):\n{}{}", queue.len(), lines.join("\n"), suffix)
}

pub(crate) fn format_convert_selection(paths: &[String]) -> String {
    if paths.is_empty() { return "Belum ada file dipilih".into(); }
    let mut lines = paths.iter().take(10).map(|raw| {
        let path = std::path::Path::new(raw);
        let name = path.file_name().map(|value| value.to_string_lossy().into_owned()).unwrap_or_else(|| raw.clone());
        let size = std::fs::metadata(path).map(|metadata| {
            let bytes = metadata.len();
            if bytes >= 1024 * 1024 { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
            else { format!("{} KB", bytes / 1024) }
        }).unwrap_or_else(|_| "ukuran tidak diketahui".into());
        let targets = convert::available_targets(path);
        let target_text = if targets.is_empty() { "tidak tersedia".to_string() } else { targets.join("/") };
        let default = convert::default_target(path).unwrap_or("-");
        format!("{name} · {size} · auto: {default} · pilihan: {target_text}")
    }).collect::<Vec<_>>();
    if paths.len() > 10 { lines.push(format!("… dan {} file lainnya", paths.len() - 10)); }
    lines.join("\n")
}

pub(crate) fn load_convert_document_preview(
    path: &std::path::Path,
    page: usize,
    dpi: u32,
) -> Result<(Vec<u8>, u32, u32, usize), String> {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    let rendered = if extension == "pdf" {
        let bytes = std::fs::read(path).map_err(|error| format!("PDF tidak dapat dibaca: {error}"))?;
        notch_tools::pdf::render_page(&bytes, page, dpi).map_err(|error| error.to_string())?
    } else if matches!(extension.as_str(), "docx" | "xlsx" | "pptx" | "doc" | "xls" | "ppt" | "odt" | "ods" | "odp" | "rtf" | "txt" | "csv") {
        let bytes = convert::convert_office_to_pdf_bytes(path)?;
        notch_tools::pdf::render_page(&bytes, page, dpi).map_err(|error| error.to_string())?
    } else {
        let image = image::open(path).map_err(|error| format!("Preview tidak tersedia: {error}"))?;
        let rgba = image.thumbnail(1600, 1600).to_rgba8();
        let (width, height) = rgba.dimensions();
        return Ok((rgba.into_raw(), width, height, 1));
    };
    let (rendered, total) = rendered;
    let rgba = rendered.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((rgba.into_raw(), width, height, total))
}

pub(crate) fn request_convert_document_preview(weak: Weak<NotchWindow>, path: String, page: usize, dpi: u32) {
    std::thread::spawn(move || {
        let result = load_convert_document_preview(std::path::Path::new(&path), page, dpi);
        let _ = weak.upgrade_in_event_loop(move |window| {
            match result {
                Ok((rgba, width, height, total)) => {
                    let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, width, height));
                    window.set_convert_source_preview(image);
                    window.set_convert_preview_page_text(format!("Halaman {} / {}", page.min(total.saturating_sub(1)) + 1, total).into());
                    window.set_convert_preview_total(total as i32);
                    window.set_convert_preview_status("".into());
                }
                Err(error) => {
                    window.set_convert_source_preview(Image::default());
                    window.set_convert_preview_page_text("Preview tidak tersedia".into());
                    window.set_convert_preview_status(error.into());
                }
            }
        });
    });
}

pub(crate) fn load_convert_preview(path: &std::path::Path) -> Image {
    if path.extension().and_then(|value| value.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok((rendered, _)) = notch_tools::pdf::render_page(&bytes, 0, 90) {
                let rgba = rendered.to_rgba8();
                return Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    rgba.as_raw(), rgba.width(), rgba.height(),
                ));
            }
        }
        return Image::default();
    }
    let candidate = if path.is_dir() {
        let mut files = std::fs::read_dir(path).ok().into_iter().flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate.extension().and_then(|value| value.to_str())
                    .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        files.sort();
        files.into_iter().next()
    } else {
        Some(path.to_path_buf())
    };
    candidate.and_then(|candidate| Image::load_from_path(&candidate).ok()).unwrap_or_default()
}

pub(crate) fn copy_convert_directory(source: &std::path::Path, destination: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_convert_directory(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn relocate_convert_output(output: &std::path::Path, destination: &std::path::Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(destination).map_err(|error| format!("Folder output tidak dapat dibuat: {error}"))?;
    let output_parent = output.parent().unwrap_or_else(|| std::path::Path::new("."));
    if output_parent.canonicalize().ok() == destination.canonicalize().ok() {
        return Ok(output.to_path_buf());
    }
    let name = output.file_name().ok_or_else(|| "Nama hasil konversi tidak valid".to_string())?;
    let mut target = destination.join(name);
    let stem = output.file_stem().and_then(|value| value.to_str()).unwrap_or("hasil");
    let extension = output.extension().and_then(|value| value.to_str());
    let mut index = 2u32;
    while target.exists() {
        let candidate = match extension {
            Some(ext) => format!("{stem}_{index}.{ext}"),
            None => format!("{stem}_{index}"),
        };
        target = destination.join(candidate);
        index = index.saturating_add(1);
    }
    match std::fs::rename(output, &target) {
        Ok(()) => Ok(target),
        Err(rename_error) => {
            let copied = if output.is_dir() {
                copy_convert_directory(output, &target)
            } else {
                std::fs::copy(output, &target).map(|_| ()).map_err(|error| error.to_string())
            };
            match copied {
                Ok(()) => {
                    if output.is_dir() { std::fs::remove_dir_all(output) } else { std::fs::remove_file(output) }
                        .map_err(|error| format!("Hasil tersalin tetapi file sementara gagal dibersihkan: {error}"))?;
                    Ok(target)
                }
                Err(copy_error) => Err(format!("Gagal memindahkan hasil ({rename_error}); salin juga gagal ({copy_error})")),
            }
        }
    }
}

pub(crate) fn record_convert_history(input: &str, output: Option<&std::path::Path>, target: &str, status: &str) {
    let path = convert_history_path();
    let mut history = load_convert_history();
    let Some(entries) = history.as_array_mut() else { return; };
    let name = std::path::Path::new(input).file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string());
    let output_text = output.map(|value| value.to_string_lossy().into_owned()).unwrap_or_default();
    let detail = output.map(|value| {
        std::fs::metadata(value).ok().map(|metadata| format!(" · {} KB", metadata.len() / 1024)).unwrap_or_default()
    }).unwrap_or_default();
    entries.push(serde_json::json!({
        "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs()).unwrap_or(0),
        "name": name,
        "output": output_text,
        "target": target.to_ascii_uppercase(),
        "status": status,
        "detail": detail
    }));
    if entries.len() > 30 {
        let excess = entries.len() - 30;
        entries.drain(0..excess);
    }
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(serialized) = serde_json::to_vec_pretty(&history) { let _ = std::fs::write(path, serialized); }
}


pub(crate) fn csv_escape_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub(crate) fn convert_history_csv() -> String {
    let mut rows = vec!["timestamp,name,output,target,status,detail".to_string()];
    if let Some(entries) = load_convert_history().as_array() {
        for entry in entries {
            let field = |key: &str| entry.get(key).and_then(serde_json::Value::as_str).unwrap_or("");
            let timestamp = entry.get("timestamp").and_then(serde_json::Value::as_u64).unwrap_or(0).to_string();
            let values = [timestamp, field("name").to_string(), field("output").to_string(), field("target").to_string(), field("status").to_string(), field("detail").to_string()];
            rows.push(values.iter().map(|value| csv_escape_field(value)).collect::<Vec<_>>().join(","));
        }
    }
    rows.join("\r\n")
}

pub(crate) fn clear_convert_history() {
    let path = convert_history_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, "[]");
}

#[cfg(test)]
mod convert_queue_display_tests {
    use super::{csv_escape_field, format_convert_file_statuses, format_convert_queue, move_convert_queue_item, remove_finished_convert_statuses, sync_convert_status_order};
    use std::collections::VecDeque;

    #[test]
    fn csv_fields_escape_quotes_commas_and_newlines() {
        assert_eq!(csv_escape_field("plain"), "plain");
        assert_eq!(csv_escape_field("a,b"), "\"a,b\"");
        assert_eq!(csv_escape_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape_field("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn file_status_display_includes_each_state_and_filename() {
        let statuses = vec![
            ("/tmp/one.pdf".to_string(), "Menunggu".to_string()),
            ("/tmp/two.png".to_string(), "Diproses".to_string()),
            ("/tmp/three.jpg".to_string(), "Berhasil".to_string()),
            ("/tmp/four.docx".to_string(), "Gagal".to_string()),
            ("/tmp/five.gif".to_string(), "Dihapus".to_string()),
        ];
        let display = format_convert_file_statuses(&statuses);
        assert!(display.contains("[Menunggu] one.pdf"));
        assert!(display.contains("[Diproses] two.png"));
        assert!(display.contains("[Berhasil] three.jpg"));
        assert!(display.contains("[Gagal] four.docx"));
        assert!(display.contains("[Dihapus] five.gif"));
        assert!(!display.contains("/tmp/"));
    }

    #[test]
    fn file_status_display_limits_visible_items() {
        let statuses = (1..=12)
            .map(|index| (format!("/tmp/file-{index}.pdf"), "Menunggu".to_string()))
            .collect::<Vec<_>>();
        let display = format_convert_file_statuses(&statuses);
        assert!(display.contains("Status file (12)"));
        assert!(display.contains("dan 2 file lainnya"));
        assert!(!display.contains("file-12.pdf"));
    }

    #[test]
    fn empty_queue_has_clear_empty_state() {
        assert_eq!(format_convert_queue(&VecDeque::new()), "Antrean kosong");
    }

    #[test]
    fn queue_displays_pending_file_names_not_full_paths() {
        let queue = VecDeque::from(vec![
            "C:/input/first.pdf".to_string(),
            "C:/input/second.docx".to_string(),
        ]);
        let display = format_convert_queue(&queue);
        assert!(display.contains("first.pdf"));
        assert!(display.contains("second.docx"));
        assert!(!display.contains("C:/input"));
    }

    #[test]
    fn queue_limits_visible_items_and_reports_the_remainder() {
        let queue = (1..=10).map(|index| format!("file-{index}.pdf")).collect::<VecDeque<_>>();
        let display = format_convert_queue(&queue);
        assert!(display.contains("file-1.pdf"));
        assert!(display.contains("file-8.pdf"));
        assert!(!display.contains("file-9.pdf"));
        assert!(display.contains("dan 2 file lainnya"));
    }

    #[test]
    fn queue_item_can_move_up_and_down_by_filename() {
        let mut queue = VecDeque::from(vec!["/tmp/one.pdf".to_string(), "/tmp/two.pdf".to_string(), "/tmp/three.pdf".to_string()]);
        assert!(move_convert_queue_item(&mut queue, "three.pdf", -1));
        assert_eq!(queue[1], "/tmp/three.pdf");
        assert!(move_convert_queue_item(&mut queue, "one.pdf", 1));
        assert_eq!(queue[1], "/tmp/one.pdf");
    }

    #[test]
    fn queue_item_cannot_move_past_boundaries_or_when_missing() {
        let mut queue = VecDeque::from(vec!["one.pdf".to_string(), "two.pdf".to_string()]);
        assert!(!move_convert_queue_item(&mut queue, "one.pdf", -1));
        assert!(!move_convert_queue_item(&mut queue, "two.pdf", 1));
        assert!(!move_convert_queue_item(&mut queue, "missing.pdf", 1));
    }

    #[test]
    fn visible_pending_statuses_follow_queue_order() {
        let queue = VecDeque::from(vec!["two.pdf".to_string(), "one.pdf".to_string()]);
        let mut statuses = vec![
            ("one.pdf".to_string(), "Menunggu".to_string()),
            ("two.pdf".to_string(), "Menunggu".to_string()),
            ("done.pdf".to_string(), "Berhasil".to_string()),
        ];
        sync_convert_status_order(&queue, &mut statuses);
        assert_eq!(statuses[0].0, "two.pdf");
        assert_eq!(statuses[1].0, "one.pdf");
        assert_eq!(statuses[2].0, "done.pdf");
    }

    #[test]
    fn cleanup_removes_only_finished_and_cancelled_statuses() {
        let mut statuses = vec![
            ("one.pdf".to_string(), "Berhasil".to_string()),
            ("two.pdf".to_string(), "Dibatalkan".to_string()),
            ("three.pdf".to_string(), "Dihapus".to_string()),
            ("four.pdf".to_string(), "Menunggu".to_string()),
            ("five.pdf".to_string(), "Diproses".to_string()),
            ("six.pdf".to_string(), "Gagal".to_string()),
        ];
        assert_eq!(remove_finished_convert_statuses(&mut statuses), 3);
        assert_eq!(statuses.len(), 3);
        assert!(statuses.iter().any(|(_, status)| status == "Menunggu"));
        assert!(statuses.iter().any(|(_, status)| status == "Diproses"));
        assert!(statuses.iter().any(|(_, status)| status == "Gagal"));
    }
}

#[cfg(test)]
mod convert_target_override_tests {
    use super::{convert_quality_override, convert_target_override};
    use std::path::Path;

    #[test]
    fn per_file_quality_is_clamped_and_case_insensitive() {
        assert_eq!(
            convert_quality_override(std::path::Path::new("Report.PDF"), "photo.png=webp@120; report.pdf=jpg@35"),
            Some(40)
        );
        assert_eq!(
            convert_quality_override(std::path::Path::new("photo.png"), "photo.png=jpg@82"),
            Some(82)
        );
    }

    #[test]
    fn per_file_quality_requires_a_valid_quality_suffix() {
        assert_eq!(
            convert_quality_override(std::path::Path::new("photo.png"), "photo.png=jpg"),
            None
        );
        assert_eq!(
            convert_quality_override(std::path::Path::new("photo.png"), "photo.png=jpg@high"),
            None
        );
        assert_eq!(
            convert_quality_override(std::path::Path::new("photo.png"), "other.png=jpg@80"),
            None
        );
    }

    #[test]
    fn matches_file_names_case_insensitively() {
        assert_eq!(
            convert_target_override(Path::new("C:/docs/Report.PDF"), "photo.png=webp; report.pdf=jpg"),
            Some("jpg".to_string())
        );
    }

    #[test]
    fn ignores_entries_for_other_files_and_empty_targets() {
        assert_eq!(
            convert_target_override(Path::new("photo.png"), "document.pdf=jpg; photo.png="),
            None
        );
        assert_eq!(
            convert_target_override(Path::new("photo.png"), "document.pdf=jpg"),
            None
        );
    }

    #[test]
    fn normalizes_dot_prefixed_target_extensions() {
        assert_eq!(
            convert_target_override(Path::new("photo.png"), "photo.png=.webp"),
            Some("webp".to_string())
        );
    }
}

#[cfg(test)]
mod convert_output_directory_tests {
    use super::relocate_convert_output;
    use std::fs;
    use std::path::PathBuf;

    fn test_root() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("notch-convert-output-test-{unique}"))
    }

    #[test]
    fn relocates_file_without_overwriting_existing_output() {
        let root = test_root();
        let source_dir = root.join("source");
        let output_dir = root.join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("photo_converted.png");
        fs::write(&source, b"new image").unwrap();
        fs::write(output_dir.join("photo_converted.png"), b"existing").unwrap();

        let result = relocate_convert_output(&source, &output_dir).unwrap();

        assert_eq!(result.file_name().unwrap(), "photo_converted_2.png");
        assert_eq!(fs::read(&result).unwrap(), b"new image");
        assert_eq!(fs::read(output_dir.join("photo_converted.png")).unwrap(), b"existing");
        assert!(!source.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_existing_destination_directory_and_uses_unique_name() {
        let root = test_root();
        let source_dir = root.join("source");
        let output_dir = root.join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("pages");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("page.png"), b"new").unwrap();
        fs::create_dir_all(output_dir.join("pages")).unwrap();
        fs::write(output_dir.join("pages").join("old.png"), b"old").unwrap();

        let result = relocate_convert_output(&source, &output_dir).unwrap();

        assert_eq!(result.file_name().unwrap(), "pages_2");
        assert_eq!(fs::read(result.join("page.png")).unwrap(), b"new");
        assert_eq!(fs::read(output_dir.join("pages").join("old.png")).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relocates_multipage_directory_and_preserves_pages() {
        let root = test_root();
        let source_dir = root.join("source");
        let output_dir = root.join("output");
        let pages = source_dir.join("document_converted_pages");
        fs::create_dir_all(&pages).unwrap();
        let page = pages.join("document-001.png");
        fs::write(&page, b"page data").unwrap();

        let result = relocate_convert_output(&pages, &output_dir).unwrap();

        assert!(result.is_dir());
        assert_eq!(fs::read(result.join("document-001.png")).unwrap(), b"page data");
        assert!(!pages.exists());
        let _ = fs::remove_dir_all(root);
    }
}
