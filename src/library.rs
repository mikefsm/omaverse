//! The library sidebar: every outline under the configured directory, grouped by
//! immediate subfolder. Mirrors how `Biblical Studies/` is already organised
//! (New Testament / Old Testament / ...).

use crate::reference::Reference;
use std::path::{Path, PathBuf};

/// What kind of document a file holds. An outline is prose, an interlinear is a
/// record set, and a diagram is a drawing; they are not the same thing. The
/// extension separates prose from the rest, and the file's own `kind` key
/// separates the two sorts of TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Outline,
    Interlinear,
    Diagram,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()? {
        "md" => Some(Kind::Outline),
        "toml" => match read_head(path, 512) {
            Ok(head) if crate::diagram::is_diagram(&head) => Some(Kind::Diagram),
            _ => Some(Kind::Interlinear),
        },
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// `None` for files sitting directly in the outline directory.
    pub name: Option<String>,
    pub entries: Vec<Entry>,
}

/// A document's display title: its `# ` heading if it has one, else a tidied
/// filename. Only the head of the file is read.
pub fn title_of(path: &Path) -> String {
    if let Ok(s) = read_head(path, 4096) {
        if matches!(kind_of(path), Some(Kind::Interlinear) | Some(Kind::Diagram)) {
            for line in s.lines().take(10) {
                if let Some(rest) = line.trim().strip_prefix("reference") {
                    if let Some(value) = rest.trim().strip_prefix('=') {
                        let value = value.trim().trim_matches('"').trim();
                        if !value.is_empty() {
                            return value.to_string();
                        }
                    }
                }
            }
        }
        // An outline names itself in its frontmatter. Falling straight to the
        // first heading would label the document by its opening section, which
        // is a different thing and often misleading.
        if let Some(title) = front_title(&s) {
            return title;
        }
        for line in s.lines().take(20) {
            if let Some(rest) = line.strip_prefix("# ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            if line.trim_start().starts_with("- ") {
                break; // past the heading region
            }
        }
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
        .unwrap_or_else(|| "Untitled".into())
}

/// The `title:` of a leading `---` frontmatter block, if there is one.
fn front_title(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        if line.trim_end() == "---" {
            return None;
        }
        if let Some(rest) = line.trim().strip_prefix("title:") {
            let t = rest.trim().trim_matches('"').trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn read_head(path: &Path, limit: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Depth-limited so a stray symlink or a deep tree can't stall startup.
const MAX_DEPTH: usize = 4;

pub fn scan(root: &Path) -> Vec<Group> {
    let mut ungrouped: Vec<Entry> = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    collect(root, root, 0, &mut ungrouped, &mut groups);

    for g in &mut groups {
        g.entries.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    }
    groups.sort_by(|a, b| {
        a.name.as_deref().unwrap_or("").to_lowercase().cmp(&b.name.as_deref().unwrap_or("").to_lowercase())
    });
    ungrouped.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let mut out = Vec::new();
    if !ungrouped.is_empty() {
        out.push(Group { name: None, entries: ungrouped });
    }
    out.extend(groups.into_iter().filter(|g| !g.entries.is_empty()));
    out
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    ungrouped: &mut Vec<Entry>,
    groups: &mut Vec<Group>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, depth + 1, ungrouped, groups);
        } else if let Some(kind) = kind_of(&path) {
            let entry = Entry { title: title_of(&path), path: path.clone(), kind };
            // Group by the first path component below the root.
            match path.strip_prefix(root).ok().and_then(|r| r.parent()).and_then(|p| p.components().next()) {
                Some(c) => {
                    let g = c.as_os_str().to_string_lossy().to_string();
                    match groups.iter_mut().find(|x| x.name.as_deref() == Some(&g)) {
                        Some(existing) => existing.entries.push(entry),
                        None => groups.push(Group { name: Some(g), entries: vec![entry] }),
                    }
                }
                None => ungrouped.push(entry),
            }
        }
    }
}


/// A passage one document covers. An outline contributes one of these per
/// section that names a passage; a sheet or a diagram contributes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Covered {
    pub path: PathBuf,
    pub kind: Kind,
    /// The document's own name, for showing in a list.
    pub title: String,
    /// The section, when the passage is part of a larger document.
    pub section: Option<String>,
    pub reference: Reference,
}

impl Covered {
    /// How it reads in a list: "Jude — Those who crept in".
    pub fn label(&self) -> String {
        match &self.section {
            Some(s) => format!("{} — {s}", self.title),
            None => self.title.clone(),
        }
    }
}

/// Everything in the library that names a passage.
///
/// References are read from the files rather than kept in a database: there is
/// no index to fall out of step, and at the size of a personal library the
/// whole scan costs a few milliseconds.
pub fn covered(root: &Path) -> Vec<Covered> {
    let mut out = Vec::new();
    for group in scan(root) {
        for entry in group.entries {
            collect_references(&entry, &mut out);
        }
    }
    out
}

fn collect_references(entry: &Entry, out: &mut Vec<Covered>) {
    let Ok(text) = std::fs::read_to_string(&entry.path) else { return };
    match entry.kind {
        Kind::Outline => {
            // The book is usually named once, by the document; a section says
            // only its chapter and verse.
            let book = Reference::parse(&entry.title).and_then(|r| r.book).or_else(|| {
                (!entry.title.trim().is_empty()).then(|| entry.title.clone())
            });
            for (section, text) in crate::docview::section_references(&text) {
                let Some(mut reference) = Reference::parse(&text) else { continue };
                if reference.book.is_none() {
                    reference.book = book.clone();
                }
                out.push(Covered {
                    path: entry.path.clone(),
                    kind: entry.kind,
                    title: entry.title.clone(),
                    section: Some(section),
                    reference,
                });
            }
        }
        Kind::Interlinear | Kind::Diagram => {
            // Their reference is the title: it is what they are named by.
            if let Some(reference) = Reference::parse(&entry.title) {
                out.push(Covered {
                    path: entry.path.clone(),
                    kind: entry.kind,
                    title: entry.title.clone(),
                    section: None,
                    reference,
                });
            }
        }
    }
}

/// What else covers any of the same ground, nearest first. The document asking
/// is left out of its own answer.
pub fn related(all: &[Covered], to: &Reference, from: &Path) -> Vec<Covered> {
    let mut found: Vec<Covered> = all
        .iter()
        .filter(|c| c.path != from && c.reference.overlaps(to))
        .cloned()
        .collect();
    // A reference inside the one asked about comes before one that merely
    // brushes it, and a sheet or diagram before a prose section.
    found.sort_by_key(|c| {
        let exact = c.reference != *to;
        let enclosed = !to.contains(&c.reference);
        let kind = matches!(c.kind, Kind::Outline);
        (exact, enclosed, kind, c.label())
    });
    found
}


/// Somewhere a search term turned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub kind: Kind,
    pub title: String,
    /// Where to put the cursor on opening, for documents that have lines.
    pub line: Option<usize>,
    /// The words around the match, for showing in a list.
    pub context: String,
}

/// Find a run of characters anywhere in the library.
///
/// Matching is case-insensitive and on plain substrings: Greek and Hebrew are
/// searched by the letters you type, which is what you have to hand when the
/// word is in front of you. Interlinears and diagrams are searched as the
/// records they are, not as the TOML they are stored in, so a search never
/// matches a key name or an id.
pub fn search(root: &Path, query: &str) -> Vec<Hit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for group in scan(root) {
        for entry in group.entries {
            search_one(&entry, &needle, &mut out);
        }
    }
    out
}

fn search_one(entry: &Entry, needle: &str, out: &mut Vec<Hit>) {
    let Ok(text) = std::fs::read_to_string(&entry.path) else { return };
    match entry.kind {
        Kind::Outline => {
            let kinds = crate::docview::classify(&text);
            for (i, line) in text.lines().enumerate() {
                // The frontmatter is bookkeeping and a reference marker is a
                // label; neither is the document's text.
                if matches!(
                    kinds.get(i),
                    Some(crate::docview::Line::Front) | Some(crate::docview::Line::Ref)
                ) {
                    continue;
                }
                // A heading is shown as it reads, not as it is written.
                let shown = match kinds.get(i) {
                    Some(crate::docview::Line::Heading { marker, .. }) => {
                        line.get(*marker..).unwrap_or(line)
                    }
                    _ => line,
                };
                if shown.to_lowercase().contains(needle) {
                    out.push(Hit {
                        path: entry.path.clone(),
                        kind: entry.kind,
                        title: entry.title.clone(),
                        line: Some(i),
                        context: around(shown, needle),
                    });
                }
            }
        }
        Kind::Interlinear => {
            let Ok(sheet) = crate::interlinear::Interlinear::parse(&text) else { return };
            for word in &sheet.words {
                let mut parts = vec![word.text.clone()];
                parts.extend(
                    ["gloss", "lemma", "parse", "note"]
                        .iter()
                        .filter_map(|f| word.field(f).map(|v| v.to_string())),
                );
                let joined = parts.join("  ");
                if joined.to_lowercase().contains(needle) {
                    out.push(Hit {
                        path: entry.path.clone(),
                        kind: entry.kind,
                        title: entry.title.clone(),
                        line: None,
                        context: joined,
                    });
                }
            }
        }
        Kind::Diagram => {
            let Ok(diagram) = crate::diagram::Diagram::parse(&text) else { return };
            for label in &diagram.labels {
                if label.text.to_lowercase().contains(needle) {
                    out.push(Hit {
                        path: entry.path.clone(),
                        kind: entry.kind,
                        title: entry.title.clone(),
                        line: None,
                        context: label.text.clone(),
                    });
                }
            }
        }
    }
}

/// Enough of the line to recognise the match, centred on it.
fn around(line: &str, needle: &str) -> String {
    const REACH: usize = 48;
    let hay = line.to_lowercase();
    let Some(at) = hay.find(needle) else { return line.trim().to_string() };
    // Byte offsets from `find` land on character boundaries of the lowercased
    // text; step out to boundaries of the original before slicing.
    let start = (0..=at.saturating_sub(REACH))
        .rev()
        .find(|i| line.is_char_boundary(*i))
        .unwrap_or(0);
    let want = (at + needle.len() + REACH).min(line.len());
    let end = (want..=line.len())
        .find(|i| line.is_char_boundary(*i))
        .unwrap_or(line.len());
    let mut s = line[start..end].trim().to_string();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < line.len() {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test directory: tests run in parallel, so keying only on the pid
    /// makes them delete each other's fixtures.
    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("omaverse-lib-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn title_prefers_the_frontmatter_then_the_heading_then_the_filename() {
        let d = tmp("titles");
        std::fs::write(d.join("a.md"), "# Romans\n\n- x\n").unwrap();
        std::fs::write(d.join("first_peter.md"), "- x\n").unwrap();
        // The document is Jude; "Greeting" is only where it starts.
        std::fs::write(
            d.join("jude.md"),
            "---\ntitle: Jude\n---\n\n# Greeting\n\nProse.\n",
        )
        .unwrap();
        assert_eq!(title_of(&d.join("a.md")), "Romans");
        assert_eq!(title_of(&d.join("first_peter.md")), "first peter");
        assert_eq!(title_of(&d.join("jude.md")), "Jude");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scan_groups_by_subfolder_and_skips_non_markdown() {
        let d = tmp("scan");
        std::fs::create_dir_all(d.join("New Testament")).unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        std::fs::write(d.join("loose.md"), "# Loose\n").unwrap();
        std::fs::write(d.join("New Testament/romans.md"), "# Romans\n").unwrap();
        std::fs::write(d.join("New Testament/john.md"), "# John\n").unwrap();
        std::fs::write(d.join("notes.docx"), "x").unwrap();
        std::fs::write(d.join(".hidden/secret.md"), "# Secret\n").unwrap();

        let groups = scan(&d);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, None);
        assert_eq!(groups[0].entries[0].title, "Loose");
        assert_eq!(groups[1].name.as_deref(), Some("New Testament"));
        let titles: Vec<_> = groups[1].entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["John", "Romans"], "entries sort by title");
        let _ = std::fs::remove_dir_all(&d);
    }


    #[test]
    fn a_section_reference_takes_the_book_from_its_document() {
        let d = tmp("covered");
        std::fs::write(
            d.join("jude.md"),
            "---\ntitle: Jude\n---\n\n# Greeting\n<!-- ref: 1-2 -->\n\nProse.\n\n\
             ## Those who crept in\n<!-- ref: 3-4 -->\n\nMore.\n",
        )
        .unwrap();
        std::fs::write(
            d.join("sheet.toml"),
            "reference = \"Jude 4\"\nlanguage = \"greek\"\n",
        )
        .unwrap();

        let all = covered(&d);
        assert_eq!(all.len(), 3, "two sections and one sheet");

        let greeting = all.iter().find(|c| c.section.as_deref() == Some("Greeting")).unwrap();
        assert_eq!(greeting.reference.book.as_deref(), Some("Jude"), "from the title");
        assert_eq!(greeting.label(), "Jude — Greeting");

        let sheet = all.iter().find(|c| c.kind == Kind::Interlinear).unwrap();
        assert_eq!(sheet.reference.book.as_deref(), Some("Jude"));
        assert_eq!(sheet.label(), "Jude 4");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn related_finds_the_overlap_and_leaves_out_the_asker() {
        let d = tmp("related");
        std::fs::write(
            d.join("jude.md"),
            "---\ntitle: Jude\n---\n\n# Greeting\n<!-- ref: 1-2 -->\n\nProse.\n\n\
             ## Those who crept in\n<!-- ref: 3-4 -->\n\nMore.\n",
        )
        .unwrap();
        std::fs::write(
            d.join("sheet.toml"),
            "reference = \"Jude 4\"\nlanguage = \"greek\"\n",
        )
        .unwrap();
        let all = covered(&d);

        // Asking from the sheet about Jude 4.
        let asking = Reference::parse("Jude 4").unwrap();
        let found = related(&all, &asking, &d.join("sheet.toml"));
        let labels: Vec<String> = found.iter().map(|c| c.label()).collect();
        assert_eq!(labels, ["Jude — Those who crept in"], "verse 4 is in 3-4, not 1-2");

        // Asking from the outline about the whole letter.
        let whole = Reference::parse("Jude 1-25").unwrap();
        let found = related(&all, &whole, &d.join("nothing.md"));
        assert_eq!(found.len(), 3, "everything overlaps it");

        // A different book does not match, however the numbers line up.
        let elsewhere = Reference::parse("Romans 4").unwrap();
        assert!(related(&all, &elsewhere, &d.join("nothing.md")).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }


    fn searchable(name: &str) -> PathBuf {
        let d = tmp(name);
        std::fs::write(
            d.join("jude.md"),
            r#"---
title: Jude
---

# Greeting
<!-- ref: 1-2 -->

Beloved in God the Father, and kept for Jesus Christ.

## Mercy

Mercy to you, and peace, and love.
"#,
        )
        .unwrap();
        std::fs::write(
            d.join("sheet.toml"),
            "reference = \"Jude 2\"\nlanguage = \"greek\"\n\n[[words]]\nid = \"w1\"\n             text = \"\u{3b5}\u{1f30}\u{3c1}\u{3ae}\u{3bd}\u{3b7}\"\ngloss = \"peace\"\n",
        )
        .unwrap();
        d
    }

    #[test]
    fn search_finds_prose_and_gives_the_line_to_open_at() {
        let d = searchable("search");
        let hits = search(&d, "beloved");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Jude");
        assert_eq!(hits[0].line, Some(7), "the line it is on");
        assert!(hits[0].context.contains("Beloved"), "got {}", hits[0].context);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn search_reads_a_sheet_as_words_not_as_toml() {
        let d = searchable("searchsheet");
        // A gloss is findable.
        let hits = search(&d, "peace");
        let kinds: Vec<Kind> = hits.iter().map(|h| h.kind).collect();
        assert!(kinds.contains(&Kind::Interlinear), "the sheet's gloss");
        assert!(kinds.contains(&Kind::Outline), "and the prose");

        // The Greek itself is findable by its own letters.
        let hits = search(&d, "\u{3b5}\u{1f30}\u{3c1}");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, Kind::Interlinear);

        // Key names and ids are not text.
        assert!(search(&d, "language").is_empty(), "a TOML key is not content");
        assert!(search(&d, "w1").is_empty(), "nor is a word id");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn search_ignores_case_and_apparatus_and_empty_queries() {
        let d = searchable("searchcase");
        let hits = search(&d, "MERCY");
        assert_eq!(hits.len(), 2, "heading and prose, either case");
        assert!(
            hits.iter().all(|h| !h.context.starts_with('#')),
            "a heading reads as it reads, not as it is written"
        );
        assert!(search(&d, "   ").is_empty(), "an empty query finds nothing");
        // The reference marker and the frontmatter are not the document's text.
        assert!(search(&d, "ref:").is_empty());
        assert!(search(&d, "title:").is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn context_is_trimmed_around_the_match_on_character_boundaries() {
        let long = format!("{}\u{3b1}\u{3b2}\u{3b3} needle \u{3b4}\u{3b5}{}", "x".repeat(200), "y".repeat(200));
        let out = around(&long, "needle");
        assert!(out.starts_with('\u{2026}') && out.ends_with('\u{2026}'), "got {out}");
        assert!(out.contains("needle"));
        assert!(out.chars().count() < 120);
    }

    #[test]
    fn missing_directory_is_empty_not_an_error() {
        assert!(scan(Path::new("/nonexistent/outlines")).is_empty());
    }
}
