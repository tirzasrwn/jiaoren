//! The eframe application: window state, scrolling and zoom, and the Markdown
//! render hook that swaps placeholders for diagrams, tables, lists, headings,
//! code, images and paragraphs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify::{RecursiveMode, Watcher};

use crate::doc::{prepare, Prepared};
use crate::mermaid::{ensure_chromium_with, Renderer, Theme};
use paragraph::ParagraphUi;

mod code;
mod diagram;
mod geometry;
mod heading;
mod image;
mod jumplist;
mod keys;
mod list;
mod paragraph;
mod placeholder;
mod quote;
mod raster;
mod searchui;
mod selection;
mod table;
mod toc;
mod widgets;

use code::CodeUi;
use diagram::DiagramUi;
use geometry::{
    bottom_pad, content_column, edge_scroll, frame_scroll_x, home_scroll, home_target,
    jump_scroll_top, overscroll_pad, reading_progress, scroll_by_columns, scroll_by_lines,
};
use heading::HeadingUi;
use image::ImageUi;
use jumplist::JumpList;
use keys::{
    apply_key_event, cancel_action, quit_suppressed, Action, CancelAction, CopyGuard, KeyState,
};
use list::ListUi;
use placeholder::{parse, Placeholder};
use searchui::SearchUi;
use selection::{cursor_for, selected_text};
use table::TableUi;

/// Re-export of [`crate::doc::read_doc`] for the app's reload path.
pub use crate::doc::read_doc;

/// Lock a mutex, recovering the guard if a previous holder panicked. Poisoning
/// must never deadlock the UI thread or silently drop queued renders.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run a worker body, turning a panic into an error result so the caller's
/// `active_workers` and `pending` bookkeeping always settles. `label` names the
/// failed job in the message ("decode panicked", "raster panicked", ...).
pub(crate) fn catch_worker<T>(label: &'static str, f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| format!("{label} panicked"))
}

/// Apply the app theme to egui so the window chrome and the mermaid renders
/// agree. `set_theme` selects the style; `set_visuals` guarantees the exact
/// light/dark palette even when the OS preference is the other one.
fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let visuals = crate::theme::visuals(theme);
    let egui_theme = if visuals.dark_mode {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(egui_theme);
    ctx.set_visuals(visuals);
}

/// Text styles scaled by the document zoom. Pure so the scaling can be tested
/// without a window; the app caches the result by zoom and DPI.
fn scaled_text_styles(
    base: &std::collections::BTreeMap<egui::TextStyle, egui::FontId>,
    zoom: f32,
) -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    base.iter()
        .map(|(kind, font)| {
            (
                kind.clone(),
                egui::FontId::new(font.size * zoom, font.family.clone()),
            )
        })
        .collect()
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Whether a filesystem event path refers to the open document.
fn same_file(event: &Path, doc: &Path) -> bool {
    if event == doc {
        return true;
    }
    if event.file_name() != doc.file_name() {
        return false;
    }
    // Compare absolute forms; canonicalization may fail if the file was just
    // deleted, in which case a matching file name is good enough.
    let abs = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    abs(event) == abs(doc)
}

/// First-run download progress shown in the status line.
enum DownloadMsg {
    Status(String),
    Done(Result<(), String>),
}

/// Renders an HTML placeholder through the diagram, table or paragraph UI.
type HtmlFn = Box<dyn Fn(&mut egui::Ui, &str)>;

/// A transient clipboard confirmation: one plain line, bottom-left, drawn over
/// the window margin like zathura's status bar.
struct Status {
    message: String,
    until: Instant,
}

/// How long a clipboard status line stays on screen.
const STATUS_DURATION: Duration = Duration::from_millis(1500);

/// How long `Ctrl+G`'s file path stays on screen; a path needs longer to read
/// than a one-word clipboard confirmation.
const PATH_STATUS_DURATION: Duration = Duration::from_millis(4000);

/// How many document positions the state file remembers. `]`/`[` restores the
/// files it has seen, oldest entries fall off first.
pub(crate) const MAX_POSITIONS: usize = 32;

/// Whether a link click this frame asked for the `link copied` chip.
fn take_link_copied(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<()>(egui::Id::new(crate::justify::LINK_COPIED)))
        .is_some()
}

/// The link under the pointer, if the document painted one since the last
/// frame. Stale entries cannot fire: the document paint clears this entry at the
/// start of every frame (see the `CommonMarkViewer` block in [`App::ui`]) before
/// anything reads it.
fn take_hovered_link(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new(crate::justify::HOVER_LINK)))
}

/// Markdown files in `dir`, sorted by name. Only plain files with a Markdown
/// extension are offered.
fn documents_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
                    })
        })
        .collect();
    files.sort();
    files
}

/// The file `delta` steps from `current` in `files`, wrapping at both ends.
fn step_document(files: &[PathBuf], current: &Path, delta: isize) -> Option<PathBuf> {
    if files.len() < 2 {
        return None;
    }
    let index = files.iter().position(|file| file == current)?;
    let len = files.len() as isize;
    let next = (index as isize + delta).rem_euclid(len) as usize;
    files.get(next).cloned()
}

/// Hand `url` to the desktop opener without a shell. `$BROWSER` wins, then the
/// usual XDG helpers; the URL is always a single argument. The child is reaped
/// on a detached thread so opening a link never blocks a frame and never leaves
/// a zombie behind.
fn open_external(url: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let spawn = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut child| {
                // Reap without blocking this frame; a failed thread just leaves
                // the child to the OS at process exit.
                let _ = std::thread::Builder::new().spawn(move || {
                    let _ = child.wait();
                });
            })
    };
    if let Some(browser) = std::env::var("BROWSER")
        .ok()
        .filter(|b| !b.trim().is_empty())
    {
        let mut parts = browser.split_whitespace();
        if let Some(program) = parts.next() {
            let args: Vec<&str> = parts.collect();
            if spawn(program, &args).is_ok() {
                return Ok(());
            }
        }
    }
    for (program, args) in [
        ("xdg-open", &[][..]),
        ("gio", &["open"][..]),
        ("sensible-browser", &[][..]),
        ("x-www-browser", &[][..]),
    ] {
        if spawn(program, args).is_ok() {
            return Ok(());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no browser opener found",
    ))
}

/// Status text shown while the first-run font download runs.
fn font_progress_text(pct: u8) -> String {
    format!("Downloading FiraCode font… {pct}% (first run only)")
}

/// Failure line, naming whatever font the window actually kept.
fn font_failure_status() -> &'static str {
    if crate::fonts::fallback().is_some() {
        "font download failed; using the system font"
    } else {
        "font download failed; using the built-in font"
    }
}

/// Solid one-line chip in a window corner, shared by the left status and the
/// right reading-progress indicator.
fn chip_area(ctx: &egui::Context, id: &str, anchor: egui::Align2, offset: egui::Vec2, text: &str) {
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Middle)
        .anchor(anchor, offset)
        .interactable(false)
        .show(ctx, |ui| {
            // Solid chip: the chip sits over the document and must stay legible.
            // `panel_fill` is the page colour here, so use the faint fill
            // (Catppuccin mantle) for contrast.
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .monospace()
                                .size(12.0)
                                .color(ui.visuals().text_color()),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        });
}

/// One-line status chip in the bottom-left corner.
fn show_status(ctx: &egui::Context, text: &str) {
    chip_area(
        ctx,
        "jiaoren-status",
        egui::Align2::LEFT_BOTTOM,
        egui::vec2(8.0, -8.0),
        text,
    );
}

/// Reading progress chip in the bottom-right corner.
fn show_progress(ctx: &egui::Context, text: &str) {
    chip_area(
        ctx,
        "jiaoren-progress",
        egui::Align2::RIGHT_BOTTOM,
        egui::vec2(-8.0, -8.0),
        text,
    );
}

/// Plain bottom-left `/query` line: monospace, on the same solid chip as the
/// status so it stays legible over the document, with a blinking block cursor
/// at the end and the query tail shown when the line is longer than the window.
fn prompt_area(ctx: &egui::Context, text: String) {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("jiaoren-search-line"))
        .order(egui::Order::Middle)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            // Same chip as the committed search status, for the same reason.
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    let color = ui.visuals().text_color();
                    let font = egui::TextStyle::Monospace.resolve(ui.style());
                    let measure = |s: &str| {
                        ui.fonts_mut(|f| {
                            f.layout_no_wrap(s.to_string(), font.clone(), color)
                                .size()
                                .x
                        })
                    };
                    let max_width = (screen.width() * 0.8).max(40.0);
                    // Keep the leading `/` (vim always shows it) and trim only
                    // the query tail, so the caret at the end stays visible.
                    let (prefix, body) = match text.strip_prefix('/') {
                        Some(body) => ("/", body),
                        None => ("", text.as_str()),
                    };
                    let shown = format!(
                        "{prefix}{}",
                        tail_within(body, max_width - measure(prefix), &measure)
                    );
                    // A block cursor after the query, blinking like a terminal.
                    let cell = measure("M");
                    let line = ui.add(
                        egui::Label::new(egui::RichText::new(shown).monospace().color(color))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    let caret = egui::Rect::from_min_size(
                        egui::Pos2::new(line.rect.right(), line.rect.top()),
                        egui::vec2(cell, line.rect.height()),
                    );
                    let time = ctx.input(|i| i.time);
                    if cursor_visible(time) {
                        ui.painter().rect_filled(caret, 0.0, color);
                    }
                    // Wake up at the next blink boundary; egui is otherwise idle.
                    let next = cursor_blink_remaining(time);
                    ctx.request_repaint_after(Duration::from_secs_f64(next));
                    // Ask the integration to route IME commits here while the
                    // command line is open; preedit is deliberately ignored.
                    let bottom = screen.bottom() - 8.0;
                    let ime_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(screen.left() + 8.0, bottom - 24.0),
                        egui::vec2(max_width, 24.0),
                    );
                    ctx.output_mut(|o| {
                        o.ime = Some(egui::output::IMEOutput {
                            purpose: egui::IMEPurpose::Normal,
                            rect: ime_rect,
                            cursor_rect: caret,
                            should_interrupt_composition: false,
                        });
                    });
                });
        });
}

/// Half-period of the command-line block cursor blink, in seconds.
const CURSOR_BLINK: f64 = 0.53;

/// Whether the block cursor is lit: on for the first half-period, then off.
fn cursor_visible(time: f64) -> bool {
    ((time / CURSOR_BLINK) as u64).is_multiple_of(2)
}

/// Seconds until the cursor next flips, clamped so a repaint is always asked
/// for (a stale or zero time must not busy-loop).
fn cursor_blink_remaining(time: f64) -> f64 {
    let next = (time / CURSOR_BLINK)
        .floor()
        .mul_add(CURSOR_BLINK, CURSOR_BLINK)
        - time;
    next.clamp(0.01, CURSOR_BLINK)
}

/// Longest suffix of `text` that fits `max_width`, cut on a char boundary.
fn tail_within(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_owned();
    }
    let mut start = 0;
    for (i, c) in text.char_indices() {
        start = i + c.len_utf8();
        if measure(&text[start..]) <= max_width {
            break;
        }
    }
    text[start..].to_owned()
}

/// One raw command-line edit in event order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SearchEdit {
    Text(String),
    Backspace,
    /// `Ctrl+W`: delete the trailing word (vim's insert-mode word erase).
    DeleteWord,
    /// `Up`: an older committed query.
    HistoryOlder,
    /// `Down`: a newer committed query, or the draft.
    HistoryNewer,
}

/// Erase one trailing word plus the whitespace before it: `"foo bar "` and
/// `"foo bar"` both become `"foo "`, matching vim's `Ctrl+W`.
fn truncate_word_tail(query: &mut String) {
    let trimmed = query.trim_end_matches(char::is_whitespace);
    let cut = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    query.truncate(cut);
}

/// One frame of raw search input, stopping at Enter so later text cannot be
/// appended to the committed pattern.
#[derive(Default)]
struct SearchBatch {
    edits: Vec<SearchEdit>,
    enter: bool,
    cancel: bool,
    copy: bool,
    /// Keys after Enter, handed to the document action path (e.g. `n`).
    after_enter: Vec<(egui::Key, egui::Modifiers)>,
}

/// Paste is a command line, not multiline text: strip CR/LF and controls.
fn sanitize_paste(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// `t` with no modifiers while an overlay owns the keyboard. The theme drives
/// the diagram the overlay draws, so the toggle stays live; every other
/// document key stays suppressed.
fn overlay_theme_toggle(events: &[egui::Event]) -> bool {
    events.iter().any(|ev| {
        matches!(
            ev,
            egui::Event::Key {
                key: egui::Key::T,
                modifiers,
                pressed: true,
                repeat: false,
                ..
            } if !modifiers.any()
        )
    })
}

fn read_search_batch(events: &[egui::Event]) -> SearchBatch {
    let mut batch = SearchBatch::default();
    let mut after_enter = false;
    for ev in events {
        if after_enter {
            if let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = ev
            {
                batch.after_enter.push((*key, *modifiers));
            }
            continue;
        }
        match ev {
            egui::Event::Text(text) if !text.is_empty() => {
                batch.edits.push(SearchEdit::Text(text.clone()));
            }
            egui::Event::Paste(text) => {
                let text = sanitize_paste(text);
                if !text.is_empty() {
                    batch.edits.push(SearchEdit::Text(text));
                }
            }
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                let text = sanitize_paste(text);
                if !text.is_empty() {
                    batch.edits.push(SearchEdit::Text(text));
                }
            }
            egui::Event::Ime(_) => {} // preedit: ignored
            egui::Event::Key {
                key: egui::Key::W,
                modifiers,
                pressed: true,
                ..
            } if modifiers.ctrl || modifiers.command => {
                batch.edits.push(SearchEdit::DeleteWord);
            }
            egui::Event::Key {
                key: egui::Key::Backspace,
                pressed: true,
                ..
            } => batch.edits.push(SearchEdit::Backspace),
            egui::Event::Key {
                key: egui::Key::ArrowUp,
                modifiers,
                pressed: true,
                ..
            } if !modifiers.any() => batch.edits.push(SearchEdit::HistoryOlder),
            egui::Event::Key {
                key: egui::Key::ArrowDown,
                modifiers,
                pressed: true,
                ..
            } if !modifiers.any() => batch.edits.push(SearchEdit::HistoryNewer),
            egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                ..
            } => {
                batch.enter = true;
                after_enter = true;
            }
            egui::Event::Key {
                key: egui::Key::Escape,
                pressed: true,
                ..
            } => {
                batch.cancel = true;
                break;
            }
            egui::Event::Key {
                key: egui::Key::C,
                modifiers,
                pressed: true,
                ..
            } if modifiers.ctrl || modifiers.command => {
                batch.cancel = true;
                break;
            }
            egui::Event::Copy => {
                batch.copy = true;
                break;
            }
            _ => {}
        }
    }
    batch
}

/// A heading to keep at the viewport top across a reload, plus how far below
/// its top the reader was.
#[derive(Clone, Debug, PartialEq)]
struct ReadingAnchor {
    heading: String,
    offset: f32,
}

/// Capture the last heading at/above `scroll_y` with its viewport offset.
/// Headings come in document order. `navigating` documents that `]`/`[` already
/// picked a saved position, so the anchor must stay unarmed.
fn reading_anchor<'a>(
    navigating: bool,
    headings: impl IntoIterator<Item = (&'a str, f32)>,
    scroll_y: f32,
) -> Option<ReadingAnchor> {
    if navigating {
        return None;
    }
    let mut anchor = None;
    for (heading, y) in headings {
        if y > scroll_y {
            break;
        }
        anchor = Some(ReadingAnchor {
            heading: heading.to_string(),
            offset: scroll_y - y,
        });
    }
    anchor
}

/// The jiaoren window: document state, theme/zoom, scrolling, and the HTML
/// render hook.
pub(crate) struct App {
    /// Path of the open document.
    pub path: PathBuf,
    /// Raw Markdown source as read from disk, for `yy`.
    source: String,
    /// Pre-parsed document text and placeholders.
    pub prepared: Prepared,
    /// Active colour theme.
    pub theme: Theme,
    /// Text and diagram zoom factor.
    pub zoom: f32,
    error: Option<String>,
    /// Why Chromium is unavailable, when auto-install failed.
    chromium_error: Option<String>,
    cache: CommonMarkCache,
    diagrams: Rc<RefCell<DiagramUi>>,
    tables: Rc<RefCell<TableUi>>,
    lists: Rc<RefCell<ListUi>>,
    paragraphs: Rc<RefCell<ParagraphUi>>,
    quotes: Rc<RefCell<quote::QuoteUi>>,
    images: Rc<RefCell<ImageUi>>,
    headings: Rc<RefCell<HeadingUi>>,
    code: Rc<RefCell<CodeUi>>,
    search: Rc<RefCell<SearchUi>>,
    html_fn: HtmlFn,
    scroll_y: f32,
    scroll_x: f32,
    /// Vertical offset to apply once after the first layout, from a saved
    /// position or from stepping to another document with `]`/`[`.
    restore_scroll: Option<f32>,
    /// Heading anchor to restore after a reload's first layout.
    reload_anchor: Option<ReadingAnchor>,
    /// Remembered scroll offsets per document, most recent first.
    positions: Vec<(std::path::PathBuf, f32)>,
    /// Content scroll limits from the last layout. Key scrolls clamp to these
    /// first, so pressing down at the bottom cannot hand egui an offset past
    /// the end for one bouncing frame.
    scroll_max: egui::Vec2,
    /// Block index the next layout must centre in the viewport (a confirmed or
    /// stepped search hit). Applied once after `home_scroll`, before the scroll
    /// area runs.
    jump_to: Option<usize>,
    /// Edge slack from the last layout, `None` until the first layout. `gg` and
    /// `a` target it before this frame's layout has run; `Some` also tells the
    /// home/resize helper whether the view is still at the reading origin.
    pad: Option<egui::Vec2>,
    /// Viewport width from the last frame, so a resize can re-home the column.
    last_view_w: Option<f32>,
    /// Set by `G`: jump to the bottom once the next layout reveals `max_y`.
    scroll_bottom: bool,
    applied_theme: Option<Theme>,
    /// Zoom/DPI key the cached scaled text styles were built for.
    styles_key: Option<(u32, i32)>,
    /// Text styles scaled by the document zoom, built only when the key changes
    /// and swapped into the panel for the duration of a frame.
    styles: std::collections::BTreeMap<egui::TextStyle, egui::FontId>,
    keys: KeyState,
    /// Vim jumplist for `Ctrl+O` / `Ctrl+I`.
    jumps: JumpList,
    /// Scroll position when `/` opened, so Enter records the pre-search place
    /// rather than the live-preview position it already jumped to.
    search_origin: Option<f32>,
    /// One Ctrl+C Copy event per Ctrl hold; autorepeats are ignored.
    copy_guard: CopyGuard,
    /// Screen position where a selection drag started, for plain-click detection.
    select_press: Option<egui::Pos2>,
    /// Pointer moved far enough during this press to count as a drag.
    select_moved: bool,
    /// The `?` keyboard-shortcut overlay is showing.
    keys_help: bool,
    /// Heading index overlay (Tab).
    toc: toc::TocState,
    /// Scroll pixels to add before this frame's layout: a selection drag held
    /// near the top or bottom of the window scrolls the document.
    autoscroll: f32,
    /// Pointer position from the last frame, so a drag that runs past the window
    /// edge keeps extending the selection instead of stalling.
    last_pointer: Option<egui::Pos2>,
    #[expect(
        dead_code,
        reason = "the watcher must stay alive for the callback to fire"
    )]
    watcher: Option<notify::RecommendedWatcher>,
    watch_rx: Option<mpsc::Receiver<notify::Event>>,
    pending_reload: Option<Instant>,
    last_mtime: Option<SystemTime>,
    last_poll: Instant,
    status: Option<Status>,
    /// First-run font download, polled once per frame until it finishes.
    font_rx: Option<mpsc::Receiver<crate::fonts::FontMsg>>,
    /// Downloaded percentage while `font_rx` is live.
    font_progress: Option<u8>,
    /// First-run Chromium/mermaid download, polled once per frame.
    download_rx: Option<mpsc::Receiver<DownloadMsg>>,
    /// Current download phase text while `download_rx` is live.
    download_status: Option<String>,
}

impl App {
    /// Build the app for `path` from its already-read `text`, installing the
    /// font and theme before the first frame.
    pub fn new(path: PathBuf, text: String, dark: bool, ctx: &egui::Context) -> Self {
        // `+`/`-` zoom the document only. egui's own Ctrl+Plus zoom would also
        // scale overlays like the heading index, so keep it off.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // Saved zoom/theme apply to every document; `--dark` still wins.
        let prefs = crate::prefs::load();
        let theme = if dark {
            Theme::Dark
        } else {
            prefs.theme.unwrap_or(Theme::Light)
        };
        // The watcher, a reload and Ctrl+G all want the real file: a path typed
        // relative to the shell would otherwise be shown and re-read relative to
        // whatever directory the viewer happens to be in.
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let restore_scroll = prefs.resume_scroll(&path);
        let positions = prefs.positions;
        // A cached FiraCode is installed before the first frame. On a fresh
        // cache, start with the system font (or egui's defaults) and download
        // in the background so the window still opens immediately.
        let cache_root = crate::mermaid::cache_root();
        crate::fonts::prune(&cache_root);
        // Documented order: override, cache, system, egui defaults. On a fresh
        // cache, start with the system font (or egui's defaults) and download
        // in the background so the window still opens immediately.
        let download = crate::fonts::download_needed(&cache_root);
        match crate::fonts::preferred(&cache_root) {
            Some(defs) => ctx.set_fonts(defs),
            // No FiraCode anywhere yet: the window still has to open (and draw
            // bold runs) while the download runs in the background.
            None => ctx.set_fonts(crate::fonts::placeholder_defs()),
        }
        let mut font_rx = None;
        if download {
            let (tx, rx) = mpsc::channel();
            let root = cache_root.clone();
            std::thread::spawn(move || {
                let mut send = |pct| {
                    let _ = tx.send(crate::fonts::FontMsg::Progress(pct));
                };
                // One retry covers a dropped connection or a truncated body;
                // curl itself already retries transient network errors.
                let mut result = crate::fonts::download_cached(&root, &mut send);
                if result.is_err() {
                    std::thread::sleep(Duration::from_secs(1));
                    send(0);
                    result = crate::fonts::download_cached(&root, &mut send);
                }
                let _ = tx.send(crate::fonts::FontMsg::Done(result));
            });
            font_rx = Some(rx);
        }
        apply_theme(ctx, theme);
        let renderer = Renderer::new();
        // First-run Chromium and mermaid.js downloads run in the background so
        // the window opens immediately and the same status line that reports
        // font progress also reports these. Diagram work waits for them.
        let chromium_error = None;
        let (download_tx, download_rx) = mpsc::channel();
        let download_cache = renderer.cache_dir.clone();
        let download_js = renderer.mermaid_js.clone();
        let download_ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut send = |text: &str| {
                let _ = download_tx.send(DownloadMsg::Status(text.to_string()));
                download_ctx.request_repaint();
            };
            let result = ensure_chromium_with(&download_cache, &mut send).and_then(|path| {
                let renderer = Renderer {
                    chromium: Some(path),
                    mermaid_js: download_js,
                    cache_dir: download_cache.clone(),
                };
                renderer
                    .ensure_mermaid_js_with(&mut send)
                    .map_err(|e| e.to_string())
            });
            let _ = download_tx.send(DownloadMsg::Done(result));
            download_ctx.request_repaint();
        });
        let shared = Arc::new(Renderer {
            chromium: renderer.chromium.clone(),
            mermaid_js: renderer.mermaid_js.clone(),
            cache_dir: renderer.cache_dir.clone(),
        });

        let prepared = prepare(&text, parent_dir(&path));

        let diagrams = {
            let mut diagrams = DiagramUi::new(shared, prepared.diagrams.clone(), theme);
            diagrams.set_ready(false);
            Rc::new(RefCell::new(diagrams))
        };
        let tables = Rc::new(RefCell::new(TableUi::new(prepared.tables.clone())));
        let lists = Rc::new(RefCell::new(ListUi::new(prepared.lists.clone())));
        let paragraphs = Rc::new(RefCell::new(ParagraphUi::new(prepared.paragraphs.clone())));
        let quotes = Rc::new(RefCell::new(quote::QuoteUi::new(prepared.quotes.clone())));
        let images = Rc::new(RefCell::new(ImageUi::new(prepared.images.clone())));
        let headings = Rc::new(RefCell::new(HeadingUi::new(prepared.headings.clone())));
        let code = Rc::new(RefCell::new(CodeUi::new(prepared.code.clone())));
        let search = Rc::new(RefCell::new(SearchUi::new()));
        let html_fn: HtmlFn = {
            let d = Rc::clone(&diagrams);
            let t = Rc::clone(&tables);
            let l = Rc::clone(&lists);
            let p = Rc::clone(&paragraphs);
            let q = Rc::clone(&quotes);
            let i = Rc::clone(&images);
            let h = Rc::clone(&headings);
            let c = Rc::clone(&code);
            let s = Rc::clone(&search);
            Box::new(move |ui: &mut egui::Ui, html: &str| match parse(html) {
                Some(Placeholder::Paragraph(id)) => {
                    p.borrow_mut().show(ui, id, &mut s.borrow_mut())
                }
                Some(Placeholder::Quote(id)) => q.borrow_mut().show(ui, id, &mut s.borrow_mut()),
                Some(Placeholder::Table(id)) => t.borrow_mut().show(ui, id, &mut s.borrow_mut()),
                Some(Placeholder::List(id)) => l.borrow_mut().show(ui, id, &mut s.borrow_mut()),
                Some(Placeholder::Heading(id)) => h.borrow_mut().show(ui, id, &mut s.borrow_mut()),
                Some(Placeholder::Code(id)) => c.borrow_mut().show(ui, id, &mut s.borrow_mut()),
                Some(Placeholder::Image(id)) => i.borrow_mut().show(ui, id),
                Some(Placeholder::Mermaid(id)) => d.borrow_mut().show(ui, id),
                None => {}
            })
        };

        let dir = parent_dir(&path).to_path_buf();
        let (tx, rx) = mpsc::channel();
        let watch_ctx = ctx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                // Wake the UI only for real changes, not our own read events.
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(ev);
                    watch_ctx.request_repaint();
                }
            }
        })
        .ok();
        let watch_rx = match watcher.as_mut() {
            Some(w) => match w.watch(&dir, RecursiveMode::NonRecursive) {
                Ok(()) => Some(rx),
                Err(_) => {
                    watcher = None;
                    None
                }
            },
            None => None,
        };
        let last_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        App {
            path,
            source: text,
            prepared,
            theme,
            zoom: prefs.zoom,
            error: None,
            chromium_error,
            cache: CommonMarkCache::default(),
            diagrams,
            tables,
            lists,
            paragraphs,
            quotes,
            images,
            headings,
            code,
            search,
            html_fn,
            scroll_y: 0.0,
            scroll_x: 0.0,
            restore_scroll,
            reload_anchor: None,
            positions,
            scroll_max: egui::Vec2::ZERO,
            jump_to: None,
            pad: None,
            last_view_w: None,
            scroll_bottom: false,
            applied_theme: Some(theme),
            styles_key: None,
            styles: std::collections::BTreeMap::new(),
            keys: KeyState::default(),
            jumps: JumpList::default(),
            search_origin: None,
            copy_guard: CopyGuard::default(),
            select_press: None,
            select_moved: false,
            keys_help: false,
            toc: toc::TocState::default(),
            autoscroll: 0.0,
            last_pointer: None,
            watcher,
            watch_rx,
            pending_reload: None,
            last_mtime,
            last_poll: Instant::now(),
            status: None,
            font_rx,
            font_progress: None,
            download_rx: Some(download_rx),
            download_status: None,
        }
    }

    /// Re-read the document, keeping the previous state if reading fails.
    pub fn load(&mut self) {
        match read_doc(&self.path) {
            Ok(text) => {
                self.prepared = prepare(&text, parent_dir(&self.path));
                self.source = text;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Reload the document and rebuild every placeholder cache.
    pub fn reload(&mut self) {
        self.reload_from_disk(false);
    }

    /// Reload and rebuild the caches. `navigating` skips the reading anchor:
    /// `]`/`[` supply a saved position instead.
    fn reload_from_disk(&mut self, navigating: bool) {
        self.reload_anchor = self.capture_anchor(navigating);
        let old_toc = self.prepared.toc.clone();
        self.load();
        crate::justify::clear_inline_cache();
        self.cache = CommonMarkCache::default();
        let sources = self.prepared.diagrams.clone();
        self.diagrams.borrow_mut().reset_for_reload(sources);
        self.tables.borrow_mut().reset(self.prepared.tables.clone());
        self.lists.borrow_mut().reset(self.prepared.lists.clone());
        self.paragraphs
            .borrow_mut()
            .reset(self.prepared.paragraphs.clone());
        self.quotes.borrow_mut().reset(self.prepared.quotes.clone());
        self.images.borrow_mut().reset(self.prepared.images.clone());
        self.headings
            .borrow_mut()
            .reset(self.prepared.headings.clone());
        self.code.borrow_mut().reset(self.prepared.code.clone());
        // Old hits point into blocks that no longer exist; the pattern stays and
        // is recomputed after the next draw records the new block texts. Folds
        // and the selection follow heading ids across shifted indices.
        self.search.borrow_mut().reset_keeping_pattern();
        self.toc.remap(&old_toc, &self.prepared.toc);
        self.jump_to = None;
    }

    /// The heading at/above the viewport top from the last completed frame.
    fn capture_anchor(&self, navigating: bool) -> Option<ReadingAnchor> {
        let search = self.search.borrow();
        reading_anchor(
            navigating,
            self.prepared.toc.iter().filter_map(|entry| {
                let block = search.heading_block(&entry.id)?;
                Some((entry.id.as_str(), search.block_y(block)?))
            }),
            self.scroll_y,
        )
    }

    /// Remember the current zoom, theme and this document's reading position.
    fn save_prefs(&mut self) {
        self.positions.retain(|(path, _)| path != &self.path);
        self.positions.insert(0, (self.path.clone(), self.scroll_y));
        self.positions.truncate(MAX_POSITIONS);
        crate::prefs::save(self.zoom, self.theme, &self.positions);
    }

    /// Open the next/previous Markdown file in this document's directory,
    /// wrapping around. The reading column is re-homed to the top.
    fn navigate(&mut self, delta: isize) {
        let dir = parent_dir(&self.path).to_path_buf();
        let files = documents_in(&dir);
        let Some(next) = step_document(&files, &self.path, delta) else {
            self.set_status("no other markdown files in this folder");
            return;
        };
        // Remember where this document was left before switching away.
        self.save_prefs();
        let position = files
            .iter()
            .position(|file| file == &next)
            .map_or(0, |i| i + 1);
        let name = next.file_name().map_or_else(
            || next.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.path = next;
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        // `]`/`[` returns to a document where it was left; a first visit starts
        // at the top. `restore_scroll` overrides the first-frame home anchoring.
        self.restore_scroll = Some(
            self.positions
                .iter()
                .find(|(path, _)| path == &self.path)
                .map_or(0.0, |(_, scroll)| *scroll),
        );
        self.pad = None;
        self.scroll_bottom = false;
        self.autoscroll = 0.0;
        self.reload_from_disk(true);
        // A different document starts with no search and a fresh index.
        self.search.borrow_mut().reset();
        self.toc.reset();
        self.last_mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        self.set_status_for(
            format!("{name} ({position}/{})", files.len()),
            STATUS_DURATION,
        );
    }

    /// Open the link under the pointer in the desktop browser. Only http(s) and
    /// mailto links are accepted, the same schemes a click may copy.
    fn open_hovered_link(&mut self, ctx: &egui::Context) {
        let Some(url) = take_hovered_link(ctx) else {
            self.set_status("point at a link first");
            return;
        };
        if !crate::justify::link_is_safe(&url) {
            self.set_status("only http(s) and mailto links open");
            return;
        }
        match open_external(&url) {
            Ok(()) => self.set_status("opening link"),
            Err(e) => self.set_status(format!("cannot open link: {e}")),
        }
    }

    /// Show a clipboard confirmation for [`STATUS_DURATION`].
    fn set_status(&mut self, message: impl Into<String>) {
        self.set_status_for(message, STATUS_DURATION);
    }

    /// Status that stays up longer than the clipboard messages.
    fn set_status_for(&mut self, message: impl Into<String>, duration: Duration) {
        self.status = Some(Status {
            message: message.into(),
            until: Instant::now() + duration,
        });
    }

    /// Drain first-run download messages; unblock diagrams when done.
    fn poll_downloads(&mut self, ctx: &egui::Context) {
        let mut done = None;
        if let Some(rx) = &self.download_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    DownloadMsg::Status(text) => self.download_status = Some(text),
                    DownloadMsg::Done(result) => done = Some(result),
                }
            }
        }
        if let Some(result) = done {
            let had_progress = self.download_status.is_some();
            self.download_rx = None;
            self.download_status = None;
            self.diagrams.borrow_mut().set_ready(true);
            match result {
                Ok(()) if had_progress && self.font_rx.is_none() => {
                    self.set_status("downloads ready");
                }
                Err(msg) => self.chromium_error = Some(msg),
                Ok(()) => {}
            }
            ctx.request_repaint();
        }
        if self.download_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    /// Paint the bottom-left status line: a transient copy message, otherwise
    /// the active search (`/pat [i/n]`). Anchored in the window margin, so it
    /// never takes layout from the reading column; the document's bottom
    /// padding keeps text clear of it.
    fn draw_status(&mut self, ctx: &egui::Context) {
        // Reading progress has its own bottom-right chip, so it never competes
        // with the left prompt/status. Hidden at the top of the document and
        // when nothing scrolls.
        if let Some(pct) = reading_progress(
            self.scroll_y,
            self.scroll_max.y,
            self.pad.map_or(0.0, |p| p.y),
        ) {
            show_progress(ctx, &format!("{pct}%"));
        }
        if self.search.borrow().is_open() {
            let line = format!("/{}", self.search.borrow().query());
            prompt_area(ctx, line);
            return;
        }
        if let Some(text) = self.search.borrow().state.status_text() {
            show_status(ctx, &text);
            return;
        }
        // Active downloads beat transient clipboard/ready messages: the user
        // should always see a running download's percentage.
        if let Some(text) = &self.download_status {
            let line = match self.font_progress {
                Some(pct) => format!("{text} · {}", font_progress_text(pct)),
                None => text.clone(),
            };
            show_status(ctx, &line);
            return;
        }
        if let Some(pct) = self.font_progress {
            show_status(ctx, &font_progress_text(pct));
            return;
        }
        if let Some(status) = &self.status {
            let remaining = status.until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.status = None;
            } else {
                show_status(ctx, &status.message);
                ctx.request_repaint_after(remaining);
            }
        }
    }

    /// Jump to the index's selected heading and close the overlay. An empty
    /// index (no heading visible at the index depth) makes Enter a no-op, so the
    /// overlay stays against the blank screen instead of closing for nothing.
    fn toc_jump(&mut self) {
        let Some(entry) = self.toc.selected_entry(&self.prepared.toc) else {
            return;
        };
        let id = entry.id.clone();
        if let Some(block) = self.search.borrow().heading_block(&id) {
            self.jumps.record(self.scroll_y);
            self.jump_to = Some(block);
        }
        self.toc.close();
    }

    /// Handle keys while the index overlay is open. Every other key is ignored.
    fn toc_input(&mut self, ui: &egui::Ui) {
        let (close, up, down, left, right, jump, gg, bottom) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Tab)
                    || i.key_pressed(egui::Key::Escape)
                    || (i.key_pressed(egui::Key::Q) && !i.modifiers.any())
                    || (i.key_pressed(egui::Key::C) && i.modifiers.ctrl)
                    || i.events.iter().any(|ev| matches!(ev, egui::Event::Copy)),
                i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space),
                i.events.iter().any(|ev| {
                    matches!(
                        ev,
                        egui::Event::Key {
                            key: egui::Key::G,
                            pressed: true,
                            modifiers,
                            ..
                        } if !modifiers.shift
                    )
                }),
                i.events.iter().any(|ev| {
                    matches!(
                        ev,
                        egui::Event::Key {
                            key: egui::Key::G,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.shift
                    )
                }),
            )
        });
        let theme_toggle = ui.input(|i| i.key_pressed(egui::Key::T) && !i.modifiers.any());
        // The overlay owns every key: drop them before egui turns Tab into
        // widget focus and paints a stray focus ring on the first row.
        ui.input_mut(|i| {
            i.events
                .retain(|ev| !matches!(ev, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        let mut changed = false;
        if theme_toggle {
            self.theme = match self.theme {
                Theme::Light => Theme::Dark,
                Theme::Dark => Theme::Light,
            };
            changed = true;
        }
        if close {
            self.toc.close();
            changed = true;
        } else {
            if up {
                self.toc.move_by(&self.prepared.toc, -1);
                changed = true;
            }
            if down {
                self.toc.move_by(&self.prepared.toc, 1);
                changed = true;
            }
            if left {
                self.toc.fold(&self.prepared.toc, false);
                changed = true;
            }
            if right {
                if self.toc.selected_has_children(&self.prepared.toc) {
                    self.toc.fold(&self.prepared.toc, true);
                } else {
                    // A leaf has nothing to expand: `l` jumps like Enter/Space.
                    self.toc_jump();
                }
                changed = true;
            }
            if bottom {
                self.toc.select_last(&self.prepared.toc);
                changed = true;
            }
            if gg && self.toc.gg(std::time::Instant::now()) {
                self.toc.select_first(&self.prepared.toc);
                changed = true;
            }
            if jump {
                self.toc_jump();
                changed = true;
            }
        }
        if changed {
            ui.ctx().request_repaint();
        }
    }

    /// Paint the `?` overlay: the shared key list on the same solid chip as the
    /// status line, centred over the document.
    fn draw_keys_help(&self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("jiaoren-keys-help"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(
                        1.0,
                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                    ))
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(crate::KEYS_HELP)
                                    .monospace()
                                    .color(ui.visuals().text_color()),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
            });
    }

    /// Update the text selection from this frame's pointer events, using the
    /// previous frame's char maps. A plain click (press+release without moving)
    /// clears instead of selecting a char.
    fn process_selection(&mut self, ui: &egui::Ui) {
        let (pressed, released, down, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });
        let mut s = self.search.borrow_mut();
        let mut copy_on_release = false;
        // A drag that leaves the window keeps its last position: the buttons are
        // still held, so the selection should keep growing.
        let drag_pos = pos.or(self.last_pointer);
        if pos.is_some() {
            self.last_pointer = pos;
        }
        if pressed {
            if let Some(p) = pos {
                if s.pointer_on_control(p) {
                    // Link/copy-button presses own the click; keep any existing
                    // selection untouched.
                } else if let Some(point) = s
                    .hit_near(p)
                    .or_else(|| s.hit(p).filter(|_| s.pointer_over_text(p)))
                {
                    s.selection.begin(point);
                    self.select_press = Some(p);
                    self.select_moved = false;
                } else {
                    s.selection.clear();
                    self.select_press = None;
                }
            }
        }
        if down && s.selection.dragging {
            if let Some(p) = drag_pos {
                if self
                    .select_press
                    .is_some_and(|start| p.distance(start) > 3.0)
                {
                    self.select_moved = true;
                }
                if let Some(point) = s.hit(p) {
                    s.selection.extend(point);
                }
                // Hugging the top or bottom strip scrolls the document, so a
                // selection can run past what the window shows.
                let view = ui.ctx().content_rect();
                self.autoscroll = edge_scroll(p.y, view.top(), view.bottom());
            }
        } else {
            self.autoscroll = 0.0;
        }
        if released && s.selection.dragging {
            s.selection.dragging = false;
            // A plain click, or a drag that stayed within one char, is not a
            // range selection.
            if !self.select_moved || !s.selection.is_active() {
                s.selection.clear();
            } else {
                // zathura behaviour: releasing a drag copies, so drag-select and
                // paste works without a second key.
                copy_on_release = true;
            }
            self.select_press = None;
        }
        drop(s);
        if copy_on_release {
            self.copy_selection_keep(ui.ctx(), true);
        }
    }

    /// Copy the drag selection to the clipboard and clear it, confirming in the
    /// status line. Shared by `y` and Ctrl+C so both copy the same text.
    fn copy_selection(&mut self, ctx: &egui::Context) {
        self.copy_selection_keep(ctx, false);
    }

    /// [`Self::copy_selection`] with a choice about clearing the highlight. A
    /// drag release copies but keeps the selection, so the next `Esc` still
    /// clears it instead of falling through to quit.
    fn copy_selection_keep(&mut self, ctx: &egui::Context, keep: bool) {
        let text = {
            let s = self.search.borrow();
            selected_text(s.texts(), &s.selection)
        };
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        ctx.copy_text(text);
        self.set_status("copied selection");
        // A selection copy is not a `yy` press: do not let a stale arm fire a
        // whole-document copy on the next y.
        self.keys.last_y = None;
        // Clearing lets a second Ctrl+C fall through to the next rung (clear
        // search, then quit) instead of re-copying the same text forever.
        if !keep {
            self.search.borrow_mut().selection.clear();
        }
        ctx.request_repaint();
    }

    /// Route Esc/Ctrl+C: cancel the prompt, copy the selection, clear a
    /// committed search, or quit.
    fn cancel_search(&mut self, ctx: &egui::Context) {
        let prompt_open = self.search.borrow().is_open();
        let search_active = self.search.borrow().state.pattern.is_some();
        let selection_active = self.search.borrow().selection.is_active();
        match cancel_action(prompt_open, search_active, selection_active) {
            CancelAction::CancelPrompt => {
                let restored = self.search.borrow_mut().cancel();
                self.jump_to = restored;
                ctx.request_repaint();
            }
            CancelAction::CopySelection => self.copy_selection(ctx),
            CancelAction::ClearSearch => {
                self.search.borrow_mut().state.clear();
                self.jump_to = None;
                ctx.request_repaint();
            }
            CancelAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// Step to the next (`dir > 0`) or previous match and jump to its block.
    fn step_search(&mut self, dir: i32, ctx: &egui::Context) {
        let mut s = self.search.borrow_mut();
        if s.state.pattern.is_none() {
            return;
        }
        if dir > 0 {
            s.state.next();
        } else {
            s.state.prev();
        }
        let target = s.state.current_block();
        self.jump_to = target;
        if target.is_some() {
            self.jumps.record(self.scroll_y);
            ctx.request_repaint();
        }
    }

    /// Jump to the next (`dir > 0`) or previous block start, vim `}`/`{`.
    /// Blocks are the recorded top-level units, so paragraphs, headings, lists,
    /// tables, code and images all count.
    fn step_paragraph(&mut self, dir: isize) {
        let target = {
            let search = self.search.borrow();
            search
                .block_after(self.scroll_y, dir)
                .and_then(|block| search.block_y(block))
        };
        if let Some(y) = target {
            self.jump_to = None;
            self.scroll_y = y;
        }
    }

    /// Drive live reload: drain watcher events (or poll mtime when the watcher
    /// is unavailable), debounce 100 ms, then reload once.
    fn watch_tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(rx) = &self.watch_rx {
            // The watcher callback requests a repaint, so an idle app does not
            // need a timer; drain whatever arrived after that wake-up.
            while let Ok(ev) = rx.try_recv() {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    continue;
                }
                if ev.paths.iter().any(|p| same_file(p, &self.path)) {
                    self.pending_reload = Some(now);
                }
            }
        } else if now.duration_since(self.last_poll) >= Duration::from_millis(500) {
            // No watcher: fall back to polling mtime.
            self.last_poll = now;
            if let Ok(meta) = std::fs::metadata(&self.path) {
                if let Ok(m) = meta.modified() {
                    match self.last_mtime {
                        Some(prev) if prev != m => self.pending_reload = Some(now),
                        _ => {}
                    }
                    self.last_mtime = Some(m);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if let Some(t) = self.pending_reload {
            if now.duration_since(t) >= Duration::from_millis(100) {
                self.pending_reload = None;
                self.reload();
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_prefs();
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_downloads(ctx);
        self.diagrams.borrow_mut().poll(ctx);
        self.images.borrow_mut().poll(ctx);
        self.watch_tick(ctx);
        if let Some(rx) = self.font_rx.take() {
            let mut done = false;
            loop {
                match rx.try_recv() {
                    Ok(crate::fonts::FontMsg::Progress(pct)) => self.font_progress = Some(pct),
                    Ok(crate::fonts::FontMsg::Done(Ok(defs))) => {
                        ctx.set_fonts(defs);
                        // `set_fonts` takes effect on the next pass; ask for it
                        // or the swap waits for the next input event.
                        ctx.request_repaint();
                        if self.font_progress.take().is_some() && self.download_rx.is_none() {
                            // Fast downloads can finish before the first frame;
                            // leave a visible confirmation once nothing else is
                            // still downloading.
                            self.set_status("font ready");
                        }
                        done = true;
                        break;
                    }
                    Ok(crate::fonts::FontMsg::Done(Err(e))) => {
                        // Another jr instance may have finished the cache while
                        // this download failed; prefer that over a scary status.
                        if let Some(defs) = crate::fonts::cached(&crate::mermaid::cache_root()) {
                            ctx.set_fonts(defs);
                            self.set_status("font ready");
                        } else {
                            eprintln!("font download failed: {e}");
                            self.set_status_for(
                                format!("font download failed: {e}; using the system font"),
                                Duration::from_secs(8),
                            );
                        }
                        self.font_progress = None;
                        done = true;
                        ctx.request_repaint();
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // The worker died without a verdict: same fallback, and
                        // no 100 ms repaint loop forever.
                        self.font_progress = None;
                        done = true;
                        self.set_status(font_failure_status());
                        ctx.request_repaint();
                        break;
                    }
                }
            }
            if !done {
                self.font_rx = Some(rx);
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A link clicked last frame asked for a heading anchor.
        if let Some(slug) = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<String>(egui::Id::new("jiaoren-anchor")))
        {
            if let Some(block) = self.search.borrow().anchor_block(&slug) {
                self.jumps.record(self.scroll_y);
                self.jump_to = Some(block);
                ui.ctx().request_repaint();
            }
        }
        // Keep egui's window chrome and the mermaid renders on one theme.
        if self.applied_theme != Some(self.theme) {
            apply_theme(ui.ctx(), self.theme);
            self.applied_theme = Some(self.theme);
        }
        // The popup owns the keyboard and the wheel while it is open; the
        // document behind it must not scroll or react to `t`, `r`, `q`, `0`.
        let popup_open =
            self.diagrams.borrow().popup.is_some() || self.images.borrow().popup.is_some();
        let (mut popup_closed_at, popup_closed_with_ctrl) = {
            let d = self.diagrams.borrow();
            let i = self.images.borrow();
            let closed = match (d.popup_closed_at, i.popup_closed_at) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            (closed, d.popup_closed_with_ctrl || i.popup_closed_with_ctrl)
        };
        // Turn this frame's key presses into viewer actions.
        let now = Instant::now();
        let input_time = ui.input(|i| i.time);
        // Ctrl holds one Ctrl+C decision: autorepeats arrive as bare Copy
        // events with no repeat flag, so re-arm only after Ctrl is released.
        self.copy_guard
            .observe_modifiers(ui.input(|i| i.modifiers.ctrl || i.modifiers.command));
        // X autorepeat starts after ~660 ms, outlasting the 0.30 s guard, so
        // keep the guard alive while the Ctrl that closed a popup is held.
        // After release the normal 0.30 s window applies.
        if popup_closed_with_ctrl {
            if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                self.diagrams.borrow_mut().popup_closed_at = Some(input_time);
                self.images.borrow_mut().popup_closed_at = Some(input_time);
                popup_closed_at = Some(input_time);
            } else {
                self.diagrams.borrow_mut().popup_closed_with_ctrl = false;
                self.images.borrow_mut().popup_closed_with_ctrl = false;
            }
        }
        let suppress_quit = quit_suppressed(input_time, popup_closed_at);
        let prompt_open = self.search.borrow().is_open();
        // The `?` overlay owns the keyboard while it is up: Esc, `q`, `?`, Ctrl+C
        // or a click closes it and every other key is dropped, so no stray key
        // can scroll or quit from behind a help screen. Snapshotted before the
        // close so the closing press is swallowed too instead of toggling the
        // overlay back on.
        let help_open = self.keys_help;
        if help_open {
            let close = ui.input(|i| {
                i.key_pressed(egui::Key::Escape)
                    || i.key_pressed(egui::Key::Questionmark)
                    || i.key_pressed(egui::Key::Q)
                    // Ctrl+C reaches winit as `Copy`; accept either form.
                    || (i.key_pressed(egui::Key::C) && (i.modifiers.ctrl || i.modifiers.command))
                    || i.events.iter().any(|ev| matches!(ev, egui::Event::Copy))
                    || i.pointer.primary_pressed()
            });
            if close {
                self.keys_help = false;
                ui.ctx().request_repaint();
            }
        }
        // Tab index overlay: its own keys, swallowed like the help screen.
        let toc_open = self.toc.open;
        if toc_open && !help_open {
            self.toc_input(ui);
        }
        // Pointer selection runs before the scroll area so a press on text can
        // suppress drag-panning in the same frame.
        if !popup_open && !prompt_open && !help_open && !toc_open {
            self.process_selection(ui);
        }
        // If a release/focus-loss happened while the prompt/popup blocked the
        // pointer path, do not keep SelectArea drag-panning disabled.
        if !ui.input(|i| i.pointer.primary_down()) {
            // A missed release must also stop edge auto-scroll, or the document
            // keeps scrolling (and repainting) under whatever is open.
            self.autoscroll = 0.0;
            let mut s = self.search.borrow_mut();
            if s.selection.dragging {
                s.selection.dragging = false;
                self.select_press = None;
            }
        }
        let mut acts: Vec<Action> = Vec::new();
        // Edge auto-scroll moves the document under a held selection drag; the
        // scroll area clamps the target to the content range.
        if self.autoscroll != 0.0 {
            self.scroll_y = (self.scroll_y + self.autoscroll).max(0.0);
            ui.ctx().request_repaint();
        }
        // The scroll area ignores the wheel while a widget holds the drag (a
        // selection drag does), so the document scrolls itself here and the
        // selection keeps growing with the content that moves under the pointer.
        if self.search.borrow().selection.dragging {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.scroll_y = (self.scroll_y - delta).max(0.0);
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
                ui.ctx().request_repaint();
            }
        }
        if !popup_open && prompt_open && !help_open {
            // Raw vim-style input: no widget, no focus. Events are consumed in
            // order and stop at Enter; later text cannot join the pattern.
            let batch = ui.input(|i| read_search_batch(&i.events));
            let ctx = ui.ctx().clone();
            let mut cancelled = false;
            if batch.copy && self.copy_guard.accept() {
                self.cancel_search(&ctx);
                cancelled = true;
            }
            if !cancelled && batch.cancel {
                self.cancel_search(&ctx);
                cancelled = true;
            }
            if !cancelled {
                let changed = !batch.edits.is_empty();
                if changed || batch.enter {
                    let mut s = self.search.borrow_mut();
                    for edit in &batch.edits {
                        match edit {
                            SearchEdit::HistoryOlder => s.history_older(),
                            SearchEdit::HistoryNewer => s.history_newer(),
                            _ => {
                                s.history_abandon();
                                let Some(query) = s.input.as_mut() else {
                                    continue;
                                };
                                match edit {
                                    SearchEdit::Text(text) => query.push_str(text),
                                    SearchEdit::Backspace => {
                                        query.pop();
                                    }
                                    SearchEdit::DeleteWord => truncate_word_tail(query),
                                    SearchEdit::HistoryOlder | SearchEdit::HistoryNewer => {}
                                }
                            }
                        }
                    }
                    if changed {
                        // One match recompute for the whole frame.
                        s.recompute_live();
                    }
                    let target = s.state.current_block();
                    if batch.enter {
                        s.commit();
                    }
                    drop(s);
                    // Enter confirms the jump that live editing already made:
                    // remember where the search started, not the preview.
                    if batch.enter && target.is_some() {
                        self.jumps
                            .record(self.search_origin.take().unwrap_or(self.scroll_y));
                    }
                    // A live edit jumps to its match; Enter keeps it. Cancelling
                    // already returned through cancel_search above.
                    self.jump_to = target;
                    ctx.request_repaint();
                }
                // Keys after Enter run through the document action path, so
                // `Enter n` commits the pattern and then steps.
                for (key, modifiers) in batch.after_enter {
                    if let Some(action) =
                        apply_key_event(&mut self.keys, key, modifiers, false, now)
                    {
                        acts.push(action);
                    }
                }
            }
        } else if popup_open {
            // The overlay owns the keys, but the theme it draws with can still
            // be switched with `t`.
            if ui.input(|i| overlay_theme_toggle(&i.events)) {
                acts.push(Action::ToggleTheme);
            }
        } else if !help_open && !toc_open {
            // Once `/` opens the prompt in this batch, later keys are text for
            // the prompt, not viewer actions (`q`, `t`, `n`, …).
            let mut search_opened = false;
            ui.input(|i| {
                for ev in &i.events {
                    if search_opened {
                        continue;
                    }
                    if let egui::Event::Key {
                        key,
                        modifiers,
                        pressed: true,
                        repeat,
                        ..
                    } = ev
                    {
                        // The next repeat of the Ctrl+C that closed the popup
                        // must not quit either.
                        if suppress_quit
                            && *key == egui::Key::C
                            && (modifiers.command || modifiers.ctrl)
                        {
                            continue;
                        }
                        // `y` copies an active drag selection; with none it
                        // keeps arming `yy`. A held `y` must not copy.
                        if !*repeat
                            && *key == egui::Key::Y
                            && !modifiers.any()
                            && self.search.borrow().selection.is_active()
                        {
                            acts.push(Action::CopySelection);
                            continue;
                        }
                        // A held key only repeats scrolling; `apply_key_event`
                        // drops repeats of `yy`, `gg`, `r`, `t`, `]`, Tab, …
                        if let Some(a) =
                            apply_key_event(&mut self.keys, *key, *modifiers, *repeat, now)
                        {
                            search_opened = a == Action::SearchOpen;
                            acts.push(a);
                        }
                    } else if !suppress_quit && matches!(ev, egui::Event::Copy) {
                        // winit reports Ctrl+C as `Copy` rather than `Key::C`.
                        // Repeats look identical, so accept one per Ctrl hold.
                        if self.copy_guard.accept() {
                            acts.push(Action::CancelOrQuit);
                        }
                    }
                }
            });
        }
        let page_h = ui.available_height();
        let mut cancel_done = false;
        for act in acts {
            if act == Action::CancelOrQuit {
                if cancel_done {
                    continue;
                }
                cancel_done = true;
            }
            match act {
                Action::ScrollLines(f) => {
                    // The panel style is the unscaled base; the document text is
                    // drawn at `zoom`, so scale the line height to match.
                    let line_h = ui.text_style_height(&egui::TextStyle::Body) * self.zoom;
                    self.scroll_y = scroll_by_lines(self.scroll_y, f, line_h, self.scroll_max.y);
                }
                Action::ScrollPages(f) => {
                    self.scroll_y = (self.scroll_y + f * page_h).clamp(0.0, self.scroll_max.y);
                }
                Action::ScrollCols(f) => {
                    let font = egui::TextStyle::Monospace.resolve(ui.style());
                    let color = ui.visuals().text_color();
                    let advance = ui.fonts_mut(|fonts| {
                        fonts.layout_no_wrap("0".to_string(), font, color).size().x
                    }) * self.zoom;
                    self.scroll_x = scroll_by_columns(self.scroll_x, f, advance, self.scroll_max.x);
                }
                Action::NextParagraph => self.step_paragraph(1),
                Action::PrevParagraph => self.step_paragraph(-1),
                Action::Top => {
                    let home = home_target(self.pad);
                    self.scroll_x = home.x;
                    self.scroll_y = home.y;
                }
                Action::Bottom => self.scroll_bottom = true,
                Action::ZoomIn => {
                    self.zoom = (self.zoom * 1.1).min(3.0);
                    self.save_prefs();
                }
                Action::ZoomOut => {
                    self.zoom = (self.zoom / 1.1).max(0.5);
                    self.save_prefs();
                }
                Action::Fit => {
                    self.zoom = 1.0;
                    self.scroll_x = self.pad.map_or(0.0, |p| p.x);
                    self.save_prefs();
                }
                Action::ResetZoom => {
                    self.zoom = 1.0;
                    self.save_prefs();
                }
                Action::ToggleTheme => {
                    self.theme = match self.theme {
                        Theme::Light => Theme::Dark,
                        Theme::Dark => Theme::Light,
                    };
                    self.save_prefs();
                }
                Action::NextFile => self.navigate(1),
                Action::PrevFile => self.navigate(-1),
                Action::OpenLink => self.open_hovered_link(ui.ctx()),
                Action::JumpBack => {
                    if let Some(y) = self.jumps.take_back(self.scroll_y) {
                        self.jump_to = None;
                        self.scroll_y = y;
                        ui.ctx().request_repaint();
                    } else {
                        self.set_status("no earlier position");
                    }
                }
                Action::JumpForward => {
                    if let Some(y) = self.jumps.take_forward(self.scroll_y) {
                        self.jump_to = None;
                        self.scroll_y = y;
                        ui.ctx().request_repaint();
                    } else {
                        self.set_status("no later position");
                    }
                }
                Action::Reload => self.reload(),
                Action::CopyDocument => {
                    ui.ctx().copy_text(self.source.clone());
                    self.set_status("copied markdown");
                }
                Action::SearchOpen => {
                    self.search_origin = Some(self.scroll_y);
                    let origin = self.search.borrow().block_at(self.scroll_y);
                    self.search.borrow_mut().open_from(origin);
                    ui.ctx().request_repaint();
                }
                Action::SearchNext => self.step_search(1, ui.ctx()),
                Action::SearchPrev => self.step_search(-1, ui.ctx()),
                Action::CopySelection => self.copy_selection(ui.ctx()),
                Action::ShowPath => {
                    // The file name is what the reader wants; a full path is
                    // clipped off the right edge on narrow windows.
                    let name = self.path.file_name().map_or_else(
                        || self.path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let path = self.path.display().to_string();
                    let text = if name == path {
                        path
                    } else {
                        format!("{name} — {path}")
                    };
                    self.set_status_for(text, PATH_STATUS_DURATION);
                }
                Action::ToggleKeysHelp => {
                    self.keys_help = !self.keys_help;
                }
                Action::ToggleToc => {
                    let ys: Vec<Option<f32>> = {
                        let search = self.search.borrow();
                        self.prepared
                            .toc
                            .iter()
                            .map(|entry| {
                                search
                                    .heading_block(&entry.id)
                                    .and_then(|block| search.block_y(block))
                            })
                            .collect()
                    };
                    self.toc.toggle_open(&self.prepared.toc, &ys, self.scroll_y);
                    ui.ctx().request_repaint();
                }
                // Esc/Ctrl+C: prompt, search, selection, then quit.
                Action::CancelOrQuit => self.cancel_search(ui.ctx()),
                Action::Quit => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        {
            let mut d = self.diagrams.borrow_mut();
            d.theme = self.theme;
            d.zoom = self.zoom;
            d.scroll_y = self.scroll_y;
        }

        // The reading column this frame, reused by the popup after the document.
        let mut column = 0.0;
        egui::CentralPanel::default().show(ui, |ui| {
            // Scale text with zoom so text and diagrams stay consistent. The
            // scaled map is rebuilt only when the zoom or the display density
            // changes, then swapped in and back out, so a steady view does no
            // per-frame clone or rebuild.
            let z = self.zoom;
            let key = (
                z.to_bits(),
                (ui.ctx().pixels_per_point() * 100.0).round() as i32,
            );
            if self.styles_key != Some(key) {
                self.styles_key = Some(key);
                self.styles = scaled_text_styles(&ui.style().text_styles, z);
            }
            std::mem::swap(&mut ui.style_mut().text_styles, &mut self.styles);
            // The document paints its own selection. egui labels are still
            // selectable by default, so a drag would also select the invisible
            // auxiliary labels around the custom blocks and paint stray chips
            // in the left margin and the gaps between blocks.
            ui.style_mut().interaction.selectable_labels = false;

            if let Some(reason) = &self.chromium_error {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!(
                        "warning: chromium not found — mermaid diagrams will not render ({reason})"
                    ),
                );
            }
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }

            let viewport_h = ui.available_height();
            let viewport_w = ui.available_width();
            let pad_y = overscroll_pad(viewport_h);
            let pad_x = overscroll_pad(viewport_w);
            let pad = egui::vec2(pad_x, pad_y);
            let current = egui::vec2(self.scroll_x, self.scroll_y);
            let home = home_scroll(current, self.pad, pad);
            self.scroll_y = home.y;
            // A resumed document starts at its saved offset instead of the
            // automatic home-to-pad, so the first frame never flashes the top.
            if let Some(y) = self.restore_scroll.take() {
                self.scroll_y = y;
            }
            // Keep the content that was in the middle of the viewport in the
            // middle after a resize; a fresh document homes on the pad. See
            // [`frame_scroll_x`].
            self.scroll_x = frame_scroll_x(
                current.x,
                self.pad.map(|p| p.x),
                pad.x,
                self.last_view_w,
                viewport_w,
            );
            self.last_view_w = Some(viewport_w);
            self.pad = Some(pad);
            // A confirmed or stepped search hit wins over home/resize anchoring.
            // Centre its block; the scroll area clamps to the content range
            // (including the overscroll pads). The horizontal home pad keeps
            // the block's left edge visible.
            if let Some(block) = self.jump_to.take() {
                let search = self.search.borrow();
                if let Some(top) = search.block_y(block) {
                    let height = search.block_height(block).unwrap_or(0.0);
                    self.scroll_y = jump_scroll_top(top, height, viewport_h);
                }
                self.scroll_x = pad.x;
            }
            let scroll_y = self.scroll_y;
            let scroll_x = self.scroll_x;
            let scroll = egui::ScrollArea::both()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll_y)
                .horizontal_scroll_offset(scroll_x)
                .on_hover_cursor(egui::CursorIcon::Grab)
                .scroll_source(egui::scroll_area::ScrollSource {
                    scroll_bar: true,
                    // Mouse drag pans both axes; a text selection owns the drag,
                    // and the popup freezes the document.
                    drag: if popup_open || self.search.borrow().selection.dragging {
                        egui::scroll_area::DragScroll::Never
                    } else {
                        egui::scroll_area::DragScroll::Always
                    },
                    // The index owns the wheel while it is open.
                    mouse_wheel: !popup_open && !self.toc.open,
                });
            let output = scroll.show(ui, |ui| {
                let available = ui.available_width();
                let col = content_column(available, z);
                column = col;
                let indent = ((available - col) * 0.5).max(0.0);
                ui.add_space(pad_y);
                let row = ui.horizontal(|ui| {
                    ui.add_space(pad_x + indent);
                    ui.vertical(|ui| {
                        ui.set_width(col);
                        let html_fn = &self.html_fn;
                        let cache = &mut self.cache;
                        let md = &self.prepared.markdown;
                        self.tables.borrow_mut().begin_frame();
                        self.lists.borrow_mut().begin_frame(col);
                        self.paragraphs.borrow_mut().begin_frame(col);
                        self.quotes.borrow_mut().begin_frame(col);
                        self.headings.borrow_mut().begin_frame(col);
                        self.code.borrow_mut().begin_frame(ui, col);
                        self.diagrams.borrow_mut().begin_frame(col);
                        self.images.borrow_mut().begin_frame(col, z, viewport_h);
                        self.search.borrow_mut().begin_frame();
                        // The document paint re-records the hovered link below;
                        // clearing first keeps `o` from using a stale one.
                        ui.ctx().data_mut(|d| {
                            d.remove_temp::<String>(egui::Id::new(crate::justify::HOVER_LINK))
                        });
                        CommonMarkViewer::new()
                            .max_image_width(Some(col as usize))
                            .render_html_fn(Some(&**html_fn))
                            .show(ui, cache, md);
                    });
                    // Normally the symmetric column margin; widen it when a
                    // narrow document would otherwise clamp the left pad.
                    let right_pad = (indent + pad_x).max(viewport_w - indent - col);
                    ui.add_space(right_pad);
                });
                let doc_h = row.response.rect.height();
                ui.add_space(bottom_pad(viewport_h, doc_h));
            });
            let max_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
            let max_x = (output.content_size.x - output.inner_rect.width()).max(0.0);
            self.scroll_max = egui::vec2(max_x, max_y);
            let adjust = std::mem::take(&mut self.diagrams.borrow_mut().scroll_adjust);
            let jump_bottom = std::mem::take(&mut self.scroll_bottom);
            self.scroll_y = if jump_bottom {
                max_y
            } else {
                (output.state.offset.y + adjust).clamp(0.0, max_y)
            };
            self.scroll_x = output.state.offset.x.clamp(0.0, max_x);
            // Screen tops recorded during the draw become content positions
            // that survive scrolling, for the next Enter/n/N jump.
            self.search
                .borrow_mut()
                .finish_frame(output.state.offset.y, output.inner_rect.top());
            // A reload keeps the heading that was at the viewport top in place,
            // now that the new layout knows its content position.
            if let Some(anchor) = self.reload_anchor.take() {
                let top = {
                    let search = self.search.borrow();
                    search
                        .heading_block(&anchor.heading)
                        .and_then(|block| search.block_y(block))
                };
                if let Some(top) = top {
                    self.scroll_y = (top + anchor.offset).clamp(0.0, max_y);
                    ui.ctx().request_repaint();
                }
            }
            if adjust != 0.0 || jump_bottom {
                // Re-lay out next frame at the corrected offset.
                ui.ctx().request_repaint();
            }
            // Hand the per-frame map back so the next frame can reuse it.
            std::mem::swap(&mut ui.style_mut().text_styles, &mut self.styles);
        });
        // Drawn last so the foreground overlay sits above the document. Images
        // and diagrams never share the overlay: whichever opened clears the
        // other. A click on an image closes a stale diagram popup, and vice
        // versa, before either is drawn.
        let image_open = self.images.borrow().popup.is_some();
        if image_open {
            self.diagrams.borrow_mut().close_popup();
        } else if self.diagrams.borrow().popup.is_some() {
            self.images.borrow_mut().close_popup();
        }
        self.diagrams.borrow_mut().draw_popup(ui.ctx(), column);
        self.images.borrow_mut().draw_popup(ui.ctx(), column);
        if self.tables.borrow_mut().take_copied() {
            self.set_status("copied CSV");
        }
        if self.code.borrow_mut().take_copied() {
            self.set_status("copied code");
        }
        // A link click during this frame's document paint copied its URL; show
        // the chip on the same frame instead of waiting for the next repaint.
        if take_link_copied(ui.ctx()) {
            self.set_status("link copied");
        }
        self.draw_status(ui.ctx());
        if self.keys_help {
            self.draw_keys_help(ui.ctx());
        }
        if self.toc.open {
            if let Some(index) = toc::draw(ui.ctx(), &self.prepared.toc, &mut self.toc, self.theme)
            {
                if let Some(entry) = self.prepared.toc.get(index) {
                    if let Some(block) = self.search.borrow().heading_block(&entry.id) {
                        self.jump_to = Some(block);
                    }
                }
                self.toc.close();
                ui.ctx().request_repaint();
            }
        }
        // Final cursor wins over the scroll area's Grab: controls first, text
        // second, pan feedback third. The plain search command line is a text
        // surface; popups keep their own cursors.
        if !popup_open {
            if prompt_open {
                ui.ctx()
                    .output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
            } else if let Some(pos) = ui.ctx().pointer_hover_pos() {
                let (control, over_text, selecting) = {
                    let s = self.search.borrow();
                    (
                        s.control_cursor(pos),
                        s.pointer_over_text(pos),
                        s.selection.dragging,
                    )
                };
                let panning =
                    ui.input(|i| i.pointer.primary_down()) && !selecting && control.is_none();
                let cursor = cursor_for(control, over_text, panning);
                ui.ctx().output_mut(|o| o.cursor_icon = cursor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    mod search_batch {
        use super::super::{
            cursor_blink_remaining, cursor_visible, read_search_batch, sanitize_paste, tail_within,
            truncate_word_tail, SearchEdit,
        };
        use egui::{Event, Key, Modifiers};

        fn pressed(key: Key) -> Event {
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }
        }

        #[test]
        fn stops_at_enter_and_keeps_later_keys_for_actions() {
            let batch = read_search_batch(&[
                Event::Text("foo".into()),
                pressed(Key::Enter),
                Event::Text("n".into()),
                pressed(Key::N),
            ]);
            assert_eq!(batch.edits, vec![SearchEdit::Text("foo".into())]);
            assert!(batch.enter);
            assert_eq!(batch.after_enter, vec![(Key::N, Modifiers::NONE)]);
        }

        #[test]
        fn paste_is_single_line_and_strips_controls() {
            assert_eq!(sanitize_paste("a\r\nb\tc"), "abc");
            let batch = read_search_batch(&[Event::Paste("a\r\nb\tc".into())]);
            assert_eq!(batch.edits, vec![SearchEdit::Text("abc".into())]);
        }

        #[test]
        fn tail_trim_keeps_longest_suffix_on_char_boundary() {
            let chars = |s: &str| s.chars().count() as f32;
            assert_eq!(tail_within("abcdef", 10.0, &chars), "abcdef");
            assert_eq!(tail_within("abcdef", 3.0, &chars), "def");
            // Multi-byte scalars: trimming by bytes would split a char.
            assert_eq!(tail_within("ééé３", 1.0, &chars), "３");
            assert_eq!(tail_within("", 0.0, &chars), "");
        }

        #[test]
        fn block_cursor_blinks_every_half_period() {
            assert!(cursor_visible(0.0));
            assert!(cursor_visible(0.52));
            assert!(!cursor_visible(0.53));
            assert!(!cursor_visible(1.0));
            assert!(cursor_visible(1.06));
            assert!(cursor_visible(2.12));
            // The repaint delay always lands on the next flip, never zero.
            let at = 0.7;
            let wait = cursor_blink_remaining(at);
            assert!((wait - 0.36).abs() < 0.01);
            assert!(!cursor_visible(at), "cursor is off before the flip");
            assert!(cursor_visible(at + wait), "lit again at the boundary");
            assert!(cursor_blink_remaining(0.0) > 0.0);
            assert!(cursor_blink_remaining(f64::MAX) > 0.0);
        }

        #[test]
        fn ctrl_w_erases_the_trailing_word_like_vim() {
            let erase = |q: &str| {
                let mut s = q.to_string();
                truncate_word_tail(&mut s);
                s
            };
            assert_eq!(erase("foo bar"), "foo ");
            assert_eq!(erase("foo bar  "), "foo ");
            assert_eq!(erase("bar"), "");
            assert_eq!(erase("   "), "");
            assert_eq!(erase(""), "");
            assert_eq!(erase("héllo wörld"), "héllo ");
            // Typing `a b` then Ctrl+W twice empties the query.
            assert_eq!(erase(&erase("a b")), "");
        }

        #[test]
        fn plain_arrows_walk_history_and_modified_arrows_do_not() {
            let batch = read_search_batch(&[pressed(Key::ArrowUp), pressed(Key::ArrowDown)]);
            assert_eq!(
                batch.edits,
                vec![SearchEdit::HistoryOlder, SearchEdit::HistoryNewer]
            );
            let modified = read_search_batch(&[Event::Key {
                key: Key::ArrowUp,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            }]);
            assert!(modified.edits.is_empty());
        }

        #[test]
        fn ctrl_w_is_a_delete_word_edit_not_a_typed_w() {
            let batch = read_search_batch(&[Event::Key {
                key: Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            }]);
            assert_eq!(batch.edits, vec![SearchEdit::DeleteWord]);
            assert!(!batch.cancel);
        }
    }

    mod reload_anchor {
        use super::super::{reading_anchor, ReadingAnchor};

        fn headings() -> Vec<(&'static str, f32)> {
            vec![("intro", 100.0), ("body", 400.0), ("tail", 900.0)]
        }

        #[test]
        fn picks_the_last_heading_at_or_above_the_viewport_top() {
            assert_eq!(
                reading_anchor(false, headings(), 450.0),
                Some(ReadingAnchor {
                    heading: "body".to_string(),
                    offset: 50.0,
                })
            );
            // A heading exactly at the top is the anchor, with no offset.
            assert_eq!(
                reading_anchor(false, headings(), 400.0),
                Some(ReadingAnchor {
                    heading: "body".to_string(),
                    offset: 0.0,
                })
            );
        }

        #[test]
        fn no_heading_at_or_above_yields_no_anchor() {
            assert_eq!(reading_anchor(false, headings(), 99.0), None);
            assert_eq!(reading_anchor(false, std::iter::empty(), 10.0), None);
        }

        #[test]
        fn offset_is_the_distance_below_the_heading_top() {
            let anchor = reading_anchor(false, headings(), 725.0).unwrap();
            assert_eq!(anchor.heading, "body");
            assert_eq!(anchor.offset, 325.0);
        }

        #[test]
        fn navigation_does_not_arm_the_anchor() {
            assert_eq!(reading_anchor(true, headings(), 450.0), None);
        }
    }

    mod read_doc {
        use super::super::*;

        #[test]
        fn read_doc_reports_missing_dir_and_bad_utf8() {
            assert!(read_doc(Path::new("/nope/missing.md"))
                .unwrap_err()
                .to_string()
                .contains("cannot read"));
            assert!(read_doc(Path::new("/tmp"))
                .unwrap_err()
                .to_string()
                .contains("cannot read"));
            let bad = std::env::temp_dir().join("jiaoren-bad-utf8.md");
            std::fs::write(&bad, [0xff, 0xfe]).unwrap();
            assert!(read_doc(&bad).unwrap_err().to_string().contains("UTF-8"));
            let _ = std::fs::remove_file(&bad);
        }
    }

    mod workers {
        use super::super::catch_worker;

        #[test]
        fn panic_becomes_an_error_result() {
            let res = catch_worker("decode", || panic!("boom"));
            assert_eq!(res.unwrap_err(), "decode panicked");
            assert_eq!(catch_worker("raster", || 7), Ok(7));
        }
    }

    mod search_blocks {
        use super::super::paragraph::paragraph_needs_library;

        #[test]
        fn library_routed_paragraphs_are_not_searchable() {
            assert!(paragraph_needs_library("see ![x](a.png)"));
            assert!(paragraph_needs_library("a<br>b"));
            assert!(!paragraph_needs_library("**bold** and `code`"));
            assert!(paragraph_needs_library("1 < 2"));
            assert!(!paragraph_needs_library("1 > 2"));
        }
    }

    mod font_progress {
        use super::super::font_progress_text;

        #[test]
        fn font_progress_text_has_percent_and_reason() {
            assert_eq!(
                font_progress_text(0),
                "Downloading FiraCode font… 0% (first run only)"
            );
            assert_eq!(
                font_progress_text(42),
                "Downloading FiraCode font… 42% (first run only)"
            );
        }
    }

    mod link_status {
        use super::super::take_link_copied;
        use crate::justify::LINK_COPIED;

        #[test]
        fn link_flag_is_consumed_once() {
            let ctx = egui::Context::default();
            assert!(!take_link_copied(&ctx));
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(LINK_COPIED), ()));
            assert!(take_link_copied(&ctx));
            assert!(!take_link_copied(&ctx));
        }
    }

    mod theme {
        use super::super::{apply_theme, overlay_theme_toggle, Theme};
        use egui::{Event, Key, Modifiers};

        fn key(key: Key, modifiers: Modifiers) -> Event {
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }
        }

        #[test]
        fn plain_t_toggles_while_an_overlay_owns_the_keys() {
            assert!(overlay_theme_toggle(&[key(Key::T, Modifiers::NONE)]));
            assert!(!overlay_theme_toggle(&[key(Key::T, Modifiers::CTRL)]));
            assert!(!overlay_theme_toggle(&[key(Key::Y, Modifiers::NONE)]));
            assert!(!overlay_theme_toggle(&[Event::Text("t".into())]));
        }

        #[test]
        fn applying_theme_updates_active_visuals() {
            let ctx = egui::Context::default();
            apply_theme(&ctx, Theme::Light);
            assert_eq!(ctx.theme(), egui::Theme::Light);
            assert!(!ctx.global_style().visuals.dark_mode);
            apply_theme(&ctx, Theme::Dark);
            assert_eq!(ctx.theme(), egui::Theme::Dark);
            assert!(ctx.global_style().visuals.dark_mode);
        }
    }

    mod styles {
        use super::super::scaled_text_styles;
        use std::collections::BTreeMap;

        #[test]
        fn scaling_multiplies_every_size_and_keeps_the_family() {
            let mut base = BTreeMap::new();
            base.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            base.insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.0));
            let scaled = scaled_text_styles(&base, 2.0);
            let body = scaled.get(&egui::TextStyle::Body).unwrap();
            let mono = scaled.get(&egui::TextStyle::Monospace).unwrap();
            assert_eq!(body.size, 28.0);
            assert_eq!(mono.size, 22.0);
            assert_eq!(body.family, egui::FontFamily::Proportional);
            assert_eq!(mono.family, egui::FontFamily::Monospace);
        }
    }

    mod navigation {
        use super::super::{documents_in, step_document};
        use std::path::{Path, PathBuf};

        fn temp_dir(tag: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("jiaoren-nav-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn lists_only_markdown_files_sorted() {
            let dir = temp_dir("list");
            for name in ["b.md", "a.MD", "note.markdown", "image.png", "noext"] {
                std::fs::write(dir.join(name), "x").unwrap();
            }
            std::fs::create_dir_all(dir.join("folder.md")).unwrap();
            let names: Vec<String> = documents_in(&dir)
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names, ["a.MD", "b.md", "note.markdown"]);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn stepping_wraps_and_needs_a_current_file() {
            let files = vec![
                PathBuf::from("/d/a.md"),
                PathBuf::from("/d/b.md"),
                PathBuf::from("/d/c.md"),
            ];
            let a = Path::new("/d/a.md");
            let c = Path::new("/d/c.md");
            assert_eq!(
                step_document(&files, a, 1).unwrap(),
                PathBuf::from("/d/b.md")
            );
            assert_eq!(
                step_document(&files, c, 1).unwrap(),
                PathBuf::from("/d/a.md")
            );
            assert_eq!(
                step_document(&files, a, -1).unwrap(),
                PathBuf::from("/d/c.md")
            );
            assert_eq!(step_document(&files, Path::new("/elsewhere/x.md"), 1), None);
            assert_eq!(step_document(&files[..1], a, 1), None);
        }
    }

    mod open_external {
        use super::super::open_external;
        use std::collections::BTreeSet;
        use std::sync::Mutex;

        /// Serialises the tests that mutate process env.
        static ENV_LOCK: Mutex<()> = Mutex::new(());

        /// Restores an environment variable on drop, even when a test panics.
        struct EnvVar(&'static str, Option<std::ffi::OsString>);

        impl EnvVar {
            fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
                let saved = std::env::var_os(name);
                std::env::set_var(name, value);
                Self(name, saved)
            }
        }

        impl Drop for EnvVar {
            fn drop(&mut self) {
                match &self.1 {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }

        /// Pids that are currently children of this process; zombies count.
        fn child_pids() -> BTreeSet<u32> {
            let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
                return BTreeSet::new();
            };
            tasks
                .flatten()
                .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
                .flat_map(|children| {
                    children
                        .split_whitespace()
                        .filter_map(|pid| pid.parse().ok())
                        .collect::<Vec<u32>>()
                })
                .collect()
        }

        #[test]
        fn opener_is_reaped_without_blocking() {
            if !std::path::Path::new("/proc/self/task").is_dir() {
                return; // needs procfs to observe children
            }
            let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let baseline = child_pids();
            let browser = EnvVar::set("BROWSER", "/bin/true");
            let opened = open_external("https://example.com");
            drop(browser);
            assert!(opened.is_ok(), "spawning /bin/true failed: {opened:?}");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let now = child_pids();
                if now.is_subset(&baseline) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "opener child left unreaped: {:?}",
                    now.difference(&baseline).collect::<Vec<_>>()
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
