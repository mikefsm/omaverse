//! Follow the current omarchy theme.
//!
//! Themes publish a palette at `<state>/omarchy/current/theme/colors.toml`, and
//! switching theme rewrites it. The palette is mapped onto libadwaita's named
//! colours, which restyles the whole of the chrome without touching any of the
//! typography -- the serif body and the outline keep the sizes they have and
//! simply inherit the themed foreground.

use std::collections::HashMap;
use std::path::PathBuf;

pub struct Palette {
    colors: HashMap<String, String>,
    pub dark: bool,
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::state::home().join(".local/state"))
}

pub fn theme_dir() -> PathBuf {
    state_dir().join("omarchy/current/theme")
}

/// The active theme's name, used to notice a theme switch.
pub fn current_name() -> Option<String> {
    std::fs::read_to_string(state_dir().join("omarchy/current/theme.name"))
        .ok()
        .map(|s| s.trim().to_string())
}

impl Palette {
    pub fn load() -> Option<Palette> {
        let src = std::fs::read_to_string(theme_dir().join("colors.toml")).ok()?;
        let parsed: toml::Table = toml::from_str(&src).ok()?;
        let mut colors = HashMap::new();
        let mut dark = true;
        for (k, v) in parsed {
            let Some(s) = v.as_str() else { continue };
            if k == "mode" {
                dark = s != "light";
            } else if s.starts_with('#') {
                // Skips entries like hyprland_active_border, which are not plain
                // colours.
                colors.insert(k, s.to_string());
            }
        }
        if colors.is_empty() {
            return None;
        }
        Some(Palette { colors, dark })
    }

    fn get<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.colors.get(key).map(|s| s.as_str()).unwrap_or(fallback)
    }

    /// Override libadwaita's named colours. Nothing here sets a font.
    pub fn css(&self) -> String {
        let bg = self.get("background", if self.dark { "#1d1d20" } else { "#fafafa" });
        let fg = self.get("foreground", if self.dark { "#ffffff" } else { "#000000" });
        let dim = self.get("dark_foreground", fg);
        let chrome = self.get("dark_background", bg);
        let deep = self.get("darker_background", chrome);
        let raised = self.get("lighter_background", bg);
        let accent = self.get("accent", self.get("blue", "#3584e4"));
        let selection = self.get("selection", raised);
        let muted = self.get("muted", raised);
        let red = self.get("red", "#e01b24");

        format!(
            "\
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {chrome};
@define-color headerbar_fg_color {fg};
@define-color headerbar_border_color {muted};
@define-color headerbar_backdrop_color {deep};
@define-color sidebar_bg_color {deep};
@define-color sidebar_fg_color {fg};
@define-color sidebar_backdrop_color {deep};
@define-color sidebar_border_color {muted};
@define-color secondary_sidebar_bg_color {deep};
@define-color secondary_sidebar_fg_color {fg};
@define-color card_bg_color {raised};
@define-color card_fg_color {fg};
@define-color dialog_bg_color {raised};
@define-color dialog_fg_color {fg};
@define-color popover_bg_color {raised};
@define-color popover_fg_color {fg};
@define-color accent_color {accent};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {bg};
@define-color destructive_color {red};
@define-color destructive_bg_color {red};
@define-color destructive_fg_color {bg};
@define-color borders {muted};
@define-color theme_selected_bg_color {accent};
@define-color theme_selected_fg_color {bg};

/* Selected rows read as the accent; the dim colour carries group headings. */
.oma-group {{ color: {dim}; }}
.oma-error {{ color: {red}; }}
textview text selection {{ background-color: {selection}; color: {fg}; }}
listview > row:selected,
listbox > row:selected {{ background-color: {accent}; color: {bg}; }}
"
        )
    }
}
