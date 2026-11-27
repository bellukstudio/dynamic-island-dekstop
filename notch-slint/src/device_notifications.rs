//! Native Windows device notifications for the Slint Notch.
//! Mirrors the Tauri device-card lifecycle and adds Wi-Fi connection changes.
use crate::*;
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct DeviceNotice {
    kind: String,
    name: String,
    detail: String,
    connected: bool,
}

type NoticeQueue = Arc<Mutex<VecDeque<DeviceNotice>>>;

fn enqueue(queue: &NoticeQueue, notice: DeviceNotice) {
    let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
    // A charger has one physical state: while the UI is busy, keep only its newest state
    // instead of replaying stale unplug/plug events later.
    if notice.kind == "charger" {
        queue.retain(|pending| pending.kind != "charger");
    }
    // Keep the newest events without allowing a rapid USB scan to grow memory indefinitely.
    if queue.len() >= 24 {
        queue.pop_front();
    }
    queue.push_back(notice);
}

fn make_notice(kind: &str, name: &str, connected: bool, extra: Option<&str>) -> DeviceNotice {
    let label = match kind {
        "charger" => "Charger",
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "storage" => "Penyimpanan",
        "keyboard" => "Keyboard",
        "mouse" => "Mouse",
        "audio" => "Audio",
        "camera" => "Kamera",
        "printer" => "Printer",
        "phone" => "Ponsel",
        "usbc" => "USB Type-C",
        _ => "USB",
    };
    let status = if connected { "Terhubung" } else { "Terputus" };
    let detail = match extra {
        Some(extra) if !extra.trim().is_empty() => format!("{label} · {status} · {extra}"),
        _ => format!("{label} · {status}"),
    };
    DeviceNotice {
        kind: kind.to_string(),
        name: if name.trim().is_empty() { label.to_string() } else { name.trim().to_string() },
        detail,
        connected,
    }
}

/// Apply or refresh the currently visible device card without restarting the native window.
fn display_notice(
    window: &NotchWindow,
    notice: &DeviceNotice,
    shown_at: &Cell<Option<Instant>>,
) {
    window.set_device_notice_kind(notice.kind.clone().into());
    window.set_device_notice_name(notice.name.clone().into());
    window.set_device_notice_detail(notice.detail.clone().into());
    window.set_device_notice_connected(notice.connected);
    let generation = window.get_device_notice_generation().wrapping_add(1);
    window.set_device_notice_generation(generation);
    window.set_device_notice_pulse(true);
    window.set_device_notice_visible(true);
    shown_at.set(Some(Instant::now()));

    let weak_pulse = window.as_weak();
    Timer::single_shot(Duration::from_millis(720), move || {
        if let Some(window) = weak_pulse.upgrade() {
            if window.get_device_notice_generation() == generation {
                window.set_device_notice_pulse(false);
            }
        }
    });
}

/// Display queued device events on the UI thread. Higher-priority surfaces keep their space.
pub fn start(window: Weak<NotchWindow>, shell: Rc<Shell>) -> Timer {
    let queue: NoticeQueue = Arc::new(Mutex::new(VecDeque::new()));

    #[cfg(windows)]
    {
        imp::start(queue.clone());
    }

    let shown_at = Rc::new(Cell::new(None::<Instant>));
    let mut timer = Timer::default();
    {
        let weak = window.clone();
        let pending = queue.clone();
        let shown_at = shown_at.clone();
        let shell = shell.clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            let Some(w) = weak.upgrade() else { return };

            let higher_priority = w.get_expanded()
                || w.get_dragging()
                || w.get_geometry_hold()
                || w.get_hud_visible()
                || w.get_adhan_active()
                || w.get_nudge_visible()
                || w.get_reminder_alert_visible()
                || w.get_greeting_inline_visible()
                || w.get_greeting_speech_visible()
                || w.get_meeting_active()
                || !w.get_meeting_finished().is_empty();

            if w.get_device_notice_visible() {
                // Selama kartu tampil, terapkan event terbaru langsung. Jangan menahan
                // cabut/pasang berikutnya sampai durasi kartu sebelumnya habis.
                if !higher_priority {
                    let next = pending.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    if let Some(notice) = next {
                        display_notice(&w, &notice, &shown_at);
                    }
                }

                // Tahan kartu minimal 3,2 detik setelah event terakhir, lalu beri 400 ms
                // untuk animasi keluar sebelum native window kembali mengecil.
                if let Some(started) = shown_at.get() {
                    if started.elapsed() >= Duration::from_millis(3_200) || higher_priority {
                        w.set_device_notice_visible(false);
                        w.set_device_notice_pulse(false);
                        shown_at.set(None);

                        // Pengendali pesan bersama di main.rs menahan region selama 400 ms
                        // lalu memulihkan ukuran native. Hindari timer shrink kedua di sini.
                    }
                }
                return;
            }

            if higher_priority {
                return;
            }

            let next = pending.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            let Some(notice) = next else { return };

            // Kartu atas 340x74 membutuhkan native window 360x140 termasuk telinga.
            // Sembunyikan notch sebentar saat ukuran native harus berubah agar tidak muncul
            // frame lama yang terpotong; dock bawah/samping tetap memakai SpeechBubble Tauri.
            let needs_device_window = w.get_dock() < 2
                && window_needs_resize(&w, COLLAPSED_DEVICE);

            if needs_device_window {
                w.set_geometry_hold(true);
                let weak_show = weak.clone();
                let pending_show = pending.clone();
                let shown_at_show = shown_at.clone();
                let shell_show = shell.clone();
                Timer::single_shot(Duration::from_millis(40), move || {
                    let Some(w) = weak_show.upgrade() else { return };
                    let blocked = w.get_expanded()
                        || w.get_dragging()
                        || w.get_hud_visible()
                        || w.get_adhan_active()
                        || w.get_nudge_visible()
                        || w.get_reminder_alert_visible()
                        || w.get_greeting_inline_visible()
                        || w.get_greeting_speech_visible()
                        || w.get_meeting_active()
                        || !w.get_meeting_finished().is_empty();
                    if blocked {
                        w.set_geometry_hold(false);
                        enqueue(&pending_show, notice);
                        return;
                    }

                    platform::wait_vsync();
                    display_notice(&w, &notice, &shown_at_show);
                    shell_show.place(&w, COLLAPSED);
                    w.set_geometry_hold(false);
                });
            } else {
                display_notice(&w, &notice, &shown_at);
                // Refresh the native region for side/bottom SpeechBubble and for an already-sized top card.
                shell.place(&w, COLLAPSED);
            }
        });
    }
    timer
}

#[cfg(windows)]
mod imp {
    use super::{enqueue, make_notice, NoticeQueue};
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eRender, IDeviceTopology, IMMDeviceEnumerator, DEVICE_STATEMASK_ALL, MMDeviceEnumerator,
    };
    use windows::Win32::Media::KernelStreaming::{
        IKsJackDescription, IKsJackDescription2, JACKDESC2_PRESENCE_DETECT_CAPABILITY,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    const NO_WINDOW: u32 = 0x0800_0000;
    const BELOW_NORMAL: u32 = 0x0000_4000;
    const RECYCLE: Duration = Duration::from_secs(30 * 60);

    #[derive(Clone, PartialEq, Eq)]
    struct UsbDevice {
        kind: String,
        name: String,
    }

    fn parse_usb(lines: &[String]) -> HashMap<String, UsbDevice> {
        let mut groups: HashMap<String, (HashSet<String>, Vec<String>)> = HashMap::new();
        for line in lines {
            let mut parts = line.splitn(3, '|');
            let (Some(id), Some(class), Some(name)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let Some(instance) = id.split('\\').nth(1) else { continue };
            let key = instance.split("&MI_").next().unwrap_or(instance).to_string();
            let group = groups.entry(key).or_default();
            group.0.insert(class.to_ascii_lowercase());
            group.1.push(name.trim().to_string());
        }

        let mut result = HashMap::new();
        for (key, (classes, names)) in groups {
            let all_names = names.join(" ").to_ascii_lowercase();
            if all_names.contains("hub") || classes.contains("bluetooth") {
                continue;
            }
            let has = |class: &str| classes.contains(class);
            let kind = if all_names.contains("mass storage") || has("diskdrive") { "storage" }
                else if all_names.contains("type-c") || all_names.contains("type c")
                    || all_names.contains("usb-c") || all_names.contains("thunderbolt") { "usbc" }
                else if has("keyboard") { "keyboard" }
                else if has("mouse") { "mouse" }
                else if has("media") || has("audioendpoint") { "audio" }
                else if has("camera") || has("image") { "camera" }
                else if has("printer") || has("printqueue") { "printer" }
                else if has("wpd") { "phone" }
                else { "usb" };
            let name = names.iter()
                .find(|n| !n.trim().is_empty() && !n.starts_with("USB Composite") && !n.starts_with("USB Input"))
                .or_else(|| names.iter().find(|n| !n.trim().is_empty()))
                .cloned()
                .unwrap_or_else(|| "USB Device".to_string());
            result.insert(key, UsbDevice { kind: kind.to_string(), name });
        }
        result
    }

    fn parse_bluetooth(lines: &[String]) -> HashMap<String, (String, Option<u8>)> {
        let mut result = HashMap::new();
        for line in lines {
            let Some(rest) = line.strip_prefix("BT|") else { continue };
            let mut parts = rest.splitn(3, '|');
            let (Some(id), Some(battery), Some(name)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let Some(key) = id.split('\\').nth(1) else { continue };
            let name = name.trim();
            if name.is_empty() { continue; }
            let battery = battery.trim().parse::<u8>().ok().filter(|value| *value <= 100);
            result.entry(key.to_string()).or_insert((name.to_string(), battery));
        }
        result
    }

    // PnP snapshots can briefly be incomplete while Windows refreshes device state.
    // Require two identical scans before publishing connect/disconnect transitions.
    fn stable_usb_snapshot(
        previous: &mut Option<HashMap<String, UsbDevice>>,
        candidate: &mut Option<HashMap<String, UsbDevice>>,
        samples: &mut u8,
        current: HashMap<String, UsbDevice>,
        queue: &NoticeQueue,
    ) {
        if candidate.as_ref() == Some(&current) {
            *samples = samples.saturating_add(1);
        } else {
            *candidate = Some(current.clone());
            *samples = 1;
        }

        if *samples >= 2 {
            diff_usb(previous, current, queue);
            *candidate = None;
            *samples = 0;
        }
    }

    fn stable_bluetooth_snapshot(
        previous: &mut Option<HashMap<String, (String, Option<u8>)>>,
        candidate: &mut Option<HashMap<String, (String, Option<u8>)>>,
        samples: &mut u8,
        current: HashMap<String, (String, Option<u8>)>,
        queue: &NoticeQueue,
    ) {
        if candidate.as_ref() == Some(&current) {
            *samples = samples.saturating_add(1);
        } else {
            *candidate = Some(current.clone());
            *samples = 1;
        }

        if *samples >= 2 {
            diff_bluetooth(previous, current, queue);
            *candidate = None;
            *samples = 0;
        }
    }

    // Snapshot jack kabel dibaca melalui Core Audio, bukan dari nama endpoint registry.
    // Hanya jack yang mengiklankan PRESENCE_DETECT_CAPABILITY yang boleh menghasilkan event.
    fn stable_jack_snapshot(
        previous: &mut Option<HashMap<String, bool>>,
        candidate: &mut Option<HashMap<String, bool>>,
        samples: &mut u8,
        current: HashMap<String, bool>,
        queue: &NoticeQueue,
    ) {
        if candidate.as_ref() == Some(&current) {
            *samples = samples.saturating_add(1);
        } else {
            *candidate = Some(current.clone());
            *samples = 1;
        }

        if *samples < 2 {
            return;
        }

        if let Some(old) = previous.as_ref() {
            for (key, connected) in &current {
                let changed = old.get(key).is_some_and(|was| was != connected);
                let newly_connected = !old.contains_key(key) && *connected;
                if changed || newly_connected {
                    enqueue(
                        queue,
                        make_notice("audio", "Earphone kabel", *connected, Some("Jack 3,5 mm")),
                    );
                }
            }

            // Beberapa driver menghapus endpoint dinamis ketika jack dicabut.
            for (key, was_connected) in old {
                if *was_connected && !current.contains_key(key) {
                    enqueue(
                        queue,
                        make_notice("audio", "Earphone kabel", false, Some("Jack 3,5 mm")),
                    );
                }
            }
        }

        *previous = Some(current);
        *candidate = None;
        *samples = 0;
    }

    fn stable_wifi_snapshot(
        previous: &mut Option<(bool, String)>,
        candidate: &mut Option<(bool, String)>,
        samples: &mut u8,
        connected: bool,
        name: String,
        queue: &NoticeQueue,
    ) {
        let current = (connected, name);
        if candidate.as_ref() == Some(&current) {
            *samples = samples.saturating_add(1);
        } else {
            *candidate = Some(current.clone());
            *samples = 1;
        }

        if *samples >= 2 {
            update_wifi(previous, current.0, current.1, queue);
            *candidate = None;
            *samples = 0;
        }
    }

    fn diff_usb(
        previous: &mut Option<HashMap<String, UsbDevice>>,
        current: HashMap<String, UsbDevice>,
        queue: &NoticeQueue,
    ) {
        if let Some(old) = previous.as_ref() {
            for (key, device) in &current {
                if !old.contains_key(key) {
                    enqueue(queue, make_notice(&device.kind, &device.name, true, None));
                }
            }
            for (key, device) in old {
                if !current.contains_key(key) {
                    enqueue(queue, make_notice(&device.kind, &device.name, false, None));
                }
            }
        }
        *previous = Some(current);
    }

    fn diff_bluetooth(
        previous: &mut Option<HashMap<String, (String, Option<u8>)>>,
        current: HashMap<String, (String, Option<u8>)>,
        queue: &NoticeQueue,
    ) {
        if let Some(old) = previous.as_ref() {
            for (key, (name, battery)) in &current {
                if !old.contains_key(key) {
                    let extra = battery.map(|value| format!("Baterai {value}%"));
                    enqueue(queue, make_notice("bluetooth", name, true, extra.as_deref()));
                }
            }
            for (key, (name, _)) in old {
                if !current.contains_key(key) {
                    enqueue(queue, make_notice("bluetooth", name, false, None));
                }
            }
        }
        *previous = Some(current);
    }

    fn update_wifi(
        previous: &mut Option<(bool, String)>,
        connected: bool,
        name: String,
        queue: &NoticeQueue,
    ) {
        let current = (connected, name.clone());
        if let Some(old) = previous.as_ref() {
            if old != &current {
                enqueue(queue, make_notice("wifi", if connected { &name } else { "Wi-Fi" }, connected, None));
            }
        }
        *previous = Some(current);
    }

    const WATCH_SCRIPT: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$i = 0
while ($true) {
  # Scan USB and Bluetooth every cycle; snapshots are still debounced in Rust.
  Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'USB\*' } |
    ForEach-Object {
      $id = $_.InstanceId.Replace('|', '/')
      $class = ([string]$_.Class).Replace('|', '/')
      $name = ([string]$_.FriendlyName).Replace('|', '/')
      'D|' + $id + '|' + $class + '|' + $name
    }

  'D|---'

  Get-PnpDevice -Class Bluetooth -PresentOnly | Where-Object {
    $_.InstanceId -like 'BTHENUM\DEV_*' -or $_.InstanceId -like 'BTHLE\DEV_*'
  } | ForEach-Object {
    $id = $_.InstanceId
    $connected = (Get-PnpDeviceProperty -InstanceId $id -KeyName $conn).Data
    if ($connected -eq $true) {
      $level = (Get-PnpDeviceProperty -InstanceId $id -KeyName $batt).Data
      $name = ([string]$_.FriendlyName).Replace('|', '/')
      'T|BT|' + $id + '|' + $level + '|' + $name
    }
  }
  'T|---'

  if ($i % 3 -eq 0) {
    $wifi = Get-NetAdapter -Physical | Where-Object {
      $_.NdisPhysicalMedium -eq 9 -or $_.InterfaceDescription -match 'Wireless|Wi-Fi|802\.11' -or $_.Name -match 'Wi-Fi|WLAN'
    } | Select-Object -First 1
    if ($wifi -and $wifi.Status -eq 'Up') {
      $profile = Get-NetConnectionProfile -InterfaceIndex $wifi.ifIndex | Select-Object -First 1
      if ($profile) {
        $ssid = ([string]$profile.Name).Replace('|', '/')
        if ([string]::IsNullOrWhiteSpace($ssid)) { $ssid = [string]$wifi.Name }
        'W|1|' + $ssid
      } else {
        # Adaptor aktif tetapi profil belum tersedia: tampilkan nama adaptor, jangan salah tandai putus.
        'W|1|' + ([string]$wifi.Name).Replace('|', '/')
      }
    } else {
      'W|0|Wi-Fi'
    }
    'W|---'
  }

  $i = ($i + 1) % 12
  if ($i -eq 0) { [GC]::Collect() }
  Start-Sleep -Milliseconds 1000
}
"##;

    fn start_pnp_watcher(queue: NoticeQueue) {
        thread::spawn(move || {
            loop {
                let spawn = Command::new("powershell")
                    .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", WATCH_SCRIPT])
                    .creation_flags(NO_WINDOW | BELOW_NORMAL)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();

                match spawn {
                    Ok(mut child) => {
                        let Some(stdout) = child.stdout.take() else {
                            let _ = child.kill();
                            let _ = child.wait();
                            thread::sleep(Duration::from_secs(5));
                            continue;
                        };
                        let started = Instant::now();
                        let mut usb_lines: Vec<String> = Vec::new();
                        let mut bt_lines: Vec<String> = Vec::new();
                        let mut usb_previous = None;
                        let mut usb_candidate = None;
                        let mut usb_candidate_samples = 0u8;
                        let mut bt_previous = None;
                        let mut bt_candidate = None;
                        let mut bt_candidate_samples = 0u8;
                        let mut wifi_previous = None;
                        let mut wifi_candidate = None;
                        let mut wifi_candidate_samples = 0u8;

                        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                            if started.elapsed() >= RECYCLE {
                                break;
                            }
                            let line = line.trim().to_string();
                            if let Some(rest) = line.strip_prefix("D|") {
                                if rest == "---" {
                                    let snapshot = parse_usb(&usb_lines);
                                    usb_lines.clear();
                                    stable_usb_snapshot(
                                        &mut usb_previous,
                                        &mut usb_candidate,
                                        &mut usb_candidate_samples,
                                        snapshot,
                                        &queue,
                                    );
                                } else if !rest.is_empty() {
                                    usb_lines.push(rest.to_string());
                                }
                            } else if let Some(rest) = line.strip_prefix("T|") {
                                if rest == "---" {
                                    let snapshot = parse_bluetooth(&bt_lines);
                                    bt_lines.clear();
                                    stable_bluetooth_snapshot(
                                        &mut bt_previous,
                                        &mut bt_candidate,
                                        &mut bt_candidate_samples,
                                        snapshot,
                                        &queue,
                                    );
                                } else if !rest.is_empty() {
                                    bt_lines.push(rest.to_string());
                                }
                            } else if let Some(rest) = line.strip_prefix("W|") {
                                if rest == "---" {
                                    // Each Wi-Fi sample is sent as one line, so the pending sample
                                    // is carried in the first entry of this small batch.
                                } else {
                                    let mut parts = rest.splitn(2, '|');
                                    if let (Some(state), Some(name)) = (parts.next(), parts.next()) {
                                        stable_wifi_snapshot(
                                            &mut wifi_previous,
                                            &mut wifi_candidate,
                                            &mut wifi_candidate_samples,
                                            state == "1",
                                            name.to_string(),
                                            &queue,
                                        );
                                    }
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    Err(error) => {
                        eprintln!("[DeviceNotice] PowerShell watcher gagal dijalankan: {error}");
                    }
                }
                thread::sleep(Duration::from_secs(2));
            }
        });
    }

    // Membaca status fisik jack lewat Core Audio Device Topology.
    // Registry MMDevices hanya menyatakan endpoint ada/aktif; ia bukan sensor plug 3,5 mm.
    unsafe fn activate_part_interface<T: Interface>(part: &IPart) -> Option<T> {
        let mut raw = std::ptr::null_mut();
        part.Activate(CLSCTX_ALL.0 as u32, &T::IID, Some(&mut raw)).ok()?;
        if raw.is_null() {
            return None;
        }
        Some(T::from_raw(raw))
    }

    fn scan_wired_headphone_jacks(
        enumerator: &IMMDeviceEnumerator,
    ) -> Option<HashMap<String, bool>> {
        unsafe {
            let endpoints = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATEMASK_ALL)
                .ok()?;
            let count = endpoints.GetCount().ok()?;
            let mut result = HashMap::new();

            for device_index in 0..count {
                let Ok(device) = endpoints.Item(device_index) else { continue };
                let Ok(endpoint_id) = device.GetId() else { continue };
                let Ok(endpoint_id) = endpoint_id.to_string() else { continue };
                let Ok(topology) = device.Activate::<IDeviceTopology>(CLSCTX_ALL, None) else { continue };
                let Ok(connector_count) = topology.GetConnectorCount() else { continue };

                for connector_index in 0..connector_count {
                    let Ok(connector) = topology.GetConnector(connector_index) else { continue };
                    let Ok(part) = connector.cast::<IPart>() else { continue };
                    let Some(jack_description) = activate_part_interface::<IKsJackDescription>(&part) else {
                        continue;
                    };
                    let Some(jack_description2) = activate_part_interface::<IKsJackDescription2>(&part) else {
                        continue;
                    };
                    let Ok(jack_count) = jack_description2.GetJackCount() else { continue };

                    for jack_index in 0..jack_count.min(16) {
                        let Ok(capabilities) = jack_description2.GetJackDescription2(jack_index) else {
                            continue;
                        };
                        if capabilities.JackCapabilities & JACKDESC2_PRESENCE_DETECT_CAPABILITY == 0 {
                            continue;
                        }

                        let mut description = windows::Win32::Media::KernelStreaming::KSJACK_DESCRIPTION::default();
                        if jack_description.GetJackDescription(jack_index, &mut description).is_err() {
                            continue;
                        }

                        // eConnType3Point5mm = 1. Hindari memicu kartu untuk HDMI, USB audio,
                        // konektor internal, atau endpoint Bluetooth.
                        if description.ConnectionType.0 != 1 {
                            continue;
                        }

                        let key = format!("{endpoint_id}|{connector_index}|{jack_index}");
                        result.insert(key, description.IsConnected.0 != 0);
                    }
                }
            }

            Some(result)
        }
    }

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize(); }
        }
    }

    fn start_jack_watcher(queue: NoticeQueue) {
        thread::spawn(move || unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
            if initialized.is_err() {
                eprintln!("[DeviceNotice] COM initialization for audio jack watcher failed: {initialized:?}");
                return;
            }
            let _apartment = ComApartment;

            let enumerator = match CoCreateInstance::<_, IMMDeviceEnumerator>(
                &MMDeviceEnumerator,
                None,
                CLSCTX_ALL,
            ) {
                Ok(enumerator) => enumerator,
                Err(error) => {
                    eprintln!("[DeviceNotice] Core Audio enumerator unavailable: {error}");
                    return;
                }
            };

            let mut previous = None;
            let mut candidate = None;
            let mut candidate_samples = 0u8;

            loop {
                thread::sleep(Duration::from_millis(500));
                if let Some(snapshot) = scan_wired_headphone_jacks(&enumerator) {
                    stable_jack_snapshot(
                        &mut previous,
                        &mut candidate,
                        &mut candidate_samples,
                        snapshot,
                        &queue,
                    );
                }
                // Scan errors are ignored: a temporary Core Audio restart must not be
                // interpreted as the user unplugging the earphone.
            }
        });
    }

    /// Return only authoritative AC-line states. Windows uses 255 when the state is unknown;
    /// treating that value as disconnected causes false connect/disconnect notifications.
    fn charger_state() -> Option<bool> {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        match status.ACLineStatus {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn start_charger_watcher(queue: NoticeQueue) {
        thread::spawn(move || {
            const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
            const REQUIRED_STABLE_SAMPLES: u8 = 3;

            // Keep the last confirmed state separate from transient/unknown readings.
            let mut stable_state = charger_state();
            let mut candidate_state: Option<bool> = None;
            let mut candidate_samples = 0u8;

            loop {
                thread::sleep(SAMPLE_INTERVAL);
                let Some(current) = charger_state() else {
                    // Unknown readings must never be interpreted as unplugging the charger.
                    candidate_state = None;
                    candidate_samples = 0;
                    continue;
                };

                if Some(current) == stable_state {
                    candidate_state = None;
                    candidate_samples = 0;
                    continue;
                }

                if candidate_state == Some(current) {
                    candidate_samples = candidate_samples.saturating_add(1);
                } else {
                    candidate_state = Some(current);
                    candidate_samples = 1;
                }

                // Require three readings (~750 ms) before publishing a transition;
                // shorter sampling improves response without trusting a single ACLineStatus glitch.
                if candidate_samples >= REQUIRED_STABLE_SAMPLES {
                    stable_state = Some(current);
                    candidate_state = None;
                    candidate_samples = 0;
                    enqueue(&queue, make_notice("charger", "Charger", current, None));
                }
            }
        });
    }

    pub fn start(queue: NoticeQueue) {
        start_pnp_watcher(queue.clone());
        start_jack_watcher(queue.clone());
        start_charger_watcher(queue);
    }
}
