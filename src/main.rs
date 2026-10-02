mod app;
mod config;
mod edit;
mod library;
mod model;
mod nodeobj;
mod parse;
mod state;

use gtk4::prelude::*;
use libadwaita as adw;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "org.mikefsm.omaverse";

/// Write via a temp file in the same directory, then rename.
///
/// Dropbox watches the outline directory, so it must never observe a
/// half-written file: rename is atomic within a filesystem, a plain write is not.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let stem = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{stem}.omaverse-tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

fn main() -> glib::ExitCode {
    let cli: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);

    let app = adw::Application::builder().application_id(APP_ID).build();
    // The file is taken from argv directly rather than through GApplication's
    // open handler, which keeps startup to a single path.
    app.connect_activate(move |a| app::build(a, cli.clone()));
    app.run_with_args::<&str>(&[])
}

use gtk4::glib;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("omaverse-aw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("a.md");
        atomic_write(&f, b"one").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "one");
        atomic_write(&f, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_creates_missing_directories() {
        let dir = std::env::temp_dir().join(format!("omaverse-aw2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("New Testament/romans.md");
        atomic_write(&f, b"# Romans\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "# Romans\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
