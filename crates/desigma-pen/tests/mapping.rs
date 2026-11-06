//! One test per mapping: a small `.pen` document in, the node properties out.

mod common;
use common::*;
use desigma_doc::*;
use serde_json::json;

/// Imports a document whose children are `children`.
fn doc_of(children: serde_json::Value) -> Document {
    from_json(&json!({ "version": "2.21", "children": children }).to_string()).document
}

fn node<'a>(d: &'a Document, id: &str) -> &'a Node {
    d.get(by_id(d, id)).unwrap()
}

fn auto(n: &Node) -> AutoLayout {
    n.kind.frame_props().and_then(|f| f.auto_layout).unwrap_or_else(|| panic!("{} has no auto layout", n.name))
}

fn text_of(n: &Node) -> &TextProps {
    match &n.kind {
        NodeKind::Text(t) => t,
        other => panic!("not text: {other:?}"),
    }
}

// ---------------------------------------------------------------------------- document

#[test]
fn the_document_has_one_page_with_the_top_level_objects() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "x": -20, "y": 30, "width": 5, "height": 6}, {"id": "b", "type": "ellipse", "x": 100, "y": 0, "width": 5, "height": 6}]),
    );
    assert_eq!(d.pages.len(), 1);
    assert_eq!(top(&d).len(), 2);
    let a = node(&d, "a");
    assert_eq!((a.transform.tx, a.transform.ty), (-20.0, 30.0));
    assert_eq!(a.parent, Some(d.pages[0]));
}

#[test]
fn comments_and_trailing_commas_are_accepted() {
    let i =
        from_json("// header\n{ \"version\": \"2.21\", /* x */ \"children\": [ {\"id\": \"a\", \"type\": \"rectangle\", \"width\": 4, \"height\": 4,}, ], }");
    assert_eq!(top(&i.document).len(), 1);
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn a_missing_or_newer_version_is_a_warning_not_an_error() {
    let i = from_json(r#"{"children": []}"#);
    assert!(i.warnings.iter().any(|w| w.contains("no \"version\"")));
    let i = from_json(r#"{"version": "3.0", "children": []}"#);
    assert!(i.warnings.iter().any(|w| w.contains("newer")));
    let i = from_json(r#"{"version": "2.8", "children": []}"#);
    assert!(i.warnings.is_empty());
}

#[test]
fn imports_are_reported() {
    let i = from_json(r#"{"version": "2.21", "imports": {"lib": "./ui.lib.pen"}, "children": []}"#);
    assert!(i.warnings.iter().any(|w| w.contains("imports")));
}

#[test]
fn repeated_warnings_are_folded() {
    let d = from_json(&json!({"version": "2.21", "children": [{"id": "a", "type": "script", "width": 1, "height": 1}, {"id": "b", "type": "script", "width": 1, "height": 1}]}).to_string());
    assert_eq!(d.warnings.len(), 1);
    assert!(d.warnings[0].ends_with("(x2)"), "{:?}", d.warnings);
}

// ---------------------------------------------------------------------------- frames and layout

#[test]
fn a_frame_without_layout_lays_out_horizontally() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "width": 200, "height": 100, "gap": 8, "padding": 10, "children": [
        {"id": "a", "type": "rectangle", "width": 20, "height": 20, "x": 99, "y": 99},
        {"id": "b", "type": "rectangle", "width": 30, "height": 20}
    ]}]));
    let f = node(&d, "f");
    let al = auto(f);
    assert_eq!(al.direction, LayoutDirection::Horizontal);
    assert_eq!((al.item_spacing, al.padding), (8.0, [10.0; 4]));
    // x/y are ignored inside a layout
    assert_eq!((node(&d, "a").transform.tx, node(&d, "a").transform.ty), (10.0, 10.0));
    assert_eq!(node(&d, "b").transform.tx, 38.0);
}

#[test]
fn layout_none_keeps_children_where_they_are() {
    let d = doc_of(
        json!([{"id": "f", "type": "frame", "layout": "none", "width": 100, "height": 100, "children": [{"id": "a", "type": "rectangle", "x": 7, "y": 9, "width": 4, "height": 4}]}]),
    );
    assert!(node(&d, "f").kind.frame_props().unwrap().auto_layout.is_none());
    assert_eq!((node(&d, "a").transform.tx, node(&d, "a").transform.ty), (7.0, 9.0));
}

#[test]
fn padding_forms() {
    let d = doc_of(json!([
        {"id": "p2", "type": "frame", "padding": [4, 8]},
        {"id": "p4", "type": "frame", "padding": [1, 2, 3, 4]},
        {"id": "p1", "type": "frame", "padding": 5}
    ]));
    assert_eq!(auto(node(&d, "p2")).padding, [4.0, 8.0, 4.0, 8.0]);
    assert_eq!(auto(node(&d, "p4")).padding, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(auto(node(&d, "p1")).padding, [5.0; 4]);
}

#[test]
fn alignment_values() {
    let d = doc_of(json!([
        {"id": "a", "type": "frame", "layout": "vertical", "justifyContent": "center", "alignItems": "end"},
        {"id": "b", "type": "frame", "justifyContent": "space_between", "alignItems": "center"},
        {"id": "c", "type": "frame", "justifyContent": "end"},
        {"id": "d", "type": "frame", "justifyContent": "space_around"}
    ]));
    let a = auto(node(&d, "a"));
    assert_eq!((a.direction, a.primary_align, a.counter_align), (LayoutDirection::Vertical, LayoutAlign::Center, LayoutAlign::End));
    let b = auto(node(&d, "b"));
    assert_eq!((b.primary_align, b.counter_align), (LayoutAlign::SpaceBetween, LayoutAlign::Center));
    assert_eq!(auto(node(&d, "c")).primary_align, LayoutAlign::End);
    assert_eq!(auto(node(&d, "d")).primary_align, LayoutAlign::SpaceBetween);
}

#[test]
fn space_around_is_reported() {
    let i = from_json(&json!({"version": "2.21", "children": [{"id": "d", "type": "frame", "justifyContent": "space_around"}]}).to_string());
    assert!(i.warnings.iter().any(|w| w.contains("space_around")));
}

#[test]
fn fit_content_hugs_and_fill_container_fills() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "layout": "vertical", "padding": 10, "gap": 4, "width": 300, "children": [
        {"id": "a", "type": "rectangle", "width": "fill_container", "height": 20},
        {"id": "b", "type": "rectangle", "width": 50, "height": 30}
    ]}]));
    let f = node(&d, "f");
    let al = auto(f);
    assert_eq!((al.primary_sizing, al.counter_sizing), (Sizing::Hug, Sizing::Fixed));
    assert_eq!((f.size.x, f.size.y), (300.0, 10.0 + 20.0 + 4.0 + 30.0 + 10.0));
    let a = node(&d, "a");
    assert_eq!(a.layout_child.horizontal, Sizing::Fill);
    assert_eq!(a.size.x, 280.0);
}

#[test]
fn fill_container_outside_a_layout_takes_the_parent_size() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "layout": "none", "width": 120, "height": 80, "children": [
        {"id": "a", "type": "rectangle", "width": "fill_container", "height": "fill_container"},
        {"id": "b", "type": "rectangle", "width": "fill_container(33)", "height": 5}
    ]}]));
    assert_eq!((node(&d, "a").size.x, node(&d, "a").size.y), (120.0, 80.0));
    assert_eq!(node(&d, "a").layout_child.horizontal, Sizing::Fixed);
    assert_eq!(node(&d, "b").size.x, 120.0);
}

#[test]
fn fit_content_fallback_applies_to_an_empty_frame_without_layout() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "layout": "none", "width": "fit_content(64)", "height": "fit_content(32)"}]));
    assert_eq!((node(&d, "f").size.x, node(&d, "f").size.y), (64.0, 32.0));
}

#[test]
fn absolutely_positioned_children_leave_the_flow() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "width": 100, "height": 100, "children": [
        {"id": "a", "type": "rectangle", "width": 10, "height": 10},
        {"id": "pin", "type": "rectangle", "layoutPosition": "absolute", "x": 70, "y": 5, "width": 10, "height": 10}
    ]}]));
    let pin = node(&d, "pin");
    assert!(pin.layout_child.absolute);
    assert_eq!((pin.transform.tx, pin.transform.ty), (70.0, 5.0));
    assert_eq!(node(&d, "a").transform.tx, 0.0);
}

#[test]
fn clip_and_stroke_inclusion() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "clip": true, "layoutIncludeStroke": true}, {"id": "g", "type": "frame"}]));
    assert!(node(&d, "f").kind.frame_props().unwrap().clips_content);
    assert!(auto(node(&d, "f")).stroke_included);
    assert!(!node(&d, "g").kind.frame_props().unwrap().clips_content);
}

#[test]
fn groups_fit_their_children() {
    let d = doc_of(json!([{"id": "g", "type": "group", "x": 50, "y": 60, "children": [
        {"id": "a", "type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
        {"id": "b", "type": "ellipse", "x": 60, "y": 50, "width": 80, "height": 80}
    ]}]));
    let g = node(&d, "g");
    assert!(matches!(g.kind, NodeKind::Group));
    assert_eq!((g.transform.tx, g.transform.ty), (60.0, 80.0));
    assert_eq!((g.size.x, g.size.y), (130.0, 110.0));
    assert_eq!((node(&d, "a").transform.tx, node(&d, "a").transform.ty), (0.0, 0.0));
}

// ---------------------------------------------------------------------------- common properties

#[test]
fn names_opacity_visibility_and_source_id() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "name": "Box", "opacity": 0.4, "enabled": false, "width": 1, "height": 1}, {"id": "b", "type": "rectangle", "opacity": 7, "width": 1, "height": 1}]),
    );
    let a = node(&d, "a");
    assert_eq!((a.name.as_str(), a.opacity, a.visible), ("Box", 0.4, false));
    assert_eq!(node(&d, "b").opacity, 1.0);
}

#[test]
fn rotation_is_counter_clockwise_around_the_top_left() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50, "rotation": 90}]));
    let t = node(&d, "a").transform;
    assert_eq!((t.tx, t.ty), (10.0, 20.0));
    // The box's right edge point (100, 0) swings up to (0, -100): counter-clockwise on screen.
    let p = t.apply(Vec2::new(100.0, 0.0));
    assert!(near(p.x, 10.0) && near(p.y, -80.0), "{p:?}");
}

#[test]
fn flips_mirror_inside_the_box() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50, "flipX": true, "flipY": true}]));
    let t = node(&d, "a").transform;
    let p = t.apply(Vec2::new(0.0, 0.0));
    assert!(near(p.x, 110.0) && near(p.y, 70.0), "{p:?}");
}

// ---------------------------------------------------------------------------- shapes

#[test]
fn rectangle_corner_radii() {
    let d = doc_of(json!([
        {"id": "a", "type": "rectangle", "width": 10, "height": 10, "cornerRadius": 6},
        {"id": "b", "type": "rectangle", "width": 10, "height": 10, "cornerRadius": [1, 2, 3, 4]}
    ]));
    assert_eq!(node(&d, "a").corner_radii, [6.0; 4]);
    assert_eq!(node(&d, "b").corner_radii, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn ellipse_arcs() {
    let d = doc_of(json!([
        {"id": "full", "type": "ellipse", "width": 10, "height": 10},
        {"id": "ring", "type": "ellipse", "width": 10, "height": 10, "innerRadius": 0.5},
        {"id": "q", "type": "ellipse", "width": 10, "height": 10, "startAngle": 0, "sweepAngle": 90}
    ]));
    let arc = |id: &str| match node(&d, id).kind {
        NodeKind::Ellipse(a) => a,
        _ => panic!(),
    };
    assert_eq!(arc("full"), ArcData::default());
    assert_eq!(arc("ring").inner_radius, 0.5);
    // counter-clockwise from the right by 90 degrees is the upper-right quadrant: -90..0 in desigma's clockwise angles
    let q = arc("q");
    assert!(near(q.start, -std::f32::consts::FRAC_PI_2) && near(q.end, 0.0), "{q:?}");
}

#[test]
fn polygons_keep_their_side_count() {
    let d = doc_of(
        json!([{"id": "p", "type": "polygon", "width": 10, "height": 10, "polygonCount": 6}, {"id": "q", "type": "polygon", "width": 10, "height": 10}]),
    );
    assert!(matches!(node(&d, "p").kind, NodeKind::Polygon { count: 6 }));
    assert!(matches!(node(&d, "q").kind, NodeKind::Polygon { count: 3 }));
}

fn path_extent(n: &Node) -> (f32, f32, f32, f32) {
    let NodeKind::Vector { paths, .. } = &n.kind else { panic!("not a vector") };
    let mut pts = Vec::new();
    for c in &paths[0].cmds {
        match *c {
            PathCmd::MoveTo(a) | PathCmd::LineTo(a) => pts.push(a),
            PathCmd::QuadTo(a, b) => pts.extend([a, b]),
            PathCmd::CubicTo(a, b, c) => pts.extend([a, b, c]),
            PathCmd::Close => {}
        }
    }
    let x0 = pts.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let y0 = pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let x1 = pts.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let y1 = pts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    (x0, y0, x1, y1)
}

#[test]
fn paths_are_scaled_from_their_view_box_onto_the_node() {
    let d = doc_of(json!([{"id": "p", "type": "path", "geometry": "M0 0L24 0L24 24Z", "viewBox": [0, 0, 24, 24], "width": 48, "height": 96, "fill": "#000"}]));
    let n = node(&d, "p");
    assert_eq!((n.size.x, n.size.y), (48.0, 96.0));
    assert_eq!(path_extent(n), (0.0, 0.0, 48.0, 96.0));
}

#[test]
fn paths_without_a_view_box_use_their_tight_bounds() {
    let d = doc_of(
        json!([{"id": "p", "type": "path", "geometry": "M10 20L30 20L30 60Z", "fill": "#000"}, {"id": "q", "type": "path", "geometry": "M10 20L30 20L30 60Z", "width": 40, "height": 80}]),
    );
    let p = node(&d, "p");
    assert_eq!((p.size.x, p.size.y), (20.0, 40.0));
    assert_eq!(path_extent(p), (0.0, 0.0, 20.0, 40.0));
    assert_eq!(path_extent(node(&d, "q")), (0.0, 0.0, 40.0, 80.0));
}

#[test]
fn path_fill_rule() {
    let d = doc_of(json!([{"id": "p", "type": "path", "geometry": "M0 0L4 0L4 4Z", "fillRule": "evenodd"}]));
    let NodeKind::Vector { paths, .. } = &node(&d, "p").kind else { panic!() };
    assert_eq!(paths[0].fill_rule, FillRule::EvenOdd);
}

#[test]
fn unreadable_geometry_is_reported_and_harmless() {
    let i = from_json(&json!({"version": "2.21", "children": [{"id": "p", "type": "path", "geometry": "hello world", "width": 4, "height": 4}]}).to_string());
    assert!(i.warnings.iter().any(|w| w.contains("geometry")));
    assert_eq!(top(&i.document).len(), 1);
}

// ---------------------------------------------------------------------------- fills

#[test]
fn solid_fills_with_alpha() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": "#FF000080"}, {"id": "b", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "color", "color": "#0F0"}}]),
    );
    let a = solid(&node(&d, "a").fills[0]);
    assert!(near(a.r, 1.0) && near(a.a, 128.0 / 255.0));
    assert_eq!(solid(&node(&d, "b").fills[0]).to_hex(), "00FF00");
}

#[test]
fn several_fills_keep_their_order_and_flags() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": [
        "#111111",
        {"type": "color", "color": "#222222", "enabled": false},
        {"type": "color", "color": "#333333", "blendMode": "multiply"}
    ]}]));
    let f = &node(&d, "a").fills;
    assert_eq!(f.len(), 3);
    assert!(f[0].visible && !f[1].visible);
    assert_eq!(f[2].blend, BlendMode::Multiply);
}

#[test]
fn linear_gradients_run_in_the_rotation_direction() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "fill": {"type": "gradient", "gradientType": "linear", "rotation": 180,
        "colors": [{"color": "#FF0000", "position": 0}, {"color": "#0000FF", "position": 1}]}}]));
    let PaintKind::LinearGradient { handles, stops } = &node(&d, "a").fills[0].kind else { panic!() };
    assert!(near(handles[0].x, 0.5) && near(handles[0].y, 0.0), "{handles:?}");
    assert!(near(handles[1].x, 0.5) && near(handles[1].y, 1.0), "{handles:?}");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[1].color.to_hex(), "0000FF");
}

#[test]
fn zero_degrees_points_up_and_ninety_points_left() {
    let grad = |deg: i32| json!({"id": format!("g{deg}"), "type": "rectangle", "width": 10, "height": 10, "fill": {"type": "gradient", "rotation": deg, "colors": [{"color": "#000", "position": 0}, {"color": "#fff", "position": 1}]}});
    let d = doc_of(json!([grad(0), grad(90)]));
    let end = |id: &str| match &node(&d, id).fills[0].kind {
        PaintKind::LinearGradient { handles, .. } => handles[1],
        _ => panic!(),
    };
    assert!(near(end("g0").x, 0.5) && near(end("g0").y, 0.0));
    assert!(near(end("g90").x, 0.0) && near(end("g90").y, 0.5));
}

#[test]
fn gradient_center_size_and_opacity() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "fill": {"type": "gradient", "gradientType": "radial", "opacity": 0.5,
        "center": {"x": 0.25, "y": 0.75}, "size": {"width": 0.5, "height": 1}, "colors": [{"color": "#000", "position": 0}, {"color": "#fff", "position": 1}]}}]));
    let p = &node(&d, "a").fills[0];
    assert_eq!(p.opacity, 0.5);
    let PaintKind::RadialGradient { handles, .. } = &p.kind else { panic!() };
    assert!(near(handles[0].x, 0.25) && near(handles[0].y, 0.75));
    assert!(near(handles[1].x, 0.5) && near(handles[1].y, 0.75));
    assert!(near(handles[2].x, 0.25) && near(handles[2].y, 1.25));
}

#[test]
fn angular_gradients_map_to_angular_paints() {
    let d = doc_of(
        json!([{"id": "a", "type": "ellipse", "width": 10, "height": 10, "fill": {"type": "gradient", "gradientType": "angular", "colors": [{"color": "#f00", "position": 0}, {"color": "#00f", "position": 1}]}}]),
    );
    assert!(matches!(node(&d, "a").fills[0].kind, PaintKind::AngularGradient { .. }));
}

#[test]
fn gradient_stop_positions_are_clamped_and_empty_gradients_dropped() {
    let d = doc_of(json!([
        {"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "gradient", "colors": [{"color": "#000", "position": -1}, {"color": "#fff", "position": 4}]}},
        {"id": "b", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "gradient", "colors": []}}
    ]));
    let PaintKind::LinearGradient { stops, .. } = &node(&d, "a").fills[0].kind else { panic!() };
    assert_eq!((stops[0].pos, stops[1].pos), (0.0, 1.0));
    assert!(node(&d, "b").fills.is_empty());
}

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

#[test]
fn image_fills_embed_data_uris() {
    let url = format!("data:image/png;base64,{PNG}");
    let i = from_json(
        &json!({"version": "2.21", "children": [
            {"id": "a", "type": "rectangle", "width": 4, "height": 4, "fill": {"type": "image", "url": url, "mode": "contain", "opacity": 0.5}},
            {"id": "b", "type": "rectangle", "width": 4, "height": 4, "fill": {"type": "image", "url": url}}
        ]})
        .to_string(),
    );
    let d = &i.document;
    assert_eq!(d.images.len(), 1);
    let (key, bytes) = d.images.iter().next().unwrap();
    assert_eq!(&bytes[..4], b"\x89PNG");
    let a = &node(d, "a").fills[0];
    assert_eq!(a.opacity, 0.5);
    assert_eq!(a.kind, PaintKind::Image { image_ref: key.clone(), scale_mode: ImageScaleMode::Fit });
    assert!(matches!(&node(d, "b").fills[0].kind, PaintKind::Image { scale_mode: ImageScaleMode::Fill, .. }));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
}

#[test]
fn image_files_load_from_the_pen_folder_only() {
    let dir = std::env::temp_dir().join(format!("desigma_pen_files_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = {
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    std::fs::write(dir.join("pic.png"), &png).unwrap();
    let doc = json!({"version": "2.21", "children": [
        {"id": "a", "type": "rectangle", "width": 4, "height": 4, "fill": {"type": "image", "url": "./pic.png"}},
        {"id": "b", "type": "rectangle", "width": 4, "height": 4, "fill": {"type": "image", "url": "../pic.png"}},
        {"id": "c", "type": "rectangle", "width": 4, "height": 4, "fill": {"type": "image", "url": "./missing.png"}}
    ]})
    .to_string();
    let i = desigma_pen::import_with(doc.as_bytes(), &desigma_pen::Options { base_dir: Some(dir.clone()), ..Default::default() }).unwrap();
    assert!(matches!(node(&i.document, "a").fills[0].kind, PaintKind::Image { .. }));
    // Anything that cannot be loaded is drawn gray, with a warning.
    assert_eq!(solid(&node(&i.document, "b").fills[0]).to_hex(), "D9D9D9");
    assert_eq!(solid(&node(&i.document, "c").fills[0]).to_hex(), "D9D9D9");
    assert_eq!(i.warnings.len(), 2, "{:?}", i.warnings);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unsupported_fills_have_fallbacks_and_warnings() {
    let i = from_json(&json!({"version": "2.21", "children": [
        {"id": "s", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "shader", "url": "./x.glsl"}},
        {"id": "m", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "mesh_gradient", "columns": 2, "rows": 2, "colors": ["#000000", "#000000", "#FFFFFF", "#FFFFFF"]}},
        {"id": "u", "type": "rectangle", "width": 1, "height": 1, "fill": {"type": "plaid"}}
    ]}).to_string());
    let d = &i.document;
    assert!(node(d, "s").fills.is_empty());
    let avg = solid(&node(d, "m").fills[0]);
    assert!(near(avg.r, 0.5) && near(avg.g, 0.5));
    assert!(node(d, "u").fills.is_empty());
    assert_eq!(i.warnings.len(), 3, "{:?}", i.warnings);
}

#[test]
fn bad_colors_are_dropped_with_a_warning() {
    let i = from_json(&json!({"version": "2.21", "children": [{"id": "a", "type": "rectangle", "width": 1, "height": 1, "fill": "red"}, {"id": "b", "type": "rectangle", "width": 1, "height": 1, "fill": 12}]}).to_string());
    assert!(node(&i.document, "a").fills.is_empty() && node(&i.document, "b").fills.is_empty());
    assert_eq!(i.warnings.len(), 2);
}

// ---------------------------------------------------------------------------- strokes

#[test]
fn stroke_in_the_current_spelling() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "stroke": "#112233", "strokeWidth": 3, "strokeAlignment": "outer", "strokeLinecap": "round", "strokeLinejoin": "bevel"}]),
    );
    let s = &node(&d, "a").stroke;
    assert_eq!(s.weight, 3.0);
    assert_eq!(solid(&s.paints[0]).to_hex(), "112233");
    assert_eq!((s.align, s.cap, s.join), (StrokeAlign::Outside, StrokeCap::Round, StrokeJoin::Bevel));
}

#[test]
fn stroke_in_the_older_object_spelling() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "stroke": {"align": "center", "thickness": 2, "join": "round", "cap": "square", "fill": "#abcdef"}}]),
    );
    let s = &node(&d, "a").stroke;
    assert_eq!((s.weight, s.align, s.cap, s.join), (2.0, StrokeAlign::Center, StrokeCap::Square, StrokeJoin::Round));
    assert_eq!(solid(&s.paints[0]).to_hex(), "ABCDEF");
}

#[test]
fn per_side_stroke_widths() {
    let d = doc_of(json!([{"id": "a", "type": "frame", "width": 10, "height": 10, "stroke": "#000", "strokeWidth": {"top": 1, "left": 4}}]));
    let s = &node(&d, "a").stroke;
    assert_eq!(s.side_weights, Some([1.0, 0.0, 0.0, 4.0]));
    assert_eq!(s.weight, 4.0);
}

#[test]
fn stroke_defaults() {
    let d = doc_of(
        json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "stroke": "#000"}, {"id": "p", "type": "path", "geometry": "M0 0L1 1", "stroke": "#000"}]),
    );
    let s = &node(&d, "a").stroke;
    assert_eq!((s.weight, s.align, s.cap, s.join), (1.0, StrokeAlign::Inside, StrokeCap::None, StrokeJoin::Miter));
    assert_eq!(node(&d, "p").stroke.align, StrokeAlign::Center);
    assert!(node(&d, "a").stroke.paints.len() == 1);
}

#[test]
fn a_stroke_can_have_several_fills() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "stroke": ["#111111", "#222222"], "strokeWidth": 2}]));
    assert_eq!(node(&d, "a").stroke.paints.len(), 2);
}

// ---------------------------------------------------------------------------- effects

#[test]
fn shadows_and_blurs() {
    let d = doc_of(json!([{"id": "a", "type": "rectangle", "width": 10, "height": 10, "effect": [
        {"type": "shadow", "shadowType": "outer", "offset": {"x": 1, "y": 2}, "blur": 6, "color": "#00000080"},
        {"type": "shadow", "shadowType": "inner", "offset": {"x": 0, "y": 1}, "blur": 3, "color": "#FFFFFF"},
        {"type": "blur", "radius": 4},
        {"type": "background_blur", "radius": 12, "enabled": false}
    ]}]));
    let e = &node(&d, "a").effects;
    assert_eq!(e.len(), 4);
    match &e[0] {
        Effect::DropShadow { color, offset, radius, visible, .. } => {
            assert!(near(color.a, 128.0 / 255.0));
            assert_eq!((offset.x, offset.y, *radius, *visible), (1.0, 2.0, 6.0, true));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&e[1], Effect::InnerShadow { radius, .. } if *radius == 3.0));
    assert_eq!(e[2], Effect::LayerBlur { radius: 4.0, visible: true });
    assert_eq!(e[3], Effect::BackgroundBlur { radius: 12.0, visible: false });
}

#[test]
fn a_single_effect_object_and_shader_effects() {
    let i = from_json(
        &json!({"version": "2.21", "children": [
            {"id": "a", "type": "rectangle", "width": 1, "height": 1, "effect": {"type": "shadow", "blur": 5}},
            {"id": "b", "type": "rectangle", "width": 1, "height": 1, "effect": {"type": "shader", "url": "./x.glsl"}}
        ]})
        .to_string(),
    );
    assert_eq!(node(&i.document, "a").effects.len(), 1);
    assert!(node(&i.document, "b").effects.is_empty());
    assert_eq!(i.warnings.len(), 1);
}

// ---------------------------------------------------------------------------- text

#[test]
fn text_styles() {
    let d = doc_of(
        json!([{"id": "t", "type": "text", "x": 5, "y": 6, "content": "Hello\nWorld", "fontFamily": "Inter", "fontSize": 20, "fontWeight": "700", "fontStyle": "italic",
        "letterSpacing": 1.5, "lineHeight": 1.4, "textAlign": "center", "textAlignVertical": "middle", "underline": true, "fill": "#336699"}]),
    );
    let n = node(&d, "t");
    let t = text_of(n);
    assert_eq!(t.content, "Hello\nWorld");
    let s = &t.style;
    assert_eq!((s.font_family.as_str(), s.font_size, s.font_weight, s.italic, s.letter_spacing), ("Inter", 20.0, 700, true, 1.5));
    assert_eq!(s.line_height, LineHeight::Percent(140.0));
    assert_eq!((s.align_h, s.align_v, s.decoration), (TextAlignH::Center, TextAlignV::Center, TextDecoration::Underline));
    assert_eq!(solid(&s.fills[0]).to_hex(), "336699");
    assert_eq!(t.auto_resize, TextAutoResize::WidthAndHeight);
    assert_eq!(n.name, "Hello");
    assert_eq!((n.transform.tx, n.transform.ty), (5.0, 6.0));
    // two lines of 20 px text with a 1.4 line height
    assert!(n.size.y > 50.0 && n.size.y < 60.0, "{}", n.size.y);
}

#[test]
fn text_growth_modes() {
    let long = "word ".repeat(40);
    let d = doc_of(json!([
        {"id": "auto", "type": "text", "content": long, "fontSize": 14, "width": 50, "height": 50},
        {"id": "fw", "type": "text", "content": long, "fontSize": 14, "textGrowth": "fixed-width", "width": 200},
        {"id": "fwh", "type": "text", "content": long, "fontSize": 14, "textGrowth": "fixed-width-height", "width": 200, "height": 30}
    ]));
    let (auto_, fw, fwh) = (node(&d, "auto"), node(&d, "fw"), node(&d, "fwh"));
    // width and height are ignored until textGrowth says otherwise
    assert!(auto_.size.x > 500.0 && auto_.size.y < 30.0);
    assert_eq!(text_of(fw).auto_resize, TextAutoResize::Height);
    assert_eq!(fw.size.x, 200.0);
    assert!(fw.size.y > 60.0);
    assert_eq!(text_of(fwh).auto_resize, TextAutoResize::None);
    assert_eq!((fwh.size.x, fwh.size.y), (200.0, 30.0));
}

#[test]
fn wrapped_text_fills_its_container_and_grows_it() {
    let d = doc_of(json!([{"id": "f", "type": "frame", "layout": "vertical", "width": 160, "children": [
        {"id": "t", "type": "text", "content": "one two three four five six seven eight nine ten", "fontSize": 16, "textGrowth": "fixed-width", "width": "fill_container"}
    ]}]));
    let (f, t) = (node(&d, "f"), node(&d, "t"));
    assert_eq!(t.size.x, 160.0);
    assert_eq!(t.layout_child.horizontal, Sizing::Fill);
    assert!(t.size.y > 40.0);
    assert_eq!(f.size.y, t.size.y);
}

#[test]
fn font_weight_words_and_strikethrough() {
    let d = doc_of(json!([
        {"id": "a", "type": "text", "content": "x", "fontWeight": "bold", "strikethrough": true},
        {"id": "b", "type": "text", "content": "x", "fontWeight": 300},
        {"id": "c", "type": "text", "content": "x", "fontWeight": "normal"}
    ]));
    assert_eq!(text_of(node(&d, "a")).style.font_weight, 700);
    assert_eq!(text_of(node(&d, "a")).style.decoration, TextDecoration::Strikethrough);
    assert_eq!(text_of(node(&d, "b")).style.font_weight, 300);
    assert_eq!(text_of(node(&d, "c")).style.font_weight, 400);
}

#[test]
fn text_defaults_to_black() {
    let d = doc_of(json!([{"id": "a", "type": "text", "content": "x"}]));
    assert_eq!(solid(&text_of(node(&d, "a")).style.fills[0]), Color::BLACK);
}

#[test]
fn notes_prompts_and_context_become_text_boxes() {
    let i = from_json(
        &json!({"version": "2.21", "children": [
            {"id": "n", "type": "note", "content": "remember", "x": 1, "y": 2, "width": 120, "height": 80},
            {"id": "p", "type": "prompt", "content": "make it blue", "model": "m", "width": 100, "height": 40},
            {"id": "c", "type": "context", "content": "brand rules", "width": 100, "height": 40}
        ]})
        .to_string(),
    );
    let d = &i.document;
    let n = node(d, "n");
    assert_eq!(text_of(n).content, "remember");
    assert_eq!((n.size.x, n.size.y), (120.0, 80.0));
    assert_eq!(n.name, "Note");
    assert_eq!(node(d, "p").name, "Prompt");
    assert_eq!(i.warnings.len(), 1);
}

// ---------------------------------------------------------------------------- placeholders

#[test]
fn icons_scripts_browsers_and_unknown_types_get_placeholders() {
    let i = from_json(
        &json!({"version": "2.21", "children": [
            {"id": "i", "type": "icon", "library": "lucide", "icon": "check", "width": 24, "height": 24, "fill": "#FF0000"},
            {"id": "s", "type": "script", "scriptUri": "./a.js", "width": 50, "height": 60},
            {"id": "b", "type": "browser", "url": "https://example.com", "width": 300, "height": 200},
            {"id": "u", "type": "hologram", "width": 7, "height": 8}
        ]})
        .to_string(),
    );
    let d = &i.document;
    let icon = node(d, "i");
    assert_eq!(icon.name, "lucide/check");
    assert_eq!((icon.size.x, icon.size.y), (24.0, 24.0));
    assert!(!icon.stroke.dashes.is_empty());
    assert_eq!(solid(&icon.stroke.paints[0]).to_hex(), "FF0000");
    let s = node(d, "s");
    assert!(matches!(&s.kind, NodeKind::Frame(f) if f.clips_content));
    assert_eq!((s.size.x, s.size.y), (50.0, 60.0));
    assert_eq!(node(d, "b").size.y, 200.0);
    assert!(matches!(&node(d, "u").kind, NodeKind::Unknown { type_name } if type_name == "hologram"));
    assert_eq!(i.warnings.len(), 4, "{:?}", i.warnings);
}

#[test]
fn objects_without_a_type_are_skipped() {
    let i = from_json(&json!({"version": "2.21", "children": [{"id": "a"}, 5, null, {"id": "b", "type": "rectangle", "width": 1, "height": 1}]}).to_string());
    assert_eq!(top(&i.document).len(), 1);
    assert!(!i.warnings.is_empty());
}
