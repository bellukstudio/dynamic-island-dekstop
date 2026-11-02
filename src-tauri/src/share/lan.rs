//! Lapisan Tauri untuk transfer PC <-> PC antar-Notch (inti protokol di peer.rs, penemuan di discovery.rs).
//!
//! - "Terlihat di jaringan" (config.enabled): terima koneksi + umumkan lewat mDNS. Tersimpan; aktif lagi
//!   saat Notch dibuka (start_background).
//! - Penawaran masuk selalu perlu persetujuan, kecuali dari perangkat tepercaya + opsi "terima otomatis".
//!   Penawaran yang menunggu memunculkan jendela Share.
//! - Enkripsi: TLS 1.3 + sertifikat per perangkat; kode verifikasi 6 digit sama di kedua layar.
use super::discovery::{self, Discovery, Peer};
use super::peer::{self, Callbacks, Identity, IncomingOffer, OnEvent, PeerEvent, Receiver};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

#[derive(Clone, Serialize, Deserialize)]
pub struct Trusted {
    fp: String,
    name: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    name: String,
    enabled: bool,
    auto_accept: bool,
    trusted: Vec<Trusted>,
}

struct Live {
    _receiver: Receiver,
    _discovery: Discovery,
}

struct Pending {
    tx: oneshot::Sender<bool>,
    from_fp: String,
    from_name: String,
    sas: String,
    names: Vec<String>,
    count: usize,
    total: u64,
}

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static CONFIG: Mutex<Option<Config>> = Mutex::new(None);
static IDENT: Mutex<Option<Identity>> = Mutex::new(None);
static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static PEERS: Mutex<Vec<Peer>> = Mutex::new(Vec::new());
static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "PC".into())
}

fn data_dir(app: &AppHandle) -> PathBuf {
    DATA_DIR
        .get_or_init(|| app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("notch")))
        .clone()
}

fn config_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("notch-lan.json")
}

fn cfg(app: &AppHandle) -> Config {
    let mut g = lock(&CONFIG);
    if g.is_none() {
        let loaded = std::fs::read_to_string(config_path(app)).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok());
        *g = Some(loaded.unwrap_or(Config { name: hostname(), enabled: false, auto_accept: false, trusted: vec![] }));
    }
    g.clone().unwrap()
}

fn update_cfg(app: &AppHandle, f: impl FnOnce(&mut Config)) -> Config {
    let mut c = cfg(app);
    f(&mut c);
    let _ = std::fs::create_dir_all(data_dir(app));
    if let Ok(s) = serde_json::to_string_pretty(&c) {
        let _ = std::fs::write(config_path(app), s);
    }
    *lock(&CONFIG) = Some(c.clone());
    c
}

fn identity(app: &AppHandle) -> Result<Identity, String> {
    let mut g = lock(&IDENT);
    if g.is_none() {
        *g = Some(peer::load_or_create_identity(&data_dir(app))?);
    }
    Ok(g.clone().unwrap())
}

fn notify(app: &AppHandle) {
    let _ = app.emit_to("share", "lan-changed", ());
}

// ---------------------------------------------------------------------------------------------
// Layanan latar belakang
// ---------------------------------------------------------------------------------------------
async fn start_live(app: &AppHandle) -> Result<(), String> {
    if lock(&LIVE).is_some() {
        return Ok(());
    }
    let id = identity(app)?;
    let (a1, a2, a3) = (app.clone(), app.clone(), app.clone());
    let on_offer: peer::OnOffer = Arc::new(move |o: IncomingOffer, tx: oneshot::Sender<bool>| {
        let c = cfg(&a1);
        if c.auto_accept && c.trusted.iter().any(|t| t.fp == o.from_fp) {
            let _ = tx.send(true);
            return;
        }
        lock(&PENDING).insert(
            o.id.clone(),
            Pending {
                tx,
                from_fp: o.from_fp.clone(),
                from_name: o.from_name.clone(),
                sas: o.sas.clone(),
                names: o.files.iter().take(6).map(|f| f.name.clone()).collect(),
                count: o.files.len(),
                total: o.total,
            },
        );
        super::show_window(&a1);
        notify(&a1);
    });
    let on_event: OnEvent = Arc::new(move |ev: PeerEvent| {
        let _ = a2.emit_to("share", "lan-event", &ev);
    });
    let cb = Arc::new(Callbacks { on_offer, on_event, inbox: Arc::new(move || super::inbox_dir(&a3)) });
    let receiver = peer::start_receiver(&id, cb).await?;
    let name = cfg(app).name;
    let app2 = app.clone();
    let on_peers: discovery::OnPeers = Arc::new(move |v: Vec<Peer>| {
        *lock(&PEERS) = v;
        notify(&app2);
    });
    let disc = discovery::start(&id.fp, &name, receiver.port, on_peers)?;
    *lock(&LIVE) = Some(Live { _receiver: receiver, _discovery: disc });
    Ok(())
}

fn stop_live() {
    let _ = lock(&LIVE).take();
    lock(&PEERS).clear();
    reject_all_pending();
}

/// Tolak semua penawaran yang menunggu (dipanggil saat jendela ditutup / layanan dimatikan).
pub fn reject_all_pending() {
    let all: Vec<Pending> = lock(&PENDING).drain().map(|(_, v)| v).collect();
    for p in all {
        let _ = p.tx.send(false);
    }
}

/// Dipanggil saat aplikasi mulai: bila "terlihat di jaringan" sebelumnya aktif, nyalakan lagi.
pub fn start_background(app: AppHandle) {
    if !cfg(&app).enabled {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_live(&app).await {
            eprintln!("lan: gagal memulai layanan: {e}");
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Command Tauri
// ---------------------------------------------------------------------------------------------
#[derive(Serialize)]
pub struct PeerView {
    fp: String,
    name: String,
    addr: String,
    trusted: bool,
}

#[derive(Serialize)]
pub struct PendingView {
    id: String,
    from_name: String,
    from_fp: String,
    sas: String,
    names: Vec<String>,
    count: usize,
    total: u64,
    trusted: bool,
}

#[derive(Serialize)]
pub struct LanState {
    enabled: bool,
    running: bool,
    name: String,
    fp: String,
    auto_accept: bool,
    peers: Vec<PeerView>,
    trusted: Vec<Trusted>,
    pending: Vec<PendingView>,
}

fn build_state(app: &AppHandle) -> LanState {
    let c = cfg(app);
    let fp = identity(app).map(|i| i.fp).unwrap_or_default();
    let is_trusted = |f: &str| c.trusted.iter().any(|t| t.fp == f);
    let peers = lock(&PEERS)
        .iter()
        .map(|p| PeerView {
            fp: p.fp.clone(),
            name: p.name.clone(),
            addr: p.addrs.first().map(|a| a.to_string()).unwrap_or_default(),
            trusted: is_trusted(&p.fp),
        })
        .collect();
    let pending = lock(&PENDING)
        .iter()
        .map(|(id, p)| PendingView {
            id: id.clone(),
            from_name: p.from_name.clone(),
            from_fp: p.from_fp.clone(),
            sas: p.sas.clone(),
            names: p.names.clone(),
            count: p.count,
            total: p.total,
            trusted: is_trusted(&p.from_fp),
        })
        .collect();
    LanState { enabled: c.enabled, running: lock(&LIVE).is_some(), name: c.name.clone(), fp, auto_accept: c.auto_accept, peers, trusted: c.trusted.clone(), pending }
}

#[tauri::command]
pub fn lan_state(app: AppHandle) -> LanState {
    build_state(&app)
}

#[tauri::command]
pub async fn lan_set_enabled(app: AppHandle, enabled: bool) -> Result<LanState, String> {
    update_cfg(&app, |c| c.enabled = enabled);
    if enabled {
        if let Err(e) = start_live(&app).await {
            update_cfg(&app, |c| c.enabled = false);
            return Err(e);
        }
    } else {
        stop_live();
    }
    Ok(build_state(&app))
}

#[tauri::command]
pub async fn lan_set_name(app: AppHandle, name: String) -> Result<LanState, String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    let name = if name.trim().is_empty() { hostname() } else { name.trim().to_string() };
    update_cfg(&app, |c| c.name = name);
    // nama ada di iklan mDNS: mulai ulang layanan agar nama baru terlihat
    if cfg(&app).enabled {
        stop_live();
        start_live(&app).await?;
    }
    Ok(build_state(&app))
}

#[tauri::command]
pub fn lan_set_auto_accept(app: AppHandle, value: bool) -> LanState {
    update_cfg(&app, |c| c.auto_accept = value);
    build_state(&app)
}

#[tauri::command]
pub fn lan_untrust(app: AppHandle, fp: String) -> LanState {
    update_cfg(&app, |c| c.trusted.retain(|t| t.fp != fp));
    build_state(&app)
}

/// Jawab penawaran masuk. `trust` = ingat perangkat pengirim sebagai tepercaya.
#[tauri::command]
pub fn lan_respond(app: AppHandle, id: String, accept: bool, trust: bool) -> LanState {
    if let Some(p) = lock(&PENDING).remove(&id) {
        if accept && trust && !cfg(&app).trusted.iter().any(|t| t.fp == p.from_fp) {
            update_cfg(&app, |c| c.trusted.push(Trusted { fp: p.from_fp.clone(), name: p.from_name.clone() }));
        }
        let _ = p.tx.send(accept);
    }
    build_state(&app)
}

#[derive(Serialize)]
pub struct SendStart {
    id: String,
    to: String,
    count: usize,
    total: u64,
}

/// Kirim file/folder ke perangkat Notch lain. Mengembalikan id transfer; kemajuan lewat event "lan-event".
#[tauri::command]
pub async fn lan_send(app: AppHandle, fp: String, paths: Vec<String>) -> Result<SendStart, String> {
    let peer = lock(&PEERS).iter().find(|p| p.fp == fp).cloned().ok_or("Perangkat tidak ditemukan lagi di jaringan")?;
    let id = identity(&app)?;
    let my_name = cfg(&app).name;
    let pbs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let files = tauri::async_runtime::spawn_blocking(move || peer::collect_files(&pbs)).await.map_err(|e| e.to_string())??;
    let total: u64 = files.iter().filter_map(|(p, _)| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let count = files.len();
    let tid = super::server::random_token()[..12].to_string();
    let addrs: Vec<SocketAddr> = peer.addrs.iter().map(|a| SocketAddr::new(*a, peer.port)).collect();
    let (app2, tid2, peer_fp) = (app.clone(), tid.clone(), peer.fp.clone());
    tauri::async_runtime::spawn(async move {
        let a_ev = app2.clone();
        let on_event: OnEvent = Arc::new(move |ev: PeerEvent| {
            let _ = a_ev.emit_to("share", "lan-event", &ev);
        });
        let a_sas = app2.clone();
        let t_sas = tid2.clone();
        let on_sas = move |s: String| {
            let _ = a_sas.emit_to("share", "lan-sas", serde_json::json!({ "id": t_sas, "sas": s }));
        };
        let r = peer::send(&id, &my_name, &addrs, &peer_fp, files, &tid2, &on_event, &on_sas).await;
        match r {
            Ok(()) => on_event(PeerEvent::Finished { id: tid2, dir: "out".into(), count, total, path: None }),
            Err(error) => on_event(PeerEvent::Failed { id: tid2, dir: "out".into(), error }),
        }
    });
    Ok(SendStart { id: tid, to: peer.name, count, total })
}

#[tauri::command]
pub async fn lan_pick_folder() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().set_title("Pilih folder untuk dikirim").pick_folder().map(|p| p.to_string_lossy().into_owned()))
        .await
        .unwrap_or(None)
}
