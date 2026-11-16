//! Transfer file PC <-> PC antar-Notch: TLS 1.3 dengan sertifikat unik per perangkat (mutual TLS),
//! verifikasi lewat sidik jari + kode 6 digit (SAS) yang tampil di kedua layar, dan konfirmasi di penerima.
//! Inti ini tidak bergantung pada Tauri (diuji sendiri); lapisan Tauri ada di lan.rs.
//!
//! Protokol di atas TLS (satu koneksi = satu penawaran):
//!   pengirim -> {"t":"offer","name":"<nama PC>","files":[{"name":"a/b.txt","size":123}, ...]}\n
//!   penerima -> {"t":"accept"}\n   atau   {"t":"reject","reason":"..."}\n
//!   pengirim -> byte mentah semua file berurutan (panjang = size)
//!   penerima -> {"t":"done","ok":true}\n
use super::server::{random_token, sanitize};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

// ---------------------------------------------------------------------------------------------
// Identitas perangkat
// ---------------------------------------------------------------------------------------------
#[derive(Clone)]
pub struct Identity {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    /// SHA-256 sertifikat, heksadesimal huruf kecil (64 karakter)
    pub fp: String,
}

pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// Muat sertifikat+kunci dari `dir`, atau buat baru (berlaku "selamanya") dan simpan.
pub fn load_or_create_identity(dir: &Path) -> Result<Identity, String> {
    let (cp, kp) = (dir.join("identity-cert.der"), dir.join("identity-key.der"));
    if let (Ok(c), Ok(k)) = (std::fs::read(&cp), std::fs::read(&kp)) {
        if !c.is_empty() && !k.is_empty() {
            return Ok(Identity { fp: fingerprint(&c), cert: c, key: k });
        }
    }
    let ck = rcgen::generate_simple_self_signed(vec!["notch.local".to_string()]).map_err(e)?;
    let (cert, key) = (ck.cert.der().to_vec(), ck.key_pair.serialize_der());
    std::fs::create_dir_all(dir).map_err(e)?;
    std::fs::write(&cp, &cert).map_err(e)?;
    std::fs::write(&kp, &key).map_err(e)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&kp, std::fs::Permissions::from_mode(0o600));
    }
    Ok(Identity { fp: fingerprint(&cert), cert, key })
}

/// Kode verifikasi 6 digit dari dua sidik jari; sama di kedua sisi, berbeda bila ada penyadap.
pub fn sas(fp_a: &str, fp_b: &str) -> String {
    let (lo, hi) = if fp_a <= fp_b { (fp_a, fp_b) } else { (fp_b, fp_a) };
    let h = Sha256::digest(format!("notch-sas|{lo}|{hi}").as_bytes());
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

// ---------------------------------------------------------------------------------------------
// TLS
// ---------------------------------------------------------------------------------------------
fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

#[derive(Debug)]
struct PinVerifier {
    expected: String,
    algs: WebPkiSupportedAlgorithms,
}
impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(&self, end: &CertificateDer<'_>, _i: &[CertificateDer<'_>], _n: &ServerName<'_>, _o: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end.as_ref()) == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("sidik jari perangkat tidak cocok".into()))
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.algs)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Server menerima sertifikat klien apa pun (kunci tetap dibuktikan lewat tanda tangan handshake);
/// siapa dia ditentukan dari sidik jarinya, lalu diputuskan oleh pengguna / daftar tepercaya.
#[derive(Debug)]
struct AnyClient {
    algs: WebPkiSupportedAlgorithms,
}
impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(&self, _e: &CertificateDer<'_>, _i: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.algs)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

fn key_of(id: &Identity) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key.clone()))
}

fn server_config(id: &Identity) -> Result<rustls::ServerConfig, String> {
    let p = provider();
    rustls::ServerConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(e)?
        .with_client_cert_verifier(Arc::new(AnyClient { algs: p.signature_verification_algorithms }))
        .with_single_cert(vec![CertificateDer::from(id.cert.clone())], key_of(id))
        .map_err(e)
}

fn client_config(id: &Identity, expected_fp: &str) -> Result<rustls::ClientConfig, String> {
    let p = provider();
    rustls::ClientConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(e)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier { expected: expected_fp.to_string(), algs: p.signature_verification_algorithms }))
        .with_client_auth_cert(vec![CertificateDer::from(id.cert.clone())], key_of(id))
        .map_err(e)
}

// ---------------------------------------------------------------------------------------------
// Tipe publik
// ---------------------------------------------------------------------------------------------
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FileMeta {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct IncomingOffer {
    pub id: String,
    pub from_fp: String,
    pub from_name: String,
    pub sas: String,
    pub files: Vec<FileMeta>,
    pub total: u64,
}

#[derive(Clone, Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PeerEvent {
    Progress { id: String, dir: String, name: String, done: u64, total: u64 },
    Finished { id: String, dir: String, count: usize, total: u64, path: Option<String> },
    Failed { id: String, dir: String, error: String },
}

pub type OnEvent = Arc<dyn Fn(PeerEvent) + Send + Sync>;
/// Dipanggil untuk tiap penawaran masuk; host harus menjawab lewat `Sender` (true = terima).
pub type OnOffer = Arc<dyn Fn(IncomingOffer, oneshot::Sender<bool>) + Send + Sync>;

pub struct Callbacks {
    pub on_offer: OnOffer,
    pub on_event: OnEvent,
    pub inbox: Arc<dyn Fn() -> PathBuf + Send + Sync>,
}

pub struct Receiver {
    pub port: u16,
    shutdown: Option<oneshot::Sender<()>>,
}
impl Drop for Receiver {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Offer {
    t: String,
    name: String,
    files: Vec<FileMeta>,
}
#[derive(Serialize, Deserialize, Default)]
struct Reply {
    t: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    ok: bool,
}

const MAX_FILES: usize = 20_000;
const CHUNK: usize = 256 * 1024;

async fn read_line<R: AsyncRead + Unpin>(rd: &mut BufReader<R>, max: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    let n = (&mut *rd).take(max as u64).read_until(b'\n', &mut buf).await.map_err(e)?;
    if n == 0 || !buf.ends_with(b"\n") {
        return Err("Koneksi terputus / pesan tidak valid".into());
    }
    String::from_utf8(buf).map_err(e)
}

async fn write_json<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, v: &T) -> Result<(), String> {
    let mut s = serde_json::to_string(v).map_err(e)?;
    s.push('\n');
    w.write_all(s.as_bytes()).await.map_err(e)?;
    w.flush().await.map_err(e)
}

struct Throttle {
    last: Instant,
}
impl Throttle {
    fn new() -> Self {
        Throttle { last: Instant::now() - Duration::from_secs(1) }
    }
    fn ready(&mut self) -> bool {
        if self.last.elapsed() >= Duration::from_millis(120) {
            self.last = Instant::now();
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Penerima
// ---------------------------------------------------------------------------------------------
fn unique_with(dir: &Path, name: &str, taken: &HashSet<PathBuf>) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let ext = p.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default();
    let mut c = dir.join(name);
    let mut n = 2;
    while c.exists() || taken.contains(&c) || PathBuf::from(format!("{}.part", c.display())).exists() {
        c = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    c
}

/// Tentukan path tujuan tiap file (nama dibersihkan, tidak menimpa, folder dipertahankan).
fn plan_dest(inbox: &Path, files: &[FileMeta]) -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut tops: HashMap<String, PathBuf> = HashMap::new();
    let mut out = Vec::new();
    let mut single: Option<PathBuf> = None;
    for f in files {
        let comps: Vec<&str> = f.name.split(['/', '\\']).filter(|c| !c.is_empty() && *c != "." && *c != "..").collect();
        let comps: Vec<String> = comps.iter().map(|c| sanitize(c)).collect();
        if comps.is_empty() {
            out.push(unique_with(inbox, "file", &taken));
        } else if comps.len() == 1 {
            let p = unique_with(inbox, &comps[0], &taken);
            taken.insert(p.clone());
            single = Some(p.clone());
            out.push(p);
            continue;
        } else {
            let top = tops
                .entry(comps[0].clone())
                .or_insert_with(|| {
                    let t = unique_with(inbox, &comps[0], &taken);
                    taken.insert(t.clone());
                    t
                })
                .clone();
            let mut p = top;
            for c in &comps[1..] {
                p = p.join(c);
            }
            out.push(p);
            continue;
        }
        if let Some(l) = out.last() {
            taken.insert(l.clone());
        }
    }
    let shown = if tops.len() == 1 && files.iter().all(|f| f.name.contains('/')) { tops.values().next().cloned() } else if files.len() == 1 { single } else { None };
    (out, shown)
}

async fn handle_conn(stream: TcpStream, acceptor: TlsAcceptor, my_fp: String, cb: Arc<Callbacks>) -> Result<(), String> {
    let tls = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await.map_err(|_| "TLS timeout".to_string())?.map_err(e)?;
    let peer_fp = {
        let (_, conn) = tls.get_ref();
        let certs = conn.peer_certificates().ok_or("Klien tanpa sertifikat")?;
        fingerprint(certs.first().ok_or("Klien tanpa sertifikat")?.as_ref())
    };
    let (rd, mut wr) = tokio::io::split(tls);
    let mut rd = BufReader::with_capacity(CHUNK, rd);
    let line = tokio::time::timeout(Duration::from_secs(15), read_line(&mut rd, 8 * 1024 * 1024)).await.map_err(|_| "Penawaran tidak diterima tepat waktu".to_string())??;
    let offer: Offer = serde_json::from_str(&line).map_err(|x| format!("Penawaran tidak valid: {x}"))?;
    if offer.t != "offer" || offer.files.is_empty() || offer.files.len() > MAX_FILES {
        return Err("Penawaran tidak valid".into());
    }
    let total: u64 = offer.files.iter().map(|f| f.size).fold(0u64, |a, b| a.saturating_add(b));
    let id = random_token()[..12].to_string();
    let name: String = offer.name.chars().filter(|c| !c.is_control()).take(60).collect();
    let (tx, rx) = oneshot::channel::<bool>();
    (cb.on_offer)(IncomingOffer { id: id.clone(), from_fp: peer_fp.clone(), from_name: name, sas: sas(&my_fp, &peer_fp), files: offer.files.clone(), total }, tx);
    let accepted = tokio::time::timeout(Duration::from_secs(120), rx).await.ok().and_then(|r| r.ok()).unwrap_or(false);
    if !accepted {
        let _ = write_json(&mut wr, &Reply { t: "reject".into(), reason: "Ditolak oleh penerima".into(), ok: false }).await;
        return Ok(());
    }
    let inbox = (cb.inbox)();
    tokio::fs::create_dir_all(&inbox).await.map_err(|x| format!("Folder terima tidak bisa dibuat: {x}"))?;
    let (dests, shown) = plan_dest(&inbox, &offer.files);
    write_json(&mut wr, &Reply { t: "accept".into(), ..Default::default() }).await?;

    let mut buf = vec![0u8; CHUNK];
    let mut done_total = 0u64;
    let mut th = Throttle::new();
    let mut parts: Vec<PathBuf> = Vec::new();
    let result: Result<(), String> = async {
        for (f, dest) in offer.files.iter().zip(&dests) {
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(e)?;
            }
            let part = PathBuf::from(format!("{}.part", dest.display()));
            parts.push(part.clone());
            let mut file = tokio::fs::File::create(&part).await.map_err(e)?;
            let mut left = f.size;
            while left > 0 {
                let n = (left as usize).min(buf.len());
                tokio::time::timeout(Duration::from_secs(60), rd.read_exact(&mut buf[..n])).await.map_err(|_| "Koneksi macet".to_string())?.map_err(|_| "Koneksi terputus".to_string())?;
                file.write_all(&buf[..n]).await.map_err(e)?;
                left -= n as u64;
                done_total += n as u64;
                if th.ready() {
                    (cb.on_event)(PeerEvent::Progress { id: id.clone(), dir: "in".into(), name: f.name.clone(), done: done_total, total });
                }
            }
            file.flush().await.map_err(e)?;
            drop(file);
            tokio::fs::rename(&part, dest).await.map_err(e)?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            let _ = write_json(&mut wr, &Reply { t: "done".into(), ok: true, ..Default::default() }).await;
            let path = shown.or_else(|| Some(inbox.clone())).map(|p| p.to_string_lossy().into_owned());
            (cb.on_event)(PeerEvent::Finished { id, dir: "in".into(), count: offer.files.len(), total, path });
            Ok(())
        }
        Err(err) => {
            for p in parts {
                let _ = tokio::fs::remove_file(p).await;
            }
            (cb.on_event)(PeerEvent::Failed { id, dir: "in".into(), error: err.clone() });
            Err(err)
        }
    }
}

/// Mulai mendengarkan di port 49250–49280 (kalau penuh, port acak).
pub async fn start_receiver(id: &Identity, cb: Arc<Callbacks>) -> Result<Receiver, String> {
    let acceptor = TlsAcceptor::from(Arc::new(server_config(id)?));
    let mut listener = None;
    for p in 49250u16..=49280 {
        if let Ok(l) = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, p))).await {
            listener = Some(l);
            break;
        }
    }
    let listener = match listener {
        Some(l) => l,
        None => TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await.map_err(|x| format!("Tidak bisa membuka port: {x}"))?,
    };
    let port = listener.local_addr().map_err(e)?.port();
    let (tx, mut rx) = oneshot::channel::<()>();
    let my_fp = id.fp.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                r = listener.accept() => {
                    let Ok((stream, _)) = r else { continue };
                    let (a, f, c) = (acceptor.clone(), my_fp.clone(), cb.clone());
                    tokio::spawn(async move { let _ = handle_conn(stream, a, f, c).await; });
                }
            }
        }
    });
    Ok(Receiver { port, shutdown: Some(tx) })
}

// ---------------------------------------------------------------------------------------------
// Pengirim
// ---------------------------------------------------------------------------------------------
/// Kumpulkan file (folder ditelusuri rekursif; symlink dilewati). Nama relatif memakai '/'.
pub fn collect_files(paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>, String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(PathBuf, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for ent in rd.flatten() {
            if out.len() >= MAX_FILES {
                return;
            }
            let Ok(ft) = ent.file_type() else { continue };
            let name = ent.file_name().to_string_lossy().into_owned();
            let r = format!("{rel}/{name}");
            if ft.is_symlink() {
                continue;
            } else if ft.is_dir() {
                walk(&ent.path(), &r, out);
            } else if ft.is_file() {
                out.push((ent.path(), r));
            }
        }
    }
    let mut out = Vec::new();
    for p in paths {
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let Ok(md) = std::fs::symlink_metadata(p) else { continue };
        if md.is_file() {
            out.push((p.clone(), name));
        } else if md.is_dir() {
            walk(p, &name, &mut out);
        }
    }
    if out.is_empty() {
        return Err("Tidak ada file yang bisa dikirim".into());
    }
    Ok(out)
}

/// Kirim file ke perangkat lain. `expect_fp` = sidik jari yang diiklankan perangkat tujuan (dipin di TLS).
/// `on_sas` dipanggil begitu koneksi aman terbentuk, supaya UI menampilkan kode verifikasi selagi menunggu.
pub async fn send(id: &Identity, my_name: &str, addrs: &[SocketAddr], expect_fp: &str, files: Vec<(PathBuf, String)>, tid: &str, on_event: &OnEvent, on_sas: &(dyn Fn(String) + Send + Sync)) -> Result<(), String> {
    let mut metas = Vec::new();
    for (p, rel) in &files {
        let size = tokio::fs::metadata(p).await.map_err(|x| format!("{rel}: {x}"))?.len();
        metas.push(FileMeta { name: rel.clone(), size });
    }
    let total: u64 = metas.iter().map(|m| m.size).sum();
    let connector = TlsConnector::from(Arc::new(client_config(id, expect_fp)?));
    let mut last_err = "Tidak ada alamat".to_string();
    let mut conn = None;
    for a in addrs {
        match tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(a)).await {
            Ok(Ok(s)) => {
                let _ = s.set_nodelay(true);
                let name = ServerName::try_from("notch.local").map_err(e)?;
                match tokio::time::timeout(Duration::from_secs(8), connector.connect(name, s)).await {
                    Ok(Ok(t)) => {
                        conn = Some(t);
                        break;
                    }
                    Ok(Err(x)) => last_err = format!("Koneksi aman gagal: {x}"),
                    Err(_) => last_err = "Koneksi aman timeout".into(),
                }
            }
            Ok(Err(x)) => last_err = format!("Tidak bisa tersambung ke {a}: {x}"),
            Err(_) => last_err = format!("Timeout ke {a}"),
        }
    }
    let tls = conn.ok_or(last_err)?;
    on_sas(sas(&id.fp, expect_fp));
    let (rd, mut wr) = tokio::io::split(tls);
    let mut rd = BufReader::new(rd);
    write_json(&mut wr, &Offer { t: "offer".into(), name: my_name.chars().take(60).collect(), files: metas.clone() }).await?;
    let line = tokio::time::timeout(Duration::from_secs(130), read_line(&mut rd, 64 * 1024)).await.map_err(|_| "Penerima tidak menjawab".to_string())??;
    let reply: Reply = serde_json::from_str(&line).map_err(e)?;
    if reply.t != "accept" {
        return Err(if reply.reason.is_empty() { "Ditolak oleh penerima".into() } else { reply.reason });
    }
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    let mut th = Throttle::new();
    for ((path, _), m) in files.iter().zip(&metas) {
        let mut f = tokio::fs::File::open(path).await.map_err(|x| format!("{}: {x}", m.name))?;
        let mut left = m.size;
        while left > 0 {
            let n = (left as usize).min(buf.len());
            f.read_exact(&mut buf[..n]).await.map_err(|x| format!("{} berubah saat dikirim: {x}", m.name))?;
            wr.write_all(&buf[..n]).await.map_err(|_| "Koneksi terputus".to_string())?;
            left -= n as u64;
            done += n as u64;
            if th.ready() {
                on_event(PeerEvent::Progress { id: tid.to_string(), dir: "out".into(), name: m.name.clone(), done, total });
            }
        }
    }
    wr.flush().await.map_err(e)?;
    let line = tokio::time::timeout(Duration::from_secs(60), read_line(&mut rd, 64 * 1024)).await.map_err(|_| "Penerima tidak mengonfirmasi".to_string())??;
    let reply: Reply = serde_json::from_str(&line).map_err(e)?;
    if reply.t == "done" && reply.ok { Ok(()) } else { Err("Penerima gagal menyimpan file".into()) }
}
