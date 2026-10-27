//! UI tests of the custom title bar: window tabs, the "+" button and the home button flow.

use super::*;
use crate::shell::Shell;

/// Writes a one-rectangle document to a temp `.desigma` file and returns the path.
fn tmp_doc(name: &str) -> PathBuf {
    let mut d = desigma_doc::Document::new(name);
    let page = d.add_page("P");
    let id = d.alloc_id();
    d.insert(page, desigma_doc::Node::rectangle(id, desigma_doc::Rect::new(0.0, 0.0, 10.0, 10.0))).unwrap();
    let p = std::env::temp_dir().join(format!("desigma_tabs_{}_{}.desigma", std::process::id(), name));
    crate::io::save_path(&d, &p).unwrap();
    p
}

#[test]
fn opening_files_adds_tabs_and_the_same_file_focuses_its_tab() {
    let mut s = Sim::new();
    let a = tmp_doc("Alpha");
    let b = tmp_doc("Beta");
    s.shell.open_external(a.clone());
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 2, "the demo tab plus Alpha");
    assert_eq!(Shell::tab_label(s.shell.peek()), "Alpha");
    s.shell.open_external(b.clone());
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 3);
    assert_eq!(s.shell.active_tab(), 2);
    // opening the same file again focuses its tab instead of duplicating it
    s.shell.open_external(a.clone());
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 3);
    assert_eq!(s.shell.active_tab(), 1);
    // clicking a tab switches to it
    s.click(s.rect_of("tab:Beta").center());
    assert_eq!(s.shell.active_tab(), 2);
    assert_eq!(Shell::tab_label(s.shell.peek()), "Beta");
    // switching to another file changes the active document
    s.shell.open_external(tmp_doc("Gamma"));
    s.frame(vec![]);
    assert_eq!(s.shell.peek().doc.name, "Gamma", "a different document is active");
    std::fs::remove_file(a).ok();
    std::fs::remove_file(b).ok();
}

#[test]
fn closing_the_active_tab_moves_to_the_neighbour() {
    let mut s = Sim::new();
    s.shell.open_external(tmp_doc("Alpha"));
    s.frame(vec![]);
    // the tab's x button sits inside its right padding
    let t = s.rect_of("tab:Alpha");
    s.mv(t.center());
    s.click(Pos2::new(t.right() - theme::TAB_PAD_X - 2.0, t.center().y));
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 1, "only the demo tab is left");
    assert_eq!(Shell::tab_label(s.shell.peek()), "Untitled");
}

#[test]
fn the_plus_button_opens_a_new_untitled_tab() {
    let mut s = Sim::new();
    s.click(s.rect_of("titlebar-plus").center());
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 2);
    assert_eq!(Shell::tab_label(s.shell.peek()), "Untitled");
    assert!(!s.shell.peek().doc.pages.is_empty(), "the new tab has a page to draw on");
}

#[test]
fn file_menu_new_opens_a_tab_instead_of_replacing_the_document() {
    let mut s = Sim::new();
    let ctx = s.ctx.clone();
    let before = s.app().doc.clone();
    crate::panels::menus::run(s.shell.session(), &ctx, crate::panels::menus::Action::New);
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 2, "File > New file is a new tab");
    assert_eq!(s.shell.active_tab(), 1);
    assert_ne!(s.app().doc.nodes.len(), before.nodes.len(), "the active document is the new one");
    // the original is still open as the first tab
    s.click(s.rect_of("tab#0").left_center());
    assert_eq!(s.app().doc.nodes.len(), before.nodes.len(), "the first tab keeps the original document");
}

#[test]
fn closing_the_last_tab_returns_to_home_and_the_created_file_is_listed() {
    let dir = std::env::temp_dir().join(format!("desigma_tabs_close_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Sim::with(Options { home: true, library: Some(dir.clone()), ..Default::default() });
    s.frame(vec![]);
    s.rect_of("home-card-Welcome");
    // + creates a library file and opens it as the only tab
    s.click(s.rect_of("titlebar-plus").center());
    s.frame(vec![]);
    assert_eq!(s.shell.tab_count(), 1);
    assert!(!s.shell.showing_home());
    assert!(s.app().lib_file.is_some(), "the new file lives in the library");
    // closing it leaves the window open on the home screen (it must not take the window down)
    let t = s.rect_of("tab#0");
    s.mv(t.center());
    s.click(Pos2::new(t.right() - theme::TAB_PAD_X - 2.0, t.center().y));
    s.frame(vec![]);
    assert!(s.shell.showing_home(), "home replaces the last tab");
    assert_eq!(s.shell.tab_count(), 0);
    // the file created by + is listed on the home screen
    s.frame(vec![]);
    s.frame(vec![]);
    s.rect_of("home-card-Untitled");
    let _ = std::fs::remove_dir_all(&dir);
}
