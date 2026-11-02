// Jendela Share (share.html): bagikan file ke HP/PC lain lewat jaringan lokal (QR + browser).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

interface SharedFile { id: number; name: string; size: number; path: string }
interface ShareState {
    running: boolean; url: string | null; urls: string[]; qr: string | null; inbox: string; files: SharedFile[];
}
interface ShareEvent { kind: "received" | "failed" | "downloaded"; name: string; size?: number; path?: string; error?: string }
interface LanPeer { fp: string; name: string; addr: string; trusted: boolean }
interface LanPending { id: string; from_name: string; from_fp: string; sas: string; names: string[]; count: number; total: number; trusted: boolean }
interface LanState {
    enabled: boolean; running: boolean; name: string; fp: string; auto_accept: boolean;
    peers: LanPeer[]; trusted: { fp: string; name: string }[]; pending: LanPending[];
}
interface Transfer {
    id: string; dir: "in" | "out"; label: string; state: "wait" | "run" | "ok" | "err";
    done: number; total: number; count: number; sas?: string; detail: string; path?: string;
}
interface Received { name: string; size: number; path: string; ok: boolean; error?: string }

const state = {
    st: null as ShareState | null,
    received: [] as Received[],
    downloads: new Map<string, number>(), // nama -> berapa kali diunduh perangkat lain
    busy: false,
    copied: false,
    lan: null as LanState | null,
    transfers: [] as Transfer[],
};

// ---------- util ----------
const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
function fmt(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const u = ["KB", "MB", "GB"];
    let v = bytes / 1024, i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v.toLocaleString("id-ID", { maximumFractionDigits: v >= 100 ? 0 : 1 })} ${u[i]}`;
}
const extOf = (n: string) => (n.includes(".") ? n.split(".").pop()! : "file").slice(0, 4).toUpperCase();
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const ICON = {
    logo: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12v7a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-7M16 6l-4-4-4 4M12 2v13"/></svg>',
    drop: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>',
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>',
    folder: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
};

// ---------- kerangka UI ----------
$("app").innerHTML = `
<div class="win">
  <header class="bar" id="bar">
    <div class="brand">
      <div class="logo">${ICON.logo}</div>
      <div><h1>Share</h1><p>Kirim &amp; terima file lewat jaringan, tanpa aplikasi di HP</p></div>
    </div>
    <button class="x" id="close" aria-label="Tutup">×</button>
  </header>
  <main id="main"></main>
  <footer>
    <span class="hint"><kbd>Esc</kbd> tutup · <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>S</kbd> buka/tutup</span>
    <div class="acts" id="acts"></div>
  </footer>
  <div class="overlay" id="dragOverlay"><div class="overlay-card">${ICON.drop}<span>Lepas untuk membagikan file</span></div></div>
</div>`;

// ---------- render ----------
function fileRow(f: SharedFile): string {
    const n = state.downloads.get(f.name) ?? 0;
    return `
    <div class="row">
      <div class="badge k-other">${esc(extOf(f.name))}</div>
      <div class="meta"><div class="nm" title="${esc(f.path)}">${esc(f.name)}</div><div class="sub">${fmt(f.size)}${n ? ` · diunduh ${n}×` : ""}</div></div>
      <div class="sz"><span class="from plain">${fmt(f.size)}</span></div>
      <span class="pill none">Dibagikan</span>
      <button class="rm" data-rm="${f.id}" aria-label="Hentikan berbagi file ini">×</button>
    </div>`;
}

function recvRow(r: Received, i: number): string {
    return `
    <div class="row">
      <div class="badge k-other">${esc(extOf(r.name))}</div>
      <div class="meta"><div class="nm" title="${esc(r.path)}">${esc(r.name)}</div>
        <div class="${r.ok ? "sub" : "sub bad"}">${r.ok ? `${fmt(r.size)} · diterima` : esc(r.error ?? "Gagal")}</div></div>
      <div class="sz"><span class="from plain">${r.ok ? fmt(r.size) : "—"}</span></div>
      ${r.ok ? `<span class="pill">Masuk</span>` : `<span class="pill bad">Gagal</span>`}
      ${r.ok ? `<button class="act" data-reveal="${i}" aria-label="Tampilkan di folder">Buka</button>` : `<span></span>`}
    </div>`;
}

function render() {
    const st = state.st;
    const main = $("main");
    const acts = $("acts");
    if (!st) { main.innerHTML = ""; return; }

    let conn: string;
    if (!st.running) {
        conn = `
        <div class="share-intro">
          <p><b>Bagikan file ke HP atau PC lain.</b> Setelah dimulai, pindai QR dengan kamera HP (atau buka alamatnya di browser perangkat lain) — langsung bisa mengunggah file ke PC ini dan mengunduh file yang Anda bagikan.</p>
          <p class="note">PC dan perangkat lain harus berada di Wi-Fi/jaringan yang sama. Server hanya aktif selama jendela ini terbuka. Windows mungkin meminta izin firewall saat pertama kali — pilih <b>Jaringan pribadi</b>.</p>
        </div>`;
    } else if (st.url) {
        const alts = st.urls.slice(1).map((u) => `<div class="alt">${esc(u)}</div>`).join("");
        conn = `
        <div class="conn">
          <div class="qrbox">${st.qr ?? ""}</div>
          <div class="conn-info">
            <div class="note">Pindai QR dengan kamera HP, atau buka alamat ini di browser perangkat lain (klik untuk menyalin):</div>
            <div class="url${state.copied ? " copied" : ""}" id="url" title="Klik untuk menyalin">${esc(st.url)}</div>
            ${alts ? `<div class="note">Jika tidak tersambung, coba alamat lain:</div>${alts}` : ""}
            <div class="note">Koneksi tidak terenkripsi (HTTP) — gunakan hanya di jaringan tepercaya. Hanya yang memiliki alamat/QR ini yang bisa mengakses.</div>
          </div>
        </div>`;
    } else {
        conn = `<div class="empty-line">Tidak ada jaringan lokal terdeteksi. Sambungkan PC ke Wi-Fi/LAN lalu mulai lagi.</div>`;
    }

    const files = st.files.length
        ? `<section class="list">${st.files.map(fileRow).join("")}</section>`
        : `<div class="empty-line">Belum ada. Seret file ke jendela ini atau klik <b>Tambah file</b> untuk dibagikan ke perangkat lain.</div>`;
    const recv = state.received.length
        ? `<section class="list">${state.received.map(recvRow).join("")}</section>`
        : `<div class="empty-line">${st.running ? "Menunggu file dari perangkat lain…" : "File yang dikirim dari perangkat lain muncul di sini."}</div>`;

    main.innerHTML = `
      <div id="lanprompt"></div>
      <section class="panel">${conn}</section>
      <div class="sec-t">Dibagikan dari PC</div>${files}
      <div class="sec-t">Diterima${st.running ? ` · disimpan di ${esc(st.inbox)}` : ""}</div>${recv}
      <div class="sec-t">Perangkat Notch di sekitar (PC ke PC, terenkripsi)</div><div id="lan"></div>`;
    renderLan();

    acts.innerHTML = `
      <button class="btn" id="add">${ICON.plus}Tambah file</button>
      <button class="btn" id="inbox">${ICON.folder}Folder terima</button>
      ${st.running
            ? `<button class="btn primary" id="stop">Hentikan berbagi</button>`
            : `<button class="btn primary" id="start" ${state.busy ? "disabled" : ""}>${state.busy ? "Memulai…" : "Mulai berbagi"}</button>`}`;
}

// ---------- aksi ----------
async function refresh() {
    state.st = await invoke<ShareState>("share_state");
    state.lan = await invoke<LanState>("lan_state");
    render();
}

async function start() {
    state.busy = true;
    render();
    try {
        state.st = await invoke<ShareState>("share_start");
    } catch (e) {
        console.error("share_start gagal:", e);
        alert(String(e));
    }
    state.busy = false;
    render();
}

async function stop() {
    state.st = await invoke<ShareState>("share_stop");
    render();
}

async function addPaths(paths: string[]) {
    if (!paths.length) return;
    const files = await invoke<SharedFile[]>("share_add", { paths });
    if (state.st) state.st.files = files;
    render();
}

async function copyUrl() {
    const u = state.st?.url;
    if (!u) return;
    try {
        await navigator.clipboard.writeText(u);
    } catch {
        const r = document.createRange();
        r.selectNodeContents($("url"));
        const s = window.getSelection();
        s?.removeAllRanges();
        s?.addRange(r);
        document.execCommand("copy");
    }
    state.copied = true;
    render();
    setTimeout(() => { state.copied = false; render(); }, 1200);
}

// ---------- dipanggil dari Rust (lewat eval) saat jendela disembunyikan / ditampilkan ----------
const w = window as unknown as { __shareReset?: () => void; __shareShown?: () => void };

w.__shareReset = () => {
    state.received = [];
    state.downloads.clear();
    state.copied = false;
    state.transfers = [];
    $("dragOverlay").classList.remove("over");
    void refresh();
};

w.__shareShown = () => {
    const win = document.querySelector<HTMLElement>(".win");
    if (win) {
        win.style.animation = "none";
        void win.offsetWidth;
        win.style.animation = "";
    }
    void refresh();
};


// ---------- PC <-> PC (perangkat Notch lain) ----------
const pct = (t: Transfer) => (t.total > 0 ? Math.min(100, Math.round((t.done / t.total) * 100)) : 0);

function transferRow(t: Transfer): string {
    const stateTxt = t.state === "wait" ? (t.dir === "out" ? "Menunggu persetujuan" : "Menyiapkan") : t.state === "run" ? `${pct(t)}%` : t.state === "ok" ? "Selesai" : "Gagal";
    const sub = t.state === "err" ? esc(t.detail) : `${t.count} item · ${fmt(t.total)}${t.sas && t.state === "wait" ? ` · kode ${esc(t.sas)}` : ""}`;
    return `
    <div class="row" id="tr-${t.id}">
      <div class="badge k-other">${t.dir === "out" ? "KIRIM" : "TERIMA"}</div>
      <div class="meta"><div class="nm">${esc(t.label)}</div><div class="${t.state === "err" ? "sub bad" : "sub"}" id="trs-${t.id}">${sub}</div>
        ${t.state === "run" || t.state === "wait" ? `<div class="tbar"><i id="trb-${t.id}" style="width:${pct(t)}%"></i></div>` : ""}</div>
      <div class="sz"><span class="from plain" id="trp-${t.id}">${t.state === "run" ? fmt(t.done) : ""}</span></div>
      <span class="pill ${t.state === "ok" ? "" : t.state === "err" ? "bad" : "none"}">${stateTxt}</span>
      ${t.path ? `<button class="act" data-lan-reveal="${esc(t.path)}">Buka</button>` : "<span></span>"}
    </div>`;
}

function renderLan() {
    const lan = state.lan;
    const box = document.getElementById("lan");
    const pbox = document.getElementById("lanprompt");
    if (!lan || !box || !pbox) return;

    pbox.innerHTML = lan.pending.map((p) => `
      <section class="panel lan-prompt">
        <div><b>${esc(p.from_name)}</b> ingin mengirim <b>${p.count}</b> file (${fmt(p.total)})${p.trusted ? " · perangkat tepercaya" : ""}</div>
        <div class="note">${p.names.map(esc).join(", ")}${p.count > p.names.length ? ", …" : ""}</div>
        <div class="note">Kode verifikasi — harus <b>sama</b> dengan yang tampil di perangkat pengirim; bila berbeda, tolak:</div>
        <div class="sas">${esc(p.sas)}</div>
        <div class="lan-btns">
          <button class="btn primary" data-lan-resp="${p.id}|1|0">Terima</button>
          ${p.trusted ? "" : `<button class="btn" data-lan-resp="${p.id}|1|1">Terima &amp; percayai</button>`}
          <button class="btn" data-lan-resp="${p.id}|0|0">Tolak</button>
        </div>
      </section>`).join("");

    // jangan bangun ulang saat pengguna sedang mengetik nama perangkat
    if (document.activeElement?.id === "lanName" && box.dataset.built) { renderTransfers(); return; }

    const peers = lan.peers.length
        ? `<section class="list">${lan.peers.map((p) => `
          <div class="row">
            <div class="badge k-other">PC</div>
            <div class="meta"><div class="nm">${esc(p.name)}</div><div class="sub">${esc(p.addr)}</div></div>
            <div class="sz"></div>
            <span class="pill ${p.trusted ? "" : "none"}">${p.trusted ? "Tepercaya" : "Baru"}</span>
            <span class="lan-btns"><button class="act" data-lan-send="${p.fp}">Kirim file…</button><button class="act" data-lan-folder="${p.fp}">Folder…</button></span>
          </div>`).join("")}</section>`
        : `<div class="empty-line">${lan.enabled ? "Mencari perangkat Notch lain di jaringan…" : "Aktifkan “Terlihat di jaringan” untuk mencari dan dikirimi file."}</div>`;

    const trusted = lan.trusted.length
        ? `<div class="note" style="margin-top:8px">Perangkat tepercaya: ${lan.trusted.map((t) => `${esc(t.name)} <a href="#" data-lan-untrust="${t.fp}">lupakan</a>`).join(" · ")}</div>`
        : "";

    box.dataset.built = "1";
    box.innerHTML = `
      <section class="panel">
        <label class="chk"><input type="checkbox" id="lanEnabled" ${lan.enabled ? "checked" : ""}/> Terlihat di jaringan &amp; bisa menerima dari Notch lain</label>
        <div class="lan-name"><span class="note">Nama perangkat ini</span><input id="lanName" value="${esc(lan.name)}" maxlength="40" /></div>
        <label class="chk"><input type="checkbox" id="lanAuto" ${lan.auto_accept ? "checked" : ""}/> Terima otomatis dari perangkat tepercaya</label>
        <div class="note">Terenkripsi (TLS 1.3). Setiap perangkat baru perlu disetujui di sini; kode verifikasi mencegah penyadapan. Sidik jari perangkat ini: ${esc(lan.fp.slice(0, 16))}…</div>
      </section>
      ${peers}${trusted}
      <div id="lantr"></div>`;
    renderTransfers();
}

function renderTransfers() {
    const el = document.getElementById("lantr");
    if (!el) return;
    el.innerHTML = state.transfers.length ? `<div class="sec-t">Transfer</div><section class="list">${state.transfers.slice(0, 6).map(transferRow).join("")}</section>` : "";
}

async function refreshLan() {
    state.lan = await invoke<LanState>("lan_state");
    renderLan();
}

function upsertTransfer(id: string, init: Partial<Transfer>): Transfer {
    let t = state.transfers.find((x) => x.id === id);
    if (!t) {
        t = { id, dir: "in", label: "Transfer", state: "wait", done: 0, total: 0, count: 0, detail: "", ...init };
        state.transfers.unshift(t);
    } else {
        Object.assign(t, init);
    }
    return t;
}

async function lanSend(fp: string, folder: boolean) {
    const paths = folder ? [await invoke<string | null>("lan_pick_folder")].filter(Boolean) as string[] : await invoke<string[]>("share_pick_files");
    if (!paths.length) return;
    try {
        const r = await invoke<{ id: string; to: string; count: number; total: number }>("lan_send", { fp, paths });
        upsertTransfer(r.id, { dir: "out", label: `Ke ${r.to}`, count: r.count, total: r.total });
    } catch (e) {
        upsertTransfer("err-" + Date.now(), { dir: "out", label: "Gagal mengirim", state: "err", detail: String(e) });
    }
    renderTransfers();
}

async function lanRespond(spec: string) {
    const [id, acc, trust] = spec.split("|");
    const p = state.lan?.pending.find((x) => x.id === id);
    state.lan = await invoke<LanState>("lan_respond", { id, accept: acc === "1", trust: trust === "1" });
    if (acc === "1" && p) upsertTransfer(id, { dir: "in", label: `Dari ${p.from_name}`, count: p.count, total: p.total, sas: p.sas });
    renderLan();
}

function onLanEvent(ev: { kind: string; id: string; dir: string; name?: string; done?: number; total?: number; count?: number; path?: string | null; error?: string }) {
    if (ev.kind === "progress") {
        const t = upsertTransfer(ev.id, { dir: ev.dir as "in" | "out", state: "run", done: ev.done ?? 0, total: ev.total ?? 0 });
        const bar = document.getElementById(`trb-${t.id}`);
        if (!bar) { renderTransfers(); return; } // baris belum menampilkan bar (mis. baru pindah dari "menunggu")
        bar.style.width = pct(t) + "%";
        const p = document.getElementById(`trp-${t.id}`);
        if (p) p.textContent = fmt(t.done);
        const sub = document.getElementById(`trs-${t.id}`);
        if (sub) sub.textContent = `${pct(t)}% · ${ev.name ?? ""}`;
    } else if (ev.kind === "finished") {
        upsertTransfer(ev.id, { dir: ev.dir as "in" | "out", state: "ok", done: ev.total ?? 0, total: ev.total ?? 0, count: ev.count ?? 0, path: ev.path ?? undefined });
        renderTransfers();
    } else if (ev.kind === "failed") {
        upsertTransfer(ev.id, { dir: ev.dir as "in" | "out", state: "err", detail: ev.error ?? "Gagal" });
        renderTransfers();
    }
}

// ---------- event ----------
const closeWin = () => void invoke("share_close");

document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeWin(); });
$("close").addEventListener("click", closeWin);
$("bar").addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void invoke("share_drag");
});

document.addEventListener("click", async (e) => {
    const t = e.target as HTMLElement;
    if (t.closest("#url")) { void copyUrl(); return; }
    const un = t.closest("[data-lan-untrust]") as HTMLElement | null;
    if (un?.dataset.lanUntrust) {
        e.preventDefault();
        state.lan = await invoke<LanState>("lan_untrust", { fp: un.dataset.lanUntrust });
        renderLan();
        return;
    }
    const btn = t.closest("button") as HTMLButtonElement | null;
    if (!btn) return;
    if (btn.dataset.lanSend !== undefined) { await lanSend(btn.dataset.lanSend, false); return; }
    if (btn.dataset.lanFolder !== undefined) { await lanSend(btn.dataset.lanFolder, true); return; }
    if (btn.dataset.lanResp !== undefined) { await lanRespond(btn.dataset.lanResp); return; }
    if (btn.dataset.lanReveal !== undefined) { void invoke("share_reveal", { path: btn.dataset.lanReveal }); return; }
    if (btn.id === "start") void start();
    else if (btn.id === "stop") void stop();
    else if (btn.id === "add") await addPaths(await invoke<string[]>("share_pick_files"));
    else if (btn.id === "inbox") void invoke("share_open_inbox");
    else if (btn.dataset.rm !== undefined) {
        const files = await invoke<SharedFile[]>("share_remove", { id: Number(btn.dataset.rm) });
        if (state.st) state.st.files = files;
        render();
    } else if (btn.dataset.reveal !== undefined) {
        const r = state.received[Number(btn.dataset.reveal)];
        if (r) void invoke("share_reveal", { path: r.path });
    }
});

document.addEventListener("change", async (e) => {
    const t = e.target as HTMLInputElement;
    try {
        if (t.id === "lanEnabled") state.lan = await invoke<LanState>("lan_set_enabled", { enabled: t.checked });
        else if (t.id === "lanAuto") state.lan = await invoke<LanState>("lan_set_auto_accept", { value: t.checked });
        else if (t.id === "lanName") state.lan = await invoke<LanState>("lan_set_name", { name: t.value });
        else return;
    } catch (err) {
        alert(String(err));
        await refreshLan();
        return;
    }
    renderLan();
});

listen("lan-changed", () => { void refreshLan(); }).catch((e) => console.error("listen lan-changed gagal (cek capabilities):", e));
listen<Parameters<typeof onLanEvent>[0]>("lan-event", (ev) => onLanEvent(ev.payload)).catch((e) => console.error("listen lan-event gagal:", e));
listen<{ id: string; sas: string }>("lan-sas", (ev) => {
    upsertTransfer(ev.payload.id, { sas: ev.payload.sas });
    renderTransfers();
}).catch((e) => console.error("listen lan-sas gagal:", e));

listen<ShareEvent>("share-event", (ev) => {
    const p = ev.payload;
    if (p.kind === "received") state.received.unshift({ name: p.name, size: p.size ?? 0, path: p.path ?? "", ok: true });
    else if (p.kind === "failed") state.received.unshift({ name: p.name, size: 0, path: "", ok: false, error: p.error });
    else if (p.kind === "downloaded") state.downloads.set(p.name, (state.downloads.get(p.name) ?? 0) + 1);
    render();
}).catch((e) => console.error("listen share-event gagal (cek capabilities):", e));

// drag & drop dari File Explorer (butuh "share" di "windows" pada capabilities/default.json)
getCurrentWebview()
    .onDragDropEvent((event: { payload: unknown }) => {
        const p = event.payload as { type: string; paths?: string[] };
        const overlay = $("dragOverlay");
        if (p.type === "enter" || p.type === "over") overlay.classList.add("over");
        else if (p.type === "leave") overlay.classList.remove("over");
        else if (p.type === "drop") {
            overlay.classList.remove("over");
            void addPaths(p.paths ?? []);
        }
    })
    .catch((e) => console.error("onDragDropEvent gagal (cek capabilities):", e));

void refresh();
