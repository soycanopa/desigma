//! UI tests for the variable side of prototyping: the interaction editor (Set variable,
//! Conditional, expression fields) and the Present variables panel.

use super::*;
use crate::protovars;
use desigma_doc::{Action, BinaryOp, Expression, MediaOp, Reaction, Trigger};

fn sim() -> Sim {
    let mut s =
        Sim::with(Options { fixture: Some(protovars::FIXTURE.into()), select: Some("Add".into()), tab: Some("prototype".into()), ..Default::default() });
    s.frame(vec![]);
    s
}

fn reaction0(s: &Sim) -> Reaction {
    s.app().doc.get(s.by_name("Add")).unwrap().prototype.reactions[0].clone()
}

fn open_editor(s: &mut Sim) {
    s.click(s.rect_of("proto-row-0").center());
    assert!(s.app().proto.editing.is_some());
}

/// Lets the popover settle: it measures its own height one frame late, so controls move once
/// after the content changes.
fn settle(s: &mut Sim) {
    s.frame(vec![]);
    s.frame(vec![]);
}

fn pick(s: &mut Sim, name: &str, i: usize) {
    let ctl = s.rect_of(name);
    s.click(ctl.center());
    let y = ctl.bottom() + 2.0 + 4.0 + i as f32 * theme::MENU_ITEM_HEIGHT + theme::MENU_ITEM_HEIGHT / 2.0;
    s.click(Pos2::new(ctl.left() + 20.0, y));
    settle(s);
}

fn type_into(s: &mut Sim, name: &str, text: &str) {
    s.click(s.rect_of(name).center());
    s.cmd_key(Key::A);
    s.type_text(text);
    s.key(Key::Enter);
    settle(s);
}

fn var_id(s: &Sim, name: &str) -> String {
    s.app().doc.variables.values().find(|v| v.name == name).map(|v| v.id.clone()).unwrap()
}

#[test]
fn the_editor_lists_set_variable_and_the_conditional_with_its_branches() {
    let mut s = sim();
    let r = reaction0(&s);
    assert!(matches!(r.actions[0], Action::SetVariable { .. }) && matches!(r.actions[1], Action::Conditional { .. }));
    // the row summarises both actions
    assert!(s.rect_of("proto-row-0").height() > 20.0);
    open_editor(&mut s);
    for name in [
        "proto-pop-action-0",
        "proto-pop-var-0",
        "proto-pop-expr-0.value",
        "proto-pop-action-1",
        "proto-pop-expr-1.if0.cond",
        "proto-pop-action-1.if0.0",
        "proto-pop-add-condition-1",
        "proto-pop-else-1",
        "proto-pop-action-1.else.0",
    ] {
        let r = s.rect_of(name);
        assert!(r.width() > 40.0 && r.height() > 10.0, "{name}: {r:?}");
    }
    // nested lists are indented under their branch (their row starts right of the condition field)
    assert!(s.rect_of("proto-pop-action-1.if0.0").left() > s.rect_of("proto-pop-expr-1.if0.cond").left());
}

#[test]
fn editing_a_condition_updates_the_document_and_is_one_undo_step() {
    let mut s = sim();
    open_editor(&mut s);
    let count = var_id(&s, "count");
    type_into(&mut s, "proto-pop-expr-1.if0.cond", "$count >= 5");
    let Action::Conditional { branches, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(branches[0].0, Expression::binary(BinaryOp::Ge, Expression::var(count.clone()), Expression::number(5.0)));
    s.cmd_key(Key::Z);
    let Action::Conditional { branches, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(branches[0].0, Expression::binary(BinaryOp::Ge, Expression::var(count), Expression::number(3.0)));
}

#[test]
fn an_invalid_expression_is_not_stored() {
    let mut s = sim();
    open_editor(&mut s);
    let before = reaction0(&s);
    // syntax error, unknown variable, wrong type (a number is not a condition)
    for bad in ["$count >=", "$nope > 1", "$count + 1"] {
        type_into(&mut s, "proto-pop-expr-1.if0.cond", bad);
        assert_eq!(reaction0(&s), before, "{bad} must not change the interaction");
    }
    // a correct one afterwards still goes through
    type_into(&mut s, "proto-pop-expr-1.if0.cond", "$member");
    let Action::Conditional { branches, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(branches[0].0, Expression::var(var_id(&s, "member")));
}

#[test]
fn add_condition_and_else_and_remove() {
    let mut s = sim();
    open_editor(&mut s);
    s.click(s.rect_of("proto-pop-add-condition-1").center());
    settle(&mut s);
    let Action::Conditional { branches, otherwise } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(branches.len(), 2, "+ Add condition adds an Else if");
    assert_eq!(branches[1].0, Expression::var(var_id(&s, "member")), "it starts from the first boolean variable");
    assert_eq!(otherwise.len(), 1);
    // the second branch has its own nested action list
    assert!(s.rect_of("proto-pop-action-1.if1.0").width() > 40.0);
    // removing it again
    s.click(s.rect_of("proto-pop-rm-branch-1.if1").center());
    settle(&mut s);
    let Action::Conditional { branches, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(branches.len(), 1);
    // removing the only else action removes the else; "Else" can be added back
    s.click(s.rect_of("proto-pop-rm-action-1.else.0").center());
    settle(&mut s);
    let Action::Conditional { otherwise, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert!(otherwise.is_empty());
    s.click(s.rect_of("proto-pop-add-else-1").center());
    settle(&mut s);
    let Action::Conditional { otherwise, .. } = &reaction0(&s).actions[1] else { panic!() };
    assert_eq!(otherwise.len(), 1);
}

#[test]
fn choosing_set_variable_starts_from_a_value_that_suits_the_variable() {
    let mut s = sim();
    open_editor(&mut s);
    // action 0 is Set variable on `count`; switch it to the label (a text variable)
    let label = var_id(&s, "label");
    let n_vars = s.app().doc.variables.len();
    assert_eq!(n_vars, 4);
    // variables are listed "State / count", "State / label", ...
    pick(&mut s, "proto-pop-var-0", 1);
    let Action::SetVariable { variable, value } = &reaction0(&s).actions[0] else { panic!() };
    assert_eq!(variable, &label);
    assert_eq!(value, &Expression::text(""), "text variables start empty");
    pick(&mut s, "proto-pop-var-0", 0);
    let Action::SetVariable { variable, value } = &reaction0(&s).actions[0] else { panic!() };
    assert_eq!(variable, &var_id(&s, "count"));
    assert!(matches!(value, Expression::Binary { op: BinaryOp::Add, .. }), "numbers start as '+ 1'");
    // a value of the wrong type is refused
    let before = reaction0(&s);
    type_into(&mut s, "proto-pop-expr-0.value", "\"seven\"");
    assert_eq!(reaction0(&s), before);
    type_into(&mut s, "proto-pop-expr-0.value", "($count + 2) * 3");
    let Action::SetVariable { value, .. } = &reaction0(&s).actions[0] else { panic!() };
    assert_eq!(crate::proto::expression_text(&s.app().doc, value), "($count + 2) * 3");
}

#[test]
fn the_action_menu_can_add_every_new_action() {
    let mut s = sim();
    open_editor(&mut s);
    // action 0 -> Set variable mode (index 9)
    pick(&mut s, "proto-pop-action-0", 9);
    let Action::SetVariableMode { collection, mode } = &reaction0(&s).actions[0] else { panic!("{:?}", reaction0(&s).actions[0]) };
    assert!(!collection.is_empty() && !mode.is_empty());
    assert!(s.rect_of("proto-pop-coll-0").width() > 40.0 && s.rect_of("proto-pop-mode-0").width() > 40.0);
    // pick the Dark mode
    pick(&mut s, "proto-pop-mode-0", 1);
    let dark = protovars::ids(&s.app().doc).dark;
    assert!(matches!(&reaction0(&s).actions[0], Action::SetVariableMode { mode, .. } if *mode == dark));
    // Control video: no video layers in this file, the editor says so but keeps the action valid
    pick(&mut s, "proto-pop-action-0", 11);
    assert!(matches!(reaction0(&s).actions[0], Action::MediaControl { op: MediaOp::TogglePlay, .. }));
    pick(&mut s, "proto-pop-media-op-0", 6);
    assert!(matches!(reaction0(&s).actions[0], Action::MediaControl { op: MediaOp::SkipBy(_), .. }));
    // Conditional
    pick(&mut s, "proto-pop-action-0", 10);
    assert!(matches!(reaction0(&s).actions[0], Action::Conditional { .. }));
}

#[test]
fn gamepad_and_key_triggers_have_their_fields() {
    let mut s = sim();
    open_editor(&mut s);
    pick(&mut s, "proto-pop-trigger", 10);
    assert!(matches!(reaction0(&s).trigger, Trigger::GamepadPress { .. }));
    type_into(&mut s, "proto-pop-gamepad-buttons", "LB+A");
    assert!(matches!(reaction0(&s).trigger, Trigger::GamepadPress { ref buttons, .. } if buttons == "LB+A"));
    pick(&mut s, "proto-pop-gamepad-device", 1);
    assert!(matches!(reaction0(&s).trigger, Trigger::GamepadPress { device: desigma_doc::GamepadDevice::Xbox, .. }));
    pick(&mut s, "proto-pop-trigger", 9);
    type_into(&mut s, "proto-pop-keys", "Ctrl+Shift+K");
    assert_eq!(reaction0(&s).trigger, Trigger::KeyPress { keys: "Ctrl+Shift+K".into() });
}

// ------------------------------------------------------------------ Present

fn present(s: &mut Sim) {
    s.click(s.rect_of("present-button").center());
    assert!(s.app().proto.present.is_some());
    s.frame(vec![]);
}

fn click_node(s: &mut Sim, name: &str) {
    let id = s.by_name(name);
    let p = s.app().proto.present.as_ref().unwrap().screen_center_of(id).unwrap();
    s.click(p);
}

fn count_value(s: &Sim) -> f32 {
    let st = s.app().proto.present.as_ref().unwrap();
    let id = var_id(s, "count");
    match st.player.variable_value(&id) {
        Some(desigma_doc::VariableValue::Number(n)) => n,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_variables_panel_shows_live_values_and_never_touches_the_document() {
    let mut s = sim();
    let before = s.app().doc.variables.clone();
    present(&mut s);
    assert!(s.app().proto.present.as_ref().unwrap().vars_rect.width() < 1.0, "the panel starts hidden");
    s.click(s.rect_of("present-vars-toggle").center());
    assert!(s.app().proto.present.as_ref().unwrap().show_vars);
    let panel = s.rect_of("present-vars-panel");
    assert!(panel.width() > 200.0 && panel.height() > 100.0);
    click_node(&mut s, "Add");
    click_node(&mut s, "Add");
    assert_eq!(count_value(&s), 2.0);
    let rows = s.app().proto.present.as_ref().unwrap().player.variable_rows();
    assert!(rows.iter().any(|r| r.name == "count" && r.value == "2" && r.changed));
    assert!(rows.iter().any(|r| r.name == "label" && r.value == "\"Items in cart: 2\""));
    // the third click navigates (conditional) and the panel keeps working on the next screen
    click_node(&mut s, "Add");
    assert_eq!(s.app().proto.present.as_ref().unwrap().player.current, s.by_name("Checkout"));
    assert_eq!(s.app().doc.variables, before, "Present never writes variables back");
    // clicking the panel does not hit the screen behind it
    let at = s.rect_of("present-vars-panel").center();
    s.click(at);
    assert_eq!(count_value(&s), 3.0);
    // Restart resets the state to the document's values
    s.click(s.rect_of("present-restart").center());
    assert_eq!(count_value(&s), 0.0);
    // toggling hides it again
    s.click(s.rect_of("present-vars-toggle").center());
    assert!(!s.app().proto.present.as_ref().unwrap().show_vars);
}

#[test]
fn present_updates_bound_text_and_the_mode_switch_recolors() {
    let mut s = sim();
    let mode_before = s.app().doc.variable_collections.values().next().unwrap().default_mode.clone();
    present(&mut s);
    let status = s.by_name("Status");
    let text = |s: &Sim| {
        let pl = &s.app().proto.present.as_ref().unwrap().player;
        match &desigma_doc::resolve_node(&pl.doc, pl.doc.get(status).unwrap()).kind {
            desigma_doc::NodeKind::Text(t) => t.content.clone(),
            _ => panic!(),
        }
    };
    assert_eq!(text(&s), "Your cart is empty");
    click_node(&mut s, "Add");
    assert_eq!(text(&s), "Items in cart: 1");
    // the member banner is bound to `member`: hidden, then shown
    let banner = s.by_name("Member banner");
    let shown = |s: &Sim| {
        let pl = &s.app().proto.present.as_ref().unwrap().player;
        desigma_doc::resolve_node(&pl.doc, pl.doc.get(banner).unwrap()).visible
    };
    assert!(!shown(&s));
    click_node(&mut s, "Join");
    assert!(shown(&s));
    click_node(&mut s, "Join");
    assert!(!shown(&s));
    // Dark mode switches the accent bar's colour
    let bar = s.by_name("Accent bar");
    let hex = |s: &Sim| {
        let pl = &s.app().proto.present.as_ref().unwrap().player;
        match &desigma_doc::resolve_node(&pl.doc, pl.doc.get(bar).unwrap()).fills[0].kind {
            desigma_doc::PaintKind::Solid(c) => c.to_hex(),
            _ => panic!(),
        }
    };
    assert_eq!(hex(&s), "0C8CE9");
    click_node(&mut s, "Dark mode");
    assert_eq!(hex(&s), "F2C94C");
    assert_eq!(s.app().doc.variable_collections.values().next().unwrap().default_mode, mode_before, "the document keeps its default mode");
}

#[test]
fn key_and_gamepad_events_reach_the_player() {
    let mut s = sim();
    let add = s.by_name("Add");
    let count = var_id(&s, "count");
    for (trigger, n) in [
        (Trigger::KeyPress { keys: "Ctrl+K".into() }, 10.0),
        (Trigger::GamepadPress { device: desigma_doc::GamepadDevice::Generic, buttons: "A".into() }, 20.0),
    ] {
        s.shell.session().doc.get_mut(add).unwrap().prototype.reactions.push(Reaction {
            trigger,
            actions: vec![Action::SetVariable { variable: count.clone(), value: Expression::number(n) }],
            transition: None,
        });
    }
    present(&mut s);
    s.key(Key::K);
    assert_eq!(count_value(&s), 0.0, "plain K is not Ctrl+K");
    s.mods = Modifiers::COMMAND;
    s.key(Key::K);
    s.mods = Modifiers::NONE;
    assert_eq!(count_value(&s), 10.0);
    // the keyboard stand-in for a controller: Alt+Shift+Z is the A button
    s.mods = Modifiers { alt: true, shift: true, ..Default::default() };
    s.key(Key::Z);
    s.mods = Modifiers::NONE;
    assert_eq!(count_value(&s), 20.0);
}

#[test]
fn dragging_an_open_overlay_down_dismisses_it_and_a_short_drag_snaps_back() {
    use desigma_doc::{Node, OverlayPosition, Rect as DR};
    let mut s = sim();
    // a bottom sheet whose handle can be dragged away
    let page = s.app().page;
    let sheet = {
        let d = &mut s.shell.session().doc;
        let id = d.alloc_id();
        let mut f = Node::frame(id, DR::new(900.0, 0.0, 390.0, 220.0));
        f.name = "Sheet".into();
        let sheet = d.insert(page, f).unwrap();
        let id = d.alloc_id();
        let mut h = Node::rectangle(id, DR::new(0.0, 0.0, 390.0, 60.0));
        h.name = "Grab".into();
        let grab = d.insert(sheet, h).unwrap();
        d.get_mut(grab).unwrap().prototype.reactions.push(Reaction { trigger: Trigger::OnDrag, actions: vec![Action::CloseOverlay], transition: None });
        sheet
    };
    let reset = s.by_name("Reset");
    s.shell.session().doc.get_mut(reset).unwrap().prototype.reactions.push(Reaction {
        trigger: Trigger::OnClick,
        actions: vec![Action::OpenOverlay {
            dest: sheet,
            position: OverlayPosition::BottomCenter,
            offset: desigma_doc::Vec2::ZERO,
            close_on_click_outside: false,
            background: None,
        }],
        transition: None,
    });
    present(&mut s);
    click_node(&mut s, "Reset");
    let open = |s: &Sim| s.app().proto.present.as_ref().unwrap().player.overlays.len();
    assert_eq!(open(&s), 1);
    let handle = {
        let st = s.app().proto.present.as_ref().unwrap();
        let o = &st.player.overlays[0];
        st.stage.min + egui::vec2((o.origin.x + 195.0) * st.scale, (o.origin.y + 30.0) * st.scale)
    };
    let scale = s.app().proto.present.as_ref().unwrap().scale;
    // a short drag moves it while held, and it snaps back on release
    s.mv(handle);
    s.button(true);
    s.mv(handle + egui::vec2(0.0, 10.0 * scale));
    s.mv(handle + egui::vec2(0.0, 30.0 * scale));
    let dragged = s.app().proto.present.as_ref().unwrap().player.overlays[0].drag.y;
    assert!(dragged > 20.0 && dragged < 40.0, "the sheet follows the pointer: {dragged}");
    s.button(false);
    assert_eq!(open(&s), 1);
    assert_eq!(s.app().proto.present.as_ref().unwrap().player.overlays[0].drag, desigma_doc::Vec2::ZERO, "snapped back");
    // a long one dismisses it
    s.mv(handle);
    s.button(true);
    for i in 1..=6 {
        s.mv(handle + egui::vec2(0.0, 30.0 * i as f32 * scale));
    }
    s.button(false);
    s.frame(vec![]);
    assert_eq!(open(&s), 0, "dragged past the threshold");
}

#[test]
fn removing_the_last_variable_ui_state_is_safe() {
    // a file without variables: the editor still opens Set variable without panicking
    let mut s = Sim::with(Options { select: Some("Button".into()), tab: Some("prototype".into()), ..Default::default() });
    s.click(s.rect_of("proto-row-0").center());
    pick(&mut s, "proto-pop-action-0", 8);
    assert!(matches!(s.app().doc.get(s.by_name("Button")).unwrap().prototype.reactions[0].actions[0], Action::SetVariable { .. }));
    s.frame(vec![]);
    pick(&mut s, "proto-pop-action-0", 9);
    s.frame(vec![]);
    pick(&mut s, "proto-pop-action-0", 10);
    s.frame(vec![]);
    assert!(s.app().proto.editing.is_some());
}
