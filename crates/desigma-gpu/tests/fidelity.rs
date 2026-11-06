//! GPU renderer vs the CPU reference. Tests print a note and return when the
//! machine has no GPU adapter.

mod common;

use common::*;
use desigma_doc::*;
use desigma_render::Viewport;
use std::sync::Arc;

#[test]
fn simple_shapes() {
    let Some(mut g) = gpu() else { return };
    let mut t = T::new();
    t.rect(None, Rect::new(10.0, 10.0, 60.0, 40.0), Color::rgb(0x18, 0xA0, 0xFB));
    let e = t.add(None, |id| Node::ellipse(id, Rect::new(90.0, 10.0, 50.0, 50.0)));
    t.node(e).fills = vec![Paint::solid(Color::rgb(0xF2, 0x4E, 0x1E))];
    let c = compare(&mut g, "simple", &t.doc, t.page, Viewport::default(), (160, 80), 8);
    // tiny scene: nearly all of its mismatches are edge pixels
    assert!(c.report.ratio < 0.03, "{:?}", c.report);
}

#[test]
fn render_demo() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = demo::demo_scene();
    let vp = Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 };
    let c = compare(&mut g, "render_demo", &doc, page, vp, (1560, 2000), 8);
    // target: under 1% at tolerance 8 (strict: every edge pixel counts). The bounds are
    // a little looser than what the reference machine measures, since MSAA sample
    // patterns differ between GPUs.
    assert!(c.report.ratio < 0.02, "strict mismatch {:.3}%", c.report.ratio * 100.0);
    assert!(c.report_aa.ratio < 0.005, "mismatch with AA forgiven {:.3}%", c.report_aa.ratio * 100.0);
}

#[test]
fn render_demo_zoomed() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = demo::demo_scene();
    // zoomed in and panned: nothing is re-rasterised on the GPU, geometry is re-flattened by bucket
    let vp = Viewport { offset: Vec2::new(300.0, 340.0), zoom: 3.0, dpr: 1.0 };
    let c = compare(&mut g, "render_demo_zoom3", &doc, page, vp, (1200, 900), 8);
    assert!(c.report.ratio < 0.02, "mismatch {:.3}%", c.report.ratio * 100.0);
}

fn fx_scene() -> (Document, NodeId) {
    let mut t = T::new();
    t.doc.images.insert("checker".into(), demo::checker());
    // stripes behind glass
    for i in 0..10 {
        t.rect(None, Rect::new(10.0 + i as f32 * 14.0, 10.0, 7.0, 90.0), Color::rgb(0x22, 0x22, 0x22));
    }
    let glass = t.rect(None, Rect::new(30.0, 30.0, 90.0, 50.0), Color::rgba(255, 255, 255, 40));
    t.node(glass).corner_radii = [14.0; 4];
    t.node(glass).effects =
        vec![Effect::Glass(GlassEffect { light_angle: 45.0, light_intensity: 0.7, refraction: 0.5, depth: 10.0, dispersion: 0.3, frost: 4.0, visible: true })];
    // noise (shape fast path) and noise on a group (general path)
    let n = t.rect(None, Rect::new(180.0, 10.0, 80.0, 60.0), Color::rgb(0x44, 0x88, 0xDD));
    t.node(n).effects = vec![Effect::Noise(NoiseEffect {
        mode: NoiseMode::Duo { color: Color::rgb(0, 0, 0), secondary: Color::WHITE },
        size: 1.5,
        density: 0.5,
        opacity: 0.4,
        blend: BlendMode::Normal,
        seed: 7,
        visible: true,
    })];
    let tx = t.rect(None, Rect::new(180.0, 90.0, 80.0, 50.0), Color::rgb(0xE0, 0x50, 0x30));
    t.node(tx).effects = vec![Effect::Texture(TextureEffect { size: 6.0, radius: 4.0, clip_to_shape: false, seed: 3, visible: true })];
    // clipped rounded frame with rotated child, image + blend modes
    let fr = t.add(None, |id| Node::frame(id, Rect::new(10.0, 120.0, 100.0, 70.0)));
    if let NodeKind::Frame(p) = &mut t.node(fr).kind {
        p.clips_content = true;
    }
    t.node(fr).corner_radii = [18.0; 4];
    let ch = t.rect(Some(fr), Rect::new(30.0, 20.0, 90.0, 60.0), Color::rgb(0x22, 0xAA, 0x66));
    t.node(ch).transform = Affine::translate(30.0, 20.0) * Affine::rotate(0.5);
    let im = t.rect(None, Rect::new(130.0, 120.0, 60.0, 60.0), Color::WHITE);
    t.node(im).fills = vec![Paint {
        kind: PaintKind::Image { image_ref: "checker".into(), scale_mode: ImageScaleMode::Fill },
        opacity: 0.9,
        visible: true,
        blend: BlendMode::Multiply,
    }];
    let modes = [BlendMode::Screen, BlendMode::Overlay, BlendMode::Difference, BlendMode::Hue, BlendMode::LinearDodge, BlendMode::SoftLight];
    for (i, m) in modes.iter().enumerate() {
        let r = t.rect(None, Rect::new(200.0 + i as f32 * 10.0, 160.0, 40.0, 30.0), Color::rgb(0xC0, 0x40, 0x90));
        t.node(r).blend = *m;
    }
    (t.doc, t.page)
}

#[test]
fn effects_and_blends() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = fx_scene();
    let c = compare(&mut g, "fx", &doc, page, Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 }, (600, 420), 8);
    assert!(c.report.ratio < 0.03, "mismatch {:.3}%", c.report.ratio * 100.0);
}

#[test]
fn cpu_fallback_matches_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = fx_scene();
    // every node with an effect goes through the CPU-into-texture fallback
    g.compiler.opts.force_fallback = Some(Arc::new(|n: &Node| !n.effects.is_empty()));
    let c = compare(&mut g, "fx_fallback", &doc, page, Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 }, (600, 420), 8);
    let stats = g.compile(&doc, page, 1, &Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 }, (600, 420)).stats();
    assert!(stats.fallback_layers >= 2, "{stats:?}");
    assert!(c.report.ratio < 0.02, "mismatch {:.3}%", c.report.ratio * 100.0);
}

#[test]
fn extreme_effects_render() {
    let Some(mut g) = gpu() else { return };
    let mut t = T::new();
    t.rect(None, Rect::new(0.0, 0.0, 200.0, 200.0), Color::rgb(0x20, 0x60, 0xC0));
    let a = t.rect(None, Rect::new(40.0, 40.0, 80.0, 80.0), Color::rgb(0xF0, 0x40, 0x20));
    t.node(a).effects = vec![
        Effect::LayerBlur { radius: 400.0, visible: true },
        Effect::DropShadow { color: Color::BLACK, offset: Vec2::new(30.0, 30.0), radius: 300.0, spread: 20.0, visible: true, show_behind: true },
    ];
    let b = t.rect(None, Rect::new(100.0, 100.0, 90.0, 90.0), Color::rgba(255, 255, 255, 80));
    t.node(b).effects = vec![Effect::BackgroundBlur { radius: 250.0, visible: true }];
    let c = compare(&mut g, "extreme", &t.doc, t.page, Viewport { offset: Vec2::ZERO, zoom: 2.0, dpr: 1.0 }, (400, 400), 8);
    // huge blurs are approximated at reduced resolution; the image must still be close
    assert!(c.report.ssim > 0.9, "{:?}", c.report);
}

fn board_scene() -> (Document, NodeId) {
    let mut t = T::new();
    t.doc.kind = DocumentKind::Board;
    t.doc.get_mut(t.page).unwrap().kind = NodeKind::Page { background: Color::rgb(0xF4, 0xF4, 0xF4) };
    let mut ids = Vec::new();
    let notes =
        [(0xFFE58F_u32, "Ship the board mode"), (0xB6D4FF, "A longer note with enough words to make the text shrink to fit the square"), (0xC4EBA6, "")];
    for (i, (c, text)) in notes.iter().enumerate() {
        let id = t.add(None, |id| Node::sticky(id, Rect::new(20.0 + i as f32 * 170.0, 20.0, 150.0, 150.0), *text));
        if let NodeKind::Sticky(s) = &mut t.node(id).kind {
            s.color = Color::rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8);
        }
        ids.push(id);
    }
    let shape = t.add(None, |id| Node::shape_with_text(id, Rect::new(30.0, 230.0, 140.0, 90.0), ShapeKind::Diamond, "Decide"));
    let shape2 = t.add(None, |id| Node::shape_with_text(id, Rect::new(300.0, 230.0, 140.0, 90.0), ShapeKind::RoundedRect, "Do it"));
    let connect = |t: &mut T, a: NodeId, b: NodeId, line: ConnectorLine, label: &str| {
        let c = t.add(None, |id| {
            let mut p =
                ConnectorProps { start: ConnectorEnd::attached(a, Magnet::Auto), end: ConnectorEnd::attached(b, Magnet::Auto), line, ..Default::default() };
            p.end_cap = ConnectorCap::Triangle;
            p.start_cap = ConnectorCap::Circle;
            if !label.is_empty() {
                let mut l = board_text(13.0, TextAlignH::Center, TextAlignV::Center);
                l.content = label.into();
                p.label = Some(l);
            }
            Node::connector(id, p)
        });
        sync_connector(&mut t.doc, c);
    };
    connect(&mut t, ids[0], shape, ConnectorLine::Elbowed, "");
    connect(&mut t, shape, shape2, ConnectorLine::Straight, "yes");
    connect(&mut t, ids[2], shape2, ConnectorLine::Curved, "");
    for (i, k) in STAMP_KINDS.iter().enumerate() {
        t.add(None, |id| Node::stamp(id, Vec2::new(30.0 + i as f32 * 60.0, 350.0), k, 48.0));
    }
    (t.doc, t.page)
}

#[test]
fn board_kinds_match_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = board_scene();
    let c = compare(&mut g, "board", &doc, page, Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 }, (1000, 800), 8);
    assert!(c.report.ratio < 0.03, "mismatch {:.3}%", c.report.ratio * 100.0);
    assert!(c.report_aa.ratio < 0.01, "mismatch with AA forgiven {:.3}%", c.report_aa.ratio * 100.0);
    // the notes really are drawn (not an empty page): the yellow sticky fill is on screen
    let p = c.gpu.pixel(2 * 90, 2 * 150).unwrap();
    assert!(p.red() > 240 && p.green() > 200 && p.blue() < 180, "{p:?}");
}

#[test]
fn board_kinds_zoomed_match_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = board_scene();
    let vp = Viewport { offset: Vec2::new(20.0, 10.0), zoom: 2.5, dpr: 1.0 };
    let c = compare(&mut g, "board_zoom", &doc, page, vp, (900, 700), 8);
    assert!(c.report.ratio < 0.03, "mismatch {:.3}%", c.report.ratio * 100.0);
}

#[test]
fn board_kinds_through_the_cpu_fallback_match_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = board_scene();
    // everything the stickies lower to (shadowed bodies, their text) takes the CPU-into-texture path
    g.compiler.opts.force_fallback = Some(Arc::new(|n: &Node| !n.effects.is_empty() || matches!(n.kind, NodeKind::Text(_))));
    let vp = Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 2.0 };
    let c = compare(&mut g, "board_fallback", &doc, page, vp, (1000, 800), 8);
    assert!(c.report.ratio < 0.02, "mismatch {:.3}%", c.report.ratio * 100.0);
}

/// Sections (translucent body, title pill above the box, children inside) and tables (cell fills,
/// grid lines, cell text).
fn blocks_scene() -> (Document, NodeId) {
    let mut t = T::new();
    t.doc.kind = DocumentKind::Board;
    t.doc.get_mut(t.page).unwrap().kind = NodeKind::Page { background: Color::rgb(0xF4, 0xF4, 0xF4) };
    for (i, c) in [SectionColor::Blue, SectionColor::LightYellow, SectionColor::Pink].into_iter().enumerate() {
        let s = t.add(None, |id| Node::board_section(id, Rect::new(20.0 + i as f32 * 220.0, 60.0, 200.0, 160.0), c.label()));
        if let NodeKind::BoardSection(p) = &mut t.node(s).kind {
            p.color = c;
        }
        if i == 0 {
            t.add(Some(s), |id| Node::sticky(id, Rect::new(20.0, 20.0, 100.0, 100.0), "In a section"));
        }
        if i == 2 {
            t.add(Some(s), |id| Node::sticky(id, Rect::new(20.0, 20.0, 100.0, 100.0), "Hidden"));
            if let NodeKind::BoardSection(p) = &mut t.node(s).kind {
                p.contents_hidden = true;
            }
        }
    }
    let nodes = table_nodes(&mut t.doc, Vec2::new(30.0, 280.0), 3, 3);
    let table = nodes[0].id;
    Command::Insert { parent: t.page, index: usize::MAX, nodes }.apply(&mut t.doc).unwrap();
    for (r, c, text, fill) in [(0, 0, "Task", None), (0, 1, "Owner", Some(Color::rgb(0xB6, 0xD4, 0xFF))), (1, 2, "Done", Some(Color::rgb(0xC4, 0xEB, 0xA6)))] {
        let id = table_cell_node(&t.doc, table, r, c).unwrap();
        if let NodeKind::TableCell(cell) = &mut t.node(id).kind {
            cell.text.content = text.into();
            cell.fill = fill;
        }
    }
    (t.doc, t.page)
}

#[test]
fn board_sections_and_tables_match_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = blocks_scene();
    let c = compare(&mut g, "board_blocks", &doc, page, Viewport { offset: Vec2::new(0.0, 20.0), zoom: 1.0, dpr: 2.0 }, (1000, 800), 8);
    assert!(c.report.ratio < 0.03, "mismatch {:.3}%", c.report.ratio * 100.0);
    assert!(c.report_aa.ratio < 0.01, "mismatch with AA forgiven {:.3}%", c.report_aa.ratio * 100.0);
}

/// Drop shadows that need a blurred silhouette (ellipse, stroked rectangle, shadow hidden behind
/// the shape), the kind whose results the executor keeps between frames.
fn shadow_scene() -> (Document, NodeId, NodeId) {
    let mut t = T::new();
    let shadow = |show_behind| Effect::DropShadow {
        color: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.4 },
        offset: Vec2::new(4.0, 6.0),
        radius: 12.0,
        spread: 0.0,
        visible: true,
        show_behind,
    };
    let e = t.add(None, |id| Node::ellipse(id, Rect::new(30.0, 30.0, 80.0, 60.0)));
    t.node(e).fills = vec![Paint::solid(Color::rgb(0x18, 0xA0, 0xFB))];
    t.node(e).effects = vec![shadow(false)];
    let r = t.rect(None, Rect::new(160.0, 40.0, 90.0, 50.0), Color::rgba(0xF2, 0x4E, 0x1E, 200));
    t.node(r).stroke = Stroke { paints: vec![Paint::solid(Color::BLACK)], weight: 3.0, ..Default::default() };
    t.node(r).effects = vec![shadow(true)];
    (t.doc, t.page, r)
}

#[test]
fn cached_shadows_match_a_fresh_render_while_panning_and_editing() {
    let (Some(mut warm), Some(mut fresh)) = (gpu(), gpu()) else { return };
    let (mut doc, page, rect) = shadow_scene();
    let size = (320, 160);
    let draw = |g: &mut desigma_gpu::HeadlessGpu, doc: &Document, rev: u64, x: f32| {
        let f = g.compile(doc, page, rev, &Viewport { offset: Vec2::new(x, 0.0), zoom: 1.0, dpr: 2.0 }, size);
        g.render_frame(&f, size)
    };
    // pan by whole device pixels: later frames reuse the shadows rendered by the first
    for step in 0..4 {
        draw(&mut warm, &doc, 1, step as f32 * 3.0);
    }
    let reused = draw(&mut warm, &doc, 1, 9.0);
    assert!(warm.renderer.stats.results_reused >= 3, "{:?}", warm.renderer.stats);
    assert_eq!(reused.data(), draw(&mut fresh, &doc, 1, 9.0).data(), "a reused shadow differs from a freshly rendered one");
    // an edit of what the silhouette depends on (fill alpha, size) must not reuse the old shadow
    doc.get_mut(rect).unwrap().fills = vec![Paint::solid(Color::rgba(0xF2, 0x4E, 0x1E, 60))];
    doc.get_mut(rect).unwrap().size = Vec2::new(70.0, 50.0);
    let edited = draw(&mut warm, &doc, 2, 9.0);
    // the ellipse's shadows are still reused, the edited rectangle's is rendered again
    assert!(warm.renderer.stats.results_reused >= 2, "{:?}", warm.renderer.stats);
    let mut clean = gpu().unwrap();
    assert_eq!(edited.data(), draw(&mut clean, &doc, 2, 9.0).data(), "an edited node kept its old shadow");
}

#[test]
fn board_sections_and_tables_through_the_cpu_fallback_match_cpu() {
    let Some(mut g) = gpu() else { return };
    let (doc, page) = blocks_scene();
    // the lowered text and the translucent bodies take the CPU-into-texture path
    g.compiler.opts.force_fallback = Some(Arc::new(|n: &Node| {
        matches!(n.kind, NodeKind::Text(_)) || !n.fills.is_empty() && n.fills.iter().any(|p| matches!(&p.kind, PaintKind::Solid(c) if c.a < 1.0))
    }));
    let vp = Viewport { offset: Vec2::new(0.0, 20.0), zoom: 1.0, dpr: 2.0 };
    let c = compare(&mut g, "board_blocks_fallback", &doc, page, vp, (1000, 800), 8);
    assert!(c.report.ratio < 0.02, "mismatch {:.3}%", c.report.ratio * 100.0);
}
