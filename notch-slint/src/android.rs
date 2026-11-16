#[path = "android_adb.rs"]
mod adb;
use std::path::{Path, PathBuf};
use adb::Adb;

pub struct AndroidState {
    adb: Option<Adb>,
    serial: String,
}
impl AndroidState {
    pub fn new() -> Self {
        let path = adb::find(None);
        Self { adb: path.map(Adb::new), serial: String::new() }
    }
    pub fn status(&mut self) -> String {
        self.adb = adb::find(None).map(Adb::new);
        match &self.adb {
            Some(a) => format!("ADB ditemukan: {}", a.path.display()),
            None => "ADB tidak ditemukan. Pasang Android Platform Tools atau atur ADB_PATH.".into(),
        }
    }
    pub fn devices(&mut self) -> String {
        let Some(adb) = self.adb.as_ref() else { return self.status() };
        match adb.devices() {
            Ok(devices) if devices.is_empty() => "Belum ada perangkat. Aktifkan USB debugging dan izinkan komputer ini.".into(),
            Ok(devices) => {
                let mut lines = Vec::new();
                for d in &devices {
                    lines.push(format!("{} | {} | {}{}", d.serial, d.model, d.state, if d.wifi { " | Wi-Fi" } else { "" }));
                }
                if self.serial.is_empty() {
                    if let Some(ready) = devices.iter().find(|device| device.state == "device") {
                        self.serial = ready.serial.clone();
                    } else if let Some(first) = devices.first() {
                        self.serial = first.serial.clone();
                    }
                }
                lines.join("\n")
            }
            Err(e) => format!("Gagal membaca perangkat: {e}"),
        }
    }
    pub fn set_serial(&mut self, serial: String) { self.serial = serial.trim().to_string(); }
    pub fn serial(&self) -> &str { &self.serial }
    pub fn list(&self, path: &str) -> String {
        let Some(adb) = self.adb.as_ref() else { return "ADB belum tersedia".into() };
        if self.serial.is_empty() { return "Pilih perangkat terlebih dahulu".into(); }
        match adb.list_dir(&self.serial, path) {
            Ok(entries) if entries.is_empty() => "Folder kosong".into(),
            Ok(entries) => entries.into_iter().map(|e| format!("{}{}  {}", if e.is_dir { "📁 " } else { "📄 " }, e.name, e.size)).collect::<Vec<_>>().join("\n"),
            Err(e) => format!("Gagal membaca folder: {e}"),
        }
    }
    pub fn pull(&self, remote: &str, destination: &Path) -> Result<(), String> {
        let adb = self.adb.as_ref().ok_or("ADB belum tersedia")?;
        if self.serial.is_empty() { return Err("Pilih perangkat terlebih dahulu".into()); }
        adb.pull(&self.serial, remote, destination)
    }
    pub fn push(&self, local: &Path, remote: &str) -> Result<(), String> {
        let adb = self.adb.as_ref().ok_or("ADB belum tersedia")?;
        if self.serial.is_empty() { return Err("Pilih perangkat terlebih dahulu".into()); }
        adb.push(&self.serial, local, remote)
    }
}
