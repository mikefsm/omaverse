//! Markdown <-> Document.
//!
//! Format:
//!   1. An optional `---` frontmatter block carries the document's title, which
//!      leaves every `#` level free for sections.
//!   2. `#` to `######` are sections; the number of hashes is the depth.
//!   3. Anything else is the body of the section above it, flush left.
//!   4. `[^id]:` definitions at the foot of the file are notes.
//!
//! Headings carry the structure, so `-` and `*` are ordinary list bullets again
//! and mean nothing to omaverse. Nothing in a file written here is a private
//! convention: it is all plain Markdown.

use crate::model::{Document, Node, NodePath, Note, NoteKind};


/// `#` to `######` open a section. The count of hashes is the depth.
fn heading(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if rest.is_empty() {
        return Some((hashes - 1, String::new()));
    }
    let title = rest.strip_prefix(' ')?;
    Some((hashes - 1, title.trim().to_string()))
}

/// A `---` fence around the frontmatter block.
fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

/// Read a leading frontmatter block, returning its fields and the rest.
fn split_front(src: &str) -> (Option<String>, String) {
    let mut lines = src.lines();
    if !lines.next().map(is_fence).unwrap_or(false) {
        return (None, src.to_string());
    }
    let mut title = None;
    let mut consumed = 1;
    for line in lines {
        consumed += 1;
        if is_fence(line) {
            let rest: Vec<&str> = src.lines().skip(consumed).collect();
            return (title, rest.join("\n"));
        }
        if let Some(value) = line.trim().strip_prefix("title:") {
            title = Some(value.trim().to_string());
        }
    }
    // No closing fence: treat the whole thing as ordinary text.
    (None, src.to_string())
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

/// `[^id]: rest` — a footnote definition, which is how a note is written.
fn note_def(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let id = &rest[..close];
    if id.is_empty() || id.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    Some((id.to_string(), rest[close + 2..].trim_start().to_string()))
}

/// Split the file into its outline and its notes. Notes live at the foot of the
/// file, so everything from the first definition on belongs to them -- which is
/// also what keeps them out of the last section's body.
fn split_notes(src: &str) -> (String, Vec<Note>) {
    let lines: Vec<&str> = src.lines().collect();
    let Some(first) = lines.iter().position(|l| note_def(l).is_some()) else {
        return (src.to_string(), Vec::new());
    };
    (lines[..first].join("\n"), read_notes(&lines[first..]))
}

fn read_notes(lines: &[&str]) -> Vec<Note> {
    let mut collected: Vec<(String, Vec<String>)> = Vec::new();
    for line in lines {
        if let Some((id, first)) = note_def(line) {
            collected.push((id, vec![first]));
        } else if let Some((_, content)) = collected.last_mut() {
            if line.trim().is_empty() {
                content.push(String::new());
            } else if line.starts_with(' ') || line.starts_with('\t') {
                content.push(line.trim_start().to_string());
            }
        }
    }
    collected.into_iter().map(|(id, content)| build_note(id, content)).collect()
}

fn build_note(id: String, content: Vec<String>) -> Note {
    let lines = trim_blank_edges(content);
    let quoted = !lines.is_empty()
        && lines.iter().filter(|l| !l.is_empty()).all(|l| l.starts_with('>'));
    if !quoted {
        return Note { id, kind: NoteKind::Comment, text: lines.join("\n"), source: None };
    }
    let mut body: Vec<String> = lines
        .iter()
        .map(|l| l.trim_start_matches('>').trim_start().to_string())
        .collect();
    while body.last().map(|l| l.is_empty()).unwrap_or(false) {
        body.pop();
    }
    let mut source = None;
    if let Some(last) = body.last() {
        let attribution = last
            .strip_prefix('—')
            .or_else(|| last.strip_prefix("--"))
            .or_else(|| last.strip_prefix('-'));
        if let Some(rest) = attribution {
            source = Some(rest.trim().to_string());
            body.pop();
        }
    }
    while body.last().map(|l| l.is_empty()).unwrap_or(false) {
        body.pop();
    }
    Note { id, kind: NoteKind::Quotation, text: body.join("\n"), source }
}

fn trim_blank_edges(lines: Vec<String>) -> Vec<String> {
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

fn serialize_notes(notes: &[Note]) -> String {
    let mut out = String::new();
    for note in notes {
        match note.kind {
            NoteKind::Comment => {
                let mut lines = note.text.lines();
                let first = lines.next().unwrap_or("");
                if first.is_empty() {
                    out.push_str(&format!("[^{}]:\n", note.id));
                } else {
                    out.push_str(&format!("[^{}]: {}\n", note.id, first));
                }
                for line in lines {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            NoteKind::Quotation => {
                let mut lines = note.text.lines();
                out.push_str(&format!("[^{}]: > {}\n", note.id, lines.next().unwrap_or("")));
                for line in lines {
                    if line.is_empty() {
                        out.push_str("    >\n");
                    } else {
                        out.push_str("    > ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                if let Some(src) = note.source.as_ref().filter(|s| !s.trim().is_empty()) {
                    out.push_str("    >\n");
                    out.push_str(&format!("    > — {}\n", src.trim()));
                }
            }
        }
        out.push('\n');
    }
    out
}

pub fn parse(src: &str) -> Document {
    let (body_src, notes) = split_notes(src);
    let (title, rest) = split_front(&body_src);
    let mut doc = Document::default();
    doc.notes = notes;
    doc.title = title;

    let lines: Vec<&str> = rest.lines().collect();
    let mut i = 0;

    // Anything before the first heading belongs to no section, and is kept so
    // nothing is silently eaten.
    let mut preamble = Vec::new();
    while i < lines.len() && heading(lines[i]).is_none() {
        preamble.push(lines[i].to_string());
        i += 1;
    }
    doc.preamble = dedent_block(&preamble);

    // `path` tracks the most recently added section; its depth is path.len() - 1.
    let mut path: NodePath = Vec::new();
    let mut body: Vec<String> = Vec::new();

    while i < lines.len() {
        if let Some((depth, title)) = heading(lines[i]) {
            flush_body(&mut doc, &path, &mut body);
            let node = Node::new(title);
            path = if path.is_empty() {
                doc.push_root(node)
            } else {
                // A heading may never be more than one level deeper than the
                // one before it; jumping from # to ### is clamped rather than
                // rejected.
                let depth = depth.min(path.len());
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
            body.push(lines[i].to_string());
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
    if let Some(title) = &doc.title {
        out.push_str("---\ntitle: ");
        out.push_str(title.trim());
        out.push_str("\n---\n\n");
    }
    if !doc.preamble.trim().is_empty() {
        out.push_str(doc.preamble.trim_end());
        out.push_str("\n\n");
    }
    for root in &doc.roots {
        emit(root, 0, &mut out);
    }
    if !doc.notes.is_empty() {
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&serialize_notes(&doc.notes));
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
    let hashes = "#".repeat((depth + 1).min(6));
    let title = node.title.trim();
    out.push_str(&hashes);
    if !title.is_empty() {
        out.push(' ');
        out.push_str(title);
    }
    out.push('\n');
    // A blank line after the heading, which is what Markdown wants and what
    // keeps a heading from swallowing the line beneath it.
    out.push('\n');

    if !node.body.trim().is_empty() {
        for line in node.body.lines() {
            out.push_str(line.trim_end());
            out.push('\n');
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

    const DOC: &str = "\
---
title: Jude
---

# Greeting and occasion

Placeholder opening sentence for the first section.

## Those who crept in

Placeholder sentence belonging to the nested section.

- an ordinary list bullet
- another one

# The charge

Placeholder closing sentence.
";

    #[test]
    fn frontmatter_carries_the_title() {
        let d = parse(DOC);
        assert_eq!(d.title.as_deref(), Some("Jude"));
    }

    #[test]
    fn hashes_give_the_depth() {
        let d = parse(DOC);
        assert_eq!(d.roots.len(), 2);
        assert_eq!(d.roots[0].title, "Greeting and occasion");
        assert_eq!(d.roots[0].children.len(), 1);
        assert_eq!(d.roots[0].children[0].title, "Those who crept in");
        assert_eq!(d.roots[1].title, "The charge");
    }

    #[test]
    fn list_bullets_are_ordinary_prose_now() {
        // The whole point of the change: `-` carries no structure.
        let d = parse(DOC);
        let nested = &d.roots[0].children[0];
        assert!(nested.children.is_empty(), "a list must not become sections");
        assert!(nested.body.contains("- an ordinary list bullet"));
        assert!(nested.body.contains("- another one"));
    }

    #[test]
    fn body_is_flush_left() {
        let d = parse(DOC);
        assert_eq!(d.roots[1].body, "Placeholder closing sentence.");
        assert!(!d.roots[1].body.starts_with(' '));
    }

    #[test]
    fn serialize_is_byte_exact() {
        assert_eq!(serialize(&parse(DOC)), DOC);
    }

    #[test]
    fn round_trip_is_idempotent() {
        for src in [DOC, "# a\n", "", "# a\n\n## b\n\n### c\n"] {
            let once = serialize(&parse(src));
            assert_eq!(once, serialize(&parse(&once)), "not idempotent for {src:?}");
        }
    }

    #[test]
    fn a_blank_line_follows_every_heading() {
        // Without it Markdown reads the next line as part of the heading.
        let mut d = Document::default();
        d.push_root(Node { title: "T".into(), body: "b".into(), children: vec![] });
        assert_eq!(serialize(&d), "# T\n\nb\n");
    }

    #[test]
    fn a_heading_with_no_blank_line_after_it_still_parses() {
        let d = parse("# T\nb\n");
        assert_eq!(d.roots[0].body, "b");
    }

    #[test]
    fn skipped_levels_are_clamped_not_lost() {
        let d = parse("# a\n\n### b\n");
        assert_eq!(d.roots.len(), 1);
        assert_eq!(d.roots[0].children[0].title, "b");
    }

    #[test]
    fn climbing_back_out_several_levels() {
        let d = parse("# a\n\n## b\n\n### c\n\n# d\n");
        assert_eq!(d.roots.len(), 2);
        assert_eq!(d.roots[1].title, "d");
        assert_eq!(d.roots[0].children[0].children[0].title, "c");
    }

    #[test]
    fn no_frontmatter_means_no_title() {
        let d = parse("# a\n");
        assert!(d.title.is_none());
        assert_eq!(serialize(&d), "# a\n");
    }

    #[test]
    fn an_unopened_fence_is_just_text() {
        // A stray --- must not swallow the document.
        let d = parse("---\nnot really frontmatter\n\n# a\n");
        assert!(d.title.is_none());
        assert_eq!(d.roots.len(), 1);
    }

    #[test]
    fn seven_hashes_is_not_a_heading() {
        let d = parse("# a\n\n####### still body\n");
        assert_eq!(d.roots.len(), 1);
        assert!(d.roots[0].body.contains("#######"));
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        let d = parse("# a\n\n#hashtag\n");
        assert_eq!(d.roots.len(), 1);
        assert_eq!(d.roots[0].body, "#hashtag");
    }

    #[test]
    fn an_unnamed_section_writes_a_bare_hash() {
        let mut d = Document::default();
        d.push_root(Node::new(""));
        assert_eq!(serialize(&d), "#\n");
        assert_eq!(parse("#\n").roots.len(), 1);
        assert_eq!(parse("#\n").roots[0].title, "");
    }

    #[test]
    fn preamble_is_preserved_not_eaten() {
        let src = "A note before any heading.\n\n# a\n";
        let d = parse(src);
        assert_eq!(d.preamble, "A note before any heading.");
        assert_eq!(serialize(&d), src);
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let d = parse("");
        assert!(d.is_empty());
        assert_eq!(serialize(&d), "");
    }

    #[test]
    fn interior_blank_lines_in_a_body_survive() {
        let d = parse("# a\n\none\n\ntwo\n");
        assert_eq!(d.roots[0].body, "one\n\ntwo");
        assert_eq!(serialize(&d), "# a\n\none\n\ntwo\n");
    }

    /// Deleting a heading line folds its text up into the section above, which
    /// is how two sections get merged by hand.
    #[test]
    fn deleting_a_heading_merges_its_text_upwards() {
        let two = parse("# One\n\nfirst text\n\n# Two\n\nsecond text\n");
        assert_eq!(two.roots.len(), 2);
        let merged = parse("# One\n\nfirst text\n\nsecond text\n");
        assert_eq!(merged.roots.len(), 1);
        assert_eq!(merged.roots[0].body, "first text\n\nsecond text");
    }

    // ---- notes ------------------------------------------------------------

    const WITH_NOTES: &str = "\
# Introduction

Paul calls himself a ==servant==[^n1] first.

[^n1]: The word is stronger than it looks in English.

[^n2]: > An invented sentence standing in for a quoted paragraph.
    >
    > — A. Author, Some Commentary, p. 52
";

    #[test]
    fn notes_are_read_from_the_foot_of_the_file() {
        let d = parse(WITH_NOTES);
        assert_eq!(d.notes.len(), 2);
        let one = d.note("n1").unwrap();
        assert_eq!(one.kind, NoteKind::Comment);
        assert!(one.source.is_none());
    }

    #[test]
    fn a_quotation_keeps_its_source_separate() {
        let two = parse(WITH_NOTES).note("n2").cloned().unwrap();
        assert_eq!(two.kind, NoteKind::Quotation);
        assert_eq!(two.text, "An invented sentence standing in for a quoted paragraph.");
        assert_eq!(two.source.as_deref(), Some("A. Author, Some Commentary, p. 52"));
    }

    #[test]
    fn notes_do_not_leak_into_the_last_section_body() {
        let d = parse(WITH_NOTES);
        assert_eq!(d.roots[0].body, "Paul calls himself a ==servant==[^n1] first.");
        assert!(!d.roots[0].body.contains("[^n1]:"));
    }

    #[test]
    fn notes_round_trip_byte_for_byte() {
        assert_eq!(serialize(&parse(WITH_NOTES)), WITH_NOTES);
    }

    #[test]
    fn a_quotation_without_a_source_round_trips() {
        let src = "# a\n\n[^q]: > Invented placeholder sentence.\n";
        assert_eq!(serialize(&parse(src)), src);
        assert!(parse(src).note("q").unwrap().source.is_none());
    }

    #[test]
    fn a_multi_line_comment_keeps_its_shape() {
        let src = "# a\n\n[^c]: First line of my own note.\n    Second line of it.\n";
        let d = parse(src);
        assert_eq!(d.note("c").unwrap().text, "First line of my own note.\nSecond line of it.");
        assert_eq!(serialize(&d), src);
    }

    #[test]
    fn sources_are_offered_once_each_and_sorted() {
        let mut d = Document::default();
        for (i, src) in ["B. Writer, Later Work", "A. Author, Title", "A. Author, Title"]
            .iter()
            .enumerate()
        {
            d.notes.push(Note {
                id: format!("n{i}"),
                kind: NoteKind::Quotation,
                text: "x".into(),
                source: Some(src.to_string()),
            });
        }
        assert_eq!(d.sources(), ["A. Author, Title", "B. Writer, Later Work"]);
    }

    #[test]
    fn fresh_ids_skip_the_ones_in_use() {
        let mut d = Document::default();
        assert_eq!(d.fresh_note_id(), "n1");
        d.notes.push(Note::comment("n1", "x"));
        d.notes.push(Note::comment("n2", "y"));
        assert_eq!(d.fresh_note_id(), "n3");
    }
}
