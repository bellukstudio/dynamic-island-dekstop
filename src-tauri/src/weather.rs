use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// Lokasi yang sedang dipakai. Kosong = ikuti config.toml saat aplikasi dibuka;
// diisi saat lokasi diubah dari jendela pengaturan (berlaku tanpa restart).
static LOC: Mutex<Option<(f64, f64)>> = Mutex::new(None);
// kanal untuk membangunkan thread cuaca agar langsung mengambil data baru
static REFRESH: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();

fn location() -> (f64, f64) {
    let saved = *LOC.lock().unwrap_or_else(|e| e.into_inner());
    saved.unwrap_or_else(|| {
        let w = &crate::config::get().weather;
        (w.lat, w.lon)
    })
}

// Dipanggil saat pengaturan disimpan. Kalau lokasinya berubah, cuaca langsung diambil ulang.
pub fn set_location(lat: f64, lon: f64) {
    let (cl, co) = location();
    if (cl - lat).abs() < 1e-6 && (co - lon).abs() < 1e-6 {
        return;
    }
    *LOC.lock().unwrap_or_else(|e| e.into_inner()) = Some((lat, lon));
    if let Some(tx) = REFRESH.get() {
        if let Ok(g) = tx.lock() {
            let _ = g.send(());
        }
    }
}

#[derive(Deserialize)]
struct Resp {
    current: Current,
    minutely_15: Option<Minutely>,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    weather_code: u32,
    is_day: u8,
}

#[derive(Deserialize)]
struct Minutely {
    precipitation: Vec<Option<f64>>,
}

#[derive(Serialize, Clone)]
struct WeatherEvent {
    temp: f64,
    feels: f64,
    humidity: f64,
    code: u32,
    is_day: bool,
    rain_in: Option<u32>, // menit sampai hujan, kalau saat ini belum hujan
}

fn fetch() -> Result<WeatherEvent, String> {
    let (lat, lon) = location();
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,is_day\
         &minutely_15=precipitation&forecast_minutely_15=8&timezone=auto"
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15)) // jangan menggantung selamanya kalau jaringan macet
        .build()
        .map_err(|e| e.to_string())?;
    let r: Resp = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let c = r.current;
    let raining_now = (51..=67).contains(&c.weather_code)
        || (80..=82).contains(&c.weather_code)
        || c.weather_code >= 95;

    // cari slot 15 menit pertama dengan curah hujan berarti
    let rain_in = if raining_now {
        None
    } else {
        r.minutely_15.and_then(|m| {
            m.precipitation
                .iter()
                .position(|p| p.unwrap_or(0.0) >= 0.2)
                .map(|i| (i as u32 + 1) * 15)
        })
    };

    Ok(WeatherEvent {
        temp: c.temperature_2m,
        feels: c.apparent_temperature,
        humidity: c.relative_humidity_2m,
        code: c.weather_code,
        is_day: c.is_day == 1,
        rain_in,
    })
}

pub fn start(app: AppHandle) {
    // halaman "weather" dihapus dari config -> tidak perlu thread dan permintaan jaringan sama sekali
    if !crate::config::get().pages.iter().any(|p| p == "weather") {
        return;
    }
    let (tx, rx) = mpsc::channel::<()>();
    let _ = REFRESH.set(Mutex::new(tx));

    std::thread::spawn(move || {
        let mut alerted = false;
        let mut silent = false; // true = pengambilan ulang karena lokasi diganti: jangan munculkan alert hujan
        loop {
            let wait = match fetch() {
                Ok(w) => {
                    println!("weather: {:.0}°C kode {} hujan_dalam {:?}", w.temp, w.code, w.rain_in);
                    // alert hujan hanya sekali per episode
                    let alert = !silent && w.rain_in.is_some() && !alerted;
                    alerted = w.rain_in.is_some();
                    let _ = app.emit("weather", (w, alert));
                    // Poll cuaca lebih sering agar Conversation Moments menangkap perubahan
                    // kondisi dengan jeda lebih pendek. Hanya berjalan jika halaman cuaca aktif.
                    Duration::from_secs(5 * 60)
                }
                Err(e) => {
                    eprintln!("weather gagal: {e}");
                    Duration::from_secs(60)
                }
            };
            // tunggu jadwal berikutnya, atau langsung jalan kalau lokasi diubah
            silent = match rx.recv_timeout(wait) {
                Ok(()) => {
                    while rx.try_recv().is_ok() {}
                    true
                }
                Err(_) => false,
            };
        }
    });
}