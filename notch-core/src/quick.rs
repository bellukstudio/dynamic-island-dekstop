//! Tombol cepat: mic, Wi-Fi, Bluetooth, keep-awake, tema gelap, kunci layar.
//! Pembacaan/aksi tiap OS ada di sini; UI (Tauri/Slint) hanya memanggil `state()` dan `toggle()`.

use serde::Serialize;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// mic: true = dimatikan | wifi, bt, awake: true = menyala | dark: true = mode gelap
// null = tidak didukung / tidak terbaca
#[derive(Serialize, Default, Clone)]
pub struct Quick {
    pub mic: Option<bool>,
    pub wifi: Option<bool>,
    pub bt: Option<bool>,
    pub awake: Option<bool>,
    pub dark: Option<bool>,
}

// ======================= Linux =======================
#[cfg(target_os = "linux")]
mod imp {
    use std::process::{Child, Command};
    use std::sync::Mutex;

    static INHIBIT: Mutex<Option<Child>> = Mutex::new(None);

    fn out(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).output().ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).to_string())
    }

    pub fn mic() -> Option<bool> {
        out("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).map(|s| s.contains("MUTED"))
    }
    pub fn wifi() -> Option<bool> {
        out("nmcli", &["radio", "wifi"]).map(|s| s.trim() == "enabled")
    }
    pub fn bt() -> Option<bool> {
        out("bluetoothctl", &["show"]).map(|s| s.contains("Powered: yes"))
    }
    pub fn awake() -> Option<bool> {
        Some(INHIBIT.lock().ok()?.is_some())
    }
    pub fn dark() -> Option<bool> {
        out("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])
            .map(|s| s.contains("prefer-dark"))
    }

    fn set_awake(on: bool) {
        let Ok(mut g) = INHIBIT.lock() else { return };
        if on {
            if g.is_none() {
                *g = Command::new("systemd-inhibit")
                    .args(["--what=idle:sleep", "--who=notch", "--why=Keep awake", "sleep", "infinity"])
                    .spawn()
                    .ok();
            }
        } else if let Some(mut c) = g.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn toggle(kind: &str) {
        match kind {
            "mic" => {
                let _ = out("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]);
            }
            "wifi" => {
                let on = wifi().unwrap_or(true);
                let _ = out("nmcli", &["radio", "wifi", if on { "off" } else { "on" }]);
            }
            "bt" => {
                let on = bt().unwrap_or(true);
                let _ = out("bluetoothctl", &["power", if on { "off" } else { "on" }]);
            }
            "awake" => set_awake(!awake().unwrap_or(false)),
            "dark" => {
                let to = if dark().unwrap_or(false) { "default" } else { "prefer-dark" };
                let _ = out("gsettings", &["set", "org.gnome.desktop.interface", "color-scheme", to]);
            }
            "lock" => {
                let _ = out("loginctl", &["lock-session"]);
            }
            _ => {}
        }
    }
}

// ======================= Windows =======================
#[cfg(target_os = "windows")]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;
    use std::time::Duration;
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eCapture, eConsole, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    const NO_WINDOW: u32 = 0x0800_0000; // cegah jendela konsol berkedip
    const THEME_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    static AWAKE: AtomicBool = AtomicBool::new(false);
    static START: Once = Once::new();

    fn hidden(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).creation_flags(NO_WINDOW).output().ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).to_string())
    }

    // ---- radio (Wi-Fi / Bluetooth) ----
    fn radio(kind: RadioKind) -> Option<Radio> {
        unsafe { let _ = RoInitialize(RO_INIT_MULTITHREADED); }
        let _ = Radio::RequestAccessAsync().ok()?.get();
        let list = Radio::GetRadiosAsync().ok()?.get().ok()?;
        for i in 0..list.Size().ok()? {
            let r = list.GetAt(i).ok()?;
            if r.Kind().ok() == Some(kind) {
                return Some(r);
            }
        }
        None
    }
    fn is_on(kind: RadioKind) -> Option<bool> {
        radio(kind)?.State().ok().map(|s| s == RadioState::On)
    }
    fn flip(kind: RadioKind) {
        if let Some(r) = radio(kind) {
            let on = r.State().ok() == Some(RadioState::On);
            let next = if on { RadioState::Off } else { RadioState::On };
            if let Ok(op) = r.SetStateAsync(next) { let _ = op.get(); }
        }
    }

    // ---- mute mikrofon: kontrol volume pada perangkat rekam DEFAULT ----
    // (sama dengan tombol mute mic di pengaturan suara Windows; mempengaruhi semua aplikasi)
    fn mic_volume() -> Option<IAudioEndpointVolume> {
        unsafe {
            // thread pemanggil belum tentu sudah memulai COM; aman dipanggil berulang
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = en.GetDefaultAudioEndpoint(eCapture, eConsole).ok()?;
            dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
        }
    }
    fn mic_muted() -> Option<bool> {
        unsafe { Some(mic_volume()?.GetMute().ok()?.as_bool()) }
    }
    fn mic_flip() {
        if let Some(v) = mic_volume() {
            unsafe {
                let muted = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                let _ = v.SetMute(!muted, std::ptr::null());
            }
        }
    }

    // ---- keep awake: thread khusus, karena SetThreadExecutionState berlaku per thread ----
    fn ensure_awake_thread() {
        START.call_once(|| {
            std::thread::spawn(|| {
                let mut last = false;
                loop {
                    let want = AWAKE.load(Ordering::SeqCst);
                    if want != last {
                        last = want;
                        unsafe {
                            if want {
                                SetThreadExecutionState(
                                    ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED,
                                );
                            } else {
                                SetThreadExecutionState(ES_CONTINUOUS);
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            });
        });
    }

    // ---- mode gelap ----
    fn broadcast_theme() {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                200,
                None,
            );
        }
    }

    pub fn mic() -> Option<bool> { mic_muted() }
    pub fn wifi() -> Option<bool> { is_on(RadioKind::WiFi) }
    pub fn bt() -> Option<bool> { is_on(RadioKind::Bluetooth) }
    pub fn awake() -> Option<bool> {
        ensure_awake_thread();
        Some(AWAKE.load(Ordering::SeqCst))
    }
    pub fn dark() -> Option<bool> {
        let s = hidden("reg", &["query", THEME_KEY, "/v", "AppsUseLightTheme"])?;
        Some(s.split_whitespace().last() == Some("0x0"))
    }

    pub fn toggle(kind: &str) {
        match kind {
            "mic" => mic_flip(),
            "wifi" => flip(RadioKind::WiFi),
            "bt" => flip(RadioKind::Bluetooth),
            "awake" => {
                ensure_awake_thread();
                AWAKE.store(!AWAKE.load(Ordering::SeqCst), Ordering::SeqCst);
            }
            "dark" => {
                let value = if dark().unwrap_or(false) { "1" } else { "0" }; // 0 = gelap
                for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
                    let _ = hidden(
                        "reg",
                        &["add", THEME_KEY, "/v", name, "/t", "REG_DWORD", "/d", value, "/f"],
                    );
                }
                broadcast_theme();
            }
            "lock" => {
                let _ = Command::new("rundll32.exe")
                    .arg("user32.dll,LockWorkStation")
                    .creation_flags(NO_WINDOW)
                    .spawn();
            }
            _ => {}
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    pub fn mic() -> Option<bool> { None }
    pub fn wifi() -> Option<bool> { None }
    pub fn bt() -> Option<bool> { None }
    pub fn awake() -> Option<bool> { None }
    pub fn dark() -> Option<bool> { None }
    pub fn toggle(_kind: &str) {}
}

// Setiap pembacaan jalan di thread sendiri dan paralel, dengan batas waktu bersama.
// Kalau satu API (radio, audio, registry) lambat atau macet, hanya tombolnya yang redup;
// tombol lain tetap hidup. Sebelumnya satu pembacaan yang macet membuat SEMUA tombol redup,
// karena frontend baru mengaktifkan tombol setelah command ini mengembalikan jawaban.
const PROBE_TIMEOUT: Duration = Duration::from_millis(3000);

pub fn state() -> Quick {
    type Probe = fn() -> Option<bool>;
    let probes: [(&str, Probe); 5] = [
        ("mic", imp::mic),
        ("wifi", imp::wifi),
        ("bt", imp::bt),
        ("awake", imp::awake),
        ("dark", imp::dark),
    ];

    let receivers: Vec<mpsc::Receiver<Option<bool>>> = probes
        .iter()
        .map(|(_, f)| {
            let f = *f;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(f());
            });
            rx
        })
        .collect();

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut out: [Option<bool>; 5] = [None; 5];
    for (i, rx) in receivers.iter().enumerate() {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(v) => out[i] = v,
            Err(_) => eprintln!("quick: pembacaan \"{}\" tidak selesai dalam 3 detik, dilewati", probes[i].0),
        }
    }

    Quick { mic: out[0], wifi: out[1], bt: out[2], awake: out[3], dark: out[4] }
}

/// Jalankan aksi (mic, wifi, bt, awake, dark, lock) lalu kembalikan keadaan terbaru.
/// Aksi diberi batas waktu supaya satu aksi yang macet tidak membekukan tombol.
pub fn toggle(kind: &str) -> Quick {
    let (tx, rx) = mpsc::channel();
    let k = kind.to_string();
    std::thread::spawn(move || {
        imp::toggle(&k);
        let _ = tx.send(());
    });
    if rx.recv_timeout(Duration::from_secs(4)).is_err() {
        eprintln!("quick: aksi \"{kind}\" tidak selesai dalam 4 detik");
    }
    std::thread::sleep(Duration::from_millis(200));
    state()
}
