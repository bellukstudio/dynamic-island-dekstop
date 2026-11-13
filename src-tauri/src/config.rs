use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::OnceLock};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Weather {
    pub lat: f64,
    pub lon: f64,
    pub city: String,
}
impl Default for Weather {
    fn default() -> Self {
        Self { lat: -6.9175, lon: 107.6191, city: "Bandung".into() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Timing {
    pub hub_auto_ms: u32,    // lama hub terbuka otomatis (lagu baru, alert hujan)
    pub card_ms: u32,        // lama kartu bluetooth / perangkat
    pub hud_ms: u32,         // lama HUD volume / brightness
    pub hover_open_ms: u32,  // jeda sebelum hub terbuka saat hover
    pub hover_close_ms: u32, // jeda sebelum hub menutup setelah kursor keluar
    pub sleep_after_s: u32,  // mata mengantuk setelah tanpa aktivitas
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            hub_auto_ms: 4000,
            card_ms: 3500,
            hud_ms: 1500,
            hover_open_ms: 150,
            hover_close_ms: 450,
            sleep_after_s: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Pomodoro {
    pub focus_min: u32,
    pub break_min: u32,
    pub long_break_min: u32,
    pub rounds: u32,
}
impl Default for Pomodoro {
    fn default() -> Self {
        Self { focus_min: 25, break_min: 5, long_break_min: 15, rounds: 4 }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,      // "auto" (ikut OS) | "dark" | "light" | "custom"
    pub background: String, // dipakai kalau theme = "custom", mis. "#1e1e2e"
    pub text: String,       // kosong = otomatis hitam/putih sesuai kontras
    pub dark_bg: String,    // warna notch saat tema gelap
    pub light_bg: String,   // warna notch saat tema terang
    pub accent: String,     // kosong = otomatis dari cover album
    pub default_mood: String, // ekspresi mata bawaan saat idle (tidak tidur/tidak memutar lagu)
    pub eye_theme: String, // lihat EYE_THEMES di bawah untuk daftar lengkap (20 skin)
    pub notch_theme: String, // tema badan notch: default | aurora | obsidian | neon | sakura | matcha
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "auto".into(),
            background: "#000000".into(),
            text: String::new(),
            dark_bg: "#000000".into(),
            light_bg: "#f5f5f7".into(),
            accent: String::new(),
            default_mood: "normal".into(),
            eye_theme: "aurora".into(),
            notch_theme: "default".into(),
        }
    }
}

// dipakai bersama oleh Appearance::sanitize dan Nudge::sanitize
const MOODS: &[&str] =
    &["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"];
const EYE_THEMES: &[&str] = &["aurora", "neon", "pastel", "pixel", "holo", "ember", "ocean", "sakura", "cyber", "monochrome",
    "kawaii", "boba", "galaxy", "matcha", "candy", "synthwave", "slime", "cotton", "pumpkin", "frost"];

// "default" = tampilan lama (warna dari theme/dark_bg/light_bg/background). Selain itu: tema siap pakai (css/notch-themes.css).
const NOTCH_THEMES: &[&str] = &["default", "aurora", "obsidian", "neon", "sakura", "matcha"];

impl Appearance {
    fn sanitize(&mut self) {
        if !NOTCH_THEMES.contains(&self.notch_theme.as_str()) {
            self.notch_theme = Appearance::default().notch_theme;
        }
        if !MOODS.contains(&self.default_mood.as_str()) {
            self.default_mood = Appearance::default().default_mood;
        }
        if !EYE_THEMES.contains(&self.eye_theme.as_str()) {
            self.eye_theme = Appearance::default().eye_theme;
        }
    }
}

// Pesan imut saat pointer lama diam ("Kamu kemana, kok diam saja?").
// Logikanya ada di frontend (src/nudge.ts); di sini hanya menyimpan dan merapikan nilainya.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Nudge {
    pub enabled: bool,
    pub idle_min: u32,       // menit pointer diam sebelum pesan muncul (1..240)
    pub message: String,     // maksimal 80 karakter
    pub emoji: String,       // ikon pesan
    pub mood: String,        // ekspresi mata: normal|happy|sleepy|surprised|angry|love|suspicious|sad
    pub show_ms: u32,        // lama pesan tampil (2000..30000)
    pub only_silent: bool,   // jangan muncul saat musik/video diputar
}
impl Default for Nudge {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_min: 2,
            message: "Kamu kemana, kok diam saja?".into(),
            emoji: "🥺".into(),
            mood: "sad".into(),
            show_ms: 6000,
            only_silent: true,
        }
    }
}

impl Nudge {
    // nilai dari file / UI bisa kosong atau di luar batas: rapikan ke yang aman
    fn sanitize(&mut self) {
        let d = Nudge::default();
        self.idle_min = self.idle_min.clamp(1, 240);
        self.show_ms = self.show_ms.clamp(2_000, 30_000);
        self.message = self.message.trim().chars().take(80).collect();
        if self.message.is_empty() {
            self.message = d.message;
        }
        self.emoji = self.emoji.trim().chars().take(12).collect();
        if self.emoji.is_empty() {
            self.emoji = d.emoji;
        }
        if !MOODS.contains(&self.mood.as_str()) {
            self.mood = d.mood;
        }
    }
}

/// Preferensi jadwal shalat dan pengingat waktu shalat.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Prayer {
    pub provinsi: String,
    pub kabkota: String,
    pub mode: String,          // "adzan" | "tone" | "silent"
    pub remind_before: bool,
    pub remind_min: u32,
}
impl Default for Prayer {
    fn default() -> Self {
        Self {
            provinsi: "Jawa Barat".into(),
            kabkota: "Kota Bandung".into(),
            mode: "tone".into(),
            remind_before: false,
            remind_min: 10,
        }
    }
}

// Naikkan angka ini setiap kali ada fitur baru yang perlu dinyalakan untuk config lama.
const PAGES_VERSION: u32 = 6;
// (nama fitur, versi PAGES_VERSION saat fitur itu ditambahkan).
// Dengan begitu fitur lama yang sengaja dimatikan pengguna tidak ikut menyala lagi
// ketika versi dinaikkan untuk fitur baru.
const NEW_FEATURES: &[(&str, u32)] = &[
    ("shelf", 2),
    ("lyrics", 2),
    ("privacy", 2),
    ("clock", 3),
    ("reminder", 4),
    ("notes", 4),
    ("prayer", 5),
];

// Config lama mendapat fitur baru satu kali. Setelah disimpan dari UI, angka versinya
// ikut tersimpan, jadi pilihan pengguna tidak ditimpa lagi.
fn migrate(cfg: &mut Config) {
    if cfg.pages_version < PAGES_VERSION {
        for (f, since) in NEW_FEATURES {
            if *since > cfg.pages_version && !cfg.pages.iter().any(|p| p == f) {
                // jam diletakkan tepat setelah musik, fitur lain di akhir daftar
                let at = if *f == "clock" {
                    cfg.pages.iter().position(|p| p == "media").map(|i| i + 1)
                } else if *f == "prayer" {
                    cfg.pages.iter().position(|p| p == "clock").map(|i| i + 1)
                } else {
                    None
                };
                match at {
                    Some(i) => cfg.pages.insert(i, (*f).to_string()),
                    None => cfg.pages.push((*f).to_string()),
                }
            }
        }
        // Prayer sebelumnya ditambahkan di akhir daftar pada versi awal patch.
        // Pindahkan satu kali setelah Jam agar ikon lebih mudah terlihat di navbar.
        if let Some(old_index) = cfg.pages.iter().position(|p| p == "prayer") {
            let prayer = cfg.pages.remove(old_index);
            let at = cfg.pages.iter().position(|p| p == "clock")
                .map(|i| i + 1)
                .unwrap_or(cfg.pages.len());
            cfg.pages.insert(at, prayer);
        }
        cfg.pages_version = PAGES_VERSION;
    }
}

// Posisi notch di layar: jika custom = false, notch berada di posisi default (tengah atas).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Position {
    pub custom: bool, // true jika posisi kustom (pernah digeser), false jika default (tengah atas)
    pub x: i32,       // koordinat tengah horizontal (center_x)
    pub y: i32,       // koordinat atas (top_y)
}
impl Default for Position {
    fn default() -> Self {
        Self {
            custom: false,
            x: 0,
            y: 0,
        }
    }
}

// Catatan: di TOML, nilai biasa (scale, hide_fullscreen, pages, pages_version) harus berada
// SEBELUM tabel (weather, timing, pomodoro, appearance, nudge, position), jadi urutan field di sini penting.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub scale: f64,            // 1.0 = normal, 0.8 = 80%
    pub hide_fullscreen: bool, // sembunyikan notch saat game/video fullscreen
    pub pages: Vec<String>,    // fitur aktif + urutan halaman hub
    // 0 = file lama (belum pernah dimigrasi); diisi PAGES_VERSION saat disimpan dari UI.
    // #[serde(default)] di level field memakai 0, bukan nilai dari Config::default().
    #[serde(default)]
    pub pages_version: u32,
    pub weather: Weather,
    pub timing: Timing,
    pub pomodoro: Pomodoro,
    pub appearance: Appearance,
    pub nudge: Nudge,
    pub position: Position,
    pub prayer: Prayer,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 1.0,
            hide_fullscreen: true,
            pages: [
                "media", "clock", "weather", "battery", "timer", "quick", "tools", "stats",
                "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            pages_version: PAGES_VERSION,
            weather: Weather::default(),
            timing: Timing::default(),
            pomodoro: Pomodoro::default(),
            appearance: Appearance::default(),
            nudge: Nudge::default(),
            position: Position::default(),
            prayer: Prayer::default(),
        }
    }
}

static CFG: OnceLock<Config> = OnceLock::new();

pub fn path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));

    base.unwrap_or_else(|| PathBuf::from(".")).join("notch").join("config.toml")
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