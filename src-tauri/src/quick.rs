//! Pembungkus Tauri untuk tombol cepat. Logikanya ada di crate `notch-core` (dipakai juga oleh Slint).

use notch_core::quick::{self, Quick};

#[tauri::command]
pub async fn quick_state() -> Quick {
    println!("quick_state: dipanggil");
    let q = quick::state();
    println!(
        "quick_state: mic={:?} wifi={:?} bt={:?} awake={:?} dark={:?}",
        q.mic, q.wifi, q.bt, q.awake, q.dark
    );
    q
}

#[tauri::command]
pub async fn quick_toggle(kind: String) -> Quick {
    quick::toggle(&kind)
}
