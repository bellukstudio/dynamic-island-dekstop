use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize, Clone)]
struct HudEvent {
    kind: &'static str, // "volume" | "brightness"
    value: u8,          // 0-100
    muted: bool,
}

pub fn start(app: AppHandle) {
    imp::start(app);
}

// ======================= Linux =======================
#[cfg(target_os = "linux")]
mod imp {
    use super::HudEvent;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};

    // `pactl subscribe` mencetak satu baris tiap ada perubahan audio. Volume cukup dibaca saat ada
    // kejadian (dulu: menjalankan wpctl 4x per detik, terus-menerus, hanya untuk melihat "tidak berubah").
    fn subscribe_audio() -> Option<mpsc::Receiver<()>> {
        let mut child = Command::new("pactl")
            .arg("subscribe")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // contoh baris: "Event 'change' on sink #52" (sink-input / source tidak relevan)
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if (line.contains(" sink ") || line.contains(" server")) && tx.send(()).is_err() {
                    break;
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        Some(rx)
    }

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).output().ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).to_string())
    }

    // "Volume: 0.45" atau "Volume: 0.45 [MUTED]"
    fn volume_wpctl() -> Option<(u8, bool)> {
        let s = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
        let v: f64 = s.split_whitespace().nth(1)?.parse().ok()?;
        Some(((v * 100.0).round().clamp(0.0, 150.0) as u8, s.contains("MUTED")))
    }

    // cadangan untuk PulseAudio murni
    fn volume_pactl() -> Option<(u8, bool)> {
        let s = run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
        let i = s.find('%')?;
        let digits: String = s[..i]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let v: u8 = digits.parse().ok()?;
        let muted = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])
            .map(|m| m.contains("yes"))
            .unwrap_or(false);
        Some((v, muted))
    }

    fn brightness() -> Option<u8> {
        for e in std::fs::read_dir("/sys/class/backlight").ok()?.flatten() {
            let p = e.path();
            let cur: f64 = std::fs::read_to_string(p.join("brightness")).ok()?.trim().parse().ok()?;
            let max: f64 = std::fs::read_to_string(p.join("max_brightness")).ok()?.trim().parse().ok()?;
            if max > 0.0 {
                return Some((cur * 100.0 / max).round() as u8);
            }
        }
        None
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            let mut last_vol = volume_wpctl().or_else(volume_pactl);
            let mut last_br = brightness();
            println!("hud: volume={:?} brightness={:?}", last_vol, last_br);

            let mut events = subscribe_audio();
            let mut tick: u32 = 0;
            loop {
                let mut check_vol = false;
                let mut drop_events = false;
                match &events {
                    Some(rx) => match rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(()) => {
                            // gabungkan rentetan kejadian (mis. saat slider digeser) jadi satu pembacaan
                            std::thread::sleep(Duration::from_millis(60));
                            while rx.try_recv().is_ok() {}
                            check_vol = true;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            tick = tick.wrapping_add(1);
                            check_vol = tick % 8 == 0; // jaring pengaman tiap ~2 dtk
                        }
                        Err(RecvTimeoutError::Disconnected) => drop_events = true,
                    },
                    None => {
                        // pactl subscribe tidak tersedia: kembali ke polling, coba lagi tiap ~10 dtk
                        std::thread::sleep(Duration::from_millis(250));
                        check_vol = true;
                        tick = tick.wrapping_add(1);
                        if tick % 40 == 0 {
                            events = subscribe_audio();
                        }
                    }
                }
                if drop_events {
                    events = None;
                    continue;
                }

                if check_vol {
                    let v = volume_wpctl().or_else(volume_pactl);
                    if v.is_some() && v != last_vol {
                        last_vol = v;
                        let (value, muted) = v.unwrap();
                        let _ = app.emit("hud", HudEvent { kind: "volume", value, muted });
                    }
                }

                let b = brightness();
                if b.is_some() && b != last_br {
                    last_br = b;
                    let _ = app.emit("hud", HudEvent { kind: "brightness", value: b.unwrap(), muted: false });
                }
            }
        });
    }
}

// ======================= Windows =======================
#[cfg(target_os = "windows")]
mod imp {
    use super::HudEvent;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_SYSKEYDOWN,
    };
    use std::sync::mpsc::{channel, Sender};
    use std::sync::OnceLock;

    // ---- Tombol volume: ditelan supaya panel volume bawaan Windows tidak muncul ----
    // Tombol F/Fn volume dikirim sebagai VK_VOLUME_UP/DOWN/MUTE. Hook keyboard tingkat rendah menelan
    // tombol itu, lalu volume diubah sendiri lewat IAudioEndpointVolume. HUD notch tetap muncul karena
    // perubahan volume dibaca seperti biasa. Matikan dengan env `NOTCH_NATIVE_OSD=1`.
    // Kecerahan TIDAK bisa diperlakukan sama: tombol itu ditangani firmware/driver dan tidak lewat hook.
    #[derive(Clone, Copy)]
    enum Key {
        Up,
        Down,
        Mute,
    }
    static KEY_TX: OnceLock<Sender<Key>> = OnceLock::new();
    const VK_VOLUME_MUTE: u32 = 0xAD;
    const VK_VOLUME_DOWN: u32 = 0xAE;
    const VK_VOLUME_UP: u32 = 0xAF;
    const STEP: f32 = 0.02; // sama dengan langkah volume Windows

    unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == 0 {
            // SAFETY: untuk WH_KEYBOARD_LL dengan code == HC_ACTION, lparam menunjuk KBDLLHOOKSTRUCT yang valid.
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let key = match kb.vkCode {
                VK_VOLUME_UP => Some(Key::Up),
                VK_VOLUME_DOWN => Some(Key::Down),
                VK_VOLUME_MUTE => Some(Key::Mute),
                _ => None,
            };
            if let Some(k) = key {
                let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
                if down {
                    if let Some(tx) = KEY_TX.get() {
                        let _ = tx.send(k);
                    }
                }
                return LRESULT(1); // telan tombol (tekan dan lepas)
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn apply_key(en: &IMMDeviceEnumerator, key: Key) -> windows::core::Result<(u8, bool)> {
        unsafe {
            let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let vol = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            let cur = vol.GetMasterVolumeLevelScalar()?;
            let mut muted = vol.GetMute()?.as_bool();
            let mut level = cur;
            match key {
                Key::Up => {
                    level = (cur + STEP).min(1.0);
                    muted = false;
                }
                Key::Down => level = (cur - STEP).max(0.0),
                Key::Mute => muted = !muted,
            }
            vol.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
            vol.SetMute(muted, std::ptr::null())?;
            Ok(((level * 100.0).round() as u8, muted))
        }
    }

    fn start_key_hook(app: AppHandle) {
        let (tx, rx) = channel::<Key>();
        if KEY_TX.set(tx).is_err() {
            return; // sudah aktif
        }
        // Pekerja: mengubah volume (COM) dan langsung memberi tahu HUD supaya terasa instan.
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
            }
            let mut en = enumerator();
            for key in rx {
                if en.is_none() {
                    en = enumerator();
                }
                match en.as_ref().map(|e| apply_key(e, key)) {
                    Some(Ok((value, muted))) => {
                        let _ = app.emit("hud", HudEvent { kind: "volume", value, muted });
                    }
                    Some(Err(_)) => en = None,
                    None => {}
                }
            }
        });
        // Hook membutuhkan thread dengan message loop sendiri.
        std::thread::spawn(|| unsafe {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0) {
                Ok(_hook) => {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                }
                Err(e) => eprintln!("hud: hook tombol volume gagal: {e}"),
            }
        });
    }

    // enumerator dibuat sekali dan dipakai ulang (dulu: CoCreateInstance baru tiap 250 ms)
    fn enumerator() -> Option<IMMDeviceEnumerator> {
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    fn volume(en: &IMMDeviceEnumerator) -> windows::core::Result<(u8, bool)> {
        unsafe {
            let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let vol = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
            let v = vol.GetMasterVolumeLevelScalar()?;
            let m = vol.GetMute()?.as_bool();
            Ok(((v * 100.0).round() as u8, m))
        }
    }

    // kecerahan layar (hanya laptop / monitor internal): baris "B|nn" dari PowerShell bersama (winps.rs), tag "H:"
    fn brightness_watcher(app: AppHandle, rx: std::sync::mpsc::Receiver<String>) {
        let mut last: Option<u8> = None;
        let mut first = true;
        for line in rx {
            if let Some(v) = line.trim().strip_prefix("B|").and_then(|s| s.parse::<u8>().ok()) {
                if !first && Some(v) != last {
                    let _ = app.emit("hud", HudEvent { kind: "brightness", value: v, muted: false });
                }
                first = false;
                last = Some(v);
            }
        }
    }

    pub fn start(app: AppHandle) {
        if std::env::var_os("NOTCH_NATIVE_OSD").is_none() {
            start_key_hook(app.clone());
        }
        let rx = crate::winps::subscribe("H");
        let a = app.clone();
        std::thread::spawn(move || brightness_watcher(a, rx));

        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
            }
            let mut en = enumerator();
            let mut last = en.as_ref().and_then(|e| volume(e).ok());
            println!("hud: volume={:?}", last);

            loop {
                std::thread::sleep(Duration::from_millis(400));
                if en.is_none() {
                    en = enumerator();
                }
                match en.as_ref().map(volume) {
                    Some(Ok(v)) => {
                        if Some(v) != last {
                            last = Some(v);
                            let _ = app.emit("hud", HudEvent { kind: "volume", value: v.0, muted: v.1 });
                        }
                    }
                    Some(Err(_)) => en = None, // buat ulang di putaran berikutnya
                    None => {}
                }
            }
        });
    }
}
