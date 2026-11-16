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

/// Gerakan organik notch ketika sedang idle.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Motion {
    pub enabled: bool,
    pub interval_secs: u32,
    pub max_moves: u32,
}
impl Default for Motion {
    fn default() -> Self {
        Self { enabled: false, interval_secs: 180, max_moves: 3 }
    }
}

impl Motion {
    /// Batasi nilai dari config lama atau hasil edit manual agar timer tetap aman.
    pub fn sanitize(&mut self) {
        self.interval_secs = self.interval_secs.clamp(60, 3_600);
        self.max_moves = self.max_moves.clamp(1, 10);
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

impl Pomodoro {
    pub fn sanitize(&mut self) {
        self.focus_min = self.focus_min.clamp(1, 180);
        self.break_min = self.break_min.clamp(1, 60);
        self.long_break_min = self.long_break_min.clamp(1, 120);
        self.rounds = self.rounds.clamp(1, 12);
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

const APPEARANCE_THEMES: &[&str] = &["auto", "dark", "light", "custom"];

impl Appearance {
    pub fn sanitize(&mut self) {
        if !APPEARANCE_THEMES.contains(&self.theme.as_str()) {
            self.theme = Appearance::default().theme;
        }
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
    pub until_move: bool,    // tahan pesan sampai pointer bergerak, lalu tampilkan kartu kembali
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
            until_move: false,
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

impl Prayer {
    pub fn sanitize(&mut self) {
        if !["adzan", "tone", "silent"].contains(&self.mode.as_str()) {
            self.mode = Prayer::default().mode;
        }
        self.remind_min = self.remind_min.clamp(1, 120);
    }
}

/// Sapaan suara ketika aplikasi Notch mulai berjalan.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Greeting {
    pub enabled: bool,
    pub mode: String,
    pub custom_text: String,
    pub voice: String,
}
impl Default for Greeting {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "random".into(),
            custom_text: "Halo, semangat menjalani hari!".into(),
            voice: "cewek".into(),
        }
    }
}
impl Greeting {
    pub fn sanitize(&mut self) {
        if !["random", "auto", "halo", "morning", "noon", "afternoon", "evening", "custom"].contains(&self.mode.as_str()) {
            self.mode = Greeting::default().mode;
        }
        if !["cewek", "cowok", "anime-cewek", "anime-cowok", "system"].contains(&self.voice.as_str()) {
            self.voice = Greeting::default().voice;
        }
        self.custom_text = self.custom_text.trim().chars().take(120).collect();
        if self.custom_text.is_empty() {
            self.custom_text = Greeting::default().custom_text;
        }
    }
}

/// Preferensi komentar otomatis berdasarkan aktivitas dan kondisi perangkat.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ConversationMoments {
    pub enabled: bool,
    pub on_play: bool,
    pub on_track_change: bool,
    pub on_pause: bool,
    pub on_return: bool,
    pub on_weather_change: bool,
    pub on_rain_soon: bool,
    pub on_low_battery: bool,
    pub on_charging: bool,
    pub return_after_min: u32,
    pub cooldown_min: u32,
}
impl Default for ConversationMoments {
    fn default() -> Self {
        Self {
            enabled: true,
            on_play: true,
            on_track_change: true,
            on_pause: false,
            on_return: true,
            on_weather_change: true,
            on_rain_soon: true,
            on_low_battery: true,
            on_charging: false,
            return_after_min: 10,
            cooldown_min: 3,
        }
    }
}
impl ConversationMoments {
    pub fn sanitize(&mut self) {
        self.return_after_min = self.return_after_min.clamp(5, 120);
        self.cooldown_min = self.cooldown_min.clamp(1, 60);
    }
}

/// Momen ketika Focus Companion memberi dorongan selama sesi Pomodoro.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct FocusCompanion {
    pub enabled: bool,
    pub on_start: bool,
    pub on_halfway: bool,
    pub on_focus_complete: bool,
    pub on_break_end: bool,
}
impl Default for FocusCompanion {
    fn default() -> Self {
        Self {
            enabled: true,
            on_start: true,
            on_halfway: true,
            on_focus_complete: true,
            on_break_end: true,
        }
    }
}

// Naikkan angka ini setiap kali ada fitur baru yang perlu dinyalakan untuk config lama.
pub const PAGES_VERSION: u32 = 7;
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
    ("currency", 7),
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
    pub motion: Motion,
    pub pomodoro: Pomodoro,
    pub appearance: Appearance,
    pub nudge: Nudge,
    pub position: Position,
    pub prayer: Prayer,
    pub shortcuts: Shortcuts,
    pub greeting: Greeting,
    pub conversation_moments: ConversationMoments,
    pub focus_companion: FocusCompanion,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 1.0,
            hide_fullscreen: true,
            pages: [
                "media", "clock", "weather", "battery", "timer", "quick", "tools", "stats",
                "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "currency",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            pages_version: PAGES_VERSION,
            weather: Weather::default(),
            timing: Timing::default(),
            motion: Motion::default(),
            pomodoro: Pomodoro::default(),
            appearance: Appearance::default(),
            nudge: Nudge::default(),
            position: Position::default(),
            prayer: Prayer::default(),
            shortcuts: Shortcuts::default(),
            greeting: Greeting::default(),
            conversation_moments: ConversationMoments::default(),
            focus_companion: FocusCompanion::default(),
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
        self.motion.sanitize();
        self.pomodoro.sanitize();
        self.prayer.sanitize();
        self.appearance.sanitize();
        self.shortcuts.sanitize();
        self.greeting.sanitize();
        self.conversation_moments.sanitize();
        migrate(self);
    }

    /// Simpan konfigurasi melalui file sementara lalu ganti file lama secara atomik.
    /// Windows membutuhkan MoveFileExW karena std::fs::rename tidak menimpa tujuan yang sudah ada.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io;
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let content = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let temp = path.with_extension("toml.tmp");
        std::fs::write(&temp, content)?;

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: both UTF-16 buffers are NUL-terminated and live through the call.
            if let Err(error) = unsafe {
                MoveFileExW(
                    PCWSTR(from.as_ptr()),
                    PCWSTR(to.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } {
                let _ = std::fs::remove_file(&temp);
                return Err(io::Error::other(error));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            std::fs::rename(&temp, path)
        }
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
        assert_eq!(c.appearance.notch_theme, "default");
        assert_eq!(c.appearance.theme, "auto");
        assert_eq!(c.shortcuts.toggle_hub, "Ctrl+Alt+N");
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
    fn automation_preferences_have_defaults_and_sanitize_invalid_values() {
        let defaults = Config::default();
        assert_eq!(defaults.greeting.mode, "random");
        assert_eq!(defaults.greeting.voice, "cewek");
        assert!(defaults.conversation_moments.enabled);
        assert!(defaults.focus_companion.enabled);

        let mut configured: Config = toml::from_str(
            "[greeting]\nmode = \"unknown\"\nvoice = \"unknown\"\n[conversation_moments]\nreturn_after_min = 0\ncooldown_min = 90\n",
        ).unwrap();
        configured.normalize();
        assert_eq!(configured.greeting.mode, "random");
        assert_eq!(configured.greeting.voice, "cewek");
        assert_eq!(configured.conversation_moments.return_after_min, 5);
        assert_eq!(configured.conversation_moments.cooldown_min, 60);
    }

    #[test]
    fn automation_preferences_round_trip_through_toml() {
        let mut config = Config::default();
        config.greeting.enabled = false;
        config.greeting.mode = "custom".into();
        config.greeting.voice = "anime-cowok".into();
        config.greeting.custom_text = "Selamat bekerja!".into();
        config.conversation_moments.enabled = false;
        config.conversation_moments.on_track_change = false;
        config.conversation_moments.return_after_min = 25;
        config.conversation_moments.cooldown_min = 7;
        config.focus_companion.enabled = false;
        config.focus_companion.on_halfway = false;

        let serialized = toml::to_string(&config).unwrap();
        let restored: Config = toml::from_str(&serialized).unwrap();

        assert!(!restored.greeting.enabled);
        assert_eq!(restored.greeting.mode, "custom");
        assert_eq!(restored.greeting.voice, "anime-cowok");
        assert_eq!(restored.greeting.custom_text, "Selamat bekerja!");
        assert!(!restored.conversation_moments.enabled);
        assert!(!restored.conversation_moments.on_track_change);
        assert_eq!(restored.conversation_moments.return_after_min, 25);
        assert_eq!(restored.conversation_moments.cooldown_min, 7);
        assert!(!restored.focus_companion.enabled);
        assert!(!restored.focus_companion.on_halfway);
    }

    #[test]
    fn nudge_until_move_defaults_off_and_can_be_loaded() {
        let defaults: Config = toml::from_str("").unwrap();
        assert!(!defaults.nudge.until_move);

        let configured: Config = toml::from_str("[nudge]\nuntil_move = true\n").unwrap();
        assert!(configured.nudge.until_move);
    }

    #[test]
    fn migrate_adds_currency_to_older_page_configs() {
        let mut c: Config = toml::from_str("pages = [\"media\", \"battery\"]\n").unwrap();
        c.normalize();
        assert!(c.pages.iter().any(|p| p == "currency"));
        assert_eq!(c.pages_version, PAGES_VERSION);
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
    fn normalize_rejects_unknown_appearance_theme() {
        let mut c: Config = toml::from_str("[appearance]\ntheme = \"ultraviolet\"\n").unwrap();
        c.normalize();
        assert_eq!(c.appearance.theme, "auto");
    }

    #[test]
    fn migrate_adds_clock_after_media_for_old_files() {
        let mut c: Config = toml::from_str("pages = [\"media\", \"battery\"]\n").unwrap();
        c.normalize();
        assert_eq!(c.pages[1], "clock");
        assert_eq!(c.pages_version, PAGES_VERSION);
        assert!(c.pages.iter().any(|p| p == "currency"));
    }

    #[test]
    fn normalize_clamps_prayer_and_pomodoro_preferences() {
        let mut c: Config = toml::from_str("[pomodoro]\nfocus_min = 0\nbreak_min = 90\nlong_break_min = 200\nrounds = 0\n[prayer]\nmode = \"invalid\"\nremind_min = 999\n").unwrap();
        c.normalize();
        assert_eq!(c.pomodoro.focus_min, 1);
        assert_eq!(c.pomodoro.break_min, 60);
        assert_eq!(c.pomodoro.long_break_min, 120);
        assert_eq!(c.pomodoro.rounds, 1);
        assert_eq!(c.prayer.mode, "tone");
        assert_eq!(c.prayer.remind_min, 120);
    }

    #[test]
    fn shortcut_values_are_sanitized() {
        let mut c: Config = toml::from_str("[shortcuts]\ntoggle_hub = \"\"\nopen_settings = \"Ctrl+Shift+S\"\n").unwrap();
        c.normalize();
        assert_eq!(c.shortcuts.toggle_hub, "Ctrl+Alt+N");
        assert_eq!(c.shortcuts.open_settings, "Ctrl+Shift+S");
    }

    #[test]
    fn save_replaces_existing_config_file() {
        let dir = std::env::temp_dir().join(format!("notch-core-save-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "scale = 0.5\\n").unwrap();

        let mut config = Config::default();
        config.scale = 1.25;
        config.save(&path).unwrap();
        let saved: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.scale, 1.25);
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_readonly_never_creates_file() {
        let p = std::env::temp_dir().join(format!("notch-core-cfg-{}", std::process::id())).join("config.toml");
        let c = Config::load_readonly(&p);
        assert!(!c.pages.is_empty());
        assert!(!p.exists() && !p.parent().unwrap().exists());
    }
}
