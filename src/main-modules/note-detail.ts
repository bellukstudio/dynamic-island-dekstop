import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Note = { id: string; text: string; updated_at: number };

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
    document.getElementById(id) as T;
const editor = $<HTMLTextAreaElement>("note-editor");
const btnSave = $<HTMLButtonElement>("btn-save");
const btnDelete = $<HTMLButtonElement>("btn-delete");
const btnDeleteLabel = $("btn-delete-label");
const metaDate = $("meta-date");
const charCount = $("char-count");
const lineCount = $("line-count");
const dirtyEl = $("dirty");
const toast = $("toast");

let noteId = "";
let originalText = "";
let toastTimer: number | undefined;
let deleteTimer: number | undefined;
let closeWarned = false;

function showToast(msg: string) {
    toast.textContent = msg;
    toast.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => toast.classList.remove("show"), 2000);
}

const isDirty = () => editor.value.trim() !== originalText.trim();

function updateStats() {
    const txt = editor.value;
    charCount.textContent = `${txt.length.toLocaleString("id")} karakter`;
    lineCount.textContent = `${txt ? txt.split("\n").length : 0} baris`;
    btnSave.disabled = !(isDirty() && txt.trim().length > 0);
    dirtyEl.classList.toggle("show", isDirty());
    if (!isDirty()) closeWarned = false;
}

async function loadNote(id: string) {
    noteId = id;
    metaDate.textContent = "";
    try {
        const list = await invoke<Note[]>("notes_list");
        const n = list.find((x) => x.id === id);
        if (!n) {
            showToast("Catatan tidak ditemukan");
            return;
        }
        originalText = n.text;
        editor.value = n.text;
        metaDate.textContent = new Date(n.updated_at).toLocaleString("id-ID", {
            day: "numeric", month: "short", year: "numeric",
            hour: "2-digit", minute: "2-digit",
        });
    } catch (e) {
        console.error(e);
        showToast("Gagal memuat catatan");
    }
    closeWarned = false;
    updateStats();
    editor.focus();
}

async function closeWindow() {
    try {
        await getCurrentWindow().close();
    } catch {
        await getCurrentWindow().destroy().catch(() => window.close());
    }
}

async function save() {
    const text = editor.value.trim();
    if (!text || btnSave.disabled || !noteId) return;
    try {
        await invoke("notes_update", { id: noteId, text });
        originalText = text;
        editor.value = text;
        updateStats();
        showToast("✓ Disimpan");
        await emit("notes-changed"); // jendela utama refresh daftar
        await loadNote(noteId);      // perbarui tanggal
    } catch (e) {
        console.error(e);
        showToast("Gagal menyimpan");
    }
}

function resetDeleteButton() {
    btnDelete.classList.remove("confirm");
    btnDeleteLabel.textContent = "Hapus";
}
async function deleteNote() {
    if (!btnDelete.classList.contains("confirm")) {
        btnDelete.classList.add("confirm");
        btnDeleteLabel.textContent = "Yakin?";
        clearTimeout(deleteTimer);
        deleteTimer = window.setTimeout(resetDeleteButton, 3000);
        return;
    }
    clearTimeout(deleteTimer);
    try {
        await invoke("notes_remove", { id: noteId });
        await emit("notes-changed");
        await closeWindow();
    } catch (e) {
        console.error(e);
        resetDeleteButton();
        showToast("Gagal menghapus");
    }
}

editor.addEventListener("input", updateStats);
btnSave.addEventListener("click", save);
btnDelete.addEventListener("click", deleteNote);

document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        save();
    } else if (e.key === "Escape") {
        if (isDirty() && !closeWarned) {
            closeWarned = true;
            showToast("Belum disimpan — tekan Esc lagi untuk tutup");
        } else {
            closeWindow();
        }
    }
});

// jendela sudah terbuka, lalu pengguna klik catatan lain di notch
void listen<string>("note-open", (e) => loadNote(e.payload));

void loadNote((window as any).__NOTE_ID__ ?? "");