import { invoke } from "@tauri-apps/api/core";

// Popup ringkasan update (jendela "whats-new", dibuka oleh perintah Rust open_whats_new).
// Data dikirim lewat initialization_script: window.__WHATS_NEW__ = { version, notes }.
// Isi notes dari Firestore: satu baris = satu poin. Awalan "-", "*", atau "•" dibuang;
// baris berawalan "#" atau berakhiran ":" menjadi sub-judul. Semua isi dipasang lewat textContent.

type WhatsNew = { version?: string; notes?: string };

const data: WhatsNew = (window as unknown as { __WHATS_NEW__?: WhatsNew }).__WHATS_NEW__ ?? {};
const $ = (id: string) => document.getElementById(id) as HTMLElement;

$("wn-version").textContent = data.version ? `Versi ${data.version}` : "";

const list = $("wn-notes");
const lines = (data.notes ?? "").replace(/\\n/g, "\n").split(/\r?\n/);
for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    const row = document.createElement("div");
    if (/^#+\s*/.test(line)) {
        row.className = "sub";
        row.textContent = line.replace(/^#+\s*/, "");
    } else if (/^[-*•]\s+/.test(line)) {
        row.className = "item";
        row.textContent = line.replace(/^[-*•]\s+/, "");
    } else if (line.endsWith(":")) {
        row.className = "sub";
        row.textContent = line.slice(0, -1);
    } else {
        row.className = "item";
        row.textContent = line;
    }
    list.appendChild(row);
}
if (!list.childElementCount) {
    const empty = document.createElement("div");
    empty.className = "plain";
    empty.textContent = "Perbaikan dan peningkatan umum.";
    list.appendChild(empty);
}

const close = () => { void invoke("close_whats_new").catch(() => window.close()); };
$("wn-close").addEventListener("click", close);
window.addEventListener("keydown", (e) => { if (e.key === "Escape" || e.key === "Enter") close(); });
$("wn-close").focus();
