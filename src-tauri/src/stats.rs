//! Pembungkus Tauri untuk pemantauan sistem. Logika sampling ada di crate `notch-core`
//! (dipakai juga oleh prototipe Slint); file ini hanya mengurus loop, jeda, dan event ke webview.

use notch_core::stats::StatsMonitor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// true hanya selama halaman Stats benar-benar terlihat di hub (diatur frontend lewat stats_set_active).
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn stats_set_active(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

pub fn start(app: AppHandle) {
    // halaman "stats" dihapus dari config -> pemantauan tidak dijalankan sama sekali
    if !crate::config::get().pages.iter().any(|p| p == "stats") {
        return;
    }

    thread::spawn(move || {
        let mut monitor = StatsMonitor::new();

        let mut was_active = false;
        loop {
            // Halaman Stats tertutup: tidak ada refresh sysinfo dan tidak ada event ke webview.
            if !ACTIVE.load(Ordering::Relaxed) {
                was_active = false;
                thread::sleep(Duration::from_millis(300));
                continue;
            }
            if !was_active {
                // baru dibuka: buang selisih selama jeda (kalau tidak, kecepatan jaringan melonjak palsu)
                was_active = true;
                monitor.resume();
            }
            // refresh CPU/jaringan memakai selisih antar-panggilan, jadi instance dipertahankan
            thread::sleep(Duration::from_millis(1000));

            // dikirim lewat referensi: tidak ada salinan Stats / String per detik
            let _ = app.emit("stats", &monitor.sample());
        }
    });
}
