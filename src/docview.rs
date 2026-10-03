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
    /// A line of the `---` frontmatter block. Apparatus, not prose.
    Front,
    /// A note definition at the foot of the file. Apparatus, not prose, so it
    /// is set apart rather than read as part of the text.
    NoteDef,
    /// `<!-- ref: 1:1-17 -->` under a heading: which passage the section is
    /// about. An HTML comment, so every other Markdown reader ignores it.
    Ref,
    Blank,
}

/// The passage a `<!-- ref: … -->` line names, if that is what the line is.
pub fn ref_marker(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner
        .strip_prefix("ref:")
        .or_else(|| inner.strip_prefix("ref "))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

/// Every section of an outline that names a passage, as (heading, reference
/// text). Sections without one are left out: a reference is something you add
/// where it is useful, not a field to fill in everywhere.
pub fn section_references(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut heading = String::new();
    for (line, kind) in text.lines().zip(classify(text)) {
        match kind {
            Line::Heading { marker, .. } => {
                heading = line.get(marker..).unwrap_or("").trim().to_string();
            }
            Line::Ref => {
                if let Some(r) = ref_marker(line) {
                    out.push((heading.clone(), r.to_string()));
                }
            }
            _ => {}
        }
    }
    out
}

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
    // Frontmatter, if present, is the block fenced by --- at the very top.
    let mut in_front = text.lines().next().map(|l| l.trim_end() == "---").unwrap_or(false);
    let mut front_done = !in_front;

    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start_matches(' ');
        let indent = raw.len() - trimmed.len();

        if in_front {
            out.push(Line::Front);
            if i > 0 && raw.trim_end() == "---" {
                in_front = false;
                front_done = true;
            }
            continue;
        }
        let _ = front_done;

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
            continue;
        }
        if ref_marker(raw).is_some() {
            out.push(Line::Ref);
            continue;
        }
        match atx(raw) {
            Some((d, marker)) => {
                depth = d;
                out.push(Line::Heading { depth, marker });
            }
            None => out.push(Line::Body { depth, indent }),
        }
    }
    out
}

/// `#` to `######` at the start of a line: the depth, and how many characters
/// of marker to hide.
fn atx(line: &str) -> Option<(usize, usize)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    match line.chars().nth(hashes) {
        None => Some((hashes - 1, hashes)),
        Some(' ') => Some((hashes - 1, hashes + 1)),
        Some(_) => None,
    }
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

/// Inline emphasis found in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Strong,
    Em,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub kind: Emphasis,
    /// The emphasised words, without their markers.
    pub text: (usize, usize),
    /// Everything including both markers.
    pub whole: (usize, usize),
}

/// Find `**bold**`, `*italic*`, `_italic_` and `` `code` `` in a line.
///
/// A `*` opening a prose list is left alone, and so is anything inside a note
/// anchor's `[^id]`, so marking up text cannot be confused with structure.
pub fn emphasis(line: &str) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let indent = line.len() - line.trim_start_matches(' ').len();
    // A prose list marker: `* ` at the start of the line's content.
    let list_marker = chars.get(indent) == Some(&'*') && chars.get(indent + 1) == Some(&' ');

    // Ranges occupied by note anchors, which emphasis must not reach into.
    let anchored: Vec<(usize, usize)> = anchors(line).iter().map(|a| (a.text.1, a.whole_end())).collect();
    let protected = |i: usize| anchored.iter().any(|&(s, e)| i >= s && i < e);

    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if protected(i) {
            i += 1;
            continue;
        }
        let (kind, marker) = match chars[i] {
            '`' => (Emphasis::Code, 1),
            '*' if chars.get(i + 1) == Some(&'*') => (Emphasis::Strong, 2),
            '*' if !(list_marker && i == indent) => (Emphasis::Em, 1),
            '_' => (Emphasis::Em, 1),
            _ => {
                i += 1;
                continue;
            }
        };
        let open = chars[i];
        let start = i + marker;
        let mut j = start;
        let close = loop {
            if j + marker > chars.len() {
                break None;
            }
            if chars[j] == open && (marker == 1 || chars.get(j + 1) == Some(&open)) {
                break Some(j);
            }
            j += 1;
        };
        match close {
            Some(end) if end > start && !protected(end) => {
                found.push(Span {
                    kind,
                    text: (start, end),
                    whole: (i, end + marker),
                });
                i = end + marker;
            }
            _ => i += marker,
        }
    }
    found
}

impl Anchor {
    fn whole_end(&self) -> usize {
        self.span.1
    }
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
            .name(format!("h{d}"))
            .scale(scales[d.min(scales.len() - 1)])
            .weight(if d <= 2 { 700 } else { 600 })
            .pixels_above_lines(if d == 0 { 22 } else { 16 })
            .pixels_below_lines(4)
            .left_margin(indent_px(d))
            .build();
        table.add(&tag);

        let body = gtk::TextTag::builder()
            .name(format!("b{d}"))
            .left_margin(indent_px(d) + 18)
            .pixels_below_lines(3)
            .build();
        table.add(&body);
    }

    // Misspellings: the usual red squiggle, nothing else changed about the word.
    let bad = gtk::TextTag::builder()
        .name("misspelled")
        .underline(gtk4::pango::Underline::Error)
        .build();
    table.add(&bad);

    // Note definitions: present, but clearly apparatus.
    let notedef = gtk::TextTag::builder()
        .name("notedef")
        .scale(0.88)
        .left_margin(24)
        .build();
    table.add(&notedef);

    // Frontmatter: present, but plainly not prose.
    let front = gtk::TextTag::builder()
        .name("front")
        .scale(0.85)
        .left_margin(24)
        .build();
    table.add(&front);

    // The passage a section is about: quiet, since it is a label on the
    // heading above it rather than something to read.
    let refmark = gtk::TextTag::builder()
        .name("refmark")
        .scale(0.82)
        .style(gtk::pango::Style::Italic)
        .left_margin(24)
        .build();
    table.add(&refmark);

    for (name, build) in [
        ("strong", 0),
        ("em", 1),
        ("code", 2),
    ] {
        let tag = match build {
            0 => gtk::TextTag::builder().name(name).weight(700).build(),
            1 => gtk::TextTag::builder()
                .name(name)
                .style(gtk4::pango::Style::Italic)
                .build(),
            _ => gtk::TextTag::builder()
                .name(name)
                .family("monospace")
                .scale(0.94)
                .build(),
        };
        table.add(&tag);
    }

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
pub fn restyle_range(
    buffer: &gtk::TextBuffer,
    first: i32,
    last: i32,
    cursor_line: i32,
    speller: Option<&crate::spell::Speller>,
) {
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
            Line::Front => (0, "front".to_string()),
            Line::Ref => (0, "refmark".to_string()),
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

        // Spelling, over the prose only: a note definition is apparatus and a
        // heading is full of names and references.
        if let Some(speller) = speller {
            if matches!(lines[i as usize], Line::Body { .. }) {
                for (from_ch, to_ch) in crate::spell::words(raw) {
                    let word: String = raw.chars().take(to_ch).skip(from_ch).collect();
                    if speller.is_correct(&word) {
                        continue;
                    }
                    let mut a = line_start;
                    a.forward_chars(from_ch as i32);
                    let mut b = line_start;
                    b.forward_chars(to_ch as i32);
                    buffer.apply_tag_by_name("misspelled", &a, &b);
                }
            }
        }

        // Emphasis renders as you write. The markers stay visible on the line
        // the cursor is on, so what you are editing is never hidden from you.
        let reveal = i == cursor_line;
        for span in emphasis(raw) {
            let at = |offset: usize| {
                let mut it = line_start;
                it.forward_chars(offset as i32);
                it
            };
            let name = match span.kind {
                Emphasis::Strong => "strong",
                Emphasis::Em => "em",
                Emphasis::Code => "code",
            };
            buffer.apply_tag_by_name(name, &at(span.text.0), &at(span.text.1));
            if !reveal {
                buffer.apply_tag_by_name("marker", &at(span.whole.0), &at(span.text.0));
                buffer.apply_tag_by_name("marker", &at(span.text.1), &at(span.whole.1));
            }
        }
    }
}

/// Restyle everything. Only for small buffers or a fresh document.
pub fn restyle(buffer: &gtk::TextBuffer) {
    restyle_range(buffer, 0, i32::MAX, -1, None);
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
        .filter_map(|(i, l)| matches!(l, Line::Heading { .. }).then_some(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_marker_is_recognised_but_only_in_its_own_form() {
        assert_eq!(ref_marker("<!-- ref: 1:1-17 -->"), Some("1:1-17"));
        assert_eq!(ref_marker("   <!--ref:Jude 4-6-->  "), Some("Jude 4-6"));
        assert_eq!(ref_marker("<!-- ref 8:1 -->"), Some("8:1"));
        assert_eq!(ref_marker("<!-- a note to myself -->"), None);
        assert_eq!(ref_marker("<!-- ref: -->"), None, "nothing named");
        assert_eq!(ref_marker("ref: 1:1"), None, "not a comment");
        assert_eq!(ref_marker("# A heading"), None);
    }

    #[test]
    fn a_reference_line_is_apparatus_not_prose() {
        let text = "# Greeting\n<!-- ref: 1-2 -->\nSome prose.\n";
        let kinds = classify(text);
        assert!(matches!(kinds[0], Line::Heading { .. }));
        assert_eq!(kinds[1], Line::Ref);
        // The reference does not become the body's depth or interrupt it.
        assert!(matches!(kinds[2], Line::Body { depth: 0, .. }));
    }

    #[test]
    fn each_section_keeps_its_own_reference() {
        let text = r#"---
title: Jude
---

# Greeting
<!-- ref: 1-2 -->

Prose.

## Those who crept in
<!-- ref: 3-4 -->

More.

# No reference here

Still more.
"#;
        let found = section_references(text);
        assert_eq!(
            found,
            vec![
                ("Greeting".to_string(), "1-2".to_string()),
                ("Those who crept in".to_string(), "3-4".to_string()),
            ],
            "sections without one are simply absent"
        );
    }


    const DOC: &str = "\
---
title: Genesis
---

# The Creation of Everything

## The Genesis of Everything

### Part One

Placeholder sentence standing in for the text of a verse.

### Part Two

Placeholder sentence for the second part.

#### Day One

# Antediluvian History
";

    #[test]
    fn frontmatter_is_apparatus_not_prose() {
        let c = classify(DOC);
        assert_eq!(c[0], Line::Front, "the opening fence");
        assert_eq!(c[1], Line::Front, "title:");
        assert_eq!(c[2], Line::Front, "the closing fence");
        assert_eq!(c[3], Line::Blank);
    }

    #[test]
    fn hashes_give_the_depth() {
        let c = classify(DOC);
        assert_eq!(c[4], Line::Heading { depth: 0, marker: 2 });
        assert_eq!(c[6], Line::Heading { depth: 1, marker: 3 });
        assert_eq!(c[8], Line::Heading { depth: 2, marker: 4 });
        assert_eq!(c[10], Line::Body { depth: 2, indent: 0 });
        assert_eq!(c[16], Line::Heading { depth: 3, marker: 5 }, "Day One");
        assert_eq!(c[18], Line::Heading { depth: 0, marker: 2 }, "back out to root");
    }

    #[test]
    fn body_inherits_the_depth_of_its_heading() {
        let c = classify("# a\n\nbody\n\n## b\n\nnested body\n");
        assert_eq!(c[2], Line::Body { depth: 0, indent: 0 });
        assert_eq!(c[6], Line::Body { depth: 1, indent: 0 });
    }

    #[test]
    fn a_hash_without_a_space_is_body() {
        assert_eq!(classify("# a\n\n#hashtag\n")[2], Line::Body { depth: 0, indent: 0 });
        assert_eq!(classify("# a\n\n####### too many\n")[2], Line::Body { depth: 0, indent: 0 });
    }

    #[test]
    fn a_list_bullet_is_body_now() {
        // The structural meaning of `-` is gone.
        assert_eq!(classify("# a\n\n- a list item\n")[2], Line::Body { depth: 0, indent: 0 });
    }

    #[test]
    fn blank_lines_are_blank() {
        assert_eq!(classify("# a\n\nx\n")[1], Line::Blank);
    }

    #[test]
    fn section_range_covers_every_descendant() {
        // "The Creation of Everything" on line 4 runs to the next depth-0
        // heading on line 18.
        assert_eq!(section_range(DOC, 4), (4, 18));
        // "Part One" on line 8 stops at "Part Two" on line 12.
        assert_eq!(section_range(DOC, 8), (8, 12));
    }

    #[test]
    fn section_range_of_the_last_section_runs_to_the_end() {
        let (start, end) = section_range(DOC, 18);
        assert_eq!(start, 18);
        assert_eq!(end, DOC.lines().count());
    }

    #[test]
    fn heading_lines_skips_the_frontmatter() {
        assert_eq!(heading_lines(DOC), vec![4, 6, 8, 12, 16, 18]);
    }

    #[test]
    fn an_unnamed_section_still_counts() {
        assert_eq!(classify("#\n")[0], Line::Heading { depth: 0, marker: 1 });
        assert_eq!(heading_lines("#\n"), vec![0]);
    }

    fn marked(line: &str, s: &Span) -> String {
        line.chars().take(s.text.1).skip(s.text.0).collect()
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
        let line = "\u{3b4}\u{3bf}ῦ\u{3bb}\u{3bf}\u{3c2} — ==servant==[^n1]";
        let a = anchors(line);
        assert_eq!(a.len(), 1);
        let chars: Vec<char> = line.chars().collect();
        let text: String = chars[a[0].text.0..a[0].text.1].iter().collect();
        assert_eq!(text, "servant", "offsets must be char-based for TextIter");
    }

    #[test]
    fn bold_italic_and_code_are_found() {
        let line = "Paul stacks **three** self-descriptions, *each* one `pointing` away.";
        let found = emphasis(line);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].kind, Emphasis::Strong);
        assert_eq!(marked(line, &found[0]), "three");
        assert_eq!(found[1].kind, Emphasis::Em);
        assert_eq!(marked(line, &found[1]), "each");
        assert_eq!(found[2].kind, Emphasis::Code);
        assert_eq!(marked(line, &found[2]), "pointing");
    }

    #[test]
    fn underscores_italicise_too() {
        let line = "the word _doulos_ is stronger";
        let found = emphasis(line);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Emphasis::Em);
        assert_eq!(marked(line, &found[0]), "doulos");
    }

    #[test]
    fn a_prose_list_marker_is_not_italic() {
        // `*` opens a list in a body; it must not pair with a later asterisk.
        assert!(emphasis("  * servant, not deacon").is_empty());
        // but emphasis inside such a line still works
        let line = "  * servant, **not** deacon";
        let found = emphasis(line);
        assert_eq!(found.len(), 1);
        assert_eq!(marked(line, &found[0]), "not");
    }

    #[test]
    fn a_note_anchor_is_left_intact() {
        // The `[^id]` must not be read as emphasis, whatever is in the id.
        let line = "a ==servant==[^n_1] first";
        assert!(emphasis(line).is_empty(), "the anchor id is off limits");
    }

    #[test]
    fn unpaired_markers_are_ignored() {
        assert!(emphasis("2 + 2 * 2 is not italic").is_empty());
        assert!(emphasis("a lone ` backtick").is_empty());
        assert!(emphasis("**").is_empty());
        assert!(emphasis("****").is_empty());
        assert!(emphasis("").is_empty());
    }

    #[test]
    fn emphasis_offsets_count_characters() {
        let line = "\u{3b4}\u{3bf}ῦ\u{3bb}\u{3bf}\u{3c2} — **servant**";
        let found = emphasis(line);
        assert_eq!(found.len(), 1);
        assert_eq!(marked(line, &found[0]), "servant");
    }

    #[test]
    fn bold_wins_over_italic_at_a_double_marker() {
        let line = "**both** and *one*";
        let found = emphasis(line);
        assert_eq!(found[0].kind, Emphasis::Strong);
        assert_eq!(found[1].kind, Emphasis::Em);
    }

}
