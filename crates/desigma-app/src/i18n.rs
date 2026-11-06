//! UI translations.
//!
//! The English text in the code is the key: `tr("Layers")` returns the translation for the
//! current language, or the English text when there is none, so untranslated strings still
//! read correctly. Catalogs are tab-separated files under `assets/i18n/`, one per language,
//! embedded at build time. Translation happens where text is drawn (menus, section titles,
//! tabs, tooltips), so code that matches on labels keeps working with English strings.
//!
//! Adding a language: add a variant to [`Lang`], a catalog file, and its arm in
//! [`Lang::catalog`]. Adding strings: add `English<TAB>translation` lines to the catalogs.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    ZhCn,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::ZhCn];

    /// BCP 47 code stored in the settings.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhCn => "zh-CN",
        }
    }

    /// The language's own name, as shown in the Language menu (never translated).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::ZhCn => "简体中文",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let c = code.trim().replace('_', "-").to_ascii_lowercase();
        if c == "en" || c.starts_with("en-") {
            Some(Lang::En)
        } else if c == "zh" || c.starts_with("zh-cn") || c.starts_with("zh-hans") || c.starts_with("zh-sg") {
            Some(Lang::ZhCn)
        } else {
            None
        }
    }

    fn catalog(self) -> Option<&'static HashMap<&'static str, &'static str>> {
        static ZH_CN: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
        match self {
            Lang::En => None,
            Lang::ZhCn => Some(ZH_CN.get_or_init(|| parse(include_str!("../assets/i18n/zh-CN.tsv")))),
        }
    }
}

/// The language from the environment (`LC_ALL`, `LC_MESSAGES`, `LANG`), for a first launch
/// without a saved choice. `C`/`POSIX` and unknown languages give `None`.
pub fn from_env() -> Option<Lang> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty())
        .and_then(|v| Lang::from_code(v.split('.').next().unwrap_or("")))
}

thread_local! {
    // UI text is laid out on the UI thread; a thread-local keeps parallel tests independent.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

pub fn set(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// `text` in the current language (unchanged when there is no translation).
pub fn tr(text: &str) -> &str {
    tr_in(current(), text)
}

/// `text` in `lang`.
pub fn tr_in(lang: Lang, text: &str) -> &str {
    lang.catalog().and_then(|c| c.get(text).copied()).unwrap_or(text)
}

/// Like [`tr`], for text that may have been built with `format!`: an exact entry wins, else
/// the first catalog template whose `{}` slots match (`Edit failed: {}` matches
/// `Edit failed: no layer 7` and becomes `编辑失败：no layer 7`). The filled-in parts stay as
/// they were (file names, error details).
pub fn tr_text(text: &str) -> std::borrow::Cow<'_, str> {
    let lang = current();
    let exact = tr_in(lang, text);
    if !std::ptr::eq(exact, text) {
        return exact.into();
    }
    // The most specific template wins ("Opened {}. Not carried over: {}" over "Opened {}").
    lang.catalog()
        .and_then(|c| {
            c.iter()
                .filter(|(k, _)| k.contains("{}"))
                .filter_map(|(k, v)| Some((k.len() - 2 * k.matches("{}").count(), fill(k, v, text)?)))
                .max_by_key(|(literal, _)| *literal)
                .map(|(_, out)| out)
        })
        .map_or(text.into(), Into::into)
}

/// `template` translated, then its `{}` slots filled with `args` in order.
pub fn trf(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut pieces = tr(template).split("{}").peekable();
    while let Some(piece) = pieces.next() {
        out.push_str(piece);
        if pieces.peek().is_some() {
            out.push_str(args.next().copied().unwrap_or(""));
        }
    }
    out
}

/// `translation` with its `{}` slots filled from `text`, when `text` matches `template`.
fn fill(template: &str, translation: &str, text: &str) -> Option<String> {
    let parts: Vec<&str> = template.split("{}").collect();
    let mut rest = text.strip_prefix(parts[0])?;
    let mut args = Vec::new();
    for (i, part) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        let at = if last {
            rest.len().checked_sub(part.len()).filter(|&a| rest[a..] == **part)?
        } else if part.is_empty() {
            return None; // two adjacent slots are ambiguous
        } else {
            rest.find(part)?
        };
        if at == 0 {
            return None; // empty slot: let a more specific entry handle it
        }
        args.push(&rest[..at]);
        rest = &rest[at + part.len()..];
    }
    let mut out = String::new();
    let mut args = args.into_iter();
    let mut pieces = translation.split("{}").peekable();
    while let Some(piece) = pieces.next() {
        out.push_str(piece);
        if pieces.peek().is_some() {
            out.push_str(args.next().unwrap_or(""));
        }
    }
    Some(out)
}

/// `English<TAB>translation` lines; `#` comments and blank lines are skipped.
fn parse(src: &'static str) -> HashMap<&'static str, &'static str> {
    src.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(en, tr)| (en, tr.trim()))
        .filter(|(_, tr)| !tr.is_empty())
        .collect()
}

/// Settings key holding the chosen language code.
pub const SETTING: &str = "language";

/// The language to start in: the saved choice, else (on an interactive launch) the system
/// language when we have a catalog for it, else English.
pub fn startup(library: Option<&std::path::Path>, use_env: bool) -> Lang {
    library
        .and_then(|root| crate::comments::settings::load_string(root, SETTING))
        .and_then(|code| Lang::from_code(&code))
        .or_else(|| if use_env { from_env() } else { None })
        .unwrap_or_default()
}

impl crate::app::App {
    /// Preferences -> Language: switches the UI now and remembers the choice.
    pub fn set_language(&mut self, lang: Lang) {
        set(lang);
        if let Some(lib) = &self.lib {
            if let Err(e) = crate::comments::settings::save_string(lib.root(), SETTING, lang.code()) {
                self.show_toast(e, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZH: &str = include_str!("../assets/i18n/zh-CN.tsv");

    /// Sources that show fixed toast messages (`show_toast("...")`).
    pub(super) const SOURCES: &[(&str, &str)] = &[
        ("board/bar.rs", include_str!("board/bar.rs")),
        ("board/session.rs", include_str!("board/session.rs")),
        ("bridge/mod.rs", include_str!("bridge/mod.rs")),
        ("canvas/builder.rs", include_str!("canvas/builder.rs")),
        ("canvas/gpu.rs", include_str!("canvas/gpu.rs")),
        ("canvas/interact.rs", include_str!("canvas/interact.rs")),
        ("collab/mod.rs", include_str!("collab/mod.rs")),
        ("collab/reconnect.rs", include_str!("collab/reconnect.rs")),
        ("collab/ui.rs", include_str!("collab/ui.rs")),
        ("comments/mod.rs", include_str!("comments/mod.rs")),
        ("comp_ops.rs", include_str!("comp_ops.rs")),
        ("dev/compare.rs", include_str!("dev/compare.rs")),
        ("dev/mod.rs", include_str!("dev/mod.rs")),
        ("dev/sections.rs", include_str!("dev/sections.rs")),
        ("history/mod.rs", include_str!("history/mod.rs")),
        ("history/session.rs", include_str!("history/session.rs")),
        ("home/dialog.rs", include_str!("home/dialog.rs")),
        ("home/mod.rs", include_str!("home/mod.rs")),
        ("home/session.rs", include_str!("home/session.rs")),
        ("libraries/mod.rs", include_str!("libraries/mod.rs")),
        ("motion/inspector.rs", include_str!("motion/inspector.rs")),
        ("motion/playback/export.rs", include_str!("motion/playback/export.rs")),
        ("motion/timeline.rs", include_str!("motion/timeline.rs")),
        ("ops.rs", include_str!("ops.rs")),
        ("panels/export.rs", include_str!("panels/export.rs")),
        ("panels/inspect.rs", include_str!("panels/inspect.rs")),
        ("panels/menus.rs", include_str!("panels/menus.rs")),
        ("panels/prototype.rs", include_str!("panels/prototype.rs")),
        ("plugins/mod.rs", include_str!("plugins/mod.rs")),
        ("updater/mod.rs", include_str!("updater/mod.rs")),
        ("vecops.rs", include_str!("vecops.rs")),
    ];

    #[test]
    fn translates_and_falls_back_to_english() {
        assert_eq!(tr_in(Lang::ZhCn, "Layers"), "图层");
        assert_eq!(tr_in(Lang::ZhCn, "Not in the catalog"), "Not in the catalog");
        assert_eq!(tr_in(Lang::En, "Layers"), "Layers");
        // the current language is per thread and English by default
        assert_eq!(tr("Layers"), "Layers");
        set(Lang::ZhCn);
        assert_eq!(tr("Layers"), "图层");
        set(Lang::En);
    }

    #[test]
    fn templates_carry_the_filled_in_parts() {
        assert_eq!(fill("Edit failed: {}", "编辑失败：{}", "Edit failed: no layer 7").as_deref(), Some("编辑失败：no layer 7"));
        assert_eq!(fill("Moved {} to the trash", "已将 {} 移到废纸篓", "Moved Logo to the trash").as_deref(), Some("已将 Logo 移到废纸篓"));
        assert_eq!(fill("Trash ({})", "废纸篓（{}）", "Trash (3)").as_deref(), Some("废纸篓（3）"));
        assert_eq!(fill("Saved {}", "已保存 {}", "Saved"), None);
        assert_eq!(fill("Saved {}", "已保存 {}", "Exported x"), None);
        set(Lang::ZhCn);
        assert_eq!(tr_text("Trash emptied"), "已清空废纸篓");
        assert_eq!(tr_text("Edit failed: boom"), "编辑失败：boom");
        assert_eq!(tr_text("Something new"), "Something new");
        assert_eq!(tr_text("Opened Poster. Not carried over: a script"), "已打开 Poster。未能导入：a script", "the more specific template wins");
        assert_eq!(tr_text("Opened Poster"), "已打开 Poster");
        assert_eq!(tr_text("5 minutes ago"), "5 分钟前");
        assert_eq!(trf("Edited {}", &[&tr_text("just now")]), "编辑于刚刚");
        set(Lang::En);
        assert_eq!(tr_text("Edit failed: boom"), "Edit failed: boom");
        assert_eq!(trf("Edited {}", &["just now"]), "Edited just now");
    }

    /// Every fixed toast message has a zh-CN entry.
    #[test]
    fn toasts_are_translated() {
        let mut missing = vec![];
        for (path, src) in SOURCES {
            for (pos, _) in src.match_indices("show_toast(\"") {
                let rest = &src[pos + "show_toast(\"".len()..];
                let msg = &rest[..rest.find('"').unwrap()];
                if tr_in(Lang::ZhCn, msg) == msg {
                    missing.push(format!("{path}: {msg}"));
                }
            }
        }
        assert!(missing.is_empty(), "add these to assets/i18n/zh-CN.tsv:\n{}", missing.join("\n"));
    }

    #[test]
    fn language_codes() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("zh_CN.UTF-8".split('.').next().unwrap()), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("zh-Hans"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("en_US"), Some(Lang::En));
        assert_eq!(Lang::from_code("C"), None);
        assert_eq!(Lang::from_code("de"), None);
    }

    #[test]
    fn saved_choice_wins_over_the_environment() {
        let dir = std::env::temp_dir().join(format!("desigma-i18n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(startup(Some(&dir), false), Lang::En, "nothing saved, environment ignored");
        crate::comments::settings::save_string(&dir, SETTING, "zh-CN").unwrap();
        assert_eq!(startup(Some(&dir), false), Lang::ZhCn);
        assert_eq!(startup(None, false), Lang::En);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_lines_are_well_formed_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for (i, line) in ZH.lines().enumerate() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (en, zh) = line.split_once('\t').unwrap_or_else(|| panic!("line {}: no TAB: {line:?}", i + 1));
            assert!(!en.is_empty() && !zh.trim().is_empty() && !zh.contains('\t'), "line {}: {line:?}", i + 1);
            assert_eq!(en, en.trim(), "line {}: stray spaces around the English key", i + 1);
            assert!(seen.insert(en), "line {}: duplicate key {en:?}", i + 1);
        }
    }

    /// The first translated surface: every main-menu, context-menu and tool label has a
    /// zh-CN entry, so new menu items are not left in English unnoticed.
    #[test]
    fn menus_and_tools_are_translated() {
        let menus = include_str!("panels/menus.rs");
        let mut missing = vec![];
        for call in ["leaf(\"", "submenu(\"", "stub_sub(\"", "(\"Select all with same "] {
            for (pos, _) in menus.match_indices(call) {
                let start = pos + call.len() - if call.starts_with("(\"Select") { "Select all with same ".len() } else { 0 };
                let rest = &menus[start..];
                let label = &rest[..rest.find('"').unwrap()];
                if !label.is_empty() && tr_in(Lang::ZhCn, label) == label {
                    missing.push(label.to_string());
                }
            }
        }
        for t in crate::canvas::tools::GROUPS.iter().flat_map(|g| g.iter()) {
            if tr_in(Lang::ZhCn, t.label()) == t.label() {
                missing.push(t.label().to_string());
            }
        }
        for t in crate::canvas::tools::GROUPS.iter().flat_map(|g| g.iter()) {
            let l = crate::panels::menus::tool_menu_label(*t);
            if tr_in(Lang::ZhCn, l) == l {
                missing.push(l.to_string());
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "add these to assets/i18n/zh-CN.tsv: {missing:?}");
    }
}
