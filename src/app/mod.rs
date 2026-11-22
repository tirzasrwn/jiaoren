//! The eframe application: window state, scrolling and zoom, and the Markdown
//! render hook that swaps placeholders for diagrams, tables, lists, headings,
//! code, images and paragraphs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify::{RecursiveMode, Watcher};

use crate::doc::{prepare, Prepared};
use crate::mermaid::{ensure_chromium_with, Renderer, Theme};

mod code;
mod diagram;
mod geometry;
mod heading;
mod image;
mod keys;
mod list;
mod placeholder;
mod quote;
mod raster;
mod searchui;
mod selection;
mod table;
mod widgets;

use code::CodeUi;
use diagram::DiagramUi;
use geometry::{
    bottom_pad, content_column, home_scroll, home_target, jump_scroll_top, overscroll_pad,
};
use heading::HeadingUi;
use image::ImageUi;
use keys::{apply_key, cancel_action, quit_suppressed, Action, CancelAction, CopyGuard, KeyState};
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

/// Whether a paragraph is painted by the library instead of the justify
/// painter: images and raw HTML. Those blocks are not searchable because their
/// painted glyphs are not the recorded run text.
pub(crate) fn paragraph_needs_library(source: &str) -> bool {
    source.contains("![") || source.contains('<')
}

/// First-run download progress shown in the status line.
enum DownloadMsg {
    Status(String),
    Done(Result<(), String>),
}

/// Shared, interior-mutable state backing the `<mdpara>` render path. Body
/// paragraphs are painted justified by `justify`; anything the mini-parser
/// cannot express (images, raw HTML) falls back to the library with a cache of
/// its own, separate from the outer viewer's.
pub struct ParagraphUi {
    paragraphs: HashMap<String, String>,
    cache: CommonMarkCache,
    /// Reading column in effect this frame, set from the same `col` the viewer
    /// is given via `ui.set_width(col)`.
    column: f32,
}

impl ParagraphUi {
    fn new(paragraphs: HashMap<String, String>) -> Self {
        ParagraphUi {
            paragraphs,
            cache: CommonMarkCache::default(),
            column: 0.0,
        }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs. `ui.available_width()` inside the html hook is not that column.
    fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    fn reset(&mut self, paragraphs: HashMap<String, String>) {
        self.paragraphs = paragraphs;
        self.cache = CommonMarkCache::default();
    }

    fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Clone: the source rarely changes and this keeps `cache` free to borrow.
        let source = match self.paragraphs.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        // Library-routed paragraphs paint glyphs this module does not model, so
        // they are not searchable: skip recording rather than index text that is
        // not on screen.
        if paragraph_needs_library(&source) {
            let gap = crate::justify::block_gap(ui);
            ui.vertical(|ui| {
                CommonMarkViewer::new().show(ui, &mut self.cache, &source);
                ui.add_space(gap);
            });
            return;
        }
        let block = search.record(ui, crate::justify::search_text(&source));
        let (ranges, current) = search.highlights(block);
        let selection = search.selection_range(block);
        let runs = crate::justify::parse_inline(&source);
        let gap = crate::justify::block_gap(ui);
        let painted = crate::justify::draw_spaced(
            ui,
            &runs,
            self.column,
            crate::justify::MatchHighlight {
                ranges: &ranges,
                current,
                selection,
            },
            gap,
        );
        search.set_height(block, painted.height);
        search.set_rect(block, painted.rect);
        search.set_char_map(block, painted.chars);
        for rect in painted.interactives {
            search.add_control(rect, egui::CursorIcon::PointingHand);
        }
    }
}

/// Renders an HTML placeholder through the diagram, table or paragraph UI.
type HtmlFn = Box<dyn Fn(&mut egui::Ui, &str)>;

/// A transient clipboard confirmation: one plain line, bottom-left, drawn over
/// the window margin like zathura's status bar.
struct Status {
    message: &'static str,
    until: Instant,
}

/// How long a clipboard status line stays on screen.
const STATUS_DURATION: Duration = Duration::from_millis(1500);

/// Colour of the status line and the search prompt.
fn status_color(theme: Theme) -> egui::Color32 {
    crate::theme::palette(theme).subtext0.into()
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

/// Bottom-left status overlay, shared by copy messages and the search status.
fn status_area(ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new("jiaoren-status"))
        .order(egui::Order::Middle)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            // Solid chip: the status sits over the document and must stay legible.
            // `panel_fill` is the page colour here, so use the faint fill
            // (Catppuccin mantle) for contrast.
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, add);
        });
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
    /// Block index the next layout must centre in the viewport (a confirmed or
    /// stepped search hit). Applied once after `home_scroll`, before the scroll
    /// area runs.
    jump_to: Option<usize>,
    /// Edge slack from the last layout, `None` until the first layout. `gg` and
    /// `a` target it before this frame's layout has run; `Some` also tells the
    /// home/resize helper whether the view is still at the reading origin.
    pad: Option<egui::Vec2>,
    /// Set by `G`: jump to the bottom once the next layout reveals `max_y`.
    scroll_bottom: bool,
    applied_theme: Option<Theme>,
    keys: KeyState,
    /// One Ctrl+C Copy event per Ctrl hold; autorepeats are ignored.
    copy_guard: CopyGuard,
    /// Screen position where a selection drag started, for plain-click detection.
    select_press: Option<egui::Pos2>,
    /// Pointer moved far enough during this press to count as a drag.
    select_moved: bool,
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
        let theme = if dark { Theme::Dark } else { Theme::Light };
        // A cached FiraCode is installed before the first frame. On a fresh
        // cache, start with the system font (or egui's defaults) and download
        // in the background so the window still opens immediately.
        let cache_root = crate::mermaid::cache_root();
        crate::fonts::prune(&cache_root);
        // Documented order: override, cache, system, egui defaults. On a fresh
        // cache, start with the system font (or egui's defaults) and download
        // in the background so the window still opens immediately.
        let download = crate::fonts::download_needed(&cache_root);
        if let Some(defs) = crate::fonts::preferred(&cache_root) {
            ctx.set_fonts(defs);
        }
        let mut font_rx = None;
        if download {
            let (tx, rx) = mpsc::channel();
            let root = cache_root.clone();
            std::thread::spawn(move || {
                let mut send = |pct| {
                    let _ = tx.send(crate::fonts::FontMsg::Progress(pct));
                };
                let result = crate::fonts::download_cached(&root, &mut send);
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
            zoom: 1.0,
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
            jump_to: None,
            pad: None,
            scroll_bottom: false,
            applied_theme: Some(theme),
            keys: KeyState::default(),
            copy_guard: CopyGuard::default(),
            select_press: None,
            select_moved: false,
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
        self.load();
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
        // Old hits point into blocks that no longer exist.
        self.search.borrow_mut().state.clear();
        self.search.borrow_mut().selection.clear();
        self.jump_to = None;
    }

    /// Show a clipboard confirmation for [`STATUS_DURATION`].
    fn set_status(&mut self, message: &'static str) {
        self.status = Some(Status {
            message,
            until: Instant::now() + STATUS_DURATION,
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
            self.download_rx = None;
            self.download_status = None;
            self.diagrams.borrow_mut().set_ready(true);
            if let Err(msg) = result {
                self.chromium_error = Some(msg);
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
        if self.search.borrow().is_open() {
            let line = format!("/{}", self.search.borrow().query());
            prompt_area(ctx, line);
            return;
        }
        if let Some(status) = &self.status {
            let remaining = status.until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.status = None;
            } else {
                let message = status.message;
                status_area(ctx, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(message).color(status_color(self.theme)),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
                ctx.request_repaint_after(remaining);
                return;
            }
        }
        if let Some(text) = self.search.borrow().state.status_text() {
            status_area(ctx, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text).color(status_color(self.theme)))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
            return;
        }
        if let Some(text) = &self.download_status {
            status_area(ctx, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text).color(status_color(self.theme)))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
            return;
        }
        if let Some(pct) = self.font_progress {
            let text = font_progress_text(pct);
            status_area(ctx, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text).color(status_color(self.theme)))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
        }
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
        if pressed {
            if let Some(p) = pos {
                if s.pointer_on_control(p) {
                    // Link/copy-button presses own the click; keep any existing
                    // selection untouched.
                } else if let Some(point) = s.hit_near(p) {
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
            if let Some(p) = pos {
                if self
                    .select_press
                    .is_some_and(|start| p.distance(start) > 3.0)
                {
                    self.select_moved = true;
                }
                if let Some(point) = s.hit(p) {
                    s.selection.extend(point);
                }
            }
        }
        if released && s.selection.dragging {
            s.selection.dragging = false;
            // A plain click, or a drag that stayed within one char, is not a
            // range selection.
            if !self.select_moved || !s.selection.is_active() {
                s.selection.clear();
            }
            self.select_press = None;
        }
    }

    /// Route Esc/Ctrl+C: cancel the prompt, clear a committed search, clear the
    /// selection, or quit.
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
            CancelAction::ClearSearch => {
                self.search.borrow_mut().state.clear();
                self.jump_to = None;
                ctx.request_repaint();
            }
            CancelAction::ClearSelection => {
                self.search.borrow_mut().selection.clear();
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
            ctx.request_repaint();
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
                        self.font_progress = None;
                        done = true;
                    }
                    Ok(crate::fonts::FontMsg::Done(Err(_))) => {
                        self.font_progress = None;
                        done = true;
                        self.set_status(font_failure_status());
                        ctx.request_repaint();
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
        // Pointer selection runs before the scroll area so a press on text can
        // suppress drag-panning in the same frame.
        if !popup_open && !prompt_open {
            self.process_selection(ui);
        }
        // If a release/focus-loss happened while the prompt/popup blocked the
        // pointer path, do not keep SelectArea drag-panning disabled.
        if !ui.input(|i| i.pointer.primary_down()) {
            let mut s = self.search.borrow_mut();
            if s.selection.dragging {
                s.selection.dragging = false;
                self.select_press = None;
            }
        }
        let mut acts: Vec<Action> = Vec::new();
        if !popup_open && prompt_open {
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
                    if let Some(query) = s.input.as_mut() {
                        for edit in &batch.edits {
                            match edit {
                                SearchEdit::Text(text) => query.push_str(text),
                                SearchEdit::Backspace => {
                                    query.pop();
                                }
                                SearchEdit::DeleteWord => truncate_word_tail(query),
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
                    // A live edit jumps to its match; Enter keeps it. Cancelling
                    // already returned through cancel_search above.
                    self.jump_to = target;
                    ctx.request_repaint();
                }
                // Keys after Enter run through the document action path, so
                // `Enter n` commits the pattern and then steps.
                for (key, modifiers) in batch.after_enter {
                    if let Some(action) = apply_key(&mut self.keys, key, modifiers, now) {
                        acts.push(action);
                    }
                }
            }
        } else if !popup_open {
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
                        // A held `q`/`Esc`/`Ctrl+C` keeps repeating after it
                        // acted; only a fresh press may quit.
                        if *repeat
                            && (matches!(*key, egui::Key::Q | egui::Key::Escape)
                                || (*key == egui::Key::C && (modifiers.command || modifiers.ctrl)))
                        {
                            continue;
                        }
                        // The next repeat of the Ctrl+C that closed the popup
                        // must not quit either.
                        if suppress_quit
                            && *key == egui::Key::C
                            && (modifiers.command || modifiers.ctrl)
                        {
                            continue;
                        }
                        // `y` copies an active drag selection; with none it
                        // keeps arming `yy`.
                        if *key == egui::Key::Y
                            && !modifiers.any()
                            && self.search.borrow().selection.is_active()
                        {
                            acts.push(Action::CopySelection);
                            continue;
                        }
                        if let Some(a) = apply_key(&mut self.keys, *key, *modifiers, now) {
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
                Action::ScrollLines(f) => self.scroll_y = (self.scroll_y + f * 40.0).max(0.0),
                Action::ScrollPages(f) => self.scroll_y = (self.scroll_y + f * page_h).max(0.0),
                Action::ScrollCols(f) => self.scroll_x = (self.scroll_x + f * 40.0).max(0.0),
                Action::Top => {
                    let home = home_target(self.pad);
                    self.scroll_x = home.x;
                    self.scroll_y = home.y;
                }
                Action::Bottom => self.scroll_bottom = true,
                Action::ZoomIn => self.zoom = (self.zoom * 1.1).min(3.0),
                Action::ZoomOut => self.zoom = (self.zoom / 1.1).max(0.5),
                Action::Fit => {
                    self.zoom = 1.0;
                    self.scroll_x = self.pad.map_or(0.0, |p| p.x);
                }
                Action::ResetZoom => self.zoom = 1.0,
                Action::ToggleTheme => {
                    self.theme = match self.theme {
                        Theme::Light => Theme::Dark,
                        Theme::Dark => Theme::Light,
                    }
                }
                Action::Reload => self.reload(),
                Action::CopyDocument => {
                    ui.ctx().copy_text(self.source.clone());
                    self.set_status("copied markdown");
                }
                Action::SearchOpen => {
                    self.search.borrow_mut().open();
                    ui.ctx().request_repaint();
                }
                Action::SearchNext => self.step_search(1, ui.ctx()),
                Action::SearchPrev => self.step_search(-1, ui.ctx()),
                Action::CopySelection => {
                    let text = {
                        let s = self.search.borrow();
                        selected_text(s.texts(), &s.selection)
                    };
                    if let Some(text) = text {
                        if !text.is_empty() {
                            ui.ctx().copy_text(text);
                            self.set_status("copied selection");
                            // A selection copy is not a `yy` press: do not let a
                            // stale arm fire a whole-document copy on the next y.
                            self.keys.last_y = None;
                        }
                    }
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
            // Scale text with zoom so text and diagrams stay consistent.
            let z = self.zoom;
            let styles = ui.style().text_styles.clone();
            ui.style_mut().text_styles = styles
                .into_iter()
                .map(|(kind, font)| (kind, egui::FontId::new(font.size * z, font.family)))
                .collect();

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
            self.scroll_x = home.x;
            self.scroll_y = home.y;
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
                    mouse_wheel: !popup_open,
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
                        self.code.borrow_mut().begin_frame(col);
                        self.diagrams.borrow_mut().begin_frame(col);
                        self.images.borrow_mut().begin_frame(col, z);
                        self.search.borrow_mut().begin_frame();
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
            if adjust != 0.0 || jump_bottom {
                // Re-lay out next frame at the corrected offset.
                ui.ctx().request_repaint();
            }
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
        self.draw_status(ui.ctx());
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
        use super::super::paragraph_needs_library;

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

    mod theme {
        use super::super::{apply_theme, Theme};
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
}
