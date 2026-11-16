use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

#[path = "transfer_server.rs"]\nmod transfer_server;

type TransferFiles = transfer_server::Files;

pub struct TransferState {
    runtime: Option<Runtime>,
    running: Option<transfer_server::Running>,
    files: TransferFiles,
    inbox: PathBuf,
}

impl TransferState {
    pub fn new(inbox: PathBuf) -> Self {
        Self {
            runtime: None,
            running: None,
            files: Arc::new(Mutex::new(Vec::new())),
            inbox,
        }
    }

    pub fn add_files(&self, paths: Vec<String>) -> usize {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        for raw in paths {
            let path = PathBuf::from(&raw);
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if !meta.is_file() || files.iter().any(|f| f.path == raw) { continue; }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            let id = files.iter().map(|f| f.id).max().unwrap_or(0).saturating_add(1);
            files.push(transfer_server::SharedFile { id, name, size: meta.len(), path: raw });
        }
        files.len()
    }

    pub fn start(&mut self) -> Result<String, String> {
        if let Some(running) = self.running.as_ref() {
            let ip = transfer_server::lan_ips().first().copied().unwrap_or(std::net::Ipv4Addr::LOCALHOST);
            return Ok(format!("http://{ip}:{}/s/{}/", running.port, running.token));
        }
        std::fs::create_dir_all(&self.inbox).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Runtime jaringan gagal: {e}"))?;
        let emit: transfer_server::Emit = Arc::new(|_| {});
        let running = runtime.block_on(transfer_server::start(self.files.clone(), self.inbox.clone(), emit))?;
        let ip = transfer_server::lan_ips().first().copied().unwrap_or(std::net::Ipv4Addr::LOCALHOST);
        let url = format!("http://{ip}:{}/s/{}/", running.port, running.token);
        self.runtime = Some(runtime);
        self.running = Some(running);
        Ok(url)
    }

    pub fn stop(&mut self) {
        self.running.take();
        self.runtime.take();
    }

    pub fn is_running(&self) -> bool { self.running.is_some() }
}

impl Drop for TransferState {
    fn drop(&mut self) { self.stop(); }
}

pub fn open_inbox(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}
