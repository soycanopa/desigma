//! Background loading of the open-source fonts a document uses.
//!
//! After a document is opened, every family/weight/style it references is
//! resolved through `desigma-fonts` (bundled, disk cache, then Fontsource) on a
//! worker thread. Results are registered with the renderer on the UI thread.

use desigma_doc::Document;
use desigma_fonts::{ensure_fonts_with, FontCache, FontReport, FontRequest};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver};

fn installed_exactly(family: &str) -> bool {
    desigma_render::resolve_font(family, 400, false).is_some_and(|f| f.family.eq_ignore_ascii_case(family))
}

#[derive(Default)]
pub struct FontLoader {
    pending: Option<Receiver<FontReport>>,
}

/// Outcome of a finished load, for the toast.
pub struct FontsLoaded {
    pub loaded: usize,
    pub failed: Vec<String>,
    /// The same fonts as structured requests.
    pub failed_requests: Vec<FontRequest>,
}

impl FontLoader {
    /// Starts resolving the document's fonts; a previous load is abandoned.
    pub fn load_for(&mut self, doc: &Document) {
        // Fonts the text names, plus the Noto fallbacks of the complex scripts it uses (Arabic, Hebrew, Devanagari, CJK).
        let mut requests = desigma_fonts::requests_for(doc);
        requests.extend(desigma_fonts::script_requests_for(doc));
        if requests.is_empty() {
            self.pending = None;
            return;
        }
        // The renderer's font database is thread-local, so check installed
        // families here on the UI thread, not in the worker.
        let installed: HashSet<String> = requests
            .iter()
            .map(|r| r.family.clone())
            .filter(|family| {
                // A script fallback is not needed when the system has a font that covers the
                // script (e.g. Microsoft YaHei for Chinese), so offline users see no failure.
                let alternatives = desigma_fonts::Script::of_family(family).map(|s| s.system_alternatives()).unwrap_or_default();
                std::iter::once(family.as_str()).chain(alternatives.iter().copied()).any(installed_exactly)
            })
            .collect();
        let doc = doc.clone_without_images();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let report = ensure_fonts_with(&doc, &FontCache::new(), |family| installed.contains(family));
            let _ = tx.send(report);
        });
        self.pending = Some(rx);
    }

    /// Registers finished fonts with the renderer. Returns `Some` once per load.
    pub fn poll(&mut self) -> Option<FontsLoaded> {
        let report = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        let loaded = report.loaded.len();
        for f in report.loaded {
            desigma_export::register_font(f.data.clone());
            desigma_render::register_font(f.data);
        }
        let failed = report.failed.iter().map(|(req, err)| format!("{} {}: {err}", req.family, req.weight)).collect();
        let failed_requests = report.failed.into_iter().map(|(req, _)| req).collect();
        Some(FontsLoaded { loaded, failed, failed_requests })
    }

    /// Pretends a load finished with `report` (tests).
    #[cfg(test)]
    pub fn inject(&mut self, report: FontReport) {
        let (tx, rx) = channel();
        let _ = tx.send(report);
        self.pending = Some(rx);
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }
}
