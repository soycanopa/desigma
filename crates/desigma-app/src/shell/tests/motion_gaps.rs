//! Motion mode gaps: the transform gizmo on the layer, the timeline export button, keyframe
//! diamonds for stroke and effects, per-track visibility, dragging the summary bar, the scale
//! lock and animated fills among several.

use super::*;
use crate::motion::{inspector as ins, timeline};
use desigma_doc::motion::edit;
use desigma_doc::motion::{self, find_preset, AnimProperty, AnimValue};
use desigma_doc::{Effect, Node, PaintKind, Rect as DRect};

fn settle(s: &mut Sim) {
    s.frame(vec![]);
    s.frame(vec![]);
}

fn exec(s: &mut Sim, cmd: desigma_doc::Command) {
    let app = s.shell.session();
    app.hist.exec(&mut app.doc, cmd).unwrap();
    app.changed();
    settle(s);
}

fn keys(s: &Sim, id: NodeId, p: AnimProperty) -> Vec<u32> {
    s.app().doc.get(id).unwrap().motion.track(p).map(|t| t.keys.iter().map(|k| k.time_ms).collect()).unwrap_or_default()
}

fn key_value(s: &Sim, id: NodeId, p: AnimProperty, t: u32) -> AnimValue {
    s.app().doc.get(id).unwrap().motion.track(p).unwrap().key_at(t).unwrap().value
}

fn type_in(s: &mut Sim, at: Pos2, text: &str) {
    s.click(at);
    s.type_text(text);
    s.key(Key::Enter);
    s.frame(vec![]);
}

/// Motion mode with a frame far from the demo content and a rectangle inside it. The rectangle
/// (local position (50, 40), 80 x 60) moves to (250, 140) over a second while rotating.
fn nested() -> (Sim, NodeId, NodeId) {
    let mut s = Sim::new();
    let (frame, id);
    {
        let app = s.shell.session();
        let page = app.page;
        let right = app.doc.get(page).unwrap().children.clone().iter().map(|c| crate::util::world_aabb(&app.doc, *c)).map(|r| r.x + r.w).fold(0.0f32, f32::max);
        frame = app.doc.alloc_id();
        app.doc.insert(page, Node::frame(frame, DRect::new(right + 400.0, 100.0, 400.0, 300.0))).unwrap();
        id = app.doc.alloc_id();
        let mut n = Node::rectangle(id, DRect::new(50.0, 40.0, 80.0, 60.0));
        let pos = n.motion.ensure_track(AnimProperty::Position);
        pos.set_key(0, AnimValue::Vec2(DV::new(50.0, 40.0)), None);
        pos.set_key(1000, AnimValue::Vec2(DV::new(250.0, 140.0)), None);
        let rot = n.motion.ensure_track(AnimProperty::Rotation);
        rot.set_key(0, AnimValue::Scalar(0.0), None);
        rot.set_key(1000, AnimValue::Scalar(90.0), None);
        app.doc.insert(frame, n).unwrap();
        app.view.offset = DV::new(right + 300.0, 0.0);
        app.selection = vec![id];
        app.motion.enabled = true;
        app.changed();
    }
    settle(&mut s);
    (s, id, frame)
}

fn close(a: Pos2, b: Pos2, what: &str) {
    assert!((a - b).length() < 0.6, "{what}: {a:?} vs {b:?}");
}

fn page_centre_of_selection(s: &Sim) -> DV {
    let (c, _) = crate::canvas::interact::selection_box(s.app()).unwrap();
    DV::new((c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0)
}

// ------------------------------------------------------------------ 1. gizmo

#[test]
fn gizmo_and_path_sit_on_the_layer_centre_inside_an_offset_frame_at_any_zoom() {
    let (mut s, id, frame) = nested();
    let (ftx, fty) = {
        let f = s.app().doc.get(frame).unwrap();
        (f.transform.tx, f.transform.ty)
    };
    for zoom in [1.0, 0.6, 2.5] {
        s.shell.session().view.zoom = zoom;
        let o = s.shell.session().view.offset;
        s.shell.session().view.offset = DV::new(o.x + 13.0, o.y - 7.0);
        for (t, local) in [(0u32, (90.0, 70.0)), (500, (190.0, 120.0)), (1000, (290.0, 170.0))] {
            s.shell.session().motion.playhead_ms = t;
            settle(&mut s);
            let app = s.app();
            let want = app.view.to_screen(app.canvas_rect.min, DV::new(ftx + local.0, fty + local.1));
            close(ins::gizmo_centre(app, app.canvas_rect, id).unwrap(), want, &format!("gizmo at {t} ms, zoom {zoom}"));
            close(ins::path_point(app, app.canvas_rect, id, t as f32).unwrap(), want, &format!("path at {t} ms"));
            // the selection box is drawn on the same spot: the layer's centre as evaluated
            let sc = page_centre_of_selection(&s);
            assert!((sc.x - (ftx + local.0)).abs() < 0.01 && (sc.y - (fty + local.1)).abs() < 0.01, "selection box centre {sc:?} at {t}");
        }
    }
    // the dotted path starts and ends where the layer does
    let app = s.app();
    let p0 = ins::path_point(app, app.canvas_rect, id, 0.0).unwrap();
    let p1 = ins::path_point(app, app.canvas_rect, id, 1000.0).unwrap();
    assert!((p0 - p1).length() > 50.0);
}

#[test]
fn gizmo_follows_clips_and_a_moving_parent() {
    let (mut s, id, frame) = nested();
    let c = edit::apply_preset(&s.app().doc, &[id], find_preset("Burst").unwrap(), 200).unwrap();
    exec(&mut s, c);
    // the frame itself slides while the child moves inside it
    let c = edit::add_keyframe(&s.app().doc, frame, AnimProperty::Position, 0, None, None).unwrap();
    exec(&mut s, c);
    let fpos = motion::base_position(s.app().doc.get(frame).unwrap());
    let c = edit::add_keyframe(&s.app().doc, frame, AnimProperty::Position, 1000, Some(AnimValue::Vec2(DV::new(fpos.x + 120.0, fpos.y + 30.0))), None).unwrap();
    exec(&mut s, c);
    for t in [0u32, 300, 700, 1000] {
        s.shell.session().motion.playhead_ms = t;
        settle(&mut s);
        let app = s.app();
        // the evaluated document, computed independently of the canvas cache
        let ev = motion::evaluate(&app.doc, app.page, t as f32);
        let n = ev.get(id).unwrap();
        let w = ev.world_transform(id).apply(DV::new(n.size.x / 2.0, n.size.y / 2.0));
        let want = app.view.to_screen(app.canvas_rect.min, w);
        close(ins::gizmo_centre(app, app.canvas_rect, id).unwrap(), want, &format!("gizmo at {t}"));
        let sc = page_centre_of_selection(&s);
        assert!((sc.x - w.x).abs() < 0.01 && (sc.y - w.y).abs() < 0.01, "selection at {t}: {sc:?} vs {w:?}");
    }
    // not the stored position
    let stored = s.app().doc.world_transform(id).apply(DV::new(40.0, 30.0));
    let now = page_centre_of_selection(&s);
    assert!((stored.x - now.x).abs() > 50.0, "gizmo is on the evaluated layer, not the stored one");
}

#[test]
fn crosshair_button_hides_the_gizmo() {
    let (mut s, _, _) = nested();
    assert!(!s.app().motion_ins.gizmo_hidden);
    s.click(s.rect_of("mo-gizmo").center());
    assert!(s.app().motion_ins.gizmo_hidden);
    s.click(s.rect_of("mo-gizmo").center());
    assert!(!s.app().motion_ins.gizmo_hidden);
}

// ------------------------------------------------------------------ 2. export button

#[test]
fn timeline_export_button_targets_the_frame_and_explains_an_empty_timeline() {
    let (mut s, id, frame) = nested();
    assert_eq!(timeline::export_target(s.app()), frame, "the top-level frame holding the selection");
    s.shell.session().selection = vec![frame];
    assert_eq!(timeline::export_target(s.app()), frame);
    s.shell.session().selection.clear();
    assert_eq!(timeline::export_target(s.app()), s.app().page, "nothing selected: the page");
    assert!(s.app().has_motion_export(frame), "the button would open the export of an animated frame");
    // the button sits in the header between the layer folding and the collapse button
    settle(&mut s);
    let b = s.rect_of("motion-export");
    assert!(b.left() > s.rect_of("motion-loop").right() && b.right() <= s.rect_of("motion-collapse").left());
    assert!((b.center().y - s.rect_of("motion-collapse").center().y).abs() < 0.5);

    // nothing animated: a click says so instead of opening a file dialog
    let c = edit::remove_track(&s.app().doc, id, AnimProperty::Position).unwrap();
    exec(&mut s, c);
    let c = edit::remove_track(&s.app().doc, id, AnimProperty::Rotation).unwrap();
    exec(&mut s, c);
    s.shell.session().selection = vec![id];
    settle(&mut s);
    assert!(!s.app().has_motion_export(frame));
    s.shell.session().toast = None;
    s.click(s.rect_of("motion-export").center());
    let t = s.app().toast.as_ref().expect("a toast explains why").text.clone();
    assert!(t.contains("Nothing to export"), "{t}");
    assert!(!s.app().canvas.motion.export.running());
}

// ------------------------------------------------------------------ 3. stroke and effects

/// A rectangle with a stroke, a drop shadow and a layer blur, selected in Motion mode.
fn styled() -> (Sim, NodeId) {
    let (mut s, id, _) = nested();
    let app = s.shell.session();
    let mut n = Node::rectangle(app.doc.alloc_id(), DRect::new(0.0, 0.0, 80.0, 60.0));
    let page = app.page;
    let sid = n.id;
    n.stroke.paints = vec![desigma_doc::Paint::solid(desigma_doc::Color::rgb(0, 0, 0))];
    n.stroke.weight = 2.0;
    n.effects = vec![
        Effect::DropShadow {
            color: desigma_doc::Color::rgba(0, 0, 0, 64),
            offset: DV::new(0.0, 4.0),
            radius: 4.0,
            spread: 0.0,
            visible: true,
            show_behind: false,
        },
        Effect::LayerBlur { radius: 3.0, visible: true },
    ];
    let right = crate::util::world_aabb(&app.doc, id).x;
    n.transform.tx = right + 600.0;
    app.doc.insert(page, n).unwrap();
    app.selection = vec![sid];
    app.motion.playhead_ms = 0;
    app.changed();
    settle(&mut s);
    (s, sid)
}

fn scroll_inspector(s: &mut Sim) {
    s.shell.session().motion_ins.scroll = 1.0e6;
    settle(s);
}

#[test]
fn stroke_colour_and_weight_have_keyframe_diamonds() {
    let (mut s, id) = styled();
    scroll_inspector(&mut s);
    s.click(s.rect_of("mo-key:Stroke").center());
    assert_eq!(keys(&s, id, AnimProperty::StrokeColor), vec![0]);
    s.click(s.rect_of("mo-key:Stroke weight").center());
    assert_eq!(keys(&s, id, AnimProperty::StrokeWeight), vec![0]);
    // editing at another time adds keyframes; the static stroke stays
    s.shell.session().motion.playhead_ms = 500;
    settle(&mut s);
    let w = s.field_rect("stroke-w");
    type_in(&mut s, w.center(), "8");
    assert_eq!(keys(&s, id, AnimProperty::StrokeWeight), vec![0, 500]);
    assert_eq!(key_value(&s, id, AnimProperty::StrokeWeight, 500), AnimValue::Scalar(8.0));
    assert_eq!(s.app().doc.get(id).unwrap().stroke.weight, 2.0);
    let hex = s.rect_of("hex:true:0");
    type_in(&mut s, hex.center(), "FF0000");
    assert_eq!(keys(&s, id, AnimProperty::StrokeColor), vec![0, 500]);
    assert_eq!(key_value(&s, id, AnimProperty::StrokeColor, 500).color().unwrap().to_hex(), "FF0000");
    // the canvas and the inspector show the animated stroke at the playhead
    let shown = s.app().shown_doc().get(id).unwrap().clone();
    assert_eq!(shown.stroke.weight, 8.0);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, id, AnimProperty::StrokeColor), vec![0], "one undo step");
    // the diamond removes the keyframe again
    s.shell.session().motion.playhead_ms = 0;
    settle(&mut s);
    s.click(s.rect_of("mo-key:Stroke weight").center());
    assert_eq!(keys(&s, id, AnimProperty::StrokeWeight), vec![500]);
}

fn open_effect(s: &mut Sim, i: usize, id: NodeId) {
    use crate::panels::effects::{PopKind, PopState};
    let anchor = Rect::from_min_size(Pos2::new(1500.0, 700.0), Vec2::splat(24.0));
    s.shell.session().props.pop = Some(PopState { kind: PopKind::Effect(i), anchor, ids: vec![id] });
    settle(s);
}

/// Typing into the field left of a popover diamond.
fn type_beside(s: &mut Sim, diamond: &str, text: &str) {
    let d = s.rect_of(diamond);
    type_in(s, Pos2::new(d.left() - 44.0, d.center().y), text);
}

#[test]
fn drop_shadow_and_layer_blur_have_keyframe_diamonds() {
    let (mut s, id) = styled();
    scroll_inspector(&mut s);
    open_effect(&mut s, 0, id);
    for (name, prop) in [
        ("mo-key:Shadow X", AnimProperty::ShadowX),
        ("mo-key:Shadow Y", AnimProperty::ShadowY),
        ("mo-key:Shadow blur", AnimProperty::ShadowBlur),
        ("mo-key:Shadow color", AnimProperty::ShadowColor),
    ] {
        s.click(s.rect_of(name).center());
        assert_eq!(keys(&s, id, prop), vec![0], "{name}");
    }
    assert_eq!(key_value(&s, id, AnimProperty::ShadowY, 0), AnimValue::Scalar(4.0), "a new key keeps the look");
    s.shell.session().motion.playhead_ms = 500;
    settle(&mut s);
    type_beside(&mut s, "mo-key:Shadow X", "12");
    type_beside(&mut s, "mo-key:Shadow blur", "20");
    assert_eq!(keys(&s, id, AnimProperty::ShadowX), vec![0, 500]);
    assert_eq!(key_value(&s, id, AnimProperty::ShadowX, 500), AnimValue::Scalar(12.0));
    assert_eq!(key_value(&s, id, AnimProperty::ShadowBlur, 500), AnimValue::Scalar(20.0));
    let n = s.app().doc.get(id).unwrap();
    assert!(matches!(n.effects[0], Effect::DropShadow { offset, radius, .. } if offset.x == 0.0 && radius == 4.0), "the stored shadow is untouched");
    let Effect::DropShadow { offset, radius, .. } = s.app().shown_doc().get(id).unwrap().effects[0] else { panic!() };
    assert_eq!((offset.x, radius), (12.0, 20.0), "the canvas shows the animated shadow");
    // the layer blur has its own diamond in its popover
    open_effect(&mut s, 1, id);
    s.click(s.rect_of("mo-key:Layer blur").center());
    assert_eq!(keys(&s, id, AnimProperty::BlurRadius), vec![500]);
    type_beside(&mut s, "mo-key:Layer blur", "9");
    assert_eq!(key_value(&s, id, AnimProperty::BlurRadius, 500), AnimValue::Scalar(9.0));
}

// ------------------------------------------------------------------ 4. per-track eye

#[test]
fn track_eye_hides_a_track_everywhere_and_undoes() {
    let (mut s, id, _) = nested();
    let stored_tx = motion::base_position(s.app().doc.get(id).unwrap()).x;
    s.shell.session().motion.playhead_ms = 1000;
    settle(&mut s);
    let moved = |s: &Sim| motion::base_position(s.app().shown_doc().get(id).unwrap()).x;
    assert!(moved(&s) > stored_tx + 100.0, "the position track moves the layer");
    // hover the row to reveal the eye, then click it
    let d = s.rect_of(&format!("motion-diamond-{}-Position", id.0));
    s.mv(Pos2::new(d.center().x - 50.0, d.center().y));
    let eye = s.rect_of(&format!("motion-eye-{}-Position", id.0));
    assert!(eye.left() > d.right(), "the eye is at the right end of the row");
    s.click(eye.center());
    let track = s.app().doc.get(id).unwrap().motion.track(AnimProperty::Position).unwrap().clone();
    assert!(!track.visible);
    assert_eq!(track.keys.len(), 2, "keyframes are kept");
    settle(&mut s);
    assert_eq!(moved(&s), stored_tx, "the hidden track no longer moves the layer");
    assert!(motion::evaluate_node(s.app().doc.get(id).unwrap(), 1000.0).position.is_none());
    // the eye stays visible while the track is off, and the Rotation track is unaffected
    s.mv(Pos2::new(10.0, 10.0));
    s.click(s.rect_of(&format!("motion-eye-{}-Position", id.0)).center());
    assert!(s.app().doc.get(id).unwrap().motion.track(AnimProperty::Position).unwrap().visible);
    assert!(s.app().doc.get(id).unwrap().motion.track(AnimProperty::Rotation).unwrap().visible);
    s.cmd_key(Key::Z);
    assert!(!s.app().doc.get(id).unwrap().motion.track(AnimProperty::Position).unwrap().visible, "each toggle is one undo step");
    // exports leave a hidden track out
    let css = desigma_export::css_animation_for(&s.app().doc, s.app().page);
    assert!(!css.contains("-position"), "{css}");
    assert!(css.contains("-rotation"), "{css}");
}

// ------------------------------------------------------------------ 5. summary bar drag

#[test]
fn dragging_the_summary_bar_moves_keys_and_clips_in_one_undo_step() {
    let (mut s, id, _) = nested();
    let c = edit::apply_preset(&s.app().doc, &[id], find_preset("Burst").unwrap(), 100).unwrap();
    exec(&mut s, c);
    let before = s.app().doc.get(id).unwrap().motion.clone();
    let clip_delays: Vec<u32> = before.clips.iter().map(|c| c.delay_ms).collect();
    let g = s.rect_of("motion-scale");
    let px = |ms: f32| g.width() * ms / 1000.0;
    let bar = s.rect_of(&format!("motion-summary-{}", id.0));
    let a = Pos2::new(bar.left() + px(300.0), bar.center().y);
    let undo_depth = s.app().hist.undo_len();
    s.drag(a, a + egui::vec2(px(250.0), 0.0));
    let m = &s.app().doc.get(id).unwrap().motion;
    let shifted = |t: u32| t + 250;
    assert_eq!(keys(&s, id, AnimProperty::Position), before.track(AnimProperty::Position).unwrap().keys.iter().map(|k| shifted(k.time_ms)).collect::<Vec<_>>());
    assert_eq!(keys(&s, id, AnimProperty::Rotation), before.track(AnimProperty::Rotation).unwrap().keys.iter().map(|k| shifted(k.time_ms)).collect::<Vec<_>>());
    assert_eq!(m.clips.iter().map(|c| c.delay_ms).collect::<Vec<_>>(), clip_delays.iter().map(|d| shifted(*d)).collect::<Vec<_>>());
    assert_eq!(s.app().hist.undo_len(), undo_depth + 1, "one undo step for the whole move");
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(id).unwrap().motion, before);
    s.cmd_key(Key::Y);
    assert_eq!(keys(&s, id, AnimProperty::Position), vec![250, 1250]);
    // a plain click on the bar does not edit
    let n = s.app().hist.undo_len();
    let bar = s.rect_of(&format!("motion-summary-{}", id.0));
    s.click(bar.center());
    assert_eq!(s.app().hist.undo_len(), n);
    // dragging left stops at zero
    let bar = s.rect_of(&format!("motion-summary-{}", id.0));
    let a = Pos2::new(bar.center().x, bar.center().y);
    s.drag(a, a - egui::vec2(px(900.0), 0.0));
    assert_eq!(keys(&s, id, AnimProperty::Position)[0], 0);
}

// ------------------------------------------------------------------ 6. scale lock

#[test]
fn scale_lock_toggles_and_scales_proportionally() {
    let (mut s, id, _) = nested();
    assert!(s.app().motion_ins.scale_locked(), "on by default, as in Figma");
    s.click(s.rect_of("mo-key:Scale").center());
    assert_eq!(keys(&s, id, AnimProperty::Scale), vec![0]);
    s.shell.session().motion.playhead_ms = 500;
    settle(&mut s);
    let x = s.field_rect("mo-sx");
    type_in(&mut s, x.center(), "50");
    assert_eq!(key_value(&s, id, AnimProperty::Scale, 500), AnimValue::Vec2(DV::new(0.5, 0.5)), "locked: both axes follow");
    // unlocked: only the edited axis changes
    s.click(s.rect_of("mo-scale-lock").center());
    assert!(!s.app().motion_ins.scale_locked());
    s.shell.session().motion.playhead_ms = 800;
    settle(&mut s);
    let y = s.field_rect("mo-sy");
    type_in(&mut s, y.center(), "200");
    let AnimValue::Vec2(v) = key_value(&s, id, AnimProperty::Scale, 800) else { panic!() };
    assert_eq!(v.x, 0.5, "unlocked: the other axis keeps the value of the previous key");
    assert_eq!(v.y, 2.0);
    // the ratio is kept when the axes differ
    assert_eq!(ins::proportional(DV::new(0.5, 1.0), 0, 1.0), DV::new(1.0, 2.0));
    assert_eq!(ins::proportional(DV::new(0.5, 1.0), 1, 2.0), DV::new(1.0, 2.0));
    assert_eq!(ins::proportional(DV::new(0.0, 1.0), 0, 0.3), DV::new(0.3, 0.3));
}

// ------------------------------------------------------------------ 7. several fills

#[test]
fn the_first_solid_fill_animates_among_several() {
    let (mut s, id, _) = nested();
    let mut n = s.app().doc.get(id).unwrap().clone();
    n.fills = vec![desigma_doc::Paint::solid(desigma_doc::Color::rgb(255, 0, 0)), desigma_doc::Paint::solid(desigma_doc::Color::rgb(0, 0, 255))];
    let c = desigma_doc::Command::Update { node: Box::new(n) };
    exec(&mut s, c);
    // the diamond belongs to the first solid row only
    s.click(s.rect_of("mo-key:Fill").center());
    assert_eq!(keys(&s, id, AnimProperty::FillColor), vec![0]);
    assert_eq!(key_value(&s, id, AnimProperty::FillColor, 0).color().unwrap().to_hex(), "FF0000");
    s.shell.session().motion.playhead_ms = 500;
    settle(&mut s);
    let hex = s.rect_of("hex:false:0");
    type_in(&mut s, hex.center(), "00FF00");
    assert_eq!(keys(&s, id, AnimProperty::FillColor), vec![0, 500]);
    // the other fill is a plain static edit
    let hex1 = s.rect_of("hex:false:1");
    type_in(&mut s, hex1.center(), "FFFF00");
    assert_eq!(keys(&s, id, AnimProperty::FillColor), vec![0, 500], "no keyframe for the second fill");
    let n = s.app().doc.get(id).unwrap();
    let PaintKind::Solid(c1) = n.fills[1].kind else { panic!() };
    assert_eq!(c1.to_hex(), "FFFF00");
    let PaintKind::Solid(c0) = n.fills[0].kind else { panic!() };
    assert_eq!(c0.to_hex(), "FF0000", "the stored first fill is untouched");
    let shown = s.app().shown_doc().get(id).unwrap();
    let PaintKind::Solid(c0) = shown.fills[0].kind else { panic!() };
    assert_eq!(c0.to_hex(), "00FF00", "the canvas shows the animated first fill");
    let PaintKind::Solid(c1) = shown.fills[1].kind else { panic!() };
    assert_eq!(c1.to_hex(), "FFFF00");
}
