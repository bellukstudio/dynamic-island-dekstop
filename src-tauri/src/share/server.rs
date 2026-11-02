//! Inti server transfer file lewat jaringan lokal (tanpa dependensi Tauri, jadi bisa diuji sendiri).
//!
//! Perangkat lain (HP, PC lain) cukup membuka alamat/QR di browser:
//!   GET  /s/{token}/             halaman web (daftar file dari PC + unggah ke PC)
//!   GET  /s/{token}/list         JSON file yang dibagikan PC
//!   GET  /s/{token}/dl/{id}      unduh file (streaming)
//!   PUT  /s/{token}/up/{nama}    unggah file (body mentah, streaming) -> folder terima
//! Token acak 128-bit ada di path; tanpa token yang benar semua jawaban 404.
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

#[derive(Clone, Serialize)]
pub struct SharedFile {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Received { name: String, size: u64, path: String },
    Failed { name: String, error: String },
    Downloaded { name: String },
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
pub type Files = Arc<Mutex<Vec<SharedFile>>>;

struct Shared {
    token: String,
    files: Files,
    inbox: PathBuf,
    emit: Emit,
}

pub struct Running {
    pub port: u16,
    pub token: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Running {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn random_token() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("RNG sistem tidak tersedia");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Nama file aman untuk disimpan: tanpa path, karakter terlarang, nama khusus Windows.
pub fn sanitize(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = last
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    s = s.trim().trim_matches('.').trim().to_string();
    if s.chars().count() > 150 {
        let ext = FsPath::new(&s).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let keep = 140usize.saturating_sub(ext.len());
        let stem: String = FsPath::new(&s).file_stem().map(|x| x.to_string_lossy().chars().take(keep).collect()).unwrap_or_default();
        s = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
    }
    let stem_upper = s.split('.').next().unwrap_or("").to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem_upper.as_str())
        || (stem_upper.len() == 4 && (stem_upper.starts_with("COM") || stem_upper.starts_with("LPT")) && stem_upper.as_bytes()[3].is_ascii_digit());
    if s.is_empty() {
        return "file".into();
    }
    if reserved { format!("_{s}") } else { s }
}

fn unique_in(dir: &FsPath, name: &str) -> PathBuf {
    let p = FsPath::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut c = dir.join(name);
    let mut n = 2;
    while c.exists() || PathBuf::from(format!("{}.part", c.display())).exists() {
        c = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    c
}

fn pct_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

async fn page(State(sh): State<Arc<Shared>>, Path(token): Path<String>) -> Response {
    if token != sh.token {
        return not_found();
    }
    let mut r = Html(PAGE).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

async fn list(State(sh): State<Arc<Shared>>, Path(token): Path<String>) -> Response {
    if token != sh.token {
        return not_found();
    }
    let v: Vec<serde_json::Value> = sh
        .files
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|f| serde_json::json!({ "id": f.id, "name": f.name, "size": f.size }))
        .collect();
    let mut r = Json(v).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

async fn download(State(sh): State<Arc<Shared>>, Path((token, id)): Path<(String, u32)>) -> Response {
    if token != sh.token {
        return not_found();
    }
    let file = sh.files.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|f| f.id == id).cloned();
    let Some(file) = file else { return not_found() };
    let Ok(f) = tokio::fs::File::open(&file.path).await else { return not_found() };
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(file.size);
    let ascii: String = file.name.chars().map(|c| if c.is_ascii() && !c.is_control() && c != '"' && c != '\\' { c } else { '_' }).collect();
    let disp = format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", pct_encode(&file.name));
    (sh.emit)(Event::Downloaded { name: file.name.clone() });
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(f, 256 * 1024));
    let mut r = Response::new(body);
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    if let Ok(v) = HeaderValue::from_str(&disp) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    r
}

async fn upload(State(sh): State<Arc<Shared>>, Path((token, name)): Path<(String, String)>, body: Body) -> Response {
    if token != sh.token {
        return not_found();
    }
    let name = sanitize(&name);
    if let Err(e) = tokio::fs::create_dir_all(&sh.inbox).await {
        let msg = format!("Folder terima tidak bisa dibuat: {e}");
        (sh.emit)(Event::Failed { name, error: msg.clone() });
        return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
    }
    let dest = unique_in(&sh.inbox, &name);
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let result: Result<u64, String> = async {
        let mut f = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
        let mut total = 0u64;
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Koneksi terputus: {e}"))?;
            total += chunk.len() as u64;
            f.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        f.flush().await.map_err(|e| e.to_string())?;
        drop(f);
        tokio::fs::rename(&part, &dest).await.map_err(|e| e.to_string())?;
        Ok(total)
    }
    .await;
    match result {
        Ok(size) => {
            let shown = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
            (sh.emit)(Event::Received { name: shown, size, path: dest.to_string_lossy().into_owned() });
            Json(serde_json::json!({ "ok": true, "size": size })).into_response()
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            (sh.emit)(Event::Failed { name, error: e.clone() });
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}

/// Jalankan server di 0.0.0.0 (port 49210–49240, kalau penuh port acak). Harus dipanggil dari dalam runtime tokio.
pub async fn start(files: Files, inbox: PathBuf, emit: Emit) -> Result<Running, String> {
    let mut listener = None;
    for p in 49210u16..=49240 {
        if let Ok(l) = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, p))).await {
            listener = Some(l);
            break;
        }
    }
    let listener = match listener {
        Some(l) => l,
        None => tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await.map_err(|e| format!("Tidak bisa membuka port: {e}"))?,
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = random_token();
    let shared = Arc::new(Shared { token: token.clone(), files, inbox, emit });
    let app = Router::new()
        .route("/s/{token}/", get(page))
        .route("/s/{token}/list", get(list))
        .route("/s/{token}/dl/{id}", get(download))
        .route("/s/{token}/up/{name}", put(upload))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(shared);
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    Ok(Running { port, token, shutdown: Some(tx) })
}

/// Alamat IPv4 jaringan lokal, yang paling mungkin dipakai (Wi-Fi/Ethernet) di urutan pertama.
pub fn lan_ips() -> Vec<Ipv4Addr> {
    let Ok(list) = local_ip_address::list_afinet_netifas() else { return vec![] };
    let virt = ["docker", "veth", "br-", "virbr", "vmnet", "vbox", "vethernet", "wsl", "tailscale", "zt", "utun", "lo", "loopback", "hyper-v", "tun", "tap"];
    let mut v: Vec<(u8, Ipv4Addr)> = vec![];
    for (name, ip) in list {
        let IpAddr::V4(ip) = ip else { continue };
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || !ip.is_private() {
            continue;
        }
        let n = name.to_lowercase();
        let rank = if virt.iter().any(|k| n.contains(k)) {
            2
        } else if n.contains("wl") || n.contains("wi-fi") || n.contains("wifi") || n.contains("wlan") || n.contains("eth") || n.starts_with("en") {
            0
        } else {
            1
        };
        v.push((rank, ip));
    }
    v.sort_by_key(|(r, ip)| (*r, ip.octets()));
    v.dedup_by_key(|(_, ip)| *ip);
    v.into_iter().map(|(_, ip)| ip).collect()
}

/// QR sebagai SVG murni (siap dipasang lewat innerHTML).
pub fn qr_svg(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    svg.find("<svg").map(|i| svg[i..].to_string())
}

const PAGE: &str = r##"<!doctype html>
<html lang="id"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Notch · Berbagi file</title>
<style>
:root{color-scheme:light dark;--bg:#fff;--fg:#111;--mut:#667;--card:#f2f3f7;--acc:#3b82f6;--line:#d8dbe3}
@media (prefers-color-scheme:dark){:root{--bg:#14151a;--fg:#eee;--mut:#9aa;--card:#1f2128;--line:#33363f}}
*{box-sizing:border-box}body{margin:0;font:16px system-ui,sans-serif;background:var(--bg);color:var(--fg);padding:18px;max-width:640px;margin:auto}
h1{font-size:20px;margin:4px 0 2px}p.s{color:var(--mut);margin:0 0 16px;font-size:14px}
h2{font-size:14px;color:var(--mut);text-transform:uppercase;letter-spacing:.05em;margin:22px 0 8px}
.drop{border:2px dashed var(--line);border-radius:14px;padding:26px 12px;text-align:center;cursor:pointer;background:var(--card)}
.drop.over{border-color:var(--acc)}.drop b{display:block;margin-bottom:4px}
.row{display:flex;align-items:center;gap:10px;background:var(--card);border-radius:12px;padding:10px 12px;margin-bottom:8px}
.row .n{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.row small{color:var(--mut);display:block}
a.dl,button{background:var(--acc);color:#fff;border:0;border-radius:9px;padding:8px 14px;font-size:14px;text-decoration:none;cursor:pointer}
.bar{height:5px;background:var(--line);border-radius:3px;overflow:hidden;margin-top:6px}.bar i{display:block;height:100%;width:0;background:var(--acc)}
.ok{color:#22a559}.err{color:#e5484d}.empty{color:var(--mut);font-size:14px;padding:6px 2px}
</style></head><body>
<h1>Notch · Berbagi file</h1><p class="s">Kirim file ke PC ini atau unduh file yang dibagikan PC.</p>
<h2>Kirim ke PC</h2>
<div class="drop" id="drop"><b>Ketuk untuk memilih file</b><span class="empty">atau seret file ke sini</span>
<input id="pick" type="file" multiple hidden></div><div id="ups"></div>
<h2>Dari PC</h2><div id="files"><div class="empty">Memuat…</div></div>
<script>
const $=id=>document.getElementById(id);
const fmt=b=>b<1024?b+" B":b<1048576?(b/1024).toFixed(1)+" KB":b<1073741824?(b/1048576).toFixed(1)+" MB":(b/1073741824).toFixed(2)+" GB";
const esc=s=>s.replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
async function refresh(){try{const l=await (await fetch("list",{cache:"no-store"})).json();
$("files").innerHTML=l.length?l.map(f=>`<div class="row"><div class="n">${esc(f.name)}<small>${fmt(f.size)}</small></div><a class="dl" href="dl/${f.id}">Unduh</a></div>`).join(""):'<div class="empty">Belum ada file yang dibagikan PC.</div>';}catch(e){}}
refresh();setInterval(refresh,3000);
function send(file){return new Promise(res=>{const row=document.createElement("div");row.className="row";
row.innerHTML=`<div class="n">${esc(file.name)}<small>${fmt(file.size)}</small><div class="bar"><i></i></div></div><span>0%</span>`;
$("ups").prepend(row);const bar=row.querySelector("i"),pct=row.querySelector("span");
const x=new XMLHttpRequest();x.open("PUT","up/"+encodeURIComponent(file.name));
x.upload.onprogress=e=>{if(e.lengthComputable){const p=Math.round(e.loaded/e.total*100);bar.style.width=p+"%";pct.textContent=p+"%"}};
x.onload=()=>{if(x.status==200){pct.textContent="Selesai";pct.className="ok";bar.style.width="100%"}else{pct.textContent="Gagal";pct.className="err"}res()};
x.onerror=()=>{pct.textContent="Gagal";pct.className="err";res()};x.send(file)})}
async function sendAll(fs){for(const f of fs)await send(f)}
$("drop").onclick=()=>$("pick").click();$("pick").onchange=e=>{sendAll([...e.target.files]);e.target.value=""};
["dragover","dragenter"].forEach(t=>$("drop").addEventListener(t,e=>{e.preventDefault();$("drop").classList.add("over")}));
["dragleave","drop"].forEach(t=>$("drop").addEventListener(t,e=>{e.preventDefault();$("drop").classList.remove("over")}));
$("drop").addEventListener("drop",e=>sendAll([...e.dataTransfer.files]));
</script></body></html>"##;
