import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Halaman jadwal shalat + pengingat. Data dari Rust (prayer_month), semua hitungan di sini.

type Day = {
    tanggal: number; imsak: string; subuh: string; terbit: string;
    dzuhur: string; ashar: string; maghrib: string; isya: string;
};
type Mode = "adzan" | "tone" | "silent";
type Prefs = { provinsi: string; kabkota: string; mode: Mode; remind_before: boolean; remind_min: number };
type Entry = { key: string; name: string; at: Date };
type Hooks = {
    card: (title: string, sub: string, ms: number) => void;
    mood: (m: string, ms: number) => void;
    adhanStart: (prayerName: string, sub: string) => void | Promise<void>;
    adhanEnd: () => void;
    adhanError?: (message: string) => void;
    ramadanExpanded?: (expanded: boolean) => void;
};

const PRAYERS: [keyof Day, string][] = [
    ["subuh", "Subuh"], ["dzuhur", "Dzuhur"], ["ashar", "Ashar"], ["maghrib", "Maghrib"], ["isya", "Isya"],
];
const ARC_X = [30, 90, 150, 210, 270];
const ARC_Y = [24, 12, 8, 12, 24];

let prefs: Prefs | null = null;
let hooks: Hooks;
let err = "";
let force = false; // tombol "Muat ulang": lewati cache sekali
const months = new Map<string, Day[]>();
const attempts = new Map<string, number>();

const el = (id: string) => document.getElementById(id);
const pad = (n: number) => String(n).padStart(2, "0");
const hhmm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const mkey = (y: number, m: number) => `${y}-${m}`;

// ---------- data ----------
function ensureMonth(y: number, m: number) {
    if (!prefs) return;
    const k = mkey(y, m);
    if (months.has(k)) return;
    if (Date.now() - (attempts.get(k) ?? 0) < 30_000) return; // jangan menembak API terus-menerus
    attempts.set(k, Date.now());
    const refresh = force;
    force = false;
    invoke<Day[]>("prayer_month", {
        provinsi: prefs.provinsi, kabkota: prefs.kabkota, year: y, month: m, refresh,
    })
        .then((d) => { months.set(k, d); err = ""; render(); })
        .catch((e) => { err = String(e); render(); });
}

function parseAt(base: Date, s: string): Date | null {
    const m = /(\d{1,2})[:.](\d{2})/.exec(s ?? "");
    if (!m) return null;
    return new Date(base.getFullYear(), base.getMonth(), base.getDate(), +m[1], +m[2], 0);
}

function dayEntries(d: Date): { list: Entry[]; day: Day } | null {
    const y = d.getFullYear(), mo = d.getMonth() + 1;
    const days = months.get(mkey(y, mo));
    if (!days) { ensureMonth(y, mo); return null; }
    const day = days.find((x) => x.tanggal === d.getDate()) ?? days[d.getDate() - 1];
    if (!day) return null;
    const list: Entry[] = [];
    for (const [k, name] of PRAYERS) {
        const at = parseAt(d, day[k] as string);
        if (at) list.push({ key: k, name, at });
    }
    return list.length ? { list, day } : null;
}

type Next = Entry & { tomorrow: boolean; entries: Entry[]; day: Day };
function nextPrayer(): Next | null {
    const now = new Date();
    const t = dayEntries(now);
    if (t) {
        const e = t.list.find((x) => x.at > now);
        if (e) return { ...e, tomorrow: false, entries: t.list, day: t.day };
    }
    const tm = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    const x = dayEntries(tm);
    if (x) return { ...x.list[0], tomorrow: true, entries: x.list, day: x.day };
    return null;
}

// ---------- tampilan ----------
function build() {
    const arc = el("pr-arc");
    if (!arc || arc.dataset.built) return;
    arc.dataset.built = "1";
    const dots = ARC_X.map((x, i) =>
        `<circle class="pr-ring" id="pr-r${i}" cx="${x}" cy="${ARC_Y[i]}" r="8"/>` +
        `<circle class="pr-dot" id="pr-d${i}" cx="${x}" cy="${ARC_Y[i]}" r="3.5"/>`).join("");
    const labels = PRAYERS.map(([, n], i) =>
        `<div class="pr-col" id="pr-c${i}"><span>${n}</span><b id="pr-t${i}">--:--</b></div>`).join("");
    arc.innerHTML =
        `<svg viewBox="0 0 300 34"><path class="pr-path" d="M30 24 Q150 -8 270 24"/>${dots}</svg>` +
        `<div class="pr-labels">${labels}</div>`;
}

function render() {
    const count = el("pr-count");
    if (!count) return; // halaman dimatikan
    build();
    const set = (id: string, t: string) => { const e = el(id); if (e) e.textContent = t; };

    set("pr-city", prefs?.kabkota ?? "-");
    set("pr-src", err ? `Gagal: ${err}`.slice(0, 40) : "Dari config");
    el("pr-src")?.classList.toggle("bad", !!err);

    const n = nextPrayer();
    if (!n) {
        set("pr-name", err ? "Offline" : "Memuat…");
        count.textContent = "--:--:--";
        set("pr-at", "");
        set("pr-when", "");
        set("pr-meta", "");
    } else {
        const s = Math.max(0, Math.floor((n.at.getTime() - Date.now()) / 1000));
        count.textContent = `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
        set("pr-name", n.name);
        set("pr-at", hhmm(n.at));
        set("pr-when", n.tomorrow ? "besok" : "hari ini");
        set("pr-meta", `Imsak ${n.day.imsak} · Terbit ${n.day.terbit}`);
        const idx = n.entries.findIndex((e) => e.key === n.key);
        n.entries.forEach((e, i) => {
            set(`pr-t${i}`, hhmm(e.at));
            el(`pr-c${i}`)?.classList.toggle("next", i === idx);
            el(`pr-c${i}`)?.classList.toggle("past", !n.tomorrow && i < idx);
            el(`pr-d${i}`)?.classList.toggle("next", i === idx);
            el(`pr-d${i}`)?.classList.toggle("past", !n.tomorrow && i < idx);
            el(`pr-r${i}`)?.classList.toggle("on", i === idx);
        });
    }

    if (prefs) {
        document.querySelectorAll<HTMLElement>("[data-mode]").forEach((b) =>
            b.classList.toggle("on", b.dataset.mode === prefs!.mode));
        el("pr-remind")?.classList.toggle("on", prefs.remind_before);
        set("pr-remind-txt", `${prefs.remind_min} mnt sebelum`);
    }
    renderRamadan();
}

// ---------- suara ----------
let ctx: AudioContext | null = null;
function tone(times: number) {
    try {
        ctx ??= new AudioContext();
        if (ctx.state === "suspended") void ctx.resume();
        const t0 = ctx.currentTime;
        for (let i = 0; i < times; i++) {
            [660, 880].forEach((f, j) => {
                const o = ctx!.createOscillator(), g = ctx!.createGain();
                const t = t0 + i * 0.9 + j * 0.28;
                o.type = "sine"; o.frequency.value = f;
                g.gain.setValueAtTime(0.0001, t);
                g.gain.exponentialRampToValueAtTime(0.22, t + 0.03);
                g.gain.exponentialRampToValueAtTime(0.0001, t + 0.5);
                o.connect(g).connect(ctx!.destination);
                o.start(t); o.stop(t + 0.52);
            });
        }
    } catch { /* audio tidak tersedia: kartu tetap tampil */ }
}

// MP3 utama dibundle oleh Vite. Dua jalur tambahan menangani proyek yang menyimpan aset
// di public/adzan.mp3 (produksi) atau src/public/adzan.mp3 (mode dev).
const ADZAN_URLS = [...new Set([
    new URL("./public/adzan.mp3", import.meta.url).href,
    "/adzan.mp3",
    "/src/public/adzan.mp3",
])];
let activeAdhanAudio: HTMLAudioElement | null = null;
let activeAdhanFinish: (() => void) | null = null;

/** Putar MP3 adzan dan coba jalur aset alternatif bila salah satu URL tidak tersedia.
 * Alert hanya ditutup oleh event `ended`, bukan oleh fallback nada singkat.
 */
async function adzan(prayerName = "Adzan", showNotch = true) {
    // Hentikan siklus lama dengan rapi sebelum memulai tes/siklus baru.
    if (activeAdhanFinish) {
        const stopPrevious = activeAdhanFinish;
        activeAdhanFinish = null;
        if (activeAdhanAudio) {
            activeAdhanAudio.pause();
            try { activeAdhanAudio.currentTime = 0; } catch { /* abaikan */ }
        }
        stopPrevious();
    }

    let audio: HTMLAudioElement | null = null;
    let finished = false;
    let sourceIndex = 0;
    const failDetails: string[] = [];

    const clearActive = () => {
        if (activeAdhanAudio === audio) activeAdhanAudio = null;
        if (activeAdhanFinish === finish) activeAdhanFinish = null;
    };
    const finish = () => {
        if (finished) return;
        finished = true;
        clearActive();
        if (showNotch) {
            hooks.adhanEnd();
            showRamadanDhikr(prayerName);
        }
    };
    activeAdhanFinish = finish;

    if (showNotch) {
        const sub = `${hhmm(new Date())} · ${prefs?.kabkota ?? "Lokasi shalat"}`;
        // Tunggu media dijeda dulu (maks. 2 dtk) supaya adzan tidak bertumpuk dengan musik.
        try {
            await Promise.race([
                Promise.resolve(hooks.adhanStart(prayerName, sub)),
                new Promise<void>((r) => setTimeout(r, 2000)),
            ]);
        } catch (e) { console.warn("[Prayer] adhanStart gagal:", e); }
        if (finished) return; // siklus ini sudah dihentikan oleh pemanggilan adzan yang baru
    }

    const failAllSources = () => {
        if (finished) return;
        finished = true;
        if (audio) audio.pause();
        clearActive();
        const detail = failDetails.length ? ` (${failDetails.join("; ")})` : "";
        const message = `File adzan.mp3 tidak dapat dimuat. Pastikan file tersedia di src/public/adzan.mp3 atau public/adzan.mp3.${detail}`;
        console.error("[Prayer] Semua jalur audio adzan gagal:", ADZAN_URLS, failDetails);
        // Jangan putar beep pendek lalu menutup alert seolah adzan selesai.
        // Akhiri siklus juga: alert tidak boleh tertahan dan media harus dilanjutkan.
        if (showNotch) { hooks.adhanError?.(message); hooks.adhanEnd(); }
    };

    const tryNextSource = (reason: unknown) => {
        if (finished) return;
        if (audio) {
            failDetails.push(`${audio.currentSrc || audio.src}: ${String(reason)}`);
            audio.pause();
            audio.removeAttribute("src");
            audio.load();
        }
        if (sourceIndex >= ADZAN_URLS.length) {
            failAllSources();
            return;
        }

        const src = ADZAN_URLS[sourceIndex++];
        const candidate = new Audio();
        audio = candidate;
        activeAdhanAudio = candidate;
        candidate.preload = "auto";
        candidate.volume = 0.9;
        let attemptFailed = false;
        const fail = (why: unknown) => {
            if (attemptFailed || finished || audio !== candidate) return;
            attemptFailed = true;
            tryNextSource(why);
        };
        candidate.addEventListener("loadedmetadata", () => {
            console.info(`[Prayer] Durasi file adzan: ${Number.isFinite(candidate.duration) ? `${candidate.duration.toFixed(1)} detik` : "belum diketahui"}`);
            if (Number.isFinite(candidate.duration) && candidate.duration > 0 && candidate.duration < 15) {
                console.warn("[Prayer] File adzan sangat singkat; pastikan ini bukan file preview.");
            }
        }, { once: true });
        candidate.addEventListener("ended", () => {
            if (audio === candidate && !attemptFailed) finish();
        }, { once: true });
        candidate.addEventListener("error", () => {
            fail(`audio error ${candidate.error?.code ?? "unknown"}`);
        }, { once: true });
        candidate.src = src;
        candidate.load();
        console.info(`[Prayer] Memuat audio adzan dari: ${src}`);
        void candidate.play().then(() => {
            console.info(`[Prayer] Audio adzan mulai diputar: ${src}`);
        }).catch(fail);
    };

    // Pemanggilan ini dimulai sebagai kandidat pertama; bila gagal, URL berikutnya dicoba.
    tryNextSource("Memulai pemutaran");
}

/** Uji siklus adzan PENUH (notch/awan + mulut + jeda & lanjut media) tanpa menunggu waktu shalat.
 * Dipanggil lagi saat adzan berbunyi = hentikan. Tombol "Tes adzan" di halaman ini tetap hanya memutar suara. */
export function testAdhan() {
    if (activeAdhanFinish) {
        const stop = activeAdhanFinish;
        activeAdhanFinish = null;
        if (activeAdhanAudio) {
            activeAdhanAudio.pause();
            try { activeAdhanAudio.currentTime = 0; } catch { /* abaikan */ }
        }
        stop();
        return;
    }
    void adzan("Tes adzan", true);
}

// ---------- pengingat ----------
const FIRED_KEY = "notch.prayer.fired";
let fired = new Set<string>();
try { fired = new Set(JSON.parse(localStorage.getItem(FIRED_KEY) ?? "[]")); } catch { /* abaikan */ }
const markFired = (k: string) => {
    fired.add(k);
    try { localStorage.setItem(FIRED_KEY, JSON.stringify([...fired].slice(-60))); } catch { /* abaikan */ }
};

function checkReminders() {
    if (!prefs) return;
    const now = new Date();
    const today = dayEntries(now);
    const tm = dayEntries(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1));
    const cands = [...(today?.list ?? []), ...(tm ? [tm.list[0]] : [])]; // Subuh besok ikut (pengingat lewat tengah malam)
    const nowMs = now.getTime();

    for (const c of cands) {
        for (const kind of ["pre", "at"] as const) {
            if (kind === "pre" && !prefs.remind_before) continue;
            const trig = c.at.getTime() - (kind === "pre" ? prefs.remind_min * 60_000 : 0);
            if (nowMs < trig || nowMs >= trig + 90_000) continue; // jendela 90 dtk: tidak menembak ulang saat app dibuka telat
            const k = `${c.at.toDateString()}|${c.key}|${kind}`;
            if (fired.has(k)) continue;
            markFired(k);

            if (kind === "pre") {
                hooks.card(`Menjelang ${c.name}`, `${prefs.remind_min} menit lagi · ${hhmm(c.at)}`, 8000);
                hooks.mood("surprised", 3000);
                if (prefs.mode !== "silent") tone(1);
            } else {
                hooks.mood("love", 4000);
                if (prefs.mode === "adzan") {
                    // Kartu tetap terbuka hingga HTMLAudioElement memicu ended (atau gagal).
                    adzan(c.name, true);
                } else {
                    hooks.card(`Waktu ${c.name}`, `${hhmm(c.at)} · ${prefs.kabkota}`, 15000);
                    if (prefs.mode === "tone") tone(3);
                }
            }
        }
    }
}

// ---------- Mode Ramadan ----------
// Markup ada di index.html dan gaya di ramadan.css. File ini hanya menyambungkan perilaku
// ke elemen yang sudah ada (tidak lagi membuat tombol, panel, atau <style> sendiri).
const RAMADAN_KEY = "notch.ramadan.enabled";
let ramadanEnabled = false;
try { ramadanEnabled = localStorage.getItem(RAMADAN_KEY) === "1"; } catch { /* storage opsional */ }

const DHIKR: { ar: string; latin: string; arti: string; note: string }[] = [
    { ar: "أَسْتَغْفِرُ اللّٰهَ", latin: "Astaghfirullah", arti: "“Aku memohon ampun kepada Allah.”", note: "Dibaca 3x setelah adzan" },
    { ar: "اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةِ التَّامَّةِ", latin: "Allahumma rabba hadzihid-da'watit-tammah", arti: "“Ya Allah, Tuhan pemilik seruan yang sempurna ini.”", note: "Doa setelah adzan" },
    { ar: "سُبْحَانَ اللّٰهِ وَبِحَمْدِهِ", latin: "Subhanallahi wa bihamdihi", arti: "“Maha Suci Allah dan segala puji bagi-Nya.”", note: "Dibaca 33x" },
];
let dhikrIdx = 0;

function installRamadanUi() {
    const page = document.querySelector<HTMLElement>(".p-prayer");
    if (!page || page.dataset.ramadanReady) return;
    const toggle = el("pr-ramadan-toggle");
    const panel = el("pr-ramadan-panel");
    if (!toggle || !panel) return;
    page.dataset.ramadanReady = "1";
    panel.hidden = !ramadanEnabled;

    const setEnabled = (v: boolean) => {
        ramadanEnabled = v;
        try { localStorage.setItem(RAMADAN_KEY, v ? "1" : "0"); } catch { /* storage opsional */ }
        hooks.ramadanExpanded?.(v);
        renderRamadan();
    };
    toggle.addEventListener("click", (e) => { e.stopPropagation(); setEnabled(!ramadanEnabled); });
    el("pr-ramadan-sw")?.addEventListener("click", (e) => { e.stopPropagation(); setEnabled(false); });
    const step = (d: number) => { dhikrIdx = (dhikrIdx + d + DHIKR.length) % DHIKR.length; renderDhikr(); };
    el("pr-dh-prev")?.addEventListener("click", (e) => { e.stopPropagation(); step(-1); });
    el("pr-dh-next")?.addEventListener("click", (e) => { e.stopPropagation(); step(1); });
    hooks.ramadanExpanded?.(ramadanEnabled);
    renderRamadan();
}

function renderDhikr() {
    const d = DHIKR[dhikrIdx];
    const set = (id: string, v: string) => { const n = el(id); if (n) n.textContent = v; };
    set("pr-dh-ar", d.ar); set("pr-dh-latin", d.latin); set("pr-dh-arti", d.arti); set("pr-dh-note", d.note);
    set("pr-dh-n", `${dhikrIdx + 1}/${DHIKR.length}`);
    const dots = el("pr-dh-dots");
    if (dots) dots.innerHTML = DHIKR.map((_, i) => `<i class="${i === dhikrIdx ? "on" : ""}"></i>`).join("");
}

function countdownText(target: Date | null): string {
    if (!target) return "--:--:--";
    const seconds = Math.max(0, Math.floor((target.getTime() - Date.now()) / 1000));
    return `${pad(Math.floor(seconds / 3600))}:${pad(Math.floor((seconds % 3600) / 60))}:${pad(seconds % 60)}`;
}

function ramadanTarget(dayKey: "imsak" | "maghrib"): { at: Date; day: Day } | null {
    const now = new Date();
    for (let offset = 0; offset <= 1; offset++) {
        const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
        const info = dayEntries(date);
        if (!info) continue;
        const at = parseAt(date, info.day[dayKey]);
        if (at && at.getTime() > now.getTime()) return { at, day: info.day };
    }
    return null;
}

function renderRamadan() {
    const toggle = el("pr-ramadan-toggle") as HTMLButtonElement | null;
    const panel = el("pr-ramadan-panel") as HTMLElement | null;
    if (!toggle || !panel) return;
    toggle.classList.toggle("on", ramadanEnabled);
    toggle.setAttribute("aria-pressed", String(ramadanEnabled));
    panel.hidden = !ramadanEnabled;
    document.querySelector(".p-prayer")?.classList.toggle("ramadan-on", ramadanEnabled);
    if (!ramadanEnabled) return;
    if (!panel.dataset.dh) { panel.dataset.dh = "1"; renderDhikr(); }

    const now = new Date();
    const todayInfo = dayEntries(now);
    const sahur = ramadanTarget("imsak");
    const buka = ramadanTarget("maghrib");
    const set = (id: string, value: string) => { const node = el(id); if (node) node.textContent = value; };
    set("pr-ramadan-date", now.toLocaleDateString("id-ID", { weekday: "long", day: "numeric", month: "long", year: "numeric" }).replace(/^(\S+) /, "$1, "));
    set("pr-ramadan-sahur", countdownText(sahur?.at ?? null));
    set("pr-ramadan-buka", countdownText(buka?.at ?? null));
    set("pr-ramadan-sahur-at", `Imsak ${sahur ? hhmm(sahur.at) : "--:--"}`);
    set("pr-ramadan-buka-at", `Maghrib ${buka ? hhmm(buka.at) : "--:--"}`);

    const times = el("pr-ramadan-times");
    if (times) {
        if (!todayInfo) {
            times.textContent = err ? "Jadwal belum tersedia" : "Memuat jadwal…";
        } else {
            const d = todayInfo.day;
            const rows: [string, string][] = [
                ["Imsak", d.imsak], ["Subuh", d.subuh], ["Terbit", d.terbit],
                ["Dzuhur", d.dzuhur], ["Ashar", d.ashar], ["Maghrib", d.maghrib], ["Isya", d.isya],
            ];
            times.replaceChildren();
            rows.forEach(([name, time], i) => {
                const item = document.createElement("div");
                item.className = "pr-ramadan-time" + (i === 0 ? " first" : "");
                const label = document.createElement("small");
                label.textContent = name;
                const value = document.createElement("b");
                value.textContent = time || "--:--";
                item.append(label, value);
                times.appendChild(item);
            });
        }
    }
}

function showRamadanDhikr(prayerName: string) {
    if (!ramadanEnabled || prayerName === "Tes adzan") return;
    hooks.card("Dzikir setelah adzan", "Allahumma rabba hadzihid-da'watit-tammah.", 12000);
}

// ---------- kontrol ----------
function savePrefs() {
    if (!prefs) return;
    invoke("prayer_set_prefs", {
        mode: prefs.mode, remindBefore: prefs.remind_before, remindMin: prefs.remind_min,
    }).catch(() => { });
}

function wire() {
    document.querySelectorAll<HTMLElement>("[data-mode]").forEach((b) =>
        b.addEventListener("click", (e) => {
            e.stopPropagation();
            if (!prefs) return;
            prefs.mode = b.dataset.mode as Mode;
            if (prefs.mode === "adzan") adzan("Tes adzan", false); else if (prefs.mode === "tone") tone(1); // pratinjau suara
            render(); savePrefs();
        }));
    el("pr-remind")?.addEventListener("click", (e) => {
        e.stopPropagation();
        if (!prefs) return;
        prefs.remind_before = !prefs.remind_before;
        render(); savePrefs();
    });
    el("pr-refresh")?.addEventListener("click", (e) => {
        e.stopPropagation();
        months.clear(); attempts.clear(); err = ""; force = true;
        render();
    });
}

export function initPrayer(h: Hooks) {
    hooks = h;
    wire();
    build();
    installRamadanUi();

    invoke<Prefs>("prayer_info").then((p) => { prefs = p; render(); }).catch((e) => { err = String(e); render(); });

    // lokasi / pilihan berubah dari jendela pengaturan atau config.toml
    listen<{ prayer?: Prefs }>("config-changed", (e) => {
        const p = e.payload?.prayer;
        if (!p) return;
        const moved = !prefs || p.provinsi !== prefs.provinsi || p.kabkota !== prefs.kabkota;
        prefs = p;
        if (moved) { months.clear(); attempts.clear(); err = ""; }
        render();
    });

    setInterval(() => { render(); checkReminders(); }, 1000);
}