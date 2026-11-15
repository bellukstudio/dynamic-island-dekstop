//! Kalender bulanan dan hari libur Indonesia.
//!
//! Padanan `calendar.ts` + `holidays.ts` + `src-tauri/src/holiday.rs`. Logika murni (tanpa UI):
//! kisi 42 sel, nama bulan/hari bahasa Indonesia, penggabungan data bawaan dengan data internet,
//! dan cache `holidays.json` yang formatnya sama dengan versi Tauri (kedua aplikasi saling memakai).
//! Pengunduhan ada di `fetch` dan hanya tersedia dengan fitur `net`.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::holidays_data::{BY_YEAR, FIXED};

/// Minggu pertama (umum di kalender Indonesia). 1 = Senin pertama.
pub const WEEK_STARTS_ON: u32 = 0;

pub const MONTHS: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember",
];
/// Indeks 0 = Minggu.
pub const WEEKDAYS: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
pub const WEEKDAYS_SHORT: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];

// ---------------------------------------------------------------- tanggal

/// Hari sejak 1970-01-01 untuk tanggal sipil (algoritma Howard Hinnant).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Kebalikan `days_from_civil`: (tahun, bulan 1-12, tanggal).
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400) as i32 + i32::from(m <= 2), m, d)
}

/// Hari dalam seminggu: 0 = Minggu ... 6 = Sabtu.
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) + 4).rem_euclid(7) as u32 // 1970-01-01 = Kamis
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Satu tanggal sipil.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Date {
        Date { y, m, d }
    }
    pub fn weekday(self) -> u32 {
        weekday(self.y, self.m, self.d)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub date: Date,
    pub in_month: bool,
}

/// 42 sel (6 minggu) supaya tinggi kalender selalu sama; hari di luar bulan ikut tampil redup.
/// `month` = 1..=12.
pub fn cells_for(year: i32, month: u32) -> Vec<Cell> {
    let lead = (weekday(year, month, 1) + 7 - WEEK_STARTS_ON) % 7;
    let first = days_from_civil(year, month, 1);
    (0..42)
        .map(|i| {
            let (y, m, d) = civil_from_days(first - i64::from(lead) + i);
            Cell { date: Date::new(y, m, d), in_month: m == month && y == year }
        })
        .collect()
}

/// "Oktober 2026"
pub fn title(year: i32, month: u32) -> String {
    format!("{} {}", MONTHS[(month - 1) as usize], year)
}

/// "Senin, 17 Agustus"
pub fn long_date(d: Date) -> String {
    format!("{}, {} {}", WEEKDAYS[d.weekday() as usize], d.d, MONTHS[(d.m - 1) as usize])
}

/// Nama kolom hari sesuai awal pekan.
pub fn weekday_headers() -> [&'static str; 7] {
    let mut out = [""; 7];
    for (i, o) in out.iter_mut().enumerate() {
        *o = WEEKDAYS_SHORT[(i + WEEK_STARTS_ON as usize) % 7];
    }
    out
}

// ---------------------------------------------------------------- hari libur

/// Jenis penting menentukan warna: libur > cuti bersama > hari peringatan.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Libur,
    Cuti,
    Hari,
}

impl Kind {
    /// 0 = tidak ada, 1 = libur, 2 = cuti, 3 = peringatan (dipakai UI).
    pub fn code(self) -> i32 {
        match self {
            Kind::Libur => 1,
            Kind::Cuti => 2,
            Kind::Hari => 3,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ev {
    pub name: String,
    pub kind: Kind,
}

/// Butir data dari internet; sama dengan `Item` di holiday.rs Tauri.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub date: String, // "YYYY-MM-DD"
    pub name: String,
    pub libur: bool, // true = libur/cuti bersama, false = hari peringatan
}

/// Sumber hanya menandai "libur" atau bukan; cuti bersama dikenali dari namanya.
fn is_cuti(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n.starts_with("cuti bersama") || n.starts_with("cuti  bersama") || n.contains("joint holiday")
}

#[derive(Default)]
pub struct Holidays {
    /// tahun -> (bulan, tanggal) -> acara
    remote: HashMap<i32, HashMap<(u32, u32), Vec<Ev>>>,
}

impl Holidays {
    pub fn new() -> Holidays {
        Holidays::default()
    }

    /// Ganti seluruh data internet dengan `items`.
    pub fn ingest(&mut self, items: &[Item]) {
        let mut next: HashMap<i32, HashMap<(u32, u32), Vec<Ev>>> = HashMap::new();
        for it in items {
            let Some((y, m, d)) = parse_date(&it.date) else { continue };
            if it.name.is_empty() {
                continue;
            }
            let kind = if !it.libur {
                Kind::Hari
            } else if is_cuti(&it.name) {
                Kind::Cuti
            } else {
                Kind::Libur
            };
            let list = next.entry(y).or_default().entry((m, d)).or_default();
            if !list.iter().any(|e| e.name == it.name) {
                list.push(Ev { name: it.name.clone(), kind });
            }
        }
        self.remote = next;
    }

    /// Ada data libur keagamaan untuk tahun ini (bawaan atau internet)?
    pub fn has_year_data(&self, year: i32) -> bool {
        self.remote.contains_key(&year) || BY_YEAR.iter().any(|(y, ..)| *y == year)
    }

    /// Acara pada satu tanggal, jenis terpenting di depan. Data internet untuk tahun itu menggantikan
    /// libur bawaan di tanggal yang sama (hari peringatan bawaan tetap dipertahankan).
    pub fn events_on(&self, d: Date) -> Vec<Ev> {
        let mut out: Vec<Ev> = Vec::new();
        let add = |out: &mut Vec<Ev>, ev: Ev| {
            if !out.iter().any(|e| e.name == ev.name) {
                out.push(ev);
            }
        };
        for (m, day, name, kind) in FIXED {
            if (*m, *day) == (d.m, d.d) {
                add(&mut out, Ev { name: (*name).into(), kind: *kind });
            }
        }
        match self.remote.get(&d.y) {
            Some(by_day) => {
                if let Some(evs) = by_day.get(&(d.m, d.d)) {
                    if evs.iter().any(|e| e.kind != Kind::Hari) {
                        out.retain(|e| e.kind == Kind::Hari);
                    }
                    for e in evs {
                        add(&mut out, e.clone());
                    }
                }
            }
            None => {
                for (y, m, day, name, kind) in BY_YEAR {
                    if *y == d.y && (*m, *day) == (d.m, d.d) {
                        add(&mut out, Ev { name: (*name).into(), kind: *kind });
                    }
                }
            }
        }
        out.sort_by_key(|e| e.kind);
        out
    }
}

fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    if !valid_date(s) {
        return None;
    }
    Some((s[0..4].parse().ok()?, s[5..7].parse().ok()?, s[8..10].parse().ok()?))
}

/// "2026-03-21" -> true
fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Format sumber: `{ "2026-01-01": { "holiday": true, "summary": ["Hari Tahun Baru"] }, "info": {...} }`.
pub fn parse(text: &str) -> Result<Vec<Item>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("format tidak dikenal")?;
    let mut out: Vec<Item> = Vec::new();
    for (date, e) in obj {
        if !valid_date(date) {
            continue; // lewati kunci "info"
        }
        let libur = e["holiday"].as_bool().unwrap_or(false);
        for s in e["summary"].as_array().into_iter().flatten() {
            if let Some(name) = s.as_str().map(str::trim).filter(|n| !n.is_empty()) {
                out.push(Item { date: date.clone(), name: name.to_string(), libur });
            }
        }
    }
    if out.is_empty() {
        return Err("data kosong".into());
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

// ---------------------------------------------------------------- cache + unduh

pub const TTL_SECS: u64 = 12 * 3600;

#[derive(Serialize, Deserialize)]
struct Cache {
    fetched: u64, // detik sejak epoch
    items: Vec<Item>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Cache dari disk: (waktu unduh, butir). Berkas sama dengan `holidays.json` versi Tauri.
pub fn read_cache(path: &Path) -> Option<(u64, Vec<Item>)> {
    let c: Cache = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    Some((c.fetched, c.items))
}

pub fn write_cache(path: &Path, items: &[Item]) {
    let c = Cache { fetched: now_secs(), items: items.to_vec() };
    if let Ok(s) = serde_json::to_string(&c) {
        let _ = std::fs::write(path, s);
    }
}

/// Cache dianggap segar bila umurnya kurang dari 12 jam.
pub fn is_fresh(fetched: u64, now: u64) -> bool {
    now.saturating_sub(fetched) < TTL_SECS
}

// dicoba berurutan; yang kedua adalah cermin CDN dari berkas yang sama
#[cfg(feature = "net")]
const SOURCES: &[&str] = &[
    "https://raw.githubusercontent.com/guangrei/APIHariLibur_V2/main/calendar.json",
    "https://cdn.jsdelivr.net/gh/guangrei/APIHariLibur_V2@main/calendar.json",
];

/// Data terbaik yang ada: cache segar -> unduhan baru -> cache lama (offline). Error hanya bila
/// belum pernah berhasil sama sekali. Memblokir (jalankan di thread latar).
#[cfg(feature = "net")]
pub fn fetch(cache_file: &Path) -> Result<Vec<Item>, String> {
    let cached = read_cache(cache_file);
    if let Some((fetched, items)) = &cached {
        if is_fresh(*fetched, now_secs()) {
            return Ok(items.clone());
        }
    }
    let mut last = String::from("tidak ada sumber");
    for url in SOURCES {
        match crate::http::get_text(url, 15).and_then(|t| parse(&t)) {
            Ok(items) => {
                write_cache(cache_file, &items);
                return Ok(items);
            }
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    match cached {
        Some((_, items)) => Ok(items),
        None => Err(last),
    }
}

// ---------------------------------------------------------------- keadaan tampilan

/// Bulan yang sedang dilihat dan tanggal terpilih. Padanan variabel modul di calendar.ts.
pub struct View {
    pub year: i32,
    pub month: u32,
    pub selected: Option<Date>,
}

impl View {
    pub fn new(today: Date, hol: &Holidays) -> View {
        let mut v = View { year: today.y, month: today.m, selected: None };
        v.pick_default(today, hol);
        v
    }

    /// Pilihan awal: hari ini bila ada peringatan; kalau tidak, peringatan terdekat berikutnya
    /// di bulan itu; bila tidak ada sama sekali, hari ini (bulan berjalan) atau kosong.
    pub fn pick_default(&mut self, today: Date, hol: &Holidays) {
        let in_current = today.y == self.year && today.m == self.month;
        let last = days_in_month(self.year, self.month);
        let from = if in_current { today.d } else { 1 };
        for d in from..=last {
            let date = Date::new(self.year, self.month, d);
            if !hol.events_on(date).is_empty() {
                self.selected = Some(date);
                return;
            }
        }
        self.selected = in_current.then_some(today);
    }

    pub fn shift_month(&mut self, delta: i32, today: Date, hol: &Holidays) {
        let idx = self.year * 12 + (self.month as i32 - 1) + delta;
        self.year = idx.div_euclid(12);
        self.month = idx.rem_euclid(12) as u32 + 1;
        self.pick_default(today, hol);
    }

    pub fn go_today(&mut self, today: Date, hol: &Holidays) {
        self.year = today.y;
        self.month = today.m;
        self.pick_default(today, hol);
    }

    /// Klik sel `index` (0..42). Tanggal di bulan lain memindahkan tampilan ke bulan itu.
    pub fn select(&mut self, index: usize) {
        let cells = cells_for(self.year, self.month);
        let Some(c) = cells.get(index) else { return };
        self.selected = Some(c.date);
        if !c.in_month {
            self.year = c.date.y;
            self.month = c.date.m;
        }
    }

    /// (judul, tanggal terpilih, keterangan, jenis 0..3) untuk bagian bawah kalender.
    pub fn footer(&self, hol: &Holidays) -> (String, String, i32) {
        let Some(sel) = self.selected else {
            return ("Tidak ada hari besar".into(), "Pilih tanggal untuk melihat keterangan".into(), 0);
        };
        let evs = hol.events_on(sel);
        let date = long_date(sel);
        if let Some(first) = evs.first() {
            let names: Vec<&str> = evs.iter().map(|e| e.name.as_str()).collect();
            (date, names.join(" \u{00B7} "), first.kind.code())
        } else if hol.has_year_data(sel.y) {
            (date, "Tidak ada hari besar".into(), 0)
        } else {
            (date, "Libur keagamaan tahun ini belum tersedia".into(), 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hari_dalam_minggu_benar() {
        assert_eq!(weekday(1970, 1, 1), 4); // Kamis
        assert_eq!(weekday(2026, 11, 15), 0); // Minggu
        assert_eq!(weekday(2026, 8, 17), 1); // Senin
        assert_eq!(weekday(2000, 2, 29), 2); // Selasa
        assert_eq!(weekday(2024, 2, 29), 4); // Kamis
    }

    #[test]
    fn bolak_balik_hari_sipil() {
        for z in [-800_000i64, -1, 0, 1, 11_000, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn jumlah_hari_per_bulan() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn kisi_enam_minggu_dimulai_minggu() {
        // Oktober 2026 diawali Kamis -> empat sel sebelumnya dari September (27..30)
        let c = cells_for(2026, 10);
        assert_eq!(c.len(), 42);
        assert_eq!(c[0].date, Date::new(2026, 9, 27));
        assert!(!c[0].in_month);
        assert_eq!(c[4].date, Date::new(2026, 10, 1));
        assert!(c[4].in_month);
        assert_eq!(c.iter().filter(|x| x.in_month).count(), 31);
        assert!(c.iter().step_by(7).all(|x| x.date.weekday() == 0), "kolom pertama = Minggu");
    }

    #[test]
    fn kisi_lintas_tahun() {
        let c = cells_for(2026, 12);
        assert_eq!(c[41].date.y, 2027);
        let c = cells_for(2027, 1);
        assert!(c.iter().any(|x| !x.in_month && x.date.y == 2026));
    }

    #[test]
    fn judul_dan_tanggal_panjang() {
        assert_eq!(title(2026, 10), "Oktober 2026");
        assert_eq!(long_date(Date::new(2026, 8, 17)), "Senin, 17 Agustus");
        assert_eq!(weekday_headers()[0], "Min");
    }

    #[test]
    fn libur_bawaan_tetap_dan_tahunan() {
        let h = Holidays::new();
        let k = h.events_on(Date::new(2026, 8, 17));
        assert_eq!(k[0].kind, Kind::Libur);
        assert!(k[0].name.contains("Proklamasi"));
        let f = h.events_on(Date::new(2026, 3, 21));
        assert!(f.iter().any(|e| e.name.starts_with("Idulfitri") && e.kind == Kind::Libur));
        assert_eq!(h.events_on(Date::new(2026, 3, 20))[0].kind, Kind::Cuti);
        assert!(h.events_on(Date::new(2026, 7, 14)).is_empty());
        assert!(h.has_year_data(2026) && h.has_year_data(2027) && !h.has_year_data(2031));
    }

    #[test]
    fn data_internet_menggantikan_libur_bawaan_tapi_menyimpan_hari_peringatan() {
        let mut h = Holidays::new();
        h.ingest(&[
            Item { date: "2026-03-21".into(), name: "Idul Fitri 1447".into(), libur: true },
            Item { date: "2026-03-20".into(), name: "Cuti Bersama Idul Fitri".into(), libur: true },
            Item { date: "2026-03-09".into(), name: "Hari Musik (versi sumber)".into(), libur: false },
            Item { date: "rusak".into(), name: "x".into(), libur: true },
        ]);
        let ev = h.events_on(Date::new(2026, 3, 21));
        assert_eq!(ev.len(), 1, "libur bawaan tanggal itu digantikan: {ev:?}");
        assert_eq!(ev[0].name, "Idul Fitri 1447");
        assert_eq!(h.events_on(Date::new(2026, 3, 20))[0].kind, Kind::Cuti);
        // hari peringatan bawaan di tanggal yang sama tidak hilang
        let m = h.events_on(Date::new(2026, 3, 9));
        assert!(m.iter().any(|e| e.name == "Hari Musik Nasional"));
        assert!(m.iter().any(|e| e.name == "Hari Musik (versi sumber)"));
        // tanggal yang tidak ada di sumber tetap memakai libur tetap
        assert_eq!(h.events_on(Date::new(2026, 8, 17))[0].kind, Kind::Libur);
        // libur bawaan per tahun (BY_YEAR) tidak dipakai bila tahun itu ada data internet
        assert!(h.events_on(Date::new(2026, 4, 3)).is_empty());
    }

    #[test]
    fn parse_format_sumber() {
        let txt = r#"{"2026-01-01":{"holiday":true,"summary":["Hari Tahun Baru"]},
                      "2026-02-09":{"holiday":false,"summary":["Hari Pers", " "]},
                      "info":{"x":1}}"#;
        let items = parse(txt).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].date, "2026-01-01");
        assert!(items[0].libur && !items[1].libur);
        assert!(parse("[]").is_err());
        assert!(parse(r#"{"info":{}}"#).is_err());
    }

    #[test]
    fn cuti_dikenali_dari_nama() {
        let mut h = Holidays::new();
        h.ingest(&[Item { date: "2028-05-02".into(), name: "Cuti Bersama Waisak".into(), libur: true }]);
        assert_eq!(h.events_on(Date::new(2028, 5, 2))[0].kind, Kind::Cuti);
    }

    #[test]
    fn cache_ditulis_dan_dibaca_serta_masa_berlaku() {
        let path = std::env::temp_dir().join(format!("notch-holidays-test-{}.json", std::process::id()));
        let items = vec![Item { date: "2026-01-01".into(), name: "Tahun Baru".into(), libur: true }];
        write_cache(&path, &items);
        let (fetched, back) = read_cache(&path).unwrap();
        assert_eq!(back, items);
        assert!(is_fresh(fetched, fetched + 3600));
        assert!(!is_fresh(fetched, fetched + TTL_SECS));
        let _ = std::fs::remove_file(&path);
        assert!(read_cache(&path).is_none());
    }

    #[test]
    fn tampilan_memilih_peringatan_terdekat() {
        let h = Holidays::new();
        // 15 Nov 2026: setelahnya di bulan itu ada Hari Guru Nasional (25 Nov)
        let today = Date::new(2026, 11, 15);
        let v = View::new(today, &h);
        assert_eq!(v.selected, Some(Date::new(2026, 11, 25)));
        let (date, name, kind) = v.footer(&h);
        assert_eq!(date, "Rabu, 25 November");
        assert_eq!(name, "Hari Guru Nasional");
        assert_eq!(kind, 3);
    }

    #[test]
    fn pindah_bulan_lintas_tahun_dan_hari_ini() {
        let h = Holidays::new();
        let today = Date::new(2026, 11, 15);
        let mut v = View::new(today, &h);
        v.shift_month(2, today, &h);
        assert_eq!((v.year, v.month), (2027, 1));
        v.shift_month(-14, today, &h);
        assert_eq!((v.year, v.month), (2025, 11));
        v.go_today(today, &h);
        assert_eq!((v.year, v.month), (2026, 11));
    }

    #[test]
    fn klik_tanggal_bulan_lain_pindah_bulan() {
        let h = Holidays::new();
        let today = Date::new(2026, 10, 15);
        let mut v = View::new(today, &h);
        v.select(0); // 27 September
        assert_eq!(v.selected, Some(Date::new(2026, 9, 27)));
        assert_eq!((v.year, v.month), (2026, 9));
        v.select(99); // di luar kisi: diabaikan
        assert_eq!(v.selected, Some(Date::new(2026, 9, 27)));
    }

    #[test]
    fn keterangan_tahun_tanpa_data() {
        let h = Holidays::new();
        let today = Date::new(2031, 5, 5);
        let mut v = View::new(today, &h);
        v.selected = Some(Date::new(2031, 5, 6));
        assert_eq!(v.footer(&h).1, "Libur keagamaan tahun ini belum tersedia");
        v.selected = None;
        assert_eq!(v.footer(&h).2, 0);
    }
}
