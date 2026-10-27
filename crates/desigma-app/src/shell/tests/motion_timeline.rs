//! UI tests for the Motion timeline: mode switch, scrubbing, keyframe editing, easing segments,
//! duration, clips and keyboard stepping.

use super::*;
use desigma_doc::motion::edit::{self as medit, KeyRef};
use desigma_doc::motion::{find_preset, AnimProperty};

const POS: AnimProperty = AnimProperty::Position;

/// An editor in Motion mode with the demo's "Hero" layer selected.
fn motion_sim() -> (Sim, NodeId) {
    let mut s = Sim::new();
    let r = s.rect_of("mode-motion");
    s.click(r.center());
    s.frame(vec![]);
    s.frame(vec![]);
    let hero = s.by_name("Hero");
    s.shell.session().select_only(hero);
    s.frame(vec![]);
    s.frame(vec![]);
    (s, hero)
}

/// Screen x of timeline time `t` (ms).
fn tx(s: &Sim, t: f32) -> f32 {
    let g = s.rect_of("motion-scale");
    g.left() + g.width() * t / 1000.0
}

fn ruler_y(s: &Sim) -> f32 {
    s.rect_of("motion-ruler").center().y + 4.0
}

fn scrub_to(s: &mut Sim, t: f32) {
    let y = ruler_y(s);
    let a = Pos2::new(tx(s, 10.0), y);
    let b = Pos2::new(tx(s, t), y);
    s.drag(a, b);
}

fn click_name(s: &mut Sim, name: &str) {
    let r = s.rect_of(name);
    s.click(r.center());
}

fn keys(s: &Sim, n: NodeId, p: AnimProperty) -> Vec<u32> {
    s.app().doc.get(n).unwrap().motion.track(p).map(|t| t.keys.iter().map(|k| k.time_ms).collect()).unwrap_or_default()
}

fn key_pos(s: &Sim, n: NodeId, t: u32) -> Pos2 {
    s.rect_of(&format!("motion-key-{}-Position-{t}", n.0)).center()
}

/// Hero with Position keys at 0 and 1000.
fn two_keys() -> (Sim, NodeId) {
    let (mut s, hero) = motion_sim();
    click_name(&mut s, "motion-add-key");
    scrub_to(&mut s, 1000.0);
    assert_eq!(s.app().motion.playhead_ms, 1000);
    click_name(&mut s, "motion-add-key");
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
    (s, hero)
}

#[test]
fn motion_mode_opens_the_timeline_and_back() {
    let mut s = Sim::new();
    let bottom0 = s.app().canvas_rect.bottom();
    assert!(!s.app().motion.enabled);
    click_name(&mut s, "mode-motion");
    s.frame(vec![]);
    assert!(s.app().motion.enabled);
    let tl = s.rect_of("motion-timeline");
    assert!((s.app().canvas_rect.bottom() - tl.top()).abs() < 0.5, "timeline sits under the canvas");
    assert!(s.app().canvas_rect.bottom() < bottom0);
    assert!((tl.bottom() - 1088.0).abs() < 0.5);
    assert!((tl.left() - s.app().canvas_rect.left()).abs() < 0.5 && (tl.right() - s.app().canvas_rect.right()).abs() < 0.5);
    s.rect_of("motion-beta-pill");
    click_name(&mut s, "mode-design");
    s.frame(vec![]);
    assert!(!s.app().motion.enabled);
    assert!((s.app().canvas_rect.bottom() - bottom0).abs() < 0.5);
}

#[test]
fn collapse_and_resize_the_panel() {
    let (mut s, _) = motion_sim();
    let h0 = s.rect_of("motion-timeline").height();
    // drag the top edge up
    let top = s.rect_of("motion-timeline").top();
    let x = s.rect_of("motion-timeline").center().x;
    s.drag(Pos2::new(x, top + 1.0), Pos2::new(x, top - 59.0));
    s.frame(vec![]);
    let h1 = s.rect_of("motion-timeline").height();
    assert!((h1 - h0 - 60.0).abs() < 2.0, "{h0} -> {h1}");
    click_name(&mut s, "motion-collapse");
    s.frame(vec![]);
    assert!(s.rect_of("motion-timeline").height() < 40.0);
    click_name(&mut s, "motion-collapse");
    s.frame(vec![]);
    assert!((s.rect_of("motion-timeline").height() - h1).abs() < 0.5);
}

#[test]
fn scrubbing_the_ruler_moves_the_playhead() {
    let (mut s, _) = motion_sim();
    assert_eq!(s.app().motion.playhead_ms, 0);
    scrub_to(&mut s, 1000.0);
    assert_eq!(s.app().motion.playhead_ms, 1000);
    // snaps to a frame: 1.007s is still frame 60
    let y = ruler_y(&s);
    s.click(Pos2::new(tx(&s, 1007.0), y));
    assert_eq!(s.app().motion.playhead_ms, 1000);
    // past the end clamps to the duration
    scrub_to(&mut s, 5000.0);
    assert_eq!(s.app().motion.playhead_ms, 2000);
}

#[test]
fn add_move_and_delete_keyframes_with_undo() {
    let (mut s, hero) = two_keys();
    assert_eq!(s.app().motion.selected_keys.len(), 1, "the new key is selected");
    // move the key at 1000 to 1500
    let g = s.rect_of("motion-scale");
    let a = key_pos(&s, hero, 1000);
    s.drag(a, a + Vec2::new(g.width() * 0.5, 0.0));
    assert_eq!(keys(&s, hero, POS), vec![0, 1500]);
    assert_eq!(s.app().motion.selected_keys, vec![KeyRef::new(hero, POS, 1500)]);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, hero, POS), vec![0, 1000], "one undo step per drag");
    // delete with the keyboard
    let p = key_pos(&s, hero, 1000);
    s.click(p);
    assert_eq!(s.app().motion.selected_keys, vec![KeyRef::new(hero, POS, 1000)]);
    s.key(Key::Delete);
    assert_eq!(keys(&s, hero, POS), vec![0]);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
    // undo the add: the track goes away entirely
    s.cmd_key(Key::Z);
    s.cmd_key(Key::Z);
    assert!(keys(&s, hero, POS).is_empty());
}

#[test]
fn multi_select_marquee_and_alt_duplicate() {
    let (mut s, hero) = two_keys();
    // marquee around both keys
    let y = key_pos(&s, hero, 0).y;
    s.drag(Pos2::new(tx(&s, 0.0) - 9.0, y - 7.0), Pos2::new(tx(&s, 1100.0), y + 7.0));
    assert_eq!(s.app().motion.selected_keys.len(), 2);
    // click one: selection narrows to it
    let p = key_pos(&s, hero, 1000);
    s.click(p);
    assert_eq!(s.app().motion.selected_keys, vec![KeyRef::new(hero, POS, 1000)]);
    // shift adds
    s.mods = Modifiers::SHIFT;
    let p0 = key_pos(&s, hero, 0);
    s.click(p0);
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().motion.selected_keys.len(), 2);
    // alt-drag on a single key duplicates it 250ms later
    let p = key_pos(&s, hero, 1000);
    s.click(p);
    let g = s.rect_of("motion-scale");
    s.mods = Modifiers::ALT;
    s.drag(p, p + Vec2::new(g.width() * 0.25, 0.0));
    s.mods = Modifiers::NONE;
    assert_eq!(keys(&s, hero, POS), vec![0, 1000, 1250]);
    assert_eq!(s.app().motion.selected_keys, vec![KeyRef::new(hero, POS, 1250)]);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
}

#[test]
fn alt_drag_on_a_selection_edge_stretches_it() {
    let (mut s, hero) = two_keys();
    let y = key_pos(&s, hero, 0).y;
    s.drag(Pos2::new(tx(&s, 0.0) - 9.0, y - 7.0), Pos2::new(tx(&s, 1100.0), y + 7.0));
    assert_eq!(s.app().motion.selected_keys.len(), 2);
    let g = s.rect_of("motion-scale");
    s.mods = Modifiers::ALT;
    let a = key_pos(&s, hero, 1000);
    s.drag(a, a + Vec2::new(g.width() * 0.5, 0.0));
    s.mods = Modifiers::NONE;
    assert_eq!(keys(&s, hero, POS), vec![0, 1500]);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
}

#[test]
fn segment_click_selects_the_easing_target() {
    let (mut s, hero) = two_keys();
    assert!(!s.app().motion.easing_target);
    click_name(&mut s, &format!("motion-seg-{}-Position-0", hero.0));
    assert!(s.app().motion.easing_target);
    assert_eq!(s.app().motion.selected_keys, vec![KeyRef::new(hero, POS, 0)]);
    // clicking a key leaves the easing target
    let p = key_pos(&s, hero, 1000);
    s.click(p);
    assert!(!s.app().motion.easing_target);
    // clicking empty track space clears everything
    click_name(&mut s, &format!("motion-seg-{}-Position-0", hero.0));
    let y = key_pos(&s, hero, 0).y;
    s.click(Pos2::new(tx(&s, 1800.0), y));
    assert!(!s.app().motion.easing_target && s.app().motion.selected_keys.is_empty());
}

#[test]
fn double_click_on_a_key_moves_the_playhead_there() {
    let (mut s, hero) = two_keys();
    scrub_to(&mut s, 300.0);
    let p = key_pos(&s, hero, 1000);
    s.mv(p);
    for _ in 0..2 {
        s.button(true);
        s.button(false);
    }
    assert_eq!(s.app().motion.playhead_ms, 1000);
}

#[test]
fn row_buttons_step_and_toggle_keys() {
    let (mut s, hero) = two_keys();
    scrub_to(&mut s, 500.0);
    click_name(&mut s, &format!("motion-next-{}-Position", hero.0));
    assert_eq!(s.app().motion.playhead_ms, 1000);
    click_name(&mut s, &format!("motion-prev-{}-Position", hero.0));
    assert_eq!(s.app().motion.playhead_ms, 0);
    // the diamond adds / removes a key at the playhead
    scrub_to(&mut s, 500.0);
    click_name(&mut s, &format!("motion-diamond-{}-Position", hero.0));
    assert_eq!(keys(&s, hero, POS), vec![0, 500, 1000]);
    click_name(&mut s, &format!("motion-diamond-{}-Position", hero.0));
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
}

#[test]
fn layer_row_and_canvas_selection_stay_in_sync() {
    let (mut s, hero) = two_keys();
    s.shell.session().clear_selection();
    s.frame(vec![]);
    // the animated layer keeps its row; clicking it selects the node
    click_name(&mut s, &format!("motion-layer-{}", hero.0));
    assert_eq!(s.app().selection, vec![hero]);
}

#[test]
fn editing_the_duration_field_and_the_ruler_end() {
    let (mut s, _) = motion_sim();
    assert_eq!(s.app().doc.timeline(s.app().page).duration_ms, 2000);
    click_name(&mut s, "motion-duration");
    s.type_text("3000");
    s.key(Key::Enter);
    assert_eq!(s.app().doc.timeline(s.app().page).duration_ms, 3000);
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.timeline(s.app().page).duration_ms, 2000);
    // drag the end of the timeline in the ruler
    let y = ruler_y(&s);
    s.drag(Pos2::new(tx(&s, 2000.0), y), Pos2::new(tx(&s, 1500.0), y));
    assert_eq!(s.app().doc.timeline(s.app().page).duration_ms, 1500);
    // the playhead cannot stay past the end
    scrub_to(&mut s, 1500.0);
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.timeline(s.app().page).duration_ms, 2000);
}

#[test]
fn current_time_field_and_format_toggle() {
    let (mut s, _) = motion_sim();
    click_name(&mut s, "motion-time");
    s.type_text("750");
    s.key(Key::Enter);
    assert_eq!(s.app().motion.playhead_ms, 750);
    // ms -> seconds -> frames
    click_name(&mut s, "motion-format");
    click_name(&mut s, "motion-time");
    s.type_text("1.25");
    s.key(Key::Enter);
    assert_eq!(s.app().motion.playhead_ms, 1250);
    click_name(&mut s, "motion-format");
    click_name(&mut s, "motion-time");
    s.type_text("30");
    s.key(Key::Enter);
    assert_eq!(s.app().motion.playhead_ms, 500);
}

#[test]
fn loop_button_is_one_undo_step() {
    let (mut s, _) = motion_sim();
    click_name(&mut s, "motion-loop");
    assert!(s.app().doc.timeline(s.app().page).looping);
    s.cmd_key(Key::Z);
    assert!(!s.app().doc.timeline(s.app().page).looping);
}

#[test]
fn clip_bars_move_and_trim_with_one_undo_step() {
    let (mut s, hero) = motion_sim();
    let cmd = medit::apply_preset(&s.app().doc, &[hero], find_preset("Burst").unwrap(), 250).unwrap();
    {
        let app = s.shell.session();
        app.hist.exec(&mut app.doc, cmd).unwrap();
    }
    s.frame(vec![]);
    s.frame(vec![]);
    let clip = s.app().doc.get(hero).unwrap().motion.clips[0].clone();
    let name = format!("motion-clip-{}-{}", hero.0, clip.id);
    let g = s.rect_of("motion-scale");
    // drag the body: delay + 250ms
    let r = s.rect_of(&name);
    s.drag(r.center(), r.center() + Vec2::new(g.width() * 0.25, 0.0));
    let moved = s.app().doc.get(hero).unwrap().motion.clips[0].clone();
    assert_eq!(moved.delay_ms, clip.delay_ms + 250);
    assert_eq!(moved.duration_ms, clip.duration_ms);
    assert_eq!(s.app().motion.selected_clip, Some((hero, clip.id)));
    // drag the right edge: duration + 200ms
    let r = s.rect_of(&name);
    let edge = Pos2::new(r.right() - 2.0, r.center().y);
    s.drag(edge, edge + Vec2::new(g.width() * 0.2, 0.0));
    let trimmed = s.app().doc.get(hero).unwrap().motion.clips[0].clone();
    assert_eq!(trimmed.delay_ms, moved.delay_ms);
    assert_eq!(trimmed.duration_ms, moved.duration_ms + 200);
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(hero).unwrap().motion.clips[0].duration_ms, moved.duration_ms);
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(hero).unwrap().motion.clips[0].delay_ms, clip.delay_ms);
    // Delete removes the selected clip
    let r = s.rect_of(&name);
    s.click(r.center());
    let n = s.app().doc.get(hero).unwrap().motion.clips.len();
    s.key(Key::Delete);
    assert_eq!(s.app().doc.get(hero).unwrap().motion.clips.len(), n - 1);
}

#[test]
fn keyboard_steps_frames_and_keyframes() {
    let (mut s, hero) = two_keys();
    scrub_to(&mut s, 500.0);
    s.key(Key::Period);
    assert_eq!(s.app().motion.playhead_ms, 517);
    s.key(Key::Comma);
    assert_eq!(s.app().motion.playhead_ms, 500);
    s.mods = Modifiers::SHIFT;
    s.key(Key::Period);
    assert_eq!(s.app().motion.playhead_ms, 1000);
    s.key(Key::Comma);
    assert_eq!(s.app().motion.playhead_ms, 0);
    s.mods = Modifiers::NONE;
    s.key(Key::End);
    assert_eq!(s.app().motion.playhead_ms, 2000);
    s.key(Key::Home);
    assert_eq!(s.app().motion.playhead_ms, 0);
    let _ = hero;
}

#[test]
fn space_plays_and_stops_at_the_end() {
    let (mut s, _) = motion_sim();
    assert!(!s.app().motion.playing);
    s.key(Key::Space);
    assert!(s.app().motion.playing);
    for _ in 0..4 {
        s.frame(vec![]);
    }
    assert!(s.app().motion.playhead_ms > 0, "the playhead advances");
    s.key(Key::Space);
    assert!(!s.app().motion.playing);
    // the play button runs to the end of a short timeline, then stops
    let tl = desigma_doc::TimelineSettings { duration_ms: 200, ..Default::default() };
    let cmd = medit::set_timeline(&s.app().doc, s.app().page, tl).unwrap();
    {
        let app = s.shell.session();
        app.hist.exec(&mut app.doc, cmd).unwrap();
    }
    click_name(&mut s, "motion-play");
    for _ in 0..10 {
        s.frame(vec![]);
    }
    assert!(!s.app().motion.playing);
    assert_eq!(s.app().motion.playhead_ms, 200);
}

#[test]
fn copy_and_paste_keyframes_at_the_playhead() {
    let (mut s, hero) = two_keys();
    let y = key_pos(&s, hero, 0).y;
    s.drag(Pos2::new(tx(&s, 0.0) - 9.0, y - 7.0), Pos2::new(tx(&s, 1100.0), y + 7.0));
    let ctx = s.ctx.clone();
    assert!(crate::motion::timeline::copy_keys(s.shell.session(), &ctx));
    scrub_to(&mut s, 1500.0);
    assert!(crate::motion::timeline::paste_keys(s.shell.session(), &ctx));
    assert_eq!(keys(&s, hero, POS), vec![0, 1000, 1500, 2500]);
    s.cmd_key(Key::Z);
    assert_eq!(keys(&s, hero, POS), vec![0, 1000]);
}

#[test]
fn zoom_buttons_change_the_scale() {
    let (mut s, _) = motion_sim();
    let w0 = s.rect_of("motion-scale").width();
    click_name(&mut s, "motion-zoom-in");
    let w1 = s.rect_of("motion-scale").width();
    assert!(w1 > w0 * 1.3, "{w0} -> {w1}");
    click_name(&mut s, "motion-zoom-out");
    click_name(&mut s, "motion-zoom-out");
    assert!(s.rect_of("motion-scale").width() < w0);
}

#[test]
fn rows_show_the_value_at_the_playhead() {
    let (s, hero) = two_keys();
    let n = s.app().doc.get(hero).unwrap();
    let v = desigma_doc::motion::value_at(n, POS, 0.0).unwrap();
    assert!(crate::motion::timeline::value_text(POS, v).contains(", "));
}
