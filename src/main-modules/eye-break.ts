// Istirahat mata 20-20-20: tiap 20 menit pemakaian, mata notch mengajak kamu menatap sesuatu
// sejauh ~6 meter (20 kaki) selama 20 detik.
//
// Alur:
//  1. Kartu ajakan tampil sebentar (INTRO_MS) dengan ikon 👀.
//  2. Sesi menatap jauh selama `look_sec`:
//     - notch dalam tampilan idle (mata terlihat): mata notch memandang jauh (eye-break.css),
//       skin mata tetap dipakai;
//     - tampilan lain (wajah jam, musik diputar, dll. — mata tidak terlihat): kartu hitung mundur.
//  3. Kartu penutup + ekspresi senang, lalu hitungan 20 menit mulai dari nol.
//
// Waktu hanya dihitung saat pointer bergerak dalam 3 menit terakhir; 5 menit tanpa gerakan dianggap
// sudah beristirahat, hitungan diulang. Kartu tidak pernah menimpa kartu/hub lain: bila notch sedang
// sibuk, ajakan menunggu sampai bebas.
//
// Pengaturan (localStorage "notch.eyeBreak", JSON): { enabled, every_min, look_sec }.
// Event Tauri opsional: "eye-break-config" (ubah pengaturan langsung) dan "eye-break-test" (coba tampilkan).
import { listen } from "@tauri-apps/api/event";
import { setCardEmoji } from "./card-emoji";
import "../css/eye-break.css";

export type EyeBreakHooks = {
  lastMove: () => number;                                  // waktu (ms epoch) gerakan pointer terakhir
  canShow: () => boolean;                                  // notch bebas (tak ada kartu/hub/adzan)
  view: () => string;                                      // view notch saat ini; "idle" = mata terlihat
  card: (title: string, sub: string, ms: number) => void;  // tampilkan kartu teks
  mood: (m: string, ms: number) => void;                   // ekspresi mata sementara
};

type Pref = { enabled: boolean; every_min: number; look_sec: number };

const KEY = "notch.eyeBreak";
const DEFAULT: Pref = { enabled: true, every_min: 20, look_sec: 20 };
const INTRO_MS = 5000;           // lama kartu ajakan
const DONE_MS = 4000;            // lama kartu penutup
const ACTIVE_WINDOW_MS = 3 * 60_000; // pointer bergerak dalam 3 menit terakhir = sedang memakai komputer
const RESET_IDLE_MS = 5 * 60_000;    // 5 menit tanpa gerakan = sudah istirahat, hitungan diulang
const TICK_MS = 1000;
const COUNTDOWN_PREFIX = "Tatap jauh";

const inRange = (v: unknown, fb: number, min: number, max: number) => {
  const n = Number(v);
  return Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : fb;
};

function clean(raw: Partial<Pref> | null | undefined): Pref {
  const r = raw ?? {};
  return {
    enabled: r.enabled !== false,
    every_min: inRange(r.every_min, DEFAULT.every_min, 5, 120),
    look_sec: inRange(r.look_sec, DEFAULT.look_sec, 10, 60),
  };
}

function read(): Pref {
  try { return clean(JSON.parse(localStorage.getItem(KEY) || "null")); } catch { return { ...DEFAULT }; }
}

function write(p: Pref) {
  try { localStorage.setItem(KEY, JSON.stringify(p)); } catch { /* penyimpanan opsional */ }
}

export function initEyeBreak(h: EyeBreakHooks) {
  let pref = read();
  const startedAt = Date.now();
  let activeMs = 0;                       // waktu pemakaian sejak istirahat terakhir
  let running = false;                    // ajakan/sesi sedang berlangsung
  let timers: number[] = [];

  const eyesEl = () => document.querySelector<HTMLElement>(".eyes");
  const later = (fn: () => void, ms: number) => { timers.push(window.setTimeout(fn, ms)); };
  const visible = () => document.visibilityState === "visible";

  function finish() {
    timers.forEach(clearTimeout);
    timers = [];
    eyesEl()?.removeAttribute("data-gaze");
    running = false;
    activeMs = 0;
  }

  function start() {
    running = true;
    const sec = pref.look_sec;
    h.card("Waktunya istirahat mata", `Tatap sesuatu ±6 meter jauhnya · ${sec} detik`, INTRO_MS);
    setCardEmoji(["👀"]);
    // beri jeda lebih sedikit dari timer kartu supaya notch sudah kembali ke tampilan idle saat sesi mulai
    later(beginGaze, INTRO_MS + 250);
  }

  function beginGaze() {
    const endAt = Date.now() + pref.look_sec * 1000;
    eyesEl()?.setAttribute("data-gaze", "far");

    const step = () => {
      const left = Math.max(0, Math.ceil((endAt - Date.now()) / 1000));
      if (left <= 0) return complete();

      const text = `${COUNTDOWN_PREFIX} · ${left} detik lagi`;
      const sub = document.getElementById("dev-sub");
      const ourCardShown = h.view() === "dev" && !!sub?.textContent?.startsWith(COUNTDOWN_PREFIX);
      if (ourCardShown && sub) {
        sub.textContent = text;                  // perbarui hitung mundur tanpa memulai ulang animasi
      } else if (h.view() !== "idle" && h.canShow()) {
        // mata tidak terlihat (jam / musik / dll.): pakai kartu hitung mundur
        h.card("Istirahat mata", text, left * 1000);
        setCardEmoji(["👀"]);
      }
      later(step, TICK_MS);
    };
    step();
  }

  function complete() {
    const sub = document.getElementById("dev-sub");
    const ourCardShown = h.view() === "dev" && !!sub?.textContent?.startsWith(COUNTDOWN_PREFIX);
    const next = pref.every_min;
    finish();
    if (ourCardShown || h.canShow()) {
      h.card("Mata sudah segar!", `Istirahat berikutnya ${next} menit lagi`, DONE_MS);
      setCardEmoji(["😊"]);
    }
    h.mood("happy", DONE_MS + 1500);
  }

  setInterval(() => {
    if (running) return;
    if (!pref.enabled) { activeMs = 0; return; }
    const idleFor = Date.now() - Math.max(h.lastMove(), startedAt);
    if (idleFor >= RESET_IDLE_MS) { activeMs = 0; return; }
    if (idleFor < ACTIVE_WINDOW_MS) activeMs += TICK_MS;
    if (activeMs < pref.every_min * 60_000) return;
    if (!visible() || !h.canShow()) return;      // sibuk / jendela tersembunyi (mis. layar penuh): coba lagi nanti
    start();
  }, TICK_MS);

  // pengaturan berubah dari jendela lain
  window.addEventListener("storage", (e) => { if (e.key === KEY) pref = read(); });
  void listen<Partial<Pref>>("eye-break-config", (e) => {
    pref = clean({ ...pref, ...(e.payload ?? {}) });
    write(pref);
    if (!pref.enabled && running) finish();
  }).catch((err) => console.warn("[EyeBreak] Listener konfigurasi gagal:", err));
  // tombol "Coba tampilkan" (mis. dari Pengaturan)
  void listen("eye-break-test", () => { if (!running) start(); })
    .catch((err) => console.warn("[EyeBreak] Listener tes gagal:", err));
}
