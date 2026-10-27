//! Whole-window slide workflows, including drafts crossing slide navigation.
use super::*;

#[test]
fn presentation_fullscreen_uses_native_window_and_escape_returns_to_preview() {
    let mut s = Sim::with(Options { fixture: Some("deck".into()), present: true, ..Default::default() });
    let original = s.app().doc.to_json().unwrap();
    let preview = s.app().proto.present.as_ref().unwrap().stage;
    let button = s.rect_of("present-fullscreen").center();
    s.mv(button);
    s.button(true);
    let ctx = s.ctx.clone();
    let out = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1088.0))),
            events: vec![Event::PointerButton { pos: button, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
            ..Default::default()
        },
        |ui| s.shell.draw(ui),
    );
    assert!(out.viewport_output.values().flat_map(|v| &v.commands).any(|c| matches!(c, egui::ViewportCommand::Fullscreen(true))));
    s.frame(vec![]);
    assert!(s.app().proto.present.as_ref().unwrap().fullscreen);
    assert!(s.app().proto.present.as_ref().unwrap().stage.width() > preview.width());
    s.key(Key::ArrowRight);
    assert_eq!(s.app().proto.present.as_ref().unwrap().player.current, s.app().deck_slides()[1]);
    s.key(Key::Escape);
    assert!(!s.app().proto.present.as_ref().unwrap().fullscreen);
    s.key(Key::F11);
    assert!(s.app().proto.present.as_ref().unwrap().fullscreen);
    s.key(Key::F11);
    assert!(!s.app().proto.present.as_ref().unwrap().fullscreen);
    s.key(Key::Escape);
    assert!(s.app().proto.present.is_none());
    assert_eq!(s.app().doc.to_json().unwrap(), original);
}

fn deck() -> Sim {
    let mut s = Sim::new();
    let ctx = s.ctx.clone();
    crate::panels::menus::run(s.shell.session(), &ctx, crate::panels::menus::Action::NewDeck);
    s.frame(vec![]);
    s.frame(vec![]);
    s
}

fn click_control(s: &mut Sim, name: &str) {
    s.click(s.rect_of(name).center());
}

fn slide_notes(s: &Sim, id: NodeId) -> &str {
    &s.app().doc.get(id).unwrap().slide.as_ref().unwrap().notes
}

#[test]
fn deck_slide_controls_create_reorder_delete_and_undo() {
    let mut s = deck();
    assert!(s.app().is_deck());
    let first = s.app().deck_slides()[0];
    assert_eq!(s.app().doc.get(first).unwrap().size, DV::new(1280.0, 720.0));
    assert_eq!(s.app().deck_slides().len(), 1);
    assert!(s.rect_of("deck-notes").top() >= s.app().canvas_rect.bottom());

    click_control(&mut s, "deck-add");
    let second = s.app().active_slide().unwrap();
    assert_ne!(second, first);
    assert_eq!(s.app().deck_slides(), vec![first, second]);

    click_control(&mut s, "deck-duplicate");
    let copy = s.app().active_slide().unwrap();
    assert_eq!(s.app().deck_slides(), vec![first, second, copy]);
    click_control(&mut s, "deck-up");
    assert_eq!(s.app().deck_slides(), vec![first, copy, second]);
    click_control(&mut s, "deck-down");
    assert_eq!(s.app().deck_slides(), vec![first, second, copy]);

    click_control(&mut s, "deck-delete");
    assert_eq!(s.app().deck_slides(), vec![first, second]);
    assert!(s.app().doc.get(copy).is_none());
    s.cmd_key(Key::Z);
    assert_eq!(s.app().deck_slides(), vec![first, second, copy]);
    s.cmd_key(Key::Y);
    assert_eq!(s.app().deck_slides(), vec![first, second]);

    click_control(&mut s, "deck-overview");
    assert!(s.app().decks.grid);
    click_control(&mut s, "deck-slide-1");
    assert_eq!(s.app().active_slide(), Some(first));
    assert!(!s.app().decks.grid);
    click_control(&mut s, "deck-layers-mode");
    assert!(s.app().decks.design_mode);
    click_control(&mut s, "deck-slides-mode");
    assert!(!s.app().decks.design_mode);
}

#[test]
fn deck_notes_navigation_commits_draft_and_undo_refreshes_editor() {
    let mut s = deck();
    let first = s.app().active_slide().unwrap();
    click_control(&mut s, "deck-add");
    let second = s.app().active_slide().unwrap();
    click_control(&mut s, "deck-slide-1");
    click_control(&mut s, "deck-notes");
    s.type_text("Explain the customer problem.");
    assert_eq!(slide_notes(&s, first), "", "typing remains a draft until Save or navigation");
    assert_eq!(s.app().decks.notes_draft, "Explain the customer problem.");

    click_control(&mut s, "deck-slide-2");
    assert_eq!(s.app().active_slide(), Some(second));
    assert_eq!(slide_notes(&s, first), "Explain the customer problem.");
    assert_eq!(slide_notes(&s, second), "");
    assert_eq!(s.app().decks.notes_draft, "");
    s.cmd_key(Key::Z);
    assert_eq!(slide_notes(&s, first), "", "one Undo reverts the complete note edit");
    assert_eq!(s.app().deck_slides().len(), 2, "Undo notes must not undo slide creation");

    click_control(&mut s, "deck-slide-1");
    assert_eq!(s.app().decks.notes_draft, "");
    s.cmd_key(Key::Y);
    assert_eq!(slide_notes(&s, first), "Explain the customer problem.");
    assert_eq!(s.app().decks.notes_draft, "Explain the customer problem.", "Redo refreshes the selected slide's notes field");
    click_control(&mut s, "deck-slide-2");
    assert_eq!(slide_notes(&s, first), "Explain the customer problem.", "navigation must not overwrite Redo with a stale draft");
}

#[test]
fn deck_notes_save_is_one_edit_and_typing_does_not_delete_slide() {
    let mut s = deck();
    let slide = s.app().active_slide().unwrap();
    click_control(&mut s, "deck-notes");
    s.type_text("Discard this draft text");
    s.cmd_key(Key::A);
    s.key(Key::Delete);
    assert!(s.app().doc.get(slide).is_some(), "Delete while typing must stay in the notes field");
    s.type_text("Opening remarks");
    click_control(&mut s, "deck-save");
    assert_eq!(slide_notes(&s, slide), "Opening remarks");
    // Selecting the same slide removes keyboard focus without changing history.
    click_control(&mut s, "deck-slide-1");
    s.cmd_key(Key::Z);
    assert_eq!(slide_notes(&s, slide), "");
    assert_eq!(s.app().decks.notes_draft, "", "saved text must follow document Undo");
    s.cmd_key(Key::Y);
    assert_eq!(slide_notes(&s, slide), "Opening remarks");
    assert_eq!(s.app().decks.notes_draft, "Opening remarks");
}
