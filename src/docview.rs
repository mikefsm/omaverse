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
    /// A note definition at the foot of the file. Apparatus, not prose, so it
    /// is set apart rather than read as part of the text.
    NoteDef,
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
    // Notes live at the foot of the file, so everything from the first
    // definition on is apparatus.
    let mut in_notes = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start_matches(' ');
        let indent = raw.len() - trimmed.len();
        if trimmed.starts_with("[^") && trimmed.contains("]:") && indent == 0 {
            in_notes = true;
            out.push(Line::NoteDef);
            continue;
        }
        if in_notes {
            out.push(if trimmed.is_empty() { Line::Blank } else { Line::NoteDef });
            continue;
        }
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

/// A note anchor inside a line: `==the words==[^id]`.
/// Offsets are in characters from the start of the line, which is what
/// GtkTextIter counts in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// The marked words, without the `==` on either side.
    pub text: (usize, usize),
    /// The whole thing including both markers and the `[^id]` reference.
    pub span: (usize, usize),
    pub id: String,
}

/// Find every note anchor in a line. A bare `==x==` with no `[^id]` after it is
/// not an anchor -- it is ordinary highlighting, and left alone.
pub fn anchors(line: &str) -> Vec<Anchor> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] != '=' || chars[i + 1] != '=' {
            i += 1;
            continue;
        }
        let text_start = i + 2;
        let Some(text_end) = (text_start..chars.len().saturating_sub(1))
            .find(|&j| chars[j] == '=' && chars[j + 1] == '=')
        else {
            break;
        };
        if text_end == text_start {
            i = text_end + 2;
            continue;
        }
        // `[^id]` must follow immediately.
        let after = text_end + 2;
        if chars.get(after) != Some(&'[') || chars.get(after + 1) != Some(&'^') {
            i = after;
            continue;
        }
        let Some(close) = (after + 2..chars.len()).find(|&j| chars[j] == ']') else {
            i = after;
            continue;
        };
        let id: String = chars[after + 2..close].iter().collect();
        if id.is_empty() || id.chars().any(|c| c.is_whitespace()) {
            i = after;
            continue;
        }
        found.push(Anchor { text: (text_start, text_end), span: (i, close + 1), id });
        i = close + 1;
    }
    found
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

    // Note definitions: present, but clearly apparatus.
    let notedef = gtk::TextTag::builder()
        .name("notedef")
        .scale(0.88)
        .left_margin(24)
        .build();
    table.add(&notedef);

    // Annotated words: underlined, so they read as marked without shouting.
    let note = gtk::TextTag::builder()
        .name("note")
        .underline(gtk4::pango::Underline::Single)
        .build();
    table.add(&note);

    // Applied over a collapsed section's range.
    let folded = gtk::TextTag::builder().name("folded").invisible(true).build();
    table.add(&folded);
}

fn indent_px(depth: usize) -> i32 {
    24 + (depth.min(MAX_DEPTH) as i32) * 22
}

/// Re-apply appearance tags over a range of lines.
///
/// Only a range: classifying is cheap Rust over a string, but applying tags is
/// GTK work proportional to how many lines are touched, and doing the whole
/// buffer on every keystroke saturates the CPU on a book-length outline.
/// Classification still runs over the whole text, because a line's depth
/// depends on the nearest heading above it.
pub fn restyle_range(buffer: &gtk::TextBuffer, first: i32, last: i32) {
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true).to_string();
    let lines = classify(&text);
    if lines.is_empty() {
        return;
    }
    let first = first.max(0);
    let last = last.min(lines.len() as i32 - 1);
    if last < first {
        return;
    }

    let Some(from) = buffer.iter_at_line(first) else { return };
    let mut to = buffer.iter_at_line(last).unwrap_or_else(|| buffer.end_iter());
    if !to.ends_line() {
        to.forward_to_line_end();
    }
    buffer.remove_tag_by_name("marker", &from, &to);
    buffer.remove_tag_by_name("note", &from, &to);
    for d in 0..=MAX_DEPTH {
        buffer.remove_tag_by_name(&format!("h{d}"), &from, &to);
        buffer.remove_tag_by_name(&format!("b{d}"), &from, &to);
    }

    for i in first..=last {
        let Some(line_start) = buffer.iter_at_line(i) else { continue };
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let (hidden, style) = match lines[i as usize] {
            Line::Blank => continue,
            Line::NoteDef => (0, "notedef".to_string()),
            Line::Heading { depth, marker } => (marker, format!("h{}", depth.min(MAX_DEPTH))),
            Line::Body { depth, indent } => (indent, format!("b{}", depth.min(MAX_DEPTH))),
        };
        let mut after = line_start;
        after.forward_chars(hidden as i32);
        if after > line_end {
            after = line_end;
        }
        buffer.apply_tag_by_name("marker", &line_start, &after);
        buffer.apply_tag_by_name(&style, &line_start, &line_end);

        // Underline annotated words and hide the `==` and `[^id]` around them.
        let raw = text.lines().nth(i as usize).unwrap_or("");
        for anchor in anchors(raw) {
            let at = |offset: usize| {
                let mut it = line_start;
                it.forward_chars(offset as i32);
                it
            };
            buffer.apply_tag_by_name("marker", &at(anchor.span.0), &at(anchor.text.0));
            buffer.apply_tag_by_name("note", &at(anchor.text.0), &at(anchor.text.1));
            buffer.apply_tag_by_name("marker", &at(anchor.text.1), &at(anchor.span.1));
        }
    }
}

/// Restyle everything. Only for small buffers or a fresh document.
pub fn restyle(buffer: &gtk::TextBuffer) {
    restyle_range(buffer, 0, i32::MAX);
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
    fn an_anchor_is_found_with_its_id() {
        let a = anchors("Paul calls himself a ==servant==[^n1] first.");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].id, "n1");
        let line = "Paul calls himself a ==servant==[^n1] first.";
        let chars: Vec<char> = line.chars().collect();
        let text: String = chars[a[0].text.0..a[0].text.1].iter().collect();
        assert_eq!(text, "servant");
        let span: String = chars[a[0].span.0..a[0].span.1].iter().collect();
        assert_eq!(span, "==servant==[^n1]");
    }

    #[test]
    fn several_anchors_on_one_line() {
        let a = anchors("==one==[^a] and ==two words==[^b] here");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].id, "a");
        assert_eq!(a[1].id, "b");
    }

    #[test]
    fn highlighting_without_a_note_is_left_alone() {
        assert!(anchors("just ==highlighted== text").is_empty());
        assert!(anchors("==no id==[^] here").is_empty());
        assert!(anchors("==bad==[^two words] here").is_empty());
    }

    #[test]
    fn unterminated_markers_do_not_panic_or_match() {
        assert!(anchors("==never closed").is_empty());
        assert!(anchors("====").is_empty());
        assert!(anchors("==").is_empty());
        assert!(anchors("").is_empty());
    }

    #[test]
    fn anchors_count_characters_not_bytes() {
        // The em dash and Greek before the anchor are multi-byte.
        let line = "\u{3b4}\u{3bf}\u{1fe6}\u{3bb}\u{3bf}\u{3c2} \u{2014} ==servant==[^n1]";
        let a = anchors(line);
        assert_eq!(a.len(), 1);
        let chars: Vec<char> = line.chars().collect();
        let text: String = chars[a[0].text.0..a[0].text.1].iter().collect();
        assert_eq!(text, "servant", "offsets must be char-based for TextIter");
    }

    #[test]
    fn an_unnamed_section_still_counts() {
        assert_eq!(classify("-\n")[0], Line::Heading { depth: 0, marker: 1 });
        assert_eq!(heading_lines("-\n"), vec![0]);
    }
}
