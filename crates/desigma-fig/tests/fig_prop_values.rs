//! Component property values from `.fig` files: newer files keep defaults and
//! instance assignments only in a typed `varValue`, older ones in a plain
//! `initialValue` / `value`. Both must reach the instance and its copies.
//! Children bind to properties through `componentPropRefs` or, in newer
//! files, through PROP_REF entries of `parameterConsumptionMap`.

use desigma_doc::*;
use desigma_fig::container::Compression;
use desigma_fig::import_fig;
use desigma_fig::testkit::prop_values_fig;

fn find<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node named {name}"))
}

fn child<'a>(doc: &'a Document, parent: &Node, name: &str) -> &'a Node {
    parent.children.iter().map(|c| doc.get(*c).unwrap()).find(|n| n.name == name).unwrap_or_else(|| panic!("{} has no child {name}", parent.name))
}

fn component_of(n: &Node) -> Option<NodeId> {
    match &n.kind {
        NodeKind::Instance { component, .. } => *component,
        other => panic!("{} is not an instance: {other:?}", n.name),
    }
}

fn text(n: &Node) -> &str {
    match &n.kind {
        NodeKind::Text(t) => &t.content,
        other => panic!("{} is not text: {other:?}", n.name),
    }
}

#[test]
fn var_value_defaults_are_read() {
    let doc = import_fig(&prop_values_fig(Compression::Zstd)).unwrap();
    let gear = find(&doc, "Icon / settings");
    let defs = &find(&doc, "Card").component.props;
    let d = |n: &str| defs.iter().find(|d| d.name == n).unwrap_or_else(|| panic!("no prop {n}"));
    assert_eq!(d("Show badge").default, PropValue::Bool(true));
    assert_eq!(d("Icon").default, PropValue::Instance(gear.id));
    assert_eq!(d("Label").default, PropValue::Text("Go".into()));
}

#[test]
fn assignments_apply_in_both_shapes() {
    let doc = import_fig(&prop_values_fig(Compression::Zstd)).unwrap();
    let users = find(&doc, "Icon / users");
    for (name, label) in [("Card A", "Buy"), ("Card B", "Old"), ("Card C", "New")] {
        let card = find(&doc, name);
        assert_eq!(card.component.values.get("Show badge"), Some(&PropValue::Bool(false)), "{name}");
        assert_eq!(card.component.values.get("Icon"), Some(&PropValue::Instance(users.id)), "{name}");
        assert_eq!(card.component.values.get("Label"), Some(&PropValue::Text(label.into())), "{name}");

        assert!(!child(&doc, card, "Badge").visible, "{name}: boolean property hides the badge");
        let slot = child(&doc, card, "Slot");
        assert_eq!(component_of(slot), Some(users.id), "{name}: instance-swap property swaps the slot");
        assert_eq!(text(child(&doc, card, "Label")), label, "{name}: text property sets the label");
    }
}
