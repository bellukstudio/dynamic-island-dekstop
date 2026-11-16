import { invoke } from "@tauri-apps/api/core";

// Popup ringkasan update (jendela "whats-new", dibuka oleh perintah Rust open_whats_new).
// Data dikirim lewat initialization_script: window.__WHATS_NEW__ = { version, notes, mode, updateUrl }.
// Isi notes dari Firestore: satu baris = satu poin. Awalan "-", "*", atau "•" dibuang;
// baris berawalan "#" atau berakhiran ":" menjadi sub-judul. Semua isi dipasang lewat textContent.
//
// Dua mode (dipicu dari main-modules/updates.ts):
//  - "release" (bawaan): "Yang baru di Notch" — ringkasan versi yang BARU SAJA terpasang,
//    satu tombol "Mengerti".
//  - "reminder": "Pembaruan tersedia" — ringkasan versi yang BELUM terpasang, ditampilkan
//    berkala selagi update opsional belum diambil; tombol "Perbarui sekarang" (updateUrl)
//    + "Nanti".

type WhatsNew = { version?: string; notes?: string; mode?: "release" | "reminder"; updateUrl?: string | null };

const data: WhatsNew = (window as unknown as { __WHATS_NEW__?: WhatsNew }).__WHATS_NEW__ ?? {};
const $ = (id: string) => document.getElementById(id) as HTMLElement;
const isReminder = data.mode === "reminder";

$("wn-version").textContent = data.version ? `Versi ${data.version}` : "";
if (isReminder) {
    $("wn-badge").textContent = "⬆️";
    $("wn-title").textContent = "Pembaruan tersedia";
}

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

const updateButton = $("wn-update") as HTMLButtonElement;
const closeButton = $("wn-close") as HTMLButtonElement;
if (isReminder && data.updateUrl) {
    // Mode pengingat: "Perbarui sekarang" jadi aksi utama, "Mengerti" diganti "Nanti" (sekunder).
    updateButton.style.display = "";
    closeButton.classList.add("secondary");
    closeButton.textContent = "Nanti";
    updateButton.addEventListener("click", async () => {
        updateButton.disabled = true;
        try {
            await invoke("open_url", { url: data.updateUrl });
            close();
        } catch (error) {
            console.warn("[WhatsNew] Gagal membuka tautan update:", error);
            updateButton.disabled = false;
        }
    });
}

closeButton.addEventListener("click", close);
window.addEventListener("keydown", (e) => { if (e.key === "Escape" || e.key === "Enter") close(); });
(isReminder && data.updateUrl ? updateButton : closeButton).focus();