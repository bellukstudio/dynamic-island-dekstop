//! Lightweight Windows privacy activity monitor for automatic meeting mode.
use std::process::Command;
use std::thread;
use std::time::Duration;
use slint::Weak;

#[derive(Clone, Default, PartialEq)]
struct Activity { mic: bool, cam: bool }

#[cfg(windows)]
fn query_kind(kind: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let key = format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{kind}");
    let Ok(out) = Command::new("reg").args(["query", &key, "/s"]).creation_flags(CREATE_NO_WINDOW).output() else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut current = String::new();
    let (mut start, mut stop) = (0u64, 0u64);
    let mut candidates = Vec::new();
    let flush = |key: &str, start: u64, stop: u64, out: &mut Vec<String>| {
        if start > stop && start != 0 {
            let leaf = key.rsplit('\\').next().unwrap_or(key);
            if leaf.contains('#') {
                let exe = leaf.rsplit('#').next().unwrap_or(leaf);
                out.push(exe.to_string());
            } else {
                out.push(leaf.split('_').next().unwrap_or(leaf).to_string());
            }
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("HKEY") {
            if !current.is_empty() { flush(&current, start, stop, &mut candidates); }
            current = t.to_string(); start = 0; stop = 0;
        } else if t.starts_with("LastUsedTimeStart") {
            start = t.split_whitespace().last().and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"),16).ok()).unwrap_or(0);
        } else if t.starts_with("LastUsedTimeStop") {
            stop = t.split_whitespace().last().and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"),16).ok()).unwrap_or(0);
        }
    }
    if !current.is_empty() { flush(&current, start, stop, &mut candidates); }
    candidates.into_iter().any(|exe| {
        if !exe.to_lowercase().ends_with(".exe") { return true; }
        Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {exe}"), "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&exe.to_lowercase()))
            .unwrap_or(false)
    })
}

#[cfg(not(windows))]
fn query_kind(_: &str) -> bool { false }

pub fn start(weak: Weak<crate::NotchWindow>) {
    thread::spawn(move || {
        let (mut mic_on, mut cam_on) = (0u8, 0u8);
        let (mut mic_off, mut cam_off) = (0u8, 0u8);
        let mut shown = false;
        loop {
            let mic = query_kind("microphone");
            let cam = query_kind("webcam");
            mic_on = if mic { mic_on.saturating_add(1) } else { 0 };
            cam_on = if cam { cam_on.saturating_add(1) } else { 0 };
            mic_off = if mic { 0 } else { mic_off.saturating_add(1) };
            cam_off = if cam { 0 } else { cam_off.saturating_add(1) };
            let active = if shown { mic_off < 2 || cam_off < 2 } else { mic_on >= 2 || cam_on >= 2 };
            if active != shown {
                shown = active;
                let source = match (mic, cam) {
                    (true, true) => "MIC + CAM",
                    (true, false) => "MIC",
                    (false, true) => "CAM",
                    _ => "",
                }.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    w.set_meeting_source(source.into());
                    w.set_meeting_active(active);
                });
            } else if shown {
                let source = match (mic, cam) {
                    (true, true) => "MIC + CAM",
                    (true, false) => "MIC",
                    (false, true) => "CAM",
                    _ => "RAPAT",
                }.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| w.set_meeting_source(source.into()));
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}
