//! Keyboard mapping: turn one pressed key into a viewer [`Action`], with no egui
//! context and no I/O so the mapping stays unit-testable.

use std::time::{Duration, Instant};

use egui::{Key, Modifiers};

/// Keys pressed on the keyboard, with no egui Context dependency (unit-testable).
#[derive(Default)]
pub struct KeyState {
    /// Instant of a pending first `g`, if any.
    pub last_g: Option<Instant>,
}

/// A viewer action produced by a key press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Scroll vertically by that many text lines.
    ScrollLines(f32),
    /// Scroll vertically by that many viewport pages.
    ScrollPages(f32),
    /// Scroll horizontally by that many columns.
    ScrollCols(f32),
    /// Jump to the top of the document.
    Top,
    /// Jump to the bottom of the document.
    Bottom,
    /// Increase the zoom factor.
    ZoomIn,
    /// Decrease the zoom factor.
    ZoomOut,
    /// Reset the zoom and horizontal scroll.
    Fit,
    /// Reset the zoom factor only.
    ResetZoom,
    /// Switch between the light and dark themes.
    ToggleTheme,
    /// Re-read the document from disk.
    Reload,
    /// Close the window.
    Quit,
}

/// How long a first `g` waits for a second one.
const GG_WINDOW: Duration = Duration::from_millis(500);

/// Window after the diagram popup closes during which the next quit-triggering
/// event is ignored. A held `Ctrl+C` is handled by extending this guard while
/// the Ctrl key stays down (see `popup_closed_with_ctrl`).
const QUIT_SUPPRESS_WINDOW: f64 = 0.30;

/// Whether quit input at input time `now` belongs to the press that just closed
/// the popup. Pure, so the guard is unit-testable.
pub fn quit_suppressed(now: f64, closed_at: Option<f64>) -> bool {
    closed_at.is_some_and(|t| now - t < QUIT_SUPPRESS_WINDOW)
}

/// Map one pressed key to an [`Action`]. Pure: no egui context, no I/O.
pub fn apply_key(
    state: &mut KeyState,
    key: Key,
    modifiers: Modifiers,
    now: Instant,
) -> Option<Action> {
    match key {
        Key::J | Key::ArrowDown => Some(Action::ScrollLines(1.0)),
        Key::K | Key::ArrowUp => Some(Action::ScrollLines(-1.0)),
        Key::H => Some(Action::ScrollCols(-1.0)),
        Key::L => Some(Action::ScrollCols(1.0)),
        Key::ArrowLeft if modifiers.shift => Some(Action::ScrollCols(-1.0)),
        Key::ArrowRight if modifiers.shift => Some(Action::ScrollCols(1.0)),
        Key::D => Some(Action::ScrollPages(0.5)),
        Key::U => Some(Action::ScrollPages(-0.5)),
        Key::Space => Some(Action::ScrollPages(1.0)),
        Key::B => Some(Action::ScrollPages(-1.0)),
        Key::Plus | Key::Equals => Some(Action::ZoomIn),
        Key::Minus => Some(Action::ZoomOut),
        Key::A => Some(Action::Fit),
        Key::Num0 => Some(Action::ResetZoom),
        Key::T => Some(Action::ToggleTheme),
        Key::R => Some(Action::Reload),
        Key::C if modifiers.command || modifiers.ctrl => Some(Action::Quit),
        Key::Q | Key::Escape => Some(Action::Quit),
        Key::G => {
            // Shift+G (bottom) is immediate; a lone `g` starts the gg window.
            if modifiers.shift {
                state.last_g = None;
                return Some(Action::Bottom);
            }
            match state.last_g.take() {
                Some(t) if now.duration_since(t) < GG_WINDOW => Some(Action::Top),
                // A `g` with no recent predecessor (including one after the
                // window elapsed) arms a fresh `gg`.
                _ => {
                    state.last_g = Some(now);
                    None
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    mod keys {
        use super::super::*;

        #[test]
        fn j_and_k_scroll_lines() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert!(
                matches!(apply_key(&mut s, Key::J, Modifiers::NONE, now), Some(Action::ScrollLines(v)) if v > 0.0)
            );
            assert!(
                matches!(apply_key(&mut s, Key::K, Modifiers::NONE, now), Some(Action::ScrollLines(v)) if v < 0.0)
            );
        }

        #[test]
        fn double_g_jumps_to_top_only() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::G,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(200)
                ),
                Some(Action::Top)
            );
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::G,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(700)
                ),
                None
            );
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::G,
                    Modifiers::SHIFT,
                    t0 + std::time::Duration::from_millis(2300)
                ),
                Some(Action::Bottom)
            );
        }

        #[test]
        fn shifted_g_jumps_to_bottom_and_clears_pending_g() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            // A lone `g` arms the window...
            assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
            // ...and Shift+G is bottom immediately, dropping the pending `g`.
            assert_eq!(
                apply_key(&mut s, Key::G, Modifiers::SHIFT, t0),
                Some(Action::Bottom)
            );
            assert!(s.last_g.is_none());
            // The cleared state means the next `g` re-arms rather than going top.
            assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
        }

        #[test]
        fn h_and_l_pan_horizontally() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert!(
                matches!(apply_key(&mut s, Key::H, Modifiers::NONE, now), Some(Action::ScrollCols(v)) if v < 0.0)
            );
            assert!(
                matches!(apply_key(&mut s, Key::L, Modifiers::NONE, now), Some(Action::ScrollCols(v)) if v > 0.0)
            );
        }

        #[test]
        fn shifted_arrows_pan_horizontally() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert!(
                matches!(apply_key(&mut s, Key::ArrowLeft, Modifiers::SHIFT, now), Some(Action::ScrollCols(v)) if v < 0.0)
            );
            assert!(
                matches!(apply_key(&mut s, Key::ArrowRight, Modifiers::SHIFT, now), Some(Action::ScrollCols(v)) if v > 0.0)
            );
            // Unshifted arrows keep their vertical meaning.
            assert!(apply_key(&mut s, Key::ArrowLeft, Modifiers::NONE, now).is_none());
        }

        #[test]
        fn zoom_keys_map_to_zoom_actions() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::Plus, Modifiers::NONE, now),
                Some(Action::ZoomIn)
            );
            assert_eq!(
                apply_key(&mut s, Key::Equals, Modifiers::NONE, now),
                Some(Action::ZoomIn)
            );
            assert_eq!(
                apply_key(&mut s, Key::Minus, Modifiers::NONE, now),
                Some(Action::ZoomOut)
            );
            assert_eq!(
                apply_key(&mut s, Key::A, Modifiers::NONE, now),
                Some(Action::Fit)
            );
        }

        #[test]
        fn zero_resets_zoom_without_scrolling() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::Num0, Modifiers::NONE, now),
                Some(Action::ResetZoom)
            );
        }

        #[test]
        fn quit_is_suppressed_only_just_after_popup_closes() {
            assert!(!quit_suppressed(10.0, None));
            assert!(quit_suppressed(10.1, Some(10.0)));
            assert!(!quit_suppressed(10.4, Some(10.0)));
        }

        #[test]
        fn ctrl_c_quits_but_plain_c_does_nothing() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::C, Modifiers::COMMAND, now),
                Some(Action::Quit)
            );
            assert_eq!(
                apply_key(&mut s, Key::C, Modifiers::CTRL, now),
                Some(Action::Quit)
            );
            assert_eq!(apply_key(&mut s, Key::C, Modifiers::NONE, now), None);
        }

        #[test]
        fn t_r_q_map_to_actions() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::T, Modifiers::NONE, now),
                Some(Action::ToggleTheme)
            );
            assert_eq!(
                apply_key(&mut s, Key::R, Modifiers::NONE, now),
                Some(Action::Reload)
            );
            assert_eq!(
                apply_key(&mut s, Key::Q, Modifiers::NONE, now),
                Some(Action::Quit)
            );
            assert_eq!(
                apply_key(&mut s, Key::Space, Modifiers::NONE, now),
                Some(Action::ScrollPages(1.0))
            );
            assert_eq!(
                apply_key(&mut s, Key::A, Modifiers::NONE, now),
                Some(Action::Fit)
            );
        }
    }
}
