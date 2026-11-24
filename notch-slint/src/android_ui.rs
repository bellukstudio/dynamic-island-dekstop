//! Halaman Android: ADB, kirim/ambil file, dan mirroring layar.

use crate::*;

pub(crate) fn setup(
    win: &NotchWindow,
    android_state: &Arc<Mutex<android::AndroidState>>,
    mirror_process: &Arc<Mutex<android_mirror::MirrorProcess>>,
) {
{
    let state = android_state.clone();
    let weak = win.as_weak();
    win.on_android_refresh(move || {
        let mut android = state.lock().unwrap_or_else(|e| e.into_inner());
        let status = android.status();
        let devices = android.devices();
        if let Some(w) = weak.upgrade() {
            w.set_android_status(status.into());
            w.set_android_devices(devices.into());
            w.set_android_serial(android.serial().into());
        }
    });
}
{
    let state = android_state.clone();
    let weak = win.as_weak();
    win.on_android_select_device(move |serial| {
        state.lock().unwrap_or_else(|e| e.into_inner()).set_serial(serial.to_string());
        if let Some(w) = weak.upgrade() { w.set_android_status("Perangkat dipilih".into()); }
    });
}
{
    let state = android_state.clone();
    let weak = win.as_weak();
    win.on_android_list(move |path| {
        let state = state.clone();
        let path = path.to_string();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let entries = state.lock().unwrap_or_else(|e| e.into_inner()).list(&path);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { w.set_android_entries(entries.into()); w.set_android_path(path.into()); }
            });
        });
    });
}
{
    let state = android_state.clone();
    let weak = win.as_weak();
    win.on_android_push(move |_serial, remote| {
        let selected = rfd::FileDialog::new().pick_files().unwrap_or_default();
        if selected.is_empty() { return; }
        let state = state.clone();
        let remote = remote.to_string();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let mut ok = 0usize;
            let mut errors = Vec::new();
            for path in selected {
                match state.lock().unwrap_or_else(|e| e.into_inner()).push(&path, &remote) {
                    Ok(()) => ok += 1,
                    Err(e) => errors.push(e),
                }
            }
            let msg = if errors.is_empty() { format!("{ok} file berhasil dikirim") } else { format!("{ok} berhasil, gagal: {}", errors.join("; ")) };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { w.set_android_status(msg.into()); }
            });
        });
    });
}
{
    let state = android_state.clone();
    let weak = win.as_weak();
    win.on_android_pull(move |remote| {
        let destination = rfd::FileDialog::new().set_title("Pilih folder tujuan").pick_folder();
        let Some(destination) = destination else { return };
        let state = state.clone();
        let remote = remote.to_string();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = state.lock().unwrap_or_else(|e| e.into_inner()).pull(&remote, &destination);
            let message = match result {
                Ok(()) => format!("File berhasil diambil ke {}", destination.display()),
                Err(error) => format!("Gagal mengambil file: {error}"),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { w.set_android_status(message.into()); }
            });
        });
    });
}
{
    let android = android_state.clone();
    let mirror = mirror_process.clone();
    let weak = win.as_weak();
    win.on_android_mirror(move || {
        let serial = android.lock().unwrap_or_else(|e| e.into_inner()).serial().to_string();
        let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).start(&serial);
        if let Some(w) = weak.upgrade() {
            w.set_android_status(match result {
                Ok(message) => message.into(),
                Err(error) => format!("Mirroring gagal: {error}").into(),
            });
        }
    });
}
{
    let mirror = mirror_process.clone();
    let weak = win.as_weak();
    win.on_android_stop_mirror(move || {
        let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).stop();
        if let Some(w) = weak.upgrade() {
            w.set_android_status(match result {
                Ok(message) => message.into(),
                Err(error) => format!("Gagal menghentikan mirroring: {error}").into(),
            });
        }
    });
}
}
