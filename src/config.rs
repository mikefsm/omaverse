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
        Config {
            outline_dir: crate::state::home()
                .join("Dropbox/Documents/Biblical Studies/Outlines"),
        }
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
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let body = format!(
                "# omaverse configuration\n\
                 # Directory scanned for outlines. Subfolders become sidebar groups.\n\
                 outline_dir = {}\n",
                toml::Value::from(cfg.outline_dir.to_string_lossy().to_string())
            );
            let _ = std::fs::write(&path, body);
            cfg
        }
    }
}
