//! Utilitas bersama: ZIP/OPC, XML (roxmltree), relasi, warna.
use roxmltree::Node;
use std::collections::HashMap;
use std::io::{Cursor, Read};

pub type Rgb = [u8; 3];
pub type Zip = zip::ZipArchive<Cursor<Vec<u8>>>;

pub fn open_zip(src: &[u8]) -> Result<Zip, String> {
    zip::ZipArchive::new(Cursor::new(src.to_vec())).map_err(|e| format!("Berkas Office tidak valid: {e}"))
}

pub fn read_bytes(z: &mut Zip, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    if f.size() > 256 * 1024 * 1024 {
        return None; // pagar zip-bomb
    }
    let mut v = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

pub fn read_text(z: &mut Zip, name: &str) -> Option<String> {
    let b = read_bytes(z, name)?;
    let s = String::from_utf8_lossy(&b).into_owned();
    Some(s.trim_start_matches('\u{feff}').to_string())
}

pub fn parse(s: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(s).map_err(|e| format!("XML rusak: {e}"))
}

pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

/// Gabungkan direktori dasar dengan target relasi (menangani `..` dan `/` absolut).
pub fn resolve(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        vec![]
    } else {
        base_dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[derive(Clone)]
pub struct Rel {
    pub target: String,
    pub ty: String,
}

/// Relasi sebuah part: `word/document.xml` -> `word/_rels/document.xml.rels`.
pub fn rels_of(z: &mut Zip, part: &str) -> HashMap<String, Rel> {
    let dir = dir_of(part);
    let file = part[dir.len()..].trim_start_matches('/');
    let path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
    let mut m = HashMap::new();
    if let Some(t) = read_text(z, &path) {
        if let Ok(d) = roxmltree::Document::parse(&t) {
            for n in d.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Relationship") {
                if n.attribute("TargetMode") == Some("External") {
                    continue;
                }
                if let (Some(id), Some(tg)) = (n.attribute("Id"), n.attribute("Target")) {
                    m.insert(id.to_string(), Rel { target: resolve(dir, tg), ty: n.attribute("Type").unwrap_or("").to_string() });
                }
            }
        }
    }
    m
}

// ---------------- helper XML (cocokkan nama lokal, abaikan namespace) ----------------
pub fn name<'a>(n: Node<'a, '_>) -> &'a str {
    n.tag_name().name()
}
pub fn at<'a>(n: Node<'a, '_>, key: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == key).map(|a| a.value())
}
pub fn kid<'a, 'i>(n: Node<'a, 'i>, nm: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && name(*c) == nm)
}
pub fn kids<'a, 'i>(n: Node<'a, 'i>, nm: &str) -> Vec<Node<'a, 'i>> {
    n.children().filter(|c| c.is_element() && name(*c) == nm).collect()
}
pub fn desc<'a, 'i>(n: Node<'a, 'i>, nm: &str) -> Option<Node<'a, 'i>> {
    n.descendants().find(|c| c.is_element() && name(*c) == nm)
}
pub fn fnum(s: Option<&str>) -> Option<f32> {
    s.and_then(|v| v.trim().parse::<f32>().ok())
}
/// Atribut boolean OOXML: ada tanpa val => true; "0"/"false"/"off" => false.
pub fn on(n: Node, nm: &str) -> Option<bool> {
    let c = kid(n, nm)?;
    Some(!matches!(at(c, "val"), Some("0") | Some("false") | Some("off") | Some("none")))
}

// ---------------- warna ----------------
pub fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let s = &s[s.len() - 6..];
    Some([u8::from_str_radix(&s[0..2], 16).ok()?, u8::from_str_radix(&s[2..4], 16).ok()?, u8::from_str_radix(&s[4..6], 16).ok()?])
}

pub fn to_hsl(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    if (mx - mn).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

pub fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
    if s <= 1e-6 {
        let v = (l.clamp(0.0, 1.0) * 255.0).round() as u8;
        return [v, v, v];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

pub fn lum(c: Rgb, l: f32) -> Rgb {
    let (h, s, _) = to_hsl(c);
    from_hsl(h, s, l.clamp(0.0, 1.0))
}

/// Tint gaya Excel (-1..1) pada luminans HSL.
pub fn tint(c: Rgb, t: f32) -> Rgb {
    let (h, s, l) = to_hsl(c);
    let nl = if t < 0.0 { l * (1.0 + t) } else { l * (1.0 - t) + t };
    from_hsl(h, s, nl)
}

/// Atribut `r:id` (namespace relationships), bukan `id` biasa.
pub fn rid<'a>(n: Node<'a, '_>) -> Option<&'a str> {
    n.attributes()
        .find(|a| a.name() == "id" && a.namespace().map(|ns| ns.contains("relationships")).unwrap_or(false))
        .map(|a| a.value())
}
