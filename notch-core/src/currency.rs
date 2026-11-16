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
    fn currency_catalog_contains_fifteen_codes() {
        assert_eq!(CURRENCIES.len(), 15);
    }
}
