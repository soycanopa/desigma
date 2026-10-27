//! The editor's link to the library: which library file is open, saving,
//! debounced autosave and thumbnails.

use super::{thumb, Home, Section};
use crate::app::App;
use desigma_doc::{Document, DocumentKind};
use desigma_library::Library;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Seconds without a document change before a library file is autosaved.
pub const AUTOSAVE_IDLE: f64 = 1.5;
/// Seconds of calm after which a revision change is compared with the saved document.
const PROBE_DELAY: f64 = 0.3;

/// The library file currently open in the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct LibFile {
    pub id: String,
    pub project_id: String,
    pub project: String,
    pub name: String,
}

#[derive(Default)]
pub struct Autosave {
    /// Last canvas revision seen (a change bumps it).
    pub seen_rev: u64,
    /// When the document last changed, while a save is pending.
    pub changed_at: Option<f64>,
    /// The pending change was checked against `saved_hash` and really differs (view-only
    /// changes such as zooming bump the revision too, and must not show "Saving").
    pub confirmed: bool,
    /// Hash of the document as last written to (or read from) the library.
    pub saved_hash: u64,
    /// Number of autosaves performed (tests).
    pub saves: u32,
}

impl Autosave {
    /// An edit is waiting for the idle delay before it is saved.
    pub fn pending(&self) -> bool {
        self.changed_at.is_some() && self.confirmed
    }
}

pub(crate) fn doc_hash(doc: &Document) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.to_json().unwrap_or_default().hash(&mut h);
    h.finish()
}

impl App {
    /// Opens (or creates) the library at `root` and seeds it on first run.
    pub fn attach_library(&mut self, root: PathBuf) {
        // Shared libraries use the same root as the projects library.
        self.libs.set_root(root.clone());
        match Library::open(&root) {
            Ok(lib) => {
                match lib.seed_first_run(crate::demo::demo_document) {
                    // The sample file gets its thumbnail right away so the home screen is not blank.
                    Ok(Some(entry)) => {
                        if let Ok(doc) = lib.load_file(&entry.id) {
                            thumb::refresh(&lib, &entry.id, &doc);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.show_toast(format!("Could not create the sample project: {e}"), true),
                }
                self.lib = Some(lib);
            }
            Err(e) => self.show_toast(format!("Cannot open the library at {}: {e}", root.display()), true),
        }
    }

    /// Shows the home screen ("Back to files"), saving pending edits first.
    pub fn go_home(&mut self) {
        if !self.leave_ok(|app| app.go_home()) {
            return;
        }
        let Some(lib) = self.lib.clone() else {
            self.show_toast("The projects library is unavailable", true);
            return;
        };
        crate::decks::ui::flush_notes(self);
        self.save_close_version();
        self.stop_plugin();
        self.flush_autosave();
        self.end_collab();
        self.canvas.reset_interaction();
        let mut home = Home::new();
        home.refresh(&lib);
        self.home = Some(home);
        self.ui.menu = None;
        self.ui.ctx_menu = None;
    }

    /// Opens a library file in the editor.
    pub fn open_library_file(&mut self, id: &str) {
        let again = id.to_string();
        if !self.leave_ok(move |app| app.open_library_file(&again)) {
            return;
        }
        let Some(lib) = self.lib.clone() else { return };
        let loaded = lib.file(id).and_then(|e| lib.load_file(id).map(|d| (e, d)));
        let (entry, mut doc) = match loaded {
            Ok(x) => x,
            Err(e) => {
                self.show_toast(format!("Cannot open the file: {e}"), true);
                return;
            }
        };
        let project = lib.project(&entry.project).map(|p| p.name).unwrap_or_default();
        doc.name = entry.name.clone();
        let needs_thumb = lib.thumbnail_path(&entry).is_none();
        self.replace_document(doc, None);
        self.lib_file = Some(LibFile { id: entry.id.clone(), project_id: entry.project.clone(), project, name: entry.name.clone() });
        self.autosave = Autosave { seen_rev: self.canvas.tiles.rev(), saved_hash: doc_hash(&self.doc), ..Default::default() };
        self.history_open_file();
        let _ = lib.mark_opened(id);
        self.home = None;
        self.ui.menu = None;
        if needs_thumb {
            thumb::refresh_in_background(lib, entry.id, self.export_doc().into_owned());
        }
    }

    /// Creates an empty design file in `project` (or the first / a new project) and opens it.
    pub fn new_library_file(&mut self, target: Option<(String, Option<String>)>) {
        self.new_library_file_of(target, DocumentKind::Design);
    }

    /// Creates an empty board in `project` (or the first / a new project) and opens it.
    pub fn new_library_board(&mut self, target: Option<(String, Option<String>)>) {
        self.new_library_file_of(target, DocumentKind::Board);
    }

    pub fn new_library_deck(&mut self, target: Option<(String, Option<String>)>) {
        self.new_library_file_of(target, DocumentKind::Deck);
    }

    pub fn new_deck_here(&mut self) {
        self.open_tab = Some(crate::app::TabOpen::NewDeck);
    }

    fn new_library_file_of(&mut self, target: Option<(String, Option<String>)>, kind: DocumentKind) {
        let again = (target.clone(), kind);
        if !self.leave_ok(move |app| app.new_library_file_of(again.0, again.1)) {
            return;
        }
        let Some(lib) = self.lib.clone() else {
            if kind == DocumentKind::Board {
                self.new_board_document();
            } else if kind == DocumentKind::Deck {
                self.new_deck_document();
            } else {
                self.new_document();
            }
            return;
        };
        let result = (|| {
            let (project, folder) = match target {
                Some(t) => t,
                None => match lib.list_projects().first() {
                    Some(p) => (p.id.clone(), None),
                    None => (lib.create_project("My project")?.id, None),
                },
            };
            let doc = match kind {
                DocumentKind::Board => crate::board::create::new_document("Untitled"),
                DocumentKind::Deck => crate::decks::new_document("Untitled deck"),
                DocumentKind::Design => {
                    let mut doc = Document::new("Untitled");
                    doc.add_page("Page 1");
                    doc
                }
            };
            lib.create_file(&project, folder.as_deref(), "Untitled", &doc)
        })();
        match result {
            Ok(e) => self.open_library_file(&e.id),
            Err(e) => self.show_toast(format!("Cannot create the file: {e}"), true),
        }
    }

    /// File → New file: a new tab next to the open library file (the Shell resolves it).
    pub fn new_file_here(&mut self) {
        self.open_tab = Some(crate::app::TabOpen::NewFile);
    }

    /// File → New board: a new tab next to the open library file.
    pub fn new_board_here(&mut self) {
        self.open_tab = Some(crate::app::TabOpen::NewBoard);
    }

    /// Writes the open library file (no toast); also refreshes its thumbnail.
    pub fn save_library_file(&mut self) -> Result<(), String> {
        crate::decks::ui::flush_notes(self);
        let (Some(lib), Some(lf)) = (self.lib.clone(), self.lib_file.clone()) else { return Err("This file is not in the library".into()) };
        if self.history.viewing.is_some() {
            // an old version is on screen: the file on disk must keep the live document
            return Err("Go back to the current version before saving".into());
        }
        lib.save_file(&lf.id, &self.doc).map_err(|e| format!("Cannot save: {e}"))?;
        self.autosave.saved_hash = doc_hash(&self.doc);
        self.autosave.changed_at = None;
        thumb::refresh_in_background(lib, lf.id, self.export_doc().into_owned());
        Ok(())
    }

    /// Ctrl+S on a library file.
    pub fn save_library_file_with_toast(&mut self) {
        match self.save_library_file() {
            Ok(()) => {
                let project = self.lib_file.as_ref().map(|f| f.project.clone()).unwrap_or_default();
                self.show_toast(format!("Saved to {project}"), false);
            }
            Err(e) => self.show_toast(e, true),
        }
    }

    /// Saves pending edits right away (before leaving the editor or the app).
    pub fn flush_autosave(&mut self) {
        if self.history.viewing.is_some() {
            return;
        }
        if self.lib_file.is_some() && doc_hash(&self.doc) != self.autosave.saved_hash {
            if let Err(e) = self.save_library_file() {
                self.show_toast(e, true);
            }
        }
        self.autosave.changed_at = None;
    }

    /// Called every frame: debounced autosave of the open library file.
    pub fn autosave_tick(&mut self, ctx: &egui::Context) {
        if self.lib_file.is_none() || self.home.is_some() || self.history.viewing.is_some() {
            self.autosave.changed_at = None;
            return;
        }
        let rev = self.canvas.tiles.rev();
        if rev != self.autosave.seen_rev {
            self.autosave.seen_rev = rev;
            self.autosave.changed_at = Some(self.now);
            self.autosave.confirmed = false;
        }
        let Some(t) = self.autosave.changed_at else { return };
        let busy = ctx.input(|i| i.pointer.any_down()) || self.canvas.text.is_some();
        if busy {
            self.autosave.changed_at = Some(self.now);
            self.autosave.confirmed = false;
        } else if !self.autosave.confirmed && self.now - t >= PROBE_DELAY {
            // Cheap-ish check once the revision settled: did the document really change?
            if doc_hash(&self.doc) == self.autosave.saved_hash {
                self.autosave.changed_at = None;
                return;
            }
            self.autosave.confirmed = true;
        } else if self.autosave.confirmed && self.now - t >= AUTOSAVE_IDLE {
            self.autosave.changed_at = None;
            if doc_hash(&self.doc) != self.autosave.saved_hash {
                match self.save_library_file() {
                    Ok(()) => self.autosave.saves += 1,
                    Err(e) => self.show_toast(e, true),
                }
            }
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// Renames the open library file (header double-click).
    pub fn rename_library_file(&mut self, name: &str) {
        let (Some(lib), Some(lf)) = (self.lib.clone(), self.lib_file.clone()) else { return };
        let name = name.trim();
        if name.is_empty() || name == lf.name {
            return;
        }
        match lib.rename_file(&lf.id, name) {
            Ok(()) => {
                if let Some(f) = self.lib_file.as_mut() {
                    f.name = name.to_string();
                }
                self.doc.name = name.to_string();
            }
            Err(e) => self.show_toast(format!("Cannot rename: {e}"), true),
        }
    }

    /// Stores the open (external or unsaved) document as a new library file and keeps editing it there.
    pub fn save_to_library_as(&mut self, project: &str, folder: Option<&str>, name: &str) {
        crate::decks::ui::flush_notes(self);
        let Some(lib) = self.lib.clone() else { return };
        let mut doc = self.doc.clone();
        doc.name = name.trim().to_string();
        match lib.create_file(project, folder, name, &doc) {
            Ok(e) => {
                self.doc.name = e.name.clone();
                let project_name = lib.project(project).map(|p| p.name).unwrap_or_default();
                self.lib_file = Some(LibFile { id: e.id.clone(), project_id: project.to_string(), project: project_name.clone(), name: e.name });
                self.autosave = Autosave { seen_rev: self.canvas.tiles.rev(), saved_hash: doc_hash(&self.doc), ..Default::default() };
                self.history_open_file();
                self.file_path = None;
                thumb::refresh_in_background(lib, e.id, self.export_doc().into_owned());
                self.show_toast(format!("Saved to {project_name}"), false);
            }
            Err(e) => self.show_toast(format!("Cannot save to the library: {e}"), true),
        }
    }

    /// Imports files into a project; returns how many succeeded.
    pub fn import_into_library(&mut self, paths: &[PathBuf], project: &str, folder: Option<&str>) -> usize {
        let Some(lib) = self.lib.clone() else { return 0 };
        let mut ok = 0;
        let mut last_err = None;
        for p in paths {
            match lib.import_file(p, project, folder) {
                Ok(e) => {
                    ok += 1;
                    if let Ok(doc) = lib.load_file(&e.id) {
                        thumb::refresh_in_background(lib.clone(), e.id, doc);
                    }
                }
                Err(e) => last_err = Some(format!("{}: {e}", p.file_name().and_then(|n| n.to_str()).unwrap_or("file"))),
            }
        }
        match (ok, last_err) {
            (0, Some(e)) => self.show_toast(format!("Import failed. {e}"), true),
            (n, Some(e)) => self.show_toast(format!("Imported {n}, but {e}"), true),
            (n, None) if n > 0 => self.show_toast(format!("Imported {n} file{}", if n == 1 { "" } else { "s" }), false),
            _ => {}
        }
        if let Some(h) = self.home.as_mut() {
            h.dirty = true;
        }
        ok
    }

    /// The section a "New design file" button creates into.
    pub fn home_target(&self) -> Option<(String, Option<String>)> {
        match &self.home.as_ref()?.section {
            Section::Project(p, f) => Some((p.clone(), f.clone())),
            _ => None,
        }
    }
}
