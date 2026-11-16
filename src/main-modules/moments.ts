// Conversation Moments + Focus Companion: komentar singkat berupa TEKS (tanpa suara) di notch.
// Pengaturannya ada di jendela Settings (localStorage "notch.conversationMoments" dan
// "notch.focusCompanion"; tersimpan saat tombol Simpan ditekan). Di sini ada dua bagian:
//  1. pendengar tombol "Coba momen" / "Coba momen cuaca" / "Coba Focus Companion" di Pengaturan
//     (event "conversation-moments-test" dan "focus-companion-test"), dan
//  2. pemicu aslinya: musik, cuaca, baterai, kembali setelah lama tidak aktif, serta timer Pomodoro.
// main.ts memanggil fungsi note*/focus* dari handler event yang sudah ada.
//
// Pesan tidak pernah menimpa kartu lain (adzan, pengingat, greeting, hub, kartu perangkat): bila notch
// sedang sibuk, pesan menunggu antrean (maks. 30-60 dtk) lalu tampil, atau dibuang bila sudah basi.
import { listen } from "@tauri-apps/api/event";

export type MomentsHooks = {
  canShow: () => boolean;                                  // notch bebas (tak ada kartu/hub/adzan/pengingat)
  say: (text: string, sub: string, ms: number) => void;    // tampilkan kartu teks
};

// ---------- pengaturan (cermin dari settings.html) ----------
const CM_KEY = "notch.conversationMoments";
const FC_KEY = "notch.focusCompanion";
type CmPref = {
  enabled: boolean; on_play: boolean; on_track_change: boolean; on_pause: boolean; on_return: boolean;
  on_weather_change: boolean; on_rain_soon: boolean; on_low_battery: boolean; on_charging: boolean;
  return_after_min: number; cooldown_min: number;
};
type FcPref = { enabled: boolean; on_start: boolean; on_halfway: boolean; on_focus_complete: boolean; on_break_end: boolean };
const inRange = (v: unknown, fb: number, min: number, max: number) => {
  const n = Number(v);
  return Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : fb;
};
function readCm(): CmPref {
  let s: any = null;
  try { s = JSON.parse(localStorage.getItem(CM_KEY) || "null"); } catch { /* pakai bawaan */ }
  if (!s || typeof s !== "object") s = {};
  return {
    enabled: s.enabled !== false, on_play: s.on_play !== false, on_track_change: s.on_track_change !== false,
    on_pause: s.on_pause === true, on_return: s.on_return !== false, on_weather_change: s.on_weather_change !== false,
    on_rain_soon: s.on_rain_soon !== false, on_low_battery: s.on_low_battery !== false, on_charging: s.on_charging === true,
    return_after_min: inRange(s.return_after_min, 10, 5, 120), cooldown_min: inRange(s.cooldown_min, 3, 1, 60),
  };
}
function readFc(): FcPref {
  let s: any = null;
  try { s = JSON.parse(localStorage.getItem(FC_KEY) || "null"); } catch { /* pakai bawaan */ }
  if (!s || typeof s !== "object") s = {};
  return {
    enabled: s.enabled !== false, on_start: s.on_start !== false, on_halfway: s.on_halfway !== false,
    on_focus_complete: s.on_focus_complete !== false, on_break_end: s.on_break_end !== false,
  };
}

// ---------- antrean tampil ----------
type Kind = "conv" | "focus";
type Msg = { kind: Kind; key: string; text: string; sub: string; expires: number };
let hooks: MomentsHooks | null = null;
const queue: Msg[] = [];
let lastConvAt = 0;
let meetingMode = false;

/** Dipanggil oleh indikator privasi; komentar yang tertahan selama rapat dibuang, bukan diputar belakangan. */
export function setMeetingMode(active: boolean) {
  meetingMode = active;
  if (active) queue.length = 0;
}

const showMs = (text: string) => Math.min(8000, Math.max(4500, 2500 + text.split(/\s+/).length * 330));

function enqueue(kind: Kind, key: string, text: string, sub: string, ttlMs: number) {
  if (!hooks || meetingMode) return;
  if (kind === "conv" && Date.now() - lastConvAt < readCm().cooldown_min * 60_000) return; // jeda antar komentar
  const i = queue.findIndex((m) => m.key === key);
  if (i >= 0) queue.splice(i, 1);
  queue.push({ kind, key, text, sub, expires: Date.now() + ttlMs });
  queue.sort((a, b) => (a.kind === b.kind ? 0 : a.kind === "focus" ? -1 : 1)); // Focus Companion lebih dulu
  if (queue.length > 4) queue.length = 4;
  pump();
}
function pump() {
  if (!hooks || meetingMode) return;
  const now = Date.now();
  for (let i = queue.length - 1; i >= 0; i--) if (queue[i].expires <= now) queue.splice(i, 1);
  if (!queue.length || !hooks.canShow()) return;
  while (queue.length) {
    const m = queue.shift()!;
    if (m.kind === "conv") {
      if (now - lastConvAt < readCm().cooldown_min * 60_000) continue; // sudah ada komentar barusan
      lastConvAt = now;
    }
    hooks.say(m.text, m.sub, showMs(m.text));
    return;
  }
}

// ---------- Conversation Moments: musik ----------
type MediaLite = { active: boolean; playing: boolean; title: string; artist: string };
let mediaBase = false, prevPlaying = false, prevTitle = "", pauseTimer: number | undefined;
const mediaSub = (m: MediaLite) => [m.title, m.artist].filter(Boolean).join(" · ") || "Pemutar media";

export function noteMedia(m: MediaLite) {
  const nowPlaying = m.active && m.playing;
  if (!mediaBase) { mediaBase = true; prevPlaying = nowPlaying; prevTitle = m.title; return; } // sampel pertama = patokan
  if (nowPlaying) {
    clearTimeout(pauseTimer); pauseTimer = undefined; // jeda sesaat saat ganti lagu bukan "berhenti"
    const cm = readCm();
    if (!prevPlaying) {
      prevPlaying = true;
      if (cm.enabled && cm.on_play) enqueue("conv", "play", "Musik mulai mengalun, selamat menikmati 🎵", mediaSub(m), 30_000);
    } else if (m.title && m.title !== prevTitle) {
      if (cm.enabled && cm.on_track_change) enqueue("conv", "track", "Lagu berganti, semoga makin semangat 🎶", mediaSub(m), 30_000);
    }
    prevTitle = m.title || prevTitle;
  } else if (prevPlaying && pauseTimer === undefined) {
    pauseTimer = window.setTimeout(() => {
      pauseTimer = undefined;
      prevPlaying = false;
      const cm = readCm();
      if (cm.enabled && cm.on_pause) enqueue("conv", "pause", "Musik berhenti. Tarik napas sebentar, ya 🌿", "Pemutar media", 30_000);
    }, 2000);
  }
}

// ---------- cuaca ----------
type WeatherLite = { temp: number; rain_in: number | null };
type WeatherDesc = { kind: string; emoji: string; text: string };
let wxRef: { fam: string; temp: number } | null = null;
let lastRainAt = 0;
const wxFamily = (kind: string) => (kind === "night" ? "clear" : kind); // siang/malam cerah bukan perubahan cuaca

export function noteWeather(w: WeatherLite, alert: boolean, d: WeatherDesc) {
  const cm = readCm();
  if (alert && w.rain_in) {
    if (cm.enabled && cm.on_rain_soon && Date.now() - lastRainAt > 30 * 60_000) {
      lastRainAt = Date.now();
      enqueue("conv", "rain", `Hujan sekitar ${w.rain_in} menit lagi, jangan lupa payung ☔`, `${Math.round(w.temp)}° · ${d.text}`, 60_000);
    }
  }
  const cur = { fam: wxFamily(d.kind), temp: w.temp };
  if (!wxRef) { wxRef = cur; return; } // sampel pertama = patokan, tidak memicu komentar
  const changed = cur.fam !== wxRef.fam || Math.abs(cur.temp - wxRef.temp) >= 4;
  if (!changed) return;
  const tempOnly = cur.fam === wxRef.fam;
  wxRef = cur;
  if (alert || !cm.enabled || !cm.on_weather_change) return;
  const text = tempOnly
    ? `Suhu berubah jadi ${Math.round(cur.temp)}°, sesuaikan pakaianmu ya ${d.emoji}`
    : `Cuaca berubah: sekarang ${d.text.toLowerCase()} ${d.emoji}`;
  enqueue("conv", "weather", text, `${Math.round(cur.temp)}° · ${d.text}`, 60_000);
}

// ---------- baterai ----------
let batBase = false, prevLow = false, prevCharging = false;
export function noteBattery(percent: number, charging: boolean) {
  const low = percent <= 15 && !charging;
  if (!batBase) { batBase = true; prevLow = low; prevCharging = charging; return; }
  const cm = readCm();
  if (low && !prevLow && cm.enabled && cm.on_low_battery) {
    enqueue("conv", "lowbat", `Baterai tinggal ${percent}%, colok charger dulu yuk 🔋`, "Baterai rendah", 120_000);
  }
  if (charging && !prevCharging && cm.enabled && cm.on_charging) {
    enqueue("conv", "charging", "Charger terpasang, baterai mulai terisi ⚡", `${percent}%`, 30_000);
  }
  prevLow = low; prevCharging = charging;
}

// ---------- kembali setelah lama tidak aktif ----------
let lastActive = Date.now();
export function noteActivity() {
  const now = Date.now();
  const gap = now - lastActive;
  lastActive = now;
  if (gap < 5 * 60_000) return; // batas minimum pengaturan = 5 menit; hindari baca pengaturan tiap gerakan mouse
  const cm = readCm();
  if (!cm.enabled || !cm.on_return || gap < cm.return_after_min * 60_000) return;
  const min = Math.round(gap / 60_000);
  const away = min >= 90 ? `${Math.round(min / 60)} jam` : `${min} menit`;
  enqueue("conv", "return", "Selamat datang kembali! Aku menunggumu 👋", `Kamu pergi sekitar ${away}`, 45_000);
}

// ---------- Focus Companion (Pomodoro) ----------
let halfFired = false;
export function focusStarted(round: number, rounds: number) {
  halfFired = false;
  const fc = readFc();
  if (fc.enabled && fc.on_start) enqueue("focus", "f-start", "Yuk, fokus satu hal dulu. Kamu pasti bisa! 🎯", `Sesi fokus · ronde ${round}/${rounds}`, 20_000);
}
export function focusTick(phase: string, round: number, total: number, remaining: number, running: boolean) {
  if (phase !== "focus" || total <= 0) return;
  if (remaining >= total - 1) halfFired = false; // sesi diulang dari awal
  if (!running || halfFired || total < 120) return;
  if (total - remaining < total / 2) return;
  halfFired = true;
  const fc = readFc();
  if (fc.enabled && fc.on_halfway) enqueue("focus", "f-half", "Sudah setengah jalan, pertahankan ritmenya! 💪", `Ronde ${round} · sisa ${Math.max(1, Math.round(remaining / 60))} menit`, 60_000);
}
// dipanggil saat waktu habis sendiri (bukan lewat tombol lewati)
export function focusFinished(finished: string, next: string, round: number, rounds: number) {
  halfFired = false;
  const fc = readFc();
  if (!fc.enabled) return;
  if (finished === "focus") {
    if (!fc.on_focus_complete) return;
    const long = next === "long";
    enqueue("focus", "f-done", long ? "Semua ronde selesai! Istirahat panjang ini kamu layak dapat 🎉" : "Sesi fokus selesai, kerja bagus! Saatnya istirahat ☕",
      long ? "Istirahat panjang" : "Istirahat sebentar", 90_000);
  } else if (fc.on_break_end) {
    enqueue("focus", "f-break-end", "Istirahat selesai. Siap lanjut fokus? 🚀", `Sesi fokus · ronde ${round}/${rounds}`, 90_000);
  }
}

// ---------- init ----------
let started = false;
export function initMoments(h: MomentsHooks) {
  if (started) return; // idempoten
  started = true;
  hooks = h;
  window.setInterval(pump, 1000);

  // tombol "Coba ..." di Pengaturan: selalu tampil (abaikan jeda dan antrean)
  type TestPayload = { text?: string; subtitle?: string };
  const onTest = (fallbackText: string, fallbackSub: string) => (e: { payload: TestPayload | null }) => {
    const text = String(e.payload?.text || fallbackText).trim();
    h.say(text, String(e.payload?.subtitle || fallbackSub), showMs(text));
  };
  listen<TestPayload>("conversation-moments-test", onTest("Asyik, kita ketemu momen baru! ✨", "Uji Conversation Moments"))
    .catch((error) => console.warn("[Moments] Gagal memasang listener tes Conversation Moments:", error));
  listen<TestPayload>("focus-companion-test", onTest("Yuk, fokus satu hal dulu. Kamu pasti bisa! 🎯", "Uji Focus Companion"))
    .catch((error) => console.warn("[Moments] Gagal memasang listener tes Focus Companion:", error));
}
