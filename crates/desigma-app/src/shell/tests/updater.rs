//! UI tests for update notifications: the banner, the dialog and the menu entries (no network: the found
//! release is injected). The "Update now" states and the restart are in `updater/install.rs`.

use super::*;
use crate::comments::settings;
use crate::panels::menus::{self, Action};
use crate::updater::Release;

mod install;

fn release() -> Release {
    Release {
        version: semver::Version::parse("99.0.0-alpha.1").unwrap(),
        page: "https://github.com/deepfounder-ai/desigma/releases/tag/v99.0.0-alpha.1".into(),
        notes: (1..=10).map(|i| format!("• change {i}")).collect(),
        assets: vec![],
    }
}

/// A Sim with its own library directory (removed by the caller).
fn sim(home: bool) -> (Sim, std::path::PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("desigma_updater_ui_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Sim::with(Options { home, library: Some(dir.clone()), ..Default::default() });
    s.shell.session().updater.available = Some(release());
    s.frame(vec![]);
    (s, dir)
}

fn click_rect(s: &mut Sim, name: &str) {
    let r = s.rect_of(name);
    s.click(r.center());
}

#[test]
fn the_banner_opens_the_dialog_and_later_hides_both() {
    for home in [false, true] {
        let (mut s, dir) = sim(home);
        assert_eq!(s.app().home.is_some(), home);
        let banner = s.rect_of("updater-banner");
        assert!((banner.center().x - 800.0).abs() < 1.0 && banner.top() < 120.0, "top centre: {banner:?}");
        click_rect(&mut s, "updater-view");
        assert!(s.app().updater.dialog);
        // keys go to the dialog, not to the canvas behind it
        s.key(Key::R);
        assert_eq!(s.app().tool, Tool::Move);
        click_rect(&mut s, "updater-later");
        assert!(!s.app().updater.dialog && s.app().updater.dismissed);
        assert!(s.app().updater.available.is_some(), "Later keeps the release for Help → Check for updates");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_dialog_shows_a_short_excerpt_of_the_notes() {
    let (mut s, dir) = sim(false);
    click_rect(&mut s, "updater-view");
    let dialog = s.rect_of("updater-dialog");
    // title, "You have", "What's changed", 8 of the 10 notes and "and 2 more", buttons: well inside the window
    assert!(dialog.height() > 250.0 && dialog.height() < 500.0, "{dialog:?}");
    let texts: Vec<String> = s
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    for want in [
        "desigma 99.0.0-alpha.1 is available",
        "What's changed",
        "• change 8",
        "and 2 more on the release page",
        "Skip this version",
        "Update now",
        "Open release page",
    ] {
        assert!(texts.iter().any(|t| t == want), "missing {want:?} in {texts:?}");
    }
    assert!(!texts.iter().any(|t| t == "• change 9"));
    s.key(Key::Escape);
    assert!(!s.app().updater.dialog);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn skip_this_version_is_remembered() {
    let (mut s, dir) = sim(false);
    click_rect(&mut s, "updater-view");
    click_rect(&mut s, "updater-skip");
    assert!(s.app().updater.available.is_none() && !s.app().updater.dialog);
    assert_eq!(s.app().updater.skip.as_deref(), Some("99.0.0-alpha.1"));
    assert_eq!(settings::load_string(&dir, "skip_update").as_deref(), Some("99.0.0-alpha.1"));
    // the next launch reads it back
    let mut state = crate::updater::UpdaterState::default();
    state.start(Some(&dir), false);
    assert_eq!(state.skip.as_deref(), Some("99.0.0-alpha.1"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn menu_entries_check_and_toggle() {
    let (mut s, dir) = sim(false);
    assert!(!menus::palette_entries(s.app(), "check for updates").is_empty());
    let ctx = s.ctx.clone();
    menus::run(s.shell.session(), &ctx, Action::ToggleUpdateCheck);
    assert!(s.app().updater.no_auto_check);
    assert_eq!(settings::load_flag(&dir, "check_updates"), Some(false));
    if !crate::RELEASE_BUILD {
        menus::run(s.shell.session(), &ctx, Action::CheckForUpdates);
        assert!(s.app().toast.as_ref().is_some_and(|t| t.text.contains("development build")));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
