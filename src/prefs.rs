//! Persisted view preferences: zoom and theme, shared by every document.
//!
//! Plain `key = value` lines, so no config crate is needed. Unknown keys and
//! malformed values are ignored, and a missing or unreadable file just means
//! defaults.

use std::path::PathBuf;

use crate::mermaid::Theme;

/// Zoom range the app itself enforces; a hand-edited file cannot escape it.
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 3.0;

/// What a document opens with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prefs {
    pub zoom: f32,
    /// `None` when the file has no usable theme line.
    pub theme: Option<Theme>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            theme: None,
        }
    }
}

/// `$XDG_CONFIG_HOME/jiaoren/state.toml`, else `$HOME/.config/...`. `None` when
/// neither variable is set, in which case preferences are simply not persisted.
fn state_path() -> Option<PathBuf> {
    let base = if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(base.join("jiaoren").join("state.toml"))
}

/// The saved preferences, or defaults when there is no file.
pub fn load() -> Prefs {
    let Some(path) = state_path() else {
        return Prefs::default();
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(_) => Prefs::default(),
    }
}

/// Persist `zoom` and `theme`. Best effort: a read-only config dir must never
/// break the viewer.
pub fn save(zoom: f32, theme: Theme) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let tmp = PathBuf::from(format!("{}.{}.tmp", path.display(), std::process::id()));
    if std::fs::write(&tmp, render(zoom, theme)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn parse(text: &str) -> Prefs {
    let mut prefs = Prefs::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "zoom" => {
                if let Ok(zoom) = value.parse::<f32>() {
                    if zoom.is_finite() {
                        prefs.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
                    }
                }
            }
            "theme" => {
                prefs.theme = match value {
                    "light" => Some(Theme::Light),
                    "dark" => Some(Theme::Dark),
                    _ => prefs.theme,
                };
            }
            _ => {}
        }
    }
    prefs
}

fn render(zoom: f32, theme: Theme) -> String {
    format!(
        "zoom = {:.3}\ntheme = {}\n",
        zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        theme.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::{parse, render, Prefs, MAX_ZOOM, MIN_ZOOM};
    use crate::mermaid::Theme;

    #[test]
    fn round_trips_zoom_and_theme() {
        let text = render(1.25, Theme::Dark);
        let prefs = parse(&text);
        assert_eq!(prefs.zoom, 1.25);
        assert_eq!(prefs.theme, Some(Theme::Dark));
        assert_eq!(parse(&render(0.8, Theme::Light)).theme, Some(Theme::Light));
    }

    #[test]
    fn missing_or_broken_lines_fall_back_to_defaults() {
        assert_eq!(parse(""), Prefs::default());
        assert_eq!(parse("zoom = banana\ntheme = sepia\n"), Prefs::default());
        assert_eq!(parse("garbage\n= =\n"), Prefs::default());
    }

    #[test]
    fn zoom_is_clamped_and_non_finite_rejected() {
        assert_eq!(
            parse(&format!("zoom = {}\n", MAX_ZOOM * 10.0)).zoom,
            MAX_ZOOM
        );
        assert_eq!(
            parse(&format!("zoom = {}\n", MIN_ZOOM / 10.0)).zoom,
            MIN_ZOOM
        );
        assert_eq!(parse("zoom = NaN\nzoom = inf\n").zoom, 1.0);
    }
}
