//! Font TrueType: pencarian (resource aplikasi -> font sistem), metrik, dan penandaan glyph terpakai
//! (untuk subsetting saat PDF ditulis). Tidak butuh font terpasang khusus: Liberation/DejaVu/Noto
//! di Linux, Arial/Times/Segoe/Calibri di Windows, atau berkas .ttf yang dibundel di resources/fonts.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Face {
    pub data: Vec<u8>,
    pub upem: f32,
    cmap: HashMap<u32, u16>,
    adv: Vec<u16>,
    pub asc: f32,
    pub desc: f32,
    pub gap: f32,
    pub bbox: [i16; 4],
    pub cap: i16,
    pub italic_angle: f32,
    pub ps_name: String,
    /// CID PDF -> (glyph font asli, karakter Unicode sumber).
    /// CID unik per karakter, bukan per glyph, agar karakter alias tetap bisa diekstrak dengan benar.
    pub used: BTreeMap<u16, (u16, char)>,
    used_chars: HashMap<char, u16>,
}

impl Face {
    fn load(path: &Path) -> Option<Face> {
        const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
        if std::fs::metadata(path).ok()?.len() > MAX_FONT_BYTES {
            return None;
        }
        let data = std::fs::read(path).ok()?;
        let (upem, cmap, adv, asc, desc, gap, bbox, cap, ia, ps) = {
            let f = ttf_parser::Face::parse(&data, 0).ok()?;
            f.tables().glyf?; // hanya outline TrueType (FontFile2)
            let mut cmap = HashMap::new();
            if let Some(c) = f.tables().cmap {
                for st in c.subtables {
                    if !st.is_unicode() {
                        continue;
                    }
                    st.codepoints(|cp| {
                        if let Some(g) = st.glyph_index(cp) {
                            cmap.entry(cp).or_insert(g.0);
                        }
                    });
                }
            }
            if cmap.is_empty() {
                return None;
            }
            let adv: Vec<u16> =
                (0..f.number_of_glyphs()).map(|g| f.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap_or(0)).collect();
            let bb = f.global_bounding_box();
            let ps = f
                .names()
                .into_iter()
                .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && n.is_unicode())
                .and_then(|n| n.to_string())
                .unwrap_or_else(|| "Font".into());
            (
                f.units_per_em() as f32,
                cmap,
                adv,
                f.ascender() as f32,
                f.descender() as f32,
                f.line_gap() as f32,
                [bb.x_min, bb.y_min, bb.x_max, bb.y_max],
                f.capital_height().unwrap_or(f.ascender()),
                f.italic_angle(),
                ps,
            )
        };
        let ps_name: String = ps.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        Some(Face {
            data,
            upem,
            cmap,
            adv,
            asc,
            desc,
            gap,
            bbox,
            cap,
            italic_angle: ia,
            ps_name: if ps_name.is_empty() { "Font".into() } else { ps_name },
            used: BTreeMap::new(),
            used_chars: HashMap::new(),
        })
    }
}

pub struct Fonts {
    pub faces: Vec<Face>,
    /// [serif?][bold | italic<<1] -> indeks face
    slots: [[usize; 4]; 2],
}

const SANS: &[[&str; 4]] = &[
    ["LiberationSans-Regular.ttf", "LiberationSans-Bold.ttf", "LiberationSans-Italic.ttf", "LiberationSans-BoldItalic.ttf"],
    ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
    ["DejaVuSans.ttf", "DejaVuSans-Bold.ttf", "DejaVuSans-Oblique.ttf", "DejaVuSans-BoldOblique.ttf"],
    ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf", "NotoSans-Italic.ttf", "NotoSans-BoldItalic.ttf"],
    ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"],
    ["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"],
];
const SERIF: &[[&str; 4]] = &[
    ["LiberationSerif-Regular.ttf", "LiberationSerif-Bold.ttf", "LiberationSerif-Italic.ttf", "LiberationSerif-BoldItalic.ttf"],
    ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
    ["DejaVuSerif.ttf", "DejaVuSerif-Bold.ttf", "DejaVuSerif-Italic.ttf", "DejaVuSerif-BoldItalic.ttf"],
    ["NotoSerif-Regular.ttf", "NotoSerif-Bold.ttf", "NotoSerif-Italic.ttf", "NotoSerif-BoldItalic.ttf"],
];

// Optional script/symbol faces are loaded after the primary families. These are
// TrueType-outline fonts so they can be embedded with the existing FontFile2 path.
const FALLBACK_FONTS: &[&str] = &[
    "NotoSansSymbols2-Regular.ttf",
    "NotoSansArabic-Regular.ttf",
    "NotoSansHebrew-Regular.ttf",
    "NotoSansDevanagari-Regular.ttf",
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJKjp-Regular.ttf",
    "arialuni.ttf",
    "seguisym.ttf",
    "msyh.ttc",
    "simsun.ttc",
    "simhei.ttf",
    "meiryo.ttc",
    "msgothic.ttc",
    "malgun.ttf",
    "gulim.ttc",
];

fn font_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Some(rd) = super::resource_dir() {
        v.push(rd.join("fonts"));
        v.push(rd.clone());
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(dir.join("fonts"));
        v.push(dir.join("resources").join("fonts"));
    }
    if let Some(p) = std::env::var_os("OFFICEPDF_FONTS") {
        v.insert(0, PathBuf::from(p));
    }
    if cfg!(target_os = "windows") {
        let win = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        v.push(win.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else {
        for d in [
            "/usr/share/fonts/truetype/liberation2",
            "/usr/share/fonts/truetype/liberation",
            "/usr/share/fonts/liberation-sans",
            "/usr/share/fonts/liberation-serif",
            "/usr/share/fonts/liberation",
            "/usr/share/fonts/TTF",
            "/usr/share/fonts/truetype/dejavu",
            "/usr/share/fonts/dejavu",
            "/usr/share/fonts/dejavu-sans-fonts",
            "/usr/share/fonts/dejavu-serif-fonts",
            "/usr/share/fonts/truetype/noto",
            "/usr/share/fonts/opentype/noto",
            "/usr/share/fonts/noto",
            "/usr/share/fonts/google-noto",
            "/usr/share/fonts/truetype/msttcorefonts",
        ] {
            v.push(PathBuf::from(d));
        }
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(&h).join(".local/share/fonts"));
            v.push(PathBuf::from(&h).join(".fonts"));
        }
    }
    v
}

fn find_family(faces: &mut Vec<Face>, sets: &[[&str; 4]], dirs: &[PathBuf]) -> Option<[usize; 4]> {
    for set in sets {
        for d in dirs {
            let Some(reg) = Face::load(&d.join(set[0])) else { continue };
            let base = faces.len();
            faces.push(reg);
            let mut slots = [base; 4];
            for (i, file) in set.iter().enumerate().skip(1) {
                if let Some(f) = Face::load(&d.join(file)) {
                    slots[i] = faces.len();
                    faces.push(f);
                }
            }
            // gaya yang tidak ada: pakai yang terdekat (bold-italic -> bold/italic -> regular)
            if slots[3] == base {
                slots[3] = if slots[1] != base { slots[1] } else { slots[2] };
            }
            return Some(slots);
        }
    }
    None
}

impl Fonts {
    pub fn load() -> Result<Fonts, String> {
        let dirs = font_dirs();
        let mut faces = Vec::new();
        let sans = find_family(&mut faces, SANS, &dirs).ok_or_else(|| {
            "Tidak ada font TrueType yang bisa dipakai. Letakkan LiberationSans-*.ttf di resources/fonts \
             atau pasang font Liberation/DejaVu (Linux) / Arial (Windows)."
                .to_string()
        })?;
        let serif = find_family(&mut faces, SERIF, &dirs).unwrap_or(sans);

        // Load optional Unicode/script fonts without making them mandatory for startup.
        // One file is loaded only once even if several system/resource directories overlap.
        const MAX_OPTIONAL_FALLBACK_FACES: usize = 8;
        let mut loaded = HashSet::new();
        let mut fallback_count = 0usize;
        'directories: for dir in &dirs {
            for file in FALLBACK_FONTS {
                if fallback_count >= MAX_OPTIONAL_FALLBACK_FACES {
                    break 'directories;
                }
                let path = dir.join(file);
                if !loaded.insert(path.clone()) {
                    continue;
                }
                if let Some(face) = Face::load(&path) {
                    faces.push(face);
                    fallback_count += 1;
                }
            }
        }
        Ok(Fonts { faces, slots: [sans, serif] })
    }

    pub fn slot(&self, serif: bool, bold: bool, italic: bool) -> usize {
        self.slots[serif as usize][(bold as usize) | ((italic as usize) << 1)]
    }

    /// Find a loaded face that contains the requested Unicode character.
    /// Keep the preferred face whenever possible so ordinary text retains its style.
    pub fn fallback_face(&self, preferred: usize, ch: char) -> usize {
        let Some(mapped) = Self::map_char(ch) else { return preferred };
        if self.faces.get(preferred).map(|face| face.cmap.contains_key(&(mapped as u32))).unwrap_or(false) {
            return preferred;
        }
        self.faces.iter().position(|face| face.cmap.contains_key(&(mapped as u32))).unwrap_or(preferred)
    }

    /// Tinggi baris (dalam em) satu face.
    pub fn line_h(&self, f: usize) -> f32 {
        let fc = &self.faces[f];
        (fc.asc - fc.desc + fc.gap.max(0.0)) / fc.upem
    }
    pub fn ascent(&self, f: usize) -> f32 {
        let fc = &self.faces[f];
        fc.asc / fc.upem
    }

    fn map_char(c: char) -> Option<char> {
        match c {
            '\u{a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{3000}' => Some(' '),
            '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}' | '\u{ad}' | '\u{202a}'..='\u{202e}' => None,
            '\u{2011}' => Some('-'),
            c if (c as u32) < 32 => None,
            c => Some(c),
        }
    }

    fn gid(&self, f: usize, c: char) -> Option<(u16, char)> {
        let c = Self::map_char(c)?;
        let fc = self.faces.get(f)?;
        if let Some(g) = fc.cmap.get(&(c as u32)) {
            return Some((*g, c));
        }
        // Use a visible fallback glyph but retain the original Unicode scalar in ToUnicode.
        // This keeps copied/extracted text faithful even when the selected face lacks a glyph.
        for fallback in ['\u{fffd}', '?'] {
            if let Some(g) = fc.cmap.get(&(fallback as u32)) {
                return Some((*g, c));
            }
        }
        None
    }

    pub fn char_w(&self, f: usize, c: char, size: f32) -> f32 {
        match self.gid(f, c) {
            Some((g, _)) => {
                let fc = &self.faces[f];
                fc.adv.get(g as usize).copied().unwrap_or(0) as f32 * size / fc.upem
            }
            None => 0.0,
        }
    }

    pub fn text_w(&self, f: usize, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.char_w(f, c, size)).sum()
    }

    /// Encode text as sequential PDF CIDs. A CID identifies a Unicode character,
    /// while CIDToGIDMap independently selects the actual glyph in the embedded font.
    pub fn encode(&mut self, f: usize, s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 4 + 2);
        out.push('<');
        for c in s.chars() {
            if let Some((g, ch)) = self.gid(f, c) {
                let face = &mut self.faces[f];
                let cid = if let Some(cid) = face.used_chars.get(&ch) {
                    *cid
                } else {
                    let next = face.used.len().saturating_add(1);
                    if next > u16::MAX as usize {
                        continue;
                    }
                    let cid = next as u16;
                    face.used.insert(cid, (g, ch));
                    face.used_chars.insert(ch, cid);
                    cid
                };
                out.push_str(&format!("{cid:04X}"));
            }
        }
        out.push('>');
        out
    }
}

impl Face {
    pub fn advance(&self, g: u16) -> u16 {
        self.adv.get(g as usize).copied().unwrap_or(0)
    }
}


#[cfg(test)]
mod tests {
    use super::{Face, Fonts};
    use std::collections::{BTreeMap, HashMap};

    fn fonts(cmap: &[(u32, u16)]) -> Fonts {
        let mut map = HashMap::new();
        for (ch, gid) in cmap {
            map.insert(*ch, *gid);
        }
        Fonts {
            faces: vec![Face {
                data: Vec::new(),
                upem: 1000.0,
                cmap: map,
                adv: vec![0, 500, 600],
                asc: 800.0,
                desc: -200.0,
                gap: 0.0,
                bbox: [0, -200, 1000, 800],
                cap: 700,
                italic_angle: 0.0,
                ps_name: "Test".into(),
                used: BTreeMap::new(),
                used_chars: HashMap::new(),
            }],
            slots: [[0; 4]; 2],
        }
    }

    #[test]
    fn unsupported_character_uses_unicode_replacement_glyph_when_available() {
        let fonts = fonts(&[('\u{fffd}' as u32, 1), ('?' as u32, 2)]);
        assert_eq!(fonts.gid(0, '漢'), Some((1, '漢')));
    }

    #[test]
    fn unsupported_character_falls_back_to_question_mark() {
        let fonts = fonts(&[('?' as u32, 2)]);
        assert_eq!(fonts.gid(0, '漢'), Some((2, '漢')));
    }

    #[test]
    fn unsupported_character_is_skipped_instead_of_using_notdef() {
        let fonts = fonts(&[('A' as u32, 1)]);
        assert_eq!(fonts.gid(0, '漢'), None);
        assert_eq!(fonts.text_w(0, "漢", 12.0), 0.0);
    }

    #[test]
    fn different_unicode_characters_get_distinct_cids_for_same_glyph() {
        let mut fonts = fonts(&[('A' as u32, 1), ('Å' as u32, 1)]);
        assert_eq!(fonts.encode(0, "AÅA"), "<000100020001>");
        assert_eq!(fonts.faces[0].used.get(&1), Some(&(1, 'A')));
        assert_eq!(fonts.faces[0].used.get(&2), Some(&(1, 'Å')));
    }

    #[test]
    fn unsupported_character_keeps_original_unicode_mapping() {
        let mut fonts = fonts(&[('?' as u32, 2)]);
        assert_eq!(fonts.encode(0, "漢"), "<0001>");
        assert_eq!(fonts.faces[0].used.get(&1), Some(&(2, '漢')));
    }

    #[test]
    fn fallback_face_prefers_original_face_when_glyph_exists() {
        let mut fonts = fonts(&[('A' as u32, 1)]);
        let mut second = fonts.faces[0].cmap.clone();
        second.insert('A' as u32, 2);
        fonts.faces.push(Face {
            data: Vec::new(), upem: 1000.0, cmap: second, adv: vec![0, 500, 600],
            asc: 800.0, desc: -200.0, gap: 0.0, bbox: [0, -200, 1000, 800],
            cap: 700, italic_angle: 0.0, ps_name: "Fallback".into(),
            used: BTreeMap::new(), used_chars: HashMap::new(),
        });
        assert_eq!(fonts.fallback_face(0, 'A'), 0);
    }

    #[test]
    fn fallback_face_selects_another_loaded_face_for_missing_glyph() {
        let mut fonts = fonts(&[('A' as u32, 1)]);
        let mut second = HashMap::new();
        second.insert('漢' as u32, 1);
        fonts.faces.push(Face {
            data: Vec::new(), upem: 1000.0, cmap: second, adv: vec![0, 500],
            asc: 800.0, desc: -200.0, gap: 0.0, bbox: [0, -200, 1000, 800],
            cap: 700, italic_angle: 0.0, ps_name: "CJKFallback".into(),
            used: BTreeMap::new(), used_chars: HashMap::new(),
        });
        assert_eq!(fonts.fallback_face(0, '漢'), 1);
    }

    #[test]
    fn invalid_face_index_is_rejected() {
        let fonts = fonts(&[('A' as u32, 1)]);
        assert_eq!(fonts.gid(9, 'A'), None);
    }
}
