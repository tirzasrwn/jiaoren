//! Persisted view preferences: zoom and theme plus each document's reading
//! position.
//!
//! Plain `key = value` lines, so no config crate is needed. Unknown keys and
//! malformed values are ignored, and a missing or unreadable file just means
//! defaults.

use std::path::{Path, PathBuf};

use crate::mermaid::Theme;

/// Zoom range the app itself enforces; a hand-edited file cannot escape it.
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 3.0;

/// What a document opens with.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub zoom: f32,
    /// `None` when the file has no usable theme line.
    pub theme: Option<Theme>,
    /// Reading positions per document, most recent first. Bounded by the app,
    /// which trims it to [`MAX_POSITIONS`](crate::app::MAX_POSITIONS).
    pub positions: Vec<(PathBuf, f32)>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            theme: None,
            positions: Vec::new(),
        }
    }
}

impl Prefs {
    /// Saved vertical offset for `path`, when this document was seen before.
    pub fn resume_scroll(&self, path: &Path) -> Option<f32> {
        self.positions
            .iter()
            .find(|(saved, _)| saved == path)
            .map(|(_, scroll)| *scroll)
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

/// Persist `zoom`, `theme` and every remembered document position.
/// Best effort: a read-only config dir must never break the viewer.
pub fn save(zoom: f32, theme: Theme, positions: &[(PathBuf, f32)]) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let tmp = PathBuf::from(format!("{}.{}.tmp", path.display(), std::process::id()));
    if std::fs::write(&tmp, render(zoom, theme, positions)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn parse(text: &str) -> Prefs {
    let mut prefs = Prefs::default();
    // Legacy one-document keys, folded into `positions` after the loop.
    let mut legacy: Option<(PathBuf, f32)> = None;
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
            "pos" => {
                // `pos = <scroll> <path>`; the path may contain spaces.
                let Some((scroll, path)) = value.split_once(' ') else {
                    continue;
                };
                let path = path.trim();
                if path.is_empty() {
                    continue;
                }
                if let Ok(scroll) = scroll.parse::<f32>() {
                    if scroll.is_finite() && scroll >= 0.0 {
                        prefs.positions.push((PathBuf::from(path), scroll));
                    }
                }
            }
            "last_path" => {
                if !value.is_empty() {
                    legacy = Some((PathBuf::from(value), legacy.map_or(0.0, |l| l.1)));
                }
            }
            "last_scroll" => {
                if let Some((_, scroll)) = legacy.as_mut() {
                    if let Ok(value) = value.parse::<f32>() {
                        if value.is_finite() && value >= 0.0 {
                            *scroll = value;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = legacy {
        if !prefs.positions.iter().any(|(path, _)| path == &entry.0) {
            prefs.positions.push(entry);
        }
    }
    prefs
}

fn render(zoom: f32, theme: Theme, positions: &[(PathBuf, f32)]) -> String {
    let mut text = format!(
        "zoom = {:.3}\ntheme = {}\n",
        zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        theme.as_str()
    );
    for (path, scroll) in positions {
        text.push_str(&format!(
            "pos = {:.1} {}\n",
            scroll.max(0.0),
            path.to_string_lossy()
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{parse, render, Prefs, MAX_ZOOM, MIN_ZOOM};
    use crate::mermaid::Theme;

    #[test]
    fn round_trips_zoom_and_theme() {
        let text = render(1.25, Theme::Dark, &[]);
        let prefs = parse(&text);
        assert_eq!(prefs.zoom, 1.25);
        assert_eq!(prefs.theme, Some(Theme::Dark));
        assert_eq!(
            parse(&render(0.8, Theme::Light, &[])).theme,
            Some(Theme::Light)
        );
    }

    #[test]
    fn round_trips_every_document_position() {
        let positions = vec![
            (std::path::PathBuf::from("/tmp/notes.md"), 123.5),
            (std::path::PathBuf::from("/tmp/with space.md"), 9.0),
        ];
        let prefs = parse(&render(1.0, Theme::Dark, &positions));
        assert_eq!(prefs.positions, positions);
        assert_eq!(
            prefs.resume_scroll(std::path::Path::new("/tmp/notes.md")),
            Some(123.5)
        );
        assert_eq!(
            prefs.resume_scroll(std::path::Path::new("/tmp/with space.md")),
            Some(9.0)
        );
        assert_eq!(
            prefs.resume_scroll(std::path::Path::new("/tmp/other.md")),
            None
        );
    }

    #[test]
    fn old_single_document_lines_still_load() {
        let prefs =
            parse("zoom = 1.5\ntheme = dark\nlast_path = /tmp/notes.md\nlast_scroll = 42\n");
        assert_eq!(prefs.zoom, 1.5);
        assert_eq!(
            prefs.resume_scroll(std::path::Path::new("/tmp/notes.md")),
            Some(42.0)
        );
    }

    #[test]
    fn malformed_position_lines_are_ignored() {
        let prefs = parse("pos = NaN /tmp/a.md\npos = -3 /tmp/b.md\npos = 12\npos = \n");
        assert!(prefs.positions.is_empty());
        let prefs = parse("pos = 7 /tmp/notes.md\n");
        assert_eq!(
            prefs.resume_scroll(std::path::Path::new("/tmp/notes.md")),
            Some(7.0)
        );
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
