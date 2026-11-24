//! Native OS auto-start integration for the Slint build.

fn target_exe() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    if let Some(path) = std::env::var_os("APPIMAGE") {
        return Ok(path.to_string_lossy().into_owned());
    }
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    const NO_WINDOW: u32 = 0x0800_0000;
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Notch";

    fn reg(args: &[&str]) -> Option<Output> {
        Command::new("reg")
            .args(args)
            .creation_flags(NO_WINDOW)
            .output()
            .ok()
    }

    pub fn is_enabled(exe: &str) -> bool {
        match reg(&["query", KEY, "/v", NAME]) {
            Some(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains(&exe.to_lowercase()),
            _ => false,
        }
    }

    pub fn set(enabled: bool, exe: &str) -> Result<(), String> {
        let output = if enabled {
            let value = format!("\"{exe}\"");
            reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"])
        } else {
            reg(&["delete", KEY, "/v", NAME, "/f"])
        };
        match output {
            Some(output) if output.status.success() => Ok(()),
            Some(_) if !enabled && reg(&["query", KEY, "/v", NAME]).map(|o| !o.status.success()).unwrap_or(false) => Ok(()),
            Some(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Err(if !stderr.is_empty() { stderr } else if !stdout.is_empty() { stdout } else { format!("reg keluar dengan kode {:?}", output.status.code()) })
            },
            None => Err("perintah reg gagal dijalankan".into()),
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("autostart").join("notch.desktop"))
    }

    pub fn is_enabled(exe: &str) -> bool {
        file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|content| content.contains(exe))
            .unwrap_or(false)
    }

    pub fn set(enabled: bool, exe: &str) -> Result<(), String> {
        let path = file().ok_or("folder konfigurasi tidak ditemukan")?;
        if enabled {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Notch\nExec=\"{exe}\"\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
            );
            std::fs::write(path, entry).map_err(|error| error.to_string())
        } else {
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    pub fn is_enabled(_exe: &str) -> bool { false }
    pub fn set(_enabled: bool, _exe: &str) -> Result<(), String> {
        Err("belum didukung di sistem operasi ini".into())
    }
}

pub fn is_enabled() -> bool {
    target_exe().map(|exe| imp::is_enabled(&exe)).unwrap_or(false)
}

pub fn set(enabled: bool) -> Result<bool, String> {
    // Debug dan release sama-sama boleh menguji autostart. Nilai Run selalu menunjuk
    // executable yang sedang berjalan; build release tetap disarankan untuk pemakaian harian.
    let exe = target_exe()?;
    imp::set(enabled, &exe)?;
    Ok(imp::is_enabled(&exe))
}
