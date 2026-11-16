//! Riwayat clipboard teks untuk UI desktop (Tauri maupun Slint).
//! Riwayat hanya disimpan di memori, maksimum 10 item, dan tidak menyimpan konten ke disk.

use arboard::Clipboard;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const MAX_ITEMS: usize = 10;
const MAX_CHARS: usize = 10_000;
const POLL_INTERVAL: Duration = Duration::from_millis(600);

#[derive(Clone)]
pub struct ClipboardMonitor {
    history: Arc<Mutex<Vec<String>>>,
    tx: mpsc::Sender<String>,
}

fn push(history: &Arc<Mutex<Vec<String>>>, text: String) {
    let mut items = history.lock().unwrap_or_else(|e| e.into_inner());
    items.retain(|item| *item != text);
    items.insert(0, text);
    items.truncate(MAX_ITEMS);
}

fn is_trackable(text: &str) -> bool {
    !text.trim().is_empty() && text.chars().count() <= MAX_CHARS
}

impl ClipboardMonitor {
    /// Memulai thread pemantau. Gagal bila clipboard OS tidak dapat diakses.
    pub fn start() -> Result<Self, String> {
        let history = Arc::new(Mutex::new(Vec::new()));
        let worker_history = Arc::clone(&history);
        let (tx, rx) = mpsc::channel::<String>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name("notch-clipboard".into())
            .spawn(move || {
                let mut clipboard = match Clipboard::new() {
                    Ok(clipboard) => clipboard,
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("Clipboard tidak tersedia: {error}")));
                        return;
                    }
                };

                // Isi clipboard saat aplikasi mulai ikut dicatat, seperti versi Tauri.
                let mut last = clipboard.get_text().unwrap_or_default();
                if is_trackable(&last) {
                    push(&worker_history, last.clone());
                }
                let _ = ready_tx.send(Ok(()));

                #[cfg(target_os = "windows")]
                let mut last_seq: u32 = u32::MAX;

                loop {
                    match rx.recv_timeout(POLL_INTERVAL) {
                        Ok(text) => {
                            if clipboard.set_text(text.clone()).is_ok() {
                                last = text.clone();
                                push(&worker_history, text);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }

                    // Di Windows cek nomor urut dahulu agar clipboard tidak dibuka terus-menerus.
                    #[cfg(target_os = "windows")]
                    {
                        let seq = unsafe {
                            windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                        };
                        if seq == last_seq {
                            continue;
                        }
                        last_seq = seq;
                    }

                    // Konten non-teks menghasilkan Err dan dilewati.
                    if let Ok(text) = clipboard.get_text() {
                        if text != last && is_trackable(&text) {
                            last = text.clone();
                            push(&worker_history, text);
                        }
                    }
                }
            })
            .map_err(|error| format!("Gagal memulai pemantau clipboard: {error}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self { history, tx }),
            Ok(Err(error)) => Err(error),
            Err(error) => Err(format!("Pemantau clipboard tidak siap: {error}")),
        }
    }

    /// Salinan terbaru berada di indeks pertama.
    pub fn list(&self) -> Vec<String> {
        self.history.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Salin ulang teks ke clipboard sistem.
    pub fn copy(&self, text: String) {
        let _ = self.tx.send(text);
    }

    /// Hapus riwayat aplikasi saja; isi clipboard sistem tidak dihapus.
    pub fn clear(&self) {
        self.history.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
