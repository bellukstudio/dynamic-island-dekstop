use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item { pub path: String, pub name: String, pub dir: bool }

pub struct Store { path: PathBuf, items: Vec<Item> }

impl Store {
    pub fn open(path: PathBuf) -> Self {
        let items = std::fs::read_to_string(&path)
            .ok().and_then(|raw| serde_json::from_str::<Vec<Item>>(&raw).ok())
            .unwrap_or_default().into_iter()
            .filter(|item| Path::new(&item.path).exists()).collect();
        Self { path, items }
    }
    fn save(&self) {
        if let Some(parent) = self.path.parent() { let _ = std::fs::create_dir_all(parent); }
        if let Ok(raw) = serde_json::to_string_pretty(&self.items) { let _ = std::fs::write(&self.path, raw); }
    }
    pub fn list(&self) -> Vec<Item> { self.items.clone() }
    pub fn add(&mut self, paths: Vec<String>) -> Vec<Item> {
        for raw in paths {
            let path = PathBuf::from(&raw);
            if !path.exists() { continue; }
            let name = path.file_name().map(|v| v.to_string_lossy().to_string()).unwrap_or_else(|| raw.clone());
            self.items.retain(|item| item.path != raw);
            self.items.insert(0, Item { path: raw, name, dir: path.is_dir() });
        }
        self.items.truncate(30);
        self.items.retain(|item| Path::new(&item.path).exists());
        self.save();
        self.list()
    }
    pub fn remove(&mut self, path: &str) -> Vec<Item> {
        self.items.retain(|item| item.path != path); self.save(); self.list()
    }
    pub fn clear(&mut self) -> Vec<Item> { self.items.clear(); self.save(); self.list() }
}
fn spawn(mut command: Command) {
    if let Ok(mut child) = command.spawn() { std::thread::spawn(move || { let _ = child.wait(); }); }
}
pub fn open(path: &str) {
    #[cfg(target_os = "windows")]
    let mut command = { let mut c = Command::new("explorer"); c.arg(path); c };
    #[cfg(target_os = "linux")]
    let mut command = { let mut c = Command::new("xdg-open"); c.arg(path); c };
    #[cfg(target_os = "macos")]
    let mut command = { let mut c = Command::new("open"); c.arg(path); c };
    spawn(command);
}
pub fn reveal(path: &str) {
    let p = Path::new(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer");
        command.raw_arg(format!("/select,\"{}\"", path));
        spawn(command);
    }
    #[cfg(target_os = "linux")]
    {
        let dir = if p.is_dir() { Some(p) } else { p.parent() };
        if let Some(dir) = dir { let mut command = Command::new("xdg-open"); command.arg(dir); spawn(command); }
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open"); command.args(["-R", path]); spawn(command);
    }
}
