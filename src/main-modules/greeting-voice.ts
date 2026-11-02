// Suara sapaan. Urutan:
//  1. Klip suara neural pra-render sesuai mode suara terpilih (public/greeting/*.mp3 + manifest.json, dibuat
//     oleh scripts/gen-greeting-audio.ts). Kata Inggris/Prancis sudah dibaca voice native-nya di dalam klip.
//  2. Bila teks tidak punya klip (mis. teks kustom), mode "Suara sistem", atau klip gagal diputar:
//     Web Speech API. Teks dipecah per bahasa: potongan Indonesia memakai voice id-ID, potongan Inggris
//     memakai voice en-US yang terpasang di Windows (bila ada), jadi lafalnya tetap benar.
// Dipakai greeting.ts dan jendela Pengaturan (lewat settings-greeting.ts) untuk tombol "Coba ucapkan".

import { type Lang, type VoicePreset, getPreset, readVoiceId, norm, splitByLanguage } from "./greeting-lang";

export type SpeakResult = { ok: boolean; reason: string; via?: "clip" | "system" };

const CLIP_BASE = "/greeting/"; // = public/greeting/ pada proyek Vite

// ---------- klip pra-render ----------
// manifest.json: { "<voiceId>|<teks>": ["<berkas-bahasa-1>.mp3", ...] }
let manifestPromise: Promise<Record<string, string[]>> | null = null;
function loadManifest(): Promise<Record<string, string[]>> {
  if (!manifestPromise) {
    manifestPromise = fetch(`${CLIP_BASE}manifest.json`, { cache: "no-cache" })
      .then((r) => (r.ok ? (r.json() as Promise<Record<string, string[]>>) : {}))
      .catch(() => ({} as Record<string, string[]>))
      .then((m) => {
        if (!Object.keys(m).length) manifestPromise = null; // belum ada / gagal: coba lagi lain kali
        return m;
      });
  }
  return manifestPromise;
}

let currentAudio: HTMLAudioElement | null = null;
let cancelClipPlayback: (() => void) | null = null;
let session = 0; // naik setiap stopSpeaking(); dipakai untuk membatalkan antrean potongan suara sistem

function playClips(files: string[], mine: number): Promise<SpeakResult> {
  return new Promise((resolve) => {
    let done = false;
    let timer: number | undefined;
    let index = 0;
    const finish = (r: SpeakResult) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      cancelClipPlayback = null;
      if (currentAudio) {
        currentAudio.onended = null;
        currentAudio.onerror = null;
        if (!r.ok || r.reason === "dihentikan") currentAudio.pause();
        currentAudio = null;
      }
      resolve(r);
    };
    cancelClipPlayback = () => finish({ ok: true, reason: "dihentikan", via: "clip" });
    const playNext = () => {
      if (mine !== session) return finish({ ok: true, reason: "dihentikan", via: "clip" });
      if (index >= files.length) return finish({ ok: true, reason: "", via: "clip" });
      const a = new Audio(CLIP_BASE + files[index++]);
      currentAudio = a;
      timer = window.setTimeout(() => finish({ ok: false, reason: "klip terlalu lama" }), 30000);
      a.onended = () => {
        clearTimeout(timer);
        currentAudio = null;
        playNext();
      };
      a.onerror = () => finish({ ok: false, reason: "klip gagal dimuat" });
      // NotAllowedError = WebView memblokir autoplay (lihat catatan di tauri.conf.json)
      a.play().catch((e) => finish({ ok: false, reason: String((e as { name?: string })?.name ?? e) }));
    };
    playNext();
  });
}

/** Hentikan suara yang sedang diputar (klip maupun Web Speech). */
export function stopSpeaking() {
  session++;
  cancelClipPlayback?.();
  try { currentAudio?.pause(); } catch { /* abaikan */ }
  currentAudio = null;
  try { window.speechSynthesis?.cancel(); } catch { /* abaikan */ }
}

// ---------- fallback: suara sistem ----------
const matchesLang = (v: SpeechSynthesisVoice, lang: Lang) => {
  if (!v.name || /undefined/i.test(v.name)) return false; // Edge kadang mengembalikan entri rusak "Microsoft undefined ..."
  if (lang === "id") return /^id(?:[-_]|$)/i.test(v.lang) || /(?:bahasa\s+indonesia|indonesian)/i.test(v.name);
  return new RegExp(`^${lang}(?:[-_]|$)`, "i").test(v.lang);
};

// suara neural ("Natural" / "Online") jauh lebih natural daripada suara offline lama; en-US lebih disukai
const voiceScore = (v: SpeechSynthesisVoice) =>
  (/natural/i.test(v.name) ? 4 : 0) + (/online/i.test(v.name) ? 2 : 0) + (/gadis|aria|jenny|guy/i.test(v.name) ? 1 : 0) + (/^en[-_]US/i.test(v.lang) ? 1 : 0);

const findVoice = (voices: SpeechSynthesisVoice[], lang: Lang) =>
  voices.filter((v) => matchesLang(v, lang)).sort((a, b) => voiceScore(b) - voiceScore(a))[0] ?? null;

// Tepat setelah boot / start, daftar suara Windows sering kosong beberapa detik. Tunggu sampai
// suara Indonesia muncul (polling + "voiceschanged"), bukan menyerah pada pengecekan pertama.
function waitForIndonesianVoice(timeoutMs = 9000): Promise<SpeechSynthesisVoice | null> {
  const synth = window.speechSynthesis;
  return new Promise((resolve) => {
    const check = () => {
      const v = findVoice(synth.getVoices(), "id");
      if (v) { cleanup(); resolve(v); }
    };
    const poll = window.setInterval(check, 300);
    const timer = window.setTimeout(() => { cleanup(); resolve(findVoice(synth.getVoices(), "id")); }, timeoutMs);
    function cleanup() { clearInterval(poll); clearTimeout(timer); synth.removeEventListener("voiceschanged", check); }
    synth.addEventListener("voiceschanged", check);
    check();
  });
}

function utter(text: string, voice: SpeechSynthesisVoice, lang: Lang, preset: VoicePreset): Promise<SpeakResult> {
  const synth = window.speechSynthesis;
  return new Promise((resolve) => {
    let started = false, done = false;
    const finish = (r: SpeakResult) => { if (!done) { done = true; clearTimeout(watchdog); clearTimeout(hard); resolve(r); } };
    const u = new SpeechSynthesisUtterance(text);
    u.lang = lang === "id" ? "id-ID" : voice.lang || (lang === "fr" ? "fr-FR" : "en-US");
    u.voice = voice; u.rate = preset.sysRate; u.pitch = preset.sysPitch;
    u.onstart = () => { started = true; };
    u.onend = () => finish({ ok: true, reason: "", via: "system" });
    u.onerror = (e) => finish({ ok: false, reason: e.error || "error" });
    const watchdog = window.setTimeout(() => {
      if (!started && !synth.speaking && !synth.pending) finish({ ok: false, reason: "audio tidak mulai diputar" });
    }, 7000);
    const hard = window.setTimeout(() => finish({ ok: true, reason: "", via: "system" }), 30000);
    try { synth.resume(); synth.speak(u); }
    catch (error) { finish({ ok: false, reason: String(error) }); }
  });
}

async function speakSystem(text: string, preset: VoicePreset, mine: number): Promise<SpeakResult> {
  const synth = window.speechSynthesis;
  if (!synth || typeof SpeechSynthesisUtterance === "undefined") return { ok: false, reason: "Web Speech tidak tersedia" };
  const idVoice = await waitForIndonesianVoice();
  if (mine !== session) return { ok: true, reason: "dihentikan", via: "system" };
  if (!idVoice) {
    const list = synth.getVoices().map((v) => `${v.name} (${v.lang})`).join(", ") || "belum ada voice yang dimuat";
    return { ok: false, reason: `voice id-ID tidak ditemukan; tersedia: ${list}` };
  }
  const voices = synth.getVoices();
  for (const seg of splitByLanguage(text)) {
    if (mine !== session) return { ok: true, reason: "dihentikan", via: "system" }; // diganti sapaan lain
    // voice Inggris/Prancis belum terpasang? pakai voice Indonesia saja daripada diam
    const voice = seg.lang === "id" ? idVoice : findVoice(voices, seg.lang) ?? idVoice;
    const r = await utter(seg.text, voice, seg.lang, preset);
    if (mine !== session) return { ok: true, reason: "dihentikan", via: "system" };
    if (!r.ok) return r;
  }
  return { ok: true, reason: "", via: "system" };
}

// ---------- API ----------
/** Ucapkan teks. `voiceId` = id preset (lihat greeting-lang.ts); kosong = pilihan di Pengaturan. */
export async function speak(text: string, voiceId?: string): Promise<SpeakResult> {
  stopSpeaking();
  const mine = session;
  const preset = getPreset(voiceId ?? readVoiceId());
  if (preset.voices) {
    const manifest = await loadManifest();
    if (mine !== session) return { ok: true, reason: "dihentikan" };
    const files = manifest[`${preset.id}|${norm(text)}`];
    if (files?.length) {
      const r = await playClips(files, mine);
      if (r.ok) return r;
      console.warn("[Greeting] Klip gagal, pindah ke suara sistem:", r.reason);
    }
  }
  if (mine !== session) return { ok: true, reason: "dihentikan" };
  return speakSystem(text, preset, mine);
}
