//! Font TrueType: pencarian (resource aplikasi -> font sistem), metrik, dan penandaan glyph terpakai
//! (untuk subsetting saat PDF ditulis). Tidak butuh font terpasang khusus: Liberation/DejaVu/Noto
//! di Linux, Arial/Times/Segoe/Calibri di Windows, atau berkas .ttf yang dibundel di resources/fonts.
use std::collections::{BTreeMap, HashMap};
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
    /// glyph yang benar-benar dipakai -> karakter Unicode-nya (untuk ToUnicode)
    pub used: BTreeMap<u16, char>,
}

impl Face {
    fn load(path: &Path) -> Option<Face> {
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
        Ok(Fonts { faces, slots: [sans, serif] })
    }

    pub fn slot(&self, serif: bool, bold: bool, italic: bool) -> usize {
        self.slots[serif as usize][(bold as usize) | ((italic as usize) << 1)]
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
        let fc = &self.faces[f];
        if let Some(g) = fc.cmap.get(&(c as u32)) {
            return Some((*g, c));
        }
        if let Some(g) = fc.cmap.get(&('?' as u32)) {
            return Some((*g, '?'));
        }
        Some((0, c))
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

    /// Ubah teks menjadi string heksadesimal glyph (Identity-H) dan catat glyph yang dipakai.
    pub fn encode(&mut self, f: usize, s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 4 + 2);
        out.push('<');
        for c in s.chars() {
            if let Some((g, ch)) = self.gid(f, c) {
                self.faces[f].used.entry(g).or_insert(ch);
                out.push_str(&format!("{g:04X}"));
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
