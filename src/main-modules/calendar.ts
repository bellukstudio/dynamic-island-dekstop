// Kalender bulanan penuh dengan hari libur & peringatan Indonesia: tampilan "cal" yang diperluas
// dari halaman jam (klik tanggal di halaman jam). Membuka/menutup tampilan diatur main.ts;
// file ini menggambar kisi, sorotan libur, dan keterangan di bawah kisi.
import { eventsOn, hasYearData, type Ev } from "./holidays";

const WEEK_STARTS_ON = 0; // 0 = Minggu pertama (umum di kalender Indonesia), 1 = Senin pertama

const fTitle = new Intl.DateTimeFormat("id-ID", { month: "long", year: "numeric" });                    // Oktober 2026
const fWd = new Intl.DateTimeFormat("id-ID", { weekday: "short" });                                     // Min, Sen, ...
const fFoot = new Intl.DateTimeFormat("id-ID", { weekday: "long", day: "numeric", month: "long" });    // Senin, 17 Agustus
const fTop = new Intl.DateTimeFormat("id-ID", { weekday: "long", day: "numeric", month: "long" });     // Senin, 26 Oktober (bilah atas)

let viewYear = new Date().getFullYear();
let viewMonth = new Date().getMonth();
let selected: Date | null = null;
let renderedToday = "";

export type Cell = { date: Date; inMonth: boolean };

// 42 sel (6 minggu) supaya tinggi kalender selalu sama; hari di luar bulan ikut ditampilkan redup
export function cellsFor(year: number, month: number): Cell[] {
    const first = new Date(year, month, 1);
    const lead = (first.getDay() - WEEK_STARTS_ON + 7) % 7;
    return Array.from({ length: 42 }, (_, i) => {
        const date = new Date(year, month, 1 - lead + i);
        return { date, inMonth: date.getMonth() === month };
    });
}

const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

// Pilihan awal: hari ini kalau ada peringatan; kalau tidak, peringatan terdekat berikutnya di bulan itu
function pickDefault() {
    const now = new Date();
    const inCurrent = now.getFullYear() === viewYear && now.getMonth() === viewMonth;
    const last = new Date(viewYear, viewMonth + 1, 0).getDate();
    for (let d = inCurrent ? now.getDate() : 1; d <= last; d++) {
        const date = new Date(viewYear, viewMonth, d);
        if (eventsOn(date).length) { selected = date; return; }
    }
    selected = inCurrent ? now : null;
}

function renderFooter() {
    const dot = document.getElementById("cal-dot");
    const dateEl = document.getElementById("cal-ev-date");
    const nameEl = document.getElementById("cal-ev-name");
    if (!dot || !dateEl || !nameEl) return;

    if (!selected) {
        dot.dataset.kind = "none";
        dateEl.textContent = "Tidak ada hari besar";
        nameEl.textContent = "Pilih tanggal untuk melihat keterangan";
        nameEl.title = "";
        return;
    }
    const evs = eventsOn(selected);
    dateEl.textContent = fFoot.format(selected);
    if (evs.length) {
        dot.dataset.kind = evs[0].kind;
        nameEl.textContent = evs.map((e) => e.name).join(" · ");
    } else {
        dot.dataset.kind = "none";
        nameEl.textContent = hasYearData(selected.getFullYear())
            ? "Tidak ada hari besar"
            : "Libur keagamaan tahun ini belum tersedia";
    }
    nameEl.title = nameEl.textContent ?? "";
}

export function renderCalendar(animate = true) {
    const grid = document.getElementById("cal-grid");
    const title = document.getElementById("cal-title");
    if (!grid || !title) return;

    const today = new Date();
    renderedToday = today.toDateString();
    title.textContent = fTitle.format(new Date(viewYear, viewMonth, 1));
    const top = document.getElementById("cal-today");
    if (top) top.textContent = fTop.format(today);

    grid.replaceChildren();
    for (const c of cellsFor(viewYear, viewMonth)) {
        const d = document.createElement("div");
        d.className = "cal-d";
        d.textContent = String(c.date.getDate());

        if (!c.inMonth) d.classList.add("out");
        if (c.date.getDay() === 0) d.classList.add("sun");
        if (sameDay(c.date, today)) d.classList.add("today");
        if (selected && sameDay(c.date, selected)) d.classList.add("sel");

        const evs: Ev[] = eventsOn(c.date);
        if (evs.length) {
            d.classList.add(`ev-${evs[0].kind}`); // jenis terpenting menentukan warna: libur > cuti > peringatan
            d.title = evs.map((e) => e.name).join("\n"); // tooltip
        }
        d.addEventListener("click", (e) => {
            e.stopPropagation();
            selected = c.date;
            if (!c.inMonth) { // klik tanggal bulan lain = pindah ke bulan itu
                viewYear = c.date.getFullYear();
                viewMonth = c.date.getMonth();
                renderCalendar(true);
            } else {
                renderCalendar(false);
            }
        });
        grid.appendChild(d);
    }
    renderFooter();

    if (animate) { // animasi singkat hanya saat bulan berganti, bukan saat memilih tanggal
        grid.classList.remove("flip");
        void grid.offsetWidth;
        grid.classList.add("flip");
    }
}

export function shiftMonth(delta: number) {
    const d = new Date(viewYear, viewMonth + delta, 1);
    viewYear = d.getFullYear();
    viewMonth = d.getMonth();
    pickDefault();
    renderCalendar(true);
}

export function goToday() {
    const now = new Date();
    viewYear = now.getFullYear();
    viewMonth = now.getMonth();
    pickDefault();
    renderCalendar(true);
}

export function initCalendar() {
    const wd = document.getElementById("cal-wd");
    if (wd) {
        wd.replaceChildren();
        for (let i = 0; i < 7; i++) {
            const dow = (WEEK_STARTS_ON + i) % 7; // 0 = Minggu
            const s = document.createElement("span");
            if (dow === 0) s.classList.add("sun");
            s.textContent = fWd.format(new Date(2023, 0, 1 + dow)); // 1 Jan 2023 = Minggu
            wd.appendChild(s);
        }
    }

    const stop = (fn: () => void) => (e: Event) => { e.stopPropagation(); fn(); };
    document.getElementById("cal-prev")?.addEventListener("click", stop(() => shiftMonth(-1)));
    document.getElementById("cal-next")?.addEventListener("click", stop(() => shiftMonth(1)));
    document.getElementById("cal-title")?.addEventListener("click", stop(goToday)); // klik judul = kembali ke bulan ini

    // scroll mouse / touchpad di atas kisi = ganti bulan
    let lock = false;
    document.getElementById("cal-grid")?.addEventListener("wheel", (e) => {
        const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
        if (Math.abs(d) < 10 || lock) return;
        lock = true;
        setTimeout(() => { lock = false; }, 250);
        shiftMonth(d > 0 ? 1 : -1);
    }, { passive: true });

    // lewat tengah malam: sorotan "hari ini" ikut pindah
    setInterval(() => {
        if (renderedToday && renderedToday !== new Date().toDateString()) renderCalendar(false);
    }, 30_000);

    pickDefault();
    renderCalendar(false);
}