//! Opening a file or link over unsaved work asks first (see `unsaved.rs`).

use super::*;
use crate::collab::link::HostEv;
use crate::collab::tests::{MemHost, Net};
use crate::instance::Open;
use desigma_collab::Role;

fn click_named(s: &mut Sim, name: &str) {
    let r = s.rect_of(name);
    s.click(r.center());
}

#[test]
fn opens_over_unsaved_work_ask_first() {
    let mut s = Sim::new();
    // a clean document is replaced without a question
    s.shell.session().request_open(Open::Link("desigma://join/abc".into()));
    assert!(s.app().pending_open.is_none() && s.app().collab.join.is_some());
    s.shell.session().collab.join = None;
    // an edited one waits for the answer
    s.shell.session().file_hash = Some(crate::home::session::doc_hash(&s.shell.session().doc));
    s.shell.session().doc.name = "Edited".into();
    s.shell.session().request_open(Open::Link("desigma://join/abc".into()));
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(s.app().pending_open.is_some() && s.app().collab.join.is_none());
    click_named(&mut s, "unsaved-cancel");
    s.frame(vec![]);
    assert!(s.app().pending_open.is_none() && s.app().collab.join.is_none(), "cancel keeps everything");
    s.shell.session().request_open(Open::Link("desigma://join/abc".into()));
    s.frame(vec![]);
    s.frame(vec![]);
    click_named(&mut s, "unsaved-discard");
    s.frame(vec![]);
    assert!(s.app().pending_open.is_none() && s.app().collab.join.is_some(), "discard goes ahead");
}

#[test]
fn a_live_session_also_asks_before_opening_something() {
    let mut s = Sim::new();
    let net = Net::default();
    s.shell.session().start_sharing_with(Box::new(MemHost(net.clone())), false, false, Role::Editor);
    net.borrow_mut().host.push_back(HostEv::Ready { link: "desigma://join/abcdefghijklmnop".into() });
    for _ in 0..4 {
        s.frame(vec![]);
    }
    s.shell.session().request_open(Open::File(std::env::temp_dir().join("x.desigma")));
    assert!(s.app().pending_open.is_some());
    s.frame(vec![]);
    s.frame(vec![]);
    click_named(&mut s, "unsaved-cancel");
    s.frame(vec![]);
    assert!(s.app().pending_open.is_none() && s.app().collab.hosting());
}
