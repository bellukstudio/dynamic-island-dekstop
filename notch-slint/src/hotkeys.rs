//! Pendaftaran global hotkey Windows untuk build Slint.
//! Thread khusus memiliki message queue sendiri; callback UI hanya menerima aksi lewat channel.

use notch_core::config::Shortcuts;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy)]
pub enum Action {
    ToggleHub,
    TogglePomodoro,
    OpenPrayer,
    ColorPicker,
    OpenSettings,
    OpenCompress,
    OpenConvert,
    OpenTransfer,
    OpenAndroid,
    ShortcutRegistrationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Binding {
    modifiers: u32,
    vk: u32,
}

fn parse_combo(value: &str) -> Option<Binding> {
    let mut modifiers = 0u32;
    let mut key = None;
    for part in value.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= 0x0002,
            "alt" => modifiers |= 0x0001,
            "shift" => modifiers |= 0x0004,
            "win" | "meta" => modifiers |= 0x0008,
            name => {
                if key.is_some() { return None; }
                key = virtual_key(name);
            }
        }
    }
    let vk = key?;
    if modifiers == 0 { return None; }
    Some(Binding { modifiers, vk })
}

fn virtual_key(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_uppercase() || byte.is_ascii_digit() { return Some(u32::from(byte)); }
    }
    match upper.as_str() {
        "," => Some(0xBC),
        "." => Some(0xBE),
        "/" => Some(0xBF),
        ";" => Some(0xBA),
        "'" => Some(0xDE),
        "[" => Some(0xDB),
        "]" => Some(0xDD),
        "-" => Some(0xBD),
        "=" => Some(0xBB),
        "SPACE" => Some(0x20),
        "ENTER" => Some(0x0D),
        "ESC" | "ESCAPE" => Some(0x1B),
        "TAB" => Some(0x09),
        "BACKSPACE" => Some(0x08),
        "F1" => Some(0x70),
        "F2" => Some(0x71),
        "F3" => Some(0x72),
        "F4" => Some(0x73),
        "F5" => Some(0x74),
        "F6" => Some(0x75),
        "F7" => Some(0x76),
        "F8" => Some(0x77),
        "F9" => Some(0x78),
        "F10" => Some(0x79),
        "F11" => Some(0x7A),
        "F12" => Some(0x7B),
        _ => None,
    }
}

/// Validasi shortcut yang didaftarkan, termasuk shortcut Color Picker bawaan.
pub fn validate_shortcuts(config: &Shortcuts) -> Result<(), String> {
    let entries = [
        ("Buka / tutup hub", config.toggle_hub.as_str()),
        ("Buka pengaturan", config.open_settings.as_str()),
        ("Pomodoro", config.toggle_pomodoro.as_str()),
        ("Jadwal shalat", config.toggle_prayer.as_str()),
        ("Color Picker", "Ctrl+Alt+Shift+P"),
        ("Compress", "Ctrl+Alt+Shift+K"),
        ("Convert", "Ctrl+Alt+Shift+C"),
        ("File Transfer", "Ctrl+Alt+Shift+S"),
        ("Android", "Ctrl+Alt+Shift+A"),
    ];
    let mut used = std::collections::HashMap::<Binding, &str>::new();
    for (label, combo) in entries {
        let binding = parse_combo(combo).ok_or_else(|| format!("Shortcut {label} tidak valid: {combo}"))?;
        if let Some(previous) = used.insert(binding, label) {
            return Err(format!("Shortcut bentrok: {previous} dan {label} menggunakan {combo}"));
        }
    }
    Ok(())
}
/// Thread hotkey tetap hidup dan menerima konfigurasi baru melalui channel.
pub fn start(config: &Shortcuts) -> (Receiver<Action>, mpsc::Sender<Shortcuts>, Option<JoinHandle<()>>) {
    let (action_tx, action_rx) = mpsc::channel();
    let (config_tx, config_rx) = mpsc::channel::<Shortcuts>();
    #[cfg(windows)]
    {
        let initial = config.clone();
        let handle = thread::spawn(move || unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                PeekMessageW, TranslateMessage, DispatchMessageW, MSG, PM_REMOVE, WM_HOTKEY,
            };
            #[link(name = "user32")]
            extern "system" {
                fn RegisterHotKey(hwnd: windows::Win32::Foundation::HWND, id: i32, modifiers: u32, vk: u32) -> windows::Win32::Foundation::BOOL;
                fn UnregisterHotKey(hwnd: windows::Win32::Foundation::HWND, id: i32) -> windows::Win32::Foundation::BOOL;
            }
            let null_hwnd = windows::Win32::Foundation::HWND::default();
            let mut registered: Vec<(i32, Action)> = Vec::new();
            let mut latest = initial;
            let mut needs_register = true;
            let mut msg = MSG::default();
            loop {
                let mut changed = false;
                while let Ok(next) = config_rx.try_recv() {
                    latest = next;
                    changed = true;
                }
                if changed { needs_register = true; }
                if needs_register {
                    for (id, _) in registered.drain(..) { let _ = UnregisterHotKey(null_hwnd, id); }
                    let entries = [
                        (1i32, latest.toggle_hub.clone(), Action::ToggleHub),
                        (2i32, latest.toggle_pomodoro.clone(), Action::TogglePomodoro),
                        (3i32, latest.toggle_prayer.clone(), Action::OpenPrayer),
                        (4i32, "Ctrl+Alt+Shift+P".to_string(), Action::ColorPicker),
                        (5i32, latest.open_settings.clone(), Action::OpenSettings),
                        (6i32, "Ctrl+Alt+Shift+K".to_string(), Action::OpenCompress),
                        (7i32, "Ctrl+Alt+Shift+C".to_string(), Action::OpenConvert),
                        (8i32, "Ctrl+Alt+Shift+S".to_string(), Action::OpenTransfer),
                        (9i32, "Ctrl+Alt+Shift+A".to_string(), Action::OpenAndroid),
                    ];
                    let mut used = std::collections::HashSet::new();
                    for (id, combo, action) in entries {
                        let Some(binding) = parse_combo(&combo) else { continue };
                        if !used.insert(binding) { continue; }
                        let modifiers = binding.modifiers | 0x4000;
                        if RegisterHotKey(null_hwnd, id, modifiers, binding.vk).as_bool() {
                            registered.push((id, action));
                        } else {
                            // Windows menolak shortcut yang sedang dipakai aplikasi lain atau OS.
                            let _ = action_tx.send(Action::ShortcutRegistrationFailed);
                        }
                    }
                    needs_register = false;
                }
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_HOTKEY {
                        let id = msg.wParam.0 as i32;
                        if let Some((_, action)) = registered.iter().find(|(registered_id, _)| *registered_id == id) {
                            let _ = action_tx.send(*action);
                        }
                    } else {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
            for (id, _) in registered { let _ = UnregisterHotKey(None, id); }
        });
        return (action_rx, config_tx, Some(handle));
    }
    #[cfg(not(windows))]
    {
        let _ = config;
        (action_rx, config_tx, None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_shortcuts() {
        assert_eq!(parse_combo("Ctrl+Alt+N"), Some(Binding { modifiers: 3, vk: 0x4E }));
        assert_eq!(parse_combo("Ctrl+Alt+,"), Some(Binding { modifiers: 3, vk: 0xBC }));
        assert_eq!(parse_combo("Shift+F12"), Some(Binding { modifiers: 4, vk: 0x7B }));
    }

    #[test]
    fn detects_duplicate_shortcuts() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.open_settings = shortcuts.toggle_hub.clone();
        assert!(validate_shortcuts(&shortcuts).unwrap_err().contains("bentrok"));
    }

    #[test]
    fn detects_conflict_with_builtin_color_picker() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.toggle_prayer = "Ctrl+Alt+Shift+P".into();
        assert!(validate_shortcuts(&shortcuts).unwrap_err().contains("Color Picker"));
    }

    #[test]
    fn detects_conflict_with_fixed_feature_shortcut() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.toggle_hub = "Ctrl+Alt+Shift+K".into();
        assert!(validate_shortcuts(&shortcuts).unwrap_err().contains("Compress"));
    }

    #[test]
    fn fixed_transfer_and_android_shortcuts_are_parseable() {
        assert_eq!(parse_combo("Ctrl+Alt+Shift+S"), Some(Binding { modifiers: 7, vk: 0x53 }));
        assert_eq!(parse_combo("Ctrl+Alt+Shift+A"), Some(Binding { modifiers: 7, vk: 0x41 }));
    }

    #[test]
    fn accepts_default_shortcuts() {
        assert!(validate_shortcuts(&Shortcuts::default()).is_ok());
    }
    #[test]
    fn rejects_invalid_or_modifier_only_combinations() {
        assert_eq!(parse_combo("Ctrl+Alt"), None);
        assert_eq!(parse_combo("Ctrl+Alt+Unknown"), None);
        assert_eq!(parse_combo("Ctrl+Alt+N+P"), None);
        assert_eq!(parse_combo("N"), None);
    }
}
