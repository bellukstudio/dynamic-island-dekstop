import { invoke } from "@tauri-apps/api/core";

type Appearance = {
    theme: string;
    background: string;
    text: string;
    dark_bg: string;
    light_bg: string;
    accent: string;
    default_mood: string;
};
type Cfg = {
    scale: number;
    hide_fullscreen: boolean;
    appearance: Appearance;
    [k: string]: unknown; // pages, weather, timing, pomodoro: dibawa apa adanya
};

const FALLBACK: Cfg = {
    scale: 1,
    hide_fullscreen: true,
    appearance: { theme: "auto", background: "#000000", text: "", dark_bg: "#000000", light_bg: "#f5f5f7", accent: "", default_mood: "normal" },
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const inp = (id: string) => $<HTMLInputElement>(id);
const statusEl = $("status");
const pill = $("pill");

function say(text: string, kind: "" | "ok" | "err" = "") {
    statusEl.textContent = text;
    statusEl.dataset.kind = kind;
}
// error apa pun harus kelihatan, bukan diam saja
window.addEventListener("error", (e) => say(`Error: ${e.message}`, "err"));
window.addEventListener("unhandledrejection", (e) => say(`Error: ${String(e.reason)}`, "err"));

let cfg: Cfg = structuredClone(FALLBACK);
let loaded = false;

// ---------- util warna ----------
const norm = (h: string, fallback: string) => {
    const m = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.exec((h ?? "").trim());
    if (!m) return fallback;
    let v = m[1].toLowerCase();
    if (v.length === 3) v = v.split("").map((c) => c + c).join("");
    return "#" + v;
};
const rgb = (h: string): number[] => {
    const n = parseInt(h.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const luma = (c: number[]) => (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / 255;

// ---------- form <-> config ----------
const radios = () => [...document.querySelectorAll<HTMLInputElement>('input[name="theme"]')];
const theme = () => radios().find((r) => r.checked)?.value ?? "auto";
const COLORS = ["background", "dark_bg", "light_bg", "text", "accent"] as const;

function fill(c: Cfg) {
    const a = c.appearance;
    radios().forEach((r) => (r.checked = r.value === a.theme));
    inp("background").value = norm(a.background, "#000000");
    inp("dark_bg").value = norm(a.dark_bg, "#000000");
    inp("light_bg").value = norm(a.light_bg, "#f5f5f7");
    inp("text_auto").checked = !a.text;
    inp("text").value = norm(a.text, "#ffffff");
    inp("accent_auto").checked = !a.accent;
    inp("accent").value = norm(a.accent, "#788cff");
    const moodEl = document.getElementById("default_mood") as HTMLSelectElement | null;
    if (moodEl) moodEl.value = a.default_mood || "normal";
    inp("scale").value = String(c.scale);
    inp("hide_fullscreen").checked = c.hide_fullscreen;
    const motionToggle = document.getElementById("bottom_play") as HTMLInputElement | null;
    if (motionToggle) {
        try { motionToggle.checked = localStorage.getItem("notch.bottomPlay") !== "0"; }
        catch { motionToggle.checked = true; }
    }
    sync();
}

function collect(): Cfg {
    const c = structuredClone(cfg);
    c.appearance = {
        theme: theme(),
        background: inp("background").value,
        dark_bg: inp("dark_bg").value,
        light_bg: inp("light_bg").value,
        text: inp("text_auto").checked ? "" : inp("text").value,
        accent: inp("accent_auto").checked ? "" : inp("accent").value,
        default_mood: (document.getElementById("default_mood") as HTMLSelectElement | null)?.value || "normal",
    };
    c.scale = parseFloat(inp("scale").value);
    c.hide_fullscreen = inp("hide_fullscreen").checked;
    return c;
}

// aktif/nonaktifkan baris sesuai mode, perbarui label hex & pratinjau
function sync() {
    const t = theme();
    const on: Record<string, boolean> = {
        background: t === "custom",
        dark_bg: t === "auto" || t === "dark",
        light_bg: t === "auto" || t === "light",
        text: !inp("text_auto").checked,
        accent: !inp("accent_auto").checked,
    };
    for (const k of COLORS) {
        $(`row-${k}`).classList.toggle("off", !on[k] && (k === "background" || k === "dark_bg" || k === "light_bg"));
        inp(k).disabled = !on[k];
        $(`hex-${k}`).textContent = on[k] ? inp(k).value : k === "text" || k === "accent" ? "otomatis" : "—";
    }
    $("scale_val").textContent = `${Number(inp("scale").value).toFixed(2)}×`;
    updatePill();
}

function updatePill() {
    const c = collect();
    const a = c.appearance;
    const light = a.theme === "light" || (a.theme === "auto" && matchMedia("(prefers-color-scheme: light)").matches);
    const bgHex = a.theme === "custom" ? a.background : light ? a.light_bg : a.dark_bg;
    const bg = rgb(norm(bgHex, "#000000"));
    const fg = a.text ? rgb(norm(a.text, "#ffffff")) : luma(bg) > 0.55 ? [17, 17, 17] : [255, 255, 255];
    pill.style.setProperty("--p-bg", `rgb(${bg.join(",")})`);
    pill.style.setProperty("--p-fg", `rgb(${fg.join(",")})`);
    pill.style.setProperty("--p-fg-rgb", fg.join(","));
    pill.style.setProperty("--p-accent", a.accent ? rgb(norm(a.accent, "#788cff")).join(",") : "120,140,255");
    pill.style.setProperty("--p-scale", String(c.scale));
}

// ---------- pratinjau langsung ke notch (tidak menulis file) ----------
let previewTimer = 0;
function preview() {
    if (!loaded) return;
    clearTimeout(previewTimer);
    previewTimer = window.setTimeout(() => {
        invoke("cfg_preview", { cfg: collect() }).catch((e) => say(`Pratinjau gagal: ${e}`, "err"));
    }, 60);
}

document.querySelectorAll<HTMLInputElement>("input").forEach((el) => {
    const isScale = el.id === "scale";
    el.addEventListener("input", () => {
        sync();
        if (!isScale) preview(); // skala baru dikirim saat slider dilepas (mengubah ukuran jendela)
    });
    el.addEventListener("change", () => {
        sync();
        preview();
    });
});
document.getElementById("default_mood")?.addEventListener("change", () => {
    sync();
    preview();
});

// Saklar gerakan otomatis dibagikan oleh mode bawah dan sisi; berlaku langsung tanpa Simpan.
(() => {
    const box = document.getElementById("bottom_play") as HTMLInputElement | null;
    if (!box) return;
    const key = "notch.bottomPlay";
    try { box.checked = localStorage.getItem(key) !== "0"; } catch { box.checked = true; }
    box.addEventListener("change", () => {
        try {
            localStorage.setItem(key, box.checked ? "1" : "0");
            say(box.checked ? "Animasi gerak otomatis diaktifkan." : "Animasi gerak otomatis dimatikan.", "ok");
        } catch {
            say("Tidak dapat menyimpan pilihan animasi pada penyimpanan lokal.", "err");
        }
    });
})();

// ---------- simpan ----------
const buttons = [$<HTMLButtonElement>("save"), $<HTMLButtonElement>("restart")];
const busy = (b: boolean) => buttons.forEach((x) => (x.disabled = b));

async function save(restart: boolean) {
    if (!loaded) {
        say("Pengaturan belum berhasil dimuat, penyimpanan dibatalkan agar file lama tidak tertimpa.", "err");
        return;
    }
    busy(true);
    say("Menyimpan…");
    let path = "";
    try {
        path = await invoke<string>("cfg_save", { cfg: collect() });
        cfg = collect();
        say(`Tersimpan ✓  ${path}`, "ok");
    } catch (e) {
        say(`Gagal menyimpan: ${e}`, "err");
        busy(false);
        return;
    }
    if (restart) {
        say("Tersimpan ✓  memulai ulang…", "ok");
        try {
            await invoke("cfg_restart");
        } catch (e) {
            say(`Tersimpan ✓  tapi restart gagal: ${e}`, "err");
        }
    }
    busy(false);
}
$("save").addEventListener("click", () => void save(false));
$("restart").addEventListener("click", () => void save(true));

// ---------- mulai ----------
fill(cfg);
(async () => {
    try {
        cfg = await invoke<Cfg>("cfg_get");
        loaded = true;
        fill(cfg);
        say("Siap.");
    } catch (e) {
        say(`Gagal membaca pengaturan: ${e}`, "err");
    }
})();