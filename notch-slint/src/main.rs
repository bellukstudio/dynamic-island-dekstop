//! Prototipe Notch dengan Slint. Target utama: Windows.
//! Di OS lain hanya untuk uji kompilasi (posisi kursor dan jam memakai fallback).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notch_core::battery::{BatteryMonitor, Reading};
use notch_core::calendar::{self, Date, Holidays, Item, View as CalView};
use notch_core::clipboard::ClipboardMonitor;
use notch_core::config::Config;
use notch_core::decor;
use notch_core::eyes::{self, Anim, EyeRenderer, Face, Morph, Mood, MoodState};
use notch_core::layout::{self, Dock};
use notch_core::notes::{Note, NotesStore};
use notch_core::pomodoro::{Phase, Pomodoro};
use notch_core::prayer::{self, Day as PrayerDay};
use notch_core::quick::{self, Quick};
use notch_core::reminder::{self, Reminder};
use notch_core::stats::StatsMonitor;
use slint::Model;
use notch_core::theme::{Palette, Rgba};
use slint::{
    ComponentHandle, Global, Image, ModelRc, PhysicalPosition, PhysicalSize, Rgba8Pixel,
    SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel, Weak,
};

mod privacy;
mod shelf;
mod hotkeys;
mod colorpick;
mod compress;
mod convert;
mod transfer;
mod android;
mod android_mirror;

slint::include_modules!();

// Ukuran jendela = pil + bahu melengkung (14 px kiri/kanan) + sedikit ruang animasi.
const COLLAPSED: (u32, u32) = (162, 38);
// 580 x 202 untuk pil + jalur di bawahnya (celah 8 px + tombol Pusatkan 22 px), sama dengan hub Tauri (242).
const EXPANDED: (u32, u32) = (612, 240);

// Gambar mata = seukuran pil kecil (px logis); harus sama dengan `Image` mata di ui/notch.slint.
const EYES_W: f32 = 130.0;
const EYES_H: f32 = 34.0;

/// Pengacak ringan untuk kedip dan tatapan acak (tanpa dependensi `rand`).
struct Rng(Cell<u64>);
impl Rng {
    fn new() -> Rng {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(Cell::new(seed | 1))
    }
    /// 0.0 ..< 1.0
    fn next(&self) -> f32 {
        let mut x = self.0.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0.set(x);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Seluruh keadaan mata: mood, transisi, tatapan, kedip, dan frame terakhir yang digambar.
struct EyeCtx {
    renderer: EyeRenderer,
    skin: usize,
    mood: MoodState,
    cur: Mood,
    cur_since: f32,
    morph: Morph,
    look: (f32, f32),
    last_cursor: (i32, i32),
    last_move: f32,
    next_random_look: f32,
    blink_until: f32,
    next_blink: f32,
    last_key: Option<(i32, i32, bool, usize, i64, u32)>,
    rng: Rng,
}

mod media;

mod platform {
    use notch_core::layout::WorkArea;

    /// Apakah Windows memakai tema aplikasi terang (untuk `theme = "auto"`). Di OS lain: gelap.
    pub fn os_light() -> bool {
        #[cfg(windows)]
        unsafe {
            use windows::core::w;
            use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
            let mut data: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            let r = RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut core::ffi::c_void),
                Some(&mut size),
            );
            r.is_ok() && data == 1
        }
        #[cfg(not(windows))]
        false
    }

    /// Posisi kursor global (piksel fisik).
    pub fn cursor_pos() -> Option<(i32, i32)> {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let mut p = POINT::default();
            GetCursorPos(&mut p).ok()?;
            Some((p.x, p.y))
        }
        #[cfg(not(windows))]
        None
    }

    /// Area kerja monitor (di luar taskbar) tempat notch berada, dalam piksel fisik: lebar penuh monitor,
    /// vertikal mengikuti area kerja (sama dengan `monitor_work_area` di src-tauri/src/lib.rs).
    /// `near` = titik (mis. tengah posisi kustom) untuk memilih monitor; `None` = monitor utama.
    pub fn work_area(near: Option<(i32, i32)>) -> WorkArea {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
            };
            let (pt, flag) = match near {
                // y bisa 100000 (menempel di dasar layar): jepit supaya titik tetap dekat monitor
                Some((x, y)) => (POINT { x, y: y.min(50_000) }, MONITOR_DEFAULTTONEAREST),
                None => (POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
            };
            let hmon = MonitorFromPoint(pt, flag);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                let (m, r) = (mi.rcMonitor, mi.rcWork);
                if r.bottom - r.top > 0 {
                    return WorkArea { x: m.left, y: r.top, w: m.right - m.left, h: r.bottom - r.top };
                }
            }
        }
        let _ = near;
        WorkArea { x: 0, y: 0, w: 1920, h: 1080 }
    }

    /// (jam, menit, hari, bulan, tahun) waktu lokal.
    pub fn local_time() -> (u32, u32, u32, u32, u32) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::SystemInformation::GetLocalTime;
            let t = GetLocalTime();
            (t.wHour as u32, t.wMinute as u32, t.wDay as u32, t.wMonth as u32, t.wYear as u32)
        }
        #[cfg(not(windows))]
        {
            // Fallback UTC tanpa dependensi tambahan.
            let s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let days = s / 86_400;
            let (y, m, d) = civil_from_days(days as i64);
            (((s / 3600) % 24) as u32, ((s / 60) % 60) as u32, d, m, y)
        }
    }

    /// Hari sejak 1970-01-01 -> (tahun, bulan, tanggal), algoritma Howard Hinnant.
    #[cfg(not(windows))]
    fn civil_from_days(z: i64) -> (u32, u32, u32) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400) as u32 + u32::from(m <= 2);
        (y, m, d)
    }
}

/// Decode PNG kecil yang ditanam di binary menjadi Image Slint. Tanpa decoder gambar umum,
/// jadi binary dan RAM tetap kecil. Mengembalikan gambar kosong jika formatnya bukan RGBA8.
fn load_png(bytes: &'static [u8]) -> Image {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let Ok(mut reader) = decoder.read_info() else { return Image::default() };
    let mut buf = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut buf) else { return Image::default() };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Image::default();
    }
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &buf[..info.buffer_size()],
        info.width,
        info.height,
    ))
}

fn gib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

/// Kecepatan jaringan dalam B/s -> teks pendek (KB/s atau MB/s).
fn fmt_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB/s", bps / 1024.0)
    }
}

fn apply_battery(w: &NotchWindow, reading: Reading) {
    match reading {
        Reading::Present(b) => {
            w.set_battery_present(true);
            w.set_battery_percent(i32::from(b.percent));
            w.set_battery_charging(b.charging);
        }
        Reading::Absent => w.set_battery_present(false),
        Reading::Unavailable => {}
    }
}

/// Isi daftar catatan di UI (maksimal 3 terbaru; halaman ini belum bisa menggulir).
fn show_notes(model: &VecModel<NoteItem>, notes: Vec<Note>) {
    model.set_vec(
        notes
            .into_iter()
            .take(3)
            .map(|n| NoteItem { id: n.id.into(), text: n.text.replace('\n', " ").into() })
            .collect::<Vec<_>>(),
    );
}
 
/// Perbarui tiga item terbaru pada halaman Clipboard. Riwayat internal tetap menyimpan 10 item.
fn show_clipboard(model: &VecModel<ClipboardItem>, monitor: Option<&ClipboardMonitor>) {
    let items = monitor.map(ClipboardMonitor::list).unwrap_or_default();
    model.set_vec(
        items
            .into_iter()
            .take(3)
            .map(|text| {
                let preview = text.replace('\n', " ").replace('\r', " ");
                let mut chars = preview.chars();
                let short: String = chars.by_ref().take(64).collect();
                let preview = if chars.next().is_some() {
                    format!("{short}…")
                } else {
                    short
                };
                ClipboardItem { preview: preview.into(), text: text.into() }
            })
            .collect::<Vec<_>>(),
    );
}

/// Ambil cuaca di thread latar, lalu kirim hasilnya ke thread Slint.
fn refresh_weather(
    weak: Weak<NotchWindow>,
    busy: Arc<AtomicBool>,
    lat: f64,
    lon: f64,
    city: String,
) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let result = notch_core::weather::fetch(lat, lon);
        busy.store(false, Ordering::SeqCst);
        let _ = weak.upgrade_in_event_loop(move |w| {
            w.set_weather_city(city.into());
            match result {
                Ok(reading) => {
                    w.set_weather_temp(format!("{:.0}°", reading.temp).into());
                    w.set_weather_condition(notch_core::weather::describe(reading.code, reading.is_day).into());
                    w.set_weather_feels(format!("Terasa {:.0}°", reading.feels).into());
                    w.set_weather_humidity(format!("Kelembapan {:.0}%", reading.humidity).into());
                    let rain = if reading.raining {
                        "Sedang hujan".to_string()
                    } else if let Some(minutes) = reading.rain_in {
                        format!("Hujan dalam {minutes} menit")
                    } else {
                        "Tidak ada hujan terdeteksi".to_string()
                    };
                    w.set_weather_rain(rain.into());
                    w.set_weather_icon(notch_core::weather::icon_path(reading.code, reading.is_day).into());
                    w.set_weather_status("".into());
                }
                Err(error) => {
                    w.set_weather_temp("--°".into());
                    w.set_weather_condition("Data cuaca belum tersedia".into());
                    w.set_weather_feels("".into());
                    w.set_weather_humidity("".into());
                    w.set_weather_rain("Periksa koneksi internet".into());
                    w.set_weather_icon("".into());
                    w.set_weather_status(format!("Gagal memuat: {error}").into());
                }
            }
        });
    });
}



#[derive(Clone, Default)]
struct PrayerSchedule { current: Vec<PrayerDay>, next: Vec<PrayerDay>, error: String }
fn prayer_at(date:(i64,i64,i64),time:&str,offset:i32)->Option<i64>{let(y,m,d)=date;reminder::parse_local_datetime(&format!("{d:02}/{m:02}/{y} {time}"),offset)}
fn prayer_countdown(target:Option<i64>,now:i64)->String{let Some(t)=target else{return "--:--:--".into()};let s=(t-now).max(0)/1000;format!("{:02}:{:02}:{:02}",s/3600,(s%3600)/60,s%60)}
fn prayer_find(days:&[PrayerDay],day:i64)->Option<PrayerDay>{days.iter().find(|x|i64::from(x.tanggal)==day).cloned()}
fn render_prayer(w:&NotchWindow,s:&PrayerSchedule,offset:i32,ramadan:bool){
 let now=reminder::now_ms();let(y,m,d,_,_)=reminder::local_parts(now,offset);let(ny,nm,nd,_,_)=reminder::local_parts(now+86_400_000,offset);
 let today=prayer_find(&s.current,d);let tomorrow=if y==ny&&m==nm{today.clone()}else{prayer_find(&s.next,nd)};
 let entries:[(&str,&str);5]=today.as_ref().map(|x|[("Subuh",x.subuh.as_str()),("Dzuhur",x.dzuhur.as_str()),("Ashar",x.ashar.as_str()),("Maghrib",x.maghrib.as_str()),("Isya",x.isya.as_str())]).unwrap_or([("Subuh",""),("Dzuhur",""),("Ashar",""),("Maghrib",""),("Isya","")]);
 let mut next:Option<(&str,&str,i64)>=None;for(name,time)in entries{if let Some(at)=prayer_at((y,m,d),time,offset){if at>now{next=Some((name,time,at));break;}}}
 if next.is_none(){if let Some(t)=tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.subuh,offset)){next=Some(("Subuh",tomorrow.as_ref().unwrap().subuh.as_str(),t));}}
 if let Some((name,time,at))=next{w.set_prayer_next_name(name.into());w.set_prayer_next_time(time.into());w.set_prayer_countdown(prayer_countdown(Some(at),now).into());}else{w.set_prayer_next_name("Jadwal belum tersedia".into());w.set_prayer_next_time("--:--".into());w.set_prayer_countdown("--:--:--".into());}
 let rows=entries.into_iter().map(|(name,time)|{let at=prayer_at((y,m,d),time,offset);let state=if next.map(|x|x.0)==Some(name){1}else if at.map(|t|t<=now).unwrap_or(false){2}else{0};PrayerItem{name:name.into(),time:if time.is_empty(){"--:--".into()}else{time.into()},state}}).collect::<Vec<_>>();
 w.set_prayer_items(ModelRc::from(Rc::new(VecModel::from(rows))));w.set_prayer_status(s.error.clone().into());
 if ramadan{let im=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.imsak,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.imsak,offset)));let bu=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.maghrib,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.maghrib,offset)));w.set_ramadan_date(format!("{d:02}/{m:02}/{y}").into());w.set_ramadan_imsak_countdown(prayer_countdown(im,now).into());w.set_ramadan_buka_countdown(prayer_countdown(bu,now).into());w.set_ramadan_imsak_time(format!("Imsak {}",today.as_ref().map(|x|x.imsak.as_str()).unwrap_or("--:--")).into());w.set_ramadan_buka_time(format!("Maghrib {}",today.as_ref().map(|x|x.maghrib.as_str()).unwrap_or("--:--")).into());}
}
fn load_ramadan()->bool{std::fs::read_to_string(notch_core::paths::data_file("ramadan.json")).ok().map(|s|s.trim()=="true").unwrap_or(false)}

fn local_timezone_offset_minutes() -> i32 {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "(Get-Date).ToString('zzz')"])
            .creation_flags(0x0800_0000)
            .output()
    };
    #[cfg(not(windows))]
    let output = std::process::Command::new("date").arg("+%z").output();

    let Ok(output) = output else { return 0 };
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let compact = raw.replace(':', "");
    if compact.len() != 5 { return 0; }
    let sign = if compact.starts_with('-') { -1 } else if compact.starts_with('+') { 1 } else { return 0 };
    let hours = compact[1..3].parse::<i32>().unwrap_or(0);
    let minutes = compact[3..5].parse::<i32>().unwrap_or(0);
    -(sign * (hours * 60 + minutes))
}

fn weekday_selection(mask: u8) -> Vec<bool> {
    (0..7).map(|day| mask & (1 << day) != 0).collect()
}

fn show_reminders(model: &VecModel<ReminderItem>, reminders: Vec<Reminder>, offset: i32) {
    let items = reminders.into_iter().take(3).map(|r| {
        let due = if r.due_at <= 0 {
            "Tanpa waktu".to_string()
        } else {
            let (year, month, day, hour, minute) = reminder::local_parts(r.due_at, offset);
            let when = format!("{day:02}/{month:02}/{year} {hour:02}:{minute:02}");
            let repeat = match reminder::effective_repeat_mode(&r) {
                "daily" => " · Harian",
                "weekly" => " · Mingguan",
                "monthly" => " · Bulanan",
                _ => "",
            };
            format!("{when}{repeat}")
        };
        ReminderItem { id: r.id.into(), text: r.text.into(), due: due.into(), done: r.done }
    }).collect::<Vec<_>>();
    model.set_vec(items);
}

fn col(c: Rgba) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn apply_theme(w: &NotchWindow, p: &Palette) {
    let t = Theme::get(w);
    t.set_body(col(p.body));
    t.set_text(col(p.text));
    t.set_text_dim(col(p.text_dim));
    t.set_text_faint(col(p.text_faint));
    t.set_accent(col(p.accent));
    t.set_on_accent(col(p.on_accent));
    t.set_ok(col(p.ok));
    t.set_chip(col(p.chip));
    t.set_chip_hover(col(p.chip_hover));
    t.set_track(col(p.track));
}

/// None -> -1 (tidak didukung/belum terbaca), false -> 0, true -> 1.
fn tri(v: Option<bool>) -> i32 {
    match v {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    }
}

fn apply_quick(w: &NotchWindow, q: &Quick) {
    w.set_q_mic(tri(q.mic));
    w.set_q_wifi(tri(q.wifi));
    w.set_q_bt(tri(q.bt));
    w.set_q_awake(tri(q.awake));
    w.set_q_dark(tri(q.dark));
}

/// Baca keadaan Quick di thread lain (bisa sampai 3 detik) lalu tampilkan di thread UI.
fn refresh_quick(weak: Weak<NotchWindow>, busy: Arc<AtomicBool>, awake: Arc<AtomicBool>) {
    if busy.swap(true, Ordering::SeqCst) {
        return; // pembacaan sebelumnya masih berjalan
    }
    std::thread::spawn(move || {
        let q = quick::state();
        awake.store(q.awake == Some(true), Ordering::Relaxed); // mata tidak mengantuk saat keep-awake
        let _ = weak.upgrade_in_event_loop(move |w| apply_quick(&w, &q));
        busy.store(false, Ordering::SeqCst);
    });
}

fn show_pomo(w: &NotchWindow, p: &Pomodoro) {
    w.set_pomo_text(p.time_text().into());
    w.set_pomo_label(p.label().into());
    w.set_pomo_progress(p.progress());
    w.set_pomo_running(p.running());
    w.set_pomo_rest(p.phase() != Phase::Focus);
}

// Ukuran pil saat melebar (px logis); harus sama dengan `pill` di ui/notch.slint.
const PILL_W: f32 = 580.0;
const PILL_H: f32 = 202.0;

/// Penempatan jendela + bentuk tepi + gambar hiasan tema. Hiasan dirender ulang hanya saat mode tepi
/// (atas/melayang/bawah) atau faktor DPI berubah.
struct Shell {
    pos: Cell<Option<(i32, i32)>>, // posisi kustom (pusat x, atas y) atau None = tengah atas
    theme: RefCell<String>,
    body: RefCell<Rgba>,
    scale: RefCell<f32>,
    memo: Cell<(Option<Dock>, u32)>, // (dock, bit faktor DPI) yang sedang terpasang
}

impl Shell {
    /// Letakkan jendela sesuai posisi config dan atur bentuk tepinya (menempel atas / melayang / bawah).
    fn place(&self, win: &NotchWindow, size: (u32, u32)) {
        let pos = self.pos.get();
        let work = platform::work_area(pos);
        // COLLAPSED/EXPANDED dalam piksel logis; jendela diukur dalam piksel fisik
        let sf = win.window().scale_factor();
        let size = ((size.0 as f32 * sf).round() as u32, (size.1 as f32 * sf).round() as u32);
        let (origin, dock) = layout::place(&work, size.0 as i32, size.1 as i32, pos);
        if self.memo.get() != (Some(dock), sf.to_bits()) {
            self.memo.set((Some(dock), sf.to_bits()));
            win.set_dock(match dock {
                Dock::Top => 0,
                Dock::Floating => 1,
                Dock::Bottom => 2,
            });
            self.rebuild_decor(win, dock, sf);
        }
        let w = win.window();
        w.set_size(PhysicalSize::new(size.0, size.1));
        w.set_position(PhysicalPosition::new(origin.0, origin.1));
    }

    fn rebuild_decor(&self, win: &NotchWindow, dock: Dock, sf: f32) {
        let (w, h) = ((PILL_W * sf).round() as u32, (PILL_H * sf).round() as u32);
        let r = (if dock == Dock::Floating { 28.0 } else { 32.0 }) * *self.scale.borrow() * sf;
        match decor::render(&self.theme.borrow(), *self.body.borrow(), dock, w, h, r, sf) {
            Some(buf) => {
                win.set_decor(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, w, h)));
                win.set_has_decor(true);
            }
            None => win.set_has_decor(false), // tema default: tanpa hiasan
        }
    }
}

/// Tanggal lokal hari ini.
fn today() -> Date {
    let (_, _, d, m, y) = platform::local_time();
    Date::new(y as i32, m, d)
}

/// Isi ulang kisi kalender, judul, dan keterangan di UI dari keadaan `view` + data libur.
fn show_calendar(w: &NotchWindow, model: &VecModel<CalCell>, view: &CalView, hol: &Holidays) {
    let t = today();
    model.set_vec(
        calendar::cells_for(view.year, view.month)
            .iter()
            .map(|c| CalCell {
                day: c.date.d as i32,
                in_month: c.in_month,
                sun: c.date.weekday() == 0,
                today: c.date == t,
                sel: view.selected == Some(c.date),
                kind: hol.events_on(c.date).first().map_or(0, |e| e.kind.code()),
            })
            .collect::<Vec<_>>(),
    );
    w.set_cal_title(calendar::title(view.year, view.month).into());
    let (date, name, kind) = view.footer(hol);
    w.set_cal_foot_date(date.into());
    w.set_cal_foot_name(name.into());
    w.set_cal_foot_kind(kind);
}

fn main() -> Result<(), slint::PlatformError> {
    if let Some(resource_dir) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) {
        notch_tools::compress::set_resource_dir(resource_dir);
    }
    // Config dibaca saja (tidak pernah ditulis/dibuat dari sini).
    let cfg = Config::load_readonly(&notch_core::paths::config_path());

    let palette = Palette::from_appearance(&cfg.appearance, platform::os_light());
    // Posisi notch: kustom dari config (pusat x, atas y; piksel fisik) atau None = tengah atas.
    // Tombol "Pusatkan" hanya mengembalikannya ke tengah selama sesi ini (config tidak ditulis dari sini).
    let shell = Rc::new(Shell {
        pos: Cell::new(cfg.position.custom.then_some((cfg.position.x, cfg.position.y))),
        theme: RefCell::new(cfg.appearance.notch_theme.clone()),
        body: RefCell::new(palette.body),
        scale: RefCell::new(cfg.scale as f32),
        memo: Cell::new((None, 0)),
    });

    let appearance_eye: Rc<RefCell<Option<Rc<RefCell<EyeCtx>>>>> = Rc::new(RefCell::new(None));
    let sleep_after_s = Rc::new(Cell::new(cfg.timing.sleep_after_s));
    let win = NotchWindow::new()?;
    let transfer_state = Arc::new(Mutex::new(transfer::TransferState::new(notch_core::paths::data_file("Notch"))));
    let android_state = Arc::new(Mutex::new(android::AndroidState::new()));
    let mirror_process = Arc::new(Mutex::new(android_mirror::MirrorProcess::new()));
    win.set_convert_selected_path("".into());
    win.set_compress_selected_path("".into());
    win.set_compress_status("Pilih gambar JPG/PNG/BMP, PDF, atau dokumen DOCX/XLSX/PPTX untuk dikompres".into());
    let (hotkey_rx, hotkey_config_tx, _hotkey_thread) = hotkeys::start(&cfg.shortcuts);
    privacy::start(win.as_weak());
    apply_theme(&win, &palette);
    win.set_ui_scale(cfg.scale as f32);
    shell.place(&win, COLLAPSED);

    // Tab mengikuti daftar dan urutan `pages` di config. Halaman yang belum ada di versi Slint
    // tampil sebagai placeholder, jadi urutan dan jumlah tab sama dengan versi Tauri.
    // Lirik bukan tab hub: ada di halaman Media (tombol "Lirik"), seperti di Tauri.
    let pages: Vec<String> = cfg.pages.iter().filter(|p| p.as_str() != "lyrics").cloned().collect();
    let pages = if pages.is_empty() { vec!["media".to_string()] } else { pages };
    win.set_tabs(ModelRc::from(Rc::new(VecModel::from(
        pages.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
    ))));
    win.set_page_id(pages[0].as_str().into());
    win.set_shortcut_hub(cfg.shortcuts.toggle_hub.clone().into());
    win.set_shortcut_settings(cfg.shortcuts.open_settings.clone().into());
    win.set_shortcut_pomodoro(cfg.shortcuts.toggle_pomodoro.clone().into());
    win.set_shortcut_prayer(cfg.shortcuts.toggle_prayer.clone().into());
    win.set_appearance_theme(cfg.appearance.theme.clone().into());
    win.set_appearance_eye_theme(cfg.appearance.eye_theme.clone().into());
    win.set_appearance_notch_theme(cfg.appearance.notch_theme.clone().into());
    win.set_appearance_mood(cfg.appearance.default_mood.clone().into());
    win.set_appearance_accent(cfg.appearance.accent.clone().into());
    win.set_appearance_dark_bg(cfg.appearance.dark_bg.clone().into());
    win.set_appearance_light_bg(cfg.appearance.light_bg.clone().into());
    win.set_prayer_mode(cfg.prayer.mode.clone().into());
    win.set_prayer_remind_before(cfg.prayer.remind_before.to_string().into());
    win.set_prayer_remind_min(cfg.prayer.remind_min.to_string().into());
    win.set_pomodoro_focus(cfg.pomodoro.focus_min.to_string().into());
    win.set_pomodoro_break(cfg.pomodoro.break_min.to_string().into());
    win.set_pomodoro_long_break(cfg.pomodoro.long_break_min.to_string().into());
    win.set_pomodoro_rounds(cfg.pomodoro.rounds.to_string().into());
    win.set_behavior_scale(format!("{:.2}", cfg.scale).into());
    win.set_behavior_hover_open(cfg.timing.hover_open_ms.to_string().into());
    win.set_behavior_hover_close(cfg.timing.hover_close_ms.to_string().into());
    win.set_behavior_sleep_after(cfg.timing.sleep_after_s.to_string().into());
    win.set_hub_pages(cfg.pages.join(", ").into());
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let mut updated_config = cfg.clone();
        win.on_settings_save_pages(move |value| {
            let allowed = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "android", "transfer", "convert", "compress", "settings"];
            let mut pages = Vec::<String>::new();
            for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !allowed.contains(&item) {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("ID halaman tidak dikenal: {item}").into());
                    }
                    return;
                }
                if !pages.iter().any(|p| p == item) {
                    pages.push(item.to_string());
                }
            }
            if pages.is_empty() || !pages.iter().any(|p| p == "media") {
                if let Some(w) = weak.upgrade() {
                    w.set_pages_save_status("Minimal sertakan halaman media.".into());
                }
                return;
            }
            let mut next = updated_config.clone();
            next.pages = pages.clone();
            next.pages_version = notch_core::config::PAGES_VERSION;
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next;
                    let tabs: Vec<String> = pages.iter().filter(|p| p.as_str() != "lyrics").cloned().collect();
                    let tabs = if tabs.is_empty() { vec!["media".to_string()] } else { tabs };
                    if let Some(w) = weak.upgrade() {
                        let current = w.get_page_id().to_string();
                        w.set_tabs(ModelRc::from(Rc::new(VecModel::from(
                            tabs.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
                        ))));
                        if !tabs.iter().any(|p| p == &current) {
                            w.set_page_id(tabs[0].as_str().into());
                        }
                        w.set_hub_pages(pages.join(", ").into());
                        w.set_pages_save_status("Halaman hub tersimpan dan langsung diperbarui.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let hotkey_config_tx = hotkey_config_tx.clone();
        let mut updated_config = cfg.clone();
        win.on_settings_save_shortcuts(move |hub, settings, pomo, prayer| {
            let mut next = updated_config.clone();
            next.shortcuts.toggle_hub = hub.to_string();
            next.shortcuts.open_settings = settings.to_string();
            next.shortcuts.toggle_pomodoro = pomo.to_string();
            next.shortcuts.toggle_prayer = prayer.to_string();
            next.normalize();
            if let Err(error) = hotkeys::validate_shortcuts(&next.shortcuts) {
                if let Some(w) = weak.upgrade() {
                    w.set_settings_save_status(error.into());
                }
                return;
            }
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next.clone();
                    let _ = hotkey_config_tx.send(next.shortcuts.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_shortcut_hub(next.shortcuts.toggle_hub.clone().into());
                        w.set_shortcut_settings(next.shortcuts.open_settings.clone().into());
                        w.set_shortcut_pomodoro(next.shortcuts.toggle_pomodoro.clone().into());
                        w.set_shortcut_prayer(next.shortcuts.toggle_prayer.clone().into());
                        w.set_settings_save_status("Tersimpan. Shortcut baru langsung aktif.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_settings_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    win.set_emoji_coffee(load_png(include_bytes!("../assets/emoji/2615.png")));
    win.set_emoji_tomato(load_png(include_bytes!("../assets/emoji/1f345.png")));
    win.set_emoji_moon(load_png(include_bytes!("../assets/emoji/1f319.png")));

    // --- State halaman ---
    let stats_timer = Rc::new(Timer::default());
    let battery_timer = Rc::new(Timer::default());
    let clipboard_timer = Rc::new(Timer::default());
    let reminder_timer = Timer::default();
    media::start(win.as_weak());
    let prayer_timer = Timer::default();
    // Indikator gelombang audio Adzan hanya bergerak selama playback aktif.
    let adhan_anim_timer = Timer::default();
    {
        let weak = win.as_weak();
        adhan_anim_timer.start(TimerMode::Repeated, Duration::from_millis(130), move || {
            if let Some(w) = weak.upgrade() {
                if w.get_adhan_active() {
                    w.set_adhan_phase((w.get_adhan_phase() + 1) % 3);
                }
            }
        });
    }
    // Durasi rapat dihitung dari transisi privasi aktif -> nonaktif.
    {
        let weak = win.as_weak();
        let started = Rc::new(Cell::new(None::<Instant>));
        let finished_at = Rc::new(Cell::new(None::<Instant>));
        meeting_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_meeting_active() {
                if started.get().is_none() { started.set(Some(Instant::now())); }
                if let Some(at) = started.get() {
                    let secs = at.elapsed().as_secs();
                    let label = if secs >= 3600 {
                        format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                    } else {
                        format!("{:02}:{:02}", secs / 60, secs % 60)
                    };
                    w.set_meeting_elapsed(label.into());
                }
            } else if let Some(at) = started.take() {
                let secs = at.elapsed().as_secs();
                let mins = secs / 60;
                let rem = secs % 60;
                let label = if mins == 0 { format!("{secs} detik") } else if rem == 0 { format!("{mins} menit") } else { format!("{mins} menit {rem} detik") };
                w.set_prayer_status(format!("Rapat selesai, {label}").into());
                w.set_meeting_elapsed("00:00".into());
            } else if let Some(at) = finished_at.get() {
                if at.elapsed() >= Duration::from_secs(6) {
                    w.set_meeting_finished("".into());
                    finished_at.set(None);
                }
            }
        });
    }
    let weather_timer = Rc::new(Timer::default());
    let monitor: Rc<RefCell<Option<StatsMonitor>>> = Rc::new(RefCell::new(None));
    let battery = Rc::new(BatteryMonitor::new());
    let notes_path = notch_core::paths::data_file("notes.json");
    let notes_store: Rc<RefCell<Option<NotesStore>>> = Rc::new(RefCell::new(None));
    let notes_model = Rc::new(VecModel::<NoteItem>::default());
    win.set_notes(ModelRc::from(notes_model.clone()));
    let shelf_store = Rc::new(RefCell::new(shelf::Store::open(notch_core::paths::data_file("shelf.json"))));
    let shelf_model = Rc::new(VecModel::<ShelfItem>::default());
    let shelf_items = shelf_store.borrow().list();
    shelf_model.set_vec(shelf_items.into_iter().map(|item| ShelfItem {
        path: item.path.into(), name: item.name.into(), dir: item.dir,
    }).collect());
    win.set_shelf_items(ModelRc::from(shelf_model.clone()));
    let clipboard_monitor = Rc::new(if pages.iter().any(|page| page == "clipboard") {
        match ClipboardMonitor::start() {
            Ok(monitor) => Some(monitor),
            Err(error) => {
                eprintln!("[Clipboard] {error}");
                None
            }
        }
    } else {
        None
    });
    let clipboard_model = Rc::new(VecModel::<ClipboardItem>::default());
    win.set_clipboard_items(ModelRc::from(clipboard_model.clone()));
    win.set_clipboard_available(clipboard_monitor.is_some());
    show_clipboard(&clipboard_model, clipboard_monitor.as_ref());
    let reminder_model = Rc::new(VecModel::<ReminderItem>::default());
    win.set_reminders(ModelRc::from(reminder_model.clone()));
    let timezone_offset = local_timezone_offset_minutes();
    let weekday_mask = Rc::new(Cell::new(62u8)); // Senin-Jumat, sama dengan pilihan awal Tauri
    win.set_reminder_weekdays_mask(62);
    win.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(62)))));
    show_reminders(&reminder_model, reminder::list(), timezone_offset);
    let active_reminder = Rc::new(RefCell::new(None::<String>));
    let prayer_prefs=Rc::new(RefCell::new(prayer::preferences()));
    win.set_prayer_city(prayer_prefs.borrow().kabkota.clone().into());win.set_prayer_province(prayer_prefs.borrow().provinsi.clone().into());
    win.set_prayer_mode(prayer_prefs.borrow().mode.clone().into());win.set_prayer_remind_before(prayer_prefs.borrow().remind_before);win.set_prayer_remind_min(prayer_prefs.borrow().remind_min.to_string().into());
    let ramadan=Rc::new(Cell::new(load_ramadan()));win.set_ramadan_enabled(ramadan.get());
    let dhikr_idx=Rc::new(Cell::new(0usize));
    let dhikr=[("أَسْتَغْفِرُ اللّٰهَ","Astaghfirullah","Aku memohon ampun kepada Allah. Dibaca 3x"),("اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ","Allahumma rabba hadzihid-da'watit-tammah","Doa setelah adzan"),("سُبْحَانَ اللّٰهِ وَبِحَمْدِهِ","Subhanallahi wa bihamdihi","Maha Suci Allah dan segala puji bagi-Nya. Dibaca 33x")];
    let set_dhikr:Rc<dyn Fn(&NotchWindow,usize)>=Rc::new(move|w,i|{let x=dhikr[i%3];w.set_dhikr_arabic(x.0.into());w.set_dhikr_latin(x.1.into());w.set_dhikr_meaning(x.2.into());w.set_dhikr_index((i%3)as i32);});set_dhikr(&win,0);
    let prayer_busy=Arc::new(AtomicBool::new(false));let prayer_data=Arc::new(Mutex::new(PrayerSchedule::default()));
    let prayer_fired=Rc::new(RefCell::new(std::collections::HashSet::<String>::new()));
    let weather_busy = Arc::new(AtomicBool::new(false));
    let weather_location = Rc::new(RefCell::new((cfg.weather.lat, cfg.weather.lon, cfg.weather.city.clone())));
    win.set_weather_city(cfg.weather.city.clone().into());
    win.set_weather_lat(cfg.weather.lat.to_string().into());
    win.set_weather_lon(cfg.weather.lon.to_string().into());
    win.set_weather_status("Cuaca dimuat saat halaman dibuka".into());
    {
        let weak = win.as_weak();
        let busy = weather_busy.clone();
        let location = weather_location.clone();
        win.on_weather_refresh(move || {
            let (lat, lon, city) = location.borrow().clone();
            refresh_weather(weak.clone(), busy.clone(), lat, lon, city);
        });
    }
    let quick_busy = Arc::new(AtomicBool::new(false));
    let keep_awake = Arc::new(AtomicBool::new(false));

    // Setiap halaman hanya bekerja saat notch melebar DAN halamannya terlihat (seperti versi Tauri):
    // Stats disampel tiap 1 dtk, Baterai dibaca tiap 5 dtk, Quick/Tools dibaca ulang saat dibuka,
    // Catatan dimuat ulang dari disk setiap dibuka (perubahan dari versi Tauri ikut terbaca).
    let sync_pages: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let (stats_timer, battery_timer, clipboard_timer, weather_timer) = (stats_timer.clone(), battery_timer.clone(), clipboard_timer.clone(), weather_timer.clone());
        let (monitor, battery) = (monitor.clone(), battery.clone());
        let (notes_store, notes_model, notes_path) =
            (notes_store.clone(), notes_model.clone(), notes_path.clone());
        let (clipboard_model, clipboard_monitor) = (clipboard_model.clone(), clipboard_monitor.clone());
        let weather_busy = weather_busy.clone();
        let weather_location = weather_location.clone();
        let quick_busy = quick_busy.clone();
        let keep_awake = keep_awake.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            let visible = w.get_expanded();
            let page = w.get_page_id();
            let page = page.as_str();

            // Stats
            if visible && page == "stats" {
                if !stats_timer.running() {
                    monitor.borrow_mut().get_or_insert_with(StatsMonitor::new).resume();
                    let weak_s = weak.clone();
                    let monitor = monitor.clone();
                    stats_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
                        let Some(w) = weak_s.upgrade() else { return };
                        if let Some(m) = monitor.borrow_mut().as_mut() {
                            let s = m.sample();
                            w.set_cpu(s.cpu);
                            w.set_ram(s.ram);
                            w.set_disk(s.disk);
                            w.set_cpu_text(format!("{:.0}%", s.cpu).into());
                            w.set_ram_text(format!("{:.1} / {:.1} GiB", gib(s.ram_used), gib(s.ram_total)).into());
                            w.set_disk_text(format!("{:.0} / {:.0} GiB", gib(s.disk_used), gib(s.disk_total)).into());
                            w.set_net_text(format!("\u{2193} {}   \u{2191} {}", fmt_rate(s.down), fmt_rate(s.up)).into());
                            w.set_sys_text(format!("{} threads \u{00B7} {} proses", s.cpu_cores, s.processes).into());
                        }
                    });
                }
            } else {
                stats_timer.stop();
            }

            // Baterai
            if visible && page == "battery" {
                if !battery_timer.running() {
                    apply_battery(&w, battery.read());
                    let weak_b = weak.clone();
                    let battery = battery.clone();
                    battery_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
                        if let Some(w) = weak_b.upgrade() {
                            apply_battery(&w, battery.read());
                        }
                    });
                }
            } else {
                battery_timer.stop();
            }

            // Quick / Tools
            if visible && (page == "quick" || page == "tools") {
                refresh_quick(weak.clone(), quick_busy.clone(), keep_awake.clone());
            }

            // Catatan
            if visible && page == "notes" {
                let store = NotesStore::open(&notes_path);
                show_notes(&notes_model, store.list());
                *notes_store.borrow_mut() = Some(store);
            }

            // Clipboard: polling UI hanya berjalan saat halaman terlihat.
            if visible && page == "clipboard" {
                if !clipboard_timer.running() {
                    show_clipboard(&clipboard_model, clipboard_monitor.as_ref());
                    let model = clipboard_model.clone();
                    let monitor = clipboard_monitor.clone();
                    clipboard_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
                        show_clipboard(&model, monitor.as_ref());
                    });
                }
            } else {
                clipboard_timer.stop();
            }

            // Cuaca: ambil data saat halaman terlihat, lalu perbarui tiap lima menit.
            if visible && page == "weather" {
                if !weather_timer.running() {
                    let (lat, lon, city) = weather_location.borrow().clone();
                    refresh_weather(weak.clone(), weather_busy.clone(), lat, lon, city.clone());
                    let weak_w = weak.clone();
                    let busy_w = weather_busy.clone();
                    weather_timer.start(TimerMode::Repeated, Duration::from_secs(5 * 60), move || {
                        refresh_weather(weak_w.clone(), busy_w.clone(), lat, lon, city.clone());
                    });
                }
            } else {
                weather_timer.stop();
            }
        })
    };

    {
        let sync = sync_pages.clone();
        win.on_page_changed(move |_| sync());
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_add_note(move |text| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.add(text.as_str()));
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_remove_note(move |id| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.remove(id.as_str()));
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_add(move || {
            let selected = rfd::FileDialog::new().pick_files();
            let Some(paths) = selected else { return; };
            let paths = paths.into_iter().map(|path| path.to_string_lossy().to_string()).collect();
            let items = store.borrow_mut().add(paths);
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect());
            if let Some(w) = weak.upgrade() {
                w.set_shelf_status(format!("{} item tersimpan. File asli tidak dipindahkan.", model.row_count()).into());
            }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_remove(move |path| {
            let items = store.borrow_mut().remove(path.as_str());
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect());
            if let Some(w) = weak.upgrade() { w.set_shelf_status(format!("Item dihapus dari Shelf · {} tersisa", model.row_count()).into()); }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_clear(move || {
            let items = store.borrow_mut().clear();
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect());
            if let Some(w) = weak.upgrade() { w.set_shelf_status("Shelf dikosongkan. File asli tetap aman.".into()); }
        });
    }
    win.on_shelf_open(|path| shelf::open(path.as_str()));
    win.on_shelf_reveal(|path| shelf::reveal(path.as_str()));


    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_media_control(|action| media::control(&action));
    win.on_adhan_stop(|| media::stop_adhan());
    {
      let enabled=ramadan.clone();
      let idx=dhikr_idx.clone();
      let set=set_dhikr.clone();
      let weak=win.as_weak();
      win.on_adhan_finished(move||{
        if !enabled.get(){return;}
        let i=1;
        idx.set(i);
        if let Some(w)=weak.upgrade(){set(&w,i);}
      });
    }
    win.on_media_seek(|position| media::seek(f64::from(position)));
      win.on_prayer_set_mode(move|v|{let mut p=prefs.borrow_mut();p.mode=v.to_string();if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_mode(p.mode.clone().into());}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind(move|v|{let mut p=prefs.borrow_mut();p.remind_before=v;if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_before(v);}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind_min(move|v|{let mut p=prefs.borrow_mut();p.remind_min=(p.remind_min as i32+v).clamp(1,60)as u32;if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_min(p.remind_min.to_string().into());}});
    }
    {
      let weak=win.as_weak();let enabled=ramadan.clone();
      win.on_prayer_toggle_ramadan(move||{let v=!enabled.get();enabled.set(v);let path=notch_core::paths::data_file("ramadan.json");if let Some(d)=path.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(path,if v{"true"}else{"false"});if let Some(w)=weak.upgrade(){w.set_ramadan_enabled(v);}});
    }
    {
      let weak=win.as_weak();let idx=dhikr_idx.clone();let set=set_dhikr.clone();
      win.on_prayer_dhikr_next(move||{let i=(idx.get()+1)%3;idx.set(i);if let Some(w)=weak.upgrade(){set(&w,i);}});
    }

    {
        let model = reminder_model.clone();
        let weak = win.as_weak();
        win.on_reminder_add(move |text, date_time, repeat_mode, weekdays_mask| {
            let text = text.to_string();
            let date_time = date_time.to_string();
            let mode = repeat_mode.to_string();
            if text.trim().is_empty() { return; }
            let due_at = if date_time.trim().is_empty() {
                0
            } else {
                match reminder::parse_local_datetime(&date_time, timezone_offset) {
                    Some(value) => value,
                    None => {
                        eprintln!("[Reminder] Format tanggal/waktu harus DD/MM/YYYY HH:MM");
                        return;
                    }
                }
            };
            if mode == "weekly" && (weekdays_mask & 0x7f) == 0 {
                eprintln!("[Reminder] Pilih setidaknya satu hari untuk pengulangan mingguan");
                return;
            }
            let items = reminder::add(
                text, due_at, mode, timezone_offset,
                Some((weekdays_mask as u8) & 0x7f),
            );
            show_reminders(&model, items, timezone_offset);
            if let Some(w) = weak.upgrade() {
                w.set_reminder_draft("".into());
                w.set_reminder_date_time("".into());
                w.set_reminder_repeat_mode("none".into());
            }
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_toggle(move |id| {
            show_reminders(&model, reminder::toggle(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_remove(move |id| {
            show_reminders(&model, reminder::remove(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_clear_done(move || {
            show_reminders(&model, reminder::clear_done(), timezone_offset);
        });
    }
    {
        let mask = weekday_mask.clone();
        let weak = win.as_weak();
        win.on_reminder_toggle_weekday(move |day| {
            if (0..=6).contains(&day) {
                let next = mask.get() ^ (1 << day);
                mask.set(next);
                if let Some(w) = weak.upgrade() {
                    w.set_reminder_weekdays_mask(i32::from(next));
                    w.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(next)))));
                }
            }
        });
    }

    {
        let monitor = clipboard_monitor.clone();
        win.on_clipboard_copy(move |text| {
            if let Some(monitor) = monitor.as_ref() {
                monitor.copy(text.to_string());
            }
        });
    }
    {
        let monitor = clipboard_monitor.clone();
        let model = clipboard_model.clone();
        win.on_clipboard_clear(move || {
            if let Some(monitor) = monitor.as_ref() {
                monitor.clear();
            }
            show_clipboard(&model, monitor.as_ref());
        });
    }

    // --- Reminder: data and recurrence rules use the same JSON schema as Tauri ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let model = reminder_model.clone();
        let active = active_reminder.clone();
        reminder_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let items = reminder::list();
            if let Some(w) = weak.upgrade() {
                if w.get_page_id() == "reminder" {
                    show_reminders(&model, items.clone(), timezone_offset);
                }
                let due = reminder::due_now();
                let active_id = active.borrow().clone();
                let active_due = active_id.as_ref().and_then(|id| due.iter().find(|r| &r.id == id));
                if active_id.is_some() && active_due.is_none() {
                    *active.borrow_mut() = None;
                    w.set_reminder_alert_visible(false);
                }
                if active.borrow().is_none() {
                    if let Some(r) = due.first() {
                        *active.borrow_mut() = Some(r.id.clone());
                        w.set_page_id("reminder".into());
                        w.set_reminder_alert_visible(true);
                        w.set_reminder_alert_text(r.text.clone().into());
                        let when = if r.due_at > 0 {
                            let (y, m, d, h, min) = reminder::local_parts(r.due_at, timezone_offset);
                            format!("{d:02}/{m:02}/{y} {h:02}:{min:02}")
                        } else {
                            "Pengingat".to_string()
                        };
                        w.set_reminder_alert_sub(when.into());
                        shell.place(&w, EXPANDED);
                        w.set_expanded(true);
                        show_reminders(&model, items, timezone_offset);
                    }
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_stop_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::stop(id); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_snooze_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::snooze(id, 5); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }


    let fetch_prayer:Rc<dyn Fn(bool)>={
      let busy=prayer_busy.clone();let data=prayer_data.clone();let prefs=prayer_prefs.clone();let weak=win.as_weak();let offset=timezone_offset;
      Rc::new(move|refresh|{if busy.swap(true,Ordering::SeqCst){return;}let p=prefs.borrow().clone();let busy=busy.clone();let data=data.clone();let weak=weak.clone();
       std::thread::spawn(move||{let now=reminder::now_ms();let(y,m,_,_,_)=reminder::local_parts(now,offset);let cur=prayer::month(&p.provinsi,&p.kabkota,y as i32,m as u32,refresh);let(ny,nm,_,_,_)=reminder::local_parts(now+86_400_000,offset);let nxt=if ny!=y||nm!=m{prayer::month(&p.provinsi,&p.kabkota,ny as i32,nm as u32,false)}else{cur.clone()};let err=cur.as_ref().err().cloned().unwrap_or_default();let v=PrayerSchedule{current:cur.unwrap_or_default(),next:nxt.unwrap_or_default(),error:err};if let Ok(mut g)=data.lock(){*g=v;}busy.store(false,Ordering::SeqCst);let _=weak.upgrade_in_event_loop(|w|w.invoke_prayer_data_ready());});
      })
    };
    fetch_prayer(false);
    {let f=fetch_prayer.clone();win.on_prayer_refresh(move||f(true));}
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let prefs=prayer_prefs.clone();let fired=prayer_fired.clone();let offset=timezone_offset;
      prayer_timer.start(TimerMode::Repeated,Duration::from_secs(1),move||{let Ok(s)=data.lock()else{return};let now=reminder::now_ms();let(y,m,d,_,_)=reminder::local_parts(now,offset);if let Some(w)=weak.upgrade(){if w.get_page_id()=="prayer"{render_prayer(&w,&s,offset,ramadan.get());}if let Some(day)=prayer_find(&s.current,d){let p=prefs.borrow().clone();for(name,t)in[("Subuh",&day.subuh),("Dzuhur",&day.dzuhur),("Ashar",&day.ashar),("Maghrib",&day.maghrib),("Isya",&day.isya)]{if let Some(at)=prayer_at((y,m,d),t,offset){for(kind,trig)in[("pre",at-i64::from(p.remind_min)*60_000),("at",at)]{if kind=="pre"&&!p.remind_before{continue;}if now<trig||now>=trig+600_000{continue;}let key=format!("{y}-{m}-{d}-{name}-{kind}");if !fired.borrow_mut().insert(key){continue;}if kind=="pre"{w.set_prayer_status(format!("Menjelang {name} · {} menit lagi",p.remind_min).into());}else{w.set_prayer_status(format!("Waktu {name} telah tiba").into());if p.mode=="tone"{#[cfg(windows)] unsafe{let _=windows::Win32::UI::WindowsAndMessaging::MessageBeep(0x00000040);}}else if p.mode=="adzan"{w.set_prayer_status(format!("Adzan {name} sedang diputar").into());media::play_adhan(w.as_weak(),name.to_string());}if ramadan.get(){w.set_dhikr_arabic("اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ".into());w.set_dhikr_latin("Allahumma rabba hadzihid-da'watit-tammah".into());w.set_dhikr_meaning("Dzikir setelah adzan".into());}}}}}}});
    }
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let offset=timezone_offset;
      win.on_prayer_data_ready(move||{if let Ok(s)=data.lock(){if let Some(w)=weak.upgrade(){render_prayer(&w,&s,offset,ramadan.get());}}});
    }

    // --- Kalender + hari libur Indonesia ---
    // Cache `holidays.json` dibaca sekarang (cepat); unduhan baru jalan di thread latar dan hasilnya
    // diserahkan lewat slot bersama + callback `holidays-ready` supaya state tetap di thread UI.
    let hol_path = notch_core::paths::data_file("holidays.json");
    let hol = Rc::new(RefCell::new(Holidays::new()));
    if let Some((_, items)) = calendar::read_cache(&hol_path) {
        hol.borrow_mut().ingest(&items);
    }
    let cal_view = Rc::new(RefCell::new(CalView::new(today(), &hol.borrow())));
    let cal_model = Rc::new(VecModel::<CalCell>::default());
    win.set_cal_cells(ModelRc::from(cal_model.clone()));
    win.set_cal_weekdays(ModelRc::from(Rc::new(VecModel::from(
        calendar::weekday_headers().iter().map(|s| SharedString::from(*s)).collect::<Vec<_>>(),
    ))));
    let redraw_cal: Rc<dyn Fn()> = {
        let (weak, model, view, hol) = (win.as_weak(), cal_model.clone(), cal_view.clone(), hol.clone());
        Rc::new(move || {
            if let Some(w) = weak.upgrade() {
                show_calendar(&w, &model, &view.borrow(), &hol.borrow());
            }
        })
    };
    let hol_slot: Arc<Mutex<Option<Vec<Item>>>> = Arc::new(Mutex::new(None));
    let hol_busy = Arc::new(AtomicBool::new(false));
    let start_fetch: Rc<dyn Fn()> = {
        let (weak, slot, busy, path) = (win.as_weak(), hol_slot.clone(), hol_busy.clone(), hol_path.clone());
        Rc::new(move || {
            if busy.swap(true, Ordering::SeqCst) {
                return;
            }
            let (weak, slot, busy, path) = (weak.clone(), slot.clone(), busy.clone(), path.clone());
            std::thread::spawn(move || {
                // gagal (offline, belum ada cache) = tetap memakai data bawaan
                if let Ok(items) = calendar::fetch(&path) {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(items);
                    }
                    let _ = weak.upgrade_in_event_loop(|w| w.invoke_holidays_ready());
                }
                busy.store(false, Ordering::SeqCst);
            });
        })
    };
    {
        let (hol, slot, redraw) = (hol.clone(), hol_slot.clone(), redraw_cal.clone());
        win.on_holidays_ready(move || {
            let items = slot.lock().ok().and_then(|mut g| g.take());
            if let Some(items) = items {
                hol.borrow_mut().ingest(&items);
                redraw();
            }
        });
    }
    {
        let (view, hol, redraw, fetch) = (cal_view.clone(), hol.clone(), redraw_cal.clone(), start_fetch.clone());
        win.on_cal_opened(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
            fetch(); // paling sering sekali per 12 jam (cache)
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_prev(move || {
            view.borrow_mut().shift_month(-1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_next(move || {
            view.borrow_mut().shift_month(1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_today(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, redraw) = (cal_view.clone(), redraw_cal.clone());
        win.on_cal_select(move |i| {
            view.borrow_mut().select(i.max(0) as usize);
            redraw();
        });
    }
    start_fetch(); // data libur terbaru di latar belakang sejak aplikasi mulai

    // --- Quick / Tools: aksi jalan di thread lain (bisa sampai 4 detik) ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let keep_awake = keep_awake.clone();
        win.on_quick_toggle(move |kind| {
            if kind.as_str() == "center" {
                // Kembali ke tengah atas (hanya untuk sesi ini; config tidak ditulis dari Slint).
                shell.pos.set(None);
                if let Some(w) = weak.upgrade() {
                    shell.place(&w, EXPANDED);
                }
                return;
            }
            let weak = weak.clone();
            let kind = kind.to_string();
            let keep_awake = keep_awake.clone();
            std::thread::spawn(move || {
                let q = quick::toggle(&kind);
                keep_awake.store(q.awake == Some(true), Ordering::Relaxed);
                let _ = weak.upgrade_in_event_loop(move |w| apply_quick(&w, &q));
            });
        });
    }

    // --- Hover dengan jeda dari config: timing.hover_open_ms / hover_close_ms ---
    let open_timer = Rc::new(Timer::default());
    let close_timer = Rc::new(Timer::default());
    let shrink_timer = Rc::new(Timer::default());
    let open_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_open_ms)));
    let close_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_close_ms)));
    let expand: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            shrink.stop();
            shell.place(&w, EXPANDED);
            w.set_expanded(true);
            sync();
        })
    };
    let collapse: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_expanded(false);
            w.set_cal_open(false); // kalender dan lirik ikut menutup saat notch menciut
            w.set_lyrics_open(false);
            sync();
            // jendela baru diperkecil setelah animasi menciut selesai
            let weak2 = weak.clone();
            let shell = shell.clone();
            shrink.start(TimerMode::SingleShot, Duration::from_millis(260), move || {
                if let Some(w) = weak2.upgrade() {
                    if !w.get_expanded() {
                        shell.place(&w, COLLAPSED);
                    }
                }
            });
        })
    };
    {
        let weak = win.as_weak();
        let (open_timer, close_timer) = (open_timer.clone(), close_timer.clone());
        let (open_ms, close_ms) = (open_ms.clone(), close_ms.clone());
        win.on_hover_changed(move |hovered| {
            let Some(w) = weak.upgrade() else { return };
            if hovered {
                close_timer.stop();
                if !w.get_expanded() {
                    let e = expand.clone();
                    open_timer.start(TimerMode::SingleShot, Duration::from_millis(open_ms.get()), move || e());
                }
            } else {
                open_timer.stop();
                if w.get_expanded() {
                    let c = collapse.clone();
                    close_timer.start(TimerMode::SingleShot, Duration::from_millis(close_ms.get()), move || c());
                }
            }
        });
    }

    // --- Jam: cek tiap detik, tapi hanya set properti kalau teks berubah ---
    let clock_timer = Timer::default();
    {
        let weak = win.as_weak();
        let last = Rc::new(Cell::new((99u32, 99u32)));
        let tick = move || {
            let Some(w) = weak.upgrade() else { return };
            let (h, m, d, mo, y) = platform::local_time();
            if last.get() != (h, m) {
                last.set((h, m));
                w.set_clock_text(format!("{h:02}:{m:02}").into());
                w.set_date_text(format!("{d:02}/{mo:02}/{y}").into());
            }
        };
        tick();
        clock_timer.start(TimerMode::Repeated, Duration::from_secs(1), tick);
    }

    // --- Pomodoro (state machine dari notch-core, sama dengan pomodoro.ts) ---
    let pomo = Rc::new(RefCell::new(Pomodoro::new(&cfg.pomodoro)));
    show_pomo(&win, &pomo.borrow());
    let pomo_timer = Timer::default();
    {
        let weak = win.as_weak();
        let pomo = pomo.clone();
        pomo_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let mut p = pomo.borrow_mut();
            if !p.running() {
                return;
            }
            let _finished = p.tick(Instant::now());
            if let Some(w) = weak.upgrade() {
                show_pomo(&w, &p);
            }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        win.on_toggle_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.toggle(Instant::now());
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        win.on_reset_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.reset();
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        win.on_skip_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.skip();
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }

    {
        let weak = win.as_weak();
        win.on_compress_pick(move || {
            let selected = rfd::FileDialog::new()
                .add_filter("Gambar, PDF & Office", &["jpg", "jpeg", "png", "bmp", "pdf", "docx", "xlsx", "pptx"])
                .pick_file();
            if let (Some(path), Some(w)) = (selected, weak.upgrade()) {
                w.set_compress_status(format!("Dipilih: {}", path.display()).into());
                // Isi path melalui callback state agar pengguna tidak perlu menyalin path manual.
                w.set_compress_selected_path(path.to_string_lossy().to_string().into());
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_compress_run(move |raw_path| {
            let path_text = raw_path.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = compress::compress_file(std::path::Path::new(&path_text), 30);
                let message = match result {
                    Ok((output, before, after)) => format!("Selesai: {} ({} KB → {} KB)", output.display(), before / 1024, after / 1024),
                    Err(error) => format!("Gagal: {error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_compress_status(message.into()); }
                });
            });
        });
    }

    {
        let weak = win.as_weak();
        win.on_convert_pick(move || {
            let selected = rfd::FileDialog::new()
                .add_filter("Gambar, PDF & Office", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "pdf", "docx", "xlsx", "pptx"])
                .pick_file();
            if let (Some(path), Some(w)) = (selected, weak.upgrade()) {
                w.set_convert_selected_path(path.to_string_lossy().to_string().into());
                w.set_convert_status(format!("Dipilih: {}", path.display()).into());
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_convert_run(move |raw_path, raw_format| {
            let path = raw_path.to_string();
            let format = raw_format.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let source = std::path::Path::new(&path);
                let result = if source.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("pdf")).unwrap_or(false) {
                    convert::convert_pdf_to_images(source, &format, 150, 85)
                        .map(|(output, pages, bytes)| (output, format!("{pages} halaman · {} KB", bytes / 1024)))
                } else if format.eq_ignore_ascii_case("pdf") {
                    convert::convert_to_pdf(source).map(|output| (output, "PDF".to_string()))
                } else {
                    convert::convert_image(source, &format).map(|output| (output, format.to_ascii_uppercase()))
                };
                let message = match result {
                    Ok((output, detail)) => format!("Konversi selesai ({detail}): {}", output.display()),
                    Err(error) => format!("Gagal: {error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_convert_status(message.into()); }
                });
            });
        });
    }

    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_start(move || {
            let result = state.lock().unwrap_or_else(|e| e.into_inner()).start();
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(url) => { w.set_transfer_address(url.into()); w.set_transfer_status("Server aktif — bagikan alamat ini ke perangkat lain".into()); }
                    Err(error) => w.set_transfer_status(format!("Gagal memulai server: {error}").into()),
                }
            }
        });
    }
    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_stop(move || {
            state.lock().unwrap_or_else(|e| e.into_inner()).stop();
            if let Some(w) = weak.upgrade() { w.set_transfer_address("".into()); w.set_transfer_status("Server dihentikan".into()); }
        });
    }
    {
        let state = transfer_state.clone();
        let weak = win.as_weak();
        win.on_transfer_add_files(move || {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                let strings = paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
                let count = state.lock().unwrap_or_else(|e| e.into_inner()).add_files(strings);
                if let Some(w) = weak.upgrade() { w.set_transfer_shared_count_text(count.to_string().into()); w.set_transfer_status("Daftar file dibagikan diperbarui".into()); }
            }
        });
    }
    {
        let inbox = notch_core::paths::data_dir().join("Notch");
        let weak = win.as_weak();
        win.on_transfer_open_inbox(move || {
            let result = transfer::open_inbox(&inbox);
            if let Err(error) = result {
                if let Some(w) = weak.upgrade() { w.set_transfer_status(format!("Gagal membuka folder: {error}").into()); }
            }
        });
    }

    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_refresh(move || {
            let mut android = state.lock().unwrap_or_else(|e| e.into_inner());
            let status = android.status();
            let devices = android.devices();
            if let Some(w) = weak.upgrade() {
                w.set_android_status(status.into());
                w.set_android_devices(devices.into());
                w.set_android_serial(android.serial().into());
            }
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_select_device(move |serial| {
            state.lock().unwrap_or_else(|e| e.into_inner()).set_serial(serial.to_string());
            if let Some(w) = weak.upgrade() { w.set_android_status("Perangkat dipilih".into()); }
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_list(move |path| {
            let state = state.clone();
            let path = path.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let entries = state.lock().unwrap_or_else(|e| e.into_inner()).list(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_entries(entries.into()); w.set_android_path(path.into()); }
                });
            });
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_push(move |_serial, remote| {
            let selected = rfd::FileDialog::new().pick_files().unwrap_or_default();
            if selected.is_empty() { return; }
            let state = state.clone();
            let remote = remote.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let mut ok = 0usize;
                let mut errors = Vec::new();
                for path in selected {
                    match state.lock().unwrap_or_else(|e| e.into_inner()).push(&path, &remote) {
                        Ok(()) => ok += 1,
                        Err(e) => errors.push(e),
                    }
                }
                let msg = if errors.is_empty() { format!("{ok} file berhasil dikirim") } else { format!("{ok} berhasil, gagal: {}", errors.join("; ")) };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_status(msg.into()); }
                });
            });
        });
    }
    {
        let state = android_state.clone();
        let weak = win.as_weak();
        win.on_android_pull(move |remote| {
            let destination = rfd::FileDialog::new().set_title("Pilih folder tujuan").pick_folder();
            let Some(destination) = destination else { return };
            let state = state.clone();
            let remote = remote.to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = state.lock().unwrap_or_else(|e| e.into_inner()).pull(&remote, &destination);
                let message = match result {
                    Ok(()) => format!("File berhasil diambil ke {}", destination.display()),
                    Err(error) => format!("Gagal mengambil file: {error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() { w.set_android_status(message.into()); }
                });
            });
        });
    }
    {
        let android = android_state.clone();
        let mirror = mirror_process.clone();
        let weak = win.as_weak();
        win.on_android_mirror(move || {
            let serial = android.lock().unwrap_or_else(|e| e.into_inner()).serial().to_string();
            let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).start(&serial);
            if let Some(w) = weak.upgrade() {
                w.set_android_status(match result {
                    Ok(message) => message.into(),
                    Err(error) => format!("Mirroring gagal: {error}").into(),
                });
            }
        });
    }
    {
        let mirror = mirror_process.clone();
        let weak = win.as_weak();
        win.on_android_stop_mirror(move || {
            let result = mirror.lock().unwrap_or_else(|e| e.into_inner()).stop();
            if let Some(w) = weak.upgrade() {
                w.set_android_status(match result {
                    Ok(message) => message.into(),
                    Err(error) => format!("Gagal menghentikan mirroring: {error}").into(),
                });
            }
        });
    }

    {
        let weak = win.as_weak();
        let location = weather_location.clone();
        let config_path = notch_core::paths::config_path();
        let mut updated_config = cfg.clone();
        win.on_settings_save_weather(move |city, lat, lon| {
            let city = city.to_string().trim().to_string();
            let Some(lat) = lat.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-90.0..=90.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Latitude harus angka antara -90 dan 90.".into()); }
                return;
            };
            let Some(lon) = lon.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-180.0..=180.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Longitude harus angka antara -180 dan 180.".into()); }
                return;
            };
            if city.is_empty() || city.len() > 80 {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Nama kota wajib diisi (maksimal 80 karakter).".into()); }
                return;
            }
            let mut next = updated_config.clone();
            next.weather.city = city.clone();
            next.weather.lat = lat;
            next.weather.lon = lon;
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next;
                    *location.borrow_mut() = (lat, lon, city.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_weather_city(city.clone().into());
                        w.set_weather_lat(lat.to_string().into());
                        w.set_weather_lon(lon.to_string().into());
                        w.set_weather_save_status("Lokasi tersimpan. Memperbarui cuaca...".into());
                        w.invoke_weather_refresh();
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_weather_save_status(format!("Gagal menyimpan: {error}").into());
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let eye_runtime = appearance_eye.clone();
        let sleep_after_s = sleep_after_s.clone();
        let config_path = notch_core::paths::config_path();
        let mut updated_config = cfg.clone();
        win.on_settings_save_appearance(move |theme, eye, notch, mood, accent, dark, light| {
            let mut next = updated_config.clone();
            next.appearance.theme = theme.to_string();
            next.appearance.eye_theme = eye.to_string();
            next.appearance.notch_theme = notch.to_string();
            next.appearance.default_mood = mood.to_string();
            next.appearance.accent = accent.to_string();
            next.appearance.dark_bg = dark.to_string();
            next.appearance.light_bg = light.to_string();
            next.normalize();
            if !["auto", "dark", "light"].contains(&next.appearance.theme.as_str()) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Tema harus auto, dark, atau light".into()); } return; }
            let valid_hex = |v: &str| v.is_empty() || (v.len() == 7 && v.starts_with("#") && v[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            if !valid_hex(&next.appearance.accent) || !valid_hex(&next.appearance.dark_bg) || !valid_hex(&next.appearance.light_bg) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Warna harus memakai format HEX #RRGGBB".into()); } return; }
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next.clone();
                    let palette = Palette::from_appearance(&next.appearance, platform::os_light());
                    *shell.theme.borrow_mut() = next.appearance.notch_theme.clone();
                    *shell.body.borrow_mut() = palette.body;
                    shell.memo.set((None, 0));
                    if let Some(w) = weak.upgrade() {
                        apply_theme(&w, &palette);
                        let size = if w.get_expanded() { EXPANDED } else { COLLAPSED };
                        shell.place(&w, size);
                        if let Some(eye) = eye_runtime.borrow().as_ref() {
                            let mut eye = eye.borrow_mut();
                            let skin = eyes::skin_index(&next.appearance.eye_theme);
                            eye.renderer = EyeRenderer::new(&next.appearance.eye_theme, EYES_W, EYES_H);
                            eye.skin = skin;
                            let mood = Mood::parse(&next.appearance.default_mood).unwrap_or(Mood::Normal);
                            eye.mood = MoodState::new(mood, next.timing.sleep_after_s, Instant::now());
                            sleep_after_s.set(next.timing.sleep_after_s);
                            eye.cur = mood;
                            eye.cur_since = 0.0;
                            eye.morph = Morph::new(Face::for_mood(mood, skin));
                            eye.last_key = None;
                        }
                        w.set_appearance_theme(next.appearance.theme.clone().into());
                        w.set_appearance_eye_theme(next.appearance.eye_theme.clone().into());
                        w.set_appearance_notch_theme(next.appearance.notch_theme.clone().into());
                        w.set_appearance_mood(next.appearance.default_mood.clone().into());
                        w.set_appearance_accent(next.appearance.accent.clone().into());
                        w.set_appearance_dark_bg(next.appearance.dark_bg.clone().into());
                        w.set_appearance_light_bg(next.appearance.light_bg.clone().into());
                        w.set_appearance_save_status("Tampilan tersimpan dan langsung diterapkan.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_appearance_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let open_ms = open_ms.clone();
        let close_ms = close_ms.clone();
        let sleep_after_s = sleep_after_s.clone();
        let eye_runtime = appearance_eye.clone();
        let config_path = notch_core::paths::config_path();
        let mut updated_config = cfg.clone();
        win.on_settings_save_behavior(move |scale, hover_open, hover_close, sleep_after| {
            let mut next = updated_config.clone();
            let scale = scale.parse::<f64>().ok().filter(|v| (0.5..=1.5).contains(v));
            let hover_open = hover_open.parse::<u32>().ok().filter(|v| *v <= 2000);
            let hover_close = hover_close.parse::<u32>().ok().filter(|v| *v <= 5000);
            let sleep_after = sleep_after.parse::<u32>().ok().filter(|v| (10..=3600).contains(v));
            let (Some(scale), Some(hover_open), Some(hover_close), Some(sleep_after)) =
                (scale, hover_open, hover_close, sleep_after) else {
                if let Some(w) = weak.upgrade() {
                    w.set_behavior_save_status("Skala 0.5–1.5, hover buka 0–2000 ms, hover tutup 0–5000 ms, idle 10–3600 detik.".into());
                }
                return;
            };
            next.scale = scale;
            next.timing.hover_open_ms = hover_open;
            next.timing.hover_close_ms = hover_close;
            next.timing.sleep_after_s = sleep_after;
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next.clone();
                    open_ms.set(u64::from(hover_open));
                    close_ms.set(u64::from(hover_close));
                    sleep_after_s.set(sleep_after);
                    *shell.scale.borrow_mut() = scale as f32;
                    shell.memo.set((None, 0));
                    if let Some(eye) = eye_runtime.borrow().as_ref() {
                        eye.borrow_mut().mood.set_sleep_after(sleep_after);
                    }
                    if let Some(w) = weak.upgrade() {
                        w.set_ui_scale(scale as f32);
                        let size = if w.get_expanded() { EXPANDED } else { COLLAPSED };
                        shell.place(&w, size);
                        w.set_behavior_scale(format!("{scale:.2}").into());
                        w.set_behavior_hover_open(hover_open.to_string().into());
                        w.set_behavior_hover_close(hover_close.to_string().into());
                        w.set_behavior_sleep_after(sleep_after.to_string().into());
                        w.set_behavior_save_status("Perilaku notch tersimpan dan langsung diterapkan.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_behavior_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let prefs = prayer_prefs.clone();
        let pomo = pomo.clone();
        let config_path = notch_core::paths::config_path();
        let mut updated_config = cfg.clone();
        win.on_settings_save_routines(move |mode, remind, minutes, focus, rest, long, rounds| {
            let mut next = updated_config.clone();
            let mode = mode.to_string();
            if !["tone", "adzan", "silent"].contains(&mode.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Mode shalat harus tone, adzan, atau silent".into()); } return; }
            let remind = remind.to_string();
            if !["true", "false"].contains(&remind.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Pengingat harus true atau false".into()); } return; }
            let parse = |v: slint::SharedString, min: u32, max: u32| -> Option<u32> { let n = v.parse::<u32>().ok()?; if (min..=max).contains(&n) { Some(n) } else { None } };
            let Some(minutes) = parse(minutes, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Menit pengingat harus 1–120".into()); } return; };
            let Some(focus) = parse(focus, 1, 180) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Fokus Pomodoro harus 1–180 menit".into()); } return; };
            let Some(rest) = parse(rest, 1, 60) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat harus 1–60 menit".into()); } return; };
            let Some(long) = parse(long, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat panjang harus 1–120 menit".into()); } return; };
            let Some(rounds) = parse(rounds, 1, 12) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Jumlah ronde harus 1–12".into()); } return; };
            next.prayer.mode = mode; next.prayer.remind_before = remind == "true"; next.prayer.remind_min = minutes;
            next.pomodoro.focus_min = focus; next.pomodoro.break_min = rest; next.pomodoro.long_break_min = long; next.pomodoro.rounds = rounds;
            match next.save(&config_path) {
                Ok(()) => {
                    updated_config = next.clone();
                    *prefs.borrow_mut() = next.prayer.clone();
                    let mut timer = pomo.borrow_mut();
                    timer.update_config(&next.pomodoro, Instant::now());
                    if let Some(w) = weak.upgrade() {
                        show_pomo(&w, &timer);
                        w.set_prayer_mode(next.prayer.mode.clone().into());
                        w.set_prayer_remind_before(next.prayer.remind_before.to_string().into());
                        w.set_prayer_remind_min(next.prayer.remind_min.to_string().into());
                        w.set_pomodoro_focus(next.pomodoro.focus_min.to_string().into());
                        w.set_pomodoro_break(next.pomodoro.break_min.to_string().into());
                        w.set_pomodoro_long_break(next.pomodoro.long_break_min.to_string().into());
                        w.set_pomodoro_rounds(next.pomodoro.rounds.to_string().into());
                        w.set_settings_save_status("Tersimpan dan langsung diterapkan ke timer shalat serta Pomodoro.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_settings_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    // --- Global shortcut Windows: diproses di thread UI agar aman untuk state Slint ---
    let hotkey_poll_timer = Timer::default();
    {
        let (expand, collapse, pomo) = (expand.clone(), collapse.clone(), pomo.clone());
        let weak = win.as_weak();
        hotkey_poll_timer.start(TimerMode::Repeated, Duration::from_millis(60), move || {
            while let Ok(action) = hotkey_rx.try_recv() {
                match action {
                    hotkeys::Action::ToggleHub => {
                        if let Some(w) = weak.upgrade() {
                            if w.get_expanded() { collapse(); } else { expand(); }
                        }
                    }
                    hotkeys::Action::TogglePomodoro => {
                        let mut p = pomo.borrow_mut();
                        p.toggle(Instant::now());
                        if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
                    }
                    hotkeys::Action::OpenPrayer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("prayer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::ColorPicker => {
                        colorpick::toggle();
                    }
                    hotkeys::Action::OpenAndroid => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("android".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenTransfer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("transfer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenConvert => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("convert".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenCompress => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("compress".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenSettings => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("settings".into());
                            expand();
                        }
                    }
                }
            }
        });
    }

    // --- Mata: 8 mood x 20 skin, dirender Rust (notch-core/src/eyes.rs) ---
    let started = Instant::now();
    let skin = eyes::skin_index(&cfg.appearance.eye_theme);
    let default_mood = Mood::parse(&cfg.appearance.default_mood).unwrap_or(Mood::Normal);
    let eye = Rc::new(RefCell::new(EyeCtx {
        renderer: EyeRenderer::new(&cfg.appearance.eye_theme, EYES_W, EYES_H),
        skin,
        mood: MoodState::new(default_mood, cfg.timing.sleep_after_s, started),
        cur: default_mood,
        cur_since: 0.0,
        morph: Morph::new(Face::for_mood(default_mood, skin)),
        look: (0.0, 0.0),
        last_cursor: (i32::MIN, i32::MIN),
        last_move: 0.0,
        next_random_look: 0.0,
        blink_until: 0.0,
        next_blink: 2.5,
        last_key: None,
        rng: Rng::new(),
    }));
    *appearance_eye.borrow_mut() = Some(eye.clone());

    // Klik / dobel klik pada notch (Tauri: klik -> happy atau marah bila 5x dalam 2 dtk; dobel -> love)
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.click(Instant::now());
        });
    }
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_double_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.double_click(Instant::now());
        });
    }

    // Mood dasar (tidur saat idle/malam, musik, baterai) dinilai ulang tiap 10 dtk (mood.ts)
    let base_timer = Timer::default();
    {
        let eye = eye.clone();
        let keep_awake = keep_awake.clone();
        let weak = win.as_weak();
        base_timer.start(TimerMode::Repeated, Duration::from_secs(10), move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            let mut e = eye.borrow_mut();
            e.mood.keep_awake = keep_awake.load(Ordering::Relaxed);
            e.mood.update_base(Instant::now(), platform::local_time().0);
        });
    }

    // Baterai dibaca tiap 30 dtk untuk mood (mengisi -> love, lemah -> mengantuk)
    let mood_battery_timer = Timer::default();
    {
        let eye = eye.clone();
        let battery = battery.clone();
        let weak = win.as_weak();
        let tick = move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            if let Reading::Present(b) = battery.read() {
                eye.borrow_mut().mood.on_battery(b.percent, b.charging, Instant::now(), platform::local_time().0);
            }
        };
        tick();
        mood_battery_timer.start(TimerMode::Repeated, Duration::from_secs(30), tick);
    }

    // Tick mata ~15 fps. Gambar baru dibuat hanya saat notch menciut DAN frame benar-benar berubah.
    let eye_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        eye_timer.start(TimerMode::Repeated, Duration::from_millis(66), move || {
            let Some(w) = weak.upgrade() else { return };
            let now = Instant::now();
            let ts = started.elapsed().as_secs_f32();
            let hour = platform::local_time().0;
            let mut guard = eye.borrow_mut();
            let e = &mut *guard;

            // Tatapan: ikuti kursor; bila diam > 5 dtk, melirik acak tiap 2,5 dtk (notch-face.ts)
            if let Some((cx, cy)) = platform::cursor_pos() {
                let (px, py) = e.last_cursor;
                if (cx as f32 - px as f32).hypot(cy as f32 - py as f32) > 2.0 {
                    e.last_move = ts;
                    // Aktivitas kursor tidak boleh mengubah ekspresi ketika Mode Rapat aktif.
                    if !w.get_meeting_active() {
                        e.mood.activity(now, hour);
                    }
                }
                if (cx, cy) != e.last_cursor {
                    e.last_cursor = (cx, cy);
                    // relatif terhadap notch (bukan tengah layar), supaya benar juga untuk posisi kustom
                    let (wp, ws) = (w.window().position(), w.window().size());
                    let mid_x = wp.x + ws.width as i32 / 2;
                    e.look = eyes::look_offset((cx - mid_x) as f32, (cy - wp.y) as f32);
                }
            }
            if ts - e.last_move > 5.0 && ts >= e.next_random_look {
                e.next_random_look = ts + 2.5;
                e.look = eyes::look_offset((e.rng.next() - 0.5) * 300.0, (e.rng.next() - 0.5) * 60.0);
            }

            // Mood: transisi 250 ms ke mood baru
            let m = e.mood.current(now);
            if m != e.cur {
                e.cur = m;
                e.cur_since = ts;
                let face = Face::for_mood(m, e.skin);
                e.morph.set_target(ts, face);
            }

            // Kedip acak 2,5-6 dtk (mengantuk 1,5-5 dtk); mata "love" tidak berkedip; kedip 130 ms
            if ts >= e.next_blink {
                if e.cur != Mood::Love {
                    e.blink_until = ts + 0.13;
                }
                let base = if e.cur == Mood::Sleepy { 1.5 } else { 2.5 };
                e.next_blink = ts + base + e.rng.next() * 3.5;
            }
            let blink = ts < e.blink_until;

            if w.get_expanded() {
                return; // mata tersembunyi saat hub terbuka: tidak perlu menggambar
            }

            let sf = w.window().scale_factor();
            let anim = Anim { mood: e.cur, t: ts, t_mood: ts - e.cur_since, look: e.look, blink };
            let key = (
                (e.look.0 * 4.0) as i32,
                (e.look.1 * 4.0) as i32,
                blink,
                e.cur as usize,
                eyes::frame_key(&anim, e.renderer.skin_name(), !e.morph.settled(ts)),
                sf.to_bits(),
            );
            if e.last_key == Some(key) {
                return;
            }
            e.last_key = Some(key);
            let face = e.morph.current(ts);
            let buf = e.renderer.render(&face, &anim, sf);
            let (pw, ph) = e.renderer.size_px(sf);
            w.set_eyes_image(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, pw, ph)));
        });
    }

    win.run()
}
