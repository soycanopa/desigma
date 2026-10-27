//! UI tests for the assets panel thumbnails and its list/grid toggle.

use super::*;
use crate::app::RailItem;

fn child_of(d: &desigma_doc::Document, parent: NodeId, name: &str) -> NodeId {
    *d.get(parent).unwrap().children.iter().find(|c| d.get(**c).unwrap().name == name).unwrap_or_else(|| panic!("no child {name}"))
}

fn assets_open() -> Sim {
    let mut s = Sim::new();
    s.shell.session().ui.rail = Some(RailItem::Assets);
    for _ in 0..4 {
        s.frame(vec![]);
    }
    s
}

#[test]
fn thumbnails_render_lazily_and_refresh_when_the_component_changes() {
    let mut s = assets_open();
    let main = s.by_name("Primary button");
    assert!(s.app().assets.has_thumb(main), "visible component has a texture");
    let rendered = s.app().assets.thumbs_rendered();
    assert!(rendered >= 1);

    // idle frames (long enough for the periodic fingerprint check) re-render nothing
    for _ in 0..30 {
        s.frame(vec![]);
    }
    assert_eq!(s.app().assets.thumbs_rendered(), rendered, "unchanged components are not re-rendered");

    // editing the component's background re-renders its thumbnail only
    let bg = child_of(&s.app().doc, main, "Bg");
    s.shell.session().doc.get_mut(bg).unwrap().fills = vec![Paint::solid(Color::rgb(255, 0, 0))];
    for _ in 0..30 {
        s.frame(vec![]);
    }
    let after = s.app().assets.thumbs_rendered();
    assert!(after > rendered, "edited component re-rendered ({after} vs {rendered})");
    assert!(after <= rendered + 2, "only the edited component re-rendered ({after} vs {rendered})");
}

#[test]
fn grid_toggle_switches_layout_and_keeps_rows_draggable() {
    let mut s = assets_open();
    let list_row = s.rect_of("asset:Primary button");
    assert!(!s.app().assets.grid);
    let grid_btn = s.rect_of("assets-view-grid");
    s.click(grid_btn.center());
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(s.app().assets.grid, "grid view on");
    let cell = s.rect_of("asset:Primary button");
    assert!(cell.width() < list_row.width(), "grid cells are narrower than list rows");
    let main = s.by_name("Primary button");
    assert!(s.app().assets.has_thumb(main), "grid view shows a thumbnail too");

    // dragging a grid cell onto the canvas still places an instance
    let before = desigma_doc::instances_of(&s.app().doc, main).len();
    let target = s.screen(300.0, 820.0);
    s.drag(cell.center(), target);
    assert_eq!(desigma_doc::instances_of(&s.app().doc, main).len(), before + 1);

    let list_btn = s.rect_of("assets-view-list");
    s.click(list_btn.center());
    s.frame(vec![]);
    assert!(!s.app().assets.grid, "back to list view");
}
