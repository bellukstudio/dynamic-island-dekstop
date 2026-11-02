// Jendela Android (android.html): jelajahi HP Android & transfer file lewat USB / Wi-Fi debugging (adb),
// plus mirror layar + kontrol (scrcpy-server lewat adb, didekode dengan WebCodecs).
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Device { serial: string; state: string; model: string; wifi: boolean }
interface Entry { name: string; is_dir: boolean; size: number; date: string }
interface PushResult { name: string; ok: boolean; error: string | null }
interface Op { id: number; label: string; state: "running" | "ok" | "error"; detail: string; path?: string }
interface MirrorInfo { ws_port: number; width: number; height: number }

const LOCS: [string, string][] = [
    ["Penyimpanan", "/sdcard"], ["Kamera", "/sdcard/DCIM/Camera"], ["Unduhan", "/sdcard/Download"],
    ["Gambar", "/sdcard/Pictures"], ["Dokumen", "/sdcard/Documents"], ["Musik", "/sdcard/Music"],
];

const state = {
    active: false,
    adbOk: true,
    devices: [] as Device[],
    serial: null as string | null,
    path: "/sdcard",
    entries: [] as Entry[],
    loading: false,
    error: null as string | null,
    ops: [] as Op[],
    wifiOpen: false,
    wifiMsg: "",
    wifiErr: false,
    wifiVals: { pairAddr: "", pairCode: "", connAddr: "" } as Record<string, string>,
    seq: 0,
};
let poll = 0;
let listToken = 0;

// ---------- util ----------
const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
function fmt(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const u = ["KB", "MB", "GB"];
    let v = bytes / 1024, i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v.toLocaleString("id-ID", { maximumFractionDigits: v >= 100 ? 0 : 1 })} ${u[i]}`;
}
function kindOf(name: string): { cls: string; label: string } {
    const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
    if (["jpg", "jpeg", "png", "gif", "webp", "heic", "bmp"].includes(ext)) return { cls: "k-image", label: ext.toUpperCase() };
    if (ext === "pdf") return { cls: "k-pdf", label: "PDF" };
    if (["doc", "docx", "xls", "xlsx", "ppt", "pptx"].includes(ext)) return { cls: "k-office", label: ext.toUpperCase() };
    return { cls: "k-other", label: (ext || "file").slice(0, 4).toUpperCase() };
}
const join = (dir: string, name: string) => dir.replace(/\/+$/, "") + "/" + name;
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const ready = () => state.devices.find((d) => d.serial === state.serial && d.state === "device");

const ICON = {
    logo: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/></svg>',
    drop: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>',
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>',
    back: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M15 5v14L5 12z"/></svg>',
    home: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4"><circle cx="12" cy="12" r="6.5"/></svg>',
    recent: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"><rect x="5.5" y="5.5" width="13" height="13" rx="2.5"/></svg>',
    power: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 3v9"/><path d="M6.4 6.8a8 8 0 1 0 11.2 0"/></svg>',
    volUp: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"/><path d="M16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11"/></svg>',
    volDown: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"/><path d="M16 9a4 4 0 0 1 0 6"/></svg>',
    stop: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
};

// ---------- kerangka UI ----------
$("app").innerHTML = `
<div class="win">
  <header class="bar" id="bar">
    <div class="brand">
      <div class="logo">${ICON.logo}</div>
      <div><h1>Android</h1><p>Jelajahi HP &amp; transfer file lewat USB atau Wi-Fi</p></div>
    </div>
     <div class="win-ctl">
      <button class="x" id="minimize" aria-label="Minimalkan" title="Minimalkan">&#8211;</button>
      <button class="x" id="maximize" aria-label="Maksimalkan" title="Maksimalkan / pulihkan">&#9633;</button>
      <button class="x" id="close" aria-label="Tutup" title="Tutup">×</button>
    </div>
  </header>
  <main id="main"></main>
  <footer>
    <span class="hint"><kbd>Esc</kbd> tutup · <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd> buka/tutup · <kbd>Ctrl</kbd>+<kbd>M</kbd> mirror · dobel-klik header: fullscreen</span>
    <div class="acts" id="acts"></div>
  </footer>
  <section class="mirror-view" id="mirrorView">
    <div class="mirror-main">
      <div class="mirror-stage" id="mirrorStage">
        <canvas id="mirrorCanvas" tabindex="0"></canvas>
        <div class="mirror-status" id="mirrorStatus">Menyambung…</div>
      </div>
      <aside class="mirror-rail">
        <button class="rail-btn" data-mkey="26" title="Power / layar"><span class="ic">${ICON.power}</span><small>Power</small></button>
        <button class="rail-btn" data-mkey="24" title="Volume naik"><span class="ic">${ICON.volUp}</span><small>Vol +</small></button>
        <button class="rail-btn" data-mkey="25" title="Volume turun"><span class="ic">${ICON.volDown}</span><small>Vol −</small></button>
        <span class="grow"></span>
        <button class="rail-btn danger" id="mirrorStop" title="Tutup mirror (Esc)"><span class="ic">${ICON.stop}</span><small>Tutup</small></button>
      </aside>
    </div>
    <nav class="mirror-nav">
      <button class="nav-btn" data-mkey="back" title="Kembali (klik kanan)" aria-label="Kembali">${ICON.back}</button>
      <button class="nav-btn" data-mkey="3" title="Home (klik tengah)" aria-label="Home">${ICON.home}</button>
      <button class="nav-btn" data-mkey="187" title="Recent" aria-label="Recent">${ICON.recent}</button>
    </nav>
  </section>
  <div class="overlay" id="dragOverlay"><div class="overlay-card">${ICON.drop}<span>Lepas untuk mengirim ke folder ini</span></div></div>
</div>`;

// ---------- render ----------
function devicesHtml(): string {
    if (!state.devices.length) {
        return `<div class="hint-card"><b>Belum ada HP terdeteksi.</b><ol>
          <li>Di HP: <i>Pengaturan → Tentang ponsel</i>, ketuk <i>Nomor build</i> 7 kali untuk membuka Opsi Pengembang.</li>
          <li>Aktifkan <i>Opsi pengembang → Debugging USB</i>.</li>
          <li>Sambungkan kabel USB (mode <i>Transfer file</i>), lalu setujui dialog <i>Izinkan debugging USB</i> di HP.</li></ol></div>`;
    }
    return `<div class="devs">${state.devices.map((d) => {
        const ok = d.state === "device";
        const note = d.state === "unauthorized" ? "setujui dialog di HP" : d.state === "offline" ? "offline" : d.wifi ? "Wi-Fi" : "USB";
        return `<div class="dev${d.serial === state.serial ? " on" : ""}${ok ? "" : " bad"}" data-dev="${esc(d.serial)}">
          <b>${esc(d.model)}</b><small>${note}</small></div>`;
    }).join("")}</div>`;
}

function wifiHtml(): string {
    const v = state.wifiVals;
    return `
    <div style="margin-top:10px"><button class="btn" id="wifitoggle">${state.wifiOpen ? "Tutup" : "Sambungkan lewat Wi-Fi"}</button></div>
    ${state.wifiOpen ? `
    <div class="hint-card" style="margin-top:8px">Android 11+: <i>Opsi pengembang → Debugging nirkabel</i>. Pairing sekali (alamat &amp; kode dari <i>Pasangkan dengan kode</i>), lalu sambungkan dengan alamat di layar utama Debugging nirkabel.</div>
    <div class="wifi-box">
      <div class="wifi-step">
        <div class="wifi-h"><span class="n">1</span>Pairing <small>(cukup sekali)</small></div>
        <label class="wifi-fld"><span>Alamat pairing</span>
          <input id="pairAddr" value="${esc(v.pairAddr)}" placeholder="192.168.1.20:37123" autocomplete="off" spellcheck="false" /></label>
        <label class="wifi-fld"><span>Kode pairing</span>
          <input id="pairCode" value="${esc(v.pairCode)}" placeholder="123456" inputmode="numeric" maxlength="6" autocomplete="off" spellcheck="false" /></label>
        <button class="btn" id="dopair">Pairing</button>
      </div>
      <div class="wifi-step single">
        <div class="wifi-h"><span class="n">2</span>Sambungkan</div>
        <label class="wifi-fld"><span>Alamat sambung</span>
          <input id="connAddr" value="${esc(v.connAddr)}" placeholder="192.168.1.20:5555" autocomplete="off" spellcheck="false" /></label>
        <button class="btn primary" id="doconn">Sambungkan</button>
      </div>
      <div class="wifi-msg${state.wifiErr ? " err" : ""}" id="wifimsg">${esc(state.wifiMsg)}</div>
    </div>` : ""}`;
}

function crumbsHtml(): string {
    const parts = state.path.split("/").filter(Boolean);
    let acc = "";
    const items = parts.map((p, i) => {
        acc += "/" + p;
        return `<button data-go="${esc(acc)}">${esc(p)}</button>${i < parts.length - 1 ? "<span>/</span>" : ""}`;
    });
    return `<div class="crumbs"><button data-go="/">/</button>${items.join("")}</div>`;
}

function entryRow(e: Entry): string {
    const full = join(state.path, e.name);
    const k = e.is_dir ? { cls: "k-other", label: "DIR" } : kindOf(e.name);
    return `
    <div class="row">
      <div class="badge ${k.cls}">${k.label}</div>
      <div class="meta"><div class="nm${e.is_dir ? " dirnm" : ""}" ${e.is_dir ? `data-go="${esc(full)}"` : ""} title="${esc(full)}">${esc(e.name)}</div>
        <div class="sub">${e.is_dir ? "Folder" : fmt(e.size)} · ${esc(e.date)}</div></div>
      <div class="sz"><span class="plain">${e.is_dir ? "" : fmt(e.size)}</span></div>
      <span class="pill none">${e.is_dir ? "Folder" : "File"}</span>
      <button class="rm" data-pull="${esc(full)}" aria-label="Unduh ke PC" style="font-size:12px;width:auto;padding:0 8px">Unduh</button>
    </div>`;
}

function opRow(o: Op): string {
    return `
    <div class="row">
      <div class="badge k-other">${o.state === "running" ? "…" : o.state === "ok" ? "OK" : "!"}</div>
      <div class="meta"><div class="nm">${esc(o.label)}</div>
        <div class="${o.state === "error" ? "sub bad" : "sub"}">${esc(o.detail)}</div></div>
      <div class="sz"></div>
      <span class="pill ${o.state === "ok" ? "" : o.state === "error" ? "bad" : "none"}">${o.state === "running" ? "Proses" : o.state === "ok" ? "Selesai" : "Gagal"}</span>
      ${o.path ? `<button class="rm" data-reveal="${esc(o.path)}" style="font-size:12px;width:auto;padding:0 8px">Buka</button>` : "<span></span>"}
    </div>`;
}

function render() {
    const main = $("main");
    const acts = $("acts");

    if (!state.adbOk) {
        main.innerHTML = `<section class="panel"><div class="hint-card"><b>adb belum ditemukan.</b>
          <p>Fitur ini memakai <i>Android platform-tools</i>. Letakkan <code>adb</code> di folder <code>adb</code> pada resource aplikasi,
          atau pasang platform-tools dan tambahkan ke PATH (variabel <code>ADB_PATH</code> juga didukung).</p></div></section>`;
        acts.innerHTML = `<button class="btn primary" id="retry">Coba lagi</button>`;
        return;
    }

    const dev = ready();
    let browser = "";
    if (dev) {
        const loc = LOCS.map(([n, p]) => `<button class="preset${state.path === p ? " on" : ""}" data-go="${p}">${n}</button>`).join("");
        const parent = state.path.replace(/\/[^/]*\/?$/, "") || "/";
        const body = state.loading
            ? `<div class="empty-line">Memuat…</div>`
            : state.error
                ? `<div class="err-line">${esc(state.error)}</div>`
                : state.entries.length
                    ? `<section class="list">${state.entries.map(entryRow).join("")}</section>`
                    : `<div class="empty-line">Folder kosong. Seret file ke jendela ini untuk mengirimnya ke sini.</div>`;
        browser = `<div class="sec-t">${esc(dev.model)}</div>
          <div class="locs">${loc}${state.path !== "/" ? `<button class="preset" data-go="${esc(parent)}">↑ Naik</button>` : ""}</div>
          ${crumbsHtml()}${body}`;
    } else if (state.devices.some((d) => d.state === "unauthorized")) {
        browser = `<div class="empty-line">Setujui dialog <b>Izinkan debugging USB</b> di layar HP (centang <i>Selalu izinkan</i>).</div>`;
    }
    const ops = state.ops.slice(0, 4);
    main.innerHTML = `
      <section class="panel">${devicesHtml()}${wifiHtml()}</section>
      ${browser}
      ${ops.length ? `<div class="sec-t">Aktivitas</div><section class="list">${ops.map(opRow).join("")}</section>` : ""}`;

    acts.innerHTML = `<button class="btn" id="mirror" ${dev ? "" : "disabled"}>Mirror layar</button>
      <button class="btn" id="send" ${dev ? "" : "disabled"}>${ICON.plus}Kirim file ke folder ini</button>
      <button class="btn" id="refresh">Segarkan</button>`;
}

// ---------- aksi ----------
async function loadDir() {
    const dev = ready();
    if (!dev) return;
    const tok = ++listToken;
    state.loading = true;
    state.error = null;
    render();
    try {
        const e = await invoke<Entry[]>("android_ls", { serial: dev.serial, path: state.path });
        if (tok !== listToken) return;
        state.entries = e;
    } catch (err) {
        if (tok !== listToken) return;
        state.entries = [];
        state.error = String(err);
    }
    state.loading = false;
    render();
}

async function refreshDevices(initial = false) {
    try {
        const d = await invoke<Device[]>("android_devices");
        state.adbOk = true;
        const changed = JSON.stringify(d) !== JSON.stringify(state.devices);
        state.devices = d;
        if (!ready()) state.serial = null;
        if (!state.serial) {
            const first = d.find((x) => x.state === "device");
            if (first) { state.serial = first.serial; state.path = "/sdcard"; void loadDir(); return; }
        }
        if (changed || initial) render();
    } catch (e) {
        if (String(e) === "ADB_MISSING") { state.adbOk = false; render(); }
    }
}

function startPoll() {
    window.clearInterval(poll);
    poll = window.setInterval(() => { if (state.active) void refreshDevices(); }, 2500);
}

async function checkAdb() {
    const s = await invoke<{ found: boolean }>("android_status");
    state.adbOk = s.found;
    if (s.found) await refreshDevices(true); else render();
}

function addOp(label: string): Op {
    const op: Op = { id: ++state.seq, label, state: "running", detail: "Berjalan…" };
    state.ops.unshift(op);
    render();
    return op;
}

async function pull(remote: string) {
    const dev = ready();
    if (!dev) return;
    const name = remote.split("/").pop() ?? remote;
    const op = addOp(`Unduh ${name}`);
    try {
        const r = await invoke<{ local_path: string }>("android_pull", { serial: dev.serial, remote });
        op.state = "ok"; op.detail = "Disimpan di " + r.local_path; op.path = r.local_path;
        // beri tahu notch utama supaya muncul kartu "Unduhan selesai"
        emit("android-download-done", { name, path: r.local_path })
            .catch((err) => console.error("[Android] emit android-download-done gagal:", err));
    } catch (e) { op.state = "error"; op.detail = String(e); }
    render();
}

async function push(paths: string[]) {
    const dev = ready();
    if (!dev || !paths.length) return;
    const op = addOp(`Kirim ${paths.length} item ke ${state.path}`);
    try {
        const res = await invoke<PushResult[]>("android_push", { serial: dev.serial, paths, remoteDir: state.path });
        const bad = res.filter((r) => !r.ok);
        if (bad.length) { op.state = "error"; op.detail = `${bad.length} gagal: ${bad[0].name} — ${bad[0].error}`; }
        else { op.state = "ok"; op.detail = `${res.length} item terkirim`; }
    } catch (e) { op.state = "error"; op.detail = String(e); }
    render();
    void loadDir();
}

// ---------- Wi-Fi debugging ----------
function setWifiMsg() {
    const el = document.getElementById("wifimsg");
    if (!el) return;
    el.textContent = state.wifiMsg;
    el.classList.toggle("err", state.wifiErr);
}

async function wifi(kind: "pair" | "conn") {
    const v = state.wifiVals;
    const addr = (kind === "pair" ? v.pairAddr : v.connAddr).trim();
    if (!addr || (kind === "pair" && !v.pairCode.trim())) {
        state.wifiMsg = kind === "pair" ? "Isi alamat dan kode pairing dulu." : "Isi alamat sambung dulu.";
        state.wifiErr = true;
        setWifiMsg();
        return;
    }
    state.wifiMsg = "Memproses…";
    state.wifiErr = false;
    setWifiMsg();
    document.querySelectorAll<HTMLButtonElement>("#dopair, #doconn").forEach((b) => (b.disabled = true));
    try {
        const msg = kind === "pair"
            ? await invoke<string>("android_pair", { addr, code: v.pairCode.trim() })
            : await invoke<string>("android_connect", { addr });
        state.wifiMsg = msg;
        state.wifiErr = false;
        if (kind === "conn") void refreshDevices(true);
    } catch (e) {
        state.wifiMsg = String(e);
        state.wifiErr = true;
    }
    document.querySelectorAll<HTMLButtonElement>("#dopair, #doconn").forEach((b) => (b.disabled = false));
    setWifiMsg();
}

// ---------- Mirror layar ----------
// Rust hanya menyalurkan byte. Video: tiap paket = header 12 byte (PTS+flag, ukuran) + data H.264
// Annex B (send_frame_meta=true). Kontrol: pesan biner protokol scrcpy (big-endian).
const SCROLL_GAIN = 1 / 120; // makin besar makin cepat scroll roda mouse
type Bytes = Uint8Array<ArrayBuffer>;
const mirror = {
    on: false,
    token: 0,
    ws: null as WebSocket | null,
    dec: null as VideoDecoder | null,
    buf: new Uint8Array(0) as Bytes,
    config: null as Bytes | null,
    needKey: true,
    frame: null as VideoFrame | null,
    raf: 0,
    fw: 0, // ukuran video terakhir (dipakai untuk koordinat sentuh; harus sama dengan ukuran video server)
    fh: 0,
    down: false,
    synced: false, // layout awal aliran sudah dikenali
    failed: false, // sudah gagal mengenali: abaikan sisa data
};

const cat = (...a: Bytes[]): Bytes => {
    const o = new Uint8Array(a.reduce((n, x) => n + x.length, 0));
    let p = 0;
    for (const x of a) { o.set(x, p); p += x.length; }
    return o;
};

function positionMirror() {
    $("mirrorView").style.top = $("bar").offsetHeight + "px";
}
window.addEventListener("resize", positionMirror);

function setMirrorStatus(msg: string, err = false) {
    const el = $("mirrorStatus");
    el.textContent = msg;
    el.className = "mirror-status" + (err ? " err" : "");
}

function showMirror(on: boolean) {
    positionMirror();
    $("mirrorView").classList.toggle("on", on);
}

function resetStream() {
    if (mirror.raf) cancelAnimationFrame(mirror.raf);
    mirror.raf = 0;
    try { if (mirror.dec && mirror.dec.state !== "closed") mirror.dec.close(); } catch { /* sudah tertutup */ }
    mirror.dec = null;
    try { mirror.frame?.close(); } catch { /* ignore */ }
    mirror.frame = null;
    mirror.buf = new Uint8Array(0);
    mirror.config = null;
    mirror.needKey = true;
    mirror.fw = 0;
    mirror.fh = 0;
    mirror.down = false;
    mirror.synced = false;
    mirror.failed = false;
}

function codecFromConfig(d: Uint8Array): string | null {
    // cari NAL SPS (tipe 7) setelah start code 00 00 01; 3 byte sesudah header NAL = profile/constraint/level
    for (let i = 0; i + 6 < d.length; i++) {
        if (d[i] === 0 && d[i + 1] === 0 && d[i + 2] === 1 && (d[i + 3] & 0x1f) === 7) {
            const h = (n: number) => n.toString(16).padStart(2, "0");
            return `avc1.${h(d[i + 4])}${h(d[i + 5])}${h(d[i + 6])}`;
        }
    }
    return null;
}

function drawFrame() {
    mirror.raf = 0;
    const f = mirror.frame;
    if (!f) return;
    const c = $<HTMLCanvasElement>("mirrorCanvas");
    if (c.width !== f.displayWidth || c.height !== f.displayHeight) {
        c.width = f.displayWidth;
        c.height = f.displayHeight;
    }
    c.getContext("2d")?.drawImage(f, 0, 0);
    mirror.fw = f.displayWidth;
    mirror.fh = f.displayHeight;
    $("mirrorStatus").classList.add("hide");
}

function onFrame(f: VideoFrame) {
    try { mirror.frame?.close(); } catch { /* ignore */ }
    mirror.frame = f; // hanya frame terbaru yang digambar (frame lama dilewati kalau PC kewalahan)
    if (!mirror.raf) mirror.raf = requestAnimationFrame(drawFrame);
}

function configureDecoder(config: Uint8Array) {
    const codec = codecFromConfig(config);
    if (!codec) { setMirrorStatus("Paket konfigurasi video tidak dikenali (SPS tidak ditemukan).", true); return; }
    try { if (mirror.dec && mirror.dec.state !== "closed") mirror.dec.close(); } catch { /* ignore */ }
    const dec = new VideoDecoder({
        output: onFrame,
        error: (e) => { if (mirror.dec === dec) setMirrorStatus("Decoder error: " + e.message, true); },
    });
    // avc.format "annexb": SPS/PPS ikut di dalam data (tanpa avcC). Properti ini belum ada di tipe lib.dom.
    dec.configure({ codec, optimizeForLatency: true, avc: { format: "annexb" } } as VideoDecoderConfig);
    mirror.dec = dec;
    mirror.needKey = true;
}

function mirrorPacket(isConfig: boolean, isKey: boolean, pts: number, data: Bytes) {
    if (isConfig) { mirror.config = data; configureDecoder(data); return; }
    const dec = mirror.dec;
    if (!dec || dec.state !== "configured") return;
    if (mirror.needKey && !isKey) return;
    mirror.needKey = false;
    try {
        dec.decode(new EncodedVideoChunk({
            type: isKey ? "key" : "delta",
            timestamp: pts,
            data: isKey && mirror.config ? cat(mirror.config, data) : data,
        }));
    } catch (e) { setMirrorStatus("Gagal mendekode: " + String(e), true); }
}

const hex = (b: Uint8Array, n = 32) =>
    Array.from(b.subarray(0, n), (x) => x.toString(16).padStart(2, "0")).join(" ");

// Paket PERTAMA scrcpy harus paket konfigurasi: bit config menyala dan isinya diawali start code Annex B.
// Header paket scrcpy: 8 byte (bit63=config, bit62=key, sisanya PTS) + 4 byte ukuran, lalu data Annex B.
// function validFirstHeader(b: Uint8Array, o: number): boolean {
//     if (b.length < o + 16) return false;
//     const dv = new DataView(b.buffer, b.byteOffset + o, 12);
//     const hi = dv.getUint32(0);
//     const size = dv.getUint32(8);
//     if ((hi & 0x80000000) === 0) return false;        // paket pertama harus config
//     if ((hi & 0x40000000) !== 0) return false;        // config tidak pernah key
//     if (size === 0 || size > 1024 * 1024) return false;
//     const p = o + 12;
//     return b[p] === 0 && b[p + 1] === 0 && (b[p + 2] === 1 || (b[p + 2] === 0 && b[p + 3] === 1));
// }

function mirrorFeed(chunk: Bytes) {
    if (mirror.failed) return;
    mirror.buf = mirror.buf.length ? cat(mirror.buf, chunk) : chunk;

    // 4 byte pertama: codec id ("h264"); setelah itu aliran paket
    if (!mirror.synced) {
        if (mirror.buf.length < 4) return;
        const b = mirror.buf;
        const id = String.fromCharCode(b[0], b[1], b[2], b[3]);
        if (id !== "h264") {
            mirror.failed = true;
            setMirrorStatus("Codec video tidak didukung / format tidak dikenali. Byte awal: " + hex(b, 32), true);
            mirror.buf = new Uint8Array(0) as Bytes;
            return;
        }
        mirror.buf = b.subarray(4);
        mirror.synced = true;
    }

    while (mirror.buf.length >= 12) {
        const b = mirror.buf;
        const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
        const hi = dv.getUint32(0);

        // paket sesi (bit 31 menyala): 4 byte flag + lebar + tinggi, tanpa data
        if ((hi & 0x80000000) !== 0) {
            mirror.fw = dv.getUint32(4);
            mirror.fh = dv.getUint32(8);
            mirror.buf = b.subarray(12);
            continue;
        }

        const size = dv.getUint32(8);
        if (size > 32 * 1024 * 1024) {
            mirror.failed = true;
            setMirrorStatus("Aliran video tidak sinkron. Byte di posisi paket: " + hex(b, 32), true);
            mirror.buf = new Uint8Array(0) as Bytes;
            return;
        }
        if (b.length < 12 + size) break;

        const lo = dv.getUint32(4);
        const payload = b.slice(12, 12 + size);
        mirror.buf = b.subarray(12 + size);
        mirrorPacket(
            (hi & 0x40000000) !== 0,                         // config
            (hi & 0x20000000) !== 0,                         // key frame
            (hi & 0x1fffffff) * 4294967296 + lo,             // PTS (61 bit)
            payload,
        );
    }
}

async function mirrorStart() {
    const dev = ready();
    if (!dev || mirror.on) return;
    if (typeof (globalThis as { VideoDecoder?: unknown }).VideoDecoder === "undefined") {
        const op = addOp("Mirror layar");
        op.state = "error";
        op.detail = "WebView ini tidak mendukung WebCodecs (VideoDecoder). Mirror butuh WebView2 di Windows; WebKitGTK di Linux umumnya belum mendukung.";
        render();
        return;
    }
    const tok = ++mirror.token;
    mirror.on = true;
    resetStream();
    showMirror(true);
    setMirrorStatus("Menyambung ke HP…");
    try {
        const info = await invoke<MirrorInfo>("android_mirror_start", { serial: dev.serial });
        if (tok !== mirror.token) { void invoke("android_mirror_stop").catch(() => { }); return; }
        const ws = new WebSocket(`ws://127.0.0.1:${info.ws_port}`);
        ws.binaryType = "arraybuffer";
        ws.onopen = () => { if (tok === mirror.token) setMirrorStatus("Menunggu video dari HP…"); };
        ws.onmessage = (ev) => { if (tok === mirror.token) mirrorFeed(new Uint8Array(ev.data as ArrayBuffer)); };
        ws.onerror = () => {
            if (tok === mirror.token) setMirrorStatus("Gagal membuka WebSocket lokal. Jika tauri.conf.json punya CSP, tambahkan connect-src ws://127.0.0.1:*", true);
        };
        ws.onclose = () => { if (tok === mirror.token && mirror.on) setMirrorStatus("Koneksi mirror terputus. Tutup lalu buka lagi.", true); };
        mirror.ws = ws;
        $("mirrorCanvas").focus();
    } catch (e) {
        if (tok === mirror.token) setMirrorStatus(String(e), true);
    }
}

function mirrorStop() {
    const was = mirror.on;
    mirror.token++;
    mirror.on = false;
    try { mirror.ws?.close(); } catch { /* ignore */ }
    mirror.ws = null;
    resetStream();
    showMirror(false);
    if (was) void invoke("android_mirror_stop").catch(() => { });
}

// --- kontrol (protokol scrcpy, big-endian) ---
function mirrorSend(data: ArrayBuffer | Bytes) {
    if (mirror.ws && mirror.ws.readyState === WebSocket.OPEN) mirror.ws.send(data);
}

// titik di canvas -> koordinat piksel video (canvas memakai object-fit: contain, jadi ada letterbox)
function mapPoint(e: { clientX: number; clientY: number }, clamp: boolean): { x: number; y: number } | null {
    if (!mirror.fw || !mirror.fh) return null;
    const r = $("mirrorCanvas").getBoundingClientRect();
    const s = Math.min(r.width / mirror.fw, r.height / mirror.fh);
    const dw = mirror.fw * s, dh = mirror.fh * s;
    let x = (e.clientX - (r.left + (r.width - dw) / 2)) / s;
    let y = (e.clientY - (r.top + (r.height - dh) / 2)) / s;
    if (!clamp && (x < 0 || y < 0 || x >= mirror.fw || y >= mirror.fh)) return null;
    x = Math.max(0, Math.min(mirror.fw - 1, x));
    y = Math.max(0, Math.min(mirror.fh - 1, y));
    return { x: Math.round(x), y: Math.round(y) };
}

// INJECT_TOUCH_EVENT (32 byte). action: 0 down, 1 up, 2 move. pointerId -2 = jari generik.
function mirrorTouch(action: 0 | 1 | 2, e: PointerEvent) {
    const p = mapPoint(e, action !== 0);
    if (!p) return;
    const v = new DataView(new ArrayBuffer(32));
    v.setUint8(0, 2);
    v.setUint8(1, action);
    v.setUint32(2, 0xffffffff);
    v.setUint32(6, 0xfffffffe);
    v.setInt32(10, p.x);
    v.setInt32(14, p.y);
    v.setUint16(18, mirror.fw);
    v.setUint16(20, mirror.fh);
    v.setUint16(22, action === 1 ? 0 : 0xffff);
    v.setInt32(24, 0);
    v.setInt32(28, 0);
    mirrorSend(v.buffer);
}

// INJECT_SCROLL_EVENT (21 byte)
function mirrorScroll(e: WheelEvent) {
    const p = mapPoint(e, false);
    if (!p) return;
    const fp = (n: number) => Math.round(Math.max(-1, Math.min(1, n)) * 0x7fff);
    const v = new DataView(new ArrayBuffer(21));
    v.setUint8(0, 3);
    v.setInt32(1, p.x);
    v.setInt32(5, p.y);
    v.setUint16(9, mirror.fw);
    v.setUint16(11, mirror.fh);
    v.setInt16(13, fp(-e.deltaX * SCROLL_GAIN));
    v.setInt16(15, fp(-e.deltaY * SCROLL_GAIN));
    v.setInt32(17, 0);
    mirrorSend(v.buffer);
}

// INJECT_KEYCODE (14 byte): tekan lalu lepas
function mirrorKey(code: number) {
    for (const action of [0, 1]) {
        const v = new DataView(new ArrayBuffer(14));
        v.setUint8(0, 0);
        v.setUint8(1, action);
        v.setInt32(2, code);
        v.setInt32(6, 0);
        v.setInt32(10, 0);
        mirrorSend(v.buffer);
    }
}

// BACK_OR_SCREEN_ON (2 byte): Kembali; kalau layar mati, menyalakannya
function mirrorBack() {
    for (const action of [0, 1]) mirrorSend(new Uint8Array([4, action]));
}

// INJECT_TEXT
function mirrorText(text: string) {
    const enc = new TextEncoder().encode(text);
    const b = new Uint8Array(5 + enc.length);
    new DataView(b.buffer).setUint8(0, 1);
    new DataView(b.buffer).setUint32(1, enc.length);
    b.set(enc, 5);
    mirrorSend(b);
}

const KEYMAP: Record<string, number> = {
    Enter: 66, Backspace: 67, Delete: 112, Tab: 61,
    ArrowUp: 19, ArrowDown: 20, ArrowLeft: 21, ArrowRight: 22,
};

{
    const c = $<HTMLCanvasElement>("mirrorCanvas");
    c.addEventListener("contextmenu", (e) => e.preventDefault());
    c.addEventListener("pointerdown", (e) => {
        if (!e.isPrimary) return;
        e.preventDefault();
        c.focus();
        if (e.button === 2) { mirrorBack(); return; }
        if (e.button === 1) { mirrorKey(3); return; }
        if (e.button !== 0) return;
        if (!mapPoint(e, false)) return;
        c.setPointerCapture(e.pointerId);
        mirror.down = true;
        mirrorTouch(0, e);
    });
    c.addEventListener("pointermove", (e) => {
        if (mirror.down && e.isPrimary) mirrorTouch(2, e);
    });
    const up = (e: PointerEvent) => {
        if (!mirror.down || !e.isPrimary) return;
        mirror.down = false;
        mirrorTouch(1, e);
    };
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);
    c.addEventListener("wheel", (e) => { e.preventDefault(); mirrorScroll(e); }, { passive: false });
    c.addEventListener("keydown", (e) => {
        if (e.ctrlKey || e.altKey || e.metaKey || e.key === "Escape") return; // Esc & Ctrl+M ditangani global
        const code = KEYMAP[e.key];
        if (code !== undefined) { e.preventDefault(); mirrorKey(code); }
        else if (e.key.length === 1) { e.preventDefault(); mirrorText(e.key); }
    });
}

// ---------- dipanggil dari Rust (lewat eval) ----------
const w = window as unknown as { __androidReset?: () => void; __androidShown?: () => void };

w.__androidReset = () => {
    state.active = false;
    state.ops = [];
    state.wifiOpen = false;
    mirrorStop();
    $("dragOverlay").classList.remove("over");
};

w.__androidShown = () => {
    state.active = true;
    const win = document.querySelector<HTMLElement>(".win");
    if (win) { win.style.animation = "none"; void win.offsetWidth; win.style.animation = ""; }
    void checkAdb();
};

// ---------- event ----------
const closeWin = () => void invoke("android_close");
document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
        if (mirror.on) mirrorStop();
        else void invoke("android_escape");
        return;
    }
    if (e.ctrlKey && !e.altKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        if (mirror.on) mirrorStop(); else void mirrorStart();
    }
});
// Enter di input Wi-Fi = tombol yang bersangkutan
document.addEventListener("keydown", (e) => {
    if (e.key !== "Enter") return;
    const id = (e.target as HTMLElement).id;
    if (id === "pairAddr" || id === "pairCode") void wifi("pair");
    else if (id === "connAddr") void wifi("conn");
});
// simpan isi input Wi-Fi ke state, karena render() membangun ulang DOM
document.addEventListener("input", (e) => {
    const t = e.target as HTMLInputElement;
    if (t.id in state.wifiVals) state.wifiVals[t.id] = t.value;
});
$("close").addEventListener("click", closeWin);
$("minimize").addEventListener("click", () => void invoke("android_minimize"));
$("maximize").addEventListener("click", () => void invoke("android_toggle_maximize"));

// klik tombol mirror tidak boleh mencuri fokus dari canvas (supaya keyboard tetap terkirim ke HP)
document.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest(".mirror-rail, .mirror-nav")) e.preventDefault();
});

function syncMax() {
    const full =
        window.innerWidth >= screen.availWidth - 40 &&
        window.innerHeight >= screen.availHeight - 40;
    document.documentElement.classList.toggle("maxed", full);
}
window.addEventListener("resize", syncMax);
syncMax();
$("bar").addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    // klik kedua dari dobel-klik: start_dragging() di klik pertama menyerahkan mouse ke OS,
    // jadi event "dblclick" normal tidak pernah sampai -> deteksi lewat e.detail di sini.
    if (e.detail >= 2) { void invoke("android_toggle_fullscreen"); return; }
    void invoke("android_drag");
});

document.addEventListener("click", async (e) => {
    const t = e.target as HTMLElement;

    const mk = (t.closest("[data-mkey]") as HTMLElement | null)?.dataset.mkey;
    if (mk) { if (mk === "back") mirrorBack(); else mirrorKey(Number(mk)); return; }

    const go = t.closest("[data-go]") as HTMLElement | null;
    if (go?.dataset.go) { state.path = go.dataset.go; void loadDir(); return; }
    const dev = t.closest("[data-dev]") as HTMLElement | null;
    if (dev?.dataset.dev) {
        const d = state.devices.find((x) => x.serial === dev.dataset.dev);
        if (d?.state === "device") { state.serial = d.serial; state.path = "/sdcard"; void loadDir(); }
        return;
    }
    const btn = t.closest("button") as HTMLButtonElement | null;
    if (!btn) return;
    if (btn.id === "retry") void checkAdb();
    else if (btn.id === "refresh") { void refreshDevices(true); void loadDir(); }
    else if (btn.id === "send") await push(await invoke<string[]>("android_pick_files"));
    else if (btn.id === "mirror") void mirrorStart();
    else if (btn.id === "mirrorStop") mirrorStop();
    else if (btn.id === "wifitoggle") { state.wifiOpen = !state.wifiOpen; render(); }
    else if (btn.id === "dopair") void wifi("pair");
    else if (btn.id === "doconn") void wifi("conn");
    else if (btn.dataset.pull) void pull(btn.dataset.pull);
    else if (btn.dataset.reveal) void invoke("android_reveal", { path: btn.dataset.reveal });
});

getCurrentWebview()
    .onDragDropEvent((event: { payload: unknown }) => {
        const p = event.payload as { type: string; paths?: string[] };
        const overlay = $("dragOverlay");
        if (!ready() || mirror.on) return;
        if (p.type === "enter" || p.type === "over") overlay.classList.add("over");
        else if (p.type === "leave") overlay.classList.remove("over");
        else if (p.type === "drop") { overlay.classList.remove("over"); void push(p.paths ?? []); }
    })
    .catch((e) => console.error("onDragDropEvent gagal (cek capabilities):", e));

state.active = true;
startPoll();
void checkAdb();