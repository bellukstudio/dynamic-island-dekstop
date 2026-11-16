use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize, Clone)]
struct DeviceEvent {
    kind: String,
    name: String,
    connected: bool,
}

pub fn start(app: AppHandle) {
    imp::start(app);
}

// ======================= Linux (/sys + pactl) =======================
#[cfg(target_os = "linux")]
mod imp {
    use super::DeviceEvent;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};

    fn read(p: &Path) -> String {
        fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    fn usb_kind(dev: &Path, dev_name: &str) -> Option<String> {
        let dclass = read(&dev.join("bDeviceClass"));
        if dclass == "09" {
            return None; // hub
        }

        let mut classes: Vec<(String, String)> = vec![(dclass.clone(), String::new())];
        if let Ok(rd) = fs::read_dir(dev) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with(&format!("{dev_name}:")) {
                    classes.push((
                        read(&e.path().join("bInterfaceClass")),
                        read(&e.path().join("bInterfaceProtocol")),
                    ));
                }
            }
        }
        let has = |c: &str| classes.iter().any(|(k, _)| k == c);
        let hid = |p: &str| classes.iter().any(|(k, pr)| k == "03" && pr == p);

        if has("e0") {
            return None; // adapter bluetooth
        }
        Some(
            if has("08") { "storage" }
            else if has("01") { "audio" }
            else if hid("01") { "keyboard" }
            else if hid("02") { "mouse" }
            else if has("0e") { "camera" }
            else if has("07") { "printer" }
            else if has("06") { "phone" }
            else { "usb" }
            .to_string(),
        )
    }

    // Hanya field yang dipakai yang disimpan; sisa JSON pactl dilewati tanpa dialokasikan.
    #[derive(serde::Deserialize)]
    struct Sink {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        ports: Option<Vec<Port>>,
    }
    #[derive(serde::Deserialize)]
    struct Port {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        availability: Option<String>,
    }

    // jack headset 3.5mm: port sink headphones/headset yang "available"
    fn scan_jack(out: &mut HashMap<String, (String, String)>) {
        let Ok(o) = std::process::Command::new("pactl")
            .args(["-f", "json", "list", "sinks"])
            .output()
        else {
            return;
        };
        let Ok(sinks) = serde_json::from_slice::<Vec<Sink>>(&o.stdout) else {
            return;
        };

        for s in &sinks {
            let sink = s.name.as_deref().unwrap_or("");
            // Bluetooth dan USB sudah ditangani modul lain
            if sink.contains("bluez") || sink.contains("usb") {
                continue;
            }
            for p in s.ports.iter().flatten() {
                let name = p.name.as_deref().unwrap_or("");
                let avail = p.availability.as_deref().unwrap_or("");
                if avail == "available" && (name.contains("headphone") || name.contains("headset")) {
                    out.insert(
                        format!("jack:{sink}"),
                        ("audio".into(), "Headphone (jack 3.5mm)".into()),
                    );
                }
            }
        }
    }

    // Jack butuh spawn `pactl`, jadi hanya disegarkan tiap putaran genap (~3 dtk); sisanya pakai hasil terakhir.
    fn scan(
        jack: &mut HashMap<String, (String, String)>,
        refresh_jack: bool,
    ) -> HashMap<String, (String, String)> {
        let mut out = HashMap::new();

        // 1) perangkat USB
        if let Ok(rd) = fs::read_dir("/sys/bus/usb/devices") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.contains(':') || name.starts_with("usb") {
                    continue;
                }
                let p = e.path();
                if let Some(kind) = usb_kind(&p, &name) {
                    let mut label = read(&p.join("product"));
                    if label.is_empty() {
                        label = read(&p.join("manufacturer"));
                    }
                    if label.is_empty() {
                        label = format!(
                            "USB {}:{}",
                            read(&p.join("idVendor")),
                            read(&p.join("idProduct"))
                        );
                    }
                    out.insert(format!("usb:{name}"), (kind, label));
                }
            }
        }

        // 2) Type-C: ada pasangan (partner) yang terhubung
        if let Ok(rd) = fs::read_dir("/sys/class/typec") {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.ends_with("-partner") {
                    out.insert(format!("typec:{n}"), ("usbc".into(), "USB Type-C".into()));
                }
            }
        }

        // 3) charger / sumber daya eksternal
        if let Ok(rd) = fs::read_dir("/sys/class/power_supply") {
            for e in rd.flatten() {
                let p = e.path();
                let ty = read(&p.join("type"));
                if matches!(ty.as_str(), "Mains" | "USB" | "USB_C" | "USB_PD" | "USB_DCP")
                    && read(&p.join("online")) == "1"
                {
                    let n = e.file_name().to_string_lossy().to_string();
                    out.insert(format!("ps:{n}"), ("charger".into(), "Charger".into()));
                }
            }
        }

        // 4) jack headset 3.5mm
        if refresh_jack {
            jack.clear();
            scan_jack(jack);
        }
        out.extend(jack.iter().map(|(k, v)| (k.clone(), v.clone())));
        out
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            let mut jack: HashMap<String, (String, String)> = HashMap::new();
            let mut round: u32 = 0;
            let mut known = scan(&mut jack, true);
            println!("devices: {} perangkat terdeteksi saat start", known.len());
            let ls = |p: &str| {
                fs::read_dir(p)
                    .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            println!("devices: /sys/class/typec = {:?}", ls("/sys/class/typec"));
            println!("devices: /sys/bus/usb/devices = {:?}", ls("/sys/bus/usb/devices"));
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                round = round.wrapping_add(1);
                let now = scan(&mut jack, round % 2 == 0);

                for (k, (kind, name)) in &now {
                    if !known.contains_key(k) {
                        println!("devices: colok {kind} {name}");
                        let _ = app.emit(
                            "device",
                            DeviceEvent { kind: kind.clone(), name: name.clone(), connected: true },
                        );
                    }
                }
                for (k, (kind, name)) in &known {
                    if !now.contains_key(k) {
                        println!("devices: cabut {kind} {name}");
                        let _ = app.emit(
                            "device",
                            DeviceEvent { kind: kind.clone(), name: name.clone(), connected: false },
                        );
                    }
                }
                known = now;
            }
        });
    }
}

// ======================= Windows (PowerShell + power status) =======================
#[cfg(target_os = "windows")]
mod imp {
    use super::DeviceEvent;
    use std::collections::{HashMap, HashSet};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    // satu perangkat fisik bisa punya beberapa entri (antarmuka &MI_xx), jadi dikelompokkan
    fn parse(lines: &[String]) -> HashMap<String, (String, String)> {
        let mut groups: HashMap<String, (HashSet<String>, Vec<String>)> = HashMap::new();
        let mut jacks: Vec<String> = Vec::new();

        for l in lines {
            if let Some(n) = l.strip_prefix("JACK|") {
                jacks.push(n.to_string());
                continue;
            }
            let mut p = l.splitn(3, '|');
            let (Some(id), Some(class), Some(name)) = (p.next(), p.next(), p.next()) else { continue };
            let Some(seg) = id.split('\\').nth(1) else { continue };
            let key = seg.split("&MI_").next().unwrap_or(seg).to_string();
            let e = groups.entry(key).or_default();
            e.0.insert(class.to_lowercase());
            e.1.push(name.to_string());
        }

        let mut out = HashMap::new();
        for (key, (classes, names)) in groups {
            let all = names.join(" ").to_lowercase();
            if all.contains("hub") || classes.contains("bluetooth") {
                continue;
            }
            let has = |c: &str| classes.contains(c);
            let kind = if all.contains("mass storage") || has("diskdrive") { "storage" }
                else if has("keyboard") { "keyboard" }
                else if has("mouse") { "mouse" }
                else if has("media") || has("audioendpoint") { "audio" }
                else if has("camera") || has("image") { "camera" }
                else if has("printer") || has("printqueue") { "printer" }
                else if has("wpd") { "phone" }
                else { "usb" };

            let label = names
                .iter()
                .find(|n| {
                    let n = n.trim();
                    !n.is_empty()
                        && !n.starts_with("USB Composite")
                        && !n.starts_with("USB Input")
                })
                .or_else(|| names.iter().find(|n| !n.trim().is_empty()))
                .cloned()
                .unwrap_or_else(|| "USB Device".to_string());

            out.insert(format!("usb:{key}"), (kind.to_string(), label));
        }

        for n in jacks {
            out.insert(format!("jack:{n}"), ("audio".to_string(), n));
        }
        out
    }

    // baris datang dari proses PowerShell bersama (winps.rs), tag "D:"
    fn watcher(app: AppHandle, rx: std::sync::mpsc::Receiver<String>) {
        let mut known: Option<HashMap<String, (String, String)>> = None;
        let mut buf: Vec<String> = Vec::new();

        for line in rx {
            let line = line.trim().to_string();
            if line == "---" {
                let now = parse(&buf);
                buf.clear();
                if let Some(k) = &known {
                    for (key, (kind, name)) in &now {
                        if !k.contains_key(key) {
                            println!("devices: colok {kind} {name}");
                            let _ = app.emit(
                                "device",
                                DeviceEvent { kind: kind.clone(), name: name.clone(), connected: true },
                            );
                        }
                    }
                    for (key, (kind, name)) in k {
                        if !now.contains_key(key) {
                            println!("devices: cabut {kind} {name}");
                            let _ = app.emit(
                                "device",
                                DeviceEvent { kind: kind.clone(), name: name.clone(), connected: false },
                            );
                        }
                    }
                }
                known = Some(now);
            } else if !line.is_empty() {
                buf.push(line);
            }
        }
    }

    // charger: perubahan status AC
    fn charger_watcher(app: AppHandle) {
        let ac = || {
            let mut s = SYSTEM_POWER_STATUS::default();
            unsafe { GetSystemPowerStatus(&mut s) }.ok().map(|_| s.ACLineStatus == 1)
        };
        let mut last = ac();
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            let now = ac();
            if now != last {
                if let (Some(was), Some(is)) = (last, now) {
                    if was != is {
                        let _ = app.emit(
                            "device",
                            DeviceEvent {
                                kind: "charger".into(),
                                name: "Charger".into(),
                                connected: is,
                            },
                        );
                    }
                }
                last = now;
            }
        }
    }

    pub fn start(app: AppHandle) {
        let rx = crate::winps::subscribe("D");
        let a = app.clone();
        std::thread::spawn(move || watcher(a, rx));
        std::thread::spawn(move || charger_watcher(app));
    }
}