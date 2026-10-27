use super::*;

#[test]
fn plugin_install_form_rejects_figma_links_and_blocks_editor_shortcuts() {
    let mut s = Sim::with(Options { select: Some("Card".into()), ..Default::default() });
    s.shell.session().plugins.root = Some(std::env::temp_dir().join("desigma-plugin-ui-empty"));
    let ctx = s.ctx.clone();
    crate::panels::menus::run(s.shell.session(), &ctx, crate::panels::menus::Action::Plugins);
    s.frame(vec![]);
    s.frame(vec![]);
    let before = s.app().doc.to_json().unwrap();
    s.key(Key::Delete);
    assert_eq!(s.app().doc.to_json().unwrap(), before);
    s.click(s.rect_of("plugin-install-new").center());
    assert!(s.app().plugins.install_form);
    s.frame(vec![]);
    s.click(s.rect_of("plugin-url").center());
    s.type_text("https://www.figma.com/community/plugin/735098390272716381/iconify");
    s.click(s.rect_of("plugin-install-submit").center());
    assert!(s.app().plugins.error.is_some());
    assert!(!s.app().plugins.pending);
    assert_eq!(s.app().doc.to_json().unwrap(), before);
    s.key(Key::Escape);
    assert!(!s.app().plugins.install_form);
    assert!(s.app().plugins.open);
    s.key(Key::Escape);
    assert!(!s.app().plugins.open);
}

#[test]
fn plugins_rail_opens_library() {
    let mut s = Sim::new();
    let i = crate::panels::rail::ITEMS.iter().position(|(item, ..)| *item == crate::app::RailItem::Plugins).unwrap();
    let y = theme::TITLE_BAR_H + crate::panels::rail::cell_tops()[i] + 24.0;
    s.click(Pos2::new(24.0, y));
    assert!(s.app().plugins.open);
    s.frame(vec![]);
    assert!(s.rect_of("plugin-install-new").width() > 0.0);
}

const PLUGIN_TEST_SVG: &str =
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32" viewBox="0 0 48 32"><rect width="48" height="32" fill="#4F46E5"/></svg>"##;

#[test]
fn plugin_svg_inserts_selected_named_geometry_as_one_undo_step() {
    let mut s = Sim::new();
    let mut before = serde_json::to_value(&s.app().doc).unwrap();
    before.as_object_mut().unwrap().remove("next_id");
    let count = s.app().doc.nodes.len();
    let previous_history = s.app().hist.can_undo();
    let id = s.shell.session().insert_plugin_svg(PLUGIN_TEST_SVG, "Library icon").unwrap();
    assert_eq!(s.app().selection.len(), 1);
    let selected = s.app().selection[0];
    assert_eq!(id, selected.0.to_string());
    let root = s.app().doc.get(selected).unwrap();
    assert_eq!(root.name, "Library icon");
    assert_eq!((root.size.x, root.size.y), (48.0, 32.0));
    assert_eq!(root.children.len(), 1);
    let shape = s.app().doc.get(root.children[0]).unwrap();
    assert!(matches!(shape.kind, desigma_doc::NodeKind::Rectangle));
    assert_eq!((shape.size.x, shape.size.y), (48.0, 32.0));
    assert!(matches!(&shape.fills[0].kind, desigma_doc::PaintKind::Solid(c) if c.to_hex() == "4F46E5"));
    assert_eq!(s.app().doc.nodes.len(), count + 2);
    s.cmd_key(Key::Z);
    let mut undone = serde_json::to_value(&s.app().doc).unwrap();
    undone.as_object_mut().unwrap().remove("next_id");
    assert_eq!(undone, before, "Undo restores the full document except its monotonic ID allocator");
    assert_eq!(s.app().hist.can_undo(), previous_history);
    s.cmd_key(Key::Y);
    assert_eq!(s.app().doc.nodes.len(), count + 2);
    assert_eq!(s.app().doc.get(selected).unwrap().name, "Library icon");
}

#[test]
fn rejected_plugin_svg_preserves_document_selection_and_history() {
    let mut s = Sim::with(Options { select: Some("Card".into()), ..Default::default() });
    let before = s.app().doc.to_json().unwrap();
    let selection = s.app().selection.clone();
    let undo = s.app().hist.can_undo();
    for svg in [
        "<svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.com/photo.png"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
    ] {
        assert!(s.shell.session().insert_plugin_svg(svg, "Rejected").is_err());
        assert_eq!(s.app().doc.to_json().unwrap(), before);
        assert_eq!(s.app().selection, selection);
        assert_eq!(s.app().hist.can_undo(), undo);
    }
    assert!(s.shell.session().insert_plugin_svg(PLUGIN_TEST_SVG, "\n").is_err());
    assert_eq!(s.app().doc.to_json().unwrap(), before);
}

#[test]
fn plugin_insertion_is_blocked_during_presentation() {
    let mut s = Sim::new();
    s.shell.session().start_present();
    assert!(s.app().proto.present.is_some());
    let before = s.app().doc.to_json().unwrap();
    assert!(s.shell.session().insert_plugin_svg(PLUGIN_TEST_SVG, "Hidden mutation").is_err());
    assert_eq!(s.app().doc.to_json().unwrap(), before);
}

fn plugin_runtime_fixture(s: &mut Sim) -> String {
    use crate::plugins::{
        runtime::Runtime,
        store::{Bundle, InstalledPlugin},
    };
    let plugin = InstalledPlugin {
        id: "integration-icons".into(),
        name: "Integration icons".into(),
        version: "1.0.0".into(),
        license: "MIT".into(),
        source_url: String::new(),
        manifest_url: String::new(),
        main: "main.js".into(),
        permissions: vec!["document:insert-svg".into()],
        runtime_supported: true,
        installed_at: 0,
        schema_version: 2,
        panel: Some("index.html".into()),
        files: vec!["index.html".into(), "main.js".into()],
        network: vec![],
    };
    let bundle = Bundle {
        plugin,
        panel: "index.html".into(),
        files: std::collections::BTreeMap::from([
            ("index.html".into(), b"<!doctype html><title>Test plugin</title>".to_vec()),
            ("main.js".into(), b"void 0;".to_vec()),
        ]),
        network: vec![],
    };
    let runtime = Runtime::start(bundle, s.ctx.clone()).unwrap();
    let url = runtime.url().to_string();
    s.shell.session().plugins.runtime = Some(runtime);
    s.shell.session().plugins.running_id = Some("integration-icons".into());
    url
}

fn plugin_http_insert(url: &str) -> Result<serde_json::Value, String> {
    let url = url::Url::parse(url).unwrap();
    let token = url.path_segments().unwrap().next().unwrap();
    let endpoint = url.join("insert-svg").unwrap();
    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(7))
        .build()
        .post(endpoint.as_str())
        .set("Origin", &url.origin().ascii_serialization())
        .set("X-Desigma-Token", token)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({"svg": PLUGIN_TEST_SVG, "name":"HTTP icon"}).to_string())
        .map_err(|e| e.to_string())?;
    serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())
}

#[test]
fn plugin_loopback_request_inserts_in_editor_and_stop_revokes_session() {
    let mut s = Sim::new();
    let before = s.app().doc.nodes.len();
    let url = plugin_runtime_fixture(&mut s);
    let request_url = url.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(plugin_http_insert(&request_url)).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let response = loop {
        s.frame(vec![]);
        if let Ok(response) = receive.try_recv() {
            break response.unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "Plugin request did not complete");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    worker.join().unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["node_id"], s.app().selection[0].0.to_string());
    assert_eq!(s.app().doc.nodes.len(), before + 2);
    assert_eq!(s.app().doc.get(s.app().selection[0]).unwrap().name, "HTTP icon");
    s.shell.session().stop_plugin();
    assert!(s.app().plugins.runtime.is_none());
    assert!(s.app().plugins.running_id.is_none());
    assert!(plugin_http_insert(&url).is_err());
    assert_eq!(s.app().doc.nodes.len(), before + 2);
}

#[test]
fn switching_document_revokes_plugin_session() {
    let mut s = Sim::new();
    let url = plugin_runtime_fixture(&mut s);
    s.shell.session().new_document();
    assert!(s.app().plugins.runtime.is_none());
    assert!(s.app().plugins.running_id.is_none());
    let before = s.app().doc.to_json().unwrap();
    assert!(plugin_http_insert(&url).is_err());
    s.frame(vec![]);
    assert_eq!(s.app().doc.to_json().unwrap(), before);
}
