mod app;
mod config;
mod docview;
mod edit;
mod library;
mod model;
mod nodeobj;
mod parse;
mod state;
mod theme;

use gtk4::gio;
use gtk4::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
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
    let app = adw::Application::builder()
        .application_id(APP_ID)
        // HANDLES_OPEN so that `omaverse some.md` while omaverse is already
        // running opens that file in the existing window. Without it GApplication
        // hands off to the running instance and the argument is silently dropped.
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // The one window, created on first activation and reused thereafter.
    let window: Rc<RefCell<Option<Rc<app::App>>>> = Rc::new(RefCell::new(None));

    app.connect_activate({
        let w = window.clone();
        move |a| show(a, &w, None)
    });
    app.connect_open({
        let w = window.clone();
        move |a, files, _hint| {
            let path = files.first().and_then(|f| f.path());
            show(a, &w, path);
        }
    });
    app.run()
}

fn show(
    gapp: &adw::Application,
    slot: &Rc<RefCell<Option<Rc<app::App>>>>,
    path: Option<PathBuf>,
) {
    let existing = slot.borrow().clone();
    match existing {
        Some(win) => {
            if let Some(p) = path {
                win.open(&p);
            }
            win.present();
        }
        None => {
            let win = app::build(gapp, path);
            *slot.borrow_mut() = Some(win.clone());
            win.present();
        }
    }
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
