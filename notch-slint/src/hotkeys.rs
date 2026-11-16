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

/// Jalankan message loop Windows pada thread terpisah. Di platform lain, receiver tetap valid
/// tetapi tidak menghasilkan aksi; build non-Windows tetap dapat dipakai untuk pemeriksaan UI.
pub fn start(config: &Shortcuts) -> (Receiver<Action>, Option<JoinHandle<()>>) {
    let (tx, rx) = mpsc::channel();
    #[cfg(windows)]
    {
        let entries = [
            (1i32, config.toggle_hub.clone(), Action::ToggleHub),
            (2i32, config.toggle_pomodoro.clone(), Action::TogglePomodoro),
            (3i32, config.toggle_prayer.clone(), Action::OpenPrayer),
            (4i32, "Ctrl+Alt+Shift+P".to_string(), Action::ColorPicker),
        ];
        let handle = thread::spawn(move || unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetMessageW, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MSG, WM_HOTKEY,
            };
            let mut registered = Vec::new();
            let mut used = std::collections::HashSet::new();
            for (id, combo, action) in entries {
                let Some(binding) = parse_combo(&combo) else { continue };
                if !used.insert(binding) { continue; }
                // MOD_NOREPEAT mencegah auto-repeat ketika tombol ditahan.
                let modifiers = HOT_KEY_MODIFIERS(binding.modifiers | 0x4000);
                if RegisterHotKey(None, id, modifiers, binding.vk).is_ok() {
                    registered.push((id, action));
                }
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_HOTKEY {
                    let id = msg.wParam.0 as i32;
                    if let Some((_, action)) = registered.iter().find(|(registered_id, _)| *registered_id == id) {
                        let _ = tx.send(*action);
                    }
                }
            }
            for (id, _) in registered { let _ = UnregisterHotKey(None, id); }
        });
        return (rx, Some(handle));
    }
    #[cfg(not(windows))]
    {
        let _ = config;
        let _ = tx;
        (rx, None)
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
    fn rejects_invalid_or_modifier_only_combinations() {
        assert_eq!(parse_combo("Ctrl+Alt"), None);
        assert_eq!(parse_combo("Ctrl+Alt+Unknown"), None);
        assert_eq!(parse_combo("Ctrl+Alt+N+P"), None);
        assert_eq!(parse_combo("N"), None);
    }
}
