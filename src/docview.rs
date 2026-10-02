//! Rendering the whole outline as one continuous document.
//!
//! The buffer holds the file's Markdown verbatim -- it *is* the document, so
//! there is one source of truth and no pane to keep in sync. Appearance comes
//! entirely from tags: the `- ` markers and leading indentation are made
//! invisible, headings are scaled by depth, and bodies are inset with a
//! paragraph margin. What is saved is exactly what was typed.

use gtk4 as gtk;
use gtk4::prelude::*;

/// How a single line of the file should be drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A `- ` bullet: a section heading at `depth`.
    /// `marker` counts the leading spaces plus the bullet itself.
    Heading { depth: usize, marker: usize },
    /// Body text belonging to the section at `depth`. `indent` counts the
    /// leading spaces to hide.
    Body { depth: usize, indent: usize },
    Blank,
}

const INDENT: usize = 2;
/// Depths beyond this share the innermost style rather than shrinking forever.
pub const MAX_DEPTH: usize = 5;

/// Classify every line. Pure, so the whole rendering decision is testable
/// without a display.
pub fn classify(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    for raw in text.lines() {
        let trimmed = raw.trim_start_matches(' ');
        let indent = raw.len() - trimmed.len();
        if trimmed.is_empty() {
            out.push(Line::Blank);
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            let _ = rest;
            depth = indent / INDENT;
            out.push(Line::Heading { depth, marker: indent + 2 });
        } else if trimmed == "-" {
            depth = indent / INDENT;
            out.push(Line::Heading { depth, marker: indent + 1 });
        } else if trimmed.starts_with("# ") && indent == 0 {
            // The document title line.
            out.push(Line::Heading { depth: 0, marker: 2 });
        } else {
            out.push(Line::Body { depth, indent });
        }
    }
    out
}

/// Create the tag table once. Tags are looked up by name afterwards.
pub fn install_tags(buffer: &gtk::TextBuffer) {
    let table = buffer.tag_table();
    if table.lookup("marker").is_some() {
        return;
    }

    let marker = gtk::TextTag::builder().name("marker").invisible(true).build();
    table.add(&marker);

    // Headings shrink with depth, then hold. Scales are relative to the view's
    // font, so they track whatever the body size is.
    let scales = [1.45_f64, 1.22, 1.10, 1.02, 1.0, 1.0];
    for d in 0..=MAX_DEPTH {
        let tag = gtk::TextTag::builder()
            .name(&format!("h{d}"))
            .scale(scales[d.min(scales.len() - 1)])
            .weight(if d <= 2 { 700 } else { 600 })
            .pixels_above_lines(if d == 0 { 22 } else { 16 })
            .pixels_below_lines(4)
            .left_margin(indent_px(d))
            .build();
        table.add(&tag);

        let body = gtk::TextTag::builder()
            .name(&format!("b{d}"))
            .left_margin(indent_px(d) + 18)
            .pixels_below_lines(3)
            .build();
        table.add(&body);
    }

    // Applied over a collapsed section's range.
    let folded = gtk::TextTag::builder().name("folded").invisible(true).build();
    table.add(&folded);
}

fn indent_px(depth: usize) -> i32 {
    24 + (depth.min(MAX_DEPTH) as i32) * 22
}

/// Re-apply every appearance tag. Folding is applied separately so that
/// re-styling never disturbs what is collapsed.
pub fn restyle(buffer: &gtk::TextBuffer) {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    let text = buffer.text(&start, &end, true).to_string();

    buffer.remove_tag_by_name("marker", &start, &end);
    for d in 0..=MAX_DEPTH {
        buffer.remove_tag_by_name(&format!("h{d}"), &start, &end);
        buffer.remove_tag_by_name(&format!("b{d}"), &start, &end);
    }

    for (i, line) in classify(&text).into_iter().enumerate() {
        let Some(line_start) = buffer.iter_at_line(i as i32) else { continue };
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        match line {
            Line::Blank => {}
            Line::Heading { depth, marker } => {
                let mut after = line_start;
                after.forward_chars(marker as i32);
                if after > line_end {
                    after = line_end;
                }
                buffer.apply_tag_by_name("marker", &line_start, &after);
                buffer.apply_tag_by_name(&format!("h{}", depth.min(MAX_DEPTH)), &line_start, &line_end);
            }
            Line::Body { depth, indent } => {
                let mut after = line_start;
                after.forward_chars(indent as i32);
                if after > line_end {
                    after = line_end;
                }
                buffer.apply_tag_by_name("marker", &line_start, &after);
                buffer.apply_tag_by_name(&format!("b{}", depth.min(MAX_DEPTH)), &line_start, &line_end);
            }
        }
    }
}

/// The line a section starts on, and the line after everything it contains.
/// Used both for folding and for scrolling to a section.
pub fn section_range(text: &str, heading_line: usize) -> (usize, usize) {
    let lines = classify(text);
    let depth = match lines.get(heading_line) {
        Some(Line::Heading { depth, .. }) => *depth,
        _ => return (heading_line, heading_line + 1),
    };
    let mut end = heading_line + 1;
    while end < lines.len() {
        if let Line::Heading { depth: d, .. } = lines[end] {
            if d <= depth {
                break;
            }
        }
        end += 1;
    }
    (heading_line, end)
}

/// Buffer line numbers of every heading, in document order. Index N here is
/// node N in a depth-first walk of the parsed document, so the outline tree and
/// the text stay addressable by the same number.
pub fn heading_lines(text: &str) -> Vec<usize> {
    classify(text)
        .into_iter()
        .enumerate()
        .filter_map(|(i, l)| match l {
            // The `# Title` line is not a node, only sections are.
            Line::Heading { marker: 2, depth: 0 } if is_title_line(text, i) => None,
            Line::Heading { .. } => Some(i),
            _ => None,
        })
        .collect()
}

fn is_title_line(text: &str, index: usize) -> bool {
    text.lines().nth(index).map(|l| l.starts_with("# ")).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
# Genesis

- The Creation of Everything — 1:1-2:3

  - The Genesis of Everything — 1:1-2:3

    - Part One — 1:1

      1:1 In the beginning, God created the heavens and the earth.

    - Part Two — 1:2-2:3

      2 The earth was without form and void.

      - Day One
- Antediluvian History — 2:4-11:26
";

    #[test]
    fn headings_and_bodies_are_classified_by_depth() {
        let c = classify(DOC);
        assert_eq!(c[0], Line::Heading { depth: 0, marker: 2 }, "the # title line");
        assert_eq!(c[2], Line::Heading { depth: 0, marker: 2 });
        assert_eq!(c[4], Line::Heading { depth: 1, marker: 4 });
        assert_eq!(c[6], Line::Heading { depth: 2, marker: 6 });
        assert_eq!(c[8], Line::Body { depth: 2, indent: 6 });
        assert_eq!(c[14], Line::Heading { depth: 3, marker: 8 }, "Day One");
        assert_eq!(c[15], Line::Heading { depth: 0, marker: 2 }, "back out to root");
    }

    #[test]
    fn body_inherits_the_depth_of_its_heading() {
        let c = classify("- a\n\n  body\n  - b\n\n    nested body\n");
        assert_eq!(c[2], Line::Body { depth: 0, indent: 2 });
        assert_eq!(c[5], Line::Body { depth: 1, indent: 4 });
    }

    #[test]
    fn blank_lines_are_blank() {
        assert_eq!(classify("- a\n\n  x\n")[1], Line::Blank);
    }

    #[test]
    fn section_range_covers_every_descendant() {
        // "The Creation of Everything" at line 2 runs until the next depth-0
        // heading on line 15.
        assert_eq!(section_range(DOC, 2), (2, 15));
        // "Part One" at line 6 stops at "Part Two" on line 10.
        assert_eq!(section_range(DOC, 6), (6, 10));
    }

    #[test]
    fn section_range_of_the_last_section_runs_to_the_end() {
        let (s, e) = section_range(DOC, 15);
        assert_eq!(s, 15);
        assert_eq!(e, DOC.lines().count());
    }

    #[test]
    fn heading_lines_skips_the_document_title() {
        assert_eq!(heading_lines(DOC), vec![2, 4, 6, 10, 14, 15]);
    }

    #[test]
    fn an_unnamed_section_still_counts() {
        assert_eq!(classify("-\n")[0], Line::Heading { depth: 0, marker: 1 });
        assert_eq!(heading_lines("-\n"), vec![0]);
    }
}
