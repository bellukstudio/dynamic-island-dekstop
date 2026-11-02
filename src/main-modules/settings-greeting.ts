// Jembatan untuk settings.html (skrip klasik tidak bisa import modul): membuka speak() milik
// greeting-voice.ts, jadi tombol "Coba ucapkan" memakai mesin suara yang sama dengan greeting asli.
import { speak, stopSpeaking } from "./greeting-voice";

(window as unknown as { __notchGreetingVoice: unknown }).__notchGreetingVoice = { speak, stop: stopSpeaking };
