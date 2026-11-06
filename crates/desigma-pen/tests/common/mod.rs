#![allow(dead_code)]

use desigma_doc::*;
use desigma_pen::{import_with, Import, Options};
use std::path::PathBuf;

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn load(name: &str) -> Import {
    let dir = fixture_dir();
    let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    import_with(&bytes, &Options { name: Some(name.into()), base_dir: Some(dir), ..Default::default() }).unwrap_or_else(|e| panic!("{name}: {e}"))
}

pub fn from_json(json: &str) -> Import {
    import_with(json.as_bytes(), &Options::default()).expect("imports")
}

pub fn top(doc: &Document) -> Vec<NodeId> {
    doc.get(doc.pages[0]).unwrap().children.clone()
}

/// First node whose `.pen` id is `id`.
pub fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.nodes.values().find(|n| n.source_id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no node {id}")).id
}

/// All nodes whose `.pen` id is `id`.
pub fn all_by_id(doc: &Document, id: &str) -> Vec<NodeId> {
    doc.nodes.values().filter(|n| n.source_id.as_deref() == Some(id)).map(|n| n.id).collect()
}

pub fn kids(doc: &Document, id: NodeId) -> Vec<&Node> {
    doc.get(id).unwrap().children.iter().map(|c| doc.get(*c).unwrap()).collect()
}

pub fn kind_name(k: &NodeKind) -> &'static str {
    match k {
        NodeKind::Frame(_) => "Frame",
        NodeKind::Group => "Group",
        NodeKind::Rectangle => "Rectangle",
        NodeKind::Ellipse(_) => "Ellipse",
        NodeKind::Polygon { .. } => "Polygon",
        NodeKind::Vector { .. } => "Vector",
        NodeKind::Text(_) => "Text",
        NodeKind::Component(_) => "Component",
        NodeKind::Instance { .. } => "Instance",
        NodeKind::Unknown { .. } => "Unknown",
        _ => "Other",
    }
}

pub fn dump(doc: &Document) -> String {
    let mut s = String::new();
    for p in &doc.pages {
        doc.walk(*p, |n, depth| {
            s.push_str(&format!(
                "{}{} \"{}\" at ({:.1},{:.1}) {:.1}x{:.1}{}\n",
                "  ".repeat(depth),
                kind_name(&n.kind),
                n.name,
                n.transform.tx,
                n.transform.ty,
                n.size.x,
                n.size.y,
                if n.visible { "" } else { " (hidden)" }
            ));
        });
    }
    s
}

pub fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

pub fn solid(p: &Paint) -> Color {
    match &p.kind {
        PaintKind::Solid(c) => *c,
        other => panic!("not solid: {other:?}"),
    }
}
