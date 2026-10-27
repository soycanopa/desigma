//! UI tests for "Update now": the dialog's states (idle, downloading, verifying, ready, failed) and the guarded
//! restart. Nothing is downloaded and no installer is ever run: states are injected, and a "prepared" update is
//! a dummy file that is only asked about.

use super::*;
use crate::updater::install::{self, Launch, Prepared};
use crate::updater::Install;

/// Every on-screen text of the last frame.
fn texts(s: &Sim) -> Vec<String> {
    s.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn has_rect(s: &Sim, name: &str) -> bool {
    s.ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("test-rect", name.to_string())))).is_some()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("desigma_updater_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The dialog in `state`, redrawn.
fn dialog_in(state: Install) -> (Sim, std::path::PathBuf) {
    let (mut s, dir) = sim(false);
    click_rect(&mut s, "updater-view");
    s.shell.session().updater.install = state;
    // forget what the idle dialog recorded, so only what the new state draws is found
    for name in ["updater-update", "updater-later", "updater-skip", "updater-page", "updater-bar", "updater-cancel", "updater-restart", "updater-retry"] {
        s.ctx.data_mut(|d| d.remove::<egui::Rect>(egui::Id::new(("test-rect", name.to_string()))));
    }
    s.frame(vec![]);
    s.frame(vec![]);
    (s, dir)
}

/// A downloaded installer waiting for the restart; nothing is ever run.
fn ready(dir: &std::path::Path) -> Install {
    let setup = dir.join("never-run-setup.exe");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&setup, "MZ").unwrap();
    Install::Ready(Prepared::Setup { setup })
}

#[test]
fn the_idle_dialog_offers_update_now_and_the_release_page() {
    let (s, dir) = dialog_in(Install::Idle);
    for name in ["updater-update", "updater-later", "updater-skip", "updater-page"] {
        assert!(has_rect(&s, name), "{name}");
    }
    for name in ["updater-cancel", "updater-restart", "updater-retry", "updater-bar"] {
        assert!(!has_rect(&s, name), "{name}");
    }
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "Update now") && t.iter().any(|x| x == "Open release page"), "{t:?}");
    assert!(!t.iter().any(|x| x == "Download"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_now_in_a_development_build_fails_with_the_page_link_left() {
    if crate::RELEASE_BUILD {
        return;
    }
    let (mut s, dir) = dialog_in(Install::Idle);
    click_rect(&mut s, "updater-update");
    let Install::Failed(e) = s.app().updater.install.clone() else { panic!("{:?}", s.app().updater.install) };
    assert!(e.contains("development build"), "{e}");
    s.frame(vec![]);
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "The update didn't finish") && t.iter().any(|x| x.starts_with("This is a development build")), "{t:?}");
    assert!(has_rect(&s, "updater-page") && has_rect(&s, "updater-retry") && has_rect(&s, "updater-skip"));
    // Try again goes back through the same check
    click_rect(&mut s, "updater-retry");
    assert!(matches!(s.app().updater.install, Install::Failed(_)));
    click_rect(&mut s, "updater-later");
    assert!(!s.app().updater.dialog);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn downloading_shows_progress_and_cancel_only() {
    let (mut s, dir) = dialog_in(Install::Downloading { done: 12_900_000, total: Some(47_800_000) });
    assert!(has_rect(&s, "updater-bar") && has_rect(&s, "updater-cancel") && has_rect(&s, "updater-page"));
    for name in ["updater-update", "updater-later", "updater-skip", "updater-restart"] {
        assert!(!has_rect(&s, name), "{name}");
    }
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "Downloading the update…") && t.iter().any(|x| x == "12.3 of 45.6 MB"), "{t:?}");
    // a click outside and Escape do not close it while the download runs (Escape cancels the download only)
    s.click(egui::Pos2::new(5.0, 5.0));
    assert!(s.app().updater.dialog);
    // unknown size: just the amount
    s.shell.session().updater.install = Install::Downloading { done: 3_000_000, total: None };
    s.frame(vec![]);
    assert!(texts(&s).iter().any(|x| x == "2.9 MB"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verifying_and_preparing_have_no_buttons_to_press() {
    for (state, line) in [(Install::Verifying, "Checking the download…"), (Install::Preparing, "Preparing the update…")] {
        let (s, dir) = dialog_in(state);
        assert!(texts(&s).iter().any(|x| x == line), "{line}");
        for name in ["updater-update", "updater-later", "updater-cancel", "updater-restart", "updater-retry", "updater-skip"] {
            assert!(!has_rect(&s, name), "{name}");
        }
        assert!(has_rect(&s, "updater-page") && has_rect(&s, "updater-bar"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_prepared_update_offers_restart_to_update() {
    let dir = scratch("ready");
    let (s, sim_dir) = dialog_in(ready(&dir));
    assert!(has_rect(&s, "updater-restart") && has_rect(&s, "updater-later") && has_rect(&s, "updater-page"));
    assert!(!has_rect(&s, "updater-update") && !has_rect(&s, "updater-skip") && !has_rect(&s, "updater-bar"));
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "Restart to update") && t.iter().any(|x| x == "desigma 99.0.0-alpha.1 is ready"), "{t:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}

#[test]
fn failures_name_the_problem_and_keep_the_release_page() {
    let (s, dir) = dialog_in(Install::Failed("the download does not match its checksum in SHA256SUMS".into()));
    let t = texts(&s);
    assert!(t.iter().any(|x| x == "The download does not match its checksum in SHA256SUMS"), "{t:?}");
    assert!(t.iter().any(|x| x.contains("release page")));
    assert!(has_rect(&s, "updater-retry") && has_rect(&s, "updater-page"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_banner_says_when_the_update_is_ready() {
    let dir = scratch("banner");
    let (mut s, sim_dir) = sim(false);
    s.shell.session().updater.install = ready(&dir);
    s.frame(vec![]);
    assert!(texts(&s).iter().any(|x| x == "desigma 99.0.0-alpha.1 is ready to install"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}

// The tests below stash and take the launch that "Restart to update" leaves for the close handling. They share
// that one slot, so they run one at a time.
static LAUNCH_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn restarting_a_clean_document_goes_straight_ahead() {
    let _slot = LAUNCH_SLOT.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("restart");
    let (mut s, sim_dir) = dialog_in(ready(&dir));
    let _ = install::take_stashed();
    click_rect(&mut s, "updater-restart");
    s.frame(vec![]);
    assert!(!s.app().updater.dialog && s.app().dialog.is_none() && s.app().pending_open.is_none() && !s.app().updater.pending_restart);
    let Some(Launch::Spawn { program, args }) = install::take_stashed() else { panic!("nothing to launch") };
    assert!(program.ends_with("never-run-setup.exe") && args.contains(&"/RELAUNCH=1".to_string()), "{program:?} {args:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}

#[test]
fn unsaved_changes_are_asked_about_before_restarting() {
    let _slot = LAUNCH_SLOT.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("unsaved");
    let (mut s, sim_dir) = dialog_in(ready(&dir));
    let _ = install::take_stashed();
    s.shell.session().file_hash = Some(crate::home::session::doc_hash(&s.shell.session().doc));
    s.shell.session().doc.name = "Edited".into();
    click_rect(&mut s, "updater-restart");
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(s.app().updater.pending_restart);
    assert!(texts(&s).iter().any(|x| x == "Restarting to install the update closes this document."));
    assert!(install::take_stashed().is_none(), "nothing happens before the answer");
    // Cancel keeps the document and the prepared update
    click_rect(&mut s, "unsaved-cancel");
    s.frame(vec![]);
    assert!(!s.app().updater.pending_restart && matches!(s.app().updater.install, Install::Ready(_)));
    assert!(install::take_stashed().is_none());
    // Discard goes ahead
    s.shell.session().request_restart();
    s.frame(vec![]);
    s.frame(vec![]);
    click_rect(&mut s, "unsaved-discard");
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(install::take_stashed().is_some());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}

#[test]
fn a_live_session_is_asked_about_before_restarting() {
    use crate::collab::link::HostEv;
    use crate::collab::tests::{MemHost, Net};
    let _slot = LAUNCH_SLOT.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("session");
    let (mut s, sim_dir) = dialog_in(ready(&dir));
    let _ = install::take_stashed();
    let net = Net::default();
    s.shell.session().start_sharing_with(Box::new(MemHost(net.clone())), false, false, desigma_collab::Role::Editor);
    net.borrow_mut().host.push_back(HostEv::Ready { link: "desigma://join/abcdefghijklmnop".into() });
    for _ in 0..4 {
        s.frame(vec![]);
    }
    s.shell.session().request_restart();
    s.frame(vec![]);
    s.frame(vec![]);
    assert_eq!(s.app().dialog.as_ref().map(|d| d.title.as_str()), Some("Stop sharing?"));
    click_rect(&mut s, "dialog-cancel");
    s.frame(vec![]);
    assert!(s.app().collab.hosting() && install::take_stashed().is_none(), "Cancel keeps the session and does not restart");
    s.shell.session().request_restart();
    s.frame(vec![]);
    s.frame(vec![]);
    click_rect(&mut s, "dialog-ok");
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(!s.app().collab.active());
    assert!(install::take_stashed().is_some(), "after leaving, the clean document restarts");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}

#[test]
fn a_failed_swap_stays_open_with_the_error() {
    let _slot = LAUNCH_SLOT.lock().unwrap_or_else(|e| e.into_inner());
    let (mut s, sim_dir) = dialog_in(Install::Ready(Prepared::Setup { setup: std::env::temp_dir().join("desigma-missing-setup.exe") }));
    let _ = install::take_stashed();
    click_rect(&mut s, "updater-restart");
    s.frame(vec![]);
    s.frame(vec![]);
    assert!(s.app().updater.dialog && install::take_stashed().is_none());
    assert!(matches!(&s.app().updater.install, Install::Failed(e) if e.contains("installer is gone")));
    let _ = std::fs::remove_dir_all(&sim_dir);
}

#[test]
fn a_different_release_forgets_the_prepared_update() {
    let dir = scratch("other");
    let (mut s, sim_dir) = dialog_in(ready(&dir));
    let newer = Release { version: semver::Version::parse("99.0.0-alpha.2").unwrap(), ..release() };
    s.shell.session().finish_check(Ok(Some(release())), true);
    assert!(matches!(s.app().updater.install, Install::Ready(_)), "the same release keeps it");
    s.shell.session().finish_check(Ok(Some(newer)), true);
    assert_eq!(s.app().updater.install, Install::Idle);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&sim_dir);
}
