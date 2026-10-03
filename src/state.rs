//! App-owned view state, deliberately kept out of the Markdown files.
//!
//! Lives under `~/.local/state/omaverse/` so it never reaches Dropbox: fold
//! state is per-machine and disposable. Every failure here is swallowed — if the
//! sidecar is missing or corrupt the app opens with everything expanded, which
//! is a fine outcome and never worth an error dialog.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DocState {
    /// Index-path keys ("0.2.1") of collapsed nodes.
    #[serde(default)]
    pub collapsed: HashSet<String>,
    #[serde(default)]
    pub selected: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub paned: Option<i32>,
    #[serde(default)]
    pub sidebar_open: Option<bool>,
    #[serde(default)]
    pub last_file: Option<PathBuf>,
    /// Most-recently-opened first. Outlines can be saved anywhere, so files
    /// outside the scanned directory would otherwise become unreachable.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
}

/// Cap on remembered recent files.
pub const RECENT_MAX: usize = 10;

/// Move `path` to the front, de-duplicating, and trim to `RECENT_MAX`.
pub fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(RECENT_MAX);
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("omaverse")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// FNV-1a. Only needs to disambiguate two files with the same stem, so a
/// non-cryptographic hash is the right tool and saves a dependency.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A readable, collision-resistant sidecar filename for a document path.
fn doc_key(doc: &Path) -> String {
    let stem: String = doc
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".into())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{}-{:016x}.json", stem, fnv1a(&doc.to_string_lossy()))
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(value) {
        let _ = crate::atomic_write(path, s.as_bytes());
    }
}

pub fn load_doc_state(doc: &Path) -> DocState {
    read_json(&state_dir().join("docs").join(doc_key(doc)))
}

pub fn save_doc_state(doc: &Path, st: &DocState) {
    write_json(&state_dir().join("docs").join(doc_key(doc)), st);
}

/// Drop what was remembered about a document. Called when it goes, so a new
/// file that later takes the same name does not inherit its folds.
pub fn forget_doc(doc: &Path) {
    let _ = std::fs::remove_file(state_dir().join("docs").join(doc_key(doc)));
}

/// Where a side of a conflict is parked before the other one wins. Deliberately
/// outside the outline directory: it is a safety net, not a document, and should
/// not appear in the library or sync anywhere.
pub fn conflict_backup(doc: &Path, side: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = doc
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "outline".into());
    state_dir().join("conflicts").join(format!("{stem}-{side}-{stamp}.md"))
}

pub fn load_window_state() -> WindowState {
    read_json(&state_dir().join("window.json"))
}

pub fn save_window_state(st: &WindowState) {
    write_json(&state_dir().join("window.json"), st);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_recent_moves_to_front_without_duplicating() {
        let mut r = vec![PathBuf::from("/b"), PathBuf::from("/a")];
        push_recent(&mut r, Path::new("/a"));
        assert_eq!(r, [PathBuf::from("/a"), PathBuf::from("/b")]);
        push_recent(&mut r, Path::new("/c"));
        assert_eq!(r, [PathBuf::from("/c"), PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn push_recent_is_capped() {
        let mut r = Vec::new();
        for i in 0..(RECENT_MAX + 5) {
            push_recent(&mut r, Path::new(&format!("/f{i}")));
        }
        assert_eq!(r.len(), RECENT_MAX);
        assert_eq!(r[0], PathBuf::from(&format!("/f{}", RECENT_MAX + 4)), "newest first");
    }

    #[test]
    fn doc_key_is_stable_and_readable() {
        let k = doc_key(Path::new("/home/m/Outlines/romans.md"));
        assert!(k.starts_with("romans-"));
        assert!(k.ends_with(".json"));
        assert_eq!(k, doc_key(Path::new("/home/m/Outlines/romans.md")));
    }

    #[test]
    fn same_stem_in_different_folders_does_not_collide() {
        let a = doc_key(Path::new("/o/New Testament/romans.md"));
        let b = doc_key(Path::new("/o/Old Testament/romans.md"));
        assert_ne!(a, b);
    }

    #[test]
    fn awkward_filenames_are_sanitized() {
        let k = doc_key(Path::new("/o/Romans 6 — Outlined!.md"));
        assert!(k.starts_with("Romans_6___Outlined_-"), "{k}");
    }

    #[test]
    fn missing_sidecar_yields_defaults_not_an_error() {
        let st: DocState = read_json(Path::new("/nonexistent/nope.json"));
        assert!(st.collapsed.is_empty());
        assert!(st.selected.is_none());
    }
}
