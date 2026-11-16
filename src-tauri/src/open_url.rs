//! Membuka URL http(s) di browser bawaan sistem (dipakai tombol "Lihat update").
//! Tanpa shell: URL diberikan sebagai argumen terpisah, jadi karakter seperti & tidak ditafsirkan.
//!
//! Pasang di lib.rs:
//!   mod open_url;
//!   ... .invoke_handler(tauri::generate_handler![ ..., open_url::open_url ])

use std::process::Command;

#[cfg(target_os = "windows")]
fn launch(url: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn launch(url: &str) -> std::io::Result<()> {
    Command::new("open").arg(url).spawn().map(|_| ())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn launch(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open").arg(url).spawn().map(|_| ())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let scheme_ok = lower.starts_with("https://") || lower.starts_with("http://");
    let chars_ok = !url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '"');
    if url.len() > 2048 || !scheme_ok || !chars_ok {
        return Err("URL tidak valid (hanya http/https)".to_string());
    }
    launch(url).map_err(|e| format!("Gagal membuka browser: {e}"))
}
