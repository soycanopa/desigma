//! Figma REST (`GET /v1/files/:key`) JSON -> [`Document`].
//!
//! The `.fig` importer converts its kiwi message into this same JSON shape and
//! reuses this mapper, so every field name below is the REST name.

use crate::svgpath::parse_path;
use crate::FigError;
use desigma_doc::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DEPTH: usize = 300;

// ------------------------------------------------------------ JSON helpers

pub(crate) fn num(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}
pub(crate) fn fnum(v: &Value, k: &str) -> Option<f32> {
    v.get(k).and_then(num)
}
pub(crate) fn fstr<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}
pub(crate) fn fbool(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}
pub(crate) fn farr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

pub(crate) fn vec2(v: &Value) -> Option<Vec2> {
    Some(Vec2::new(fnum(v, "x")?, fnum(v, "y")?))
}

pub(crate) fn color(v: &Value) -> Option<Color> {
    Some(Color { r: fnum(v, "r")?, g: fnum(v, "g")?, b: fnum(v, "b")?, a: fnum(v, "a").unwrap_or(1.0) })
}

pub(crate) fn blend(s: &str) -> BlendMode {
    match s {
        "PASS_THROUGH" => BlendMode::PassThrough,
        "DARKEN" => BlendMode::Darken,
        "MULTIPLY" => BlendMode::Multiply,
        // No linear burn/dodge in the document model; closest relatives.
        "LINEAR_BURN" => BlendMode::LinearBurn,
        "COLOR_BURN" => BlendMode::ColorBurn,
        "LIGHTEN" => BlendMode::Lighten,
        "SCREEN" => BlendMode::Screen,
        "LINEAR_DODGE" => BlendMode::LinearDodge,
        "COLOR_DODGE" => BlendMode::ColorDodge,
        "OVERLAY" => BlendMode::Overlay,
        "SOFT_LIGHT" => BlendMode::SoftLight,
        "HARD_LIGHT" => BlendMode::HardLight,
        "DIFFERENCE" => BlendMode::Difference,
        "EXCLUSION" => BlendMode::Exclusion,
        "HUE" => BlendMode::Hue,
        "SATURATION" => BlendMode::Saturation,
        "COLOR" => BlendMode::Color,
        "LUMINOSITY" => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

// ------------------------------------------------------------ paints & effects

pub(crate) fn parse_paint(v: &Value) -> Option<Paint> {
    let t = fstr(v, "type")?;
    let handles = || -> [Vec2; 3] {
        let h = farr(v, "gradientHandlePositions");
        let g = |i: usize, d: Vec2| h.get(i).and_then(vec2).unwrap_or(d);
        [g(0, Vec2::new(0.5, 0.0)), g(1, Vec2::new(0.5, 1.0)), g(2, Vec2::new(1.0, 0.0))]
    };
    let stops = || -> Vec<GradientStop> {
        farr(v, "gradientStops").iter().filter_map(|s| Some(GradientStop { pos: fnum(s, "position")?, color: s.get("color").and_then(color)? })).collect()
    };
    let kind = match t {
        "SOLID" => PaintKind::Solid(v.get("color").and_then(color).unwrap_or(Color::BLACK)),
        "GRADIENT_LINEAR" => PaintKind::LinearGradient { handles: handles(), stops: stops() },
        "GRADIENT_RADIAL" => PaintKind::RadialGradient { handles: handles(), stops: stops() },
        "GRADIENT_ANGULAR" => PaintKind::AngularGradient { handles: handles(), stops: stops() },
        "GRADIENT_DIAMOND" => PaintKind::DiamondGradient { handles: handles(), stops: stops() },
        "IMAGE" => PaintKind::Image {
            image_ref: fstr(v, "imageRef")?.to_string(),
            scale_mode: match fstr(v, "scaleMode") {
                Some("FIT") => ImageScaleMode::Fit,
                Some("TILE") => ImageScaleMode::Tile,
                Some("STRETCH") => ImageScaleMode::Crop,
                _ => ImageScaleMode::Fill,
            },
        },
        _ => return None, // EMOJI, VIDEO, ...
    };
    Some(Paint {
        kind,
        opacity: fnum(v, "opacity").unwrap_or(1.0),
        visible: fbool(v, "visible").unwrap_or(true),
        blend: fstr(v, "blendMode").map(blend).unwrap_or(BlendMode::Normal),
    })
}

pub(crate) fn parse_paints(v: Option<&Value>) -> Vec<Paint> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(parse_paint).collect()).unwrap_or_default()
}

fn parse_effect(v: &Value) -> Option<Effect> {
    let visible = fbool(v, "visible").unwrap_or(true);
    let radius = fnum(v, "radius").unwrap_or(0.0);
    let col = v.get("color").and_then(color).unwrap_or(Color::BLACK.with_alpha(0.25));
    let offset = v.get("offset").and_then(vec2).unwrap_or(Vec2::ZERO);
    let spread = fnum(v, "spread").unwrap_or(0.0);
    Some(match fstr(v, "type")? {
        "DROP_SHADOW" => Effect::DropShadow { color: col, offset, radius, spread, visible, show_behind: fbool(v, "showShadowBehindNode").unwrap_or(false) },
        "INNER_SHADOW" => Effect::InnerShadow { color: col, offset, radius, spread, visible },
        "LAYER_BLUR" => Effect::LayerBlur { radius, visible },
        "BACKGROUND_BLUR" => Effect::BackgroundBlur { radius, visible },
        // Noise / texture / glass: every field is optional; missing ones fall back to the
        // editor defaults. Names match what `export.rs` writes.
        "NOISE" => {
            let primary = v.get("color").and_then(color).unwrap_or(Color::BLACK);
            let mode = match fstr(v, "noiseType").map(str::to_ascii_uppercase).as_deref() {
                Some("DUOTONE") => NoiseMode::Duo { color: primary, secondary: v.get("secondaryColor").and_then(color).unwrap_or(Color::WHITE) },
                Some("MULTITONE") => NoiseMode::Multi,
                _ => NoiseMode::Mono { color: primary },
            };
            Effect::Noise(NoiseEffect {
                mode,
                size: fnum(v, "noiseSize").unwrap_or(1.0),
                density: fnum(v, "density").unwrap_or(0.5).clamp(0.0, 1.0),
                opacity: fnum(v, "opacity").unwrap_or(0.25).clamp(0.0, 1.0),
                blend: fstr(v, "blendMode").map(blend).unwrap_or(BlendMode::Normal),
                seed: fnum(v, "seed").unwrap_or(1.0).max(0.0) as u32,
                visible,
            })
        }
        "TEXTURE" => Effect::Texture(TextureEffect {
            size: fnum(v, "noiseSize").unwrap_or(2.0),
            radius: fnum(v, "radius").unwrap_or(2.0),
            clip_to_shape: fbool(v, "clipToShape").unwrap_or(true),
            seed: fnum(v, "seed").unwrap_or(1.0).max(0.0) as u32,
            visible,
        }),
        "GLASS" => Effect::Glass(GlassEffect {
            light_angle: fnum(v, "lightAngle").unwrap_or(315.0),
            light_intensity: fnum(v, "lightIntensity").unwrap_or(0.5).clamp(0.0, 1.0),
            refraction: fnum(v, "refraction").unwrap_or(0.5).clamp(0.0, 1.0),
            depth: fnum(v, "depth").unwrap_or(10.0),
            dispersion: fnum(v, "dispersion").unwrap_or(0.25).clamp(0.0, 1.0),
            frost: fnum(v, "radius").unwrap_or(4.0),
            visible,
        }),
        _ => return None,
    })
}

/// `exportSettings` of a node: format, suffix and the SCALE / WIDTH / HEIGHT constraint.
pub(crate) fn parse_export_settings(v: &Value) -> Vec<ExportPreset> {
    farr(v, "exportSettings")
        .iter()
        .map(|e| {
            let format = match fstr(e, "format").unwrap_or("PNG").to_ascii_uppercase().as_str() {
                "JPG" | "JPEG" => ExportFileFormat::Jpg,
                "SVG" => ExportFileFormat::Svg,
                "PDF" => ExportFileFormat::Pdf,
                _ => ExportFileFormat::Png,
            };
            let c = e.get("constraint");
            let value = c.and_then(|c| fnum(c, "value")).unwrap_or(1.0);
            let constraint = match c.and_then(|c| fstr(c, "type")) {
                Some("WIDTH") => ExportConstraint::Width(value),
                Some("HEIGHT") => ExportConstraint::Height(value),
                _ => ExportConstraint::Scale(value),
            };
            ExportPreset { format, constraint, suffix: fstr(e, "suffix").unwrap_or("").to_string() }
        })
        .collect()
}

/// `guides` of a page or frame: `[{axis: "X" | "Y", offset}]`.
fn parse_guides(v: &Value, frame: Option<NodeId>) -> Vec<Guide> {
    farr(v, "guides")
        .iter()
        .filter_map(|g| {
            let axis = match fstr(g, "axis")?.to_ascii_uppercase().as_str() {
                "X" => GuideAxis::X,
                "Y" => GuideAxis::Y,
                _ => return None,
            };
            Some(Guide { axis, offset: fnum(g, "offset")?, frame })
        })
        .collect()
}

fn parse_grid(v: &Value) -> Option<LayoutGrid> {
    if fbool(v, "visible") == Some(false) {
        return None; // the model has no per-grid visibility flag
    }
    let col = v.get("color").and_then(color).unwrap_or(Color::rgba(255, 0, 0, 25));
    let count = v.get("count").and_then(Value::as_u64).unwrap_or(5) as u32;
    let gutter = fnum(v, "gutterSize").unwrap_or(0.0);
    let offset = fnum(v, "offset").unwrap_or(0.0);
    match fstr(v, "pattern")? {
        "GRID" => Some(LayoutGrid::Grid { size: fnum(v, "sectionSize").unwrap_or(10.0), color: col }),
        "COLUMNS" => Some(LayoutGrid::Columns { count, gutter, offset, color: col }),
        "ROWS" => Some(LayoutGrid::Rows { count, gutter, offset, color: col }),
        _ => None,
    }
}

// ------------------------------------------------------------ text

/// Byte offset of each UTF-16 unit boundary: `out[u]` is the byte offset where
/// UTF-16 index `u` falls (low surrogates map to the end of their character).
pub(crate) fn utf16_to_byte_offsets(s: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(s.len() + 1);
    for (b, c) in s.char_indices() {
        out.push(b);
        if c.len_utf16() == 2 {
            out.push(b + c.len_utf8());
        }
    }
    out.push(s.len());
    out
}

/// Applies the (possibly partial) REST `TypeStyle` fields in `v` onto `ts`.
pub(crate) fn apply_type_style(ts: &mut TextStyle, v: &Value) {
    if let Some(f) = fstr(v, "fontFamily") {
        ts.font_family = f.to_string();
    }
    if let Some(w) = fnum(v, "fontWeight") {
        ts.font_weight = w.round().clamp(1.0, 1000.0) as u16;
    }
    if let Some(i) = fbool(v, "italic") {
        ts.italic = i;
    }
    if let Some(s) = fnum(v, "fontSize") {
        ts.font_size = s;
    }
    if let Some(a) = fstr(v, "textAlignHorizontal") {
        ts.align_h = match a {
            "CENTER" => TextAlignH::Center,
            "RIGHT" => TextAlignH::Right,
            "JUSTIFIED" => TextAlignH::Justified,
            _ => TextAlignH::Left,
        };
    }
    if let Some(a) = fstr(v, "textAlignVertical") {
        ts.align_v = match a {
            "CENTER" => TextAlignV::Center,
            "BOTTOM" => TextAlignV::Bottom,
            _ => TextAlignV::Top,
        };
    }
    if let Some(l) = fnum(v, "letterSpacing") {
        ts.letter_spacing = l;
    }
    if let Some(p) = fnum(v, "paragraphSpacing") {
        ts.paragraph_spacing = p;
    }
    // Percentage spacing from a partial `.fig` override: resolved against the final size.
    if let Some(p) = fnum(v, "letterSpacingPercent") {
        ts.letter_spacing = p / 100.0 * ts.font_size;
    }
    match fstr(v, "lineHeightUnit") {
        Some("PIXELS") => {
            if let Some(px) = fnum(v, "lineHeightPx") {
                ts.line_height = LineHeight::Pixels(px);
            }
        }
        Some("FONT_SIZE_%") => {
            if let Some(p) = fnum(v, "lineHeightPercentFontSize").or_else(|| fnum(v, "lineHeightPercent")) {
                ts.line_height = LineHeight::Percent(p);
            }
        }
        Some(_) => ts.line_height = LineHeight::Auto,
        None => {
            if let Some(px) = fnum(v, "lineHeightPx") {
                ts.line_height = LineHeight::Pixels(px);
            }
        }
    }
    if let Some(c) = fstr(v, "textCase") {
        ts.case = match c {
            "UPPER" => TextCase::Upper,
            "LOWER" => TextCase::Lower,
            "TITLE" => TextCase::Title,
            "SMALL_CAPS" | "SMALL_CAPS_FORCED" => TextCase::SmallCaps,
            _ => TextCase::Original,
        };
    }
    if let Some(d) = fstr(v, "textDecoration") {
        ts.decoration = match d {
            "UNDERLINE" => TextDecoration::Underline,
            "STRIKETHROUGH" => TextDecoration::Strikethrough,
            _ => TextDecoration::None,
        };
    }
    if let Some(f) = v.get("opentypeFlags") {
        crate::textfmt::merge_features(&mut ts.features, crate::textfmt::features_from_flags(f));
    }
    if let Some(f) = v.get("fills") {
        ts.fills = parse_paints(Some(f));
    }
}

fn auto_resize(s: &str) -> TextAutoResize {
    match s {
        "WIDTH_AND_HEIGHT" => TextAutoResize::WidthAndHeight,
        "HEIGHT" => TextAutoResize::Height,
        "TRUNCATE" => TextAutoResize::Truncate,
        _ => TextAutoResize::None,
    }
}

// ------------------------------------------------------------ importer

struct Ctx {
    doc: Document,
    ids: BTreeMap<String, NodeId>,
    /// (instance, REST componentId)
    instances: Vec<(NodeId, String)>,
    /// Instances whose component is not in this file (library components of
    /// other files), by REST componentId.
    unresolved: Vec<(NodeId, String)>,
    component_keys: BTreeMap<String, String>,
    /// `.fig` instance overrides / derived data, keyed by instance.
    overrides: BTreeMap<NodeId, Overrides>,
    next_clone: u64,
    /// The file is a FigJam board: `SECTION` is a board section.
    figjam: bool,
    /// `.fig` override keys (`_overrideKey`): the id override paths use for a
    /// node. Clones inherit the key of the node they copy.
    override_keys: BTreeMap<NodeId, String>,
    /// `.fig` `uniformScaleFactor` of instances made with the scale tool.
    scales: BTreeMap<NodeId, f32>,
    /// Library components and sets of `.fig` internal canvases: node ->
    /// (`componentKey`, `sourceLibraryKey`).
    library_keys: BTreeMap<NodeId, (String, Option<String>)>,
    /// Instance-swap property values naming a component by REST id:
    /// (node, property name, REST component id, is a definition default).
    pending_swaps: Vec<(NodeId, String, String, bool)>,
    /// REST `overrides` entries: (instance, node id inside it, field names).
    rest_overrides: Vec<(NodeId, String, Vec<String>)>,
    /// Prototype data to resolve once every node id is known.
    proto: Vec<(NodeId, Value)>,
    /// Overlay presentation of potential overlay destinations, by REST id.
    overlay_props: BTreeMap<String, Value>,
    /// `flowStartingPoints` of each page.
    flow_pages: Vec<(NodeId, Value)>,
}

/// Overrides carried on `.fig` instance JSON as `_overrides` / `_derived`:
/// `[{"path": ["1:6", ...], "set": {<partial REST node fields>}}]`.
#[derive(Default)]
struct Overrides {
    set: Vec<OverrideEntry>,
    derived: Vec<OverrideEntry>,
}

struct OverrideEntry {
    path: Vec<String>,
    set: Value,
}

fn parse_override_entries(v: Option<&Value>) -> Vec<OverrideEntry> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let path = e.get("path")?.as_array()?.iter().filter_map(|s| s.as_str().map(String::from)).collect();
                    Some(OverrideEntry { path, set: e.get("set")?.clone() })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
struct ParentInfo {
    /// Absolute bounding-box origin of the parent, for the no-transform fallback.
    origin: Vec2,
    layout: Option<LayoutDirection>,
}

/// Parses REST JSON text (rejecting pathologically deep nesting).
pub(crate) fn parse_json_text(json: &str) -> Result<Value, FigError> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for &b in json.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > 600 {
                    return Err(FigError::Unsupported("JSON nesting too deep".into()));
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let mut de = serde_json::Deserializer::from_str(json);
    de.disable_recursion_limit();
    let v = serde::Deserialize::deserialize(&mut de)?;
    de.end()?;
    Ok(v)
}

pub fn import_rest_json(json: &str) -> Result<Document, FigError> {
    let v = parse_json_text(json)?;
    import_rest_value(&v, false)
}

/// Maps an already-parsed REST-shaped value. `expand_instances` clones the
/// component subtree into childless instances (used by `.fig`, where instance
/// children are often not stored).
pub(crate) fn import_rest_value(v: &Value, expand_instances: bool) -> Result<Document, FigError> {
    let root = match v.get("document") {
        Some(d) => d,
        None if fstr(v, "type").is_some() => v,
        None => return Err(FigError::Format("REST JSON has no `document`".into())),
    };
    let mut cx = Ctx {
        doc: Document::new(fstr(v, "name").unwrap_or("Untitled")),
        ids: BTreeMap::new(),
        instances: Vec::new(),
        unresolved: Vec::new(),
        component_keys: BTreeMap::new(),
        overrides: BTreeMap::new(),
        next_clone: 0,
        figjam: crate::board::is_figjam(v),
        override_keys: BTreeMap::new(),
        scales: BTreeMap::new(),
        library_keys: BTreeMap::new(),
        pending_swaps: Vec::new(),
        rest_overrides: Vec::new(),
        proto: Vec::new(),
        overlay_props: BTreeMap::new(),
        flow_pages: Vec::new(),
    };
    if let Some(comps) = v.get("components").and_then(Value::as_object) {
        for (id, c) in comps {
            if let Some(k) = fstr(c, "key") {
                cx.component_keys.insert(id.clone(), k.to_string());
            }
        }
    }
    // Variables first: instance overrides bound to variables resolve their
    // values while the nodes are imported.
    crate::variables::import_variables_value(&mut cx.doc, v);
    let pages: Vec<&Value> = match fstr(root, "type") {
        Some("CANVAS") => vec![root],
        _ => farr(root, "children").iter().filter(|c| fstr(c, "type") == Some("CANVAS")).collect(),
    };
    for p in pages {
        cx.import_page(p);
    }
    // `.fig` internal-only canvases: what the file uses from libraries.
    let library_pages = farr(v, "libraryPages");
    if !library_pages.is_empty() {
        let lib = ensure_library_page(&mut cx.doc);
        let info = ParentInfo { origin: Vec2::ZERO, layout: None };
        for p in library_pages {
            for c in farr(p, "children") {
                cx.import_node(c, lib, info, 0);
            }
        }
    }
    cx.finish(expand_instances);
    cx.register_library_assets();
    cx.doc.kind = if crate::board::is_figjam(v) { DocumentKind::Board } else { DocumentKind::Design };
    if cx.doc.kind == DocumentKind::Board {
        // a board draws its own dotted canvas: the pages are transparent over it
        for p in cx.doc.pages.clone() {
            if let Some(NodeKind::Page { background }) = cx.doc.get_mut(p).map(|n| &mut n.kind) {
                background.a = 0.0;
            }
        }
    }
    crate::board::link_connectors(&mut cx.doc, &cx.ids, root);
    cx.link_remote_components(v);
    cx.doc.style_meta = style_meta(v.get("styles"), &cx.doc);
    Ok(cx.doc)
}

impl Ctx {
    fn import_page(&mut self, v: &Value) {
        let pid = self.doc.add_page(fstr(v, "name").unwrap_or("Page"));
        if let Some(src) = fstr(v, "id") {
            self.ids.insert(src.to_string(), pid);
        }
        let bg = v.get("backgroundColor").and_then(color).unwrap_or(Color::rgb(0xE5, 0xE5, 0xE5));
        if let Some(n) = self.doc.get_mut(pid) {
            n.kind = NodeKind::Page { background: bg.with_alpha(1.0) };
            n.source_id = fstr(v, "id").map(String::from);
        }
        if let Some(fl) = v.get("flowStartingPoints") {
            self.flow_pages.push((pid, serde_json::json!({ "flowStartingPoints": fl })));
        }
        let guides = parse_guides(v, None);
        if !guides.is_empty() {
            self.doc.guides.entry(pid).or_default().extend(guides);
        }
        if let Some(t) = v.get("motionTimeline").and_then(|t| serde_json::from_value::<TimelineSettings>(t.clone()).ok()) {
            let t = t.sanitized();
            if !t.is_default() {
                self.doc.timelines.insert(pid, t);
            }
        }
        let info = ParentInfo { origin: Vec2::ZERO, layout: None };
        for c in farr(v, "children") {
            self.import_node(c, pid, info, 0);
        }
    }

    fn import_node(&mut self, v: &Value, parent: NodeId, pinfo: ParentInfo, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(ty) = fstr(v, "type") else { return };
        if ty == "CANVAS" || ty == "DOCUMENT" {
            return;
        }
        let id = self.doc.alloc_id();
        let (mut node, my_layout) = self.build_node(id, ty, v, pinfo);
        node.source_id = fstr(v, "id").map(String::from);
        if let Some(src) = &node.source_id {
            self.ids.insert(src.clone(), id);
        }
        crate::proto::apply_scroll(&mut node, v);
        if let Some(k) = fstr(v, "_overrideKey") {
            self.override_keys.insert(id, k.to_string());
        }
        if let (Some(k), true) = (fstr(v, "componentKey"), matches!(node.kind, NodeKind::Component(_) | NodeKind::ComponentSet(_))) {
            self.library_keys.insert(id, (k.to_string(), fstr(v, "sourceLibraryKey").map(String::from)));
        }
        let is_instance = matches!(node.kind, NodeKind::Instance { .. });
        let origin = abs_origin(v).unwrap_or(pinfo.origin);
        let container = node.kind.is_container();
        let is_frame = node.kind.frame_props().is_some();
        if self.doc.insert(parent, node).is_err() {
            return;
        }
        if is_frame {
            let guides = parse_guides(v, Some(id));
            if let (false, Some(page)) = (guides.is_empty(), self.doc.page_of(id)) {
                self.doc.guides.entry(page).or_default().extend(guides);
            }
        }
        if let Some(raw) = crate::proto::collect(v) {
            self.proto.push((id, raw));
        }
        if let Some(o) = crate::proto::collect_overlay(v) {
            if let Some(src) = fstr(v, "id") {
                self.overlay_props.insert(src.to_string(), o);
            }
        }
        if crate::proto::is_fixed(v) {
            if let Some(p) = self.doc.get_mut(parent) {
                p.prototype.fixed_children.push(id);
            }
        }
        if is_instance {
            if let Some(cid) = fstr(v, "componentId") {
                self.instances.push((id, cid.to_string()));
            }
            if let Some(s) = fnum(v, "_uniformScale").filter(|s| *s > 0.0 && (*s - 1.0).abs() > 1e-4) {
                self.scales.insert(id, s);
            }
            for e in farr(v, "overrides") {
                if let Some(nid) = fstr(e, "id") {
                    let fields: Vec<String> = farr(e, "overriddenFields").iter().filter_map(|f| f.as_str().map(String::from)).collect();
                    self.rest_overrides.push((id, nid.to_string(), fields));
                }
            }
            let ov = Overrides { set: parse_override_entries(v.get("_overrides")), derived: parse_override_entries(v.get("_derived")) };
            if !ov.set.is_empty() || !ov.derived.is_empty() {
                self.overrides.insert(id, ov);
            }
        }
        if container {
            let info = ParentInfo { origin, layout: my_layout };
            if fstr(v, "type") == Some("TABLE") {
                for c in crate::board::table_children(v) {
                    self.import_node(c, id, info, depth + 1);
                }
                crate::board::finish_table(&mut self.doc, id);
            } else {
                for c in farr(v, "children") {
                    self.import_node(c, id, info, depth + 1);
                }
            }
        }
    }

    /// Returns the node and, for auto-layout frames, its direction.
    fn build_node(&mut self, id: NodeId, ty: &str, v: &Value, pinfo: ParentInfo) -> (Node, Option<LayoutDirection>) {
        let frame = || frame_props(v);
        // Text outlines (glyph paths) are not stored; the renderer lays text out itself.
        let fill_geo = (ty != "TEXT").then(|| parse_geometry(farr(v, "fillGeometry"))).filter(|g| !g.is_empty());
        let stroke_geo = (ty != "TEXT").then(|| parse_geometry(farr(v, "strokeGeometry"))).filter(|g| !g.is_empty());
        let kind = match ty {
            "FRAME" => NodeKind::Frame(frame()),
            "GROUP" => NodeKind::Group,
            "SECTION" if !self.figjam => NodeKind::Section,
            "SECTION" | "STICKY" | "SHAPE_WITH_TEXT" | "STAMP" | "CONNECTOR" | "TABLE" | "TABLE_CELL" => {
                crate::board::kind_from_json(ty, v, pinfo.origin).unwrap_or(NodeKind::Unknown { type_name: ty.to_string() })
            }
            "COMPONENT" => NodeKind::Component(frame()),
            "COMPONENT_SET" => NodeKind::ComponentSet(frame()),
            "INSTANCE" => NodeKind::Instance {
                frame: frame(),
                component: None,
                component_key: fstr(v, "componentId").and_then(|c| self.component_keys.get(c).cloned()).or_else(|| fstr(v, "componentKey").map(String::from)),
            },
            "RECTANGLE" => NodeKind::Rectangle,
            "ELLIPSE" => {
                let a = v.get("arcData");
                let d = ArcData::default();
                NodeKind::Ellipse(ArcData {
                    start: a.and_then(|a| fnum(a, "startingAngle")).unwrap_or(d.start),
                    end: a.and_then(|a| fnum(a, "endingAngle")).unwrap_or(d.end),
                    inner_radius: a.and_then(|a| fnum(a, "innerRadius")).unwrap_or(0.0),
                })
            }
            "LINE" => NodeKind::Line,
            "REGULAR_POLYGON" => NodeKind::Polygon { count: v.get("pointCount").and_then(Value::as_u64).unwrap_or(3).clamp(3, 1000) as u32 },
            "STAR" => NodeKind::Star {
                count: v.get("pointCount").and_then(Value::as_u64).unwrap_or(5).clamp(3, 1000) as u32,
                inner_ratio: fnum(v, "starInnerRadius").unwrap_or(0.382),
            },
            "VECTOR" => {
                // an editable network when the file carries one; Figma's own outline stays the rendered geometry
                let network = v.get("vectorNetwork").and_then(crate::vecnet::network_from_json).filter(|n| !n.vertices.is_empty());
                let paths = fill_geo.clone().or_else(|| network.as_ref().map(|n| n.to_paths())).unwrap_or_default();
                NodeKind::Vector { paths, network }
            }
            "TEXT" => NodeKind::Text(text_props(v)),
            "BOOLEAN_OPERATION" => NodeKind::BooleanOp(match fstr(v, "booleanOperation") {
                Some("SUBTRACT") => BooleanOp::Subtract,
                Some("INTERSECT") => BooleanOp::Intersect,
                Some("EXCLUDE") => BooleanOp::Exclude,
                _ => BooleanOp::Union,
            }),
            other => NodeKind::Unknown { type_name: other.to_string() },
        };
        let is_text = matches!(kind, NodeKind::Text(_));
        let my_layout = kind.frame_props().and_then(|f| f.auto_layout).map(|a| a.direction);
        let default_blend = if matches!(kind, NodeKind::Rectangle | NodeKind::Ellipse(_) | NodeKind::Line | NodeKind::Vector { .. } | NodeKind::Text(_)) {
            BlendMode::Normal
        } else {
            BlendMode::PassThrough
        };
        let mut n = Node::new(id, kind);
        if let Some(s) = fstr(v, "name") {
            n.name = s.to_string();
        }
        n.visible = fbool(v, "visible").unwrap_or(true);
        n.locked = fbool(v, "locked").unwrap_or(false);
        n.opacity = fnum(v, "opacity").unwrap_or(1.0);
        n.variables = crate::variables::parse_node_variables(v);
        n.blend = fstr(v, "blendMode").map(blend).unwrap_or(default_blend);
        n.is_mask = fbool(v, "isMask").unwrap_or(false);
        n.export_settings = parse_export_settings(v);
        n.dev = crate::dev::parse_dev(v);
        n.fill_geometry = fill_geo;
        n.stroke_geometry = stroke_geo;

        // Placement.
        let bbox = v.get("absoluteBoundingBox");
        n.transform = match v.get("relativeTransform").and_then(parse_transform) {
            Some(t) => t,
            None => match bbox {
                Some(b) => Affine::translate(fnum(b, "x").unwrap_or(0.0) - pinfo.origin.x, fnum(b, "y").unwrap_or(0.0) - pinfo.origin.y),
                None => Affine::IDENTITY,
            },
        };
        n.size = v.get("size").and_then(vec2).or_else(|| bbox.and_then(|b| Some(Vec2::new(fnum(b, "width")?, fnum(b, "height")?)))).unwrap_or(Vec2::ZERO);
        if let Some(m) = v.get("motion") {
            n.motion = crate::motion::parse_node_motion(m, &n);
        }

        // Paint & stroke.
        let fills = parse_paints(v.get("fills"));
        // Text keeps its color in `TextStyle::fills` (set by `text_props`).
        if !is_text {
            n.fills = fills;
        }
        n.stroke.paints = parse_paints(v.get("strokes"));
        if let Some(w) = fnum(v, "strokeWeight") {
            n.stroke.weight = w;
        }
        if let Some(i) = v.get("individualStrokeWeights") {
            let g = |k: &str| fnum(i, k).unwrap_or(0.0);
            n.stroke.side_weights = Some([g("top"), g("right"), g("bottom"), g("left")]);
        }
        n.stroke.align = match fstr(v, "strokeAlign") {
            Some("CENTER") => StrokeAlign::Center,
            Some("OUTSIDE") => StrokeAlign::Outside,
            _ => StrokeAlign::Inside,
        };
        n.stroke.cap = match fstr(v, "strokeCap") {
            Some("ROUND") => StrokeCap::Round,
            Some("SQUARE") => StrokeCap::Square,
            _ => StrokeCap::None,
        };
        n.stroke.join = match fstr(v, "strokeJoin") {
            Some("BEVEL") => StrokeJoin::Bevel,
            Some("ROUND") => StrokeJoin::Round,
            _ => StrokeJoin::Miter,
        };
        if let Some(l) = fnum(v, "strokeMiterLimit") {
            n.stroke.miter_limit = l;
        } else if let Some(deg) = fnum(v, "strokeMiterAngle") {
            let s = (deg.to_radians() / 2.0).sin();
            if s > 1e-4 {
                n.stroke.miter_limit = 1.0 / s;
            }
        }
        n.stroke.dashes = farr(v, "strokeDashes").iter().filter_map(num).collect();
        n.effects = farr(v, "effects").iter().filter_map(parse_effect).collect();

        // Corners.
        if let Some(r) = v.get("rectangleCornerRadii").and_then(Value::as_array).filter(|a| a.len() == 4) {
            for (i, x) in r.iter().enumerate() {
                n.corner_radii[i] = num(x).unwrap_or(0.0);
            }
        } else if let Some(r) = fnum(v, "cornerRadius") {
            n.corner_radii = [r; 4];
        }
        n.corner_smoothing = fnum(v, "cornerSmoothing").unwrap_or(0.0);

        // Constraints.
        if let Some(c) = v.get("constraints") {
            n.constraints = Constraints {
                horizontal: match fstr(c, "horizontal") {
                    Some("RIGHT") => Constraint::Max,
                    Some("CENTER") => Constraint::Center,
                    Some("LEFT_RIGHT") => Constraint::Stretch,
                    Some("SCALE") => Constraint::Scale,
                    _ => Constraint::Min,
                },
                vertical: match fstr(c, "vertical") {
                    Some("BOTTOM") => Constraint::Max,
                    Some("CENTER") => Constraint::Center,
                    Some("TOP_BOTTOM") => Constraint::Stretch,
                    Some("SCALE") => Constraint::Scale,
                    _ => Constraint::Min,
                },
            };
        }

        n.layout_child = layout_child(v, pinfo.layout, &n.kind);

        // Style references and library style definitions.
        if let Some(styles) = v.get("styles").and_then(Value::as_object) {
            for (k, sid) in styles {
                let Some(sid) = sid.as_str() else { continue };
                let key = match k.as_str() {
                    "fills" | "fill" => "fill",
                    "strokes" | "stroke" => "stroke",
                    "text" => "text",
                    "effect" | "effects" => "effect",
                    "grid" | "grids" => "grid",
                    other => other,
                };
                n.style_refs.insert(key.to_string(), sid.to_string());
                if self.doc.styles.contains_key(sid) {
                    continue;
                }
                let def = match key {
                    "fill" => Some(Style::Fill(match &n.kind {
                        NodeKind::Text(t) => t.style.fills.clone(),
                        _ => n.fills.clone(),
                    })),
                    "stroke" => Some(Style::Fill(n.stroke.paints.clone())),
                    "text" => match &n.kind {
                        NodeKind::Text(t) => Some(Style::Text(t.style.clone())),
                        _ => None,
                    },
                    "effect" => Some(Style::Effect(n.effects.clone())),
                    "grid" => n.kind.frame_props().map(|f| Style::Grid(f.grids.clone())),
                    _ => None,
                };
                if let Some(d) = def {
                    self.doc.styles.insert(sid.to_string(), d);
                }
            }
        }
        self.parse_component_props(&mut n, v);
        crate::board::finish_node(&mut n);
        (n, my_layout)
    }

    /// `componentPropertyDefinitions` / `componentProperties` /
    /// `componentPropertyReferences` -> `props` / `values` / `bindings`.
    fn parse_component_props(&mut self, n: &mut Node, v: &Value) {
        let id = n.id;
        if let Some(defs) = v.get("componentPropertyDefinitions").and_then(Value::as_object) {
            for (key, d) in defs {
                let name = prop_name(key);
                let dv = d.get("defaultValue");
                let text = || dv.and_then(Value::as_str).unwrap_or("").to_string();
                let (kind, default) = match fstr(d, "type") {
                    Some("BOOLEAN") => (PropKind::Boolean, PropValue::Bool(dv.and_then(Value::as_bool).unwrap_or(true))),
                    Some("TEXT") => (PropKind::Text, PropValue::Text(text())),
                    Some("VARIANT") => (
                        PropKind::Variant { options: farr(d, "variantOptions").iter().filter_map(|o| o.as_str().map(String::from)).collect() },
                        PropValue::Variant(text()),
                    ),
                    Some("INSTANCE_SWAP") => {
                        self.pending_swaps.push((id, name.clone(), text(), true));
                        (PropKind::InstanceSwap { preferred: Vec::new() }, PropValue::Instance(NodeId(0)))
                    }
                    _ => continue,
                };
                n.component.props.push(ComponentPropDef { name, kind, default });
            }
        }
        if let Some(vals) = v.get("componentProperties").and_then(Value::as_object) {
            for (key, d) in vals {
                let name = prop_name(key);
                let val = d.get("value");
                let text = || val.and_then(Value::as_str).unwrap_or("").to_string();
                let pv = match fstr(d, "type") {
                    Some("BOOLEAN") => PropValue::Bool(val.and_then(Value::as_bool).unwrap_or(true)),
                    Some("TEXT") => PropValue::Text(text()),
                    Some("VARIANT") => PropValue::Variant(text()),
                    Some("INSTANCE_SWAP") => {
                        self.pending_swaps.push((id, name.clone(), text(), false));
                        PropValue::Instance(NodeId(0))
                    }
                    _ => continue,
                };
                n.component.values.insert(name, pv);
            }
        }
        if let Some(refs) = v.get("componentPropertyReferences").and_then(Value::as_object) {
            for (k, p) in refs {
                let field = match k.as_str() {
                    "visible" => OverrideField::Visible,
                    "characters" => OverrideField::Characters,
                    "mainComponent" => OverrideField::MainComponent,
                    _ => continue,
                };
                if let Some(p) = p.as_str() {
                    n.component.bindings.insert(field, prop_name(p));
                }
            }
        }
    }

    fn finish(&mut self, expand_instances: bool) {
        let links = std::mem::take(&mut self.instances);
        let mut targets: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        for (inst, cid) in links {
            let Some(&comp) = self.ids.get(&cid) else {
                self.unresolved.push((inst, cid));
                continue;
            };
            if let Some(NodeKind::Instance { component, .. }) = self.doc.get_mut(inst).map(|n| &mut n.kind) {
                *component = Some(comp);
            }
            targets.insert(inst, comp);
        }
        self.link_sources(&targets);
        self.resolve_swaps();
        if !expand_instances {
            self.apply_rest_overrides();
            self.resolve_proto();
            return;
        }
        // Clone ids are handed out sequentially above every imported id, in
        // instance order, so the same input always yields the same ids.
        self.next_clone = self.doc.nodes.keys().next_back().map(|k| k.0).unwrap_or(0) + 1;
        let mut done = BTreeSet::new();
        let order: Vec<NodeId> = targets.keys().copied().collect();
        for inst in order {
            self.expand_rec(inst, &targets, &mut done);
        }
        // Instances whose component is unknown still honour their own overrides.
        let rest: Vec<NodeId> = self.overrides.keys().copied().filter(|i| !done.contains(i)).collect();
        for inst in rest {
            self.apply_overrides(inst, &targets, &mut done);
        }
        self.apply_rest_overrides();
        self.resolve_proto();
    }

    /// Instances of components defined in other files (`remote: true` in the
    /// REST `components` map, or simply absent from this file) get a stand-in
    /// main component on the hidden library page, grouped under a placeholder
    /// library named after the file key, so they keep working and are listed
    /// as coming from another file.
    fn link_remote_components(&mut self, v: &Value) {
        let unresolved = std::mem::take(&mut self.unresolved);
        if unresolved.is_empty() {
            return;
        }
        let file_key = fstr(v, "fileKey").or_else(|| fstr(v, "file_key")).unwrap_or("external");
        let library_id = format!("{}{file_key}", desigma_doc::REMOTE_LIBRARY_PREFIX);
        let meta = v.get("components").and_then(Value::as_object);
        // Group by component key (falling back to the REST id), in first-use order.
        let mut order: Vec<String> = Vec::new();
        let mut groups: BTreeMap<String, (String, Vec<NodeId>)> = BTreeMap::new();
        for (inst, cid) in unresolved {
            let info = meta.and_then(|m| m.get(&cid));
            let key = info.and_then(|c| fstr(c, "key")).map(String::from).unwrap_or_else(|| cid.clone());
            let name = info.and_then(|c| fstr(c, "name")).map(String::from).or_else(|| self.doc.get(inst).map(|n| n.name.clone())).unwrap_or_default();
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_insert_with(|| (name, Vec::new())).1.push(inst);
        }
        for key in order {
            let (name, insts) = &groups[&key];
            for i in insts {
                if let Some(NodeKind::Instance { component_key, .. }) = self.doc.get_mut(*i).map(|n| &mut n.kind) {
                    component_key.get_or_insert_with(|| key.clone());
                }
            }
            desigma_doc::link_remote_component(&mut self.doc, &library_id, &key, name, insts);
        }
    }

    /// Maps collected prototype data (REST ids) onto the imported nodes.
    fn resolve_proto(&mut self) {
        let res = crate::proto::Resolver { ids: &self.ids, overlay: &self.overlay_props };
        let mut reactions = Vec::new();
        for (id, raw) in std::mem::take(&mut self.proto) {
            reactions.push((id, res.reactions(&raw)));
        }
        let mut flows = Vec::new();
        for (page, canvas) in std::mem::take(&mut self.flow_pages) {
            flows.extend(res.flows(page, &canvas));
        }
        for (id, r) in reactions {
            if let Some(n) = self.doc.get_mut(id) {
                n.prototype.reactions = r;
            }
        }
        self.doc.flows.extend(flows);
    }

    /// Sets `ComponentLink::source` on the children REST already inlined under
    /// instances (matching `I<instance>;<id>` ids to the main component's
    /// nodes) and on every instance root.
    fn link_sources(&mut self, targets: &BTreeMap<NodeId, NodeId>) {
        for inst in targets.keys() {
            let mut stack: Vec<NodeId> = self.doc.get(*inst).map(|n| n.children.clone()).unwrap_or_default();
            while let Some(d) = stack.pop() {
                let Some(n) = self.doc.get(d) else { continue };
                let kids = n.children.clone();
                let src = n.source_id.as_deref().and_then(counterpart_key).and_then(|k| self.ids.get(&k).copied()).filter(|s| *s != d);
                if let (Some(s), Some(m)) = (src, self.doc.get_mut(d)) {
                    m.component.source = Some(s);
                }
                stack.extend(kids);
            }
        }
        for (inst, comp) in targets {
            if let Some(n) = self.doc.get_mut(*inst) {
                if n.component.source.is_none() {
                    n.component.source = Some(*comp);
                }
            }
        }
    }

    fn resolve_swaps(&mut self) {
        for (node, name, rest_id, is_def) in std::mem::take(&mut self.pending_swaps) {
            let target = self.ids.get(&rest_id).copied().unwrap_or(NodeId(0));
            let Some(n) = self.doc.get_mut(node) else { continue };
            if is_def {
                if let Some(d) = n.component.props.iter_mut().find(|d| d.name == name) {
                    d.default = PropValue::Instance(target);
                }
            } else if target.0 == 0 {
                n.component.values.remove(&name);
            } else {
                n.component.values.insert(name, PropValue::Instance(target));
            }
        }
    }

    /// REST `overrides[].overriddenFields` -> `ComponentLink::overrides`.
    fn apply_rest_overrides(&mut self) {
        for (_inst, nid, fields) in std::mem::take(&mut self.rest_overrides) {
            let Some(&id) = self.ids.get(&nid) else { continue };
            let Some(n) = self.doc.get_mut(id) else { continue };
            let is_text = matches!(n.kind, NodeKind::Text(_));
            for f in &fields {
                if let Some(of) = override_field(f, is_text) {
                    n.component.overrides.insert(of);
                }
            }
        }
    }

    fn fresh_id(&mut self) -> NodeId {
        let id = NodeId(self.next_clone);
        self.next_clone += 1;
        self.doc.reserve_ids(id);
        id
    }

    /// Expands `inst`, first expanding instances nested inside its component so
    /// their (overridden) children are copied too.
    fn expand_rec(&mut self, inst: NodeId, targets: &BTreeMap<NodeId, NodeId>, done: &mut BTreeSet<NodeId>) {
        if !done.insert(inst) {
            return;
        }
        let Some(&comp) = targets.get(&inst) else { return };
        self.expand_nested(comp, targets, done);
        self.expand_instance(inst, comp);
        if let Some(s) = self.scales.get(&inst).copied() {
            self.scale_contents(inst, s);
        }
        // Property values first: an instance swap changes which nodes the overrides address.
        self.apply_props(inst, targets, done);
        self.apply_overrides(inst, targets, done);
    }

    /// Scales the copied children of an instance made with the scale tool
    /// (`uniformScaleFactor`): positions, sizes, radii, strokes, text sizes,
    /// layout spacing and geometry. Overrides and derived data are applied after.
    fn scale_contents(&mut self, inst: NodeId, s: f32) {
        fn scale_path(p: &mut Path, s: f32) {
            let f = |v: &mut Vec2| *v = Vec2::new(v.x * s, v.y * s);
            for c in &mut p.cmds {
                match c {
                    PathCmd::MoveTo(a) | PathCmd::LineTo(a) => f(a),
                    PathCmd::QuadTo(a, b) => {
                        f(a);
                        f(b);
                    }
                    PathCmd::CubicTo(a, b, c) => {
                        f(a);
                        f(b);
                        f(c);
                    }
                    PathCmd::Close => {}
                }
            }
        }
        let mut stack: Vec<NodeId> = self.doc.get(inst).map(|n| n.children.clone()).unwrap_or_default();
        while let Some(d) = stack.pop() {
            let Some(n) = self.doc.get_mut(d) else { continue };
            stack.extend(n.children.iter().copied());
            n.transform.tx *= s;
            n.transform.ty *= s;
            n.size = Vec2::new(n.size.x * s, n.size.y * s);
            for r in &mut n.corner_radii {
                *r *= s;
            }
            n.stroke.weight *= s;
            for g in n.fill_geometry.iter_mut().chain(n.stroke_geometry.iter_mut()).flatten() {
                scale_path(g, s);
            }
            match &mut n.kind {
                NodeKind::Text(t) => {
                    t.style.font_size *= s;
                    for r in &mut t.runs {
                        r.style.font_size *= s;
                    }
                }
                NodeKind::Vector { paths, network } => {
                    paths.iter_mut().for_each(|p| scale_path(p, s));
                    if let Some(net) = network {
                        *net = net.transformed(&Affine::scale(s, s));
                    }
                }
                k => {
                    if let Some(al) = k.frame_props_mut().and_then(|f| f.auto_layout.as_mut()) {
                        al.item_spacing *= s;
                        al.counter_spacing *= s;
                        al.padding.iter_mut().for_each(|p| *p *= s);
                    }
                }
            }
        }
    }

    /// Expands the instances inside `comp` (innermost first).
    fn expand_nested(&mut self, comp: NodeId, targets: &BTreeMap<NodeId, NodeId>, done: &mut BTreeSet<NodeId>) {
        let mut stack = vec![comp];
        let mut nested = Vec::new();
        while let Some(n) = stack.pop() {
            if let Some(node) = self.doc.get(n) {
                if n != comp && targets.contains_key(&n) {
                    nested.push(n);
                }
                stack.extend(node.children.iter().copied());
            }
        }
        nested.sort();
        for n in nested {
            self.expand_rec(n, targets, done);
        }
    }

    /// Applies the instance's component property values to the nodes of its
    /// copy that are bound to them: visibility, text, and instance swaps.
    fn apply_props(&mut self, inst: NodeId, targets: &BTreeMap<NodeId, NodeId>, done: &mut BTreeSet<NodeId>) {
        let values = match self.doc.get(inst) {
            Some(n) if !n.component.values.is_empty() => n.component.values.clone(),
            _ => return,
        };
        let mut stack: Vec<NodeId> = self.doc.get(inst).map(|n| n.children.clone()).unwrap_or_default();
        while let Some(d) = stack.pop() {
            let Some(n) = self.doc.get(d) else { continue };
            stack.extend(n.children.iter().copied());
            let bindings: Vec<(OverrideField, String)> = n.component.bindings.iter().map(|(f, p)| (*f, p.clone())).collect();
            for (field, prop) in bindings {
                match (field, values.get(&prop)) {
                    (OverrideField::Visible, Some(PropValue::Bool(b))) => {
                        if let Some(n) = self.doc.get_mut(d) {
                            n.visible = *b;
                        }
                    }
                    (OverrideField::Characters, Some(PropValue::Text(s))) => {
                        if let Some(NodeKind::Text(t)) = self.doc.get_mut(d).map(|n| &mut n.kind) {
                            t.content = s.clone();
                            t.runs.clear();
                            t.lists.clear();
                        }
                    }
                    (OverrideField::MainComponent, Some(PropValue::Instance(target))) => self.swap_nested(d, *target, targets, done),
                    _ => {}
                }
            }
        }
    }

    /// Points the nested instance `d` at `target` and rebuilds its children.
    fn swap_nested(&mut self, d: NodeId, target: NodeId, targets: &BTreeMap<NodeId, NodeId>, done: &mut BTreeSet<NodeId>) {
        let current = match self.doc.get(d).map(|n| &n.kind) {
            Some(NodeKind::Instance { component, .. }) => *component,
            _ => return,
        };
        if current == Some(target) || !matches!(self.doc.get(target).map(|n| &n.kind), Some(NodeKind::Component(_))) {
            return;
        }
        self.expand_nested(target, targets, done);
        for k in self.doc.get(d).map(|n| n.children.clone()).unwrap_or_default() {
            let _ = self.doc.remove(k);
        }
        // Like Figma, a layer still named after its component takes the new
        // component's name; a renamed one keeps its own. Name overrides are
        // applied after the swap.
        let rename = match (current.and_then(|c| self.display_name(c)), self.display_name(target)) {
            (Some(old), Some(new)) if self.doc.get(d).is_some_and(|n| n.name == old) => Some(new),
            _ => None,
        };
        if let Some(n) = self.doc.get_mut(d) {
            if let NodeKind::Instance { component, component_key, .. } = &mut n.kind {
                *component = Some(target);
                *component_key = None;
            }
            if let Some(name) = rename {
                n.name = name;
            }
        }
        self.expand_instance(d, target);
        // The fresh children are at 1x; a slot made with the scale tool keeps its scale.
        if let Some(s) = self.scale_of(d) {
            self.scale_contents(d, s);
        }
    }

    /// `uniformScaleFactor` of `id`, or of the node an instance copy was made from.
    fn scale_of(&self, mut id: NodeId) -> Option<f32> {
        for _ in 0..MAX_DEPTH {
            if let Some(s) = self.scales.get(&id) {
                return Some(*s);
            }
            id = self.doc.get(id)?.component.source?;
        }
        None
    }

    /// Name an instance of `comp` gets by default: the component set's name
    /// for a variant, else the component's own.
    fn display_name(&self, comp: NodeId) -> Option<String> {
        let c = self.doc.get(comp)?;
        let set = c.parent.and_then(|p| self.doc.get(p)).filter(|p| matches!(p.kind, NodeKind::ComponentSet(_)));
        Some(set.unwrap_or(c).name.clone())
    }

    /// `.fig` library components of internal-only canvases become external
    /// assets of placeholder libraries (one per `sourceLibraryKey`), and the
    /// instances using them learn their component key.
    fn register_library_assets(&mut self) {
        let Some(lp) = self.doc.library_page else { return };
        let keys = std::mem::take(&mut self.library_keys);
        for (node, (key, lib)) in &keys {
            let Some(n) = self.doc.get(*node) else { continue };
            let in_set = n.parent.and_then(|p| self.doc.get(p)).is_some_and(|p| matches!(p.kind, NodeKind::ComponentSet(_)));
            if in_set || !self.doc.is_ancestor(lp, *node) {
                continue;
            }
            let kind = if matches!(n.kind, NodeKind::ComponentSet(_)) { LibraryAssetKind::ComponentSet } else { LibraryAssetKind::Component };
            let library_id = format!("{}{}", REMOTE_LIBRARY_PREFIX, lib.as_deref().unwrap_or("file"));
            self.doc.library.external.insert(local_key(&library_id, key), ExternalAsset { library_id, version: 0, kind, node: Some(*node), local_key: None });
        }
        let insts: Vec<NodeId> = self.doc.nodes.values().filter(|n| matches!(n.kind, NodeKind::Instance { .. })).map(|n| n.id).collect();
        for i in insts {
            let comp = match self.doc.get(i).map(|n| &n.kind) {
                Some(NodeKind::Instance { component: Some(c), .. }) => *c,
                _ => continue,
            };
            if let Some((key, _)) = keys.get(&comp) {
                if let Some(NodeKind::Instance { component_key, .. }) = self.doc.get_mut(i).map(|n| &mut n.kind) {
                    component_key.get_or_insert_with(|| key.clone());
                }
            }
        }
        self.library_keys = keys;
    }

    /// Deep-copies the component's children under an empty instance.
    fn expand_instance(&mut self, inst: NodeId, comp: NodeId) {
        if !self.doc.get(inst).map(|n| n.children.is_empty()).unwrap_or(false) || self.doc.is_ancestor(comp, inst) || comp == inst {
            return;
        }
        let prefix = self.rest_key(inst).unwrap_or_default();
        let kids = self.doc.get(comp).map(|n| n.children.clone()).unwrap_or_default();
        let mut budget = 50_000usize;
        for k in kids {
            self.clone_subtree(k, inst, &prefix, &mut budget, 0);
        }
    }

    /// The id a node contributes to instance-copy ids. A node of a library
    /// component copy is addressed by its library-side id (`.fig`
    /// overrideKey), as Figma's REST ids are; a copy already carries its full
    /// `I...` path.
    fn rest_key(&self, node: NodeId) -> Option<String> {
        let sid = self.doc.get(node)?.source_id.clone();
        match sid {
            Some(s) if s.starts_with('I') => Some(s),
            other => self.override_keys.get(&node).cloned().or(other),
        }
    }

    fn clone_subtree(&mut self, src: NodeId, parent: NodeId, prefix: &str, budget: &mut usize, depth: usize) {
        if *budget == 0 || depth > MAX_DEPTH {
            return;
        }
        *budget -= 1;
        let Some(orig) = self.doc.get(src).cloned() else { return };
        let id = self.fresh_id();
        let mut n = orig.clone();
        n.id = id;
        n.children.clear();
        n.parent = None;
        n.source_id = self.rest_key(src).map(|s| instance_copy_id(prefix, &s));
        n.component.source = Some(src);
        n.component.overrides.clear();
        n.component.props.clear();
        let container = n.kind.is_container();
        if let Some(k) = self.override_keys.get(&src).cloned() {
            self.override_keys.insert(id, k);
        }
        if self.doc.insert(parent, n).is_err() {
            return;
        }
        if container {
            for c in orig.children {
                self.clone_subtree(c, id, prefix, budget, depth + 1);
            }
        }
    }

    /// Applies `.fig` `symbolOverrides` and then `derivedSymbolData` to the
    /// instance's copied subtree.
    fn apply_overrides(&mut self, inst: NodeId, targets: &BTreeMap<NodeId, NodeId>, done: &mut BTreeSet<NodeId>) {
        let Some(ov) = self.overrides.remove(&inst) else { return };
        // Nested instance swaps (`overriddenSymbolID`) first: they change which
        // nodes the deeper overrides address.
        for e in &ov.set {
            let Some(cid) = e.set.get("componentId").and_then(Value::as_str) else { continue };
            let (Some(t), Some(&target)) = (self.resolve_path(inst, &e.path), self.ids.get(cid)) else { continue };
            self.swap_nested(t, target, targets, done);
        }
        for e in &ov.set {
            if let Some(t) = self.resolve_path(inst, &e.path) {
                self.apply_override(t, &e.set, false);
                self.apply_override_bindings(t, &e.set);
            }
        }
        // Figma's computed results for resized instances win over the raw overrides.
        for e in &ov.derived {
            if let Some(t) = self.resolve_path(inst, &e.path) {
                self.apply_override(t, &e.set, true);
            }
        }
    }

    /// Variable bindings carried by an override (`boundVariables` from the
    /// overridden paints' `colorVar`): rebinds the copied node and writes the
    /// variable's resolved value into the bound property, since the literal
    /// colour stored next to an alias in `.fig` overrides can be stale.
    fn apply_override_bindings(&mut self, id: NodeId, set: &Value) {
        if set.get("boundVariables").is_none() {
            return;
        }
        let nv = crate::variables::parse_node_variables(set);
        for (key, var) in nv.bindings {
            let value = self.doc.resolve_variable(id, &var);
            let Some(n) = self.doc.get_mut(id) else { return };
            n.variables.bindings.insert(key.clone(), var);
            if let Some(v) = value {
                set_bound_property(n, &key, &v);
            }
        }
    }

    /// Walks a `guidPath` (ids of the nodes inside the component, outermost
    /// first) down from the instance. Clone source ids look like
    /// `I<instance>;<original>`; only the part after the last `;` identifies the
    /// original node.
    fn resolve_path(&self, inst: NodeId, path: &[String]) -> Option<NodeId> {
        // `.fig` override paths use override keys; REST-derived ids use the tail of the source id.
        let matches = |c: NodeId, g: &String| -> bool {
            if let Some(k) = self.override_keys.get(&c) {
                return k == g;
            }
            self.doc.get(c).is_some_and(|n| {
                let s = n.source_id.as_deref().unwrap_or("");
                s.rsplit(';').next().unwrap_or(s) == g
            })
        };
        let mut cur = inst;
        for g in path {
            let node = self.doc.get(cur)?;
            let direct = node.children.iter().copied().find(|c| matches(*c, g));
            cur = match direct {
                Some(c) => c,
                None => {
                    // Tolerate paths that skip intermediate containers.
                    let mut stack: Vec<NodeId> = node.children.iter().rev().copied().collect();
                    let mut found = None;
                    while let Some(n) = stack.pop() {
                        let Some(nn) = self.doc.get(n) else { continue };
                        if matches(n, g) {
                            found = Some(n);
                            break;
                        }
                        stack.extend(nn.children.iter().rev().copied());
                    }
                    found?
                }
            };
        }
        Some(cur)
    }

    /// Applies partial REST node fields to an existing node. `derived_only`
    /// restricts to size, transform and geometry.
    fn apply_override(&mut self, id: NodeId, set: &Value, derived_only: bool) {
        let Some(n) = self.doc.get_mut(id) else { return };
        let keep = |k: &str| !derived_only || matches!(k, "size" | "relativeTransform" | "fillGeometry" | "strokeGeometry");
        let has = |k: &str| keep(k) && set.get(k).is_some();
        if has("size") {
            if let Some(sz) = set.get("size").and_then(vec2) {
                n.size = sz;
            }
        }
        if has("relativeTransform") {
            if let Some(t) = set.get("relativeTransform").and_then(parse_transform) {
                n.transform = t;
            }
        }
        if has("fillGeometry") {
            let g = parse_geometry(farr(set, "fillGeometry"));
            if let NodeKind::Vector { paths, network } = &mut n.kind {
                *paths = g.clone();
                // the new outline replaces what the network described
                *network = None;
            }
            n.fill_geometry = Some(g).filter(|g| !g.is_empty());
        }
        if has("strokeGeometry") {
            n.stroke_geometry = Some(parse_geometry(farr(set, "strokeGeometry"))).filter(|g| !g.is_empty());
        }
        if derived_only {
            return;
        }
        let is_text = matches!(n.kind, NodeKind::Text(_));
        if let Some(obj) = set.as_object() {
            for k in obj.keys() {
                if let Some(of) = override_field(k, is_text) {
                    n.component.overrides.insert(of);
                }
            }
        }
        if let Some(s) = fstr(set, "name") {
            n.name = s.to_string();
        }
        if let Some(b) = fbool(set, "visible") {
            n.visible = b;
        }
        if let Some(b) = fbool(set, "locked") {
            n.locked = b;
        }
        if let Some(o) = fnum(set, "opacity") {
            n.opacity = o;
        }
        if let Some(b) = fstr(set, "blendMode") {
            n.blend = blend(b);
        }
        if let Some(b) = fbool(set, "isMask") {
            n.is_mask = b;
        }
        if set.get("fills").is_some() {
            let fills = parse_paints(set.get("fills"));
            match &mut n.kind {
                NodeKind::Text(t) => t.style.fills = fills,
                _ => n.fills = fills,
            }
        }
        if set.get("strokes").is_some() {
            n.stroke.paints = parse_paints(set.get("strokes"));
        }
        if let Some(w) = fnum(set, "strokeWeight") {
            n.stroke.weight = w;
        }
        if let Some(a) = fstr(set, "strokeAlign") {
            n.stroke.align = match a {
                "CENTER" => StrokeAlign::Center,
                "OUTSIDE" => StrokeAlign::Outside,
                _ => StrokeAlign::Inside,
            };
        }
        if set.get("effects").is_some() {
            n.effects = farr(set, "effects").iter().filter_map(parse_effect).collect();
        }
        if let Some(r) = set.get("rectangleCornerRadii").and_then(Value::as_array).filter(|a| a.len() == 4) {
            for (i, x) in r.iter().enumerate() {
                n.corner_radii[i] = num(x).unwrap_or(0.0);
            }
        } else if let Some(r) = fnum(set, "cornerRadius") {
            n.corner_radii = [r; 4];
        }
        if let NodeKind::Text(t) = &mut n.kind {
            if let Some(st) = set.get("style") {
                apply_type_style(&mut t.style, st);
                crate::textfmt::apply_paragraph(t, st);
                if let Some(a) = fstr(st, "textAutoResize") {
                    t.auto_resize = auto_resize(a);
                }
                if fstr(st, "textTruncation") == Some("ENDING") {
                    t.auto_resize = TextAutoResize::Truncate;
                }
                if let Some(m) = st.get("maxLines").and_then(Value::as_u64) {
                    t.max_lines = Some(m.min(u32::MAX as u64) as u32).filter(|m| *m > 0);
                }
            }
            if let Some(c) = fstr(set, "characters") {
                if c != t.content {
                    t.runs.clear(); // byte ranges no longer apply
                }
                t.content = c.to_string();
                t.lists.clear();
            }
            if set.get("lineTypes").is_some() {
                crate::textfmt::apply_lines(t, set);
            }
            if set.get("characterStyleOverrides").is_some() {
                let ids: Vec<u64> = farr(set, "characterStyleOverrides").iter().map(|x| x.as_u64().unwrap_or(0)).collect();
                t.runs = build_runs(&t.content, &t.style, &ids, set.get("styleOverrideTable"));
            }
        }
    }
}

/// Library style metadata from the top-level REST `styles` map. A FILL style
/// that is only ever used as a stroke gets [`StyleProperty::Stroke`].
fn style_meta(styles: Option<&Value>, doc: &Document) -> BTreeMap<String, StyleMeta> {
    let mut out = BTreeMap::new();
    let Some(map) = styles.and_then(Value::as_object) else { return out };
    let mut used_as_fill: BTreeSet<&str> = BTreeSet::new();
    let mut used_as_stroke: BTreeSet<&str> = BTreeSet::new();
    for n in doc.nodes.values() {
        for (k, sid) in &n.style_refs {
            match k.as_str() {
                "stroke" => used_as_stroke.insert(sid),
                "fill" => used_as_fill.insert(sid),
                _ => continue,
            };
        }
    }
    for (id, m) in map {
        if !doc.styles.contains_key(id) {
            continue;
        }
        let property = match fstr(m, "styleType") {
            Some("TEXT") => StyleProperty::Text,
            Some("EFFECT") => StyleProperty::Effect,
            Some("GRID") => StyleProperty::Grid,
            _ if used_as_stroke.contains(id.as_str()) && !used_as_fill.contains(id.as_str()) => StyleProperty::Stroke,
            _ => StyleProperty::Fill,
        };
        out.insert(
            id.clone(),
            StyleMeta {
                name: fstr(m, "name").unwrap_or("").to_string(),
                key: fstr(m, "key").map(String::from).filter(|k| !k.is_empty()),
                description: fstr(m, "description").unwrap_or("").to_string(),
                property,
            },
        );
    }
    out
}

/// Property name without Figma's `#<id>` suffix.
fn prop_name(key: &str) -> String {
    key.split('#').next().unwrap_or(key).to_string()
}

/// REST id of `src` copied into the instance `inst`: one `I` and the flat
/// guid path, as Figma writes it (`I<inst>;<a>;<b>`), also when `inst` or
/// `src` is itself a copy inside another instance.
fn instance_copy_id(inst: &str, src: &str) -> String {
    let inst = inst.strip_prefix('I').unwrap_or(inst);
    let src = src.strip_prefix('I').unwrap_or(src);
    format!("I{inst};{src}")
}

/// For an id inside an instance copy (`I<inst>;<a>;<b>`), the id of the node
/// of the main component it mirrors (`<a>`, or `I<a>;<b>` when nested).
fn counterpart_key(sid: &str) -> Option<String> {
    let rest = sid.strip_prefix('I')?;
    let mut segs = rest.split(';');
    segs.next()?;
    let tail: Vec<&str> = segs.collect();
    match tail.len() {
        0 => None,
        1 => Some(tail[0].to_string()),
        _ => Some(format!("I{}", tail.join(";"))),
    }
}

/// Maps a REST field name (as in `overriddenFields` or an override entry) to
/// the document-model override category.
pub(crate) fn override_field(key: &str, is_text: bool) -> Option<OverrideField> {
    Some(match key {
        "name" => OverrideField::Name,
        "visible" => OverrideField::Visible,
        "locked" => OverrideField::Locked,
        "opacity" => OverrideField::Opacity,
        "blendMode" => OverrideField::Blend,
        "size" => OverrideField::Size,
        "relativeTransform" | "transform" => OverrideField::Transform,
        "fills" if is_text => OverrideField::TextStyle,
        "fills" => OverrideField::Fills,
        "strokes" | "strokeWeight" | "strokeAlign" | "strokeCap" | "strokeJoin" | "strokeDashes" | "individualStrokeWeights" => OverrideField::Stroke,
        "effects" => OverrideField::Effects,
        "cornerRadius" | "rectangleCornerRadii" | "cornerSmoothing" => OverrideField::CornerRadii,
        "characters" => OverrideField::Characters,
        "style"
        | "characterStyleOverrides"
        | "styleOverrideTable"
        | "fontSize"
        | "fontName"
        | "fontFamily"
        | "textAutoResize"
        | "textTruncation"
        | "lineTypes"
        | "lineIndentations"
        | "opentypeFlags"
        | "maxLines" => OverrideField::TextStyle,
        "layoutMode"
        | "itemSpacing"
        | "counterAxisSpacing"
        | "paddingLeft"
        | "paddingRight"
        | "paddingTop"
        | "paddingBottom"
        | "primaryAxisAlignItems"
        | "counterAxisAlignItems"
        | "primaryAxisSizingMode"
        | "counterAxisSizingMode"
        | "layoutWrap"
        | "clipsContent"
        | "layoutGrids"
        | "layoutAlign"
        | "layoutGrow"
        | "layoutPositioning"
        | "layoutSizingHorizontal"
        | "layoutSizingVertical" => OverrideField::Layout,
        "componentId" | "mainComponent" | "overrides" => OverrideField::MainComponent,
        _ => return None,
    })
}

fn abs_origin(v: &Value) -> Option<Vec2> {
    let b = v.get("absoluteBoundingBox")?;
    Some(Vec2::new(fnum(b, "x")?, fnum(b, "y")?))
}

fn parse_transform(v: &Value) -> Option<Affine> {
    let r = v.as_array()?;
    let row = |i: usize| -> Option<[f32; 3]> {
        let a = r.get(i)?.as_array()?;
        Some([num(a.first()?)?, num(a.get(1)?)?, num(a.get(2)?)?])
    };
    let (r0, r1) = (row(0)?, row(1)?);
    Some(Affine { a: r0[0], c: r0[1], tx: r0[2], b: r1[0], d: r1[1], ty: r1[2] })
}

fn parse_geometry(items: &[Value]) -> Vec<Path> {
    items
        .iter()
        .filter_map(|g| {
            let d = fstr(g, "path")?;
            let rule = match fstr(g, "windingRule") {
                Some("EVENODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            Some(parse_path(d, rule))
        })
        .collect()
}

fn frame_props(v: &Value) -> FrameProps {
    let dir = match fstr(v, "layoutMode") {
        Some("HORIZONTAL") => Some(LayoutDirection::Horizontal),
        Some("VERTICAL") => Some(LayoutDirection::Vertical),
        _ => None,
    };
    let auto_layout = dir.map(|direction| {
        let align = |s: Option<&str>| match s {
            Some("CENTER") => LayoutAlign::Center,
            Some("MAX") => LayoutAlign::End,
            Some("SPACE_BETWEEN") => LayoutAlign::SpaceBetween,
            Some("BASELINE") => LayoutAlign::Baseline,
            _ => LayoutAlign::Start,
        };
        let sizing = |s: Option<&str>| if s == Some("AUTO") { Sizing::Hug } else { Sizing::Fixed };
        let p = |k: &str| fnum(v, k).unwrap_or(0.0);
        AutoLayout {
            direction,
            wrap: fstr(v, "layoutWrap") == Some("WRAP"),
            item_spacing: p("itemSpacing"),
            counter_spacing: p("counterAxisSpacing"),
            padding: [p("paddingTop"), p("paddingRight"), p("paddingBottom"), p("paddingLeft")],
            primary_align: align(fstr(v, "primaryAxisAlignItems")),
            counter_align: align(fstr(v, "counterAxisAlignItems")),
            primary_sizing: sizing(fstr(v, "primaryAxisSizingMode")),
            counter_sizing: sizing(fstr(v, "counterAxisSizingMode")),
            stroke_included: fbool(v, "strokesIncludedInLayout").unwrap_or(false),
            reverse_z: fbool(v, "itemReverseZIndex").unwrap_or(false),
        }
    });
    FrameProps { clips_content: fbool(v, "clipsContent").unwrap_or(false), auto_layout, grids: farr(v, "layoutGrids").iter().filter_map(parse_grid).collect() }
}

fn text_props(v: &Value) -> TextProps {
    let content = fstr(v, "characters").unwrap_or("").to_string();
    let mut style = TextStyle::default();
    let st = v.get("style");
    if let Some(st) = st {
        apply_type_style(&mut style, st);
    }
    if v.get("fills").is_some() {
        style.fills = parse_paints(v.get("fills"));
    }
    let mut auto = fstr(v, "textAutoResize").or_else(|| st.and_then(|s| fstr(s, "textAutoResize"))).map(auto_resize).unwrap_or(TextAutoResize::None);
    if fstr(v, "textTruncation").or_else(|| st.and_then(|s| fstr(s, "textTruncation"))) == Some("ENDING") {
        auto = TextAutoResize::Truncate;
    }
    let max_lines =
        v.get("maxLines").or_else(|| st.and_then(|s| s.get("maxLines"))).and_then(Value::as_u64).filter(|n| *n > 0).map(|n| n.min(u32::MAX as u64) as u32);

    let overrides: Vec<u64> = farr(v, "characterStyleOverrides").iter().map(|x| x.as_u64().unwrap_or(0)).collect();
    let runs = build_runs(&content, &style, &overrides, v.get("styleOverrideTable"));
    let mut tp = TextProps { content, style, runs, auto_resize: auto, max_lines, ..Default::default() };
    if let Some(st) = st {
        crate::textfmt::apply_paragraph(&mut tp, st);
    }
    crate::textfmt::apply_lines(&mut tp, v);
    tp
}

/// Style runs over byte ranges from per-UTF-16-unit override ids.
fn build_runs(content: &str, base: &TextStyle, overrides: &[u64], table: Option<&Value>) -> Vec<TextRun> {
    let mut runs = Vec::new();
    if overrides.iter().any(|&x| x != 0) {
        let offs = utf16_to_byte_offsets(content);
        let units = offs.len() - 1;
        let mut i = 0;
        while i < overrides.len().min(units) {
            let sid = overrides[i];
            let mut j = i + 1;
            while j < overrides.len().min(units) && overrides[j] == sid {
                j += 1;
            }
            if sid != 0 {
                if let Some(o) = table.and_then(|t| t.get(sid.to_string())) {
                    let mut rs = base.clone();
                    apply_type_style(&mut rs, o);
                    let (s, e) = (offs[i], offs[j]);
                    if e > s {
                        runs.push(TextRun { start: s, end: e, style: rs });
                    }
                }
            }
            i = j;
        }
    }
    runs
}

fn layout_child(v: &Value, parent: Option<LayoutDirection>, kind: &NodeKind) -> LayoutChild {
    let sizing = |s: Option<&str>| match s {
        Some("HUG") => Some(Sizing::Hug),
        Some("FILL") => Some(Sizing::Fill),
        Some("FIXED") => Some(Sizing::Fixed),
        _ => None,
    };
    let grow = fnum(v, "layoutGrow").unwrap_or(0.0);
    let mut lc = LayoutChild {
        absolute: fstr(v, "layoutPositioning") == Some("ABSOLUTE"),
        grow,
        min_w: fnum(v, "minWidth"),
        max_w: fnum(v, "maxWidth"),
        min_h: fnum(v, "minHeight"),
        max_h: fnum(v, "maxHeight"),
        ..Default::default()
    };
    let (sh, sv) = (sizing(fstr(v, "layoutSizingHorizontal")), sizing(fstr(v, "layoutSizingVertical")));
    // Derive from the legacy fields when the explicit ones are absent.
    let mut h = Sizing::Fixed;
    let mut vt = Sizing::Fixed;
    let own_hug = |dir_h: bool| -> bool {
        match kind {
            NodeKind::Text(t) => match t.auto_resize {
                TextAutoResize::WidthAndHeight => true,
                TextAutoResize::Height => !dir_h,
                _ => false,
            },
            k => match k.frame_props().and_then(|f| f.auto_layout) {
                Some(al) => {
                    let along = (al.direction == LayoutDirection::Horizontal) == dir_h;
                    (if along { al.primary_sizing } else { al.counter_sizing }) == Sizing::Hug
                }
                None => false,
            },
        }
    };
    if own_hug(true) {
        h = Sizing::Hug;
    }
    if own_hug(false) {
        vt = Sizing::Hug;
    }
    if let Some(dir) = parent {
        let stretch = fstr(v, "layoutAlign") == Some("STRETCH");
        let (prim_fill, counter_fill) = (grow > 0.0, stretch);
        match dir {
            LayoutDirection::Horizontal => {
                if prim_fill {
                    h = Sizing::Fill;
                }
                if counter_fill {
                    vt = Sizing::Fill;
                }
            }
            LayoutDirection::Vertical => {
                if prim_fill {
                    vt = Sizing::Fill;
                }
                if counter_fill {
                    h = Sizing::Fill;
                }
            }
        }
    }
    lc.horizontal = sh.unwrap_or(h);
    lc.vertical = sv.unwrap_or(vt);
    lc
}

/// Every `imageRef` used by fills, strokes and text styles, sorted and deduplicated.
pub fn image_refs(doc: &Document) -> Vec<String> {
    let mut set = BTreeSet::new();
    let mut add = |ps: &[Paint]| {
        for p in ps {
            if let PaintKind::Image { image_ref, .. } = &p.kind {
                set.insert(image_ref.clone());
            }
        }
    };
    for n in doc.nodes.values() {
        add(&n.fills);
        add(&n.stroke.paints);
        if let NodeKind::Text(t) = &n.kind {
            add(&t.style.fills);
            for r in &t.runs {
                add(&r.style.fills);
            }
        }
    }
    for s in doc.styles.values() {
        match s {
            Style::Fill(p) => add(p),
            Style::Text(t) => add(&t.fills),
            _ => {}
        }
    }
    set.into_iter().collect()
}
