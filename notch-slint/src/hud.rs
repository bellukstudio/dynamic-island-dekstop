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

        // Pembacaan WMI dan hasil shortcut dapat tiba hampir bersamaan.
        // Jangan mem-pulse ulang ikon untuk nilai yang sama; cukup pertahankan animasi yang sedang berjalan.
        if w.get_hud_visible()
            && w.get_hud_kind() == kind
            && w.get_hud_value() == value as i32
            && w.get_hud_muted() == muted
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
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, FindWindowExW, GetMessageW, GetWindowRect, IsWindow,
        IsWindowVisible, SetWindowsHookExW, ShowWindow, KBDLLHOOKSTRUCT, MSG, SW_HIDE, SW_SHOWNA,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
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

    // Windows brightness hotkeys are often delivered to Monitor.sys through ACPI and
    // never reach WH_KEYBOARD_LL. Hide the shared shell OSD host instead, so the hardware
    // brightness hotkey keeps working while Notch displays its own HUD from the WMI watcher.
    fn find_osd_by_hierarchy(
        outer_class: PCWSTR,
        inner_class: PCWSTR,
        inner_inner_class: Option<PCWSTR>,
    ) -> Option<HWND> {
        let mut after: Option<HWND> = None;
        let mut result: Option<HWND> = None;
        let mut pair_count = 0usize;

        // Jangan langsung mengambil host pertama. Windows dapat memiliki beberapa
        // XamlExplorerHostIslandWindow, termasuk island kosong 0x0 yang bukan OSD.
        // Pilih kandidat terakhir yang punya ukuran nyata, sama seperti pencari OSD Tauri/Windhawk.
        loop {
            let outer = match unsafe {
                FindWindowExW(None, after, outer_class, PCWSTR::null())
            } {
                Ok(hwnd) => hwnd,
                Err(_) => break,
            };

            let inner = match unsafe {
                FindWindowExW(Some(outer), None, inner_class, PCWSTR::null())
            } {
                Ok(hwnd) => hwnd,
                Err(_) => {
                    after = Some(outer);
                    continue;
                }
            };

            let nested_matches = match inner_inner_class {
                Some(class) => unsafe {
                    FindWindowExW(Some(inner), None, class, PCWSTR::null()).is_ok()
                },
                None => true,
            };
            if nested_matches {
                let mut rect = RECT::default();
                let has_bounds = unsafe {
                    GetWindowRect(outer, &mut rect).is_ok()
                };
                let (width, height) = if has_bounds {
                    (rect.right - rect.left, rect.bottom - rect.top)
                } else {
                    (0, 0)
                };

                // Simpan kandidat pertama sebagai fallback jika semuanya masih 0x0,
                // tetapi prioritaskan host yang telah dibuat dengan ukuran non-zero.
                if pair_count == 0 || (width > 0 && height > 0) {
                    result = Some(outer);
                }
                pair_count += 1;
            }

            after = Some(outer);
        }

        result
    }

    fn find_osd_window() -> Option<HWND> {
        // Windows 11 (termasuk 25H2): pilih host XAML yang memiliki bounds nyata.
        find_osd_by_hierarchy(
            w!("XamlExplorerHostIslandWindow"),
            w!("Windows.UI.Composition.DesktopWindowContentBridge"),
            Some(w!("DesktopWindowXamlSource")),
        )
        .or_else(|| {
            find_osd_by_hierarchy(
                w!("XamlExplorerHostIslandWindow"),
                w!("Windows.UI.Composition.DesktopWindowContentBridge"),
                None,
            )
        })
        // Windows 10: host OSD klasik DirectUI.
        .or_else(|| {
            find_osd_by_hierarchy(
                w!("NativeHWNDHost"),
                w!("DirectUIHWND"),
                None,
            )
        })
    }

    fn start_osd_suppressor(window: Weak<NotchWindow>) {
        thread::spawn(move || {
            let mut osd: Option<HWND> = None;
            let mut hidden_by_us = false;
            loop {
                // When Notch exits, restore the shell host only if this worker minimized it.
                // This prevents the system OSD from remaining hidden after the app is closed.
                if window.upgrade().is_none() {
                    if hidden_by_us {
                        if let Some(hwnd) = osd {
                            if unsafe { IsWindow(Some(hwnd)).as_bool() } {
                                unsafe { ShowWindow(hwnd, SW_SHOWNA) };
                            }
                        }
                    }
                    break;
                }

                let mut should_rediscover = osd.is_none();
                if let Some(hwnd) = osd {
                    if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                        osd = None;
                        hidden_by_us = false;
                        should_rediscover = true;
                    } else if unsafe { IsWindowVisible(hwnd).as_bool() }
                    {
                        // Keep the shell OSD host hidden: hardware/ACPI brightness
                        // changes still work, but the system popup never takes the screen.
                        unsafe { ShowWindow(hwnd, SW_HIDE) };
                        hidden_by_us = true;
                    }
                }

                if should_rediscover {
                    osd = find_osd_window();
                    if let Some(hwnd) = osd {
                        if unsafe { IsWindowVisible(hwnd).as_bool() }
                            {
                            unsafe { ShowWindow(hwnd, SW_HIDE) };
                            hidden_by_us = true;
                        }
                    }
                }

                thread::sleep(Duration::from_millis(80));
            }
        });
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
$i = 0
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
  $i++
  if ($i -ge 1800) { exit }
  Start-Sleep -Milliseconds 1000
}
"#;

    /// Mengubah brightness panel internal tanpa memicu hotkey/OSD bawaan Windows.
    /// WMI WmiSetBrightness mengatur level dalam persen; shortcut ini milik Notch.
    pub fn adjust_brightness(window: Weak<NotchWindow>, delta: i32) {
        thread::spawn(move || {
            let script = format!(r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$brightness = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness |
    Where-Object {{ $_.Active }} | Select-Object -First 1
$method = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods |
    Where-Object {{ $_.Active }} | Select-Object -First 1
if ($null -eq $brightness -or $null -eq $method) {{ exit 2 }}
$target = [Math]::Max(0, [Math]::Min(100, [int]$brightness.CurrentBrightness + ({delta})))
$result = Invoke-CimMethod -InputObject $method -MethodName WmiSetBrightness -Arguments @{{ Timeout = 0; Brightness = [byte]$target }}
if ($null -eq $result -or $result.ReturnValue -ne 0) {{ exit 3 }}
Write-Output $target
"#);
            let output = Command::new("powershell")
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
                .arg(&script)
                .creation_flags(NO_WINDOW | BELOW_NORMAL)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();

            match output {
                Ok(result) if result.status.success() => {
                    let value = String::from_utf8_lossy(&result.stdout)
                        .lines()
                        .rev()
                        .find_map(|line| line.trim().parse::<u8>().ok());
                    if let Some(value) = value {
                        publish(&window, "brightness", value.min(100), false);
                    } else {
                        eprintln!("[HUD] WMI berhasil dipanggil tetapi level brightness tidak terbaca.");
                    }
                }
                Ok(result) => {
                    eprintln!(
                        "[HUD] Pengaturan brightness via WMI gagal (status {:?}); perangkat mungkin tidak mendukung WmiSetBrightness.",
                        result.status.code()
                    );
                }
                Err(error) => eprintln!("[HUD] Gagal menjalankan PowerShell untuk brightness: {error}"),
            }
        });
    }

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
        // Menangani OSD brightness yang dipicu langsung oleh driver ACPI, bukan keyboard hook.
        if std::env::var_os("NOTCH_DISABLE_SYSTEM_OSD").is_none() {
            start_osd_suppressor(window.clone());
        }
        start_volume_watcher(window.clone(), last_volume);
        start_brightness_watcher(window);
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::NotchWindow;
    use slint::Weak;

    pub fn adjust_brightness(_window: Weak<NotchWindow>, _delta: i32) {}
    pub fn start(_window: Weak<NotchWindow>) {}
}

pub fn adjust_brightness(window: Weak<NotchWindow>, delta: i32) {
    imp::adjust_brightness(window, delta);
}

pub fn start(window: Weak<NotchWindow>) {
    imp::start(window);
}
