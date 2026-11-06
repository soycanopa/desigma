//! Two editors in one process: through in-memory channels (fast, deterministic) and, in
//! `over_real_network`, through desigma-net on loopback.

use super::link::{Closed, GuestEv, GuestLink, HostEv, HostLink};
use super::*;
use crate::app::App;
use desigma_doc::{Document, Node, NodeId, Rect};
use desigma_fonts::{FontError, FontReport, FontRequest};
use desigma_net::JoinError;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct Wire {
    pub host: VecDeque<HostEv>,
    pub guests: BTreeMap<u32, VecDeque<GuestEv>>,
    next: u32,
    answered: Vec<(u64, bool)>,
    /// Resume secret each connecting guest shows.
    pub shown: BTreeMap<u32, String>,
    /// Secrets the host let back in without asking.
    pub allowed: Vec<String>,
    /// What the transport says the link is right now.
    pub link: Option<String>,
    /// Every reliable message the guests sent, as (peer, bytes).
    pub sent: Vec<(u32, Vec<u8>)>,
    /// Every reliable message the host sent, as (peer, bytes).
    pub down: Vec<(u32, Vec<u8>)>,
}
pub(crate) type Net = Rc<RefCell<Wire>>;

pub(crate) struct MemHost(pub(crate) Net);
impl HostLink for MemHost {
    fn drain(&self) -> Vec<HostEv> {
        self.0.borrow_mut().host.drain(..).collect()
    }
    fn respond(&self, id: u64, accept: bool) {
        self.0.borrow_mut().answered.push((id, accept));
    }
    fn send(&self, peer: u32, bytes: Vec<u8>) {
        let mut w = self.0.borrow_mut();
        w.down.push((peer, bytes.clone()));
        if let Some(q) = w.guests.get_mut(&peer) {
            q.push_back(GuestEv::Msg(bytes));
        }
    }
    fn send_presence(&self, peer: u32, bytes: &[u8]) {
        if let Some(q) = self.0.borrow_mut().guests.get_mut(&peer) {
            q.push_back(GuestEv::Presence(bytes.to_vec()));
        }
    }
    fn kick(&self, peer: u32) {
        if let Some(q) = self.0.borrow_mut().guests.get_mut(&peer) {
            q.push_back(GuestEv::Closed(Closed::Kicked));
        }
    }
    fn link(&self) -> Option<String> {
        self.0.borrow().link.clone()
    }
    fn join_token(&self, peer: u32) -> Option<String> {
        self.0.borrow_mut().shown.remove(&peer)
    }
    fn allow_resume(&self, token: &str) {
        self.0.borrow_mut().allowed.push(token.to_string());
    }
}

pub(crate) struct MemGuest(pub(crate) Net, pub(crate) u32);
impl GuestLink for MemGuest {
    fn drain(&self) -> Vec<GuestEv> {
        self.0.borrow_mut().guests.get_mut(&self.1).map(|q| q.drain(..).collect()).unwrap_or_default()
    }
    fn send(&self, bytes: Vec<u8>) {
        let mut w = self.0.borrow_mut();
        w.sent.push((self.1, bytes.clone()));
        w.host.push_back(HostEv::Msg { peer: self.1, bytes });
    }
    fn send_presence(&self, bytes: &[u8]) {
        self.0.borrow_mut().host.push_back(HostEv::Presence { peer: self.1, bytes: bytes.to_vec() });
    }
    fn leave(&self) {
        self.0.borrow_mut().host.push_back(HostEv::Left { peer: self.1 });
    }
}

pub(crate) fn connect(net: &Net, name: &str) -> MemGuest {
    connect_with(net, name, None)
}

/// A guest dials in, showing `token` when it is coming back.
pub(crate) fn connect_with(net: &Net, name: &str, token: Option<&str>) -> MemGuest {
    let mut w = net.borrow_mut();
    w.next += 1;
    let peer = w.next;
    w.guests.insert(peer, VecDeque::from([GuestEv::Joined]));
    if let Some(t) = token {
        w.shown.insert(peer, t.to_string());
    }
    w.host.push_back(HostEv::Joined { peer, name: name.into() });
    MemGuest(net.clone(), peer)
}

/// The connection of guest `peer` breaks without a goodbye: both ends find out on their own.
fn drop_connection(net: &Net, peer: u32) {
    let mut w = net.borrow_mut();
    if let Some(q) = w.guests.get_mut(&peer) {
        q.clear();
        q.push_back(GuestEv::Closed(Closed::Lost("network down".into())));
    }
    w.host.retain(|e| !matches!(e, HostEv::Msg { peer: p, .. } | HostEv::Presence { peer: p, .. } if *p == peer));
    w.host.push_back(HostEv::Left { peer });
}

/// A guest on `net` that dials again the way the real one does, counting the attempts.
pub(crate) fn redialing(net: &Net, name: &str) -> (link::Redial, Rc<std::cell::Cell<usize>>) {
    let (net, name, tries) = (net.clone(), name.to_string(), Rc::new(std::cell::Cell::new(0)));
    let count = tries.clone();
    let redial: link::Redial = Box::new(move |token| {
        count.set(count.get() + 1);
        Ok(Box::new(connect_with(&net, &name, token)) as Box<dyn GuestLink>)
    });
    (redial, tries)
}

fn doc() -> (Document, NodeId, NodeId) {
    let mut d = Document::new("Shared");
    let page = d.add_page("Page 1");
    let a = d.alloc_id();
    d.insert(page, Node::rectangle(a, Rect::new(0.0, 0.0, 100.0, 80.0))).unwrap();
    let b = d.alloc_id();
    d.insert(page, Node::rectangle(b, Rect::new(200.0, 0.0, 50.0, 50.0))).unwrap();
    (d, a, b)
}

struct Rig {
    host: App,
    guest: App,
    net: Net,
    now: f64,
    a: NodeId,
    b: NodeId,
}

impl Rig {
    fn new(role: Role) -> Rig {
        let (d, a, b) = doc();
        Rig::with_doc(role, d, a, b)
    }

    fn with_doc(role: Role, d: Document, a: NodeId, b: NodeId) -> Rig {
        let mut host = App::new(d);
        let net = Net::default();
        host.start_sharing_with(Box::new(MemHost(net.clone())), false, false, role);
        let mut guest = App::new(Document::new("Empty guest window"));
        guest.collab.images = cache_in("guest");
        let (redial, _) = redialing(&net, "Bea");
        guest.join_with_redial(Box::new(connect(&net, "Bea")), Some(redial));
        let mut r = Rig { host, guest, net, now: 1.0, a, b };
        r.settle();
        assert!(r.guest.collab.is_guest(), "guest joined");
        r
    }

    /// A few frames on both sides, 100 ms apart.
    fn settle(&mut self) {
        for _ in 0..8 {
            self.now += 0.1;
            frame(&mut self.host, self.now);
            frame(&mut self.guest, self.now);
        }
    }

    fn name(app: &App, id: NodeId) -> String {
        app.doc.get(id).map(|n| n.name.clone()).unwrap_or_else(|| "<gone>".into())
    }
}

/// One editor frame as far as the session is concerned (the canvas render consumes `dirty`).
fn frame(app: &mut App, now: f64) {
    app.now = now;
    pump_with(app, now, &Local::default());
    crate::canvas::tiles::take_rev(app);
}

#[test]
fn the_shared_file_opens_on_the_guest() {
    let r = Rig::new(Role::Editor);
    assert_eq!(r.guest.doc.name, "Shared");
    assert_eq!(r.guest.doc.nodes.len(), r.host.doc.nodes.len());
    assert!(r.guest.file_path.is_none() && r.guest.lib_file.is_none(), "a shared file is not tied to a path");
    assert_eq!(r.guest.doc.id_peer(), Some(1));
    // ids the guest hands out come from its own range
    let mut g = r.guest;
    assert_eq!(g.doc.alloc_id().0 >> 40, 1);
    assert_eq!(r.host.doc.id_peer(), Some(0));
}

#[test]
fn edits_travel_both_ways() {
    let mut r = Rig::new(Role::Editor);
    let a = r.a;
    r.host.edit_nodes(&[a], |n| n.name = "From host".into());
    r.settle();
    assert_eq!(Rig::name(&r.guest, a), "From host");

    let b = r.b;
    r.guest.edit_nodes(&[b], |n| n.name = "From guest".into());
    r.settle();
    assert_eq!(Rig::name(&r.host, b), "From guest");
    assert!(r.host.canvas.tiles.rev() > 0, "the host canvas was told to redraw");
}

#[test]
fn nodes_created_by_a_guest_use_its_id_range_and_reach_the_host() {
    let mut r = Rig::new(Role::Editor);
    let page = r.guest.page;
    let id = r.guest.doc.alloc_id();
    let mut cmd = Node::rectangle(id, Rect::new(5.0, 5.0, 10.0, 10.0));
    cmd.name = "New".into();
    let mut tx = crate::edit::Tx::new();
    tx.cmd(&mut r.guest.doc, desigma_doc::Command::Insert { parent: page, index: 0, nodes: vec![cmd] }).unwrap();
    r.guest.run_tx(tx);
    r.settle();
    assert_eq!(Rig::name(&r.host, id), "New");
    assert_eq!(id.0 >> 40, 1);
}

#[test]
fn presence_round_trips_and_leaving_removes_it() {
    let mut r = Rig::new(Role::Editor);
    let a = r.a;
    r.host.selection = vec![a];
    // the guest's pointer is over its canvas at (300, 200) on screen
    let pointer = r.guest.canvas_rect.min + egui::vec2(300.0, 200.0);
    for _ in 0..4 {
        r.now += 0.1;
        frame(&mut r.host, r.now);
        pump_with(&mut r.guest, r.now, &Local { pointer: Some(pointer), esc: false });
        crate::canvas::tiles::take_rev(&mut r.guest);
    }
    let seen = &r.guest.collab.remotes[&0];
    assert_eq!(seen.p.sel, vec![a.0], "the host's selection reached the guest");
    assert!(seen.p.name.len() > 1);
    let theirs = &r.host.collab.remotes[&1];
    let want = r.guest.view.to_page(r.guest.canvas_rect.min, pointer);
    let got = theirs.p.cursor.expect("cursor");
    assert!((got[0] - want.x).abs() < 0.2 && (got[1] - want.y).abs() < 0.2);
    assert_eq!(theirs.p.page, r.guest.page.0);

    // the guest leaves: its cursor goes away on the host
    r.guest.leave_session();
    r.settle();
    assert!(r.host.collab.remotes.is_empty());
    assert!(r.host.collab.host.as_ref().unwrap().peers.is_empty());
}

#[test]
fn a_third_person_sees_a_guest_through_the_host() {
    let mut r = Rig::new(Role::Editor);
    let mut carol = App::new(Document::new("c"));
    carol.join_with(Box::new(connect(&r.net, "Cy")));
    for _ in 0..8 {
        r.now += 0.1;
        frame(&mut r.host, r.now);
        frame(&mut r.guest, r.now);
        frame(&mut carol, r.now);
    }
    assert!(carol.collab.is_guest());
    assert!(carol.collab.remotes.contains_key(&0) && carol.collab.remotes.contains_key(&1), "host and the other guest are both visible");
    assert_eq!(carol.collab.remotes[&1].p.name, r.guest.collab_name());
}

#[test]
fn view_only_guests_cannot_edit() {
    let mut r = Rig::new(Role::Viewer);
    assert!(r.guest.collab.view_only());
    assert!(r.guest.read_only());
    let a = r.a;
    // through the editor's own edit path: refused outright
    r.guest.edit_nodes(&[a], |n| n.name = "Nope".into());
    r.settle();
    assert_eq!(Rig::name(&r.guest, a), Rig::name(&r.host, a));
    assert_ne!(Rig::name(&r.host, a), "Nope");
    // a change forced into the document anyway is refused by the host and corrected
    r.guest.doc.get_mut(a).unwrap().name = "Sneaky".into();
    r.guest.changed();
    r.settle();
    assert_ne!(Rig::name(&r.host, a), "Sneaky");
    assert_ne!(Rig::name(&r.guest, a), "Sneaky", "the guest was corrected");
    // the host still reaches the guest
    r.host.edit_nodes(&[a], |n| n.name = "Host".into());
    r.settle();
    assert_eq!(Rig::name(&r.guest, a), "Host");
}

#[test]
fn the_host_can_switch_a_guest_to_view_only_and_back() {
    let mut r = Rig::new(Role::Editor);
    assert!(!r.guest.read_only());
    r.host.set_all_roles(Role::Viewer);
    r.settle();
    assert!(r.guest.read_only());
    r.host.set_peer_role(1, Role::Editor);
    r.settle();
    assert!(!r.guest.read_only());
}

#[test]
fn a_guests_undo_does_not_revert_a_host_edit() {
    let mut r = Rig::new(Role::Editor);
    let a = r.a;
    let before = Rig::name(&r.host, a);
    r.guest.edit_nodes(&[a], |n| n.name = "Guest".into());
    r.settle();
    r.host.edit_nodes(&[a], |n| n.name = "Host later".into());
    r.settle();
    assert_eq!(Rig::name(&r.guest, a), "Host later");
    r.guest.undo();
    r.settle();
    assert_eq!(Rig::name(&r.guest, a), "Host later", "undo kept the newer host value");
    assert_eq!(Rig::name(&r.host, a), "Host later");
    assert_ne!(before, "Host later");
}

#[test]
fn a_hosts_undo_does_not_revert_a_guest_edit() {
    let mut r = Rig::new(Role::Editor);
    let a = r.a;
    r.host.edit_nodes(&[a], |n| n.name = "Host".into());
    r.settle();
    r.guest.edit_nodes(&[a], |n| n.name = "Guest later".into());
    r.settle();
    assert_eq!(Rig::name(&r.host, a), "Guest later");
    r.host.undo();
    r.settle();
    assert_eq!(Rig::name(&r.host, a), "Guest later");
    assert_eq!(Rig::name(&r.guest, a), "Guest later");
}

#[test]
fn undo_of_something_a_person_deleted_is_a_quiet_no_op() {
    let mut r = Rig::new(Role::Editor);
    let b = r.b;
    r.guest.edit_nodes(&[b], |n| n.name = "Edited".into());
    r.settle();
    // the host deletes the node
    let mut tx = crate::edit::Tx::new();
    tx.cmd(&mut r.host.doc, desigma_doc::Command::Remove { id: b }).unwrap();
    r.host.run_tx(tx);
    r.settle();
    assert!(r.guest.doc.get(b).is_none());
    r.guest.toast = None;
    assert!(!r.guest.undo());
    assert!(r.guest.toast.is_none(), "no error toast: {:?}", r.guest.toast.as_ref().map(|t| &t.text));
    // the document still agrees
    assert_eq!(r.guest.doc.nodes.len(), r.host.doc.nodes.len());
}

#[test]
fn kicking_a_guest_ends_its_session_but_keeps_its_copy() {
    let mut r = Rig::new(Role::Editor);
    r.host.kick_peer(1);
    r.settle();
    let g = r.guest.collab.guest.as_ref().expect("copy kept");
    assert_eq!(g.ended, Some(Closed::Kicked));
    assert!(!r.guest.read_only(), "an ended session is just a local copy");
    assert!(r.guest.doc.get(r.a).is_some());
    // edits after the end stay local
    let a = r.a;
    r.guest.edit_nodes(&[a], |n| n.name = "Local".into());
    r.settle();
    assert_ne!(Rig::name(&r.host, a), "Local");
}

#[test]
fn the_host_ending_the_session_tells_guests() {
    let mut r = Rig::new(Role::Editor);
    r.net.borrow_mut().guests.get_mut(&1).unwrap().push_back(GuestEv::Closed(Closed::HostEnded));
    r.host.stop_sharing();
    r.settle();
    assert_eq!(r.guest.collab.guest.as_ref().unwrap().ended, Some(Closed::HostEnded));
    assert!(r.guest.collab.remotes.is_empty());
    assert!(!r.host.collab.hosting());
}

#[test]
fn approval_requests_are_listed_until_answered() {
    let (d, _, _) = doc();
    let mut host = App::new(d);
    let net = Net::default();
    host.start_sharing_with(Box::new(MemHost(net.clone())), true, true, Role::Editor);
    net.borrow_mut().host.push_back(HostEv::Request { id: 7, name: "Dee".into() });
    net.borrow_mut().host.push_back(HostEv::Ready { link: "desigma://join/abc".into() });
    frame(&mut host, 1.0);
    assert_eq!(host.collab.requests, vec![Request { id: 7, name: "Dee".into() }]);
    assert_eq!(host.collab.host.as_ref().unwrap().url.as_deref(), Some("desigma://join/abc"));
    host.respond_request(7, true);
    assert!(host.collab.requests.is_empty());
    assert_eq!(net.borrow().answered, vec![(7, true)]);
}

#[test]
fn replacing_the_document_ends_the_session() {
    let mut r = Rig::new(Role::Editor);
    // whatever path replaces the document ends the session, as a backstop to the question
    r.host.replace_document(Document::new("Other"), None);
    assert!(!r.host.collab.active());
    r.guest.replace_document(Document::new("Other"), None);
    assert!(!r.guest.collab.active());
}

#[test]
fn the_session_does_not_flood_when_nothing_changes() {
    let mut r = Rig::new(Role::Editor);
    r.settle();
    let before = r.net.borrow().host.len();
    for _ in 0..20 {
        r.now += 0.01;
        frame(&mut r.guest, r.now);
    }
    // only presence keepalives at most, never edits
    let w = r.net.borrow();
    assert!(w.host.iter().skip(before).all(|e| matches!(e, HostEv::Presence { .. })));
}

#[test]
fn following_tracks_the_viewport_until_the_user_takes_over() {
    let mut r = Rig::new(Role::Editor);
    r.host.view.zoom = 2.0;
    r.host.view.offset = desigma_doc::Vec2::new(400.0, 300.0);
    for _ in 0..6 {
        r.settle();
    }
    r.guest.toggle_follow(0);
    for _ in 0..40 {
        r.settle();
    }
    assert!((r.guest.view.zoom - 2.0).abs() < 0.05, "zoom {}", r.guest.view.zoom);
    let hc = r.host.view.to_page(r.host.canvas_rect.min, r.host.canvas_rect.center());
    let gc = r.guest.view.to_page(r.guest.canvas_rect.min, r.guest.canvas_rect.center());
    assert!((hc.x - gc.x).abs() < 2.0 && (hc.y - gc.y).abs() < 2.0, "{hc:?} {gc:?}");
    assert_eq!(r.guest.collab.following, Some(0));
    // the user pans: following stops
    r.guest.view.offset.x += 50.0;
    r.settle();
    assert_eq!(r.guest.collab.following, None);
    // Escape also stops it
    r.guest.toggle_follow(0);
    r.settle();
    assert_eq!(r.guest.collab.following, Some(0));
    pump_with(&mut r.guest, r.now + 0.1, &Local { pointer: None, esc: true });
    assert_eq!(r.guest.collab.following, None);
}

#[test]
fn join_error_texts_are_clear() {
    use desigma_net::JoinError::*;
    let all =
        [VersionMismatch { host: 2, guest: 1 }, WrongPassword, Locked { retry_secs: 30 }, Denied, Full, Connect("x".into()), Timeout, Protocol("x".into())];
    for e in all {
        let t = link::join_error_text(&e);
        assert!(t.len() > 20 && !t.contains("Error"), "{t}");
    }
    // The older side is the one told to update.
    assert!(link::join_error_text(&VersionMismatch { host: 1, guest: 2 }).contains("Ask them to update"));
    assert!(link::join_error_text(&VersionMismatch { host: 2, guest: 1 }).contains("Update desigma"));
}

#[test]
fn id_ranges_survive_everything_the_app_does_to_a_document() {
    let (d, ..) = doc();
    let mut app = App::new(d);
    let ids_before: Vec<u64> = (0..3).map(|_| app.doc.alloc_id().0).collect();
    let net = Net::default();
    app.start_sharing_with(Box::new(MemHost(net)), false, false, Role::Editor);
    assert_eq!(app.doc.id_peer(), Some(0));
    let after = app.doc.alloc_id().0;
    assert!(after > *ids_before.last().unwrap(), "host ids keep counting up");
}

#[test]
fn over_real_network() {
    std::env::set_var("DESIGMA_COLLAB_NETWORK", "loopback");
    let ctx = egui::Context::default();
    let (d, a, _) = doc();
    let mut host = App::new(d);
    host.start_sharing(&ctx, Some("pw".into()), false, Role::Editor);
    let mut guest = App::new(Document::new("x"));
    let mut now = 1.0;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let step = |host: &mut App, guest: &mut App, now: &mut f64| {
        *now += 0.1;
        frame(host, *now);
        frame(guest, *now);
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let link = loop {
        step(&mut host, &mut guest, &mut now);
        if let Some(u) = host.collab.host.as_ref().and_then(|h| h.url.clone()) {
            break u;
        }
        assert!(std::time::Instant::now() < end, "host never became ready");
    };
    // wrong password first
    assert!(guest.join_session(&ctx, &link, "Bea", "").is_err(), "a password is required");
    guest.join_session(&ctx, &link, "Bea", "pw").unwrap();
    while !guest.collab.is_guest() {
        step(&mut host, &mut guest, &mut now);
        assert!(std::time::Instant::now() < end, "guest never joined");
    }
    assert_eq!(guest.doc.nodes.len(), host.doc.nodes.len());
    host.edit_nodes(&[a], |n| n.name = "Over the wire".into());
    while Rig::name(&guest, a) != "Over the wire" {
        step(&mut host, &mut guest, &mut now);
        assert!(std::time::Instant::now() < end, "host edit never arrived");
    }
    guest.edit_nodes(&[a], |n| n.name = "And back".into());
    while Rig::name(&host, a) != "And back" {
        step(&mut host, &mut guest, &mut now);
        assert!(std::time::Instant::now() < end, "guest edit never arrived");
    }
    for _ in 0..40 {
        step(&mut host, &mut guest, &mut now);
    }
    assert!(guest.collab.remotes.contains_key(&0) && host.collab.remotes.contains_key(&1), "presence over datagrams");
    host.stop_sharing();
    while guest.collab.guest.as_ref().is_some_and(|g| g.ended.is_none()) {
        step(&mut host, &mut guest, &mut now);
        assert!(std::time::Instant::now() < end, "guest never noticed the end");
    }
}

// ---- reconnect ----

/// An image cache in a directory of its own.
pub(crate) fn cache_in(tag: &str) -> assets::ImageCache {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("desigma-collab-test-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assets::ImageCache::with_dir(dir, 64 << 20)
}

impl Rig {
    /// Both editors run for `secs` seconds, 0.1 s a frame.
    fn run(&mut self, secs: f64) {
        for _ in 0..(secs * 10.0).round() as usize {
            self.now += 0.1;
            frame(&mut self.host, self.now);
            frame(&mut self.guest, self.now);
        }
    }

    fn guest_state(&self) -> &GuestState {
        self.guest.collab.guest.as_ref().expect("a guest")
    }
}

fn insert_rect(app: &mut App, name: &str) -> NodeId {
    let page = app.page;
    let id = app.doc.alloc_id();
    let mut n = Node::rectangle(id, Rect::new(5.0, 5.0, 10.0, 10.0));
    n.name = name.into();
    let mut tx = crate::edit::Tx::new();
    tx.cmd(&mut app.doc, desigma_doc::Command::Insert { parent: page, index: 0, nodes: vec![n] }).unwrap();
    app.run_tx(tx);
    id
}

#[test]
fn a_dropped_guest_reconnects_and_keeps_edits_made_on_both_sides() {
    let mut r = Rig::new(Role::Editor);
    let (a, b) = (r.a, r.b);
    let token = r.guest_state().sess.token().expect("the host handed out a secret").to_string();
    assert_eq!(r.net.borrow().allowed, vec![token.clone()], "the host trusts that secret without asking again");
    drop_connection(&r.net, 1);
    r.run(0.3);
    // the guest shows it is reconnecting but keeps the document editable
    assert!(r.guest_state().reconnect.is_some() && r.guest_state().ended.is_none());
    assert!(!r.guest.collab.connected() && !r.guest.read_only());
    // the host forgot the cursor and the connection but keeps the seat
    assert!(r.host.collab.host.as_ref().unwrap().peers.is_empty() && r.host.collab.remotes.is_empty());
    assert_eq!(r.host.collab.host.as_ref().unwrap().sess.token(1), Some(token.as_str()));

    // work goes on during the outage, including the same field on both sides
    r.guest.edit_nodes(&[b], |n| n.name = "Guest offline".into());
    r.guest.edit_nodes(&[a], |n| n.opacity = 0.25);
    let made = insert_rect(&mut r.guest, "Made offline");
    r.host.edit_nodes(&[a], |n| n.name = "Host meanwhile".into());
    r.host.edit_nodes(&[a], |n| n.opacity = 0.75);
    r.run(0.4);
    assert_ne!(Rig::name(&r.host, b), "Guest offline", "nothing crosses a dead link");

    r.run(4.0);
    assert!(r.guest_state().reconnect.is_none() && r.guest.collab.connected(), "back after the first retry");
    assert_eq!(Rig::name(&r.host, b), "Guest offline");
    assert_eq!(Rig::name(&r.host, made), "Made offline");
    assert_eq!(Rig::name(&r.guest, a), "Host meanwhile");
    // the same field: the guest's own write came after the host's
    assert_eq!(r.host.doc.get(a).unwrap().opacity, 0.25);
    assert_eq!(r.guest.doc.get(a).unwrap().opacity, 0.25);
    assert_eq!(r.guest.doc.nodes.len(), r.host.doc.nodes.len());
    // the same person: peer index and id range
    assert_eq!(r.guest_state().sess.peer(), 1);
    assert_eq!(made.0 >> 40, 1);
    assert_eq!(r.host.collab.host.as_ref().unwrap().peers.keys().collect::<Vec<_>>(), vec![&1]);
    assert_eq!(r.guest.toast.as_ref().map(|t| t.text.as_str()), Some("Reconnected"));
    // edits and presence flow again, both ways
    r.guest.edit_nodes(&[b], |n| n.name = "After".into());
    r.host.edit_nodes(&[a], |n| n.name = "Host after".into());
    r.run(3.0);
    assert_eq!(Rig::name(&r.host, b), "After");
    assert_eq!(Rig::name(&r.guest, a), "Host after");
    assert!(r.host.collab.remotes.contains_key(&1) && r.guest.collab.remotes.contains_key(&0));
    let id = r.guest.doc.alloc_id();
    assert_eq!(id.0 >> 40, 1);
}

#[test]
fn a_guest_gives_up_after_two_minutes_and_keeps_a_local_copy() {
    let mut r = Rig::new(Role::Editor);
    // nobody answers on the network the guest redials into
    let dead = Net::default();
    let (redial, tries) = redialing(&dead, "Bea");
    r.guest.collab.guest.as_mut().unwrap().redial = Some(redial);
    drop_connection(&r.net, 1);
    let b = r.b;
    r.guest.edit_nodes(&[b], |n| n.name = "Kept".into());
    while r.now < 100.0 {
        r.now += 1.0;
        frame(&mut r.guest, r.now);
    }
    assert!(r.guest_state().reconnect.is_some(), "still trying at 100 s");
    assert!(tries.get() >= 3, "retried with backoff: {} attempts", tries.get());
    while r.now < 130.0 {
        r.now += 1.0;
        frame(&mut r.guest, r.now);
    }
    let g = r.guest_state();
    assert!(g.reconnect.is_none() && matches!(g.ended, Some(Closed::Lost(_))));
    assert!(!g.banner_hidden);
    assert!(!r.guest.read_only(), "the copy is the guest's own now");
    assert_eq!(Rig::name(&r.guest, b), "Kept");
    let n = tries.get();
    r.now += 60.0;
    frame(&mut r.guest, r.now);
    assert_eq!(tries.get(), n, "no more attempts");
}

struct Refusing;
impl GuestLink for Refusing {
    fn drain(&self) -> Vec<GuestEv> {
        vec![GuestEv::Failed(JoinError::WrongPassword)]
    }
    fn send(&self, _: Vec<u8>) {}
    fn send_presence(&self, _: &[u8]) {}
    fn leave(&self) {}
}

#[test]
fn a_refusal_ends_the_reconnecting_at_once() {
    let mut r = Rig::new(Role::Editor);
    r.guest.collab.guest.as_mut().unwrap().redial = Some(Box::new(|_| Ok(Box::new(Refusing) as Box<dyn GuestLink>)));
    drop_connection(&r.net, 1);
    r.run(5.0);
    assert!(matches!(r.guest_state().ended, Some(Closed::Lost(_))));
}

#[test]
fn a_kicked_or_ended_session_is_not_redialled() {
    let mut r = Rig::new(Role::Editor);
    r.host.kick_peer(1);
    r.run(5.0);
    assert_eq!(r.guest_state().ended, Some(Closed::Kicked));
    assert!(r.guest_state().reconnect.is_none());
}

#[test]
fn a_kicked_guest_cannot_come_back_with_its_old_secret() {
    let mut r = Rig::new(Role::Editor);
    let token = r.guest_state().sess.token().unwrap().to_string();
    r.host.kick_peer(1);
    r.run(0.5);
    let _again = connect_with(&r.net, "Bea", Some(&token));
    r.run(1.0);
    let peers: Vec<u32> = r.host.collab.host.as_ref().unwrap().peers.keys().copied().collect();
    assert_eq!(peers, vec![2], "it is a new person with a new index");
}

#[test]
fn a_returning_view_only_guest_stays_view_only() {
    let mut r = Rig::new(Role::Editor);
    r.host.set_peer_role(1, Role::Viewer);
    r.run(0.5);
    assert!(r.guest.read_only());
    drop_connection(&r.net, 1);
    r.run(4.0);
    assert!(r.guest_state().reconnect.is_none());
    assert_eq!(r.host.collab.host.as_ref().unwrap().peers[&1].role, Role::Viewer);
    assert!(r.guest.read_only(), "the host's setting survived the reconnect");
}

#[test]
fn a_guest_that_vanished_without_a_goodbye_is_cleaned_up_and_its_seat_kept() {
    let mut r = Rig::new(Role::Editor);
    r.run(1.0);
    assert!(r.host.collab.remotes.contains_key(&1));
    drop_connection(&r.net, 1);
    r.host.collab.following = Some(1);
    frame(&mut r.host, r.now + 0.1);
    let h = r.host.collab.host.as_ref().unwrap();
    assert!(h.peers.is_empty());
    assert!(!r.host.collab.remotes.contains_key(&1) && r.host.collab.following.is_none());
    // someone else joining gets a new index, not the reserved one
    let _other = connect(&r.net, "Cy");
    frame(&mut r.host, r.now + 0.2);
    assert_eq!(r.host.collab.host.as_ref().unwrap().peers.keys().copied().collect::<Vec<_>>(), vec![2]);
}

#[test]
fn a_returning_guest_replaces_a_connection_the_host_has_not_noticed_is_dead() {
    let mut r = Rig::new(Role::Editor);
    let token = r.guest_state().sess.token().unwrap().to_string();
    // the old connection is still open on the host's side when the guest shows up again
    let _fresh = connect_with(&r.net, "Bea", Some(&token));
    frame(&mut r.host, r.now + 0.1);
    let h = r.host.collab.host.as_ref().unwrap();
    assert_eq!(h.peers.len(), 1);
    assert_eq!(h.peers[&1].net, 2, "the newest connection holds the seat");
    // the old one is told to go
    assert!(matches!(r.net.borrow_mut().guests.get_mut(&1).unwrap().back(), Some(GuestEv::Closed(Closed::Kicked))));
}

// ---- fonts ----

struct FakeFonts {
    files: BTreeMap<String, Vec<u8>>,
    installed: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl assets::FontIo for FakeFonts {
    fn file(&self, key: &assets::FontKey) -> Option<Vec<u8>> {
        self.files.get(&key.family).cloned()
    }
    fn install(&self, bytes: Vec<u8>) {
        self.installed.borrow_mut().push(bytes);
    }
}

fn with_text(d: &mut Document, family: &str) {
    let page = d.pages[0];
    let id = d.alloc_id();
    let style = desigma_doc::TextStyle { font_family: family.into(), ..Default::default() };
    d.insert(page, Node::text(id, Rect::new(0.0, 0.0, 50.0, 20.0), "Hello", style)).unwrap();
}

fn failed(family: &str) -> (FontRequest, FontError) {
    (FontRequest::new(family, 400, false), FontError::NotFound(family.into()))
}

fn font_requests_sent(net: &Net) -> Vec<Vec<assets::FontKey>> {
    net.borrow()
        .sent
        .iter()
        .filter_map(|(_, b)| match serde_json::from_slice::<ToHost>(b) {
            Ok(ToHost::Fonts(k)) => Some(k),
            _ => None,
        })
        .collect()
}

#[test]
fn a_guest_asks_the_host_for_fonts_it_cannot_load_and_only_for_those_the_file_uses() {
    let (mut d, a, b) = doc();
    for fam in ["Fancy Sans", "Rare Serif", "Huge Display"] {
        with_text(&mut d, fam);
    }
    let mut r = Rig::with_doc(Role::Editor, d, a, b);
    let ttf = vec![0, 1, 0, 0, 9, 9, 9];
    let files = BTreeMap::from([
        ("Fancy Sans".to_string(), ttf.clone()),
        ("Huge Display".to_string(), vec![0u8; assets::MAX_FONT + 1]),
        ("Not In The File".to_string(), ttf.clone()),
    ]);
    r.host.collab.font_io = assets::FontIoBox(Box::new(FakeFonts { files, installed: Default::default() }));
    let loaded = Rc::new(RefCell::new(Vec::new()));
    r.guest.collab.font_io = assets::FontIoBox(Box::new(FakeFonts { files: BTreeMap::new(), installed: loaded.clone() }));

    // The guest's background load found none of them, and one that no text asks for (a script fallback).
    let report =
        FontReport { failed: vec![failed("Fancy Sans"), failed("Rare Serif"), failed("Huge Display"), failed("Noto Sans Arabic")], ..Default::default() };
    r.guest.fonts.inject(report);
    r.guest.poll_fonts();
    assert!(r.guest.toast.as_ref().is_some_and(|t| t.text.starts_with("Font unavailable: Noto Sans Arabic")), "what the file does not use is not asked for");
    r.guest.toast = None;
    r.settle();

    let asked = font_requests_sent(&r.net);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].iter().map(|k| k.family.as_str()).collect::<Vec<_>>(), vec!["Fancy Sans", "Rare Serif", "Huge Display"]);
    assert_eq!(*loaded.borrow(), vec![ttf], "the font the host has arrived and was loaded");
    assert_eq!(r.guest_state().fonts.loaded.len(), 1);
    // the two the host could not send show up in the missing-font toast
    let toast = r.guest.toast.as_ref().expect("a toast");
    assert!(toast.error && toast.text.starts_with("Font unavailable: Rare Serif") && toast.text.contains("(+1 more)"), "{}", toast.text);

    // the same failures again do not ask again
    r.guest.toast = None;
    r.guest.fonts.inject(FontReport { failed: vec![failed("Rare Serif")], ..Default::default() });
    r.guest.poll_fonts();
    r.settle();
    assert_eq!(font_requests_sent(&r.net).len(), 1);
    assert!(r.guest.toast.is_none());

    // a request for a face the file does not use is answered with nothing, whatever the host owns
    let key = assets::FontKey { family: "Not In The File".into(), weight: 400, italic: false };
    r.guest_state().link.send(encode(&ToHost::Fonts(vec![key.clone()])));
    r.settle();
    let replies: Vec<ToGuest> = r.net.borrow().down.iter().filter_map(|(_, b)| serde_json::from_slice(b).ok()).collect();
    assert!(replies.iter().any(|m| matches!(m, ToGuest::Font { key: k, bytes } if *k == key && bytes.is_empty())));
}

#[test]
fn fonts_travel_as_base64_not_number_arrays() {
    let key = assets::FontKey { family: "F".into(), weight: 400, italic: false };
    let bytes: Vec<u8> = (0..50_000u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8).collect();
    let json = encode(&ToGuest::Font { key, bytes: bytes.clone() });
    assert!(json.len() < bytes.len() * 4 / 3 + 100);
    let Ok(ToGuest::Font { bytes: back, .. }) = serde_json::from_slice(&json) else { panic!("round trip") };
    assert_eq!(back, bytes);
}

// ---- images ----

/// Bytes that do not compress, like an encoded image.
fn picture(seed: u8, len: usize) -> Vec<u8> {
    let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ (u64::from(seed) << 32 | 1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn image_requests(net: &Net, peer: u32) -> usize {
    net.borrow()
        .sent
        .iter()
        .filter(|(p, _)| *p == peer)
        .filter(|(_, b)| matches!(serde_json::from_slice::<ToHost>(b), Ok(ToHost::Sync(ClientMsg::ImageRequest { .. }))))
        .count()
}

#[test]
fn a_guest_asks_only_for_images_its_cache_lacks() {
    let (mut d, a, b) = doc();
    d.images.insert("img-1".into(), picture(1, 5000));
    d.images.insert("img-2".into(), picture(2, 7000));
    let mut r = Rig::with_doc(Role::Editor, d, a, b);
    assert_eq!(image_requests(&r.net, 1), 2, "an empty cache asks for everything");
    assert_eq!(r.guest.doc.images["img-1"], picture(1, 5000));
    assert_eq!(r.guest.doc.images["img-2"], picture(2, 7000));

    // a second guest that shares the first one's cache dir (the next session on the same machine)
    let mut carol = App::new(Document::new("c"));
    carol.collab.images = assets::ImageCache::with_dir(r.guest.collab.images.dir().to_path_buf(), 64 << 20);
    carol.join_with(Box::new(connect(&r.net, "Cy")));
    for _ in 0..8 {
        r.now += 0.1;
        frame(&mut r.host, r.now);
        frame(&mut carol, r.now);
    }
    assert!(carol.collab.is_guest());
    assert_eq!(image_requests(&r.net, 2), 0, "everything came from the cache");
    assert_eq!(carol.doc.images["img-1"], picture(1, 5000));
    assert_eq!(carol.doc.images["img-2"], picture(2, 7000));

    // an image added later is not cached anywhere yet: it is fetched from the host
    r.host.doc.images.insert("img-3".into(), picture(3, 3000));
    r.host.changed();
    for _ in 0..8 {
        r.now += 0.1;
        frame(&mut r.host, r.now);
        frame(&mut carol, r.now);
    }
    assert_eq!(carol.doc.images.get("img-3"), Some(&picture(3, 3000)));
    assert_eq!(image_requests(&r.net, 2), 1);
}

#[test]
fn image_bytes_that_do_not_match_the_announced_digest_are_dropped() {
    let (mut d, a, b) = doc();
    d.images.insert("img-1".into(), picture(1, 2000));
    let mut r = Rig::with_doc(Role::Editor, d, a, b);
    let mut g = r.guest.collab.guest.take().unwrap();
    g.digests.insert("img-9".into(), assets::digest(b"the real thing"));
    let forged = ServerMsg::ImageData { hash: "img-9".into(), bytes: b"something else".to_vec() };
    assert!(exchange::vet(&mut r.guest, &mut g, forged).is_none());
    assert!(!r.guest.collab.images.contains(&assets::digest(b"something else")));
    let honest = ServerMsg::ImageData { hash: "img-9".into(), bytes: b"the real thing".to_vec() };
    assert!(exchange::vet(&mut r.guest, &mut g, honest).is_some());
    assert!(r.guest.collab.images.contains(&assets::digest(b"the real thing")));
}

#[test]
fn base64_images_cost_about_their_raw_size_on_the_wire_where_number_arrays_cost_a_third_more() {
    let bytes = picture(7, 200_000);
    let b64 = serde_json::to_vec(&ServerMsg::ImageData { hash: "h".into(), bytes: bytes.clone() }).unwrap();
    let array = serde_json::json!({ "ImageData": { "hash": "h", "bytes": bytes } }).to_string().into_bytes();
    let wire = |json: &[u8]| desigma_net::frame::encode(json).unwrap().len();
    let (b64_wire, array_wire) = (wire(&b64), wire(&array));
    eprintln!("200000 image bytes: base64 {} JSON / {} on the wire; number array {} JSON / {} on the wire", b64.len(), b64_wire, array.len(), array_wire);
    assert!(b64.len() < bytes.len() * 4 / 3 + 100, "JSON size {}", b64.len());
    assert!(array.len() > bytes.len() * 3, "the old form is 3+ bytes per byte: {}", array.len());
    assert!(b64_wire < bytes.len() * 12 / 10, "after deflate base64 costs about the raw size: {b64_wire}");
    assert!(array_wire > b64_wire * 5 / 4, "the old form stays much bigger on the wire: {array_wire} against {b64_wire}");
}

// ---- leaving, link ----

#[test]
fn leaving_while_hosting_or_joined_asks_first() {
    let mut r = Rig::new(Role::Editor);
    // the host opens another file: asked, nothing happens yet
    r.host.new_document();
    let d = r.host.dialog.clone().expect("a question");
    assert_eq!(d.title, "Stop sharing?");
    assert_eq!(d.lines[0].0, "1 person will be disconnected.");
    assert!(d.danger && r.host.collab.hosting() && r.host.doc.name == "Shared");
    // cancelling keeps everything as it was
    r.host.dialog = None;
    assert!(r.host.collab.hosting());
    // confirming ends the session, then does what was asked
    r.host.new_document();
    let d = r.host.dialog.take().unwrap();
    crate::home::dialog::finish(&mut r.host, d);
    assert!(!r.host.collab.active());
    assert_eq!(r.host.doc.name, "Untitled");

    // a guest is asked "Leave the session?"
    let mut r = Rig::new(Role::Editor);
    r.guest.new_board_document();
    let d = r.guest.dialog.clone().expect("a question");
    assert_eq!(d.title, "Leave the session?");
    assert!(r.guest.collab.is_guest() && r.guest.doc.name == "Shared");
    let d = r.guest.dialog.take().unwrap();
    crate::home::dialog::finish(&mut r.guest, d);
    assert!(!r.guest.collab.active());
    assert_ne!(r.guest.doc.name, "Shared");
}

#[test]
fn a_host_alone_and_a_reconnecting_guest_are_asked_too_but_a_finished_session_is_not() {
    let (d, ..) = doc();
    let mut host = App::new(d);
    host.start_sharing_with(Box::new(MemHost(Net::default())), false, false, Role::Editor);
    host.new_document();
    assert_eq!(host.dialog.as_ref().unwrap().lines[0].0, "Nobody is in the file now. The link stops working.");

    let mut r = Rig::new(Role::Editor);
    drop_connection(&r.net, 1);
    r.run(0.3);
    r.guest.new_document();
    assert_eq!(r.guest.dialog.as_ref().map(|d| d.title.as_str()), Some("Leave the session?"));
    r.guest.dialog = None;

    // after the session is over the file is just a copy: no question
    let mut r = Rig::new(Role::Editor);
    r.host.kick_peer(1);
    r.run(1.0);
    r.guest.new_document();
    assert!(r.guest.dialog.is_none());
    assert_eq!(r.guest.doc.name, "Untitled");
}

#[test]
fn the_share_link_follows_the_transport_while_the_popover_is_open() {
    let (d, ..) = doc();
    let mut host = App::new(d);
    let net = Net::default();
    host.start_sharing_with(Box::new(MemHost(net.clone())), false, false, Role::Editor);
    net.borrow_mut().host.push_back(HostEv::Ready { link: "desigma://join/first".into() });
    frame(&mut host, 1.0);
    let url = |h: &App| h.collab.host.as_ref().unwrap().url.clone();
    assert_eq!(url(&host).as_deref(), Some("desigma://join/first"));
    // the address changed while the popover was closed; opening it shows the new one
    net.borrow_mut().link = Some("desigma://join/second".into());
    frame(&mut host, 1.1);
    assert_eq!(url(&host).as_deref(), Some("desigma://join/first"), "nothing to refresh while nobody looks");
    host.collab.share.open = true;
    frame(&mut host, 1.2);
    assert_eq!(url(&host).as_deref(), Some("desigma://join/second"));
    net.borrow_mut().link = Some("desigma://join/third".into());
    frame(&mut host, 1.3);
    assert_eq!(url(&host).as_deref(), Some("desigma://join/third"));
}

// ---- over the real transport ----

/// Lets a test keep a handle on the real host while the app owns it.
struct SharedHost(Rc<link::NetHost>);

impl HostLink for SharedHost {
    fn drain(&self) -> Vec<HostEv> {
        self.0.drain()
    }
    fn respond(&self, id: u64, accept: bool) {
        self.0.respond(id, accept)
    }
    fn send(&self, peer: u32, bytes: Vec<u8>) {
        self.0.send(peer, bytes)
    }
    fn send_presence(&self, peer: u32, bytes: &[u8]) {
        self.0.send_presence(peer, bytes)
    }
    fn kick(&self, peer: u32) {
        self.0.kick(peer)
    }
    fn link(&self) -> Option<String> {
        self.0.link()
    }
    fn join_token(&self, peer: u32) -> Option<String> {
        self.0.join_token(peer)
    }
    fn allow_resume(&self, token: &str) {
        self.0.allow_resume(token)
    }
    fn revoke_resume(&self, token: &str) {
        self.0.revoke_resume(token)
    }
}

#[test]
fn over_real_network_a_severed_guest_comes_back_with_its_edits() {
    std::env::set_var("DESIGMA_COLLAB_NETWORK", "loopback");
    let ctx = egui::Context::default();
    let (d, a, b) = doc();
    let mut host = App::new(d);
    // approval is on: a returning guest must not be asked again
    let net_host = Rc::new(link::NetHost::start(None, true, Box::new(|| {})).unwrap());
    host.start_sharing_with(Box::new(SharedHost(net_host.clone())), false, true, Role::Editor);
    let mut guest = App::new(Document::new("x"));
    guest.collab.images = cache_in("real");
    let mut now = 1.0;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let step = |host: &mut App, guest: &mut App, now: &mut f64| {
        *now += 0.1;
        frame(host, *now);
        frame(guest, *now);
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let link = loop {
        step(&mut host, &mut guest, &mut now);
        if let Some(u) = host.collab.host.as_ref().and_then(|h| h.url.clone()) {
            break u;
        }
        assert!(std::time::Instant::now() < end, "host never became ready");
    };
    guest.join_session(&ctx, &link, "Bea", "").unwrap();
    while !guest.collab.is_guest() {
        step(&mut host, &mut guest, &mut now);
        if let Some(rq) = host.collab.requests.first().cloned() {
            host.respond_request(rq.id, true);
        }
        assert!(std::time::Instant::now() < end, "guest never joined");
    }
    assert_eq!(guest.collab.guest.as_ref().unwrap().sess.peer(), 1);

    // the network fails; both sides keep editing
    let net_peer = host.collab.host.as_ref().unwrap().peers[&1].net;
    net_host.sever(net_peer);
    guest.edit_nodes(&[b], |n| n.name = "Guest offline".into());
    host.edit_nodes(&[a], |n| n.name = "Host meanwhile".into());
    let mut reconnecting = false;
    while Rig::name(&host, b) != "Guest offline" || Rig::name(&guest, a) != "Host meanwhile" {
        step(&mut host, &mut guest, &mut now);
        reconnecting |= guest.collab.guest.as_ref().is_some_and(|g| g.reconnect.is_some());
        assert!(host.collab.requests.is_empty(), "a returning guest is not asked about again");
        assert!(std::time::Instant::now() < end, "never converged after the outage");
    }
    assert!(reconnecting, "the guest went through the reconnecting state");
    let g = guest.collab.guest.as_ref().unwrap();
    assert!(g.ended.is_none() && g.reconnect.is_none());
    assert_eq!(g.sess.peer(), 1, "same peer after the reconnect");
    let peers: Vec<u32> = host.collab.host.as_ref().unwrap().peers.keys().copied().collect();
    assert_eq!(peers, vec![1]);
    // and it still works afterwards
    guest.edit_nodes(&[a], |n| n.name = "Afterwards".into());
    while Rig::name(&host, a) != "Afterwards" {
        step(&mut host, &mut guest, &mut now);
        assert!(std::time::Instant::now() < end, "edit after reconnect never arrived");
    }
    host.stop_sharing();
}
