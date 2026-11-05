//! Pembungkus `adb` (tanpa dependensi Tauri, jadi bisa diuji dengan adb palsu).
//! adb dicari: ADB_PATH -> folder resource aplikasi (adb/) -> folder exe -> PATH -> Android SDK.
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct Device {
    pub serial: String,
    /// "device" (siap) | "unauthorized" (belum disetujui di HP) | "offline" | ...
    pub state: String,
    pub model: String,
    pub wifi: bool,
    /// true = terhubung lewat MTP (hanya file, tanpa USB debugging; tidak bisa mirror layar).
    pub mtp: bool,
}

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub date: String,
}

/// Satu layanan mDNS yang terlihat oleh adb (`adb mdns services`).
#[derive(Clone, Debug, PartialEq)]
pub struct Mdns {
    pub name: String,
    pub kind: String,
    pub addr: String,
}

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Out {
    fn msg(&self) -> String {
        let s = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("adb gagal tanpa pesan").trim().to_string()
    }
}

fn hide(cmd: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Jalankan perintah, kumpulkan keluaran lewat thread (aman untuk keluaran besar), dengan batas waktu opsional.
fn run(mut cmd: Command, timeout: Option<Duration>) -> Result<Out, String> {
    hide(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan adb: {e}"))?;
    let (mut so, mut se) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if timeout.map(|t| start.elapsed() > t).unwrap_or(false) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("adb tidak merespons (timeout)".into()); // thread pembaca dilepas
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let clean = |b: Vec<u8>| String::from_utf8_lossy(&b).replace('\r', "");
    Ok(Out { ok: status.success(), stdout: clean(t1.join().unwrap_or_default()), stderr: clean(t2.join().unwrap_or_default()) })
}

fn works(p: &Path) -> bool {
    let mut c = Command::new(p);
    c.arg("version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    hide(&mut c).status().map(|s| s.success()).unwrap_or(false)
}

pub fn find(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "adb.exe" } else { "adb" };
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("ADB_PATH") {
        c.push(PathBuf::from(p));
    }
    if let Some(rd) = resource_dir {
        c.push(rd.join("adb").join(exe));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        c.push(dir.join("adb").join(exe));
        c.push(dir.join("resources").join("adb").join(exe));
        c.push(dir.join(exe));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            c.push(d.join(exe));
        }
    }
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(h) = std::env::var_os(var) {
            c.push(Path::new(&h).join("platform-tools").join(exe));
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            c.push(Path::new(&l).join("Android").join("Sdk").join("platform-tools").join(exe));
        }
    } else if let Some(h) = std::env::var_os("HOME") {
        c.push(Path::new(&h).join("Android/Sdk/platform-tools").join(exe));
        c.push(PathBuf::from("/usr/bin/adb"));
    }
    c.into_iter().find(|p| p.is_file() && works(p))
}

/// Kutip argumen untuk shell di perangkat (adb shell menggabungkan argumen jadi satu baris perintah).
pub fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn parse_devices(text: &str) -> Vec<Device> {
    let mut v = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('*') || l.starts_with("List of devices") {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(serial), Some(state)) = (it.next(), it.next()) else { continue };
        let model = it.find_map(|t| t.strip_prefix("model:")).map(|m| m.replace('_', " ")).unwrap_or_default();
        v.push(Device {
            serial: serial.to_string(),
            state: state.to_string(),
            model: if model.is_empty() { serial.to_string() } else { model },
            wifi: serial.contains(':') || serial.contains("_adb-tls"),
            mtp: false,
        });
    }
    v
}

/// Keluaran `adb mdns services`: "<nama>  <jenis>  <ip:port>" per baris.
pub fn parse_mdns(text: &str) -> Vec<Mdns> {
    text.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() >= 3 && t[1].starts_with('_') {
                Some(Mdns { name: t[0].into(), kind: t[1].into(), addr: t[2].into() })
            } else {
                None
            }
        })
        .collect()
}

fn take_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

/// Parse keluaran `ls -lA` (toybox/busybox): perms links owner group size tanggal jam nama
pub fn parse_ls(text: &str, show_hidden: bool) -> Vec<Entry> {
    let mut v = Vec::new();
    for line in text.lines() {
        let l = line.trim_end();
        if l.is_empty() || l.starts_with("total ") || !matches!(l.chars().next(), Some('d' | '-' | 'l' | 'c' | 'b' | 's' | 'p')) {
            continue;
        }
        let mut rest = l;
        let mut toks: Vec<&str> = Vec::with_capacity(7);
        for _ in 0..7 {
            match take_token(rest) {
                Some((t, r)) => {
                    toks.push(t);
                    rest = r;
                }
                None => break,
            }
        }
        if toks.len() < 7 {
            continue;
        }
        let name_raw = rest.strip_prefix(' ').unwrap_or(rest);
        let link = toks[0].starts_with('l');
        let name = if link { name_raw.split(" -> ").next().unwrap_or(name_raw) } else { name_raw };
        if name.is_empty() || name == "." || name == ".." || (!show_hidden && name.starts_with('.')) {
            continue;
        }
        v.push(Entry { name: name.to_string(), is_dir: toks[0].starts_with('d') || link, size: toks[4].parse().unwrap_or(0), date: format!("{} {}", toks[5], toks[6]) });
    }
    v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

pub struct Adb {
    pub path: PathBuf,
}

impl Adb {
    pub fn new(path: PathBuf) -> Adb {
        Adb { path }
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.path);
        // Windows: paksa backend mDNS Openscreen (tanpa Bonjour). Versi adb lama mengabaikan variabel ini.
        c.args(args).env("ADB_MDNS_OPENSCREEN", "1");
        c
    }

    /// Pastikan server adb hidup. Stdio dibuang agar daemon yang di-fork tidak menahan pipe.
    fn ensure_server(&self) {
        let mut c = self.cmd(&["start-server"]);
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let _ = hide(&mut c).status();
    }

    pub fn devices(&self) -> Result<Vec<Device>, String> {
        self.ensure_server();
        let o = run(self.cmd(&["devices", "-l"]), Some(Duration::from_secs(10)))?;
        if !o.ok {
            return Err(o.msg());
        }
        Ok(parse_devices(&o.stdout))
    }

    pub fn list_dir(&self, serial: &str, path: &str) -> Result<Vec<Entry>, String> {
        let p = if path.ends_with('/') { path.to_string() } else { format!("{path}/") };
        let q = shq(&p);
        let o = run(self.cmd(&["-s", serial, "shell", "ls", "-lA", &q]), Some(Duration::from_secs(30)))?;
        let entries = parse_ls(&o.stdout, false);
        if entries.is_empty() && (!o.ok || o.stdout.contains("ls:")) {
            let m = o.msg();
            return Err(if m.contains("Permission denied") { "Izin ditolak: folder ini dilindungi Android".into() } else { m });
        }
        Ok(entries)
    }

    /// Kirim file/folder lokal ke folder `remote_dir` di perangkat.
    pub fn push(&self, serial: &str, local: &Path, remote_dir: &str) -> Result<(), String> {
        let dir = if remote_dir.ends_with('/') { remote_dir.to_string() } else { format!("{remote_dir}/") };
        let mut c = self.cmd(&["-s", serial, "push"]);
        c.arg(local).arg(&dir);
        let o = run(c, None)?;
        if o.ok { Ok(()) } else { Err(o.msg()) }
    }

    /// Ambil file/folder dari perangkat ke path lokal `dest` (belum ada; adb membuatnya).
    pub fn pull(&self, serial: &str, remote: &str, dest: &Path) -> Result<(), String> {
        let mut c = self.cmd(&["-s", serial, "pull", remote]);
        c.arg(dest);
        let o = run(c, None)?;
        if o.ok { Ok(()) } else { Err(o.msg()) }
    }

    pub fn connect(&self, addr: &str) -> Result<String, String> {
        let o = run(self.cmd(&["connect", addr]), Some(Duration::from_secs(15)))?;
        let t = format!("{}{}", o.stdout, o.stderr);
        let line = t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        if o.ok && (line.starts_with("connected") || line.starts_with("already connected")) { Ok(line) } else { Err(if line.is_empty() { "Gagal tersambung".into() } else { line }) }
    }

    pub fn pair(&self, addr: &str, code: &str) -> Result<String, String> {
        let o = run(self.cmd(&["pair", addr, code]), Some(Duration::from_secs(20)))?;
        let t = format!("{}{}", o.stdout, o.stderr);
        let line = t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        if o.ok && line.starts_with("Successfully paired") { Ok(line) } else { Err(if line.is_empty() { "Pairing gagal".into() } else { line }) }
    }

    pub fn disconnect(&self, addr: &str) {
        let _ = run(self.cmd(&["disconnect", addr]), Some(Duration::from_secs(8)));
    }

    /// Cek apakah backend mDNS adb tersedia (dipakai pairing lewat QR).
    pub fn mdns_check(&self) -> Result<String, String> {
        self.ensure_server();
        let o = run(self.cmd(&["mdns", "check"]), Some(Duration::from_secs(8)))?;
        Ok(format!("{}{}", o.stdout, o.stderr))
    }

    /// Daftar layanan mDNS yang sedang terlihat (pairing & connect dari HP).
    pub fn mdns_services(&self) -> Result<Vec<Mdns>, String> {
        let o = run(self.cmd(&["mdns", "services"]), Some(Duration::from_secs(8)))?;
        if !o.ok {
            return Err(o.msg());
        }
        Ok(parse_mdns(&o.stdout))
    }
}