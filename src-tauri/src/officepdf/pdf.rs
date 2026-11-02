//! Penulis PDF minimal di atas lopdf: halaman, teks (font TrueType ter-subset, Identity-H), gambar JPEG,
//! bentuk dasar. Koordinat API: titik (pt), sumbu Y dari ATAS halaman (diubah ke sistem PDF di sini).
use super::font::Fonts;
use super::util::Rgb;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

fn n(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" { "0".into() } else { s.to_string() }
}
fn col(c: Rgb) -> String {
    format!("{:.3} {:.3} {:.3}", c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

struct PageData {
    w: f32,
    h: f32,
    ops: String,
    imgs: BTreeSet<usize>,
}

struct ImgData {
    w: u32,
    h: u32,
    jpeg: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct ImgInfo {
    pub idx: usize,
    pub w: u32,
    pub h: u32,
}

pub struct Pdf {
    pub fonts: Fonts,
    pages: Vec<PageData>,
    imgs: Vec<ImgData>,
    cache: HashMap<String, Option<ImgInfo>>,
}

impl Pdf {
    pub fn new(fonts: Fonts) -> Pdf {
        Pdf { fonts, pages: vec![], imgs: vec![], cache: HashMap::new() }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn new_page(&mut self, w: f32, h: f32) {
        self.pages.push(PageData { w, h, ops: String::new(), imgs: BTreeSet::new() });
    }

    fn page(&mut self) -> &mut PageData {
        if self.pages.is_empty() {
            self.new_page(595.3, 841.9);
        }
        self.pages.last_mut().unwrap()
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgb) {
        let p = self.page();
        let _ = writeln!(p.ops, "{} rg {} {} {} {} re f", col(c), n(x), n(p.h - y - h), n(w), n(h));
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, lw: f32, c: Rgb) {
        let p = self.page();
        let _ = writeln!(p.ops, "{} RG {} w {} {} {} {} re S", col(c), n(lw), n(x), n(p.h - y - h), n(w), n(h));
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, lw: f32, c: Rgb) {
        let p = self.page();
        let _ = writeln!(p.ops, "{} RG {} w {} {} m {} {} l S", col(c), n(lw), n(x1), n(p.h - y1), n(x2), n(p.h - y2));
    }

    pub fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let p = self.page();
        let _ = writeln!(p.ops, "q {} {} {} {} re W n", n(x), n(p.h - y - h), n(w), n(h));
    }
    pub fn restore(&mut self) {
        self.page().ops.push_str("Q\n");
    }

    /// Path rounded-rect (r=0 -> persegi biasa) atau elips; lalu isi/garis.
    pub fn shape(&mut self, kind: Shape, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgb>, stroke: Option<(Rgb, f32)>) {
        if fill.is_none() && stroke.is_none() {
            return;
        }
        let p = self.page();
        let top = p.h - y; // y PDF sisi atas
        let bot = top - h;
        let mut s = String::new();
        const K: f32 = 0.5523;
        match kind {
            Shape::Rect => {
                let _ = write!(s, "{} {} {} {} re ", n(x), n(bot), n(w), n(h));
            }
            Shape::Round(r) => {
                let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
                let k = r * K;
                let (x2, t) = (x + w, top);
                let _ = write!(
                    s,
                    "{} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c h ",
                    n(x + r), n(t), n(x2 - r), n(t),
                    n(x2 - r + k), n(t), n(x2), n(t - r + k), n(x2), n(t - r),
                    n(x2), n(bot + r),
                    n(x2), n(bot + r - k), n(x2 - r + k), n(bot), n(x2 - r), n(bot),
                    n(x + r), n(bot),
                    n(x + r - k), n(bot), n(x), n(bot + r - k), n(x), n(bot + r),
                    n(x), n(t - r),
                    n(x), n(t - r + k), n(x + r - k), n(t), n(x + r), n(t),
                );
            }
            Shape::Ellipse => {
                let (cx, cy, rx, ry) = (x + w / 2.0, bot + h / 2.0, w / 2.0, h / 2.0);
                let (kx, ky) = (rx * K, ry * K);
                let _ = write!(
                    s,
                    "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h ",
                    n(cx + rx), n(cy),
                    n(cx + rx), n(cy + ky), n(cx + kx), n(cy + ry), n(cx), n(cy + ry),
                    n(cx - kx), n(cy + ry), n(cx - rx), n(cy + ky), n(cx - rx), n(cy),
                    n(cx - rx), n(cy - ky), n(cx - kx), n(cy - ry), n(cx), n(cy - ry),
                    n(cx + kx), n(cy - ry), n(cx + rx), n(cy - ky), n(cx + rx), n(cy),
                );
            }
        }
        let mut ops = String::new();
        if let Some(f) = fill {
            let _ = write!(ops, "{} rg ", col(f));
        }
        if let Some((c, lw)) = stroke {
            let _ = write!(ops, "{} RG {} w ", col(c), n(lw));
        }
        let paint = match (fill.is_some(), stroke.is_some()) {
            (true, true) => "B",
            (true, false) => "f",
            _ => "S",
        };
        let _ = writeln!(p.ops, "{}{}{}", ops, s, paint);
    }

    /// Satu baris teks; `y` adalah garis dasar (dari atas halaman).
    pub fn text(&mut self, x: f32, y: f32, face: usize, size: f32, c: Rgb, s: &str) {
        if s.is_empty() || size <= 0.0 {
            return;
        }
        let hexs = self.fonts.encode(face, s);
        if hexs == "<>" {
            return;
        }
        let p = self.page();
        let _ = writeln!(p.ops, "BT /F{} {} Tf {} rg 1 0 0 1 {} {} Tm {} Tj ET", face, n(size), col(c), n(x), n(p.h - y), hexs);
    }

    pub fn image(&mut self, idx: usize, x: f32, y: f32, w: f32, h: f32) {
        let p = self.page();
        p.imgs.insert(idx);
        let _ = writeln!(p.ops, "q {} 0 0 {} {} {} cm /Im{} Do Q", n(w), n(h), n(x), n(p.h - y - h), idx);
    }

    /// Decode gambar (apa pun yang didukung crate `image`), ratakan alpha di atas putih, kecilkan bila
    /// terlalu besar, simpan sebagai JPEG. Hasil di-cache menurut `key`.
    pub fn add_image(&mut self, key: &str, bytes: &[u8]) -> Option<ImgInfo> {
        if let Some(v) = self.cache.get(key) {
            return *v;
        }
        let r = self.build_image(bytes);
        self.cache.insert(key.to_string(), r);
        r
    }

    fn build_image(&mut self, bytes: &[u8]) -> Option<ImgInfo> {
        let img = crate::compress::decode_image(bytes, true).ok()?;
        let mut rgba = img.to_rgba8();
        for px in rgba.pixels_mut() {
            let a = px[3] as u32;
            if a < 255 {
                for i in 0..3 {
                    px[i] = ((px[i] as u32 * a + 255 * (255 - a)) / 255) as u8;
                }
                px[3] = 255;
            }
        }
        let mut dynimg = image::DynamicImage::ImageRgba8(rgba);
        let (w, h) = (dynimg.width(), dynimg.height());
        const MAXD: u32 = 1800;
        if w.max(h) > MAXD {
            let s = MAXD as f32 / w.max(h) as f32;
            dynimg = dynimg.resize_exact(((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1), image::imageops::FilterType::Triangle);
        }
        let jpeg = crate::compress::encode_jpeg(&dynimg, 80).ok()?;
        let info = ImgInfo { idx: self.imgs.len(), w, h };
        self.imgs.push(ImgData { w: dynimg.width(), h: dynimg.height(), jpeg });
        Some(info)
    }

    // ------------------------------------------------------------------------------------------
    pub fn finish(self) -> Result<Vec<u8>, String> {
        let Pdf { fonts, mut pages, imgs, .. } = self;
        if pages.is_empty() {
            pages.push(PageData { w: 595.3, h: 841.9, ops: String::new(), imgs: BTreeSet::new() });
        }
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let mut font_res = Dictionary::new();
        for (fi, face) in fonts.faces.iter().enumerate() {
            if face.used.is_empty() {
                continue;
            }
            let id = write_font(&mut doc, fi, face);
            font_res.set(format!("F{fi}"), Object::Reference(id));
        }

        let img_ids: Vec<ObjectId> = imgs
            .into_iter()
            .map(|im| {
                let mut d = Dictionary::new();
                d.set("Type", Object::Name(b"XObject".to_vec()));
                d.set("Subtype", Object::Name(b"Image".to_vec()));
                d.set("Width", Object::Integer(im.w as i64));
                d.set("Height", Object::Integer(im.h as i64));
                d.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
                d.set("BitsPerComponent", Object::Integer(8));
                d.set("Filter", Object::Name(b"DCTDecode".to_vec()));
                let mut s = Stream::new(d, im.jpeg);
                s.allows_compression = false;
                doc.add_object(s)
            })
            .collect();

        let mut kids = Vec::new();
        for pg in pages {
            let content_id = doc.add_object(Stream::new(Dictionary::new(), pg.ops.into_bytes()));
            let mut res = Dictionary::new();
            res.set("Font", Object::Dictionary(font_res.clone()));
            if !pg.imgs.is_empty() {
                let mut xo = Dictionary::new();
                for i in &pg.imgs {
                    xo.set(format!("Im{i}"), Object::Reference(img_ids[*i]));
                }
                res.set("XObject", Object::Dictionary(xo));
            }
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"Page".to_vec()));
            d.set("Parent", Object::Reference(pages_id));
            d.set(
                "MediaBox",
                Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Real(pg.w), Object::Real(pg.h)]),
            );
            d.set("Contents", Object::Reference(content_id));
            d.set("Resources", Object::Dictionary(res));
            kids.push(Object::Reference(doc.add_object(d)));
        }
        let count = kids.len() as i64;
        let mut pd = Dictionary::new();
        pd.set("Type", Object::Name(b"Pages".to_vec()));
        pd.set("Kids", Object::Array(kids));
        pd.set("Count", Object::Integer(count));
        doc.objects.insert(pages_id, Object::Dictionary(pd));
        let mut cat = Dictionary::new();
        cat.set("Type", Object::Name(b"Catalog".to_vec()));
        cat.set("Pages", Object::Reference(pages_id));
        let cat_id = doc.add_object(cat);
        doc.trailer.set("Root", Object::Reference(cat_id));
        let mut info = Dictionary::new();
        info.set("Producer", Object::string_literal("Notch"));
        let info_id = doc.add_object(info);
        doc.trailer.set("Info", Object::Reference(info_id));

        doc.compress();
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

#[derive(Clone, Copy)]
pub enum Shape {
    Rect,
    Round(f32),
    Ellipse,
}

fn write_font(doc: &mut Document, fi: usize, face: &super::font::Face) -> ObjectId {
    use subsetter::GlyphRemapper;
    let gids: Vec<u16> = face.used.keys().copied().collect();
    let mut rm = GlyphRemapper::new();
    rm.remap(0);
    for g in &gids {
        rm.remap(*g);
    }
    // CID = glyph id ASLI; CIDToGIDMap memetakan ke glyph id baru di font ter-subset
    let (font_bytes, cid2gid) = match subsetter::subset(&face.data, 0, &rm) {
        Ok(b) => {
            let maxg = *gids.last().unwrap() as usize;
            let mut m = vec![0u8; (maxg + 1) * 2];
            for g in &gids {
                let ng = rm.get(*g).unwrap_or(0);
                m[*g as usize * 2] = (ng >> 8) as u8;
                m[*g as usize * 2 + 1] = (ng & 0xff) as u8;
            }
            (b, Some(m))
        }
        Err(_) => (face.data.clone(), None),
    };
    let tag = format!("{}{}AAAA", (b'A' + ((fi / 26) % 26) as u8) as char, (b'A' + (fi % 26) as u8) as char);
    let base_name = format!("{}+{}", tag, face.ps_name);
    let s = 1000.0 / face.upem;

    let mut fd = Dictionary::new();
    fd.set("Length1", Object::Integer(font_bytes.len() as i64));
    let ff = doc.add_object(Stream::new(fd, font_bytes));

    let mut desc = Dictionary::new();
    desc.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    desc.set("FontName", Object::Name(base_name.clone().into_bytes()));
    desc.set("Flags", Object::Integer(4));
    desc.set(
        "FontBBox",
        Object::Array(face.bbox.iter().map(|v| Object::Integer((*v as f32 * s).round() as i64)).collect()),
    );
    desc.set("ItalicAngle", Object::Real(face.italic_angle));
    desc.set("Ascent", Object::Integer((face.asc * s).round() as i64));
    desc.set("Descent", Object::Integer((face.desc * s).round() as i64));
    desc.set("CapHeight", Object::Integer((face.cap as f32 * s).round() as i64));
    desc.set("StemV", Object::Integer(80));
    desc.set("FontFile2", Object::Reference(ff));
    let desc_id = doc.add_object(desc);

    let mut w_arr: Vec<Object> = Vec::new();
    for g in &gids {
        let adv = face_adv(face, *g);
        w_arr.push(Object::Integer(*g as i64));
        w_arr.push(Object::Array(vec![Object::Integer((adv as f32 * s).round() as i64)]));
    }

    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
    cid.set("BaseFont", Object::Name(base_name.clone().into_bytes()));
    let mut csi = Dictionary::new();
    csi.set("Registry", Object::string_literal("Adobe"));
    csi.set("Ordering", Object::string_literal("Identity"));
    csi.set("Supplement", Object::Integer(0));
    cid.set("CIDSystemInfo", Object::Dictionary(csi));
    cid.set("FontDescriptor", Object::Reference(desc_id));
    cid.set("DW", Object::Integer(1000));
    cid.set("W", Object::Array(w_arr));
    match cid2gid {
        Some(m) => {
            let id = doc.add_object(Stream::new(Dictionary::new(), m));
            cid.set("CIDToGIDMap", Object::Reference(id));
        }
        None => cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec())),
    }
    let cid_id = doc.add_object(cid);

    let tu = doc.add_object(Stream::new(Dictionary::new(), to_unicode(face).into_bytes()));
    let mut t0 = Dictionary::new();
    t0.set("Type", Object::Name(b"Font".to_vec()));
    t0.set("Subtype", Object::Name(b"Type0".to_vec()));
    t0.set("BaseFont", Object::Name(base_name.into_bytes()));
    t0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    t0.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
    t0.set("ToUnicode", Object::Reference(tu));
    doc.add_object(t0)
}

fn face_adv(face: &super::font::Face, g: u16) -> u16 {
    face.advance(g)
}

fn to_unicode(face: &super::font::Face) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let items: Vec<(&u16, &char)> = face.used.iter().collect();
    for chunk in items.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, c) in chunk {
            let mut buf = [0u16; 2];
            let enc = c.encode_utf16(&mut buf);
            let hex: String = enc.iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(s, "<{:04X}> <{}>", g, hex);
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}
