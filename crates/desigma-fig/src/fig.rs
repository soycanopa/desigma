//! `.fig` (fig-kiwi) import.
//!
//! Pipeline: container -> kiwi schema + message -> REST-shaped JSON tree ->
//! the same mapper as the REST importer. Field names follow community
//! reverse engineering of Figma's schema; everything is optional and unknown
//! fields are ignored.

use crate::container::{self, Archive};
use crate::kiwi::{self, KiwiValue, Schema};
use crate::rest_import::{self, fnum, fstr};
use crate::svgpath::write_path;
use crate::FigError;
use desigma_doc::{Affine, Document, FillRule, Path, PathCmd, Vec2};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

struct Decoded {
    archive: Archive,
    message: KiwiValue,
}

fn decode(bytes: &[u8]) -> Result<Decoded, FigError> {
    let archive = container::open(bytes)?;
    let (schema_bin, msg_bin) = container::read_chunks(&archive.canvas)?;
    let schema = Schema::decode(&schema_bin).map_err(|e| FigError::Format(format!("kiwi schema: {e}")))?;
    let root = schema.root_message().ok_or_else(|| FigError::Format("schema has no message definition".into()))?;
    let message = schema.decode_message(root, &msg_bin).map_err(|e| FigError::Format(format!("kiwi message: {e}")))?;
    Ok(Decoded { archive, message })
}

/// Raw kiwi message as JSON (enums by name, byte arrays as hex) for debugging.
pub fn decode_fig_to_json(bytes: &[u8]) -> Result<Value, FigError> {
    let d = decode(bytes)?;
    let mut out = Map::new();
    out.insert("message".into(), kiwi::to_json(&d.message));
    if let Some(m) = d.archive.meta {
        out.insert("meta".into(), m);
    }
    out.insert("images".into(), Value::Array(d.archive.images.keys().map(|k| Value::String(k.clone())).collect()));
    Ok(Value::Object(out))
}

/// Imports a `.fig` file (raw fig-kiwi or zip-wrapped) from bytes.
pub fn import_fig(bytes: &[u8]) -> Result<Document, FigError> {
    let d = decode(bytes)?;
    let name = d.archive.meta.as_ref().and_then(|m| fstr(m, "file_name").or_else(|| fstr(m, "name"))).unwrap_or("Untitled").to_string();
    let rest = message_to_rest(&d.message, &name);
    let mut doc = rest_import::import_rest_value(&rest, true)?;
    doc.images = d.archive.images;
    Ok(doc)
}

// ------------------------------------------------------------------ tree

struct Entry {
    id: String,
    parent: Option<String>,
    pos: String,
    json: Value,
}

pub(crate) fn guid_str(g: &Value) -> Option<String> {
    Some(format!("{}:{}", g.get("sessionID")?.as_u64()?, g.get("localID")?.as_u64()?))
}

/// Library asset key of an `{assetRef: {key, version}}` reference.
pub(crate) fn asset_key(v: &Value) -> Option<&str> {
    v.get("assetRef").and_then(|a| fstr(a, "key"))
}

/// Cross-node lookups of a message: blobs, library keys and property
/// definitions. Library assets (styles, variables, variable sets) are
/// referenced by `{assetRef: {key}}`; files that define them locally use a
/// plain `{guid}` instead.
struct Lk<'a> {
    blobs: &'a [&'a [u8]],
    /// Variable key -> `VariableID:s:l`.
    var_by_key: HashMap<&'a str, String>,
    /// Variable-set key -> `VariableCollectionId:s:l`.
    set_by_key: HashMap<&'a str, String>,
    /// Style key -> `s:l` of the style node.
    style_by_key: HashMap<&'a str, String>,
    /// Style key -> its node (for paints the style supplies).
    style_nodes: HashMap<&'a str, &'a Value>,
    /// Style node id (`s:l`) -> node, for styles defined in this file.
    style_by_id: HashMap<&'a str, &'a Value>,
    /// Property definition id (including the `parentPropDefId` aliases that
    /// variants carry) -> (`Name#s:l`, REST type).
    prop_defs: HashMap<String, (String, &'static str)>,
    /// Node id -> Figma Motion data (`motion.rs`).
    motion: HashMap<String, Value>,
}

impl<'a> Lk<'a> {
    fn new(entries: &'a [Entry], blobs: &'a [&'a [u8]]) -> Self {
        let mut lk = Lk {
            blobs,
            var_by_key: HashMap::new(),
            set_by_key: HashMap::new(),
            style_by_key: HashMap::new(),
            style_nodes: HashMap::new(),
            style_by_id: HashMap::new(),
            prop_defs: HashMap::new(),
            motion: crate::motion::extract(&entries.iter().map(|e| &e.json).collect::<Vec<_>>()),
        };
        let mut aliases: Vec<(String, String)> = Vec::new();
        for e in entries {
            let j = &e.json;
            let key = fstr(j, "key").filter(|k| !k.is_empty());
            if fstr(j, "styleType").is_some() {
                lk.style_by_id.insert(e.id.as_str(), j);
            }
            match (fstr(j, "type"), key) {
                (Some("VARIABLE"), Some(k)) => {
                    lk.var_by_key.insert(k, format!("VariableID:{}", e.id));
                }
                (Some("VARIABLE_SET"), Some(k)) => {
                    lk.set_by_key.insert(k, format!("VariableCollectionId:{}", e.id));
                }
                (_, Some(k)) if fstr(j, "styleType").is_some() => {
                    lk.style_by_key.insert(k, e.id.clone());
                    lk.style_nodes.insert(k, j);
                }
                _ => {}
            }
            for d in rest_import::farr(j, "componentPropDefs") {
                let Some(id) = d.get("id").and_then(guid_str) else { continue };
                match fstr(d, "name") {
                    Some(name) => {
                        let ty = match fstr(d, "type") {
                            Some("BOOL") => "BOOLEAN",
                            Some("TEXT") => "TEXT",
                            Some("INSTANCE_SWAP") => "INSTANCE_SWAP",
                            _ => "VARIANT",
                        };
                        let key = if ty == "VARIANT" { name.to_string() } else { format!("{name}#{id}") };
                        lk.prop_defs.insert(id, (key, ty));
                    }
                    None => {
                        if let Some(parent) = d.get("parentPropDefId").and_then(guid_str) {
                            aliases.push((id, parent));
                        }
                    }
                }
            }
        }
        for (id, parent) in aliases {
            if let Some(def) = lk.prop_defs.get(&parent).cloned() {
                lk.prop_defs.entry(id).or_insert(def);
            }
        }
        lk
    }

    /// Variable id for an alias value (`{assetRef}` or a local `{guid}`).
    fn variable_id(&self, alias: &Value) -> Option<String> {
        if let Some(k) = asset_key(alias) {
            return self.var_by_key.get(k).cloned();
        }
        alias.get("guid").and_then(guid_str).map(|g| format!("VariableID:{g}"))
    }

    fn set_id(&self, r: &Value) -> Option<String> {
        if let Some(k) = asset_key(r) {
            return self.set_by_key.get(k).cloned();
        }
        r.get("guid").and_then(guid_str).map(|g| format!("VariableCollectionId:{g}"))
    }

    /// Style id of a `styleIdFor*` reference: the style node's id, or
    /// `key:<key>` for a library style that is not in the file.
    fn style_id(&self, r: &Value) -> Option<String> {
        if let Some(k) = asset_key(r) {
            return Some(self.style_by_key.get(k).cloned().unwrap_or_else(|| format!("key:{k}")));
        }
        r.get("guid").and_then(guid_str)
    }

    /// The style node a `styleIdFor*` reference points at.
    fn style_node(&self, r: &Value) -> Option<&'a Value> {
        match asset_key(r) {
            Some(k) => self.style_nodes.get(k).copied(),
            None => self.style_by_id.get(r.get("guid").and_then(guid_str)?.as_str()).copied(),
        }
    }

    fn prop_def(&self, r: &Value) -> Option<&(String, &'static str)> {
        self.prop_defs.get(&r.get("defID").and_then(guid_str)?)
    }
}

/// Builds the REST-shaped `{name, document, ...}` value from a decoded message.
pub(crate) fn message_to_rest(msg: &KiwiValue, name: &str) -> Value {
    let blobs: Vec<&[u8]> = msg
        .get("blobs")
        .and_then(KiwiValue::as_array)
        .map(|a| a.iter().map(|b| b.get("bytes").and_then(KiwiValue::as_bytes).unwrap_or(&[])).collect())
        .unwrap_or_default();
    let changes: Vec<Value> = msg.get("nodeChanges").and_then(KiwiValue::as_array).unwrap_or(&[]).iter().map(kiwi::to_json).collect();
    nodes_to_rest(changes, &blobs, name)
}

/// Imports node changes given as kiwi-shaped JSON (`[{guid, parentIndex, type, ...}]`,
/// enums by name), as `fig2json` prints them. For synthetic test fixtures.
#[doc(hidden)]
pub fn import_fig_node_changes(changes: &Value, name: &str) -> Result<Document, FigError> {
    let list = changes.as_array().cloned().unwrap_or_default();
    let rest = nodes_to_rest(list, &[], name);
    rest_import::import_rest_value(&rest, true)
}

fn nodes_to_rest(changes: Vec<Value>, blobs: &[&[u8]], name: &str) -> Value {
    let mut entries: Vec<Entry> = Vec::new();
    for j in changes {
        if fstr(&j, "phase") == Some("REMOVED") {
            continue;
        }
        let Some(id) = j.get("guid").and_then(guid_str) else { continue };
        let parent = j.get("parentIndex").and_then(|p| p.get("guid")).and_then(guid_str);
        let pos = j.get("parentIndex").and_then(|p| fstr(p, "position")).unwrap_or("").to_string();
        entries.push(Entry { id, parent, pos, json: j });
    }

    let index: HashMap<&str, usize> = entries.iter().enumerate().map(|(i, e)| (e.id.as_str(), i)).collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        match e.parent.as_deref().and_then(|p| index.get(p)) {
            Some(&p) if p != i => children.entry(p).or_default().push(i),
            _ => roots.push(i),
        }
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| entries[*a].pos.cmp(&entries[*b].pos));
    }

    let doc_idx = roots.iter().copied().find(|&i| fstr(&entries[i].json, "type") == Some("DOCUMENT"));
    let canvases: Vec<usize> = match doc_idx {
        Some(d) => children.get(&d).cloned().unwrap_or_default(),
        None => roots.iter().copied().filter(|&i| fstr(&entries[i].json, "type") == Some("CANVAS")).collect(),
    };

    let lk = Lk::new(&entries, blobs);
    let mut visited: HashSet<usize> = HashSet::new();
    let is_internal = |i: usize| entries[i].json.get("internalOnly").and_then(Value::as_bool) == Some(true);
    let pages: Vec<Value> = canvases
        .iter()
        .copied()
        .filter(|&i| !is_internal(i))
        .filter_map(|i| build(&entries, &children, &lk, i, &mut visited, 0))
        .map(|mut p| {
            crate::proto::lift_flow_starts(&mut p);
            p
        })
        .collect();
    // Internal-only canvases hold what the file uses from libraries (remote
    // components and styles); they become the hidden library page.
    let library_pages: Vec<Value> = canvases
        .iter()
        .copied()
        .filter(|&i| is_internal(i))
        .filter_map(|i| build(&entries, &children, &lk, i, &mut visited, 0))
        .filter(|p| p.get("children").and_then(Value::as_array).is_some_and(|c| !c.is_empty()))
        .collect();

    let var_entries: Vec<&Value> = entries.iter().map(|e| &e.json).filter(|j| matches!(fstr(j, "type"), Some("VARIABLE_SET" | "VARIABLE"))).collect();
    let mut out = json!({
        "name": name,
        "document": {"id": "0:0", "name": "Document", "type": "DOCUMENT", "children": pages},
        "styles": style_table(&entries),
    });
    if let (Some(v), Some(o)) = (crate::variables::from_fig_entries(&var_entries), out.as_object_mut()) {
        o.insert("variables".into(), v);
    }
    if !library_pages.is_empty() {
        out["libraryPages"] = Value::Array(library_pages);
    }
    out
}

/// REST-style `styles` map from shared-style nodes (nodes carrying `styleType`
/// FILL/TEXT/EFFECT/GRID; they normally live on the internal-only canvas). The
/// library key is the node's `key`, the description its `styleDescription`.
fn style_table(entries: &[Entry]) -> Value {
    let mut out = Map::new();
    for e in entries {
        let j = &e.json;
        let Some(ty) = fstr(j, "styleType").filter(|t| matches!(*t, "FILL" | "TEXT" | "EFFECT" | "GRID")) else { continue };
        let key = fstr(j, "key").unwrap_or("");
        let mut m = Map::new();
        if !key.is_empty() {
            m.insert("key".into(), key.into());
        }
        m.insert("name".into(), fstr(j, "name").unwrap_or("").into());
        m.insert("description".into(), fstr(j, "styleDescription").unwrap_or("").into());
        m.insert("styleType".into(), ty.into());
        out.insert(e.id.clone(), Value::Object(m));
    }
    Value::Object(out)
}

fn build(entries: &[Entry], children: &HashMap<usize, Vec<usize>>, lk: &Lk, i: usize, visited: &mut HashSet<usize>, depth: usize) -> Option<Value> {
    if depth > 250 || !visited.insert(i) {
        return None;
    }
    let e = &entries[i];
    let mut m = convert_node(&e.json, &e.id, lk)?;
    let kids: Vec<Value> =
        children.get(&i).map(|c| c.iter().filter_map(|&k| build(entries, children, lk, k, visited, depth + 1)).collect()).unwrap_or_default();
    if !kids.is_empty() || matches!(fstr(&e.json, "type"), Some("CANVAS" | "FRAME" | "GROUP" | "SYMBOL" | "INSTANCE" | "SECTION" | "BOOLEAN_OPERATION")) {
        m.insert("children".into(), Value::Array(kids));
    }
    Some(Value::Object(m))
}

// ------------------------------------------------------------------ node

fn copy(m: &mut Map<String, Value>, from: &Value, src: &str, dst: &str) {
    if let Some(v) = from.get(src) {
        m.insert(dst.into(), v.clone());
    }
}

fn enum_map(m: &mut Map<String, Value>, from: &Value, src: &str, dst: &str, table: &[(&str, &str)]) {
    if let Some(s) = fstr(from, src) {
        let out = table.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or(s);
        m.insert(dst.into(), out.into());
    }
}

/// REST `exportSettings` from kiwi export settings: `imageType` (PNG/JPEG/SVG/PDF) and a
/// `constraint` whose type is CONTENT_SCALE / CONTENT_WIDTH / CONTENT_HEIGHT.
fn convert_export_settings(nc: &Value) -> Option<Value> {
    let list = rest_import::farr(nc, "exportSettings");
    let out: Vec<Value> = list
        .iter()
        .map(|e| {
            let format = match fstr(e, "imageType").or_else(|| fstr(e, "format")) {
                Some("JPEG") => "JPG",
                Some(other) => other,
                None => "PNG",
            };
            let c = e.get("constraint");
            let ty = match c.and_then(|c| fstr(c, "type")) {
                Some("CONTENT_WIDTH") | Some("WIDTH") => "WIDTH",
                Some("CONTENT_HEIGHT") | Some("HEIGHT") => "HEIGHT",
                _ => "SCALE",
            };
            let value = c.and_then(|c| fnum(c, "value")).unwrap_or(1.0);
            json!({"suffix": fstr(e, "suffix").unwrap_or(""), "format": format, "constraint": {"type": ty, "value": value}})
        })
        .collect();
    (!out.is_empty()).then_some(Value::Array(out))
}

/// REST-shaped `guides` (`[{axis, offset}]`) from kiwi guides.
fn convert_guides(nc: &Value) -> Option<Value> {
    let out: Vec<Value> = rest_import::farr(nc, "guides")
        .iter()
        .filter_map(|g| {
            let axis = fstr(g, "axis")?;
            let offset = fnum(g, "offset").or_else(|| fnum(g, "position"))?;
            Some(json!({"axis": axis, "offset": offset}))
        })
        .collect();
    (!out.is_empty()).then_some(Value::Array(out))
}

fn convert_node(nc: &Value, id: &str, lk: &Lk) -> Option<Map<String, Value>> {
    let blobs = lk.blobs;
    let ty = match fstr(nc, "type")? {
        // Variables are mapped separately (`variables::from_fig_entries`); they are not canvas content.
        "DOCUMENT" | "NONE" | "VARIABLE" | "VARIABLE_SET" | "VARIABLE_OVERRIDE" => return None,
        // Tracks, keyframes and animation presets are folded into the animated nodes (`motion.rs`).
        _ if crate::motion::is_motion_node(nc) => return None,
        "SYMBOL" => "COMPONENT",
        "ROUNDED_RECTANGLE" => "RECTANGLE",
        "FRAME" if nc.get("isStateGroup").and_then(Value::as_bool) == Some(true) => "COMPONENT_SET",
        // Groups are stored as frames that resize to fit their children
        // (and, unlike hugging auto-layout frames, have no stack mode).
        "FRAME" if nc.get("resizeToFit").and_then(Value::as_bool) == Some(true) && matches!(fstr(nc, "stackMode"), None | Some("NONE")) => "GROUP",
        other => other,
    };
    let mut m = Map::new();
    m.insert("id".into(), id.into());
    m.insert("type".into(), ty.into());
    // Override paths address nodes by `overrideKey`, not by guid.
    if let Some(k) = nc.get("overrideKey").and_then(guid_str) {
        m.insert("_overrideKey".into(), k.into());
    }
    copy(&mut m, nc, "name", "name");
    copy(&mut m, nc, "componentKey", "componentKey");
    copy(&mut m, nc, "sourceLibraryKey", "sourceLibraryKey");
    copy(&mut m, nc, "visible", "visible");
    copy(&mut m, nc, "locked", "locked");
    copy(&mut m, nc, "opacity", "opacity");
    copy(&mut m, nc, "blendMode", "blendMode");
    copy(&mut m, nc, "mask", "isMask");
    copy(&mut m, nc, "size", "size");
    if let Some(t) = nc.get("transform") {
        m.insert("relativeTransform".into(), matrix_json(t));
    }
    if let Some(mo) = lk.motion.get(id) {
        m.insert("motion".into(), mo.clone());
    }
    if let Some(e) = convert_export_settings(nc) {
        m.insert("exportSettings".into(), e);
    }
    if let Some(g) = convert_guides(nc) {
        m.insert("guides".into(), g);
    }
    if ty == "CANVAS" {
        if let Some(t) = crate::motion::page_timeline(nc) {
            m.insert("motionTimeline".into(), t);
        }
        copy(&mut m, nc, "backgroundColor", "backgroundColor");
        return Some(m);
    }
    crate::proto::convert_fig(&mut m, nc);

    // Paint.
    // A style the file defines wins over the node's own (possibly stale) copy of its values,
    // unless a variable is bound to one of the paints.
    let paints = |k: &str, style_key: &str| -> Value {
        let own = rest_import::farr(nc, k);
        let from_style = nc.get(style_key).and_then(|r| lk.style_node(r)).filter(|_| !own.iter().any(|p| p.get("colorVar").is_some()));
        let list = from_style.map(|s| rest_import::farr(s, "fillPaints")).unwrap_or(own);
        Value::Array(list.iter().filter_map(convert_paint).collect())
    };
    m.insert("fills".into(), paints("fillPaints", "styleIdForFill"));
    m.insert("strokes".into(), paints("strokePaints", "styleIdForStrokeFill"));
    copy(&mut m, nc, "strokeWeight", "strokeWeight");
    copy(&mut m, nc, "strokeAlign", "strokeAlign");
    enum_map(&mut m, nc, "strokeCap", "strokeCap", &[("ARROW_LINES", "NONE"), ("ARROW_EQUILATERAL", "NONE")]);
    copy(&mut m, nc, "strokeJoin", "strokeJoin");
    copy(&mut m, nc, "dashPattern", "strokeDashes");
    copy(&mut m, nc, "strokeMiterAngle", "strokeMiterAngle");
    copy(&mut m, nc, "miterLimit", "strokeMiterLimit");
    if nc.get("borderStrokeWeightsIndependent").and_then(Value::as_bool) == Some(true) {
        let w = fnum(nc, "strokeWeight").unwrap_or(1.0);
        let g = |k: &str| fnum(nc, k).unwrap_or(w);
        m.insert(
            "individualStrokeWeights".into(),
            json!({"top": g("borderTopWeight"), "right": g("borderRightWeight"), "bottom": g("borderBottomWeight"), "left": g("borderLeftWeight")}),
        );
    }
    let effects: Vec<Value> = rest_import::farr(nc, "effects").iter().filter_map(convert_effect).collect();
    m.insert("effects".into(), Value::Array(effects));

    // Corners.
    copy(&mut m, nc, "cornerRadius", "cornerRadius");
    copy(&mut m, nc, "cornerSmoothing", "cornerSmoothing");
    if nc.get("rectangleCornerRadiiIndependent").and_then(Value::as_bool) == Some(true) {
        let base = fnum(nc, "cornerRadius").unwrap_or(0.0);
        let g = |k: &str| fnum(nc, k).unwrap_or(base);
        m.insert(
            "rectangleCornerRadii".into(),
            json!([
                g("rectangleTopLeftCornerRadius"),
                g("rectangleTopRightCornerRadius"),
                g("rectangleBottomRightCornerRadius"),
                g("rectangleBottomLeftCornerRadius")
            ]),
        );
    }

    // Constraints.
    let h = match fstr(nc, "horizontalConstraint") {
        Some("CENTER") => "CENTER",
        Some("MAX") => "RIGHT",
        Some("STRETCH") => "LEFT_RIGHT",
        Some("SCALE") => "SCALE",
        _ => "LEFT",
    };
    let v = match fstr(nc, "verticalConstraint") {
        Some("CENTER") => "CENTER",
        Some("MAX") => "BOTTOM",
        Some("STRETCH") => "TOP_BOTTOM",
        Some("SCALE") => "SCALE",
        _ => "TOP",
    };
    m.insert("constraints".into(), json!({"horizontal": h, "vertical": v}));

    // Frames / auto layout.
    if matches!(ty, "FRAME" | "COMPONENT" | "COMPONENT_SET" | "INSTANCE") {
        let masked = nc.get("frameMaskDisabled").and_then(Value::as_bool) != Some(true);
        m.insert("clipsContent".into(), masked.into());
        convert_stack(&mut m, nc);
        if let Some(g) = convert_grids(nc) {
            m.insert("layoutGrids".into(), g);
        }
    }
    // Child layout.
    if let Some(g) = fnum(nc, "stackChildPrimaryGrow") {
        m.insert("layoutGrow".into(), g.into());
    }
    if fstr(nc, "stackChildAlignSelf") == Some("STRETCH") {
        m.insert("layoutAlign".into(), "STRETCH".into());
    }
    if fstr(nc, "stackPositioning") == Some("ABSOLUTE") {
        m.insert("layoutPositioning".into(), "ABSOLUTE".into());
    }
    for (src, dst_x, dst_y) in [("minSize", "minWidth", "minHeight"), ("maxSize", "maxWidth", "maxHeight")] {
        if let Some(s) = nc.get(src) {
            let s = s.get("value").unwrap_or(s);
            if let Some(x) = fnum(s, "x") {
                m.insert(dst_x.into(), x.into());
            }
            if let Some(y) = fnum(s, "y") {
                m.insert(dst_y.into(), y.into());
            }
        }
    }

    // Style references (`{assetRef: {key}}` for library styles, `{guid}` for local ones).
    let mut styles = Map::new();
    for (src, dst) in
        [("styleIdForFill", "fill"), ("styleIdForStrokeFill", "stroke"), ("styleIdForText", "text"), ("styleIdForEffect", "effect"), ("styleIdForGrid", "grid")]
    {
        if let Some(g) = nc.get(src).and_then(|r| lk.style_id(r)) {
            styles.insert(dst.into(), g.into());
        }
    }
    if !styles.is_empty() {
        m.insert("styles".into(), Value::Object(styles));
    }
    if let Some(b) = bound_variables(nc, lk) {
        m.insert("boundVariables".into(), b);
    }
    if let Some(modes) = explicit_modes(nc, lk) {
        m.insert("explicitVariableModes".into(), modes);
    }
    component_props(&mut m, nc, lk);

    // Type-specific.
    match ty {
        "ELLIPSE" => copy(&mut m, nc, "arcData", "arcData"),
        "STAR" => {
            if let Some(c) = nc.get("count").or_else(|| nc.get("pointCount")) {
                m.insert("pointCount".into(), c.clone());
            }
            copy(&mut m, nc, "starInnerScale", "starInnerRadius");
        }
        "REGULAR_POLYGON" => {
            if let Some(c) = nc.get("count").or_else(|| nc.get("pointCount")) {
                m.insert("pointCount".into(), c.clone());
            }
        }
        "BOOLEAN_OPERATION" => enum_map(&mut m, nc, "booleanOperation", "booleanOperation", &[("XOR", "EXCLUDE")]),
        "INSTANCE" => {
            let sd = nc.get("symbolData");
            if let Some(g) = sd.and_then(|s| s.get("symbolID")).and_then(guid_str) {
                m.insert("componentId".into(), g.into());
            }
            if let Some(s) = sd.and_then(|s| s.get("uniformScaleFactor")) {
                m.insert("_uniformScale".into(), s.clone());
            }
            let ov = override_entries(sd.and_then(|s| s.get("symbolOverrides")), lk, false);
            if !ov.is_empty() {
                m.insert("_overrides".into(), Value::Array(ov));
            }
            let derived = override_entries(nc.get("derivedSymbolData").or_else(|| sd.and_then(|s| s.get("derivedSymbolData"))), lk, true);
            if !derived.is_empty() {
                m.insert("_derived".into(), Value::Array(derived));
            }
        }
        "TEXT" => convert_text(&mut m, nc, lk),
        _ => {}
    }
    // Vector networks: vertices, segments and regions of editable vectors.
    if ty == "VECTOR" {
        if let Some(net) = convert_vector_network(nc, blobs) {
            m.insert("vectorNetwork".into(), net);
        }
    }
    // Precomputed outlines (text keeps none: the renderer lays glyphs out itself).
    if ty != "TEXT" {
        for k in ["fillGeometry", "strokeGeometry"] {
            if let Some(g) = convert_geometry(nc, k, blobs) {
                m.insert(k.into(), g);
            }
        }
    }
    Some(m)
}

/// `boundVariables` (REST shape) from `variableConsumptionMap` entries
/// (`{variableField, variableData: {dataType: ALIAS, value: {alias}}}`) and
/// the `colorVar` of fill, stroke and effect colors.
fn bound_variables(nc: &Value, lk: &Lk) -> Option<Value> {
    let alias = |data: &Value| -> Option<Value> {
        let a = data.get("value")?.get("alias")?;
        Some(json!({"type": "VARIABLE_ALIAS", "id": lk.variable_id(a)?}))
    };
    let mut out = Map::new();
    let entries = nc.get("variableConsumptionMap").map(|m| rest_import::farr(m, "entries")).unwrap_or(&[]);
    for e in entries {
        let (Some(field), Some(a)) = (fstr(e, "variableField"), e.get("variableData").and_then(alias)) else { continue };
        let keys: &[&str] = match field {
            "STACK_PADDING_LEFT" => &["paddingLeft"],
            "STACK_PADDING_RIGHT" => &["paddingRight"],
            "STACK_PADDING_TOP" => &["paddingTop"],
            "STACK_PADDING_BOTTOM" => &["paddingBottom"],
            "STACK_SPACING" => &["itemSpacing"],
            "STACK_COUNTER_SPACING" => &["counterAxisSpacing"],
            "RECTANGLE_TOP_LEFT_CORNER_RADIUS" => &["topLeftRadius"],
            "RECTANGLE_TOP_RIGHT_CORNER_RADIUS" => &["topRightRadius"],
            "RECTANGLE_BOTTOM_LEFT_CORNER_RADIUS" => &["bottomLeftRadius"],
            "RECTANGLE_BOTTOM_RIGHT_CORNER_RADIUS" => &["bottomRightRadius"],
            "CORNER_RADIUS" => &["topLeftRadius", "topRightRadius", "bottomLeftRadius", "bottomRightRadius"],
            "WIDTH" => &["width"],
            "HEIGHT" => &["height"],
            "OPACITY" => &["opacity"],
            "STROKE_WEIGHT" => &["strokeWeight"],
            "FONT_SIZE" => &["fontSize"],
            "FONT_FAMILY" => &["fontFamily"],
            "FONT_WEIGHT" => &["fontWeight"],
            "LINE_HEIGHT" => &["lineHeight"],
            "LETTER_SPACING" => &["letterSpacing"],
            _ => &[],
        };
        for k in keys {
            out.insert((*k).into(), a.clone());
        }
    }
    for (src, dst) in [("fillPaints", "fills"), ("strokePaints", "strokes")] {
        let list: Vec<Value> = rest_import::farr(nc, src).iter().map(|p| p.get("colorVar").and_then(alias).unwrap_or(Value::Null)).collect();
        if list.iter().any(|v| !v.is_null()) {
            out.insert(dst.into(), Value::Array(list));
        }
    }
    let fx: Vec<Value> = rest_import::farr(nc, "effects")
        .iter()
        .map(|e| match e.get("colorVar").and_then(alias) {
            Some(a) => json!({"color": a}),
            None => Value::Null,
        })
        .collect();
    if fx.iter().any(|v| !v.is_null()) {
        out.insert("effects".into(), Value::Array(fx));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// `explicitVariableModes` (collection id -> mode id) from `variableModeBySetMap`.
fn explicit_modes(nc: &Value, lk: &Lk) -> Option<Value> {
    let entries = nc.get("variableModeBySetMap").map(|m| rest_import::farr(m, "entries")).unwrap_or(&[]);
    let out: Map<String, Value> =
        entries.iter().filter_map(|e| Some((lk.set_id(e.get("variableSetID")?)?, Value::String(guid_str(e.get("variableModeID")?)?)))).collect();
    (!out.is_empty()).then_some(Value::Object(out))
}

/// Component properties in REST shape: `componentPropertyDefinitions` (from
/// `componentPropDefs`), `componentProperties` (instance
/// `componentPropAssignments`) and `componentPropertyReferences` (node
/// `componentPropRefs`). Variants refer to their set's definitions through
/// `parentPropDefId`; see [`Lk::prop_defs`].
fn component_props(m: &mut Map<String, Value>, nc: &Value, lk: &Lk) {
    // Older files hold a plain `ComponentPropValue` (`initialValue` / `value`:
    // boolValue, textValue, guidValue); newer ones may hold only a typed
    // `varValue` whose `value` carries boolValue, textDataValue or
    // symbolIdValue.guid.
    let text_of = |v: &Value, k: &str| -> Option<Value> { Some(Value::String(fstr(v.get(k)?, "characters")?.to_string())) };
    let value_of = |v: &Value| -> Option<Value> {
        v.get("boolValue")
            .cloned()
            .or_else(|| text_of(v, "textValue"))
            .or_else(|| text_of(v, "textDataValue"))
            .or_else(|| v.get("guidValue").and_then(guid_str).map(Value::String))
            .or_else(|| v.get("symbolIdValue").and_then(|s| s.get("guid")).and_then(guid_str).map(Value::String))
    };
    let prop_value = |p: &Value, plain: &str| -> Option<Value> {
        p.get(plain).and_then(value_of).or_else(|| p.get("varValue").and_then(|v| v.get("value")).and_then(value_of))
    };
    let mut defs = Map::new();
    for d in rest_import::farr(nc, "componentPropDefs") {
        let (Some(id), Some(_)) = (d.get("id").and_then(guid_str), fstr(d, "name")) else { continue };
        let Some((key, ty)) = lk.prop_defs.get(&id) else { continue };
        let mut def = Map::new();
        def.insert("type".into(), (*ty).into());
        if let Some(v) = prop_value(d, "initialValue") {
            def.insert("defaultValue".into(), v);
        }
        if *ty == "VARIANT" {
            let options: Vec<Value> = rest_import::farr(nc, "stateGroupPropertyValueOrders")
                .iter()
                .find(|o| fstr(o, "property") == fstr(d, "name"))
                .map(|o| rest_import::farr(o, "values").to_vec())
                .unwrap_or_default();
            def.insert("variantOptions".into(), Value::Array(options));
        }
        defs.insert(key.clone(), Value::Object(def));
    }
    if !defs.is_empty() {
        m.insert("componentPropertyDefinitions".into(), Value::Object(defs));
    }
    let mut vals = Map::new();
    for a in rest_import::farr(nc, "componentPropAssignments") {
        let Some((key, ty)) = a.get("defID").and_then(guid_str).and_then(|id| lk.prop_defs.get(&id)) else { continue };
        let Some(v) = prop_value(a, "value") else { continue };
        vals.insert(key.clone(), json!({"type": ty, "value": v}));
    }
    if !vals.is_empty() {
        m.insert("componentProperties".into(), Value::Object(vals));
    }
    let ref_field = |f: Option<&str>| match f {
        Some("VISIBLE") => Some("visible"),
        Some("TEXT_DATA") => Some("characters"),
        Some("OVERRIDDEN_SYMBOL_ID") => Some("mainComponent"),
        _ => None,
    };
    let mut refs = Map::new();
    for r in rest_import::farr(nc, "componentPropRefs") {
        let Some((key, _)) = lk.prop_def(r) else { continue };
        let Some(field) = ref_field(fstr(r, "componentPropNodeField")) else { continue };
        refs.insert(field.into(), key.clone().into());
    }
    // Newer files bind through `parameterConsumptionMap`: a PROP_REF
    // variable whose `propRefValue.defId` names the property.
    for e in nc.get("parameterConsumptionMap").map(|m| rest_import::farr(m, "entries")).unwrap_or(&[]) {
        let Some(data) = e.get("variableData").filter(|d| fstr(d, "dataType") == Some("PROP_REF")) else { continue };
        let def = data.get("value").and_then(|v| v.get("propRefValue")).and_then(|p| p.get("defId")).and_then(guid_str);
        let (Some((key, _)), Some(field)) = (def.and_then(|d| lk.prop_defs.get(&d)), ref_field(fstr(e, "variableField"))) else { continue };
        refs.entry(field).or_insert_with(|| key.clone().into());
    }
    if !refs.is_empty() {
        m.insert("componentPropertyReferences".into(), Value::Object(refs));
    }
}

/// REST-shaped `vectorNetwork` from the `vectorData` blob of a `.fig` vector.
/// The blob is stored in the normalised size; vertices and tangents are
/// scaled to the node's size.
fn convert_vector_network(nc: &Value, blobs: &[&[u8]]) -> Option<Value> {
    let vd = nc.get("vectorData")?;
    let blob = blobs.get(vd.get("vectorNetworkBlob")?.as_u64()? as usize)?;
    let size = |v: Option<&Value>| v.and_then(|s| Some((fnum(s, "x")?, fnum(s, "y")?)));
    let scale = match (size(nc.get("size")), size(vd.get("normalizedSize"))) {
        (Some((w, h)), Some((nw, nh))) if nw > 0.0 && nh > 0.0 => (w / nw, h / nh),
        _ => (1.0, 1.0),
    };
    let (mut net, styles) = crate::vecnet::network_from_blob(blob, scale)?;
    if let Some(table) = vd.get("styleOverrideTable").and_then(Value::as_array) {
        crate::vecnet::apply_style_table(&mut net, &styles, table);
    }
    Some(crate::vecnet::network_to_json(&net))
}

fn matrix_json(t: &Value) -> Value {
    let g = |k: &str, d: f64| t.get(k).and_then(Value::as_f64).unwrap_or(d);
    json!([[g("m00", 1.0), g("m01", 0.0), g("m02", 0.0)], [g("m10", 0.0), g("m11", 1.0), g("m12", 0.0)]])
}

/// REST geometry list from `fillGeometry` / `strokeGeometry` blob commands.
/// Winding rule: kiwi `WindingRule` is NONZERO / ODD.
fn convert_geometry(nc: &Value, key: &str, blobs: &[&[u8]]) -> Option<Value> {
    let geo: Vec<Value> = rest_import::farr(nc, key)
        .iter()
        .filter_map(|g| {
            let idx = g.get("commandsBlob")?.as_u64()? as usize;
            let odd = matches!(fstr(g, "windingRule"), Some("ODD" | "EVENODD"));
            let path = commands_blob_to_path(blobs.get(idx)?, odd);
            Some(json!({"path": write_path(&path), "windingRule": if odd { "EVENODD" } else { "NONZERO" }}))
        })
        .collect();
    (!geo.is_empty()).then_some(Value::Array(geo))
}

/// REST `layoutGrids` from kiwi grids: `pattern` (GRID/COLUMNS/ROWS; else `type`,
/// else `axis` X = columns / Y = rows), `sectionSize`, `gutterSize`, `offset`,
/// `count` (or `numSections`), `color`, `visible`.
fn convert_grids(nc: &Value) -> Option<Value> {
    let grids: Vec<Value> = rest_import::farr(nc, "layoutGrids")
        .iter()
        .filter_map(|g| {
            let is_pat = |s: &&str| matches!(*s, "GRID" | "COLUMNS" | "ROWS");
            let pattern = fstr(g, "pattern").filter(is_pat).or_else(|| fstr(g, "type").filter(is_pat)).or(match fstr(g, "axis") {
                Some("X") => Some("COLUMNS"),
                Some("Y") => Some("ROWS"),
                _ => None,
            })?;
            let mut m = Map::new();
            m.insert("pattern".into(), pattern.into());
            copy(&mut m, g, "sectionSize", "sectionSize");
            copy(&mut m, g, "gutterSize", "gutterSize");
            copy(&mut m, g, "offset", "offset");
            copy(&mut m, g, "color", "color");
            copy(&mut m, g, "visible", "visible");
            if let Some(c) = g.get("count").or_else(|| g.get("numSections")) {
                m.insert("count".into(), c.clone());
            }
            Some(Value::Object(m))
        })
        .collect();
    (!grids.is_empty()).then_some(Value::Array(grids))
}

/// `guidPath` as `"s:l"` strings (accepts `{guids: [...]}` or a bare array).
fn guid_path(o: &Value) -> Vec<String> {
    let p = o.get("guidPath");
    let list = p.and_then(|p| p.get("guids")).or(p).and_then(Value::as_array);
    list.map(|a| a.iter().filter_map(guid_str).collect()).unwrap_or_default()
}

/// Partial REST node fields from an override / derived-data entry: only keys
/// present in the entry are emitted.
fn convert_override(o: &Value, lk: &Lk) -> Map<String, Value> {
    let blobs = lk.blobs;
    let mut m = Map::new();
    for k in ["name", "visible", "locked", "opacity", "blendMode", "size", "strokeWeight", "strokeAlign", "cornerRadius"] {
        copy(&mut m, o, k, k);
    }
    copy(&mut m, o, "mask", "isMask");
    // Instance swap of a nested instance: the override names the new SYMBOL.
    if let Some(g) = o.get("overriddenSymbolID").and_then(guid_str) {
        m.insert("componentId".into(), g.into());
    }
    // Variable bindings of the overridden paints (`colorVar`): the literal
    // `color` next to an alias can be stale, the variable's value is the truth.
    if let Some(bv) = bound_variables(o, lk) {
        m.insert("boundVariables".into(), bv);
    }
    if let Some(t) = o.get("transform") {
        m.insert("relativeTransform".into(), matrix_json(t));
    }
    for (src, dst, style) in [("fillPaints", "fills", "styleIdForFill"), ("strokePaints", "strokes", "styleIdForStrokeFill")] {
        if o.get(src).is_some() {
            m.insert(dst.into(), Value::Array(rest_import::farr(o, src).iter().filter_map(convert_paint).collect()));
        } else if let Some(sn) = o.get(style).and_then(|r| lk.style_node(r)) {
            // An override that only names a library style: the style supplies the paints.
            m.insert(dst.into(), Value::Array(rest_import::farr(sn, "fillPaints").iter().filter_map(convert_paint).collect()));
        }
    }
    if o.get("effects").is_some() {
        m.insert("effects".into(), Value::Array(rest_import::farr(o, "effects").iter().filter_map(convert_effect).collect()));
    }
    for k in ["fillGeometry", "strokeGeometry"] {
        if let Some(g) = convert_geometry(o, k, blobs) {
            m.insert(k.into(), g);
        }
    }
    // Text.
    let mut st = convert_type_style(o, None);
    st.remove("fills");
    if let Some(ls) = o.get("letterSpacing") {
        if fstr(ls, "units") == Some("PERCENT") && fnum(o, "fontSize").is_none() {
            // Resolved against the target node's own font size when applied.
            st.remove("letterSpacing");
            st.insert("letterSpacingPercent".into(), kiwi::clean_f32(fnum(ls, "value").unwrap_or(0.0)));
        }
    }
    for k in ["textAutoResize", "textTruncation", "maxLines"] {
        copy(&mut st, o, k, k);
    }
    if !st.is_empty() {
        m.insert("style".into(), Value::Object(st));
    }
    if let Some(td) = o.get("textData") {
        if let Some(c) = fstr(td, "characters") {
            m.insert("characters".into(), c.into());
        }
        if let Some(lines) = td.get("lines").and_then(Value::as_array) {
            let mut st = m.get("style").and_then(Value::as_object).cloned().unwrap_or_default();
            crate::textfmt::lines_from_kiwi(lines, &mut m, &mut st);
            if !st.is_empty() {
                m.insert("style".into(), Value::Object(st));
            }
        }
        text_overrides(&mut m, td, fnum(o, "fontSize"));
    }
    m
}

fn override_entries(list: Option<&Value>, lk: &Lk, derived: bool) -> Vec<Value> {
    let Some(a) = list.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .map(|o| {
            let mut set = convert_override(o, lk);
            if derived {
                set.retain(|k, _| matches!(k.as_str(), "size" | "relativeTransform" | "fillGeometry" | "strokeGeometry"));
            }
            json!({"path": guid_path(o), "set": Value::Object(set)})
        })
        .collect()
}

fn convert_stack(m: &mut Map<String, Value>, nc: &Value) {
    let mode = match fstr(nc, "stackMode") {
        Some("HORIZONTAL") => "HORIZONTAL",
        Some("VERTICAL") => "VERTICAL",
        _ => return,
    };
    m.insert("layoutMode".into(), mode.into());
    copy(m, nc, "stackSpacing", "itemSpacing");
    copy(m, nc, "stackCounterSpacing", "counterAxisSpacing");
    let h = fnum(nc, "stackHorizontalPadding");
    let v = fnum(nc, "stackVerticalPadding");
    let ins = |m: &mut Map<String, Value>, k: &str, x: Option<f32>| {
        if let Some(x) = x {
            m.insert(k.into(), x.into());
        }
    };
    ins(m, "paddingLeft", h);
    ins(m, "paddingRight", fnum(nc, "stackPaddingRight").or(h));
    ins(m, "paddingTop", v);
    ins(m, "paddingBottom", fnum(nc, "stackPaddingBottom").or(v));
    let align = |s: Option<&str>| -> &'static str {
        match s {
            Some("CENTER") => "CENTER",
            Some("MAX") => "MAX",
            Some("SPACE_BETWEEN") | Some("SPACE_EVENLY") => "SPACE_BETWEEN",
            Some("BASELINE") => "BASELINE",
            _ => "MIN",
        }
    };
    m.insert("primaryAxisAlignItems".into(), align(fstr(nc, "stackPrimaryAlignItems")).into());
    m.insert("counterAxisAlignItems".into(), align(fstr(nc, "stackCounterAlignItems")).into());
    let sizing = |s: Option<&str>| if s.map(|s| s.starts_with("RESIZE_TO_FIT")).unwrap_or(false) { "AUTO" } else { "FIXED" };
    // Real files only ever write `FIXED` for the primary axis (absent = hug) and
    // `RESIZE_TO_FIT_WITH_IMPLICIT_SIZE` for the counter axis (absent = fixed).
    let primary = if fstr(nc, "stackPrimarySizing") == Some("FIXED") { "FIXED" } else { "AUTO" };
    m.insert("primaryAxisSizingMode".into(), primary.into());
    m.insert("counterAxisSizingMode".into(), sizing(fstr(nc, "stackCounterSizing")).into());
    if nc.get("stackWrap").and_then(Value::as_bool) == Some(true) {
        m.insert("layoutWrap".into(), "WRAP".into());
    }
    copy(m, nc, "stackReverseZIndex", "itemReverseZIndex");
    copy(m, nc, "bordersTakeSpace", "strokesIncludedInLayout");
}

// ------------------------------------------------------------------ paint / effect

fn convert_paint(p: &Value) -> Option<Value> {
    let ty = fstr(p, "type")?;
    let mut m = Map::new();
    m.insert("type".into(), ty.into());
    copy(&mut m, p, "opacity", "opacity");
    copy(&mut m, p, "visible", "visible");
    copy(&mut m, p, "blendMode", "blendMode");
    match ty {
        "SOLID" => copy(&mut m, p, "color", "color"),
        t if t.starts_with("GRADIENT") => {
            let stops: Vec<Value> = rest_import::farr(p, "stops").iter().map(|s| json!({"position": s.get("position"), "color": s.get("color")})).collect();
            m.insert("gradientStops".into(), Value::Array(stops));
            let t = p.get("transform");
            let g = |k: &str, d: f32| t.and_then(|t| fnum(t, k)).unwrap_or(d);
            let xf = Affine { a: g("m00", 1.0), c: g("m01", 0.0), tx: g("m02", 0.0), b: g("m10", 0.0), d: g("m11", 1.0), ty: g("m12", 0.0) };
            let inv = xf.invert().unwrap_or(Affine::IDENTITY);
            let h = [Vec2::new(0.0, 0.5), Vec2::new(1.0, 0.5), Vec2::new(0.0, 1.0)].map(|q| {
                let r = inv.apply(q);
                json!({"x": kiwi::clean_f32(r.x), "y": kiwi::clean_f32(r.y)})
            });
            m.insert("gradientHandlePositions".into(), Value::Array(h.to_vec()));
        }
        "IMAGE" => {
            let hash = p.get("image").and_then(|i| fstr(i, "hash")).or_else(|| fstr(p, "imageHash"))?;
            m.insert("imageRef".into(), hash.into());
            enum_map(&mut m, p, "imageScaleMode", "scaleMode", &[]);
        }
        _ => return None,
    }
    Some(Value::Object(m))
}

fn convert_effect(e: &Value) -> Option<Value> {
    let ty = match fstr(e, "type")? {
        "FOREGROUND_BLUR" => "LAYER_BLUR",
        other => other,
    };
    let mut m = Map::new();
    m.insert("type".into(), ty.into());
    for k in ["color", "offset", "radius", "spread", "visible", "showShadowBehindNode"] {
        copy(&mut m, e, k, k);
    }
    Some(Value::Object(m))
}

// ------------------------------------------------------------------ text

/// "SemiBold Italic" -> (600, true)
pub(crate) fn weight_from_style_name(style: &str) -> (u16, bool) {
    let norm: String = style.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let italic = norm.contains("italic") || norm.contains("oblique");
    const TABLE: &[(&str, u16)] = &[
        ("extralight", 200),
        ("ultralight", 200),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("semibold", 600),
        ("demibold", 600),
        ("thin", 100),
        ("hairline", 100),
        ("light", 300),
        ("medium", 500),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
        ("regular", 400),
        ("normal", 400),
        ("book", 400),
    ];
    let w = TABLE.iter().find(|(k, _)| norm.contains(k)).map(|(_, w)| *w).unwrap_or(400);
    (w, italic)
}

/// Partial REST `TypeStyle` from kiwi text properties (`base_size` resolves
/// percentage letter spacing for override entries without their own size).
fn convert_type_style(o: &Value, base_size: Option<f32>) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(f) = o.get("fontName") {
        if let Some(fam) = fstr(f, "family") {
            m.insert("fontFamily".into(), fam.into());
        }
        if let Some(st) = fstr(f, "style") {
            let (w, i) = weight_from_style_name(st);
            m.insert("fontWeight".into(), w.into());
            m.insert("italic".into(), i.into());
        }
    }
    let size = fnum(o, "fontSize").or(base_size);
    if let Some(s) = fnum(o, "fontSize") {
        m.insert("fontSize".into(), s.into());
    }
    copy(&mut m, o, "textAlignHorizontal", "textAlignHorizontal");
    copy(&mut m, o, "textAlignVertical", "textAlignVertical");
    copy(&mut m, o, "textCase", "textCase");
    copy(&mut m, o, "textDecoration", "textDecoration");
    copy(&mut m, o, "paragraphSpacing", "paragraphSpacing");
    for k in ["paragraphIndent", "listSpacing", "hangingPunctuation", "hangingList", "leadingTrim"] {
        copy(&mut m, o, k, k);
    }
    if let Some(flags) = crate::textfmt::flags_from_kiwi(o) {
        m.insert("opentypeFlags".into(), flags);
    }
    if let Some(ls) = o.get("letterSpacing") {
        let v = fnum(ls, "value").unwrap_or(0.0);
        let px = if fstr(ls, "units") == Some("PERCENT") { v / 100.0 * size.unwrap_or(0.0) } else { v };
        m.insert("letterSpacing".into(), kiwi::clean_f32(px));
    }
    if let Some(lh) = o.get("lineHeight") {
        // Kiwi `Number {value, units}`: PIXELS = px; PERCENT = percent of the font
        // size (100 means exactly 1.0 x font size, NOT auto); RAW = multiplier,
        // where RAW 1 is Figma's "auto". A missing value also means auto.
        let intrinsic = |m: &mut Map<String, Value>| {
            m.insert("lineHeightUnit".into(), "INTRINSIC_%".into());
        };
        let percent = |m: &mut Map<String, Value>, p: f32| {
            m.insert("lineHeightUnit".into(), "FONT_SIZE_%".into());
            m.insert("lineHeightPercentFontSize".into(), kiwi::clean_f32(p));
        };
        match (fnum(lh, "value"), fstr(lh, "units")) {
            (None, _) => intrinsic(&mut m),
            (Some(v), Some("PIXELS")) => {
                m.insert("lineHeightUnit".into(), "PIXELS".into());
                m.insert("lineHeightPx".into(), kiwi::clean_f32(v));
            }
            (Some(v), Some("RAW")) if (v - 1.0).abs() < 1e-3 => intrinsic(&mut m),
            (Some(v), Some("RAW")) => percent(&mut m, v * 100.0),
            (Some(v), _) => percent(&mut m, v),
        }
    }
    if o.get("fillPaints").is_some() {
        let fills: Vec<Value> = rest_import::farr(o, "fillPaints").iter().filter_map(convert_paint).collect();
        m.insert("fills".into(), Value::Array(fills));
    }
    m
}

fn convert_text(m: &mut Map<String, Value>, nc: &Value, lk: &Lk) {
    let td = nc.get("textData");
    let chars = td.and_then(|t| fstr(t, "characters")).unwrap_or("");
    m.insert("characters".into(), chars.into());
    let mut style = convert_type_style(nc, None);
    // Linked styles are authoritative: kiwi keeps pre-link properties on the
    // node (e.g. 48px even though the active shared style specifies 18px).
    // Character overrides are applied afterwards, against this resolved base.
    if let Some(shared) = nc.get("styleIdForText").and_then(|r| lk.style_node(r)) {
        let mut linked = convert_type_style(shared, None);
        for (key, default) in [("textCase", "ORIGINAL"), ("textDecoration", "NONE")] {
            linked.entry(key.to_string()).or_insert_with(|| default.into());
        }
        // A text style does not supply the layer's paint or box alignment.
        for key in ["fills", "textAlignHorizontal", "textAlignVertical"] {
            linked.remove(key);
        }
        style.extend(linked);
    }
    style.remove("fills"); // node-level fills are the text color
    copy(&mut style, nc, "textAutoResize", "textAutoResize");
    if let Some(a) = fstr(nc, "textAutoResize") {
        m.insert("textAutoResize".into(), a.into());
    }
    // textTruncation ENDING and maxLines (kiwi fields of the same names).
    copy(m, nc, "textTruncation", "textTruncation");
    copy(m, nc, "maxLines", "maxLines");
    if let Some(lines) = td.and_then(|t| t.get("lines")).and_then(Value::as_array) {
        crate::textfmt::lines_from_kiwi(lines, m, &mut style);
    }
    let base_size = style.get("fontSize").and_then(Value::as_f64).map(|n| n as f32);
    m.insert("style".into(), Value::Object(style));
    if let Some(td) = td {
        text_overrides(m, td, base_size);
    }
}

/// Per-character style ids and the override table from kiwi `textData`.
fn text_overrides(m: &mut Map<String, Value>, td: &Value, base_size: Option<f32>) {
    if let Some(ids) = td.get("characterStyleIDs").and_then(Value::as_array) {
        m.insert("characterStyleOverrides".into(), Value::Array(ids.clone()));
        let mut table = Map::new();
        for o in rest_import::farr(td, "styleOverrideTable") {
            if let Some(sid) = o.get("styleID").and_then(Value::as_u64) {
                table.insert(sid.to_string(), Value::Object(convert_type_style(o, base_size)));
            }
        }
        m.insert("styleOverrideTable".into(), Value::Object(table));
    }
}

// ------------------------------------------------------------------ blobs

/// Decodes a Figma path commands blob (0=close, 1=move, 2=line, 3=quad, 4=cubic; f32 LE coords).
pub fn commands_blob_to_path(b: &[u8], even_odd: bool) -> Path {
    let mut cmds = Vec::new();
    let mut i = 0usize;
    let f = |i: usize| -> Option<f32> { b.get(i..i + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]])) };
    let pt = |i: usize| -> Option<Vec2> { Some(Vec2::new(f(i)?, f(i + 4)?)) };
    while i < b.len() {
        let op = b[i];
        i += 1;
        let ok = match op {
            0 => {
                cmds.push(PathCmd::Close);
                Some(())
            }
            1 => pt(i).map(|p| {
                cmds.push(PathCmd::MoveTo(p));
                i += 8;
            }),
            2 => pt(i).map(|p| {
                cmds.push(PathCmd::LineTo(p));
                i += 8;
            }),
            3 => pt(i).zip(pt(i + 8)).map(|(c, p)| {
                cmds.push(PathCmd::QuadTo(c, p));
                i += 16;
            }),
            4 => pt(i).zip(pt(i + 8)).zip(pt(i + 16)).map(|((c1, c2), p)| {
                cmds.push(PathCmd::CubicTo(c1, c2, p));
                i += 24;
            }),
            _ => None,
        };
        if ok.is_none() {
            break;
        }
    }
    Path { cmds, fill_rule: if even_odd { FillRule::EvenOdd } else { FillRule::NonZero } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_names() {
        assert_eq!(weight_from_style_name("Regular"), (400, false));
        assert_eq!(weight_from_style_name("Semi Bold Italic"), (600, true));
        assert_eq!(weight_from_style_name("ExtraBold"), (800, false));
        assert_eq!(weight_from_style_name("Bold"), (700, false));
        assert_eq!(weight_from_style_name("Extra-Light"), (200, false));
    }

    #[test]
    fn kiwi_export_settings_and_guides_map_to_rest_shapes() {
        let nc = json!({
            "exportSettings": [
                {"suffix": "@2x", "imageType": "JPEG", "constraint": {"type": "CONTENT_SCALE", "value": 2.0}},
                {"imageType": "SVG", "constraint": {"type": "CONTENT_WIDTH", "value": 512.0}},
                {"imageType": "PDF", "constraint": {"type": "CONTENT_HEIGHT", "value": 64.0}}
            ],
            "guides": [{"axis": "X", "offset": 10.0}, {"axis": "Y", "offset": 4.5}, {"offset": 1.0}]
        });
        let doc = rest_import::parse_export_settings(&json!({"exportSettings": convert_export_settings(&nc).unwrap()}));
        use desigma_doc::{ExportConstraint as C, ExportFileFormat as F};
        assert_eq!((doc[0].format, doc[0].constraint, doc[0].suffix.as_str()), (F::Jpg, C::Scale(2.0), "@2x"));
        assert_eq!((doc[1].format, doc[1].constraint), (F::Svg, C::Width(512.0)));
        assert_eq!((doc[2].format, doc[2].constraint), (F::Pdf, C::Height(64.0)));
        assert_eq!(convert_guides(&nc).unwrap(), json!([{"axis": "X", "offset": 10.0}, {"axis": "Y", "offset": 4.5}]));
        assert!(convert_export_settings(&json!({})).is_none() && convert_guides(&json!({})).is_none());
    }

    #[test]
    fn blob_decoding_is_tolerant() {
        let mut b = vec![1u8];
        for v in [1.0f32, 2.0] {
            b.extend(v.to_le_bytes());
        }
        b.push(2);
        b.extend(3.0f32.to_le_bytes()); // truncated lineTo
        let p = commands_blob_to_path(&b, false);
        assert_eq!(p.cmds, vec![PathCmd::MoveTo(Vec2::new(1.0, 2.0))]);
        assert!(commands_blob_to_path(&[9, 9, 9], false).cmds.is_empty());
    }
}
