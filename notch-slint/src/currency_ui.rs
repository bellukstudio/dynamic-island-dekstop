//! Halaman Kurs/Saham: state, pemformat nilai, penggambaran grafik, dan pengingat kurs.

use crate::*;

// ---------- Currency: kurs Rupiah, saham IDX, pengingat kurs ----------

#[derive(Default)]
pub(crate) struct CurrencyFetch {
    pub(crate) rates: Option<Result<notch_core::currency::Rates, String>>,
    pub(crate) stocks: Option<Result<Vec<notch_core::currency::StockPoint>, String>>,
}

pub(crate) struct CurrencyState {
    pub(crate) rates: Option<notch_core::currency::Rates>,
    pub(crate) stocks: Vec<notch_core::currency::StockPoint>,
    pub(crate) rates_error: String,
    pub(crate) rates_warning: String,
    pub(crate) stock_error: String,
    pub(crate) amount: String, // hanya digit
    pub(crate) target: usize,
    pub(crate) alerts: Vec<notch_core::currency::Alert>,
    pub(crate) updated: String,
    pub(crate) rates_loaded_at: Option<Instant>,
    pub(crate) stocks_loaded_at: Option<Instant>,
    pub(crate) loading: bool,
}

impl CurrencyState {
    pub(crate) fn new(alerts: Vec<notch_core::currency::Alert>) -> Self {
        Self {
            rates: None,
            stocks: Vec::new(),
            rates_error: String::new(),
            rates_warning: String::new(),
            stock_error: String::new(),
            amount: "100000".into(),
            target: 0,
            alerts,
            updated: String::new(),
            rates_loaded_at: None,
            stocks_loaded_at: None,
            loading: false,
        }
    }
}

/// Format Rupiah untuk grafik kurs: dua desimal di bawah 1.000 (mis. Yen), selebihnya bilangan bulat dengan titik ribuan.
pub(crate) fn fx_idr(value: f64) -> String {
    if value < 1000.0 { format!("Rp {}", format!("{value:.2}").replace('.', ",")) } else { format!("Rp {}", group_digits(&format!("{value:.0}"))) }
}

pub(crate) fn fx_axis_label(value: f64, span: f64) -> String {
    let decimals = if span < 10.0 { 2 } else if span < 100.0 { 1 } else { 0 };
    fmt_id_number(value, decimals)
}

pub(crate) const CURRENCY_QUICK: [u64; 6] = [10_000, 50_000, 100_000, 500_000, 1_000_000, 10_000_000];
pub(crate) fn rupiah(value: f64) -> String {
    format!("Rp {}", fmt_id_number(value, if value >= 1000.0 { 0 } else { 2 }))
}

pub(crate) fn fmt_currency_value(value: f64, code: &str) -> String {
    let decimals = if matches!(code, "JPY" | "KRW") { 0 } else { 2 };
    let mut number = fmt_id_number(value, decimals);
    if decimals > 0 {
        while number.ends_with('0') { number.pop(); }
        if number.ends_with(',') { number.pop(); }
    }
    let (symbol, spaced) = match code {
        "USD" => ("US$", false), "SGD" => ("SGD", true), "EUR" => ("€", false),
        "JPY" => ("JP¥", false), "MYR" => ("MYR", true), "CNY" => ("CN¥", false),
        "GBP" => ("GBP", true), "AUD" => ("A$", false), "SAR" => ("SAR", true),
        "KRW" => ("₩", false), "THB" => ("฿", false), "INR" => ("₹", false),
        "CHF" => ("CHF", true), "HKD" => ("HK$", false), "CAD" => ("CA$", false),
        _ => (code, true),
    };
    if spaced { format!("{symbol} {number}") } else { format!("{symbol}{number}") }
}

pub(crate) fn alert_text(a: &notch_core::currency::Alert) -> String {
    format!("{} {} {}", a.code, if a.above { "\u{2265}" } else { "\u{2264}" }, rupiah(a.target))
}

pub(crate) fn paint_currency(
    w: &NotchWindow,
    s: &CurrencyState,
    gainers: &VecModel<StockRow>,
    losers: &VecModel<StockRow>,
    alerts: &VecModel<CurrencyAlertItem>,
) {
    use notch_core::currency::CURRENCIES;
    let (code, name) = CURRENCIES[s.target % CURRENCIES.len()];
    let amount: f64 = s.amount.parse().unwrap_or(0.0);
    let per_unit = s.rates.as_ref().and_then(|r| r.idr_for(code));
    let (result, rate) = match per_unit {
        Some(idr) => {
            let value = amount / idr;
            (
                fmt_currency_value(value, code),
                format!("1 {code} = {}", rupiah(idr)),
            )
        }
        None => (String::new(), String::new()),
    };
    const FULL_MONTHS: [&str; 12] = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
    let (year, month) = notch_core::currency::year_month();
    w.set_currency(CurrencyView {
        amount: group_digits(&s.amount).into(),
        code: code.into(),
        name: name.into(),
        result: result.into(),
        rate: rate.into(),
        alert_rate: per_unit.map(rupiah).unwrap_or_else(|| "—".to_string()).into(),
        updated: s.updated.clone().into(),
        warning: s.rates_warning.clone().into(),
        stock_period: format!("MTD \u{00B7} 1 {}", SHORT_MONTHS[(month.clamp(1, 12) - 1) as usize]).into(),
        stock_status: if s.stock_error.is_empty() {
            format!("Pilihan saham IDX \u{00B7} {} {} \u{00B7} data Yahoo Finance, bukan seluruh bursa", FULL_MONTHS[(month.clamp(1, 12) - 1) as usize], year).into()
        } else {
            s.stock_error.clone().into()
        },
        error: s.rates_error.clone().into(),
        loading: s.loading,
    });
    let currency_rows = CURRENCIES.iter().map(|(item_code, item_name)| {
        let value = s.rates.as_ref().and_then(|rates| rates.idr_for(item_code));
        CurrencyItem {
            code: (*item_code).into(),
            name: (*item_name).into(),
            value: value.map(fx_idr).unwrap_or_else(|| "—".into()).into(),
            selected: *item_code == code,
        }
    }).collect::<Vec<_>>();
    w.set_currency_items(ModelRc::from(Rc::new(VecModel::from(currency_rows))));
    if w.get_currency_amount().as_str() != group_digits(&s.amount) {
        w.set_currency_amount(group_digits(&s.amount).into());
    }
    let row = |p: &notch_core::currency::StockPoint, positive: bool| StockRow {
        name: p.label.clone().into(),
        price: rupiah(p.last).into(),
        change: format!("{:+.2}%", p.change).replace('.', ",").into(),
        positive,
    };
    gainers.set_vec(s.stocks.iter().filter(|p| p.change > 0.0).take(3).map(|p| row(p, true)).collect::<Vec<_>>());
    let mut down: Vec<_> = s.stocks.iter().filter(|p| p.change < 0.0).collect();
    down.sort_by(|a, b| a.change.partial_cmp(&b.change).unwrap_or(std::cmp::Ordering::Equal));
    losers.set_vec(down.into_iter().take(3).map(|p| row(p, false)).collect::<Vec<_>>());
    alerts.set_vec(
        s.alerts
            .iter()
            .map(|a| CurrencyAlertItem {
                id: a.id.clone().into(),
                text: alert_text(a).into(),
                stock: a.kind == notch_core::currency::AlertKind::Stock,
                repeat: a.repeat,
                armed: a.armed,
            })
            .collect::<Vec<_>>(),
    );
}

/// Periksa alert kurs dan saham dari snapshot harga terbaru.
/// Alert sekali pakai dihapus setelah tampil; alert berulang menunggu harga kembali
/// ke sisi sebaliknya dari ambang sebelum dipersenjatai lagi.
pub(crate) fn fire_due_currency_alert(w: &NotchWindow, s: &mut CurrencyState, path: &std::path::Path) -> bool {
    let can_show = !w.get_expanded() && !w.get_nudge_visible() && !w.get_reminder_alert_visible()
        && !w.get_meeting_active() && w.get_meeting_finished().is_empty() && !w.get_adhan_active();

    let mut rearmed = false;
    let mut due: Option<(notch_core::currency::Alert, f64)> = None;
    for alert in &mut s.alerts {
        let now = match alert.kind {
            notch_core::currency::AlertKind::Currency => s.rates.as_ref().and_then(|rates| rates.idr_for(&alert.code)),
            notch_core::currency::AlertKind::Stock => s.stocks.iter()
                .find(|point| point.label == alert.code)
                .map(|point| point.last),
        };
        let Some(now) = now.filter(|value| value.is_finite() && *value > 0.0) else { continue };
        if alert.repeat && !alert.armed {
            if notch_core::currency::alert_returned_to_opposite_side(alert, now) {
                alert.armed = true;
                rearmed = true;
            }
            continue;
        }
        if notch_core::currency::alert_target_reached(alert, now) && can_show {
            due = Some((alert.clone(), now));
            break;
        }
    }

    if rearmed {
        notch_core::currency::save_alerts(path, &s.alerts);
    }
    let Some((alert, now)) = due else { return rearmed };
    if alert.repeat {
        if let Some(saved) = s.alerts.iter_mut().find(|item| item.id == alert.id) {
            saved.armed = false;
        }
    } else {
        s.alerts.retain(|item| item.id != alert.id);
    }

    let action = if alert.above { "naik" } else { "turun" };
    let asset = match alert.kind {
        notch_core::currency::AlertKind::Stock => "saham IDX",
        notch_core::currency::AlertKind::Currency => "kurs",
    };
    w.set_nudge_message(format!("{} {} ke {}", alert.code, action, rupiah(now)).into());
    w.set_nudge_detail(format!("Pengingat {asset} · target {}", alert_text(&alert)).into());
    w.set_nudge_visible(true);
    let weak = w.as_weak();
    Timer::single_shot(Duration::from_secs(8), move || {
        if let Some(w) = weak.upgrade() {
            w.set_nudge_visible(false);
        }
    });
    notch_core::currency::save_alerts(path, &s.alerts);
    true
}
