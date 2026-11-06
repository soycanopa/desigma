//! Components, instances, variables and themes.

mod common;
use common::*;
use desigma_doc::*;
use serde_json::json;

fn doc_of(children: serde_json::Value) -> Import {
    from_json(&json!({ "version": "2.21", "children": children }).to_string())
}

use desigma_pen::Import;

fn node<'a>(d: &'a Document, id: &str) -> &'a Node {
    d.get(by_id(d, id)).unwrap()
}

fn text(n: &Node) -> &str {
    match &n.kind {
        NodeKind::Text(t) => &t.content,
        other => panic!("not text: {other:?}"),
    }
}

fn component_of(n: &Node) -> NodeId {
    instance_component(n).unwrap_or_else(|| panic!("{} is not an instance", n.name))
}

fn spec() -> Import {
    load("components-and-themes.pen")
}

// ---------------------------------------------------------------------------- components

#[test]
fn reusable_frames_become_components() {
    let i = spec();
    let d = &i.document;
    let c = node(d, "round-button");
    assert!(matches!(c.kind, NodeKind::Component(_)));
    assert_eq!(c.name, "Round button");
    assert_eq!(kids(d, c.id).len(), 1);
    // it stays on the canvas where the file put it
    assert_eq!((c.transform.tx, c.transform.ty), (0.0, 0.0));
    assert_eq!(c.parent, Some(d.pages[0]));
}

#[test]
fn an_instance_copies_the_component_and_applies_overrides() {
    let i = spec();
    let d = &i.document;
    let comp = by_id(d, "round-button");
    let inst = node(d, "red-round-button");
    assert_eq!(component_of(inst), comp);
    assert_eq!((inst.transform.tx, inst.transform.ty), (0.0, 80.0));
    assert_eq!(solid(&inst.fills[0]).to_hex(), "FF0000");
    assert_eq!(inst.corner_radii, [9999.0; 4]);
    let label = kids(d, inst.id)[0];
    assert_eq!(text(label), "Cancel");
    // the component itself is untouched
    assert_eq!(solid(&node(d, "round-button").fills[0]).to_hex(), "2563EB");
    assert_eq!(text(kids(d, comp)[0]), "Submit");
}

#[test]
fn instances_are_linked_and_record_what_they_override() {
    let i = spec();
    let d = &i.document;
    let comp = by_id(d, "round-button");
    let inst = node(d, "red-round-button");
    assert_eq!(inst.component.source, Some(comp));
    assert!(inst.component.overrides.contains(&OverrideField::Fills));
    assert!(!inst.component.overrides.contains(&OverrideField::Transform));
    assert!(!inst.component.overrides.contains(&OverrideField::CornerRadii));
    let label = kids(d, inst.id)[0];
    assert_eq!(label.component.source, Some(kids(d, comp)[0].id));
    assert!(label.component.overrides.contains(&OverrideField::Characters));
    assert!(!label.component.overrides.contains(&OverrideField::Name));
    assert!(!label.component.overrides.contains(&OverrideField::Fills));
    assert!(instances_of(d, comp).contains(&inst.id));
}

#[test]
fn nested_instances_and_deep_overrides() {
    let i = spec();
    let d = &i.document;
    let alert = by_id(d, "alert");
    let save = node(d, "save-alert");
    assert_eq!(component_of(save), alert);
    let round = by_id(d, "round-button");
    let k = kids(d, save.id);
    assert_eq!(text(k[0]), "You have unsaved changes. Do you want to save them?");
    // children that are instances themselves stay instances of their own component
    assert_eq!(component_of(k[1]), round);
    assert_eq!(component_of(k[2]), round);
    assert_eq!(text(kids(d, k[1].id)[0]), "Save");
    let discard = kids(d, k[2].id)[0];
    assert_eq!(text(discard), "Discard Changes");
    assert_eq!(
        solid(&match &discard.kind {
            NodeKind::Text(t) => t.style.fills[0].clone(),
            _ => panic!(),
        })
        .to_hex(),
        "FF0000"
    );
    // a nested node mirrors the matching node of the alert component, not of the button
    let alert_ok = kids(d, alert)[1];
    assert_eq!(k[1].component.source, Some(alert_ok.id));
    assert_eq!(kids(d, k[1].id)[0].component.source, Some(kids(d, alert_ok.id)[0].id));
    // the alert component's own buttons say OK and Cancel
    assert_eq!(text(kids(d, alert_ok.id)[0]), "OK");
}

#[test]
fn a_component_made_from_another_component() {
    let i = spec();
    let d = &i.document;
    let c = node(d, "icon-button");
    assert!(matches!(c.kind, NodeKind::Component(_)));
    // its label was replaced by an icon
    let inner = kids(d, c.id);
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].source_id.as_deref(), Some("icon"));
    assert_eq!(inner[0].name, "lucide/check");
    assert!(i.warnings.iter().any(|w| w.contains("icon")));
}

#[test]
fn replacing_the_children_of_an_object_in_an_instance() {
    let i = spec();
    let d = &i.document;
    let menu = node(d, "menu-sidebar");
    assert_eq!(component_of(menu), by_id(d, "sidebar"));
    let parts = kids(d, menu.id);
    assert_eq!(parts.len(), 3);
    let content = parts[1];
    let buttons = kids(d, content.id);
    assert_eq!(buttons.len(), 2);
    assert_eq!(text(kids(d, buttons[0].id)[0]), "Home");
    assert_eq!(text(kids(d, buttons[1].id)[0]), "Settings");
    // the sidebar component still has its empty slot
    assert!(kids(d, kids(d, by_id(d, "sidebar"))[1].id).is_empty());
    // the replacements are instances in their own right, not mirrors of the sidebar's nodes
    assert_eq!(buttons[0].component.source, Some(by_id(d, "round-button")));
    assert_eq!(component_of(buttons[0]), by_id(d, "round-button"));
}

#[test]
fn instance_sizes_follow_their_content() {
    let i = spec();
    let d = &i.document;
    let short = node(d, "red-round-button");
    let long = kids(d, node(d, "save-alert").id)[2];
    assert!(long.size.x > short.size.x + 50.0, "{} vs {}", long.size.x, short.size.x);
    assert_eq!(short.size.y, 40.0);
}

#[test]
fn syncing_a_component_keeps_instance_overrides() {
    let i = spec();
    let mut d = i.document;
    let comp = by_id(&d, "round-button");
    let inst = by_id(&d, "red-round-button");
    let mut c = d.get(comp).unwrap().clone();
    c.opacity = 0.5;
    Command::Update { node: Box::new(c) }.apply(&mut d).unwrap();
    sync_instances(&mut d, comp).apply(&mut d).unwrap();
    let n = d.get(inst).unwrap();
    assert_eq!(n.opacity, 0.5);
    assert_eq!(solid(&n.fills[0]).to_hex(), "FF0000");
    assert_eq!(text(kids(&d, inst)[0]), "Cancel");
}

#[test]
fn reusable_shapes_that_are_not_frames_get_a_component_frame() {
    let i = doc_of(json!([
        {"id": "dot", "type": "ellipse", "reusable": true, "x": 10, "y": 20, "width": 16, "height": 16, "fill": "#FF0000"},
        {"id": "dot2", "type": "ref", "ref": "dot", "x": 100, "y": 20, "fill": "#0000FF"},
        {"id": "box", "type": "rectangle", "reusable": true, "width": 30, "height": 30, "fill": "#00FF00", "cornerRadius": 4},
        {"id": "box2", "type": "ref", "ref": "box", "x": 50, "y": 0}
    ]));
    let d = &i.document;
    // the wrapper and the shape inside it both carry the .pen id
    let dot = all_by_id(d, "dot").into_iter().map(|id| d.get(id).unwrap()).find(|n| matches!(n.kind, NodeKind::Component(_))).expect("component frame");
    assert_eq!((dot.transform.tx, dot.transform.ty, dot.size.x), (10.0, 20.0, 16.0));
    assert!(matches!(kids(d, dot.id)[0].kind, NodeKind::Ellipse(_)));
    let dot2 = all_by_id(d, "dot2").into_iter().map(|id| d.get(id).unwrap()).find(|n| matches!(n.kind, NodeKind::Instance { .. })).expect("instance frame");
    assert_eq!(component_of(dot2), dot.id);
    assert_eq!((dot2.transform.tx, dot2.transform.ty), (100.0, 20.0));
    let inner = kids(d, dot2.id)[0];
    assert_eq!(solid(&inner.fills[0]).to_hex(), "0000FF");
    assert_eq!(inner.component.source, Some(kids(d, dot.id)[0].id));
    // rectangles are components directly
    let box_ = node(d, "box");
    assert!(matches!(box_.kind, NodeKind::Component(_)));
    assert_eq!(box_.corner_radii, [4.0; 4]);
    assert_eq!(component_of(node(d, "box2")), box_.id);
    assert_eq!(node(d, "box2").fills, box_.fills);
}

#[test]
fn reusable_groups_and_instances_of_them() {
    let i = doc_of(json!([
        {"id": "g", "type": "group", "reusable": true, "children": [{"id": "r", "type": "rectangle", "x": 5, "y": 5, "width": 20, "height": 10, "fill": "#000"}]},
        {"id": "g2", "type": "ref", "ref": "g", "x": 100, "y": 0}
    ]));
    let d = &i.document;
    assert!(matches!(node(d, "g").kind, NodeKind::Component(_)));
    assert_eq!(node(d, "g").size.x, 25.0);
    let g2 = node(d, "g2");
    assert!(matches!(g2.kind, NodeKind::Instance { .. }));
    assert_eq!((g2.size.x, g2.size.y), (25.0, 15.0));
}

#[test]
fn a_component_used_before_it_is_defined() {
    let i = doc_of(json!([
        {"id": "use", "type": "ref", "ref": "later", "x": 0, "y": 0},
        {"id": "later", "type": "frame", "reusable": true, "width": 10, "height": 10, "fill": "#123456"}
    ]));
    let d = &i.document;
    assert_eq!(top(d).len(), 2);
    assert_eq!(component_of(node(d, "use")), by_id(d, "later"));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    // the component is on the page exactly once
    assert_eq!(top(d).iter().filter(|t| **t == by_id(d, "later")).count(), 1);
}

#[test]
fn components_nested_in_frames_stay_where_they_are() {
    let i = doc_of(json!([{"id": "page", "type": "frame", "width": 100, "height": 100, "children": [
        {"id": "inner", "type": "frame", "reusable": true, "width": 10, "height": 10},
        {"id": "copy", "type": "ref", "ref": "inner"}
    ]}]));
    let d = &i.document;
    let page = node(d, "page");
    assert_eq!(page.children.len(), 2);
    assert!(matches!(d.get(page.children[0]).unwrap().kind, NodeKind::Component(_)));
    assert!(matches!(d.get(page.children[1]).unwrap().kind, NodeKind::Instance { .. }));
}

#[test]
fn circular_and_missing_references_do_not_hang_or_panic() {
    let i = doc_of(json!([
        {"id": "a", "type": "frame", "reusable": true, "width": 10, "height": 10, "children": [{"id": "ra", "type": "ref", "ref": "b"}]},
        {"id": "b", "type": "frame", "reusable": true, "width": 10, "height": 10, "children": [{"id": "rb", "type": "ref", "ref": "a"}]},
        {"id": "self", "type": "frame", "reusable": true, "width": 10, "height": 10, "children": [{"id": "rs", "type": "ref", "ref": "self"}]},
        {"id": "ghost", "type": "ref", "ref": "nothing", "width": 40, "height": 20},
        {"id": "lib", "type": "ref", "ref": "ui:button", "width": 40, "height": 20},
        {"id": "plain", "type": "frame", "width": 10, "height": 10},
        {"id": "copy", "type": "ref", "ref": "plain"},
        {"id": "x", "type": "ref"},
        {"id": "ca", "type": "ref", "ref": "ca2"},
        {"id": "ca2", "type": "ref", "ref": "ca", "reusable": true}
    ]));
    assert!(!i.warnings.is_empty());
    let d = &i.document;
    let ghost = node(d, "ghost");
    assert!(matches!(ghost.kind, NodeKind::Frame(_)));
    assert_eq!((ghost.size.x, ghost.size.y), (40.0, 20.0));
    assert!(ghost.name.starts_with("Missing component"));
    assert!(i.warnings.iter().any(|w| w.contains("imported libraries")));
}

#[test]
fn components_of_components_cannot_blow_up_memory() {
    // Each level doubles the number of instances below it: 2^40 objects without a limit.
    let mut kids = vec![json!({"id": "c0", "type": "frame", "reusable": true, "width": 4, "height": 4})];
    for n in 1..40 {
        kids.push(json!({"id": format!("c{n}"), "type": "frame", "reusable": true, "width": 4, "height": 4, "layout": "none", "children": [
            {"id": "l", "type": "ref", "ref": format!("c{}", n - 1)}, {"id": "r", "type": "ref", "ref": format!("c{}", n - 1)}]}));
    }
    let json = json!({"version": "2.21", "children": kids}).to_string();
    let i = desigma_pen::import_with(json.as_bytes(), &desigma_pen::Options { max_objects: Some(5_000), ..Default::default() }).unwrap();
    assert!(i.document.nodes.len() < 6_000, "{}", i.document.nodes.len());
    assert!(i.warnings.iter().any(|w| w.contains("more than 5000 objects")), "{:?}", i.warnings);
}

// ---------------------------------------------------------------------------- variables and themes

#[test]
fn variables_become_collections_with_modes() {
    let i = spec();
    let d = &i.document;
    let mode = &d.variable_collections["pen:mode"];
    assert_eq!(mode.modes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["light", "dark"]);
    assert_eq!(mode.default_mode, "light");
    assert_eq!(mode.variables, vec!["pen:color.background".to_string(), "pen:color.text".to_string()]);
    assert_eq!(d.variables["pen:color.background"].kind, VariableKind::Color);
    assert_eq!(d.variables["pen:color.background"].values["dark"], VariableValue::Color(Color::BLACK));
    let spacing = &d.variable_collections["pen:spacing"];
    assert_eq!(spacing.default_mode, "regular");
    assert_eq!(d.variables["pen:text.title"].values["condensed"], VariableValue::Number(36.0));
    assert_eq!(d.variables["pen:radius.pill"].values["default"], VariableValue::Number(9999.0));
    assert_eq!(d.variables["pen:radius.pill"].collection, "pen:default");
}

#[test]
fn properties_that_name_a_variable_are_bound_to_it() {
    let i = spec();
    let d = &i.document;
    let page = node(d, "landing-page-light");
    assert_eq!(page.variables.bindings["fills/0"], "pen:color.background");
    let label = node(d, "welcome-label");
    assert_eq!(label.variables.bindings["fills/0"], "pen:color.text");
    assert_eq!(label.variables.bindings["fontSize"], "pen:text.title");
    // the stored value is the default theme's
    assert_eq!(solid(&page.fills[0]).to_hex(), "FFFFFF");
    match &label.kind {
        NodeKind::Text(t) => assert_eq!(t.style.font_size, 72.0),
        _ => panic!(),
    }
    let c = node(d, "round-button");
    assert_eq!(c.variables.bindings["topLeftRadius"], "pen:radius.pill");
    assert_eq!(c.variables.bindings.len(), 4);
}

#[test]
fn a_nodes_theme_selects_modes_for_its_subtree() {
    let i = spec();
    let d = &i.document;
    assert!(node(d, "landing-page-light").variables.modes.is_empty());
    let dark = node(d, "landing-page-dark");
    assert_eq!(dark.variables.modes["pen:mode"], "dark");
    let both = node(d, "landing-page-dark-condensed");
    assert_eq!(both.variables.modes["pen:mode"], "dark");
    assert_eq!(both.variables.modes["pen:spacing"], "condensed");
    // the document resolves what the nodes show
    let label = by_id(d, "welcome-label");
    let dark_label = by_id(d, "welcome-label-dark");
    let small_label = by_id(d, "welcome-label-condensed");
    assert_eq!(d.bound_value(label, "fills/0"), Some(VariableValue::Color(Color::rgb(0x33, 0x33, 0x33))));
    assert_eq!(d.bound_value(dark_label, "fills/0"), Some(VariableValue::Color(Color::rgb(0xAA, 0xAA, 0xAA))));
    assert_eq!(d.bound_value(dark_label, "fontSize"), Some(VariableValue::Number(72.0)));
    assert_eq!(d.bound_value(small_label, "fontSize"), Some(VariableValue::Number(36.0)));
}

#[test]
fn rendering_follows_the_theme() {
    let i = spec();
    let d = &i.document;
    let pixel = |id: &str| {
        let pm = desigma_render::render_node(d, by_id(d, id), 1.0).unwrap();
        let p = pm.pixel(470, 190).unwrap();
        (p.red(), p.green(), p.blue())
    };
    assert_eq!(pixel("landing-page-light"), (255, 255, 255));
    assert_eq!(pixel("landing-page-dark"), (0, 0, 0));
}

#[test]
fn variables_that_refer_to_variables_become_aliases() {
    let i = from_json(
        &json!({"version": "2.21", "themes": {"mode": ["light", "dark"]},
        "variables": {
            "brand": {"type": "color", "value": [{"value": "#FF0000", "theme": {"mode": "light"}}, {"value": "#880000", "theme": {"mode": "dark"}}]},
            "button": {"type": "color", "value": "$brand"},
            "gap": {"type": "number", "value": 12},
            "flag": {"type": "boolean", "value": false},
            "label": {"type": "string", "value": "Hello"}
        },
        "children": [{"id": "f", "type": "frame", "width": 50, "height": 50, "fill": "$button", "gap": "$gap", "enabled": "$flag",
            "children": [{"id": "t", "type": "text", "content": "$label", "fill": "#000"}]}]})
        .to_string(),
    );
    let d = &i.document;
    assert_eq!(d.variables["pen:button"].values["default"], VariableValue::Alias("pen:brand".into()));
    assert_eq!(d.variables["pen:flag"].kind, VariableKind::Boolean);
    let f = node(d, "f");
    assert_eq!(f.variables.bindings["itemSpacing"], "pen:gap");
    assert_eq!(f.variables.bindings["visible"], "pen:flag");
    assert!(!f.visible);
    assert_eq!(solid(&f.fills[0]).to_hex(), "FF0000");
    let t = node(d, "t");
    assert_eq!(t.variables.bindings["characters"], "pen:label");
    assert_eq!(text(t), "Hello");
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn variables_in_unbindable_places_are_resolved_for_the_theme_in_effect() {
    let i = from_json(
        &json!({"version": "2.21", "themes": {"mode": ["light", "dark"]},
        "variables": {"o": {"type": "number", "value": [{"value": 1, "theme": {"mode": "light"}}, {"value": 0.25, "theme": {"mode": "dark"}}]}},
        "children": [
            {"id": "a", "type": "rectangle", "width": 1, "height": 1, "opacity": "$o"},
            {"id": "b", "type": "rectangle", "width": 1, "height": 1, "opacity": "$o", "theme": {"mode": "dark"}}
        ]})
        .to_string(),
    );
    assert_eq!(node(&i.document, "a").opacity, 1.0);
    assert_eq!(node(&i.document, "b").opacity, 0.25);
}

#[test]
fn unknown_variables_are_reported() {
    let i = doc_of(json!([{"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": "$nope", "cornerRadius": "$missing"}]));
    assert!(node(&i.document, "a").fills.is_empty());
    assert!(i.warnings.len() >= 2, "{:?}", i.warnings);
}

#[test]
fn width_and_height_variables_bind() {
    let i = from_json(
        &json!({"version": "2.21", "variables": {"w": {"type": "number", "value": 64}},
        "children": [{"id": "a", "type": "rectangle", "width": "$w", "height": 10}]})
        .to_string(),
    );
    let a = node(&i.document, "a");
    assert_eq!(a.size.x, 64.0);
    assert_eq!(a.variables.bindings["width"], "pen:w");
}
