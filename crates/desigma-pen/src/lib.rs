//! Pencil (`.pen`) file import for desigma.
//!
//! A `.pen` file is plain JSON (comments allowed): an object tree where every object has an
//! `id` and a `type` (`frame`, `rectangle`, `text`, ...), plus document-level `variables`,
//! `themes` and `imports`. [`import`] maps that onto a desigma [`Document`] with one page;
//! [`import_with`] also returns the list of things that could not be mapped.
//! `FIELDS.md` next to this crate's `Cargo.toml` lists every property and what happens to it.
//!
//! * **Layout.** Frames lay their children out like a flexbox (horizontal unless `layout`
//!   says otherwise), so they become auto-layout frames; `fit_content` and `fill_container`
//!   become hug and fill. The sizes the file leaves implicit are computed with the layout
//!   engine after conversion.
//! * **Components.** `reusable` objects become components; `ref` objects become instances
//!   linked to them, with `descendants` overrides applied and recorded as overrides.
//! * **Variables.** `variables` and `themes` become variable collections with modes (see
//!   `vars.rs`); properties that reference a variable are bound to it.
//! * **Images.** `data:` URIs and files below the `.pen`'s folder are embedded when
//!   [`Options::base_dir`] is set.
//!
//! The format is documented as unstable ("breaking changes may be introduced"), so unknown
//! types, properties and newer versions never fail the import: they are skipped and listed
//! in [`Import::warnings`]. Only input that is not a `.pen` document at all is an error.

mod convert;
mod img;
mod instance;
mod jsonc;
mod layout;
mod paint;
mod text;
mod vars;

use desigma_doc::Document;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Newest schema version the importer was written against.
pub const SUPPORTED_VERSION: &str = "2.21";

#[derive(Debug, thiserror::Error)]
pub enum PenError {
    /// The bytes are not JSON (a binary or encrypted file lands here too).
    #[error("not a readable .pen file: {0}")]
    Json(String),
    /// Valid JSON that is not a `.pen` document.
    #[error("not a .pen document: {0}")]
    Format(String),
}

/// Settings for [`import_with`].
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Name of the resulting document (empty when `None`; callers usually use the file name).
    pub name: Option<String>,
    /// Folder of the `.pen` file, for image fills that name files (`./image.jpg`). Without it
    /// only `data:` images load.
    pub base_dir: Option<PathBuf>,
    /// Most objects converted, instances counted with everything they expand to (default
    /// 250 000). What does not fit is left out and reported.
    pub max_objects: Option<usize>,
}

/// A converted document and what was left out.
#[derive(Debug)]
pub struct Import {
    pub document: Document,
    /// Human-readable notes, first seen first, repeated ones folded into "(x3)".
    pub warnings: Vec<String>,
}

/// Collects warnings, folding repeats.
#[derive(Default)]
pub(crate) struct Warnings {
    order: Vec<String>,
    counts: HashMap<String, u32>,
}

impl Warnings {
    pub fn add(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        match self.counts.get_mut(&msg) {
            Some(n) => *n += 1,
            None => {
                self.counts.insert(msg.clone(), 1);
                self.order.push(msg);
            }
        }
    }

    fn finish(self) -> Vec<String> {
        self.order
            .into_iter()
            .map(|m| match self.counts[&m] {
                1 => m,
                n => format!("{m} (x{n})"),
            })
            .collect()
    }
}

/// Imports a `.pen` file. Anything that cannot be mapped is skipped; use [`import_with`] to see what.
pub fn import(bytes: &[u8]) -> Result<Document, PenError> {
    import_with(bytes, &Options::default()).map(|i| i.document)
}

/// Whether `bytes` look like a `.pen` document: a JSON object with a `children` list.
pub fn looks_like_pen(bytes: &[u8]) -> bool {
    parse(bytes).is_ok()
}

fn parse(bytes: &[u8]) -> Result<Value, PenError> {
    let text = std::str::from_utf8(bytes).map_err(|_| PenError::Json("the file is not UTF-8 text (it may be encrypted or in a binary format)".into()))?;
    let text = text.trim_start_matches('\u{feff}');
    let text = jsonc::clean(text);
    if !text.trim_start().starts_with('{') {
        return Err(PenError::Json("the file does not start with a JSON object (it may be encrypted or in a binary format)".into()));
    }
    let value: Value = serde_json::from_str(&text).map_err(|e| PenError::Json(e.to_string()))?;
    if !value.get("children").is_some_and(Value::is_array) {
        return Err(PenError::Format("the top-level object has no \"children\" list".into()));
    }
    Ok(value)
}

/// Imports a `.pen` file with options, returning the warnings too.
pub fn import_with(bytes: &[u8], opts: &Options) -> Result<Import, PenError> {
    let root = parse(bytes)?;
    let mut notes = Vec::new();
    let vars = vars::Vars::parse(&root, &mut notes);
    let mut doc = Document::new(opts.name.clone().unwrap_or_default());
    let page = doc.add_page("Page 1");
    let mut conv = convert::Conv::new(opts, doc, vars);
    for n in notes {
        conv.warn.add(n);
    }
    check_version(&root, &mut conv.warn);
    if root.get("imports").and_then(Value::as_object).is_some_and(|o| !o.is_empty()) {
        conv.warn.add("imported libraries (\"imports\") are not loaded");
    }
    let mut install_notes = Vec::new();
    conv.vars.install(&mut conv.doc, &mut install_notes);
    for n in install_notes {
        conv.warn.add(n);
    }
    let ctx = conv.root_ctx();
    let children = root.get("children").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    for c in children {
        conv.index(c, &ctx);
    }
    let tops: Vec<_> = children.iter().filter_map(|c| conv.node(c, &ctx)).collect();
    for t in &tops {
        if let Some(n) = conv.doc.nodes.get_mut(t) {
            n.parent = Some(page);
        }
    }
    if let Some(p) = conv.doc.nodes.get_mut(&page) {
        p.children = tops.clone();
    }
    conv.finalize(&tops);
    let convert::Conv { doc, warn, .. } = conv;
    Ok(Import { document: doc, warnings: warn.finish() })
}

fn check_version(root: &Value, warn: &mut Warnings) {
    let Some(v) = root.get("version").and_then(Value::as_str) else {
        warn.add("the file has no \"version\"; assuming the current schema");
        return;
    };
    let parts = |s: &str| -> Option<(u32, u32)> {
        let (a, b) = s.split_once('.')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    match (parts(v), parts(SUPPORTED_VERSION)) {
        (Some(file), Some(known)) if file > known => {
            warn.add(format!("the file uses schema {v}, newer than the {SUPPORTED_VERSION} this importer knows; newer features are skipped"))
        }
        (Some((major, _)), Some((known_major, _))) if major < known_major => {
            warn.add(format!("the file uses the old schema {v}; it is read as if it were {SUPPORTED_VERSION}"))
        }
        (None, _) => warn.add(format!("the file's version \"{v}\" is not understood")),
        _ => {}
    }
}
