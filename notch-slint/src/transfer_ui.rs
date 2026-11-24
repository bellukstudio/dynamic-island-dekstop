//! Halaman Transfer: server berbagi file lewat jaringan lokal.

use crate::*;

pub(crate) fn setup(win: &NotchWindow, transfer_state: &Arc<Mutex<transfer::TransferState>>) {
{
    let state = transfer_state.clone();
    let weak = win.as_weak();
    win.on_transfer_start(move || {
        let result = state.lock().unwrap_or_else(|e| e.into_inner()).start();
        if let Some(w) = weak.upgrade() {
            match result {
                Ok(url) => { w.set_transfer_address(url.into()); w.set_transfer_status("Server aktif — bagikan alamat ini ke perangkat lain".into()); }
                Err(error) => w.set_transfer_status(format!("Gagal memulai server: {error}").into()),
            }
        }
    });
}
{
    let state = transfer_state.clone();
    let weak = win.as_weak();
    win.on_transfer_stop(move || {
        state.lock().unwrap_or_else(|e| e.into_inner()).stop();
        if let Some(w) = weak.upgrade() { w.set_transfer_address("".into()); w.set_transfer_status("Server dihentikan".into()); }
    });
}
{
    let state = transfer_state.clone();
    let weak = win.as_weak();
    win.on_transfer_add_files(move || {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            let strings = paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let count = state.lock().unwrap_or_else(|e| e.into_inner()).add_files(strings);
            if let Some(w) = weak.upgrade() { w.set_transfer_shared_count_text(count.to_string().into()); w.set_transfer_status("Daftar file dibagikan diperbarui".into()); }
        }
    });
}
{
    let inbox = notch_core::paths::config_path().parent().unwrap_or(std::path::Path::new(".")).join("Notch");
    let weak = win.as_weak();
    win.on_transfer_open_inbox(move || {
        let result = transfer::open_inbox(&inbox);
        if let Err(error) = result {
            if let Some(w) = weak.upgrade() { w.set_transfer_status(format!("Gagal membuka folder: {error}").into()); }
        }
    });
}
}
