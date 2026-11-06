//! Files saved by Pencil (see `tests/fixtures/ATTRIBUTION.md`): import, then check the tree,
//! sizes, fills and text.

mod common;
use common::*;
use desigma_doc::*;

const FIXTURES: [&str; 6] = ["components-and-themes.pen", "shapes.pen", "wigify-logo.pen", "terminal-icon.pen", "nojs-landing.pen", "pencil-scripts.pen"];

fn node<'a>(d: &'a Document, id: &str) -> &'a Node {
    d.get(by_id(d, id)).unwrap()
}

fn named<'a>(d: &'a Document, name: &str) -> &'a Node {
    d.nodes.values().find(|n| n.name == name).unwrap_or_else(|| panic!("no node named {name}"))
}

fn count(d: &Document, f: impl Fn(&Node) -> bool) -> usize {
    d.nodes.values().filter(|n| f(n)).count()
}

#[test]
fn wigify_logo_tree() {
    let i = load("wigify-logo.pen");
    let d = &i.document;
    assert_eq!(top(d).len(), 1);
    let logo = named(d, "Wigify Logo");
    assert_eq!((logo.size.x, logo.size.y), (1024.0, 1024.0));
    assert_eq!((logo.transform.tx, logo.transform.ty), (1486.0, 137.0));
    assert_eq!(logo.corner_radii, [228.0; 4]);
    assert!(matches!(&logo.kind, NodeKind::Frame(f) if f.clips_content && f.auto_layout.is_none()));
    let parts = kids(d, logo.id);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "Background");
    // the grid is a reusable frame, so it is a component
    let grid = parts[1];
    assert!(matches!(grid.kind, NodeKind::Component(_)));
    assert_eq!((grid.size.x, grid.size.y), (720.0, 600.0));
    let al = grid.kind.frame_props().unwrap().auto_layout.unwrap();
    assert_eq!(
        (al.direction, al.item_spacing, al.counter_align, al.primary_align),
        (LayoutDirection::Vertical, 10.0, LayoutAlign::Center, LayoutAlign::SpaceBetween)
    );
    let rows = kids(d, grid.id);
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(kids(d, row.id).len(), 5);
        // fill_container rows share the height left by the gaps: (600 - 2 * 10) / 3
        assert!(near(row.size.y, 580.0 / 3.0), "{}", row.size.y);
        assert_eq!(row.size.x, 720.0);
    }
    assert!(i.warnings.iter().any(|w| w.contains("space_around")));
}

#[test]
fn wigify_logo_paints() {
    let i = load("wigify-logo.pen");
    let d = &i.document;
    let bg = named(d, "Background");
    let PaintKind::LinearGradient { stops, handles } = &bg.fills[0].kind else { panic!("{:?}", bg.fills) };
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].color.to_hex(), "B11717");
    assert_eq!(stops[2].color.to_hex(), "F97316");
    assert_eq!(stops[1].pos, 0.5);
    // 150 degrees counter-clockwise from up: toward the bottom left
    assert!(handles[1].x < handles[0].x && handles[1].y > handles[0].y, "{handles:?}");
    // the white cells: opaque, translucent, or fully transparent
    let cell = named(d, "r1c1");
    assert_eq!(solid(&cell.fills[0]).to_hex(), "FFFFFF");
    assert_eq!(cell.corner_radii, [24.0; 4]);
    assert!(matches!(&cell.effects[0], Effect::DropShadow { radius, offset, .. } if *radius == 16.0 && offset.y == 4.0));
    let alpha = solid(&named(d, "r2c1").fills[0]).a;
    assert!(near(alpha, 0xEE as f32 / 255.0), "{alpha}");
    assert_eq!(solid(&named(d, "r1c2").fills[0]).a, 0.0);
}

#[test]
fn terminal_icon_vector() {
    let i = load("terminal-icon.pen");
    let d = &i.document;
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    let frame = node(d, "bi8Au");
    assert_eq!((frame.transform.tx, frame.transform.ty, frame.size.x), (-150.0, -136.0, 1024.0));
    assert_eq!(solid(&frame.fills[0]).to_hex(), "FFFFFF");
    let v = node(d, "BGQPu");
    assert_eq!((v.size.x, v.size.y), (512.0, 448.0));
    assert_eq!((v.transform.tx, v.transform.ty), (256.0, 288.0));
    // the stroke is the older object spelling
    let s = &v.stroke;
    assert_eq!((s.weight, s.align, s.cap, s.join), (48.0, StrokeAlign::Center, StrokeCap::Round, StrokeJoin::Round));
    assert_eq!(solid(&s.paints[0]).to_hex(), "9B9BBD");
    assert!(v.fills.is_empty());
    let NodeKind::Vector { paths, .. } = &v.kind else { panic!() };
    // the geometry's 16 x 14 view box was scaled up to the node's 512 x 448
    let max_x = paths[0].cmds.iter().filter_map(|c| if let PathCmd::LineTo(p) | PathCmd::MoveTo(p) = c { Some(p.x) } else { None }).fold(0.0, f32::max);
    assert!(near(max_x, 512.0), "{max_x}");
}

#[test]
fn nojs_landing_page() {
    let i = load("nojs-landing.pen");
    let d = &i.document;
    let page = named(d, "NoJS Landing Page v2.0 Proposal");
    assert_eq!((page.size.x, page.size.y), (1440.0, 900.0));
    // its fill is a shader, which cannot be drawn
    assert!(page.fills.is_empty());
    assert!(i.warnings.iter().any(|w| w.contains("shader")));
    let glow = named(d, "Background Glow");
    assert_eq!(glow.fills.len(), 2);
    assert!(glow.fills.iter().all(|p| matches!(p.kind, PaintKind::RadialGradient { .. })));
    // hero text
    let headline = named(d, "Headline L1");
    let NodeKind::Text(t) = &headline.kind else { panic!() };
    assert_eq!(t.content, "Build the Web.");
    assert_eq!(t.style.font_weight, 800);
    assert_eq!(t.style.font_family, "Geist");
    // wrapped text keeps its width and measures its height from it
    let sub = named(d, "Subtitle Text");
    let NodeKind::Text(t) = &sub.kind else { panic!() };
    assert_eq!(t.auto_resize, TextAutoResize::Height);
    assert_eq!(sub.size.x, 448.0);
    assert!(sub.size.y > 50.0 && sub.size.y < 70.0, "{}", sub.size.y);
    // object counts of the file: 28 frames, 2 rectangles, 3 ellipses, 14 paths, 66 texts and 2 icons
    assert_eq!(count(d, |n| matches!(n.kind, NodeKind::Frame(_))), 28);
    assert_eq!(count(d, |n| matches!(n.kind, NodeKind::Text(_))), 66);
    assert_eq!(count(d, |n| matches!(n.kind, NodeKind::Vector { .. })), 14);
    assert_eq!(count(d, |n| matches!(n.kind, NodeKind::Ellipse(_))), 3);
    // the two icons are dashed placeholders next to the 2 rectangles
    assert_eq!(count(d, |n| matches!(n.kind, NodeKind::Rectangle)), 2 + 2);
    assert!(i.warnings.iter().any(|w| w.contains("icons") && w.ends_with("(x2)")), "{:?}", i.warnings);
}

#[test]
fn nojs_variables_and_bindings() {
    let i = load("nojs-landing.pen");
    let d = &i.document;
    let source: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture_dir().join("nojs-landing.pen")).unwrap()).unwrap();
    assert_eq!(d.variables.len(), source["variables"].as_object().unwrap().len());
    // every variable lives in the single default collection, one mode
    assert_eq!(d.variable_collections.len(), 1);
    assert_eq!(d.variable_collections["pen:default"].modes.len(), 1);
    let blue = &d.variables["pen:accent-blue"];
    assert_eq!(blue.kind, VariableKind::Color);
    assert_eq!(blue.values["default"], VariableValue::Color(Color::rgb(0x25, 0x63, 0xEB)));
    // the diamonds' outlines use a color variable
    let diamond = named(d, "D1 body store");
    assert_eq!(diamond.variables.bindings["strokes/0"], "pen:diamond-border");
    assert_eq!(diamond.stroke.weight, 1.0);
    assert_eq!(diamond.opacity, 0.8);
    // a variable the bound property resolves through
    assert!(matches!(d.bound_value(diamond.id, "strokes/0"), Some(VariableValue::Color(_))));
}

#[test]
fn script_objects_become_named_clipping_frames() {
    let i = load("pencil-scripts.pen");
    let d = &i.document;
    let sheet = named(d, "Selection");
    assert_eq!((sheet.transform.tx, sheet.transform.ty), (-7548.0, -2561.0));
    assert_eq!(solid(&sheet.fills[0]).to_hex(), "FFFFFF");
    let chart = named(d, "chart");
    assert!(matches!(&chart.kind, NodeKind::Frame(f) if f.clips_content));
    assert_eq!((chart.size.x, chart.size.y), (660.0, 336.0));
    assert_eq!(kids(d, sheet.id).len(), 11);
    assert_eq!(i.warnings.len(), 1);
    assert!(i.warnings[0].contains("script") && i.warnings[0].ends_with("(x11)"), "{:?}", i.warnings);
    // the file's one variable is kept even though only a script input uses it
    assert_eq!(d.variables["pen:time"].values["default"], VariableValue::Number(0.0));
}

#[test]
fn every_fixture_imports_without_unexpected_warnings() {
    for f in FIXTURES {
        let i = load(f);
        assert_eq!(i.document.pages.len(), 1, "{f}");
        assert_eq!(i.document.name, f);
        // all parent/child links agree
        for n in i.document.nodes.values() {
            for c in &n.children {
                assert_eq!(i.document.get(*c).unwrap().parent, Some(n.id), "{f}");
            }
            if let Some(p) = n.parent {
                assert!(i.document.get(p).unwrap().children.contains(&n.id), "{f}");
            }
        }
        for n in i.document.nodes.values() {
            assert!(n.size.x.is_finite() && n.size.y.is_finite() && n.size.x >= 0.0 && n.size.y >= 0.0, "{f}: {}", n.name);
        }
    }
}

#[test]
fn imported_documents_survive_a_desigma_round_trip() {
    for f in FIXTURES {
        let i = load(f);
        let json = i.document.to_json().unwrap();
        let back = Document::from_json(&json).unwrap();
        assert_eq!(back.nodes.len(), i.document.nodes.len(), "{f}");
        assert_eq!(back.variables.len(), i.document.variables.len(), "{f}");
    }
}
