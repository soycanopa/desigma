//! Window shell: lays out the panels, routes shortcuts and drives screenshots.

use crate::app::{App, MenuKind};
use crate::canvas::tools::Tool;
use crate::canvas::{self, interact};
use crate::cli::Options;
use crate::demo;
use crate::icons::Icon;
use crate::panels::menus::{self, Action};
use crate::panels::{self, left, rail, right, toolbar};
use crate::theme;
use crate::widgets;
use egui::{Align2, Context, Event, Key, Pos2, Rect, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once an `--screenshot` run has written its PNG.
pub static SHOT_OK: AtomicBool = AtomicBool::new(false);

struct Shot {
    path: PathBuf,
    size: (u32, u32),
    /// Device pixel ratio: the PNG is `size * scale` pixels, laid out as `size` points.
    scale: f32,
    frames: u32,
    requested: bool,
}

pub struct Shell {
    /// One editor session per open file; `active` is drawn when the home screen is not.
    tabs: Vec<App>,
    active: usize,
    /// The home-screen session (file browser). `Some` = it exists; shown while `show_home`.
    home: Option<App>,
    show_home: bool,
    /// Library root every new session re-attaches (None in tests without a library).
    lib_root: Option<PathBuf>,
    shot: Option<Shot>,
    title: String,
    /// Listens for links and files handed over by a second launch; `None` when another editor owns it.
    _instance: Option<crate::instance::Instance>,
}

/// Window-level state that follows the focus between sessions: the agent bridge and the
/// updater. Plugins belong to their document and are stopped when it is left.
struct WindowBits {
    bridge: crate::bridge::BridgeState,
    updater: crate::updater::UpdaterState,
}

impl Shell {
    /// The session being drawn: the home screen while it is shown, else the active tab.
    pub fn session(&mut self) -> &mut App {
        if self.show_home {
            self.home.as_mut().expect("the home session exists while shown")
        } else {
            &mut self.tabs[self.active]
        }
    }

    pub fn peek(&self) -> &App {
        if self.show_home {
            self.home.as_ref().expect("the home session exists while shown")
        } else {
            &self.tabs[self.active]
        }
    }

    /// Whether the home screen is shown in place of a tab (the title-bar home button state).
    pub(crate) fn showing_home(&self) -> bool {
        self.show_home
    }

    /// How many open editor tabs there are.
    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Index of the active tab.
    pub(crate) fn active_tab(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }

    /// The session of tab `i` (any tab, not just the active one).
    pub(crate) fn tab_at(&self, i: usize) -> &App {
        &self.tabs[i]
    }

    fn take_window_bits(app: &mut App) -> WindowBits {
        WindowBits { bridge: std::mem::take(&mut app.bridge), updater: std::mem::take(&mut app.updater) }
    }

    fn give_window_bits(app: &mut App, bits: WindowBits) {
        app.bridge = bits.bridge;
        app.updater = bits.updater;
        if let Some(b) = &app.bridge.bridge {
            b.update_document(&app.doc.name, app.file_path.as_deref());
        }
    }

    /// A fresh editor session with an empty untitled document (and the library attached).
    fn blank_session(&self) -> App {
        let mut doc = desigma_doc::Document::new("Untitled");
        doc.add_page("Page 1");
        let mut app = App::new(doc);
        if let Some(root) = self.lib_root.clone() {
            app.attach_library(root);
        }
        app
    }

    /// Saves and parks the session being left (autosave, version snapshot, plugin).
    /// Nothing to park when no tab is open.
    fn park(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let app = self.session();
        crate::decks::ui::flush_notes(app);
        app.save_close_version();
        app.stop_plugin();
        app.flush_autosave();
        app.canvas.reset_interaction();
    }

    /// The title-bar home button: show the file browser, keeping every tab open.
    /// Also the landing spot when the last tab closes; `bits` carries window state onto it.
    /// While the session being left is sharing or joined, "Stop sharing?" / "Leave the
    /// session?" comes first (agreeing ends it and re-requests home; see `collab`).
    pub(crate) fn enter_home(&mut self) {
        if self.show_home {
            return;
        }
        // The sharing question comes before the library check, like `go_home`: agreeing ends
        // the session and re-requests home next frame.
        if !self.tabs.is_empty() && !self.session().leave_ok(|app| app.want_home = true) {
            return;
        }
        if self.lib_root.is_none() {
            self.session().show_toast("The projects library is unavailable", true);
            return;
        }
        self.park();
        let bits = (!self.tabs.is_empty()).then(|| Self::take_window_bits(&mut self.tabs[self.active]));
        if self.home.is_none() {
            let mut app = self.blank_session();
            let mut home = crate::home::Home::new();
            if let Some(lib) = &app.lib {
                home.refresh(lib);
            }
            app.home = Some(home);
            self.home = Some(app);
        } else if let Some(home) = self.home.as_mut().and_then(|h| h.home.as_mut()) {
            // other tabs may have saved while home was parked
            home.dirty = true;
        }
        if let Some(bits) = bits {
            Self::give_window_bits(self.home.as_mut().expect("home session"), bits);
        }
        self.show_home = true;
    }

    pub(crate) fn switch_tab(&mut self, i: usize) {
        let Some(last) = self.tabs.len().checked_sub(1) else { return };
        let i = i.min(last);
        if !self.show_home && i == self.active {
            return;
        }
        if self.show_home {
            let bits = Self::take_window_bits(self.home.as_mut().expect("home session"));
            self.show_home = false;
            self.active = i;
            Self::give_window_bits(&mut self.tabs[i], bits);
        } else {
            self.park();
            let bits = Self::take_window_bits(&mut self.tabs[self.active]);
            self.active = i;
            Self::give_window_bits(&mut self.tabs[i], bits);
        }
    }

    /// Closes tab `i`, saving like leaving the editor does. The window bits survive onto
    /// the neighbouring tab; closing the last tab shows the home screen.
    pub(crate) fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let was_active = !self.show_home && i == self.active;
        {
            let t = &mut self.tabs[i];
            crate::decks::ui::flush_notes(t);
            t.save_close_version();
            t.stop_plugin();
            t.flush_autosave();
        }
        let bits = if was_active { Some(Self::take_window_bits(&mut self.tabs[i])) } else { None };
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            // the last tab closed: the home screen (or a fresh document with no library)
            // takes over, carrying the window state with it
            self.active = 0;
            if self.home.is_some() || self.lib_root.is_some() {
                self.show_home = false;
                self.enter_home();
            } else {
                let app = self.blank_session();
                self.tabs.push(app);
                self.active = 0;
                self.show_home = false;
            }
            if let Some(bits) = bits {
                if self.show_home {
                    if let Some(h) = self.home.as_mut() {
                        Self::give_window_bits(h, bits);
                    }
                } else {
                    Self::give_window_bits(&mut self.tabs[self.active], bits);
                }
            }
            return;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        if let Some(bits) = bits {
            Self::give_window_bits(&mut self.tabs[self.active], bits);
        }
    }

    /// After the home screen opened a file (its session left home mode), that session
    /// becomes the newly active tab.
    fn promote_home(&mut self) {
        let promote = self.show_home && self.home.as_ref().is_some_and(|h| h.home.is_none());
        if !promote {
            return;
        }
        if let Some(h) = self.home.take() {
            self.tabs.push(h);
            self.active = self.tabs.len() - 1;
            self.show_home = false;
        }
    }

    /// A `.json` that is a variables or comments payload merges into the active document
    /// instead of opening as a file.
    fn merges_in_place(p: &std::path::Path) -> bool {
        let is_json = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        is_json && std::fs::read_to_string(p).is_ok_and(|t| crate::comments::import::is_comments_payload(&t) || crate::variables::is_variables_payload(&t))
    }

    /// Opens an external file as a new tab (or focuses the tab that already has it).
    pub(crate) fn open_external(&mut self, p: PathBuf) {
        if Self::merges_in_place(&p) {
            self.session().open_path(p);
            return;
        }
        if let Err(e) = crate::io::load_path(&p) {
            self.session().show_toast(e, true);
            return;
        }
        let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
        let same = |t: &App| t.file_path.as_ref().is_some_and(|f| f.canonicalize().unwrap_or_else(|_| f.to_path_buf()) == canon);
        if !self.show_home {
            if let Some(i) = self.tabs.iter().position(same) {
                self.switch_tab(i);
                return;
            }
        }
        if self.show_home {
            // from home the file opens in the home session, which promotes to a tab
            self.session().open_path(p);
        } else {
            let mut app = self.blank_session();
            app.open_path(p);
            let bits = Self::take_window_bits(self.session());
            Self::give_window_bits(&mut app, bits);
            self.tabs.push(app);
            self.active = self.tabs.len() - 1;
        }
    }

    /// The title-bar "+" (and File → New): a new file next to the open one, as a tab.
    pub(crate) fn open_new_file(&mut self, kind: crate::app::TabOpen) {
        use crate::app::TabOpen;
        if self.show_home {
            let target = self.session().home_target();
            match kind {
                TabOpen::NewFile => self.session().new_library_file(target),
                TabOpen::NewBoard => self.session().new_library_board(target),
                TabOpen::NewDeck => self.session().new_library_deck(target),
                TabOpen::Path(p) => self.session().open_path(p),
            }
            return;
        }
        let target = self.peek().lib_file.as_ref().map(|f| (f.project_id.clone(), None));
        let mut app = self.blank_session();
        match kind {
            TabOpen::NewFile => app.new_library_file(target),
            TabOpen::NewBoard => app.new_library_board(target),
            TabOpen::NewDeck => app.new_library_deck(target),
            TabOpen::Path(p) => app.open_path(p),
        }
        let bits = Self::take_window_bits(self.session());
        Self::give_window_bits(&mut app, bits);
        self.tabs.push(app);
        self.active = self.tabs.len() - 1;
    }

    /// Runs the tab-open and back-to-home requests the editor UI bubbled up this frame.
    fn run_shell_requests(&mut self) {
        let (req, home) = {
            let a = self.session();
            (a.open_tab.take(), a.want_home)
        };
        if home {
            self.session().want_home = false;
            self.enter_home();
            return;
        }
        match req {
            Some(crate::app::TabOpen::Path(p)) => self.open_external(p),
            Some(kind @ (crate::app::TabOpen::NewFile | crate::app::TabOpen::NewBoard | crate::app::TabOpen::NewDeck)) => self.open_new_file(kind),
            None => {}
        }
    }

    /// The label a tab shows: the library file name, else the document name
    /// (the file stem when the document carries none).
    pub(crate) fn tab_label(app: &App) -> String {
        if let Some(f) = &app.lib_file {
            return f.name.clone();
        }
        if app.doc.name.is_empty() {
            if let Some(s) = app.file_path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()) {
                return s.to_string();
            }
        }
        if app.doc.name.is_empty() {
            "Untitled".into()
        } else {
            app.doc.name.clone()
        }
    }
}

impl Shell {
    pub fn new(cc: &eframe::CreationContext<'_>, mut opts: Options) -> Self {
        // A real launch always has a library; screenshot runs only touch it for the home screen.
        if opts.library.is_none() && (opts.screenshot.is_none() || opts.home) {
            opts.library = Some(desigma_library::Library::default_root());
        }
        opts.home = opts.wants_home();
        let screenshot = opts.screenshot.is_some();
        // "Open in new window" children leave update checks to the first window
        let launch = !screenshot && opts.open_id.is_none();
        let (gpu_flag, bridge_flag, library) = (opts.gpu, opts.bridge, opts.library.clone());
        let mut shell = Self::build(&cc.egui_ctx, opts);
        if let Some(rs) = &cc.wgpu_render_state {
            if let Some(max) = canvas::gpu::init(rs) {
                shell.session().canvas.gpu.available = true;
                shell.session().canvas.gpu.set_max_texture(max);
            }
        }
        // GPU canvas on by default; screenshots stay on the deterministic CPU path unless --gpu
        let saved = library.as_deref().and_then(|l| crate::comments::settings::load_flag(l, "use_gpu"));
        shell.session().canvas.gpu.pref = if screenshot { gpu_flag } else { gpu_flag || saved.unwrap_or(true) };
        shell.session().updater.start(library.as_deref(), launch);
        // Links and files from a second launch (and, on macOS, from Finder and the browser)
        crate::instance::set_context(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        crate::macos::register();
        if launch {
            match crate::instance::Instance::start(&crate::instance::default_dir(), crate::instance::push) {
                Ok(i) => shell._instance = i,
                Err(e) => eprintln!("desigma: cannot listen for links: {e}"),
            }
        }
        // --bridge turns the agent bridge on for this launch only; the Preferences toggle saves it
        let saved_bridge = library.as_deref().and_then(|l| crate::comments::settings::load_flag(l, crate::bridge::FLAG)) == Some(true);
        if launch && (bridge_flag || saved_bridge) {
            if let Err(e) = shell.session().start_bridge(&cc.egui_ctx) {
                shell.session().show_toast(e, true);
            }
        }
        shell
    }

    pub fn build(ctx: &Context, opts: Options) -> Self {
        theme::install_fonts(ctx);
        theme::apply(ctx);
        // `--motion` on the plain demo opens the animated onboarding instead
        let motion_demo = opts.motion && opts.fixture.is_none() && opts.file.is_none() && opts.open_id.is_none() && !opts.home;
        let mut app = App::new(match opts.fixture.as_deref() {
            Some("no-selection") => demo::reference_document(),
            Some("variables") => demo::variables_document(),
            Some(crate::showcase::FIXTURE) => crate::showcase::document(),
            Some(crate::showcase::MOTION_FIXTURE) => crate::showcase::motion_document(),
            None if motion_demo => crate::showcase::motion_document(),
            Some(crate::protovars::FIXTURE) => crate::protovars::document(),
            Some("deck") => crate::decks::demo::document(),
            Some("board") => crate::board::demo::document(),
            Some("board-empty") => crate::board::create::new_document("Untitled"),
            Some(f) if crate::vecfixture::FIXTURES.contains(&f) => crate::vecfixture::document(),
            _ => demo::demo_document(),
        });
        if !matches!(opts.fixture.as_deref(), Some("variables" | "deck" | crate::showcase::FIXTURE | crate::showcase::MOTION_FIXTURE)) && !motion_demo {
            app.doc.name = "Untitled".into();
        }
        if opts.fixture.as_deref() == Some("deck") {
            if let Some(id) = app.deck_slides().first().copied() {
                app.select_slide(id);
            }
        }
        if opts.fixture.as_deref() == Some("comments") {
            crate::comments::fixture::apply(&mut app);
        }
        if let Some(f) = opts.fixture.as_deref().filter(|f| crate::vecfixture::FIXTURES.contains(f)) {
            crate::vecfixture::apply(&mut app, f);
        }
        if let Some(root) = opts.library.clone() {
            app.attach_library(root);
        }
        if let Some(f) = &opts.file {
            app.open_path(f.clone());
        }
        if let Some(link) = opts.join.clone() {
            app.open_join(Some(link));
        }
        if let Some(id) = &opts.open_id {
            app.open_library_file(id);
        } else if opts.home {
            app.go_home();
        }
        if let Some(name) = opts.select.as_ref().filter(|n| n.as_str() != "none") {
            // a top-level layer (screen) wins over same-named layers deeper in the tree
            let named =
                |top: bool| app.doc.nodes.values().find(|n| &n.name == name && (!top || n.parent.is_some_and(|p| app.doc.pages.contains(&p)))).map(|n| n.id);
            if let Some(id) = named(true).or_else(|| named(false)) {
                if let Some(pg) = app.doc.page_of(id) {
                    app.page = pg;
                }
                app.selection = vec![id];
            }
        }
        if let Some(spec) = &opts.menu {
            let (w, h) = opts.size.unwrap_or((theme::DEFAULT_WINDOW.0 as u32, theme::DEFAULT_WINDOW.1 as u32));
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32, h as f32));
            if !menus::preset_menu(ctx, &mut app, spec, screen) {
                eprintln!("desigma: unknown menu '{spec}'");
            }
        }
        if let Some(l) = &opts.dev {
            app.ui.dev_mode = true;
            app.ui.right_tab = Some(crate::app::RightTab::Design);
            app.dev.lang = match l.as_str() {
                "tailwind" => crate::dev::Language::Tailwind,
                "swiftui" => crate::dev::Language::SwiftUi,
                "compose" => crate::dev::Language::Compose,
                "react" => crate::dev::Language::React,
                _ => crate::dev::Language::Css,
            };
            if opts.dev_demo {
                crate::dev::preset::apply(&mut app);
            }
        }
        if opts.motion {
            crate::motion::timeline::open_demo(&mut app);
        }
        app.ui.rulers = opts.rulers;
        if opts.grid {
            app.ui.pixel_grid = true;
        }
        app.startup_zoom = opts.zoom.map(|z| z / 100.0);
        if let Some(kind) = &opts.layout_grid {
            let target = app.selection.first().copied();
            if let Some(id) = target {
                let k = ["grid", "columns", "rows"].iter().position(|n| n == kind).unwrap_or(1);
                // screenshot aid: a red 10% grid, columns with the usual 5 x 20px gutter
                let grid = crate::panels::layoutguide::default_grid(k, desigma_doc::Color::rgba(255, 0, 0, 26));
                app.edit_nodes(&[id], |n| {
                    if let Some(f) = n.kind.frame_props_mut() {
                        f.grids.push(grid.clone());
                    }
                });
            }
        }
        if let Some(name) = &opts.edit_text {
            let found = app.doc.nodes.values().find(|n| &n.name == name && matches!(n.kind, desigma_doc::NodeKind::Text(_))).map(|n| n.id);
            if let Some(id) = found {
                if let Some(pg) = app.doc.page_of(id) {
                    app.page = pg;
                }
                canvas::interact::begin_text_edit(&mut app, id);
                if let (Some((a, b)), Some(sess)) = (opts.select_text, app.canvas.text.as_mut()) {
                    if let Some(desigma_doc::NodeKind::Text(t)) = app.doc.get(id).map(|n| &n.kind) {
                        let byte = |c: usize| t.content.char_indices().nth(c).map(|(i, _)| i).unwrap_or(t.content.len());
                        sess.anchor = byte(a);
                        sess.caret = byte(b);
                    }
                }
            } else {
                eprintln!("desigma: no text layer named '{name}'");
            }
        }
        if opts.tab.as_deref() == Some("prototype") {
            app.ui.right_tab = Some(crate::app::RightTab::Prototype);
        }
        if opts.present {
            app.start_present();
            if let Some(st) = &mut app.proto.present {
                st.start_fullscreen = opts.fullscreen;
            }
            if opts.panel.as_deref() == Some("present-variables") {
                if opts.fixture.as_deref() == Some(crate::protovars::FIXTURE) {
                    crate::protovars::prime_present(&mut app);
                } else if let Some(st) = app.proto.present.as_mut() {
                    st.show_vars = true;
                }
            }
        }
        if let Some(q) = &opts.find {
            app.find.query = q.clone();
            app.open_find();
            app.find.focus = false;
            if let Some(r) = &opts.replace_with {
                app.find.replacement = r.clone();
                app.find.show_replace = true;
            }
        }
        match opts.panel.as_deref() {
            Some("interaction") => {
                app.ui.right_tab = Some(crate::app::RightTab::Prototype);
                crate::protovars::open_editor(&mut app);
            }
            Some("present-variables") => {}
            Some("libraries") => app.open_libraries(),
            Some(panel @ ("plugins" | "plugin-install")) => {
                app.open_plugins();
                app.plugins.install_form = panel == "plugin-install";
                app.plugins.fresh = app.plugins.install_form;
            }
            Some("version-history") => crate::history::preset::apply(&mut app, opts.library.clone()),
            Some(p @ ("motion-inspector" | "motion-easing" | "motion-library" | "motion-clip")) => crate::motion::inspector::preset(&mut app, p),
            Some(p @ ("dev-annotate" | "dev-compare")) => crate::dev::preset::panel(&mut app, p),
            Some("share") => app.open_share(),
            Some("collab") => crate::collab::preview(&mut app),
            Some(p @ ("updater" | "updater-banner")) => crate::updater::preset(&mut app, p),
            Some(panel @ ("variables" | "variables-floating")) => {
                app.vars.full = panel == "variables";
                app.open_variables();
            }
            Some(spec) => {
                let (w, h) = opts.size.unwrap_or((theme::DEFAULT_WINDOW.0 as u32, theme::DEFAULT_WINDOW.1 as u32));
                panels::properties::preset_popup(&mut app, spec, Vec2::new(w as f32, h as f32));
            }
            None => {}
        }
        let shot = opts.screenshot.map(|path| Shot {
            path,
            size: opts.size.unwrap_or((theme::DEFAULT_WINDOW.0 as u32, theme::DEFAULT_WINDOW.1 as u32)),
            scale: opts.scale.unwrap_or(1.0),
            frames: 0,
            requested: false,
        });
        // A session still on the home screen is the home session; anything else is tab 0.
        let (tabs, home, show_home) = if app.home.is_some() { (Vec::new(), Some(app), true) } else { (vec![app], None, false) };
        Self { tabs, active: 0, home, show_home, lib_root: opts.library, shot, title: String::new(), _instance: None }
    }

    fn drive_screenshot(&mut self, ctx: &Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        ctx.request_repaint();
        if shot.frames == 1 {
            // `scale` physical pixels per egui point (1 by default, so the PNG matches the requested size).
            ctx.set_pixels_per_point(shot.scale);
            let npp = ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(1.0).max(0.1);
            let phys = Vec2::new(shot.size.0 as f32 * shot.scale, shot.size.1 as f32 * shot.scale);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(phys / npp));
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            if let Event::Screenshot { image, .. } = ev {
                let (w, h) = (image.width() as u32, image.height() as u32);
                let raw: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w, h, raw) {
                    let want = ((shot.size.0 as f32 * shot.scale).round() as u32, (shot.size.1 as f32 * shot.scale).round() as u32);
                    let buf = if (w, h) != want { image::imageops::resize(&buf, want.0, want.1, image::imageops::FilterType::Lanczos3) } else { buf };
                    if let Some(dir) = shot.path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    match buf.save(&shot.path) {
                        Ok(()) => SHOT_OK.store(true, Ordering::SeqCst),
                        Err(e) => eprintln!("desigma: cannot write {}: {e}", shot.path.display()),
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        let sized = {
            let r = ctx.content_rect();
            (r.width() - shot.size.0 as f32).abs() < 1.5 && (r.height() - shot.size.1 as f32).abs() < 1.5
        };
        if !shot.requested && ((shot.frames >= 12 && sized) || shot.frames >= 60) {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if shot.frames > 240 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_keys(&mut self, ctx: &Context) {
        if self.session().plugins.open
            || self.session().dialog.is_some()
            || self.session().history.save_modal.is_some()
            || self.session().dev.compare.is_some()
            || self.session().updater.dialog
        {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        if self.session().vars.open {
            // The Variables view handles its own keys (and undo / redo).
            return;
        }
        // Open menus own the keyboard (arrows, Enter, Escape, typing in the palette).
        if self.session().ui.menu.is_some() || self.session().ui.ctx_menu.is_some() || self.session().vars.open {
            return;
        }
        let editing_text = self.session().canvas.text.is_some();
        if typing && !editing_text {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                Event::Copy if !editing_text && crate::motion::timeline::copy_keys(self.session(), ctx) => {}
                Event::Paste(_) if !editing_text && crate::motion::timeline::paste_keys(self.session(), ctx) => {}
                Event::Copy if !editing_text => {
                    if let Some(s) = self.session().copy_selection() {
                        ctx.copy_text(s);
                    }
                }
                Event::Cut if !editing_text => {
                    if let Some(s) = self.session().cut_selection() {
                        ctx.copy_text(s);
                    }
                }
                Event::Paste(s) if !editing_text => self.session().paste(Some(&s)),
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if editing_text {
                        continue;
                    }
                    self.key(ctx, key, modifiers);
                }
                _ => {}
            }
        }
    }

    fn key(&mut self, ctx: &Context, key: Key, m: egui::Modifiers) {
        let app = self.session();
        let cmd = m.command;
        if app.motion.enabled && crate::motion::timeline::key(app, key, m) {
            return;
        }
        if cmd {
            let act = match (key, m.shift, m.alt) {
                (Key::E, false, _) => Some(Action::Flatten),
                (Key::O, false, true) => Some(Action::OutlineStroke),
                (Key::J, false, false) => Some(Action::VectorJoin),
                (Key::J, true, false) => Some(Action::VectorSplit),
                (Key::Z, false, _) => Some(Action::Undo),
                (Key::Z, true, _) | (Key::Y, false, _) => Some(Action::Redo),
                (Key::O, false, _) => Some(Action::Open),
                (Key::S, false, true) => Some(Action::SaveVersion),
                (Key::S, false, _) => Some(Action::Save),
                (Key::S, true, _) => Some(Action::SaveAs),
                (Key::D, false, _) => Some(Action::Duplicate),
                (Key::G, false, false) => Some(Action::Group),
                (Key::G, true, _) => Some(Action::Ungroup),
                (Key::G, false, true) => Some(Action::FrameSelection),
                (Key::K, false, true) => Some(Action::CreateComponent),
                (Key::B, false, true) => Some(Action::Detach),
                (Key::K, true, false) => Some(Action::CombineVariants),
                (Key::A, true, false) => Some(Action::SelectInverse),
                (Key::A, false, true) => Some(Action::SelectMatching),
                (Key::A, false, _) => Some(Action::SelectAll),
                (Key::F, false, false) => Some(Action::Find),
                (Key::Quote, false, _) => Some(Action::TogglePixelGrid),
                (Key::Quote, true, _) => Some(Action::ToggleSnapPixel),
                (Key::Num4, true, _) => Some(Action::ToggleLayoutGrids),
                (Key::B, false, false) => Some(Action::TextBold),
                (Key::I, false, false) => Some(Action::TextItalic),
                (Key::U, false, false) => Some(Action::TextUnderline),
                (Key::Period, true, false) => Some(Action::TextSizeUp),
                (Key::Comma, true, false) => Some(Action::TextSizeDown),
                (Key::Num8, true, false) => Some(Action::TextBulletList),
                (Key::Num7, true, false) => Some(Action::TextNumberedList),
                (Key::CloseBracket, false, _) => Some(Action::BringForward),
                (Key::OpenBracket, false, _) => Some(Action::SendBackward),
                (Key::CloseBracket, true, _) => Some(Action::BringToFront),
                (Key::OpenBracket, true, _) => Some(Action::SendToBack),
                (Key::Equals | Key::Plus, _, _) => Some(Action::ZoomIn),
                (Key::Minus, _, _) => Some(Action::ZoomOut),
                (Key::Backslash, _, _) => Some(Action::ToggleSidebar),
                (Key::K, false, false) => Some(Action::OpenPalette),
                (Key::Enter, false, true) => {
                    app.start_present();
                    None
                }
                _ => None,
            };
            if let Some(a) = act {
                menus::run(app, ctx, a);
            }
            return;
        }
        if app.canvas.vedit.is_some() && canvas::vector::key(app, key, m) {
            return;
        }
        if app.canvas.builder.is_some() && matches!(key, Key::Escape | Key::Enter) {
            // a brush stroke in progress is dropped first; otherwise the result is applied
            if matches!(app.canvas.inter, canvas::Interaction::Builder) {
                canvas::builder::cancel_brush(app);
            } else {
                canvas::builder::finish(app);
                app.tool = Tool::Move;
            }
            return;
        }
        match key {
            // Shift+Backspace picks the eraser on a board
            Key::Backspace if m.shift && app.is_board() => app.set_tool(Tool::Eraser),
            Key::Delete | Key::Backspace => menus::run(app, ctx, Action::Delete),
            // an open comment draft or thread takes the Escape itself (egui drops text focus on Escape,
            // so the shortcut handler cannot tell that the key was meant for the text box)
            Key::Escape if app.comments.captures_escape() => {}
            Key::Escape => {
                if !matches!(app.canvas.inter, canvas::Interaction::None) {
                    app.cancel_interaction();
                } else if app.canvas.pen.is_some() {
                    interact::finish_pen(app);
                    app.tool = Tool::Move;
                } else if app.ui.menu.is_some() {
                    app.ui.menu = None;
                } else if app.proto.editing.is_some() {
                    app.proto.editing = None;
                } else if app.find.open {
                    app.close_find();
                } else if app.layers.filter_open {
                    app.close_layer_filter();
                } else if app.tool != Tool::Move {
                    app.tool = Tool::Move;
                } else {
                    app.clear_selection();
                }
            }
            Key::Enter => {
                if app.canvas.pen.is_some() {
                    interact::finish_pen(app);
                    app.tool = Tool::Move;
                } else if m.shift {
                    menus::run(app, ctx, Action::SelectParent);
                } else if let Some(n) = app.selection.first().filter(|_| app.selection.len() == 1).and_then(|s| app.doc.get(*s)) {
                    // A single text or vector layer is entered for editing; anything else selects its children.
                    let id = n.id;
                    if matches!(n.kind, desigma_doc::NodeKind::Text(_))
                        || matches!(n.kind, desigma_doc::NodeKind::Sticky(_) | desigma_doc::NodeKind::ShapeWithText(_) | desigma_doc::NodeKind::CodeBlock(_))
                    {
                        interact::begin_text_edit(app, id);
                    } else if matches!(n.kind, desigma_doc::NodeKind::Table(_)) {
                        crate::board::table::tab_selected(app, true);
                    } else if matches!(n.kind, desigma_doc::NodeKind::BoardSection(_)) {
                        crate::board::section::begin_rename(app, id);
                    } else if matches!(n.kind, desigma_doc::NodeKind::Connector(_)) {
                        crate::board::create::edit_label(app, id);
                    } else if canvas::vector::is_vector(app, id) {
                        // vectors and basic shapes (which become vectors on the first edit)
                        canvas::vector::enter(app, id);
                    } else {
                        menus::run(app, ctx, Action::SelectChildren);
                    }
                } else {
                    menus::run(app, ctx, Action::SelectChildren);
                }
            }
            // board: Tab starts the next sticky note to the right of the selected one
            Key::Tab if app.is_board() => {
                if m.shift {
                    crate::board::table::tab_selected(app, false);
                } else if !crate::board::create::tab_next(app) {
                    crate::board::table::tab_selected(app, true);
                }
            }
            Key::Backslash => menus::run(app, ctx, Action::SelectParent),
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                if !app.selection.is_empty() {
                    let step = if m.shift { theme::NUDGE_BIG } else { theme::NUDGE };
                    let (dx, dy) = match key {
                        Key::ArrowLeft => (-step, 0.0),
                        Key::ArrowRight => (step, 0.0),
                        Key::ArrowUp => (0.0, -step),
                        _ => (0.0, step),
                    };
                    app.nudge(dx, dy);
                }
            }
            Key::Num1 if m.shift => menus::run(app, ctx, Action::FitAll),
            Key::Num2 if m.shift => menus::run(app, ctx, Action::FitSelection),
            Key::Num0 if m.shift => menus::run(app, ctx, Action::Zoom100),
            Key::A if m.shift => menus::run(app, ctx, Action::AutoLayout),
            Key::R if m.shift => menus::run(app, ctx, Action::ToggleRulers),
            Key::Quote if m.shift => menus::run(app, ctx, Action::TogglePixelGrid),
            k => {
                if !m.alt {
                    let tool = if app.is_board() { Tool::from_key_board(k, m.shift) } else { Tool::from_key(k, m.shift) };
                    if let Some(t) = tool {
                        app.set_tool(t);
                    }
                }
            }
        }
    }

    /// Right-click on the canvas selects the layer under the pointer and opens the selection menu.
    fn context_menu(&mut self, ui: &egui::Ui, canvas_rect: Rect) {
        let (clicked, pos) = ui.ctx().input(|i| (i.pointer.secondary_clicked(), i.pointer.interact_pos()));
        let Some(pos) = pos.filter(|p| clicked && canvas_rect.contains(*p) && !self.session().vars.open) else { return };
        let app = self.session();
        if !matches!(app.canvas.inter, canvas::Interaction::None) || app.canvas.text.is_some() {
            return;
        }
        let page = app.view.to_page(app.canvas_rect.min, pos);
        match interact::hit_under(app, page, false) {
            Some(hit) if !app.is_selected(hit) => app.select_only(hit),
            Some(_) => {}
            None => app.clear_selection(),
        }
        app.ui.ctx_menu = Some(pos);
    }

    fn toast(&mut self, ui: &egui::Ui, screen: Rect) {
        if self.peek().toast.as_ref().is_some_and(|t| self.peek().now > t.until) {
            self.session().toast = None;
            return;
        }
        let Some(t) = self.peek().toast.as_ref() else { return };
        let p = ui.painter();
        let font = theme::font_body();
        let w = widgets::text_width(p, &t.text, &font) + 48.0;
        let r = Rect::from_center_size(
            Pos2::new(screen.center().x, screen.bottom() - theme::TOAST_BOTTOM - theme::TOAST_HEIGHT / 2.0),
            Vec2::new(w, theme::TOAST_HEIGHT),
        );
        widgets::elevation_shadow(p, r, theme::TOAST_RADIUS);
        p.rect_filled(r, theme::cr(theme::TOAST_RADIUS), theme::BG_MENU);
        p.circle_filled(Pos2::new(r.left() + 16.0, r.center().y), 4.0, if t.error { theme::DANGER } else { theme::BG_BRAND });
        p.text(Pos2::new(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, &t.text, font, theme::TEXT);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
}

impl eframe::App for Shell {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.17, 0.17, 0.17, 1.0]
    }

    /// Tab belongs to the text session (it nests list items): take it out of the input before egui
    /// uses it to move keyboard focus between widgets.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.session().plugins.open {
            return;
        }
        let Some(sess) = self.session().canvas.text.as_mut() else { return };
        if ctx.memory(|m| m.focused()).is_some() {
            return;
        }
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } if !modifiers.command => {
                if *pressed {
                    sess.tabs.push(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    /// Runs even while the window is minimized or occluded (eframe then skips `ui`), so agent
    /// calls keep being answered; the bridge server requests a repaint for each one.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.session().now = ctx.input(|i| i.time);
        self.session().poll_bridge();
        self.handle_opens(ctx);
        // Live sessions keep receiving messages while their tab is not the one being drawn.
        for t in &mut self.tabs {
            crate::collab::pump(t, ctx);
        }
        if let Some(h) = &mut self.home {
            crate::collab::pump(h, ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.session().poll_fonts();
        if self.session().fonts.is_loading() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.draw(ui);
    }
}

impl Shell {
    /// Links and files handed over by a second launch or the OS (see `instance.rs`): a link
    /// opens Join session prefilled (asking first when it would replace unsaved work), a file
    /// opens as a new tab, and the window comes to the front.
    fn handle_opens(&mut self, ctx: &Context) {
        let opens = crate::instance::take();
        if opens.is_empty() {
            return;
        }
        for open in opens {
            match open {
                crate::instance::Open::Link(link) => self.session().request_open(crate::instance::Open::Link(link)),
                crate::instance::Open::File(path) => self.open_external(path),
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        // when the OS refuses to steal focus this flashes the taskbar / bounces the Dock instead
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        ctx.request_repaint();
    }

    /// Files dropped on the window: `.svg` files are placed at the drop point,
    /// anything else (the first file) is opened as a new tab.
    fn handle_drops(&mut self, ctx: &Context) {
        let (files, pos) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.hover_pos().or(i.pointer.latest_pos())));
        let is_svg = |f: &egui::DroppedFile| {
            let name = f.path.as_ref().and_then(|p| p.to_str()).unwrap_or(&f.name);
            name.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        };
        if let Some(path) = files.iter().filter(|f| !is_svg(f)).find_map(|f| f.path.clone()) {
            // a new tab never throws the current document away, so no unsaved question here
            self.session().open_tab = Some(crate::app::TabOpen::Path(path));
            return;
        }
        for (nth, f) in files.iter().filter(|f| is_svg(f)).enumerate() {
            let text = match (&f.path, &f.bytes) {
                (Some(p), _) => std::fs::read_to_string(p).map_err(|e| format!("Cannot read {}: {e}", p.display())),
                (None, Some(b)) => String::from_utf8(b.to_vec()).map_err(|_| "The SVG is not valid UTF-8".to_string()),
                (None, None) => continue,
            };
            let stem = f
                .path
                .as_deref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .or_else(|| f.name.strip_suffix(".svg"))
                .filter(|s| !s.is_empty())
                .unwrap_or("svg")
                .to_string();
            match text {
                Ok(t) => crate::clipboard::drop_svg(self.session(), &t, &stem, pos, nth),
                Err(e) => self.session().show_toast(e, true),
            }
        }
    }

    fn sync_title(&mut self, ctx: &Context) {
        let title = if self.show_home { theme::APP_TITLE.to_string() } else { self.peek().title() };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// One frame of the home screen (file browser): files dropped on the window are imported.
    fn home_frame(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && self.session().dialog.is_none() {
            let target =
                self.session().home_target().or_else(|| self.session().home.as_ref().and_then(|h| h.snap.projects.first().map(|p| (p.id.clone(), None))));
            if let Some((p, f)) = target {
                self.session().import_into_library(&dropped, &p, f.as_deref());
            }
        }
        crate::home::show(self.session(), ui);
        crate::home::dialog::show(self.session(), ctx);
        crate::collab::ui::join_dialog(self.session(), ctx);
        crate::unsaved::dialog(self.session(), ctx);
        // opening a file from home turned the session into an editor tab
        self.promote_home();
        let full = ui.max_rect();
        self.toast(ui, full);
        self.sync_title(ctx);
    }

    /// One full UI pass over the window.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.session().now = ctx.input(|i| i.time);
        self.session().poll_plugins();
        self.session().poll_updater(&ctx);
        self.session().poll_install(&ctx);
        self.drive_screenshot(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            for t in &mut self.tabs {
                crate::decks::ui::flush_notes(t);
                t.save_close_version();
                t.flush_autosave();
            }
            if let Some(h) = &mut self.home {
                crate::decks::ui::flush_notes(h);
                h.save_close_version();
                h.flush_autosave();
            }
            // "Restart to update": everything is saved, so the new version can start now
            crate::updater::install::run_stashed();
        }
        self.session().scrub_sync(&ctx);
        // Present mode takes the whole window, including the title bar area.
        if !self.show_home && self.session().proto.present.is_some() {
            crate::present::show(self.session(), ui);
            return;
        }
        // The custom title bar spans the window top; the editor and the home screen lay
        // out in the body below it.
        let window = ui.max_rect();
        let bar = Rect::from_min_size(window.min, Vec2::new(window.width(), theme::TITLE_BAR_H));
        let full = Rect::from_min_max(Pos2::new(window.left(), bar.bottom()), window.max);
        crate::titlebar::show(self, ui, bar);
        if self.show_home {
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(full));
            self.home_frame(&mut body, &ctx);
            crate::plugins::ui::show(self.session(), &ctx);
            crate::updater::ui::show(self.session(), &ctx);
            return;
        }
        self.run_shell_requests();
        self.session().autosave_tick(&ctx);
        self.session().history_tick();
        self.session().dev_tick(&ctx);
        self.session().motion_tick(&ctx);
        self.handle_keys(&ctx);
        if !self.session().plugins.open {
            self.handle_drops(&ctx);
        }

        ui.painter().rect_filled(full, 0.0, theme::BG);
        let rail_w = theme::RAIL_WIDTH + theme::BORDER_WIDTH;
        let panel_w = theme::LEFT_PANEL_WIDTH + theme::BORDER_WIDTH;
        let rail_rect = Rect::from_min_max(full.min, Pos2::new(full.left() + rail_w, full.bottom()));
        let left_rect = Rect::from_min_max(Pos2::new(rail_rect.right(), full.top()), Pos2::new(rail_rect.right() + panel_w, full.bottom()));
        // a board has no right panel: its contextual toolbar floats over the canvas instead
        let board = self.session().is_board();
        let right_w = if board { 0.0 } else { theme::RIGHT_PANEL_WIDTH + theme::BORDER_WIDTH };
        let right_rect = Rect::from_min_max(Pos2::new(full.right() - right_w, full.top()), full.max);
        let canvas_left = if self.session().ui.left_collapsed { rail_rect.right() } else { left_rect.right() };
        let deck = self.session().is_deck();
        let notes_h = if deck { 132.0_f32.min(full.height() * 0.28) } else { 0.0 };
        let canvas_rect = Rect::from_min_max(Pos2::new(canvas_left, full.top()), Pos2::new(right_rect.left(), full.bottom() - notes_h));
        let notes_rect = Rect::from_min_max(Pos2::new(canvas_left, canvas_rect.bottom()), Pos2::new(right_rect.left(), full.bottom()));
        // Motion mode: the timeline docks under the canvas and the toolbar floats above it.
        let motion_on = self.session().motion.enabled && !board && !deck;
        let (canvas_rect, timeline_rect) = if motion_on {
            let (c, t) = crate::motion::timeline::split(&ctx, canvas_rect);
            (c, Some(t))
        } else {
            (canvas_rect, None)
        };

        let full_vars = self.session().vars.open && self.session().vars.full;
        if full_vars {
            // The local-variables full view replaces the left panel, canvas and right panel.
            rail::show(self.session(), ui, rail_rect);
            let content = Rect::from_min_max(Pos2::new(rail_rect.right(), full.top()), full.max);
            crate::variables::view::show(self.session(), ui, content);
        } else {
            canvas::show(self.session(), ui, canvas_rect);
            if let Some(tr) = timeline_rect {
                crate::motion::timeline::show(self.session(), ui, tr);
            }
            if deck {
                crate::decks::ui::notes(self.session(), ui, notes_rect);
            }
            rail::show(self.session(), ui, rail_rect);
            if self.session().ui.left_collapsed {
                let b = Rect::from_min_size(Pos2::new(canvas_rect.left() + 8.0, canvas_rect.top() + 8.0), Vec2::splat(theme::PANEL_ACTION_BUTTON));
                ui.painter().rect_filled(b, theme::cr(theme::RADIUS_MEDIUM), theme::BG);
                if widgets::icon_button(ui, b, "show-sidebar", Icon::Sidebar, false, "Show sidebar").clicked() {
                    self.session().ui.left_collapsed = false;
                }
            } else if deck {
                let mode = Rect::from_min_size(left_rect.min, Vec2::new(left_rect.width(), 36.0));
                crate::decks::ui::mode_bar(self.session(), ui, mode);
                let body = Rect::from_min_max(Pos2::new(left_rect.left(), mode.bottom()), left_rect.max);
                if self.session().decks.design_mode {
                    left::show(self.session(), ui, body);
                } else {
                    crate::decks::ui::sidebar(self.session(), ui, body);
                }
            } else {
                left::show(self.session(), ui, left_rect);
            }
            if board {
                crate::board::toolbar::show(self.session(), ui, canvas_rect_for_toolbar(full));
                crate::board::bar::show(self.session(), ui, canvas_rect);
                crate::board::section::rename_ui(self.session(), ui, canvas_rect);
                crate::board::canvas::zoom_pill(self.session(), ui, canvas_rect);
                crate::board::session::show(self.session(), ui, canvas_rect);
                crate::collab::ui::board_strip(self.session(), ui, canvas_rect);
            } else {
                right::show(self.session(), ui, right_rect);
                let tb = if deck {
                    canvas_rect
                } else if timeline_rect.is_some() {
                    Rect::from_min_max(full.min, Pos2::new(full.right(), canvas_rect.bottom()))
                } else {
                    canvas_rect_for_toolbar(full)
                };
                toolbar::show(self.session(), ui, tb);
            }
            crate::history::banner::show(self.session(), ui, canvas_rect);
            crate::collab::ui::banner(self.session(), ui, canvas_rect);
            self.context_menu(ui, canvas_rect);
            menus::show(self.session(), &ctx);
            panels::context::show(self.session(), &ctx);
            // the floating panel (or only the inspector's binding picker when closed)
            crate::variables::view::show(self.session(), ui, full);
        }
        crate::home::dialog::show(self.session(), &ctx);
        crate::history::modal::show(self.session(), &ctx);
        crate::dev::compare::show(self.session(), &ctx);
        crate::libraries::show(self.session(), &ctx);
        crate::plugins::ui::show(self.session(), &ctx);
        crate::updater::ui::show(self.session(), &ctx);
        crate::bridge::ui::show(self.session(), &ctx);
        crate::collab::ui::share_popover(self.session(), &ctx);
        crate::collab::ui::requests(self.session(), &ctx);
        crate::collab::ui::join_dialog(self.session(), &ctx);
        crate::unsaved::dialog(self.session(), &ctx);
        self.toast(ui, full);

        self.sync_title(&ctx);
        if self.session().ui.menu == Some(MenuKind::Main) && self.session().ui.menu_anchor.is_none() {
            self.session().ui.menu = None;
        }
    }
}

/// The toolbar is centered on the window (not the canvas).
fn canvas_rect_for_toolbar(full: Rect) -> Rect {
    full
}

#[cfg(test)]
mod tests;
