//! Pembungkus Tauri untuk status baterai. Pembacaan per-OS ada di crate `notch-core`;
//! file ini hanya mengurus polling 5 detik dan event ke webview.

use notch_core::battery::{BatteryMonitor, Reading};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let monitor = BatteryMonitor::new();
        loop {
            match monitor.read() {
                Reading::Present(b) => {
                    let _ = app.emit("battery", b);
                }
                Reading::Absent => {
                    println!("baterai tidak ditemukan (PC desktop?), modul baterai dilewati");
                    return;
                }
                Reading::Unavailable => {}
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}
