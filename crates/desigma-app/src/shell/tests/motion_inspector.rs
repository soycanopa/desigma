//! UI tests for the Motion inspector: keyframe diamonds, auto-key, the easing panel, the
//! animation library and the clip popover.

use super::*;
use crate::motion::inspector as ins;
use desigma_doc::motion::edit::{self, KeyRef};
use desigma_doc::motion::{find_preset, AnimProperty, AnimValue, Ease};
use desigma_doc::{Node, Rect as DRect};

fn setup() -> (Sim, NodeId) {
    let mut s = Sim::new();
    let id;
    {
        let app = s.shell.session();
        id = app.doc.alloc_id();
        let page = app.page;
        // Well clear of the demo content, so a drag never lands in a frame.
        let right = app.doc.get(page).unwrap().children.clone().iter().map(|c| crate::util::world_aabb(&app.doc, *c)).map(|r| r.x + r.w).fold(0.0f32, f32::max);
        app.doc.insert(page, Node::rectangle(id, DRect::new(right + 400.0, 100.0, 80.0, 60.0))).unwrap();
        app.view.offset = DV::new(right + 300.0, 0.0);
        app.selection = vec![id];
        app.motion.enabled = true;
        app.changed();
    }
    s.frame(vec![]);
    s.frame(vec![]);
    (s, id)
}

fn exec(s: &mut Sim, cmd: desigma_doc::Command) {
    let app = s.shell.session();
    app.hist.exec(&mut app.doc, cmd).unwrap();
    s.frame(vec![]);
    s.frame(vec![]);
}

fn track_keys(s: &Sim, id: NodeId, p: AnimProperty) -> Vec<u32> {
    s.app().doc.get(id).unwrap().motion.track(p).map(|t| t.keys.iter().map(|k| k.time_ms).collect()).unwrap_or_default()
}

fn easing_of(s: &Sim, id: NodeId, p: AnimProperty, t: u32) -> Ease {
    s.app().doc.get(id).unwrap().motion.track(p).unwrap().key_at(t).unwrap().easing
}

fn type_in(s: &mut Sim, rect: Rect, text: &str) {
    s.click(rect.center());
    s.type_text(text);
    s.key(Key::Enter);
    s.frame(vec![]);
}

#[test]
fn diamond_toggles_a_keyframe_at_the_playhead() {
    let (mut s, id) = setup();
    assert!(track_keys(&s, id, AnimProperty::Position).is_empty());
    s.click(s.rect_of("mo-key:Position").center());
    assert_eq!(track_keys(&s, id, AnimProperty::Position), vec![0]);
    s.click(s.rect_of("mo-key:Position").center());
    assert!(track_keys(&s, id, AnimProperty::Position).is_empty());
    s.cmd_key(Key::Z);
    assert_eq!(track_keys(&s, id, AnimProperty::Position), vec![0]);
}

#[test]
fn editing_an_animated_field_at_another_time_adds_a_keyframe() {
    let (mut s, id) = setup();
    s.click(s.rect_of("mo-key:Opacity").center());
    assert_eq!(track_keys(&s, id, AnimProperty::Opacity), vec![0]);
    s.shell.session().motion.playhead_ms = 500;
    s.frame(vec![]);
    let r = s.field_rect("mo-op");
    type_in(&mut s, r, "30");
    assert_eq!(track_keys(&s, id, AnimProperty::Opacity), vec![0, 500]);
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.motion.track(AnimProperty::Opacity).unwrap().key_at(500).unwrap().value, AnimValue::Scalar(0.3));
    assert_eq!(n.opacity, 1.0, "the static value is untouched");
    // A property without keyframes changes statically, with no animation created.
    let r = s.field_rect("mo-radius");
    type_in(&mut s, r, "12");
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.corner_radii[0], 12.0);
    assert!(n.motion.track(AnimProperty::CornerRadius).is_none());
}

#[test]
fn easing_panel_presets_bezier_drag_and_spring_apply_with_undo() {
    let (mut s, id) = setup();
    for t in [0, 1000] {
        let c = edit::add_keyframe(&s.app().doc, id, AnimProperty::Position, t, None, None).unwrap();
        exec(&mut s, c);
    }
    let key = KeyRef::new(id, AnimProperty::Position, 0);
    ins::open_easing(s.shell.session(), vec![key]);
    s.frame(vec![]);
    s.frame(vec![]);
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::Linear);

    // Preset from the dropdown menu.
    s.click(s.rect_of("mo-ease-dd:panel").center());
    s.click(s.rect_of("mo-ease-row:Ease out").center());
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::EaseOut);
    s.cmd_key(Key::Z);
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::Linear);

    // Dragging a bezier handle is one undo step.
    let h = s.rect_of("mo-ease-h1:panel").center();
    s.drag(h, h + egui::vec2(20.0, -30.0));
    let after = easing_of(&s, id, AnimProperty::Position, 0);
    let Ease::CustomBezier(c) = after else { panic!("{after:?}") };
    assert!(c[0] > 0.1 && c[1] > 0.1, "{c:?}");
    s.cmd_key(Key::Z);
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::Linear, "one undo for the whole drag");
    s.cmd_key(Key::Y);
    assert!(matches!(easing_of(&s, id, AnimProperty::Position, 0), Ease::CustomBezier(_)));

    // Typed values.
    let r = s.rect_of("mo-ease-values:panel");
    type_in(&mut s, r, "0.07, 0.97, 0.58, 1");
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::bezier(0.07, 0.97, 0.58, 1.0));

    // Spring tab and bounce.
    s.click(s.rect_of("mo-ease-tab:Spring:panel").center());
    assert!(easing_of(&s, id, AnimProperty::Position, 0).is_spring());
    let r = s.rect_of("mo-ease-bounce:panel");
    type_in(&mut s, r, "0.5");
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::spring(0.5));
    s.cmd_key(Key::Z);
    assert_eq!(easing_of(&s, id, AnimProperty::Position, 0), Ease::spring(motion_default()));

    // Back leaves the panel.
    s.click(s.rect_of("mo-ease-back").center());
    assert!(!s.app().motion.easing_target);
}

fn motion_default() -> f32 {
    desigma_doc::motion::DEFAULT_BOUNCE
}

#[test]
fn library_search_filter_and_apply_create_clips() {
    let (mut s, id) = setup();
    s.click(s.rect_of("mo-presets").center());
    assert!(s.app().motion_ins.library.is_some());
    s.frame(vec![]);
    s.type_text("burst");
    s.frame(vec![]);
    s.frame(vec![]);
    let _ = s.rect_of("mo-lib:Burst");
    // Filtering is applied to the list.
    let all: usize = ins::library_visible("", 0).iter().map(|g| g.1.len()).sum();
    assert_eq!(all, 41);
    assert_eq!(ins::library_visible("", 2).len(), 1, "one group (Enter)");
    assert!(ins::library_visible("burst", 0).iter().all(|g| g.1.iter().all(|p| p.name == "Burst")));
    s.shell.session().motion.playhead_ms = 250;
    s.click(s.rect_of("mo-lib:Burst").center());
    assert!(s.app().motion_ins.library.is_none(), "applying closes the popover");
    let clips = s.app().doc.get(id).unwrap().motion.clips.clone();
    assert_eq!(clips.len(), find_preset("Burst").unwrap().clips().len());
    assert!(clips.iter().all(|c| c.delay_ms >= 250));
    s.cmd_key(Key::Z);
    assert!(s.app().doc.get(id).unwrap().motion.clips.is_empty(), "one undo removes them all");
}

#[test]
fn clip_popover_edits_delay_duration_and_type() {
    let (mut s, id) = setup();
    let c = edit::apply_preset(&s.app().doc, &[id], find_preset("Burst").unwrap(), 0).unwrap();
    exec(&mut s, c);
    let cid = s.app().doc.get(id).unwrap().motion.clips[0].id;
    s.click(s.rect_of(&format!("mo-clip:{cid}")).center());
    assert_eq!(s.app().motion.selected_clip, Some((id, cid)));
    s.frame(vec![]);
    let r = s.rect_of("mo-clip-delay");
    type_in(&mut s, r, "400");
    let r = s.rect_of("mo-clip-duration");
    type_in(&mut s, r, "750");
    let clip = s.app().doc.get(id).unwrap().motion.clip(cid).unwrap().clone();
    assert_eq!((clip.delay_ms, clip.duration_ms), (400, 750));
    // Type dropdown: the third entry is Custom.
    let dd = s.rect_of("mo-clip-type");
    s.click(dd.center());
    s.click(Pos2::new(dd.center().x, dd.bottom() + 2.0 + 8.0 + 12.0 + 24.0 * 2.0));
    let clip = s.app().doc.get(id).unwrap().motion.clip(cid).unwrap().clone();
    assert_eq!(clip.mode, desigma_doc::motion::ClipMode::Custom);
    // The close button dismisses the popover.
    s.click(s.rect_of("mo-clip-close").center());
    assert_eq!(s.app().motion.selected_clip, None);
}

#[test]
fn dragging_a_layer_in_motion_mode_keys_its_position() {
    let (mut s, id) = setup();
    s.click(s.rect_of("mo-key:Position").center());
    s.shell.session().motion.playhead_ms = 400;
    s.frame(vec![]);
    let x0 = s.app().doc.get(id).unwrap().transform.tx;
    let c = s.screen(x0 + 40.0, 130.0);
    s.mods = Modifiers::COMMAND;
    s.drag(c, c + egui::vec2(40.0, 0.0));
    s.mods = Modifiers::NONE;
    assert_eq!(track_keys(&s, id, AnimProperty::Position), vec![0, 400]);
    let n = s.app().doc.get(id).unwrap();
    assert_eq!(n.transform.tx, x0, "static position unchanged");
    let moved = n.motion.track(AnimProperty::Position).unwrap().key_at(400).unwrap().value.vec2().unwrap();
    assert!(moved.x > x0 + 20.0, "{moved:?}");
}
