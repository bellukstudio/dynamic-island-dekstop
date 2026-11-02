//! Mirror layar Android (ala scrcpy) lewat adb, ditampilkan & dikontrol dari jendela "mirror".
//!
//! Alur: push scrcpy-server.jar ke HP -> jalankan lewat `adb shell app_process` -> `adb forward`
//! ke socket lokal di perangkat -> dua koneksi TCP (video lalu control -- scrcpy membedakan
//! keduanya dari URUTAN soket yang diterima, bukan dari isi datanya) -> dijembatani ke satu
//! WebSocket lokal yang dipakai jendela Android (src/main-modules/android.ts).
//!
//! PENTING: modul ini TIDAK mengerti isi protokol video/control sama sekali -- cuma menyalurkan
//! byte apa adanya kedua arah. Parsing H.264 (WebCodecs) dan serialisasi pesan kontrol (touch,
//! keyboard, dst) ada di sisi TypeScript, supaya lebih gampang diperbaiki kalau ada yang meleset.
//!
//! SERVER_VERSION harus PERSIS sama dengan versi file scrcpy-server.jar yang dibundel di
//! resources/ -- scrcpy menolak jalan kalau client & server tidak satu versi.
use super::adb;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::io::Read as _;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

const DEVICE_JAR_PATH: &str = "/data/local/tmp/scrcpy-server.jar";
const SERVER_CLASS: &str = "com.genymobile.scrcpy.Server";
/// HARUS persis sama dengan versi scrcpy-server.jar di src-tauri/resources/.
const SERVER_VERSION: &str = "5.0.1";

fn hide(cmd: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Cari jar di satu folder: nama persis `scrcpy-server.jar`, atau berversi seperti
/// `scrcpy-server-v5.0.1.jar` (yang memuat SERVER_VERSION didahulukan).
fn jar_in(dir: &Path) -> Option<PathBuf> {
    let exact = dir.join("scrcpy-server.jar");
    if exact.is_file() {
        return Some(exact);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    let n = n.to_ascii_lowercase();
                    n.starts_with("scrcpy-server") && n.ends_with(".jar")
                })
        })
        .collect();
    found.sort();
    let pos = found
        .iter()
        .position(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(SERVER_VERSION)));
    match pos {
        Some(i) => Some(found.swap_remove(i)),
        None => found.into_iter().next(),
    }
}

fn find_jar(resource_dir: Option<&Path>) -> Option<PathBuf> {
    // SCRCPY_SERVER_JAR: path file langsung
    if let Some(p) = std::env::var_os("SCRCPY_SERVER_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(rd) = resource_dir {
        dirs.push(rd.to_path_buf());
        dirs.push(rd.join("resources"));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(dir.clone());
        dirs.push(dir.join("resources"));
    }
    // mode dev (cargo run): langsung dari src-tauri/resources di source tree
    if let Some(m) = option_env!("CARGO_MANIFEST_DIR") {
        dirs.push(PathBuf::from(m).join("resources"));
    }
    dirs.into_iter().find_map(|d| jar_in(&d))
}

fn free_port() -> Result<u16, String> {
    let l = StdTcpListener::bind("127.0.0.1:0").map_err(|e| format!("Tidak bisa cari port bebas: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// scid: angka acak, ditulis heksadesimal 8 digit. Server mem-parse-nya sebagai i32 (Java
/// Integer.parseInt(s, 16)) jadi bit tertinggi harus 0.
fn random_scid() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    let v = u32::from_be_bytes(b) & 0x7FFF_FFFF;
    format!("{v:08x}")
}

/// Jalankan satu perintah adb singkat sampai selesai (BUKAN proses server yang hidup terus).
fn run_adb(adb_path: &Path, args: &[&str], timeout: Duration) -> Result<(bool, String), String> {
    let mut cmd = Command::new(adb_path);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan adb: {e}"))?;
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut b = String::new();
        let _ = so.read_to_string(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = String::new();
        let _ = se.read_to_string(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("adb tidak merespons (timeout)".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok((status.success(), format!("{}{}", t1.join().unwrap_or_default(), t2.join().unwrap_or_default())))
}

/// Sambung ke socket lokal hasil `adb forward`, coba ulang. Dipakai untuk socket control
/// (socket video memakai `connect_video` yang juga menunggu byte dummy).
async fn connect_retry(port: u16, timeout: Duration) -> Result<TcpStream, String> {
    let start = Instant::now();
    loop {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(s) => return Ok(s),
            Err(e) => {
                if start.elapsed() > timeout {
                    return Err(format!("Tidak bisa tersambung ke scrcpy-server: {e}"));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

type Log = Arc<Mutex<String>>;

/// Baca stdout/stderr server terus-menerus: dicetak ke terminal + disimpan untuk pesan error.
fn pump_log<R: std::io::Read + Send + 'static>(mut r: R, log: Log, tag: &'static str) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).into_owned();
                    eprint!("mirror: scrcpy-server[{tag}]: {s}");
                    let mut g = log.lock().unwrap_or_else(|e| e.into_inner());
                    if g.len() < 16 * 1024 {
                        g.push_str(&s);
                    }
                }
            }
        }
    });
}

fn server_log(log: &Log) -> String {
    let g = log.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.trim();
    if t.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = t.lines().collect();
    let tail = &lines[lines.len().saturating_sub(6)..];
    format!("\n\nLog server:\n{}", tail.join("\n"))
}

/// Gagal start: beri waktu log terbaca, bunuh server, hapus forward, kembalikan pesan + log.
async fn fail(adb_path: &Path, serial: &str, port: u16, child: &mut Child, log: &Log, msg: String) -> String {
    tokio::time::sleep(Duration::from_millis(300)).await;
    let _ = child.kill();
    let _ = child.wait();
    let _ = run_adb(adb_path, &["-s", serial, "forward", "--remove", &format!("tcp:{port}")], Duration::from_secs(5));
    format!("{msg}{}", server_log(log))
}

/// Sambung ke socket video + baca byte dummy, DIULANG sampai server di HP benar-benar siap.
/// Sebelum siap, adb menerima koneksi lokal lalu menutupnya lagi (socket di HP belum ada).
async fn connect_video(port: u16, timeout: Duration, child: &mut Child) -> Result<TcpStream, String> {
    let start = Instant::now();
    loop {
        if let Ok(Some(st)) = child.try_wait() {
            return Err(format!("scrcpy-server berhenti di HP ({st})"));
        }
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)).await {
            let mut d = [0u8; 1];
            match tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut d)).await {
                Ok(Ok(_)) => return Ok(s),
                Ok(Err(_)) => {} // ditutup adb karena server belum siap: ulangi
                Err(_) => return Err("Server tidak mengirim byte dummy (timeout)".into()),
            }
        }
        if start.elapsed() > timeout {
            return Err("Tidak bisa tersambung ke scrcpy-server di HP (timeout)".into());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[derive(Serialize)]
pub struct MirrorInfo {
    pub ws_port: u16,
    pub width: u32,
    pub height: u32,
}

struct Session {
    shutdown: Option<oneshot::Sender<()>>,
    server_child: Option<Child>,
    adb_path: PathBuf,
    serial: String,
    forward_port: u16,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(mut c) = self.server_child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = run_adb(
            &self.adb_path,
            &["-s", &self.serial, "forward", "--remove", &format!("tcp:{}", self.forward_port)],
            Duration::from_secs(5),
        );
    }
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn stop() {
    // Ambil dulu dari mutex lalu drop di luar kunci, supaya adb yang blocking tidak menahan mutex.
    let old = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(old); // Drop membereskan proses + forward
}

/// Ukuran layar fisik perangkat lewat `wm size` (dipakai untuk validasi koordinat sentuh --
/// scrcpy menolak perintah touch kalau videoWidth/videoHeight yang dikirim tidak cocok).
fn screen_size(adb_path: &Path, serial: &str) -> Result<(u32, u32), String> {
    let (_, out) = run_adb(adb_path, &["-s", serial, "shell", "wm", "size"], Duration::from_secs(8))?;
    let line = out
        .lines()
        .find(|l| l.contains("Override size"))
        .or_else(|| out.lines().find(|l| l.contains("Physical size")))
        .ok_or("Tidak bisa membaca ukuran layar (wm size)")?;
    let dims = line.rsplit(':').next().unwrap_or("").trim();
    let (w, h) = dims.split_once('x').ok_or("Format ukuran layar tidak dikenali")?;
    Ok((
        w.trim().parse().map_err(|_| "Ukuran layar tidak valid")?,
        h.trim().parse().map_err(|_| "Ukuran layar tidak valid")?,
    ))
}

pub async fn start(resource_dir: Option<PathBuf>, serial: String) -> Result<MirrorInfo, String> {
    // satu sesi mirror dalam satu waktu (stop() memanggil adb yang blocking -> thread terpisah)
    let _ = tauri::async_runtime::spawn_blocking(stop).await;

    let adb_path = adb::find(resource_dir.as_deref()).ok_or("adb tidak ditemukan")?;
    let jar = find_jar(resource_dir.as_deref())
        .ok_or("scrcpy-server.jar tidak ditemukan di resources/ -- taruh di src-tauri/resources/scrcpy-server.jar")?;

    let (w, h) = screen_size(&adb_path, &serial)?;

    // push jar ke HP (adb push menimpa kalau sudah ada -- aman dipanggil berulang)
    let jar_str = jar.to_str().ok_or("Path scrcpy-server.jar tidak valid")?;
    let (ok, msg) = run_adb(&adb_path, &["-s", &serial, "push", jar_str, DEVICE_JAR_PATH], Duration::from_secs(30))?;
    if !ok {
        return Err(format!("Gagal mengirim scrcpy-server.jar ke HP: {msg}"));
    }

    let scid = random_scid();
    let port = free_port()?;

    // Jalankan server di HP -- proses panjang (nyala selama mirror aktif), JANGAN ditunggu selesai.
    let classpath = format!("CLASSPATH={DEVICE_JAR_PATH}");
    let mut server_cmd = Command::new(&adb_path);
    server_cmd
        .args(["-s", &serial, "shell", &classpath, "app_process", "/", SERVER_CLASS, SERVER_VERSION])
        .arg(format!("scid={scid}"))
        .args([
            "log_level=info",
            "audio=false",
            "video=true",
            "control=true",
            // Tanpa metadata nama perangkat/codec. Frame meta TETAP aktif: tiap paket video diawali
            // header 12 byte (PTS+flag, ukuran) yang dipakai sisi TS untuk memotong frame.
            "send_device_meta=false",
            "send_frame_meta=true",
            "send_codec_meta=false",
            // Dummy byte di socket PERTAMA (video) = sinyal bahwa adb forward sungguh sudah
            // tersambung ke perangkat, bukan cuma port lokal yang kebuka.
            "send_dummy_byte=true",
            "tunnel_forward=true",
            "cleanup=true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide(&mut server_cmd);
    let mut server_child = server_cmd.spawn().map_err(|e| format!("Gagal menjalankan scrcpy-server: {e}"))?;

    // stdout + stderr dibaca terus (kalau pipe penuh, proses di HP bisa macet) dan disimpan untuk error.
    let log: Log = Arc::new(Mutex::new(String::new()));
    if let Some(so) = server_child.stdout.take() {
        pump_log(so, log.clone(), "out");
    }
    if let Some(se) = server_child.stderr.take() {
        pump_log(se, log.clone(), "err");
    }

    // adb forward: TCP lokal <port> <-> socket `localabstract:scrcpy_<scid>` di perangkat.
    let addr = format!("localabstract:scrcpy_{scid}");
    match run_adb(&adb_path, &["-s", &serial, "forward", &format!("tcp:{port}"), &addr], Duration::from_secs(8)) {
        Ok((true, _)) => {}
        Ok((false, msg)) => {
            return Err(fail(&adb_path, &serial, port, &mut server_child, &log, format!("adb forward gagal: {msg}")).await);
        }
        Err(e) => {
            return Err(fail(&adb_path, &serial, port, &mut server_child, &log, e).await);
        }
    }

    // Video (socket PERTAMA, + byte dummy) lalu control (socket KEDUA): scrcpy menentukan peran tiap
    // socket dari urutan koneksi diterima, bukan dari isi datanya.
    let video = match connect_video(port, Duration::from_secs(15), &mut server_child).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("{e}. Cek versi scrcpy-server.jar cocok dengan SERVER_VERSION ({SERVER_VERSION}) di mirror.rs.");
            return Err(fail(&adb_path, &serial, port, &mut server_child, &log, msg).await);
        }
    };
    let control = match connect_retry(port, Duration::from_secs(8)).await {
        Ok(s) => s,
        Err(e) => {
            return Err(fail(&adb_path, &serial, port, &mut server_child, &log, e).await);
        }
    };

    // --- jembatan WebSocket lokal, dipakai jendela Android ---
    let ws_port = match free_port() {
        Ok(p) => p,
        Err(e) => return Err(fail(&adb_path, &serial, port, &mut server_child, &log, e).await),
    };
    let ws_listener = match TcpListener::bind(("127.0.0.1", ws_port)).await {
        Ok(l) => l,
        Err(e) => {
            let msg = format!("Tidak bisa buka WebSocket lokal: {e}");
            return Err(fail(&adb_path, &serial, port, &mut server_child, &log, msg).await);
        }
    };
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    tauri::async_runtime::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => {}
            accepted = ws_listener.accept() => {
                if let Ok((stream, _)) = accepted {
                    if let Ok(ws) = tokio_tungstenite::accept_async(stream).await {
                        // bridge ikut dibatalkan kalau sesi dihentikan
                        tokio::select! {
                            _ = &mut shutdown_rx => {}
                            _ = bridge(ws, video, control) => {}
                        }
                    }
                }
            }
        }
    });

    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        shutdown: Some(shutdown_tx),
        server_child: Some(server_child),
        adb_path,
        serial,
        forward_port: port,
    });

    Ok(MirrorInfo { ws_port, width: w, height: h })
}

/// Pompa byte dua arah: video TCP -> WS (binary). WS (binary, pesan kontrol dari TS) -> control
/// TCP. Socket control juga terus dibaca & dibuang di task sendiri supaya tidak menyumbat
/// (pesan server->client seperti sinkronisasi clipboard diabaikan dulu di versi ini).
async fn bridge(ws: tokio_tungstenite::WebSocketStream<TcpStream>, mut video: TcpStream, control: TcpStream) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (mut control_read, mut control_write) = control.into_split();

    tauri::async_runtime::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match control_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    let video_to_ws = async {
        let mut buf = [0u8; 64 * 1024];
        loop {
            match video.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // tokio-tungstenite 0.26+: Message::Binary membawa Bytes
                    if ws_tx.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    };

    let ws_to_control = async {
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let Message::Binary(data) = msg {
                if control_write.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    };

    tokio::select! {
        _ = video_to_ws => {}
        _ = ws_to_control => {}
    }
}