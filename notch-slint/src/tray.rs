//! Ikon tray (area notifikasi Windows) dengan menu Pengaturan, Pusatkan Notch, dan Keluar, sama dengan versi Tauri.
//! Notch tidak punya tombol taskbar, jadi tray adalah jalan utama untuk membuka Pengaturan atau keluar.
//!
//! Ikon dan menu dibuat di thread UI; pesan menu dari Windows diteruskan ke event loop Slint, sehingga
//! `on_action` selalu berjalan di thread UI dan boleh menyentuh komponen Slint.

/// Pilihan di menu tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Buka hub di halaman Pengaturan.
    Settings,
    /// Kembalikan notch ke tengah atas layar.
    Center,
    /// Tutup aplikasi.
    Quit,
}

/// Hanya Windows yang punya ikon tray; di platform lain `install` selalu gagal dan tidak perlu dicoba ulang.
pub const SUPPORTED: bool = cfg!(windows);

/// Menjaga ikon tetap ada di tray. Ikon dihapus dari tray saat objek ini di-drop.
#[allow(dead_code)]
pub struct Tray(imp::Handle);

/// Pasang ikon tray. Gagal bila shell belum siap (mis. aplikasi jalan otomatis saat login sebelum Explorer
/// selesai dimuat), jadi pemanggil sebaiknya mencoba lagi nanti.
pub fn install(on_action: impl Fn(Action) + 'static) -> Result<Tray, String> {
    imp::install(Box::new(on_action)).map(Tray)
}

#[cfg(windows)]
mod imp {
    use super::Action;
    use std::cell::RefCell;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    /// Ikon yang sama dengan ikon aplikasi Tauri (src-tauri/icons/32x32.png).
    const ICON_PNG: &[u8] = include_bytes!("../assets/tray.png");

    const ID_SETTINGS: &str = "settings";
    const ID_CENTER: &str = "center";
    const ID_QUIT: &str = "quit";

    thread_local! {
        // Hanya dibaca di thread UI (lewat invoke_from_event_loop), jadi tidak perlu Send/Sync.
        static DISPATCH: RefCell<Option<Box<dyn Fn(Action)>>> = const { RefCell::new(None) };
    }

    #[allow(dead_code)]
    pub struct Handle(TrayIcon);

    fn action_for(id: &str) -> Option<Action> {
        match id {
            ID_SETTINGS => Some(Action::Settings),
            ID_CENTER => Some(Action::Center),
            ID_QUIT => Some(Action::Quit),
            _ => None,
        }
    }

    fn icon() -> Result<Icon, String> {
        let mut decoder = png::Decoder::new(ICON_PNG);
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err("ikon tray harus PNG RGBA 8-bit".into());
        }
        buf.truncate(info.buffer_size());
        Icon::from_rgba(buf, info.width, info.height).map_err(|e| e.to_string())
    }

    pub fn install(on_action: Box<dyn Fn(Action)>) -> Result<Handle, String> {
        let menu = Menu::new();
        menu.append_items(&[
            &MenuItem::with_id(ID_SETTINGS, "Pengaturan", true, None),
            &MenuItem::with_id(ID_CENTER, "Pusatkan Notch", true, None),
            &MenuItem::with_id(ID_QUIT, "Keluar", true, None),
        ])
        .map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Notch")
            .with_icon(icon()?)
            .build()
            .map_err(|e| e.to_string())?;
        DISPATCH.with(|d| *d.borrow_mut() = Some(on_action));
        // Handler dipanggil dari window procedure tray; pindahkan ke event loop Slint agar aman dan tidak
        // membuka jendela/komponen di tengah pemrosesan pesan Windows.
        MenuEvent::set_event_handler(Some(|event: MenuEvent| {
            let Some(action) = action_for(event.id().as_ref()) else { return };
            let _ = slint::invoke_from_event_loop(move || {
                DISPATCH.with(|d| {
                    if let Some(handler) = d.borrow().as_ref() {
                        handler(action);
                    }
                });
            });
        }));
        Ok(Handle(tray))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn menu_ids_map_to_actions() {
            assert_eq!(action_for("settings"), Some(Action::Settings));
            assert_eq!(action_for("center"), Some(Action::Center));
            assert_eq!(action_for("quit"), Some(Action::Quit));
            assert_eq!(action_for("lainnya"), None);
        }

        #[test]
        fn tray_icon_is_valid_rgba_png() {
            assert!(icon().is_ok());
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Action;

    pub struct Handle;

    pub fn install(_on_action: Box<dyn Fn(Action)>) -> Result<Handle, String> {
        Err("ikon tray hanya tersedia di Windows".into())
    }
}
