//! UI tests for components and instances.

use super::*;
use crate::app::RailItem;
use desigma_doc::{OverrideField, PropValue};

fn center(app: &App, id: NodeId) -> DV {
    crate::util::rect_center(&crate::util::world_aabb(&app.doc, id))
}

fn ctrl_alt() -> Modifiers {
    Modifiers { alt: true, ctrl: true, command: true, ..Default::default() }
}

fn child_of(d: &desigma_doc::Document, parent: NodeId, name: &str) -> NodeId {
    *d.get(parent).unwrap().children.iter().find(|c| d.get(**c).unwrap().name == name).unwrap_or_else(|| panic!("no child {name}"))
}

#[test]
fn ctrl_alt_k_makes_a_component_and_wraps_shapes() {
    let mut s = Sim::with(Options { select: Some("Hero".into()), ..Default::default() });
    let hero = s.by_name("Hero");
    s.mods = ctrl_alt();
    s.key(Key::K);
    s.mods = Modifiers::NONE;
    let comp = s.app().selection[0];
    assert_ne!(comp, hero);
    assert!(matches!(s.app().doc.get(comp).unwrap().kind, NodeKind::Component(_)));
    assert_eq!(s.app().doc.get(hero).unwrap().parent, Some(comp));
    s.cmd_key(Key::Z);
    assert!(s.app().doc.get(comp).is_none());
    assert_eq!(s.app().doc.get(hero).unwrap().parent, Some(s.by_name("Welcome")));
}

#[test]
fn ctrl_alt_k_converts_a_frame_in_place() {
    let mut s = Sim::with(Options { select: Some("Card".into()), ..Default::default() });
    let card = s.by_name("Card");
    s.mods = ctrl_alt();
    s.key(Key::K);
    s.mods = Modifiers::NONE;
    assert!(matches!(s.app().doc.get(card).unwrap().kind, NodeKind::Component(_)));
    assert_eq!(s.app().selection, vec![card]);
}

#[test]
fn ctrl_alt_b_detaches_an_instance() {
    let mut s = Sim::with(Options { select: Some("Get started".into()), ..Default::default() });
    let inst = s.by_name("Get started");
    assert!(matches!(s.app().doc.get(inst).unwrap().kind, NodeKind::Instance { .. }));
    s.mods = ctrl_alt();
    s.key(Key::B);
    s.mods = Modifiers::NONE;
    let n = s.app().doc.get(inst).unwrap();
    assert!(matches!(n.kind, NodeKind::Frame(_)));
    assert!(n.component.is_empty());
    s.cmd_key(Key::Z);
    assert!(matches!(s.app().doc.get(inst).unwrap().kind, NodeKind::Instance { .. }));
}

#[test]
fn editing_the_main_component_updates_instances_in_one_undo_step() {
    let mut s = Sim::new();
    let main = s.by_name("Primary button");
    let inst = s.by_name("Learn more");
    let main_bg = child_of(&s.app().doc, main, "Bg");
    let inst_bg = child_of(&s.app().doc, inst, "Bg");
    let main_label = child_of(&s.app().doc, main, "Label");
    let inst_label = child_of(&s.app().doc, inst, "Label");

    // The second button overrides its Bg fill, so only the label opacity follows.
    let red = vec![desigma_doc::Paint::solid(Color::rgb(255, 0, 0))];
    let inst_fill = s.app().doc.get(inst_bg).unwrap().fills.clone();
    s.shell.session().edit_nodes(&[main_bg, main_label], |n| {
        if n.name == "Bg" {
            n.fills = red.clone();
        } else {
            n.opacity = 0.5;
        }
    });
    assert_eq!(s.app().doc.get(inst_label).unwrap().opacity, 0.5);
    assert_eq!(s.app().doc.get(inst_bg).unwrap().fills, inst_fill, "overridden fill is kept");
    let first = s.by_name("Get started");
    assert_eq!(s.app().doc.get(child_of(&s.app().doc, first, "Bg")).unwrap().fills, red, "plain instance follows");
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(inst_label).unwrap().opacity, 1.0);
    assert_ne!(s.app().doc.get(child_of(&s.app().doc, first, "Bg")).unwrap().fills, red);
    assert!(s.app().doc.get(main_label).unwrap().opacity == 1.0);
}

#[test]
fn editing_inside_an_instance_records_overrides() {
    let mut s = Sim::new();
    let inst = s.by_name("Get started");
    let icon = child_of(&s.app().doc, inst, "Icon");
    let bg = child_of(&s.app().doc, inst, "Bg");
    s.shell.session().edit_nodes(&[bg], |n| n.opacity = 0.4);
    let n = s.app().doc.get(bg).unwrap();
    assert_eq!(n.component.overrides, [OverrideField::Opacity].into_iter().collect());
    // Moving the instance itself is not an override.
    s.shell.session().edit_nodes(&[inst], |n| n.transform.tx += 10.0);
    assert!(!s.app().doc.get(inst).unwrap().component.overrides.contains(&OverrideField::Transform));
    // Main edits don't clobber the override.
    let main = s.by_name("Primary button");
    let main_bg = child_of(&s.app().doc, main, "Bg");
    s.shell.session().edit_nodes(&[main_bg], |n| n.opacity = 0.9);
    assert_eq!(s.app().doc.get(bg).unwrap().opacity, 0.4);
    let _ = icon;
}

#[test]
fn layers_inside_an_instance_are_hidden_not_deleted() {
    let mut s = Sim::new();
    let inst = s.by_name("Get started");
    let bg = child_of(&s.app().doc, inst, "Bg");
    s.shell.session().selection = vec![bg];
    let n0 = s.count();
    s.key(Key::Delete);
    assert_eq!(s.count(), n0);
    let n = s.app().doc.get(bg).unwrap();
    assert!(!n.visible);
    assert!(n.component.overrides.contains(&OverrideField::Visible));
}

#[test]
fn inspector_toggle_changes_the_boolean_property() {
    let mut s = Sim::with(Options { select: Some("Learn more".into()), ..Default::default() });
    let inst = s.by_name("Learn more");
    let icon = child_of(&s.app().doc, inst, "Icon");
    assert!(!s.app().doc.get(icon).unwrap().visible);
    let r = s.rect_of("toggle:prop:Show icon");
    s.click(r.left_center() + Vec2::new(10.0, 0.0));
    assert!(s.app().doc.get(icon).unwrap().visible, "toggle turns the icon on");
    assert_eq!(s.app().doc.get(inst).unwrap().component.values["Show icon"], PropValue::Bool(true));
    s.cmd_key(Key::Z);
    assert!(!s.app().doc.get(icon).unwrap().visible);
}

#[test]
fn inspector_text_property_relabels_the_instance() {
    let mut s = Sim::with(Options { select: Some("Get started".into()), ..Default::default() });
    let inst = s.by_name("Get started");
    let label = child_of(&s.app().doc, inst, "Label");
    let r = s.rect_of("prop:Label");
    s.click(r.center());
    s.frame(vec![Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    s.type_text("Go");
    s.key(Key::Enter);
    s.frame(vec![]);
    match &s.app().doc.get(label).unwrap().kind {
        NodeKind::Text(t) => assert_eq!(t.content, "Go"),
        k => panic!("{k:?}"),
    }
}

#[test]
fn instance_header_swaps_and_goes_to_main() {
    let mut s = Sim::with(Options { select: Some("Get started".into()), ..Default::default() });
    let inst = s.by_name("Get started");
    let main = s.by_name("Primary button");
    let go = s.rect_of("go-to-main");
    s.click(go.center());
    assert_eq!(s.app().selection, vec![main]);
    assert_eq!(s.app().doc.page_of(main), Some(s.app().page));
    assert_eq!(s.app().doc.get(s.app().page).unwrap().name, "Components");
    let _ = inst;
}

#[test]
fn component_selected_shows_property_definitions_and_adds_one() {
    let mut s = Sim::with(Options { select: Some("Primary button".into()), ..Default::default() });
    let main = s.by_name("Primary button");
    assert_eq!(s.app().doc.get(main).unwrap().component.props.len(), 2);
    let plus = s.rect_of("prop-add");
    s.click(plus.center());
    assert!(s.app().ui.prop_menu.is_some());
    // First entry of the popup is "Boolean".
    let at = plus.left_bottom() + Vec2::new(-150.0, 2.0);
    s.click(at + Vec2::new(40.0, theme::MENU_PADDING + theme::MENU_ITEM_HEIGHT / 2.0));
    let props = &s.app().doc.get(main).unwrap().component.props;
    assert_eq!(props.len(), 3);
    assert_eq!(props[2].kind, desigma_doc::PropKind::Boolean);
    // Delete it again through its minus button.
    let name = props[2].name.clone();
    let del = s.rect_of(&format!("propdel:{name}"));
    s.click(del.center());
    assert_eq!(s.app().doc.get(main).unwrap().component.props.len(), 2);
}

#[test]
fn binding_a_layer_to_a_property_from_its_inspector() {
    let mut s = Sim::new();
    let main = s.by_name("Primary button");
    let bg = child_of(&s.app().doc, main, "Bg");
    s.shell.session().selection = vec![bg];
    s.frame(vec![]);
    s.frame(vec![]);
    let dd = s.rect_of("bind:Visibility");
    s.click(dd.center());
    // Options: None, Show icon.
    let at = dd.left_bottom() + Vec2::new(0.0, 2.0);
    s.click(at + Vec2::new(30.0, theme::MENU_PADDING + theme::MENU_ITEM_HEIGHT * 1.5));
    assert_eq!(s.app().doc.get(bg).unwrap().component.bindings.get(&OverrideField::Visible).map(String::as_str), Some("Show icon"));
}

#[test]
fn assets_panel_lists_components_and_drag_places_an_instance() {
    let mut s = Sim::new();
    s.shell.session().ui.rail = Some(RailItem::Assets);
    s.frame(vec![]);
    s.frame(vec![]);
    let row = s.rect_of("asset:Primary button");
    let main = s.by_name("Primary button");
    let before = desigma_doc::instances_of(&s.app().doc, main).len();
    let target = s.screen(300.0, 820.0);
    s.drag(row.center(), target);
    let insts = desigma_doc::instances_of(&s.app().doc, main);
    assert_eq!(insts.len(), before + 1);
    let new = *insts.iter().find(|i| s.app().selection == vec![**i]).expect("new instance selected");
    let n = s.app().doc.get(new).unwrap();
    assert_eq!(n.parent, Some(s.app().page));
    let c = center(s.app(), new);
    assert!((c.x - 300.0).abs() < 6.0 && (c.y - 820.0).abs() < 6.0, "centred on the drop point: {c:?}");
    assert_eq!(n.children.len(), 3);
    s.cmd_key(Key::Z);
    assert_eq!(desigma_doc::instances_of(&s.app().doc, main).len(), before);
}

#[test]
fn assets_search_filters_rows() {
    let mut s = Sim::new();
    s.shell.session().ui.rail = Some(RailItem::Assets);
    s.shell.session().assets.query = "zzz".into();
    s.frame(vec![]);
    s.frame(vec![]);
    let entries = s.shell.session().local_components();
    assert!(!entries.is_empty());
    assert!(crate::panels::assets::filtered(&entries, "zzz").is_empty());
    assert_eq!(crate::panels::assets::filtered(&entries, "PRIMARY").len(), 1);
    assert_eq!(crate::panels::assets::filtered(&entries, "components").len(), 1, "matches the page name");
}

#[test]
fn alt_drag_duplicate_of_an_instance_stays_an_instance() {
    let mut s = Sim::with(Options { select: Some("Get started".into()), ..Default::default() });
    let inst = s.by_name("Get started");
    let main = s.by_name("Primary button");
    let c = center(s.app(), inst);
    s.mods = Modifiers::ALT;
    let a = s.screen(c.x, c.y);
    s.drag(a, a + Vec2::new(0.0, 90.0));
    s.mods = Modifiers::NONE;
    let insts = desigma_doc::instances_of(&s.app().doc, main);
    assert_eq!(insts.len(), 3, "the copy is a third instance");
    let copy = *insts.iter().find(|i| **i != inst && s.app().selection == vec![**i]).expect("copy selected");
    let n = s.app().doc.get(copy).unwrap();
    assert_eq!(n.component.source, Some(main));
    assert!(n.children.iter().all(|k| s.app().doc.get(*k).unwrap().component.source.is_some()));
    // The copy keeps following the main component.
    let main_bg = child_of(&s.app().doc, main, "Bg");
    s.shell.session().edit_nodes(&[main_bg], |n| n.opacity = 0.3);
    assert_eq!(s.app().doc.get(child_of(&s.app().doc, copy, "Bg")).unwrap().opacity, 0.3);
}

#[test]
fn context_menu_detaches_and_combines() {
    let mut s = Sim::new();
    let inst = s.by_name("Get started");
    let c = center(s.app(), inst);
    let p = s.screen(c.x, c.y - 10.0);
    s.right_click(p);
    assert_eq!(s.app().selection, vec![inst]);
    let at = s.app().ui.ctx_menu.expect("menu opens at the pointer");
    let labels = crate::panels::context::labels(s.app());
    let idx = labels.iter().position(|l| l == "Detach instance").unwrap();
    assert!(labels.contains(&"Combine as variants".to_string()) && labels.contains(&"Create component".to_string()));
    // Items are 24px tall with a 17px separator block after "Add auto layout" (index 4).
    let sep = if idx > 4 { crate::widgets::menu::SEP_H } else { 0.0 };
    let y = at.y + theme::MENU_PADDING + idx as f32 * theme::MENU_ITEM_HEIGHT + sep + theme::MENU_ITEM_HEIGHT / 2.0;
    s.click(Pos2::new(at.x + 60.0, y));
    assert!(s.app().ui.ctx_menu.is_none());
    assert!(matches!(s.app().doc.get(inst).unwrap().kind, NodeKind::Frame(_)));
}

#[test]
fn combine_selection_builds_a_component_set() {
    let mut s = Sim::new();
    let main = s.by_name("Primary button");
    let first = s.app().doc.get(main).unwrap().clone();
    s.shell.session().selection = vec![main];
    s.shell.session().duplicate_selection();
    let copy = s.shell.session().selection[0];
    assert_ne!(copy, main);
    s.shell.session().selection = vec![main, copy];
    s.shell.session().combine_selection_as_variants();
    let set = s.app().selection[0];
    let n = s.app().doc.get(set).unwrap();
    assert!(matches!(n.kind, NodeKind::ComponentSet(_)));
    assert_eq!(n.children.len(), 2);
    assert_eq!(s.app().doc.get(main).unwrap().parent, Some(set));
    assert!(s.app().doc.get(main).unwrap().name.contains('='));
    let _ = first;
    s.cmd_key(Key::Z);
    assert!(s.app().doc.get(set).is_none());
}
