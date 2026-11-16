//! DOCX -> PDF: paragraf (gaya, perataan, indentasi, spasi), penomoran/bullet, tabel, gambar inline,
//! kotak teks, hyperlink (berwarna), page break, ukuran/margin halaman dari sectPr.
use super::pdf::Pdf;
use super::text::*;
use super::util::*;
use roxmltree::Node;
use std::collections::HashMap;

#[derive(Default, Clone)]
struct RPr {
    bold: Option<bool>,
    italic: Option<bool>,
    size: Option<f32>,
    color: Option<Rgb>,
    ul: Option<bool>,
    strike: Option<bool>,
    serif: Option<bool>,
    caps: Option<bool>,
    small: Option<bool>,
    style: Option<String>,
}
fn merge_r(t: &RPr, b: &RPr) -> RPr {
    RPr {
        bold: t.bold.or(b.bold),
        italic: t.italic.or(b.italic),
        size: t.size.or(b.size),
        color: t.color.or(b.color),
        ul: t.ul.or(b.ul),
        strike: t.strike.or(b.strike),
        serif: t.serif.or(b.serif),
        caps: t.caps.or(b.caps),
        small: t.small.or(b.small),
        style: t.style.clone().or_else(|| b.style.clone()),
    }
}

#[derive(Default, Clone)]
struct PPr {
    jc: Option<Align>,
    before: Option<f32>,
    after: Option<f32>,
    line: Option<(f32, bool)>, // (nilai, exact)
    left: Option<f32>,
    right: Option<f32>,
    first: Option<f32>,
    num: Option<(u32, u32)>,
    page_break: Option<bool>,
}
fn merge_p(t: &PPr, b: &PPr) -> PPr {
    PPr {
        jc: t.jc.or(b.jc),
        before: t.before.or(b.before),
        after: t.after.or(b.after),
        line: t.line.or(b.line),
        left: t.left.or(b.left),
        right: t.right.or(b.right),
        first: t.first.or(b.first),
        num: t.num.or(b.num),
        page_break: t.page_break.or(b.page_break),
    }
}

fn is_serif_name(n: &str) -> bool {
    let l = n.to_lowercase();
    if l.contains("sans") {
        return false;
    }
    ["times", "georgia", "cambria", "garamond", "palatino", "book antiqua", "serif", "century", "minion", "baskerville"]
        .iter()
        .any(|k| l.contains(k))
}

fn parse_rpr(n: Node) -> RPr {
    let mut r = RPr::default();
    r.bold = on(n, "b");
    r.italic = on(n, "i");
    r.ul = on(n, "u");
    r.strike = on(n, "strike").or_else(|| on(n, "dstrike"));
    r.caps = on(n, "caps");
    r.small = on(n, "smallCaps");
    if let Some(c) = kid(n, "color") {
        r.color = at(c, "val").and_then(hex);
    }
    if let Some(s) = kid(n, "sz") {
        r.size = fnum(at(s, "val")).map(|v| v / 2.0);
    }
    if let Some(v) = kid(n, "vertAlign") {
        if matches!(at(v, "val"), Some("superscript") | Some("subscript")) {
            r.size = r.size.map(|s| s * 0.7).or(Some(7.0));
        }
    }
    if let Some(f) = kid(n, "rFonts") {
        if let Some(a) = at(f, "ascii").or_else(|| at(f, "hAnsi")) {
            r.serif = Some(is_serif_name(a));
        }
    }
    r.style = kid(n, "rStyle").and_then(|s| at(s, "val")).map(|s| s.to_string());
    r
}

fn parse_ppr(n: Node) -> PPr {
    let mut p = PPr::default();
    if let Some(j) = kid(n, "jc") {
        p.jc = match at(j, "val") {
            Some("center") => Some(Align::Center),
            Some("right") | Some("end") => Some(Align::Right),
            Some("both") | Some("distribute") => Some(Align::Justify),
            Some(_) => Some(Align::Left),
            None => None,
        };
    }
    if let Some(s) = kid(n, "spacing") {
        p.before = fnum(at(s, "before")).map(|v| v / 20.0);
        p.after = fnum(at(s, "after")).map(|v| v / 20.0);
        if let Some(l) = fnum(at(s, "line")) {
            let exact = matches!(at(s, "lineRule"), Some("exact") | Some("atLeast"));
            p.line = Some((l, exact));
        }
    }
    if let Some(i) = kid(n, "ind") {
        p.left = fnum(at(i, "left").or_else(|| at(i, "start"))).map(|v| v / 20.0);
        p.right = fnum(at(i, "right").or_else(|| at(i, "end"))).map(|v| v / 20.0);
        if let Some(h) = fnum(at(i, "hanging")) {
            p.first = Some(-h / 20.0);
        } else if let Some(f) = fnum(at(i, "firstLine")) {
            p.first = Some(f / 20.0);
        }
    }
    if let Some(np) = kid(n, "numPr") {
        let id = kid(np, "numId").and_then(|x| at(x, "val")).and_then(|v| v.parse().ok());
        let lv = kid(np, "ilvl").and_then(|x| at(x, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
        if let Some(id) = id {
            p.num = Some((id, lv));
        }
    }
    p.page_break = on(n, "pageBreakBefore");
    p
}

// ------------------------------------------------------------------------------------------------
struct Style {
    based: Option<String>,
    p: PPr,
    r: RPr,
}
#[derive(Default)]
struct Styles {
    map: HashMap<String, Style>,
    def_p: PPr,
    def_r: RPr,
    normal: Option<String>,
}
impl Styles {
    fn load(xml: &str) -> Styles {
        let mut st = Styles::default();
        let Ok(d) = roxmltree::Document::parse(xml) else { return st };
        if let Some(dd) = desc(d.root_element(), "docDefaults") {
            if let Some(r) = kid(dd, "rPrDefault").and_then(|x| kid(x, "rPr")) {
                st.def_r = parse_rpr(r);
            }
            if let Some(p) = kid(dd, "pPrDefault").and_then(|x| kid(x, "pPr")) {
                st.def_p = parse_ppr(p);
            }
        }
        for s in d.descendants().filter(|n| n.is_element() && name(*n) == "style") {
            let Some(id) = at(s, "styleId") else { continue };
            if at(s, "type") == Some("paragraph") && at(s, "default") == Some("1") {
                st.normal = Some(id.to_string());
            }
            st.map.insert(
                id.to_string(),
                Style {
                    based: kid(s, "basedOn").and_then(|b| at(b, "val")).map(|v| v.to_string()),
                    p: kid(s, "pPr").map(parse_ppr).unwrap_or_default(),
                    r: kid(s, "rPr").map(parse_rpr).unwrap_or_default(),
                },
            );
        }
        st
    }
    fn resolve(&self, id: Option<&str>) -> (PPr, RPr) {
        let mut chain: Vec<&Style> = vec![];
        let mut cur = id.map(|s| s.to_string());
        while let Some(c) = cur {
            if chain.len() > 12 {
                break;
            }
            let Some(s) = self.map.get(&c) else { break };
            chain.push(s);
            cur = s.based.clone();
        }
        let (mut p, mut r) = (self.def_p.clone(), self.def_r.clone());
        for s in chain.iter().rev() {
            p = merge_p(&s.p, &p);
            r = merge_r(&s.r, &r);
        }
        (p, r)
    }
}

// ------------------------------------------------------------------------------------------------
struct Lvl {
    fmt: String,
    text: String,
    start: u32,
    left: Option<f32>,
    first: Option<f32>,
}
#[derive(Default)]
struct Numbering {
    nums: HashMap<u32, u32>,
    abs: HashMap<u32, HashMap<u32, Lvl>>,
    counters: HashMap<(u32, u32), u32>,
}

fn roman(mut n: u32, upper: bool) -> String {
    let t = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}
fn letters(n: u32, upper: bool) -> String {
    let n = n.max(1) - 1;
    let c = (b'a' + (n % 26) as u8) as char;
    let s: String = std::iter::repeat(c).take((n / 26 + 1) as usize).collect();
    if upper { s.to_uppercase() } else { s }
}
fn fmt_num(fmt: &str, n: u32) -> String {
    match fmt {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n, false),
        "upperRoman" => roman(n, true),
        "decimalZero" => format!("{n:02}"),
        "none" => String::new(),
        _ => n.to_string(),
    }
}

impl Numbering {
    fn load(xml: &str) -> Numbering {
        let mut nb = Numbering::default();
        let Ok(d) = roxmltree::Document::parse(xml) else { return nb };
        for a in d.descendants().filter(|n| n.is_element() && name(*n) == "abstractNum") {
            let Some(id) = at(a, "abstractNumId").and_then(|v| v.parse::<u32>().ok()) else { continue };
            let mut lv = HashMap::new();
            for l in kids(a, "lvl") {
                let il = at(l, "ilvl").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                let ind = kid(l, "pPr").and_then(|p| kid(p, "ind"));
                let (mut left, mut first) = (None, None);
                if let Some(i) = ind {
                    left = fnum(at(i, "left").or_else(|| at(i, "start"))).map(|v| v / 20.0);
                    first = if let Some(h) = fnum(at(i, "hanging")) { Some(-h / 20.0) } else { fnum(at(i, "firstLine")).map(|v| v / 20.0) };
                }
                lv.insert(
                    il,
                    Lvl {
                        fmt: kid(l, "numFmt").and_then(|x| at(x, "val")).unwrap_or("decimal").to_string(),
                        text: kid(l, "lvlText").and_then(|x| at(x, "val")).unwrap_or("").to_string(),
                        start: kid(l, "start").and_then(|x| at(x, "val")).and_then(|v| v.parse().ok()).unwrap_or(1),
                        left,
                        first,
                    },
                );
            }
            nb.abs.insert(id, lv);
        }
        for n in d.descendants().filter(|n| n.is_element() && name(*n) == "num") {
            if let (Some(id), Some(a)) = (
                at(n, "numId").and_then(|v| v.parse::<u32>().ok()),
                kid(n, "abstractNumId").and_then(|x| at(x, "val")).and_then(|v| v.parse::<u32>().ok()),
            ) {
                nb.nums.insert(id, a);
            }
        }
        nb
    }

    /// (label, left, first) untuk paragraf bernomor; menaikkan penghitung.
    fn label(&mut self, num: u32, lvl: u32) -> Option<(String, Option<f32>, Option<f32>)> {
        let abs = self.abs.get(self.nums.get(&num)?)?;
        let l = abs.get(&lvl)?;
        if l.fmt == "bullet" {
            let c = l.text.chars().next();
            let t = match c {
                None => "•".to_string(),
                Some(c) if (c as u32) >= 0xF000 && (c as u32) <= 0xF0FF => "•".to_string(),
                Some(_) => l.text.clone(),
            };
            return Some((t, l.left, l.first));
        }
        let cnt = self.counters.entry((num, lvl)).or_insert(l.start - 1);
        *cnt += 1;
        let cur = *cnt;
        self.counters.retain(|(n, lv), _| !(*n == num && *lv > lvl));
        let mut out = l.text.clone();
        for i in 0..9u32 {
            let key = format!("%{}", i + 1);
            if out.contains(&key) {
                let (f, v) = if i == lvl {
                    (l.fmt.as_str(), cur)
                } else {
                    let ol = abs.get(&i);
                    (ol.map(|o| o.fmt.as_str()).unwrap_or("decimal"), self.counters.get(&(num, i)).copied().unwrap_or(ol.map(|o| o.start).unwrap_or(1)))
                };
                out = out.replace(&key, &fmt_num(f, v));
            }
        }
        Some((out, l.left, l.first))
    }
}

// ------------------------------------------------------------------------------------------------
enum Piece {
    R(Run),
    Img(Block),
    Brk,
}

struct Conv<'a> {
    pdf: &'a mut Pdf,
    z: &'a mut Zip,
    rels: HashMap<String, Rel>,
    styles: Styles,
    num: Numbering,
}

fn px_or_pt(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some(x) = v.strip_suffix("pt") {
        x.parse().ok()
    } else if let Some(x) = v.strip_suffix("px") {
        x.parse::<f32>().ok().map(|p| p * 0.75)
    } else if let Some(x) = v.strip_suffix("in") {
        x.parse::<f32>().ok().map(|p| p * 72.0)
    } else {
        None
    }
}

impl<'a> Conv<'a> {
    fn run_from(&self, eff: &RPr, text: String) -> Run {
        let size = eff.size.unwrap_or(10.0);
        let text = if eff.caps == Some(true) || eff.small == Some(true) { text.to_uppercase() } else { text };
        Run {
            text,
            face: self.pdf.fonts.slot(eff.serif.unwrap_or(false), eff.bold.unwrap_or(false), eff.italic.unwrap_or(false)),
            size,
            color: eff.color.unwrap_or([0, 0, 0]),
            ul: eff.ul.unwrap_or(false),
            st: eff.strike.unwrap_or(false),
        }
    }

    fn blocks_into(&mut self, parent: Node, in_cell: bool, out: &mut Vec<Block>) {
        for c in parent.children().filter(|c| c.is_element()) {
            match name(c) {
                "p" => self.para(c, in_cell, out),
                "tbl" => {
                    if in_cell {
                        for tr in kids(c, "tr") {
                            for tc in kids(tr, "tc") {
                                self.blocks_into(tc, true, out);
                            }
                        }
                    } else {
                        self.table(c, out);
                    }
                }
                "sdt" => {
                    if let Some(sc) = kid(c, "sdtContent") {
                        self.blocks_into(sc, in_cell, out);
                    }
                }
                "sdtContent" | "customXml" | "ins" => self.blocks_into(c, in_cell, out),
                _ => {}
            }
        }
    }

    fn para(&mut self, p: Node, in_cell: bool, out: &mut Vec<Block>) {
        let ppr = kid(p, "pPr");
        let direct = ppr.map(parse_ppr).unwrap_or_default();
        let sid = ppr.and_then(|n| kid(n, "pStyle")).and_then(|n| at(n, "val")).map(|s| s.to_string()).or_else(|| self.styles.normal.clone());
        let (sp, sr) = self.styles.resolve(sid.as_deref());
        // penomoran: langsung > gaya
        let numref = direct.num.or(sp.num);
        let mut np = PPr::default();
        let mut label_text = None;
        if let Some((nid, lvl)) = numref {
            if nid != 0 {
                if let Some((t, l, f)) = self.num.label(nid, lvl) {
                    np.left = l;
                    np.first = f;
                    label_text = Some(t);
                }
            }
        }
        let pf = merge_p(&direct, &merge_p(&np, &sp));

        let mut pieces = Vec::new();
        let mut extra = Vec::new();
        let base = sr.clone();
        self.inline(p, &base, false, &mut pieces, &mut extra);

        let size0 = sr.size.unwrap_or(10.0);
        let face0 = self.pdf.fonts.slot(sr.serif.unwrap_or(false), sr.bold.unwrap_or(false), sr.italic.unwrap_or(false));
        let cell_tight = in_cell && ppr.and_then(|n| kid(n, "spacing")).is_none();
        let (mult, exact) = match pf.line {
            Some((v, true)) => (1.0, Some(v / 20.0)),
            Some((v, false)) => (v / 240.0, None),
            None => (1.0, None),
        };
        let align = pf.jc.unwrap_or(Align::Left);
        let mk = |runs: Vec<Run>| -> Para {
            let mut para = Para::new(face0, size0);
            para.runs = runs;
            para.align = align;
            para.left = pf.left.unwrap_or(0.0).max(0.0);
            para.first = pf.first.unwrap_or(0.0);
            para.right = pf.right.unwrap_or(0.0).max(0.0);
            para.before = if cell_tight { 0.0 } else { pf.before.unwrap_or(0.0) };
            para.after = if cell_tight { 0.0 } else { pf.after.unwrap_or(0.0) };
            para.mult = if cell_tight { 1.0 } else { mult };
            para.exact = exact;
            para
        };

        let mut blocks: Vec<Block> = Vec::new();
        let mut cur: Vec<Run> = Vec::new();
        for piece in pieces {
            match piece {
                Piece::R(r) => cur.push(r),
                Piece::Img(mut b) => {
                    if !cur.is_empty() {
                        blocks.push(Block::Para(mk(std::mem::take(&mut cur))));
                    }
                    if let Block::Image(i) = &mut b {
                        i.align = align;
                    }
                    blocks.push(b);
                }
                Piece::Brk => {
                    if !cur.is_empty() {
                        blocks.push(Block::Para(mk(std::mem::take(&mut cur))));
                    }
                    blocks.push(Block::PageBreak);
                }
            }
        }
        if !cur.is_empty() || blocks.is_empty() {
            blocks.push(Block::Para(mk(cur)));
        }
        // label & break_before hanya di paragraf pertama; before/after hanya di awal/akhir
        let n_blocks = blocks.len();
        let mut seen_first = false;
        for (i, b) in blocks.iter_mut().enumerate() {
            if let Block::Para(para) = b {
                if !seen_first {
                    seen_first = true;
                    para.break_before = pf.page_break == Some(true);
                    if let Some(t) = &label_text {
                        let run = para.runs.first().cloned().unwrap_or_else(|| self.run_from(&sr, String::new()));
                        let mut lr = run;
                        lr.text = String::new();
                        lr.ul = false;
                        lr.st = false;
                        para.label = Some(Label { text: t.clone(), run: lr });
                    }
                } else {
                    para.before = 0.0;
                }
                if i + 1 != n_blocks {
                    para.after = 0.0;
                }
            }
        }
        out.extend(blocks);
        out.extend(extra);
    }

    fn inline(&mut self, node: Node, base: &RPr, hl: bool, pieces: &mut Vec<Piece>, extra: &mut Vec<Block>) {
        for c in node.children().filter(|c| c.is_element()) {
            match name(c) {
                "r" => self.run(c, base, hl, pieces, extra),
                "hyperlink" => self.inline(c, base, true, pieces, extra),
                "fldSimple" | "smartTag" | "ins" | "customXml" | "bdo" | "dir" | "sdtContent" => self.inline(c, base, hl, pieces, extra),
                "sdt" => {
                    if let Some(sc) = kid(c, "sdtContent") {
                        self.inline(sc, base, hl, pieces, extra);
                    }
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, r: Node, base: &RPr, hl: bool, pieces: &mut Vec<Piece>, extra: &mut Vec<Block>) {
        let rpr = kid(r, "rPr").map(parse_rpr).unwrap_or_default();
        let rs = match &rpr.style {
            Some(s) => self.styles.resolve(Some(s)).1,
            None => RPr::default(),
        };
        let mut eff = merge_r(&rpr, &merge_r(&rs, base));
        if hl {
            if eff.color.is_none() || rpr.color.is_none() {
                eff.color = rpr.color.or(Some([5, 99, 193]));
            }
            if rpr.ul.is_none() {
                eff.ul = Some(true);
            }
        }
        let mut buf = String::new();
        self.run_children(r, &eff, &mut buf, pieces, extra);
        self.flush(&eff, &mut buf, pieces);
    }

    fn flush(&mut self, eff: &RPr, buf: &mut String, pieces: &mut Vec<Piece>) {
        if !buf.is_empty() {
            let run = self.run_from(eff, std::mem::take(buf));
            pieces.push(Piece::R(run));
        }
    }

    fn run_children(&mut self, parent: Node, eff: &RPr, buf: &mut String, pieces: &mut Vec<Piece>, extra: &mut Vec<Block>) {
        for c in parent.children().filter(|c| c.is_element()) {
            match name(c) {
                "t" => buf.push_str(c.text().unwrap_or("")),
                "tab" => buf.push('\t'),
                "noBreakHyphen" => buf.push('-'),
                "br" | "cr" => {
                    if at(c, "type") == Some("page") {
                        self.flush(eff, buf, pieces);
                        pieces.push(Piece::Brk);
                    } else {
                        buf.push('\n');
                    }
                }
                "drawing" | "pict" | "object" => {
                    self.flush(eff, buf, pieces);
                    self.picture(c, pieces, extra);
                }
                "AlternateContent" => {
                    if let Some(ch) = kid(c, "Choice").or_else(|| kid(c, "Fallback")) {
                        self.run_children(ch, eff, buf, pieces, extra);
                    }
                }
                _ => {}
            }
        }
    }

    fn picture(&mut self, c: Node, pieces: &mut Vec<Piece>, extra: &mut Vec<Block>) {
        // kotak teks di dalam drawing/pict -> paragraf biasa setelah paragraf ini
        for t in c.descendants().filter(|n| n.is_element() && name(*n) == "txbxContent") {
            self.blocks_into(t, false, extra);
        }
        let (mut w, mut h, rid): (f32, f32, Option<String>);
        if name(c) == "drawing" {
            let Some(inl) = kid(c, "inline").or_else(|| kid(c, "anchor")) else { return };
            if name(inl) == "anchor" && matches!(at(inl, "behindDoc"), Some("1") | Some("true")) {
                return;
            }
            let ext = desc(inl, "extent");
            w = ext.and_then(|e| fnum(at(e, "cx"))).unwrap_or(0.0) / 12700.0;
            h = ext.and_then(|e| fnum(at(e, "cy"))).unwrap_or(0.0) / 12700.0;
            rid = desc(inl, "blip").and_then(|b| at(b, "embed")).map(|s| s.to_string());
        } else {
            let Some(im) = desc(c, "imagedata") else { return };
            rid = at(im, "id").map(|s| s.to_string());
            w = 0.0;
            h = 0.0;
            if let Some(sh) = desc(c, "shape") {
                if let Some(style) = at(sh, "style") {
                    for kv in style.split(';') {
                        if let Some((k, v)) = kv.split_once(':') {
                            match k.trim() {
                                "width" => w = px_or_pt(v).unwrap_or(0.0),
                                "height" => h = px_or_pt(v).unwrap_or(0.0),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        let Some(rid) = rid else { return };
        let Some(rel) = self.rels.get(&rid).cloned() else { return };
        let Some(bytes) = read_bytes(self.z, &rel.target) else { return };
        let Some(info) = self.pdf.add_image(&rel.target, &bytes) else { return };
        if w <= 0.0 || h <= 0.0 {
            w = info.w as f32 * 0.75;
            h = info.h as f32 * 0.75;
        }
        pieces.push(Piece::Img(Block::Image(ImgBlock { idx: info.idx, w, h, align: Align::Left })));
    }

    fn table(&mut self, t: Node, out: &mut Vec<Block>) {
        let cols: Vec<f32> = kid(t, "tblGrid")
            .map(|g| kids(g, "gridCol").iter().filter_map(|c| fnum(at(*c, "w"))).map(|v| v / 20.0).collect())
            .unwrap_or_default();
        let mut borders = false;
        if let Some(pr) = kid(t, "tblPr") {
            if let Some(b) = kid(pr, "tblBorders") {
                borders = b.children().filter(|c| c.is_element()).any(|c| !matches!(at(c, "val"), Some("nil") | Some("none") | None));
            }
            if let Some(s) = kid(pr, "tblStyle").and_then(|s| at(s, "val")) {
                if s.to_lowercase().contains("grid") {
                    borders = true;
                }
            }
        }
        let mut rows = Vec::new();
        for tr in kids(t, "tr") {
            let mut cells = Vec::new();
            for tc in kids(tr, "tc") {
                let pr = kid(tc, "tcPr");
                let span = pr.and_then(|p| kid(p, "gridSpan")).and_then(|g| at(g, "val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                let fill = pr.and_then(|p| kid(p, "shd")).and_then(|s| at(s, "fill")).and_then(|f| if f == "auto" { None } else { hex(f) });
                let cont = pr.and_then(|p| kid(p, "vMerge")).map(|v| at(v, "val") != Some("restart")).unwrap_or(false);
                if let Some(cb) = pr.and_then(|p| kid(p, "tcBorders")) {
                    if cb.children().filter(|c| c.is_element()).any(|c| !matches!(at(c, "val"), Some("nil") | Some("none") | None)) {
                        borders = true;
                    }
                }
                let mut blocks = Vec::new();
                self.blocks_into(tc, true, &mut blocks);
                if blocks.is_empty() {
                    blocks.push(Block::Para(Para::new(0, 10.0)));
                }
                cells.push(Cell { blocks, span, fill, covered: cont });
            }
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
        out.push(Block::Table(Table { cols, rows, borders }));
    }
}

pub fn convert(src: &[u8], mut pdf: Pdf) -> Result<Vec<u8>, String> {
    let mut z = open_zip(src)?;
    let main = "word/document.xml";
    let doc_xml = read_text(&mut z, main).ok_or("word/document.xml tidak ditemukan")?;
    let styles = Styles::load(&read_text(&mut z, "word/styles.xml").unwrap_or_default());
    let num = Numbering::load(&read_text(&mut z, "word/numbering.xml").unwrap_or_default());
    let rels = rels_of(&mut z, main);
    let d = parse(&doc_xml)?;
    let body = desc(d.root_element(), "body").ok_or("Isi dokumen kosong")?;

    // ukuran & margin halaman (sectPr terakhir di body)
    let (mut pw, mut ph, mut ml, mut mr, mut mt, mut mb) = (595.3f32, 841.9f32, 72.0f32, 72.0f32, 72.0f32, 72.0f32);
    if let Some(sp) = kids(body, "sectPr").last() {
        if let Some(sz) = kid(*sp, "pgSz") {
            if let (Some(w), Some(h)) = (fnum(at(sz, "w")), fnum(at(sz, "h"))) {
                pw = w / 20.0;
                ph = h / 20.0;
            }
        }
        if let Some(m) = kid(*sp, "pgMar") {
            ml = fnum(at(m, "left")).map(|v| v / 20.0).unwrap_or(ml);
            mr = fnum(at(m, "right")).map(|v| v / 20.0).unwrap_or(mr);
            mt = fnum(at(m, "top")).map(|v| v / 20.0).unwrap_or(mt);
            mb = fnum(at(m, "bottom")).map(|v| v / 20.0).unwrap_or(mb);
        }
    }
    let (mt, mb) = (mt.min(ph / 3.0), mb.min(ph / 3.0));

    let mut blocks = Vec::new();
    {
        let mut cv = Conv { pdf: &mut pdf, z: &mut z, rels, styles, num };
        cv.blocks_into(body, false, &mut blocks);
    }
    let mut flow = Flow::new(pw, ph, ml.min(pw / 3.0), mr.min(pw / 3.0), mt, mb);
    flow.ensure(&mut pdf);
    for b in &blocks {
        flow.block(&mut pdf, b);
    }
    pdf.finish()
}
