use serde::Serialize;
use std::sync::Mutex;
use tauri::AppHandle;

// Dikirim ke frontend (event "bluetooth") saat perangkat terhubung / terputus.
#[derive(Serialize, Clone)]
struct BtEvent {
    name: String,
    battery: Option<u8>,
    connected: bool,
}

// Perangkat yang sedang terhubung SAAT INI (diperbarui oleh pemantau tiap ±3 detik).
// Dipakai frontend untuk menampilkan keadaan nyata, termasuk perangkat yang sudah
// terhubung sebelum aplikasi dibuka dan keadaan "tidak ada perangkat".
#[derive(Serialize, Clone)]
pub struct BtDev {
    name: String,
    battery: Option<u8>,
}

static CURRENT: Mutex<Vec<BtDev>> = Mutex::new(Vec::new());

fn set_current(v: Vec<BtDev>) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

#[tauri::command]
pub fn bt_list() -> Vec<BtDev> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn start(app: AppHandle) {
    imp::start(app);
}

// ======================= Linux (bluetoothctl) =======================
#[cfg(target_os = "linux")]
mod imp {
    use super::BtEvent;
    use std::collections::HashMap;
    use std::process::Command;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Emitter};

    // None = perintah gagal dijalankan (bukan sekadar "tidak ada perangkat")
    fn ctl(args: &[&str]) -> Option<String> {
        let o = Command::new("bluetoothctl").args(args).output().ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).to_string())
    }

    // baris: "Battery Percentage: 0x46 (70)"
    fn parse_battery(info: &str) -> Option<u8> {
        let line = info.lines().find(|l| l.contains("Battery Percentage"))?;
        let start = line.find('(')? + 1;
        let end = line.find(')')?;
        line[start..end].trim().parse().ok()
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            let mut known: HashMap<String, String> = HashMap::new(); // mac -> nama
            let mut first = true;
            // `bluetoothctl info` = satu proses per perangkat. Baterai jarang berubah: simpan sementara.
            let mut batt: HashMap<String, (Option<u8>, Instant)> = HashMap::new();

            loop {
                // gagal membaca -> lewati putaran ini, jangan dianggap semua perangkat putus
                let Some(list) = ctl(&["devices", "Connected"]) else {
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                };

                let mut now: HashMap<String, String> = HashMap::new();
                let mut cur: Vec<super::BtDev> = Vec::new();
                // baris: "Device AA:BB:CC:DD:EE:FF Nama Perangkat"
                for line in list.lines() {
                    let mut parts = line.splitn(3, ' ');
                    if parts.next() != Some("Device") { continue; }
                    let (Some(mac), Some(name)) = (parts.next(), parts.next()) else { continue };
                    let battery = match batt.get(mac) {
                        Some((b, at))
                            if at.elapsed() < Duration::from_secs(if b.is_some() { 30 } else { 6 }) =>
                        {
                            *b
                        }
                        _ => {
                            let b = ctl(&["info", mac]).and_then(|s| parse_battery(&s));
                            batt.insert(mac.to_string(), (b, Instant::now()));
                            b
                        }
                    };
                    now.insert(mac.to_string(), name.to_string());
                    cur.push(super::BtDev { name: name.to_string(), battery });

                    // baru terhubung (bukan yang sudah terhubung saat app dibuka)
                    if !first && !known.contains_key(mac) {
                        let _ = app.emit(
                            "bluetooth",
                            BtEvent { name: name.to_string(), battery, connected: true },
                        );
                    }
                }

                if !first {
                    for (mac, name) in &known {
                        if !now.contains_key(mac) {
                            let _ = app.emit(
                                "bluetooth",
                                BtEvent { name: name.clone(), battery: None, connected: false },
                            );
                        }
                    }
                }
                batt.retain(|k, _| now.contains_key(k));
                super::set_current(cur);
                known = now;
                first = false;
                std::thread::sleep(Duration::from_secs(3));
            }
        });
    }
}

// ======================= Windows (PowerShell / PnP) =======================
#[cfg(target_os = "windows")]
mod imp {
    use super::BtEvent;
    use std::collections::{HashMap, HashSet};
    use tauri::{AppHandle, Emitter};

    type Snapshot = HashMap<String, (String, Option<u8>)>; // kunci perangkat -> (nama, baterai)

    fn parse(lines: &[String]) -> Snapshot {
        let mut out = Snapshot::new();
        for l in lines {
            let Some(rest) = l.strip_prefix("BT|") else { continue };
            let mut p = rest.splitn(3, '|');
            let (Some(id), Some(batt), Some(name)) = (p.next(), p.next(), p.next()) else { continue };
            // BTHENUM\DEV_AABBCCDDEEFF\...  -> kunci = bagian kedua (alamat perangkat)
            let Some(key) = id.split('\\').nth(1) else { continue };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let battery = batt.trim().parse::<u8>().ok().filter(|b| *b <= 100);
            out.entry(key.to_string()).or_insert((name.to_string(), battery));
        }
        out
    }

    // baris datang dari PowerShell bersama (winps.rs), tag "T:":
    //   BT|<instanceId>|<baterai atau kosong>|<nama>, diakhiri "---"
    fn watch(app: &AppHandle, rx: std::sync::mpsc::Receiver<String>) {
        let mut announced: HashMap<String, String> = HashMap::new(); // sudah diketahui: kunci -> nama
        let mut waiting: HashSet<String> = HashSet::new(); // baru muncul, baterai belum terbaca
        let mut first = true;
        let mut buf: Vec<String> = Vec::new();

        for line in rx {
            let line = line.trim().to_string();
            if line != "---" {
                if !line.is_empty() {
                    buf.push(line);
                }
                continue;
            }
            let now = parse(&buf);
            buf.clear();
            super::set_current(
                now.values()
                    .map(|(n, b)| super::BtDev { name: n.clone(), battery: *b })
                    .collect(),
            );

            // terhubung
            for (key, (name, battery)) in &now {
                if announced.contains_key(key) {
                    continue;
                }
                if first {
                    // yang sudah terhubung saat app dibuka: catat saja, tanpa notifikasi
                    announced.insert(key.clone(), name.clone());
                } else if battery.is_none() && !waiting.contains(key) {
                    // baterai biasanya baru terbaca beberapa detik setelah terhubung: tunda satu putaran
                    waiting.insert(key.clone());
                } else {
                    waiting.remove(key);
                    announced.insert(key.clone(), name.clone());
                    println!("bluetooth: terhubung {name} {battery:?}");
                    let _ = app.emit(
                        "bluetooth",
                        BtEvent { name: name.clone(), battery: *battery, connected: true },
                    );
                }
            }
            waiting.retain(|k| now.contains_key(k));

            // terputus
            let gone: Vec<(String, String)> = announced
                .iter()
                .filter(|(k, _)| !now.contains_key(*k))
                .map(|(k, n)| (k.clone(), n.clone()))
                .collect();
            for (key, name) in gone {
                announced.remove(&key);
                println!("bluetooth: terputus {name}");
                let _ = app.emit("bluetooth", BtEvent { name, battery: None, connected: false });
            }
            first = false;
        }
    }

    pub fn start(app: AppHandle) {
        let rx = crate::winps::subscribe("T");
        std::thread::spawn(move || watch(&app, rx));
    }
}

// ======================= platform lain: belum didukung =======================
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    use tauri::AppHandle;
    pub fn start(_app: AppHandle) {}
}