//! Markdown <-> Document.
//!
//! Format rules (see also the project README):
//!   1. A leading `# ` heading is the document title.
//!   2. `- ` at indent N is an outline node at depth N/2.
//!   3. Any other line indented under a node is that node's body.
//!   4. `*` / `+` bullets are prose inside a body, never structure.
//!
//! Serializing puts a blank line around a body so the file still renders
//! correctly as ordinary Markdown elsewhere (without it, Markdown's lazy
//! continuation would join the body onto the title line). Parsing trims blank
//! lines from a body's edges, which is what makes the round trip stable.

use crate::model::{Document, Node, NodePath};

const INDENT: usize = 2;

/// `- ` is structural. `*`, `+`, and `---` are not.
fn bullet(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();
    if let Some(title) = trimmed.strip_prefix("- ") {
        return Some((indent, title.trim_end().to_string()));
    }
    if trimmed == "-" {
        return Some((indent, String::new()));
    }
    None
}

/// Leading tabs count as one indent level each.
fn expand_tabs(line: &str) -> String {
    let rest = line.trim_start_matches('\t');
    let tabs = line.len() - rest.len();
    if tabs == 0 {
        return line.to_string();
    }
    format!("{}{}", " ".repeat(tabs * INDENT), rest)
}

/// Strip the body block's *common* leading indent, rather than a fixed expected
/// column. Uniformly over-indented hand-written text normalizes, while deliberate
/// relative indentation inside the body survives.
fn dedent_block(lines: &[String]) -> String {
    let min = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let stripped: Vec<String> = lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                let mut s = l.as_str();
                for _ in 0..min {
                    match s.strip_prefix(' ') {
                        Some(r) => s = r,
                        None => break,
                    }
                }
                s.trim_end().to_string()
            }
        })
        .collect();
    join_trimmed(&stripped)
}

/// Join lines, dropping whitespace-only lines at both ends but keeping interior ones.
fn join_trimmed(lines: &[String]) -> String {
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].join("\n"),
        _ => String::new(),
    }
}

pub fn parse(src: &str) -> Document {
    let mut doc = Document::default();
    let lines: Vec<String> = src.lines().map(expand_tabs).collect();
    let mut i = 0;

    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    if let Some(rest) = lines.get(i).and_then(|l| l.strip_prefix("# ")) {
        doc.title = Some(rest.trim().to_string());
        i += 1;
    }

    let mut preamble = Vec::new();
    while i < lines.len() && bullet(&lines[i]).is_none() {
        preamble.push(lines[i].clone());
        i += 1;
    }
    doc.preamble = join_trimmed(&preamble);

    // `path` tracks the most recently added node; its depth is path.len() - 1.
    let mut path: NodePath = Vec::new();
    let mut body: Vec<String> = Vec::new();

    while i < lines.len() {
        if let Some((indent, title)) = bullet(&lines[i]) {
            flush_body(&mut doc, &path, &mut body);
            let node = Node::new(title);
            path = if path.is_empty() {
                doc.push_root(node)
            } else {
                // A bullet may never be more than one level deeper than the
                // previous one; over-indentation is clamped rather than rejected.
                let depth = (indent / INDENT).min(path.len());
                if depth == path.len() {
                    let parent = path.clone();
                    doc.insert_child(&parent, node).expect("parent exists")
                } else {
                    let mut sibling = path.clone();
                    sibling.truncate(depth + 1);
                    doc.insert_sibling_after(&sibling, node).expect("sibling exists")
                }
            };
        } else if !path.is_empty() {
            // Kept raw; the common indent is stripped when the body is flushed.
            body.push(lines[i].clone());
        }
        i += 1;
    }
    flush_body(&mut doc, &path, &mut body);
    doc
}

fn flush_body(doc: &mut Document, path: &[usize], body: &mut Vec<String>) {
    if path.is_empty() {
        body.clear();
        return;
    }
    let text = dedent_block(body);
    body.clear();
    if !text.is_empty() {
        if let Some(n) = doc.get_mut(path) {
            n.body = text;
        }
    }
}

pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str("# ");
        out.push_str(t.trim());
        out.push_str("\n\n");
    }
    if !doc.preamble.trim().is_empty() {
        out.push_str(doc.preamble.trim_end());
        out.push_str("\n\n");
    }
    for root in &doc.roots {
        emit(root, 0, &mut out);
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn emit(node: &Node, depth: usize, out: &mut String) {
    let indent = " ".repeat(depth * INDENT);
    out.push_str(&indent);
    // A bare "-" rather than "- " keeps trailing whitespace out of the file for
    // nodes that have not been named yet.
    let title = node.title.trim_end();
    if title.is_empty() {
        out.push('-');
    } else {
        out.push_str("- ");
        out.push_str(title);
    }
    out.push('\n');

    if !node.body.trim().is_empty() {
        let content = " ".repeat((depth + 1) * INDENT);
        out.push('\n');
        for line in node.body.lines() {
            if line.trim().is_empty() {
                out.push('\n');
            } else {
                out.push_str(&content);
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
        out.push('\n');
    }
    for child in &node.children {
        emit(child, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROMANS: &str = "\
# Romans

- Introduction (1:1-17)

  Paul stacks three self-descriptions here, each one
  pointing away from himself.

  * servant — δοῦλος, not διάκονος
  * apostle — sent with authority

  - The greeting

    Note the inversion: \"called to be an apostle\".

  - Thanksgiving
- God's wrath revealed (1:18-32)
";

    #[test]
    fn parses_the_reference_document() {
        let d = parse(ROMANS);
        assert_eq!(d.title.as_deref(), Some("Romans"));
        assert_eq!(d.roots.len(), 2);
        let intro = &d.roots[0];
        assert_eq!(intro.title, "Introduction (1:1-17)");
        assert_eq!(intro.children.len(), 2);
        assert_eq!(intro.children[0].title, "The greeting");
        assert_eq!(intro.children[1].title, "Thanksgiving");
        assert!(intro.children[1].body.is_empty());
        assert_eq!(d.roots[1].title, "God's wrath revealed (1:18-32)");
    }

    #[test]
    fn prose_bullets_stay_in_the_body() {
        let d = parse(ROMANS);
        let body = &d.roots[0].body;
        assert!(body.contains("* servant — δοῦλος, not διάκονος"));
        assert!(body.contains("* apostle"));
        // The interior blank line between prose and the `*` list survives.
        assert_eq!(
            body,
            "Paul stacks three self-descriptions here, each one\n\
             pointing away from himself.\n\
             \n\
             * servant — δοῦλος, not διάκονος\n\
             * apostle — sent with authority"
        );
    }

    #[test]
    fn serialize_is_byte_exact_for_the_reference_document() {
        assert_eq!(serialize(&parse(ROMANS)), ROMANS);
    }

    #[test]
    fn round_trip_is_idempotent() {
        for src in [ROMANS, "- a\n", "", "# T\n", "- a\n  - b\n    - c\n"] {
            let once = serialize(&parse(src));
            let twice = serialize(&parse(&once));
            assert_eq!(once, twice, "not idempotent for {src:?}");
        }
    }

    #[test]
    fn body_is_separated_by_a_blank_line_so_markdown_renders_it() {
        // Without the blank line, Markdown's lazy continuation would join the
        // body onto the title line in Obsidian / GitHub.
        let mut d = Document::default();
        d.push_root(Node { title: "T".into(), body: "b".into(), children: vec![] });
        assert_eq!(serialize(&d), "- T\n\n  b\n");
    }

    #[test]
    fn lazy_continuation_input_is_still_accepted() {
        // A hand-written file with no blank line parses the same way.
        let d = parse("- T\n  b\n");
        assert_eq!(d.roots[0].body, "b");
        assert_eq!(serialize(&d), "- T\n\n  b\n");
    }

    #[test]
    fn over_indentation_is_clamped_not_lost() {
        let d = parse("- a\n      - b\n");
        assert_eq!(d.roots.len(), 1);
        assert_eq!(d.roots[0].children.len(), 1);
        assert_eq!(d.roots[0].children[0].title, "b");
    }

    #[test]
    fn outdenting_multiple_levels_at_once() {
        let d = parse("- a\n  - b\n    - c\n- d\n");
        assert_eq!(d.roots.len(), 2);
        assert_eq!(d.roots[1].title, "d");
        assert_eq!(d.roots[0].children[0].children[0].title, "c");
    }

    #[test]
    fn tabs_count_as_indent() {
        let d = parse("- a\n\t- b\n");
        assert_eq!(d.roots[0].children[0].title, "b");
    }

    #[test]
    fn no_title_falls_back_to_none() {
        let d = parse("- a\n");
        assert!(d.title.is_none());
        assert_eq!(serialize(&d), "- a\n");
    }

    #[test]
    fn preamble_is_preserved_not_eaten() {
        let src = "# T\n\nA note before any bullets.\n\n- a\n";
        let d = parse(src);
        assert_eq!(d.preamble, "A note before any bullets.");
        assert_eq!(serialize(&d), src);
    }

    #[test]
    fn an_unnamed_node_writes_a_bare_dash_and_reads_back() {
        let mut d = Document::default();
        d.push_root(Node::new(""));
        assert_eq!(serialize(&d), "-\n");
        assert_eq!(parse("-\n").roots.len(), 1);
        assert_eq!(parse("-\n").roots[0].title, "");
    }

    /// Deleting a section's heading line should fold its text up into the
    /// section above -- that is how you merge two sections by hand.
    #[test]
    fn deleting_a_heading_merges_its_text_into_the_section_above() {
        let before = "\
- Part One — 1:1

  1:1 In the beginning, God created the heavens and the earth.

- Part Two — 1:2

  2 The earth was without form and void.
";
        // the user deletes the "- Part Two — 1:2" line
        let after = "\
- Part One — 1:1

  1:1 In the beginning, God created the heavens and the earth.

  2 The earth was without form and void.
";
        let d = parse(after);
        assert_eq!(d.roots.len(), 1, "one section now, not two");
        assert_eq!(d.roots[0].title, "Part One — 1:1");
        assert_eq!(
            d.roots[0].body,
            "1:1 In the beginning, God created the heavens and the earth.\n\n\
             2 The earth was without form and void."
        );
        assert_eq!(parse(before).roots.len(), 2, "two sections beforehand");
    }

    /// The same merge when the deleted section was indented deeper: its text
    /// keeps its own relative indentation rather than being flattened.
    #[test]
    fn merging_a_deeper_section_keeps_relative_indentation() {
        let d = parse("- A\n\n  text of A\n\n    text that was under B\n");
        assert_eq!(d.roots.len(), 1);
        assert_eq!(d.roots[0].body, "text of A\n\n  text that was under B");
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let d = parse("");
        assert!(d.is_empty());
        assert_eq!(serialize(&d), "");
    }

    #[test]
    fn horizontal_rule_in_body_is_not_a_node() {
        let d = parse("- a\n\n  ---\n");
        assert_eq!(d.roots.len(), 1);
        assert_eq!(d.roots[0].body, "---");
    }

    /// Mirrors what the app does on save: parse what is on disk, re-serialize.
    /// The input here is deliberately messy -- tab indent, an over-indented
    /// bullet, bodies with no blank line, trailing spaces.
    #[test]
    fn a_messy_hand_written_file_normalizes_to_canonical_markdown() {
        let messy = "# Romans\n\n\
                     - Introduction (1:1-17)\n\
                     \u{20}\u{20}Paul stacks three self-descriptions here.   \n\
                     \u{20}\u{20}* servant \u{2014} \u{3b4}\u{3bf}\u{1fe6}\u{3bb}\u{3bf}\u{3c2}\n\
                     \t- The greeting\n\
                     \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}Note the inversion.\n\
                     \u{20}\u{20}- Thanksgiving (8-15)\n\
                     - God's wrath revealed (1:18-32)\n";
        let canonical = serialize(&parse(messy));
        assert_eq!(
            canonical,
            "# Romans\n\n\
             - Introduction (1:1-17)\n\n\
             \u{20}\u{20}Paul stacks three self-descriptions here.\n\
             \u{20}\u{20}* servant \u{2014} \u{3b4}\u{3bf}\u{1fe6}\u{3bb}\u{3bf}\u{3c2}\n\n\
             \u{20}\u{20}- The greeting\n\n\
             \u{20}\u{20}\u{20}\u{20}Note the inversion.\n\n\
             \u{20}\u{20}- Thanksgiving (8-15)\n\
             - God's wrath revealed (1:18-32)\n"
        );
        // And saving again changes nothing.
        assert_eq!(serialize(&parse(&canonical)), canonical);
    }

    #[test]
    fn interior_blank_lines_in_a_body_survive() {
        let d = parse("- a\n\n  one\n\n  two\n");
        assert_eq!(d.roots[0].body, "one\n\ntwo");
        assert_eq!(serialize(&d), "- a\n\n  one\n\n  two\n");
    }
}
