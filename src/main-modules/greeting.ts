// Greeting di jendela utama Notch. Dua sumber sapaan, satu layer tampilan (.l-greeting di index.html):
//  1. Saat aplikasi dijalankan (juga setelah komputer restart): baca pilihan dari Pengaturan
//     (localStorage "notch.startupGreeting"), tampilkan kartu teks dan ucapkan dengan TTS id-ID.
//  2. Tombol "▶ Coba ucapkan" di Pengaturan: jendela Settings yang berbicara, lalu mengirim event
//     "greeting-start" {id, text, subtitle, durationMs} dan "greeting-end" {id}. Di sini cukup
//     menampilkan / menutup kartunya (tanpa berbicara lagi agar tidak dobel).
import { listen } from "@tauri-apps/api/event";

export type GreetingHooks = {
  canShow: () => boolean;                 // tidak sedang ada kartu/hub lain
  isCard: () => boolean;                  // kartu "dev" sedang aktif (bukan hub/kartu lain)
  show: (ms: number) => void;             // tampilkan kartu "dev" selama ms
  mood: (m: string, ms: number) => void;  // ekspresi mata sementara
  showBubble?: (text: string, sub: string, ms: number) => boolean;
  hideBubble?: () => void;
  isMeeting?: () => boolean;
};
export type GreetingApi = {
  // kartu teks yang sama dipakai Conversation Moments / Focus Companion (menimpa kartu "dev" yang sedang tampil)
  say: (text: string, sub: string, ms: number) => void;
};
type StartPayload = { id?: string; text?: string; subtitle?: string; durationMs?: number };
type EndPayload = { id?: string };
type Pref = { enabled: boolean; mode: string; custom_text: string };

// harus sama dengan settings.html
const STARTUP_GREETING_KEY = "notch.startupGreeting";
const MODES = ["auto", "random", "halo", "morning", "noon", "afternoon", "evening", "custom"];
const DEFAULT: Pref = { enabled: true, mode: "random", custom_text: "Halo, semangat menjalani hari!" };

function readPref(): Pref {
  try {
    const saved = JSON.parse(localStorage.getItem(STARTUP_GREETING_KEY) || "null");
    if (!saved || typeof saved !== "object") return { ...DEFAULT };
    return {
      enabled: saved.enabled !== false,
      mode: MODES.includes(saved.mode) ? saved.mode : "auto",
      custom_text: typeof saved.custom_text === "string" ? saved.custom_text.slice(0, 120) : DEFAULT.custom_text,
    };
  } catch { return { ...DEFAULT }; }
}

function greetingText(pref: Pref, d = new Date()): string {
  if (pref.mode === "custom") return pref.custom_text.trim() || DEFAULT.custom_text;
  if (pref.mode === "halo") return "Halo!";
  if (pref.mode === "morning") return "Selamat pagi!";
  if (pref.mode === "noon") return "Selamat siang!";
  if (pref.mode === "afternoon") return "Selamat sore!";
  if (pref.mode === "evening") return "Selamat malam!";
  const h = d.getHours();
  const part = h >= 4 && h < 11 ? "pagi" : h >= 11 && h < 15 ? "siang" : h >= 15 && h < 18 ? "sore" : "malam";
  return `Halo! Selamat ${part}!`;
}
// ---------- sapaan acak (lucu), menyesuaikan waktu ----------
// [judul, punchline, jamMulai?, jamSelesai?]. Tanpa jam = bisa muncul kapan saja. Dengan jam = hanya
// muncul pada rentang itu (jamMulai > jamSelesai berarti melewati tengah malam) dan lebih sering dipilih.
// {waktu} -> Pagi/Siang/Sore/Malam, {bonjour} -> Bonjour/Bonsoir (otomatis sesuai jam sekarang).
// Daftar ini harus sama dengan settings.html.
type Phrase = [title: string, sub: string, from?: number, to?: number];
const PHRASES: Phrase[] = [
  ["{waktu}, NPC kesayangan!", "Dunia sudah loading, masa kamu belum?"],
  ["Welcome back, legend.", "Akhirnya pemeran utamanya muncul."],
  ["Wih, masih hidup!", "Server kehidupan masih online rupanya."],
  ["{bonjour}, manusia sibuk.", "Sibuk beneran atau cuma buka tab baru?"],
  ["Selamat datang kembali.", "Kami pura-pura nggak nungguin."],
  ["Aura +1000.", "Tinggal produktifnya yang ditingkatkan."],
  ["Look who's here.", "Notch kecil, ego besar."],
  ["Hari baru, kamu lagi.", "Semoga hari ini bukan copy-paste kemarin.", 4, 11],
  ["Hello, world!", "Error juga ikut menyambutmu."],
  ["Compile dulu, panik kemudian.", "Kita profesional, katanya."],
  ["Bug hunter detected.", "Semoga hari ini bukan bug yang berburu balik."],
  ["Git push, then pray.", "Ritual leluhur software engineer."],
  ["Code mode: ON.", "Produktivitas sedang dinegosiasikan."],
  ["404: Motivasi not found.", "Coba restart manusia."],
  ["Works on my machine.", "Kalimat terakhir sebelum meeting panjang."],
  ["Commit your feelings.", "Jangan semuanya disimpan di local."],
  ["Semangat, full-stack warrior.", "Frontend menangis, backend menyusul."],
  ["Satu bug lagi.", "Kalimat yang mengakhiri tiga jam hidupmu."],
  ["Psst... manusia.", "Iya, kamu. Jangan pura-pura nggak lihat."],
  ["Aku melihatmu.", "Bukan CCTV, cuma penasaran."],
  ["Produktif dikit, boleh?", "Biar hari ini punya plot."],
  ["Bro is back.", "Timeline kembali masuk akal."],
  ["Main character energy.", "Background music belum tersedia."],
  ["Kamu terlihat sibuk.", "Alt-tab bukan olahraga kardio."],
  ["Tenang, Bos.", "Masalahnya belum tentu tenang."],
  ["Plot twist hari ini?", "Kamu menyelesaikan pekerjaan tepat waktu."],
  ["Aura terdeteksi.", "Sumbernya belum diketahui."],
  ["Jangan overthinking.", "CPU otakmu sudah 98%."],
  ["Santai dulu, komandan.", "Tidak semua masalah perlu di-fix sekarang."],
  ["Achievement unlocked.", "Kamu berhasil membuka aplikasi lagi."],
  ["Masih melek, chief?", "Windows saja tahu kapan harus sleep.", 22, 4],
  ["Night shift activated.", "Besok jangan salahkan alarm.", 19, 4],
  ["Tidur adalah fitur premium.", "Dan kamu belum berlangganan.", 22, 4],
  ["Last task, katanya.", "Kita tahu ending cerita ini.", 19, 4],
  ["Sudah malam, bro.", "Bug boleh lembur, kamu jangan.", 19, 4],
  ["Save your progress.", "Termasuk progres kesehatanmu.", 20, 4],
  ["Closing time.", "Laptop boleh ditutup, pikiran juga.", 17, 4],
  ["Good night, legend.", "Semoga mimpi tanpa stack trace.", 21, 4],
];
const LAST_PHRASE_KEY = "notch.greeting.lastPhrase";
const inHours = (h: number, from: number, to: number) => (from <= to ? h >= from && h < to : h >= from || h < to);
const partOfDay = (h: number) => (h >= 4 && h < 11 ? "pagi" : h >= 11 && h < 15 ? "siang" : h >= 15 && h < 18 ? "sore" : "malam");

function pickPhrase(d = new Date()): { text: string; sub: string } {
  const h = d.getHours();
  const part = partOfDay(h);
  let last = -1;
  try { last = Number(localStorage.getItem(LAST_PHRASE_KEY) ?? -1); } catch { /* abaikan */ }

  // kandidat yang cocok dengan jam sekarang, tidak sama dengan yang terakhir; yang khusus jam diberi bobot 3
  const pool: { i: number; w: number }[] = [];
  PHRASES.forEach((p, i) => {
    if (i === last) return;
    const [, , from, to] = p;
    const timed = from !== undefined && to !== undefined;
    if (timed && !inHours(h, from, to)) return;
    pool.push({ i, w: timed ? 3 : 1 });
  });
  let r = Math.random() * pool.reduce((a, c) => a + c.w, 0);
  let chosen = pool[pool.length - 1].i;
  for (const c of pool) { r -= c.w; if (r < 0) { chosen = c.i; break; } }
  try { localStorage.setItem(LAST_PHRASE_KEY, String(chosen)); } catch { /* abaikan */ }

  const [title, sub] = PHRASES[chosen];
  const text = title
    .replace("{waktu}", part[0].toUpperCase() + part.slice(1))
    .replace("{bonjour}", part === "malam" ? "Bonsoir" : "Bonjour");
  return { text, sub };
}
const estimateMs = (text: string) => {
  const words = text.trim().split(/\s+/).filter(Boolean).length;
  return Math.min(11000, Math.max(3500, 1800 + words * 470));
};

// ---------- suara ----------
const findIndonesianVoice = (voices: SpeechSynthesisVoice[]) =>
  voices.find((v) => /^id(?:[-_]|$)/i.test(v.lang) || /(?:bahasa\s+indonesia|indonesian)/i.test(v.name)) ?? null;

// Tepat setelah boot / start, daftar suara Windows sering kosong beberapa detik. Tunggu sampai
// suara Indonesia muncul (polling + "voiceschanged"), bukan menyerah pada pengecekan pertama.
function waitForIndonesianVoice(timeoutMs = 9000): Promise<SpeechSynthesisVoice | null> {
  const synth = window.speechSynthesis;
  return new Promise((resolve) => {
    const check = () => {
      const v = findIndonesianVoice(synth.getVoices());
      if (v) { cleanup(); resolve(v); }
    };
    const poll = window.setInterval(check, 300);
    const timer = window.setTimeout(() => { cleanup(); resolve(findIndonesianVoice(synth.getVoices())); }, timeoutMs);
    function cleanup() { clearInterval(poll); clearTimeout(timer); synth.removeEventListener("voiceschanged", check); }
    synth.addEventListener("voiceschanged", check);
    check();
  });
}

async function speak(text: string): Promise<{ ok: boolean; reason: string }> {
  const synth = window.speechSynthesis;
  if (!synth || typeof SpeechSynthesisUtterance === "undefined") return { ok: false, reason: "Web Speech tidak tersedia" };
  const voice = await waitForIndonesianVoice();
  if (!voice) {
    const list = synth.getVoices().map((v) => `${v.name} (${v.lang})`).join(", ") || "belum ada voice yang dimuat";
    return { ok: false, reason: `voice id-ID tidak ditemukan; tersedia: ${list}` };
  }
  return new Promise((resolve) => {
    let started = false, done = false;
    const finish = (r: { ok: boolean; reason: string }) => { if (!done) { done = true; clearTimeout(watchdog); clearTimeout(hard); resolve(r); } };
    const u = new SpeechSynthesisUtterance(text);
    u.lang = "id-ID"; u.voice = voice; u.rate = 0.95; u.pitch = 1.04;
    u.onstart = () => { started = true; };
    u.onend = () => finish({ ok: true, reason: "" });
    u.onerror = (e) => finish({ ok: false, reason: e.error || "error" });
    const watchdog = window.setTimeout(() => {
      if (!started && !synth.speaking && !synth.pending) finish({ ok: false, reason: "audio tidak mulai diputar" });
    }, 7000);
    const hard = window.setTimeout(() => finish({ ok: true, reason: "" }), 30000);
    try { synth.cancel(); synth.resume(); synth.speak(u); }
    catch (error) { finish({ ok: false, reason: String(error) }); }
  });
}

// ---------- jendela utama ----------
let started = false;
export function initGreeting(h: GreetingHooks): GreetingApi | null {
  if (started) return null; // idempoten
  started = true;
  const notchEl = document.getElementById("notch");
  const textElRaw = document.getElementById("greeting-text");
  const subElRaw = document.getElementById("greeting-sub");
  if (!notchEl || !textElRaw || !subElRaw) { console.warn("[Greeting] Layer greeting tidak ada di index.html"); return null; }
  // Konstanta bertipe non-null: aman dipakai di dalam semua closure / callback.
  const notch: HTMLElement = notchEl;
  const textEl: HTMLElement = textElRaw;
  const subEl: HTMLElement = subElRaw;
  let activeId = "";
  let activeBubble = false;
  let clearTimer: number | undefined;

  const scheduleClear = (ms: number) => {
    clearTimeout(clearTimer);
    clearTimer = window.setTimeout(() => {
      notch.classList.remove("greeting-active");
      if (activeBubble) h.hideBubble?.();
      activeBubble = false;
    }, ms);
  };
  function showCard(id: string, text: string, sub: string, ms: number) {
    if (h.isMeeting?.()) return;
    activeId = id;

    // Jika notch sedang dock di sisi/bawah layar, greeting ikut memakai awan
    // percakapan. Bila tidak didukung/ruang tidak tersedia, pertahankan kartu lama.
    activeBubble = h.showBubble?.(text, sub, ms) === true;
    if (activeBubble) {
      notch.classList.remove("greeting-active");
    } else {
      textEl.textContent = text;
      subEl.textContent = sub;
      notch.classList.add("greeting-active");
      h.show(ms);
    }

    h.mood("happy", ms);
    scheduleClear(ms + 600);
  }
  function endCard(id?: string) {
    if (id && activeId && id !== activeId) return;           // sapaan lain sudah menggantikan
    if (activeBubble) {
      activeId = "";
      h.hideBubble?.();
      activeBubble = false;
      clearTimeout(clearTimer);
      return;
    }
    if (!notch.classList.contains("greeting-active")) return;
    activeId = "";
    if (h.isCard()) h.show(900);                              // beri jeda singkat lalu tutup
    scheduleClear(1500);
  }

  // dari tombol "Coba ucapkan" di Pengaturan
  listen<StartPayload>("greeting-start", (e) => {
    const p = e.payload ?? {};
    const text = String(p.text ?? "").trim();
    if (!text) return;
    const ms = Math.min(15000, Math.max(2500, Math.round(Number(p.durationMs)) || estimateMs(text)));
    showCard(String(p.id ?? `ext-${Date.now()}`), text, p.subtitle || "Sapaan dari Notch", ms);
  }).catch((error) => console.warn("[Greeting] Gagal memasang listener greeting-start:", error));
  listen<EndPayload>("greeting-end", (e) => endCard(e.payload?.id))
    .catch((error) => console.warn("[Greeting] Gagal memasang listener greeting-end:", error));

  // sapaan saat aplikasi dijalankan. Setelah boot/login bisa ada kartu lain yang tampil atau jendela
  // belum siap, jadi coba lagi beberapa kali.
  let tries = 0;
  const attempt = () => {
    const pref = readPref();
    if (!pref.enabled) return;
    if (!h.canShow()) {
      if (++tries <= 15) window.setTimeout(attempt, 2000);
      else console.warn("[Greeting] Dilewati: notch terus sibuk");
      return;
    }
    // mode "random": judul di kartu + punchline sebagai subteks, dan keduanya diucapkan
    const random = pref.mode === "random" ? pickPhrase() : null;
    const text = random ? random.text : greetingText(pref);
    const sub = random ? random.sub
      : new Date().toLocaleDateString("id-ID", { weekday: "long", day: "numeric", month: "short", year: "numeric" });
    const spoken = random ? `${random.text} ${random.sub}` : text;
    showCard("startup", text, sub, estimateMs(spoken) + 1800);
    void speak(spoken).then((r) => {
      if (!r.ok) console.warn("[Greeting] Suara gagal:", r.reason);
      endCard("startup");
    });
  };
  window.setTimeout(attempt, 2000);

  return { say: (text, sub, ms) => showCard(`msg-${Date.now()}`, text, sub, ms) };
}
