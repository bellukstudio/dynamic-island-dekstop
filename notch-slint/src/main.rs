//! Prototipe Notch dengan Slint. Target utama: Windows.
//! Di OS lain hanya untuk uji kompilasi (posisi kursor dan jam memakai fallback).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notch_core::battery::{BatteryMonitor, Reading};
use notch_core::calendar::{self, Date, Holidays, Item, View as CalView};
use notch_core::clipboard::ClipboardMonitor;
use notch_core::config::Config;
use notch_core::decor;
use notch_core::eyes::{self, Anim, EyeRenderer, Face, Morph, Mood, MoodState};
use notch_core::layout::{self, Dock};
use notch_core::notes::{Note, NotesStore};
use notch_core::pomodoro::{Phase, Pomodoro};
use notch_core::prayer::{self, Day as PrayerDay};
use notch_core::quick::{self, Quick};
use notch_core::reminder::{self, Reminder};
use notch_core::stats::StatsMonitor;
use slint::Model;
use notch_core::theme::{Palette, Rgba};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{
    ComponentHandle, Global, Image, ModelRc, PhysicalPosition, PhysicalSize, Rgba8Pixel,
    SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel, Weak,
};

mod privacy;
mod shelf;
mod tray;
mod clip;
mod hotkeys;
mod colorpick;
mod compress;
mod convert;
mod transfer;
mod android;
mod android_mirror;
mod autostart;
mod speech;

slint::include_modules!();

fn convert_output_dir_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-output-dir.txt")
}

fn load_convert_output_dir() -> String {
    std::fs::read_to_string(convert_output_dir_path()).unwrap_or_default().trim().to_string()
}

fn save_convert_output_dir(value: &str) {
    let path = convert_output_dir_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, value);
}


fn convert_queue_state_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-queue.json")
}

fn save_convert_queue_state(queue: &std::collections::VecDeque<String>, statuses: &[(String, String)]) {
    let value = serde_json::json!({
        "queue": queue.iter().collect::<Vec<_>>(),
        "statuses": statuses.iter().map(|(path, status)| serde_json::json!({"path": path, "status": status})).collect::<Vec<_>>()
    });
    let path = convert_queue_state_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(raw) = serde_json::to_vec_pretty(&value) { let _ = std::fs::write(path, raw); }
}

fn load_convert_queue_state() -> (std::collections::VecDeque<String>, Vec<(String, String)>) {
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

fn convert_history_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-history.json")
}

fn convert_overrides_path() -> std::path::PathBuf {
    notch_core::paths::data_file("convert-overrides.txt")
}

fn load_convert_overrides() -> String {
    std::fs::read_to_string(convert_overrides_path()).unwrap_or_default()
}

fn save_convert_overrides(value: &str) {
    let path = convert_overrides_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, value);
}

fn load_convert_history() -> serde_json::Value {
    std::fs::read_to_string(convert_history_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(serde_json::Value::is_array)
        .unwrap_or_else(|| serde_json::json!([]))
}

fn latest_convert_output() -> Option<std::path::PathBuf> {
    load_convert_history()
        .as_array()?
        .iter()
        .rev()
        .find(|entry| entry.get("status").and_then(serde_json::Value::as_str) == Some("OK"))
        .and_then(|entry| entry.get("output").and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

fn format_convert_history() -> String {
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

fn convert_quality_override(path: &std::path::Path, raw: &str) -> Option<u8> {
    let name = path.file_name()?.to_string_lossy();
    raw.split(';').filter_map(|entry| {
        let (key, value) = entry.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(&name) {
            value.split_once('@')?.1.trim().parse::<u8>().ok().map(|v| v.clamp(40, 100))
        } else { None }
    }).last()
}

fn convert_target_override(path: &std::path::Path, raw: &str) -> Option<String> {
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


fn move_convert_queue_item(queue: &mut std::collections::VecDeque<String>, raw_name: &str, direction: i32) -> bool {
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

fn sync_convert_status_order(queue: &std::collections::VecDeque<String>, statuses: &mut Vec<(String, String)>) {
    let positions = queue.iter().enumerate().map(|(index, path)| (path.clone(), index)).collect::<std::collections::HashMap<_, _>>();
    statuses.sort_by_key(|(path, status)| {
        if status == "Menunggu" {
            positions.get(path).copied().unwrap_or(usize::MAX)
        } else {
            usize::MAX
        }
    });
}

fn remove_finished_convert_statuses(statuses: &mut Vec<(String, String)>) -> usize {
    let before = statuses.len();
    statuses.retain(|(_, status)| status != "Berhasil" && status != "Dibatalkan" && status != "Dihapus" && status != "File tidak ditemukan");
    before - statuses.len()
}

fn format_convert_file_statuses(statuses: &[(String, String)]) -> String {
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

fn format_convert_queue(queue: &std::collections::VecDeque<String>) -> String {
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

fn format_convert_selection(paths: &[String]) -> String {
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

fn load_convert_document_preview(
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

fn request_convert_document_preview(weak: Weak<NotchWindow>, path: String, page: usize, dpi: u32) {
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

fn load_convert_preview(path: &std::path::Path) -> Image {
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

fn copy_convert_directory(source: &std::path::Path, destination: &std::path::Path) -> Result<(), String> {
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

fn relocate_convert_output(output: &std::path::Path, destination: &std::path::Path) -> Result<std::path::PathBuf, String> {
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

fn record_convert_history(input: &str, output: Option<&std::path::Path>, target: &str, status: &str) {
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


fn csv_escape_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn convert_history_csv() -> String {
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

fn clear_convert_history() {
    let path = convert_history_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let _ = std::fs::write(path, "[]");
}

// Ruang saat menciut mencakup bubble 280 x 90 px dan pil setengah lingkaran dock bawah.
const COLLAPSED: (u32, u32) = (300, 140);
// Side dock: window 300 x 262 px memuat pil vertikal 60 x 120 dan bubble 230 px.
const COLLAPSED_SIDE: (u32, u32) = (300, 262);
// Greeting inline memerlukan ruang horizontal tambahan di sekitar badan Notch.
const GREETING_INLINE: (u32, u32) = (420, 112);
// 580 x 202 untuk pil + jalur di bawahnya (celah 8 px + tombol Pusatkan 22 px).
const EXPANDED: (u32, u32) = (612, 240);
/// Dashboard Currency membutuhkan ruang vertikal untuk konverter dan ringkasan pasar.
const EXPANDED_CURRENCY: (u32, u32) = (812, 558);
/// Hub Reminder menyediakan ruang untuk header, banner aktif, form dan daftar tanpa terpotong.
const EXPANDED_REMINDER: (u32, u32) = (812, 540);
/// Hub catatan yang diperluas (300 px) + jalur Pusatkan di bawahnya.
const EXPANDED_TALL: (u32, u32) = (612, 338);

/// Hub Ramadan (780 x 540) + jalur Pusatkan dan telinga.
const EXPANDED_RAMADAN: (u32, u32) = (812, 578);

fn expanded_size(w: &NotchWindow) -> (u32, u32) {
    if w.get_page_id() == "prayer" && w.get_ramadan_enabled() {
        EXPANDED_RAMADAN
    } else if w.get_page_id() == "currency" {
        EXPANDED_CURRENCY
    } else if w.get_page_id() == "reminder" {
        EXPANDED_REMINDER
    } else if w.get_page_id() == "notes" && w.get_notes_expanded() {
        EXPANDED_TALL
    } else {
        EXPANDED
    }
}

/// File yang dilepas ke notch masuk ke Shelf kecuali halaman Convert sedang terbuka (halaman itu punya antrean sendiri)
/// atau halaman Shelf dimatikan di pengaturan. Dibaca langsung dari UI agar mengikuti perubahan pengaturan.
fn shelf_accepts_drop(w: &NotchWindow) -> bool {
    !(w.get_expanded() && w.get_page_id() == "convert") && w.get_active_pages().iter().any(|p| p == "shelf")
}

// Canvas mata mengikuti ukuran notch sesuai orientasi dock.
const EYES_W: f32 = 130.0;
const EYES_H: f32 = 34.0;
const EYES_TOP_H: f32 = 26.0;

fn eye_renderer_for_dock(skin: &str, dock: i32) -> (EyeRenderer, (f32, f32)) {
    let size = match dock {
        0 => (EYES_W, EYES_TOP_H),
        3 | 4 => (EYES_H, EYES_W),
        _ => (EYES_W, EYES_H),
    };
    let renderer = if matches!(dock, 3 | 4) {
        EyeRenderer::new_vertical(skin, size.0, size.1)
    } else {
        EyeRenderer::new(skin, size.0, size.1)
    };
    (renderer, size)
}

/// Pengacak ringan untuk kedip dan tatapan acak (tanpa dependensi `rand`).
struct Rng(Cell<u64>);
impl Rng {
    fn new() -> Rng {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(Cell::new(seed | 1))
    }
    /// 0.0 ..< 1.0
    fn next(&self) -> f32 {
        let mut x = self.0.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0.set(x);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Seluruh keadaan mata: mood, transisi, tatapan, kedip, dan frame terakhir yang digambar.
struct EyeCtx {
    renderer: EyeRenderer,
    canvas_size: (f32, f32),
    skin: usize,
    mood: MoodState,
    cur: Mood,
    cur_since: f32,
    morph: Morph,
    look: (f32, f32),
    look_target: (f32, f32),
    last_frame_ts: f32,
    last_cursor: (i32, i32),
    last_move: f32,
    next_random_look: f32,
    blink_until: f32,
    next_blink: f32,
    last_key: Option<(i32, i32, bool, usize, i64, u32)>,
    rng: Rng,
}

mod media;

mod platform {
    use notch_core::layout::WorkArea;

    /// Apakah Windows memakai tema aplikasi terang (untuk `theme = "auto"`). Di OS lain: gelap.
    pub fn os_light() -> bool {
        #[cfg(windows)]
        unsafe {
            use windows::core::w;
            use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
            let mut data: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            let r = RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut core::ffi::c_void),
                Some(&mut size),
            );
            r.is_ok() && data == 1
        }
        #[cfg(not(windows))]
        false
    }

    /// Posisi kursor global (piksel fisik).
    pub fn cursor_pos() -> Option<(i32, i32)> {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let mut p = POINT::default();
            GetCursorPos(&mut p).ok()?;
            Some((p.x, p.y))
        }
        #[cfg(not(windows))]
        None
    }

    /// Status tombol kiri mouse global untuk drag native (Windows).
    pub fn left_mouse_down() -> bool {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
            return GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0;
        }
        #[cfg(not(windows))]
        false
    }

    /// Cegah Windows menggambar bingkai klasik (caption biru muda "Notch" + tombol x, atau kotak putih seukuran jendela)
    /// di atas notch. winit selalu memberi jendela style WS_CAPTION (agar Aero Snap jalan) dan menyembunyikannya lewat
    /// WM_NCCALCSIZE; tetapi pada jendela dengan region (SetWindowRgn) bingkai tidak lagi digambar DWM melainkan USER,
    /// tepat di dalam persegi jendela, setiap kali ada WM_NCPAINT atau pergantian aktivasi (klik tray, buka Pengaturan).
    /// Subclass ini menahan keduanya. Aman dipanggil berulang (id subclass sama hanya memperbarui yang lama).
    #[cfg(windows)]
    unsafe fn no_border(hwnd: windows::Win32::Foundation::HWND) {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
        use windows::Win32::UI::Shell::SetWindowSubclass;
        const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &DWMWA_COLOR_NONE as *const u32 as *const core::ffi::c_void,
            core::mem::size_of::<u32>() as u32,
        );
        let _ = SetWindowSubclass(hwnd, Some(nc_guard), NC_GUARD_ID, 0);
    }

    #[cfg(windows)]
    const NC_GUARD_ID: usize = 0x4E43; // "NC"

    #[cfg(windows)]
    unsafe extern "system" fn nc_guard(
        hwnd: windows::Win32::Foundation::HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
        _id: usize,
        _data: usize,
    ) -> windows::Win32::Foundation::LRESULT {
        use windows::Win32::Foundation::{LPARAM, LRESULT};
        use windows::Win32::UI::Shell::DefSubclassProc;
        use windows::Win32::UI::WindowsAndMessaging::{WM_NCACTIVATE, WM_NCPAINT};
        match msg {
            // Jangan menggambar area non-client sama sekali.
            WM_NCPAINT => LRESULT(0),
            // lParam = -1: winit tetap menerima perubahan aktivasi, tetapi DefWindowProc tidak menggambar ulang bingkai.
            WM_NCACTIVATE => DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }

    /// Sembunyikan jendela dari taskbar Windows (dan Alt+Tab). Idempotent, aman dipanggil berulang.
    /// Tiga lapis karena winit memasang WS_EX_APPWINDOW (memaksa tombol taskbar) dan menimpa style jendela
    /// setiap `show()` ulang: (1) `set_skip_taskbar` (ITaskbarList::DeleteTab), (2) buang WS_EX_APPWINDOW dan
    /// pasang WS_EX_TOOLWINDOW, (3) hide/show sesaat bila style berubah agar shell membaca style baru.
    /// Jendela winit hanya bisa diakses saat event loop aktif (dari timer/callback); sebelum `win.run()` berjalan
    /// pemanggilan ini tidak berefek.
    pub fn hide_from_taskbar(window: &slint::Window) {
        #[cfg(windows)]
        {
            use slint::winit_030::winit::platform::windows::WindowExtWindows;
            use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            use slint::winit_030::WinitWindowAccessor;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_HIDE, SW_SHOWNA,
                WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
            };
            let raw = window
                .with_winit_window(|w| {
                    w.set_skip_taskbar(true);
                    match w.window_handle().map(|h| h.as_raw()) {
                        Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                        _ => None,
                    }
                })
                .flatten();
            if let Some(raw) = raw {
                unsafe {
                    let hwnd = HWND(raw as *mut core::ffi::c_void);
                    no_border(hwnd);
                    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                    let want = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
                    if want != ex {
                        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        let _ = ShowWindow(hwnd, SW_SHOWNA);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        let _ = window;
    }

    /// Blok sampai komposisi DWM berikutnya selesai (maksimal satu frame, ±16 ms). Dipakai agar perubahan ukuran
    /// jendela jatuh tepat setelah vsync. Tidak melakukan apa pun di platform lain.
    pub fn wait_vsync() {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
    }

    /// Atur posisi DAN ukuran jendela dalam satu panggilan (piksel fisik). `set_size` lalu `set_position` terpisah
    /// menyisakan satu frame saat jendela sudah berukuran baru tetapi masih di posisi lama, sehingga notch yang
    /// ditengahkan di dalamnya terlihat melompat/berkedip saat hub membuka atau menutup. Mengembalikan false bila
    /// jendela native belum ada (atau bukan Windows); pemanggil lalu memakai API Slint biasa.
    pub fn set_bounds(window: &slint::Window, x: i32, y: i32, w: u32, h: u32) -> bool {
        #[cfg(windows)]
        {
            use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            use slint::winit_030::WinitWindowAccessor;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
            let raw = window
                .with_winit_window(|win| match win.window_handle().map(|h| h.as_raw()) {
                    Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                    _ => None,
                })
                .flatten();
            if let Some(raw) = raw {
                unsafe {
                    let hwnd = HWND(raw as *mut core::ffi::c_void);
                    return SetWindowPos(hwnd, None, x, y, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE).is_ok();
                }
            }
            false
        }
        #[cfg(not(windows))]
        {
            let _ = (window, x, y, w, h);
            false
        }
    }

    /// Batasi bagian jendela yang tampil dan menerima klik ke persegi `(x, y, lebar, tinggi)` (piksel fisik, relatif
    /// ke sudut jendela), atau `None` untuk menampilkan seluruh jendela lagi. Dipakai agar jendela tidak perlu
    /// diperkecil saat notch menciut. Mengembalikan false bila gagal (atau bukan Windows).
    pub fn set_region(window: &slint::Window, rect: Option<(i32, i32, i32, i32)>) -> bool {
        #[cfg(windows)]
        {
            use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            use slint::winit_030::WinitWindowAccessor;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn};
            let raw = window
                .with_winit_window(|win| match win.window_handle().map(|h| h.as_raw()) {
                    Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                    _ => None,
                })
                .flatten();
            if let Some(raw) = raw {
                unsafe {
                    let hwnd = HWND(raw as *mut core::ffi::c_void);
                    // Pasang penahan bingkai SEBELUM region berubah: SetWindowRgn memicu repaint non-client.
                    no_border(hwnd);
                    let region = rect.map(|(x, y, w, h)| CreateRectRgn(x, y, x + w, y + h));
                    let ok = SetWindowRgn(hwnd, region, true) != 0;
                    // Bila berhasil, sistem memiliki region itu; bila gagal, kita yang harus membebaskannya.
                    if !ok {
                        if let Some(region) = region {
                            let _ = DeleteObject(region.into());
                        }
                    }
                    return ok;
                }
            }
            false
        }
        #[cfg(not(windows))]
        {
            let _ = (window, rect);
            false
        }
    }

    /// Area kerja monitor (di luar taskbar) tempat notch berada, dalam piksel fisik: lebar penuh monitor,
    /// vertikal mengikuti area kerja (sama dengan `monitor_work_area` di src-tauri/src/lib.rs).
    /// `near` = titik (mis. tengah posisi kustom) untuk memilih monitor; `None` = monitor utama.
    pub fn work_area(near: Option<(i32, i32)>) -> WorkArea {
        #[cfg(windows)]
        unsafe {
            use windows::core::PCWSTR;
            use windows::Win32::Foundation::{POINT, RECT};
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
            };
            use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};
            let (pt, flag) = match near {
                // y bisa 100000 (menempel di dasar layar): jepit supaya titik tetap dekat monitor
                Some((x, y)) => (POINT { x, y: y.min(50_000) }, MONITOR_DEFAULTTONEAREST),
                None => (POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
            };
            let hmon = MonitorFromPoint(pt, flag);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                let (m, r) = (mi.rcMonitor, mi.rcWork);
                if r.bottom - r.top > 0 {
                    // rcWork adalah sumber utama. Periksa juga HWND taskbar agar posisi bawah
                    // tetap aman pada konfigurasi Windows yang melaporkan rcWork penuh.
                    let class_name: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
                    let mut tray_rect = RECT::default();
                    let work_bottom = match FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null()) {
                        Ok(tray)
                            if !tray.0.is_null()
                                && GetWindowRect(tray, &mut tray_rect).is_ok()
                                && tray_rect.left < m.right
                                && tray_rect.right > m.left
                                && tray_rect.top >= r.top
                                && tray_rect.top < m.bottom =>
                        {
                            r.bottom.min(tray_rect.top)
                        }
                        _ => r.bottom,
                    };
                    if work_bottom > r.top {
                        return WorkArea {
                            x: m.left,
                            y: r.top,
                            w: m.right - m.left,
                            h: work_bottom - r.top,
                        };
                    }
                }
            }
        }
        let _ = near;
        WorkArea { x: 0, y: 0, w: 1920, h: 1080 }
    }

    /// Deteksi aplikasi foreground yang menutupi seluruh monitor (fullscreen/borderless).
    /// Maximized biasa tidak terdeteksi karena masih menyisakan area taskbar.
    pub fn foreground_is_fullscreen() -> bool {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::RECT;
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
            };

            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return false;
            }
            // Jendela milik proses ini (menu tray, jendela Pengaturan) dan jendela shell Windows (taskbar, flyout tray,
            // Start, Alt+Tab) sering seukuran monitor atau dibuat transparan, tetapi bukan aplikasi fullscreen.
            // Tanpa pengecualian ini notch ikut disembunyikan lalu ditampilkan ulang (tampil putih) tiap tray dibuka.
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                return false;
            }
            let mut class = [0u16; 64];
            let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..class_len.min(class.len())]);
            const SHELL_CLASSES: [&str; 14] = [
                "Progman",
                "WorkerW",
                "Shell_TrayWnd",
                "Shell_SecondaryTrayWnd",
                "NotifyIconOverflowWindow",
                "TopLevelWindowForOverflowXamlIsland",
                "XamlExplorerHostIslandWindow",
                "Windows.UI.Core.CoreWindow",
                "ForegroundStaging",
                "MultitaskingViewFrame",
                "TaskSwitcherWnd",
                "TaskListThumbnailWnd",
                "#32768",
                "tray_icon_app",
            ];
            if SHELL_CLASSES.contains(&class.as_str()) {
                return false;
            }
            let mut window_rect = RECT::default();
            if !GetWindowRect(hwnd, &mut window_rect).is_ok() {
                return false;
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if monitor.0.is_null() {
                return false;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            let bounds = info.rcMonitor;
            const TOLERANCE: i32 = 2;
            window_rect.left <= bounds.left + TOLERANCE
                && window_rect.top <= bounds.top + TOLERANCE
                && window_rect.right >= bounds.right - TOLERANCE
                && window_rect.bottom >= bounds.bottom - TOLERANCE
        }
        #[cfg(not(windows))]
        false
    }

    /// (jam, menit, hari, bulan, tahun) waktu lokal.
    pub fn local_time() -> (u32, u32, u32, u32, u32) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::SystemInformation::GetLocalTime;
            let t = GetLocalTime();
            (t.wHour as u32, t.wMinute as u32, t.wDay as u32, t.wMonth as u32, t.wYear as u32)
        }
        #[cfg(not(windows))]
        {
            // Fallback UTC tanpa dependensi tambahan.
            let s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let days = s / 86_400;
            let (y, m, d) = civil_from_days(days as i64);
            (((s / 3600) % 24) as u32, ((s / 60) % 60) as u32, d, m, y)
        }
    }

    /// (detik, hari dalam pekan: 0 = Minggu).
    pub fn local_sec_wday() -> (u32, u32) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::SystemInformation::GetLocalTime;
            let t = GetLocalTime();
            (t.wSecond as u32, t.wDayOfWeek as u32)
        }
        #[cfg(not(windows))]
        {
            let s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            ((s % 60) as u32, (((s / 86_400) + 4) % 7) as u32)
        }
    }

    /// Hari sejak 1970-01-01 -> (tahun, bulan, tanggal), algoritma Howard Hinnant.
    #[allow(dead_code)]
    pub fn civil_from_days(z: i64) -> (u32, u32, u32) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400) as u32 + u32::from(m <= 2);
        (y, m, d)
    }
}

/// Decode PNG kecil yang ditanam di binary menjadi Image Slint. Tanpa decoder gambar umum,
/// jadi binary dan RAM tetap kecil. Mengembalikan gambar kosong jika formatnya bukan RGBA8.
fn load_png(bytes: &'static [u8]) -> Image {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let Ok(mut reader) = decoder.read_info() else { return Image::default() };
    let mut buf = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut buf) else { return Image::default() };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Image::default();
    }
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &buf[..info.buffer_size()],
        info.width,
        info.height,
    ))
}

const ID_DAYS: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
const ID_MONTHS: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni",
    "Juli", "Agustus", "September", "Oktober", "November", "Desember",
];

/// "3h 4j" / "5j 12m" / "9m" (sama dengan fmtUptime di Tauri).
fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs / 3600) % 24, (secs / 60) % 60);
    if d > 0 { format!("{d}h {h}j") } else if h > 0 { format!("{h}j {m}m") } else { format!("{m}m") }
}

fn gib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

/// Kecepatan jaringan dalam B/s -> teks pendek (KB/s atau MB/s).
fn fmt_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB/s", bps / 1024.0)
    }
}

fn apply_battery(w: &NotchWindow, reading: Reading) {
    match reading {
        Reading::Present(b) => {
            w.set_battery_present(true);
            w.set_battery_percent(i32::from(b.percent));
            w.set_battery_charging(b.charging);
        }
        Reading::Absent => w.set_battery_present(false),
        Reading::Unavailable => {}
    }
}

/// Isi daftar catatan di UI (maksimal 3 terbaru; halaman ini belum bisa menggulir).
/// "Baru saja" / "5 mnt lalu" / "3 jam lalu" / "2 hari lalu" / "17 Nov" (sama dengan fmtNoteDate di Tauri).
fn fmt_note_date(updated_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(updated_ms);
    let min = ((now - updated_ms) / 60_000).max(0);
    if min < 1 {
        return "Baru saja".into();
    }
    if min < 60 {
        return format!("{min} mnt lalu");
    }
    let h = min / 60;
    if h < 24 {
        return format!("{h} jam lalu");
    }
    let d = h / 24;
    if d < 7 {
        return format!("{d} hari lalu");
    }
    let (_, mo, day) = platform::civil_from_days(updated_ms.div_euclid(86_400_000));
    const SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    format!("{day} {}", SHORT[(mo.clamp(1, 12) - 1) as usize])
}

fn show_notes(model: &VecModel<NoteItem>, notes: Vec<Note>) {
    model.set_vec(
        notes
            .into_iter()
            .map(|n| {
                let first_lines = n.text.split('\n').take(2).collect::<Vec<_>>().join(" ");
                let mut preview: String = first_lines.chars().take(120).collect();
                if n.text.chars().count() > 120 {
                    preview.push('\u{2026}');
                }
                let meta = format!("{} \u{00B7} {} kar", fmt_note_date(n.updated_at), n.text.chars().count());
                NoteItem { id: n.id.into(), text: n.text.into(), preview: preview.into(), meta: meta.into() }
            })
            .collect::<Vec<_>>(),
    );
}

/// Perbarui tiga item terbaru pada halaman Clipboard. Riwayat internal tetap menyimpan 10 item.
// ---------- Currency: kurs Rupiah, saham IDX, pengingat kurs ----------

#[derive(Default)]
struct CurrencyFetch {
    rates: Option<Result<notch_core::currency::Rates, String>>,
    stocks: Option<Result<Vec<notch_core::currency::StockPoint>, String>>,
}

struct CurrencyState {
    rates: Option<notch_core::currency::Rates>,
    stocks: Vec<notch_core::currency::StockPoint>,
    rates_error: String,
    rates_warning: String,
    stock_error: String,
    amount: String, // hanya digit
    target: usize,
    alerts: Vec<notch_core::currency::Alert>,
    updated: String,
    rates_loaded_at: Option<Instant>,
    stocks_loaded_at: Option<Instant>,
    loading: bool,
}

impl CurrencyState {
    fn new(alerts: Vec<notch_core::currency::Alert>) -> Self {
        Self {
            rates: None,
            stocks: Vec::new(),
            rates_error: String::new(),
            rates_warning: String::new(),
            stock_error: String::new(),
            amount: "100000".into(),
            target: 0,
            alerts,
            updated: String::new(),
            rates_loaded_at: None,
            stocks_loaded_at: None,
            loading: false,
        }
    }
}

/// Format Rupiah untuk grafik kurs: dua desimal di bawah 1.000 (mis. Yen), selebihnya bilangan bulat dengan titik ribuan.
fn fx_idr(value: f64) -> String {
    if value < 1000.0 { format!("Rp {}", format!("{value:.2}").replace('.', ",")) } else { format!("Rp {}", group_digits(&format!("{value:.0}"))) }
}

fn fx_axis_label(value: f64, span: f64) -> String {
    let decimals = if span < 10.0 { 2 } else if span < 100.0 { 1 } else { 0 };
    fmt_id_number(value, decimals)
}

const CURRENCY_QUICK: [u64; 6] = [10_000, 50_000, 100_000, 500_000, 1_000_000, 10_000_000];
const SHORT_MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/// "1234567" -> "1.234.567".
fn group_digits(digits: &str) -> String {
    let n = digits.chars().count();
    let mut out = String::with_capacity(n + n / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Format angka gaya Indonesia: titik ribuan, koma desimal.
fn fmt_id_number(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (int, frac) = match text.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (text.clone(), None),
    };
    let mut out = group_digits(&int);
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    if value < 0.0 {
        out.insert(0, '-');
    }
    out
}

fn rupiah(value: f64) -> String {
    format!("Rp {}", fmt_id_number(value, if value >= 1000.0 { 0 } else { 2 }))
}

fn fmt_currency_value(value: f64, code: &str) -> String {
    let decimals = if matches!(code, "JPY" | "KRW") { 0 } else { 2 };
    let mut number = fmt_id_number(value, decimals);
    if decimals > 0 {
        while number.ends_with('0') { number.pop(); }
        if number.ends_with(',') { number.pop(); }
    }
    let (symbol, spaced) = match code {
        "USD" => ("US$", false), "SGD" => ("SGD", true), "EUR" => ("€", false),
        "JPY" => ("JP¥", false), "MYR" => ("MYR", true), "CNY" => ("CN¥", false),
        "GBP" => ("GBP", true), "AUD" => ("A$", false), "SAR" => ("SAR", true),
        "KRW" => ("₩", false), "THB" => ("฿", false), "INR" => ("₹", false),
        "CHF" => ("CHF", true), "HKD" => ("HK$", false), "CAD" => ("CA$", false),
        _ => (code, true),
    };
    if spaced { format!("{symbol} {number}") } else { format!("{symbol}{number}") }
}

fn alert_text(a: &notch_core::currency::Alert) -> String {
    format!("{} {} {}", a.code, if a.above { "\u{2265}" } else { "\u{2264}" }, rupiah(a.target))
}

fn paint_currency(
    w: &NotchWindow,
    s: &CurrencyState,
    gainers: &VecModel<StockRow>,
    losers: &VecModel<StockRow>,
    alerts: &VecModel<CurrencyAlertItem>,
) {
    use notch_core::currency::CURRENCIES;
    let (code, name) = CURRENCIES[s.target % CURRENCIES.len()];
    let amount: f64 = s.amount.parse().unwrap_or(0.0);
    let per_unit = s.rates.as_ref().and_then(|r| r.idr_for(code));
    let (result, rate) = match per_unit {
        Some(idr) => {
            let value = amount / idr;
            (
                fmt_currency_value(value, code),
                format!("1 {code} = {}", rupiah(idr)),
            )
        }
        None => (String::new(), String::new()),
    };
    const FULL_MONTHS: [&str; 12] = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
    let (year, month) = notch_core::currency::year_month();
    w.set_currency(CurrencyView {
        amount: group_digits(&s.amount).into(),
        code: code.into(),
        name: name.into(),
        result: result.into(),
        rate: rate.into(),
        alert_rate: per_unit.map(rupiah).unwrap_or_else(|| "—".to_string()).into(),
        updated: s.updated.clone().into(),
        warning: s.rates_warning.clone().into(),
        stock_period: format!("MTD \u{00B7} 1 {}", SHORT_MONTHS[(month.clamp(1, 12) - 1) as usize]).into(),
        stock_status: if s.stock_error.is_empty() {
            format!("Pilihan saham IDX \u{00B7} {} {} \u{00B7} data Yahoo Finance, bukan seluruh bursa", FULL_MONTHS[(month.clamp(1, 12) - 1) as usize], year).into()
        } else {
            s.stock_error.clone().into()
        },
        error: s.rates_error.clone().into(),
        loading: s.loading,
    });
    let currency_rows = CURRENCIES.iter().map(|(item_code, item_name)| {
        let value = s.rates.as_ref().and_then(|rates| rates.idr_for(item_code));
        CurrencyItem {
            code: (*item_code).into(),
            name: (*item_name).into(),
            value: value.map(fx_idr).unwrap_or_else(|| "—".into()).into(),
            selected: *item_code == code,
        }
    }).collect::<Vec<_>>();
    w.set_currency_items(ModelRc::from(Rc::new(VecModel::from(currency_rows))));
    if w.get_currency_amount().as_str() != group_digits(&s.amount) {
        w.set_currency_amount(group_digits(&s.amount).into());
    }
    let row = |p: &notch_core::currency::StockPoint, positive: bool| StockRow {
        name: p.label.clone().into(),
        price: rupiah(p.last).into(),
        change: format!("{:+.2}%", p.change).replace('.', ",").into(),
        positive,
    };
    gainers.set_vec(s.stocks.iter().filter(|p| p.change > 0.0).take(3).map(|p| row(p, true)).collect::<Vec<_>>());
    let mut down: Vec<_> = s.stocks.iter().filter(|p| p.change < 0.0).collect();
    down.sort_by(|a, b| a.change.partial_cmp(&b.change).unwrap_or(std::cmp::Ordering::Equal));
    losers.set_vec(down.into_iter().take(3).map(|p| row(p, false)).collect::<Vec<_>>());
    alerts.set_vec(
        s.alerts
            .iter()
            .map(|a| CurrencyAlertItem {
                id: a.id.clone().into(),
                text: alert_text(a).into(),
                stock: a.kind == notch_core::currency::AlertKind::Stock,
                repeat: a.repeat,
                armed: a.armed,
            })
            .collect::<Vec<_>>(),
    );
}

/// Periksa alert kurs dan saham dari snapshot harga terbaru.
/// Alert sekali pakai dihapus setelah tampil; alert berulang menunggu harga kembali
/// ke sisi sebaliknya dari ambang sebelum dipersenjatai lagi.
fn fire_due_currency_alert(w: &NotchWindow, s: &mut CurrencyState, path: &std::path::Path) -> bool {
    let can_show = !w.get_expanded() && !w.get_nudge_visible() && !w.get_reminder_alert_visible()
        && !w.get_meeting_active() && w.get_meeting_finished().is_empty() && !w.get_adhan_active();

    let mut rearmed = false;
    let mut due: Option<(notch_core::currency::Alert, f64)> = None;
    for alert in &mut s.alerts {
        let now = match alert.kind {
            notch_core::currency::AlertKind::Currency => s.rates.as_ref().and_then(|rates| rates.idr_for(&alert.code)),
            notch_core::currency::AlertKind::Stock => s.stocks.iter()
                .find(|point| point.label == alert.code)
                .map(|point| point.last),
        };
        let Some(now) = now.filter(|value| value.is_finite() && *value > 0.0) else { continue };
        if alert.repeat && !alert.armed {
            if notch_core::currency::alert_returned_to_opposite_side(alert, now) {
                alert.armed = true;
                rearmed = true;
            }
            continue;
        }
        if notch_core::currency::alert_target_reached(alert, now) && can_show {
            due = Some((alert.clone(), now));
            break;
        }
    }

    if rearmed {
        notch_core::currency::save_alerts(path, &s.alerts);
    }
    let Some((alert, now)) = due else { return rearmed };
    if alert.repeat {
        if let Some(saved) = s.alerts.iter_mut().find(|item| item.id == alert.id) {
            saved.armed = false;
        }
    } else {
        s.alerts.retain(|item| item.id != alert.id);
    }

    let action = if alert.above { "naik" } else { "turun" };
    let asset = match alert.kind {
        notch_core::currency::AlertKind::Stock => "saham IDX",
        notch_core::currency::AlertKind::Currency => "kurs",
    };
    w.set_nudge_message(format!("{} {} ke {}", alert.code, action, rupiah(now)).into());
    w.set_nudge_detail(format!("Pengingat {asset} · target {}", alert_text(&alert)).into());
    w.set_nudge_visible(true);
    let weak = w.as_weak();
    Timer::single_shot(Duration::from_secs(8), move || {
        if let Some(w) = weak.upgrade() {
            w.set_nudge_visible(false);
        }
    });
    notch_core::currency::save_alerts(path, &s.alerts);
    true
}

fn note_meta(ms: i64) -> String {
    let (y, m, d, h, mi) = reminder::local_parts(ms, local_timezone_offset_minutes());
    format!("{d} {} {y}, {h:02}.{mi:02}", SHORT_MONTHS[(m.clamp(1, 12) - 1) as usize])
}

fn note_counts(text: &str) -> (String, String) {
    (
        format!("{} karakter", group_digits(&text.chars().count().to_string())),
        format!("{} baris", if text.is_empty() { 0 } else { text.split('\n').count() }),
    )
}

/// Buka (atau fokuskan) jendela detail untuk satu catatan: edit, simpan (Ctrl+S), hapus dua langkah, Esc.
fn open_note_detail(
    id: &str,
    store: &Rc<RefCell<Option<NotesStore>>>,
    path: &std::path::Path,
    model: &Rc<VecModel<NoteItem>>,
    details: &Rc<RefCell<std::collections::HashMap<String, NoteDetailWindow>>>,
    accent: slint::Color,
) {
    if let Some(existing) = details.borrow().get(id) {
        let _ = existing.show();
        return;
    }
    let note = {
        let mut guard = store.borrow_mut();
        let s = guard.get_or_insert_with(|| NotesStore::open(path));
        s.list().into_iter().find(|n| n.id == id)
    };
    let Some(note) = note else { return };
    let Ok(win) = NoteDetailWindow::new() else { return };
    win.set_accent(accent);
    win.set_text(note.text.clone().into());
    win.set_meta(note_meta(note.updated_at).into());
    let (chars, lines) = note_counts(&note.text);
    win.set_chars(chars.into());
    win.set_lines(lines.into());
    let original = Rc::new(RefCell::new(note.text.clone()));
    let warned = Rc::new(Cell::new(false));
    let note_id = id.to_string();

    let toast = {
        let weak = win.as_weak();
        Rc::new(move |msg: &str| {
            if let Some(w) = weak.upgrade() {
                w.set_toast(msg.into());
                let weak = w.as_weak();
                Timer::single_shot(Duration::from_secs(2), move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_toast("".into());
                    }
                });
            }
        })
    };
    let close: Rc<dyn Fn()> = {
        let (weak, details, note_id) = (win.as_weak(), details.clone(), note_id.clone());
        Rc::new(move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
            // Lepas jendela setelah callback selesai agar tidak membebaskan dirinya sendiri.
            let (details, note_id) = (details.clone(), note_id.clone());
            Timer::single_shot(Duration::from_millis(50), move || {
                details.borrow_mut().remove(&note_id);
            });
        })
    };
    {
        let (weak, original, warned) = (win.as_weak(), original.clone(), warned.clone());
        win.on_edited(move |text| {
            let Some(w) = weak.upgrade() else { return };
            let dirty = text.trim() != original.borrow().trim();
            w.set_dirty(dirty);
            w.set_can_save(dirty && !text.trim().is_empty());
            let (chars, lines) = note_counts(text.as_str());
            w.set_chars(chars.into());
            w.set_lines(lines.into());
            if !dirty {
                warned.set(false);
            }
        });
    }
    {
        let (weak, store, path, model, original, toast, note_id) =
            (win.as_weak(), store.clone(), path.to_path_buf(), model.clone(), original.clone(), toast.clone(), note_id.clone());
        win.on_save(move || {
            let Some(w) = weak.upgrade() else { return };
            let text = w.get_text().trim().to_string();
            if text.is_empty() || !w.get_can_save() {
                return;
            }
            let list = {
                let mut guard = store.borrow_mut();
                let s = guard.get_or_insert_with(|| NotesStore::open(&path));
                s.update(&note_id, &text)
            };
            if let Some(n) = list.iter().find(|n| n.id == note_id) {
                w.set_meta(note_meta(n.updated_at).into());
            }
            show_notes(&model, list);
            *original.borrow_mut() = text.clone();
            w.set_text(text.clone().into());
            let (chars, lines) = note_counts(&text);
            w.set_chars(chars.into());
            w.set_lines(lines.into());
            w.set_dirty(false);
            w.set_can_save(false);
            toast("\u{2713} Disimpan");
        });
    }
    {
        let (weak, store, path, model, close, note_id) =
            (win.as_weak(), store.clone(), path.to_path_buf(), model.clone(), close.clone(), note_id.clone());
        win.on_delete(move || {
            let Some(w) = weak.upgrade() else { return };
            if !w.get_confirm_delete() {
                w.set_confirm_delete(true);
                let weak = w.as_weak();
                Timer::single_shot(Duration::from_secs(3), move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_confirm_delete(false);
                    }
                });
                return;
            }
            let list = {
                let mut guard = store.borrow_mut();
                let s = guard.get_or_insert_with(|| NotesStore::open(&path));
                s.remove(&note_id)
            };
            show_notes(&model, list);
            close();
        });
    }
    {
        let (weak, close, warned, toast) = (win.as_weak(), close.clone(), warned.clone(), toast.clone());
        win.on_esc(move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_dirty() && !warned.get() {
                warned.set(true);
                toast("Belum disimpan \u{2014} tekan Esc lagi untuk tutup");
            } else {
                close();
            }
        });
    }
    {
        let close = close.clone();
        win.window().on_close_requested(move || {
            close();
            slint::CloseRequestResponse::HideWindow
        });
    }
    let _ = win.show();
    details.borrow_mut().insert(id.to_string(), win);
}

fn show_clipboard(model: &VecModel<ClipboardItem>, monitor: Option<&ClipboardMonitor>) {
    let items = monitor.map(ClipboardMonitor::list).unwrap_or_default();
    model.set_vec(
        items
            .into_iter()
            .take(10)
            .map(|text| {
                let preview = text.replace('\n', " ").replace('\r', " ");
                let mut chars = preview.chars();
                let short: String = chars.by_ref().take(64).collect();
                let preview = if chars.next().is_some() {
                    format!("{short}…")
                } else {
                    short
                };
                ClipboardItem { preview: preview.into(), text: text.into() }
            })
            .collect::<Vec<_>>(),
    );
}

/// Ambil cuaca di thread latar, lalu kirim hasilnya ke thread Slint.
fn refresh_weather(
    weak: Weak<NotchWindow>,
    busy: Arc<AtomicBool>,
    lat: f64,
    lon: f64,
    city: String,
) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let result = notch_core::weather::fetch(lat, lon);
        busy.store(false, Ordering::SeqCst);
        let _ = weak.upgrade_in_event_loop(move |w| {
            w.set_weather_city(city.into());
            match result {
                Ok(reading) => {
                    w.set_weather_temp(format!("{:.0}°", reading.temp).into());
                    w.set_weather_condition(notch_core::weather::describe(reading.code, reading.is_day).into());
                    w.set_weather_feels(format!("Terasa {:.0}°", reading.feels).into());
                    w.set_weather_humidity(format!("Kelembapan {:.0}%", reading.humidity).into());
                    let rain = if reading.raining {
                        "Sedang hujan".to_string()
                    } else if let Some(minutes) = reading.rain_in {
                        format!("Hujan dalam {minutes} menit")
                    } else {
                        "Tidak ada hujan terdeteksi".to_string()
                    };
                    w.set_weather_rain(rain.into());
                    w.set_weather_icon(notch_core::weather::icon_path(reading.code, reading.is_day).into());
                    w.set_weather_status("".into());
                }
                Err(error) => {
                    w.set_weather_temp("--°".into());
                    w.set_weather_condition("Data cuaca belum tersedia".into());
                    w.set_weather_feels("".into());
                    w.set_weather_humidity("".into());
                    w.set_weather_rain("Periksa koneksi internet".into());
                    w.set_weather_icon("".into());
                    w.set_weather_status(format!("Gagal memuat: {error}").into());
                }
            }
        });
    });
}



#[derive(Clone, Default)]
struct PrayerSchedule { current: Vec<PrayerDay>, next: Vec<PrayerDay>, error: String }
fn prayer_at(date:(i64,i64,i64),time:&str,offset:i32)->Option<i64>{let(y,m,d)=date;reminder::parse_local_datetime(&format!("{d:02}/{m:02}/{y} {time}"),offset)}
fn prayer_countdown(target:Option<i64>,now:i64)->String{let Some(t)=target else{return "--:--:--".into()};let s=(t-now).max(0)/1000;format!("{:02}:{:02}:{:02}",s/3600,(s%3600)/60,s%60)}
fn prayer_find(days:&[PrayerDay],day:i64)->Option<PrayerDay>{days.iter().find(|x|i64::from(x.tanggal)==day).cloned()}
fn render_prayer(w:&NotchWindow,s:&PrayerSchedule,offset:i32,ramadan:bool){
 let now=reminder::now_ms();let(y,m,d,_,_)=reminder::local_parts(now,offset);let(ny,nm,nd,_,_)=reminder::local_parts(now+86_400_000,offset);
 let today=prayer_find(&s.current,d);let tomorrow=if y==ny&&m==nm{today.clone()}else{prayer_find(&s.next,nd)};
 let entries:[(&str,&str);5]=today.as_ref().map(|x|[("Subuh",x.subuh.as_str()),("Dzuhur",x.dzuhur.as_str()),("Ashar",x.ashar.as_str()),("Maghrib",x.maghrib.as_str()),("Isya",x.isya.as_str())]).unwrap_or([("Subuh",""),("Dzuhur",""),("Ashar",""),("Maghrib",""),("Isya","")]);
 let mut next:Option<(&str,&str,i64)>=None;for(name,time)in entries{if let Some(at)=prayer_at((y,m,d),time,offset){if at>now{next=Some((name,time,at));break;}}}
 if next.is_none(){if let Some(t)=tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.subuh,offset)){next=Some(("Subuh",tomorrow.as_ref().unwrap().subuh.as_str(),t));}}
 if let Some((name,time,at))=next{w.set_prayer_next_name(name.into());w.set_prayer_next_time(time.into());w.set_prayer_countdown(prayer_countdown(Some(at),now).into());}else{w.set_prayer_next_name("Jadwal belum tersedia".into());w.set_prayer_next_time("--:--".into());w.set_prayer_countdown("--:--:--".into());}
 let rows=entries.into_iter().map(|(name,time)|{let at=prayer_at((y,m,d),time,offset);let state=if next.map(|x|x.0)==Some(name){1}else if at.map(|t|t<=now).unwrap_or(false){2}else{0};PrayerItem{name:name.into(),time:if time.is_empty(){"--:--".into()}else{time.into()},state}}).collect::<Vec<_>>();
 w.set_prayer_items(ModelRc::from(Rc::new(VecModel::from(rows))));w.set_prayer_status(s.error.clone().into());
 {let tm=|t:&str|if t.is_empty(){"--:--".to_string()}else{t.to_string()};
  let (im,te)=today.as_ref().map(|x|(tm(&x.imsak),tm(&x.terbit))).unwrap_or(("--:--".into(),"--:--".into()));
  w.set_prayer_meta(format!("Imsak {im} \u{00B7} Terbit {te}").into());
  let tomorrow_next=next.map(|x|reminder::local_parts(x.2,offset).2!=d).unwrap_or(false);
  w.set_prayer_when(if next.is_none(){""}else if tomorrow_next{"Besok"}else{"Hari ini"}.into());
  let sheet:Vec<(&str,String)>=match today.as_ref(){Some(x)=>vec![("Imsak",tm(&x.imsak)),("Subuh",tm(&x.subuh)),("Terbit",tm(&x.terbit)),("Dzuhur",tm(&x.dzuhur)),("Ashar",tm(&x.ashar)),("Maghrib",tm(&x.maghrib)),("Isya",tm(&x.isya))],None=>["Imsak","Subuh","Terbit","Dzuhur","Ashar","Maghrib","Isya"].into_iter().map(|n|(n,"--:--".to_string())).collect()};
  w.set_prayer_imsakiyah(ModelRc::from(Rc::new(VecModel::from(sheet.into_iter().map(|(n,t)|PrayerItem{name:n.into(),time:t.into(),state:0}).collect::<Vec<_>>()))));}
 if ramadan{let im=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.imsak,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.imsak,offset)));let bu=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.maghrib,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.maghrib,offset)));{let (_,wd)=platform::local_sec_wday();w.set_ramadan_date(format!("{}, {d} {} {y}",ID_DAYS[(wd%7) as usize],ID_MONTHS[(m.clamp(1,12)-1) as usize]).into());}w.set_ramadan_imsak_countdown(prayer_countdown(im,now).into());w.set_ramadan_buka_countdown(prayer_countdown(bu,now).into());w.set_ramadan_imsak_time(format!("Imsak {}",today.as_ref().map(|x|x.imsak.as_str()).unwrap_or("--:--")).into());w.set_ramadan_buka_time(format!("Maghrib {}",today.as_ref().map(|x|x.maghrib.as_str()).unwrap_or("--:--")).into());}
}
fn load_ramadan()->bool{std::fs::read_to_string(notch_core::paths::data_file("ramadan.json")).ok().map(|s|s.trim()=="true").unwrap_or(false)}

fn local_timezone_offset_minutes() -> i32 {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "(Get-Date).ToString('zzz')"])
            .creation_flags(0x0800_0000)
            .output()
    };
    #[cfg(not(windows))]
    let output = std::process::Command::new("date").arg("+%z").output();

    let Ok(output) = output else { return 0 };
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let compact = raw.replace(':', "");
    if compact.len() != 5 { return 0; }
    let sign = if compact.starts_with('-') { -1 } else if compact.starts_with('+') { 1 } else { return 0 };
    let hours = compact[1..3].parse::<i32>().unwrap_or(0);
    let minutes = compact[3..5].parse::<i32>().unwrap_or(0);
    -(sign * (hours * 60 + minutes))
}

fn weekday_selection(mask: u8) -> Vec<bool> {
    (0..7).map(|day| mask & (1 << day) != 0).collect()
}

fn show_reminders(model: &VecModel<ReminderItem>, reminders: Vec<Reminder>, offset: i32) {
    let items = reminders.into_iter().map(|r| {
        let due = if r.due_at <= 0 {
            "Tanpa waktu".to_string()
        } else {
            let (year, month, day, hour, minute) = reminder::local_parts(r.due_at, offset);
            let when = format!("{day:02}/{month:02}/{year} {hour:02}:{minute:02}");
            let repeat = match reminder::effective_repeat_mode(&r) {
                "daily" => " · Harian",
                "weekly" => " · Mingguan",
                "monthly" => " · Bulanan",
                _ => "",
            };
            format!("{when}{repeat}")
        };
        ReminderItem { id: r.id.into(), text: r.text.into(), due: due.into(), done: r.done }
    }).collect::<Vec<_>>();
    model.set_vec(items);
}

fn col(c: Rgba) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn apply_theme(w: &NotchWindow, p: &Palette) {
    let t = Theme::get(w);
    t.set_body(col(p.body));
    t.set_text(col(p.text));
    t.set_text_dim(col(p.text_dim));
    t.set_text_faint(col(p.text_faint));
    t.set_accent(col(p.accent));
    t.set_on_accent(col(p.on_accent));
    t.set_ok(col(p.ok));
    t.set_chip(col(p.chip));
    t.set_chip_hover(col(p.chip_hover));
    t.set_track(col(p.track));
}

/// None -> -1 (tidak didukung/belum terbaca), false -> 0, true -> 1.
fn tri(v: Option<bool>) -> i32 {
    match v {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    }
}

fn apply_quick(w: &NotchWindow, q: &Quick) {
    w.set_q_mic(tri(q.mic));
    w.set_q_wifi(tri(q.wifi));
    w.set_q_bt(tri(q.bt));
    w.set_q_awake(tri(q.awake));
    w.set_q_dark(tri(q.dark));
}

/// Baca keadaan Quick di thread lain (bisa sampai 3 detik) lalu tampilkan di thread UI.
fn refresh_quick(weak: Weak<NotchWindow>, busy: Arc<AtomicBool>, awake: Arc<AtomicBool>) {
    if busy.swap(true, Ordering::SeqCst) {
        return; // pembacaan sebelumnya masih berjalan
    }
    std::thread::spawn(move || {
        let q = quick::state();
        awake.store(q.awake == Some(true), Ordering::Relaxed); // mata tidak mengantuk saat keep-awake
        let _ = weak.upgrade_in_event_loop(move |w| apply_quick(&w, &q));
        busy.store(false, Ordering::SeqCst);
    });
}

fn show_pomo(w: &NotchWindow, p: &Pomodoro) {
    w.set_pomo_text(p.time_text().into());
    w.set_pomo_label(p.label().into());
    w.set_pomo_progress(p.progress());
    w.set_pomo_running(p.running());
    w.set_pomo_rest(p.phase() != Phase::Focus);
}

// Ukuran pil saat melebar (px logis); harus sama dengan `pill` di ui/notch.slint.
const PILL_W: f32 = 580.0;
const PILL_H: f32 = 202.0;

/// Penempatan jendela + bentuk tepi + gambar hiasan tema. Hiasan dirender ulang hanya saat mode tepi
/// (atas/melayang/bawah) atau faktor DPI berubah.
struct Shell {
    pos: Cell<Option<(i32, i32)>>, // posisi kustom (pusat x, atas y) atau None = tengah atas
    // Anchor monitor untuk memulihkan dock kiri/kanan; sentinel posisi tidak menyimpan koordinat monitor.
    monitor_hint: Cell<Option<(i32, i32)>>,
    theme: RefCell<String>,
    body: RefCell<Rgba>,
    scale: RefCell<f32>,
    memo: Cell<(Option<Dock>, u32)>, // (dock, bit faktor DPI) yang sedang terpasang
}

impl Shell {
    /// Letakkan jendela sesuai posisi config dan atur bentuk tepinya (menempel atas / melayang / bawah).
    fn place(&self, win: &NotchWindow, size: (u32, u32)) {
        let (origin, dock, size) = self.geometry(win, size);
        if self.memo.get() != (Some(dock), win.window().scale_factor().to_bits()) {
            self.memo.set((Some(dock), win.window().scale_factor().to_bits()));
            win.set_dock(match dock {
                Dock::Top => 0,
                Dock::Floating => 1,
                Dock::Bottom => 2,
                Dock::Left => 3,
                Dock::Right => 4,
            });
            self.rebuild_decor(win, dock, win.window().scale_factor());
        }
        let w = win.window();
        if platform::set_bounds(w, origin.0, origin.1, size.0, size.1) {
            // Selaraskan layout Slint dengan ukuran native saat itu juga, bukan menunggu event Resized.
            w.set_size(PhysicalSize::new(size.0, size.1));
        } else {
            w.set_size(PhysicalSize::new(size.0, size.1));
            w.set_position(PhysicalPosition::new(origin.0, origin.1));
        }
        // Region selalu dipasang (persegi penuh seukuran jendela, sejak penempatan pertama) dan TIDAK PERNAH NULL: DWM
        // menggambar bingkai non-client yang tembus terlihat lewat klien transparan (bingkai biru muda di tepi jendela)
        // pada jendela tanpa region. Pemotongan notch menciut (clip_to_collapsed) hanya mempersempit region ini.
        platform::set_region(w, Some((0, 0, size.0 as i32, size.1 as i32)));
    }

    /// Posisi layar, dock, dan ukuran fisik jendela untuk `size` (piksel logis), tanpa menerapkannya.
    fn geometry(&self, win: &NotchWindow, size: (u32, u32)) -> ((i32, i32), Dock, (u32, u32)) {
        let pos = self.pos.get();
        // Sentinel sisi bukan koordinat monitor. Saat drag ke sisi, pertahankan monitor
        // tempat jendela sedang berada (posisi fisik sebelum ukuran/docking diterapkan).
        // Jika belum ada posisi jendela yang valid, fallback tetap ke monitor utama.
        let side_docked = pos.is_some_and(|(x, _)| x == layout::LEFT_TX || x == layout::RIGHT_TX);
        let monitor_hint = if let Some(anchor) = self.monitor_hint.get() {
            Some(anchor)
        } else if side_docked {
            // Config lama belum menyimpan anchor: gunakan monitor tempat window berada.
            let current = win.window().position();
            let current_size = win.window().size();
            (current_size.width > 0 && current_size.height > 0).then_some((
                current.x + current_size.width as i32 / 2,
                current.y + current_size.height as i32 / 2,
            ))
        } else {
            // Sentinel BOTTOM_TY bukan koordinat desktop. Tanpa anchor lama, fallback ke monitor utama.
            pos.filter(|(_, y)| *y < layout::BOTTOM_TY / 2)
        };
        let work = platform::work_area(monitor_hint);
        // COLLAPSED/EXPANDED dalam piksel logis; jendela diukur dalam piksel fisik
        let sf = win.window().scale_factor();
        let side = pos.is_some_and(|(x, _)| x == layout::LEFT_TX || x == layout::RIGHT_TX);
        let logical_size = if side && size == COLLAPSED { COLLAPSED_SIDE } else { size };
        let size = ((logical_size.0 as f32 * sf).round() as u32, (logical_size.1 as f32 * sf).round() as u32);
        let (origin, dock) = layout::place(&work, size.0 as i32, size.1 as i32, pos);
        (origin, dock, size)
    }

    /// Tampilkan notch menciut TANPA mengubah ukuran jendela: jendela tetap seukuran hub dan hanya persegi notch
    /// yang terlihat (lihat `clip`). Mengembalikan false bila notch tidak bisa disejajarkan dengan pasti (dock
    /// samping, atau jendela besar terjepit tepi layar); pemanggil lalu mengecilkan jendela seperti biasa.
    fn clip_to_collapsed(&self, win: &NotchWindow, size: (u32, u32)) -> bool {
        // Saklar uji: jalankan dengan variabel lingkungan NOTCH_NO_CLIP=1 untuk memakai cara lama (jendela diperkecil).
        static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DISABLED.get_or_init(|| std::env::var_os("NOTCH_NO_CLIP").is_some()) {
            return false;
        }
        let (origin, dock, csize) = self.geometry(win, size);
        // Dock samping dan bawah menempel di tepi lain (dan tatapan mata memakai tepi atas jendela): tetap diperkecil.
        if !matches!(dock, Dock::Top | Dock::Floating) {
            return false;
        }
        let w = win.window();
        let (bpos, bsize) = (w.position(), w.size());
        let Some(rect) = clip::collapsed_rect(
            origin,
            (csize.0 as i32, csize.1 as i32),
            (bpos.x, bpos.y),
            (bsize.width as i32, bsize.height as i32),
        ) else {
            return false;
        };
        platform::set_region(w, Some(rect))
    }

    fn rebuild_decor(&self, win: &NotchWindow, dock: Dock, sf: f32) {
        if matches!(dock, Dock::Left | Dock::Right) {
            win.set_has_decor(false);
            return;
        }
        let (w, h) = ((PILL_W * sf).round() as u32, (PILL_H * sf).round() as u32);
        let r = (if dock == Dock::Floating { 28.0 } else { 32.0 }) * *self.scale.borrow() * sf;
        match decor::render(&self.theme.borrow(), *self.body.borrow(), dock, w, h, r, sf) {
            Some(buf) => {
                win.set_decor(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, w, h)));
                win.set_has_decor(true);
            }
            None => win.set_has_decor(false), // tema default: tanpa hiasan
        }
    }
}

/// Pastikan ukuran jendela native mengikuti hub yang terbuka. Pada hover pertama setelah startup, `set_size`
/// bisa diabaikan atau ditimpa (jendela native belum siap, atau hide/show untuk menyembunyikan taskbar), sehingga
/// hub tergambar terpotong di jendela 300 x 140. Cek ulang beberapa kali dan pasang ulang ukuran bila tidak cocok.
fn settle_expanded_geometry(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>, tries_left: u8) {
    if tries_left == 0 {
        return;
    }
    Timer::single_shot(Duration::from_millis(80), move || {
        let Some(w) = weak.upgrade() else { return };
        if !w.get_expanded() {
            return;
        }
        let (lw, lh) = expanded_size(&w);
        let sf = w.window().scale_factor();
        let want = ((lw as f32 * sf).round() as i32, (lh as f32 * sf).round() as i32);
        let got = w.window().size();
        if (got.width as i32 - want.0).abs() > 2 || (got.height as i32 - want.1).abs() > 2 {
            eprintln!("[Geometry] ukuran hub {}x{} != {}x{}, dipasang ulang", got.width, got.height, want.0, want.1);
            shell.memo.set((None, 0));
            shell.place(&w, (lw, lh));
            settle_expanded_geometry(weak.clone(), shell.clone(), tries_left - 1);
        }
    });
}

/// Perkecil jendela ke ukuran menciut setelah animasi menciut selesai (notch tetap terlihat, seperti Tauri).
/// Saat jendela native berubah ukuran/posisi, Windows bisa menampilkan frame terakhir pada koordinat baru sampai
/// Slint menggambar ulang. Perubahan dijatuhkan tepat setelah vsync DWM supaya gambar ulang (jendela kecil, cepat)
/// selesai sebelum komposisi berikutnya dan frame basi hampir tidak sempat tampil.
fn shrink_window_synced(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>) {
    let Some(w) = weak.upgrade() else { return };
    if w.get_expanded() {
        return;
    }
    // Greeting inline (385 px) masih tampil: jangan potong ke 300 px.
    let target = |w: &NotchWindow| if w.get_greeting_inline_visible() { GREETING_INLINE } else { COLLAPSED };
    let size = target(&w);
    if !window_needs_resize(&w, size) {
        // Ukuran sama (hanya posisi/dekorasi yang bisa berubah): tidak ada frame basi yang perlu disembunyikan.
        shell.place(&w, size);
        return;
    }
    // Cara utama: jendela dibiarkan besar dan hanya persegi notch yang tampil. Tanpa perubahan ukuran tidak ada
    // frame basi, jadi notch tidak berkedip. Jendela baru benar-benar diperkecil bila ini tidak mungkin.
    if shell.clip_to_collapsed(&w, size) {
        return;
    }
    // Jendela akan mengecil sementara notch (mata) masih tampak: frame lama jendela besar tampil di koordinat baru
    // sebagai notch/mata ganda ("hantu"), sama seperti sisi expand(). Sembunyikan notch dulu, beri beberapa frame
    // agar frame kosong sempat sampai ke layar, baru ubah ukuran jendela tepat setelah vsync.
    w.set_geometry_hold(true);
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(w) = weak.upgrade() else { return };
        // Hub dibuka lagi selama jeda: expand() yang mengatur ukuran, cukup lepas penahan.
        if w.get_expanded() {
            w.set_geometry_hold(false);
            return;
        }
        platform::wait_vsync();
        // Jendela sudah berganti ukuran di bawah frame kosong, jadi notch boleh tampil seketika (tanpa fade-in).
        w.set_hold_instant(true);
        w.set_geometry_hold(false);
        shell.place(&w, target(&w));
        // Kembalikan fade-in normal (dipakai saat hub membuka) setelah animasi pill sempat dievaluasi.
        let weak = w.as_weak();
        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(w) = weak.upgrade() {
                w.set_hold_instant(false);
            }
        });
    });
}

/// Tampilkan kartu greeting inline (dock atas) dengan aman. Jendela harus membesar 300 -> 385 px; bila itu dilakukan
/// saat notch masih tampak, frame lama muncul di koordinat baru sebagai notch ganda ("hantu"), sama seperti pada
/// `expand()`. Karena itu notch disembunyikan dulu, beberapa frame diberi waktu sampai ke layar, jendela dibesarkan
/// tepat setelah vsync, lalu kartu ditampilkan di jendela yang sudah berukuran benar.
fn show_greeting_inline(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>) {
    let Some(w) = weak.upgrade() else { return };
    if w.get_expanded() {
        return;
    }
    if !window_needs_resize(&w, GREETING_INLINE) {
        shell.place(&w, GREETING_INLINE);
        w.set_greeting_inline_visible(true);
        return;
    }
    w.set_geometry_hold(true);
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(w) = weak.upgrade() else { return };
        // Hub dibuka selama jeda: expand() yang mengatur ukuran, cukup lepas penahan.
        if w.get_expanded() {
            w.set_geometry_hold(false);
            return;
        }
        platform::wait_vsync();
        w.set_hold_instant(true);
        shell.place(&w, GREETING_INLINE);
        w.set_greeting_inline_visible(true);
        w.set_geometry_hold(false);
        let weak = w.as_weak();
        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(w) = weak.upgrade() {
                w.set_hold_instant(false);
            }
        });
    });
}

/// True bila ukuran jendela native berbeda dari `size` (piksel logis) setelah dikali skala layar.
fn window_needs_resize(w: &NotchWindow, size: (u32, u32)) -> bool {
    let sf = w.window().scale_factor();
    let got = w.window().size();
    let want = ((size.0 as f32 * sf).round() as i32, (size.1 as f32 * sf).round() as i32);
    (got.width as i32 - want.0).abs() > 2 || (got.height as i32 - want.1).abs() > 2
}

/// Bagian akhir `expand()`: besarkan jendela, buka hub, dan tampilkan notch lagi (memudar masuk).
fn finish_expand(weak: &slint::Weak<NotchWindow>, shell: &Rc<Shell>, sync: &dyn Fn()) {
    let Some(w) = weak.upgrade() else { return };
    w.set_geometry_hold(false);
    let size = expanded_size(&w);
    shell.place(&w, size);
    w.set_expanded(true);
    sync();
    settle_expanded_geometry(weak.clone(), shell.clone(), 8);
}

/// Tanggal lokal hari ini.
fn today() -> Date {
    let (_, _, d, m, y) = platform::local_time();
    Date::new(y as i32, m, d)
}

/// Isi ulang kisi kalender, judul, dan keterangan di UI dari keadaan `view` + data libur.
fn show_calendar(w: &NotchWindow, model: &VecModel<CalCell>, view: &CalView, hol: &Holidays) {
    let t = today();
    model.set_vec(
        calendar::cells_for(view.year, view.month)
            .iter()
            .map(|c| CalCell {
                day: c.date.d as i32,
                in_month: c.in_month,
                sun: c.date.weekday() == 0,
                today: c.date == t,
                sel: view.selected == Some(c.date),
                kind: hol.events_on(c.date).first().map_or(0, |e| e.kind.code()),
            })
            .collect::<Vec<_>>(),
    );
    w.set_cal_title(calendar::title(view.year, view.month).into());
    let (date, name, kind) = view.footer(hol);
    w.set_cal_foot_date(date.into());
    w.set_cal_foot_name(name.into());
    w.set_cal_foot_kind(kind);
}

/// Diset oleh tombol "Mulai ulang Notch" di Pengaturan; `main` menjalankan proses baru setelah event loop berhenti.
static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Cari kota lewat Open-Meteo geocoding, sama dengan kolom "Cari kota" di Pengaturan Tauri.
/// Blokir sampai selesai: panggil dari thread latar. Hasil: (label, nama, lat, lon).
fn search_cities(query: &str) -> Result<Vec<(String, String, String, String)>, String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language=id&format=json",
        percent_encode(query)
    );
    let body = notch_core::http::get_text(&url, 8)?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for item in json.get("results").and_then(|r| r.as_array()).into_iter().flatten() {
        let (Some(name), Some(lat), Some(lon)) = (
            item.get("name").and_then(|v| v.as_str()),
            item.get("latitude").and_then(|v| v.as_f64()),
            item.get("longitude").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        let mut label = name.to_string();
        for key in ["admin1", "country"] {
            if let Some(part) = item.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty() && *v != name) {
                label.push_str(", ");
                label.push_str(part);
            }
        }
        hits.push((label, name.chars().take(80).collect(), lat.to_string(), lon.to_string()));
    }
    Ok(hits)
}

/// Salin nilai tampilan dari NotchWindow (pemilik state) ke jendela Pengaturan. Property yang nilainya sama tidak memicu render ulang.
fn sync_settings_window(m: &NotchWindow, s: &SettingsWindow) {
    s.set_weather_city(m.get_weather_city());
    s.set_weather_lat(m.get_weather_lat());
    s.set_weather_lon(m.get_weather_lon());
    s.set_weather_save_status(m.get_weather_save_status());
    s.set_weather_hits(m.get_weather_hits());
    s.set_weather_search_status(m.get_weather_search_status());
    s.set_adhan_active(m.get_adhan_active());
    s.set_shortcut_hub(m.get_shortcut_hub());
    s.set_shortcut_settings(m.get_shortcut_settings());
    s.set_shortcut_pomodoro(m.get_shortcut_pomodoro());
    s.set_shortcut_prayer(m.get_shortcut_prayer());
    s.set_appearance_theme(m.get_appearance_theme());
    s.set_appearance_eye_theme(m.get_appearance_eye_theme());
    s.set_appearance_notch_theme(m.get_appearance_notch_theme());
    s.set_appearance_mood(m.get_appearance_mood());
    s.set_appearance_accent(m.get_appearance_accent());
    s.set_appearance_custom_bg(m.get_appearance_custom_bg());
    s.set_appearance_custom_text(m.get_appearance_custom_text());
    s.set_appearance_dark_bg(m.get_appearance_dark_bg());
    s.set_appearance_light_bg(m.get_appearance_light_bg());
    s.set_appearance_save_status(m.get_appearance_save_status());
    s.set_behavior_scale(m.get_behavior_scale());
    s.set_behavior_scale_value(m.get_behavior_scale().parse::<f32>().unwrap_or(1.0));
    s.set_behavior_hover_open(m.get_behavior_hover_open());
    s.set_behavior_hover_close(m.get_behavior_hover_close());
    s.set_behavior_sleep_after(m.get_behavior_sleep_after());
    s.set_behavior_hide_fullscreen(m.get_behavior_hide_fullscreen());
    s.set_autostart_enabled(m.get_autostart_enabled());
    s.set_autostart_save_status(m.get_autostart_save_status());
    s.set_behavior_save_status(m.get_behavior_save_status());
    s.set_motion_enabled(m.get_motion_enabled());
    s.set_dock_play_enabled(m.get_dock_play_enabled());
    s.set_motion_interval(m.get_motion_interval());
    s.set_motion_max_moves(m.get_motion_max_moves());
    s.set_motion_save_status(m.get_motion_save_status());
    s.set_nudge_enabled(m.get_nudge_enabled());
    s.set_nudge_idle_min(m.get_nudge_idle_min());
    s.set_nudge_message(m.get_nudge_message());
    s.set_nudge_emoji(m.get_nudge_emoji());
    s.set_nudge_mood(m.get_nudge_mood());
    s.set_nudge_show_ms(m.get_nudge_show_ms());
    s.set_nudge_only_silent(m.get_nudge_only_silent());
    s.set_nudge_until_move(m.get_nudge_until_move());
    s.set_nudge_save_status(m.get_nudge_save_status());
    s.set_greeting_enabled(m.get_greeting_enabled());
    s.set_greeting_mode(m.get_greeting_mode());
    s.set_greeting_voice(m.get_greeting_voice());
    s.set_greeting_custom_text(m.get_greeting_custom_text());
    s.set_greeting_save_status(m.get_greeting_save_status());
    s.set_conversation_enabled(m.get_conversation_enabled());
    s.set_conversation_on_play(m.get_conversation_on_play());
    s.set_conversation_on_track_change(m.get_conversation_on_track_change());
    s.set_conversation_on_pause(m.get_conversation_on_pause());
    s.set_conversation_on_return(m.get_conversation_on_return());
    s.set_conversation_on_weather_change(m.get_conversation_on_weather_change());
    s.set_conversation_on_rain_soon(m.get_conversation_on_rain_soon());
    s.set_conversation_on_low_battery(m.get_conversation_on_low_battery());
    s.set_conversation_on_charging(m.get_conversation_on_charging());
    s.set_conversation_return_after_min(m.get_conversation_return_after_min());
    s.set_conversation_cooldown_min(m.get_conversation_cooldown_min());
    s.set_conversation_save_status(m.get_conversation_save_status());
    s.set_focus_companion_enabled(m.get_focus_companion_enabled());
    s.set_focus_companion_on_start(m.get_focus_companion_on_start());
    s.set_focus_companion_on_halfway(m.get_focus_companion_on_halfway());
    s.set_focus_companion_on_focus_complete(m.get_focus_companion_on_focus_complete());
    s.set_focus_companion_on_break_end(m.get_focus_companion_on_break_end());
    s.set_focus_companion_save_status(m.get_focus_companion_save_status());
    s.set_position_mode(m.get_position_mode());
    s.set_active_pages(m.get_active_pages());
    s.set_hub_pages(m.get_hub_pages());
    s.set_pages_save_status(m.get_pages_save_status());
    s.set_prayer_province(m.get_prayer_province());
    s.set_prayer_city(m.get_prayer_city());
    s.set_prayer_mode(m.get_prayer_mode());
    s.set_prayer_remind_before(m.get_prayer_remind_before());
    s.set_prayer_remind_min(m.get_prayer_remind_min());
    s.set_pomodoro_focus(m.get_pomodoro_focus());
    s.set_pomodoro_break(m.get_pomodoro_break());
    s.set_pomodoro_long_break(m.get_pomodoro_long_break());
    s.set_pomodoro_rounds(m.get_pomodoro_rounds());
    s.set_save_status(m.get_settings_save_status());
}

/// Sinkronkan status yang berubah saat Settings terbuka tanpa menimpa input yang sedang diedit pengguna.
fn sync_settings_live_state(m: &NotchWindow, s: &SettingsWindow) {
    s.set_weather_save_status(m.get_weather_save_status());
    s.set_weather_hits(m.get_weather_hits());
    s.set_weather_search_status(m.get_weather_search_status());
    s.set_adhan_active(m.get_adhan_active());
    s.set_appearance_save_status(m.get_appearance_save_status());
    s.set_autostart_save_status(m.get_autostart_save_status());
    s.set_behavior_save_status(m.get_behavior_save_status());
    s.set_motion_save_status(m.get_motion_save_status());
    s.set_nudge_save_status(m.get_nudge_save_status());
    s.set_greeting_save_status(m.get_greeting_save_status());
    s.set_conversation_save_status(m.get_conversation_save_status());
    s.set_focus_companion_save_status(m.get_focus_companion_save_status());
    s.set_active_pages(m.get_active_pages());
    s.set_hub_pages(m.get_hub_pages());
    s.set_pages_save_status(m.get_pages_save_status());
    s.set_save_status(m.get_settings_save_status());
}

/// Teruskan tombol Simpan/Tes di jendela Pengaturan ke handler yang sudah ada di NotchWindow.
fn wire_settings_window(m: &NotchWindow, s: &SettingsWindow) {
    {
        let weak = m.as_weak();
        s.on_save_weather(move |a0, a1, a2| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_weather(a0, a1, a2); } });
    }
    {
        let weak = m.as_weak();
        s.on_search_city(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_search_city(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_adhan(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_test_adhan(); } });
    }
    {
        let weak = m.as_weak();
        s.on_pause_adhan(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_pause_adhan(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_stop_adhan(move || { if let Some(m) = weak.upgrade() { m.invoke_adhan_stop(); } });
    }
    {
        let weak = m.as_weak();
        s.on_restart_app(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_restart(); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_appearance(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_appearance(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9); } });
    }
    {
        let weak = m.as_weak();
        s.on_preview_appearance(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9| {
            if let Some(m) = weak.upgrade() {
                m.invoke_settings_preview_appearance(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
            }
        });
    }
    {
        let weak = m.as_weak();
        s.on_save_nudge(move |a0, a1, a2, a3, a4, a5, a6, a7| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_nudge(a0, a1, a2, a3, a4, a5, a6, a7); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_nudge(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_nudge(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_greeting(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_greeting(a0, a1, a2, a3); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_greeting(move |a0, a1, a2| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_greeting(a0, a1, a2); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_conversation(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_conversation(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_conversation(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_conversation(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_focus_companion(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_focus_companion(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_focus_companion(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_focus_companion(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_behavior(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_behavior(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_autostart(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_autostart(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_position(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_position(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_motion(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_motion(a0, a1, a2, a3); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_dock_play(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_test_dock_play(); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_pages(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_pages(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_toggle_page(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_toggle_page(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_routines(move |a0, a1, a2, a3, a4, a5, a6, a7, a8| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_routines(a0, a1, a2, a3, a4, a5, a6, a7, a8); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_shortcuts(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_shortcuts(a0, a1, a2, a3); } });
    }
}

/// Jendela Pengaturan memakai palet gelap sendiri (seperti versi Tauri) agar selalu terbaca, apa pun tema notch.
/// Hanya warna aksen yang diikuti dari pengaturan pengguna. Tiap jendela Slint punya instance global Theme sendiri.
fn sync_settings_theme(m: &NotchWindow, s: &SettingsWindow) {
    let to = Theme::get(s);
    let rgba = |r: u8, g: u8, b: u8, a: u8| slint::Color::from_argb_u8(a, r, g, b);
    let accent = Theme::get(m).get_accent();
    // Aksen terlalu gelap tidak terbaca di latar gelap: pakai aksen bawaan.
    let luma = 0.299 * f32::from(accent.red()) + 0.587 * f32::from(accent.green()) + 0.114 * f32::from(accent.blue());
    let (accent, on_accent) = if luma < 90.0 {
        (rgba(0x78, 0x8c, 0xff, 255), rgba(255, 255, 255, 255))
    } else if luma > 170.0 {
        (accent, rgba(0x10, 0x12, 0x18, 255))
    } else {
        (accent, rgba(255, 255, 255, 255))
    };
    to.set_body(rgba(0x14, 0x16, 0x1d, 255));
    to.set_text(rgba(0xec, 0xee, 0xf5, 255));
    to.set_text_dim(rgba(0xec, 0xee, 0xf5, 0xb8));
    to.set_text_faint(rgba(0xec, 0xee, 0xf5, 0x85));
    to.set_accent(accent);
    to.set_on_accent(on_accent);
    to.set_ok(rgba(0x7b, 0xdc, 0x9a, 255));
    to.set_chip(rgba(255, 255, 255, 0x16));
    to.set_chip_hover(rgba(255, 255, 255, 0x28));
    to.set_track(rgba(255, 255, 255, 0x1f));
}

/// Jendela Pengaturan yang sedang terbuka. Timer menjaga nilainya sama dengan NotchWindow selama jendela hidup;
/// saat jendela ditutup, `SettingsHandle` dilepas sehingga memori dan timer ikut bebas.
struct SettingsHandle {
    win: SettingsWindow,
    _sync: Timer,
}
type SettingsSlot = Rc<RefCell<Option<SettingsHandle>>>;

/// Buka (atau munculkan kembali) jendela Pengaturan terpisah, setara jendela Pengaturan versi Tauri.
fn open_settings_window(main: &NotchWindow, slot: &SettingsSlot) {
    // Ambil Weak dalam scope borrow singkat agar operasi show/focus tidak berjalan saat RefCell dipinjam.
    let existing = {
        slot.borrow().as_ref().map(|handle| handle.win.as_weak())
    };
    if let Some(existing) = existing {
        if let Some(win) = existing.upgrade() {
            sync_settings_theme(main, &win);
            sync_settings_window(main, &win);
            let _ = win.show();
            let _ = win.window().with_winit_window(|w| {
                w.set_minimized(false);
                w.focus_window();
            });
            return;
        }
        slot.borrow_mut().take();
    }
    let win = match SettingsWindow::new() {
        Ok(win) => win,
        Err(error) => {
            eprintln!("[Settings] Gagal membuat jendela: {error}");
            return;
        }
    };
    sync_settings_theme(main, &win);
    sync_settings_window(main, &win);
    wire_settings_window(main, &win);
    {
        let slot = slot.clone();
        win.window().on_close_requested(move || {
            // Lepas jendela setelah callback selesai agar tidak membebaskan dirinya sendiri.
            let slot = slot.clone();
            Timer::single_shot(Duration::from_millis(50), move || {
                slot.borrow_mut().take();
            });
            slint::CloseRequestResponse::HideWindow
        });
    }
    let sync = Timer::default();
    {
        let (main_weak, win_weak) = (main.as_weak(), win.as_weak());
        sync.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            if let (Some(m), Some(s)) = (main_weak.upgrade(), win_weak.upgrade()) {
                sync_settings_theme(&m, &s);
                sync_settings_live_state(&m, &s);
            }
        });
    }
    if let Err(error) = win.show() {
        eprintln!("[Settings] Gagal menampilkan jendela: {error}");
        return;
    }
    *slot.borrow_mut() = Some(SettingsHandle { win, _sync: sync });
}

fn main() -> Result<(), slint::PlatformError> {
    if let Some(resource_dir) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) {
        notch_tools::pdf::set_resource_dir(resource_dir.clone());
        notch_tools::compress::set_resource_dir(resource_dir);
    }
    // Config dibaca saja (tidak pernah ditulis/dibuat dari sini).
    let cfg = Config::load_readonly(&notch_core::paths::config_path());

    let palette = Palette::from_appearance(&cfg.appearance, platform::os_light());
    // Posisi notch: kustom dari config (pusat x, atas y; piksel fisik) atau None = tengah atas.
    // Tombol "Pusatkan" hanya mengembalikannya ke tengah selama sesi ini (config tidak ditulis dari sini).
    let shell = Rc::new(Shell {
        pos: Cell::new(cfg.position.custom.then_some((cfg.position.x, cfg.position.y))),
        monitor_hint: Cell::new((cfg.position.custom
            && (cfg.position.monitor_center_x != 0 || cfg.position.monitor_center_y != 0))
            .then_some((cfg.position.monitor_center_x, cfg.position.monitor_center_y))),
        theme: RefCell::new(cfg.appearance.notch_theme.clone()),
        body: RefCell::new(palette.body),
        scale: RefCell::new(cfg.scale as f32),
        memo: Cell::new((None, 0)),
    });

    let appearance_eye: Rc<RefCell<Option<Rc<RefCell<EyeCtx>>>>> = Rc::new(RefCell::new(None));
    let greeting_voice_runtime = Rc::new(RefCell::new(cfg.greeting.voice.clone()));
    let sleep_after_s = Rc::new(Cell::new(cfg.timing.sleep_after_s));
    let hide_fullscreen = Rc::new(Cell::new(cfg.hide_fullscreen));
    let win = NotchWindow::new()?;
    // Pengaturan dibuka sebagai jendela terpisah (bukan halaman di dalam notch): tab Pengaturan, tray, dan shortcut memakai jalur ini.
    let settings_slot: SettingsSlot = Rc::default();
    let open_settings: Rc<dyn Fn()> = {
        let (weak, slot) = (win.as_weak(), settings_slot.clone());
        let opening = Rc::new(Cell::new(false));
        Rc::new(move || {
            if opening.replace(true) {
                return;
            }
            let (weak, slot, opening) = (weak.clone(), slot.clone(), opening.clone());
            Timer::single_shot(Duration::from_millis(1), move || {
                if let Some(w) = weak.upgrade() {
                    open_settings_window(&w, &slot);
                }
                opening.set(false);
            });
        })
    };
    {
        let open = open_settings.clone();
        win.on_open_settings(move || open());
    }
    win.set_convert_history_text(format_convert_history().into());
    win.set_convert_source_preview(Image::default());
    win.set_convert_result_preview(Image::default());
    let transfer_state = Arc::new(Mutex::new(transfer::TransferState::new(notch_core::paths::data_file("Notch"))));
    let android_state = Arc::new(Mutex::new(android::AndroidState::new()));
    let mirror_process = Arc::new(Mutex::new(android_mirror::MirrorProcess::new()));
    win.set_convert_selected_path("".into());
    win.set_compress_selected_path("".into());
    win.set_compress_status("Pilih gambar JPG/PNG/BMP, PDF, atau dokumen DOCX/XLSX/PPTX untuk dikompres".into());
    let (hotkey_rx, hotkey_config_tx, _hotkey_thread) = hotkeys::start(&cfg.shortcuts);
    privacy::start(win.as_weak());
    apply_theme(&win, &palette);
    win.set_ui_scale(cfg.scale as f32);
    shell.place(&win, COLLAPSED);

    // Sembunyikan notch saat aplikasi foreground benar-benar menutupi seluruh monitor.
    // Timer tetap aktif ketika window disembunyikan agar notch muncul kembali setelah fullscreen berakhir.
    let fullscreen_timer = Timer::default();
    // Dibagikan ke timer Liquid Movement agar perpindahan tidak berjalan saat Notch tersembunyi.
    let fullscreen_hidden = Rc::new(Cell::new(false));
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let enabled = hide_fullscreen.clone();
        let hidden = fullscreen_hidden.clone();
        fullscreen_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
            let Some(w) = weak.upgrade() else { return };
            let should_hide = enabled.get() && platform::foreground_is_fullscreen();
            if should_hide && !hidden.get() {
                if w.hide().is_ok() {
                    hidden.set(true);
                }
            } else if !should_hide && hidden.get() {
                if w.show().is_ok() {
                    platform::hide_from_taskbar(w.window());
                    // Jendela transparan yang baru di-show bisa tampil putih (tanpa alpha) sampai ada gambar ulang:
                    // pasang ulang ukuran, posisi, dan region lalu paksa redraw.
                    let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                    shell.place(&w, size);
                    w.window().request_redraw();
                    hidden.set(false);
                }
            }
        });
    }

    // Drag native: polling pointer global agar seluruh permukaan Notch dapat dipindahkan
    // tanpa WebView/Tauri. Ambang 3px membedakan klik biasa dari drag.
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let config_path = notch_core::paths::config_path();
        let mut was_down = false;
        let mut drag: Option<((i32, i32), (i32, i32), bool, (i32, i32))> = None;
        // Tekan di dalam jendela yang belum "armed": event press Slint bisa tiba beberapa milidetik
        // setelah GetAsyncKeyState melaporkan tombol turun, jadi pemeriksaan diulang selama tombol ditahan.
        let mut candidate: Option<((i32, i32), (i32, i32))> = None;
        // Skala dan ukuran ORB fisik saat ini; diperbarui bila pointer berpindah monitor DPI berbeda.
        let mut drag_scale = 1.0f32;
        let mut drag_orb_size = 0u32;
        let click_suppress_timer = Timer::default();
        let drag_timer = Timer::default();
        drag_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let down = platform::left_mouse_down();
            let cursor = platform::cursor_pos();
            if down && !was_down {
                if let Some((cx, cy)) = cursor {
                    let p = w.window().position();
                    let z = w.window().size();
                    if cx >= p.x && cx < p.x + z.width as i32 && cy >= p.y && cy < p.y + z.height as i32 {
                        if w.get_drag_armed() {
                            drag = Some(((cx, cy), (p.x, p.y), false, (0, 0)));
                            w.set_play_interaction_active(true);
                        } else {
                            candidate = Some(((cx, cy), (p.x, p.y)));
                        }
                    }
                }
            }
            if down && drag.is_none() {
                if let Some((start_cursor, start_window)) = candidate {
                    if w.get_drag_armed() {
                        drag = Some((start_cursor, start_window, false, (0, 0)));
                        candidate = None;
                        w.set_play_interaction_active(true);
                    }
                }
            }
            if down {
                if let (Some((cx, cy)), Some((start_cursor, start_window, moved, mut orb_offset))) = (cursor, drag) {
                    let dx = cx - start_cursor.0;
                    let dy = cy - start_cursor.1;
                    // Ambang drag 3 logical px, dikonversi ke koordinat pointer fisik.
                    let drag_threshold = (3.0 * w.window().scale_factor()).round() as i32;
                    let is_moved = moved || dx.abs() + dy.abs() >= drag_threshold;
                    if is_moved {
                        w.set_play_interaction_active(true);
                        if !moved {
                            // Klik baru boleh dipulihkan setelah tombol mouse dilepas.
                            // Timer di sini terlalu dini: drag panjang (>350 ms) bisa memicu klik saat release.
                            w.set_suppress_next_pill_click(true);
                            let sf = w.window().scale_factor();
                            let orb_size = (64.0 * sf).round() as u32;
                            // Pusat ORB langsung mengikuti titik pointer yang memulai drag.
                            // Offset tengah window lama membuat snap sisi gagal bila drag dimulai dari ujung hub.
                            orb_offset = (
                                cx - start_window.0 - orb_size as i32 / 2,
                                cy - start_window.1 - orb_size as i32 / 2,
                            );
                            w.window().set_size(PhysicalSize::new(orb_size, orb_size));
                            drag_scale = sf;
                            drag_orb_size = orb_size;
                            // Saat drag dimulai dari hub yang terbuka, tutup konten sebelum window menjadi ORB.
                            w.set_expanded(false);
                            w.set_dragging(true);
                        }
                        // Saat melintasi monitor dengan DPI berbeda, jaga ORB tetap 64 logical px.
                        let current_scale = w.window().scale_factor();
                        if (current_scale - drag_scale).abs() > f32::EPSILON {
                            let next_orb_size = (64.0 * current_scale).round() as u32;
                            let center_adjust = (drag_orb_size as i32 - next_orb_size as i32) / 2;
                            orb_offset.0 += center_adjust;
                            orb_offset.1 += center_adjust;
                            w.window().set_size(PhysicalSize::new(next_orb_size, next_orb_size));
                            drag_scale = current_scale;
                            drag_orb_size = next_orb_size;
                        }
                        w.window().set_position(PhysicalPosition::new(start_window.0 + dx + orb_offset.0, start_window.1 + dy + orb_offset.1));
                        drag = Some((start_cursor, start_window, true, orb_offset));
                    }
                }
            } else if was_down {
                candidate = None;
                if let Some((_, _, moved, _)) = drag.take() {
                    if moved {
                        // Jaga proteksi klik selama seluruh drag, lalu beri jeda singkat
                        // agar event release dari sistem window tidak lolos sebagai klik.
                        let clear_weak = weak.clone();
                        click_suppress_timer.start(TimerMode::SingleShot, Duration::from_millis(350), move || {
                            if let Some(w) = clear_weak.upgrade() { w.set_suppress_next_pill_click(false); }
                        });
                        // Jika pembacaan pointer gagal tepat saat release, gunakan pusat ORB
                        // yang terakhir diposisikan. Ini mencegah jendela tertinggal dalam ukuran 64px.
                        let (cx, cy) = cursor.unwrap_or_else(|| {
                            let p = w.window().position();
                            let s = w.window().size();
                            (p.x + s.width as i32 / 2, p.y + s.height as i32 / 2)
                        });
                        {
                            let work = platform::work_area(Some((cx, cy)));
                            let size = w.window().size();
                            let sf = w.window().scale_factor();
                            let previous_dock = w.get_dock();
                            let was_left = previous_dock == 3;
                            let was_right = previous_dock == 4;
                            let was_bottom = previous_dock == 2;
                            let was_top = previous_dock == 0;
                            // Samakan prioritas dan hysteresis Tauri: bawah lebih dulu,
                            // lalu atas, baru sisi. Semua ambang dikonversi ke piksel fisik.
                            let near_bottom = work.y + work.h - cy
                                <= ((if was_bottom { 90.0 } else { 48.0 }) * sf).round() as i32;
                            let near_top = cy - work.y
                                <= ((if was_top { 60.0 } else { 24.0 }) * sf).round() as i32;
                            let near_left = cx - work.x
                                <= ((if was_left { 110.0 } else { 70.0 }) * sf).round() as i32;
                            let near_right = work.x + work.w - cx
                                <= ((if was_right { 110.0 } else { 70.0 }) * sf).round() as i32;
                            let (pos, mode) = if near_bottom {
                                ((cx, layout::BOTTOM_TY), "bottom")
                            } else if near_top {
                                ((cx, work.y), "top")
                            } else if near_left {
                                ((layout::LEFT_TX, cy), "left")
                            } else if near_right {
                                ((layout::RIGHT_TX, cy), "right")
                            } else {
                                ((cx, cy - (size.height as i32 / 2)), "floating")
                            };
                            shell.pos.set(Some(pos));
                            let monitor_center = (work.x + work.w / 2, work.y + work.h / 2);
                            // Simpan anchor monitor untuk semua posisi, termasuk bottom sentinel.
                            shell.monitor_hint.set(Some(monitor_center));
                            shell.memo.set((None, 0));
                            shell.place(&w, COLLAPSED);
                            let mut next = Config::load_readonly(&config_path);
                            next.position.custom = true;
                            next.position.x = pos.0;
                            next.position.y = pos.1;
                            next.position.monitor_center_x = monitor_center.0;
                            next.position.monitor_center_y = monitor_center.1;
                            if next.save(&config_path).is_ok() {

                            }
                            w.set_position_mode(mode.into());
                            w.set_dragging(false);
                            w.set_play_interaction_active(false);
                        }
                    } else {
                        w.set_play_interaction_active(false);
                    }
                }
            }
            was_down = down;
        });
    }

    // Liquid movement: timer selalu hidup agar toggle dan interval dapat diterapkan tanpa restart.
    // Nilai runtime dibagikan dengan handler pengaturan; tidak perlu membuat timer baru.
    let motion_runtime = Rc::new((
        Cell::new(cfg.motion.enabled),
        Cell::new(cfg.motion.interval_secs.clamp(60, 3_600)),
        Cell::new(cfg.motion.max_moves.clamp(1, 10)),
    ));
    let liquid_move_timer = Timer::default();
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let runtime = motion_runtime.clone();
        let hidden_by_fullscreen = fullscreen_hidden.clone();
        let started_at = Instant::now();
        let mut last_move = Instant::now();
        let mut moves = 0u32;
        let mut was_blocked = false;
        let mut animation: Option<(PhysicalPosition, PhysicalPosition, Instant)> = None;
        const MOVE_DURATION: Duration = Duration::from_millis(2_800);

        liquid_move_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };

            if !runtime.0.get() {
                if animation.take().is_some() {
                    let position = w.window().position();
                    let size = w.window().size();
                    shell.pos.set(Some((position.x + size.width as i32 / 2, position.y)));
                }
                // Batas perpindahan dihitung ulang setiap kali fitur diaktifkan kembali.
                moves = 0;
                last_move = Instant::now();
                return;
            }
            let interval = runtime.1.get().clamp(60, 3_600);
            let max_moves = runtime.2.get().clamp(1, 10);

            let blocked = hidden_by_fullscreen.get()
                || w.get_expanded()
                || w.get_play_interaction_active()
                || w.get_nudge_visible()
                || w.get_reminder_alert_visible()
                || !w.get_meeting_finished().is_empty()
                || w.get_meeting_active()
                || w.get_adhan_active();
            if blocked {
                // Jangan biarkan gerakan otomatis mengambil alih pointer atau menggeser
                // notch ketika kartu/pengingat/rapat/adzan sedang tampil.
                if !was_blocked {
                    last_move = Instant::now();
                    was_blocked = true;
                }
                if animation.take().is_some() {
                    let position = w.window().position();
                    let size = w.window().size();
                    shell.pos.set(Some((position.x + size.width as i32 / 2, position.y)));
                }
                return;
            }
            if was_blocked {
                // Mulai ulang hitungan interval setelah interaksi berakhir; jangan langsung
                // memindahkan notch pada frame pertama setelah pointer keluar.
                last_move = Instant::now();
                was_blocked = false;
            }

            if let Some((from, to, began)) = animation {
                let progress = (began.elapsed().as_secs_f32() / MOVE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
                // Smoothstep (ease-in-out): akselerasi dan perlambatan lembut di kedua ujung.
                let eased = progress * progress * (3.0 - 2.0 * progress);
                let x = from.x as f32 + (to.x - from.x) as f32 * eased;
                let y = from.y as f32 + (to.y - from.y) as f32 * eased;
                w.window().set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));

                if progress >= 1.0 {
                    w.window().set_position(to);
                    shell.place(&w, COLLAPSED);
                    animation = None;
                    last_move = Instant::now();
                    moves = moves.saturating_add(1);
                }
                return;
            }

            if moves >= max_moves || last_move.elapsed() < Duration::from_secs(u64::from(interval)) {
                return;
            }

            let current = w.window().position();
            let size = w.window().size();
            // Gunakan pusat jendela agar monitor terdeteksi benar pada konfigurasi multi-monitor,
            // termasuk monitor yang berada di sebelah kiri/atas dengan koordinat negatif.
            let monitor_center = (
                current.x + size.width as i32 / 2,
                current.y + size.height as i32 / 2,
            );
            let work = platform::work_area(Some(monitor_center));
            // Margin dalam logical pixels agar konsisten pada monitor dengan DPI berbeda.
            let margin = (36.0 * w.window().scale_factor()).round() as i32;
            // Posisi kustom Shell memakai titik tengah horizontal dan sisi atas vertikal.
            let half_width = size.width as i32 / 2;
            let min_x = work.x + half_width + margin;
            let max_x = (work.x + work.w - half_width - margin).max(min_x);
            let min_y = work.y + margin;
            let max_y = (work.y + work.h - size.height as i32 - margin).max(min_y);

            let seed = started_at.elapsed().as_nanos() as u64
                ^ (moves as u64).wrapping_mul(0x9E3779B97F4A7C15);
            let mut value = seed ^ (seed >> 30);
            value = value.wrapping_mul(0xBF58476D1CE4E5B9);
            value ^= value >> 27;
            let rx = (value % (max_x - min_x + 1).max(1) as u64) as i32;
            value = value.wrapping_mul(0x94D049BB133111EB);
            let ry = (value % (max_y - min_y + 1).max(1) as u64) as i32;
            let target_pos = (min_x + rx, min_y + ry);
            let target = layout::place(&work, size.width as i32, size.height as i32, Some(target_pos)).0;
            let from = PhysicalPosition::new(current.x, current.y);
            let to = PhysicalPosition::new(target.0, target.1);

            shell.pos.set(Some(target_pos));
            if from == to {
                shell.place(&w, COLLAPSED);
                last_move = Instant::now();
                moves = moves.saturating_add(1);
            } else {
                animation = Some((from, to, Instant::now()));
            }
        });
    }

    // Tab mengikuti daftar dan urutan `pages` di config. Halaman yang belum ada di versi Slint
    // tampil sebagai placeholder, jadi urutan dan jumlah tab sama dengan versi Tauri.
    // Lirik bukan tab hub: ada di halaman Media (tombol "Lirik"), seperti di Tauri.
    let pages: Vec<String> = cfg.pages.iter().filter(|p| p.as_str() != "lyrics").cloned().collect();
    let pages = if pages.is_empty() { vec!["media".to_string()] } else { pages };
    win.set_tabs(ModelRc::from(Rc::new(VecModel::from(
        pages.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
    ))));
    win.set_page_id(pages[0].as_str().into());
    win.set_shortcut_hub(cfg.shortcuts.toggle_hub.clone().into());
    win.set_shortcut_settings(cfg.shortcuts.open_settings.clone().into());
    win.set_shortcut_pomodoro(cfg.shortcuts.toggle_pomodoro.clone().into());
    win.set_shortcut_prayer(cfg.shortcuts.toggle_prayer.clone().into());
    win.set_appearance_theme(cfg.appearance.theme.clone().into());
    win.set_appearance_eye_theme(cfg.appearance.eye_theme.clone().into());
    win.set_appearance_notch_theme(cfg.appearance.notch_theme.clone().into());
    win.set_appearance_mood(cfg.appearance.default_mood.clone().into());
    win.set_appearance_accent(cfg.appearance.accent.clone().into());
    win.set_appearance_custom_bg(cfg.appearance.background.clone().into());
    win.set_appearance_custom_text(cfg.appearance.text.clone().into());
    win.set_appearance_dark_bg(cfg.appearance.dark_bg.clone().into());
    win.set_appearance_light_bg(cfg.appearance.light_bg.clone().into());
    win.set_prayer_province(cfg.prayer.provinsi.clone().into());
    win.set_prayer_city(cfg.prayer.kabkota.clone().into());
    win.set_prayer_mode(cfg.prayer.mode.clone().into());
    win.set_prayer_remind_before(cfg.prayer.remind_before.to_string().into());
    win.set_prayer_remind_min(cfg.prayer.remind_min.to_string().into());
    win.set_pomodoro_focus(cfg.pomodoro.focus_min.to_string().into());
    win.set_pomodoro_break(cfg.pomodoro.break_min.to_string().into());
    win.set_pomodoro_long_break(cfg.pomodoro.long_break_min.to_string().into());
    win.set_pomodoro_rounds(cfg.pomodoro.rounds.to_string().into());
    win.set_behavior_scale(format!("{:.2}", cfg.scale).into());
    win.set_behavior_hover_open(cfg.timing.hover_open_ms.to_string().into());
    win.set_behavior_hover_close(cfg.timing.hover_close_ms.to_string().into());
    win.set_behavior_sleep_after(cfg.timing.sleep_after_s.to_string().into());
    win.set_behavior_hide_fullscreen(cfg.hide_fullscreen);
    win.set_autostart_enabled(autostart::is_enabled());
    win.set_motion_enabled(cfg.motion.enabled);
    win.set_dock_play_enabled(cfg.motion.dock_play_enabled);
    win.set_motion_interval(cfg.motion.interval_secs.to_string().into());
    win.set_motion_max_moves(cfg.motion.max_moves.to_string().into());
    win.set_nudge_enabled(cfg.nudge.enabled);
    win.set_nudge_idle_min(cfg.nudge.idle_min.to_string().into());
    win.set_nudge_message(cfg.nudge.message.clone().into());
    win.set_nudge_emoji(cfg.nudge.emoji.clone().into());
    win.set_nudge_mood(cfg.nudge.mood.clone().into());
    win.set_nudge_show_ms((cfg.nudge.show_ms / 1_000).to_string().into());
    win.set_nudge_only_silent(cfg.nudge.only_silent);
    win.set_nudge_until_move(cfg.nudge.until_move);
    win.set_greeting_enabled(cfg.greeting.enabled);
    win.set_greeting_mode(cfg.greeting.mode.clone().into());
    win.set_greeting_voice(cfg.greeting.voice.clone().into());
    win.set_greeting_custom_text(cfg.greeting.custom_text.clone().into());
    win.set_conversation_enabled(cfg.conversation_moments.enabled);
    win.set_conversation_on_play(cfg.conversation_moments.on_play);
    win.set_conversation_on_track_change(cfg.conversation_moments.on_track_change);
    win.set_conversation_on_pause(cfg.conversation_moments.on_pause);
    win.set_conversation_on_return(cfg.conversation_moments.on_return);
    win.set_conversation_on_weather_change(cfg.conversation_moments.on_weather_change);
    win.set_conversation_on_rain_soon(cfg.conversation_moments.on_rain_soon);
    win.set_conversation_on_low_battery(cfg.conversation_moments.on_low_battery);
    win.set_conversation_on_charging(cfg.conversation_moments.on_charging);
    win.set_conversation_return_after_min(cfg.conversation_moments.return_after_min.to_string().into());
    win.set_conversation_cooldown_min(cfg.conversation_moments.cooldown_min.to_string().into());
    win.set_focus_companion_enabled(cfg.focus_companion.enabled);
    win.set_focus_companion_on_start(cfg.focus_companion.on_start);
    win.set_focus_companion_on_halfway(cfg.focus_companion.on_halfway);
    win.set_focus_companion_on_focus_complete(cfg.focus_companion.on_focus_complete);
    win.set_focus_companion_on_break_end(cfg.focus_companion.on_break_end);
    win.set_hub_pages(cfg.pages.join(", ").into());
    let initial_position_mode = if !cfg.position.custom {
        "top"
    } else if cfg.position.x == notch_core::layout::LEFT_TX {
        "left"
    } else if cfg.position.x == notch_core::layout::RIGHT_TX {
        "right"
    } else if cfg.position.y >= notch_core::layout::BOTTOM_TY / 2 {
        "bottom"
    } else {
        "floating"
    };
    win.set_position_mode(initial_position_mode.into());
    win.set_active_pages(ModelRc::from(Rc::new(VecModel::from(cfg.pages.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_pages(move |value| {
            let allowed = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "android", "transfer", "convert", "compress", "settings", "currency"];
            let mut pages = Vec::<String>::new();
            for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !allowed.contains(&item) {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("ID halaman tidak dikenal: {item}").into());
                    }
                    return;
                }
                if !pages.iter().any(|p| p == item) {
                    pages.push(item.to_string());
                }
            }
            if pages.is_empty() || !pages.iter().any(|p| p == "media") {
                if let Some(w) = weak.upgrade() {
                    w.set_pages_save_status("Minimal sertakan halaman media.".into());
                }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.pages = pages.clone();
            next.pages_version = notch_core::config::PAGES_VERSION;
            match next.save(&config_path) {
                Ok(()) => {

                    let tabs: Vec<String> = pages.iter().filter(|p| p.as_str() != "lyrics").cloned().collect();
                    let tabs = if tabs.is_empty() { vec!["media".to_string()] } else { tabs };
                    if let Some(w) = weak.upgrade() {
                        let current = w.get_page_id().to_string();
                        w.set_tabs(ModelRc::from(Rc::new(VecModel::from(
                            tabs.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
                        ))));
                        if !tabs.iter().any(|p| p == &current) {
                            w.set_page_id(tabs[0].as_str().into());
                        }
                        w.set_hub_pages(pages.join(", ").into());
                        w.set_active_pages(ModelRc::from(Rc::new(VecModel::from(pages.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        w.set_pages_save_status("Halaman hub tersimpan dan langsung diperbarui.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    // Animasi perpindahan jendela native saat pengguna mengganti docking.
    // Timer berjalan hanya selama transisi; state baru menggantikan animasi sebelumnya.
    let dock_move_state = Rc::new(RefCell::new(None::<(i32, i32, i32, i32, Instant)>));
    let dock_move_timer = Rc::new(Timer::default());

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let shell = shell.clone();
        let move_state = dock_move_state.clone();
        let move_timer = dock_move_timer.clone();
        win.on_settings_save_position(move |mode| {
            let mode = mode.to_string();
            if !["top", "floating", "bottom", "left", "right"].contains(&mode.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_behavior_save_status("Mode posisi tidak valid".into()); }
                return;
            }
            // Terapkan preset pada monitor tempat notch sedang berada, bukan selalu monitor utama.
            let current_monitor = weak.upgrade().and_then(|w| {
                let position = w.window().position();
                let size = w.window().size();
                (size.width > 0 && size.height > 0).then_some((
                    position.x + size.width as i32 / 2,
                    position.y + size.height as i32 / 2,
                ))
            });
            let work = platform::work_area(current_monitor);
            let center_x = work.x + work.w / 2;
            let center_y = work.y + work.h / 2;
            let next_pos = match mode.as_str() {
                "top" => None,
                "floating" => Some((center_x, work.y + 100)),
                "bottom" => Some((center_x, notch_core::layout::BOTTOM_TY)),
                "left" => Some((notch_core::layout::LEFT_TX, center_y)),
                "right" => Some((notch_core::layout::RIGHT_TX, center_y)),
                _ => None,
            };
            let mut next = Config::load_readonly(&config_path);
            match next_pos {
                Some((x, y)) => {
                    next.position.custom = true;
                    next.position.x = x;
                    next.position.y = y;
                    // Anchor monitor disimpan untuk semua preset agar bottom sentinel tidak
                    // dipakai sebagai koordinat monitor saat aplikasi dibuka ulang.
                    next.position.monitor_center_x = center_x;
                    next.position.monitor_center_y = center_y;
                }
                None => {
                    next.position.custom = false;
                    next.position.x = 0;
                    next.position.y = 0;
                    next.position.monitor_center_x = 0;
                    next.position.monitor_center_y = 0;
                }
            }
            match next.save(&config_path) {
                Ok(()) => {

                    shell.pos.set(next_pos);
                    shell.monitor_hint.set(next_pos.map(|_| (center_x, center_y)));
                    if let Some(w) = weak.upgrade() {
                        let from = w.window().position();
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        let target = w.window().position();
                        if (from.x, from.y) != (target.x, target.y) {
                            w.window().set_position(from);
                            move_state.replace(Some((from.x, from.y, target.x, target.y, Instant::now())));
                            move_timer.start(TimerMode::Repeated, Duration::from_millis(16), {
                                let state = move_state.clone();
                                let weak = weak.clone();
                                let timer_weak = Rc::downgrade(&move_timer);
                                move || {
                                    let Some((sx, sy, tx, ty, started_at)) = *state.borrow() else {
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                        return;
                                    };
                                    let Some(w) = weak.upgrade() else {
                                        state.replace(None);
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                        return;
                                    };
                                    let t = (started_at.elapsed().as_secs_f32() / 0.360).clamp(0.0, 1.0);
                                    let eased = 1.0 - (1.0 - t).powi(3);
                                    let x = sx as f32 + (tx - sx) as f32 * eased;
                                    let y = sy as f32 + (ty - sy) as f32 * eased;
                                    w.window().set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
                                    if t >= 1.0 {
                                        state.replace(None);
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                    }
                                }
                            });
                        }
                        w.set_position_mode(mode.clone().into());
                        w.set_behavior_save_status(format!("Posisi notch disimpan: {mode}").into());
                    }
                }
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_behavior_save_status(format!("Gagal menyimpan posisi: {error}").into()); } }
            }
        });
    }
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        win.on_settings_toggle_page(move |id, enabled| {
            let allowed = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "android", "transfer", "convert", "compress", "settings", "currency"];
            let id = id.to_string();
            if !allowed.contains(&id.as_str()) { return; }
            let mut next = Config::load_readonly(&config_path);
            if enabled { if !next.pages.iter().any(|page| page == &id) { next.pages.push(id.clone()); } }
            else {
                if id == "media" { if let Some(w) = weak.upgrade() { w.set_pages_save_status("Halaman Musik wajib tetap aktif.".into()); } return; }
                next.pages.retain(|page| page != &id);
            }
            next.pages_version = notch_core::config::PAGES_VERSION;
            match next.save(&config_path) {
                Ok(()) => {

                    let tabs = next.pages.iter().filter(|page| page.as_str() != "lyrics").cloned().collect::<Vec<_>>();
                    let tabs = if tabs.is_empty() { vec!["media".to_string()] } else { tabs };
                    if let Some(w) = weak.upgrade() {
                        let current = w.get_page_id().to_string();
                        w.set_tabs(ModelRc::from(Rc::new(VecModel::from(tabs.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        if !tabs.iter().any(|page| page == &current) { w.set_page_id(tabs[0].as_str().into()); }
                        w.set_hub_pages(next.pages.join(", ").into());
                        w.set_active_pages(ModelRc::from(Rc::new(VecModel::from(next.pages.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        w.set_pages_save_status(format!("{} {}", if enabled { "Diaktifkan:" } else { "Dinonaktifkan:" }, id).into());
                    }
                }
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_pages_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let hotkey_config_tx = hotkey_config_tx.clone();
        win.on_settings_save_shortcuts(move |hub, settings, pomo, prayer| {
            let mut next = Config::load_readonly(&config_path);
            next.shortcuts.toggle_hub = hub.to_string();
            next.shortcuts.open_settings = settings.to_string();
            next.shortcuts.toggle_pomodoro = pomo.to_string();
            next.shortcuts.toggle_prayer = prayer.to_string();
            next.normalize();
            if let Err(error) = hotkeys::validate_shortcuts(&next.shortcuts) {
                if let Some(w) = weak.upgrade() {
                    w.set_settings_save_status(error.into());
                }
                return;
            }
            match next.save(&config_path) {
                Ok(()) => {

                    let _ = hotkey_config_tx.send(next.shortcuts.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_shortcut_hub(next.shortcuts.toggle_hub.clone().into());
                        w.set_shortcut_settings(next.shortcuts.open_settings.clone().into());
                        w.set_shortcut_pomodoro(next.shortcuts.toggle_pomodoro.clone().into());
                        w.set_shortcut_prayer(next.shortcuts.toggle_prayer.clone().into());
                        w.set_settings_save_status("Tersimpan. Shortcut baru langsung aktif.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_settings_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        win.on_settings_save_autostart(move |enabled| {
            match autostart::set(enabled) {
                Ok(actual) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_autostart_enabled(actual);
                        w.set_autostart_save_status(if actual {
                            "Notch akan berjalan otomatis saat login.".into()
                        } else {
                            "Jalan otomatis dinonaktifkan.".into()
                        });
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_autostart_enabled(autostart::is_enabled());
                        w.set_autostart_save_status(format!("Gagal mengubah jalan otomatis: {error}").into());
                    }
                }
            }
        });
    }

    win.set_emoji_coffee(load_png(include_bytes!("../assets/emoji/2615.png")));
    win.set_emoji_tomato(load_png(include_bytes!("../assets/emoji/1f345.png")));
    win.set_emoji_moon(load_png(include_bytes!("../assets/emoji/1f319.png")));

    // --- State halaman ---
    let app_started_at = Instant::now();
    let stats_timer = Rc::new(Timer::default());
    let battery_timer = Rc::new(Timer::default());
    let clipboard_timer = Rc::new(Timer::default());
    let reminder_timer = Timer::default();
    media::start(win.as_weak());
    let prayer_timer = Timer::default();
    let meeting_timer = Timer::default();
    // Indikator gelombang audio Adzan hanya bergerak selama playback aktif.
    let adhan_anim_timer = Timer::default();
    {
        let weak = win.as_weak();
        adhan_anim_timer.start(TimerMode::Repeated, Duration::from_millis(130), move || {
            if let Some(w) = weak.upgrade() {
                if w.get_adhan_active() {
                    w.set_adhan_phase((w.get_adhan_phase() + 1) % 3);
                }
            }
        });
    }
    // Durasi rapat dihitung dari transisi privasi aktif -> nonaktif.
    {
        let weak = win.as_weak();
        let started = Rc::new(Cell::new(None::<Instant>));
        let finished_at = Rc::new(Cell::new(None::<Instant>));
        meeting_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_meeting_active() {
                if started.get().is_none() {
                    started.set(Some(Instant::now()));
                    w.set_meeting_finished("".into());
                    finished_at.set(None);
                }
                if let Some(at) = started.get() {
                    let secs = at.elapsed().as_secs();
                    let label = if secs >= 3600 {
                        format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                    } else {
                        format!("{:02}:{:02}", secs / 60, secs % 60)
                    };
                    w.set_meeting_elapsed(label.into());
                }
            } else if let Some(at) = started.take() {
                let secs = at.elapsed().as_secs();
                let mins = secs / 60;
                let rem = secs % 60;
                let label = if mins == 0 { format!("{secs} detik") } else if rem == 0 { format!("{mins} menit") } else { format!("{mins} menit {rem} detik") };
                let finished_label = format!("Rapat selesai, {label}");
                w.set_prayer_status(finished_label.clone().into());
                w.set_meeting_finished(finished_label.into());
                w.set_meeting_elapsed("00:00".into());
                finished_at.set(Some(Instant::now()));
            } else if let Some(at) = finished_at.get() {
                if at.elapsed() >= Duration::from_secs(6) {
                    w.set_meeting_finished("".into());
                    finished_at.set(None);
                }
            }
        });
    }
    let weather_timer = Rc::new(Timer::default());
    let monitor: Rc<RefCell<Option<StatsMonitor>>> = Rc::new(RefCell::new(None));
    let battery = Rc::new(BatteryMonitor::new());
    let notes_path = notch_core::paths::data_file("notes.json");
    let notes_store: Rc<RefCell<Option<NotesStore>>> = Rc::new(RefCell::new(None));
    let notes_model = Rc::new(VecModel::<NoteItem>::default());
    win.set_notes(ModelRc::from(notes_model.clone()));

    // Wajah idle terakhir (mata / jam) diingat antar sesi.
    if std::fs::read_to_string(notch_core::paths::data_file("idle_face.txt")).map(|t| t.trim() == "clock").unwrap_or(false) {
        win.set_idle_clock(true);
    }

    // ----- Currency -----
    let currency_path = notch_core::paths::data_file("currency_alerts.json");
    let currency = Rc::new(RefCell::new(CurrencyState::new(notch_core::currency::load_alerts(&currency_path))));
    let currency_fetch = Arc::new(Mutex::new(CurrencyFetch::default()));
    let currency_busy = Arc::new(AtomicBool::new(false));
    let stock_gainers = Rc::new(VecModel::<StockRow>::default());
    let stock_losers = Rc::new(VecModel::<StockRow>::default());
    let currency_alert_model = Rc::new(VecModel::<CurrencyAlertItem>::default());
    win.set_stock_gainers(ModelRc::from(stock_gainers.clone()));
    win.set_stock_losers(ModelRc::from(stock_losers.clone()));
    win.set_currency_alerts(ModelRc::from(currency_alert_model.clone()));
    paint_currency(&win, &currency.borrow(), &stock_gainers, &stock_losers, &currency_alert_model);
    // Kurs dan saham memiliki cache 10 menit masing-masing; kegagalan satu sumber
    // tidak memaksa data dari sumber lain untuk diunduh ulang.
    let currency_request: Rc<dyn Fn(bool, bool)> = {
        let (weak, state, fetch, busy) = (win.as_weak(), currency.clone(), currency_fetch.clone(), currency_busy.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        Rc::new(move |force: bool, include_stocks: bool| {
            let (load_rates, load_stocks) = {
                let s = state.borrow();
                let fresh = |loaded: Option<Instant>| loaded.map(|t| t.elapsed() < Duration::from_secs(600)).unwrap_or(false);
                (!fresh(s.rates_loaded_at) || force, include_stocks && (!fresh(s.stocks_loaded_at) || force))
            };
            if !load_rates && !load_stocks {
                return;
            }
            if busy.swap(true, Ordering::SeqCst) {
                return;
            }
            state.borrow_mut().loading = true;
            if let Some(w) = weak.upgrade() {
                paint_currency(&w, &state.borrow(), &gm, &lm, &am);
            }
            let (weak, fetch, busy) = (weak.clone(), fetch.clone(), busy.clone());
            std::thread::spawn(move || {
                let rates = if load_rates { Some(notch_core::currency::fetch_rates()) } else { None };
                let stock_result = if load_stocks { Some(notch_core::currency::fetch_stocks()) } else { None };
                if let Ok(mut guard) = fetch.lock() {
                    guard.rates = rates;
                    guard.stocks = stock_result;
                }
                busy.store(false, Ordering::SeqCst);
                let _ = weak.upgrade_in_event_loop(|w| w.invoke_currency_data_ready());
            });
        })
    };
    {
        let (weak, state, fetch) = (win.as_weak(), currency.clone(), currency_fetch.clone());
        let (gm, lm, am, path) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone(), currency_path.clone());
        win.on_currency_data_ready(move || {
            let Some(w) = weak.upgrade() else { return };
            let (rates, stocks) = match fetch.lock() {
                Ok(mut g) => (g.rates.take(), g.stocks.take()),
                Err(_) => (None, None),
            };
            let mut s = state.borrow_mut();
            s.loading = false;
            match rates {
                Some(Ok(r)) => {
                    let (h, mi, _, _, _) = platform::local_time();
                    s.rates = Some(r);
                    s.rates_error.clear();
                    s.rates_warning.clear();
                    s.rates_loaded_at = Some(Instant::now());
                    s.updated = format!("Diperbarui {h:02}:{mi:02}");
                }
                Some(Err(e)) => {
                    if s.rates.is_none() {
                        s.rates_error = format!("Kurs belum tersedia: {e}");
                    } else {
                        s.rates_warning = format!("Pembaruan gagal · memakai kurs terakhir ({e})");
                    }
                }
                None => {}
            }
            match stocks {
                Some(Ok(v)) => {
                    s.stocks = v;
                    s.stock_error.clear();
                    s.stocks_loaded_at = Some(Instant::now());
                }
                Some(Err(e)) => {
                    s.stocks_loaded_at = Some(Instant::now());
                    if s.stocks.is_empty() {
                        s.stock_error = e;
                    }
                }
                None => {}
            }
            fire_due_currency_alert(&w, &mut s, &path);
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let req = currency_request.clone();
        win.on_currency_refresh(move || req(true, true));
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_amount_edited(move |text| {
            let Some(w) = weak.upgrade() else { return };
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            let digits = digits.trim_start_matches('0');
            let mut s = state.borrow_mut();
            s.amount = digits.chars().take(15).collect();
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_quick(move |i| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            s.amount = CURRENCY_QUICK.get(i.max(0) as usize).copied().unwrap_or(100_000).to_string();
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_step_target(move |n| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let len = notch_core::currency::CURRENCIES.len() as i32;
            s.target = ((s.target as i32 + n).rem_euclid(len)) as usize;
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_select_target(move |code, days| {
            let Some(w) = weak.upgrade() else { return };
            let selected_code = code.to_string();
            {
                let mut s = state.borrow_mut();
                if let Some(index) = notch_core::currency::CURRENCIES.iter().position(|(candidate, _)| *candidate == selected_code.as_str()) {
                    s.target = index;
                    paint_currency(&w, &s, &gm, &lm, &am);
                }
            }
            if w.get_currency_expanded() {
                w.invoke_currency_chart_request(code, days);
            }
        });
    }
    let currency_chart_cache = Arc::new(Mutex::new(
        std::collections::HashMap::<String, (Instant, Vec<(i64, f64)>, String)>::new()
    ));
    {
        // Cache riwayat 10 menit per mata uang/periode; token mencegah respons lama menimpa pilihan terbaru.
        let token = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cache = currency_chart_cache.clone();
        let weak = win.as_weak();
        win.on_currency_chart_request(move |code, days| {
            let Some(w) = weak.upgrade() else { return };
            w.set_fx_chart_line("".into());
            w.set_fx_chart_area("".into());
            w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::<FxChartPoint>::default())));
            w.set_fx_chart_last("".into());
            w.set_fx_chart_change("".into());
            w.set_fx_chart_max("".into());
            w.set_fx_chart_y1("".into());
            w.set_fx_chart_y2("".into());
            w.set_fx_chart_min("".into());
            w.set_fx_chart_date_start("".into());
            w.set_fx_chart_date_mid("".into());
            w.set_fx_chart_date_end("".into());
            w.set_fx_chart_dot_y(50.0);
            w.set_fx_chart_status("Memuat grafik…".into());
            let mine = token.fetch_add(1, Ordering::SeqCst) + 1;
            let (token, weak, cache, code, days) = (
                token.clone(), w.as_weak(), cache.clone(), code.to_string(), i64::from(days.clamp(7, 730))
            );
            std::thread::spawn(move || {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64).unwrap_or(0);
                let cache_key = format!("{code}:{days}");
                let cached = cache.lock().ok().and_then(|entries| {
                    entries.get(&cache_key)
                        .filter(|(loaded_at, _, _)| loaded_at.elapsed() < Duration::from_secs(600))
                        .cloned()
                });
                let history = match cached {
                    Some((_, points, source)) => Ok((points, source)),
                    None => notch_core::currency::fetch_fx_history_with_source(
                        &code, now - days * 86_400, now + 86_400
                    ).map(|(points, source)| {
                        let source = source.to_string();
                        if let Ok(mut entries) = cache.lock() {
                            entries.insert(cache_key, (Instant::now(), points.clone(), source.clone()));
                        }
                        (points, source)
                    }),
                };
                let result = history.and_then(|(pts, source)| {
                    if pts.len() < 2 { return Err("Riwayat kosong".to_string()); }
                    let count = pts.len();
                    let first_time = pts[0].0;
                    let middle_time = pts[pts.len() / 2].0;
                    let last_time = pts[pts.len() - 1].0;
                    let chart = notch_core::currency::build_fx_chart(&pts)
                        .ok_or_else(|| "Riwayat kosong".to_string())?;
                    let span = chart.max - chart.min;
                    let pad = if span.abs() <= f64::EPSILON { chart.max.abs() * 0.01 } else { span * 0.14 };
                    let axis_min = chart.min - pad;
                    let axis_max = chart.max + pad;
                    let axis_span = (axis_max - axis_min).max(f64::EPSILON);
                    let axis = [
                        fx_axis_label(axis_max, axis_span),
                        fx_axis_label(axis_min + axis_span * 2.0 / 3.0, axis_span),
                        fx_axis_label(axis_min + axis_span / 3.0, axis_span),
                        fx_axis_label(axis_min, axis_span),
                    ];
                    let dot_y = if span.abs() <= f64::EPSILON {
                        50.0
                    } else {
                        94.0 - (chart.last - axis_min) / axis_span * 88.0
                    };
                    let dates = [
                        notch_core::currency::short_fx_date_from_unix(first_time),
                        notch_core::currency::short_fx_date_from_unix(middle_time),
                        notch_core::currency::short_fx_date_from_unix(last_time),
                    ];
                    let t0 = first_time as f64;
                    let t1 = last_time as f64;
                    let time_span = (t1 - t0).max(1.0);
                    let chart_points = pts.iter().enumerate().map(|(index, (timestamp, value))| {
                        let x = (*timestamp as f64 - t0) / time_span;
                        let left = if index == 0 {
                            0.0
                        } else {
                            ((pts[index - 1].0 as f64 + *timestamp as f64) * 0.5 - t0) / time_span
                        };
                        let right = if index + 1 == pts.len() {
                            1.0
                        } else {
                            ((*timestamp as f64 + pts[index + 1].0 as f64) * 0.5 - t0) / time_span
                        };
                        let y = if span.abs() <= f64::EPSILON {
                            50.0
                        } else {
                            94.0 - (*value - axis_min) / axis_span * 88.0
                        };
                        FxChartPoint {
                            x: x as f32,
                            y: y as f32,
                            left: left as f32,
                            right: right as f32,
                            date: notch_core::currency::full_fx_date_from_unix(*timestamp).into(),
                            value: fx_idr(*value).into(),
                        }
                    }).collect::<Vec<_>>();
                    Ok((chart, count, axis, dates, dot_y, chart_points, source))
                });
                if token.load(Ordering::SeqCst) != mine { return; }
                let _ = weak.upgrade_in_event_loop(move |w| match result {
                    Ok((chart, count, axis, dates, dot_y, chart_points, source)) => {
                        let pct = if chart.first > 0.0 {
                            (chart.last - chart.first) / chart.first * 100.0
                        } else { 0.0 };
                        let period = match days {
                            30 => "30 hari", 90 => "3 bulan", 180 => "6 bulan", 365 => "1 tahun",
                            _ => "periode dipilih",
                        };
                        w.set_fx_chart_line(chart.line.into());
                        w.set_fx_chart_area(chart.area.into());
                        w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::from(chart_points))));
                        w.set_fx_chart_status(format!("Kurs penutupan harian · {count} titik data · sumber {source}").into());
                        w.set_fx_chart_max(axis[0].clone().into());
                        w.set_fx_chart_y1(axis[1].clone().into());
                        w.set_fx_chart_y2(axis[2].clone().into());
                        w.set_fx_chart_min(axis[3].clone().into());
                        w.set_fx_chart_date_start(dates[0].clone().into());
                        w.set_fx_chart_date_mid(dates[1].clone().into());
                        w.set_fx_chart_date_end(dates[2].clone().into());
                        w.set_fx_chart_dot_y(dot_y as f32);
                        w.set_fx_chart_last(fx_idr(chart.last).into());
                        w.set_fx_chart_change(format!(
                            "{}{:.2}% · {period}",
                            if pct >= 0.0 { "+" } else { "" }, pct
                        ).replace('.', ",").into());
                        w.set_fx_chart_up(pct >= 0.0);
                    }
                    Err(error) => {
                        w.set_fx_chart_line("".into());
                        w.set_fx_chart_area("".into());
                        w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::<FxChartPoint>::default())));
                        w.set_fx_chart_last("".into());
                        w.set_fx_chart_change("".into());
                        w.set_fx_chart_max("".into());
                        w.set_fx_chart_y1("".into());
                        w.set_fx_chart_y2("".into());
                        w.set_fx_chart_min("".into());
                        w.set_fx_chart_date_start("".into());
                        w.set_fx_chart_date_mid("".into());
                        w.set_fx_chart_date_end("".into());
                        w.set_fx_chart_dot_y(50.0);
                        w.set_fx_chart_status(format!("Grafik {code} belum tersedia ({error})").into());
                    }
                });
            });
        });
    }
    {
        let (weak, state, path, req) = (win.as_weak(), currency.clone(), currency_path.clone(), currency_request.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_add_alert(move |above, text, is_stock, code, repeat| {
            let Some(w) = weak.upgrade() else { return };
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            let Ok(target) = digits.parse::<f64>() else { return };
            if target <= 0.0 {
                return;
            }
            let code = code.to_string();
            let kind = if is_stock {
                notch_core::currency::AlertKind::Stock
            } else {
                notch_core::currency::AlertKind::Currency
            };
            let valid_code = match kind {
                notch_core::currency::AlertKind::Stock => notch_core::currency::is_allowed_stock_code(&code),
                notch_core::currency::AlertKind::Currency => notch_core::currency::CURRENCIES.iter().any(|(item, _)| *item == code),
            };
            if !valid_code {
                return;
            }
            let mut s = state.borrow_mut();
            if s.alerts.iter().any(|item| item.code == code && item.kind == kind && item.above == above && item.target == target && item.repeat == repeat) {
                return;
            }
            let id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis().to_string())
                .unwrap_or_default();
            s.alerts.push(notch_core::currency::Alert {
                id,
                code,
                kind,
                above,
                target,
                repeat,
                armed: true,
            });
            s.alerts.truncate(20);
            notch_core::currency::save_alerts(&path, &s.alerts);
            paint_currency(&w, &s, &gm, &lm, &am);
            drop(s);
            // Saat memasang alert saham, pastikan snapshot harga seluruh saham IDX segera diminta.
            if is_stock {
                req(false, true);
            }
        });
    }
    {
        let (weak, state, path) = (win.as_weak(), currency.clone(), currency_path.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_remove_alert(move |id| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            s.alerts.retain(|a| a.id != id.as_str());
            notch_core::currency::save_alerts(&path, &s.alerts);
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    // Pengingat kurs dan saham berjalan walau halaman tidak dibuka: cek cache tiap 30 dtk, ambil data baru tiap 10 menit.
    let currency_alert_timer = Timer::default();
    {
        let (weak, state, req, path) = (win.as_weak(), currency.clone(), currency_request.clone(), currency_path.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        let ticks = Cell::new(0u32);
        currency_alert_timer.start(TimerMode::Repeated, Duration::from_secs(30), move || {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            if s.alerts.is_empty() {
                return;
            }
            ticks.set(ticks.get() + 1);
            let has_stock_alert = s.alerts.iter().any(|alert| alert.kind == notch_core::currency::AlertKind::Stock);
            let stocks_stale = has_stock_alert && s.stocks_loaded_at
                .map(|loaded| loaded.elapsed() >= Duration::from_secs(600))
                .unwrap_or(true);
            if s.rates.is_none() || ticks.get() % 20 == 0 || stocks_stale {
                drop(s);
                req(false, has_stock_alert);
                return;
            }
            if fire_due_currency_alert(&w, &mut s, &path) {
                paint_currency(&w, &s, &gm, &lm, &am);
            }
        });
    }
    let shelf_store = Rc::new(RefCell::new(shelf::Store::open(notch_core::paths::data_file("shelf.json"))));
    let shelf_model = Rc::new(VecModel::<ShelfItem>::default());
    let shelf_items = shelf_store.borrow().list();
    shelf_model.set_vec(shelf_items.into_iter().map(|item| ShelfItem {
        path: item.path.into(), name: item.name.into(), dir: item.dir,
    }).collect::<Vec<_>>());
    win.set_shelf_items(ModelRc::from(shelf_model.clone()));
    let clipboard_monitor = Rc::new(if pages.iter().any(|page| page == "clipboard") {
        match ClipboardMonitor::start() {
            Ok(monitor) => Some(monitor),
            Err(error) => {
                eprintln!("[Clipboard] {error}");
                None
            }
        }
    } else {
        None
    });
    let clipboard_model = Rc::new(VecModel::<ClipboardItem>::default());
    win.set_clipboard_items(ModelRc::from(clipboard_model.clone()));
    win.set_clipboard_available(clipboard_monitor.is_some());
    show_clipboard(&clipboard_model, clipboard_monitor.as_ref().as_ref());
    let reminder_model = Rc::new(VecModel::<ReminderItem>::default());
    win.set_reminders(ModelRc::from(reminder_model.clone()));
    let timezone_offset = local_timezone_offset_minutes();
    let weekday_mask = Rc::new(Cell::new(62u8)); // Senin-Jumat, sama dengan pilihan awal Tauri
    win.set_reminder_weekdays_mask(62);
    win.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(62)))));
    show_reminders(&reminder_model, reminder::list(), timezone_offset);
    let active_reminder = Rc::new(RefCell::new(None::<String>));
    let prayer_prefs=Rc::new(RefCell::new(prayer::preferences()));
    win.set_prayer_city(prayer_prefs.borrow().kabkota.clone().into());win.set_prayer_province(prayer_prefs.borrow().provinsi.clone().into());
    win.set_prayer_mode(prayer_prefs.borrow().mode.clone().into());win.set_prayer_remind_before(if prayer_prefs.borrow().remind_before { "true" } else { "false" }.into());win.set_prayer_remind_min(prayer_prefs.borrow().remind_min.to_string().into());
    let ramadan=Rc::new(Cell::new(load_ramadan()));win.set_ramadan_enabled(ramadan.get());
    let dhikr_idx=Rc::new(Cell::new(0usize));
    let dhikr=[("أَسْتَغْفِرُ اللّٰهَ","Astaghfirullah","Aku memohon ampun kepada Allah. Dibaca 3x"),("اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ","Allahumma rabba hadzihid-da'watit-tammah","Doa setelah adzan"),("سُبْحَانَ اللّٰهِ وَبِحَمْدِهِ","Subhanallahi wa bihamdihi","Maha Suci Allah dan segala puji bagi-Nya. Dibaca 33x")];
    let set_dhikr:Rc<dyn Fn(&NotchWindow,usize)>=Rc::new(move|w,i|{let x=dhikr[i%3];w.set_dhikr_arabic(x.0.into());w.set_dhikr_latin(x.1.into());w.set_dhikr_meaning(x.2.into());w.set_dhikr_idx((i%3) as i32);});set_dhikr(&win,0);
    let prayer_busy=Arc::new(AtomicBool::new(false));let prayer_data=Arc::new(Mutex::new(PrayerSchedule::default()));
    let prayer_fired=Rc::new(RefCell::new(std::collections::HashSet::<String>::new()));
    let weather_busy = Arc::new(AtomicBool::new(false));
    let weather_location = Rc::new(RefCell::new((cfg.weather.lat, cfg.weather.lon, cfg.weather.city.clone())));
    win.set_weather_city(cfg.weather.city.clone().into());
    win.set_weather_lat(cfg.weather.lat.to_string().into());
    win.set_weather_lon(cfg.weather.lon.to_string().into());
    win.set_weather_status("Cuaca dimuat saat halaman dibuka".into());
    {
        let weak = win.as_weak();
        let busy = weather_busy.clone();
        let location = weather_location.clone();
        win.on_weather_refresh(move || {
            let (lat, lon, city) = location.borrow().clone();
            refresh_weather(weak.clone(), busy.clone(), lat, lon, city);
        });
    }
    let quick_busy = Arc::new(AtomicBool::new(false));
    let keep_awake = Arc::new(AtomicBool::new(false));

    // Setiap halaman hanya bekerja saat notch melebar DAN halamannya terlihat (seperti versi Tauri):
    // Stats disampel tiap 1 dtk, Baterai dibaca tiap 5 dtk, Quick/Tools dibaca ulang saat dibuka,
    // Catatan dimuat ulang dari disk setiap dibuka (perubahan dari versi Tauri ikut terbaca).
    let sync_pages: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let (stats_timer, battery_timer, clipboard_timer, weather_timer) = (stats_timer.clone(), battery_timer.clone(), clipboard_timer.clone(), weather_timer.clone());
        let (monitor, battery) = (monitor.clone(), battery.clone());
        let (notes_store, notes_model, notes_path) =
            (notes_store.clone(), notes_model.clone(), notes_path.clone());
        let (clipboard_model, clipboard_monitor) = (clipboard_model.clone(), clipboard_monitor.clone());
        let currency_request = currency_request.clone();
        let weather_busy = weather_busy.clone();
        let weather_location = weather_location.clone();
        let quick_busy = quick_busy.clone();
        let keep_awake = keep_awake.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            let visible = w.get_expanded();
            let page = w.get_page_id();
            let page = page.as_str();

            // Stats
            if visible && page == "stats" {
                if !stats_timer.running() {
                    monitor.borrow_mut().get_or_insert_with(StatsMonitor::new).resume();
                    let weak_s = weak.clone();
                    let monitor = monitor.clone();
                    stats_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
                        let Some(w) = weak_s.upgrade() else { return };
                        if let Some(m) = monitor.borrow_mut().as_mut() {
                            let s = m.sample();
                            w.set_cpu(s.cpu);
                            w.set_ram(s.ram);
                            w.set_disk(s.disk);
                            w.set_cpu_text(format!("{:.0}%", s.cpu).into());
                            w.set_ram_text(format!("{:.1} / {:.1} GiB", gib(s.ram_used), gib(s.ram_total)).into());
                            w.set_disk_text(format!("{:.0} / {:.0} GiB", gib(s.disk_used), gib(s.disk_total)).into());
                            w.set_net_text(format!("\u{2193} {}   \u{2191} {}", fmt_rate(s.down), fmt_rate(s.up)).into());
                            w.set_sys_text(format!("{} threads \u{00B7} {} proses", s.cpu_cores, s.processes).into());
                            // Tampilan halaman Stats (sama dengan quick-stats.ts di Tauri)
                            let freq = if s.cpu_freq_mhz > 0 { format!(" \u{00B7} {:.1} GHz", s.cpu_freq_mhz as f64 / 1000.0) } else { String::new() };
                            w.set_stats(StatsView {
                                host: s.host_name.into(),
                                cpu: s.cpu, ram: s.ram, swap: s.swap, disk: s.disk,
                                cpu_v: format!("{:.0}%", s.cpu).into(),
                                ram_v: format!("{:.0}%", s.ram).into(),
                                swap_v: format!("{:.0}%", s.swap).into(),
                                disk_v: format!("{:.0}%", s.disk).into(),
                                cpu_meta: format!("{} \u{00B7} {} thread{}", s.cpu_name, s.cpu_cores, freq).into(),
                                ram_meta: format!("{:.1} / {:.1} GiB digunakan", gib(s.ram_used), gib(s.ram_total)).into(),
                                swap_meta: format!("{:.1} / {:.1} GiB digunakan", gib(s.swap_used), gib(s.swap_total)).into(),
                                disk_meta: if s.disk_total > 0.0 {
                                    format!("{:.0} / {:.0} GiB \u{00B7} {}", gib(s.disk_used), gib(s.disk_total), s.disk_label).into()
                                } else { "Penyimpanan N/A".into() },
                                down: format!("\u{2193} {}", fmt_rate(s.down)).into(),
                                up: format!("\u{2191} {}", fmt_rate(s.up)).into(),
                                processes: format!("{} proses", s.processes).into(),
                                uptime: format!("Aktif {}", fmt_uptime(s.uptime_secs)).into(),
                                os: format!("{} {}", s.os_name, s.os_version).trim().to_string().into(),
                                kernel: s.kernel_version.into(),
                            });
                        }
                    });
                }
            } else {
                stats_timer.stop();
            }

            // Baterai
            if visible && page == "battery" {
                if !battery_timer.running() {
                    apply_battery(&w, battery.read());
                    let weak_b = weak.clone();
                    let battery = battery.clone();
                    battery_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
                        if let Some(w) = weak_b.upgrade() {
                            apply_battery(&w, battery.read());
                        }
                    });
                }
            } else {
                battery_timer.stop();
            }

            // Quick / Tools
            if visible && (page == "quick" || page == "tools") {
                refresh_quick(weak.clone(), quick_busy.clone(), keep_awake.clone());
            }

            // Catatan
            if visible && page == "notes" {
                let store = NotesStore::open(&notes_path);
                show_notes(&notes_model, store.list());
                *notes_store.borrow_mut() = Some(store);
            }

            // Clipboard: polling UI hanya berjalan saat halaman terlihat.
            if visible && page == "clipboard" {
                if !clipboard_timer.running() {
                    show_clipboard(&clipboard_model, clipboard_monitor.as_ref().as_ref());
                    let model = clipboard_model.clone();
                    let monitor = clipboard_monitor.clone();
                    clipboard_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
                        show_clipboard(&model, monitor.as_ref().as_ref());
                    });
                }
            } else {
                clipboard_timer.stop();
            }

            // Currency: muat kurs + saham saat halaman dibuka (cache 10 menit).
            if visible && page == "currency" {
                currency_request(false, true);
            }

            // Cuaca: ambil data saat halaman terlihat, lalu perbarui tiap lima menit.
            if visible && page == "weather" {
                if !weather_timer.running() {
                    let (lat, lon, city) = weather_location.borrow().clone();
                    refresh_weather(weak.clone(), weather_busy.clone(), lat, lon, city.clone());
                    let weak_w = weak.clone();
                    let busy_w = weather_busy.clone();
                    weather_timer.start(TimerMode::Repeated, Duration::from_secs(5 * 60), move || {
                        refresh_weather(weak_w.clone(), busy_w.clone(), lat, lon, city.clone());
                    });
                }
            } else {
                weather_timer.stop();
            }
        })
    };

    {
        let sync = sync_pages.clone();
        let (weak_pc, shell_pc) = (win.as_weak(), shell.clone());
        win.on_page_changed(move |_| {
            sync();
            if let Some(w) = weak_pc.upgrade() {
                if w.get_expanded() {
                    shell_pc.place(&w, expanded_size(&w));
                }
            }
        });
        let (weak_nx, shell_nx) = (win.as_weak(), shell.clone());
        win.on_notes_expand_toggled(move |_| {
            if let Some(w) = weak_nx.upgrade() {
                if w.get_expanded() {
                    shell_nx.place(&w, expanded_size(&w));
                }
            }
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_add_note(move |text| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.add(text.as_str()));
        });
    }
    let note_details: Rc<RefCell<std::collections::HashMap<String, NoteDetailWindow>>> = Rc::default();
    {
        let (weak, store, model, path, details) =
            (win.as_weak(), notes_store.clone(), notes_model.clone(), notes_path.clone(), note_details.clone());
        win.on_open_note(move |id| {
            let accent = weak.upgrade().map(|w| Theme::get(&w).get_accent()).unwrap_or(slint::Color::from_rgb_u8(59, 130, 246));
            open_note_detail(id.as_str(), &store, &path, &model, &details, accent);
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_clear_notes(move || {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.clear());
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_save_note(move |id, text| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.update(id.as_str(), text.as_str()));
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_remove_note(move |id| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.remove(id.as_str()));
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_add(move || {
            let selected = rfd::FileDialog::new().pick_files();
            let Some(paths) = selected else { return; };
            let paths = paths.into_iter().map(|path| path.to_string_lossy().to_string()).collect();
            let items = store.borrow_mut().add(paths);
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() {
                w.set_shelf_status(format!("{} item tersimpan. File asli tidak dipindahkan.", model.row_count()).into());
            }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_remove(move |path| {
            let items = store.borrow_mut().remove(path.as_str());
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() { w.set_shelf_status(format!("Item dihapus dari Shelf · {} tersisa", model.row_count()).into()); }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_clear(move || {
            let items = store.borrow_mut().clear();
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() { w.set_shelf_status("Shelf dikosongkan. File asli tetap aman.".into()); }
        });
    }
    win.on_shelf_open(|path| shelf::open(path.as_str()));
    win.on_shelf_reveal(|path| shelf::reveal(path.as_str()));

    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_media_control(|action| media::control(&action));
    win.on_adhan_stop(|| media::stop_adhan());
    {
      let enabled=ramadan.clone();
      let idx=dhikr_idx.clone();
      let set=set_dhikr.clone();
      let weak=win.as_weak();
      win.on_adhan_finished(move||{
        if !enabled.get(){return;}
        let i=1;
        idx.set(i);
        if let Some(w)=weak.upgrade(){set(&w,i);}
      });
    }
    win.on_media_seek(|position| media::seek(f64::from(position)));
      win.on_prayer_set_mode(move|v|{let mut p=prefs.borrow_mut();p.mode=v.to_string();if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_mode(p.mode.clone().into());}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind(move|v|{let mut p=prefs.borrow_mut();p.remind_before=v;if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_before(if v { "true" } else { "false" }.into());}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind_min(move|v|{let mut p=prefs.borrow_mut();p.remind_min=if v>=5&&p.remind_min as i32+v>60{5}else{(p.remind_min as i32+v).clamp(1,60)as u32};if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_min(p.remind_min.to_string().into());}});
    }
    {
      let weak=win.as_weak();let enabled=ramadan.clone();let shell_r=shell.clone();
      win.on_prayer_toggle_ramadan(move||{let v=!enabled.get();enabled.set(v);let path=notch_core::paths::data_file("ramadan.json");if let Some(d)=path.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(path,if v{"true"}else{"false"});if let Some(w)=weak.upgrade(){w.set_ramadan_enabled(v);if w.get_expanded(){shell_r.place(&w,expanded_size(&w));}}});
    }
    {
      let weak=win.as_weak();let idx=dhikr_idx.clone();let set=set_dhikr.clone();
      let (weak_p,idx_p,set_p)=(weak.clone(),idx.clone(),set.clone());
      win.on_prayer_dhikr_prev(move||{let i=(idx_p.get()+2)%3;idx_p.set(i);if let Some(w)=weak_p.upgrade(){set_p(&w,i);}});
      win.on_prayer_dhikr_next(move||{let i=(idx.get()+1)%3;idx.set(i);if let Some(w)=weak.upgrade(){set(&w,i);}});
    }

    {
        let model = reminder_model.clone();
        let weak = win.as_weak();
        win.on_reminder_add(move |text, date_time, repeat_mode, weekdays_mask| {
            let text = text.to_string();
            let date_time = date_time.to_string();
            let mode = repeat_mode.to_string();
            if text.trim().is_empty() {
                if let Some(w) = weak.upgrade() { w.set_reminder_status("Isi deskripsi pengingat terlebih dahulu.".into()); }
                return;
            }
            if mode != "none" && date_time.trim().is_empty() {
                if let Some(w) = weak.upgrade() {
                    w.set_reminder_status("Pengulangan memerlukan tanggal dan waktu.".into());
                }
                return;
            }
            let due_at = if date_time.trim().is_empty() {
                0
            } else {
                match reminder::parse_local_datetime(&date_time, timezone_offset) {
                    Some(value) => value,
                    None => {
                        if let Some(w) = weak.upgrade() { w.set_reminder_status("Format tanggal/waktu harus DD/MM/YYYY HH:MM.".into()); }
                        return;
                    }
                }
            };
            if mode == "weekly" && (weekdays_mask & 0x7f) == 0 {
                if let Some(w) = weak.upgrade() { w.set_reminder_status("Pilih minimal satu hari untuk pengulangan mingguan.".into()); }
                return;
            }
            let items = reminder::add(
                text, due_at, mode, timezone_offset,
                Some((weekdays_mask as u8) & 0x7f),
            );
            show_reminders(&model, items, timezone_offset);
            if let Some(w) = weak.upgrade() {
                w.set_reminder_status("Pengingat berhasil ditambahkan.".into());
                w.set_reminder_draft("".into());
                w.set_reminder_date_time("".into());
                w.set_reminder_repeat_mode("none".into());
            }
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_toggle(move |id| {
            show_reminders(&model, reminder::toggle(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_remove(move |id| {
            show_reminders(&model, reminder::remove(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_clear_done(move || {
            show_reminders(&model, reminder::clear_done(), timezone_offset);
        });
    }
    {
        let mask = weekday_mask.clone();
        let weak = win.as_weak();
        win.on_reminder_toggle_weekday(move |day| {
            if (0..=6).contains(&day) {
                let next = mask.get() ^ (1 << day);
                mask.set(next);
                if let Some(w) = weak.upgrade() {
                    w.set_reminder_weekdays_mask(i32::from(next));
                    w.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(next)))));
                }
            }
        });
    }

    {
        let monitor = clipboard_monitor.clone();
        win.on_clipboard_copy(move |text| {
            if let Some(monitor) = monitor.as_ref() {
                monitor.copy(text.to_string());
            }
        });
    }
    {
        let monitor = clipboard_monitor.clone();
        let model = clipboard_model.clone();
        win.on_clipboard_clear(move || {
            if let Some(monitor) = monitor.as_ref() {
                monitor.clear();
            }
            show_clipboard(&model, monitor.as_ref().as_ref());
        });
    }

    // --- Reminder: data and recurrence rules use the same JSON schema as Tauri ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let model = reminder_model.clone();
        let active = active_reminder.clone();
        reminder_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let items = reminder::list();
            if let Some(w) = weak.upgrade() {
                if w.get_page_id() == "reminder" {
                    show_reminders(&model, items.clone(), timezone_offset);
                }
                let due = reminder::due_now();
                let active_id = active.borrow().clone();
                let active_due = active_id.as_ref().and_then(|id| due.iter().find(|r| &r.id == id));
                if active_id.is_some() && active_due.is_none() {
                    *active.borrow_mut() = None;
                    w.set_reminder_alert_visible(false);
                }
                // Jangan menimpa rapat, adzan, atau interaksi hub yang sedang berlangsung.
                // Reminder tetap berstatus due dan akan ditampilkan pada siklus timer berikutnya.
                let can_interrupt = !w.get_expanded()
                    && !w.get_meeting_active()
                    && w.get_meeting_finished().is_empty()
                    && !w.get_adhan_active();
                if active.borrow().is_none() && can_interrupt {
                    if let Some(r) = due.first() {
                        *active.borrow_mut() = Some(r.id.clone());
                        w.set_page_id("reminder".into());
                        w.set_reminder_alert_visible(true);
                        w.set_reminder_alert_text(r.text.clone().into());
                        let when = if r.due_at > 0 {
                            let (y, m, d, h, min) = reminder::local_parts(r.due_at, timezone_offset);
                            format!("{d:02}/{m:02}/{y} {h:02}:{min:02}")
                        } else {
                            "Pengingat".to_string()
                        };
                        w.set_reminder_alert_sub(when.into());
                        // Halaman Pengingat lebih besar dari EXPANDED; ukuran jendela harus mengikuti badan hub.
                        shell.place(&w, expanded_size(&w));
                        w.set_expanded(true);
                        show_reminders(&model, items, timezone_offset);
                    }
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_stop_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::stop(id); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_snooze_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::snooze(id, 5); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }


    let fetch_prayer:Rc<dyn Fn(bool)>={
      let busy=prayer_busy.clone();let data=prayer_data.clone();let prefs=prayer_prefs.clone();let weak=win.as_weak();let offset=timezone_offset;
      Rc::new(move|refresh|{if busy.swap(true,Ordering::SeqCst){return;}let p=prefs.borrow().clone();let busy=busy.clone();let data=data.clone();let weak=weak.clone();
       std::thread::spawn(move||{let now=reminder::now_ms();let(y,m,_,_,_)=reminder::local_parts(now,offset);let cur=prayer::month(&p.provinsi,&p.kabkota,y as i32,m as u32,refresh);let(ny,nm,_,_,_)=reminder::local_parts(now+86_400_000,offset);let nxt=if ny!=y||nm!=m{prayer::month(&p.provinsi,&p.kabkota,ny as i32,nm as u32,false)}else{cur.clone()};let err=cur.as_ref().err().cloned().unwrap_or_default();let v=PrayerSchedule{current:cur.unwrap_or_default(),next:nxt.unwrap_or_default(),error:err};if let Ok(mut g)=data.lock(){*g=v;}busy.store(false,Ordering::SeqCst);let _=weak.upgrade_in_event_loop(|w|w.invoke_prayer_data_ready());});
      })
    };
    fetch_prayer(false);
    {let f=fetch_prayer.clone();win.on_prayer_refresh(move||f(true));}
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let prefs=prayer_prefs.clone();let fired=prayer_fired.clone();let offset=timezone_offset;
      prayer_timer.start(
          TimerMode::Repeated,
          Duration::from_secs(1),
          move || {
              let Ok(schedule) = data.lock() else {
                  return;
              };
              let now = reminder::now_ms();
              let (year, month, day_number, _, _) = reminder::local_parts(now, offset);

              let Some(window) = weak.upgrade() else {
                  return;
              };

              if window.get_page_id() == "prayer" {
                  render_prayer(&window, &schedule, offset, ramadan.get());
              }

              if let Some(day) = prayer_find(&schedule.current, day_number) {
                  let preferences = prefs.borrow().clone();
                  let prayer_times = [
                      ("Subuh", &day.subuh),
                      ("Dzuhur", &day.dzuhur),
                      ("Ashar", &day.ashar),
                      ("Maghrib", &day.maghrib),
                      ("Isya", &day.isya),
                  ];

                  for (name, time) in prayer_times {
                      let Some(at) = prayer_at((year, month, day_number), time, offset) else {
                          continue;
                      };

                      let triggers = [
                          ("pre", at - i64::from(preferences.remind_min) * 60_000),
                          ("at", at),
                      ];

                      for (kind, trigger) in triggers {
                          if kind == "pre" && !preferences.remind_before {
                              continue;
                          }
                          if now < trigger || now >= trigger + 600_000 {
                              continue;
                          }

                          let key = format!("{year}-{month}-{day_number}-{name}-{kind}");
                          if !fired.borrow_mut().insert(key) {
                              continue;
                          }

                          if kind == "pre" {
                              window.set_prayer_status(
                                  format!(
                                      "Menjelang {name} · {} menit lagi",
                                      preferences.remind_min
                                  )
                                  .into(),
                              );
                              continue;
                          }

                          window.set_prayer_status(
                              format!("Waktu {name} telah tiba").into(),
                          );

                          if preferences.mode == "tone" {
                              #[cfg(windows)]
                              unsafe {
                                  let _ = windows::Win32::System::Diagnostics::Debug::Beep(880, 250);
                              }
                          } else if preferences.mode == "adzan" {
                              window.set_prayer_status(
                                  format!("Adzan {name} sedang diputar").into(),
                              );
                              media::play_adhan(window.as_weak(), name.to_string());
                          }

                          if ramadan.get() {
                              window.set_dhikr_arabic(
                                  "اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ".into(),
                              );
                              window.set_dhikr_latin(
                                  "Allahumma rabba hadzihid-da'watit-tammah".into(),
                              );
                              window.set_dhikr_meaning(
                                  "Dzikir setelah adzan".into(),
                              );
                          }
                      }
                  }
              }
          },
      );
    }
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let offset=timezone_offset;
      win.on_prayer_data_ready(move||{if let Ok(s)=data.lock(){if let Some(w)=weak.upgrade(){render_prayer(&w,&s,offset,ramadan.get());}}});
    }

    // --- Kalender + hari libur Indonesia ---
    // Cache `holidays.json` dibaca sekarang (cepat); unduhan baru jalan di thread latar dan hasilnya
    // diserahkan lewat slot bersama + callback `holidays-ready` supaya state tetap di thread UI.
    let hol_path = notch_core::paths::data_file("holidays.json");
    let hol = Rc::new(RefCell::new(Holidays::new()));
    if let Some((_, items)) = calendar::read_cache(&hol_path) {
        hol.borrow_mut().ingest(&items);
    }
    let cal_view = Rc::new(RefCell::new(CalView::new(today(), &hol.borrow())));
    let cal_model = Rc::new(VecModel::<CalCell>::default());
    win.set_cal_cells(ModelRc::from(cal_model.clone()));
    win.set_cal_weekdays(ModelRc::from(Rc::new(VecModel::from(
        calendar::weekday_headers().iter().map(|s| SharedString::from(*s)).collect::<Vec<_>>(),
    ))));
    let redraw_cal: Rc<dyn Fn()> = {
        let (weak, model, view, hol) = (win.as_weak(), cal_model.clone(), cal_view.clone(), hol.clone());
        Rc::new(move || {
            if let Some(w) = weak.upgrade() {
                show_calendar(&w, &model, &view.borrow(), &hol.borrow());
            }
        })
    };
    let hol_slot: Arc<Mutex<Option<Vec<Item>>>> = Arc::new(Mutex::new(None));
    let hol_busy = Arc::new(AtomicBool::new(false));
    let start_fetch: Rc<dyn Fn()> = {
        let (weak, slot, busy, path) = (win.as_weak(), hol_slot.clone(), hol_busy.clone(), hol_path.clone());
        Rc::new(move || {
            if busy.swap(true, Ordering::SeqCst) {
                return;
            }
            let (weak, slot, busy, path) = (weak.clone(), slot.clone(), busy.clone(), path.clone());
            std::thread::spawn(move || {
                // gagal (offline, belum ada cache) = tetap memakai data bawaan
                if let Ok(items) = calendar::fetch(&path) {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(items);
                    }
                    let _ = weak.upgrade_in_event_loop(|w| w.invoke_holidays_ready());
                }
                busy.store(false, Ordering::SeqCst);
            });
        })
    };
    {
        let (hol, slot, redraw) = (hol.clone(), hol_slot.clone(), redraw_cal.clone());
        win.on_holidays_ready(move || {
            let items = slot.lock().ok().and_then(|mut g| g.take());
            if let Some(items) = items {
                hol.borrow_mut().ingest(&items);
                redraw();
            }
        });
    }
    {
        let (view, hol, redraw, fetch) = (cal_view.clone(), hol.clone(), redraw_cal.clone(), start_fetch.clone());
        win.on_cal_opened(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
            fetch(); // paling sering sekali per 12 jam (cache)
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_prev(move || {
            view.borrow_mut().shift_month(-1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_next(move || {
            view.borrow_mut().shift_month(1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_today(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, redraw) = (cal_view.clone(), redraw_cal.clone());
        win.on_cal_select(move |i| {
            view.borrow_mut().select(i.max(0) as usize);
            redraw();
        });
    }
    start_fetch(); // data libur terbaru di latar belakang sejak aplikasi mulai

    // --- Quick / Tools: aksi jalan di thread lain (bisa sampai 4 detik) ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let keep_awake = keep_awake.clone();
        win.on_quick_toggle(move |kind| {
            if kind.as_str() == "center" {
                // Kembali ke tengah atas (hanya untuk sesi ini; config tidak ditulis dari Slint).
                shell.pos.set(None);
                shell.monitor_hint.set(None);
                if let Some(w) = weak.upgrade() {
                    shell.place(&w, EXPANDED);
                }
                return;
            }
            let weak = weak.clone();
            let kind = kind.to_string();
            let keep_awake = keep_awake.clone();
            std::thread::spawn(move || {
                let q = quick::toggle(&kind);
                keep_awake.store(q.awake == Some(true), Ordering::Relaxed);
                let _ = weak.upgrade_in_event_loop(move |w| apply_quick(&w, &q));
            });
        });
    }

    // --- Hover dengan jeda dari config: timing.hover_open_ms / hover_close_ms ---
    let open_timer = Rc::new(Timer::default());
    let close_timer = Rc::new(Timer::default());
    let shrink_timer = Rc::new(Timer::default());
    // Pembukaan hub yang ditunda satu frame agar notch sempat disembunyikan sebelum jendela membesar.
    let expand_pending = Rc::new(Timer::default());
    let open_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_open_ms)));
    let close_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_close_ms)));
    let expand: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let pending = expand_pending.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            shrink.stop();
            pending.stop();
            // Dengan wajah jam, halaman home harus ditetapkan sebelum ukuran native dihitung.
            if w.get_idle_clock() {
                w.set_page_id("clock".into());
            }
            // Hover saat greeting tampil: hub menggantikan greeting. Flag harus dibersihkan, kalau tidak kartu 385 px
            // muncul lagi di jendela 300 px begitu hub menciut.
            w.set_greeting_inline_visible(false);
            let size = expanded_size(&w);
            if !w.get_expanded() && window_needs_resize(&w, size) {
                // Jendela akan membesar sementara notch (mata) masih tampak: frame lama akan tampil di koordinat
                // baru sebagai "mata hantu". Sembunyikan notch dulu, beri satu frame untuk tergambar, baru besarkan.
                w.set_geometry_hold(true);
                let (weak, shell, sync) = (weak.clone(), shell.clone(), sync.clone());
                // 40 ms = beberapa frame agar frame tanpa notch sempat tergambar dan sampai ke layar; lalu tunggu
                // komposisi DWM berikutnya (seperti shrink_window_synced) dan besarkan jendela tepat setelahnya.
                pending.start(TimerMode::SingleShot, Duration::from_millis(40), move || {
                    platform::wait_vsync();
                    finish_expand(&weak, &shell, &*sync);
                });
            } else {
                finish_expand(&weak, &shell, &*sync);
            }
        })
    };
    let collapse: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let pending = expand_pending.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            // Kursor keluar sebelum hub sempat dibuka: batalkan pembukaan yang tertunda dan tampilkan notch lagi.
            if pending.running() {
                pending.stop();
                w.set_geometry_hold(false);
            }
            // Hub yang ditutup pengguna: bila halaman terakhir adalah Jam, notch berganti dari mata ke jam + tanggal;
            // di halaman lain kembali ke mata (sama dengan collapse() di view-controller.ts Tauri).
            if w.get_expanded() && w.get_active_pages().iter().any(|p| p == "clock") {
                let clock = w.get_page_id() == "clock";
                if w.get_idle_clock() != clock {
                    w.set_idle_clock(clock);
                    let _ = std::fs::write(notch_core::paths::data_file("idle_face.txt"), if clock { "clock" } else { "eyes" });
                }
            }
            w.set_notes_expanded(false);
            w.set_expanded(false);
            w.set_cal_open(false); // kalender dan lirik ikut menutup saat notch menciut
            w.set_lyrics_open(false);
            sync();
            // jendela baru diperkecil setelah animasi menciut selesai
            let weak2 = weak.clone();
            let shell = shell.clone();
            // 300 ms animasi menciut + sedikit jeda, baru jendela diperkecil.
            shrink.start(TimerMode::SingleShot, Duration::from_millis(330), move || {
                shrink_window_synced(weak2.clone(), shell.clone());
            });
        })
    };
    {
        let weak = win.as_weak();
        let (open_timer, close_timer) = (open_timer.clone(), close_timer.clone());
        let (open_ms, close_ms) = (open_ms.clone(), close_ms.clone());
        let expand_on_hover = expand.clone();
        let collapse_on_hover = collapse.clone();
        win.on_hover_changed(move |hovered| {
            let Some(w) = weak.upgrade() else { return };
            w.set_play_interaction_active(hovered);
            if hovered {
                close_timer.stop();
                if !w.get_expanded() {
                    let e = expand_on_hover.clone();
                    open_timer.start(TimerMode::SingleShot, Duration::from_millis(open_ms.get()), move || e());
                }
            } else {
                open_timer.stop();
                if w.get_expanded() {
                    let c = collapse_on_hover.clone();
                    let weak_check = weak.clone();
                close_timer.start(TimerMode::SingleShot, Duration::from_millis(close_ms.get()), move || {
                    let Some(w) = weak_check.upgrade() else { return };
                    // Slint Flickable dapat mengambil alih pointer saat roda mouse digulir.
                    // Pada halaman Clipboard, event hover sementara bisa melaporkan keluar
                    // meskipun kursor masih berada di dalam window. Jangan ciutkan notch
                    // selama pointer masih berada di batas window; hover berikutnya akan
                    // tetap mengatur penutupan normal ketika pointer benar-benar keluar.
                    if w.get_page_id() == "clipboard" {
                        if let Some((cx, cy)) = platform::cursor_pos() {
                            let p = w.window().position();
                            let s = w.window().size();
                            if cx >= p.x && cx < p.x + s.width as i32
                                && cy >= p.y && cy < p.y + s.height as i32
                            {
                                return;
                            }
                        }
                    }
                    c();
                });
                }
            }
        });
    }

    // --- Shelf: drag file MASUK (dari Explorer) dan KELUAR (dari tile Shelf) ---
    // Slint hanya menyimpan SATU callback on_winit_window_event (pendaftaran baru menimpa yang lama), jadi semua
    // pendengar event jendela (Shelf, Convert) didaftarkan ke `winit_handlers` dan dipanggil lewat satu dispatcher.
    let winit_handlers: Rc<RefCell<Vec<Box<dyn FnMut(&winit::event::WindowEvent)>>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let handlers = winit_handlers.clone();
        win.window().on_winit_window_event(move |_window, event| {
            for handler in handlers.borrow_mut().iter_mut() {
                handler(event);
            }
            EventResult::Propagate
        });
    }
    // True selama kita sendiri yang menyeret file keluar: event hover/drop dari drag itu diabaikan.
    let shelf_drag_out = Rc::new(Cell::new(false));
    // True bila hub dibuka oleh drag file (bukan oleh pengguna), supaya ditutup otomatis setelah selesai.
    let shelf_opened_by_drag = Rc::new(Cell::new(false));
    // Tutup hub yang dibuka oleh drag setelah jeda, kecuali pointer masih di dalam jendela
    // (hover Slint yang menutupnya saat pointer benar-benar keluar).
    let shelf_close_later: Rc<dyn Fn(u64)> = {
        let weak = win.as_weak();
        let close_timer = close_timer.clone();
        let collapse = collapse.clone();
        let opened = shelf_opened_by_drag.clone();
        Rc::new(move |delay_ms| {
            let weak = weak.clone();
            let collapse = collapse.clone();
            let opened = opened.clone();
            close_timer.start(TimerMode::SingleShot, Duration::from_millis(delay_ms), move || {
                let Some(w) = weak.upgrade() else { return };
                if !opened.get() || !w.get_expanded() { return; }
                if let Some((cx, cy)) = platform::cursor_pos() {
                    let (p, z) = (w.window().position(), w.window().size());
                    if cx >= p.x && cx < p.x + z.width as i32 && cy >= p.y && cy < p.y + z.height as i32 { return; }
                }
                opened.set(false);
                collapse();
            });
        })
    };
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        let (open_timer, close_timer, shrink_timer) = (open_timer.clone(), close_timer.clone(), shrink_timer.clone());
        let (sync, shell) = (sync_pages.clone(), shell.clone());
        let (drag_out, opened_by_drag, close_later) = (shelf_drag_out.clone(), shelf_opened_by_drag.clone(), shelf_close_later.clone());
        // Buka hub di halaman Shelf SEKARANG: ubah halaman, perbesar jendela OS, lalu expand. Mengubah properti dari Rust
        // tidak memicu callback page-changed/hover Slint, jadi ukuran jendela dan sinkron halaman dilakukan di sini.
        let open_shelf = {
            let (weak, shell, sync) = (weak.clone(), shell.clone(), sync.clone());
            move || {
                let Some(w) = weak.upgrade() else { return };
                shrink_timer.stop();
                w.set_page_id("shelf".into());
                shell.place(&w, expanded_size(&w));
                w.set_expanded(true);
                sync();
            }
        };
        winit_handlers.borrow_mut().push(Box::new(move |event| {
            let Some(w) = weak.upgrade() else { return };
            match event {
                winit::event::WindowEvent::HoveredFile(_) => {
                    if drag_out.get() || !shelf_accepts_drop(&w) { return; }
                    open_timer.stop();
                    close_timer.stop();
                    if !(w.get_expanded() && w.get_page_id() == "shelf") {
                        opened_by_drag.set(!w.get_expanded());
                        open_shelf();
                    }
                    w.set_shelf_dropping(true);
                    w.set_shelf_status("Lepaskan file atau folder untuk menyimpannya ke Shelf".into());
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    if drag_out.get() { return; }
                    w.set_shelf_dropping(false);
                    w.set_shelf_status("Seret file ke Notch · file asli tetap aman".into());
                    // File keluar dari jendela tanpa dilepas: tutup lagi bila hub dibuka oleh drag ini.
                    if opened_by_drag.get() { close_later(700); }
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    if drag_out.get() || !shelf_accepts_drop(&w) { return; }
                    // Winit mengirim satu DroppedFile per path; hub dipastikan terbuka walau tanpa HoveredFile.
                    let items = store.borrow_mut().add(vec![path.to_string_lossy().to_string()]);
                    model.set_vec(items.into_iter().map(|item| ShelfItem {
                        path: item.path.into(), name: item.name.into(), dir: item.dir,
                    }).collect::<Vec<_>>());
                    close_timer.stop();
                    if !(w.get_expanded() && w.get_page_id() == "shelf") {
                        opened_by_drag.set(!w.get_expanded());
                        open_shelf();
                    }
                    w.set_shelf_dropping(false);
                    w.set_shelf_status(format!("File ditambahkan · {} item tersimpan · file asli tetap di lokasi semula", model.row_count()).into());
                    // Beri waktu melihat hasilnya, lalu tutup bila hub dibuka oleh drag ini dan pointer sudah keluar.
                    if opened_by_drag.get() { close_later(2500); }
                }
                _ => {}
            }
        }));
    }

    // Native drag-out memakai crate drag-rs (OLE DoDragDrop, memblokir sampai drop selesai).
    #[cfg(windows)]
    {
        const DRAG_ICON: &[u8] = &[137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,137,0,0,0,13,73,68,65,84,120,156,99,96,24,5,0,0,1,0,1,90,44,12,72,0,0,0,0,73,69,78,68,174,66,96,130];
        let weak_window = win.as_weak();
        let (drag_out, close_later, opened_by_drag) = (shelf_drag_out.clone(), shelf_close_later.clone(), shelf_opened_by_drag.clone());
        win.on_shelf_drag(move |raw_path| {
            if drag_out.get() { return; }
            let path = std::path::PathBuf::from(raw_path.as_str());
            if !path.exists() { return; }
            let path = match std::fs::canonicalize(path) { Ok(path) => path, Err(_) => return };
            // Upgrade weak handle di dalam callback agar tidak menyimpan borrow
            // dari `win.window()` ke closure yang harus hidup selama 'static.
            let Some(window_component) = weak_window.upgrade() else { return };
            let dropped = Arc::new(AtomicBool::new(false));
            let dropped_cb = dropped.clone();
            drag_out.set(true);
            let started = window_component.window().with_winit_window(|window| {
                let item = drag::DragItem::Files(vec![path]);
                let preview = drag::Image::Raw(DRAG_ICON.to_vec());
                drag::start_drag(
                    window,
                    item,
                    preview,
                    move |result, _cursor| { dropped_cb.store(matches!(result, drag::DragResult::Dropped), Ordering::Relaxed); },
                    drag::Options::default(),
                )
            });
            drag_out.set(false);
            match started {
                Some(Ok(())) => {
                    window_component.set_shelf_status(if dropped.load(Ordering::Relaxed) {
                        "File dikirim ke tujuan · file asli tetap tersimpan di Shelf".into()
                    } else {
                        "Seret dibatalkan · file tetap di Shelf".into()
                    });
                    // Selama drag modal Slint tidak menerima event pointer, jadi hub dapat tertinggal terbuka.
                    // Hanya hub yang dibuka oleh drag masuk yang ditutup otomatis; hub buka manual dibiarkan.
                    if opened_by_drag.get() { close_later(1200); }
                }
                Some(Err(error)) => {
                    eprintln!("Shelf drag-out gagal: {error:?}");
                    window_component.set_shelf_status("Gagal menyeret file keluar".into());
                }
                None => {}
            }
        });
    }

    // --- Jam: cek tiap detik, tapi hanya set properti kalau teks berubah ---
    let clock_timer = Timer::default();
    {
        let weak = win.as_weak();
        let last = Rc::new(Cell::new((99u32, 99u32)));
        let last_sec = Rc::new(Cell::new(99u32));
        let tick = move || {
            let Some(w) = weak.upgrade() else { return };
            let (h, m, d, mo, y) = platform::local_time();
            let (sec, wday) = platform::local_sec_wday();
            if last_sec.get() != sec {
                last_sec.set(sec);
                w.set_sec_text(format!("{sec:02}").into());
            }
            if last.get() != (h, m) {
                last.set((h, m));
                w.set_clock_text(format!("{h:02}:{m:02}").into());
                w.set_day_text(ID_DAYS[(wday % 7) as usize].into());
                w.set_date_short(format!("{d} {}", SHORT_MONTHS[(mo.clamp(1, 12) - 1) as usize]).into());
                w.set_date_text(format!("{d} {} {y}", ID_MONTHS[(mo.clamp(1, 12) - 1) as usize]).into());
            }
        };
        tick();
        clock_timer.start(TimerMode::Repeated, Duration::from_secs(1), tick);
    }

    // --- Pomodoro (state machine dari notch-core, sama dengan pomodoro.ts) ---
    let pomo = Rc::new(RefCell::new(Pomodoro::new(&cfg.pomodoro)));
    let focus_companion_runtime = Rc::new(RefCell::new(cfg.focus_companion.clone()));
    let conversation_runtime = Rc::new(RefCell::new(cfg.conversation_moments.clone()));
    let companion_focus_started = Rc::new(Cell::new(false));
    let companion_halfway_shown = Rc::new(Cell::new(false));
    let companion_hide_timer = Rc::new(Timer::default());
    // Focus Companion memakai antrean singkat agar tidak menimpa greeting, pengingat,
    // kartu Conversation Moments, atau overlay lain yang sedang tampil.
    let companion_pending = Rc::new(RefCell::new(None::<(String, String, Instant)>));
    show_pomo(&win, &pomo.borrow());
    let pomo_timer = Timer::default();
    {
        let weak = win.as_weak();
        let pomo = pomo.clone();
        let companion = focus_companion_runtime.clone();
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        let hide_timer = companion_hide_timer.clone();
        let pending = companion_pending.clone();
        pomo_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let mut p = pomo.borrow_mut();
            let finished = if p.running() { p.tick(Instant::now()) } else { None };
            let phase = p.phase();

            // Event otomatis hanya dipicu ketika fase benar-benar selesai, bukan saat tombol Skip ditekan.
            if let Some(completed_phase) = finished {
                let settings = companion.borrow().clone();
                if settings.enabled {
                    let message = match completed_phase {
                        Phase::Focus if settings.on_focus_complete => Some(("Fokus selesai! Kerja bagus. 🎉", "Focus Companion · waktunya istirahat")),
                        Phase::Break | Phase::Long if settings.on_break_end => Some(("Istirahat selesai. Siap fokus lagi? 🚀", "Focus Companion · sesi fokus berikutnya")),
                        _ => None,
                    };
                    if let Some((message, detail)) = message {
                        if let Some(w) = weak.upgrade() {
                            if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                                *pending.borrow_mut() = Some((message.to_string(), detail.to_string(), Instant::now() + Duration::from_secs(60)));
                            }
                        }
                    }
                }
                if completed_phase == Phase::Focus {
                    focus_started.set(false);
                    halfway_shown.set(false);
                } else if matches!(completed_phase, Phase::Break | Phase::Long) {
                    focus_started.set(false);
                    halfway_shown.set(false);
                }
            }

            // Sesi fokus dimulai saat timer pertama kali dijalankan, bukan saat resume setelah jeda.
            if phase == Phase::Focus && p.running() && !focus_started.get() {
                focus_started.set(true);
                halfway_shown.set(false);
                let settings = companion.borrow().clone();
                if settings.enabled && settings.on_start {
                    if let Some(w) = weak.upgrade() {
                        if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                            *pending.borrow_mut() = Some((
                                "Yuk, fokus satu hal dulu. Kamu pasti bisa! 🎯".to_string(),
                                "Focus Companion · sesi fokus dimulai".to_string(),
                                Instant::now() + Duration::from_secs(60),
                            ));
                        }
                    }
                }
            }

            if phase == Phase::Focus && p.running() && p.progress() >= 0.5 && !halfway_shown.get() {
                halfway_shown.set(true);
                let settings = companion.borrow().clone();
                if settings.enabled && settings.on_halfway {
                    if let Some(w) = weak.upgrade() {
                        if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                            *pending.borrow_mut() = Some((
                                "Sudah separuh jalan! Pertahankan ritmenya. 💪".to_string(),
                                "Focus Companion · 50% sesi fokus".to_string(),
                                Instant::now() + Duration::from_secs(60),
                            ));
                        }
                    }
                }
            }

            if pending.borrow().as_ref().is_some_and(|(_, _, expires)| Instant::now() >= *expires) {
                pending.borrow_mut().take();
            }
            if let Some(w) = weak.upgrade() {
                // Jangan memutar pesan fokus yang tertahan selama rapat atau Adzan.
                if w.get_meeting_active() || w.get_adhan_active() {
                    pending.borrow_mut().take();
                }
                let can_show = !w.get_expanded() && !w.get_nudge_visible()
                    && !w.get_reminder_alert_visible() && !w.get_meeting_active()
                    && w.get_meeting_finished().is_empty() && !w.get_adhan_active();
                if can_show {
                    if let Some((message, detail, _)) = pending.borrow_mut().take() {
                        w.set_nudge_message(message.into());
                        w.set_nudge_detail(detail.into());
                        w.set_nudge_visible(true);
                        let weak_hide = weak.clone();
                        hide_timer.start(TimerMode::SingleShot, Duration::from_secs(5), move || {
                            if let Some(w) = weak_hide.upgrade() { w.set_nudge_visible(false); }
                        });
                    }
                }
                show_pomo(&w, &p);
            }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        win.on_toggle_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.toggle(Instant::now());
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        win.on_reset_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.reset();
            focus_started.set(false);
            halfway_shown.set(false);
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        win.on_skip_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.skip();
            focus_started.set(false);
            halfway_shown.set(false);
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }

    // Conversation Moments: amati transisi media dan aktivitas pointer secara native.
    // Cooldown bersama mencegah beberapa pemicu menumpuk menjadi spam komentar.
    let conversation_timer = Timer::default();
    {
        let weak = win.as_weak();
        let settings = conversation_runtime.clone();
        let eye = appearance_eye.clone();
        let hide_timer = Rc::new(Timer::default());
        let battery = battery.clone();
        // Momen yang terjadi ketika hub/kartu/Adzan sedang aktif menunggu sebentar,
        // mengikuti perilaku antrean di implementasi Tauri.
        let mut pending_event: Option<(&'static str, &'static str, Instant)> = None;
        let state = Rc::new(RefCell::new((
            false,
            String::new(),
            None::<Instant>,
            eye.borrow().as_ref().map(|ctx| ctx.borrow().last_move).unwrap_or(0.0),
            None::<bool>,
            None::<bool>,
            String::new(),
            false,
        )));
        conversation_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(w) = weak.upgrade() else { return };
            let cfg = settings.borrow().clone();
            let playing = w.get_media_playing();
            let title = w.get_media_title().to_string();
            let pointer_move = eye.borrow().as_ref().map(|ctx| ctx.borrow().last_move).unwrap_or(0.0);
            let battery_reading = battery.read();
            let weather_condition = w.get_weather_condition().to_string();
            let weather_rain = w.get_weather_rain().to_string();
            let mut state = state.borrow_mut();
            let (last_playing, last_title, last_message, last_move, was_low_battery, was_charging, last_weather, was_rain_soon) = &mut *state;
            let mut event: Option<(&'static str, &'static str)> = None;

            if playing && !*last_playing && cfg.on_play {
                event = Some(("Musik mulai diputar. Semoga playlist-nya bikin semangat! 🎵", "Conversation Moments · musik dimulai"));
            } else if playing && !title.is_empty() && title != *last_title && cfg.on_track_change {
                event = Some(("Lagu baru, suasana baru. 🎶", "Conversation Moments · lagu berganti"));
            } else if !playing && *last_playing && cfg.on_pause {
                event = Some(("Musik berhenti dulu. Mau istirahat sebentar?", "Conversation Moments · media dijeda"));
            }

            if pointer_move > *last_move + 0.001 {
                // last_move adalah timestamp gerakan sebelumnya (detik sejak startup).
                // Ukur sampai waktu sekarang, bukan selisih dua event pointer yang berdekatan.
                let idle_secs = (app_started_at.elapsed().as_secs_f32() - *last_move).max(0.0);
                if event.is_none() && cfg.on_return && idle_secs >= cfg.return_after_min as f32 * 60.0 {
                    event = Some(("Akhirnya kembali! Siap melanjutkan aktivitas? 👋", "Conversation Moments · kembali setelah lama tidak aktif"));
                }
                *last_move = pointer_move;
            }

            *last_playing = playing;
            if !title.is_empty() && title != "Tidak ada media" {
                *last_title = title;
            } else if !playing {
                *last_title = String::new();
            }

            // Perubahan kondisi dan prakiraan hujan hanya dinilai setelah data cuaca valid tersedia.
            if !weather_condition.is_empty()
                && weather_condition != "Memuat cuaca..."
                && weather_condition != "Data cuaca belum tersedia"
            {
                if !last_weather.is_empty() && weather_condition != *last_weather
                    && cfg.on_weather_change && event.is_none()
                {
                    event = Some(("Cuaca berubah. Sesuaikan aktivitasmu dengan kondisi di luar, ya! 🌤️", "Conversation Moments · kondisi cuaca berubah"));
                }
                *last_weather = weather_condition;
            }
            let rain_soon = weather_rain.starts_with("Hujan dalam ");
            if rain_soon && !*was_rain_soon && cfg.on_rain_soon && event.is_none() {
                event = Some(("Sepertinya hujan akan turun. Jangan lupa siapkan payung! ☔", "Conversation Moments · hujan segera"));
            }
            *was_rain_soon = rain_soon;

            // Status baterai dibaca ringan; komentar hanya muncul pada transisi menuju kondisi.
            if let Reading::Present(value) = battery_reading {
                // Sampel pertama hanya menjadi baseline; baterai yang memang sudah lemah
                // ketika aplikasi dibuka tidak dianggap sebagai transisi baru.
                let low = value.percent <= 15 && !value.charging;
                if let Some(previous_low) = *was_low_battery {
                    if low && !previous_low && cfg.on_low_battery && event.is_none() {
                        event = Some(("Baterai tinggal 15%. Saatnya mencari charger, ya! 🔋", "Conversation Moments · baterai lemah"));
                    }
                }
                if value.charging && *was_charging == Some(false) && cfg.on_charging && event.is_none() {
                    event = Some(("Laptop mulai mengisi daya. Energi kembali penuh! ⚡", "Conversation Moments · mulai mengisi daya"));
                }
                *was_low_battery = Some(low);
                *was_charging = Some(value.charging);
            }

            if w.get_meeting_active() {
                // Sama seperti Tauri: momen yang tertahan selama rapat dibuang.
                pending_event = None;
            }
            if cfg.enabled {
                if let Some((message, detail)) = event {
                    // Satu momen pending cukup agar pemicu tidak menimpa kartu yang sedang tampil.
                    if pending_event.is_none() {
                        pending_event = Some((message, detail, Instant::now() + Duration::from_secs(30)));
                    }
                }
            } else {
                pending_event = None;
            }
            if pending_event.is_some_and(|(_, _, expires)| Instant::now() >= expires) {
                pending_event = None;
            }
            let cooldown_ok = last_message.map(|at| at.elapsed() >= Duration::from_secs(u64::from(cfg.cooldown_min) * 60)).unwrap_or(true);
            let can_show = !w.get_expanded() && !w.get_nudge_visible()
                && !w.get_reminder_alert_visible() && !w.get_meeting_active()
                && w.get_meeting_finished().is_empty() && !w.get_adhan_active();
            if cfg.enabled && cooldown_ok && can_show {
                if let Some((message, detail, _)) = pending_event.take() {
                    w.set_nudge_message(message.into());
                    w.set_nudge_detail(detail.into());
                    w.set_nudge_visible(true);
                    if let Some(ctx) = eye.borrow().as_ref() {
                        ctx.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
                    }
                    *last_message = Some(Instant::now());
                    let weak_hide = weak.clone();
                    hide_timer.start(TimerMode::SingleShot, Duration::from_secs(5), move || {
                        if let Some(w) = weak_hide.upgrade() { w.set_nudge_visible(false); }
                    });
                }
            }
        });
    }

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

    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_start(move || {
            let result = state.lock().unwrap_or_else(|e| e.into_inner()).start();
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(url) => { w.set_transfer_address(url.into()); w.set_transfer_status("Server aktif — bagikan alamat ini ke perangkat lain".into()); }
                    Err(error) => w.set_transfer_status(format!("Gagal memulai server: {error}").into()),
                }
            }
        });
    }
    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_stop(move || {
            state.lock().unwrap_or_else(|e| e.into_inner()).stop();
            if let Some(w) = weak.upgrade() { w.set_transfer_address("".into()); w.set_transfer_status("Server dihentikan".into()); }
        });
    }
    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_add_files(move || {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                let strings = paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
                let count = state.lock().unwrap_or_else(|e| e.into_inner()).add_files(strings);
                if let Some(w) = weak.upgrade() { w.set_transfer_shared_count_text(count.to_string().into()); w.set_transfer_status("Daftar file dibagikan diperbarui".into()); }
            }
        });
    }
    {
        let inbox = notch_core::paths::config_path().parent().unwrap_or(std::path::Path::new(".")).join("Notch");
        let weak = win.as_weak();
        win.on_transfer_open_inbox(move || {
            let result = transfer::open_inbox(&inbox);
            if let Err(error) = result {
                if let Some(w) = weak.upgrade() { w.set_transfer_status(format!("Gagal membuka folder: {error}").into()); }
            }
        });
    }

    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_refresh(move || {
            let mut android = state.lock().unwrap_or_else(|e| e.into_inner());
            let status = android.status();
            let devices = android.devices();
            if let Some(w) = weak.upgrade() {
                w.set_android_status(status.into());
                w.set_android_devices(devices.into());
                w.set_android_serial(android.serial().into());
            }
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_select_device(move |serial| {
            state.lock().unwrap_or_else(|e| e.into_inner()).set_serial(serial.to_string());
            if let Some(w) = weak.upgrade() { w.set_android_status("Perangkat dipilih".into()); }
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_list(move |path| {
            let state = state.clone();
            let path = path.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let entries = state.lock().unwrap_or_else(|e| e.into_inner()).list(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_entries(entries.into()); w.set_android_path(path.into()); }
                });
            });
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_push(move |_serial, remote| {
            let selected = rfd::FileDialog::new().pick_files().unwrap_or_default();
            if selected.is_empty() { return; }
            let state = state.clone();
            let remote = remote.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let mut ok = 0usize;
                let mut errors = Vec::new();
                for path in selected {
                    match state.lock().unwrap_or_else(|e| e.into_inner()).push(&path, &remote) {
                        Ok(()) => ok += 1,
                        Err(e) => errors.push(e),
                    }
                }
                let msg = if errors.is_empty() { format!("{ok} file berhasil dikirim") } else { format!("{ok} berhasil, gagal: {}", errors.join("; ")) };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_status(msg.into()); }
                });
            });
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_pull(move |remote| {
            let destination = rfd::FileDialog::new().set_title("Pilih folder tujuan").pick_folder();
            let Some(destination) = destination else { return };
            let state = state.clone();
            let remote = remote.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = state.lock().unwrap_or_else(|e| e.into_inner()).pull(&remote, &destination);
                let message = match result {
                    Ok(()) => format!("File berhasil diambil ke {}", destination.display()),
                    Err(error) => format!("Gagal mengambil file: {error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_status(message.into()); }
                });
            });
        });
    }
    {
        let android = android_state.clone();
        let mirror = mirror_process.clone();
        let weak = win.as_weak();
        win.on_android_mirror(move || {
            let serial = android.lock().unwrap_or_else(|e| e.into_inner()).serial().to_string();
            let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).start(&serial);
            if let Some(w) = weak.upgrade() {
                w.set_android_status(match result {
                    Ok(message) => message.into(),
                    Err(error) => format!("Mirroring gagal: {error}").into(),
                });
            }
        });
    }
    {
        let mirror = mirror_process.clone();
        let weak = win.as_weak();
        win.on_android_stop_mirror(move || {
            let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).stop();
            if let Some(w) = weak.upgrade() {
                w.set_android_status(match result {
                    Ok(message) => message.into(),
                    Err(error) => format!("Gagal menghentikan mirroring: {error}").into(),
                });
            }
        });
    }

    let nudge_runtime_config = Rc::new(RefCell::new(cfg.nudge.clone()));
    {
        let weak = win.as_weak();
        let runtime_config = nudge_runtime_config.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_nudge(move |enabled, idle, message, emoji, mood, show, only_silent, until_move| {
            let idle = idle.to_string().trim().parse::<u32>().ok().filter(|v| (1..=240).contains(v));
            let show = show.to_string().trim().parse::<u32>().ok().filter(|v| (2..=30).contains(v)).map(|seconds| seconds * 1_000);
            let message = message.to_string().trim().to_string();
            let emoji = emoji.to_string().trim().to_string();
            let mood = mood.to_string().trim().to_ascii_lowercase();
            let allowed_moods = ["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"];
            let (Some(idle), Some(show)) = (idle, show) else {
                if let Some(w) = weak.upgrade() { w.set_nudge_save_status("Idle harus 1–240 menit dan durasi 2–30 detik.".into()); }
                return;
            };
            if message.is_empty() || message.chars().count() > 80 || emoji.is_empty() || emoji.chars().count() > 12 || !allowed_moods.contains(&mood.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_nudge_save_status("Pesan wajib 1–80 karakter, emoji 1–12 karakter, dan ekspresi harus valid.".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.nudge.enabled = enabled;
            next.nudge.idle_min = idle;
            next.nudge.message = message;
            next.nudge.emoji = emoji;
            next.nudge.mood = mood;
            next.nudge.show_ms = show;
            next.nudge.only_silent = only_silent;
            next.nudge.until_move = until_move;
            next.nudge.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime_config.borrow_mut() = next.nudge.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_nudge_enabled(next.nudge.enabled);
                        w.set_nudge_idle_min(next.nudge.idle_min.to_string().into());
                        w.set_nudge_message(next.nudge.message.clone().into());
                        w.set_nudge_emoji(next.nudge.emoji.clone().into());
                        w.set_nudge_mood(next.nudge.mood.clone().into());
                        w.set_nudge_show_ms((next.nudge.show_ms / 1_000).to_string().into());
                        w.set_nudge_only_silent(next.nudge.only_silent);
                        w.set_nudge_until_move(next.nudge.until_move);
                        w.set_nudge_save_status("Pengaturan sapaan otomatis tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_nudge_save_status(format!("Gagal menyimpan: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let greeting_voice_runtime = greeting_voice_runtime.clone();
        win.on_settings_save_greeting(move |enabled, mode, voice, custom_text| {
            let mode = mode.to_string();
            let voice = voice.to_string();
            let custom_text = custom_text.to_string().trim().to_string();
            if !["random", "auto", "halo", "morning", "noon", "afternoon", "evening", "custom"].contains(&mode.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Mode greeting tidak valid.".into()); }
                return;
            }
            if !["cewek", "cowok", "anime-cewek", "anime-cowok", "system"].contains(&voice.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Mode suara tidak valid.".into()); }
                return;
            }
            if custom_text.chars().count() > 120 || (mode == "custom" && custom_text.is_empty()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Teks greeting kustom wajib diisi dan maksimal 120 karakter.".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.greeting.enabled = enabled;
            next.greeting.mode = mode;
            next.greeting.voice = voice;
            next.greeting.custom_text = custom_text;
            next.greeting.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *greeting_voice_runtime.borrow_mut() = next.greeting.voice.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_greeting_enabled(next.greeting.enabled);
                        w.set_greeting_mode(next.greeting.mode.clone().into());
                        w.set_greeting_voice(next.greeting.voice.clone().into());
                        w.set_greeting_custom_text(next.greeting.custom_text.clone().into());
                        w.set_greeting_save_status("Preferensi greeting tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_greeting_save_status(format!("Gagal menyimpan greeting: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = conversation_runtime.clone();
        win.on_settings_save_conversation(move |enabled, on_play, on_track_change, on_pause, on_return, on_weather_change, on_rain_soon, on_low_battery, on_charging, return_after, cooldown| {
            let return_after = return_after.parse::<u32>().ok().filter(|v| (5..=120).contains(v));
            let cooldown = cooldown.parse::<u32>().ok().filter(|v| (1..=60).contains(v));
            let (Some(return_after), Some(cooldown)) = (return_after, cooldown) else {
                if let Some(w) = weak.upgrade() { w.set_conversation_save_status("Durasi kembali harus 5–120 menit dan jeda komentar 1–60 menit.".into()); }
                return;
            };
            let mut next = Config::load_readonly(&config_path);
            next.conversation_moments.enabled = enabled;
            next.conversation_moments.on_play = on_play;
            next.conversation_moments.on_track_change = on_track_change;
            next.conversation_moments.on_pause = on_pause;
            next.conversation_moments.on_return = on_return;
            next.conversation_moments.on_weather_change = on_weather_change;
            next.conversation_moments.on_rain_soon = on_rain_soon;
            next.conversation_moments.on_low_battery = on_low_battery;
            next.conversation_moments.on_charging = on_charging;
            next.conversation_moments.return_after_min = return_after;
            next.conversation_moments.cooldown_min = cooldown;
            next.conversation_moments.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime.borrow_mut() = next.conversation_moments.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_conversation_enabled(next.conversation_moments.enabled);
                        w.set_conversation_on_play(next.conversation_moments.on_play);
                        w.set_conversation_on_track_change(next.conversation_moments.on_track_change);
                        w.set_conversation_on_pause(next.conversation_moments.on_pause);
                        w.set_conversation_on_return(next.conversation_moments.on_return);
                        w.set_conversation_on_weather_change(next.conversation_moments.on_weather_change);
                        w.set_conversation_on_rain_soon(next.conversation_moments.on_rain_soon);
                        w.set_conversation_on_low_battery(next.conversation_moments.on_low_battery);
                        w.set_conversation_on_charging(next.conversation_moments.on_charging);
                        w.set_conversation_return_after_min(return_after.to_string().into());
                        w.set_conversation_cooldown_min(cooldown.to_string().into());
                        w.set_conversation_save_status("Preferensi Conversation Moments tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_conversation_save_status(format!("Gagal menyimpan Conversation Moments: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = focus_companion_runtime.clone();
        win.on_settings_save_focus_companion(move |enabled, on_start, on_halfway, on_focus_complete, on_break_end| {
            let mut next = Config::load_readonly(&config_path);
            next.focus_companion.enabled = enabled;
            next.focus_companion.on_start = on_start;
            next.focus_companion.on_halfway = on_halfway;
            next.focus_companion.on_focus_complete = on_focus_complete;
            next.focus_companion.on_break_end = on_break_end;
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime.borrow_mut() = next.focus_companion.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_focus_companion_enabled(next.focus_companion.enabled);
                        w.set_focus_companion_on_start(next.focus_companion.on_start);
                        w.set_focus_companion_on_halfway(next.focus_companion.on_halfway);
                        w.set_focus_companion_on_focus_complete(next.focus_companion.on_focus_complete);
                        w.set_focus_companion_on_break_end(next.focus_companion.on_break_end);
                        w.set_focus_companion_save_status("Preferensi Focus Companion tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_focus_companion_save_status(format!("Gagal menyimpan Focus Companion: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let shell = shell.clone();
        let timer = Timer::default();
        win.on_settings_test_greeting(move |mode, custom_text, voice_preset| {
            let Some(w) = weak.upgrade() else { return };
            let mode = mode.to_string();
            let custom_text = custom_text.to_string().trim().to_string();
            let voice_preset = voice_preset.to_string();
            if !["cewek", "cowok", "anime-cewek", "anime-cowok", "system"].contains(&voice_preset.as_str()) {
                w.set_greeting_save_status("Preset suara tidak valid.".into());
                return;
            }
            let (hour, _, _, _, _) = platform::local_time();
            let greeting = match speech::greeting_content(&mode, &custom_text, hour) {
                Some(greeting) => greeting,
                None => {
                    w.set_greeting_save_status("Mode greeting tidak dikenal atau teks kustom kosong.".into());
                    return;
                }
            };
            // Pratinjau memakai preset yang sedang dipilih di UI, meskipun belum disimpan.
            let selected_voice = voice_preset;
            let speech_voice = selected_voice.clone();
            let greeting_duration_ms = speech::estimated_duration_ms(&greeting.spoken);
            let _ = speech::speak(greeting.spoken.clone(), speech_voice);
            w.set_greeting_message(greeting.message.into());
            w.set_greeting_detail(greeting.detail.into());
            // Greeting memakai layer tersendiri agar tidak menimpa isi notifikasi Nudge.
            if w.get_dock() < 2 {
                w.set_greeting_speech_visible(false);
                w.set_nudge_visible(false);
                show_greeting_inline(weak.clone(), shell.clone());
            } else {
                w.set_greeting_inline_visible(false);
                w.set_nudge_visible(false);
                w.set_greeting_speech_visible(true);
                shell.place(&w, COLLAPSED);
            }
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, greeting_duration_ms, Instant::now());
            }
            w.set_greeting_save_status("Pratinjau teks dan audio diputar. Pengaturan belum disimpan.".into());
            let weak = weak.clone();
            let shell_hide = shell.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(greeting_duration_ms + 600), move || {
                if let Some(w) = weak.upgrade() {
                    let was_inline = w.get_greeting_inline_visible() && !w.get_expanded();
                    w.set_greeting_inline_visible(false);
                    if was_inline {
                        shrink_window_synced(weak.clone(), shell_hide.clone());
                    }
                    w.set_greeting_speech_visible(false);
                    w.set_nudge_visible(false);
                }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let timer = Timer::default();
        win.on_settings_test_conversation(move |message, detail| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let detail = detail.to_string().trim().to_string();
            if message.is_empty() || message.chars().count() > 100 || detail.chars().count() > 100 {
                w.set_conversation_save_status("Pesan pratinjau wajib diisi dan maksimal 100 karakter.".into());
                return;
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau Conversation Moments", detail).into());
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_conversation_save_status("Pratinjau ditampilkan tanpa menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(5_000), move || {
                if let Some(w) = weak.upgrade() { w.set_nudge_visible(false); }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let timer = Timer::default();
        win.on_settings_test_focus_companion(move |message, detail| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let detail = detail.to_string().trim().to_string();
            if message.is_empty() || message.chars().count() > 100 || detail.chars().count() > 100 {
                w.set_focus_companion_save_status("Pesan pratinjau wajib diisi dan maksimal 100 karakter.".into());
                return;
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau Focus Companion", detail).into());
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_focus_companion_save_status("Pratinjau ditampilkan tanpa menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(5_000), move || {
                if let Some(w) = weak.upgrade() { w.set_nudge_visible(false); }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let timer = Timer::default();
        win.on_settings_test_nudge(move |message, emoji, mood, show, _until_move| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let emoji = emoji.to_string().trim().to_string();
            let mood = mood.to_string();
            let Some(show_ms) = show.to_string().parse::<u32>().ok().filter(|v| (2_000..=30_000).contains(v)) else {
                w.set_nudge_save_status("Durasi pratinjau harus 2000–30000 ms.".into());
                return;
            };
            if message.is_empty() || message.chars().count() > 80 || emoji.chars().count() > 8
                || !["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"].contains(&mood.as_str()) {
                w.set_nudge_save_status("Periksa pesan (1–80 karakter), emoji (maks. 8), dan ekspresi.".into());
                return;
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau sapaan otomatis", emoji).into());
            let eye_state = eye.borrow().clone();
            if let Some(eye_state) = eye_state {
                eye_state.borrow_mut().mood.flash(Mood::parse(&mood).unwrap_or(Mood::Sad), u64::from(show_ms) + 5_000, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_nudge_save_status("Pratinjau memakai nilai yang sedang diedit; belum menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(show_ms as u64), move || {
                if let Some(w) = weak.upgrade() { w.set_nudge_visible(false); }
            });
        });
    }

    {
        let weak = win.as_weak();
        let location = weather_location.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_weather(move |city, lat, lon| {
            let city = city.to_string().trim().to_string();
            let Some(lat) = lat.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-90.0..=90.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Latitude harus angka antara -90 dan 90.".into()); }
                return;
            };
            let Some(lon) = lon.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-180.0..=180.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Longitude harus angka antara -180 dan 180.".into()); }
                return;
            };
            if city.is_empty() || city.len() > 80 {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Nama kota wajib diisi (maksimal 80 karakter).".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.weather.city = city.clone();
            next.weather.lat = lat;
            next.weather.lon = lon;
            match next.save(&config_path) {
                Ok(()) => {

                    *location.borrow_mut() = (lat, lon, city.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_weather_city(city.clone().into());
                        w.set_weather_lat(lat.to_string().into());
                        w.set_weather_lon(lon.to_string().into());
                        w.set_weather_save_status("Lokasi tersimpan. Memperbarui cuaca...".into());
                        w.invoke_weather_refresh();
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_weather_save_status(format!("Gagal menyimpan: {error}").into());
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let eye_runtime = appearance_eye.clone();
        let sleep_after_s = sleep_after_s.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_appearance(move |theme, eye, notch, mood, accent, custom_bg, custom_text, dark, light, scale| {
            let mut next = Config::load_readonly(&config_path);
            let scale = f64::from(scale);
            if !(0.5..=1.5).contains(&scale) {
                if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Skala notch harus berada di antara 0.50× dan 1.50×".into()); }
                return;
            }
            next.scale = scale;
            next.appearance.theme = theme.to_string();
            next.appearance.eye_theme = eye.to_string();
            next.appearance.notch_theme = notch.to_string();
            next.appearance.default_mood = mood.to_string();
            next.appearance.accent = accent.to_string();
            next.appearance.background = custom_bg.to_string();
            next.appearance.text = custom_text.to_string();
            next.appearance.dark_bg = dark.to_string();
            next.appearance.light_bg = light.to_string();
            next.normalize();
            if !["auto", "dark", "light", "custom"].contains(&next.appearance.theme.as_str()) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Tema harus auto, dark, light, atau custom".into()); } return; }
            let valid_hex = |v: &str| v.is_empty() || (v.len() == 7 && v.starts_with("#") && v[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            if !valid_hex(&next.appearance.accent) || !valid_hex(&next.appearance.background) || !valid_hex(&next.appearance.text) || !valid_hex(&next.appearance.dark_bg) || !valid_hex(&next.appearance.light_bg) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Warna harus memakai format HEX #RRGGBB".into()); } return; }
            match next.save(&config_path) {
                Ok(()) => {

                    let palette = Palette::from_appearance(&next.appearance, platform::os_light());
                    *shell.theme.borrow_mut() = next.appearance.notch_theme.clone();
                    *shell.body.borrow_mut() = palette.body;
                    *shell.scale.borrow_mut() = scale as f32;
                    shell.memo.set((None, 0));
                    if let Some(w) = weak.upgrade() {
                        apply_theme(&w, &palette);
                        w.set_ui_scale(scale as f32);
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        if let Some(eye) = eye_runtime.borrow().as_ref() {
                            let mut eye = eye.borrow_mut();
                            let skin = eyes::skin_index(&next.appearance.eye_theme);
                            let (renderer, canvas_size) = eye_renderer_for_dock(&next.appearance.eye_theme, w.get_dock());
                            eye.renderer = renderer;
                            eye.canvas_size = canvas_size;
                            eye.skin = skin;
                            let mood = Mood::parse(&next.appearance.default_mood).unwrap_or(Mood::Normal);
                            eye.mood = MoodState::new(mood, next.timing.sleep_after_s, Instant::now());
                            sleep_after_s.set(next.timing.sleep_after_s);
                            eye.cur = mood;
                            eye.cur_since = 0.0;
                            eye.morph = Morph::new(Face::for_mood(mood, skin));
                            eye.last_key = None;
                        }
                        w.set_appearance_theme(next.appearance.theme.clone().into());
                        w.set_appearance_eye_theme(next.appearance.eye_theme.clone().into());
                        w.set_appearance_notch_theme(next.appearance.notch_theme.clone().into());
                        w.set_appearance_mood(next.appearance.default_mood.clone().into());
                        w.set_appearance_accent(next.appearance.accent.clone().into());
                        w.set_appearance_custom_bg(next.appearance.background.clone().into());
                        w.set_appearance_custom_text(next.appearance.text.clone().into());
                        w.set_appearance_dark_bg(next.appearance.dark_bg.clone().into());
                        w.set_appearance_light_bg(next.appearance.light_bg.clone().into());
                        w.set_behavior_scale(format!("{scale:.2}").into());
                        w.set_appearance_save_status("Tampilan dan ukuran tersimpan dan langsung diterapkan.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_appearance_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let eye_runtime = appearance_eye.clone();
        let sleep_after_s = sleep_after_s.clone();
        let preview_config = cfg.clone();
        win.on_settings_preview_appearance(move |theme, eye, notch, mood, accent, custom_bg, custom_text, dark, light, scale| {
            let mut next = preview_config.clone();
            next.appearance.theme = theme.to_string();
            next.appearance.eye_theme = eye.to_string();
            next.appearance.notch_theme = notch.to_string();
            next.appearance.default_mood = mood.to_string();
            next.appearance.accent = accent.to_string();
            next.appearance.background = custom_bg.to_string();
            next.appearance.text = custom_text.to_string();
            next.appearance.dark_bg = dark.to_string();
            next.appearance.light_bg = light.to_string();
            let scale = f64::from(scale);
            if !(0.5..=1.5).contains(&scale) {
                return;
            }
            next.scale = scale;
            next.normalize();
            if !["auto", "dark", "light", "custom"].contains(&next.appearance.theme.as_str()) {
                return;
            }
            let valid_hex = |v: &str| v.is_empty() || (v.len() == 7 && v.starts_with("#") && v[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            if !valid_hex(&next.appearance.accent) || !valid_hex(&next.appearance.background) || !valid_hex(&next.appearance.text) || !valid_hex(&next.appearance.dark_bg) || !valid_hex(&next.appearance.light_bg) {
                return;
            }

            // Preview only: update the running notch and the settings mirror, but never write config.toml.
            let palette = Palette::from_appearance(&next.appearance, platform::os_light());
            *shell.theme.borrow_mut() = next.appearance.notch_theme.clone();
            *shell.body.borrow_mut() = palette.body;
            *shell.scale.borrow_mut() = scale as f32;
            shell.memo.set((None, 0));
            if let Some(w) = weak.upgrade() {
                apply_theme(&w, &palette);
                w.set_ui_scale(scale as f32);
                let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                shell.place(&w, size);

                if let Some(eye) = eye_runtime.borrow().as_ref() {
                    let mut eye = eye.borrow_mut();
                    let skin = eyes::skin_index(&next.appearance.eye_theme);
                    let (renderer, canvas_size) = eye_renderer_for_dock(&next.appearance.eye_theme, w.get_dock());
                    eye.renderer = renderer;
                    eye.canvas_size = canvas_size;
                    eye.skin = skin;
                    let mood = Mood::parse(&next.appearance.default_mood).unwrap_or(Mood::Normal);
                    eye.mood = MoodState::new(mood, next.timing.sleep_after_s, Instant::now());
                    sleep_after_s.set(next.timing.sleep_after_s);
                    eye.cur = mood;
                    eye.cur_since = 0.0;
                    eye.morph = Morph::new(Face::for_mood(mood, skin));
                    eye.last_key = None;
                }

                w.set_appearance_theme(next.appearance.theme.clone().into());
                w.set_appearance_eye_theme(next.appearance.eye_theme.clone().into());
                w.set_appearance_notch_theme(next.appearance.notch_theme.clone().into());
                w.set_appearance_mood(next.appearance.default_mood.clone().into());
                w.set_appearance_accent(next.appearance.accent.clone().into());
                w.set_appearance_custom_bg(next.appearance.background.clone().into());
                w.set_appearance_custom_text(next.appearance.text.clone().into());
                w.set_appearance_dark_bg(next.appearance.dark_bg.clone().into());
                w.set_appearance_light_bg(next.appearance.light_bg.clone().into());
                w.set_behavior_scale(format!("{scale:.2}").into());
                w.set_appearance_save_status("Pratinjau aktif — belum disimpan.".into());
            }
        });
    }

    {
        let weak = win.as_weak();
        win.on_settings_test_dock_play(move || {
            let Some(w) = weak.upgrade() else { return };
            if !w.get_dock_play_enabled() {
                w.set_motion_save_status("Aktifkan Bottom Play / Side Play terlebih dahulu.".into());
                return;
            }
            if !matches!(w.get_dock(), 2 | 3 | 4) {
                w.set_motion_save_status("Tes tersedia saat notch berada di dock bawah, kiri, atau kanan.".into());
                return;
            }
            w.set_play_test_request(true);
            w.set_motion_save_status("Tes liquid siap. Tutup pengaturan agar animasi dimulai.".into());
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = motion_runtime.clone();
        win.on_settings_save_motion(move |enabled, interval, maximum, dock_play_enabled| {
            let interval = interval.parse::<u32>().ok().filter(|v| (60..=3600).contains(v));
            let maximum = maximum.parse::<u32>().ok().filter(|v| (1..=10).contains(v));
            let (Some(interval), Some(maximum)) = (interval, maximum) else {
                if let Some(w) = weak.upgrade() {
                    w.set_motion_save_status("Interval harus 60–3600 detik dan batas perpindahan 1–10.".into());
                }
                return;
            };
            let mut next = Config::load_readonly(&config_path);
            next.motion.enabled = enabled;
            next.motion.interval_secs = interval;
            next.motion.max_moves = maximum;
            next.motion.dock_play_enabled = dock_play_enabled;
            match next.save(&config_path) {
                Ok(()) => {

                    runtime.0.set(enabled);
                    runtime.1.set(interval);
                    runtime.2.set(maximum);
                    if let Some(w) = weak.upgrade() {
                        w.set_motion_enabled(enabled);
                        w.set_dock_play_enabled(dock_play_enabled);
                        if !dock_play_enabled { w.set_play_test_request(false); }
                        w.set_motion_interval(interval.to_string().into());
                        w.set_motion_max_moves(maximum.to_string().into());
                        w.set_motion_save_status(if dock_play_enabled && enabled {
                            "Pengaturan tersimpan. Liquid Movement dan Bottom/Side Play langsung diterapkan.".into()
                        } else if enabled {
                            "Liquid Movement langsung diterapkan. Bottom/Side Play dinonaktifkan.".into()
                        } else {
                            "Liquid Movement dan Bottom/Side Play dinonaktifkan.".into()
                        });
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_motion_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let open_ms = open_ms.clone();
        let close_ms = close_ms.clone();
        let sleep_after_s = sleep_after_s.clone();
        let hide_fullscreen = hide_fullscreen.clone();
        let eye_runtime = appearance_eye.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_behavior(move |scale, hover_open, hover_close, sleep_after, hide_fullscreen_enabled| {
            let mut next = Config::load_readonly(&config_path);
            let scale = scale.parse::<f64>().ok().filter(|v| (0.5..=1.5).contains(v));
            let hover_open = hover_open.parse::<u32>().ok().filter(|v| *v <= 2000);
            let hover_close = hover_close.parse::<u32>().ok().filter(|v| *v <= 5000);
            let sleep_after = sleep_after.parse::<u32>().ok().filter(|v| (10..=3600).contains(v));
            let (Some(scale), Some(hover_open), Some(hover_close), Some(sleep_after)) =
                (scale, hover_open, hover_close, sleep_after) else {
                if let Some(w) = weak.upgrade() {
                    w.set_behavior_save_status("Skala 0.5–1.5, hover buka 0–2000 ms, hover tutup 0–5000 ms, idle 10–3600 detik.".into());
                }
                return;
            };
            next.scale = scale;
            next.timing.hover_open_ms = hover_open;
            next.timing.hover_close_ms = hover_close;
            next.timing.sleep_after_s = sleep_after;
            next.hide_fullscreen = hide_fullscreen_enabled;
            match next.save(&config_path) {
                Ok(()) => {

                    open_ms.set(u64::from(hover_open));
                    close_ms.set(u64::from(hover_close));
                    sleep_after_s.set(sleep_after);
                    hide_fullscreen.set(hide_fullscreen_enabled);
                    *shell.scale.borrow_mut() = scale as f32;
                    shell.memo.set((None, 0));
                    if let Some(eye) = eye_runtime.borrow().as_ref() {
                        eye.borrow_mut().mood.set_sleep_after(sleep_after);
                    }
                    if let Some(w) = weak.upgrade() {
                        w.set_ui_scale(scale as f32);
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        w.set_behavior_scale(format!("{scale:.2}").into());
                        w.set_behavior_hover_open(hover_open.to_string().into());
                        w.set_behavior_hover_close(hover_close.to_string().into());
                        w.set_behavior_sleep_after(sleep_after.to_string().into());
                        w.set_behavior_hide_fullscreen(hide_fullscreen_enabled);
                        w.set_behavior_save_status("Perilaku notch tersimpan dan langsung diterapkan.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_behavior_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let prefs = prayer_prefs.clone();
        let pomo = pomo.clone();
        let fetch_prayer = fetch_prayer.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_routines(move |mode, remind, minutes, focus, rest, long, rounds, province, city| {
            let mut next = Config::load_readonly(&config_path);
            let mode = mode.to_string();
            if !["tone", "adzan", "silent"].contains(&mode.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Mode shalat harus tone, adzan, atau silent".into()); } return; }
            let remind = remind.to_string();
            if !["true", "false"].contains(&remind.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Pengingat harus true atau false".into()); } return; }
            let parse = |v: slint::SharedString, min: u32, max: u32| -> Option<u32> { let n = v.parse::<u32>().ok()?; if (min..=max).contains(&n) { Some(n) } else { None } };
            let Some(minutes) = parse(minutes, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Menit pengingat harus 1–120".into()); } return; };
            let Some(focus) = parse(focus, 1, 180) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Fokus Pomodoro harus 1–180 menit".into()); } return; };
            let Some(rest) = parse(rest, 1, 60) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat harus 1–60 menit".into()); } return; };
            let Some(long) = parse(long, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat panjang harus 1–120 menit".into()); } return; };
            let Some(rounds) = parse(rounds, 1, 12) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Jumlah ronde harus 1–12".into()); } return; };
            let province = province.to_string().trim().to_string();
            let city = city.to_string().trim().to_string();
            if province.is_empty() || province.chars().count() > 80 || city.is_empty() || city.chars().count() > 100 {
                if let Some(w) = weak.upgrade() { w.set_settings_save_status("Provinsi wajib diisi (maks. 80 karakter) dan kabupaten/kota wajib diisi (maks. 100 karakter).".into()); }
                return;
            }
            next.prayer.provinsi = province;
            next.prayer.kabkota = city;
            next.prayer.mode = mode; next.prayer.remind_before = remind == "true"; next.prayer.remind_min = minutes;
            next.pomodoro.focus_min = focus; next.pomodoro.break_min = rest; next.pomodoro.long_break_min = long; next.pomodoro.rounds = rounds;
            match next.save(&config_path) {
                Ok(()) => {

                    *prefs.borrow_mut() = next.prayer.clone();
                    fetch_prayer(true);
                    let mut timer = pomo.borrow_mut();
                    timer.update_config(&next.pomodoro, Instant::now());
                    if let Some(w) = weak.upgrade() {
                        show_pomo(&w, &timer);
                        w.set_prayer_province(next.prayer.provinsi.clone().into());
                        w.set_prayer_city(next.prayer.kabkota.clone().into());
                        w.set_prayer_mode(next.prayer.mode.clone().into());
                        w.set_prayer_remind_before(next.prayer.remind_before.to_string().into());
                        w.set_prayer_remind_min(next.prayer.remind_min.to_string().into());
                        w.set_pomodoro_focus(next.pomodoro.focus_min.to_string().into());
                        w.set_pomodoro_break(next.pomodoro.break_min.to_string().into());
                        w.set_pomodoro_long_break(next.pomodoro.long_break_min.to_string().into());
                        w.set_pomodoro_rounds(next.pomodoro.rounds.to_string().into());
                        w.set_settings_save_status("Tersimpan dan langsung diterapkan ke timer shalat serta Pomodoro.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_settings_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    // --- Global shortcut Windows: diproses di thread UI agar aman untuk state Slint ---
    let hotkey_poll_timer = Timer::default();
    {
        let (expand, collapse, pomo) = (expand.clone(), collapse.clone(), pomo.clone());
        let open_settings = open_settings.clone();
        let weak = win.as_weak();
        hotkey_poll_timer.start(TimerMode::Repeated, Duration::from_millis(60), move || {
            while let Ok(action) = hotkey_rx.try_recv() {
                match action {
                    hotkeys::Action::ToggleHub => {
                        if let Some(w) = weak.upgrade() {
                            if w.get_expanded() { collapse(); } else { expand(); }
                        }
                    }
                    hotkeys::Action::TogglePomodoro => {
                        let mut p = pomo.borrow_mut();
                        p.toggle(Instant::now());
                        if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
                    }
                    hotkeys::Action::OpenPrayer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("prayer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::ColorPicker => {
                        colorpick::toggle();
                    }
                    hotkeys::Action::OpenAndroid => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("android".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenTransfer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("transfer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenConvert => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("convert".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenCompress => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("compress".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenSettings => open_settings(),
                    hotkeys::Action::ShortcutRegistrationFailed => {
                        if let Some(w) = weak.upgrade() {
                            w.set_settings_save_status("Windows menolak salah satu shortcut karena sedang dipakai aplikasi lain atau sistem. Ganti kombinasinya.".into());
                        }
                    }
                }
            }
        });
    }

    // --- Mata: 8 mood x 20 skin, dirender Rust (notch-core/src/eyes.rs) ---
    let started = Instant::now();
    let skin = eyes::skin_index(&cfg.appearance.eye_theme);
    let default_mood = Mood::parse(&cfg.appearance.default_mood).unwrap_or(Mood::Normal);
    let (initial_eye_renderer, initial_eye_size) = eye_renderer_for_dock(&cfg.appearance.eye_theme, win.get_dock());
    let eye = Rc::new(RefCell::new(EyeCtx {
        renderer: initial_eye_renderer,
        canvas_size: initial_eye_size,
        skin,
        mood: MoodState::new(default_mood, cfg.timing.sleep_after_s, started),
        cur: default_mood,
        cur_since: 0.0,
        morph: Morph::new(Face::for_mood(default_mood, skin)),
        look: (0.0, 0.0),
        look_target: (0.0, 0.0),
        last_frame_ts: 0.0,
        last_cursor: (i32::MIN, i32::MIN),
        last_move: 0.0,
        next_random_look: 0.0,
        blink_until: 0.0,
        next_blink: 2.5,
        last_key: None,
        rng: Rng::new(),
    }));
    *appearance_eye.borrow_mut() = Some(eye.clone());

    // Sapaan otomatis saat pointer idle. Satu sapaan per periode diam; gerakan baru mengaktifkan periode berikutnya.
    let nudge_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        let config = nudge_runtime_config.clone();
        // (aktif, waktu gerak terakhir, waktu pesan ditampilkan, sedang menampilkan kartu kembali)
        let state = Rc::new(RefCell::new((false, 0.0_f32, 0.0_f32, false)));
        let state_tick = state.clone();
        nudge_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let Some(w) = weak.upgrade() else { return };
            let elapsed = started.elapsed().as_secs_f32();
            let last_move = eye.borrow().last_move;
            let cfg = config.borrow();
            let mut state = state_tick.borrow_mut();
            if last_move > state.1 + 0.001 {
                // Sapaan balik ditampilkan setiap kali periode diam berakhir dengan gerakan,
                // termasuk bila kartu awal sudah tertutup otomatis (until_move = false).
                let was_nudge_shown = state.0 && !state.3;
                let was_return_card = state.3;
                state.0 = false;
                state.1 = last_move;
                state.2 = 0.0;
                state.3 = false;
                if was_nudge_shown && !was_return_card && cfg.enabled {
                    state.0 = true;
                    state.2 = elapsed;
                    state.3 = true;
                    w.set_nudge_message("Akhirnya kamu kembali!".into());
                    w.set_nudge_detail("Senang melihatmu kembali".into());
                    eye.borrow_mut().mood.flash(Mood::Happy, u64::from(cfg.show_ms) + 5_000, Instant::now());
                    w.set_nudge_visible(true);
                } else {
                    w.set_nudge_visible(false);
                }
            }
            if !cfg.enabled {
                w.set_nudge_visible(false);
                state.0 = false;
                state.3 = false;
                return;
            }
            if state.0 {
                // Sapaan awal bertahan sampai pointer bergerak. Kartu "kembali" tetap
                // memiliki durasi sendiri, lalu menunggu periode idle berikutnya.
                if (state.3 || !cfg.until_move)
                    && elapsed - state.2 >= cfg.show_ms as f32 / 1000.0
                {
                    w.set_nudge_visible(false);
                    state.3 = false;
                }
                return;
            }
            let idle_secs = (elapsed - last_move).max(0.0);
            if idle_secs < cfg.idle_min as f32 * 60.0 { return; }
            if cfg.only_silent && w.get_media_playing() { return; }
            if w.get_expanded() || w.get_reminder_alert_visible() || w.get_adhan_active() || w.get_meeting_active() || w.get_meeting_finished() != "" { return; }
            state.0 = true;
            state.2 = elapsed;
            w.set_nudge_message(cfg.message.clone().into());
            w.set_nudge_detail(format!("{}  ·  Pointer diam {} menit", cfg.emoji, (idle_secs / 60.0).floor() as u32).into());
            eye.borrow_mut().mood.flash(Mood::parse(&cfg.mood).unwrap_or(Mood::Sad), u64::from(cfg.show_ms) + 5_000, Instant::now());
            w.set_nudge_visible(true);
        });
    }

    // Klik / dobel klik pada notch (Tauri: klik -> happy atau marah bila 5x dalam 2 dtk; dobel -> love)
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.click(Instant::now());
        });
    }
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_double_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.double_click(Instant::now());
        });
    }

    // Mood dasar (tidur saat idle/malam, musik, baterai) dinilai ulang tiap 10 dtk (mood.ts)
    let base_timer = Timer::default();
    {
        let eye = eye.clone();
        let keep_awake = keep_awake.clone();
        let weak = win.as_weak();
        base_timer.start(TimerMode::Repeated, Duration::from_secs(10), move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            let mut e = eye.borrow_mut();
            e.mood.keep_awake = keep_awake.load(Ordering::Relaxed);
            e.mood.update_base(Instant::now(), platform::local_time().0);
        });
    }

    // Baterai dibaca tiap 30 dtk untuk mood (mengisi -> love, lemah -> mengantuk)
    let mood_battery_timer = Timer::default();
    {
        let eye = eye.clone();
        let battery = battery.clone();
        let weak = win.as_weak();
        let tick = move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            if let Reading::Present(b) = battery.read() {
                eye.borrow_mut().mood.on_battery(b.percent, b.charging, Instant::now(), platform::local_time().0);
            }
        };
        tick();
        mood_battery_timer.start(TimerMode::Repeated, Duration::from_secs(30), tick);
    }

    // Tick mata ~60 fps agar tatapan dapat mengikuti pointer dengan interpolasi halus.
    let eye_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        eye_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let now = Instant::now();
            let ts = started.elapsed().as_secs_f32();
            let hour = platform::local_time().0;
            let mut guard = eye.borrow_mut();
            let e = &mut *guard;

            // Tatapan: ikuti kursor; bila diam > 5 dtk, melirik acak tiap 2,5 dtk (notch-face.ts)
            if let Some((cx, cy)) = platform::cursor_pos() {
                let (px, py) = e.last_cursor;
                if (cx as f32 - px as f32).hypot(cy as f32 - py as f32) > 2.0 {
                    e.last_move = ts;
                    // Aktivitas kursor tidak boleh mengubah ekspresi ketika Mode Rapat aktif.
                    if !w.get_meeting_active() {
                        e.mood.activity(now, hour);
                    }
                }
                if (cx, cy) != e.last_cursor {
                    e.last_cursor = (cx, cy);
                    // relatif terhadap notch (bukan tengah layar), supaya benar juga untuk posisi kustom
                    let (wp, ws) = (w.window().position(), w.window().size());
                    let mid_x = wp.x + ws.width as i32 / 2;
                    e.look_target = eyes::look_offset((cx - mid_x) as f32, (cy - wp.y) as f32);
                }
            }
            if ts - e.last_move > 5.0 && ts >= e.next_random_look {
                e.next_random_look = ts + 2.5;
                e.look_target = eyes::look_offset((e.rng.next() - 0.5) * 300.0, (e.rng.next() - 0.5) * 60.0);
            }

            // Smooth-follow: filter eksponensial yang stabil terhadap variasi frame time.
            // Tau 85 ms menjaga respons terasa cepat, tetapi menghilangkan gerak patah saat pointer berubah.
            let frame_dt = if e.last_frame_ts > 0.0 {
                (ts - e.last_frame_ts).clamp(0.0, 0.1)
            } else {
                1.0 / 60.0
            };
            e.last_frame_ts = ts;
            let follow = 1.0 - (-frame_dt / 0.085).exp();
            e.look.0 += (e.look_target.0 - e.look.0) * follow;
            e.look.1 += (e.look_target.1 - e.look.1) * follow;
            if (e.look_target.0 - e.look.0).abs() < 0.015 {
                e.look.0 = e.look_target.0;
            }
            if (e.look_target.1 - e.look.1).abs() < 0.015 {
                e.look.1 = e.look_target.1;
            }

            // Mood: transisi 250 ms ke mood baru
            let m = e.mood.current(now);
            if m != e.cur {
                e.cur = m;
                e.cur_since = ts;
                let face = Face::for_mood(m, e.skin);
                e.morph.set_target(ts, face);
            }
            w.set_eyes_mood(e.cur.name().into());

            // Kedip acak 2,5-6 dtk (mengantuk 1,5-5 dtk); mata "love" tidak berkedip; kedip 130 ms
            if ts >= e.next_blink {
                if e.cur != Mood::Love {
                    e.blink_until = ts + 0.13;
                }
                let base = if e.cur == Mood::Sleepy { 1.5 } else { 2.5 };
                e.next_blink = ts + base + e.rng.next() * 3.5;
            }
            let blink = ts < e.blink_until;

            if w.get_expanded() {
                return; // mata tersembunyi saat hub terbuka: tidak perlu menggambar
            }

            let (canvas_w, canvas_h) = match w.get_dock() {
                0 => (EYES_W, EYES_TOP_H),
                3 | 4 => (EYES_H, EYES_W),
                _ => (EYES_W, EYES_H),
            };
            if e.canvas_size != (canvas_w, canvas_h) {
                let skin_name = e.renderer.skin_name();
                e.renderer = if matches!(w.get_dock(), 3 | 4) {
                    EyeRenderer::new_vertical(skin_name, canvas_w, canvas_h)
                } else {
                    EyeRenderer::new(skin_name, canvas_w, canvas_h)
                };
                e.canvas_size = (canvas_w, canvas_h);
                e.last_key = None;
            }
            let sf = w.window().scale_factor();
            let anim = Anim { mood: e.cur, t: ts, t_mood: ts - e.cur_since, look: e.look, blink };
            let key = (
                (e.look.0 * 8.0) as i32,
                (e.look.1 * 8.0) as i32,
                blink,
                e.cur as usize,
                eyes::frame_key(&anim, e.renderer.skin_name(), !e.morph.settled(ts)),
                sf.to_bits(),
            );
            if e.last_key == Some(key) {
                return;
            }
            e.last_key = Some(key);
            let face = e.morph.current(ts);
            let buf = e.renderer.render(&face, &anim, sf);
            let (pw, ph) = e.renderer.size_px(sf);
            w.set_eyes_image(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, pw, ph)));
        });
    }

    // Bottom Play / Side Play native: mesin gerak organik berbasis waktu dan easing.
    // Semua transform tetap di dalam Window Slint; tidak menggeser native window.
    let dock_play_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        let battery = battery.clone();
        // Sama dengan Tauri: gerakan pertama boleh muncul setelah jeda 4 detik.
        let mut next_at = Instant::now() + Duration::from_secs(4);
        // source: 0=random, 1=Liquid terjadwal, 2=tes manual.
        let mut active: Option<(u8, Instant, Duration, f32, u8)> = None;
        let mut last_choice: Option<u8> = None;
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        // Setiap tiga menit jalankan tiga rangkaian Liquid, mengikuti jadwal Tauri.
        let mut next_liquid_at = Instant::now() + Duration::from_secs(180);
        let mut liquid_repetitions_left = 0u8;
        let mut next_liquid_run_at = Instant::now();
        // Pegas semi-implisit untuk perpindahan dan deformasi badan.
        // [posisi, kecepatan] sehingga target dapat berubah tanpa transform meloncat.
        let mut spring_offset = (0.0f32, 0.0f32);
        let mut spring_sx = (0.0f32, 0.0f32);
        let mut spring_sy = (0.0f32, 0.0f32);
        let mut last_frame = Instant::now();

        dock_play_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let now = Instant::now();
            let dock = w.get_dock();
            let supported = matches!(dock, 2 | 3 | 4);
            let blocked = !w.get_dock_play_enabled()
                || !supported
                || w.get_expanded()
                || w.get_play_interaction_active()
                || w.get_nudge_visible()
                || w.get_reminder_alert_visible()
                || !w.get_meeting_finished().is_empty()
                || w.get_meeting_active()
                || w.get_adhan_active();

            if blocked {
                // Liquid terjadwal/tes manual yang terpotong tetap antre, seperti catch(CANCEL) di Tauri.
                if let Some((kind, _, _, _, source)) = active.take() {
                    if kind == 0 {
                        if source == 1 {
                            liquid_repetitions_left = liquid_repetitions_left.saturating_add(1).min(3);
                            next_liquid_run_at = now + Duration::from_millis(2_500);
                        } else if source == 2 {
                            w.set_play_test_request(true);
                        }
                    }
                }
                // Hover, panel, meeting, Adzan, atau saklar mati menghentikan trik baru,
                // tetapi badan kembali netral dengan redaman agar tidak tersentak.
                let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
                last_frame = now;
                let steps = (dt / 0.008).ceil().clamp(1.0, 8.0) as u32;
                let h = dt / steps as f32;
                for _ in 0..steps {
                    spring_offset.1 += (-105.0 * spring_offset.0 - 17.0 * spring_offset.1) * h;
                    spring_offset.0 += spring_offset.1 * h;
                    spring_sx.1 += (-165.0 * spring_sx.0 - 20.0 * spring_sx.1) * h;
                    spring_sx.0 += spring_sx.1 * h;
                    spring_sy.1 += (-165.0 * spring_sy.0 - 20.0 * spring_sy.1) * h;
                    spring_sy.0 += spring_sy.1 * h;
                }
                let ox = spring_offset.0.clamp(-90.0, 90.0);
                let sx = (1.0 + spring_sx.0).clamp(0.78, 1.25);
                let sy = (1.0 + spring_sy.0).clamp(0.78, 1.45);
                let side = dock == 3 || dock == 4;
                w.set_play_offset(ox);
                w.set_play_body_scale_x(sx);
                w.set_play_body_scale_y(sy);
                w.set_eyes_offset_x(if side { 0.0 } else { ox * 0.10 });
                w.set_eyes_offset_y(if side { ox * 0.10 } else { 0.0 });
                w.set_eyes_scale_x(sx);
                w.set_eyes_scale_y(sy);
                if ox.abs() < 0.08 && spring_offset.1.abs() < 0.5
                    && spring_sx.0.abs() < 0.001 && spring_sy.0.abs() < 0.001 {
                    spring_offset = (0.0, 0.0);
                    spring_sx = (0.0, 0.0);
                    spring_sy = (0.0, 0.0);
                }
                // Poll ulang setelah 1,5 detik seperti blocked retry di Bottom Play Tauri.
                next_at = now + Duration::from_millis(1_500);
                return;
            }

            // Pilih gerakan dengan variasi jeda. Musik membuat Notch lebih ekspresif;
            // mood mengantuk memperlambat gerakan tanpa mematikan fitur.
            let test_requested = w.get_play_test_request();
            if liquid_repetitions_left == 0 && now >= next_liquid_at {
                liquid_repetitions_left = 3;
                next_liquid_run_at = now;
            }
            let liquid_due = liquid_repetitions_left > 0 && now >= next_liquid_run_at;
            if active.is_none() && (now >= next_at || test_requested || liquid_due) {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let roll = (seed >> 32) as u32;
                let low_battery = matches!(battery.read(), Reading::Present(value) if value.percent <= 15 && !value.charging);
                let mood_state = eye.borrow();
                let calm = mood_state.mood.is_sleeping() || mood_state.mood.low_battery || low_battery;
                drop(mood_state);
                let playing = w.get_media_playing();
                // Tes manual dan jadwal tiga rangkaian Liquid mendahului gerakan acak.
                let source = if test_requested { 2 } else if liquid_due { 1 } else { 0 };
                let choice = if test_requested {
                    w.set_play_test_request(false);
                    0
                } else if liquid_due {
                    liquid_repetitions_left = liquid_repetitions_left.saturating_sub(1);
                    if liquid_repetitions_left == 0 {
                        next_liquid_at = now + Duration::from_secs(180);
                    }
                    0
                } else {
                    // Bobot sama dengan NORMAL/MUSIC/CALM di bottom-play.ts.
                    // Pilihan terakhir dikeluarkan agar gerakan tidak berulang berturut-turut.
                    let weights: [u32; 8] = if calm {
                        [1, 0, 0, 0, 2, 3, 0, 3] // liquid, squish, sway, yawn
                    } else if playing {
                        [7, 3, 3, 2, 1, 0, 5, 0]
                    } else {
                        [6, 4, 3, 2, 2, 2, 1, 1]
                    };
                    let mut available = weights;
                    if let Some(previous) = last_choice { available[previous as usize] = 0; }
                    let total: u32 = available.iter().sum();
                    let mut pick = roll % total.max(1);
                    let mut selected = 0u8;
                    for (index, weight) in available.iter().enumerate() {
                        if pick < *weight { selected = index as u8; break; }
                        pick -= *weight;
                    }
                    selected
                };
                last_choice = Some(choice);
                // Durasi gerak dan ekspresi mengikuti tabel MOOD serta urutan TRICKS Tauri.
                let (duration_ms, mood_ms) = match choice {
                    0 => (4_800, 1_900), // liquid: beberapa langkah aliran
                    1 => (4_200, 2_600), // ghost: berjalan, menjulang, lalu menyusut
                    2 => (4_500, 2_600), // roam: pergi dan kembali
                    3 => (900, 1_200),   // wiggle
                    4 => (1_700, 900),   // squish
                    5 => (3_000, 2_200), // sway
                    6 => (2_100, 1_800), // dance
                    _ => (1_600, 2_400), // yawn
                };
                // Ekspresi ikut gerakan dan kembali ke mood dasar melalui MoodState,
                // sehingga renderer mata tetap menjadi satu-satunya sumber gambar wajah.
                let animation_mood = match choice {
                    0 | 2 | 3 => Mood::Happy,
                    1 | 4 => Mood::Surprised,
                    5 => Mood::Suspicious,
                    6 => Mood::Love,
                    _ => Mood::Sleepy,
                };
                eye.borrow_mut().mood.flash(animation_mood, mood_ms, now);
                let direction = if roll & 1 == 0 { -1.0 } else { 1.0 };
                active = Some((choice, now, Duration::from_millis(duration_ms), direction, source));
            }

            let Some((kind, started, duration, direction, source)) = active else {
                // Kembali ke bentuk netral memakai pegas, bukan menghapus transform seketika.
                let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
                last_frame = now;
                for _ in 0..2 {
                    let h = dt * 0.5;
                    spring_offset.1 += (-105.0 * spring_offset.0 - 17.0 * spring_offset.1) * h;
                    spring_offset.0 += spring_offset.1 * h;
                    spring_sx.1 += (-165.0 * spring_sx.0 - 20.0 * spring_sx.1) * h;
                    spring_sx.0 += spring_sx.1 * h;
                    spring_sy.1 += (-165.0 * spring_sy.0 - 20.0 * spring_sy.1) * h;
                    spring_sy.0 += spring_sy.1 * h;
                }
                let ox = spring_offset.0.clamp(-90.0, 90.0);
                let sx = (1.0 + spring_sx.0).clamp(0.78, 1.25);
                let sy = (1.0 + spring_sy.0).clamp(0.78, 1.45);
                let side = dock == 3 || dock == 4;
                w.set_play_offset(ox);
                w.set_play_body_scale_x(sx);
                w.set_play_body_scale_y(sy);
                w.set_eyes_offset_x(if side { 0.0 } else { ox * 0.10 });
                w.set_eyes_offset_y(if side { ox * 0.10 } else { 0.0 });
                w.set_eyes_scale_x(sx);
                w.set_eyes_scale_y(sy);
                if ox.abs() < 0.08 && spring_offset.1.abs() < 0.5
                    && spring_sx.0.abs() < 0.001 && spring_sy.0.abs() < 0.001 {
                    spring_offset = (0.0, 0.0);
                    spring_sx = (0.0, 0.0);
                    spring_sy = (0.0, 0.0);
                }
                return;
            };

            let elapsed = now.saturating_duration_since(started).as_secs_f32();
            let total = duration.as_secs_f32().max(0.001);
            let t = (elapsed / total).clamp(0.0, 1.0);
            if t >= 1.0 {
                active = None;
                if kind == 0 && source == 1 && liquid_repetitions_left > 0 {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    next_liquid_run_at = now + Duration::from_secs(7 + u64::from((seed >> 32) as u32 % 15));
                }
                let low_battery = matches!(battery.read(), Reading::Present(value) if value.percent <= 15 && !value.charging);
                let mood_state = eye.borrow();
                let calm = mood_state.mood.is_sleeping() || mood_state.mood.low_battery || low_battery;
                drop(mood_state);
                let playing = w.get_media_playing();
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let fraction = ((seed >> 32) as u32) as f32 / u32::MAX as f32;
                // Jeda antar-gerakan sama dengan schedule() versi Tauri:
                // calm 10–18s, musik 5–9,5s, normal 7–14s.
                let delay = if calm { 10.0 + fraction * 8.0 }
                    else if playing { 5.0 + fraction * 4.5 }
                    else { 7.0 + fraction * 7.0 };
                next_at = now + Duration::from_secs_f32(delay);
                return;
            }

            // Envelope halus: setiap gerakan masuk dan kembali ke posisi netral tanpa hentakan.
            let envelope = (std::f32::consts::PI * t).sin().max(0.0);
            let wave = (std::f32::consts::TAU * t).sin();
            let fast_wave = (std::f32::consts::TAU * 3.0 * t).sin();
            let mut offset = 0.0;
            let mut sx = 1.0;
            let mut sy = 1.0;

            // Gerakan perjalanan memakai beberapa titik tujuan yang dibatasi di dalam
            // area transform Notch. Interpolasi smoothstep menghindari perpindahan patah.
            let smooth = |v: f32| {
                let v = v.clamp(0.0, 1.0);
                v * v * (3.0 - 2.0 * v)
            };
            let random_offset = |index: u32, limit: f32| {
                let mut value = seed
                    .wrapping_add(u64::from(index).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                value ^= value >> 30;
                value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                value ^= value >> 27;
                value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
                value ^= value >> 31;
                let unit = (value >> 40) as f32 / ((1u32 << 24) - 1) as f32;
                (unit * 2.0 - 1.0) * limit
            };
            let travel_segment = |progress: f32, count: u32, limit: f32, return_home: bool| {
                let scaled = progress.clamp(0.0, 1.0) * count as f32;
                let index = (scaled.floor() as u32).min(count.saturating_sub(1));
                let local = smooth(scaled - index as f32);
                let from = if index == 0 { 0.0 } else { random_offset(index, limit) };
                let to = if return_home && index + 1 == count {
                    0.0
                } else {
                    random_offset(index + 1, limit)
                };
                from + (to - from) * local
            };

            match kind {
                0 => { // LIQUID: 3–6 langkah pendek dengan arah dan jarak bervariasi.
                    let steps = 3 + (seed as u32 % 4);
                    offset = travel_segment(t, steps, 56.0, true);
                    sx += 0.055 * fast_wave * envelope;
                    sy -= 0.045 * fast_wave * envelope;
                    let ripple = (std::f32::consts::TAU * 5.0 * t).sin() * envelope;
                    sx += 0.035 * ripple;
                    sy -= 0.03 * ripple;
                }
                1 => { // GHOST: meluncur pelan sambil menjulang, lalu menyusut.
                    let lift = (std::f32::consts::PI * t).sin().max(0.0);
                    sx = 1.0 - 0.13 * lift;
                    sy = 1.0 + 0.38 * lift;
                    offset = direction * 42.0 * smooth(t);
                }
                2 => { // ROAM: satu atau dua perjalanan, kemudian kembali ke titik awal.
                    let legs = if seed & 1 == 0 { 2 } else { 3 };
                    offset = travel_segment(t, legs, 78.0, true);
                    sx += 0.035 * envelope;
                    sy -= 0.025 * envelope;
                }
                3 => { // WIGGLE: getaran berirama yang tetap lembut.
                    offset = 10.0 * fast_wave * envelope;
                    sx += 0.045 * fast_wave * envelope;
                    sy -= 0.035 * fast_wave * envelope;
                }
                4 => { // SQUISH: gepeng-memanjang bergantian.
                    let pulse = (std::f32::consts::TAU * 2.5 * t).sin() * envelope;
                    sx += 0.12 * pulse;
                    sy -= 0.10 * pulse;
                }
                5 => { // SWAY: ayunan lambat.
                    offset = 9.0 * wave * envelope;
                    sx += 0.025 * wave * envelope;
                    sy -= 0.02 * wave * envelope;
                }
                6 => { // DANCE: dua ketukan cepat saat musik aktif.
                    let beat = (std::f32::consts::TAU * 4.0 * t).sin();
                    offset = 13.0 * beat * envelope;
                    sx += 0.07 * beat * envelope;
                    sy -= 0.055 * beat * envelope;
                }
                _ => { // YAWN: meregang panjang, menahan, lalu lemas.
                    let stretch = (std::f32::consts::PI * t).sin().max(0.0);
                    sx = 1.0 - 0.07 * stretch;
                    sy = 1.0 + 0.22 * stretch;
                }
            }

            // Di dock samping, sumbu offset tetap mengikuti tepi secara vertikal.
            // Skala badan dan wajah dikendalikan bersama agar ekspresi tidak terlepas dari bentuk.
            let side = dock == 3 || dock == 4;
            // Spring step dengan substep 8 ms menjaga kestabilan ketika frame melambat.
            let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
            last_frame = now;
            let target_x = offset.clamp(-90.0, 90.0);
            let target_sx = (sx - 1.0).clamp(-0.22, 0.25);
            let target_sy = (sy - 1.0).clamp(-0.22, 0.45);
            let steps = (dt / 0.008).ceil().clamp(1.0, 8.0) as u32;
            let h = dt / steps as f32;
            for _ in 0..steps {
                spring_offset.1 += (105.0 * (target_x - spring_offset.0) - 17.0 * spring_offset.1) * h;
                spring_offset.0 += spring_offset.1 * h;
                spring_sx.1 += (165.0 * (target_sx - spring_sx.0) - 20.0 * spring_sx.1) * h;
                spring_sx.0 += spring_sx.1 * h;
                spring_sy.1 += (165.0 * (target_sy - spring_sy.0) - 20.0 * spring_sy.1) * h;
                spring_sy.0 += spring_sy.1 * h;
            }
            let body_x = (1.0 + spring_sx.0).clamp(0.78, 1.25);
            let body_y = (1.0 + spring_sy.0).clamp(0.78, 1.45);
            let body_offset = spring_offset.0.clamp(-90.0, 90.0);
            w.set_play_offset(body_offset);
            w.set_play_body_scale_x(body_x);
            w.set_play_body_scale_y(body_y);
            w.set_eyes_offset_x(if side { 0.0 } else { body_offset * 0.10 });
            w.set_eyes_offset_y(if side { body_offset * 0.10 } else { 0.0 });
            w.set_eyes_scale_x(body_x);
            w.set_eyes_scale_y(body_y);
        });
    }

    {
        let weak = win.as_weak();
        let selected_paths = convert_selected_paths.clone();
        let last_output = convert_last_output.clone();
        let running = convert_running.clone();
        let preview_page = convert_preview_page.clone();
        let preview_path = convert_preview_path.clone();
        let weak_preview = weak.clone();
        winit_handlers.borrow_mut().push(Box::new(move |event| {
            if let winit::event::WindowEvent::DroppedFile(path) = event {
                // File lepas ke Shelf kecuali halaman Convert terbuka (lihat shelf_accepts_drop).
                if weak.upgrade().is_some_and(|w| shelf_accepts_drop(&w)) {
                    return;
                }
                if running.load(std::sync::atomic::Ordering::Acquire) {
                    if let Some(w) = weak.upgrade() {
                        w.set_convert_status("Selesaikan atau batalkan batch sebelum menambahkan file".into());
                    }
                    return;
                }
                if !path.is_file() {
                    return;
                }
                if convert::available_targets(path).is_empty() {
                    if let Some(w) = weak.upgrade() {
                        w.set_convert_status(format!("Format file tidak didukung: {}", path.display()).into());
                    }
                    return;
                }
                let raw = path.to_string_lossy().to_string();
                let mut selected = selected_paths.borrow_mut();
                if !selected.iter().any(|existing| existing.eq_ignore_ascii_case(&raw)) {
                    selected.push(raw.clone());
                }
                let paths = selected.clone();
                drop(selected);
                *last_output.lock().unwrap_or_else(|error| error.into_inner()) = None;
                if let Some(w) = weak.upgrade() {
                    w.set_convert_selected_path(paths.first().cloned().unwrap_or(raw).into());
                    w.set_convert_files_text(format_convert_selection(&paths).into());
                    if let Some(first) = paths.first() {
                        preview_page.set(0);
                        *preview_path.borrow_mut() = first.clone();
                        w.set_convert_preview_page_text("Memuat preview…".into());
                        request_convert_document_preview(weak_preview.clone(), first.clone(), 0, convert_preview_dpi.get());
                    }
                    w.set_convert_result_preview(Image::default());
                    w.set_convert_history_text(format_convert_history().into());
                    w.set_convert_progress_text("".into());
                    w.set_convert_status(format!("{} file siap dikonversi · file diterima dari Explorer", paths.len()).into());
                }
            }
        }));
    }

    // Sapaan otomatis native saat aplikasi mulai. Timer hidup selama event loop Slint berjalan.
    let startup_greeting_timer = Timer::default();
    let startup_greeting_hide_timer = Rc::new(Timer::default());
    if cfg.greeting.enabled {
        let weak = win.as_weak();
        let hide_timer = startup_greeting_hide_timer.clone();
        let shell = shell.clone();
        let greeting = speech::greeting_content(&cfg.greeting.mode, &cfg.greeting.custom_text, platform::local_time().0);
        let voice = cfg.greeting.voice.clone();
        startup_greeting_timer.start(TimerMode::SingleShot, Duration::from_millis(1_800), move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_meeting_active() || w.get_adhan_active() || w.get_expanded() { return; }
            if let Some(greeting) = greeting.as_ref() {
                w.set_greeting_message(greeting.message.clone().into());
                w.set_greeting_detail(greeting.detail.clone().into());
                // Greeting memakai kartu inline di dock atas dan speech bubble di dock bawah/samping.
                if w.get_dock() < 2 {
                    w.set_greeting_speech_visible(false);
                    w.set_nudge_visible(false);
                    show_greeting_inline(weak.clone(), shell.clone());
                } else {
                    w.set_greeting_inline_visible(false);
                    w.set_nudge_visible(false);
                    w.set_greeting_speech_visible(true);
                    shell.place(&w, COLLAPSED);
                }
                let greeting_duration_ms = speech::estimated_duration_ms(&greeting.spoken);
                let _ = speech::speak(greeting.spoken.clone(), voice.clone());
                let weak_hide = weak.clone();
                let shell_hide = shell.clone();
                hide_timer.start(TimerMode::SingleShot, Duration::from_millis(greeting_duration_ms + 600), move || {
                    if let Some(w) = weak_hide.upgrade() {
                        let was_inline = w.get_greeting_inline_visible() && !w.get_expanded();
                        w.set_greeting_inline_visible(false);
                        if was_inline {
                            shrink_window_synced(weak_hide.clone(), shell_hide.clone());
                        }
                        w.set_greeting_speech_visible(false);
                        w.set_nudge_visible(false);
                    }
                });
            }
        });
    }

    // Koreksi geometri setelah native Winit window siap; shell.place() pertama terjadi sebelum event loop.
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        Timer::single_shot(Duration::from_millis(350), move || {
            if let Some(w) = weak.upgrade() {
                shell.memo.set((None, 0));
                let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                shell.place(&w, size);
                platform::hide_from_taskbar(w.window());
            }
        });
    }

    // Sembunyikan dari taskbar. Jendela winit baru bisa diakses setelah event loop berjalan, jadi diterapkan dari timer;
    // diulang beberapa kali karena shell Windows membuat tombol taskbar secara asinkron.
    for delay_ms in [100u64, 400, 1500] {
        let weak = win.as_weak();
        Timer::single_shot(Duration::from_millis(delay_ms), move || {
            if let Some(w) = weak.upgrade() {
                platform::hide_from_taskbar(w.window());
            }
        });
    }

    // --- Pengaturan: cari kota, tes adzan, mulai ulang (setara jendela Pengaturan Tauri) ---
    {
        let weak = win.as_weak();
        win.on_settings_search_city(move |query| {
            let query = query.trim().to_string();
            let Some(w) = weak.upgrade() else { return };
            if query.chars().count() < 2 {
                w.set_weather_hits(ModelRc::new(VecModel::<CityHit>::default()));
                w.set_weather_search_status("Ketik minimal 2 huruf nama kota.".into());
                return;
            }
            w.set_weather_search_status("Mencari...".into());
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = search_cities(&query);
                let _ = weak.upgrade_in_event_loop(move |w| match result {
                    Ok(hits) => {
                        let status = if hits.is_empty() {
                            "Kota tidak ditemukan. Coba ejaan lain atau isi koordinat manual."
                        } else {
                            "Pilih salah satu hasil."
                        };
                        let rows: Vec<CityHit> = hits
                            .into_iter()
                            .map(|(label, name, lat, lon)| CityHit { label: label.into(), name: name.into(), lat: lat.into(), lon: lon.into() })
                            .collect();
                        w.set_weather_hits(ModelRc::new(VecModel::from(rows)));
                        w.set_weather_search_status(status.into());
                    }
                    Err(error) => {
                        w.set_weather_hits(ModelRc::new(VecModel::<CityHit>::default()));
                        w.set_weather_search_status(format!("Pencarian gagal: {error}").into());
                    }
                });
            });
        });
        let weak = win.as_weak();
        win.on_settings_test_adhan(move || media::play_adhan(weak.clone(), "Tes".to_string()));
        win.on_settings_pause_adhan(|pause| media::pause_adhan(pause));
        win.on_settings_restart(|| {
            RESTART_REQUESTED.store(true, Ordering::SeqCst);
            let _ = slint::quit_event_loop();
        });
    }

    // --- Tray: Pengaturan, Pusatkan Notch, Keluar (sama dengan tray versi Tauri) ---
    let tray_handle: Rc<RefCell<Option<tray::Tray>>> = Rc::new(RefCell::new(None));
    let tray_retry = Rc::new(Timer::default());
    {
        let weak = win.as_weak();
        let open_settings = open_settings.clone();
        let on_action: Rc<dyn Fn(tray::Action)> = Rc::new(move |action| {
            let Some(w) = weak.upgrade() else { return };
            match action {
                tray::Action::Settings => open_settings(),
                // Jalur yang sama dengan pilihan posisi "top" di Pengaturan: disimpan ke config seperti di Tauri.
                tray::Action::Center => w.invoke_settings_save_position("top".into()),
                tray::Action::Quit => {
                    let _ = slint::quit_event_loop();
                }
            }
        });
        let try_install = {
            let holder = tray_handle.clone();
            move || -> bool {
                if holder.borrow().is_some() {
                    return true;
                }
                let on_action = on_action.clone();
                match tray::install(move |action| on_action(action)) {
                    Ok(installed) => {
                        *holder.borrow_mut() = Some(installed);
                        true
                    }
                    Err(error) => {
                        eprintln!("tray gagal dibuat (pengaturan tidak bisa dibuka lewat tray): {error}");
                        false
                    }
                }
            }
        };
        // Saat berjalan otomatis ketika login, tray kadang belum siap: coba lagi tiap 3 detik (maksimal 20 kali).
        if tray::SUPPORTED && !try_install() {
            let tries = Cell::new(0u32);
            let timer = Rc::downgrade(&tray_retry);
            tray_retry.start(TimerMode::Repeated, Duration::from_secs(3), move || {
                tries.set(tries.get() + 1);
                if try_install() || tries.get() >= 20 {
                    if let Some(timer) = timer.upgrade() {
                        timer.stop();
                    }
                }
            });
        }
    }

    let result = win.run();
    if RESTART_REQUESTED.load(Ordering::SeqCst) {
        // Lepas jendela dan tray lebih dulu supaya hotkey/ikon lama tidak bentrok dengan proses baru.
        drop(win);
        tray_handle.borrow_mut().take();
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
            Ok(_) => {}
            Err(error) => eprintln!("gagal memulai ulang Notch: {error}"),
        }
    }
    result
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