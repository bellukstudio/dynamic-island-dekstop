//! Pengambil teks lewat HTTPS yang dipakai bersama (hari libur, cuaca, kurs, lirik).
//! Hanya ada dengan fitur `net`, supaya Tauri (yang sudah punya reqwest) tidak ikut menarik ureq.

use std::time::Duration;

/// GET `url` dan kembalikan isinya sebagai teks. Memblokir sampai `timeout_s` detik:
/// panggil dari thread latar, bukan dari thread UI.
pub fn get_text(url: &str, timeout_s: u64) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_s)))
        .user_agent("Notch/0.1")
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}
