//! Pengisi panel UI sederhana: baterai, clipboard, cuaca, Quick/Tools, Pomodoro, dan kalender.

use crate::*;

pub(crate) fn apply_battery(w: &NotchWindow, reading: Reading) {
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

/// Perbarui tiga item terbaru pada halaman Clipboard. Riwayat internal tetap menyimpan 10 item.
pub(crate) fn show_clipboard(model: &VecModel<ClipboardItem>, monitor: Option<&ClipboardMonitor>) {
    let items = monitor.map(ClipboardMonitor::list).unwrap_or_default();
    model.set_vec(
        items
            .into_iter()
            .take(10)
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
pub(crate) fn refresh_weather(
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



/// None -> -1 (tidak didukung/belum terbaca), false -> 0, true -> 1.
pub(crate) fn tri(v: Option<bool>) -> i32 {
    match v {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    }
}

pub(crate) fn apply_quick(w: &NotchWindow, q: &Quick) {
    w.set_q_mic(tri(q.mic));
    w.set_q_wifi(tri(q.wifi));
    w.set_q_bt(tri(q.bt));
    w.set_q_awake(tri(q.awake));
    w.set_q_dark(tri(q.dark));
}

/// Baca keadaan Quick di thread lain (bisa sampai 3 detik) lalu tampilkan di thread UI.
pub(crate) fn refresh_quick(weak: Weak<NotchWindow>, busy: Arc<AtomicBool>, awake: Arc<AtomicBool>) {
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

pub(crate) fn show_pomo(w: &NotchWindow, p: &Pomodoro) {
    w.set_pomo_text(p.time_text().into());
    w.set_pomo_label(p.label().into());
    w.set_pomo_progress(p.progress());
    w.set_pomo_running(p.running());
    w.set_pomo_rest(p.phase() != Phase::Focus);
}

/// Tanggal lokal hari ini.
pub(crate) fn today() -> Date {
    let (_, _, d, m, y) = platform::local_time();
    Date::new(y as i32, m, d)
}

/// Isi ulang kisi kalender, judul, dan keterangan di UI dari keadaan `view` + data libur.
pub(crate) fn show_calendar(w: &NotchWindow, model: &VecModel<CalCell>, view: &CalView, hol: &Holidays) {
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

/// Cari kota lewat Open-Meteo geocoding, sama dengan kolom "Cari kota" di Pengaturan Tauri.
/// Blokir sampai selesai: panggil dari thread latar. Hasil: (label, nama, lat, lon).
pub(crate) fn search_cities(query: &str) -> Result<Vec<(String, String, String, String)>, String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language=id&format=json",
        percent_encode(query)
    );
    let body = notch_core::http::get_text(&url, 8)?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for item in json.get("results").and_then(|r| r.as_array()).into_iter().flatten() {
        let (Some(name), Some(lat), Some(lon)) = (
            item.get("name").and_then(|v| v.as_str()),
            item.get("latitude").and_then(|v| v.as_f64()),
            item.get("longitude").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        let mut label = name.to_string();
        for key in ["admin1", "country"] {
            if let Some(part) = item.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty() && *v != name) {
                label.push_str(", ");
                label.push_str(part);
            }
        }
        hits.push((label, name.chars().take(80).collect(), lat.to_string(), lon.to_string()));
    }
    Ok(hits)
}
