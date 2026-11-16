// Jam & tanggal.
//  - halaman hub "clock": jam besar di KIRI, tanggal di KANAN
//  - tampilan idle "clock": jam di kiri notch, tanggal di kanan (menggantikan mata)
// Pergantian mata <-> jam diputuskan di main.ts (saat hover dilepas dari halaman jam);
// file ini hanya menggambar teks dan mengingat pilihan terakhir.
export type Face = "eyes" | "clock";

const KEY = "notch.idleFace";

export function loadFace(): Face {
    try { return localStorage.getItem(KEY) === "clock" ? "clock" : "eyes"; } catch { return "eyes"; }
}
export function saveFace(f: Face) {
    try { localStorage.setItem(KEY, f); } catch { /* penyimpanan tidak tersedia: pilihan tidak diingat */ }
}

const fDay = new Intl.DateTimeFormat("id-ID", { weekday: "long" });                          // Senin
const fFull = new Intl.DateTimeFormat("id-ID", { day: "numeric", month: "long", year: "numeric" }); // 26 Oktober 2026
const fShort = new Intl.DateTimeFormat("id-ID", { weekday: "short", day: "numeric", month: "short" }); // Sen, 26 Okt
const p2 = (n: number) => String(n).padStart(2, "0");

// hanya menyentuh DOM kalau teksnya benar-benar berubah
const cache = new Map<string, string>();
function put(id: string, text: string) {
    if (cache.get(id) === text) return;
    cache.set(id, text);
    const el = document.getElementById(id);
    if (el) el.textContent = text;
}

export function paintClock(now = new Date()) {
    const hm = `${p2(now.getHours())}:${p2(now.getMinutes())}`;
    put("cl-time", `${hm}:${p2(now.getSeconds())}`); // idle: jam + detik (kiri)
    put("cl-date", fShort.format(now));         // idle: tanggal (kanan)
    put("ck-time", hm);                                                   // halaman hub: jam besar (kiri)
    put("ck-sec", p2(now.getSeconds()));
    put("ck-day", fDay.format(now));            // halaman hub: tanggal (kanan)
    put("ck-full", fFull.format(now));
}

export function startClock() {
    const tick = () => {
        paintClock();
        setTimeout(tick, 1000 - (Date.now() % 1000) + 5); // selaras dengan pergantian detik
    };
    tick();
}