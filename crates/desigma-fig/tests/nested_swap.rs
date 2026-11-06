//! A `symbolOverrides` entry with `overriddenSymbolID` swaps a nested instance
//! inside an instance; deeper overrides then address the swapped subtree.

use desigma_doc::*;
use desigma_fig::container::Compression;
use desigma_fig::import_fig;
use desigma_fig::testkit::nested_swap_fig;

fn find<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node named {name}"))
}

fn child<'a>(doc: &'a Document, parent: &Node, name: &str) -> &'a Node {
    parent.children.iter().map(|c| doc.get(*c).unwrap()).find(|n| n.name == name).unwrap_or_else(|| panic!("{} has no child {name}", parent.name))
}

fn children(doc: &Document, n: &Node) -> Vec<String> {
    n.children.iter().map(|c| doc.get(*c).unwrap().name.clone()).collect()
}

fn component_of(n: &Node) -> Option<NodeId> {
    match &n.kind {
        NodeKind::Instance { component, .. } => *component,
        other => panic!("{} is not an instance: {other:?}", n.name),
    }
}

#[test]
fn overridden_symbol_id_swaps_the_nested_instance() {
    let doc = import_fig(&nested_swap_fig(Compression::Zstd)).unwrap();
    let users = find(&doc, "Icon / users");
    let gear = find(&doc, "Icon / settings");

    // Row A swaps its icon slot to `Icon / users` and recolours one of its children.
    let icon_a = child(&doc, find(&doc, "Row A"), "Icon");
    assert_eq!(component_of(icon_a), Some(users.id), "slot points at the swapped component");
    assert_eq!(children(&doc, icon_a), vec!["People", "People2"], "children come from the swapped component");
    let people = child(&doc, icon_a, "People");
    match people.fills.first().map(|p| &p.kind) {
        Some(PaintKind::Solid(c)) => assert_eq!(*c, Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, "deeper override applied after the swap"),
        other => panic!("unexpected fill {other:?}"),
    }

    // Row B has no overrides and keeps the component's default icon.
    let icon_b = child(&doc, find(&doc, "Row B"), "Icon");
    assert_eq!(component_of(icon_b), Some(gear.id));
    assert_eq!(children(&doc, icon_b), vec!["Gear"]);
}

#[test]
fn swapped_instance_takes_the_new_name_unless_renamed() {
    let doc = import_fig(&nested_swap_fig(Compression::Zstd)).unwrap();
    let users = find(&doc, "Icon / users");
    // "Icon" was renamed in the main component, so it keeps that name after the swap ...
    assert_eq!(children(&doc, find(&doc, "Row A")), vec!["Icon", "Icon / users"]);
    // ... while the slot still named after its component follows the swap.
    let swapped = child(&doc, find(&doc, "Row A"), "Icon / users");
    assert_eq!(component_of(swapped), Some(users.id));
    assert_eq!(children(&doc, find(&doc, "Row B")), vec!["Icon", "Icon / settings"]);
}
