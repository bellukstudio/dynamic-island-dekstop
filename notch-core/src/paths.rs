//! Lokasi data aplikasi. Dipakai bersama oleh UI Tauri dan Slint supaya keduanya membaca
//! file yang sama (config.toml, notes.json, reminders.json, ...).

use std::path::PathBuf;

/// Path `config.toml`. File data lain disimpan di sampingnya lewat `with_file_name`.
pub fn config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));

    base.unwrap_or_else(|| PathBuf::from("."))
        .join("notch")
        .join("config.toml")
}

/// Path file data di samping config.toml, mis. `data_file("notes.json")`.
pub fn data_file(name: &str) -> PathBuf {
    config_path().with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_file_sits_next_to_config() {
        let cfg = config_path();
        assert_eq!(cfg.file_name().unwrap(), "config.toml");
        assert_eq!(data_file("notes.json").parent(), cfg.parent());
        assert_eq!(data_file("notes.json").file_name().unwrap(), "notes.json");
    }
}
