// Jendela Convert (convert.html): seret / pilih file, pilih format tujuan per file, konversi.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

type Kind = "image" | "pdf" | "office" | "other";
interface Info {
    path: string; name: string; size: number; kind: Kind; ext: string;
    targets: string[]; default_target: string; note: string | null;
    engine: string | null; // mesin Office -> PDF: "bawaan" | "LibreOffice"
}
interface ConvResult { path: string; out_path: string; count: number; size: number; engine: string }
interface Item extends Info {
    target: string;
    state: "idle" | "running" | "done" | "error";
    result?: ConvResult;
    error?: string;
    seen?: boolean; // animasi masuk hanya sekali
}

const TARGET_ORDER = ["png", "jpg", "webp", "bmp", "gif", "pdf"];
const DPI_PRESETS = [72, 100, 150, 200, 300];

const state = {
    items: [] as Item[],
    quality: 90,
    dpi: 150,
    phase: "idle" as "idle" | "running" | "done",
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
const badgeText = (it: Info) => (it.ext || "FILE").slice(0, 4).toUpperCase();
const label = (t: string) => t.toUpperCase();
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canConvert = (it: Item) => it.targets.length > 0 && !!it.target;
/** Keterangan mesin untuk dokumen Office ("" bila bukan Office / tidak ada). */
const engineNote = (engine: string | null | undefined) =>
    engine ? ` · mesin PDF ${engine === "bawaan" ? "bawaan" : engine}` : "";

const ICON = {
    logo: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3l4 4-4 4M21 7H8a4 4 0 0 0-4 4M7 21l-4-4 4-4M3 17h13a4 4 0 0 0 4-4"/></svg>',
    drop: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>',
    arrow: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>',
    check: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
    folder: '<svg viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
};

// ---------- kerangka UI ----------
$("app").innerHTML = `
<div class="win">
  <header class="bar" id="bar">
    <div class="brand">
      <div class="logo">${ICON.logo}</div>
      <div><h1>Convert</h1><p>Ubah format gambar, PDF, dan dokumen Office</p></div>
    </div>
    <button class="x" id="close" aria-label="Tutup">×</button>
  </header>
  <main id="main"></main>
  <footer>
    <span class="hint"><kbd>Esc</kbd> tutup · <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd> buka/tutup</span>
    <div class="acts" id="acts"></div>
  </footer>
  <div class="overlay" id="dragOverlay"><div class="overlay-card">${ICON.drop}<span>Lepas untuk menambah file</span></div></div>
</div>`;

// ---------- render ----------
function rowHtml(it: Item, i: number): string {
    const ext = (it.ext || "file").toUpperCase();
    const idle = state.phase === "idle";
    let sub = "", subCls = "sub", pill = "", mid = "";

    if (it.state === "done" && it.result) {
        const r = it.result;
        sub = `${r.count > 1 ? `${r.count} gambar · ` : ""}${fmt(r.size)}${engineNote(r.engine)} · disimpan di samping file asli`;
        pill = `<span class="pill">Selesai</span>`;
        mid = `<span class="from plain">${esc(ext)}</span>${ICON.arrow}<span class="to">${esc(label(it.target))}</span>`;
    } else if (it.state === "error") {
        sub = esc(it.error ?? "Gagal");
        subCls = "sub bad";
        pill = `<span class="pill bad">Gagal</span>`;
        mid = `<span class="from plain">${esc(ext)}</span>`;
    } else if (!canConvert(it)) {
        sub = esc(it.note ?? "Tidak didukung");
        subCls = "sub warn";
        pill = `<span class="pill warn">Tak didukung</span>`;
        mid = `<span class="from plain">${esc(ext)}</span>`;
    } else {
        sub = `${fmt(it.size)}${it.kind === "pdf" ? " · tiap halaman jadi satu gambar" : ""}${it.kind === "office" ? engineNote(it.engine) : ""}`;
        if (it.state === "running") { sub = "Mengonversi…"; pill = `<span class="pill none">…</span>`; }
        else pill = `<span class="pill none">Siap</span>`;
        mid = idle
            ? `<span class="from plain">${esc(ext)}</span>${ICON.arrow}<select class="sel" data-i="${i}" aria-label="Format tujuan">${it.targets
                .map((t) => `<option value="${t}"${t === it.target ? " selected" : ""}>${label(t)}</option>`)
                .join("")}</select>`
            : `<span class="from plain">${esc(ext)}</span>${ICON.arrow}<span class="to">${esc(label(it.target))}</span>`;
    }

    const anim = it.seen ? ' style="animation:none"' : "";
    it.seen = true;
    const running = it.state === "running";
    return `
    <div class="row${running ? " cur" : ""}"${anim}>
      <div class="badge k-${it.kind}">${esc(badgeText(it))}</div>
      <div class="meta"><div class="nm" title="${esc(it.path)}">${esc(it.name)}</div><div class="${subCls}">${sub}</div></div>
      <div class="sz">${mid}</div>
      ${pill}
      <button class="rm" data-rm="${i}" aria-label="Hapus dari daftar" ${state.phase === "running" ? "hidden" : ""}>×</button>
      ${running ? '<div class="prog"><i></i></div>' : ""}
    </div>`;
}

function render() {
    const main = $("main");
    const acts = $("acts");

    if (state.phase === "done") {
        const ok = state.items.filter((i) => i.state === "done");
        const failed = state.items.filter((i) => i.state === "error").length;
        const files = ok.reduce((a, i) => a + (i.result?.count ?? 0), 0);
        main.innerHTML = `
        <div class="done">
          <div class="ok">${ICON.check}</div>
          <h2>${ok.length ? "Selesai dikonversi" : "Tidak ada yang berhasil dikonversi"}</h2>
          ${ok.length
                ? `<p><b>${ok.length}</b> file menjadi ${files} berkas hasil${failed ? ` · ${failed} gagal` : ""}</p>
               <p style="font-size:12px">Hasil disimpan di folder yang sama dengan file asli</p>`
                : `<p>Periksa pesan galat pada tiap file, lalu coba lagi.</p>`}
        </div>`;
        acts.innerHTML = `
          ${ok.length ? `<button class="btn" id="reveal">${ICON.folder}Buka folder</button>` : ""}
          <button class="btn" id="again">Konversi lagi</button>
          <button class="btn primary" id="finish">Selesai</button>`;
        return;
    }

    if (!state.items.length) {
        main.innerHTML = `
        <section class="drop" id="drop">
          <div class="drop-ico">${ICON.drop}</div>
          <h2>Seret file ke sini</h2>
          <p>atau pilih dari komputer — banyak file sekaligus</p>
          <button class="btn primary" id="pick" style="margin-top:6px">${ICON.plus}Pilih file</button>
          <div class="chips"><span class="chip">JPG · PNG · WEBP · BMP · GIF</span><span class="chip">Gambar → PDF</span><span class="chip">PDF → Gambar</span><span class="chip">DOCX · XLSX · PPTX → PDF (tanpa LibreOffice)</span></div>
        </section>`;
        acts.innerHTML = "";
        return;
    }

    const running = state.phase === "running";
    const convertibles = state.items.filter(canConvert);
    const union = TARGET_ORDER.filter((t) => state.items.some((i) => i.targets.includes(t)));
    // slider kualitas hanya relevan untuk keluaran JPEG dari gambar / PDF yang dibuat dari gambar;
    // Office -> PDF tidak memakainya
    const needsQuality = convertibles.some((i) => i.kind !== "office" && (i.target === "jpg" || i.target === "pdf"));
    const hasPdf = convertibles.some((i) => i.kind === "pdf");
    const dis = running ? "disabled" : "";

    main.innerHTML = `
      <section class="list" id="list">${state.items.map(rowHtml).join("")}</section>
      <section class="panel">
        <div class="opt">
          <div class="lbl">Ubah semua ke</div>
          <div class="presets">${union.map((t) => `<button class="preset" data-all="${t}" ${dis}>${label(t)}</button>`).join("")}</div>
        </div>
        ${needsQuality ? `
        <div class="opt stack">
          <div class="opt-h"><div class="lbl">Kualitas JPEG (juga untuk PDF dari gambar)</div><div class="val"><span id="qv">${state.quality}</span>%</div></div>
          <input type="range" id="q" min="50" max="100" step="5" value="${state.quality}" style="--p:${((state.quality - 50) / 50) * 100}%" ${dis}/>
        </div>` : ""}
        ${hasPdf ? `
        <div class="opt">
          <div class="lbl">Resolusi halaman PDF (DPI)</div>
          <div class="presets">${DPI_PRESETS.map((d) => `<button class="preset${state.dpi === d ? " on" : ""}" data-dpi="${d}" ${dis}>${d}</button>`).join("")}</div>
        </div>` : ""}
      </section>`;

    acts.innerHTML = `
      <button class="btn" id="add" ${dis}>${ICON.plus}Tambah file</button>
      <button class="btn primary" id="go" ${running || !convertibles.length ? "disabled" : ""}>${running ? "Mengonversi…" : `Konversi ${convertibles.length} file`}</button>`;
}

// ---------- aksi ----------
async function addPaths(paths: string[]) {
    if (state.phase === "running" || !paths.length) return;
    if (state.phase === "done") resetAll();
    try {
        const infos = await invoke<Info[]>("convert_file_info", { paths });
        const known = new Set(state.items.map((i) => i.path));
        for (const f of infos) {
            if (!known.has(f.path)) state.items.push({ ...f, target: f.default_target, state: "idle" });
        }
        render();
    } catch (e) {
        console.error("convert_file_info gagal:", e);
    }
}

function resetAll() {
    state.items = [];
    state.phase = "idle";
}

async function pickFiles() {
    const paths = await invoke<string[]>("convert_pick_files");
    await addPaths(paths);
}

async function runAll() {
    if (state.phase === "running") return;
    state.phase = "running";
    for (const it of state.items) {
        if (!canConvert(it)) continue;
        it.state = "running";
        it.error = undefined;
        render();
        try {
            it.result = await invoke<ConvResult>("convert_run_one", {
                path: it.path, target: it.target, quality: state.quality, dpi: state.dpi,
            });
            it.state = "done";
        } catch (e) {
            it.error = String(e);
            it.state = "error";
        }
    }
    state.phase = "done";
    render();
}

// ---------- dipanggil dari Rust (lewat eval) saat jendela disembunyikan / ditampilkan ----------
const w = window as unknown as { __convertReset?: () => void; __convertShown?: () => void };

w.__convertReset = () => {
    if (state.phase === "running") return;
    resetAll();
    $("dragOverlay").classList.remove("over");
    render();
};

w.__convertShown = () => {
    const win = document.querySelector<HTMLElement>(".win");
    if (!win) return;
    win.style.animation = "none";
    void win.offsetWidth;
    win.style.animation = "";
};

// ---------- event ----------
const closeWin = () => void invoke("convert_close");

document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeWin(); });
$("close").addEventListener("click", closeWin);
$("bar").addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void invoke("convert_drag");
});

document.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button") as HTMLButtonElement | null;
    if (!btn) return;
    if (btn.id === "pick" || btn.id === "add") void pickFiles();
    else if (btn.id === "go") void runAll();
    else if (btn.id === "finish") closeWin();
    else if (btn.id === "again") { resetAll(); render(); }
    else if (btn.id === "reveal") {
        const first = state.items.find((i) => i.state === "done");
        if (first?.result) void invoke("convert_reveal", { path: first.result.out_path });
    } else if (btn.dataset.rm !== undefined) {
        state.items.splice(Number(btn.dataset.rm), 1);
        render();
    } else if (btn.dataset.all) {
        const t = btn.dataset.all;
        for (const it of state.items) if (it.targets.includes(t)) it.target = t;
        render();
    } else if (btn.dataset.dpi) {
        state.dpi = Number(btn.dataset.dpi);
        render();
    }
});

document.addEventListener("change", (e) => {
    const t = e.target;
    if (t instanceof HTMLSelectElement && t.dataset.i !== undefined) {
        state.items[Number(t.dataset.i)].target = t.value;
        render(); // slider kualitas muncul/hilang tergantung format yang dipilih
    }
});

document.addEventListener("input", (e) => {
    const t = e.target as HTMLInputElement;
    if (t.id !== "q") return;
    state.quality = Number(t.value);
    t.style.setProperty("--p", `${((state.quality - 50) / 50) * 100}%`);
    $("qv").textContent = String(state.quality);
});

// drag & drop dari File Explorer (butuh capability: "convert" ada di "windows" pada capabilities/default.json)
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