use desigma_doc::*;
use desigma_fig::import_rest_json;
use serde_json::json;

fn find<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node named {name}"))
}

fn child<'a>(doc: &'a Document, parent: &Node, name: &str) -> &'a Node {
    parent.children.iter().map(|c| doc.get(*c).unwrap()).find(|n| n.name == name).unwrap_or_else(|| panic!("no child {name}"))
}

fn rect(id: &str, name: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "type": "RECTANGLE", "size": {"x": 10, "y": 10}})
}

fn file() -> serde_json::Value {
    let comp_children = |prefix: &str, label: &str, icon_visible: bool| {
        json!([
            rect(&format!("{prefix}1:3"), "Bg"),
            {"id": format!("{prefix}1:4"), "name": "Label", "type": "TEXT", "characters": label,
             "componentPropertyReferences": {"characters": "Label#1:8"}},
            {"id": format!("{prefix}1:5"), "name": "Icon", "type": "ELLIPSE", "visible": icon_visible,
             "componentPropertyReferences": {"visible": "Show icon#1:9"}}
        ])
    };
    json!({
        "name": "props",
        "document": {"type": "DOCUMENT", "children": [{"id": "0:1", "type": "CANVAS", "name": "P", "children": [
            {"id": "1:1", "name": "Btn", "type": "COMPONENT_SET",
             "componentPropertyDefinitions": {
                "Size": {"type": "VARIANT", "defaultValue": "Small", "variantOptions": ["Small", "Large"]},
                "Show icon#1:9": {"type": "BOOLEAN", "defaultValue": true},
                "Label#1:8": {"type": "TEXT", "defaultValue": "Go"},
                "Swap#1:7": {"type": "INSTANCE_SWAP", "defaultValue": "1:2"}
             },
             "children": [
                {"id": "1:2", "name": "Size=Small", "type": "COMPONENT", "size": {"x": 100, "y": 40},
                 "children": comp_children("", "Go", true)}
             ]},
            {"id": "2:1", "name": "Btn instance", "type": "INSTANCE", "componentId": "1:2", "size": {"x": 100, "y": 40},
             "componentProperties": {
                "Show icon#1:9": {"type": "BOOLEAN", "value": false},
                "Label#1:8": {"type": "TEXT", "value": "Buy"},
                "Size": {"type": "VARIANT", "value": "Small"},
                "Swap#1:7": {"type": "INSTANCE_SWAP", "value": "1:2"}
             },
             "overrides": [
                {"id": "I2:1;1:4", "overriddenFields": ["characters", "fills"]},
                {"id": "I2:1;1:5", "overriddenFields": ["visible"]},
                {"id": "2:1", "overriddenFields": ["name"]}
             ],
             "children": comp_children("I2:1;", "Buy", false)}
        ]}]}
    })
}

#[test]
fn maps_property_definitions_values_and_references() {
    let doc = import_rest_json(&file().to_string()).unwrap();
    let set = find(&doc, "Btn");
    let defs = &set.component.props;
    assert_eq!(defs.len(), 4);
    let d = |n: &str| defs.iter().find(|d| d.name == n).unwrap_or_else(|| panic!("{n}"));
    assert_eq!(d("Show icon").kind, PropKind::Boolean);
    assert_eq!(d("Show icon").default, PropValue::Bool(true));
    assert_eq!(d("Label").default, PropValue::Text("Go".into()));
    assert_eq!(d("Size").kind, PropKind::Variant { options: vec!["Small".into(), "Large".into()] });
    assert_eq!(d("Size").default, PropValue::Variant("Small".into()));
    let comp = find(&doc, "Size=Small");
    assert_eq!(d("Swap").default, PropValue::Instance(comp.id), "instance-swap defaults resolve to node ids");

    let label = child(&doc, comp, "Label");
    assert_eq!(label.component.bindings.get(&OverrideField::Characters).map(String::as_str), Some("Label"));
    assert_eq!(child(&doc, comp, "Icon").component.bindings.get(&OverrideField::Visible).map(String::as_str), Some("Show icon"));

    let inst = find(&doc, "Btn instance");
    assert_eq!(inst.component.values["Show icon"], PropValue::Bool(false));
    assert_eq!(inst.component.values["Label"], PropValue::Text("Buy".into()));
    assert_eq!(inst.component.values["Size"], PropValue::Variant("Small".into()));
    assert_eq!(inst.component.values["Swap"], PropValue::Instance(comp.id));
    // The set's definitions are visible from the variant component.
    assert_eq!(prop_defs(&doc, comp.id).len(), 4);
}

#[test]
fn rest_export_writes_component_properties() {
    let doc = import_rest_json(&file().to_string()).unwrap();
    let out = desigma_fig::export_rest_json(&doc);
    let by_id = exported_by_id(&out);
    let inst = by_id["2:1"];
    assert_eq!(
        inst["componentProperties"],
        json!({
            "Show icon": {"type": "BOOLEAN", "value": false},
            "Label": {"type": "TEXT", "value": "Buy"},
            "Size": {"type": "VARIANT", "value": "Small"},
            "Swap": {"type": "INSTANCE_SWAP", "value": "1:2"}
        })
    );
    let defs = &by_id["1:1"]["componentPropertyDefinitions"];
    assert_eq!(defs["Show icon"], json!({"type": "BOOLEAN", "defaultValue": true}));
    assert_eq!(defs["Size"], json!({"type": "VARIANT", "defaultValue": "Small", "variantOptions": ["Small", "Large"]}));
    assert_eq!(defs["Swap"], json!({"type": "INSTANCE_SWAP", "defaultValue": "1:2"}));
    assert_eq!(by_id["1:4"]["componentPropertyReferences"], json!({"characters": "Label"}));
    assert_eq!(by_id["I2:1;1:5"]["componentPropertyReferences"], json!({"visible": "Show icon"}));

    // Exported properties import back to the same values.
    let again = import_rest_json(&out.to_string()).unwrap();
    assert_eq!(find(&again, "Btn instance").component.values, find(&doc, "Btn instance").component.values);
}

#[test]
fn rest_export_lists_unassigned_properties_at_their_default() {
    let mut j = file();
    j["document"]["children"][0]["children"].as_array_mut().unwrap().push(json!(
        {"id": "3:1", "name": "Plain instance", "type": "INSTANCE", "componentId": "1:2", "size": {"x": 100, "y": 40},
         "componentProperties": {"Label#1:8": {"type": "TEXT", "value": "Ok"}}}
    ));
    let doc = import_rest_json(&j.to_string()).unwrap();
    let out = desigma_fig::export_rest_json(&doc);
    assert_eq!(
        exported_by_id(&out)["3:1"]["componentProperties"],
        json!({
            "Show icon": {"type": "BOOLEAN", "value": true},
            "Label": {"type": "TEXT", "value": "Ok"},
            "Size": {"type": "VARIANT", "value": "Small"},
            "Swap": {"type": "INSTANCE_SWAP", "value": "1:2"}
        })
    );
}

fn exported_by_id(out: &serde_json::Value) -> std::collections::HashMap<String, &serde_json::Value> {
    let mut by_id = std::collections::HashMap::new();
    let mut stack = vec![&out["document"]];
    while let Some(n) = stack.pop() {
        if let Some(id) = n["id"].as_str() {
            by_id.insert(id.to_string(), n);
        }
        stack.extend(n["children"].as_array().into_iter().flatten());
    }
    by_id
}

#[test]
fn links_children_and_records_rest_overrides() {
    let mut doc = import_rest_json(&file().to_string()).unwrap();
    let comp = find(&doc, "Size=Small").clone();
    let inst = find(&doc, "Btn instance").clone();
    assert_eq!(inst.component.source, Some(comp.id));
    assert_eq!(inst.component.overrides, [OverrideField::Name].into_iter().collect());
    for name in ["Bg", "Label", "Icon"] {
        assert_eq!(child(&doc, &inst, name).component.source, Some(child(&doc, &comp, name).id), "{name}");
    }
    let label = child(&doc, &inst, "Label");
    assert_eq!(label.component.overrides, [OverrideField::Characters, OverrideField::TextStyle].into_iter().collect());
    assert_eq!(child(&doc, &inst, "Icon").component.overrides, [OverrideField::Visible].into_iter().collect());
    assert!(child(&doc, &inst, "Bg").component.overrides.is_empty());

    // The imported linkage is what sync_instances works with: no structural fix-ups.
    let Command::Batch(v) = sync_instances(&mut doc, comp.id) else { panic!() };
    for c in &v {
        assert!(matches!(c, Command::Update { .. }), "unexpected structural change: {c:?}");
    }
}

#[test]
fn nested_instance_sources_follow_inline_ids() {
    let j = json!({"document": {"type": "DOCUMENT", "children": [{"id": "0:1", "type": "CANVAS", "children": [
        {"id": "1:1", "name": "Inner", "type": "COMPONENT", "size": {"x": 5, "y": 5}, "children": [rect("1:2", "Dot")]},
        {"id": "2:1", "name": "Outer", "type": "COMPONENT", "size": {"x": 50, "y": 50}, "children": [
            {"id": "2:2", "name": "N", "type": "INSTANCE", "componentId": "1:1", "size": {"x": 5, "y": 5}, "children": [rect("I2:2;1:2", "Dot")]}
        ]},
        {"id": "3:1", "name": "Use", "type": "INSTANCE", "componentId": "2:1", "size": {"x": 50, "y": 50}, "children": [
            {"id": "I3:1;2:2", "name": "N", "type": "INSTANCE", "componentId": "1:1", "size": {"x": 5, "y": 5}, "children": [rect("I3:1;2:2;1:2", "Dot")]}
        ]}
    ]}]}});
    let doc = import_rest_json(&j.to_string()).unwrap();
    let outer = find(&doc, "Outer");
    let n_main = doc.get(outer.children[0]).unwrap();
    let inst = find(&doc, "Use");
    let n_copy = doc.get(inst.children[0]).unwrap();
    assert_eq!(n_copy.component.source, Some(n_main.id));
    let dot_main = doc.get(n_main.children[0]).unwrap();
    let dot_copy = doc.get(n_copy.children[0]).unwrap();
    assert_eq!(dot_copy.component.source, Some(dot_main.id));
}
