//! Model teks + tata letak: pemotongan baris, perataan, tab, bullet; aliran halaman (Flow) dengan
//! paragraf, gambar, dan tabel. Dipakai bersama oleh DOCX, XLSX, dan PPTX.
use super::font::Fonts;
use super::pdf::Pdf;
use super::util::Rgb;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone)]
pub struct Run {
    pub text: String,
    pub face: usize,
    pub size: f32,
    pub color: Rgb,
    pub ul: bool,
    pub st: bool,
}

#[derive(Clone)]
pub struct Label {
    pub text: String,
    pub run: Run,
}

#[derive(Clone)]
pub struct Para {
    pub runs: Vec<Run>,
    pub align: Align,
    pub left: f32,
    pub first: f32,
    pub right: f32,
    pub before: f32,
    pub after: f32,
    pub mult: f32,
    pub exact: Option<f32>,
    pub label: Option<Label>,
    pub break_before: bool,
    pub empty_size: f32,
    pub empty_face: usize,
}

impl Para {
    pub fn new(face: usize, size: f32) -> Para {
        Para {
            runs: vec![],
            align: Align::Left,
            left: 0.0,
            first: 0.0,
            right: 0.0,
            before: 0.0,
            after: 0.0,
            mult: 1.0,
            exact: None,
            label: None,
            break_before: false,
            empty_size: size,
            empty_face: face,
        }
    }
}

pub struct Seg {
    pub run: Run,
    pub x: f32,
    pub w: f32,
    pub sp: usize,
}

pub struct Line {
    pub segs: Vec<Seg>,
    pub w: f32,
    pub h: f32,
    pub base: f32,
    pub x0: f32,
    pub avail: f32,
    pub label: Option<(Label, f32)>,
    pub nsp: usize,
    pub justify: bool,
}

enum Tk {
    Word(usize, String, f32),
    Space(usize, f32, String),
    Tab,
    Nl,
}

fn fallback_runs(fonts: &Fonts, runs: &[Run]) -> Vec<Run> {
    let mut resolved = Vec::new();
    for run in runs {
        let mut active_face = None;
        let mut text = String::new();
        for ch in run.text.chars() {
            let face = fonts.fallback_face(run.face, ch);
            if active_face.map(|active| active != face).unwrap_or(false) && !text.is_empty() {
                let mut part = run.clone();
                part.face = active_face.unwrap();
                part.text = std::mem::take(&mut text);
                resolved.push(part);
            }
            active_face = Some(face);
            text.push(ch);
        }
        if !text.is_empty() {
            let mut part = run.clone();
            part.face = active_face.unwrap_or(run.face);
            part.text = text;
            resolved.push(part);
        }
    }
    resolved
}

fn tokenize(fonts: &Fonts, runs: &[Run]) -> Vec<Tk> {
    let mut out = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        let mut word = String::new();
        let mut spaces = 0.0f32;
        let mut sp_s = String::new();
        let flush_word = |out: &mut Vec<Tk>, word: &mut String| {
            if !word.is_empty() {
                let w = fonts.text_w(r.face, word, r.size);
                out.push(Tk::Word(ri, std::mem::take(word), w));
            }
        };
        for c in r.text.chars() {
            match c {
                '\t' | '\n' => {
                    flush_word(&mut out, &mut word);
                    if spaces > 0.0 {
                        out.push(Tk::Space(ri, spaces, std::mem::take(&mut sp_s)));
                        spaces = 0.0;
                    }
                    out.push(if c == '\t' { Tk::Tab } else { Tk::Nl });
                }
                c if c.is_whitespace() => {
                    flush_word(&mut out, &mut word);
                    spaces += fonts.char_w(r.face, ' ', r.size);
                    sp_s.push(' ');
                }
                c => {
                    if spaces > 0.0 {
                        out.push(Tk::Space(ri, spaces, std::mem::take(&mut sp_s)));
                        spaces = 0.0;
                    }
                    word.push(c);
                }
            }
        }
        flush_word(&mut out, &mut word);
        if spaces > 0.0 {
            out.push(Tk::Space(ri, spaces, std::mem::take(&mut sp_s)));
        }
    }
    out
}

struct Build {
    segs: Vec<Seg>,
    x: f32,
    end_w: f32,
    has_word: bool,
    soft: bool,
    last_ri: usize,
    nsp: usize,
    sp_pending: usize,
    tab: bool,
}
impl Build {
    fn new(soft: bool) -> Build {
        Build { segs: vec![], x: 0.0, end_w: 0.0, has_word: false, soft, last_ri: usize::MAX, nsp: 0, sp_pending: 0, tab: false }
    }
}

pub fn layout(fonts: &Fonts, p: &Para, width: f32) -> Vec<Line> {
    // posisi awal teks & label (bullet)
    let (first_x, label) = match &p.label {
        Some(l) => {
            let mut label = l.clone();
            if let Some(ch) = l.text.chars().find(|c| !c.is_whitespace()) {
                label.run.face = fonts.fallback_face(l.run.face, ch);
            }
            let lw = fonts.text_w(label.run.face, &label.text, label.run.size);
            if p.first < 0.0 {
                (p.left, Some((label, (p.left + p.first).max(0.0))))
            } else {
                let lx = p.left + p.first;
                (lx + lw + 4.0, Some((label, lx)))
            }
        }
        None => ((p.left + p.first).max(0.0), None),
    };
    let rest_x = p.left.max(0.0);
    let avail_first = (width - first_x - p.right).max(8.0);
    let avail_rest = (width - rest_x - p.right).max(8.0);
    let justify = p.align == Align::Justify;

    let runs = fallback_runs(fonts, &p.runs);
    let toks = tokenize(fonts, &runs);
    let mut lines: Vec<Line> = Vec::new();
    let mut cur = Build::new(false);

    let finish = |cur: &mut Build, lines: &mut Vec<Line>, forced: bool, label: &mut Option<(Label, f32)>| {
        let first = lines.is_empty();
        let (mut hmax, mut amax) = (0.0f32, 0.0f32);
        for s in &cur.segs {
            hmax = hmax.max(s.run.size * fonts.line_h(s.run.face));
            amax = amax.max(s.run.size * fonts.ascent(s.run.face));
        }
        if cur.segs.is_empty() {
            hmax = p.empty_size * fonts.line_h(p.empty_face);
            amax = p.empty_size * fonts.ascent(p.empty_face);
        }
        let h = p.exact.unwrap_or(hmax * p.mult);
        let base = (h - hmax) + amax;
        lines.push(Line {
            segs: std::mem::take(&mut cur.segs),
            w: cur.end_w,
            h,
            base,
            x0: if first { first_x } else { rest_x },
            avail: if first { avail_first } else { avail_rest },
            label: if first { label.take() } else { None },
            nsp: cur.nsp,
            justify: justify && !forced && !cur.tab,
        });
        *cur = Build::new(true);
    };
    let mut label = label;

    for t in toks {
        let first = lines.is_empty();
        let avail = if first { avail_first } else { avail_rest };
        let x0 = if first { first_x } else { rest_x };
        match t {
            Tk::Nl => finish(&mut cur, &mut lines, true, &mut label),
            Tk::Tab => {
                let abs = x0 + cur.x;
                let stop = ((abs / 36.0).floor() + 1.0) * 36.0;
                cur.x += (stop - abs).max(2.0);
                cur.has_word = true;
                cur.tab = true;
            }
            Tk::Space(ri, w, txt) => {
                if cur.soft && cur.segs.is_empty() {
                    continue;
                }
                if !justify && cur.last_ri == ri {
                    if let Some(l) = cur.segs.last_mut() {
                        if (cur.x - (l.x + l.w)).abs() < 0.01 {
                            l.run.text.push_str(&txt);
                            l.w += w;
                        }
                    }
                }
                cur.x += w;
                if cur.has_word {
                    cur.nsp += 1;
                    cur.sp_pending += 1;
                }
            }
            Tk::Word(ri, s, w) => {
                if cur.x + w > avail + 0.01 && cur.has_word && !cur.segs.is_empty() {
                    finish(&mut cur, &mut lines, false, &mut label);
                }
                let first = lines.is_empty();
                let avail = if first { avail_first } else { avail_rest };
                {
                    // kata lebih panjang dari baris: pecah per karakter
                    let r = &runs[ri];
                    if w > avail && cur.x + w > avail {
                        let mut piece = String::new();
                        let mut pw = 0.0;
                        for c in s.chars() {
                            let cw = fonts.char_w(r.face, c, r.size);
                            if cur.x + pw + cw > avail && !(piece.is_empty() && cur.segs.is_empty()) {
                                push_seg(&mut cur, &runs, ri, &piece, pw, justify);
                                finish(&mut cur, &mut lines, false, &mut label);
                                piece.clear();
                                pw = 0.0;
                            }
                            piece.push(c);
                            pw += cw;
                        }
                        if !piece.is_empty() {
                            push_seg(&mut cur, &runs, ri, &piece, pw, justify);
                        }
                    } else {
                        push_seg(&mut cur, &runs, ri, &s, w, justify);
                    }
                }
            }
        }
    }
    finish(&mut cur, &mut lines, true, &mut label);
    lines
}

fn push_seg(cur: &mut Build, runs: &[Run], ri: usize, text: &str, w: f32, justify: bool) {
    // gabung dengan segmen sebelumnya bila gaya sama dan bersambung (hindari ribuan Tj)
    let merge = !justify
        && cur.last_ri == ri
        && cur.segs.last().map(|l| (cur.x - (l.x + l.w)).abs() < 0.01).unwrap_or(false);
    if merge {
        let l = cur.segs.last_mut().unwrap();
        l.run.text.push_str(text);
        l.w += w;
    } else {
        let mut run = runs[ri].clone();
        run.text = text.to_string();
        cur.segs.push(Seg { run, x: cur.x, w, sp: cur.sp_pending });
    }
    cur.last_ri = ri;
    cur.x += w;
    cur.end_w = cur.x;
    cur.has_word = true;
    cur.soft = false;
}

pub fn draw_line(pdf: &mut Pdf, l: &Line, align: Align, x: f32, top: f32) {
    let off = match align {
        Align::Center => ((l.avail - l.w) / 2.0).max(0.0),
        Align::Right => (l.avail - l.w).max(0.0),
        _ => 0.0,
    };
    let extra = if l.justify && l.nsp > 0 && l.w < l.avail { (l.avail - l.w) / l.nsp as f32 } else { 0.0 };
    let base = top + l.base;
    if let Some((lab, lx)) = &l.label {
        pdf.text(x + lx, base, lab.run.face, lab.run.size, lab.run.color, &lab.text);
    }
    for s in &l.segs {
        let sx = x + l.x0 + off + s.x + s.sp as f32 * extra;
        let w = s.w + if extra > 0.0 { 0.0 } else { 0.0 };
        pdf.text(sx, base, s.run.face, s.run.size, s.run.color, &s.run.text);
        if s.run.ul {
            pdf.fill_rect(sx, base + s.run.size * 0.11, w, (s.run.size * 0.055).max(0.4), s.run.color);
        }
        if s.run.st {
            pdf.fill_rect(sx, base - s.run.size * 0.3, w, (s.run.size * 0.055).max(0.4), s.run.color);
        }
    }
}

// =================================================================================================
// Blok dokumen
// =================================================================================================
#[derive(Clone)]
pub struct ImgBlock {
    pub idx: usize,
    pub w: f32,
    pub h: f32,
    pub align: Align,
}

#[derive(Clone)]
pub enum Block {
    Para(Para),
    Image(ImgBlock),
    Table(Table),
    PageBreak,
    Section(PageSetup),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SectionBreak {
    None,
    Continuous,
    NextPage,
    OddPage,
    EvenPage,
}

#[derive(Clone, Copy)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub break_type: SectionBreak,
}

#[derive(Clone)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub span: usize,
    pub fill: Option<Rgb>,
    pub covered: bool,
}
#[derive(Clone)]
pub struct Table {
    pub cols: Vec<f32>,
    pub rows: Vec<Vec<Cell>>,
    pub borders: bool,
}

pub const CELL_PAD_X: f32 = 5.0;
pub const CELL_PAD_Y: f32 = 2.5;

fn img_size(b: &ImgBlock, width: f32) -> (f32, f32) {
    if !b.w.is_finite() || !b.h.is_finite() || !width.is_finite()
        || b.w <= 0.0 || b.h <= 0.0 || width <= 0.0
    {
        return (0.0, 0.0);
    }
    if b.w > width {
        (width, b.h * width / b.w)
    } else {
        (b.w, b.h)
    }
}

pub fn blocks_height(fonts: &Fonts, blocks: &[Block], width: f32) -> f32 {
    let mut h = 0.0;
    for b in blocks {
        match b {
            Block::Para(p) => h += p.before + layout(fonts, p, width).iter().map(|l| l.h).sum::<f32>() + p.after,
            Block::Image(i) => h += img_size(i, width).1,
            _ => {}
        }
    }
    h
}

pub fn draw_blocks(pdf: &mut Pdf, blocks: &[Block], x: f32, mut y: f32, width: f32) {
    for b in blocks {
        match b {
            Block::Para(p) => {
                y += p.before;
                let lines = layout(&pdf.fonts, p, width);
                for l in &lines {
                    draw_line(pdf, l, p.align, x, y);
                    y += l.h;
                }
                y += p.after;
            }
            Block::Image(i) => {
                let (w, h) = img_size(i, width);
                let off = match i.align {
                    Align::Center => (width - w) / 2.0,
                    Align::Right => width - w,
                    _ => 0.0,
                };
                pdf.image(i.idx, x + off.max(0.0), y, w, h);
                y += h;
            }
            _ => {}
        }
    }
}

// =================================================================================================
// Flow: aliran halaman
// =================================================================================================
fn section_page_size_changed(old_width: f32, old_height: f32, new_width: f32, new_height: f32) -> bool {
    (old_width - new_width).abs() > 0.01 || (old_height - new_height).abs() > 0.01
}

fn section_needs_blank_page(page_number: usize, break_type: SectionBreak) -> bool {
    match break_type {
        SectionBreak::OddPage => page_number % 2 == 0,
        SectionBreak::EvenPage => page_number % 2 == 1,
        _ => false,
    }
}

pub struct Flow {
    pub pw: f32,
    pub ph: f32,
    pub ml: f32,
    pub mr: f32,
    pub mt: f32,
    pub mb: f32,
    pub y: f32,
    pub open: bool,
    pub max_pages: usize,
}

impl Flow {
    pub fn new(pw: f32, ph: f32, ml: f32, mr: f32, mt: f32, mb: f32) -> Flow {
        Flow { pw, ph, ml, mr, mt, mb, y: mt, open: false, max_pages: 600 }
    }
    pub fn avail(&self) -> f32 {
        self.pw - self.ml - self.mr
    }
    pub fn bottom(&self) -> f32 {
        self.ph - self.mb
    }
    pub fn full(&self) -> bool {
        false
    }
    pub fn ensure(&mut self, pdf: &mut Pdf) {
        if !self.open {
            self.page_break(pdf);
        }
    }
    pub fn page_break(&mut self, pdf: &mut Pdf) {
        if pdf.page_count() >= self.max_pages {
            self.open = false;
            return;
        }
        pdf.new_page(self.pw, self.ph);
        self.y = self.mt;
        self.open = true;
    }
    pub fn at_top(&self) -> bool {
        self.y <= self.mt + 0.1
    }

    pub fn block(&mut self, pdf: &mut Pdf, b: &Block) {
        match b {
            Block::Para(p) => self.para(pdf, p),
            Block::Image(i) => self.image(pdf, i),
            Block::Table(t) => self.table(pdf, t),
            Block::PageBreak => self.page_break(pdf),
            Block::Section(setup) => self.section(pdf, *setup),
        }
    }

    pub fn section(&mut self, pdf: &mut Pdf, setup: PageSetup) {
        let size_changed = section_page_size_changed(self.pw, self.ph, setup.width, setup.height);
        self.pw = setup.width;
        self.ph = setup.height;
        self.ml = setup.left;
        self.mr = setup.right;
        self.mt = setup.top;
        self.mb = setup.bottom;
        match setup.break_type {
            SectionBreak::NextPage => self.page_break(pdf),
            SectionBreak::OddPage => {
                self.page_break(pdf);
                // Page numbers are one-based. If the new page is even, insert a blank page.
                if self.open && section_needs_blank_page(pdf.page_count(), SectionBreak::OddPage) {
                    self.page_break(pdf);
                }
            }
            SectionBreak::EvenPage => {
                self.page_break(pdf);
                // If the new page is odd, insert a blank page before the section starts.
                if self.open && section_needs_blank_page(pdf.page_count(), SectionBreak::EvenPage) {
                    self.page_break(pdf);
                }
            }
            SectionBreak::Continuous => {
                // A PDF page cannot change its MediaBox midway through the page.
                // Word also promotes a continuous section to a new page when its
                // physical page size/orientation changes.
                if self.open && size_changed {
                    self.page_break(pdf);
                } else if !self.open {
                    self.y = self.mt;
                    self.ensure(pdf);
                }
            }
            SectionBreak::None => {
                if !self.open {
                    self.y = self.mt;
                    self.ensure(pdf);
                }
            }
        }
    }

    pub fn para(&mut self, pdf: &mut Pdf, p: &Para) {
        if p.break_before {
            self.page_break(pdf);
        } else {
            self.ensure(pdf);
        }
        if !self.open {
            return;
        }
        if !self.at_top() {
            self.y += p.before;
        }
        let lines = layout(&pdf.fonts, p, self.avail());
        for l in &lines {
            if self.y + l.h > self.bottom() + 0.5 && !self.at_top() {
                self.page_break(pdf);
                if !self.open {
                    return;
                }
            }
            draw_line(pdf, l, p.align, self.ml, self.y);
            self.y += l.h;
        }
        self.y += p.after;
    }

    pub fn image(&mut self, pdf: &mut Pdf, i: &ImgBlock) {
        self.ensure(pdf);
        if !self.open {
            return;
        }
        let avail = self.avail();
        let (mut w, mut h) = img_size(i, avail);
        let maxh = self.bottom() - self.mt;
        if h > maxh {
            w *= maxh / h;
            h = maxh;
        }
        if self.y + h > self.bottom() + 0.5 && !self.at_top() {
            self.page_break(pdf);
            if !self.open {
                return;
            }
        }
        let off = match i.align {
            Align::Center => (avail - w) / 2.0,
            Align::Right => avail - w,
            _ => 0.0,
        };
        pdf.image(i.idx, self.ml + off.max(0.0), self.y, w, h);
        self.y += h;
    }

    pub fn table(&mut self, pdf: &mut Pdf, t: &Table) {
        self.ensure(pdf);
        if !self.open {
            return;
        }
        let avail = self.avail();
        let ncols = t.rows.iter().map(|r| r.iter().map(|c| c.span.max(1)).sum::<usize>()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let mut cols: Vec<f32> = t.cols.clone();
        if cols.len() < ncols {
            let known: f32 = cols.iter().sum();
            let rest = (avail - known).max(20.0 * (ncols - cols.len()) as f32);
            let each = rest / (ncols - cols.len()) as f32;
            cols.resize(ncols, each);
        }
        let sum: f32 = cols.iter().sum();
        if sum > avail {
            let s = avail / sum;
            cols.iter_mut().for_each(|c| *c *= s);
        }
        let line_c: Rgb = [0x80, 0x80, 0x80];
        for row in &t.rows {
            // lebar tiap sel & tinggi baris
            let mut widths = Vec::new();
            let mut ci = 0;
            for c in row {
                let sp = c.span.max(1);
                widths.push(cols[ci.min(cols.len() - 1)..(ci + sp).min(cols.len())].iter().sum::<f32>());
                ci += sp;
            }
            let mut rh = 14.0f32;
            for (c, w) in row.iter().zip(&widths) {
                if c.covered {
                    continue;
                }
                rh = rh.max(blocks_height(&pdf.fonts, &c.blocks, (w - 2.0 * CELL_PAD_X).max(10.0)) + 2.0 * CELL_PAD_Y);
            }
            if self.y + rh > self.bottom() + 0.5 && !self.at_top() {
                self.page_break(pdf);
                if !self.open {
                    return;
                }
            }
            let mut x = self.ml;
            for (c, w) in row.iter().zip(&widths) {
                if let Some(f) = c.fill {
                    pdf.fill_rect(x, self.y, *w, rh, f);
                }
                if t.borders {
                    pdf.stroke_rect(x, self.y, *w, rh, 0.5, line_c);
                }
                if !c.covered {
                    draw_blocks(pdf, &c.blocks, x + CELL_PAD_X, self.y + CELL_PAD_Y, (w - 2.0 * CELL_PAD_X).max(10.0));
                }
                x += w;
            }
            self.y += rh;
        }
    }
}


#[cfg(test)]
mod image_layout_tests {
    use super::{img_size, Align, ImgBlock};

    #[test]
    fn image_dimensions_are_scaled_to_available_width() {
        let image = ImgBlock { idx: 0, w: 200.0, h: 100.0, align: Align::Left };
        assert_eq!(img_size(&image, 100.0), (100.0, 50.0));
    }

    #[test]
    fn invalid_image_dimensions_are_ignored() {
        let invalid = [
            ImgBlock { idx: 0, w: f32::NAN, h: 10.0, align: Align::Left },
            ImgBlock { idx: 0, w: 10.0, h: f32::INFINITY, align: Align::Left },
            ImgBlock { idx: 0, w: -1.0, h: 10.0, align: Align::Left },
        ];
        for image in invalid {
            assert_eq!(img_size(&image, 100.0), (0.0, 0.0));
        }
    }
}


#[cfg(test)]
mod section_break_tests {
    use super::{section_needs_blank_page, SectionBreak};

    #[test]
    fn odd_page_section_inserts_blank_only_when_next_page_is_even() {
        assert!(section_needs_blank_page(2, SectionBreak::OddPage));
        assert!(!section_needs_blank_page(3, SectionBreak::OddPage));
    }

    #[test]
    fn even_page_section_inserts_blank_only_when_next_page_is_odd() {
        assert!(section_needs_blank_page(1, SectionBreak::EvenPage));
        assert!(!section_needs_blank_page(2, SectionBreak::EvenPage));
    }

    #[test]
    fn regular_and_continuous_sections_do_not_insert_parity_pages() {
        assert!(!section_needs_blank_page(1, SectionBreak::NextPage));
        assert!(!section_needs_blank_page(2, SectionBreak::Continuous));
    }
}


#[cfg(test)]
mod section_page_size_tests {
    use super::section_page_size_changed;

    #[test]
    fn detects_physical_page_size_changes() {
        assert!(section_page_size_changed(595.0, 842.0, 842.0, 595.0));
        assert!(!section_page_size_changed(595.0, 842.0, 595.0, 842.0));
        assert!(!section_page_size_changed(595.0, 842.0, 595.005, 842.005));
    }
}
