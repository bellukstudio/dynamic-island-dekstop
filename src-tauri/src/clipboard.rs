use arboard::Clipboard;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MAX_ITEMS: usize = 10;
const MAX_CHARS: usize = 10_000; // teks lebih panjang dari ini diabaikan

// Riwayat hanya ada di memori: tidak disimpan ke disk dan hilang saat aplikasi ditutup.
static HISTORY: Mutex<Vec<String>> = Mutex::new(Vec::new());
// kanal ke thread pemantau (instance Clipboard harus hidup di satu thread)
static TX: OnceLock<Mutex<mpsc::Sender<String>>> = OnceLock::new();

fn push(text: String) -> Vec<String> {
    let mut h = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    h.retain(|t| *t != text);
    h.insert(0, text);
    h.truncate(MAX_ITEMS);
    h.clone()
}

pub fn start(app: AppHandle) {
    // halaman "clipboard" dihapus dari config -> pemantauan tidak dijalankan sama sekali
    if !crate::config::get().pages.iter().any(|p| p == "clipboard") {
        return;
    }

    let (tx, rx) = mpsc::channel::<String>();
    let _ = TX.set(Mutex::new(tx));

    std::thread::spawn(move || {
        let mut cb = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("clipboard tidak tersedia: {e}");
                return;
            }
        };

        // isi clipboard saat aplikasi mulai ikut dicatat
        let mut last = cb.get_text().unwrap_or_default();
        if !last.trim().is_empty() && last.chars().count() <= MAX_CHARS {
            push(last.clone());
        }

        // Windows: nomor urut clipboard murah dibaca dan tidak membuka clipboard. get_text() tiap 600 ms
        // membuka clipboard terus-menerus (boros dan bisa mengganggu aplikasi lain).
        #[cfg(target_os = "windows")]
        let mut last_seq: u32 = u32::MAX;

        loop {
            // permintaan "salin ulang" dari frontend; sekaligus berfungsi sebagai jeda polling
            match rx.recv_timeout(Duration::from_millis(600)) {
                Ok(text) => {
                    if cb.set_text(text.clone()).is_ok() {
                        last = text.clone();
                        let h = push(text);
                        let _ = app.emit("clipboard", h);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            #[cfg(target_os = "windows")]
            {
                let seq = unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
                if seq == last_seq {
                    continue;
                }
                last_seq = seq;
            }

            // konten non-teks (gambar, file) menghasilkan Err dan dilewati
            if let Ok(t) = cb.get_text() {
                if t != last && !t.trim().is_empty() && t.chars().count() <= MAX_CHARS {
                    last = t.clone();
                    let h = push(t);
                    let _ = app.emit("clipboard", h);
                }
            }
        }
    });
}

#[tauri::command]
pub fn clip_list() -> Vec<String> {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
pub fn clip_copy(text: String) {
    if let Some(tx) = TX.get() {
        if let Ok(g) = tx.lock() {
            let _ = g.send(text);
        }
    }
}

#[tauri::command]
pub fn clip_clear() -> Vec<String> {
    let mut h = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    h.clear();
    h.clone()
}

/// Salin warna langsung ke clipboard, tanpa bergantung pada fitur riwayat clipboard.
#[tauri::command]
pub fn color_copy(text: String) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard tidak tersedia: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("Gagal menyalin warna: {e}"))
}
