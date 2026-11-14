pub use notch_core::config::*; // struct Config dkk. ada di notch-core (dipakai juga oleh Slint)
use std::{path::PathBuf, sync::OnceLock};

static CFG: OnceLock<Config> = OnceLock::new();

pub fn path() -> PathBuf {
    // penentuan lokasi ada di notch-core supaya UI Tauri dan Slint memakai folder data yang sama
    notch_core::paths::config_path()
}

// template bawaan (lengkap dengan komentar) untuk instalasi baru
const DEFAULT_TOML: &str = include_str!("default_config.toml");

// ditambahkan ke config.toml lama yang belum punya section [appearance]
const APPEARANCE_BLOCK: &str = r##"
[appearance]
# theme: auto (ikut OS) | dark | light | custom
theme = "auto"
background = "#1e1e2e"  # hanya untuk theme = "custom"
text = ""               # kosong = otomatis kontras
dark_bg = "#000000"
light_bg = "#f5f5f7"
accent = ""             # kosong = otomatis dari cover album, contoh: "#ff6b6b"
eye_theme = "aurora"  # aurora | neon | pastel | pixel | holo | ember | ocean | sakura | cyber | monochrome
                      # kawaii | boba | galaxy | matcha | candy | synthwave | slime | cotton | pumpkin | frost
notch_theme = "default"  # default | aurora | obsidian | neon | sakura | matcha  (menggantikan warna latar & aksen)
"##;

// ditambahkan ke config.toml lama maupun template bawaan yang belum punya section [prayer]
const PRAYER_BLOCK: &str = r##"
[prayer]
provinsi = "Jawa Barat"
kabkota = "Kota Bandung"
mode = "tone"            # adzan | tone | silent
remind_before = false
remind_min = 10
"##;

// ditambahkan ke config.toml (lama maupun template bawaan) yang belum punya section [nudge]
const NUDGE_BLOCK: &str = r##"
[nudge]
enabled = true                       # pesan muncul saat pointer lama diam
idle_min = 2                         # menit pointer diam sebelum pesan muncul
message = "Kamu kemana, kok diam saja?"
emoji = "🥺"
mood = "sad"                         # normal | happy | sleepy | surprised | angry | love | suspicious | sad
show_ms = 6000                       # lama pesan tampil (milidetik)
only_silent = true                   # jangan muncul saat musik/video sedang diputar
"##;

pub fn init() {
    let p = path();
    let mut cfg = match std::fs::read_to_string(&p) {
        Ok(s) => {
            // user lama: tambahkan blok yang belum ada di akhir file
            let mut extra = String::new();
            if !s.contains("[appearance]") {
                extra.push_str(APPEARANCE_BLOCK);
            }
            if !s.contains("[nudge]") {
                extra.push_str(NUDGE_BLOCK);
            }
            if !s.contains("[prayer]") {
                extra.push_str(PRAYER_BLOCK);
            }
            if !extra.is_empty() {
                use std::io::Write;
                let _ = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&p)
                    .and_then(|mut f| f.write_all(extra.as_bytes()));
            }
            toml::from_str::<Config>(&s).unwrap_or_else(|e| {
                eprintln!("config: file rusak, memakai bawaan ({e})");
                Config::default()
            })
        }
        Err(_) => {
            // pertama kali: tulis template bawaan supaya mudah diedit
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut text = DEFAULT_TOML.to_string();
            if !text.contains("[appearance]") {
                text.push_str(APPEARANCE_BLOCK);
            }
            if !text.contains("[nudge]") {
                text.push_str(NUDGE_BLOCK);
            }
            if !text.contains("[prayer]") {
                text.push_str(PRAYER_BLOCK);
            }
            let _ = std::fs::write(&p, &text);
            toml::from_str::<Config>(&text).unwrap_or_default()
        }
    };
    cfg.scale = cfg.scale.clamp(0.5, 1.5);
    if cfg.pages.is_empty() {
        cfg.pages = Config::default().pages;
    }
    cfg.nudge.sanitize();
    cfg.appearance.sanitize();
    migrate(&mut cfg);
    println!("config: {}", p.display());
    let _ = CFG.set(cfg);
}

pub fn get() -> &'static Config {
    CFG.get_or_init(Config::default)
}

// ======================= command untuk jendela pengaturan =======================

// baca dari file (bukan dari CFG statis) supaya selalu yang terbaru
#[tauri::command]
pub fn cfg_get() -> Config {
    let mut cfg: Config = std::fs::read_to_string(path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    cfg.nudge.sanitize();
    cfg.appearance.sanitize();
    migrate(&mut cfg); // UI menampilkan keadaan yang sama dengan yang sedang berjalan
    cfg
}

// pratinjau langsung: hanya dikirim ke notch, tidak menulis file
#[tauri::command]
pub fn cfg_preview(app: tauri::AppHandle, mut cfg: Config) {
    use tauri::Emitter;
    cfg.nudge.sanitize();
    cfg.appearance.sanitize();
    let _ = app.emit("config-changed", &cfg);
}

// simpan ke config.toml, lalu kabari notch. Mengembalikan lokasi file.
#[tauri::command]
pub fn cfg_save(app: tauri::AppHandle, mut cfg: Config) -> Result<String, String> {
    use tauri::Emitter;
    cfg.scale = cfg.scale.clamp(0.5, 1.5);
    if cfg.pages.is_empty() {
        cfg.pages = Config::default().pages;
    }
    cfg.pages_version = PAGES_VERSION; // pilihan fitur dari UI dianggap final
    cfg.weather.lat = cfg.weather.lat.clamp(-90.0, 90.0);
    cfg.weather.lon = cfg.weather.lon.clamp(-180.0, 180.0);
    if cfg.weather.city.trim().is_empty() {
        cfg.weather.city = "Lokasi saya".into();
    }
    cfg.nudge.sanitize();
    cfg.appearance.sanitize();
    let p = path();
    let text = toml::to_string_pretty(&cfg).map_err(|e| format!("gagal membuat TOML: {e}"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("gagal membuat folder: {e}"))?;
    }
    std::fs::write(&p, text).map_err(|e| format!("gagal menulis {}: {e}", p.display()))?;
    println!("config: disimpan ke {}", p.display());
    // lokasi cuaca berlaku langsung tanpa restart
    crate::weather::set_location(cfg.weather.lat, cfg.weather.lon);
    let _ = app.emit("config-changed", &cfg);
    Ok(p.display().to_string())
}

// Restart otomatis hanya di build release. Di `tauri dev`, proses yang keluar
// justru menghentikan sesi dev, jadi di sana dikembalikan pesan saja.
#[tauri::command]
pub fn cfg_restart(app: tauri::AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Err("mode dev: tutup lalu jalankan ulang `npm run tauri dev` secara manual".into());
    }
    app.restart();
}

#[tauri::command]
pub fn nudge_test(app: tauri::AppHandle, mut nudge: Nudge) {
    use tauri::Emitter;
    nudge.sanitize();
    let _ = app.emit("nudge-test", &nudge);
}