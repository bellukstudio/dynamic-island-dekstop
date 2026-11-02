// Preset suara + pemecah bahasa untuk greeting. Dipakai bersama oleh:
//   - greeting-voice.ts (runtime, fallback suara sistem per-bahasa)
//   - scripts/gen-greeting-audio.ts (membuat klip: tiap potongan bahasa memakai voice-nya sendiri)
//   - greeting.ts (validasi pilihan voice)
// Masalah yang diselesaikan: voice id-ID membaca kata Inggris dengan lafal Indonesia
// ("bug" -> "bug" seperti "bugar", "Welcome back" -> "wel-kom bak"). Solusinya kalimat dipecah per
// bahasa, tiap potongan dibaca voice native-nya, lalu disambung jadi satu klip.

export type Lang = "id" | "en" | "fr";
export type Seg = { lang: Lang; text: string };

export type VoicePreset = {
  id: string;                    // disimpan di localStorage ("notch.startupGreeting".voice); samakan dengan settings.html
  label: string;
  voices?: Record<Lang, string>; // voice edge-tts per bahasa; kosong = hanya suara sistem
  pitchHz: Record<Lang, number>; // geser pitch (edge-tts); positif = lebih tinggi / "imut"
  ratePct: number;               // kecepatan edge-tts, persen
  sysPitch: number;              // pitch Web Speech (0..2) untuk fallback teks kustom
  sysRate: number;               // rate Web Speech untuk fallback
};

export const DEFAULT_VOICE_ID = "cewek";

export const PRESETS: VoicePreset[] = [
  {
    id: "cewek", label: "Cewek natural (Gadis)",
    voices: { id: "id-ID-GadisNeural", en: "en-US-AriaNeural", fr: "fr-FR-DeniseNeural" },
    pitchHz: { id: 0, en: 0, fr: 0 }, ratePct: -5, sysPitch: 1, sysRate: 0.97,
  },
  {
    id: "cowok", label: "Cowok natural (Ardi)",
    voices: { id: "id-ID-ArdiNeural", en: "en-US-GuyNeural", fr: "fr-FR-HenriNeural" },
    pitchHz: { id: 0, en: 0, fr: 0 }, ratePct: -5, sysPitch: 1, sysRate: 0.97,
  },
  {
    id: "anime-cewek", label: "Anime cewek imut",
    // Gadis dinaikkan pitch-nya; bagian Inggris pakai AnaNeural (suara anak, cocok untuk karakter imut)
    voices: { id: "id-ID-GadisNeural", en: "en-US-AnaNeural", fr: "fr-FR-DeniseNeural" },
    pitchHz: { id: 40, en: 12, fr: 40 }, ratePct: 8, sysPitch: 1.55, sysRate: 1.05,
  },
  {
    id: "anime-cowok", label: "Anime cowok (shonen)",
    voices: { id: "id-ID-ArdiNeural", en: "en-US-AndrewNeural", fr: "fr-FR-HenriNeural" },
    pitchHz: { id: 22, en: 18, fr: 22 }, ratePct: 10, sysPitch: 1.3, sysRate: 1.08,
  },
  {
    id: "system", label: "Suara sistem (Windows)",
    pitchHz: { id: 0, en: 0, fr: 0 }, ratePct: 0, sysPitch: 1, sysRate: 0.97,
  },
];

export const getPreset = (id?: string | null): VoicePreset =>
  PRESETS.find((p) => p.id === id) ?? PRESETS.find((p) => p.id === DEFAULT_VOICE_ID)!;
export const isVoiceId = (id: unknown): id is string => PRESETS.some((p) => p.id === id);

/** Pilihan voice dari Pengaturan (localStorage). */
export function readVoiceId(): string {
  try {
    const saved = JSON.parse(localStorage.getItem("notch.startupGreeting") || "null");
    return isVoiceId(saved?.voice) ? saved.voice : DEFAULT_VOICE_ID;
  } catch { return DEFAULT_VOICE_ID; }
}

export const norm = (s: string) => s.trim().replace(/\s+/g, " "); // harus sama dengan manifest & skrip generator

// ---------- deteksi bahasa per kata ----------
// Kata yang tidak ada di daftar mana pun dianggap Indonesia. Kalau ada kata Inggris yang masih dibaca
// salah, cukup tambahkan ke EN_WORDS (huruf kecil), lalu jalankan ulang skrip generator.
const words = (s: string) => new Set(s.toLowerCase().split(/\s+/).filter(Boolean));

const EN_WORDS = words(`
  the a an is are was be you your my i it its not and or of to for in on at with this here there then than
  who's what's it's i'm you're don't can't let's that's
  hello hi hey good morning afternoon evening night day today tomorrow welcome back thanks thank please sorry
  yes okay great nice cool awesome amazing love life time work play game gamer hustle focus break coffee energy
  power mode level up unlocked unlock achievement boss king queen hero legend master pro user admin system
  update upgrade install download upload app apps web website internet wifi online offline login logout password
  email message chat call meeting deadline project task todo team manager office home mood vibe vibes chill relax
  lets go keep going stay strong happy smile ready start begin finish done complete next previous new old free
  fast slow big small hot cold light dark open close save load loading reload restart reboot shutdown sleep wake
  awake crash bug bugs fix debug error warning success fail failed code coder coding developer programmer
  software hardware frontend backend fullstack full stack server client database api cloud commit push pull merge
  branch deploy build compile run runtime test testing release version trace local notch laptop computer keyboard
  mouse screen tab window windows alt ctrl shift enter timeline background music character energy twist plot
  hunter detected activated closing last found works machine feelings warrior engineer progress world look
  legend chief bro dude guys sleepy tired overthinking productive productivity
`);
const FR_WORDS = words("bonjour bonsoir merci");
// frasa yang harus dibaca Inggris walau kata tunggalnya ambigu dengan bahasa Indonesia ("main" = bermain)
const EN_PHRASES = [["main", "character"]];

const stripEdges = (t: string) => t.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");

function tokenLang(token: string): Lang | null {
  const raw = stripEdges(token.replace(/[’‘]/g, "'"));
  if (!raw || !/\p{L}/u.test(raw)) return null;                  // angka/simbol: ikut tetangganya
  if (/^[A-Z]{2,5}$/.test(raw)) return "en";                     // NPC, CPU, CCTV
  const w = raw.toLowerCase();
  if (FR_WORDS.has(w)) return "fr";
  if (EN_WORDS.has(w)) return "en";
  if (w.includes("-")) {                                         // copy-paste, full-stack, alt-tab; "di-fix" tetap id
    const parts = w.split("-").filter(Boolean);
    if (parts.length > 1 && parts.every((p) => EN_WORDS.has(p))) return "en";
  }
  return "id";
}

export function splitByLanguage(text: string): Seg[] {
  const tokens = norm(text).split(" ").filter(Boolean);
  if (!tokens.length) return [];
  const cores = tokens.map((t) => stripEdges(t).toLowerCase());
  const langs: (Lang | null)[] = tokens.map(tokenLang);

  for (const phrase of EN_PHRASES) {
    for (let i = 0; i + phrase.length <= cores.length; i++) {
      if (phrase.every((w, k) => cores[i + k] === w)) phrase.forEach((_, k) => (langs[i + k] = "en"));
    }
  }
  // token netral mewarisi bahasa sebelumnya; yang di awal kalimat mengikuti bahasa berikutnya
  let prev: Lang | null = null;
  for (let i = 0; i < langs.length; i++) { if (langs[i]) prev = langs[i]; else if (prev) langs[i] = prev; }
  let next: Lang = "id";
  for (let i = langs.length - 1; i >= 0; i--) { if (langs[i]) next = langs[i]!; else langs[i] = next; }

  const segs: Seg[] = [];
  tokens.forEach((t, i) => {
    const lang = langs[i]!;
    const last = segs[segs.length - 1];
    if (last && last.lang === lang) last.text += " " + t;
    else segs.push({ lang, text: t });
  });
  return segs;
}
