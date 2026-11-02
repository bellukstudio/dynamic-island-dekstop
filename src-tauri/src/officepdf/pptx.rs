//! PPTX -> PDF: satu halaman per slide, ukuran sesuai slide. Posisi absolut (EMU), pewarisan
//! placeholder slide -> layout -> master, warna tema (+lumMod/lumOff/tint/shade), latar, bentuk dasar
//! (rect/roundRect/ellipse/garis), grup, gambar (+crop), tabel, dan teks (bullet, penomoran, perataan,
//! spasi, autofit fontScale, anchor vertikal).
use super::pdf::{Pdf, Shape};
use super::text::*;
use super::util::*;
use roxmltree::{Document, Node};
use std::collections::HashMap;

type N<'a> = Node<'a, 'a>;

#[derive(Clone, Copy)]
struct Xf {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}
impl Xf {
    fn id() -> Xf {
        Xf { sx: 1.0, sy: 1.0, tx: 0.0, ty: 0.0 }
    }
    fn rect(&self, x: f64, y: f64, cx: f64, cy: f64) -> (f32, f32, f32, f32) {
        (((x * self.sx + self.tx) / 12700.0) as f32, ((y * self.sy + self.ty) / 12700.0) as f32, (cx * self.sx / 12700.0) as f32, (cy * self.sy / 12700.0) as f32)
    }
}

type Rect = (f32, f32, f32, f32);

fn xfrm_vals(x: Node) -> Option<(f64, f64, f64, f64)> {
    let off = kid(x, "off")?;
    let ext = kid(x, "ext")?;
    Some((
        at(off, "x")?.parse().ok()?,
        at(off, "y")?.parse().ok()?,
        at(ext, "cx")?.parse().ok()?,
        at(ext, "cy")?.parse().ok()?,
    ))
}

fn norm_ty(t: Option<&str>) -> &str {
    match t {
        Some("ctrTitle") | Some("title") => "title",
        Some("subTitle") | Some("body") | Some("obj") | None => "body",
        Some(o) => o,
    }
}

fn find_ph<'a>(root: N<'a>, ty: Option<&str>, idx: Option<&str>, by_idx: bool) -> Option<N<'a>> {
    for sp in root.descendants().filter(|n| n.is_element() && name(*n) == "sp") {
        let Some(ph) = kid(sp, "nvSpPr").and_then(|n| kid(n, "nvPr")).and_then(|n| kid(n, "ph")) else { continue };
        let ok = if by_idx && idx.is_some() { at(ph, "idx") == idx } else { norm_ty(at(ph, "type")) == norm_ty(ty) };
        if ok {
            return Some(sp);
        }
    }
    None
}

fn ca<'a>(chain: &[N<'a>], key: &str) -> Option<&'a str> {
    chain.iter().find_map(|n| at(*n, key))
}
fn cc<'a>(chain: &[N<'a>], nm: &str) -> Option<N<'a>> {
    chain.iter().find_map(|n| kid(*n, nm))
}

struct R<'a, 'b> {
    pdf: &'b mut Pdf,
    z: &'b mut Zip,
    theme: HashMap<String, Rgb>,
    clrmap: HashMap<String, String>,
    layout: Option<N<'a>>,
    master: Option<N<'a>>,
    pres: Option<N<'a>>,
    bg_default: Rgb,
}

enum Fill {
    Unset,
    No,
    Solid(Rgb),
}

impl<'a, 'b> R<'a, 'b> {
    fn scheme(&self, v: &str) -> Option<Rgb> {
        let m = self.clrmap.get(v).map(|s| s.as_str()).unwrap_or(v);
        self.theme.get(m).copied()
    }

    fn clr(&self, holder: N, ph: Option<Rgb>) -> Option<Rgb> {
        let c = holder.children().find(|c| c.is_element() && matches!(name(*c), "srgbClr" | "schemeClr" | "sysClr" | "prstClr"))?;
        let mut col = match name(c) {
            "srgbClr" => at(c, "val").and_then(hex)?,
            "sysClr" => at(c, "lastClr").and_then(hex).or_else(|| if at(c, "val") == Some("window") { Some([255, 255, 255]) } else { Some([0, 0, 0]) })?,
            "prstClr" => match at(c, "val") {
                Some("white") => [255, 255, 255],
                Some("black") => [0, 0, 0],
                Some("red") => [255, 0, 0],
                Some("green") => [0, 128, 0],
                Some("blue") => [0, 0, 255],
                Some("yellow") => [255, 255, 0],
                Some("gray") | Some("grey") => [128, 128, 128],
                _ => return None,
            },
            _ => {
                let v = at(c, "val")?;
                if v == "phClr" { ph? } else { self.scheme(v)? }
            }
        };
        for m in c.children().filter(|m| m.is_element()) {
            let val = fnum(at(m, "val")).map(|v| v / 100000.0);
            match (name(m), val) {
                ("lumMod", Some(v)) => {
                    let (h, s, l) = to_hsl(col);
                    col = from_hsl(h, s, (l * v).clamp(0.0, 1.0));
                }
                ("lumOff", Some(v)) => {
                    let (h, s, l) = to_hsl(col);
                    col = from_hsl(h, s, (l + v).clamp(0.0, 1.0));
                }
                ("satMod", Some(v)) => {
                    let (h, s, l) = to_hsl(col);
                    col = from_hsl(h, (s * v).clamp(0.0, 1.0), l);
                }
                ("tint", Some(v)) => {
                    for i in 0..3 {
                        col[i] = (col[i] as f32 * v + 255.0 * (1.0 - v)).round().clamp(0.0, 255.0) as u8;
                    }
                }
                ("shade", Some(v)) => {
                    for i in 0..3 {
                        col[i] = (col[i] as f32 * v).round().clamp(0.0, 255.0) as u8;
                    }
                }
                _ => {}
            }
        }
        Some(col)
    }

    fn fill_of(&self, sppr: N, ph: Option<Rgb>) -> Fill {
        for c in sppr.children().filter(|c| c.is_element()) {
            match name(c) {
                "noFill" => return Fill::No,
                "solidFill" => return self.clr(c, ph).map(Fill::Solid).unwrap_or(Fill::No),
                "gradFill" => {
                    return desc(c, "gs").and_then(|g| self.clr(g, ph)).map(Fill::Solid).unwrap_or(Fill::No);
                }
                "pattFill" => return kid(c, "fgClr").and_then(|g| self.clr(g, ph)).map(Fill::Solid).unwrap_or(Fill::No),
                "blipFill" => return Fill::No,
                _ => {}
            }
        }
        Fill::Unset
    }

    fn line_of(&self, sppr: N, ph: Option<Rgb>) -> Option<Option<(Rgb, f32)>> {
        let ln = kid(sppr, "ln")?;
        let w = fnum(at(ln, "w")).map(|v| v / 12700.0).unwrap_or(0.75).max(0.25);
        if kid(ln, "noFill").is_some() {
            return Some(None);
        }
        match kid(ln, "solidFill").and_then(|s| self.clr(s, ph)) {
            Some(c) => Some(Some((c, w))),
            None => None,
        }
    }

    fn tx1(&self) -> Rgb {
        self.scheme("tx1").unwrap_or([0, 0, 0])
    }

    fn inherited_rect(&self, ph: N) -> Option<Rect> {
        let (ty, idx) = (at(ph, "type"), at(ph, "idx"));
        let from = |sp: N| kid(sp, "spPr").and_then(|s| kid(s, "xfrm")).and_then(xfrm_vals);
        if let Some(l) = self.layout.and_then(|l| find_ph(l, ty, idx, true)) {
            if let Some(v) = from(l) {
                return Some(Xf::id().rect(v.0, v.1, v.2, v.3));
            }
        }
        if let Some(m) = self.master.and_then(|m| find_ph(m, ty, idx, false)) {
            if let Some(v) = from(m) {
                return Some(Xf::id().rect(v.0, v.1, v.2, v.3));
            }
        }
        None
    }

    // ------------------------------------------------------------------------------------------
    fn draw_tree(&mut self, tree: N, xf: Xf, skip_ph: bool, rels: &HashMap<String, Rel>) {
        for c in tree.children().filter(|c| c.is_element()) {
            if desc(c, "cNvPr").map(|n| at(n, "hidden") == Some("1")).unwrap_or(false) && name(c) != "grpSp" {
                continue;
            }
            match name(c) {
                "sp" | "cxnSp" => self.shape(c, xf, skip_ph, rels),
                "pic" => self.pic(c, xf, skip_ph, rels),
                "graphicFrame" => self.frame(c, xf),
                "grpSp" => {
                    let Some(gp) = kid(c, "grpSpPr").and_then(|g| kid(g, "xfrm")) else {
                        self.draw_tree(c, xf, skip_ph, rels);
                        continue;
                    };
                    let (Some(o), Some(e)) = (xfrm_vals(gp), kid(gp, "chOff").zip(kid(gp, "chExt"))) else { continue };
                    let (cox, coy) = (fnum(at(e.0, "x")).unwrap_or(0.0) as f64, fnum(at(e.0, "y")).unwrap_or(0.0) as f64);
                    let (cex, cey) = (fnum(at(e.1, "cx")).unwrap_or(0.0) as f64, fnum(at(e.1, "cy")).unwrap_or(0.0) as f64);
                    let kx = if cex > 0.0 { o.2 / cex } else { 1.0 };
                    let ky = if cey > 0.0 { o.3 / cey } else { 1.0 };
                    let nx = Xf { sx: xf.sx * kx, sy: xf.sy * ky, tx: xf.sx * (o.0 - cox * kx) + xf.tx, ty: xf.sy * (o.1 - coy * ky) + xf.ty };
                    self.draw_tree(c, nx, skip_ph, rels);
                }
                _ => {}
            }
        }
    }

    fn shape(&mut self, c: N, xf: Xf, skip_ph: bool, _rels: &HashMap<String, Rel>) {
        let ph = kid(c, "nvSpPr").and_then(|n| kid(n, "nvPr")).and_then(|n| kid(n, "ph"));
        if ph.is_some() && skip_ph {
            return;
        }
        let sppr = kid(c, "spPr");
        let own = sppr.and_then(|s| kid(s, "xfrm"));
        let rect: Rect = match own.and_then(xfrm_vals) {
            Some(v) => xf.rect(v.0, v.1, v.2, v.3),
            None => match ph.and_then(|p| self.inherited_rect(p)) {
                Some(r) => r,
                None => return,
            },
        };
        let (x, y, w, h) = rect;
        // gaya (p:style) sebagai cadangan fill/garis
        let style = kid(c, "style");
        let style_fill = style.and_then(|s| kid(s, "fillRef")).filter(|f| at(*f, "idx") != Some("0")).and_then(|f| self.clr(f, None));
        let style_ln = style.and_then(|s| kid(s, "lnRef")).filter(|f| at(*f, "idx") != Some("0")).and_then(|f| self.clr(f, None));
        let fill = match sppr.map(|s| self.fill_of(s, style_fill)).unwrap_or(Fill::Unset) {
            Fill::Solid(c) => Some(c),
            Fill::No => None,
            Fill::Unset => style_fill,
        };
        let line = match sppr.and_then(|s| self.line_of(s, style_ln)) {
            Some(l) => l,
            None => style_ln.map(|c| (c, 1.0)),
        };
        let geom = sppr.and_then(|s| kid(s, "prstGeom"));
        let prst = geom.and_then(|g| at(g, "prst")).unwrap_or("");
        match prst {
            "rect" | "flowChartProcess" | "snip1Rect" | "plaque" => self.pdf.shape(Shape::Rect, x, y, w, h, fill, line),
            "roundRect" => {
                let adj = geom.and_then(|g| desc(g, "gd")).and_then(|g| at(g, "fmla")).and_then(|f| f.strip_prefix("val ")).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(16667.0);
                self.pdf.shape(Shape::Round(w.min(h) * adj / 100000.0), x, y, w, h, fill, line)
            }
            "ellipse" | "flowChartConnector" => self.pdf.shape(Shape::Ellipse, x, y, w, h, fill, line),
            "line" | "straightConnector1" | "bentConnector3" => {
                if let Some((lc, lw)) = line {
                    let (fh, fv) = own.map(|o| (at(o, "flipH") == Some("1"), at(o, "flipV") == Some("1"))).unwrap_or((false, false));
                    let (x1, x2) = if fh { (x + w, x) } else { (x, x + w) };
                    let (y1, y2) = if fv { (y + h, y) } else { (y, y + h) };
                    self.pdf.line(x1, y1, x2, y2, lw, lc);
                }
            }
            _ => {}
        }
        // teks
        let Some(tx) = kid(c, "txBody") else { return };
        let mut containers: Vec<N> = vec![];
        let mut bodies: Vec<N> = vec![];
        if let Some(b) = kid(tx, "bodyPr") {
            bodies.push(b);
        }
        if let Some(l) = kid(tx, "lstStyle") {
            containers.push(l);
        }
        if let Some(ph) = ph {
            let (ty, idx) = (at(ph, "type"), at(ph, "idx"));
            for sp in [self.layout.and_then(|l| find_ph(l, ty, idx, true)), self.master.and_then(|m| find_ph(m, ty, idx, false))].into_iter().flatten() {
                if let Some(t) = kid(sp, "txBody") {
                    if let Some(l) = kid(t, "lstStyle") {
                        containers.push(l);
                    }
                    if let Some(b) = kid(t, "bodyPr") {
                        bodies.push(b);
                    }
                }
            }
            if let Some(ts) = self.master.and_then(|m| desc(m, "txStyles")) {
                let key = match norm_ty(ty) {
                    "title" => "titleStyle",
                    "body" => "bodyStyle",
                    _ => "otherStyle",
                };
                if let Some(s) = kid(ts, key) {
                    containers.push(s);
                }
            }
        }
        if let Some(p) = self.pres.and_then(|p| kid(p, "defaultTextStyle")) {
            containers.push(p);
        }
        let dc = style.and_then(|s| kid(s, "fontRef")).and_then(|f| self.clr(f, None)).unwrap_or_else(|| self.tx1());
        self.draw_text(tx, rect, &containers, &bodies, dc);
    }

    fn pic(&mut self, c: N, xf: Xf, skip_ph: bool, rels: &HashMap<String, Rel>) {
        let ph = kid(c, "nvPicPr").and_then(|n| kid(n, "nvPr")).and_then(|n| kid(n, "ph"));
        if ph.is_some() && skip_ph {
            return;
        }
        let rect = match kid(c, "spPr").and_then(|s| kid(s, "xfrm")).and_then(xfrm_vals) {
            Some(v) => xf.rect(v.0, v.1, v.2, v.3),
            None => match ph.and_then(|p| self.inherited_rect(p)) {
                Some(r) => r,
                None => return,
            },
        };
        let Some(bf) = kid(c, "blipFill") else { return };
        let Some(emb) = kid(bf, "blip").and_then(|b| at(b, "embed")) else { return };
        let Some(rel) = rels.get(emb) else { return };
        let Some(bytes) = read_bytes(self.z, &rel.target) else { return };
        let Some(info) = self.pdf.add_image(&rel.target, &bytes) else { return };
        let (x, y, w, h) = rect;
        let sr = kid(bf, "srcRect");
        let g = |k: &str| sr.and_then(|s| fnum(at(s, k))).unwrap_or(0.0).max(0.0) / 100000.0;
        let (l, t, r, b) = (g("l"), g("t"), g("r"), g("b"));
        if l + t + r + b > 0.0 && l + r < 0.95 && t + b < 0.95 {
            let (bw, bh) = (w / (1.0 - l - r), h / (1.0 - t - b));
            self.pdf.clip_rect(x, y, w, h);
            self.pdf.image(info.idx, x - l * bw, y - t * bh, bw, bh);
            self.pdf.restore();
        } else {
            self.pdf.image(info.idx, x, y, w, h);
        }
    }

    fn frame(&mut self, c: N, xf: Xf) {
        let Some(tbl) = desc(c, "tbl") else { return };
        let Some(v) = kid(c, "xfrm").and_then(xfrm_vals) else { return };
        let (x, y, w, _h) = xf.rect(v.0, v.1, v.2, v.3);
        self.table(tbl, x, y, w, xf.sx as f32);
    }

    // ------------------------------------------------------------------------------------------
    fn table(&mut self, tbl: N, x: f32, y: f32, width: f32, _s: f32) {
        let grid: Vec<f32> = kid(tbl, "tblGrid").map(|g| kids(g, "gridCol").iter().filter_map(|c| fnum(at(*c, "w"))).map(|v| v / 12700.0).collect()).unwrap_or_default();
        if grid.is_empty() {
            return;
        }
        let gsum: f32 = grid.iter().sum();
        let k = if gsum > 0.0 { width / gsum } else { 1.0 };
        let cols: Vec<f32> = grid.iter().map(|g| g * k).collect();
        let pr = kid(tbl, "tblPr");
        let default_style = pr.and_then(|p| kid(p, "tableStyleId")).and_then(|s| s.text()).map(|t| t.trim().eq_ignore_ascii_case("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}")).unwrap_or(false);
        let first_row = pr.and_then(|p| at(p, "firstRow")) == Some("1");
        let band_row = pr.and_then(|p| at(p, "bandRow")) == Some("1");
        let accent = self.scheme("accent1").unwrap_or([68, 114, 196]);
        let containers: Vec<N> = self.pres.and_then(|p| kid(p, "defaultTextStyle")).into_iter().collect();
        let rows = kids(tbl, "tr");
        // tinggi baris = max(h, isi)
        let mut heights = Vec::new();
        for tr in &rows {
            let mut rh = fnum(at(*tr, "h")).unwrap_or(0.0) / 12700.0;
            let mut ci = 0;
            for tc in kids(*tr, "tc") {
                let span = at(tc, "gridSpan").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                let w: f32 = cols[ci.min(cols.len() - 1)..(ci + span).min(cols.len())].iter().sum();
                ci += span;
                if matches!(at(tc, "hMerge"), Some("1")) || matches!(at(tc, "vMerge"), Some("1")) {
                    continue;
                }
                if let Some(tx) = kid(tc, "txBody") {
                    let tp = kid(tc, "tcPr");
                    let ins = |k: &str, d: f32| tp.and_then(|t| fnum(at(t, k))).unwrap_or(d) / 12700.0;
                    let th = self.text_height(tx, w - ins("marL", 91440.0) - ins("marR", 91440.0), &containers);
                    rh = rh.max(th + ins("marT", 45720.0) + ins("marB", 45720.0));
                }
            }
            heights.push(rh);
        }
        let mut cy = y;
        for (ri, tr) in rows.iter().enumerate() {
            let mut cx = x;
            let mut ci = 0;
            for tc in kids(*tr, "tc") {
                let span = at(tc, "gridSpan").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                let rspan = at(tc, "rowSpan").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                let w: f32 = cols[ci.min(cols.len() - 1)..(ci + span).min(cols.len())].iter().sum();
                let h: f32 = heights[ri..(ri + rspan).min(heights.len())].iter().sum();
                ci += span;
                let skip = matches!(at(tc, "hMerge"), Some("1")) || matches!(at(tc, "vMerge"), Some("1"));
                if !skip {
                    let tp = kid(tc, "tcPr");
                    let mut fill = match tp.map(|t| self.fill_of(t, None)).unwrap_or(Fill::Unset) {
                        Fill::Solid(c) => Some(c),
                        _ => None,
                    };
                    let mut dc = self.tx1();
                    let mut force_bold = false;
                    if default_style && fill.is_none() && tp.map(|t| matches!(self.fill_of(t, None), Fill::Unset)).unwrap_or(true) {
                        if first_row && ri == 0 {
                            fill = Some(accent);
                            dc = [255, 255, 255];
                            force_bold = true;
                        } else if band_row {
                            let k = if (ri + if first_row { 0 } else { 1 }) % 2 == 1 { 0.8 } else { 0.9 };
                            fill = Some([0, 1, 2].map(|i| (accent[i] as f32 * (1.0 - k) + 255.0 * k) as u8));
                        }
                    }
                    if let Some(f) = fill {
                        self.pdf.fill_rect(cx, cy, w, h, f);
                    }
                    if let Some(t) = tp {
                        for (nm, side) in [("lnL", 0), ("lnR", 1), ("lnT", 2), ("lnB", 3)] {
                            let Some(ln) = kid(t, nm) else { continue };
                            if kid(ln, "noFill").is_some() {
                                continue;
                            }
                            let Some(col) = kid(ln, "solidFill").and_then(|s| self.clr(s, None)) else { continue };
                            let lw = (fnum(at(ln, "w")).unwrap_or(12700.0) / 12700.0).max(0.25);
                            match side {
                                0 => self.pdf.line(cx, cy, cx, cy + h, lw, col),
                                1 => self.pdf.line(cx + w, cy, cx + w, cy + h, lw, col),
                                2 => self.pdf.line(cx, cy, cx + w, cy, lw, col),
                                _ => self.pdf.line(cx, cy + h, cx + w, cy + h, lw, col),
                            }
                        }
                    }
                    if let Some(tx) = kid(tc, "txBody") {
                        let ins = |k: &str, d: f32| tp.and_then(|t| fnum(at(t, k))).unwrap_or(d) / 12700.0;
                        let rect = (cx + ins("marL", 91440.0), cy + ins("marT", 45720.0), w - ins("marL", 91440.0) - ins("marR", 91440.0), h - ins("marT", 45720.0) - ins("marB", 45720.0));
                        let body = [kid(tx, "bodyPr")].into_iter().flatten().collect::<Vec<_>>();
                        self.draw_text_ex(tx, (rect.0 - 7.2, rect.1 - 3.6, rect.2 + 14.4, rect.3 + 7.2), &containers, &body, dc, force_bold, Some((7.2, 3.6, 7.2, 3.6)), tp.and_then(|t| at(t, "anchor")));
                    }
                }
                cx += w;
            }
            cy += heights[ri];
        }
    }

    // ------------------------------------------------------------------------------------------
    fn build_paras(&self, tx: N, width: f32, containers: &[N], fscale: f32, dc: Rgb, force_bold: bool) -> Vec<Para> {
        let mut out = Vec::new();
        let mut auto: HashMap<u32, u32> = HashMap::new();
        for p in kids(tx, "p") {
            let ppr = kid(p, "pPr");
            let lvl = ppr.and_then(|n| at(n, "lvl")).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(8);
            let key = format!("lvl{}pPr", lvl + 1);
            let mut chain: Vec<N> = vec![];
            if let Some(n) = ppr {
                chain.push(n);
            }
            for c in containers {
                if let Some(n) = kid(*c, &key) {
                    chain.push(n);
                }
            }
            let mut para = Para::new(0, 18.0);
            // jalur rPr untuk run: rPr run, lalu defRPr tiap level
            let def_chain: Vec<N> = chain.iter().filter_map(|n| kid(*n, "defRPr")).collect();
            let mk_run = |text: String, rpr: Option<N>| -> Run {
                let mut rc: Vec<N> = vec![];
                if let Some(r) = rpr {
                    rc.push(r);
                }
                rc.extend(def_chain.iter().copied());
                let size = ca(&rc, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0).unwrap_or(18.0) * fscale;
                let flag = |k: &str| matches!(ca(&rc, k), Some("1") | Some("true"));
                let base = ca(&rc, "baseline").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                let size = if base > 0.0 { size * 0.7 } else { size };
                let color = rc.iter().find_map(|n| kid(*n, "solidFill").and_then(|s| self.clr(s, None))).unwrap_or(dc);
                let serif = rc.iter().find_map(|n| kid(*n, "latin")).and_then(|l| at(l, "typeface")).map(|t| {
                    let l = t.to_lowercase();
                    !t.starts_with('+') && !l.contains("sans") && ["times", "georgia", "cambria", "garamond", "palatino", "serif"].iter().any(|k| l.contains(k))
                });
                let text = if ca(&rc, "cap") == Some("all") { text.to_uppercase() } else { text };
                Run {
                    text,
                    face: self.pdf_slot(serif.unwrap_or(false), flag("b") || force_bold, flag("i")),
                    size,
                    color,
                    ul: matches!(ca(&rc, "u"), Some(u) if u != "none"),
                    st: matches!(ca(&rc, "strike"), Some(s) if s != "noStrike"),
                }
            };
            let mut runs: Vec<Run> = vec![];
            for c in p.children().filter(|c| c.is_element()) {
                match name(c) {
                    "r" | "fld" => {
                        let t = kid(c, "t").and_then(|t| t.text()).unwrap_or("").to_string();
                        if !t.is_empty() {
                            runs.push(mk_run(t, kid(c, "rPr")));
                        }
                    }
                    "br" => runs.push(mk_run("\n".into(), kid(c, "rPr"))),
                    _ => {}
                }
            }
            let end = mk_run(String::new(), kid(p, "endParaRPr"));
            para.empty_size = end.size;
            para.empty_face = end.face;
            let has_text = runs.iter().any(|r| !r.text.trim().is_empty());
            let first_run = runs.first().cloned().unwrap_or_else(|| end.clone());
            para.runs = runs;
            para.align = match ca(&chain, "algn") {
                Some("ctr") => Align::Center,
                Some("r") => Align::Right,
                Some("just") | Some("dist") => Align::Justify,
                _ => Align::Left,
            };
            let emu = |k: &str| ca(&chain, k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / 12700.0;
            para.left = emu("marL");
            para.first = emu("indent");
            para.right = emu("marR");
            let spc = |nm: &str| -> f32 {
                match cc(&chain, nm) {
                    Some(n) => {
                        if let Some(v) = kid(n, "spcPts").and_then(|x| fnum(at(x, "val"))) {
                            v / 100.0
                        } else if let Some(v) = kid(n, "spcPct").and_then(|x| fnum(at(x, "val"))) {
                            v / 100000.0 * first_run.size * 1.2
                        } else {
                            0.0
                        }
                    }
                    None => 0.0,
                }
            };
            para.before = spc("spcBef");
            para.after = spc("spcAft");
            if let Some(n) = cc(&chain, "lnSpc") {
                if let Some(v) = kid(n, "spcPct").and_then(|x| fnum(at(x, "val"))) {
                    para.mult = v / 100000.0;
                } else if let Some(v) = kid(n, "spcPts").and_then(|x| fnum(at(x, "val"))) {
                    para.exact = Some(v / 100.0);
                }
            }
            // bullet
            let bu = chain.iter().find(|n| kid(**n, "buNone").is_some() || kid(**n, "buChar").is_some() || kid(**n, "buAutoNum").is_some());
            let mut label = None;
            if let (Some(b), true) = (bu, has_text) {
                if let Some(ch) = kid(*b, "buChar") {
                    let wing = kid(*b, "buFont").and_then(|f| at(f, "typeface")).map(|t| t.starts_with("Wingdings") || t == "Symbol").unwrap_or(false);
                    let c = at(ch, "char").unwrap_or("•");
                    label = Some(if wing || c.chars().next().map(|x| (x as u32) >= 0xF000).unwrap_or(false) { "•".to_string() } else { c.to_string() });
                    auto.clear();
                } else if let Some(an) = kid(*b, "buAutoNum") {
                    let start = at(an, "startAt").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
                    let cnt = auto.entry(lvl).and_modify(|c| *c += 1).or_insert(start);
                    let n = *cnt;
                    auto.retain(|k, _| *k <= lvl);
                    let ty = at(an, "type").unwrap_or("arabicPeriod");
                    let body = if ty.starts_with("alphaLc") { letters(n, false) } else if ty.starts_with("alphaUc") { letters(n, true) } else if ty.starts_with("romanLc") { roman(n, false) } else if ty.starts_with("romanUc") { roman(n, true) } else { n.to_string() };
                    label = Some(if ty.ends_with("ParenR") { format!("{body})") } else if ty.ends_with("ParenBoth") { format!("({body})") } else { format!("{body}.") });
                } else {
                    auto.clear();
                }
            } else if !has_text {
                // paragraf kosong tidak memutus penomoran
            } else {
                auto.clear();
            }
            if let Some(t) = label {
                let mut run = first_run.clone();
                run.text = String::new();
                run.ul = false;
                run.st = false;
                if para.left == 0.0 && para.first == 0.0 {
                    para.left = run.size * 1.0;
                    para.first = -run.size * 1.0;
                }
                para.label = Some(Label { text: t, run });
            }
            let _ = width;
            out.push(para);
        }
        out
    }

    fn pdf_slot(&self, serif: bool, bold: bool, italic: bool) -> usize {
        self.pdf.fonts.slot(serif, bold, italic)
    }

    fn text_height(&mut self, tx: N, width: f32, containers: &[N]) -> f32 {
        let dc = self.tx1();
        let paras = self.build_paras(tx, width, containers, 1.0, dc, false);
        let mut h = 0.0;
        for (i, p) in paras.iter().enumerate() {
            if i > 0 {
                h += p.before;
            }
            h += layout(&self.pdf.fonts, p, width.max(10.0)).iter().map(|l| l.h).sum::<f32>() + p.after;
        }
        h
    }

    fn draw_text(&mut self, tx: N, rect: Rect, containers: &[N], bodies: &[N], dc: Rgb) {
        self.draw_text_ex(tx, rect, containers, bodies, dc, false, None, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_ex(&mut self, tx: N, rect: Rect, containers: &[N], bodies: &[N], dc: Rgb, force_bold: bool, ins_override: Option<(f32, f32, f32, f32)>, anchor_override: Option<&str>) {
        let (x, y, w, h) = rect;
        let e = |k: &str, d: f32| ca(bodies, k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d) / 12700.0;
        let (li, ti, ri, bi) = ins_override.unwrap_or((e("lIns", 91440.0), e("tIns", 45720.0), e("rIns", 91440.0), e("bIns", 45720.0)));
        let anchor = anchor_override.or_else(|| ca(bodies, "anchor")).unwrap_or("t");
        let nowrap = ca(bodies, "wrap") == Some("none");
        let fscale = bodies.first().and_then(|b| desc(*b, "normAutofit")).and_then(|n| fnum(at(n, "fontScale"))).map(|v| v / 100000.0).unwrap_or(1.0);
        let inner_w = if nowrap { 100000.0 } else { (w - li - ri).max(10.0) };
        let paras = self.build_paras(tx, inner_w, containers, fscale, dc, force_bold);
        let mut laid = Vec::new();
        let mut total = 0.0;
        for (i, p) in paras.iter().enumerate() {
            let lines = layout(&self.pdf.fonts, p, inner_w);
            let before = if i > 0 { p.before } else { 0.0 };
            total += before + lines.iter().map(|l| l.h).sum::<f32>() + p.after;
            laid.push((before, lines, p.align, p.after));
        }
        let avail_h = h - ti - bi;
        let mut cy = match anchor {
            "ctr" => y + ti + (avail_h - total) / 2.0,
            "b" => y + h - bi - total,
            _ => y + ti,
        };
        for (before, lines, align, after) in laid {
            cy += before;
            for l in &lines {
                // kotak tanpa bungkus: perataan relatif lebar kotak
                if nowrap {
                    let off = match align {
                        Align::Center => (w - li - ri - l.w) / 2.0,
                        Align::Right => w - li - ri - l.w,
                        _ => 0.0,
                    };
                    draw_line(self.pdf, l, Align::Left, x + li + off, cy);
                } else {
                    draw_line(self.pdf, l, align, x + li, cy);
                }
                cy += l.h;
            }
            cy += after;
        }
    }
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

// ------------------------------------------------------------------------------------------------
fn theme_map(xml: &str) -> HashMap<String, Rgb> {
    let mut m = HashMap::new();
    if let Ok(d) = roxmltree::Document::parse(xml) {
        if let Some(cs) = desc(d.root_element(), "clrScheme") {
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
        }
    }
    m
}

fn bg_color(r: &R, root: Option<N>) -> Option<Rgb> {
    let bg = desc(root?, "bg")?;
    if let Some(pr) = kid(bg, "bgPr") {
        if let Fill::Solid(c) = r.fill_of(pr, None) {
            return Some(c);
        }
        return None;
    }
    kid(bg, "bgRef").and_then(|b| r.clr(b, None))
}

pub fn convert(src: &[u8], mut pdf: Pdf) -> Result<Vec<u8>, String> {
    let mut z = open_zip(src)?;
    let pres_path = "ppt/presentation.xml";
    let pres_xml = read_text(&mut z, pres_path).ok_or("ppt/presentation.xml tidak ditemukan")?;
    let pres_rels = rels_of(&mut z, pres_path);
    let pdoc = parse(&pres_xml)?;
    let proot = pdoc.root_element();
    let (sw, sh) = desc(proot, "sldSz").map(|s| (fnum(at(s, "cx")).unwrap_or(9144000.0) / 12700.0, fnum(at(s, "cy")).unwrap_or(5143500.0) / 12700.0)).unwrap_or((720.0, 405.0));
    let slides: Vec<String> = desc(proot, "sldIdLst")
        .map(|l| kids(l, "sldId").into_iter().filter_map(|s| rid(s).and_then(|id| pres_rels.get(id)).map(|r| r.target.clone())).collect())
        .unwrap_or_default();
    if slides.is_empty() {
        return Err("Presentasi tidak punya slide".into());
    }

    for sp in slides {
        if pdf.page_count() >= 500 {
            break;
        }
        let Some(sxml) = read_text(&mut z, &sp) else { continue };
        let srels = rels_of(&mut z, &sp);
        let lay = srels.values().find(|r| r.ty.ends_with("/slideLayout")).cloned();
        let lxml = lay.as_ref().and_then(|l| read_text(&mut z, &l.target));
        let lrels = lay.as_ref().map(|l| rels_of(&mut z, &l.target)).unwrap_or_default();
        let mas = lrels.values().find(|r| r.ty.ends_with("/slideMaster")).cloned();
        let mxml = mas.as_ref().and_then(|m| read_text(&mut z, &m.target));
        let mrels = mas.as_ref().map(|m| rels_of(&mut z, &m.target)).unwrap_or_default();
        let theme = mrels.values().find(|r| r.ty.ends_with("/theme")).and_then(|t| read_text(&mut z, &t.target)).map(|t| theme_map(&t)).unwrap_or_default();

        let Ok(sd) = roxmltree::Document::parse(&sxml) else { continue };
        let ld = lxml.as_deref().and_then(|x| roxmltree::Document::parse(x).ok());
        let md = mxml.as_deref().and_then(|x| roxmltree::Document::parse(x).ok());
        if at(sd.root_element(), "show") == Some("0") {
            continue;
        }
        let mut clrmap: HashMap<String, String> = HashMap::new();
        if let Some(cm) = md.as_ref().and_then(|m| desc(m.root_element(), "clrMap")) {
            for a in cm.attributes() {
                clrmap.insert(a.name().to_string(), a.value().to_string());
            }
        } else {
            for (k, v) in [("bg1", "lt1"), ("tx1", "dk1"), ("bg2", "lt2"), ("tx2", "dk2")] {
                clrmap.insert(k.into(), v.into());
            }
        }
        let mut theme = theme;
        for (k, v) in [("lt1", [255, 255, 255]), ("dk1", [0, 0, 0])] {
            theme.entry(k.to_string()).or_insert(v);
        }
        let mut r = R {
            pdf: &mut pdf,
            z: &mut z,
            theme,
            clrmap,
            layout: ld.as_ref().map(|d| d.root_element()),
            master: md.as_ref().map(|d| d.root_element()),
            pres: Some(proot),
            bg_default: [255, 255, 255],
        };
        r.pdf.new_page(sw, sh);
        let bg = bg_color(&r, Some(sd.root_element())).or_else(|| bg_color(&r, r.layout)).or_else(|| bg_color(&r, r.master)).or_else(|| r.scheme("bg1")).unwrap_or(r.bg_default);
        if bg != [255, 255, 255] {
            r.pdf.fill_rect(0.0, 0.0, sw, sh, bg);
        }
        let hide_master = at(sd.root_element(), "showMasterSp") == Some("0");
        let layout_hides = r.layout.map(|l| at(l, "showMasterSp") == Some("0")).unwrap_or(false);
        if !hide_master && !layout_hides {
            if let Some(t) = r.master.and_then(|m| desc(m, "spTree")) {
                r.draw_tree(t, Xf::id(), true, &mrels);
            }
        }
        if !hide_master {
            if let Some(t) = r.layout.and_then(|l| desc(l, "spTree")) {
                r.draw_tree(t, Xf::id(), true, &lrels);
            }
        }
        if let Some(t) = desc(sd.root_element(), "spTree") {
            r.draw_tree(t, Xf::id(), false, &srels);
        }
    }
    pdf.finish()
}
