//! Motion playback in the whole window: the playhead follows the clock, stops at the end, and
//! Present plays animated frames.

use super::*;

fn motion_sim() -> Sim {
    // the demo animation (position, rotation, opacity, Burst) on the first layer of the page
    Sim::with(Options { motion: true, ..Default::default() })
}

fn texture_ids(s: &Sim) -> std::collections::BTreeSet<egui::TextureId> {
    s.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Mesh(m) => Some(m.texture_id),
            _ => None,
        })
        .collect()
}

#[test]
fn playing_advances_the_playhead_with_the_clock_and_stops_at_the_end() {
    let mut s = motion_sim();
    let page = s.app().page;
    let tl = desigma_doc::motion::TimelineSettings { looping: false, ..s.app().doc.timeline(page) };
    if let Ok(cmd) = desigma_doc::motion::edit::set_timeline(&s.app().doc, page, tl) {
        let sess = s.shell.session();
        sess.hist.exec(&mut sess.doc, cmd).unwrap();
    }
    let duration = tl.duration_ms;
    s.shell.session().motion.playhead_ms = 0;
    s.shell.session().motion.playing = true;
    for _ in 0..10 {
        s.frame(vec![]);
    }
    // 10 frames of 50 ms
    let t = s.app().motion.playhead_ms;
    assert!((350..=650).contains(&t), "playhead {t} after ~0.5 s");
    assert!(s.app().motion.playing);
    for _ in 0..100 {
        s.frame(vec![]);
    }
    assert!(!s.app().motion.playing, "a one-shot timeline stops");
    assert_eq!(s.app().motion.playhead_ms, duration);
    // the stored document was never touched by playback
    assert!(s.app().doc.nodes.values().any(|n| !n.motion.is_empty()));
}

#[test]
fn looping_playback_wraps() {
    let mut s = motion_sim();
    let page = s.app().page;
    let tl = desigma_doc::motion::TimelineSettings { looping: true, ..s.app().doc.timeline(page) };
    if let Ok(cmd) = desigma_doc::motion::edit::set_timeline(&s.app().doc, page, tl) {
        let sess = s.shell.session();
        sess.hist.exec(&mut sess.doc, cmd).unwrap();
    }
    s.shell.session().motion.playhead_ms = tl.duration_ms - 100;
    s.shell.session().motion.playing = true;
    for _ in 0..8 {
        s.frame(vec![]);
    }
    assert!(s.app().motion.playing);
    assert!(s.app().motion.playhead_ms < tl.duration_ms - 100, "wrapped around: {}", s.app().motion.playhead_ms);
}

#[test]
fn present_plays_the_timeline_of_an_animated_frame() {
    let mut s = motion_sim();
    s.shell.session().motion.enabled = false;
    s.shell.session().start_present();
    assert!(s.app().proto.present.is_some());
    s.frame(vec![]);
    let mut seen = texture_ids(&s);
    for _ in 0..12 {
        s.frame(vec![]);
        seen.extend(texture_ids(&s));
    }
    // every timeline step is a new picture: the canvas would show one fixed texture
    assert!(seen.len() >= 4, "Present drew {} different pictures while the animation ran", seen.len());
}

#[test]
fn dev_code_includes_the_animation_of_animated_layers() {
    let s = motion_sim();
    let id = s.app().doc.get(s.app().page).unwrap().children[0];
    for (lang, needle) in
        [(crate::dev::Language::Css, "@keyframes"), (crate::dev::Language::SwiftUi, ".keyframeAnimator"), (crate::dev::Language::Compose, "keyframes {")]
    {
        let code = lang.generate(&s.app().doc, id, false);
        assert!(code.contains(needle), "{}: {code}", lang.label());
    }
    assert!(!crate::dev::Language::Tailwind.generate(&s.app().doc, id, false).contains("keyframes"));
}
