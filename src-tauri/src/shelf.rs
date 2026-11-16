use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

// Shelf: tempat singgah file. File diseret ke notch, disimpan referensinya (bukan disalin),
// lalu dibuka / ditampilkan di folder nanti. Daftar disimpan di app data dir (shelf.json).
#[derive(Serialize, Deserialize, Clone)]
pub struct Item {
    path: String,
    name: String,
    dir: bool,
}

static ITEMS: Mutex<Vec<Item>> = Mutex::new(Vec::new());
const MAX: usize = 30;

fn data_file(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("shelf.json"))
}

fn save(app: &AppHandle, items: &[Item]) {
    if let Some(f) = data_file(app) {
        if let Ok(s) = serde_json::to_string(items) {
            let _ = std::fs::write(f, s);
        }
    }
}

// daftar saat ini; file yang sudah dihapus/dipindah dibuang
fn alive() -> Vec<Item> {
    let mut g = ITEMS.lock().unwrap();
    g.retain(|i| Path::new(&i.path).exists());
    g.clone()
}

pub fn start(app: AppHandle) {
    if let Some(f) = data_file(&app) {
        if let Ok(s) = std::fs::read_to_string(f) {
            if let Ok(v) = serde_json::from_str::<Vec<Item>>(&s) {
                *ITEMS.lock().unwrap() = v;
            }
        }
    }
}

// diagnostik: frontend menulis jejak drag-drop ke terminal (tauri dev) lewat sini
#[tauri::command]
pub fn shelf_log(msg: String) {
    eprintln!("[shelf] {msg}");
}

#[tauri::command]
pub fn shelf_list() -> Vec<Item> {
    alive()
}

#[tauri::command]
pub fn shelf_add(app: AppHandle, paths: Vec<String>) -> Vec<Item> {
    {
        let mut g = ITEMS.lock().unwrap();
        for p in paths {
            let pb = PathBuf::from(&p);
            if !pb.exists() {
                continue;
            }
            let name = pb
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone());
            g.retain(|i| i.path != p); // tidak dobel, naikkan ke paling depan
            g.insert(0, Item { path: p, name, dir: pb.is_dir() });
        }
        g.truncate(MAX);
    }
    let list = alive();
    save(&app, &list);
    list
}

#[tauri::command]
pub fn shelf_remove(app: AppHandle, path: String) -> Vec<Item> {
    ITEMS.lock().unwrap().retain(|i| i.path != path);
    let list = alive();
    save(&app, &list);
    list
}

#[tauri::command]
pub fn shelf_clear(app: AppHandle) -> Vec<Item> {
    ITEMS.lock().unwrap().clear();
    save(&app, &[]);
    Vec::new()
}

fn spawn(mut cmd: std::process::Command) {
    if let Ok(mut child) = cmd.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait(); // cegah proses zombie
        });
    }
}

#[tauri::command]
pub fn shelf_open(path: String) {
    #[cfg(target_os = "linux")]
    {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&path);
        spawn(c);
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = std::process::Command::new("explorer");
        c.arg(&path);
        spawn(c);
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        c.arg(&path);
        spawn(c);
    }
}

// tampilkan file di pengelola file
#[tauri::command]
pub fn shelf_reveal(path: String) {
    #[cfg(target_os = "linux")]
    {
        let p = Path::new(&path);
        let dir = if p.is_dir() { Some(p) } else { p.parent() };
        if let Some(d) = dir {
            let mut c = std::process::Command::new("xdg-open");
            c.arg(d);
            spawn(c);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("explorer");
        c.raw_arg(format!("/select,\"{}\"", path));
        spawn(c);
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        c.args(["-R", &path]);
        spawn(c);
    }
}