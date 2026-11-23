//! Keyboard mapping: turn one pressed key into a viewer [`Action`], with no egui
//! context and no I/O so the mapping stays unit-testable.

use std::time::{Duration, Instant};

use egui::{Key, Modifiers};

/// Keys pressed on the keyboard, with no egui Context dependency (unit-testable).
#[derive(Default)]
pub struct KeyState {
    /// Instant of a pending first `g`, if any.
    pub last_g: Option<Instant>,
    /// Instant of a pending first `y`, if any.
    pub last_y: Option<Instant>,
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
    /// Show the open document's path in the status line (`Ctrl+G`).
    ShowPath,
    /// Toggle the keyboard-shortcut overlay (`?`).
    ToggleKeysHelp,
    /// Copy the whole document as Markdown.
    CopyDocument,
    /// Open the `/` search command line.
    SearchOpen,
    /// Jump to the next search match, wrapping.
    SearchNext,
    /// Jump to the previous search match, wrapping.
    SearchPrev,
    /// Copy the active text selection.
    CopySelection,
    /// Clear the active search, cancel the prompt, or quit when neither applies.
    CancelOrQuit,
    /// Close the window.
    Quit,
}

/// Result of Esc/Ctrl+C, which share one cancel-then-quit ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelAction {
    /// Close the `/` prompt and discard its buffer.
    CancelPrompt,
    /// Copy the drag selection, exactly like `y`.
    CopySelection,
    /// Clear the committed pattern, hits and highlights.
    ClearSearch,
    /// Close the window.
    Quit,
}

/// Route Esc/Ctrl+C: prompt, then copy the selection, then clear search, then
/// quit. Ctrl+C copies selected text the way every other app does, so a drag
/// selection is never thrown away by the usual copy key.
pub fn cancel_action(
    prompt_open: bool,
    search_active: bool,
    selection_active: bool,
) -> CancelAction {
    if prompt_open {
        CancelAction::CancelPrompt
    } else if selection_active {
        CancelAction::CopySelection
    } else if search_active {
        CancelAction::ClearSearch
    } else {
        CancelAction::Quit
    }
}

/// Fresh-press guard for Ctrl+C's `Event::Copy`. egui-winit discards key-repeat
/// information for Copy events, so while Ctrl stays held every event looks like
/// a new press. One Copy is accepted per Ctrl hold; releasing Ctrl re-arms it.
#[derive(Default)]
pub struct CopyGuard {
    held: bool,
    saw_release: bool,
}

impl CopyGuard {
    /// Sync with the current Ctrl/Cmd state. Re-arm only on a fresh press after
    /// a release frame, so a Copy event queued just after key-up cannot act as
    /// a second press.
    pub fn observe_modifiers(&mut self, ctrl_held: bool) {
        if !ctrl_held {
            self.saw_release = true;
        } else if self.saw_release {
            self.held = false;
            self.saw_release = false;
        }
    }

    /// True for the first Copy of a Ctrl press, false for autorepeats and for a
    /// Copy left over after the key was released.
    pub fn accept(&mut self) -> bool {
        if self.held {
            false
        } else {
            self.held = true;
            true
        }
    }
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
        // Ctrl+G shows the file path; a bare `g` still starts the `gg` window.
        Key::G if modifiers.command || modifiers.ctrl => Some(Action::ShowPath),
        Key::Questionmark => Some(Action::ToggleKeysHelp),
        Key::C if modifiers.command || modifiers.ctrl => Some(Action::CancelOrQuit),
        Key::Q => Some(Action::Quit),
        // Esc and Ctrl+C share the cancel prompt → clear search → quit ladder.
        Key::Escape => Some(Action::CancelOrQuit),
        Key::Slash if modifiers.is_none() => Some(Action::SearchOpen),
        Key::N if modifiers.matches_exact(Modifiers::SHIFT) => Some(Action::SearchPrev),
        Key::N if modifiers.matches_exact(Modifiers::NONE) => Some(Action::SearchNext),
        Key::Y if !modifiers.any() => {
            // Starting a `y` sequence breaks a pending `gg`.
            state.last_g = None;
            match state.last_y.take() {
                Some(t) if now.duration_since(t) < GG_WINDOW => Some(Action::CopyDocument),
                // A `y` with no recent predecessor (including one after the
                // window elapsed) arms a fresh `yy`; a single `y` never copies.
                _ => {
                    state.last_y = Some(now);
                    None
                }
            }
        }
        Key::G => {
            // Starting a `g` sequence breaks a pending `yy`.
            state.last_y = None;
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
        fn double_y_copies_the_document() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            assert_eq!(apply_key(&mut s, Key::Y, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(200)
                ),
                Some(Action::CopyDocument)
            );
            assert!(s.last_y.is_none());
        }

        #[test]
        fn single_y_does_not_copy() {
            let mut s = KeyState::default();
            assert_eq!(
                apply_key(&mut s, Key::Y, Modifiers::NONE, std::time::Instant::now()),
                None
            );
            // Shift+Y is not a copy chord either.
            assert_eq!(
                apply_key(&mut s, Key::Y, Modifiers::SHIFT, std::time::Instant::now()),
                None
            );
        }

        #[test]
        fn expired_y_rearms_instead_of_copying() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            assert_eq!(apply_key(&mut s, Key::Y, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(700)
                ),
                None
            );
            assert!(s.last_y.is_some());
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(900)
                ),
                Some(Action::CopyDocument)
            );
        }

        #[test]
        fn yy_window_boundary_is_exclusive() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            assert_eq!(apply_key(&mut s, Key::Y, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(499)
                ),
                Some(Action::CopyDocument)
            );

            let mut s = KeyState::default();
            assert_eq!(apply_key(&mut s, Key::Y, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(500)
                ),
                None
            );
            assert!(s.last_y.is_some());
        }

        #[test]
        fn g_then_y_then_g_neither_copies_nor_jumps() {
            let mut s = KeyState::default();
            let t0 = std::time::Instant::now();
            assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::Y,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(100)
                ),
                None
            );
            assert_eq!(
                apply_key(
                    &mut s,
                    Key::G,
                    Modifiers::NONE,
                    t0 + std::time::Duration::from_millis(200)
                ),
                None
            );
            assert!(s.last_g.is_some());
            assert!(s.last_y.is_none());
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
        fn ctrl_c_cancels_or_quits_but_plain_c_does_nothing() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::C, Modifiers::COMMAND, now),
                Some(Action::CancelOrQuit)
            );
            assert_eq!(
                apply_key(&mut s, Key::C, Modifiers::CTRL, now),
                Some(Action::CancelOrQuit)
            );
            assert_eq!(apply_key(&mut s, Key::C, Modifiers::NONE, now), None);
        }

        #[test]
        fn cancel_action_is_prompt_then_copy_then_search_then_quit() {
            assert_eq!(
                cancel_action(true, false, false),
                CancelAction::CancelPrompt
            );
            assert_eq!(cancel_action(true, true, true), CancelAction::CancelPrompt);
            // A live selection is copied, not discarded, by Ctrl+C.
            assert_eq!(
                cancel_action(false, true, true),
                CancelAction::CopySelection
            );
            assert_eq!(
                cancel_action(false, false, true),
                CancelAction::CopySelection
            );
            assert_eq!(cancel_action(false, true, false), CancelAction::ClearSearch);
            assert_eq!(cancel_action(false, false, false), CancelAction::Quit);
        }

        #[test]
        fn ctrl_g_shows_the_path_and_questionmark_the_key_help() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::G, Modifiers::CTRL, now),
                Some(Action::ShowPath)
            );
            assert_eq!(
                apply_key(&mut s, Key::G, Modifiers::COMMAND, now),
                Some(Action::ShowPath)
            );
            assert_eq!(
                apply_key(&mut s, Key::Questionmark, Modifiers::SHIFT, now),
                Some(Action::ToggleKeysHelp)
            );
            // A bare `g` still arms `gg`.
            assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, now), None);
        }

        #[test]
        fn copy_guard_ignores_held_repeats_and_rearms_after_release() {
            let mut guard = CopyGuard::default();
            // First Copy of a Ctrl hold acts.
            guard.observe_modifiers(true);
            assert!(guard.accept());
            // Autorepeats in later frames look identical; only the first acts.
            for _ in 0..3 {
                guard.observe_modifiers(true);
                assert!(!guard.accept());
            }
            // Enter+Copy batch routing uses the prompt-open snapshot: Copy is
            // processed first, so the prompt rung is the one that acts.
            assert_eq!(cancel_action(true, true, true), CancelAction::CancelPrompt);
            // Releasing Ctrl re-arms: a queued autorepeat after key-up is still
            // ignored; the next fresh press may advance the ladder.
            guard.observe_modifiers(false);
            assert!(!guard.accept());
            guard.observe_modifiers(true);
            assert!(guard.accept());
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

        #[test]
        fn slash_opens_search() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::Slash, Modifiers::NONE, now),
                Some(Action::SearchOpen)
            );
            // `?` (Shift+/) is not a search opener.
            assert_eq!(apply_key(&mut s, Key::Slash, Modifiers::SHIFT, now), None);
        }

        #[test]
        fn n_and_upper_n_step_matches() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::N, Modifiers::NONE, now),
                Some(Action::SearchNext)
            );
            assert_eq!(
                apply_key(&mut s, Key::N, Modifiers::SHIFT, now),
                Some(Action::SearchPrev)
            );
            // Ctrl+N and Alt+N are not match keys either.
            assert_eq!(apply_key(&mut s, Key::N, Modifiers::CTRL, now), None);
            assert_eq!(apply_key(&mut s, Key::N, Modifiers::ALT, now), None);
            assert_eq!(
                apply_key(&mut s, Key::N, Modifiers::CTRL | Modifiers::SHIFT, now),
                None
            );
        }

        #[test]
        fn escape_is_not_a_direct_quit_action() {
            let mut s = KeyState::default();
            let now = std::time::Instant::now();
            assert_eq!(
                apply_key(&mut s, Key::Escape, Modifiers::NONE, now),
                Some(Action::CancelOrQuit)
            );
            assert_eq!(
                apply_key(&mut s, Key::Q, Modifiers::NONE, now),
                Some(Action::Quit)
            );
        }
    }
}
