//! Penemuan Notch lain di jaringan lokal lewat mDNS (`_notchshare._tcp`).
//! Iklan memuat: sidik jari sertifikat (fp), nama perangkat (name), port. Tanpa dependensi Tauri.
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

const TY: &str = "_notchshare._tcp.local.";

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct Peer {
    /// sidik jari sertifikat (juga jadi id)
    pub fp: String,
    pub name: String,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

pub type OnPeers = Arc<dyn Fn(Vec<Peer>) + Send + Sync>;

pub struct Discovery {
    daemon: ServiceDaemon,
    instance_fullname: String,
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.instance_fullname);
        let _ = self.daemon.shutdown();
    }
}

fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(60).collect()
}

/// Umumkan perangkat ini dan mulai memantau perangkat lain. `on_peers` dipanggil tiap daftar berubah.
pub fn start(my_fp: &str, my_name: &str, port: u16, on_peers: OnPeers) -> Result<Discovery, String> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS gagal dimulai: {e}"))?;
    let id = &my_fp[..16.min(my_fp.len())];
    let host = format!("notch-{id}.local.");
    let props = [("fp", my_fp), ("name", my_name), ("v", "1")];
    let info = ServiceInfo::new(TY, id, &host, "", port, &props[..]).map_err(|e| e.to_string())?.enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(|e| format!("mDNS register gagal: {e}"))?;
    let rx = daemon.browse(TY).map_err(|e| format!("mDNS browse gagal: {e}"))?;

    let me = my_fp.to_string();
    let peers: Arc<Mutex<HashMap<String, Peer>>> = Arc::new(Mutex::new(HashMap::new()));
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            let changed = match ev {
                ServiceEvent::ServiceResolved(info) => {
                    let fp = info.get_property_val_str("fp").unwrap_or("").to_string();
                    if fp.len() != 64 || fp == me {
                        false
                    } else {
                        let mut addrs: Vec<IpAddr> = info.get_addresses().iter().copied().filter(|a| match a { IpAddr::V4(v) => !v.is_unspecified(), IpAddr::V6(v) => !v.is_loopback() && (v.segments()[0] & 0xffc0) != 0xfe80 }).collect();
                        addrs.sort_by_key(|a| (a.is_ipv6(), a.to_string()));
                        let p = Peer { fp: fp.clone(), name: clean_name(info.get_property_val_str("name").unwrap_or("Perangkat")), addrs, port: info.get_port() };
                        let mut g = peers.lock().unwrap_or_else(|e| e.into_inner());
                        let ch = g.get(&fp) != Some(&p);
                        g.insert(fp, p);
                        ch
                    }
                }
                ServiceEvent::ServiceRemoved(_, full) => {
                    // fullname = "<id>._notchshare._tcp.local."; id = 16 karakter awal fp
                    let id = full.split('.').next().unwrap_or("").to_string();
                    let mut g = peers.lock().unwrap_or_else(|e| e.into_inner());
                    let before = g.len();
                    g.retain(|fp, _| !fp.starts_with(&id) || id.is_empty());
                    g.len() != before
                }
                ServiceEvent::SearchStopped(_) => break,
                _ => false,
            };
            if changed {
                let mut v: Vec<Peer> = peers.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
                v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                on_peers(v);
            }
        }
    });
    Ok(Discovery { daemon, instance_fullname: fullname })
}
