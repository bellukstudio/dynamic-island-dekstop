// Jendela Compress (compress.html): drag & drop / pilih file, atur persen kompresi, lihat estimasi, kompres.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

type Kind = "image" | "pdf" | "office" | "other";
interface FileInfo { path: string; name: string; size: number; kind: Kind; ext: string }
interface Estimate {
    orig: number; size: number; saved_pct: number;
    reachable: boolean; no_gain: boolean; label: string; out_ext: string;
}
interface RunResult { path: string; out_path: string; orig: number; size: number; saved_pct: number; status: "ok" | "no_gain" }
interface Item extends FileInfo {
    est?: Estimate;
    loading: boolean;
    error?: string;
    result?: RunResult;
    seen?: boolean; // animasi masuk hanya sekali
}

const PRESETS: [string, number][] = [["Ringan", 30], ["Seimbang", 50], ["Tinggi", 70], ["Maksimal", 85]];
// Jumlah file yang diestimasi bersamaan. Membatasi ini mencegah banyak proses kompresi berat
// (encode gambar / re-zip dokumen) berebut CPU sekaligus saat banyak file ditambahkan atau
// slider digeser — itulah sumber lag utama saat banyak file dipilih.
const ESTIMATE_CONCURRENCY = 3;

const state = {
    items: [] as Item[],
    percent: 50,
    phase: "idle" as "idle" | "running" | "done",
    current: -1,
    token: 0,
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
const badgeText = (it: FileInfo) => (it.ext || "FILE").slice(0, 4).toUpperCase();
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const ICON = {
    logo: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7"/></svg>',
    drop: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>',
    arrow: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>',
    check: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
    folder: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
    upload: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>',
};

// ---------- kerangka UI ----------
$("app").innerHTML = `
<div class="win">
  <header class="bar" id="bar">
    <div class="brand">
      <div class="logo">${ICON.logo}</div>
      <div><h1>Compress</h1><p>Kecilkan ukuran file, lihat estimasinya langsung</p></div>
    </div>
    <button class="x" id="close" aria-label="Tutup">×</button>
  </header>
  <main id="main"></main>
  <footer>
    <span class="hint"><kbd>Esc</kbd> tutup · <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>K</kbd> buka/tutup</span>
    <div class="acts" id="acts"></div>
  </footer>
  <div class="overlay" id="dragOverlay"><div class="overlay-card">${ICON.upload}<span>Lepas untuk menambah file</span></div></div>
</div>`;

// ---------- render (dijadwalkan lewat rAF agar banyak update berurutan tidak memicu banyak
// pembangunan ulang DOM penuh secara beruntun — ini penyebab utama UI terasa lag/patah saat
// banyak estimasi file selesai hampir bersamaan) ----------
let renderScheduled = false;
function scheduleRender() {
    if (renderScheduled) return;
    renderScheduled = true;
    requestAnimationFrame(() => { renderScheduled = false; render(); });
}

// ---------- render ----------
function totals() {
    let orig = 0, est = 0, unreachable = 0, pending = 0;
    for (const it of state.items) {
        orig += it.size;
        if (it.est) {
            est += it.est.size;
            if (!it.est.reachable && !it.est.no_gain) unreachable++;
        } else if (!it.error) {
            est += it.size;
            pending++;
        } else {
            est += it.size;
        }
    }
    return { orig, est, unreachable, pending };
}

function rowHtml(it: Item, i: number): string {
    const r = it.result;
    const running = state.phase === "running" && state.current === i;
    let to = "", pill = "", sub = "";
    if (r) {
        to = `<span class="to">${fmt(r.size)}</span>`;
        pill = r.status === "ok" ? `<span class="pill">−${Math.round(r.saved_pct)}%</span>` : `<span class="pill none">Sama</span>`;
        sub = r.status === "ok" ? "Selesai · disimpan di samping file asli" : "Tidak bisa dikecilkan lagi · dilewati";
    } else if (it.error) {
        to = `<span class="to">—</span>`;
        pill = `<span class="pill bad">Gagal</span>`;
        sub = `<span class="sub bad">${esc(it.error)}</span>`;
    } else if (it.est && !it.loading) {
        const e = it.est;
        to = `<span class="to">${fmt(e.size)}</span>`;
        if (e.no_gain) { pill = `<span class="pill none">0%</span>`; sub = "Sudah optimal, tidak bisa lebih kecil"; }
        else if (!e.reachable) { pill = `<span class="pill warn">−${Math.round(e.saved_pct)}%</span>`; sub = `Target tak tercapai · ${esc(e.label)}`; }
        else { pill = `<span class="pill">−${Math.round(e.saved_pct)}%</span>`; sub = e.label; }
    } else {
        to = `<span class="skel"></span>`;
        pill = `<span class="pill none">…</span>`;
        sub = "Menghitung estimasi…";
    }
    const subCls = !r && !it.error && it.est && !it.loading && !it.est.reachable && !it.est.no_gain ? "sub warn" : "sub";
    const subHtml = it.error ? sub : `<div class="${subCls}">${sub}</div>`;
    const anim = it.seen ? ' style="animation:none"' : "";
    it.seen = true;
    return `
    <div class="row${running ? " cur" : ""}"${anim}>
      <div class="badge k-${it.kind}">${esc(badgeText(it))}</div>
      <div class="meta"><div class="nm" title="${esc(it.path)}">${esc(it.name)}</div>${subHtml}</div>
      <div class="sz"><span class="from">${fmt(it.size)}</span>${ICON.arrow}${to}</div>
      ${pill}
      <button class="rm" data-rm="${i}" aria-label="Hapus dari daftar" ${state.phase === "running" ? "hidden" : ""}>×</button>
      ${running ? '<div class="prog"><i></i></div>' : ""}
    </div>`;
}

function render() {
    const main = $("main");
    const acts = $("acts");
    const has = state.items.length > 0;

    if (state.phase === "done") {
        const ok = state.items.filter((i) => i.result?.status === "ok");
        const before = ok.reduce((a, i) => a + i.size, 0);
        const after = ok.reduce((a, i) => a + (i.result?.size ?? 0), 0);
        const saved = before - after;
        main.innerHTML = `
        <div class="done">
          <div class="ok">${ICON.check}</div>
          <h2>${ok.length ? "Selesai dikompres" : "Tidak ada yang bisa dikecilkan"}</h2>
          ${ok.length
                ? `<p>Hemat <b>${fmt(saved)}</b> (${before ? Math.round((saved / before) * 100) : 0}%) dari ${ok.length} file · ${fmt(before)} → ${fmt(after)}</p>
               <p style="font-size:12px">Hasil disimpan di folder yang sama dengan akhiran <b style="color:inherit">-compressed</b></p>`
                : `<p>File-file ini sudah optimal atau tidak bisa dikompres lebih jauh.</p>`}
        </div>`;
        acts.innerHTML = `
          ${ok.length ? `<button class="btn" id="reveal">${ICON.folder}Buka folder</button>` : ""}
          <button class="btn" id="again">Kompres lagi</button>
          <button class="btn primary" id="finish">Selesai</button>`;
        return;
    }

    if (!has) {
        main.innerHTML = `
        <section class="drop" id="drop">
          <div class="drop-ico">${ICON.drop}</div>
          <h2>Seret file ke sini</h2>
          <p>atau pilih dari komputer — banyak file sekaligus</p>
          <button class="btn primary" id="pick" style="margin-top:6px">${ICON.plus}Pilih file</button>
          <div class="chips"><span class="chip">JPG · PNG</span><span class="chip">PDF</span><span class="chip">DOCX · XLSX · PPTX</span><span class="chip">File lain → ZIP</span></div>
        </section>`;
        acts.innerHTML = "";
        return;
    }

    const t = totals();
    const afterPct = t.orig ? Math.max(3, Math.min(100, (t.est / t.orig) * 100)) : 100;
    const savedBytes = t.orig - t.est;
    const savedPct = t.orig ? Math.round((savedBytes / t.orig) * 100) : 0;
    const running = state.phase === "running";

    main.innerHTML = `
      <section class="list" id="list">${state.items.map(rowHtml).join("")}</section>
      <section class="panel">
        <div class="lvl">
          <div><div class="lbl">Tingkat kompresi (pengurangan ukuran)</div><div class="pct"><span id="pct">${state.percent}</span><small>%</small></div></div>
          <div class="presets">${PRESETS.map(([n, v]) => `<button class="preset${state.percent === v ? " on" : ""}" data-p="${v}" ${running ? "disabled" : ""}>${n}</button>`).join("")}</div>
        </div>
        <input type="range" id="range" min="10" max="90" step="5" value="${state.percent}" style="--p:${((state.percent - 10) / 80) * 100}%" ${running ? "disabled" : ""}/>
        <div class="scale"><span>Kualitas tinggi</span><span>File lebih kecil</span></div>
        <div class="sum">
          <div class="bars">
            <div class="bar-row"><span>Sebelum</span><div class="track"><div class="fill before"></div></div></div>
            <div class="bar-row"><span>Sesudah</span><div class="track"><div class="fill after" id="afterbar" style="width:${afterPct}%"></div></div></div>
          </div>
          <div class="est">
            <div class="big${t.pending ? " dim" : ""}">${t.pending ? "…" : "≈ " + fmt(t.est)}</div>
            <div class="cap">dari ${fmt(t.orig)}${t.pending ? "" : ` · hemat <b>${fmt(Math.max(0, savedBytes))}</b> (${Math.max(0, savedPct)}%)`}</div>
          </div>
        </div>
        ${t.unreachable ? `<div class="note">${t.unreachable} file tidak bisa mencapai ${state.percent}% — dikompres semaksimal mungkin.</div>` : ""}
      </section>`;

    acts.innerHTML = `
      <button class="btn" id="add" ${running ? "disabled" : ""}>${ICON.plus}Tambah file</button>
      <button class="btn primary" id="go" ${running || t.pending ? "disabled" : ""}>${running ? "Mengompres…" : `Kompres ${state.items.length} file`}</button>`;
}

// ---------- estimasi ----------
let debounce = 0;
function refreshEstimates(delay = 0) {
    window.clearTimeout(debounce);
    const token = ++state.token;
    for (const it of state.items) { it.loading = true; it.error = undefined; }
    render();
    debounce = window.setTimeout(() => { void runEstimateQueue(token); }, delay);
}

// Memproses estimasi dengan concurrency terbatas (bukan menembak semua file sekaligus) dan
// menjadwalkan render lewat rAF: dua perubahan ini bersama-sama yang membuat daftar file besar
// tidak lagi membuat jendela terasa macet saat slider digeser atau banyak file baru ditambahkan.
async function runEstimateQueue(token: number) {
    const queue = state.items.slice();
    const worker = async () => {
        while (queue.length) {
            if (token !== state.token) return;
            const it = queue.shift();
            if (!it) return;
            try {
                const e = await invoke<Estimate>("compress_estimate", { path: it.path, percent: state.percent });
                if (token !== state.token) return;
                it.est = e;
            } catch (err) {
                if (token !== state.token) return;
                it.error = String(err);
            } finally {
                if (token === state.token) { it.loading = false; scheduleRender(); }
            }
        }
    };
    const workers = Array.from({ length: Math.min(ESTIMATE_CONCURRENCY, queue.length) }, worker);
    await Promise.all(workers);
}

// ---------- aksi ----------
async function addPaths(paths: string[]) {
    if (state.phase === "running" || !paths.length) return;
    if (state.phase === "done") resetAll();
    try {
        const infos = await invoke<FileInfo[]>("compress_file_info", { paths });
        const known = new Set(state.items.map((i) => i.path));
        for (const f of infos) if (!known.has(f.path)) state.items.push({ ...f, loading: true });
        refreshEstimates();
    } catch (e) {
        console.error("compress_file_info gagal:", e);
    }
}

function resetAll() {
    state.items = [];
    state.phase = "idle";
    state.current = -1;
    state.token++;
}

async function pickFiles() {
    const paths = await invoke<string[]>("compress_pick_files");
    await addPaths(paths);
}

async function runAll() {
    if (state.phase === "running") return;
    state.phase = "running";
    for (let i = 0; i < state.items.length; i++) {
        state.current = i;
        render();
        const it = state.items[i];
        try {
            it.result = await invoke<RunResult>("compress_run_one", { path: it.path, percent: state.percent });
        } catch (e) {
            it.error = String(e);
        }
    }
    state.current = -1;
    state.phase = "done";
    render();
}

// ---------- dipanggil dari Rust (lewat eval) saat jendela disembunyikan / ditampilkan ----------
// Jendela Compress tidak dihancurkan saat ditutup, hanya di-hide, jadi UI harus direset manual.
// Memakai eval (bukan event) supaya tidak bergantung pada capability event.
const w = window as unknown as { __compressReset?: () => void; __compressShown?: () => void };

w.__compressReset = () => {
    if (state.phase === "running") return; // jangan ganggu proses yang sedang berjalan
    resetAll();
    $("dragOverlay").classList.remove("over");
    render();
};

w.__compressShown = () => {
    // ulang animasi masuk setiap kali jendela ditampilkan lagi
    const win = document.querySelector<HTMLElement>(".win");
    if (!win) return;
    win.style.animation = "none";
    void win.offsetWidth;
    win.style.animation = "";
};

// ---------- event ----------
const closeWin = () => void invoke("compress_close");

document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeWin(); });
$("close").addEventListener("click", closeWin);
$("bar").addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void invoke("compress_drag");
});

document.addEventListener("click", (e) => {
    const t = e.target as HTMLElement;
    const btn = t.closest("button") as HTMLButtonElement | null;
    if (!btn) return;
    if (btn.id === "pick" || btn.id === "add") void pickFiles();
    else if (btn.id === "go") void runAll();
    else if (btn.id === "finish") closeWin();
    else if (btn.id === "again") { resetAll(); render(); }
    else if (btn.id === "reveal") {
        const first = state.items.find((i) => i.result?.status === "ok");
        if (first?.result) void invoke("compress_reveal", { path: first.result.out_path });
    } else if (btn.dataset.rm !== undefined) {
        state.items.splice(Number(btn.dataset.rm), 1);
        state.token++;
        render();
    } else if (btn.dataset.p) {
        state.percent = Number(btn.dataset.p);
        refreshEstimates(150);
    }
});

document.addEventListener("input", (e) => {
    const t = e.target as HTMLInputElement;
    if (t.id !== "range") return;
    state.percent = Number(t.value);
    // update ringan saat digeser; estimasi sungguhan dihitung setelah berhenti
    t.style.setProperty("--p", `${((state.percent - 10) / 80) * 100}%`);
    $("pct").textContent = String(state.percent);
    document.querySelectorAll<HTMLElement>(".preset").forEach((b) => b.classList.toggle("on", Number(b.dataset.p) === state.percent));
});
document.addEventListener("change", (e) => {
    if ((e.target as HTMLElement).id === "range") refreshEstimates(120);
});

// drag & drop file dari File Explorer (Tauri memberi path lengkap). Overlay ini aktif di seluruh
// jendela — baik saat daftar masih kosong maupun saat sudah ada file — supaya menambah file lewat
// drag & drop tetap terlihat jelas kapan pun, bukan cuma di layar kosong awal.
// Catatan: listen() event butuh capability. Pastikan "compress" ada di "windows" pada
// src-tauri/capabilities/default.json, kalau tidak promise ini ditolak (sekarang terlihat di console).
getCurrentWebview()
    .onDragDropEvent((event: { payload: unknown }) => {
        const p = event.payload as { type: string; paths?: string[] };
        if (state.phase === "running") return;
        const overlay = $("dragOverlay");
        const zone = document.getElementById("drop");
        if (p.type === "enter" || p.type === "over") {
            overlay.classList.add("over");
            zone?.classList.add("over");
        } else if (p.type === "leave") {
            overlay.classList.remove("over");
            zone?.classList.remove("over");
        } else if (p.type === "drop") {
            overlay.classList.remove("over");
            zone?.classList.remove("over");
            void addPaths(p.paths ?? []);
        }
    })
    .catch((e) => console.error("onDragDropEvent gagal (cek capabilities):", e));

render();