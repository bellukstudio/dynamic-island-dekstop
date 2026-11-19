//! Layanan data kurs dan riwayat pasar untuk UI native Slint.
//! Semua request bersifat blocking: panggil dari worker thread, bukan thread UI.

use serde_json::Value;
use std::collections::BTreeMap;

pub const CURRENCIES: &[(&str, &str)] = &[
    ("USD", "Dolar AS"), ("SGD", "Dolar Singapura"), ("EUR", "Euro"),
    ("JPY", "Yen Jepang"), ("MYR", "Ringgit Malaysia"), ("CNY", "Yuan Tiongkok"),
    ("GBP", "Pound Inggris"), ("AUD", "Dolar Australia"), ("SAR", "Riyal Saudi"),
    ("KRW", "Won Korea"), ("THB", "Baht Thailand"), ("INR", "Rupee India"),
    ("CHF", "Franc Swiss"), ("HKD", "Dolar Hong Kong"), ("CAD", "Dolar Kanada"),
];

const STOCKS: &[&str] = &[
    "BBCA.JK", "BBRI.JK", "BMRI.JK", "BBNI.JK", "TLKM.JK", "ASII.JK",
    "ANTM.JK", "GOTO.JK", "ICBP.JK", "MDKA.JK", "BRPT.JK", "INDF.JK",
];

#[derive(Clone, Debug)]
pub struct Rates {
    /// Banyaknya IDR untuk satu unit mata uang asing.
    pub idr_per_unit: BTreeMap<String, f64>,
    pub date: String,
    pub source: String,
}

#[derive(serde::Deserialize)]
struct RateResponse {
    date: Option<String>,
    rates: Option<BTreeMap<String, f64>>,
    time_last_update_utc: Option<String>,
}

/// Ambil kurs IDR dari beberapa sumber publik. Nilai tidak valid tidak pernah dipakai;
/// bila suatu kode tidak tersedia dari seluruh sumber, kode itu tidak dimasukkan.
pub fn fetch_rates() -> Result<Rates, String> {
    let sources = [
        ("https://open.er-api.com/v6/latest/IDR", "ExchangeRate-API"),
        ("https://api.frankfurter.dev/v1/latest?base=IDR", "Frankfurter"),
        ("https://api.frankfurter.app/latest?from=IDR", "Frankfurter fallback"),
    ];
    let mut merged = BTreeMap::new();
    let mut date = String::new();
    let mut used_sources = Vec::new();
    let mut last_error = String::from("Sumber kurs tidak tersedia");

    for (url, source) in sources {
        if merged.len() == CURRENCIES.len() { break; }
        match crate::http::get_text(url, 9)
            .and_then(|body| serde_json::from_str::<RateResponse>(&body).map_err(|e| e.to_string()))
        {
            Ok(response) => {
                if let Some(rates) = response.rates {
                    for (code, _) in CURRENCIES {
                        if let Some(value) = rates.get(*code).copied() {
                            if value.is_finite() && value > 0.0 {
                                merged.entry((*code).to_string()).or_insert(value);
                            }
                        }
                    }
                    if !merged.is_empty() {
                        used_sources.push(source);
                        if date.is_empty() {
                            date = response.date.or(response.time_last_update_utc).unwrap_or_default();
                        }
                    }
                }
            }
            Err(error) => last_error = error,
        }
    }

    if merged.is_empty() { return Err(last_error); }
    Ok(Rates { idr_per_unit: merged, date, source: used_sources.join(", ") })
}

fn allowed_symbol(symbol: &str) -> bool {
    STOCKS.contains(&symbol) || symbol.strip_suffix("IDR=X")
        .is_some_and(|code| CURRENCIES.iter().any(|(allowed, _)| *allowed == code))
}

/// Mengambil respons chart Yahoo Finance untuk saham IDX atau pasangan mata uang yang diizinkan.
pub fn fetch_yahoo_chart(symbol: &str, period1: i64, period2: i64) -> Result<Value, String> {
    if !allowed_symbol(symbol) { return Err(format!("Simbol tidak diizinkan: {symbol}")); }
    if period1 <= 0 || period2 <= period1 { return Err("Rentang tanggal tidak valid".into()); }

    let encoded = symbol.replace('=', "%3D");
    let mut last_error = String::from("Yahoo Finance tidak dapat diakses");
    for host in ["query1.finance.yahoo.com", "query2.finance.yahoo.com"] {
        let url = format!("https://{host}/v8/finance/chart/{encoded}?period1={period1}&period2={period2}&interval=1d&events=history");
        match crate::http::get_text(&url, 12) {
            Ok(body) => match serde_json::from_str::<Value>(&body) {
                Ok(value) if value.pointer("/chart/result/0").is_some() => return Ok(value),
                Ok(_) => last_error = "Format respons Yahoo Finance tidak valid".into(),
                Err(error) => last_error = format!("Respons Yahoo Finance bukan JSON valid: {error}"),
            },
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

impl Rates {
    /// Rupiah untuk satu unit `code`. API sumber mengembalikan "unit asing per 1 IDR" (jauh di bawah 1),
    /// sehingga nilainya dibalik; nilai di atas 1 dianggap sudah berupa IDR per unit.
    pub fn idr_for(&self, code: &str) -> Option<f64> {
        let v = *self.idr_per_unit.get(code)?;
        if !v.is_finite() || v <= 0.0 { return None; }
        Some(if v > 1.0 { v } else { 1.0 / v })
    }
}

/// Performa satu saham IDX sejak awal periode.
#[derive(Clone, Debug)]
pub struct StockPoint {
    pub label: String,
    pub last: f64,
    /// Persen perubahan dari penutupan pertama periode ke penutupan terakhir.
    pub change: f64,
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Epoch detik awal bulan berjalan (UTC) dan label "1 Nov" untuk periode MTD.
pub fn month_start() -> (i64, i64) {
    let now = now_secs();
    let (y, m) = civil_from_days(now.div_euclid(86_400));
    (days_from_civil(y, m, 1) * 86_400, m)
}

fn stock_point(symbol: &str, period1: i64, period2: i64) -> Option<StockPoint> {
    let data = fetch_yahoo_chart(symbol, period1, period2).ok()?;
    let result = data.pointer("/chart/result/0")?;
    let times = result.get("timestamp")?.as_array()?;
    let closes = result.pointer("/indicators/quote/0/close")?.as_array()?;
    let mut points: Vec<(i64, f64)> = closes
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((times.get(i)?.as_i64()?, c.as_f64()?)))
        .filter(|(t, c)| *t > 0 && c.is_finite() && *c > 0.0)
        .collect();
    points.sort_by_key(|(t, _)| *t);
    if points.len() < 2 { return None; }
    let first = points[0].1;
    let last = points[points.len() - 1].1;
    Some(StockPoint { label: symbol.trim_end_matches(".JK").to_string(), last, change: (last - first) / first * 100.0 })
}

fn collect_stocks(period1: i64, period2: i64) -> Vec<StockPoint> {
    let mut out = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = STOCKS.iter().map(|symbol| scope.spawn(move || stock_point(symbol, period1, period2))).collect();
        for handle in handles {
            if let Ok(Some(point)) = handle.join() { out.push(point); }
        }
    });
    out
}

/// Performa saham IDX pilihan bulan berjalan (MTD), terurut dari yang terbaik. Blocking.
/// Di awal bulan data belum cukup, sehingga rentang diperluas ke 30 hari terakhir.
pub fn fetch_stocks() -> Result<Vec<StockPoint>, String> {
    let (start, _) = month_start();
    let end = now_secs() + 86_400;
    let mut points = collect_stocks(start, end);
    if points.len() < 2 { points = collect_stocks(end - 31 * 86_400, end); }
    if points.len() < 2 {
        return Err("Data saham belum tersedia. Periksa koneksi atau akses sumber data.".into());
    }
    points.sort_by(|a, b| b.change.partial_cmp(&a.change).unwrap_or(std::cmp::Ordering::Equal));
    Ok(points)
}

/// Pengingat kurs: beri tahu sekali ketika 1 `code` mencapai `target` rupiah.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Alert {
    pub id: String,
    pub code: String,
    /// true = saat kurs >= target, false = saat kurs <= target.
    pub above: bool,
    pub target: f64,
}

pub fn load_alerts(path: &std::path::Path) -> Vec<Alert> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Alert>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.target.is_finite() && a.target > 0.0 && CURRENCIES.iter().any(|(c, _)| *c == a.code))
        .collect()
}

pub fn save_alerts(path: &std::path::Path, alerts: &[Alert]) {
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Ok(text) = serde_json::to_string_pretty(alerts) { let _ = std::fs::write(path, text); }
}

/// Alert yang sudah tercapai oleh `rates`, beserta kurs saat ini (rupiah per unit).
pub fn due_alerts(alerts: &[Alert], rates: &Rates) -> Vec<(Alert, f64)> {
    alerts
        .iter()
        .filter_map(|a| {
            let now = rates.idr_for(&a.code)?;
            let hit = if a.above { now >= a.target } else { now <= a.target };
            hit.then(|| (a.clone(), now))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_allowlist_rejects_arbitrary_symbols() {
        assert!(allowed_symbol("BBCA.JK"));
        assert!(allowed_symbol("USDIDR=X"));
        assert!(!allowed_symbol("AAPL"));
        assert!(!allowed_symbol("FAKEIDR=X"));
    }

    #[test]
    fn rates_are_inverted_when_given_per_idr() {
        let mut r = Rates { idr_per_unit: BTreeMap::new(), date: String::new(), source: String::new() };
        r.idr_per_unit.insert("USD".into(), 1.0 / 16_000.0);
        r.idr_per_unit.insert("EUR".into(), 17_500.0);
        assert!((r.idr_for("USD").unwrap() - 16_000.0).abs() < 1e-6);
        assert_eq!(r.idr_for("EUR"), Some(17_500.0));
        assert_eq!(r.idr_for("JPY"), None);
    }

    #[test]
    fn alerts_fire_on_threshold() {
        let mut r = Rates { idr_per_unit: BTreeMap::new(), date: String::new(), source: String::new() };
        r.idr_per_unit.insert("USD".into(), 1.0 / 16_500.0);
        let up = Alert { id: "1".into(), code: "USD".into(), above: true, target: 16_400.0 };
        let down = Alert { id: "2".into(), code: "USD".into(), above: false, target: 16_400.0 };
        let due = due_alerts(&[up, down], &r);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0.id, "1");
    }

    #[test]
    fn month_start_is_first_day() {
        let (start, month) = month_start();
        assert!((1..=12).contains(&month));
        assert_eq!(start % 86_400, 0);
    }

    #[test]
    fn currency_catalog_contains_fifteen_codes() {
        assert_eq!(CURRENCIES.len(), 15);
    }
}

// ---------- Grafik riwayat kurs (halaman Currency yang diperluas) ----------

/// Ambil pasangan (waktu detik, penutupan) dari respons Yahoo chart. Nilai tidak valid dibuang.
pub fn parse_fx_points(data: &Value) -> Vec<(i64, f64)> {
    let ts = data.pointer("/chart/result/0/timestamp").and_then(Value::as_array);
    let closes = data.pointer("/chart/result/0/indicators/quote/0/close").and_then(Value::as_array);
    let (Some(ts), Some(closes)) = (ts, closes) else { return Vec::new() };
    let mut out: Vec<(i64, f64)> = ts
        .iter()
        .zip(closes.iter())
        .filter_map(|(t, c)| {
            let (t, c) = (t.as_i64()?, c.as_f64()?);
            (t > 0 && c.is_finite() && c > 0.0).then_some((t, c))
        })
        .collect();
    out.sort_by_key(|p| p.0);
    out
}

/// Lebar ruang grafik. Tinggi 100. Rasionya (2,4:1) mendekati panel grafik di Slint, karena `Path` Slint mempertahankan rasio viewbox.
pub const FX_CHART_W: f64 = 240.0;

/// Garis dan area grafik dalam ruang `FX_CHART_W` x 100 (untuk `Path` Slint dengan viewbox 240 x 100).
#[derive(Clone, Debug, PartialEq)]
pub struct FxChart {
    pub line: String,
    pub area: String,
    pub min: f64,
    pub max: f64,
    pub first: f64,
    pub last: f64,
}

pub fn build_fx_chart(points: &[(i64, f64)]) -> Option<FxChart> {
    if points.len() < 2 { return None; }
    let min = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (t0, t1) = (points[0].0 as f64, points[points.len() - 1].0 as f64);
    let span_t = (t1 - t0).max(1.0);
    let span_v = max - min;
    // Sama seperti Tauri: sediakan padding 14% pada skala Y agar garis tidak menempel di batas plot.
    let value_pad = if span_v.abs() <= f64::EPSILON { max.abs() * 0.01 } else { span_v * 0.14 };
    let axis_min = min - value_pad;
    let axis_span = (max + value_pad - axis_min).max(f64::EPSILON);
    let xy = |p: &(i64, f64)| {
        let x = (p.0 as f64 - t0) / span_t * FX_CHART_W;
        let y = if span_v <= f64::EPSILON { 50.0 } else { 94.0 - (p.1 - axis_min) / axis_span * 88.0 };
        (x, y)
    };
    let mut line = String::new();
    for (i, p) in points.iter().enumerate() {
        let (x, y) = xy(p);
        line.push_str(&format!("{} {:.2} {:.2} ", if i == 0 { "M" } else { "L" }, x, y));
    }
    let (lx, _) = xy(&points[points.len() - 1]);
    let area = format!("{}L {:.2} 100 L 0 100 Z", line, lx);
    Some(FxChart { line: line.trim_end().to_string(), area, min, max, first: points[0].1, last: points[points.len() - 1].1 })
}

pub fn iso_date_from_unix(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let (year, month) = civil_from_days(days);
    let day = days - days_from_civil(year, month, 1) + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

/// Label tanggal pendek untuk sumbu grafik Currency, mengikuti format lokal Indonesia.
pub fn short_fx_date_from_unix(timestamp: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    let days = timestamp.div_euclid(86_400);
    let (year, month) = civil_from_days(days);
    let day = days - days_from_civil(year, month, 1) + 1;
    format!("{day} {}", MONTHS[(month.clamp(1, 12) - 1) as usize])
}

fn unix_from_iso_date(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // civil_from_days hanya mengembalikan tahun dan bulan. Cocokkan keduanya
    // agar tanggal yang dinormalisasi (mis. 31 Februari) ditolak.
    if civil_from_days(days) != (year, month) {
        return None;
    }
    Some(days * 86_400)
}

/// Riwayat kurs untuk panel Currency yang diperluas.
/// Coba Yahoo Finance terlebih dahulu, lalu Frankfurter (ECB) sebagai fallback seperti Tauri.
/// Hanya kode yang tercantum di CURRENCIES yang dapat dipakai.
pub fn fetch_fx_history_with_source(code: &str, period1: i64, period2: i64) -> Result<(Vec<(i64, f64)>, &'static str), String> {
    if !CURRENCIES.iter().any(|(allowed, _)| *allowed == code) {
        return Err(format!("Mata uang tidak diizinkan: {code}"));
    }
    if period1 <= 0 || period2 <= period1 {
        return Err("Rentang tanggal tidak valid".into());
    }

    let symbol = format!("{code}IDR=X");
    if let Ok(data) = fetch_yahoo_chart(&symbol, period1, period2) {
        let points = parse_fx_points(&data);
        if points.len() >= 2 {
            return Ok((points, "Yahoo Finance"));
        }
    }

    let start = iso_date_from_unix(period1);
    // Frankfurter memakai tanggal kalender hingga hari ini; period2 untuk Yahoo bersifat eksklusif.
    let end = iso_date_from_unix(now_secs());
    let url = format!("https://api.frankfurter.dev/v1/{start}..{end}?base={code}&symbols=IDR");
    let body = crate::http::get_text(&url, 12)
        .map_err(|error| format!("Yahoo Finance dan Frankfurter tidak tersedia: {error}"))?;
    let json: Value = serde_json::from_str(&body)
        .map_err(|error| format!("Respons Frankfurter tidak valid: {error}"))?;
    let mut points = json.get("rates")
        .and_then(Value::as_object)
        .map(|rates| {
            rates.iter().filter_map(|(day, values)| {
                let time = unix_from_iso_date(day)?;
                let rate = values.get("IDR")?.as_f64()?;
                (rate.is_finite() && rate > 0.0).then_some((time, rate))
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    points.sort_by_key(|point| point.0);
    if points.len() < 2 {
        return Err(format!("Riwayat {code} belum tersedia dari Yahoo Finance maupun Frankfurter"));
    }
    Ok((points, "Frankfurter (ECB)"))
}

/// Compatibility wrapper for callers that only need historical point values.
pub fn fetch_fx_history(code: &str, period1: i64, period2: i64) -> Result<Vec<(i64, f64)>, String> {
    fetch_fx_history_with_source(code, period1, period2).map(|(points, _)| points)
}

/// Localized date label for chart inspection tooltips.
pub fn full_fx_date_from_unix(timestamp: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    let days = timestamp.div_euclid(86_400);
    let (year, month) = civil_from_days(days);
    let day = days - days_from_civil(year, month, 1) + 1;
    format!("{day} {} {year}", MONTHS[(month.clamp(1, 12) - 1) as usize])
}

#[cfg(test)]
mod fx_chart_tests {
    use super::*;

    #[test]
    fn parses_and_builds_chart() {
        let json: Value = serde_json::from_str(r#"{"chart":{"result":[{"timestamp":[300,100,200,400],
            "indicators":{"quote":[{"close":[16000.0,15000.0,null,17000.0]}]}}]}}"#).unwrap();
        let pts = parse_fx_points(&json);
        assert_eq!(pts, vec![(100, 15000.0), (300, 16000.0), (400, 17000.0)]);
        let c = build_fx_chart(&pts).unwrap();
        assert_eq!((c.min, c.max, c.first, c.last), (15000.0, 17000.0, 15000.0, 17000.0));
        assert!(c.line.starts_with("M 0.00 94.00 "));
        assert!(c.line.ends_with("L 240.00 6.00"));
        assert!(build_fx_chart(&pts[..1]).is_none());
    }

    #[test]
    fn frankfurter_dates_round_trip_across_leap_day() {
        let timestamp = unix_from_iso_date("2024-02-29").unwrap();
        assert_eq!(iso_date_from_unix(timestamp), "2024-02-29");
        assert_eq!(full_fx_date_from_unix(timestamp), "29 Feb 2024");
        assert_eq!(unix_from_iso_date("1970-01-01"), Some(0));
    }

    #[test]
    fn frankfurter_rejects_invalid_calendar_dates() {
        assert_eq!(unix_from_iso_date("2023-02-29"), None);
        assert_eq!(unix_from_iso_date("2024-13-01"), None);
        assert_eq!(unix_from_iso_date("not-a-date"), None);
    }

    #[test]
    fn flat_series_sits_in_the_middle() {
        let c = build_fx_chart(&[(1, 5.0), (2, 5.0)]).unwrap();
        assert!(c.line.contains("50.00"));
    }
}
