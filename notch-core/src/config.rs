//! Struktur config.toml dan aturan merapikannya. Dipakai bersama oleh Tauri (yang juga menulis file)
//! dan Slint (hanya membaca, lewat `Config::load_readonly`).

use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub fn sanitize(&mut self) {
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
    pub fn sanitize(&mut self) {
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
pub const PAGES_VERSION: u32 = 6;
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
pub fn migrate(cfg: &mut Config) {
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

/// Pintasan global aplikasi. Format kombinasi disimpan sebagai teks agar dapat diedit
/// dari halaman Pengaturan; implementasi native akan mendaftarkannya melalui Windows API.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Shortcuts {
    pub toggle_hub: String,
    pub open_settings: String,
    pub toggle_pomodoro: String,
    pub toggle_prayer: String,
}
impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            toggle_hub: "Ctrl+Alt+N".into(),
            open_settings: "Ctrl+Alt+,".into(),
            toggle_pomodoro: "Ctrl+Alt+P".into(),
            toggle_prayer: "Ctrl+Alt+A".into(),
        }
    }
}
impl Shortcuts {
    pub fn sanitize(&mut self) {
        let defaults = Shortcuts::default();
        self.toggle_hub = Self::clean(&self.toggle_hub, &defaults.toggle_hub);
        self.open_settings = Self::clean(&self.open_settings, &defaults.open_settings);
        self.toggle_pomodoro = Self::clean(&self.toggle_pomodoro, &defaults.toggle_pomodoro);
        self.toggle_prayer = Self::clean(&self.toggle_prayer, &defaults.toggle_prayer);
    }
    fn clean(value: &str, fallback: &str) -> String {
        let value = value.trim();
        if value.is_empty() || value.len() > 32 || !value.contains('+') {
            fallback.to_string()
        } else {
            value.chars().take(32).collect()
        }
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
    pub shortcuts: Shortcuts,
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
            shortcuts: Shortcuts::default(),
        }
    }
}

impl Config {
    /// Rapikan seperti saat aplikasi Tauri memulai: batas scale, daftar halaman kosong -> bawaan,
    /// bagian nudge/appearance, lalu migrasi fitur baru.
    pub fn normalize(&mut self) {
        self.scale = self.scale.clamp(0.5, 1.5);
        if self.pages.is_empty() {
            self.pages = Config::default().pages;
        }
        self.nudge.sanitize();
        self.appearance.sanitize();
        self.shortcuts.sanitize();
        migrate(self);
    }

    /// Simpan konfigurasi melalui file sementara agar file lama tidak terpotong bila gagal.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let content = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temp = path.with_extension("toml.tmp");
        std::fs::write(&temp, content)?;
        std::fs::rename(temp, path)
    }

    /// Baca config.toml TANPA menulis apa pun (beda dengan `init` di Tauri yang membuat/menambah file).
    /// File hilang atau rusak = config bawaan.
    pub fn load_readonly(path: &Path) -> Config {
        let mut cfg = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str::<Config>(&s).ok())
            .unwrap_or_default();
        cfg.normalize();
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pages_and_values() {
        let c = Config::default();
        assert_eq!(c.pages[0], "media");
        assert!(c.pages.iter().any(|p| p == "prayer"));
        assert_eq!(c.pomodoro.focus_min, 25);
        assert_eq!(c.timing.hover_open_ms, 150);
        assert_eq!(c.appearance.notch_theme, "default");\n        assert_eq!(c.shortcuts.toggle_hub, "Ctrl+Alt+N");
    }

    #[test]
    fn parses_partial_file_with_defaults() {
        let c: Config = toml::from_str("scale = 0.8\npages = [\"media\", \"timer\"]\n[pomodoro]\nrounds = 2\n").unwrap();
        assert_eq!(c.pages, ["media", "timer"]);
        assert_eq!(c.pomodoro.rounds, 2);
        assert_eq!(c.pomodoro.focus_min, 25); // tidak ditulis -> bawaan
        assert_eq!(c.timing.hover_close_ms, 450);
    }

    #[test]
    fn normalize_clamps_and_sanitizes() {
        let mut c: Config = toml::from_str("scale = 9.0\npages = []\n[appearance]\nnotch_theme = \"tidak-ada\"\n").unwrap();
        c.normalize();
        assert_eq!(c.scale, 1.5);
        assert!(!c.pages.is_empty());
        assert_eq!(c.appearance.notch_theme, "default");
    }

    #[test]
    fn migrate_adds_clock_after_media_for_old_files() {
        let mut c: Config = toml::from_str("pages = [\"media\", \"battery\"]\n").unwrap();
        c.normalize();
        assert_eq!(c.pages[1], "clock");
        assert_eq!(c.pages_version, 6);
    }

    #[test]
    fn shortcut_values_are_sanitized() {
        let mut c: Config = toml::from_str("[shortcuts]\ntoggle_hub = \"\"\nopen_settings = \"Ctrl+Shift+S\"\n").unwrap();
        c.normalize();
        assert_eq!(c.shortcuts.toggle_hub, "Ctrl+Alt+N");
        assert_eq!(c.shortcuts.open_settings, "Ctrl+Shift+S");
    }

    #[test]
    fn load_readonly_never_creates_file() {
        let p = std::env::temp_dir().join(format!("notch-core-cfg-{}", std::process::id())).join("config.toml");
        let c = Config::load_readonly(&p);
        assert!(!c.pages.is_empty());
        assert!(!p.exists() && !p.parent().unwrap().exists());
    }
}
