//! Jalan otomatis saat komputer menyala, tanpa plugin tambahan dan tanpa proses pendamping:
//!  - Windows: nilai "Notch" di HKCU\...\Run (lewat `reg`, tanpa jendela konsol)
//!  - Linux:   ~/.config/autostart/notch.desktop
//! Tidak ada yang berjalan di latar belakang; OS yang menjalankan aplikasi saat login.

// Path yang harus dijalankan OS saat login.
// Di AppImage, current_exe() menunjuk ke folder sementara yang hilang setelah ditutup,
// jadi dipakai path file .AppImage asli (variabel APPIMAGE).
fn target_exe() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Ok(p.to_string_lossy().to_string());
    }
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

// ======================= Windows =======================
#[cfg(target_os = "windows")]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    const NO_WINDOW: u32 = 0x0800_0000;
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Notch";

    fn reg(args: &[&str]) -> Option<Output> {
        Command::new("reg").args(args).creation_flags(NO_WINDOW).output().ok()
    }

    // aktif hanya jika entri ada DAN menunjuk ke exe ini (kalau file dipindah, dianggap belum aktif)
    pub fn is_enabled(exe: &str) -> bool {
        match reg(&["query", KEY, "/v", NAME]) {
            Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains(&exe.to_lowercase()),
            _ => false,
        }
    }

    pub fn set(on: bool, exe: &str) -> Result<(), String> {
        let out = if on {
            let value = format!("\"{exe}\"");
            reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"])
        } else {
            reg(&["delete", KEY, "/v", NAME, "/f"])
        };
        match out {
            Some(o) if o.status.success() => Ok(()),
            // menghapus entri yang memang tidak ada bukan kesalahan
            Some(_) if !on && reg(&["query", KEY, "/v", NAME]).map(|o| !o.status.success()).unwrap_or(false) => Ok(()),
            Some(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            None => Err("perintah reg gagal dijalankan".into()),
        }
    }
}

// ======================= Linux =======================
#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart").join("notch.desktop"))
    }

    pub fn is_enabled(exe: &str) -> bool {
        file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|s| s.contains(exe))
            .unwrap_or(false)
    }

    pub fn set(on: bool, exe: &str) -> Result<(), String> {
        let f = file().ok_or("folder konfigurasi tidak ditemukan")?;
        if on {
            if let Some(dir) = f.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Notch\nExec=\"{exe}\"\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
            );
            std::fs::write(&f, entry).map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(&f) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    pub fn is_enabled(_exe: &str) -> bool { false }
    pub fn set(_on: bool, _exe: &str) -> Result<(), String> {
        Err("belum didukung di sistem operasi ini".into())
    }
}

#[tauri::command]
pub fn autostart_get() -> bool {
    target_exe().map(|e| imp::is_enabled(&e)).unwrap_or(false)
}

// Mengembalikan keadaan sebenarnya setelah diubah.
#[tauri::command]
pub fn autostart_set(on: bool) -> Result<bool, String> {
    // `tauri dev` menjalankan exe debug yang butuh server dev: jangan didaftarkan ke OS
    if cfg!(debug_assertions) {
        return Err("hanya tersedia di build release (bukan `tauri dev`)".into());
    }
    let exe = target_exe()?;
    imp::set(on, &exe)?;
    Ok(imp::is_enabled(&exe))
}