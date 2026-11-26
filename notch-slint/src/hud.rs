//! HUD volume dan kecerahan Windows untuk Notch native Slint.
//! Mengikuti perilaku versi Tauri: event hanya ditampilkan saat nilainya berubah,
//! level 0-100, status mute, dan durasi tampil 1,5 detik.
use crate::*;
const HUD_VISIBLE_MS: u64 = 1_500;

fn publish(window: &Weak<NotchWindow>, kind: &'static str, value: u8, muted: bool) {
    let weak = window.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        // HUD tidak boleh menimpa hub, Adzan, greeting, atau pesan prioritas lain.
        if w.get_expanded()
            || w.get_adhan_active()
            || w.get_nudge_visible()
            || w.get_reminder_alert_visible()
            || w.get_greeting_inline_visible()
            || w.get_greeting_speech_visible()
            || w.get_meeting_active()
            || !w.get_meeting_finished().is_empty()
        {
            return;
        }

        let generation = w.get_hud_generation().wrapping_add(1);
        w.set_hud_generation(generation);
        w.set_hud_kind(kind.into());
        w.set_hud_value(value as i32);
        w.set_hud_muted(muted);
        w.set_hud_value_text(if muted { "mute".into() } else { format!("{value}%").into() });
        // Toggle memicu pulse baru setiap kali tombol volume / brightness berubah.
        w.set_hud_icon_pulse(!w.get_hud_icon_pulse());
        w.set_hud_visible(true);

        let weak_hide = w.as_weak();
        Timer::single_shot(std::time::Duration::from_millis(HUD_VISIBLE_MS), move || {
            if let Some(w) = weak_hide.upgrade() {
                if w.get_hud_generation() == generation {
                    w.set_hud_visible(false);
                    w.set_hud_icon_pulse(false);
                }
            }
        });
    });
}

#[cfg(windows)]
mod imp {
    use super::{publish, NotchWindow, Weak};
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_SYSKEYDOWN,
    };

    const NO_WINDOW: u32 = 0x0800_0000;
    const BELOW_NORMAL: u32 = 0x0000_4000;
    const VK_VOLUME_MUTE: u32 = 0xAD;
    const VK_VOLUME_DOWN: u32 = 0xAE;
    const VK_VOLUME_UP: u32 = 0xAF;
    const VOLUME_STEP: f32 = 0.02;

    #[derive(Clone, Copy)]
    enum Key {
        Up,
        Down,
        Mute,
    }

    static KEY_TX: OnceLock<Sender<Key>> = OnceLock::new();

    unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == 0 {
            // SAFETY: WH_KEYBOARD_LL dengan code == HC_ACTION menyediakan KBDLLHOOKSTRUCT valid.
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let key = match kb.vkCode {
                VK_VOLUME_UP => Some(Key::Up),
                VK_VOLUME_DOWN => Some(Key::Down),
                VK_VOLUME_MUTE => Some(Key::Mute),
                _ => None,
            };
            if let Some(key) = key {
                let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
                if down {
                    if let Some(tx) = KEY_TX.get() {
                        let _ = tx.send(key);
                    }
                }
                // Sama seperti Tauri: cegah OSD volume Windows dan tampilkan HUD Notch.
                return LRESULT(1);
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn enumerator() -> Option<IMMDeviceEnumerator> {
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    fn volume(en: &IMMDeviceEnumerator) -> windows::core::Result<(u8, bool)> {
        unsafe {
            let device = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            let level = endpoint.GetMasterVolumeLevelScalar()?;
            let muted = endpoint.GetMute()?.as_bool();
            Ok(((level * 100.0).round().clamp(0.0, 100.0) as u8, muted))
        }
    }

    fn apply_key(en: &IMMDeviceEnumerator, key: Key) -> windows::core::Result<(u8, bool)> {
        unsafe {
            let device = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let endpoint = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            let current = endpoint.GetMasterVolumeLevelScalar()?;
            let mut muted = endpoint.GetMute()?.as_bool();
            let level = match key {
                Key::Up => {
                    muted = false;
                    (current + VOLUME_STEP).min(1.0)
                }
                Key::Down => (current - VOLUME_STEP).max(0.0),
                Key::Mute => {
                    muted = !muted;
                    current
                }
            };
            endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
            endpoint.SetMute(muted, std::ptr::null())?;
            Ok(((level * 100.0).round().clamp(0.0, 100.0) as u8, muted))
        }
    }

    fn publish_volume_if_changed(
        window: &Weak<NotchWindow>,
        last: &Arc<Mutex<Option<(u8, bool)>>>,
        state: (u8, bool),
    ) {
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != Some(state) {
            *last = Some(state);
            drop(last);
            publish(window, "volume", state.0, state.1);
        }
    }

    fn start_key_hook(window: Weak<NotchWindow>, last: Arc<Mutex<Option<(u8, bool)>>>) {
        let (tx, rx) = channel::<Key>();
        if KEY_TX.set(tx).is_err() {
            return;
        }

        // Thread COM khusus untuk mengubah volume ketika tombol media fisik ditekan.
        let win = window.clone();
        let last_state = last.clone();
        thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
            }
            let mut en = enumerator();
            for key in rx {
                if en.is_none() {
                    en = enumerator();
                }
                match en.as_ref().map(|device| apply_key(device, key)) {
                    Some(Ok(state)) => publish_volume_if_changed(&win, &last_state, state),
                    Some(Err(error)) => {
                        eprintln!("[HUD] Gagal mengubah volume: {error}");
                        en = None;
                    }
                    None => {}
                }
            }
        });

        // WH_KEYBOARD_LL memerlukan message loop milik thread tersendiri.
        thread::spawn(|| unsafe {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0) {
                Ok(_hook) => {
                    let mut message = MSG::default();
                    while GetMessageW(&mut message, None, 0, 0).as_bool() {}
                }
                Err(error) => eprintln!("[HUD] Hook tombol volume gagal: {error}"),
            }
        });
    }

    fn start_volume_watcher(window: Weak<NotchWindow>, last: Arc<Mutex<Option<(u8, bool)>>>) {
        thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
            }
            let mut en = enumerator();
            if let Some(device) = en.as_ref() {
                if let Ok(initial) = volume(device) {
                    *last.lock().unwrap_or_else(|e| e.into_inner()) = Some(initial);
                }
            }
            loop {
                thread::sleep(Duration::from_millis(400));
                if en.is_none() {
                    en = enumerator();
                }
                match en.as_ref().map(volume) {
                    Some(Ok(state)) => publish_volume_if_changed(&window, &last, state),
                    Some(Err(_)) => en = None,
                    None => {}
                }
            }
        });
    }

    // Satu proses PowerShell jangka panjang, mengikuti watcher WMI pada versi Tauri.
    // Laptop/monitor yang tidak mendukung WmiMonitorBrightness tidak dipoll setiap detik terus-menerus.
    const BRIGHTNESS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$cs = $null
try { $cs = New-CimSession } catch {}
$last = -1
$fails = 0
$skip = 0
while ($true) {
  if ($skip -gt 0) { $skip-- }
  else {
    if ($cs) {
      $b = (Get-CimInstance -CimSession $cs -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness
    } else {
      $b = (Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness
    }
    if ($null -ne $b) {
      $fails = 0
      if ([int]$b -ne $last) { $last = [int]$b; Write-Output ('B|' + $last) }
    } else {
      $fails++
      if ($fails -ge 3) { $skip = 30 }
    }
  }
  Start-Sleep -Milliseconds 1000
}
"#;

    fn start_brightness_watcher(window: Weak<NotchWindow>) {
        thread::spawn(move || {
            loop {
                let child = Command::new("powershell")
                    .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", BRIGHTNESS_SCRIPT])
                    .creation_flags(NO_WINDOW | BELOW_NORMAL)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();

                match child {
                    Ok(mut child) => {
                        if let Some(stdout) = child.stdout.take() {
                            let mut first = true;
                            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                                if let Some(value) = line.trim().strip_prefix("B|").and_then(|v| v.parse::<u8>().ok()) {
                                    // Nilai pertama menjadi baseline; HUD hanya muncul ketika brightness berubah.
                                    if !first {
                                        publish(&window, "brightness", value.min(100), false);
                                    }
                                    first = false;
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    Err(error) => eprintln!("[HUD] PowerShell brightness watcher gagal: {error}"),
                }
                thread::sleep(Duration::from_secs(10));
            }
        });
    }

    pub fn start(window: Weak<NotchWindow>) {
        let last_volume = Arc::new(Mutex::new(None));
        if std::env::var_os("NOTCH_NATIVE_OSD").is_none() {
            start_key_hook(window.clone(), last_volume.clone());
        }
        start_volume_watcher(window.clone(), last_volume);
        start_brightness_watcher(window);
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::NotchWindow;
    use slint::Weak;

    pub fn start(_window: Weak<NotchWindow>) {}
}

pub fn start(window: Weak<NotchWindow>) {
    imp::start(window);
}
