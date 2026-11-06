//! `.fig` groups: a FRAME with `resizeToFit` and no stack mode is a group.

use desigma_doc::*;
use desigma_fig::container::Compression;
use desigma_fig::testkit::groups_fig;
use desigma_fig::{export_rest_json, import_fig};

fn find<'a>(doc: &'a Document, name: &str) -> &'a Node {
    doc.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node named {name}"))
}

#[test]
fn resize_to_fit_frames_import_as_groups() {
    let doc = import_fig(&groups_fig(Compression::Zstd)).unwrap();
    assert!(matches!(find(&doc, "Group 1").kind, NodeKind::Group), "{:?}", find(&doc, "Group 1").kind);
    assert!(matches!(find(&doc, "Hug").kind, NodeKind::Frame(_)), "auto-layout frames that hug stay frames");
    assert!(matches!(find(&doc, "Plain").kind, NodeKind::Frame(_)));
    assert_eq!(find(&doc, "Group 1").children.len(), 1);

    let rest = export_rest_json(&doc).to_string();
    assert!(rest.contains(r#""name":"Group 1""#) && rest.contains(r#""type":"GROUP""#), "{rest}");
}
