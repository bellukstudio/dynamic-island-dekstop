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
