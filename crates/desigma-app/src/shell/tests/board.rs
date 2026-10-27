//! UI tests of board (whiteboard) mode: the toolbar, stickies and typing into them, Tab to the next
//! sticky, connectors dragged out of magnet dots and following their nodes, the contextual toolbar,
//! alt-drag, double-click editing, and undo for all of it.

use super::*;
use crate::board::connect;
use crate::canvas::tools::Tool;
use desigma_doc::{connector_route, ConnectorCap, ConnectorLine, Magnet, Node, NodeKind, Rect as DRect, ShapeKind, StickyProps, STICKY_SIZE};

fn board_sim() -> Sim {
    Sim::with(Options { fixture: Some("board-empty".into()), ..Default::default() })
}

fn board_demo_sim() -> Sim {
    Sim::with(Options { fixture: Some("board".into()), ..Default::default() })
}

fn stickies(s: &Sim) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = s.app().doc.nodes.values().filter(|n| matches!(n.kind, NodeKind::Sticky(_))).map(|n| n.id).collect();
    v.sort();
    v
}

fn connectors(s: &Sim) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = s.app().doc.nodes.values().filter(|n| matches!(n.kind, NodeKind::Connector(_))).map(|n| n.id).collect();
    v.sort();
    v
}

fn sticky_text(s: &Sim, id: NodeId) -> String {
    match &s.app().doc.get(id).unwrap().kind {
        NodeKind::Sticky(p) => p.text.content.clone(),
        k => panic!("{k:?}"),
    }
}

/// Adds a shape with text on the page at page position `(x, y)`.
fn add_shape(s: &mut Sim, name: &str, x: f32, y: f32) -> NodeId {
    let id = s.shell.session().doc.alloc_id();
    let mut n = Node::shape_with_text(id, DRect::new(x, y, 160.0, 100.0), ShapeKind::RoundedRect, "box");
    n.name = name.into();
    let page = s.app().page;
    let id = s.shell.session().doc.insert(page, n).unwrap();
    s.shell.session().changed();
    s.frame(vec![]);
    id
}

fn node_center(s: &Sim, id: NodeId) -> Pos2 {
    let r = crate::util::world_aabb(&s.app().doc, id);
    s.screen(r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn dot(s: &Sim, id: NodeId, m: Magnet) -> Pos2 {
    let (_, p) = connect::dot_positions(s.app(), id).into_iter().find(|(k, _)| *k == m).unwrap();
    s.screen(p.x, p.y)
}

#[test]
fn board_files_get_the_board_ui() {
    let s = board_sim();
    assert!(s.app().is_board());
    assert_eq!(s.app().doc.kind, desigma_doc::DocumentKind::Board);
    // no right panel: the canvas runs to the window edge
    assert!((s.app().canvas_rect.right() - 1600.0).abs() < 1.0, "canvas right {}", s.app().canvas_rect.right());
    // the board toolbar, with its own tools
    for t in [Tool::Move, Tool::Hand, Tool::Marker, Tool::Sticky, Tool::BoardShape, Tool::Connector, Tool::Text, Tool::Section, Tool::Stamp] {
        let _ = s.rect_of(&format!("board-tool-{t:?}"));
    }
    // a design file keeps its panels and has no board toolbar
    let d = Sim::new();
    assert!(!d.app().is_board());
    assert!(d.app().canvas_rect.right() < 1600.0 - 200.0);
    assert!(d.ctx.data(|dt| dt.get_temp::<Rect>(egui::Id::new(("test-rect", "board-tool-Sticky".to_string())))).is_none());
}

#[test]
fn toolbar_buttons_pick_tools_and_shortcuts_follow_the_board_keys() {
    let mut s = board_sim();
    let r = s.rect_of("board-tool-Sticky");
    s.click(r.center());
    assert_eq!(s.app().tool, Tool::Sticky);
    s.key(Key::Escape);
    assert_eq!(s.app().tool, Tool::Move);
    for (k, shift, t) in [
        (Key::S, false, Tool::Sticky),
        (Key::X, false, Tool::Connector),
        (Key::E, false, Tool::Stamp),
        (Key::R, false, Tool::BoardShape),
        (Key::T, false, Tool::Text),
        (Key::P, false, Tool::Marker),
        (Key::P, true, Tool::Highlighter),
        (Key::S, true, Tool::Section),
        (Key::V, false, Tool::Move),
    ] {
        s.mods = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
        s.key(k);
        s.mods = Modifiers::NONE;
        assert_eq!(s.app().tool, t, "{k:?}");
    }
}

#[test]
fn sticky_tool_places_a_note_and_typing_goes_into_it() {
    let mut s = board_sim();
    s.key(Key::S);
    assert_eq!(s.app().tool, Tool::Sticky);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    let notes = stickies(&s);
    assert_eq!(notes.len(), 1);
    let n = s.app().doc.get(notes[0]).unwrap();
    // centred on the click, default size, colour from the palette's first entry
    assert_eq!(n.size, desigma_doc::Vec2::new(STICKY_SIZE, STICKY_SIZE));
    assert!((n.transform.tx + n.size.x / 2.0 - 300.0).abs() < 4.0 && (n.transform.ty + n.size.y / 2.0 - 300.0).abs() < 4.0, "{:?}", n.transform);
    assert!(matches!(&n.kind, NodeKind::Sticky(StickyProps { color, .. }) if *color == desigma_doc::STICKY_YELLOW));
    // the tool is spent and typing starts right away
    assert_eq!(s.app().tool, Tool::Move);
    assert!(s.app().canvas.text.is_some(), "typing starts in the new sticky");
    s.type_text("Hello board");
    s.key(Key::Escape);
    assert!(s.app().canvas.text.is_none());
    assert_eq!(sticky_text(&s, notes[0]), "Hello board");
    assert_eq!(s.app().selection, notes);
}

#[test]
fn sticky_creation_and_text_undo() {
    let mut s = board_sim();
    s.key(Key::S);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    s.type_text("abc");
    s.key(Key::Escape);
    let id = stickies(&s)[0];
    assert_eq!(sticky_text(&s, id), "abc");
    // creation and first text are separate steps: undo drops the text, then the note
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    assert!(stickies(&s).is_empty() || sticky_text(&s, stickies(&s)[0]).is_empty());
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    assert!(stickies(&s).is_empty(), "everything is undone");
    // and redo brings it all back
    for _ in 0..4 {
        s.mods = Modifiers::COMMAND | Modifiers::SHIFT;
        s.key(Key::Z);
        s.mods = Modifiers::NONE;
    }
    assert_eq!(stickies(&s).len(), 1);
}

#[test]
fn tab_makes_the_next_sticky_to_the_right() {
    let mut s = board_sim();
    s.key(Key::S);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    s.type_text("first");
    s.key(Key::Tab);
    let notes = stickies(&s);
    assert_eq!(notes.len(), 2, "Tab adds a sticky");
    let (a, b) = (s.app().doc.get(notes[0]).unwrap(), s.app().doc.get(notes[1]).unwrap());
    assert_eq!(sticky_text(&s, notes[0]), "first", "the first note kept its text");
    assert!(
        (b.transform.tx - (a.transform.tx + a.size.x + crate::theme::BOARD_STICKY_GAP)).abs() < 1.0,
        "to the right: {} vs {}",
        b.transform.tx,
        a.transform.tx
    );
    assert!((b.transform.ty - a.transform.ty).abs() < 1.0, "same row");
    assert_eq!(a.size, b.size);
    // typing continues in the new note, and Tab again keeps going
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(notes[1]));
    s.type_text("second");
    s.key(Key::Tab);
    s.type_text("third");
    s.key(Key::Escape);
    let notes = stickies(&s);
    assert_eq!(notes.len(), 3);
    assert_eq!(notes.iter().map(|n| sticky_text(&s, *n)).collect::<Vec<_>>(), vec!["first", "second", "third"]);
    let tx: Vec<f32> = notes.iter().map(|n| s.app().doc.get(*n).unwrap().transform.tx).collect();
    assert!(tx[0] < tx[1] && tx[1] < tx[2]);
    // the same colour, picked up from the first note
    let colors: Vec<_> = notes.iter().map(|n| if let NodeKind::Sticky(p) = &s.app().doc.get(*n).unwrap().kind { p.color } else { unreachable!() }).collect();
    assert!(colors.iter().all(|c| *c == colors[0]));
}

#[test]
fn tab_works_on_a_selected_note_too_and_undoes() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 1");
    s.shell.session().selection = vec![a];
    let before = stickies(&s).len();
    s.key(Key::Tab);
    assert_eq!(stickies(&s).len(), before + 1);
    s.key(Key::Escape);
    s.cmd_key(Key::Z);
    assert_eq!(stickies(&s).len(), before, "one undo removes the new note");
}

#[test]
fn double_click_edits_a_sticky() {
    let mut s = board_demo_sim();
    let id = s.by_name("Sticky 3");
    let c = node_center(&s, id);
    s.click(c);
    s.click(c);
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(id), "double-click opens the text");
    s.key(Key::End);
    s.mods = Modifiers::COMMAND;
    s.key(Key::A);
    s.mods = Modifiers::NONE;
    s.type_text("Replaced");
    s.key(Key::Escape);
    assert_eq!(sticky_text(&s, id), "Replaced");
    s.cmd_key(Key::Z);
    assert_eq!(sticky_text(&s, id), "Text shrinks to fit the note");
}

#[test]
fn shapes_are_created_with_text_and_the_shape_picker_changes_what_is_placed() {
    let mut s = board_sim();
    // pick a diamond from the shape popover
    let chev = s.rect_of("board-chevron-Shape");
    s.click(chev.center());
    let cell = s.rect_of("board-shape-Diamond");
    s.click(cell.center());
    assert_eq!(s.app().tool, Tool::BoardShape);
    assert_eq!(s.app().board.shape, ShapeKind::Diamond);
    let at = s.screen(400.0, 300.0);
    s.click(at);
    let id = s.app().selection[0];
    assert!(matches!(&s.app().doc.get(id).unwrap().kind, NodeKind::ShapeWithText(p) if p.shape == ShapeKind::Diamond));
    s.type_text("Decide");
    s.key(Key::Escape);
    assert!(matches!(&s.app().doc.get(id).unwrap().kind, NodeKind::ShapeWithText(p) if p.text.content == "Decide"));
}

#[test]
fn stamp_tool_places_stamps() {
    let mut s = board_sim();
    let chev = s.rect_of("board-chevron-Stamp");
    s.click(chev.center());
    let cell = s.rect_of("board-stamp-heart");
    s.click(cell.center());
    assert_eq!(s.app().tool, Tool::Stamp);
    let at = s.screen(500.0, 400.0);
    s.click(at);
    let id = s.app().selection[0];
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.kind, NodeKind::Stamp { kind: "heart".into() });
    assert_eq!(n.size, desigma_doc::Vec2::new(64.0, 64.0));
    assert_eq!(s.app().tool, Tool::Move);
}

#[test]
fn hovering_a_shape_shows_four_magnet_dots() {
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    assert_eq!(s.app().board.dots, None);
    let c = node_center(&s, a);
    s.mv(c);
    assert_eq!(s.app().board.dots, Some(a));
    let dots = connect::dot_positions(s.app(), a);
    assert_eq!(dots.len(), 4);
    let sides: Vec<Magnet> = dots.iter().map(|d| d.0).collect();
    assert_eq!(sides, vec![Magnet::Top, Magnet::Right, Magnet::Bottom, Magnet::Left]);
    // the dots sit just outside the sides, so moving onto one keeps them
    let right = dot(&s, a, Magnet::Right);
    s.mv(right);
    assert_eq!(s.app().board.dots, Some(a));
    // far away they go
    let away = s.screen(900.0, 600.0);
    s.mv(away);
    assert_eq!(s.app().board.dots, None);
}

#[test]
fn dragging_from_a_dot_to_another_shape_attaches_a_connector_and_moving_reroutes_it() {
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    let b = add_shape(&mut s, "B", 600.0, 200.0);
    s.mv(node_center(&s, a));
    let from = dot(&s, a, Magnet::Right);
    // drop on the middle of B
    s.drag(from, node_center(&s, b));
    let conns = connectors(&s);
    assert_eq!(conns.len(), 1, "one connector was made");
    let c = conns[0];
    let NodeKind::Connector(p) = &s.app().doc.get(c).unwrap().kind else { unreachable!() };
    assert_eq!(p.start.node, Some(a));
    assert_eq!(p.start.magnet, Magnet::Right);
    assert_eq!(p.end.node, Some(b), "attached to the node it was dropped on");
    assert_eq!(s.app().selection, vec![c]);
    let r = connector_route(&s.app().doc, c).unwrap();
    assert_eq!(r.start(), desigma_doc::Vec2::new(360.0, 250.0), "right side of A");
    assert_eq!(r.end(), desigma_doc::Vec2::new(600.0, 250.0), "left side of B");

    // move B down by dragging it: the connector re-routes live and after the drop
    s.key(Key::Escape);
    let bc = node_center(&s, b);
    s.drag(bc, bc + Vec2::new(0.0, 160.0 * s.app().view.zoom));
    let nb = s.app().doc.get(b).unwrap();
    assert!(nb.transform.ty > 300.0, "B moved: {}", nb.transform.ty);
    let r2 = connector_route(&s.app().doc, c).unwrap();
    assert!((r2.end().y - (nb.transform.ty + nb.size.y / 2.0)).abs() < 1.0 || r2.end().y > 300.0, "the end follows B: {:?}", r2.end());
    assert_ne!(r2.pts, r.pts, "the route changed");
    let conn = s.app().doc.get(c).unwrap();
    // the connector node covers its new route
    let bb = r2.bounds();
    assert!(
        conn.transform.tx <= bb.x + 0.5
            && conn.transform.ty <= bb.y + 0.5
            && conn.transform.tx + conn.size.x >= bb.x + bb.w - 0.5
            && conn.transform.ty + conn.size.y >= bb.y + bb.h - 0.5
    );

    // undo restores B and the route in one step
    s.cmd_key(Key::Z);
    let r3 = connector_route(&s.app().doc, c).unwrap();
    assert_eq!(r3.pts, r.pts, "undo puts the connector back with its node");
    assert_eq!(s.app().doc.get(b).unwrap().transform.ty, 200.0);
    // undoing again removes the connector
    s.cmd_key(Key::Z);
    assert!(connectors(&s).is_empty());
}

#[test]
fn the_connector_tool_connects_two_notes_and_the_connector_survives_a_move_of_either_end() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 4");
    let b = s.by_name("Release");
    let n0 = connectors(&s).len();
    s.key(Key::X);
    assert_eq!(s.app().tool, Tool::Connector);
    s.drag(node_center(&s, a), node_center(&s, b));
    let conns = connectors(&s);
    assert_eq!(conns.len(), n0 + 1);
    let c = *conns.last().unwrap();
    let NodeKind::Connector(p) = &s.app().doc.get(c).unwrap().kind else { unreachable!() };
    assert_eq!((p.start.node, p.end.node), (Some(a), Some(b)));
    assert_eq!(s.app().tool, Tool::Move, "the tool is spent");
    // move the start note: the line follows it
    let before = connector_route(&s.app().doc, c).unwrap();
    s.key(Key::Escape);
    let ac = node_center(&s, a);
    s.drag(ac, ac + Vec2::new(30.0, 90.0));
    let after = connector_route(&s.app().doc, c).unwrap();
    assert_ne!(before.start(), after.start());
}

#[test]
fn a_click_on_a_dot_without_dragging_makes_no_connector() {
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    s.mv(node_center(&s, a));
    let d = dot(&s, a, Magnet::Bottom);
    s.click(d);
    assert!(connectors(&s).is_empty());
    assert!(matches!(s.app().canvas.inter, crate::canvas::Interaction::None));
}

#[test]
fn dropping_a_connector_on_empty_canvas_leaves_a_free_end() {
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    s.mv(node_center(&s, a));
    let from = dot(&s, a, Magnet::Right);
    s.drag(from, s.screen(700.0, 500.0));
    let c = connectors(&s)[0];
    let NodeKind::Connector(p) = &s.app().doc.get(c).unwrap().kind else { unreachable!() };
    assert_eq!(p.start.node, Some(a));
    assert_eq!(p.end.node, None);
    assert!((p.end.point.x - 700.0).abs() < 3.0 && (p.end.point.y - 500.0).abs() < 3.0, "{:?}", p.end.point);
}

#[test]
fn deleting_a_node_leaves_its_connector_with_a_free_end_and_undo_reattaches() {
    let mut s = board_demo_sim();
    let rel = s.by_name("Release");
    let c = s.by_name("Connector 2");
    let end_before = connector_route(&s.app().doc, c).unwrap().end();
    s.shell.session().selection = vec![rel];
    s.key(Key::Delete);
    assert!(s.app().doc.get(rel).is_none());
    let end_after = connector_route(&s.app().doc, c).unwrap().end();
    assert_eq!(end_after, end_before, "the free end stays where it was");
    s.cmd_key(Key::Z);
    assert!(s.app().doc.get(rel).is_some());
    let NodeKind::Connector(p) = &s.app().doc.get(c).unwrap().kind else { unreachable!() };
    assert_eq!(p.end.node, Some(rel), "undo reconnects it");
}

#[test]
fn alt_drag_duplicates_a_sticky_and_keeps_the_original() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 1");
    let tx = s.app().doc.get(a).unwrap().transform;
    let n0 = stickies(&s).len();
    s.mods = Modifiers::ALT;
    let c = node_center(&s, a);
    s.drag(c, c + Vec2::new(0.0, 300.0 * s.app().view.zoom));
    s.mods = Modifiers::NONE;
    assert_eq!(stickies(&s).len(), n0 + 1);
    assert_eq!(s.app().doc.get(a).unwrap().transform, tx, "the original stays");
    let dup = s.app().selection[0];
    assert_ne!(dup, a);
    let d = s.app().doc.get(dup).unwrap();
    assert!(d.transform.ty > tx.ty + 100.0, "the copy moved");
    assert_eq!(sticky_text(&s, dup), sticky_text(&s, a));
    s.cmd_key(Key::Z);
    assert_eq!(stickies(&s).len(), n0, "one undo takes the copy back");
}

#[test]
fn contextual_bar_recolors_notes_and_undoes() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 1");
    // no selection: no bar
    assert!(s.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("test-rect", "board-bar".to_string())))).is_none() || s.app().selection.is_empty());
    s.shell.session().selection = vec![a];
    s.frame(vec![]);
    let bar = s.rect_of("board-bar");
    let sel = crate::util::world_aabb(&s.app().doc, a);
    let top = s.screen(sel.x, sel.y).y;
    assert!(bar.bottom() < top, "the bar floats above the selection");
    let sw = s.rect_of("board-swatch-5");
    s.click(sw.center());
    let color = match &s.app().doc.get(a).unwrap().kind {
        NodeKind::Sticky(p) => crate::board::rgb_of(p.color),
        _ => unreachable!(),
    };
    assert_eq!(color, crate::theme::BOARD_PALETTE[5]);
    assert_eq!(crate::board::rgb_of(s.app().board.color), crate::theme::BOARD_PALETTE[5], "new notes start in the picked colour");
    s.cmd_key(Key::Z);
    let color = match &s.app().doc.get(a).unwrap().kind {
        NodeKind::Sticky(p) => crate::board::rgb_of(p.color),
        _ => unreachable!(),
    };
    assert_eq!(color, crate::theme::BOARD_PALETTE[0]);
}

#[test]
fn contextual_bar_text_size_and_alignment() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 1");
    s.shell.session().selection = vec![a];
    s.frame(vec![]);
    let minus = s.rect_of("board-size-minus");
    s.click(minus.center());
    let size = |s: &Sim| match &s.app().doc.get(a).unwrap().kind {
        NodeKind::Sticky(p) => p.text.style.font_size,
        _ => unreachable!(),
    };
    assert_eq!(size(&s), 24.0, "32 steps down to 24");
    let plus = s.rect_of("board-size-plus");
    s.click(plus.center());
    s.click(plus.center());
    assert_eq!(size(&s), 40.0);
    let center = s.rect_of("board-align-center");
    s.click(center.center());
    assert!(matches!(&s.app().doc.get(a).unwrap().kind, NodeKind::Sticky(p) if p.text.style.align_h == desigma_doc::TextAlignH::Center));
}

#[test]
fn contextual_bar_for_connectors_has_line_style_and_caps() {
    let mut s = board_demo_sim();
    let c = s.by_name("Connector 1");
    s.shell.session().selection = vec![c];
    s.frame(vec![]);
    let curved = s.rect_of("board-line-Curved");
    s.click(curved.center());
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.line == ConnectorLine::Curved));
    // the end cap popover
    let cap = s.rect_of("board-cap-end");
    s.click(cap.center());
    let diamond = s.rect_of("board-bar-cap-Diamond");
    s.click(diamond.center());
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.end_cap == ConnectorCap::Diamond));
    let start = s.rect_of("board-cap-start");
    s.click(start.center());
    let circle = s.rect_of("board-bar-cap-Circle");
    s.click(circle.center());
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.start_cap == ConnectorCap::Circle));
    // ink colour
    let sw = s.rect_of("board-swatch-2");
    s.click(sw.center());
    assert_eq!(crate::board::bar::current_color(s.app().doc.get(c).unwrap()), Some(crate::theme::BOARD_INK[2]));
    // each was its own undo step
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.line == ConnectorLine::Elbowed && p.end_cap == ConnectorCap::Triangle));
}

#[test]
fn contextual_bar_for_shapes_swaps_the_shape() {
    let mut s = board_demo_sim();
    let d = s.by_name("Decision");
    s.shell.session().selection = vec![d];
    s.frame(vec![]);
    let btn = s.rect_of("board-shape-btn");
    s.click(btn.center());
    let star = s.rect_of("board-bar-shape-Star");
    s.click(star.center());
    assert!(matches!(&s.app().doc.get(d).unwrap().kind, NodeKind::ShapeWithText(p) if p.shape == ShapeKind::Star));
    s.cmd_key(Key::Z);
    assert!(matches!(&s.app().doc.get(d).unwrap().kind, NodeKind::ShapeWithText(p) if p.shape == ShapeKind::Diamond));
}

#[test]
fn marker_draws_thick_round_strokes_and_stays_active() {
    let mut s = board_sim();
    s.key(Key::P);
    assert_eq!(s.app().tool, Tool::Marker);
    let (a, b) = (s.screen(300.0, 300.0), s.screen(500.0, 380.0));
    s.drag(a, b);
    assert_eq!(s.app().tool, Tool::Marker, "the marker stays selected for the next stroke");
    let page = s.app().page;
    let kids = s.app().doc.get(page).unwrap().children.clone();
    assert_eq!(kids.len(), 1);
    let n = s.app().doc.get(kids[0]).unwrap();
    assert!(matches!(n.kind, NodeKind::Vector { .. }));
    assert_eq!(n.stroke.weight, crate::theme::BOARD_MARKER_WIDTH);
    assert_eq!(n.stroke.cap, desigma_doc::StrokeCap::Round);
    s.cmd_key(Key::Z);
    assert!(s.app().doc.get(page).unwrap().children.is_empty());
}

#[test]
fn highlighter_is_wide_and_translucent() {
    let mut s = board_sim();
    s.mods = Modifiers::SHIFT;
    s.key(Key::P);
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().tool, Tool::Highlighter);
    let (a, b) = (s.screen(300.0, 300.0), s.screen(500.0, 300.0));
    s.drag(a, b);
    let page = s.app().page;
    let id = s.app().doc.get(page).unwrap().children[0];
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.stroke.weight, crate::theme::BOARD_HIGHLIGHTER_WIDTH);
    assert!(matches!(&n.stroke.paints[0].kind, desigma_doc::PaintKind::Solid(c) if c.a < 0.6));
}

#[test]
fn section_tool_makes_a_section_that_adopts_notes_dropped_into_it() {
    let mut s = board_sim();
    s.mods = Modifiers::SHIFT;
    s.key(Key::S);
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().tool, Tool::Section);
    s.mods = Modifiers::COMMAND;
    s.drag(s.screen(100.0, 100.0), s.screen(800.0, 600.0));
    s.mods = Modifiers::NONE;
    let sec = s.app().selection[0];
    assert!(matches!(s.app().doc.get(sec).unwrap().kind, NodeKind::BoardSection(_)));
    // a sticky placed inside becomes its child
    s.key(Key::S);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    let note = stickies(&s)[0];
    assert_eq!(s.app().doc.get(note).unwrap().parent, Some(sec));
}

#[test]
fn text_tool_in_a_board_makes_larger_text() {
    let mut s = board_sim();
    s.key(Key::T);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    s.type_text("Title");
    s.key(Key::Escape);
    let id = s.app().selection[0];
    match &s.app().doc.get(id).unwrap().kind {
        NodeKind::Text(t) => {
            assert_eq!(t.content, "Title");
            assert_eq!(t.style.font_size, 24.0);
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn connector_labels_can_be_typed_by_double_clicking_the_line() {
    let mut s = board_demo_sim();
    let c = s.by_name("Connector 1");
    let route = connector_route(&s.app().doc, c).unwrap();
    let mid = route.midpoint();
    let at = s.screen(mid.x, mid.y);
    s.click(at);
    s.click(at);
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(c), "double-click edits the label");
    s.type_text("maybe");
    s.key(Key::Escape);
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.label.as_ref().is_some_and(|l| l.content == "maybe")));
    // emptying a label removes it (a pause first, so these are not counted as more clicks of one multi-click)
    s.t += 1.0;
    s.frame(vec![]);
    s.click(at);
    s.click(at);
    s.mods = Modifiers::COMMAND;
    s.key(Key::A);
    s.mods = Modifiers::NONE;
    s.key(Key::Backspace);
    s.key(Key::Escape);
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.label.is_none()));
}

#[test]
fn a_typed_sticky_text_shrinks_instead_of_overflowing_the_note() {
    let mut s = board_sim();
    s.key(Key::S);
    let at = s.screen(300.0, 300.0);
    s.click(at);
    let id = stickies(&s)[0];
    s.type_text("Word word word word word word word word word word word word word word word word word word word word word word word word");
    let doc = &s.app().doc;
    let f = crate::board::create::text_frame(doc, doc.get(id).unwrap()).unwrap();
    assert!(f.props.style.font_size < desigma_doc::STICKY_FONT_MAX, "the drawn size shrank to {}", f.props.style.font_size);
    let h = desigma_render::text_geometry(&f.props, f.size).height;
    assert!(h <= f.size.y + 0.5, "the text fits its box: {h} <= {}", f.size.y);
    // while the stored size stays at the maximum, so deleting text grows it back
    assert!(matches!(&doc.get(id).unwrap().kind, NodeKind::Sticky(p) if p.text.style.font_size == desigma_doc::STICKY_FONT_MAX));
    s.key(Key::Escape);
}

#[test]
fn board_items_survive_a_copy_paste_with_their_connectors() {
    let mut s = board_demo_sim();
    let a = s.by_name("Decision");
    let b = s.by_name("Release");
    let c = s.by_name("Connector 2");
    s.shell.session().selection = vec![a, b, c];
    let n0 = s.count();
    s.shell.session().duplicate_selection();
    assert_eq!(s.count(), n0 + 3);
    let copies = s.app().selection.clone();
    assert_eq!(copies.len(), 3);
    // the copied connector joins the copied shapes, not the originals
    let cc = copies.iter().copied().find(|i| matches!(s.app().doc.get(*i).unwrap().kind, NodeKind::Connector(_))).unwrap();
    let NodeKind::Connector(p) = &s.app().doc.get(cc).unwrap().kind else { unreachable!() };
    assert!(p.start.node.is_some_and(|n| copies.contains(&n)) && p.end.node.is_some_and(|n| copies.contains(&n)), "{p:?}");
}

#[test]
fn author_line_toggle_on_the_bar() {
    let mut s = board_demo_sim();
    let a = s.by_name("Sticky 1");
    s.shell.session().selection = vec![a];
    s.frame(vec![]);
    let show = |s: &Sim| matches!(&s.app().doc.get(a).unwrap().kind, NodeKind::Sticky(p) if p.show_author);
    assert!(!show(&s));
    let btn = s.rect_of("board-author");
    s.click(btn.center());
    assert!(show(&s));
    assert!(matches!(&s.app().doc.get(a).unwrap().kind, NodeKind::Sticky(p) if p.author.as_deref().is_some_and(|n| !n.is_empty())));
    s.cmd_key(Key::Z);
    assert!(!show(&s));
}

#[test]
fn undo_and_redo_cover_every_board_edit_in_one_history() {
    // creating, typing, moving, connecting, restyling: undo them all, in order, back to the empty board
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    let b = add_shape(&mut s, "B", 600.0, 200.0);
    // shapes were added straight to the document, so history starts empty
    s.mv(node_center(&s, a));
    let from = dot(&s, a, Magnet::Right);
    s.drag(from, node_center(&s, b));
    let c = connectors(&s)[0];
    s.key(Key::Escape);
    let bc = node_center(&s, b);
    s.drag(bc, bc + Vec2::new(0.0, 100.0 * s.app().view.zoom));
    s.shell.session().selection = vec![c];
    s.frame(vec![]);
    let curved = s.rect_of("board-line-Curved");
    s.click(curved.center());
    // restyle, move, connect: three undo steps
    s.cmd_key(Key::Z);
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.line == ConnectorLine::Elbowed));
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(b).unwrap().transform.ty, 200.0);
    s.cmd_key(Key::Z);
    assert!(connectors(&s).is_empty());
    assert!(!s.app().hist.can_undo());
    // and forward again
    for _ in 0..3 {
        s.mods = Modifiers::COMMAND | Modifiers::SHIFT;
        s.key(Key::Z);
        s.mods = Modifiers::NONE;
    }
    assert_eq!(connectors(&s).len(), 1);
    assert!(matches!(&s.app().doc.get(c).unwrap().kind, NodeKind::Connector(p) if p.line == ConnectorLine::Curved));
    assert!(s.app().doc.get(b).unwrap().transform.ty > 200.0);
}

#[test]
fn connectors_follow_nudges_resizes_and_moves_of_a_whole_section() {
    let mut s = board_demo_sim();
    let rel = s.by_name("Release");
    let c = s.by_name("Connector 2");
    let end0 = connector_route(&s.app().doc, c).unwrap().end();
    // arrow keys nudge the selection, one undo step each
    s.shell.session().selection = vec![rel];
    s.mods = Modifiers::SHIFT;
    s.key(Key::ArrowDown);
    s.mods = Modifiers::NONE;
    let end1 = connector_route(&s.app().doc, c).unwrap().end();
    assert!((end1.y - end0.y - 10.0).abs() < 0.01, "nudged 10 down: {end0:?} -> {end1:?}");
    s.cmd_key(Key::Z);
    assert_eq!(connector_route(&s.app().doc, c).unwrap().end(), end0);

    // resizing from the bottom right handle moves the middle of the left side
    let (corners, _) = crate::canvas::interact::selection_box(s.app()).unwrap();
    let br = s.app().view.to_screen(s.app().canvas_rect.min, corners[2]);
    let h0 = s.app().doc.get(rel).unwrap().size.y;
    s.drag(br, br + Vec2::new(0.0, 60.0 * s.app().view.zoom));
    assert!(s.app().doc.get(rel).unwrap().size.y > h0 + 20.0);
    let end2 = connector_route(&s.app().doc, c).unwrap().end();
    assert!(end2.y > end0.y + 10.0, "the end follows the taller node's middle: {end0:?} -> {end2:?}");
    s.cmd_key(Key::Z);
    assert_eq!(connector_route(&s.app().doc, c).unwrap().end(), end0);

    // moving the whole section carries its notes, and every connector attached to them follows
    let sec = s.by_name("Went well");
    let c1 = s.by_name("Connector 1");
    let start0 = connector_route(&s.app().doc, c1).unwrap().start();
    s.shell.session().selection = vec![sec];
    s.shell.session().nudge(0.0, 40.0);
    s.frame(vec![]);
    let start1 = connector_route(&s.app().doc, c1).unwrap().start();
    assert!((start1.y - start0.y - 40.0).abs() < 0.01, "{start0:?} -> {start1:?}");
    // all three connectors were re-synced to the moved nodes, in the same undo step
    s.cmd_key(Key::Z);
    assert_eq!(connector_route(&s.app().doc, c1).unwrap().start(), start0);
    assert_eq!(connector_route(&s.app().doc, c).unwrap().end(), end0);
}

#[test]
fn moving_a_free_connector_carries_its_loose_ends() {
    let mut s = board_sim();
    let a = add_shape(&mut s, "A", 200.0, 200.0);
    s.mv(node_center(&s, a));
    let from = dot(&s, a, Magnet::Right);
    s.drag(from, s.screen(700.0, 500.0));
    let c = connectors(&s)[0];
    s.key(Key::Escape);
    // a connector with one loose end moves by its loose end; the attached end stays on its node
    let r0 = connector_route(&s.app().doc, c).unwrap();
    s.shell.session().selection = vec![c];
    s.shell.session().nudge(0.0, 30.0);
    let r1 = connector_route(&s.app().doc, c).unwrap();
    assert_eq!(r1.start(), r0.start(), "the end on the shape stays put");
    assert!((r1.end().y - r0.end().y - 30.0).abs() < 0.01, "{:?} -> {:?}", r0.end(), r1.end());
    s.cmd_key(Key::Z);
    assert_eq!(connector_route(&s.app().doc, c).unwrap().end(), r0.end());
}

// ------------------------------------------------------------------ code blocks

fn code_blocks(s: &Sim) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = s.app().doc.nodes.values().filter(|n| matches!(n.kind, NodeKind::CodeBlock(_))).map(|n| n.id).collect();
    v.sort();
    v
}

fn code_of(s: &Sim, id: NodeId) -> desigma_doc::CodeBlockProps {
    match &s.app().doc.get(id).unwrap().kind {
        NodeKind::CodeBlock(c) => c.clone(),
        k => panic!("{k:?}"),
    }
}

fn add_code(s: &mut Sim, x: f32, y: f32, lang: desigma_doc::CodeLang, code: &str) -> NodeId {
    let id = s.shell.session().doc.alloc_id();
    let n = Node::code_block(id, DRect::new(x, y, 400.0, 200.0), lang, code);
    let page = s.app().page;
    let id = s.shell.session().doc.insert(page, n).unwrap();
    s.shell.session().changed();
    s.frame(vec![]);
    id
}

fn add_sticky(s: &mut Sim, x: f32, y: f32, text: &str) -> NodeId {
    let id = s.shell.session().doc.alloc_id();
    let n = Node::sticky(id, DRect::new(x, y, STICKY_SIZE, STICKY_SIZE), text);
    let page = s.app().page;
    let id = s.shell.session().doc.insert(page, n).unwrap();
    s.shell.session().changed();
    s.frame(vec![]);
    id
}

#[test]
fn code_tool_places_a_block_and_typing_goes_into_it() {
    let mut s = board_sim();
    let tool = s.rect_of("board-tool-Code");
    s.click(tool.center());
    assert_eq!(s.app().tool, Tool::Code);
    let at = s.screen(500.0, 400.0);
    s.click(at);
    let blocks = code_blocks(&s);
    assert_eq!(blocks.len(), 1);
    let n = s.app().doc.get(blocks[0]).unwrap();
    assert_eq!(n.size, desigma_doc::Vec2::new(desigma_doc::CODE_BLOCK_W, desigma_doc::CODE_BLOCK_H));
    assert_eq!(s.app().tool, Tool::Move);
    assert!(s.app().canvas.text.is_some(), "typing starts in the new block");
    s.type_text("fn main() {}");
    s.key(Key::Tab);
    s.type_text("x");
    s.key(Key::Escape);
    let c = code_of(&s, blocks[0]);
    assert_eq!(c.text.content, "fn main() {}    x", "Tab indents instead of starting a sticky");
    assert!(stickies(&s).is_empty());
    assert_eq!(c.lang, desigma_doc::CodeLang::Plain);
    assert!(c.line_numbers);
}

#[test]
fn code_bar_picks_language_theme_line_numbers_and_copies() {
    let mut s = board_sim();
    let id = add_code(&mut s, 200.0, 200.0, desigma_doc::CodeLang::Plain, "let a = 1;");
    s.shell.session().selection = vec![id];
    s.frame(vec![]);
    // language picker
    let lang = s.rect_of("board-lang-btn");
    s.click(lang.center());
    let rust = s.rect_of("board-bar-lang-Rust");
    s.click(rust.center());
    assert_eq!(code_of(&s, id).lang, desigma_doc::CodeLang::Rust);
    assert_eq!(s.app().board.lang, desigma_doc::CodeLang::Rust, "the next block starts in the language last picked");
    // theme
    assert_eq!(code_of(&s, id).theme, desigma_doc::CodeTheme::Dark);
    let theme = s.rect_of("board-code-theme");
    s.click(theme.center());
    assert_eq!(code_of(&s, id).theme, desigma_doc::CodeTheme::Light);
    // line numbers
    let lines = s.rect_of("board-code-lines");
    s.click(lines.center());
    assert!(!code_of(&s, id).line_numbers);
    // copy
    assert!(s.app().board.copied.is_none());
    let copy = s.rect_of("board-code-copy");
    s.click(copy.center());
    assert_eq!(s.app().board.copied.as_deref(), Some("let a = 1;"));
    // every change is one undo step
    s.cmd_key(Key::Z);
    assert!(code_of(&s, id).line_numbers);
    s.cmd_key(Key::Z);
    assert_eq!(code_of(&s, id).theme, desigma_doc::CodeTheme::Dark);
    s.cmd_key(Key::Z);
    assert_eq!(code_of(&s, id).lang, desigma_doc::CodeLang::Plain);
}

#[test]
fn code_bar_has_text_size_but_no_alignment() {
    let mut s = board_sim();
    let id = add_code(&mut s, 200.0, 200.0, desigma_doc::CodeLang::Json, "{}");
    s.shell.session().selection = vec![id];
    s.frame(vec![]);
    let plus = s.rect_of("board-size-plus");
    s.click(plus.center());
    assert!(code_of(&s, id).text.style.font_size > desigma_doc::CODE_FONT_SIZE);
    let aligned = s.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("test-rect", "board-align-center".to_string()))));
    assert!(aligned.is_none(), "code has no alignment buttons");
}

// ------------------------------------------------------------------ stickies and text

#[test]
fn sticky_palette_is_complete_and_wide_notes_come_from_the_menu() {
    assert_eq!(crate::theme::BOARD_PALETTE.len(), 10);
    assert!(crate::theme::BOARD_PALETTE.contains(&0xFFFFFF) && crate::theme::BOARD_PALETTE.contains(&0xE6E6E6));
    let mut s = board_sim();
    let chev = s.rect_of("board-chevron-Sticky");
    s.click(chev.center());
    let wide = s.rect_of("board-sticky-wide");
    s.click(wide.center());
    assert_eq!(s.app().tool, Tool::Sticky);
    assert!(s.app().board.wide);
    let at = s.screen(400.0, 300.0);
    s.click(at);
    s.key(Key::Escape);
    let id = stickies(&s)[0];
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.size, desigma_doc::Vec2::new(desigma_doc::STICKY_WIDE_W, desigma_doc::STICKY_WIDE_H));
    assert!(matches!(&n.kind, NodeKind::Sticky(p) if p.wide));
    // the bar toggles the shape back, keeping the height
    s.shell.session().selection = vec![id];
    s.frame(vec![]);
    let toggle = s.rect_of("board-wide");
    s.click(toggle.center());
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.size, desigma_doc::Vec2::new(desigma_doc::STICKY_WIDE_H, desigma_doc::STICKY_WIDE_H));
    assert!(matches!(&n.kind, NodeKind::Sticky(p) if !p.wide));
    assert!(!s.app().board.wide, "the Sticky tool follows the toggle");
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(id).unwrap().size.x, desigma_doc::STICKY_WIDE_W);
    // white is in the palette now
    let white = s.rect_of("board-swatch-9");
    s.click(white.center());
    assert!(matches!(&s.app().doc.get(id).unwrap().kind, NodeKind::Sticky(p) if p.color == desigma_doc::Color::WHITE));
}

#[test]
fn text_layers_can_be_highlighted_from_the_bar() {
    let mut s = board_sim();
    let id = s.shell.session().doc.alloc_id();
    let page = s.app().page;
    let style = desigma_doc::TextStyle { font_size: 24.0, ..Default::default() };
    s.shell.session().doc.insert(page, Node::text(id, DRect::new(300.0, 300.0, 200.0, 40.0), "Important", style)).unwrap();
    s.shell.session().changed();
    s.shell.session().selection = vec![id];
    s.frame(vec![]);
    let hl = |s: &Sim| s.app().doc.get(id).unwrap().fills.first().and_then(crate::board::create::solid_rgb);
    assert_eq!(hl(&s), None);
    let btn = s.rect_of("board-highlight");
    s.click(btn.center());
    assert_eq!(hl(&s), Some(crate::board::rgb_of(s.app().board.color)), "highlight starts in the note colour");
    // a colour for the background
    let sw = s.rect_of("board-hl-swatch-5");
    s.click(sw.center());
    assert_eq!(hl(&s), Some(crate::theme::BOARD_PALETTE[5]));
    // the highlight is drawn behind the text by the renderer
    assert!(desigma_render::board::lowers(&s.app().doc, s.app().doc.get(id).unwrap()));
    let btn = s.rect_of("board-highlight");
    s.click(btn.center());
    assert_eq!(hl(&s), None);
    s.cmd_key(Key::Z);
    assert_eq!(hl(&s), Some(crate::theme::BOARD_PALETTE[5]));
}

#[test]
fn the_stamp_picker_offers_the_full_set() {
    let mut s = board_sim();
    let chev = s.rect_of("board-chevron-Stamp");
    s.click(chev.center());
    for k in desigma_doc::STAMP_KINDS {
        let cell = s.rect_of(&format!("board-stamp-{k}"));
        assert!(cell.width() > 20.0, "{k}");
    }
    let cell = s.rect_of("board-stamp-fire");
    s.click(cell.center());
    let at = s.screen(500.0, 400.0);
    s.click(at);
    let id = s.app().selection[0];
    assert_eq!(s.app().doc.get(id).unwrap().kind, NodeKind::Stamp { kind: "fire".into() });
}

// ------------------------------------------------------------------ timer and voting

fn session(s: &Sim) -> desigma_doc::BoardSession {
    s.app().doc.board_session.clone()
}

#[test]
fn timer_starts_counts_adds_minutes_resets_and_undoes() {
    let mut s = board_sim();
    assert!(session(&s).timer.is_none());
    let start = s.rect_of("board-timer-start");
    s.click(start.center());
    let t = session(&s).timer.unwrap();
    assert!(!t.running());
    assert_eq!(t.total_secs, 300.0);
    assert_eq!(t.clock(0.0), "5:00");
    // play
    let play = s.rect_of("board-timer-play");
    s.click(play.center());
    assert!(session(&s).timer.unwrap().running());
    // one more minute
    let add = s.rect_of("board-timer-add");
    s.click(add.center());
    let left = session(&s).timer.unwrap().remaining(crate::board::session::now());
    assert!(left > 355.0 && left <= 360.0, "{left}");
    // pause holds the time
    let play = s.rect_of("board-timer-play");
    s.click(play.center());
    let t = session(&s).timer.unwrap();
    assert!(!t.running());
    assert!(t.left_secs > 355.0 && t.left_secs <= 360.0);
    // reset goes back to the set length
    let reset = s.rect_of("board-timer-reset");
    s.click(reset.center());
    let t = session(&s).timer.unwrap();
    assert_eq!(t.left_secs, t.total_secs);
    // close removes it, and undo brings it back
    let close = s.rect_of("board-timer-close");
    s.click(close.center());
    assert!(session(&s).timer.is_none());
    s.cmd_key(Key::Z);
    assert!(session(&s).timer.is_some());
    // it is saved with the file
    let back = desigma_doc::Document::from_json(&s.app().doc.to_json().unwrap()).unwrap();
    assert_eq!(back.board_session, session(&s));
}

#[test]
fn a_running_timer_that_ran_out_shows_zero() {
    let mut s = board_sim();
    let now = crate::board::session::now();
    s.shell.session().doc.board_session.timer = Some(desigma_doc::TimerState { total_secs: 60.0, left_secs: 60.0, started_at: Some(now - 90.0) });
    s.frame(vec![]);
    let t = session(&s).timer.unwrap();
    assert!(t.finished(now));
    assert_eq!(t.clock(now), "0:00");
    let _ = s.rect_of("board-timer-clock");
}

#[test]
fn voting_session_counts_votes_and_lists_results() {
    let mut s = board_sim();
    let a = add_sticky(&mut s, 100.0, 200.0, "Alpha");
    let b = add_sticky(&mut s, 400.0, 200.0, "Beta");
    let c = add_sticky(&mut s, 700.0, 200.0, "Gamma");
    // set up: 3 -> 4 votes each, then start
    let start = s.rect_of("board-vote-start");
    s.click(start.center());
    let plus = s.rect_of("board-vote-plus");
    s.click(plus.center());
    assert_eq!(s.app().board.votes_each, desigma_doc::VOTES_DEFAULT + 1);
    let go = s.rect_of("board-vote-go");
    s.click(go.center());
    let me = crate::board::session::voter_id(s.shell.session());
    let v = session(&s).vote.unwrap();
    assert_eq!((v.votes_per_person, v.ended, v.used(&me)), (4, false, 0));
    assert!(session(&s).voting());
    // clicking items votes instead of selecting
    let (pa, pb, pc) = (node_center(&s, a), node_center(&s, b), node_center(&s, c));
    s.click(pa);
    s.click(pa);
    s.click(pb);
    assert!(s.app().selection.is_empty(), "votes do not select");
    let v = session(&s).vote.unwrap();
    assert_eq!((v.count(a), v.count(b), v.left(&me)), (2, 1, 1));
    // Alt takes a vote back
    s.mods = Modifiers::ALT;
    s.click(pa);
    s.mods = Modifiers::NONE;
    assert_eq!(session(&s).vote.unwrap().count(a), 1);
    // spend the rest; a further vote is refused
    s.click(pc);
    s.click(pc);
    s.click(pc);
    let v = session(&s).vote.unwrap();
    assert_eq!((v.count(c), v.left(&me)), (2, 0), "the third click found no votes left");
    s.click(pb);
    assert_eq!(session(&s).vote.unwrap().count(b), 1, "no votes left");
    // each vote is one undo step
    s.cmd_key(Key::Z);
    assert_eq!(session(&s).vote.unwrap().count(c), 1);
    s.mods = Modifiers::COMMAND | Modifiers::SHIFT;
    s.key(Key::Z);
    s.mods = Modifiers::NONE;
    assert_eq!(session(&s).vote.unwrap().count(c), 2);
    // end: the results are sorted by votes
    let end = s.rect_of("board-vote-end");
    s.click(end.center());
    let v = session(&s).vote.unwrap();
    assert!(v.ended && !session(&s).voting());
    assert_eq!(v.results(&s.app().doc), vec![(c, 2), (a, 1), (b, 1)]);
    let _ = s.rect_of("board-vote-panel");
    for i in 0..3 {
        let _ = s.rect_of(&format!("board-vote-row-{i}"));
    }
    // a result row selects its item
    let row = s.rect_of("board-vote-row-0");
    s.click(row.center());
    assert_eq!(s.app().selection, vec![c]);
    // voting is closed: clicks select again and add nothing
    s.click(pb);
    assert_eq!(s.app().selection, vec![b]);
    assert_eq!(session(&s).vote.unwrap().count(b), 1);
    // clear removes the dots and the session; undo brings the results back
    let clear = s.rect_of("board-vote-clear");
    s.click(clear.center());
    assert!(session(&s).vote.is_none());
    s.cmd_key(Key::Z);
    assert!(session(&s).vote.is_some());
}

#[test]
fn the_vote_limit_counts_only_my_votes_and_totals_are_sums() {
    let mut s = board_sim();
    let a = add_sticky(&mut s, 100.0, 200.0, "Alpha");
    let b = add_sticky(&mut s, 400.0, 200.0, "Beta");
    let go_vote = |s: &mut Sim| {
        let start = s.rect_of("board-vote-start");
        s.click(start.center());
        let go = s.rect_of("board-vote-go");
        s.click(go.center());
    };
    go_vote(&mut s);
    let me = crate::board::session::voter_id(s.shell.session());
    assert_eq!(crate::board::session::voter_id(s.shell.session()), me, "the id is stable");
    // someone else spent all their votes, and used up more than the limit in total
    let mut sess = session(&s);
    let v = sess.vote.as_mut().unwrap();
    for _ in 0..desigma_doc::VOTES_DEFAULT {
        assert!(v.add(a, "other"));
    }
    s.shell.session().doc.board_session = sess;
    s.frame(vec![]);
    let (pa, pb) = (node_center(&s, a), node_center(&s, b));
    // my own votes still work: three of mine, then the fourth is refused
    for _ in 0..4 {
        s.click(pa);
    }
    let v = session(&s).vote.unwrap();
    assert_eq!((v.count_by(a, &me), v.count_by(a, "other"), v.count(a)), (3, 3, 6));
    assert_eq!((v.left(&me), v.left("other")), (0, 0));
    s.click(pb);
    assert_eq!(session(&s).vote.unwrap().count(b), 0, "I have none left");
    // Alt takes back only mine
    s.mods = Modifiers::ALT;
    s.click(pb);
    s.click(pa);
    s.mods = Modifiers::NONE;
    let v = session(&s).vote.unwrap();
    assert_eq!((v.count_by(a, &me), v.count_by(a, "other"), v.left(&me)), (2, 3, 1));
    // the results show the sum of everyone
    s.click(pb);
    let end = s.rect_of("board-vote-end");
    s.click(end.center());
    let v = session(&s).vote.unwrap();
    assert_eq!(v.results(&s.app().doc), vec![(a, 5), (b, 1)]);
    assert_eq!(v.total(), 6);
    let _ = s.rect_of("board-vote-row-1");
}

#[test]
fn votes_of_an_old_file_count_as_one_legacy_voter() {
    let mut s = board_sim();
    let a = add_sticky(&mut s, 100.0, 200.0, "Alpha");
    let mut sess = session(&s);
    let old = format!(r#"{{"votes_per_person":3,"ended":false,"votes":{{"{}":2}}}}"#, a.0);
    sess.vote = Some(serde_json::from_str(&old).unwrap());
    s.shell.session().doc.board_session = sess;
    s.frame(vec![]);
    let v = session(&s).vote.unwrap();
    assert_eq!(v.count(a), 2);
    // this install is a new voter with its full budget, on top of the old votes
    let me = crate::board::session::voter_id(s.shell.session());
    assert_eq!(v.left(&me), 3);
    let pa = node_center(&s, a);
    s.click(pa);
    let v = session(&s).vote.unwrap();
    assert_eq!((v.count(a), v.count_by(a, desigma_doc::LEGACY_VOTER)), (3, 2));
}
