//! Jadwal shalat equran.id dan preferensi yang digunakan bersama oleh Tauri dan Slint.
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Day {
    pub tanggal: u32, pub imsak: String, pub subuh: String, pub terbit: String,
    pub dhuha: String, pub dzuhur: String, pub ashar: String, pub maghrib: String, pub isya: String,
}
#[derive(Deserialize)]
struct ApiResp { data: Option<ApiData> }
#[derive(Deserialize)]
struct ApiData { #[serde(default)] jadwal: Vec<Day> }

static IO: Mutex<()> = Mutex::new(());
fn cache_path() -> PathBuf { crate::paths::data_file("prayer_cache.json") }
fn load_cache() -> HashMap<String, Vec<Day>> {
    std::fs::read_to_string(cache_path()).ok()
        .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Ambil jadwal satu bulan. Cache digunakan dahulu dan tetap dipertahankan jika jaringan gagal.
pub fn month(provinsi: &str, kabkota: &str, year: i32, month: u32, refresh: bool) -> Result<Vec<Day>, String> {
    if !(1..=12).contains(&month) || !(1970..=9999).contains(&year) {
        return Err("Bulan atau tahun tidak valid".into());
    }
    let key = format!("{provinsi}|{kabkota}|{year}-{month}");
    let _guard = IO.lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = load_cache();
    if !refresh {
        if let Some(days) = cache.get(&key) { return Ok(days.clone()); }
    }
    let body = serde_json::json!({"provinsi":provinsi,"kabkota":kabkota,"bulan":month,"tahun":year});
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15))).user_agent("Notch/0.1").build().into();
    let mut response = agent.post("https://equran.id/api/v2/shalat")
        .send_json(body).map_err(|e| e.to_string())?;
    let raw = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let parsed: ApiResp = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let days = parsed.data.map(|d| d.jadwal).unwrap_or_default();
    if days.is_empty() { return Err("Jadwal kosong; periksa nama provinsi/kabupaten/kota".into()); }
    cache.insert(key, days.clone());
    let prefix = format!("{provinsi}|{kabkota}|");
    if cache.len() > 12 { cache.retain(|k, _| k.starts_with(&prefix)); }
    if let Some(dir) = cache_path().parent() { let _ = std::fs::create_dir_all(dir); }
    std::fs::write(cache_path(), serde_json::to_string(&cache).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(days)
}

pub fn preferences() -> crate::config::Prayer {
    crate::config::Config::load_readonly(&crate::paths::config_path()).prayer
}
pub fn save_preferences(mode: &str, remind_before: bool, remind_min: u32) -> Result<(), String> {
    let path = crate::paths::config_path();
    let mut cfg = crate::config::Config::load_readonly(&path);
    cfg.prayer.mode = match mode { "adzan" | "tone" | "silent" => mode.to_string(), _ => "tone".into() };
    cfg.prayer.remind_before = remind_before;
    cfg.prayer.remind_min = remind_min.clamp(1, 60);
    let encoded = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
    std::fs::write(path, encoded).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn equran_day_deserializes() {
        let d: Day = serde_json::from_str(r#"{"tanggal":16,"imsak":"04:15","subuh":"04:25","terbit":"05:40","dhuha":"06:05","dzuhur":"11:45","ashar":"15:00","maghrib":"17:50","isya":"19:00"}"#).unwrap();
        assert_eq!(d.maghrib, "17:50");
    }
}
