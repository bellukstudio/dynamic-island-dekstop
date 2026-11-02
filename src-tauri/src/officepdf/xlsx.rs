//! XLSX -> PDF: setiap sheet jadi satu/lebih halaman. Mendukung shared strings, format angka/tanggal/
//! persen/mata uang, lebar kolom & tinggi baris, merge cell, isi warna (RGB + tema/tint), border,
//! perataan, wrap text, teks meluber ke sel kosong di sebelah, dan skala otomatis agar kolom muat.
use super::pdf::Pdf;
use super::text::*;
use super::util::*;
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Locale tampilan angka/tanggal: true = Indonesia (1.234,56 · nama bulan Indonesia).
const LOCALE_ID: bool = true;

const MONTHS_FULL: [&str; 12] = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
const MONTHS_ABBR: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
const DAYS_FULL: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
const DAYS_ABBR: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];

// ------------------------------------------------------------------------------------------------
// Format angka & tanggal
// ------------------------------------------------------------------------------------------------
fn builtin_fmt(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        14 => "dd/mm/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "dd/mm/yyyy h:mm",
        37 | 38 => "#,##0;-#,##0",
        39 | 40 | 43 | 44 => "#,##0.00;-#,##0.00",
        41 | 42 => "#,##0",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        49 => "@",
        _ => return None,
    })
}

fn split_sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut q = false;
    for c in code.chars() {
        if c == '"' {
            q = !q;
        }
        if c == ';' && !q {
            out.push(String::new());
        } else {
            out.last_mut().unwrap().push(c);
        }
    }
    out
}

fn is_date_code(sec: &str) -> bool {
    let (mut q, mut br, mut esc) = (false, false, false);
    let (mut has_ph, mut has_m, mut has_other) = (false, false, false);
    for c in sec.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' => esc = true,
            '"' => q = !q,
            '[' if !q => br = true,
            ']' if !q => br = false,
            _ if q || br => {}
            '0' | '#' | '?' => has_ph = true,
            'm' | 'M' => has_m = true,
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => has_other = true,
            _ => {}
        }
    }
    has_other || (has_m && !has_ph)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_date(v: f64, sec: &str) -> String {
    let whole = v.floor();
    let unix = if whole >= 61.0 { whole as i64 - 25569 } else { whole as i64 - 25568 };
    let (y, mo, d) = civil_from_days(unix);
    let wd = (unix + 4).rem_euclid(7) as usize;
    let secs = (((v - whole) * 86400.0).round() as i64).clamp(0, 86399);
    let (hh, mi, ss) = ((secs / 3600) as u32, ((secs % 3600) / 60) as u32, (secs % 60) as u32);
    let lower = sec.to_lowercase();
    let ampm = lower.contains("am/pm") || lower.contains("a/p");
    let ch: Vec<char> = lower.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last_hour = false;
    while i < ch.len() {
        let c = ch[i];
        let mut run = 1;
        while i + run < ch.len() && ch[i + run] == c {
            run += 1;
        }
        match c {
            '"' => {
                i += 1;
                while i < ch.len() && ch[i] != '"' {
                    out.push(ch[i]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            '\\' => {
                if i + 1 < ch.len() {
                    out.push(ch[i + 1]);
                }
                i += 2;
                continue;
            }
            '[' => {
                // [h] / [mm] -> perlakukan sebagai token biasa; lainnya dilewati
                let mut j = i + 1;
                while j < ch.len() && ch[j] != ']' {
                    j += 1;
                }
                let inner: String = ch[i + 1..j.min(ch.len())].iter().collect();
                if inner.starts_with('h') {
                    out.push_str(&(hh + (whole as u32 * 0)).to_string());
                    last_hour = true;
                }
                i = j + 1;
                continue;
            }
            'y' => {
                if run >= 3 {
                    out.push_str(&format!("{y:04}"));
                } else {
                    out.push_str(&format!("{:02}", y.rem_euclid(100)));
                }
                last_hour = false;
            }
            'm' => {
                let next_s = ch[i + run..].iter().find(|c| !matches!(c, ':' | ' ' | '.')).map(|c| *c == 's').unwrap_or(false);
                if last_hour || next_s {
                    out.push_str(&format!("{mi:02}"));
                } else {
                    match run {
                        1 => out.push_str(&mo.to_string()),
                        2 => out.push_str(&format!("{mo:02}")),
                        3 => out.push_str(if LOCALE_ID { MONTHS_ABBR[(mo - 1) as usize] } else { &MONTHS_FULL[(mo - 1) as usize][..3] }),
                        _ => out.push_str(MONTHS_FULL[(mo - 1) as usize]),
                    }
                }
                last_hour = false;
            }
            'd' => {
                match run {
                    1 => out.push_str(&d.to_string()),
                    2 => out.push_str(&format!("{d:02}")),
                    3 => out.push_str(DAYS_ABBR[wd]),
                    _ => out.push_str(DAYS_FULL[wd]),
                }
                last_hour = false;
            }
            'h' => {
                let h = if ampm { if hh % 12 == 0 { 12 } else { hh % 12 } } else { hh };
                out.push_str(&if run >= 2 { format!("{h:02}") } else { h.to_string() });
                last_hour = true;
            }
            's' => {
                out.push_str(&format!("{ss:02}"));
                last_hour = false;
            }
            'a' => {
                if lower[i..].starts_with("am/pm") {
                    out.push_str(if hh < 12 { "AM" } else { "PM" });
                    i += 5;
                    continue;
                } else if lower[i..].starts_with("a/p") {
                    out.push_str(if hh < 12 { "A" } else { "P" });
                    i += 3;
                    continue;
                } else {
                    out.push('a');
                }
            }
            c => {
                for _ in 0..run {
                    out.push(c);
                }
            }
        }
        i += run;
    }
    out
}

fn group(int: &str) -> String {
    let sep = if LOCALE_ID { '.' } else { ',' };
    let mut out = String::new();
    let n = int.len();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn dec_sep() -> &'static str {
    if LOCALE_ID { "," } else { "." }
}

fn general(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let int_digits = if v.abs() >= 1.0 { v.abs().log10().floor() as i32 + 1 } else { 1 };
    let dec = (10 - int_digits).clamp(0, 10) as usize;
    let s = format!("{:.*}", dec, v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    s.replace('.', dec_sep())
}

fn fmt_section(v: f64, sec: &str, neg_sign: bool) -> String {
    let ch: Vec<char> = sec.chars().collect();
    let (mut prefix, mut pat, mut suffix) = (String::new(), String::new(), String::new());
    let mut state = 0;
    let mut pct = false;
    let mut i = 0;
    let lit = |s: &str, state: &mut i32, prefix: &mut String, suffix: &mut String| {
        if *state == 0 {
            prefix.push_str(s);
        } else {
            *state = 2;
            suffix.push_str(s);
        }
    };
    while i < ch.len() {
        let c = ch[i];
        match c {
            '"' => {
                let mut j = i + 1;
                let mut s = String::new();
                while j < ch.len() && ch[j] != '"' {
                    s.push(ch[j]);
                    j += 1;
                }
                lit(&s, &mut state, &mut prefix, &mut suffix);
                i = j + 1;
                continue;
            }
            '\\' => {
                if i + 1 < ch.len() {
                    lit(&ch[i + 1].to_string(), &mut state, &mut prefix, &mut suffix);
                }
                i += 2;
                continue;
            }
            '_' => {
                lit(" ", &mut state, &mut prefix, &mut suffix);
                i += 2;
                continue;
            }
            '*' => {
                i += 2;
                continue;
            }
            '[' => {
                let mut j = i + 1;
                let mut s = String::new();
                while j < ch.len() && ch[j] != ']' {
                    s.push(ch[j]);
                    j += 1;
                }
                if let Some(rest) = s.strip_prefix('$') {
                    let cur = rest.split('-').next().unwrap_or("");
                    lit(cur, &mut state, &mut prefix, &mut suffix);
                }
                i = j + 1;
                continue;
            }
            '%' => {
                pct = true;
                lit("%", &mut state, &mut prefix, &mut suffix);
            }
            '0' | '#' | '?' => {
                if state == 2 {
                    // placeholder setelah suffix: abaikan
                } else {
                    state = 1;
                    pat.push(c);
                }
            }
            ',' | '.' if state == 1 => {
                let next_ph = ch.get(i + 1).map(|n| matches!(n, '0' | '#' | '?')).unwrap_or(false);
                if next_ph || c == '.' {
                    pat.push(c);
                }
            }
            c => lit(&c.to_string(), &mut state, &mut prefix, &mut suffix),
        }
        i += 1;
    }
    let v = if pct { v * 100.0 } else { v };
    let sign = if neg_sign { "-" } else { "" };
    if pat.is_empty() {
        return format!("{sign}{prefix}{suffix}");
    }
    let (ip, dp) = match pat.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (pat.clone(), String::new()),
    };
    let min_dec = dp.chars().filter(|c| *c == '0').count();
    let max_dec = dp.chars().filter(|c| matches!(c, '0' | '#' | '?')).count();
    let thousands = ip.contains(',');
    let min_int = ip.chars().filter(|c| *c == '0').count();
    let mut s = format!("{:.*}", max_dec, v.abs());
    let mut frac = String::new();
    if let Some((a, b)) = s.clone().split_once('.') {
        s = a.to_string();
        frac = b.to_string();
        while frac.len() > min_dec && frac.ends_with('0') {
            frac.pop();
        }
    }
    while s.len() < min_int {
        s.insert(0, '0');
    }
    let int = if thousands { group(&s) } else { s };
    let num = if frac.is_empty() { int } else { format!("{int}{}{frac}", dec_sep()) };
    format!("{sign}{prefix}{num}{suffix}")
}

fn format_value(v: f64, code: &str) -> String {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") || code == "@" {
        return general(v);
    }
    let secs = split_sections(code);
    let (sec, neg_sign) = if v < 0.0 && secs.len() > 1 {
        (secs[1].as_str(), false)
    } else if v == 0.0 && secs.len() > 2 {
        (secs[2].as_str(), false)
    } else {
        (secs[0].as_str(), v < 0.0)
    };
    if is_date_code(sec) {
        return fmt_date(v, sec);
    }
    if sec.to_lowercase().contains("general") {
        return general(v);
    }
    fmt_section(v, sec, neg_sign)
}

// ------------------------------------------------------------------------------------------------
// Gaya
// ------------------------------------------------------------------------------------------------
struct FontS {
    bold: bool,
    italic: bool,
    size: f32,
    color: Rgb,
    ul: bool,
    strike: bool,
    serif: bool,
}
struct Xf {
    fmt: u32,
    font: usize,
    fill: Option<Rgb>,
    border: [bool; 4], // kiri, kanan, atas, bawah
    h: Option<String>,
    v: Option<String>,
    wrap: bool,
}
struct Styles {
    xfs: Vec<Xf>,
    fonts: Vec<FontS>,
    fmts: HashMap<u32, String>,
    theme: Vec<Rgb>,
}

fn default_theme() -> Vec<Rgb> {
    ["FFFFFF", "000000", "E7E6E6", "44546A", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"].iter().map(|h| hex(h).unwrap()).collect()
}

fn load_theme(xml: &str) -> Vec<Rgb> {
    let Ok(d) = roxmltree::Document::parse(xml) else { return default_theme() };
    let Some(cs) = desc(d.root_element(), "clrScheme") else { return default_theme() };
    let mut m: HashMap<String, Rgb> = HashMap::new();
    for c in cs.children().filter(|c| c.is_element()) {
        let col = c.children().filter(|x| x.is_element()).find_map(|x| match name(x) {
            "srgbClr" => at(x, "val").and_then(hex),
            "sysClr" => at(x, "lastClr").and_then(hex),
            _ => None,
        });
        if let Some(col) = col {
            m.insert(name(c).to_string(), col);
        }
    }
    let order = ["lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"];
    let def = default_theme();
    order.iter().enumerate().map(|(i, k)| m.get(*k).copied().unwrap_or(def[i])).collect()
}

fn color_of(n: Node, theme: &[Rgb]) -> Option<Rgb> {
    if let Some(r) = at(n, "rgb") {
        return hex(r);
    }
    if let Some(t) = at(n, "theme").and_then(|v| v.parse::<usize>().ok()) {
        let base = *theme.get(t)?;
        return Some(match fnum(at(n, "tint")) {
            Some(ti) if ti != 0.0 => tint(base, ti),
            _ => base,
        });
    }
    if let Some(ix) = at(n, "indexed").and_then(|v| v.parse::<usize>().ok()) {
        let pal: [&str; 8] = ["000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF"];
        return pal.get(ix).and_then(|h| hex(h));
    }
    None
}

impl Styles {
    fn load(xml: &str, theme: Vec<Rgb>) -> Styles {
        let mut st = Styles { xfs: vec![], fonts: vec![], fmts: HashMap::new(), theme };
        let Ok(d) = roxmltree::Document::parse(xml) else { return st };
        let root = d.root_element();
        if let Some(nf) = kid(root, "numFmts") {
            for f in kids(nf, "numFmt") {
                if let (Some(id), Some(code)) = (at(f, "numFmtId").and_then(|v| v.parse().ok()), at(f, "formatCode")) {
                    st.fmts.insert(id, code.to_string());
                }
            }
        }
        if let Some(fs) = kid(root, "fonts") {
            for f in kids(fs, "font") {
                let serif = kid(f, "name").and_then(|n| at(n, "val")).map(|n| {
                    let l = n.to_lowercase();
                    !l.contains("sans") && ["times", "georgia", "cambria", "garamond", "serif"].iter().any(|k| l.contains(k))
                });
                st.fonts.push(FontS {
                    bold: on(f, "b").unwrap_or(false),
                    italic: on(f, "i").unwrap_or(false),
                    size: fnum(kid(f, "sz").and_then(|s| at(s, "val"))).unwrap_or(11.0),
                    color: kid(f, "color").and_then(|c| color_of(c, &st.theme)).unwrap_or([0, 0, 0]),
                    ul: on(f, "u").unwrap_or(false),
                    strike: on(f, "strike").unwrap_or(false),
                    serif: serif.unwrap_or(false),
                });
            }
        }
        let mut fills: Vec<Option<Rgb>> = vec![];
        if let Some(fs) = kid(root, "fills") {
            for f in kids(fs, "fill") {
                let c = kid(f, "patternFill").filter(|p| at(*p, "patternType") == Some("solid")).and_then(|p| kid(p, "fgColor")).and_then(|c| color_of(c, &st.theme));
                fills.push(c);
            }
        }
        let mut borders: Vec<[bool; 4]> = vec![];
        if let Some(bs) = kid(root, "borders") {
            for b in kids(bs, "border") {
                let side = |nm: &str, alt: &str| kid(b, nm).or_else(|| kid(b, alt)).and_then(|s| at(s, "style")).map(|s| s != "none").unwrap_or(false);
                borders.push([side("left", "start"), side("right", "end"), side("top", "top"), side("bottom", "bottom")]);
            }
        }
        if let Some(cx) = kid(root, "cellXfs") {
            for x in kids(cx, "xf") {
                let al = kid(x, "alignment");
                st.xfs.push(Xf {
                    fmt: at(x, "numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0),
                    font: at(x, "fontId").and_then(|v| v.parse().ok()).unwrap_or(0),
                    fill: at(x, "fillId").and_then(|v| v.parse::<usize>().ok()).and_then(|i| fills.get(i).copied().flatten()),
                    border: at(x, "borderId").and_then(|v| v.parse::<usize>().ok()).and_then(|i| borders.get(i).copied()).unwrap_or([false; 4]),
                    h: al.and_then(|a| at(a, "horizontal")).map(|s| s.to_string()),
                    v: al.and_then(|a| at(a, "vertical")).map(|s| s.to_string()),
                    wrap: al.and_then(|a| at(a, "wrapText")).map(|v| v == "1" || v == "true").unwrap_or(false),
                });
            }
        }
        if st.fonts.is_empty() {
            st.fonts.push(FontS { bold: false, italic: false, size: 11.0, color: [0, 0, 0], ul: false, strike: false, serif: false });
        }
        if st.xfs.is_empty() {
            st.xfs.push(Xf { fmt: 0, font: 0, fill: None, border: [false; 4], h: None, v: None, wrap: false });
        }
        st
    }
    fn code(&self, id: u32) -> String {
        self.fmts.get(&id).cloned().or_else(|| builtin_fmt(id).map(|s| s.to_string())).unwrap_or_default()
    }
}

// ------------------------------------------------------------------------------------------------
// Sheet
// ------------------------------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Num,
    Text,
    Bool,
}
struct Cell {
    text: String,
    xf: usize,
    kind: Kind,
}
#[derive(Default)]
struct Sheet {
    cells: BTreeMap<(u32, u32), Cell>,
    cols: HashMap<u32, (f32, bool)>,
    rows: HashMap<u32, (Option<f32>, bool)>,
    merges: Vec<(u32, u32, u32, u32)>,
    def_w: f32,
    def_h: f32,
}

fn col_num(s: &str) -> u32 {
    s.bytes().filter(|b| b.is_ascii_alphabetic()).fold(0u32, |a, b| a * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32)
}
fn cell_ref(s: &str) -> Option<(u32, u32)> {
    let row: u32 = s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    let c = col_num(s);
    if c == 0 { None } else { Some((row, c)) }
}

fn text_of(n: Node) -> String {
    n.descendants()
        .filter(|x| x.is_element() && name(*x) == "t" && !x.ancestors().any(|a| name(a) == "rPh"))
        .map(|x| x.text().unwrap_or(""))
        .collect()
}

fn parse_sheet(xml: &str, sst: &[String], st: &Styles) -> Option<Sheet> {
    let d = roxmltree::Document::parse(xml).ok()?;
    let root = d.root_element();
    let mut sh = Sheet { def_w: 48.0, def_h: 15.0, ..Default::default() };
    if let Some(f) = kid(root, "sheetFormatPr") {
        if let Some(h) = fnum(at(f, "defaultRowHeight")) {
            sh.def_h = h;
        }
        if let Some(w) = fnum(at(f, "defaultColWidth")) {
            sh.def_w = w * 5.25;
        } else if let Some(b) = fnum(at(f, "baseColWidth")) {
            sh.def_w = (b + 1.14) * 5.25;
        }
    }
    if let Some(cs) = kid(root, "cols") {
        for c in kids(cs, "col") {
            let (Some(mn), Some(mx)) = (at(c, "min").and_then(|v| v.parse::<u32>().ok()), at(c, "max").and_then(|v| v.parse::<u32>().ok())) else { continue };
            let w = fnum(at(c, "width")).map(|w| w * 5.25).unwrap_or(sh.def_w);
            let hidden = matches!(at(c, "hidden"), Some("1") | Some("true"));
            for i in mn..=mx.min(mn + 2000) {
                sh.cols.insert(i, (w, hidden));
            }
        }
    }
    if let Some(sd) = kid(root, "sheetData") {
        for (ri, r) in kids(sd, "row").into_iter().enumerate() {
            let rn = at(r, "r").and_then(|v| v.parse::<u32>().ok()).unwrap_or(ri as u32 + 1);
            let hidden = matches!(at(r, "hidden"), Some("1") | Some("true"));
            sh.rows.insert(rn, (fnum(at(r, "ht")), hidden));
            for (ci, c) in kids(r, "c").into_iter().enumerate() {
                let (row, col) = at(c, "r").and_then(cell_ref).unwrap_or((rn, ci as u32 + 1));
                let xf = at(c, "s").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(st.xfs.len() - 1);
                let v = kid(c, "v").and_then(|v| v.text());
                let (text, kind) = match at(c, "t") {
                    Some("s") => (v.and_then(|i| i.trim().parse::<usize>().ok()).and_then(|i| sst.get(i).cloned()).unwrap_or_default(), Kind::Text),
                    Some("str") | Some("e") => (v.unwrap_or("").to_string(), Kind::Text),
                    Some("inlineStr") => (kid(c, "is").map(text_of).unwrap_or_default(), Kind::Text),
                    Some("b") => ((if v == Some("1") { "TRUE" } else { "FALSE" }).to_string(), Kind::Bool),
                    _ => match v.and_then(|x| x.trim().parse::<f64>().ok()) {
                        Some(n) => (format_value(n, &st.code(st.xfs[xf].fmt)), Kind::Num),
                        None => (String::new(), Kind::Text),
                    },
                };
                if !text.is_empty() || st.xfs[xf].fill.is_some() || st.xfs[xf].border.iter().any(|b| *b) {
                    sh.cells.insert((row, col), Cell { text, xf, kind });
                }
            }
        }
    }
    if let Some(ms) = kid(root, "mergeCells") {
        for m in kids(ms, "mergeCell") {
            if let Some((a, b)) = at(m, "ref").and_then(|r| r.split_once(':')) {
                if let (Some((r1, c1)), Some((r2, c2))) = (cell_ref(a), cell_ref(b)) {
                    sh.merges.push((r1, c1, r2, c2));
                }
            }
        }
    }
    Some(sh)
}

// ------------------------------------------------------------------------------------------------
const MARGIN: f32 = 36.0;
const MIN_SCALE: f32 = 0.6;

fn render_sheet(pdf: &mut Pdf, sh: &Sheet, st: &Styles) {
    if sh.cells.is_empty() {
        return;
    }
    let (rmin, rmax) = (sh.cells.keys().map(|k| k.0).min().unwrap(), sh.cells.keys().map(|k| k.0).max().unwrap());
    let (cmin, cmax) = (sh.cells.keys().map(|k| k.1).min().unwrap(), sh.cells.keys().map(|k| k.1).max().unwrap());
    let cw = |c: u32| -> f32 {
        match sh.cols.get(&c) {
            Some((_, true)) => 0.0,
            Some((w, _)) => *w,
            None => sh.def_w,
        }
    };
    let total_w: f32 = (cmin..=cmax).map(cw).sum();
    let (pw, ph) = if total_w <= 595.3 - 2.0 * MARGIN { (595.3, 841.9) } else { (841.9, 595.3) };
    let avail = pw - 2.0 * MARGIN;

    // pita kolom
    let mut bands: Vec<(u32, u32)> = vec![];
    let (mut start, mut acc) = (cmin, 0.0f32);
    for c in cmin..=cmax {
        let w = cw(c);
        if acc + w > avail / MIN_SCALE && c > start {
            bands.push((start, c - 1));
            start = c;
            acc = 0.0;
        }
        acc += w;
    }
    bands.push((start, cmax));

    // merge
    let mut merge_at: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    let mut covered: HashSet<(u32, u32)> = HashSet::new();
    for (r1, c1, r2, c2) in &sh.merges {
        merge_at.insert((*r1, *c1), (r2 - r1 + 1, c2 - c1 + 1));
        for r in *r1..=*r2 {
            for c in *c1..=*c2 {
                if (r, c) != (*r1, *c1) {
                    covered.insert((r, c));
                }
            }
        }
    }
    let row_h = |r: u32| -> f32 {
        match sh.rows.get(&r) {
            Some((_, true)) => 0.0,
            Some((Some(h), _)) => *h,
            _ => sh.def_h,
        }
    };

    for (b0, b1) in bands {
        let bw: f32 = (b0..=b1).map(cw).sum();
        let scale = (avail / bw.max(1.0)).min(1.0);
        pdf.new_page(pw, ph);
        let mut y = MARGIN;
        let bottom = ph - MARGIN;
        for r in rmin..=rmax {
            if pdf.page_count() > 400 {
                return;
            }
            let base = row_h(r) * scale;
            if base <= 0.0 {
                continue;
            }
            // tinggi baris (wrap text bisa menambah)
            let mut rh = base;
            for ((_, c), cell) in sh.cells.range((r, b0)..=(r, b1)) {
                let xf = &st.xfs[cell.xf];
                if xf.wrap && !cell.text.is_empty() && !covered.contains(&(r, *c)) && !merge_at.contains_key(&(r, *c)) {
                    let f = &st.fonts[xf.font.min(st.fonts.len() - 1)];
                    let face = pdf.fonts.slot(f.serif, f.bold, f.italic);
                    let mut p = Para::new(face, f.size * scale);
                    p.runs = vec![Run { text: cell.text.clone(), face, size: f.size * scale, color: f.color, ul: false, st: false }];
                    let lines = layout(&pdf.fonts, &p, (cw(*c) * scale - 4.0).max(8.0));
                    rh = rh.max(lines.iter().map(|l| l.h).sum::<f32>() + 3.0);
                }
            }
            if y + rh > bottom + 0.5 && y > MARGIN + 0.1 {
                pdf.new_page(pw, ph);
                y = MARGIN;
            }
            // x awal tiap kolom
            let mut xs: HashMap<u32, f32> = HashMap::new();
            let mut x = MARGIN;
            for c in b0..=b1 {
                xs.insert(c, x);
                x += cw(c) * scale;
            }
            let cells: Vec<(&(u32, u32), &Cell)> = sh.cells.range((r, b0)..=(r, b1)).collect();
            // 1) isi warna
            for ((_, c), cell) in &cells {
                if covered.contains(&(r, *c)) {
                    continue;
                }
                let xf = &st.xfs[cell.xf];
                if let Some(f) = xf.fill {
                    let (w, h) = span_size(r, *c, b1, &merge_at, &cw, &row_h, scale, rh);
                    pdf.fill_rect(xs[c], y, w, h, f);
                }
            }
            // 2) teks
            for ((_, c), cell) in &cells {
                if cell.text.is_empty() || covered.contains(&(r, *c)) {
                    continue;
                }
                let xf = &st.xfs[cell.xf];
                let f = &st.fonts[xf.font.min(st.fonts.len() - 1)];
                let face = pdf.fonts.slot(f.serif, f.bold, f.italic);
                let size = f.size * scale;
                let (w, h) = span_size(r, *c, b1, &merge_at, &cw, &row_h, scale, rh);
                let cx = xs[c];
                let align = match xf.h.as_deref() {
                    Some("left") | Some("justify") | Some("fill") => Align::Left,
                    Some("center") | Some("centerContinuous") => Align::Center,
                    Some("right") => Align::Right,
                    _ => match cell.kind {
                        Kind::Num => Align::Right,
                        Kind::Bool => Align::Center,
                        Kind::Text => Align::Left,
                    },
                };
                let pad = 2.0 * scale.max(0.6);
                let mut p = Para::new(face, size);
                let txt = if xf.wrap { cell.text.clone() } else { cell.text.replace('\n', " ") };
                p.runs = vec![Run { text: txt, face, size, color: f.color, ul: f.ul, st: f.strike }];
                if xf.wrap {
                    let lines = layout(&pdf.fonts, &p, (w - 2.0 * pad).max(8.0));
                    let total: f32 = lines.iter().map(|l| l.h).sum();
                    let mut ly = match xf.v.as_deref() {
                        Some("top") => y + 1.0,
                        Some("center") => y + (h - total) / 2.0,
                        _ => y + h - total - 1.0,
                    };
                    pdf.clip_rect(cx, y, w, h);
                    for l in &lines {
                        draw_line(pdf, l, align, cx + pad, ly);
                        ly += l.h;
                    }
                    pdf.restore();
                } else {
                    let lines = layout(&pdf.fonts, &p, 100000.0);
                    let Some(l) = lines.first() else { continue };
                    // teks rata kiri boleh meluber ke sel kosong di kanannya
                    let mut clip_w = w;
                    if align == Align::Left && !merge_at.contains_key(&(r, *c)) {
                        let mut cc = c + 1;
                        while cc <= b1 && clip_w < l.w + pad && !sh.cells.get(&(r, cc)).map(|x| !x.text.is_empty()).unwrap_or(false) && !covered.contains(&(r, cc)) {
                            clip_w += cw(cc) * scale;
                            cc += 1;
                        }
                    }
                    let tx = match align {
                        Align::Right => cx + w - pad - l.w,
                        Align::Center => cx + (w - l.w) / 2.0,
                        _ => cx + pad,
                    };
                    let ly = match xf.v.as_deref() {
                        Some("top") => y + 1.0,
                        Some("center") => y + (h - l.h) / 2.0,
                        _ => y + h - l.h - 1.0,
                    };
                    pdf.clip_rect(cx, y, clip_w, h);
                    draw_line(pdf, l, Align::Left, tx, ly);
                    pdf.restore();
                }
            }
            // 3) border
            for ((_, c), cell) in &cells {
                if covered.contains(&(r, *c)) {
                    continue;
                }
                let xf = &st.xfs[cell.xf];
                if !xf.border.iter().any(|b| *b) {
                    continue;
                }
                let (w, h) = span_size(r, *c, b1, &merge_at, &cw, &row_h, scale, rh);
                let (x0, y0) = (xs[c], y);
                let k: Rgb = [0, 0, 0];
                if xf.border[0] { pdf.line(x0, y0, x0, y0 + h, 0.5, k); }
                if xf.border[1] { pdf.line(x0 + w, y0, x0 + w, y0 + h, 0.5, k); }
                if xf.border[2] { pdf.line(x0, y0, x0 + w, y0, 0.5, k); }
                if xf.border[3] { pdf.line(x0, y0 + h, x0 + w, y0 + h, 0.5, k); }
            }
            y += rh;
        }
    }
}

fn span_size(r: u32, c: u32, b1: u32, merge_at: &HashMap<(u32, u32), (u32, u32)>, cw: &dyn Fn(u32) -> f32, row_h: &dyn Fn(u32) -> f32, scale: f32, rh: f32) -> (f32, f32) {
    match merge_at.get(&(r, c)) {
        Some((rs, cs)) => {
            let w: f32 = (c..(c + cs).min(b1 + 1)).map(|k| cw(k)).sum::<f32>() * scale;
            let h: f32 = (r..r + rs).map(|k| row_h(k) * scale).sum::<f32>().max(rh);
            (w, h)
        }
        None => (cw(c) * scale, rh),
    }
}

pub fn convert(src: &[u8], mut pdf: Pdf) -> Result<Vec<u8>, String> {
    let mut z = open_zip(src)?;
    let wb_path = "xl/workbook.xml";
    let wb_xml = read_text(&mut z, wb_path).ok_or("xl/workbook.xml tidak ditemukan")?;
    let rels = rels_of(&mut z, wb_path);
    let theme = read_text(&mut z, "xl/theme/theme1.xml").map(|t| load_theme(&t)).unwrap_or_else(default_theme);
    let st = Styles::load(&read_text(&mut z, "xl/styles.xml").unwrap_or_default(), theme);
    let sst: Vec<String> = match read_text(&mut z, "xl/sharedStrings.xml") {
        Some(x) => match roxmltree::Document::parse(&x) {
            Ok(d) => d.root_element().children().filter(|n| n.is_element() && name(*n) == "si").map(text_of).collect(),
            Err(_) => vec![],
        },
        None => vec![],
    };
    let wb = parse(&wb_xml)?;
    let sheets: Vec<(String, bool)> = desc(wb.root_element(), "sheets")
        .map(|s| {
            kids(s, "sheet")
                .into_iter()
                .filter(|sh| !matches!(at(*sh, "state"), Some("hidden") | Some("veryHidden")))
                .filter_map(|sh| Some((rels.get(at(sh, "id")?)?.target.clone(), true)))
                .collect()
        })
        .unwrap_or_default();
    for (path, _) in sheets {
        let Some(xml) = read_text(&mut z, &path) else { continue };
        if let Some(sh) = parse_sheet(&xml, &sst, &st) {
            render_sheet(&mut pdf, &sh, &st);
        }
    }
    pdf.finish()
}
