//! UI tests for live collaboration: the Share button and popover, avatars, the Join dialog and the
//! banner, driven through the whole window over the in-memory transport.

use super::*;
use crate::collab::tests::{connect, MemHost, Net};
use crate::collab::{link::Closed, link::GuestEv, link::HostEv};
use desigma_collab::Role;

fn click_named(s: &mut Sim, name: &str) {
    let r = s.rect_of(name);
    s.click(r.center());
}

fn shared() -> (Sim, Net) {
    let mut s = Sim::new();
    let net = Net::default();
    s.shell.session().start_sharing_with(Box::new(MemHost(net.clone())), false, false, Role::Editor);
    net.borrow_mut().host.push_back(HostEv::Ready { link: "desigma://join/abcdefghijklmnop".into() });
    for _ in 0..4 {
        s.frame(vec![]);
    }
    (s, net)
}

#[test]
fn idle_editor_has_a_share_button_that_opens_the_start_popover() {
    let mut s = Sim::new();
    let share = s.rect_of("collab-share");
    assert!(share.width() > 30.0 && share.right() <= s.rect_of("present-button").left(), "Share sits left of Present");
    click_named(&mut s, "collab-share");
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(s.app().collab.share.open);
    for name in ["collab-password", "collab-access", "collab-approval", "collab-start"] {
        s.rect_of(name);
    }
    // choosing view only and approval is remembered for the start
    let access = s.rect_of("collab-access");
    s.click(egui::pos2(access.right() - 20.0, access.center().y));
    click_named(&mut s, "collab-approval");
    assert!(s.app().collab.share.view_only && s.app().collab.share.approval);
}

#[test]
fn hosting_shows_the_link_copy_people_and_kick() {
    let (mut s, net) = shared();
    let guest = connect(&net, "Bea");
    s.frame(vec![]);
    s.frame(vec![]);
    drop(guest);
    // the Share button now reads Sharing and the avatar row has the host and the guest
    s.rect_of("collab-avatar-0");
    s.rect_of("collab-avatar-1");
    click_named(&mut s, "collab-share");
    s.frame(vec![]);
    s.frame(vec![]);
    s.rect_of("collab-link");
    click_named(&mut s, "collab-copy");
    assert_eq!(s.app().toast.as_ref().map(|t| t.text.as_str()), Some("Link copied"));
    click_named(&mut s, "collab-kick-1");
    s.frame(vec![]);
    assert!(s.app().collab.host.as_ref().unwrap().peers.is_empty());
    assert!(matches!(net.borrow_mut().guests.get_mut(&1).unwrap().back(), Some(GuestEv::Closed(Closed::Kicked))));
    click_named(&mut s, "collab-stop");
    s.frame(vec![]);
    assert!(!s.app().collab.hosting());
}

#[test]
fn an_approval_request_shows_accept_and_reject() {
    let (mut s, net) = shared();
    net.borrow_mut().host.push_back(HostEv::Request { id: 3, name: "Dee".into() });
    s.frame(vec![]);
    s.frame(vec![]);
    s.rect_of("collab-reject");
    click_named(&mut s, "collab-accept");
    s.frame(vec![]);
    assert!(s.app().collab.requests.is_empty());
}

#[test]
fn clicking_an_avatar_follows_and_pressing_escape_stops() {
    let (mut s, net) = shared();
    let _guest = connect(&net, "Bea");
    for _ in 0..3 {
        s.frame(vec![]);
    }
    // the guest's presence arrives (the mem guest here is not a running editor)
    let p =
        crate::collab::presence::Presence { peer: 1, name: "Bea".into(), page: s.app().page.0, cursor: None, sel: vec![], view: [0.0, 0.0, 1.0], gone: false };
    let now = s.app().now;
    crate::collab::presence::apply(&mut s.shell.session().collab.remotes, p, now);
    click_named(&mut s, "collab-avatar-1");
    assert_eq!(s.app().collab.following, Some(1));
    click_named(&mut s, "collab-avatar-1");
    assert_eq!(s.app().collab.following, None);
}

#[test]
fn the_join_dialog_explains_a_bad_link_and_keeps_the_name() {
    let mut s = Sim::new();
    s.shell.session().open_join(None);
    s.frame(vec![]);
    s.frame(vec![]);
    s.rect_of("collab-join-link");
    s.rect_of("collab-join-name");
    s.type_text("not a link");
    s.key(Key::Enter);
    s.frame(vec![]);
    let err = s.app().collab.join.as_ref().and_then(|j| j.error.clone()).expect("an error is shown");
    assert!(err.contains("desigma://join/"), "{err}");
    assert!(s.app().collab.joining.is_none());
    click_named(&mut s, "collab-join-cancel");
    s.frame(vec![]);
    assert!(s.app().collab.join.is_none());
}

#[test]
fn a_link_from_the_command_line_fills_the_join_dialog() {
    let s = Sim::with(Options { join: Some("desigma://join/abc".into()), ..Default::default() });
    assert_eq!(s.app().collab.join.as_ref().map(|j| j.link.as_str()), Some("desigma://join/abc"));
}

#[test]
fn the_home_screen_and_file_menu_offer_join_session() {
    let mut s = Sim::new();
    crate::panels::menus::run(s.shell.session(), &s.ctx.clone(), crate::panels::menus::Action::JoinSession);
    s.frame(vec![]);
    assert!(s.app().collab.join.is_some());
}

#[test]
fn a_guest_sees_the_ended_banner_with_save_a_copy() {
    let (mut host, net) = shared();
    let mut guest = Sim::new();
    guest.shell.session().join_with(Box::new(connect(&net, "Bea")));
    for _ in 0..8 {
        host.frame(vec![]);
        guest.frame(vec![]);
    }
    assert!(guest.app().collab.is_guest());
    assert!(guest.app().home.is_none() && guest.app().file_path.is_none());
    // a guest has Leave instead of Share
    guest.rect_of("collab-share");
    net.borrow_mut().guests.get_mut(&1).unwrap().push_back(GuestEv::Closed(Closed::HostEnded));
    guest.frame(vec![]);
    guest.frame(vec![]);
    guest.rect_of("collab-banner");
    let save = guest.rect_of("collab-save-copy");
    assert!(save.width() > 20.0);
    // the copy is intact and editable: the document stays open
    assert!(guest.app().doc.get(guest.app().page).is_some() && !guest.app().read_only());
}

#[test]
fn view_only_guests_see_a_notice_and_cannot_move_things() {
    let mut host = Sim::new();
    let net = Net::default();
    host.shell.session().start_sharing_with(Box::new(MemHost(net.clone())), false, false, Role::Viewer);
    let mut guest = Sim::new();
    guest.shell.session().join_with(Box::new(connect(&net, "Bea")));
    for _ in 0..8 {
        host.frame(vec![]);
        guest.frame(vec![]);
    }
    guest.rect_of("collab-banner");
    assert!(guest.app().read_only());
    let before = guest.app().doc.nodes.values().map(|n| n.transform).collect::<Vec<_>>();
    guest.key(Key::A);
    guest.cmd_key(Key::A);
    guest.key(Key::ArrowRight);
    let after = guest.app().doc.nodes.values().map(|n| n.transform).collect::<Vec<_>>();
    assert_eq!(before, after);
}

#[test]
fn going_back_to_files_while_hosting_asks_how_many_people_will_be_disconnected() {
    let (mut s, net) = shared();
    let _guest = connect(&net, "Bea");
    for _ in 0..3 {
        s.frame(vec![]);
    }
    crate::panels::menus::run(s.shell.session(), &s.ctx.clone(), crate::panels::menus::Action::BackToFiles);
    s.frame(vec![]);
    s.frame(vec![]);
    let d = s.app().dialog.as_ref().expect("a question");
    assert_eq!(d.title, "Stop sharing?");
    assert_eq!(d.lines[0].0, "1 person will be disconnected.");
    // Cancel keeps sharing
    click_named(&mut s, "dialog-cancel");
    s.frame(vec![]);
    assert!(s.app().collab.hosting() && s.app().dialog.is_none());
    // Stop sharing ends it
    crate::panels::menus::run(s.shell.session(), &s.ctx.clone(), crate::panels::menus::Action::BackToFiles);
    s.frame(vec![]);
    s.frame(vec![]);
    click_named(&mut s, "dialog-ok");
    s.frame(vec![]);
    assert!(!s.app().collab.active());
}

#[test]
fn a_reconnecting_guest_sees_a_banner_without_save_a_copy_until_it_gives_up() {
    let (mut host, net) = shared();
    let mut guest = Sim::new();
    let (redial, _) = crate::collab::tests::redialing(&net, "Bea");
    guest.shell.session().join_with_redial(Box::new(connect(&net, "Bea")), Some(redial));
    for _ in 0..8 {
        host.frame(vec![]);
        guest.frame(vec![]);
    }
    assert!(guest.app().collab.is_guest());
    net.borrow_mut().guests.get_mut(&1).unwrap().push_back(GuestEv::Closed(Closed::Lost("network down".into())));
    guest.frame(vec![]);
    guest.frame(vec![]);
    assert!(guest.app().collab.guest.as_ref().unwrap().reconnect.is_some());
    guest.rect_of("collab-banner");
    let has = |s: &Sim, name: &str| s.ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("test-rect", name.to_string())))).is_some();
    guest.frame(vec![]);
    assert!(!has(&guest, "collab-save-copy"), "nothing to save yet: the connection may come back");
}
