//! Configuration. One setting today; a file so there's somewhere to put the next.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Scanned recursively for `.md` files to populate the library sidebar.
    pub outline_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config { outline_dir: crate::state::home().join("Documents/Omaverse") }
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::state::home().join(".config"))
        .join("omaverse/config.toml")
}

/// Load config, writing a commented default on first run so the file is
/// discoverable rather than something you have to know to create.
pub fn load() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
            eprintln!("omaverse: {} is invalid ({e}); using defaults", path.display());
            Config::default()
        }),
        Err(_) => {
            let cfg = Config::default();
            // First run: make the folder as well as the file. An empty library
            // pointing at a directory that does not exist is a poor welcome,
            // and the alternative is every new user editing TOML before they
            // can write anything.
            let _ = std::fs::create_dir_all(&cfg.outline_dir);
            cfg.save();
            cfg
        }
    }
}

impl Config {
    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = format!(
            "# omaverse configuration\n\
             # The library: scanned for outlines, interlinears and diagrams.\n\
             # Subfolders become groups in the sidebar.\n\
             # Changeable from the app: the document menu, \"Library folder…\".\n\
             outline_dir = {}\n",
            toml::Value::from(self.outline_dir.to_string_lossy().to_string())
        );
        if let Err(e) = crate::atomic_write(&path, body.as_bytes()) {
            eprintln!("omaverse: could not save {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_library_is_not_anyone_in_particulars_folder() {
        let dir = Config::default().outline_dir;
        let shown = dir.to_string_lossy();
        assert!(shown.ends_with("Documents/Omaverse"), "got {shown}");
        assert!(!shown.contains("Dropbox"), "no one else has that folder");
    }
}
