use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};

// Jadwal shalat dari equran.id. Rust hanya mengambil + menyimpan cache (memori & disk);
// hitung mundur dan pengingat dikerjakan frontend (src/prayer.ts).

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Day {
    pub tanggal: u32,
    pub imsak: String,
    pub subuh: String,
    pub terbit: String,
    pub dhuha: String,
    pub dzuhur: String,
    pub ashar: String,
    pub maghrib: String,
    pub isya: String,
}

#[derive(Deserialize)]
struct ApiResp {
    data: Option<ApiData>,
}
#[derive(Deserialize)]
struct ApiData {
    #[serde(default)]
    jadwal: Vec<Day>,
}

// Cache "provinsi|kabkota|tahun-bulan" -> jadwal sebulan hanya hidup di disk. Dipanggil jarang
// (sekitar sekali per bulan), jadi dibaca saat perlu lalu dibuang; tidak ditahan di memori.
static IO: Mutex<()> = Mutex::new(());

fn cache_path() -> PathBuf {
    crate::config::path().with_file_name("prayer_cache.json")
}

fn load_disk() -> HashMap<String, Vec<Day>> {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn with_cache<R>(f: impl FnOnce(&mut HashMap<String, Vec<Day>>) -> R) -> R {
    let _g = IO.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = load_disk();
    f(&mut m)
}

fn fetch(prov: &str, kab: &str, year: i32, month: u32) -> Result<Vec<Day>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "provinsi": prov,
        "kabkota": kab,
        "bulan": month,
        "tahun": year,
    });
    let r: ApiResp = client
        .post("https://equran.id/api/v2/shalat")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| format!("{e} (cek nama provinsi/kabkota di config.toml)"))?
        .json()
        .map_err(|e| e.to_string())?;
    let days = r.data.map(|d| d.jadwal).unwrap_or_default();
    if days.is_empty() {
        return Err("jadwal kosong (cek nama provinsi/kabkota di config.toml)".into());
    }
    Ok(days)
}

// Jadwal sebulan. Cache dipakai dulu; kalau jaringan gagal, cache lama tetap dipakai.
#[tauri::command]
pub async fn prayer_month(
    provinsi: String,
    kabkota: String,
    year: i32,
    month: u32,
    refresh: Option<bool>,
) -> Result<Vec<Day>, String> {
    let job = tauri::async_runtime::spawn_blocking(move || {
        let key = format!("{provinsi}|{kabkota}|{year}-{month}");
        if !refresh.unwrap_or(false) {
            if let Some(d) = with_cache(|m| m.get(&key).cloned()) {
                return Ok(d);
            }
        }
        match fetch(&provinsi, &kabkota, year, month) {
            Ok(d) => {
                let prefix = format!("{provinsi}|{kabkota}|");
                with_cache(|m| {
                    m.insert(key.clone(), d.clone());
                    if m.len() > 12 {
                        m.retain(|k, _| k.starts_with(&prefix)); // buang lokasi lama
                    }
                    if let Ok(s) = serde_json::to_string(&*m) {
                        let _ = std::fs::write(cache_path(), s);
                    }
                });
                Ok(d)
            }
            Err(e) => match with_cache(|m| m.get(&key).cloned()) {
                Some(d) => Ok(d),
                None => Err(e),
            },
        }
    })
    .await;
    match job {
        Ok(r) => r,
        Err(e) => Err(e.to_string()),
    }
}

// lokasi + preferensi terbaru dari config.toml
#[tauri::command]
pub fn prayer_info() -> crate::config::Prayer {
    crate::config::cfg_get().prayer
}

// simpan pilihan Adzan/Tone/Silent dan pengingat dari halaman Sholat
#[tauri::command]
pub fn prayer_set_prefs(
    app: tauri::AppHandle,
    mode: String,
    remind_before: bool,
    remind_min: u32,
) -> Result<(), String> {
    let mut cfg = crate::config::cfg_get();
    cfg.prayer.mode = mode;
    cfg.prayer.remind_before = remind_before;
    cfg.prayer.remind_min = remind_min;
    crate::config::cfg_save(app, cfg).map(|_| ())
}