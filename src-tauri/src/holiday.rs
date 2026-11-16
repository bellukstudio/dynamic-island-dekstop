//! Data hari libur Indonesia yang diperbarui otomatis.
//!
//! Sumber: kalender publik yang diperbarui bot di GitHub (guangrei/APIHariLibur_V2, hasil olahan
//! Google Calendar). Satu berkas kecil (~10 KB) memuat tahun berjalan dan tahun depan, jadi cakupannya
//! ikut maju sendiri. Hasil disimpan di <folder config>/notch/holidays.json dan dipakai lagi saat
//! offline. Diambil paling sering sekali per 12 jam; tidak ada data pribadi yang dikirim.
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// dicoba berurutan; yang kedua adalah cermin CDN dari berkas yang sama
const SOURCES: &[&str] = &[
    "https://raw.githubusercontent.com/guangrei/APIHariLibur_V2/main/calendar.json",
    "https://cdn.jsdelivr.net/gh/guangrei/APIHariLibur_V2@main/calendar.json",
];
const TTL_SECS: u64 = 12 * 3600;

#[derive(Serialize, Deserialize, Clone)]
pub struct Item {
    date: String, // "YYYY-MM-DD"
    name: String,
    libur: bool, // true = hari libur / cuti bersama, false = hari peringatan ("Perayaan")
}

#[derive(Serialize, Deserialize)]
struct Cache {
    fetched: u64, // detik sejak epoch
    items: Vec<Item>,
}

fn cache_path() -> std::path::PathBuf {
    crate::config::path().with_file_name("holidays.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read_cache() -> Option<Cache> {
    serde_json::from_str(&std::fs::read_to_string(cache_path()).ok()?).ok()
}

fn write_cache(items: &[Item]) {
    let c = Cache { fetched: now(), items: items.to_vec() };
    if let Ok(s) = serde_json::to_string(&c) {
        let _ = std::fs::write(cache_path(), s);
    }
}

// "2026-03-21" -> true
fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

// Format sumber: { "2026-01-01": { "holiday": true, "summary": ["Hari Tahun Baru"], ... }, "info": {...} }
fn parse(text: &str) -> Result<Vec<Item>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("format tidak dikenal")?;
    let mut out: Vec<Item> = Vec::new();
    for (date, e) in obj {
        if !valid_date(date) {
            continue; // lewati kunci "info"
        }
        let libur = e["holiday"].as_bool().unwrap_or(false);
        for s in e["summary"].as_array().into_iter().flatten() {
            if let Some(name) = s.as_str().map(str::trim).filter(|n| !n.is_empty()) {
                out.push(Item { date: date.clone(), name: name.to_string(), libur });
            }
        }
    }
    if out.is_empty() {
        return Err("data kosong".into());
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

fn download() -> Result<Vec<Item>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Notch/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let mut last = String::from("tidak ada sumber");
    for url in SOURCES {
        let res = client
            .get(*url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text());
        match res {
            Ok(text) => match parse(&text) {
                Ok(items) => return Ok(items),
                Err(e) => last = format!("{url}: {e}"),
            },
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

// Dipanggil frontend (holidays.ts). Selalu mengembalikan data terbaik yang ada:
// cache segar -> unduhan baru -> cache lama (offline). Error hanya kalau belum pernah berhasil sama sekali.
#[tauri::command]
pub async fn holidays_fetch() -> Result<Vec<Item>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let cached = read_cache();
        if let Some(c) = &cached {
            if now().saturating_sub(c.fetched) < TTL_SECS {
                return Ok(c.items.clone());
            }
        }
        match download() {
            Ok(items) => {
                write_cache(&items);
                println!("holidays: {} entri diperbarui dari internet", items.len());
                Ok(items)
            }
            Err(e) => match cached {
                Some(c) => {
                    eprintln!("holidays: unduh gagal ({e}), memakai cache lama");
                    Ok(c.items)
                }
                None => Err(e),
            },
        }
    })
    .await
    .map_err(|e| e.to_string())?
}