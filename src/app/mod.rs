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
use crate::mermaid::{ensure_chromium, Renderer, Theme};

mod code;
mod diagram;
mod geometry;
mod heading;
mod image;
mod keys;
mod list;
mod placeholder;
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
            CommonMarkViewer::new().show(ui, &mut self.cache, &source);
            return;
        }
        let block = search.record(ui, crate::justify::search_text(&source));
        let (ranges, current) = search.highlights(block);
        let selection = search.selection_range(block);
        let runs = crate::justify::parse_inline(&source);
        let painted = crate::justify::draw(
            ui,
            &runs,
            self.column,
            crate::justify::MatchHighlight {
                ranges: &ranges,
                current,
                selection,
            },
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

/// Bottom-left status overlay, shared by copy messages and the search status.
fn status_area(ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new("jiaoren-status"))
        .order(egui::Order::Middle)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            // Solid chip: the status sits over the document and must stay legible.
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, add);
        });
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
}

impl App {
    /// Build the app for `path` from its already-read `text`, installing the
    /// font and theme before the first frame.
    pub fn new(path: PathBuf, text: String, dark: bool, ctx: &egui::Context) -> Self {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        // Once, before the first frame: FiraCode primary, egui defaults as fallback.
        crate::fonts::install(ctx);
        apply_theme(ctx, theme);
        let mut renderer = Renderer::new();
        // First-run downloads happen here, once, on the UI thread, before any
        // render worker can spawn: concurrent workers must never race on the
        // shared temp file.
        let mut chromium_error = None;
        match ensure_chromium(&renderer.cache_dir) {
            Ok(path) => {
                renderer.chromium = Some(path);
                let _ = renderer.ensure_mermaid_js();
            }
            Err(msg) => {
                renderer.chromium = None;
                chromium_error = Some(msg);
            }
        }
        let shared = Arc::new(Renderer {
            chromium: renderer.chromium.clone(),
            mermaid_js: renderer.mermaid_js.clone(),
            cache_dir: renderer.cache_dir.clone(),
        });

        let prepared = prepare(&text, parent_dir(&path));

        let diagrams = Rc::new(RefCell::new(DiagramUi::new(
            shared,
            prepared.diagrams.clone(),
            theme,
        )));
        let tables = Rc::new(RefCell::new(TableUi::new(prepared.tables.clone())));
        let lists = Rc::new(RefCell::new(ListUi::new(prepared.lists.clone())));
        let paragraphs = Rc::new(RefCell::new(ParagraphUi::new(prepared.paragraphs.clone())));
        let images = Rc::new(RefCell::new(ImageUi::new(prepared.images.clone())));
        let headings = Rc::new(RefCell::new(HeadingUi::new(prepared.headings.clone())));
        let code = Rc::new(RefCell::new(CodeUi::new(prepared.code.clone())));
        let search = Rc::new(RefCell::new(SearchUi::new()));
        let html_fn: HtmlFn = {
            let d = Rc::clone(&diagrams);
            let t = Rc::clone(&tables);
            let l = Rc::clone(&lists);
            let p = Rc::clone(&paragraphs);
            let i = Rc::clone(&images);
            let h = Rc::clone(&headings);
            let c = Rc::clone(&code);
            let s = Rc::clone(&search);
            Box::new(move |ui: &mut egui::Ui, html: &str| match parse(html) {
                Some(Placeholder::Paragraph(id)) => {
                    p.borrow_mut().show(ui, id, &mut s.borrow_mut())
                }
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

    /// Paint the bottom-left status line: a transient copy message, otherwise
    /// the active search (`/pat [i/n]`). Anchored in the window margin, so it
    /// never takes layout from the reading column; the document's bottom
    /// padding keeps text clear of it.
    fn draw_status(&mut self, ctx: &egui::Context) {
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
        }
    }

    /// The `/` command line: a single-line edit at the status position, focused
    /// when nothing else owns the keyboard.
    fn draw_search_prompt(&mut self, ctx: &egui::Context) {
        let input_id = egui::Id::new("jiaoren-search-input");
        // Focus before the widget runs: the TextEdit then sees this frame's
        // text/paste/IME events, and a `/` that opened the prompt in this same
        // frame can accept a paste. The widget is drawn below, so egui's
        // dead-man focus switch keeps the id.
        ctx.memory_mut(|m| m.request_focus(input_id));
        let mut text = self.search.borrow_mut().input.take().unwrap_or_default();
        egui::Area::new(egui::Id::new("jiaoren-search"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
            .show(ctx, |ui| {
                ui.set_max_width(500.0);
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(input_id)
                        .desired_width(500.0)
                        .hint_text("search"),
                );
            });
        self.search.borrow_mut().input = Some(text);
    }

    /// Confirm the `/` buffer: compute hits over the last drawn block texts,
    /// remember them, jump to the first match and close the command line.
    fn confirm_search(&mut self, ctx: &egui::Context) {
        let Some(pattern) = self.search.borrow_mut().take_input() else {
            return;
        };
        let mut s = self.search.borrow_mut();
        // Recorded block texts in document order from the previous frame.
        let texts = s.texts().to_vec();
        s.state.set_pattern(&texts, &pattern);
        let target = s.state.current_block();
        self.jump_to = target;
        // The prompt was drawn before the keys this frame; repaint so the
        // status line replaces it even when there is nothing to jump to.
        ctx.request_repaint();
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
                self.search.borrow_mut().cancel();
                self.jump_to = None;
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
        self.diagrams.borrow_mut().poll(ctx);
        self.images.borrow_mut().poll(ctx);
        self.watch_tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        // The prompt is drawn before any key is acted on, so its TextEdit has
        // already consumed this frame's text, paste and IME events. Enter then
        // confirms the current buffer instead of the previous frame's.
        let prompt_open = self.search.borrow().is_open();
        let mut prompt_drawn = prompt_open;
        if !popup_open && prompt_open {
            self.draw_search_prompt(ui.ctx());
        }
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
            // While the command line owns the keyboard, document keys are
            // suppressed: `q`, `t`, `r`, `0`, `/`, … reach the TextEdit.
            let mut keys: Vec<(egui::Key, egui::Modifiers)> = Vec::new();
            let mut copy = false;
            ui.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Key {
                        key,
                        modifiers,
                        pressed: true,
                        ..
                    } = ev
                    {
                        keys.push((*key, *modifiers));
                    } else if matches!(ev, egui::Event::Copy) {
                        copy = true;
                    }
                }
            });
            let ctx = ui.ctx().clone();
            // Process Copy before keys: a same-frame Enter+Copy cancels the
            // prompt instead of confirming and then clearing. One Copy per Ctrl
            // hold is accepted, so autorepeats cannot advance the ladder.
            let mut cancelled = false;
            if copy && self.copy_guard.accept() {
                self.cancel_search(&ctx);
                cancelled = true;
            }
            // A single Ctrl+C may arrive as Copy and as Key::C; only the first
            // cancel in this frame may act, so a prompt cancel never also
            // clears the committed pattern behind it.
            for (key, modifiers) in keys {
                match key {
                    // Enter first, so a following `n` in the same batch steps.
                    egui::Key::Enter if self.search.borrow().is_open() => {
                        self.confirm_search(&ctx);
                    }
                    egui::Key::Escape if !cancelled => {
                        self.cancel_search(&ctx);
                        cancelled = true;
                    }
                    egui::Key::C if !cancelled && (modifiers.ctrl || modifiers.command) => {
                        self.cancel_search(&ctx);
                        cancelled = true;
                    }
                    // A character key typed before Enter stays text; only a
                    // key after the prompt closed can step.
                    egui::Key::N
                        if !self.search.borrow().is_open()
                            && modifiers.matches_exact(egui::Modifiers::SHIFT) =>
                    {
                        self.step_search(-1, &ctx);
                    }
                    egui::Key::N
                        if !self.search.borrow().is_open()
                            && modifiers.matches_exact(egui::Modifiers::NONE) =>
                    {
                        self.step_search(1, &ctx);
                    }
                    _ => {}
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
                    // The printable event for the `/` that opened the prompt
                    // must not become part of the pattern when the prompt is
                    // drawn (and focused) in this same frame. Drop just the
                    // first `/` text event.
                    let mut dropped = false;
                    ui.input_mut(|i| {
                        i.events.retain(|ev| {
                            if !dropped {
                                if let egui::Event::Text(text) = ev {
                                    if text == "/" {
                                        dropped = true;
                                        return false;
                                    }
                                }
                            }
                            true
                        });
                    });
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
        // If `/` opened the prompt this frame, draw it now: the TextEdit gets
        // focus before it handles events, so a paste/type in this frame lands in
        // the buffer.
        if !prompt_drawn && !popup_open && self.search.borrow().is_open() {
            self.draw_search_prompt(ui.ctx());
            prompt_drawn = true;
            ui.ctx().request_repaint();
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
        // The prompt already occupied the bottom-left this frame; the next
        // frame shows the confirmed status instead of drawing both at once.
        if !prompt_drawn {
            self.draw_status(ui.ctx());
        }
        // Final cursor wins over the scroll area's Grab: controls first, text
        // second, pan feedback third.
        if !popup_open && !prompt_open {
            if let Some(pos) = ui.ctx().pointer_hover_pos() {
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
