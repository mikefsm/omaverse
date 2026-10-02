//! The library sidebar: every outline under the configured directory, grouped by
//! immediate subfolder. Mirrors how `Biblical Studies/` is already organised
//! (New Testament / Old Testament / ...).

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
    fn title_prefers_the_heading_then_the_filename() {
        let d = tmp("titles");
        std::fs::write(d.join("a.md"), "# Romans\n\n- x\n").unwrap();
        std::fs::write(d.join("first_peter.md"), "- x\n").unwrap();
        assert_eq!(title_of(&d.join("a.md")), "Romans");
        assert_eq!(title_of(&d.join("first_peter.md")), "first peter");
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
    fn missing_directory_is_empty_not_an_error() {
        assert!(scan(Path::new("/nonexistent/outlines")).is_empty());
    }
}
