//! Status baterai. Pemanggil mengatur jeda polling (versi Tauri: 5 detik).

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Baterai terbaca.
    Present(Battery),
    /// Tidak ada baterai (PC desktop). Pemanggil boleh berhenti memantau.
    Absent,
    /// Gagal membaca saat ini; coba lagi pada polling berikutnya.
    Unavailable,
}

/// Tafsir `SYSTEM_POWER_STATUS` Windows (dipisah dari panggilan API agar bisa diuji di OS mana pun).
/// `flag` 128 = tidak ada baterai; `life` > 100 (255) = tidak diketahui; `ac` 1 = tersambung listrik.
pub fn from_power_status(flag: u8, life: u8, ac: u8) -> Reading {
    if flag == 128 || life > 100 {
        Reading::Absent
    } else {
        Reading::Present(Battery { percent: life, charging: ac == 1 })
    }
}

/// Tafsir isi `/sys/class/power_supply/*/{capacity,status}` Linux.
pub fn from_sysfs(capacity: &str, status: &str) -> Battery {
    Battery {
        percent: capacity.trim().parse().unwrap_or(0),
        charging: matches!(status.trim(), "Charging" | "Full"),
    }
}

pub struct BatteryMonitor {
    #[cfg(target_os = "linux")]
    dir: Option<std::path::PathBuf>,
}

impl BatteryMonitor {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            dir: find_linux_battery(),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn read(&self) -> Reading {
        let Some(dir) = &self.dir else { return Reading::Absent };
        let capacity = std::fs::read_to_string(dir.join("capacity")).unwrap_or_default();
        let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
        Reading::Present(from_sysfs(&capacity, &status))
    }

    #[cfg(target_os = "windows")]
    pub fn read(&self) -> Reading {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut s = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut s) }.is_ok() {
            from_power_status(s.BatteryFlag, s.BatteryLifePercent, s.ACLineStatus)
        } else {
            Reading::Unavailable
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub fn read(&self) -> Reading {
        Reading::Absent
    }
}

impl Default for BatteryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
fn find_linux_battery() -> Option<std::path::PathBuf> {
    std::fs::read_dir("/sys/class/power_supply")
        .ok()
        .and_then(|d| {
            d.flatten().find(|e| {
                std::fs::read_to_string(e.path().join("type"))
                    .map(|t| t.trim() == "Battery")
                    .unwrap_or(false)
                    && e.path().join("capacity").exists()
            })
        })
        .map(|e| e.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_status_mapping() {
        assert_eq!(from_power_status(128, 0, 1), Reading::Absent);
        assert_eq!(from_power_status(0, 255, 0), Reading::Absent);
        assert_eq!(
            from_power_status(1, 80, 1),
            Reading::Present(Battery { percent: 80, charging: true })
        );
        assert_eq!(
            from_power_status(0, 42, 0),
            Reading::Present(Battery { percent: 42, charging: false })
        );
    }

    #[test]
    fn sysfs_parsing() {
        assert_eq!(from_sysfs("57\n", "Charging\n"), Battery { percent: 57, charging: true });
        assert_eq!(from_sysfs("100", "Full"), Battery { percent: 100, charging: true });
        assert_eq!(from_sysfs("20", "Discharging"), Battery { percent: 20, charging: false });
        assert_eq!(from_sysfs("abc", ""), Battery { percent: 0, charging: false });
    }
}
