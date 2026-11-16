import { invoke } from "@tauri-apps/api/core";

export type Kind = "libur" | "cuti" | "hari";
export type Ev = { name: string; kind: Kind };

// ---------- data bawaan (cadangan saat offline / belum pernah terhubung) ----------
// Hari libur tetap + hari peringatan nasional. Berlaku setiap tahun.
const FIXED: Array<[number, number, string, Kind]> = [
    [1, 1, "Tahun Baru Masehi", "libur"],
    [5, 1, "Hari Buruh Internasional", "libur"],
    [6, 1, "Hari Lahir Pancasila", "libur"],
    [8, 17, "Proklamasi Kemerdekaan RI", "libur"],
    [12, 25, "Hari Raya Natal (Kelahiran Yesus Kristus)", "libur"],

    [2, 9, "Hari Pers Nasional", "hari"],
    [3, 1, "Hari Penegakan Kedaulatan Negara", "hari"],
    [3, 9, "Hari Musik Nasional", "hari"],
    [4, 21, "Hari Kartini", "hari"],
    [5, 2, "Hari Pendidikan Nasional", "hari"],
    [5, 20, "Hari Kebangkitan Nasional", "hari"],
    [6, 29, "Hari Keluarga Nasional", "hari"],
    [7, 1, "Hari Bhayangkara", "hari"],
    [7, 23, "Hari Anak Nasional", "hari"],
    [8, 14, "Hari Pramuka", "hari"],
    [9, 9, "Hari Olahraga Nasional", "hari"],
    [9, 23, "Hari Maritim Nasional", "hari"],
    [9, 24, "Hari Tani Nasional", "hari"],
    [10, 1, "Hari Kesaktian Pancasila", "hari"],
    [10, 2, "Hari Batik Nasional", "hari"],
    [10, 5, "Hari TNI", "hari"],
    [10, 22, "Hari Santri", "hari"],
    [10, 28, "Hari Sumpah Pemuda", "hari"],
    [11, 10, "Hari Pahlawan", "hari"],
    [11, 12, "Hari Kesehatan Nasional", "hari"],
    [11, 25, "Hari Guru Nasional", "hari"],
    [12, 19, "Hari Bela Negara", "hari"],
    [12, 22, "Hari Ibu", "hari"],
];

// Libur keagamaan & cuti bersama (berubah tiap tahun). Hanya dipakai kalau data dari internet
// untuk tahun itu belum ada; kalau sudah ada, data internet menggantikannya.
const BY_YEAR: Record<number, Array<[string, string, Kind]>> = {
    2026: [
        ["01-16", "Isra Mikraj Nabi Muhammad saw.", "libur"],
        ["02-16", "Cuti Bersama Tahun Baru Imlek", "cuti"],
        ["02-17", "Tahun Baru Imlek 2577 Kongzili", "libur"],
        ["03-18", "Cuti Bersama Hari Suci Nyepi", "cuti"],
        ["03-19", "Hari Suci Nyepi (Tahun Baru Saka 1948)", "libur"],
        ["03-20", "Cuti Bersama Idulfitri 1447 H", "cuti"],
        ["03-21", "Idulfitri 1447 H", "libur"],
        ["03-22", "Idulfitri 1447 H", "libur"],
        ["03-23", "Cuti Bersama Idulfitri 1447 H", "cuti"],
        ["03-24", "Cuti Bersama Idulfitri 1447 H", "cuti"],
        ["04-03", "Wafat Yesus Kristus", "libur"],
        ["04-05", "Kebangkitan Yesus Kristus (Paskah)", "libur"],
        ["05-14", "Kenaikan Yesus Kristus", "libur"],
        ["05-15", "Cuti Bersama Kenaikan Yesus Kristus", "cuti"],
        ["05-27", "Iduladha 1447 H", "libur"],
        ["05-28", "Cuti Bersama Iduladha 1447 H", "cuti"],
        ["05-31", "Hari Raya Waisak 2570 BE", "libur"],
        ["06-16", "1 Muharam, Tahun Baru Islam 1448 H", "libur"],
        ["08-25", "Maulid Nabi Muhammad saw.", "libur"],
        ["12-24", "Cuti Bersama Natal", "cuti"],
    ],
    2027: [
        ["01-05", "Isra Mikraj Nabi Muhammad saw. 1448 H", "libur"],
        ["02-05", "Cuti Bersama Tahun Baru Imlek", "cuti"],
        ["02-06", "Tahun Baru Imlek 2578 Kongzili", "libur"],
        ["03-08", "Hari Suci Nyepi (Tahun Baru Saka 1949)", "libur"],
        ["03-09", "Cuti Bersama Idulfitri 1448 H", "cuti"],
        ["03-10", "Idulfitri 1448 H", "libur"],
        ["03-11", "Idulfitri 1448 H", "libur"],
        ["03-12", "Cuti Bersama Idulfitri 1448 H", "cuti"],
        ["03-15", "Cuti Bersama Idulfitri 1448 H", "cuti"],
        ["03-25", "Cuti Bersama Wafat Yesus Kristus", "cuti"],
        ["03-26", "Wafat Yesus Kristus", "libur"],
        ["03-28", "Kebangkitan Yesus Kristus (Paskah)", "libur"],
        ["05-06", "Kenaikan Yesus Kristus", "libur"],
        ["05-17", "Iduladha 1448 H", "libur"],
        ["05-18", "Cuti Bersama Iduladha 1448 H", "cuti"],
        ["05-19", "Cuti Bersama Waisak", "cuti"],
        ["05-20", "Hari Raya Waisak 2571 BE", "libur"],
        ["06-06", "1 Muharam, Tahun Baru Islam 1449 H", "libur"],
        ["08-15", "Maulid Nabi Muhammad saw.", "libur"],
        ["12-24", "Cuti Bersama Natal", "cuti"],
        ["12-26", "Isra Mikraj Nabi Muhammad saw. 1449 H", "libur"],
    ],
};

// ---------- data dari internet (diambil lewat Rust: holidays.rs) ----------
type Remote = { date: string; name: string; libur: boolean };
// tahun -> "MM-DD" -> daftar acara
const remoteByYear = new Map<number, Map<string, Ev[]>>();

// sumber hanya menandai "libur" atau bukan; cuti bersama dikenali dari namanya
const isCuti = (name: string) => /^\s*cuti\s+bersama|joint\s+holiday/i.test(name);

const ORDER: Record<Kind, number> = { libur: 0, cuti: 1, hari: 2 };
const p2 = (n: number) => String(n).padStart(2, "0");
const cache = new Map<number, Map<string, Ev[]>>();

function ingest(items: Remote[]) {
    const next = new Map<number, Map<string, Ev[]>>();
    for (const it of items) {
        const m = /^(\d{4})-(\d{2}-\d{2})$/.exec(it.date);
        if (!m || !it.name) continue;
        const year = Number(m[1]);
        const kind: Kind = !it.libur ? "hari" : isCuti(it.name) ? "cuti" : "libur";
        let byDay = next.get(year);
        if (!byDay) { byDay = new Map(); next.set(year, byDay); }
        const list = byDay.get(m[2]) ?? [];
        if (!list.some((e) => e.name === it.name)) list.push({ name: it.name, kind });
        byDay.set(m[2], list);
    }
    remoteByYear.clear();
    for (const [y, v] of next) remoteByYear.set(y, v);
    cache.clear(); // indeks per tahun dibangun ulang saat dibutuhkan
}

function build(year: number): Map<string, Ev[]> {
    const m = new Map<string, Ev[]>();
    const add = (md: string, ev: Ev) => {
        const list = m.get(md);
        if (!list) m.set(md, [ev]);
        else if (!list.some((e) => e.name === ev.name)) list.push(ev);
    };
    for (const [mo, d, name, kind] of FIXED) add(`${p2(mo)}-${p2(d)}`, { name, kind });

    const remote = remoteByYear.get(year);
    if (remote) {
        for (const [md, evs] of remote) {
            // tanggal ini sudah dicakup data internet: libur bawaan di tanggal itu digantikan (hindari nama ganda)
            if (evs.some((e) => e.kind !== "hari")) {
                const keep = (m.get(md) ?? []).filter((e) => e.kind === "hari");
                if (keep.length) m.set(md, keep); else m.delete(md);
            }
            for (const e of evs) add(md, e);
        }
    } else {
        for (const [md, name, kind] of BY_YEAR[year] ?? []) add(md, { name, kind });
    }
    for (const list of m.values()) list.sort((a, b) => ORDER[a.kind] - ORDER[b.kind]);
    return m;
}

export function eventsOn(d: Date): Ev[] {
    const y = d.getFullYear();
    let idx = cache.get(y);
    if (!idx) { idx = build(y); cache.set(y, idx); }
    return idx.get(`${p2(d.getMonth() + 1)}-${p2(d.getDate())}`) ?? [];
}

export const hasYearData = (year: number) => year in BY_YEAR || remoteByYear.has(year);

// Ambil data terbaru dari internet (Rust menyimpan cache & memakainya saat offline).
// onDone dipanggil hanya kalau ada data baru, supaya kalender bisa digambar ulang.
let inflight = false;
export async function refreshHolidays(onDone: () => void) {
    if (inflight) return;
    inflight = true;
    try {
        const items = await invoke<Remote[]>("holidays_fetch");
        if (Array.isArray(items) && items.length) {
            ingest(items);
            onDone();
        }
    } catch (e) {
        console.warn("[holidays] memakai data bawaan:", e);
    } finally {
        inflight = false;
    }
}