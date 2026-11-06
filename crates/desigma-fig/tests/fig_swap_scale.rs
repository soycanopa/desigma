//! A nested instance made with the scale tool (`uniformScaleFactor`) keeps its
//! scale when an instance override or property swaps it to another component.
//! Hand-written kiwi-shaped JSON; nothing here comes from a real file.

use desigma_doc::*;
use desigma_fig::import_fig_node_changes;
use serde_json::{json, Value};

fn g(s: u64, l: u64) -> Value {
    json!({"sessionID": s, "localID": l})
}

fn node(id: (u64, u64), parent: (u64, u64), pos: &str, ty: &str, name: &str, extra: Value) -> Value {
    let mut v = json!({
        "guid": g(id.0, id.1), "parentIndex": {"guid": g(parent.0, parent.1), "position": pos},
        "type": ty, "name": name, "phase": "CREATED", "visible": true, "opacity": 1.0,
        "size": {"x": 20.0, "y": 20.0},
        "transform": {"m00": 1.0, "m01": 0.0, "m02": 0.0, "m10": 0.0, "m11": 1.0, "m12": 0.0}
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn glyph(id: (u64, u64), parent: (u64, u64), name: &str) -> Value {
    node(
        id,
        parent,
        "a",
        "ROUNDED_RECTANGLE",
        name,
        json!({"size": {"x": 10.0, "y": 10.0}, "transform": {"m00": 1.0, "m01": 0.0, "m02": 4.0, "m10": 0.0, "m11": 1.0, "m12": 4.0}}),
    )
}

fn fixture() -> Value {
    // "Card" nests "Icon A" scaled 2x; "Card 1" swaps that slot to "Icon B".
    json!([
        node((0, 0), (0, 0), "!", "DOCUMENT", "Document", json!({})),
        node((0, 1), (0, 0), "a", "CANVAS", "Page", json!({})),
        node((5, 1), (0, 1), "a", "SYMBOL", "Icon A", json!({})),
        glyph((5, 11), (5, 1), "Glyph"),
        node((5, 2), (0, 1), "b", "SYMBOL", "Icon B", json!({})),
        glyph((5, 21), (5, 2), "Dot"),
        node((5, 3), (0, 1), "c", "SYMBOL", "Card", json!({"size": {"x": 100.0, "y": 40.0}})),
        node(
            (5, 4),
            (5, 3),
            "a",
            "INSTANCE",
            "slot",
            json!({"size": {"x": 40.0, "y": 40.0}, "symbolData": {"symbolID": g(5, 1), "uniformScaleFactor": 2.0, "symbolOverrides": []}}),
        ),
        node(
            (1, 1),
            (0, 1),
            "d",
            "INSTANCE",
            "Card 1",
            json!({"size": {"x": 100.0, "y": 40.0}, "symbolData": {"symbolID": g(5, 3), "symbolOverrides": [
                {"guidPath": {"guids": [g(5, 4)]}, "overriddenSymbolID": g(5, 2)}]}}),
        ),
        node((1, 2), (0, 1), "e", "INSTANCE", "Card 2", json!({"size": {"x": 100.0, "y": 40.0}, "symbolData": {"symbolID": g(5, 3)}})),
    ])
}

fn find<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}"))
}

fn child<'a>(doc: &'a Document, n: &Node, name: &str) -> &'a Node {
    n.children.iter().map(|c| doc.get(*c).unwrap()).find(|c| c.name == name).unwrap_or_else(|| panic!("{} has no child {name}", n.name))
}

#[test]
fn swapped_nested_instance_keeps_its_scale() {
    let doc = import_fig_node_changes(&fixture(), "t").unwrap();
    // Unswapped: the slot shows Icon A at 2x.
    let glyph = child(&doc, child(&doc, find(&doc, "Card 2"), "slot"), "Glyph");
    assert_eq!(glyph.size, Vec2::new(20.0, 20.0));
    // Swapped to Icon B: still 2x.
    let dot = child(&doc, child(&doc, find(&doc, "Card 1"), "slot"), "Dot");
    assert_eq!(dot.size, Vec2::new(20.0, 20.0), "the swap rebuilt the slot without its scale");
    assert_eq!(dot.transform.apply(Vec2::new(0.0, 0.0)), Vec2::new(8.0, 8.0));
}
