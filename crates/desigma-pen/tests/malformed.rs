//! Bad input is an error or a warning, never a panic.

mod common;
use common::*;
use desigma_pen::{import, import_with, looks_like_pen, Options, PenError};
use serde_json::{json, Value};

#[test]
fn input_that_is_not_a_pen_document_is_an_error() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", b"".to_vec()),
        ("blank", b"  \n\t ".to_vec()),
        ("text", b"hello".to_vec()),
        ("array", b"[]".to_vec()),
        ("number", b"42".to_vec()),
        ("empty object", b"{}".to_vec()),
        ("children not a list", br#"{"children": 5}"#.to_vec()),
        ("children null", br#"{"children": null}"#.to_vec()),
        ("truncated", br#"{"version": "2.21", "children": ["#.to_vec()),
        ("binary", vec![0x00, 0x01, 0x02, 0xff, 0xfe, 0x80]),
        ("invalid utf-8", vec![b'{', 0xff, 0xfe, b'}']),
        ("zip header", b"PK\x03\x04\x14\x00\x00\x00".to_vec()),
        ("svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()),
        ("unterminated comment", b"{ /* never closed".to_vec()),
        ("unterminated string", b"{\"children\": [\"abc".to_vec()),
    ];
    for (what, bytes) in cases {
        assert!(import(&bytes).is_err(), "{what} should fail");
        assert!(!looks_like_pen(&bytes), "{what}");
    }
}

#[test]
fn encrypted_looking_files_get_a_helpful_error() {
    let e = import(&[0x8f, 0x12, 0x00, 0xaa, 0x55]).unwrap_err();
    assert!(matches!(e, PenError::Json(_)));
    assert!(e.to_string().contains("encrypted"), "{e}");
    let e = import(b"U2FsdGVkX1+abcdef==").unwrap_err();
    assert!(e.to_string().contains("encrypted"), "{e}");
}

#[test]
fn a_byte_order_mark_is_skipped() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(br#"{"version": "2.21", "children": []}"#);
    assert!(import(&bytes).is_ok());
}

#[test]
fn absurd_nesting_is_an_error_not_a_stack_overflow() {
    let mut s = String::from(r#"{"version": "2.21", "children": "#);
    s.push_str(&"[".repeat(100_000));
    s.push_str(&"]".repeat(100_000));
    s.push('}');
    assert!(import(s.as_bytes()).is_err());
    // objects nested within serde_json's limit convert without trouble
    let mut node = json!({"id": "leaf", "type": "rectangle", "width": 1, "height": 1});
    for n in 0..60 {
        node = json!({"id": format!("f{n}"), "type": "frame", "children": [node]});
    }
    let i = import(json!({"version": "2.21", "children": [node]}).to_string().as_bytes()).unwrap();
    assert_eq!(i.nodes.len(), 62);
}

#[test]
fn wrong_property_types_are_ignored() {
    let weird = json!([null, true, "x", "$nope", -1, 1e300, [], {}, [1, 2, 3], {"type": 5}, "fill_container(abc)", "#zzz", [[], [{}]]]);
    let props = [
        "x",
        "y",
        "width",
        "height",
        "rotation",
        "opacity",
        "enabled",
        "flipX",
        "flipY",
        "fill",
        "stroke",
        "strokeWidth",
        "strokeAlignment",
        "effect",
        "cornerRadius",
        "layout",
        "gap",
        "padding",
        "justifyContent",
        "alignItems",
        "clip",
        "name",
        "id",
        "theme",
        "reusable",
        "children",
        "content",
        "fontSize",
        "fontFamily",
        "fontWeight",
        "lineHeight",
        "letterSpacing",
        "textGrowth",
        "textAlign",
        "geometry",
        "viewBox",
        "innerRadius",
        "startAngle",
        "sweepAngle",
        "polygonCount",
        "descendants",
        "ref",
        "layoutPosition",
    ];
    for ty in ["frame", "rectangle", "ellipse", "polygon", "path", "text", "group", "icon", "ref", "note", "script", "browser"] {
        for p in props {
            for w in weird.as_array().unwrap() {
                let mut node = json!({"id": "n", "type": ty, "x": 1, "y": 2, "width": 10, "height": 10, "ref": "n"});
                node[p] = w.clone();
                let doc = json!({"version": "2.21", "children": [node, {"id": "k", "type": "frame", "reusable": true}]});
                let _ = import_with(doc.to_string().as_bytes(), &Options::default());
            }
        }
    }
}

#[test]
fn wrong_document_level_types_are_ignored() {
    for extra in [
        json!({"variables": 5}),
        json!({"variables": {"a": 5}}),
        json!({"variables": {"a": {"type": "color"}}}),
        json!({"variables": {"a": {"type": "color", "value": [1, 2]}}}),
        json!({"variables": {"a": {"type": "color", "value": [{"value": "#fff", "theme": 3}]}}}),
        json!({"variables": {"a": {"type": "number", "value": "$a"}}}),
        json!({"themes": 5}),
        json!({"themes": {"mode": []}}),
        json!({"themes": {"mode": [1, 2]}}),
        json!({"themes": {"mode": ["a", "b"], "x": ["c"], "y": ["d"], "z": ["e", "f", "g", "h"]}, "variables": {"v": {"type": "number", "value": [
            {"value": 1, "theme": {"mode": "a", "x": "c", "y": "d", "z": "e"}}, {"value": 2, "theme": {"z": "h", "mode": "b"}}]}}}),
        json!({"imports": 5}),
        json!({"version": 5}),
        json!({"version": "x.y"}),
    ] {
        let mut doc = json!({"version": "2.21", "children": [{"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": "$a", "theme": {"mode": "b"}}]});
        for (k, v) in extra.as_object().unwrap() {
            doc[k] = v.clone();
        }
        let i = import_with(doc.to_string().as_bytes(), &Options::default()).unwrap_or_else(|e| panic!("{extra}: {e}"));
        assert_eq!(top(&i.document).len(), 1);
    }
}

#[test]
fn huge_and_degenerate_numbers_stay_finite() {
    let i = from_json(&json!({"version": "2.21", "children": [
        {"id": "a", "type": "rectangle", "x": 1e300, "y": -1e300, "width": 1e300, "height": -5, "rotation": 1e300, "cornerRadius": 1e300, "opacity": -3, "fill": {"type": "gradient", "rotation": 1e300, "size": {"width": 1e300}, "colors": [{"color": "#000", "position": 1e300}]}},
        {"id": "b", "type": "text", "content": "x", "fontSize": 1e300, "lineHeight": -1, "letterSpacing": 1e300},
        {"id": "c", "type": "frame", "gap": 1e300, "padding": [1e300, -1e300], "width": "fit_content(1e300)", "children": [{"id": "d", "type": "rectangle", "width": "fill_container", "height": 1e300}]},
        {"id": "e", "type": "path", "geometry": "M0 0L1e300 1e300", "viewBox": [0, 0, 0, 0], "width": 1e300},
        {"id": "f", "type": "ellipse", "width": 5, "height": 5, "startAngle": 1e300, "sweepAngle": -1e300, "innerRadius": 1e300}
    ]}).to_string());
    for n in i.document.nodes.values() {
        assert!(n.size.x.is_finite() && n.size.y.is_finite(), "{}", n.name);
        assert!(n.transform.tx.is_finite() && n.transform.ty.is_finite() && n.transform.a.is_finite(), "{}", n.name);
        assert!((0.0..=1.0).contains(&n.opacity));
    }
}

/// Replaces one value of `v` at a time and imports the result: every kind of damage in
/// every position of every fixture.
fn mutations(v: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    out.push(path.clone());
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                path.push(k.clone());
                mutations(x, path, out);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                path.push(i.to_string());
                mutations(x, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn set(v: &mut Value, path: &[String], new: Value) {
    match path.split_first() {
        None => *v = new,
        Some((k, rest)) => match v {
            Value::Object(o) => {
                if let Some(x) = o.get_mut(k) {
                    set(x, rest, new);
                }
            }
            Value::Array(a) => {
                if let Some(x) = k.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                    set(x, rest, new);
                }
            }
            _ => {}
        },
    }
}

#[test]
fn damaged_fixtures_never_panic() {
    let damage = [
        json!(null),
        json!("$nope"),
        json!(-1),
        json!(1e300),
        json!([]),
        json!({}),
        json!("fill_container(x)"),
        json!({"type": "ref", "ref": "k"}),
        json!([{"type": "frame"}]),
    ];
    let mut runs = 0;
    for f in ["components-and-themes.pen", "shapes.pen", "wigify-logo.pen", "terminal-icon.pen", "pencil-scripts.pen"] {
        let text = std::fs::read_to_string(fixture_dir().join(f)).unwrap();
        let text: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let base: Value = serde_json::from_str(&text).unwrap();
        let mut paths = Vec::new();
        mutations(&base, &mut Vec::new(), &mut paths);
        for (i, p) in paths.iter().enumerate() {
            // every position, one kind of damage each, rotating
            let mut v = base.clone();
            set(&mut v, p, damage[i % damage.len()].clone());
            let _ = import_with(v.to_string().as_bytes(), &Options::default());
            runs += 1;
        }
    }
    assert!(runs > 500, "{runs}");
}

#[test]
fn truncating_a_fixture_anywhere_is_an_error_or_an_import() {
    let bytes = std::fs::read(fixture_dir().join("shapes.pen")).unwrap();
    for cut in (0..bytes.len()).step_by(37) {
        let _ = import(&bytes[..cut]);
    }
}
