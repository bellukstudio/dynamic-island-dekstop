use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone)]
pub struct Reminder {
    pub id: String,
    pub text: String,
    pub due_at: i64, // unix ms; 0 = tanpa waktu (tugas biasa, tidak memicu notifikasi)
    pub done: bool,
    #[serde(default)]
    pub notified: bool,
    /// Mode pengulangan: "none", "daily", "weekly" (hari tertentu), atau "monthly".
    #[serde(default)]
    pub repeat_mode: String,
    /// Disimpan untuk membaca data reminder versi lama.
    #[serde(default)]
    pub repeat_daily: bool,
    /// Waktu awal sebagai jangkar agar snooze tidak menggeser jadwal berulang.
    #[serde(default)]
    pub repeat_anchor_at: i64,
    /// Hari dalam bulan untuk pengulangan bulanan (1-31).
    #[serde(default)]
    pub repeat_day_of_month: i32,
    /// Hasil Date.getTimezoneOffset() dari frontend.
    #[serde(default)]
    pub repeat_timezone_offset_minutes: i32,
    /// Hari aktif untuk mode "weekly" sebagai bitmask: bit 0 = Minggu, 1 = Senin, ... 6 = Sabtu
    /// (sama dengan Date.getDay() di JavaScript).
    #[serde(default)]
    pub repeat_weekdays: u8,
}

fn path() -> PathBuf {
    crate::config::path().with_file_name("reminders.json")
}

static ITEMS: Mutex<Option<Vec<Reminder>>> = Mutex::new(None);

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MASK: u8 = 0b0111_1111;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn uid() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{:x}{:x}", n.as_millis(), n.subsec_nanos())
}

fn load() -> Vec<Reminder> {
    let mut items: Vec<Reminder> = std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // Migrasi ringan: reminder harian dari versi lama belum memiliki repeat_mode.
    for item in &mut items {
        if item.repeat_mode.is_empty() && item.repeat_daily {
            item.repeat_mode = "daily".to_string();
        }
        item.repeat_daily = item.repeat_mode == "daily" || item.repeat_daily;
        if item.repeat_anchor_at <= 0 && item.repeat_daily {
            item.repeat_anchor_at = item.due_at;
        }
    }
    items
}

fn save(items: &[Reminder]) {
    if let Some(dir) = path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(path(), text);
    }
}

fn with_items<R>(f: impl FnOnce(&mut Vec<Reminder>) -> R) -> R {
    let mut guard = ITEMS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load());
    }
    let items = guard.as_mut().unwrap();
    let result = f(items);
    save(items);
    result
}

// hanya baca: tidak menulis ulang file (with_items selalu menyimpan)
fn read_items<R>(f: impl FnOnce(&Vec<Reminder>) -> R) -> R {
    let mut guard = ITEMS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load());
    }
    f(guard.as_ref().unwrap())
}

fn sorted(mut items: Vec<Reminder>) -> Vec<Reminder> {
    items.sort_by(|a, b| {
        a.done.cmp(&b.done).then_with(|| {
            let au = a.due_at == 0;
            let bu = b.due_at == 0;
            au.cmp(&bu).then(a.due_at.cmp(&b.due_at))
        })
    });
    items
}

fn effective_repeat_mode(r: &Reminder) -> &str {
    if r.repeat_mode == "monthly" {
        "monthly"
    } else if r.repeat_mode == "weekly" && (r.repeat_weekdays & WEEK_MASK) != 0 {
        "weekly"
    } else if r.repeat_mode == "daily" || r.repeat_daily {
        "daily"
    } else {
        "none"
    }
}

#[tauri::command]
pub fn reminders_list() -> Vec<Reminder> {
    read_items(|items| sorted(items.clone()))
}

#[tauri::command]
pub fn reminders_add(
    text: String,
    due_at: i64,
    repeat_mode: String,
    timezone_offset_minutes: i32,
    repeat_weekdays: Option<u8>,
) -> Vec<Reminder> {
    with_items(|items| {
        let text: String = text.trim().chars().take(200).collect();
        let mut due_at = due_at.max(0);
        let weekdays = repeat_weekdays.unwrap_or(0) & WEEK_MASK;
        let repeat_mode = if due_at == 0 {
            "none".to_string()
        } else {
            match repeat_mode.as_str() {
                "daily" => "daily".to_string(),
                "monthly" => "monthly".to_string(),
                // "weekly" tanpa hari terpilih tidak bermakna -> diperlakukan tidak berulang
                "weekly" if weekdays != 0 => "weekly".to_string(),
                _ => "none".to_string(),
            }
        };
        let repeat_daily = repeat_mode == "daily";
        let offset = timezone_offset_minutes.clamp(-840, 840);
        // Jadwal pertama harus jatuh pada hari yang aktif: geser ke hari aktif terdekat (jam tetap).
        if repeat_mode == "weekly" {
            due_at = next_weekly_occurrence(due_at, offset, weekdays, due_at - 1);
        }
        let repeat_day_of_month = if repeat_mode == "monthly" {
            local_parts(due_at, offset).2 as i32
        } else {
            0
        };

        if !text.is_empty() {
            items.push(Reminder {
                id: uid(),
                text,
                due_at,
                done: false,
                notified: false,
                repeat_mode: repeat_mode.clone(),
                repeat_daily,
                repeat_anchor_at: if repeat_mode == "none" { 0 } else { due_at },
                repeat_day_of_month,
                repeat_timezone_offset_minutes: if repeat_mode == "none" { 0 } else { offset },
                repeat_weekdays: if repeat_mode == "weekly" { weekdays } else { 0 },
            });
        }
        sorted(items.clone())
    })
}

#[tauri::command]
pub fn reminders_toggle(id: String) -> Vec<Reminder> {
    with_items(|items| {
        let now = now_ms();
        if let Some(r) = items.iter_mut().find(|r| r.id == id) {
            if r.done {
                r.done = false;
                if effective_repeat_mode(r) != "none" && r.due_at > 0 {
                    let anchor = if r.repeat_anchor_at > 0 { r.repeat_anchor_at } else { r.due_at };
                    let next = next_occurrence(r, now);
                    if let Some(next_due) = next {
                        r.repeat_anchor_at = anchor;
                        r.due_at = next_due;
                        r.notified = false;
                    }
                }
            } else {
                r.done = true;
            }
        }
        sorted(items.clone())
    })
}

#[tauri::command]
pub fn reminders_remove(id: String) -> Vec<Reminder> {
    with_items(|items| {
        items.retain(|r| r.id != id);
        sorted(items.clone())
    })
}

#[tauri::command]
pub fn reminders_clear_done() -> Vec<Reminder> {
    with_items(|items| {
        items.retain(|r| !r.done);
        sorted(items.clone())
    })
}

fn next_daily_occurrence(anchor_ms: i64, now: i64) -> i64 {
    if anchor_ms > now {
        return anchor_ms;
    }
    let elapsed = now.saturating_sub(anchor_ms);
    let days = elapsed / DAY_MS + 1;
    anchor_ms.saturating_add(days.saturating_mul(DAY_MS))
}

// Kalender sipil tanpa dependency eksternal. Timestamp dikonversi ke waktu lokal
// menggunakan offset yang dipilih frontend saat reminder dibuat.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 30,
    }
}

// Mengembalikan (tahun, bulan, hari, jam, menit) menurut zona waktu reminder.
fn local_parts(timestamp_ms: i64, offset_minutes: i32) -> (i64, i64, i64, i64, i64) {
    let local_ms = timestamp_ms.saturating_sub(offset_minutes as i64 * MINUTE_MS);
    let days = local_ms.div_euclid(DAY_MS);
    let within_day = local_ms.rem_euclid(DAY_MS);
    let (year, month, day) = civil_from_days(days);
    let hour = within_day / HOUR_MS;
    let minute = (within_day % HOUR_MS) / MINUTE_MS;
    (year, month, day, hour, minute)
}

fn local_datetime_to_timestamp_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    offset_minutes: i32,
) -> i64 {
    let local_ms = days_from_civil(year, month, day) * DAY_MS
        + hour * HOUR_MS
        + minute * MINUTE_MS;
    local_ms.saturating_add(offset_minutes as i64 * MINUTE_MS)
}

fn next_monthly_occurrence(anchor_ms: i64, offset_minutes: i32, desired_day: i64, now: i64) -> i64 {
    let (_, _, anchor_day, hour, minute) = local_parts(anchor_ms, offset_minutes);
    let wanted_day = if (1..=31).contains(&desired_day) { desired_day } else { anchor_day };
    let (mut year, mut month, _, _, _) = local_parts(now, offset_minutes);

    // Cari tanggal yang sama pada bulan ini atau bulan-bulan berikutnya.
    // Jika bulan tidak memiliki tanggal itu (mis. tanggal 31 di Februari),
    // gunakan hari terakhir bulan tersebut. Bulan selanjutnya tetap memakai tanggal jangkar.
    for _ in 0..14 {
        let day = wanted_day.min(days_in_month(year, month));
        let candidate = local_datetime_to_timestamp_ms(year, month, day, hour, minute, offset_minutes);
        if candidate > now {
            return candidate;
        }
        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }
    now.saturating_add(31 * DAY_MS)
}

// Hari dalam pekan (0 = Minggu ... 6 = Sabtu) untuk jumlah hari sejak epoch. 1 Jan 1970 = Kamis.
fn weekday_from_days(days_since_epoch: i64) -> u8 {
    (days_since_epoch + 4).rem_euclid(7) as u8
}

// Kejadian berikutnya (> now) pada salah satu hari aktif, memakai jam/menit dari jangkar.
fn next_weekly_occurrence(anchor_ms: i64, offset_minutes: i32, weekdays: u8, now: i64) -> i64 {
    let (_, _, _, hour, minute) = local_parts(anchor_ms, offset_minutes);
    let local_now = now.saturating_sub(offset_minutes as i64 * MINUTE_MS);
    let today = local_now.div_euclid(DAY_MS);
    for add in 0..=7 {
        let day = today + add;
        if weekdays & (1 << weekday_from_days(day)) == 0 {
            continue;
        }
        let (y, m, d) = civil_from_days(day);
        let candidate = local_datetime_to_timestamp_ms(y, m, d, hour, minute, offset_minutes);
        if candidate > now {
            return candidate;
        }
    }
    now.saturating_add(DAY_MS)
}

fn next_occurrence(r: &Reminder, now: i64) -> Option<i64> {
    let anchor = if r.repeat_anchor_at > 0 { r.repeat_anchor_at } else { r.due_at };
    match effective_repeat_mode(r) {
        "daily" => Some(next_daily_occurrence(anchor, now)),
        "weekly" => Some(next_weekly_occurrence(
            anchor,
            r.repeat_timezone_offset_minutes,
            r.repeat_weekdays & WEEK_MASK,
            now,
        )),
        "monthly" => Some(next_monthly_occurrence(
            anchor,
            r.repeat_timezone_offset_minutes,
            r.repeat_day_of_month as i64,
            now,
        )),
        _ => None,
    }
}

// Pengingat sekali jalan menjadi selesai; pengingat berulang lanjut ke jadwal berikutnya.
#[tauri::command]
pub fn reminders_stop(id: String) -> Vec<Reminder> {
    with_items(|items| {
        let now = now_ms();
        if let Some(r) = items.iter_mut().find(|r| r.id == id) {
            if let Some(next_due) = next_occurrence(r, now) {
                let mode = effective_repeat_mode(r).to_string();
                let anchor = if r.repeat_anchor_at > 0 { r.repeat_anchor_at } else { r.due_at };
                r.repeat_mode = mode.clone();
                r.repeat_daily = mode == "daily";
                r.repeat_anchor_at = anchor;
                r.due_at = next_due;
                r.done = false;
                r.notified = false;
            } else {
                r.done = true;
                r.notified = true;
            }
        }
        sorted(items.clone())
    })
}

// Tunda: jadwalkan ulang bunyi untuk beberapa menit. Jangkar pengulangan tetap dipertahankan.
#[tauri::command]
pub fn reminders_snooze(id: String, minutes: i64) -> Vec<Reminder> {
    with_items(|items| {
        if let Some(r) = items.iter_mut().find(|r| r.id == id) {
            r.due_at = now_ms() + minutes.max(1) * MINUTE_MS;
            r.notified = false;
        }
        sorted(items.clone())
    })
}

fn due_now() -> Vec<Reminder> {
    let mut guard = ITEMS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load());
    }
    let now = now_ms();
    guard
        .as_ref()
        .unwrap()
        .iter()
        .filter(|r| !r.done && r.due_at > 0 && r.due_at <= now)
        .cloned()
        .collect()
}

// Dicek tiap 5 detik. Pengingat yang jatuh tempo dikirim segera, lalu diulang
// tiap 20 detik selama belum dihentikan atau ditunda.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let due = due_now();
            last.retain(|id, _| due.iter().any(|r| &r.id == id));
            for r in due {
                let again = last
                    .get(&r.id)
                    .map(|t| t.elapsed() >= Duration::from_secs(20))
                    .unwrap_or(true);
                if again {
                    last.insert(r.id.clone(), Instant::now());
                    let _ = app.emit("reminder-due", &r);
                }
            }
        }
    });
}