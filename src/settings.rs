//! What the settings panel changes, kept in a small JSON file between runs:
//! `%APPDATA%\gitcloner\settings.json` on Windows, `~/.config/gitcloner/`
//! elsewhere (or `$XDG_CONFIG_HOME`). Tokens are never written here.

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::ui::THEMES;

/// Rows in the settings panel: theme, animations, popup, update check,
/// start screen, review.
pub const ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Index into `ui::THEMES`; stored by name.
    pub theme: usize,
    pub motion: bool,
    /// Sum things up in a popup when a batch of clones finishes.
    pub popup: bool,
    pub update_check: bool,
    /// Open on the planet-and-menu start screen rather than the form.
    pub splash: bool,
    /// Look pasted GitHub repos up and show them in the table before cloning.
    pub review: bool,
    /// Where these live. `None` (as in tests) means never written.
    pub path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: 0, motion: true, popup: true, update_check: true, splash: true, review: true, path: None }
    }
}

fn file() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    Some(base?.join("gitcloner").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(file())
    }

    /// Missing or unreadable file, or a missing key: the default for it.
    fn load_from(path: Option<PathBuf>) -> Self {
        let v: Value = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let flag = |k: &str| v[k].as_bool().unwrap_or(true);
        Self {
            theme: v["theme"]
                .as_str()
                .and_then(|name| THEMES.iter().position(|t| t.name == name))
                .unwrap_or(0),
            motion: flag("animations"),
            popup: flag("popup_when_done"),
            update_check: flag("check_for_updates"),
            splash: flag("start_screen"),
            review: flag("review_pasted_repos"),
            path,
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let body = json!({
            "theme": THEMES[self.theme].name,
            "animations": self.motion,
            "popup_when_done": self.popup,
            "check_for_updates": self.update_check,
            "start_screen": self.splash,
            "review_pasted_repos": self.review,
        });
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("cannot save settings: {e}"))?;
        }
        let text = serde_json::to_string_pretty(&body).unwrap_or_default();
        fs::write(path, text + "\n").map_err(|e| format!("cannot save settings: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_restart() {
        let path = std::env::temp_dir().join(format!("gitcloner-settings-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        assert_eq!(Settings::load_from(Some(path.clone())), Settings { path: Some(path.clone()), ..Settings::default() });

        let s = Settings { theme: 3, motion: false, popup: false, update_check: true, splash: false, review: false, path: Some(path.clone()) };
        s.save().unwrap();
        assert_eq!(Settings::load_from(Some(path.clone())), s);

        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load_from(Some(path.clone())).theme, 0, "a broken file falls back to defaults");
        let _ = fs::remove_file(&path);
    }
}
