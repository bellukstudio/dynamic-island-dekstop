use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct MirrorProcess {
    child: Option<Child>,
}
impl MirrorProcess {
    pub fn new() -> Self { Self { child: None } }

    fn find_scrcpy() -> Option<PathBuf> {
        let exe = if cfg!(target_os = "windows") { "scrcpy.exe" } else { "scrcpy" };
        let mut candidates = Vec::new();
        if let Some(value) = std::env::var_os("SCRCPY_PATH") { candidates.push(PathBuf::from(value)); }
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                candidates.push(dir.join("scrcpy").join(exe));
                candidates.push(dir.join("resources").join("scrcpy").join(exe));
                candidates.push(dir.join(exe));
            }
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) { candidates.push(dir.join(exe)); }
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    pub fn start(&mut self, serial: &str) -> Result<String, String> {
        if let Some(child) = self.child.as_mut() {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Ok("Jendela mirroring sudah berjalan".into());
            }
            self.child = None;
        }
        if serial.trim().is_empty() { return Err("Pilih perangkat ADB terlebih dahulu".into()); }
        let scrcpy = Self::find_scrcpy().ok_or_else(|| "scrcpy tidak ditemukan. Pasang scrcpy dan tambahkan ke PATH, atau atur SCRCPY_PATH.".to_string())?;
        let mut command = Command::new(&scrcpy);
        command.args(["-s", serial, "--window-title", "Notch · Android Mirror"]);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| format!("Gagal menjalankan scrcpy: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Some(status) = child.try_wait().map_err(|e| format!("Gagal memeriksa scrcpy: {e}"))? {
            return Err(format!("scrcpy langsung berhenti ({status}). Pastikan USB debugging aktif dan perangkat berstatus \"device\"."));
        }
        self.child = Some(child);
        Ok(format!("Mirroring dimulai untuk {serial}. Kontrol layar melalui jendela scrcpy."))
    }

    pub fn stop(&mut self) -> Result<String, String> {
        if let Some(mut child) = self.child.take() {
            match child.try_wait() {
                Ok(Some(_)) => return Ok("Jendela mirroring sudah ditutup".into()),
                Ok(None) => child.kill().map_err(|e| format!("Gagal menghentikan scrcpy: {e}"))?,
                Err(e) => return Err(e.to_string()),
            }
            let _ = child.wait();
            Ok("Mirroring dihentikan".into())
        } else {
            Ok("Tidak ada sesi mirroring yang dikelola Notch".into())
        }
    }
}
impl Drop for MirrorProcess {
    fn drop(&mut self) { let _ = self.stop(); }
}

pub fn scrcpy_exists() -> bool { MirrorProcess::find_scrcpy().is_some() }
