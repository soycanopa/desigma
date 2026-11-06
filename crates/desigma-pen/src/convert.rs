//! Object tree -> desigma nodes.
//!
//! Every `.pen` object becomes a node in `Conv::doc` (the arena) as soon as it is
//! converted; parents are linked when their children are done. Components are
//! converted on first use, wherever the reference is met, and handed to their own
//! position in the tree later (see `component_node`).

use crate::vars::{var_id, Theme, Vars};
use crate::{img, Options, Warnings};
use desigma_doc::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Default for [`Options::max_objects`]: most nodes created from one file, so that reference
/// expansion (components of components of components) cannot use unbounded memory.
const MAX_NODES: usize = 250_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Fixed,
    Fill,
    Hug,
}

/// One dimension: a number, `fill_container`, `fit_content` or a variable.
#[derive(Clone, Debug)]
pub(crate) struct Axis {
    pub mode: Mode,
    /// The number, or the fallback in parentheses.
    pub px: Option<f32>,
    pub var: Option<String>,
}

impl Axis {
    pub fn fixed(px: Option<f32>) -> Self {
        Self { mode: Mode::Fixed, px, var: None }
    }
}

/// What a child needs to know about the object that contains it.
#[derive(Clone, Debug)]
pub(crate) struct Ctx {
    /// `Some(horizontal)` when the parent lays its children out.
    pub layout: Option<bool>,
    /// Size of the parent when it is a plain number.
    pub parent_size: (Option<f32>, Option<f32>),
    /// Effective theme: every axis has a value.
    pub theme: Theme,
}

impl Ctx {
    /// Whether `n` is placed by the parent's layout (not absolutely positioned).
    pub fn in_flow(&self, n: &Node) -> bool {
        self.layout.is_some() && !n.layout_child.absolute
    }
}

pub(crate) struct Conv<'a> {
    pub opts: &'a Options,
    pub doc: Document,
    pub vars: Vars,
    pub warn: Warnings,
    pub defaults: Theme,
    /// Every object with an `id`, first one wins.
    pub all: HashMap<&'a str, &'a Value>,
    /// Context each reusable object is converted in.
    pub ctxs: HashMap<&'a str, Ctx>,
    pub comp_nodes: HashMap<String, NodeId>,
    pub in_progress: HashSet<String>,
    pub ref_depth: usize,
    pub count: usize,
    /// Nodes flipped on an axis; applied once sizes are final.
    pub flips: Vec<(NodeId, bool, bool)>,
    /// (instance root, component) pairs to link once sizes are final.
    pub links: Vec<(NodeId, NodeId)>,
    image_keys: HashMap<String, Option<String>>,
}

pub(crate) fn clampf(x: f64) -> f32 {
    x.clamp(-1e7, 1e7) as f32
}

fn is_true(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool) == Some(true)
}

/// Types that cannot be a component on their own and get a component frame around them.
pub(crate) fn needs_wrapper(ty: &str) -> bool {
    !matches!(ty, "frame" | "rectangle" | "group")
}

impl<'a> Conv<'a> {
    pub fn new(opts: &'a Options, doc: Document, vars: Vars) -> Self {
        let defaults = vars.default_theme();
        Self {
            opts,
            doc,
            vars,
            warn: Warnings::default(),
            defaults,
            all: HashMap::new(),
            ctxs: HashMap::new(),
            comp_nodes: HashMap::new(),
            in_progress: HashSet::new(),
            ref_depth: 0,
            count: 0,
            flips: Vec::new(),
            links: Vec::new(),
            image_keys: HashMap::new(),
        }
    }

    pub fn root_ctx(&self) -> Ctx {
        Ctx { layout: None, parent_size: (None, None), theme: self.defaults.clone() }
    }

    // ------------------------------------------------------------------ value readers

    /// Boolean or boolean variable (in the theme in effect).
    pub fn flag(&self, v: &Value, ctx: &Ctx) -> Option<bool> {
        v.as_bool().or_else(|| self.vars.boolean(v, &ctx.theme))
    }

    /// Number or number variable, resolved in the theme in effect. For properties desigma cannot bind.
    pub fn plain_num(&self, v: &Value, ctx: &Ctx) -> Option<f32> {
        match v {
            Value::Number(n) => n.as_f64().map(clampf),
            _ => self.vars.number(v, &ctx.theme).map(clampf),
        }
    }

    /// String or string variable, resolved in the theme in effect.
    pub fn plain_string(&self, v: &Value, ctx: &Ctx) -> Option<String> {
        if self.vars.reference(v, VariableKind::String).is_some() {
            return self.vars.string(v, &ctx.theme);
        }
        v.as_str().map(String::from)
    }

    /// Number or number variable for a bindable property: the value in the default theme and
    /// the variable's name. The mode in effect at the node picks the shown value when drawn.
    pub fn numv(&mut self, v: &Value, _ctx: &Ctx) -> Option<(f32, Option<String>)> {
        match v {
            Value::Number(n) => n.as_f64().map(|n| (clampf(n), None)),
            Value::String(s) => {
                if let Some(name) = self.vars.reference(v, VariableKind::Number) {
                    let val = self.vars.number(v, &self.defaults).map(clampf);
                    if val.is_none() {
                        self.warn.add(format!("variable \"{name}\" has no number value"));
                    }
                    return val.map(|x| (x, Some(name)));
                }
                if s.starts_with('$') {
                    self.warn.add(format!("number variable {s} does not exist"));
                    return None;
                }
                s.trim().parse::<f64>().ok().map(|n| (clampf(n), None))
            }
            _ => None,
        }
    }

    /// `width`/`height` value.
    pub fn axis(&mut self, v: Option<&Value>, ctx: &Ctx, default: Mode) -> Axis {
        let Some(v) = v else { return Axis { mode: default, px: None, var: None } };
        if let Value::String(s) = v {
            let paren = |s: &str| s.split_once('(').and_then(|(_, r)| r.trim_end_matches(')').trim().parse::<f64>().ok()).map(clampf);
            if s.starts_with("fit_content") {
                return Axis { mode: Mode::Hug, px: paren(s), var: None };
            }
            if s.starts_with("fill_container") {
                return Axis { mode: Mode::Fill, px: paren(s), var: None };
            }
        }
        match self.numv(v, ctx) {
            Some((px, var)) => Axis { mode: Mode::Fixed, px: Some(px.max(0.0)), var },
            None => {
                self.warn.add("a width or height could not be read");
                Axis { mode: default, px: None, var: None }
            }
        }
    }

    pub fn image_key(&mut self, url: &str) -> Option<String> {
        if let Some(k) = self.image_keys.get(url) {
            return k.clone();
        }
        let key = match img::load(url, self.opts.base_dir.as_deref()) {
            Ok(bytes) => {
                let key = img::key(&bytes);
                self.doc.images.insert(key.clone(), bytes);
                Some(key)
            }
            Err(e) => {
                self.warn.add(format!("{e}; the fill is drawn gray"));
                None
            }
        };
        self.image_keys.insert(url.to_string(), key.clone());
        key
    }

    // ------------------------------------------------------------------ contexts

    /// `ctx` with the object's own `theme` merged in.
    pub fn with_theme(&self, v: &Value, ctx: &Ctx) -> Ctx {
        let mut out = ctx.clone();
        if let Some(t) = v.get("theme").and_then(Value::as_object) {
            for (axis, value) in t {
                if let Some(s) = value.as_str() {
                    out.theme.insert(axis.clone(), s.to_string());
                }
            }
        }
        out
    }

    /// Context for the children of `v`.
    pub fn child_ctx(&self, v: &Value, own: &Ctx) -> Ctx {
        let layout = match v.get("type").and_then(Value::as_str) {
            Some("frame") => match v.get("layout").and_then(Value::as_str) {
                Some("none") => None,
                Some("vertical") => Some(false),
                _ => Some(true),
            },
            _ => None,
        };
        let px = |k: &str| v.get(k).and_then(Value::as_f64).map(clampf);
        Ctx { layout, parent_size: (px("width"), px("height")), theme: own.theme.clone() }
    }

    /// Records every object by id and the context reusable ones are converted in.
    pub fn index(&mut self, v: &'a Value, ctx: &Ctx) {
        let Some(obj) = v.as_object() else { return };
        let own = self.with_theme(v, ctx);
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            self.all.entry(id).or_insert(v);
            if is_true(v, "reusable") {
                self.ctxs.entry(id).or_insert_with(|| ctx.clone());
            }
        }
        if let Some(kids) = obj.get("children").and_then(Value::as_array) {
            let cctx = self.child_ctx(v, &own);
            for k in kids {
                self.index(k, &cctx);
            }
        }
    }

    // ------------------------------------------------------------------ node building blocks

    /// A node with the properties every object shares.
    pub fn base(&mut self, v: &Value, kind: NodeKind, ctx: &Ctx) -> Node {
        self.count += 1;
        let id = self.doc.alloc_id();
        let mut n = Node::new(id, kind);
        n.source_id = v.get("id").and_then(Value::as_str).map(String::from);
        if let Some(name) = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            n.name = name.to_string();
        }
        if let Some(e) = v.get("enabled") {
            if let Some(name) = self.vars.reference(e, VariableKind::Boolean) {
                n.visible = self.vars.boolean(e, &self.defaults).unwrap_or(true);
                n.variables.bindings.insert("visible".into(), var_id(&name));
            } else if let Some(b) = e.as_bool() {
                n.visible = b;
            }
        }
        if let Some(o) = v.get("opacity").and_then(|o| self.plain_num(o, ctx)) {
            n.opacity = o.clamp(0.0, 1.0);
        }
        if ctx.layout.is_some() && v.get("layoutPosition").and_then(Value::as_str) == Some("absolute") {
            n.layout_child.absolute = true;
        }
        if v.get("theme").is_some_and(|t| t.as_object().is_some_and(|o| !o.is_empty())) {
            for (collection, mode) in self.vars.modes_for(&ctx.theme) {
                n.variables.modes.insert(collection, mode);
            }
        }
        n
    }

    /// Position, rotation and flips. `x`/`y` are ignored where the parent lays children out.
    pub fn place(&mut self, n: &mut Node, v: &Value, ctx: &Ctx) {
        let (mut x, mut y) = (0.0, 0.0);
        if !ctx.in_flow(n) {
            x = v.get("x").and_then(|x| self.plain_num(x, ctx)).unwrap_or(0.0);
            y = v.get("y").and_then(|y| self.plain_num(y, ctx)).unwrap_or(0.0);
        }
        let deg = v.get("rotation").and_then(|r| self.plain_num(r, ctx)).unwrap_or(0.0);
        // `.pen` rotates counter-clockwise around the top-left corner.
        n.transform = Affine::translate(x, y) * Affine::rotate(-deg.to_radians());
        let fx = v.get("flipX").and_then(|f| self.flag(f, ctx)).unwrap_or(false);
        let fy = v.get("flipY").and_then(|f| self.flag(f, ctx)).unwrap_or(false);
        if fx || fy {
            self.flips.push((n.id, fx, fy));
        }
    }

    /// Applies `width`/`height` to the node: its size and how a layout parent treats it.
    pub fn apply_size(&mut self, n: &mut Node, w: &Axis, h: &Axis, ctx: &Ctx) {
        let flow = ctx.in_flow(n);
        let pick = |a: &Axis, parent: Option<f32>| match a.mode {
            Mode::Fill if !flow => parent.or(a.px).unwrap_or(0.0),
            _ => a.px.unwrap_or(0.0),
        };
        n.size = Vec2::new(pick(w, ctx.parent_size.0), pick(h, ctx.parent_size.1));
        if flow {
            if w.mode == Mode::Fill {
                n.layout_child.horizontal = Sizing::Fill;
            }
            if h.mode == Mode::Fill {
                n.layout_child.vertical = Sizing::Fill;
            }
        }
        if let Some(var) = &w.var {
            n.variables.bindings.insert("width".into(), var_id(var));
        }
        if let Some(var) = &h.var {
            n.variables.bindings.insert("height".into(), var_id(var));
        }
    }

    /// `fill`, `stroke`, `effect` and (when given) corner radii.
    pub fn appearance(&mut self, n: &mut Node, v: &Value, ctx: &Ctx, align: StrokeAlign) {
        if let Some(f) = v.get("fill") {
            let outs = self.fills(f, ctx);
            Self::bind_paints(&mut n.variables.bindings, &outs, "fills");
            n.fills = outs.into_iter().map(|o| o.paint).collect();
        }
        self.stroke(v, ctx, n, align);
        if let Some(e) = v.get("effect") {
            n.effects = self.effects(e, ctx);
        }
    }

    /// `cornerRadius`: one value or four (top-left, top-right, bottom-right, bottom-left).
    pub fn corner_radii(&mut self, n: &mut Node, v: &Value, ctx: &Ctx) {
        const KEYS: [&str; 4] = ["topLeftRadius", "topRightRadius", "bottomRightRadius", "bottomLeftRadius"];
        match v.get("cornerRadius") {
            Some(Value::Array(list)) => {
                for (i, r) in list.iter().take(4).enumerate() {
                    if let Some((val, var)) = self.numv(r, ctx) {
                        n.corner_radii[i] = val.max(0.0);
                        if let Some(var) = var {
                            n.variables.bindings.insert(KEYS[i].into(), var_id(&var));
                        }
                    }
                }
            }
            Some(r) => {
                if let Some((val, var)) = self.numv(r, ctx) {
                    n.corner_radii = [val.max(0.0); 4];
                    if let Some(var) = var {
                        for k in KEYS {
                            n.variables.bindings.insert(k.into(), var_id(&var));
                        }
                    }
                }
            }
            None => {}
        }
    }

    /// Stores `n` in the arena as the parent of `kids`.
    pub fn put(&mut self, mut n: Node, kids: Vec<NodeId>) -> NodeId {
        let id = n.id;
        for k in &kids {
            if let Some(c) = self.doc.nodes.get_mut(k) {
                c.parent = Some(id);
            }
        }
        n.children = kids;
        self.doc.nodes.insert(id, n);
        id
    }

    /// Converts the `children` of `v`.
    fn children(&mut self, v: &Value, cctx: &Ctx) -> Vec<NodeId> {
        let Some(list) = v.get("children").and_then(Value::as_array) else { return Vec::new() };
        list.iter().filter_map(|c| self.node(c, cctx)).collect()
    }

    // ------------------------------------------------------------------ dispatch

    /// Converts one object (and everything below it) and returns its node.
    pub fn node(&mut self, v: &Value, ctx: &Ctx) -> Option<NodeId> {
        let max = self.opts.max_objects.unwrap_or(MAX_NODES);
        if self.count >= max {
            self.warn.add(format!("the file has more than {max} objects (counting instances); the rest was left out"));
            return None;
        }
        if !v.is_object() {
            self.warn.add("an entry in a list of children is not an object and was skipped");
            return None;
        }
        if is_true(v, "reusable") {
            if let Some(id) = v.get("id").and_then(Value::as_str) {
                if self.all.get(id).is_some_and(|indexed| std::ptr::eq(*indexed, v)) {
                    let found = self.component_node(id);
                    if found.is_none() {
                        self.warn.add(format!("component \"{id}\" refers to itself and was left out"));
                    }
                    return found;
                }
                self.warn.add(format!("id \"{id}\" is used more than once; the later reusable object is imported as an ordinary one"));
            }
        }
        self.convert_typed(v, ctx, false)
    }

    /// Converts `v` as a component when `as_component`.
    pub fn convert_typed(&mut self, v: &Value, ctx: &Ctx, as_component: bool) -> Option<NodeId> {
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let ctx = self.with_theme(v, ctx);
        let ctx = &ctx;
        let id = match ty {
            "frame" => self.frame_node(v, ctx, as_component),
            "group" => self.group_node(v, ctx, as_component),
            "rectangle" => self.rectangle_node(v, ctx, as_component),
            "ref" => return self.ref_node(v, ctx, as_component),
            "" => {
                self.warn.add("an object has no type and was skipped");
                return None;
            }
            _ => {
                let inner = self.leaf_node(v, ty, ctx);
                if as_component {
                    self.wrap_component(inner)
                } else {
                    inner
                }
            }
        };
        Some(id)
    }

    fn leaf_node(&mut self, v: &Value, ty: &str, ctx: &Ctx) -> NodeId {
        match ty {
            "ellipse" => self.ellipse_node(v, ctx),
            "polygon" => self.polygon_node(v, ctx),
            "path" => self.path_node(v, ctx),
            "text" => {
                let mut n = self.text_node(v, ctx, false);
                self.place(&mut n, v, ctx);
                self.put(n, Vec::new())
            }
            "note" | "prompt" | "context" => {
                self.warn.add("notes, prompts and context objects are imported as plain text boxes");
                let mut n = self.text_node(v, ctx, true);
                if v.get("name").and_then(Value::as_str).is_none_or(str::is_empty) {
                    n.name = ty[..1].to_uppercase() + &ty[1..];
                }
                self.place(&mut n, v, ctx);
                self.put(n, Vec::new())
            }
            "icon" => self.icon_node(v, ctx),
            "script" => self.placeholder_node(v, ctx, "script", "scripts are not run: the generated shapes are missing"),
            "browser" => self.placeholder_node(v, ctx, "browser", "live web pages are not shown"),
            other => {
                self.warn.add(format!("object type \"{other}\" is not known and was imported as an empty layer"));
                let mut n = self.base(v, NodeKind::Unknown { type_name: other.to_string() }, ctx);
                let (w, h) = (self.axis(v.get("width"), ctx, Mode::Fixed), self.axis(v.get("height"), ctx, Mode::Fixed));
                self.apply_size(&mut n, &w, &h, ctx);
                self.place(&mut n, v, ctx);
                self.put(n, Vec::new())
            }
        }
    }

    /// Wraps a non-frame object in a component frame of the same box.
    pub fn wrap_component(&mut self, inner: NodeId) -> NodeId {
        self.wrap(inner, |_| NodeKind::Component(FrameProps::default()))
    }

    /// Puts `inner` in a new frame-like node that takes over its box and place in a layout.
    pub fn wrap(&mut self, inner: NodeId, kind: impl FnOnce(&Node) -> NodeKind) -> NodeId {
        let id = self.doc.alloc_id();
        let Some(i) = self.doc.nodes.get_mut(&inner) else { return inner };
        let mut w = Node::new(id, kind(i));
        w.name = i.name.clone();
        w.source_id = i.source_id.clone();
        w.transform = i.transform;
        w.size = i.size;
        w.layout_child = i.layout_child;
        w.visible = i.visible;
        i.transform = Affine::IDENTITY;
        i.layout_child = LayoutChild::default();
        self.put(w, vec![inner])
    }

    // ------------------------------------------------------------------ containers

    fn frame_node(&mut self, v: &Value, ctx: &Ctx, as_component: bool) -> NodeId {
        let w = self.axis(v.get("width"), ctx, Mode::Hug);
        let h = self.axis(v.get("height"), ctx, Mode::Hug);
        let horizontal = match v.get("layout").and_then(Value::as_str) {
            Some("none") => None,
            Some("vertical") => Some(false),
            Some("horizontal") | None => Some(true),
            Some(other) => {
                self.warn.add(format!("layout \"{other}\" is not known; using horizontal"));
                Some(true)
            }
        };
        let mut bindings = Vec::new();
        let auto_layout = horizontal.map(|horiz| {
            let mut al = AutoLayout {
                direction: if horiz { LayoutDirection::Horizontal } else { LayoutDirection::Vertical },
                primary_sizing: if (if horiz { &w } else { &h }).mode == Mode::Hug { Sizing::Hug } else { Sizing::Fixed },
                counter_sizing: if (if horiz { &h } else { &w }).mode == Mode::Hug { Sizing::Hug } else { Sizing::Fixed },
                ..Default::default()
            };
            if let Some((gap, var)) = v.get("gap").and_then(|g| self.numv(g, ctx)) {
                al.item_spacing = gap.max(0.0);
                bindings.extend(var.map(|n| ("itemSpacing", var_id(&n))));
            }
            if let Some(p) = v.get("padding") {
                let (pad, vars) = self.padding(p, ctx);
                al.padding = pad;
                bindings.extend(vars);
            }
            al.primary_align = match v.get("justifyContent").and_then(Value::as_str) {
                Some("center") => LayoutAlign::Center,
                Some("end") => LayoutAlign::End,
                Some("space_between") => LayoutAlign::SpaceBetween,
                Some("space_around") => {
                    self.warn.add("justifyContent space_around is drawn as space_between");
                    LayoutAlign::SpaceBetween
                }
                _ => LayoutAlign::Start,
            };
            al.counter_align = match v.get("alignItems").and_then(Value::as_str) {
                Some("center") => LayoutAlign::Center,
                Some("end") => LayoutAlign::End,
                _ => LayoutAlign::Start,
            };
            al.stroke_included = v.get("layoutIncludeStroke").and_then(Value::as_bool).unwrap_or(false);
            al
        });
        let props = FrameProps { clips_content: v.get("clip").and_then(|c| self.flag(c, ctx)).unwrap_or(false), auto_layout, grids: Vec::new() };
        let kind = if as_component { NodeKind::Component(props) } else { NodeKind::Frame(props) };
        let mut n = self.base(v, kind, ctx);
        for (k, id) in bindings {
            n.variables.bindings.insert(k.into(), id);
        }
        self.apply_size(&mut n, &w, &h, ctx);
        self.appearance(&mut n, v, ctx, StrokeAlign::Inside);
        self.corner_radii(&mut n, v, ctx);
        self.place(&mut n, v, ctx);
        let mut cctx = self.child_ctx(v, ctx);
        cctx.parent_size = (if w.mode == Mode::Fixed { w.px } else { None }, if h.mode == Mode::Fixed { h.px } else { None });
        let kids = self.children(v, &cctx);
        self.put(n, kids)
    }

    /// `padding`: all sides, `[vertical, horizontal]` or `[top, right, bottom, left]`, as
    /// (top, right, bottom, left) plus the variable bindings.
    fn padding(&mut self, p: &Value, ctx: &Ctx) -> ([f32; 4], Vec<(&'static str, String)>) {
        const KEYS: [&str; 4] = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"];
        let items: Vec<&Value> = match p {
            Value::Array(a) => a.iter().collect(),
            single => vec![single],
        };
        let order: [usize; 4] = match items.len() {
            1 => [0, 0, 0, 0],
            2 => [0, 1, 0, 1],
            3 => [0, 1, 2, 1],
            _ => [0, 1, 2, 3],
        };
        let mut out = [0.0; 4];
        let mut bindings = Vec::new();
        for side in 0..4 {
            let Some(item) = items.get(order[side]) else { continue };
            if let Some((val, var)) = self.numv(item, ctx) {
                out[side] = val.max(0.0);
                bindings.extend(var.map(|n| (KEYS[side], var_id(&n))));
            }
        }
        (out, bindings)
    }

    fn group_node(&mut self, v: &Value, ctx: &Ctx, as_component: bool) -> NodeId {
        let kind = if as_component { NodeKind::Component(FrameProps::default()) } else { NodeKind::Group };
        let mut n = self.base(v, kind, ctx);
        if let Some(e) = v.get("effect") {
            n.effects = self.effects(e, ctx);
        }
        self.place(&mut n, v, ctx);
        let cctx = self.child_ctx(v, ctx);
        let kids = self.children(v, &cctx);
        if as_component {
            n.size = self.children_extent(&kids);
        }
        self.put(n, kids)
    }

    /// Bottom-right corner of the union of `kids` boxes (in their parent's space), from the origin.
    pub fn children_extent(&self, kids: &[NodeId]) -> Vec2 {
        let mut ext = Vec2::ZERO;
        for k in kids {
            if let Some(c) = self.doc.nodes.get(k) {
                let r = Rect::new(0.0, 0.0, c.size.x, c.size.y);
                for p in [Vec2::new(r.x, r.y), Vec2::new(r.w, r.y), Vec2::new(r.x, r.h), Vec2::new(r.w, r.h)] {
                    let q = c.transform.apply(p);
                    ext.x = ext.x.max(q.x);
                    ext.y = ext.y.max(q.y);
                }
            }
        }
        ext
    }

    // ------------------------------------------------------------------ shapes

    fn sized_shape(&mut self, v: &Value, kind: NodeKind, ctx: &Ctx, align: StrokeAlign) -> Node {
        let mut n = self.base(v, kind, ctx);
        let w = self.axis(v.get("width"), ctx, Mode::Fixed);
        let h = self.axis(v.get("height"), ctx, Mode::Fixed);
        self.apply_size(&mut n, &w, &h, ctx);
        self.appearance(&mut n, v, ctx, align);
        self.place(&mut n, v, ctx);
        n
    }

    fn rectangle_node(&mut self, v: &Value, ctx: &Ctx, as_component: bool) -> NodeId {
        let kind = if as_component { NodeKind::Component(FrameProps::default()) } else { NodeKind::Rectangle };
        let mut n = self.sized_shape(v, kind, ctx, StrokeAlign::Inside);
        self.corner_radii(&mut n, v, ctx);
        self.put(n, Vec::new())
    }

    fn ellipse_node(&mut self, v: &Value, ctx: &Ctx) -> NodeId {
        let mut arc = ArcData::default();
        let deg = |c: &Self, k: &str| v.get(k).and_then(|x| c.plain_num(x, ctx));
        if let Some(r) = deg(self, "innerRadius") {
            arc.inner_radius = r.clamp(0.0, 1.0);
        }
        let start = deg(self, "startAngle").unwrap_or(0.0);
        let sweep = deg(self, "sweepAngle").unwrap_or(360.0).clamp(-360.0, 360.0);
        if sweep.abs() < 360.0 {
            // `.pen` angles run counter-clockwise from the right; desigma's run clockwise.
            let (a, b) = (-start.to_radians(), -(start + sweep).to_radians());
            arc.start = a.min(b);
            arc.end = a.max(b);
        }
        let n = self.sized_shape(v, NodeKind::Ellipse(arc), ctx, StrokeAlign::Inside);
        self.put(n, Vec::new())
    }

    fn polygon_node(&mut self, v: &Value, ctx: &Ctx) -> NodeId {
        let count = v.get("polygonCount").and_then(|c| self.plain_num(c, ctx)).map(|c| c.round().clamp(3.0, 100.0) as u32).unwrap_or(3);
        let mut n = self.sized_shape(v, NodeKind::Polygon { count }, ctx, StrokeAlign::Inside);
        self.corner_radii(&mut n, v, ctx);
        self.put(n, Vec::new())
    }

    fn path_node(&mut self, v: &Value, ctx: &Ctx) -> NodeId {
        let geometry = v.get("geometry").and_then(Value::as_str).unwrap_or("");
        let rule = if v.get("fillRule").and_then(Value::as_str) == Some("evenodd") { FillRule::EvenOdd } else { FillRule::NonZero };
        let mut path = desigma_fig::svgpath::parse_path(geometry, rule);
        if path.cmds.is_empty() && !geometry.trim().is_empty() {
            self.warn.add("a path's geometry could not be read");
        }
        let vb = v
            .get("viewBox")
            .and_then(Value::as_array)
            .filter(|a| a.len() == 4)
            .map(|a| a.iter().map(|x| x.as_f64().map(clampf).unwrap_or(0.0)).collect::<Vec<f32>>());
        let bounds = match &vb {
            Some(b) => Rect::new(b[0], b[1], b[2], b[3]),
            None => path_bounds(&path),
        };
        let w = self.axis(v.get("width"), ctx, Mode::Fixed);
        let h = self.axis(v.get("height"), ctx, Mode::Fixed);
        // A missing width/height keeps the geometry's own size.
        let target = Vec2::new(w.px.unwrap_or(bounds.w), h.px.unwrap_or(bounds.h));
        let sx = if bounds.w > 0.0 { target.x / bounds.w } else { 1.0 };
        let sy = if bounds.h > 0.0 { target.y / bounds.h } else { 1.0 };
        let map = |p: Vec2| Vec2::new((p.x - bounds.x) * sx, (p.y - bounds.y) * sy);
        for c in &mut path.cmds {
            *c = match *c {
                PathCmd::MoveTo(a) => PathCmd::MoveTo(map(a)),
                PathCmd::LineTo(a) => PathCmd::LineTo(map(a)),
                PathCmd::QuadTo(a, b) => PathCmd::QuadTo(map(a), map(b)),
                PathCmd::CubicTo(a, b, c2) => PathCmd::CubicTo(map(a), map(b), map(c2)),
                PathCmd::Close => PathCmd::Close,
            };
        }
        let mut n = self.base(v, NodeKind::Vector { paths: vec![path], network: None }, ctx);
        let keep_geometry_size = |a: Axis, px: f32| if a.px.is_none() && a.mode == Mode::Fixed { Axis::fixed(Some(px)) } else { a };
        let (w, h) = (keep_geometry_size(w, target.x), keep_geometry_size(h, target.y));
        self.apply_size(&mut n, &w, &h, ctx);
        self.appearance(&mut n, v, ctx, StrokeAlign::Center);
        self.place(&mut n, v, ctx);
        self.put(n, Vec::new())
    }

    /// Icon fonts are not bundled: a dashed outline in the icon's color marks the spot.
    fn icon_node(&mut self, v: &Value, ctx: &Ctx) -> NodeId {
        let lib = v.get("library").and_then(|l| self.plain_string(l, ctx)).unwrap_or_default();
        let icon = v.get("icon").and_then(|l| self.plain_string(l, ctx)).unwrap_or_default();
        self.warn.add("icons from icon libraries are not bundled and became dashed placeholders");
        let mut n = self.base(v, NodeKind::Rectangle, ctx);
        if v.get("name").and_then(Value::as_str).is_none_or(str::is_empty) {
            n.name = format!("{lib}/{icon}");
        }
        let w = self.axis(v.get("width"), ctx, Mode::Fixed);
        let h = self.axis(v.get("height"), ctx, Mode::Fixed);
        self.apply_size(&mut n, &w, &h, ctx);
        let color = v.get("fill").map(|f| self.fills(f, ctx)).and_then(|f| f.into_iter().next()).map(|o| o.paint).unwrap_or_else(|| Paint::solid(Color::BLACK));
        n.stroke = Stroke { paints: vec![color], weight: 1.0, align: StrokeAlign::Inside, dashes: vec![3.0, 2.0], miter_limit: 4.0, ..Default::default() };
        if let Some(e) = v.get("effect") {
            n.effects = self.effects(e, ctx);
        }
        self.place(&mut n, v, ctx);
        self.put(n, Vec::new())
    }

    /// An empty clipping frame standing in for something that cannot be shown.
    fn placeholder_node(&mut self, v: &Value, ctx: &Ctx, what: &str, why: &str) -> NodeId {
        self.warn.add(format!("{what} objects: {why}"));
        let mut n = self.base(v, NodeKind::Frame(FrameProps { clips_content: true, ..Default::default() }), ctx);
        if v.get("name").and_then(Value::as_str).is_none_or(str::is_empty) {
            n.name = what[..1].to_uppercase() + &what[1..];
        }
        let w = self.axis(v.get("width"), ctx, Mode::Fixed);
        let h = self.axis(v.get("height"), ctx, Mode::Fixed);
        self.apply_size(&mut n, &w, &h, ctx);
        self.appearance(&mut n, v, ctx, StrokeAlign::Inside);
        self.corner_radii(&mut n, v, ctx);
        self.place(&mut n, v, ctx);
        self.put(n, Vec::new())
    }
}

/// Tight bounds of a path (curves are sampled).
fn path_bounds(p: &Path) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut add = |q: Vec2| {
        x0 = x0.min(q.x);
        y0 = y0.min(q.y);
        x1 = x1.max(q.x);
        y1 = y1.max(q.y);
    };
    let mut cur = Vec2::ZERO;
    for c in &p.cmds {
        match *c {
            PathCmd::MoveTo(a) | PathCmd::LineTo(a) => {
                add(a);
                cur = a;
            }
            PathCmd::QuadTo(a, b) => {
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    add(Vec2::new(u * u * cur.x + 2.0 * u * t * a.x + t * t * b.x, u * u * cur.y + 2.0 * u * t * a.y + t * t * b.y));
                }
                cur = b;
            }
            PathCmd::CubicTo(a, b, d) => {
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    let (c0, c1, c2, c3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    add(Vec2::new(c0 * cur.x + c1 * a.x + c2 * b.x + c3 * d.x, c0 * cur.y + c1 * a.y + c2 * b.y + c3 * d.y));
                }
                cur = d;
            }
            PathCmd::Close => {}
        }
    }
    if x0 > x1 {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
