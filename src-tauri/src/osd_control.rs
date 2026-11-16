//! Temporary KDE Plasma OSD control for Notch.
//!
//! Only toggles the Plasma status-change OSD setting and restores its previous
//! value when the application exits. Other Plasma settings are left untouched.

#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "linux")]
static PREVIOUS: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();

#[cfg(target_os = "linux")]
fn run_config_tool(args: &[&str]) -> Option<std::process::Output> {
    for tool in ["kwriteconfig6", "kwriteconfig5"] {
        if let Ok(output) = std::process::Command::new(tool).args(args).output() {
            if output.status.success() {
                return Some(output);
            }
        }
    }
    None
}

/// Disable KDE Plasma's OSD popups for status changes while Notch is running.
#[cfg(target_os = "linux")]
pub fn activate() {
    let state = PREVIOUS.get_or_init(|| Mutex::new(None));
    let mut saved = match state.lock() {
        Ok(value) => value,
        Err(_) => return,
    };
    if saved.is_some() {
        return;
    }

    // kreadconfig6/5 returns the supplied sentinel when the key was not set.
    let mut previous = None;
    for tool in ["kreadconfig6", "kreadconfig5"] {
        if let Ok(output) = std::process::Command::new(tool)
            .args([
                "--file", "plasmashellrc",
                "--group", "OSD",
                "--key", "Enabled",
                "--default", "__NOTCH_UNSET__",
            ])
            .output()
        {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                previous = if value == "__NOTCH_UNSET__" { None } else { Some(value) };
                break;
            }
        }
    }

    // Save the state only if the write succeeded; otherwise leave the system alone.
    if run_config_tool(&[
        "--file", "plasmashellrc",
        "--group", "OSD",
        "--key", "Enabled",
        "false",
    ]).is_some() {
        *saved = Some(previous);
    }
}

/// Restore the exact previous value (or remove the key if it was previously absent).
#[cfg(target_os = "linux")]
pub fn restore() {
    let Some(state) = PREVIOUS.get() else { return };
    let mut saved = match state.lock() {
        Ok(value) => value,
        Err(_) => return,
    };
    let Some(previous) = saved.take() else { return };

    match previous {
        Some(value) => {
            let _ = run_config_tool(&[
                "--file", "plasmashellrc",
                "--group", "OSD",
                "--key", "Enabled",
                &value,
            ]);
        }
        None => {
            let _ = run_config_tool(&[
                "--delete",
                "--file", "plasmashellrc",
                "--group", "OSD",
                "--key", "Enabled",
            ]);
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn activate() {}

#[cfg(not(target_os = "linux"))]
pub fn restore() {}
