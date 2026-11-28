//! Persisted view preferences: zoom and theme plus each document's reading
//! position.
//!
//! State lives in the SQLite store. The legacy `state.toml` is parsed once for
//! migration and then removed; nothing writes TOML any more.

use std::path::{Path, PathBuf};

use crate::mermaid::Theme;
use crate::store::{self, State};

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

impl From<State> for Prefs {
    fn from(state: State) -> Self {
        Self {
            zoom: clamp_zoom(state.zoom),
            theme: state.theme.as_deref().and_then(theme_from_token),
            positions: state.positions,
        }
    }
}

impl From<&Prefs> for State {
    fn from(prefs: &Prefs) -> Self {
        Self {
            zoom: clamp_zoom(prefs.zoom),
            theme: prefs.theme.map(|theme| theme.as_str().to_owned()),
            positions: prefs.positions.clone(),
        }
    }
}

fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

fn theme_from_token(token: &str) -> Option<Theme> {
    match token {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

/// `$XDG_CONFIG_HOME/jiaoren/state.toml`, else `$HOME/.config/...`. Only read
/// during migration; `None` when neither variable is set.
fn legacy_path() -> Option<PathBuf> {
    let base = if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(base.join("jiaoren").join("state.toml"))
}

/// The saved preferences, or defaults when there is no database and no legacy
/// file.
pub fn load() -> Prefs {
    if let Some(state) = store::load_state() {
        return Prefs::from(state);
    }
    let Some(path) = legacy_path() else {
        return Prefs::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Prefs::default();
    };
    let prefs = parse(&text);
    if store::save_state(&State::from(&prefs)) {
        let _ = std::fs::remove_file(&path);
    }
    prefs
}

/// Persist `zoom`, `theme` and every remembered document position.
/// Best effort: an unwritable database must never break the viewer.
pub fn save(zoom: f32, theme: Theme, positions: &[(PathBuf, f32)]) {
    let prefs = Prefs {
        zoom,
        theme: Some(theme),
        positions: positions.to_vec(),
    };
    store::save_state(&State::from(&prefs));
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
                if let Some(theme) = theme_from_token(value) {
                    prefs.theme = Some(theme);
                }
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

#[cfg(test)]
mod tests {
    use super::{parse, Prefs, MAX_ZOOM, MIN_ZOOM};
    use crate::mermaid::Theme;
    use crate::store::State;
    use std::path::PathBuf;

    #[test]
    fn legacy_toml_parses_into_store_state() {
        let text = "zoom = 1.5\ntheme = dark\npos = 123.5 /tmp/notes.md\n\
                    last_path = /tmp/old.md\nlast_scroll = 42\n";
        let prefs = parse(text);
        let state = State::from(&prefs);
        assert_eq!(state.zoom, 1.5);
        assert_eq!(state.theme.as_deref(), Some("dark"));
        assert_eq!(state.positions, prefs.positions);
        assert_eq!(state.positions.len(), 2);
    }

    #[test]
    fn state_conversion_preserves_theme_zoom_and_positions() {
        for theme in [None, Some(Theme::Light), Some(Theme::Dark)] {
            let prefs = Prefs {
                zoom: 1.25,
                theme,
                positions: vec![
                    (PathBuf::from("/a.md"), 10.0),
                    (PathBuf::from("/b.md"), 20.0),
                ],
            };
            let state = State::from(&prefs);
            assert_eq!(state.zoom, 1.25);
            assert_eq!(state.theme.as_deref(), theme.map(Theme::as_str));
            assert_eq!(Prefs::from(state), prefs);
        }
    }

    #[test]
    fn state_conversion_clamps_zoom_and_drops_unknown_theme() {
        let prefs = Prefs::from(State {
            zoom: MAX_ZOOM * 10.0,
            theme: Some("sepia".into()),
            positions: Vec::new(),
        });
        assert_eq!(prefs.zoom, MAX_ZOOM);
        assert_eq!(prefs.theme, None);
        let prefs = Prefs::from(State {
            zoom: f32::NAN,
            theme: None,
            positions: Vec::new(),
        });
        assert_eq!(prefs.zoom, 1.0);
    }

    #[test]
    fn legacy_toml_parses_zoom_and_theme() {
        let prefs = parse("zoom = 1.25\ntheme = dark\n");
        assert_eq!(prefs.zoom, 1.25);
        assert_eq!(prefs.theme, Some(Theme::Dark));
        assert_eq!(
            parse("zoom = 0.8\ntheme = light\n").theme,
            Some(Theme::Light)
        );
    }

    #[test]
    fn legacy_toml_parses_every_document_position() {
        let positions = vec![
            (std::path::PathBuf::from("/tmp/notes.md"), 123.5),
            (std::path::PathBuf::from("/tmp/with space.md"), 9.0),
        ];
        let prefs = parse(
            "zoom = 1\ntheme = dark\npos = 123.5 /tmp/notes.md\npos = 9 /tmp/with space.md\n",
        );
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
