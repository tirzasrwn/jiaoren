//! The eframe application: window state, scrolling and zoom, and the Markdown
//! render hook that swaps placeholders for diagrams, tables and paragraphs.

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

mod diagram;
mod geometry;
mod keys;
mod list;
mod placeholder;
mod raster;
mod table;

use diagram::DiagramUi;
use geometry::{bottom_pad, content_column};
use keys::{apply_key, quit_suppressed, Action, KeyState};
use list::ListUi;
use placeholder::{parse, Placeholder};
use table::{table_pad, TableUi};

/// Re-export of [`crate::doc::read_doc`] for the app's reload path.
pub use crate::doc::read_doc;

/// Lock a mutex, recovering the guard if a previous holder panicked. Poisoning
/// must never deadlock the UI thread or silently drop queued renders.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

    fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        // Clone: the source rarely changes and this keeps `cache` free to borrow.
        let source = match self.paragraphs.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        if source.contains("![") || source.contains('<') {
            CommonMarkViewer::new().show(ui, &mut self.cache, &source);
            return;
        }
        let runs = crate::justify::parse_inline(&source);
        crate::justify::draw(ui, &runs, self.column);
    }
}

/// Renders an HTML placeholder through the diagram, table or paragraph UI.
type HtmlFn = Box<dyn Fn(&mut egui::Ui, &str)>;

/// The jiaoren window: document state, theme/zoom, scrolling, and the HTML
/// render hook.
pub(crate) struct App {
    /// Path of the open document.
    pub path: PathBuf,
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
    html_fn: HtmlFn,
    scroll_y: f32,
    scroll_x: f32,
    /// Set by `G`: jump to the bottom once the next layout reveals `max_y`.
    scroll_bottom: bool,
    applied_theme: Option<Theme>,
    keys: KeyState,
    #[expect(
        dead_code,
        reason = "the watcher must stay alive for the callback to fire"
    )]
    watcher: Option<notify::RecommendedWatcher>,
    watch_rx: Option<mpsc::Receiver<notify::Event>>,
    pending_reload: Option<Instant>,
    last_mtime: Option<SystemTime>,
    last_poll: Instant,
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
        let html_fn: HtmlFn = {
            let d = Rc::clone(&diagrams);
            let t = Rc::clone(&tables);
            let l = Rc::clone(&lists);
            let p = Rc::clone(&paragraphs);
            Box::new(move |ui: &mut egui::Ui, html: &str| match parse(html) {
                Some(Placeholder::Paragraph(id)) => p.borrow_mut().show(ui, id),
                Some(Placeholder::Table(id)) => t.borrow_mut().show(ui, id),
                Some(Placeholder::List(id)) => l.borrow_mut().show(ui, id),
                Some(Placeholder::Mermaid(id)) => d.borrow_mut().show(ui, id),
                Some(Placeholder::Space) => {
                    ui.add_space(table_pad(ui.text_style_height(&egui::TextStyle::Body)))
                }
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
            html_fn,
            scroll_y: 0.0,
            scroll_x: 0.0,
            scroll_bottom: false,
            applied_theme: Some(theme),
            keys: KeyState::default(),
            watcher,
            watch_rx,
            pending_reload: None,
            last_mtime,
            last_poll: Instant::now(),
        }
    }

    /// Re-read the document, keeping the previous state if reading fails.
    pub fn load(&mut self) {
        match read_doc(&self.path) {
            Ok(text) => {
                self.prepared = prepare(&text, parent_dir(&self.path));
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
        let popup_open = self.diagrams.borrow().popup.is_some();
        let mut popup_closed_at = self.diagrams.borrow().popup_closed_at;
        // Turn this frame's key presses into viewer actions.
        let now = Instant::now();
        let input_time = ui.input(|i| i.time);
        // X autorepeat starts after ~660 ms, outlasting the 0.30 s guard, so
        // keep the guard alive while the Ctrl that closed the popup is held.
        // After release the normal 0.30 s window applies.
        if self.diagrams.borrow().popup_closed_with_ctrl {
            if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                self.diagrams.borrow_mut().popup_closed_at = Some(input_time);
                popup_closed_at = Some(input_time);
            } else {
                self.diagrams.borrow_mut().popup_closed_with_ctrl = false;
            }
        }
        let suppress_quit = quit_suppressed(input_time, popup_closed_at);
        let mut acts: Vec<Action> = Vec::new();
        if !popup_open {
            ui.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Key {
                        key,
                        modifiers,
                        pressed: true,
                        repeat,
                        ..
                    } = ev
                    {
                        // A held `q`/`Esc` keeps repeating after it closed the
                        // popup; only a fresh press may quit.
                        if *repeat && matches!(*key, egui::Key::Q | egui::Key::Escape) {
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
                        if let Some(a) = apply_key(&mut self.keys, *key, *modifiers, now) {
                            acts.push(a);
                        }
                    } else if !suppress_quit && matches!(ev, egui::Event::Copy) {
                        // winit reports Ctrl+C as `Copy` rather than `Key::C`.
                        acts.push(Action::Quit);
                    }
                }
            });
        }
        let page_h = ui.available_height();
        for act in acts {
            match act {
                Action::ScrollLines(f) => self.scroll_y = (self.scroll_y + f * 40.0).max(0.0),
                Action::ScrollPages(f) => self.scroll_y = (self.scroll_y + f * page_h).max(0.0),
                Action::ScrollCols(f) => self.scroll_x = (self.scroll_x + f * 40.0).max(0.0),
                Action::Top => self.scroll_y = 0.0,
                Action::Bottom => self.scroll_bottom = true,
                Action::ZoomIn => self.zoom = (self.zoom * 1.1).min(3.0),
                Action::ZoomOut => self.zoom = (self.zoom / 1.1).max(0.5),
                Action::Fit => {
                    self.zoom = 1.0;
                    self.scroll_x = 0.0;
                }
                Action::ResetZoom => self.zoom = 1.0,
                Action::ToggleTheme => {
                    self.theme = match self.theme {
                        Theme::Light => Theme::Dark,
                        Theme::Dark => Theme::Light,
                    }
                }
                Action::Reload => self.reload(),
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
            let scroll_y = self.scroll_y;
            let scroll_x = self.scroll_x;
            let mut scroll = egui::ScrollArea::both()
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll_y)
                .horizontal_scroll_offset(scroll_x);
            if popup_open {
                // Freeze the document while the overlay is up.
                scroll = scroll.scroll_source(egui::scroll_area::ScrollSource {
                    scroll_bar: true,
                    drag: egui::scroll_area::DragScroll::Never,
                    mouse_wheel: false,
                });
            }
            let output = scroll.show(ui, |ui| {
                let available = ui.available_width();
                let col = content_column(available, z);
                column = col;
                let indent = ((available - col) * 0.5).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.vertical(|ui| {
                        ui.set_width(col);
                        let html_fn = &self.html_fn;
                        let cache = &mut self.cache;
                        let md = &self.prepared.markdown;
                        self.tables.borrow_mut().begin_frame();
                        self.lists.borrow_mut().begin_frame(col);
                        self.paragraphs.borrow_mut().begin_frame(col);
                        self.diagrams.borrow_mut().begin_frame(col);
                        CommonMarkViewer::new()
                            .max_image_width(Some(col as usize))
                            .render_html_fn(Some(&**html_fn))
                            .show(ui, cache, md);
                    });
                });
                ui.add_space(bottom_pad(viewport_h));
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
            if adjust != 0.0 || jump_bottom {
                // Re-lay out next frame at the corrected offset.
                ui.ctx().request_repaint();
            }
        });
        // Drawn last so the foreground overlay sits above the document.
        self.diagrams.borrow_mut().draw_popup(ui.ctx(), column);
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
