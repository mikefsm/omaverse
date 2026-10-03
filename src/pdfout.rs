//! Printing. Each of the three document kinds goes onto paper in its own way:
//! an outline is flowing prose, an interlinear is a grid of words, and a
//! diagram is a drawing that has to fit the sheet.
//!
//! Everything is drawn through cairo and pango, the same pair that draws the
//! diagram on screen, so what prints is what was seen.

use crate::diagram::{self, Diagram};
use crate::docview::{self, Line};
use crate::interlinear::Interlinear;
use gtk4::cairo;
use cairo::{Context, PdfSurface};
use gtk4::pango;
use pango::FontDescription;
use std::path::Path;

/// Three quarters of an inch all round.
const MARGIN: f64 = 54.0;
const BODY_FONT: &str = "SBL BibLit, Source Serif 4, Noto Serif, serif";
const WORD_FONT: &str = "SBL BibLit, SBL Greek, SBL Hebrew, serif";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Letter,
    A4,
    Legal,
}

impl Paper {
    /// Width and height in points, upright.
    fn upright(self) -> (f64, f64) {
        match self {
            Paper::Letter => (612.0, 792.0),
            Paper::A4 => (595.28, 841.89),
            Paper::Legal => (612.0, 1008.0),
        }
    }

    pub fn all() -> [Paper; 3] {
        [Paper::Letter, Paper::A4, Paper::Legal]
    }

    pub fn label(self) -> &'static str {
        match self {
            Paper::Letter => "US Letter",
            Paper::A4 => "A4",
            Paper::Legal => "US Legal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn all() -> [Orientation; 2] {
        [Orientation::Portrait, Orientation::Landscape]
    }

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
    }
}

/// The sheet being printed on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
}

impl Sheet {
    pub fn new(paper: Paper, orientation: Orientation) -> Sheet {
        let (w, h) = paper.upright();
        match orientation {
            Orientation::Portrait => Sheet { width: w, height: h },
            Orientation::Landscape => Sheet { width: h, height: w },
        }
    }

    fn content_width(&self) -> f64 {
        self.width - MARGIN * 2.0
    }

    fn content_height(&self) -> f64 {
        self.height - MARGIN * 2.0
    }
}

/// How many sheets came out, which is worth telling the user and is the only
/// honest way to check pagination: a PDF's own page objects are compressed.
type Done = Result<usize, cairo::Error>;

fn surface(path: &Path, sheet: Sheet) -> Result<PdfSurface, cairo::Error> {
    PdfSurface::new(sheet.width, sheet.height, path).map_err(cairo::Error::from)
}

fn layout(cr: &Context, text: &str, font: &str, size: f64, width: Option<f64>) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut desc = FontDescription::from_string(font);
    desc.set_size((size * pango::SCALE as f64) as i32);
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    if let Some(w) = width {
        layout.set_width((w * pango::SCALE as f64) as i32);
        layout.set_wrap(pango::WrapMode::WordChar);
    }
    layout
}

fn height_of(layout: &pango::Layout) -> f64 {
    layout.pixel_size().1 as f64
}

/// An ordinary Markdown list item, as against prose.
fn is_bullet(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")
}

/// A dash reads as a dash on screen, where the file is in front of you, but on
/// paper it should look like the list it is.
fn bulleted(line: &str) -> String {
    match line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        Some(rest) => format!("\u{2022}\u{2003}{rest}"),
        None => line.to_string(),
    }
}

/// The running foot: which page this is.
fn number_page(cr: &Context, sheet: Sheet, page: usize) {
    let text = format!("{page}");
    let l = layout(cr, &text, BODY_FONT, 8.5, None);
    let w = l.pixel_size().0 as f64;
    cr.set_source_rgb(0.45, 0.45, 0.45);
    cr.move_to((sheet.width - w) / 2.0, sheet.height - MARGIN * 0.62);
    pangocairo::functions::show_layout(cr, &l);
    cr.set_source_rgb(0.0, 0.0, 0.0);
}

// ---- outlines ---------------------------------------------------------------

/// An outline prints as what it is: headings and the prose under them. Folding
/// is a reading convenience and has no bearing on the page, so the caller hands
/// over the whole text.
pub fn export_outline(path: &Path, text: &str, title: &str, sheet: Sheet) -> Done {
    let surface = surface(path, sheet)?;
    let cr = Context::new(&surface)?;
    cr.set_source_rgb(0.0, 0.0, 0.0);

    let kinds = docview::classify(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut y = MARGIN;
    let mut page = 1;
    number_page(&cr, sheet, page);

    if !title.is_empty() {
        let l = layout(&cr, title, BODY_FONT, 20.0, Some(sheet.content_width()));
        cr.move_to(MARGIN, y);
        pangocairo::functions::show_layout(&cr, &l);
        y += height_of(&l) + 16.0;
    }

    // Consecutive body lines are one paragraph in Markdown, so they are joined
    // and re-wrapped to the page instead of keeping the file's hard breaks,
    // which would leave every line short.
    let mut blocks: Vec<(usize, String)> = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        let raw = lines.get(i).copied().unwrap_or("");
        let runs_on = match (kind, blocks.last()) {
            (Line::Body { .. }, Some((last, _))) => {
                matches!(kinds.get(*last), Some(Line::Body { depth, indent })
                    if Some(&Line::Body { depth: *depth, indent: *indent }) == kinds.get(i))
                    && !is_bullet(raw)
                    && !is_bullet(lines.get(*last).copied().unwrap_or(""))
            }
            _ => false,
        };
        if runs_on {
            if let Some((_, text)) = blocks.last_mut() {
                text.push(' ');
                text.push_str(raw.trim());
            }
        } else {
            blocks.push((i, raw.to_string()));
        }
    }

    let bottom = sheet.height - MARGIN;
    for (i, raw) in &blocks {
        let (i, raw) = (*i, raw.as_str());
        let kind = &kinds[i];
        let (text, size, indent, space_before, space_after) = match kind {
            // The frontmatter is bookkeeping, and the title is already set.
            Line::Front => continue,
            Line::Blank => {
                y += 5.0;
                continue;
            }
            Line::Heading { depth, marker } => {
                let body = raw.get(*marker..).unwrap_or("").trim();
                let depth = *depth as f64;
                (
                    body.to_string(),
                    16.0 - depth * 1.4,
                    depth * 13.0,
                    if depth == 0.0 { 15.0 } else { 11.0 },
                    3.0,
                )
            }
            Line::Body { depth, indent } => (
                bulleted(raw.trim_start()),
                10.5,
                *depth as f64 * 13.0 + *indent as f64 * 5.0,
                0.0,
                1.5,
            ),
            Line::NoteDef => (raw.trim().to_string(), 9.0, 0.0, 0.0, 1.5),
        };
        if text.is_empty() {
            y += 5.0;
            continue;
        }

        let bold = matches!(kind, Line::Heading { .. });
        let font = if bold {
            format!("{BODY_FONT} Bold")
        } else {
            BODY_FONT.to_string()
        };
        let l = layout(&cr, &text, &font, size, Some(sheet.content_width() - indent));
        let h = height_of(&l);

        // A heading that would sit alone at the foot of a page goes over.
        let needed = if bold { h + 26.0 } else { h };
        if y + space_before + needed > bottom {
            cr.show_page()?;
            page += 1;
            number_page(&cr, sheet, page);
            y = MARGIN;
        } else {
            y += space_before;
        }

        cr.move_to(MARGIN + indent, y);
        pangocairo::functions::show_layout(&cr, &l);
        y += h + space_after;
    }

    cr.show_page()?;
    surface.finish();
    Ok(page)
}

// ---- interlinears -----------------------------------------------------------

/// A sheet prints as it reads on screen: each word with its annotations beneath,
/// running on until the line is full.
pub fn export_interlinear(path: &Path, sheet_doc: &Interlinear, sheet: Sheet) -> Done {
    let surface = surface(path, sheet)?;
    let cr = Context::new(&surface)?;
    cr.set_source_rgb(0.0, 0.0, 0.0);

    let rtl = sheet_doc.language.right_to_left();
    let mut y = MARGIN;
    let mut page = 1;
    number_page(&cr, sheet, page);

    let heading = layout(&cr, &sheet_doc.reference, BODY_FONT, 18.0, None);
    cr.move_to(MARGIN, y);
    pangocairo::functions::show_layout(&cr, &heading);
    y += height_of(&heading) + 18.0;

    // Measure every word first: a column is as wide as its widest part.
    struct Column {
        word: pango::Layout,
        rows: Vec<pango::Layout>,
        width: f64,
        height: f64,
    }
    let mut columns = Vec::new();
    for w in &sheet_doc.words {
        let word = layout(&cr, &w.text, WORD_FONT, 15.0, None);
        let rows: Vec<pango::Layout> = sheet_doc
            .rows
            .iter()
            .map(|r| layout(&cr, w.field(r).unwrap_or(""), BODY_FONT, 8.5, None))
            .collect();
        let width = rows
            .iter()
            .map(|l| l.pixel_size().0 as f64)
            .fold(word.pixel_size().0 as f64, f64::max);
        let height = height_of(&word) + rows.iter().map(height_of).sum::<f64>();
        columns.push(Column { word, rows, width, height });
    }

    const GUTTER: f64 = 12.0;
    let bottom = sheet.height - MARGIN;
    let mut row: Vec<&Column> = Vec::new();
    let mut used = 0.0;

    // Nothing is drawn until a line is full, since a right-to-left line can
    // only be placed once its whole width is known.
    let draw = |row: &Vec<&Column>, y: f64| {
        let mut x = MARGIN;
        let ordered: Vec<&&Column> = if rtl {
            row.iter().rev().collect()
        } else {
            row.iter().collect()
        };
        for column in ordered {
            let mut cy = y;
            let centre = |l: &pango::Layout, x: f64, width: f64| x + (width - l.pixel_size().0 as f64) / 2.0;
            cr.move_to(centre(&column.word, x, column.width), cy);
            pangocairo::functions::show_layout(&cr, &column.word);
            cy += height_of(&column.word);
            for r in &column.rows {
                cr.move_to(centre(r, x, column.width), cy);
                pangocairo::functions::show_layout(&cr, r);
                cy += height_of(r);
            }
            x += column.width + GUTTER;
        }
    };

    for column in &columns {
        let step = column.width + GUTTER;
        if used + step > sheet.content_width() && !row.is_empty() {
            let tall = row.iter().map(|c| c.height).fold(0.0, f64::max);
            if y + tall > bottom {
                cr.show_page()?;
                page += 1;
                number_page(&cr, sheet, page);
                y = MARGIN;
            }
            draw(&row, y);
            y += tall + 16.0;
            row.clear();
            used = 0.0;
        }
        row.push(column);
        used += step;
    }
    if !row.is_empty() {
        let tall = row.iter().map(|c| c.height).fold(0.0, f64::max);
        if y + tall > bottom {
            cr.show_page()?;
            page += 1;
            number_page(&cr, sheet, page);
            y = MARGIN;
        }
        draw(&row, y);
    }

    cr.show_page()?;
    surface.finish();
    Ok(page)
}

// ---- diagrams ---------------------------------------------------------------

/// A diagram prints on one page, scaled to fill it. A sentence diagram that
/// spills onto a second sheet is no use at all.
pub fn export_diagram(path: &Path, d: &Diagram, sheet: Sheet) -> Done {
    let surface = surface(path, sheet)?;
    let cr = Context::new(&surface)?;
    cr.set_source_rgb(0.0, 0.0, 0.0);

    let heading = layout(&cr, &d.reference, BODY_FONT, 14.0, None);
    cr.move_to(MARGIN, MARGIN * 0.55);
    pangocairo::functions::show_layout(&cr, &heading);

    // What the drawing covers, words included.
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut stretch = |x: f64, y: f64| {
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    };
    for line in &d.lines {
        stretch(line.x1, line.y1);
        stretch(line.x2, line.y2);
    }
    // Words are measured and drawn in the diagram's own units, so the whole
    // thing scales together and the page shows what the screen showed.
    let mut words = Vec::new();
    for label in &d.labels {
        let l = layout(&cr, &label.text, WORD_FONT, 15.0, None);
        let (w, h) = l.pixel_size();
        let (ax, ay, angle) = d.anchor_of(label);
        let reach = w as f64 / 2.0 + 2.0;
        stretch(ax - reach, ay - h as f64);
        stretch(ax + reach, ay + 4.0);
        words.push((l, ax, ay, angle, w as f64));
    }

    let Some((x0, y0, x1, y1)) = bounds else {
        cr.show_page()?;
        surface.finish();
        return Ok(1);
    };

    let top = MARGIN;
    let usable_h = sheet.content_height() - 10.0;
    let (dw, dh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    // Fill the page, but never blow a small diagram up past legibility.
    let scale = (sheet.content_width() / dw).min(usable_h / dh).min(2.5);
    cr.translate(
        MARGIN + (sheet.content_width() - dw * scale) / 2.0,
        top + (usable_h - dh * scale) / 2.0,
    );
    cr.scale(scale, scale);
    cr.translate(-x0, -y0);

    cr.set_line_width(1.4 / scale);
    for line in &d.lines {
        match line.stroke {
            diagram::Stroke::Solid => cr.set_dash(&[], 0.0),
            diagram::Stroke::Dotted => cr.set_dash(&[4.0 / scale, 4.0 / scale], 0.0),
        }
        cr.move_to(line.x1, line.y1);
        cr.line_to(line.x2, line.y2);
        cr.stroke()?;
    }
    cr.set_dash(&[], 0.0);
    for (jx, jy) in d.joints() {
        cr.arc(jx, jy, 2.6 / scale, 0.0, std::f64::consts::TAU);
        cr.fill()?;
    }

    for (l, ax, ay, angle, w) in words {
        cr.save()?;
        cr.translate(ax, ay);
        if angle != 0.0 {
            cr.rotate(angle);
        }
        let baseline = l.baseline() as f64 / pango::SCALE as f64;
        cr.move_to(-w / 2.0, -baseline - 2.0);
        pangocairo::functions::show_layout(&cr, &l);
        cr.restore()?;
    }

    cr.show_page()?;
    surface.finish();
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interlinear::Language;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("omaverse-pdf-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn paper_turns_on_its_side() {
        let up = Sheet::new(Paper::Letter, Orientation::Portrait);
        let over = Sheet::new(Paper::Letter, Orientation::Landscape);
        assert_eq!((up.width, up.height), (612.0, 792.0));
        assert_eq!((over.width, over.height), (792.0, 612.0));
        assert_eq!(Sheet::new(Paper::A4, Orientation::Portrait).width, 595.28);
    }

    #[test]
    fn an_outline_prints_and_runs_to_more_than_one_page() {
        let mut text = String::from("---\ntitle: Jude\n---\n\n# Greeting\n\n");
        for i in 0..240 {
            text.push_str(&format!("A line of the body, number {i}.\n\n"));
        }
        let path = tmp("outline.pdf");
        let pages =
            export_outline(&path, &text, "Jude", Sheet::new(Paper::Letter, Orientation::Portrait))
                .expect("should write");
        let bytes = std::fs::read(&path).expect("a file");
        assert!(bytes.starts_with(b"%PDF"), "a real PDF");
        assert!(bytes.len() > 2000, "with something on it");
        assert!(pages > 1, "that long a document needs more than one page");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_short_sheet_fits_one_page_and_a_long_one_does_not() {
        let mut short = Interlinear::new("Jude 4", Language::Greek);
        short.append_text("alpha beta gamma");
        let path = tmp("short.pdf");
        let pages = export_interlinear(&path, &short, Sheet::new(Paper::Letter, Orientation::Portrait))
            .expect("should write");
        assert_eq!(pages, 1);
        let _ = std::fs::remove_file(&path);

        let mut long = Interlinear::new("Romans 1", Language::Greek);
        for _ in 0..90 {
            long.append_text("alpha beta gamma delta epsilon zeta eta theta");
        }
        let path = tmp("long.pdf");
        let pages = export_interlinear(&path, &long, Sheet::new(Paper::Letter, Orientation::Portrait))
            .expect("should write");
        assert!(pages > 1, "a chapter does not fit one sheet, got {pages}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_interlinear_prints() {
        let mut doc = Interlinear::new("Jude 4", Language::Greek);
        doc.append_text("alpha beta gamma delta epsilon");
        doc.word_mut("w1").unwrap().set_field("gloss", "first");
        let path = tmp("sheet.pdf");
        export_interlinear(&path, &doc, Sheet::new(Paper::Letter, Orientation::Portrait))
            .expect("should write");
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_hebrew_sheet_prints_without_complaint() {
        let mut doc = Interlinear::new("Genesis 1:1", Language::Hebrew);
        doc.append_text("\u{05D1}\u{05B0}\u{05BC}\u{05E8}\u{05B5}\u{05D0}\u{05E9}\u{05C1}\u{05B4}\u{05D9}\u{05EA} \u{05D1}\u{05B8}\u{05BC}\u{05E8}\u{05B8}\u{05D0}");
        let path = tmp("hebrew.pdf");
        export_interlinear(&path, &doc, Sheet::new(Paper::A4, Orientation::Portrait))
            .expect("should write");
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_diagram_prints_on_one_page() {
        let mut d = Diagram::new("John 1:1");
        let base = d.add_line(crate::diagram::Preset::Base, 300.0, 300.0);
        let divider = d.add_line(crate::diagram::Preset::Divider, 280.0, 300.0);
        d.settle_line(&divider, 12.0);
        let label = d.add_free_label("alpha", 250.0, 300.0);
        d.set_rest(&label, &base, 0.2);

        let path = tmp("diagram.pdf");
        let pages = export_diagram(&path, &d, Sheet::new(Paper::Letter, Orientation::Landscape))
            .expect("should write");
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        assert_eq!(pages, 1, "a diagram belongs on one sheet");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_diagram_still_prints() {
        let d = Diagram::new("Nothing yet");
        let path = tmp("empty.pdf");
        export_diagram(&path, &d, Sheet::new(Paper::Letter, Orientation::Landscape))
            .expect("should write");
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }
}

