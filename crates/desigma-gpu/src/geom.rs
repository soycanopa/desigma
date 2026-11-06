//! Per-node geometry cache: shapes, stroke outlines and glyph outlines
//! tessellated into meshes in node-local space.
//!
//! Meshes depend only on the node's geometry-relevant fields and on a zoom
//! bucket (curve flattening accuracy), never on the node's position, so moving
//! a node or panning re-uses them as they are.

use crate::mesh::{tessellate, Mesh};
use desigma_doc as d;
use desigma_doc::{Document, Node, NodeId, NodeKind, Vec2};
use desigma_render::tiny_skia as sk;
use desigma_render::{font_generation, outline_of, stroke_outline, stroke_plan, text_outlines, NodeShape, StrokeClip};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// The fields of a node that determine its geometry.
struct GeomSrc {
    kind: NodeKind,
    size: Vec2,
    radii: [f32; 4],
    smoothing: f32,
    fill_geometry: Option<Vec<d::Path>>,
    stroke_geometry: Option<Vec<d::Path>>,
    stroke: StrokeKey,
    font_gen: usize,
    /// Children that feed a boolean operation (recursively).
    kids: Vec<KidSrc>,
}

#[derive(Clone, PartialEq)]
struct StrokeKey {
    weight: f32,
    side_weights: Option<[f32; 4]>,
    align: d::StrokeAlign,
    cap: d::StrokeCap,
    join: d::StrokeJoin,
    miter_limit: f32,
    dashes: Vec<f32>,
}

impl StrokeKey {
    fn of(n: &Node) -> Self {
        let s = &n.stroke;
        Self { weight: s.weight, side_weights: s.side_weights, align: s.align, cap: s.cap, join: s.join, miter_limit: s.miter_limit, dashes: s.dashes.clone() }
    }
    fn same(&self, n: &Node) -> bool {
        let s = &n.stroke;
        self.weight == s.weight
            && self.side_weights == s.side_weights
            && self.align == s.align
            && self.cap == s.cap
            && self.join == s.join
            && self.miter_limit == s.miter_limit
            && self.dashes == s.dashes
    }
}

struct KidSrc {
    id: NodeId,
    visible: bool,
    transform: d::Affine,
    src: GeomSrc,
}

fn feeds_boolean(n: &Node) -> bool {
    matches!(n.kind, NodeKind::BooleanOp(_) | NodeKind::Group)
}

impl GeomSrc {
    fn build(doc: &Document, n: &Node, nested: bool) -> Self {
        let mut kids = Vec::new();
        if feeds_boolean(n) && (nested || matches!(n.kind, NodeKind::BooleanOp(_))) {
            for c in &n.children {
                if let Some(child) = doc.get(*c) {
                    kids.push(KidSrc { id: *c, visible: child.visible, transform: child.transform, src: GeomSrc::build(doc, child, true) });
                }
            }
        }
        Self {
            kind: n.kind.clone(),
            size: n.size,
            radii: n.corner_radii,
            smoothing: n.corner_smoothing,
            fill_geometry: n.fill_geometry.clone(),
            stroke_geometry: n.stroke_geometry.clone(),
            stroke: StrokeKey::of(n),
            font_gen: font_generation(),
            kids,
        }
    }

    fn same(&self, doc: &Document, n: &Node, nested: bool) -> bool {
        if self.kind != n.kind
            || self.size != n.size
            || self.radii != n.corner_radii
            || self.smoothing != n.corner_smoothing
            || self.fill_geometry != n.fill_geometry
            || self.stroke_geometry != n.stroke_geometry
            || !self.stroke.same(n)
            || (matches!(n.kind, NodeKind::Text(_)) && self.font_gen != font_generation())
        {
            return false;
        }
        let want_kids = feeds_boolean(n) && (nested || matches!(n.kind, NodeKind::BooleanOp(_)));
        if !want_kids {
            return self.kids.is_empty();
        }
        if self.kids.len() != n.children.len() {
            return false;
        }
        self.kids.iter().zip(&n.children).all(|(k, c)| {
            k.id == *c && doc.get(*c).is_some_and(|child| child.visible == k.visible && child.transform == k.transform && k.src.same(doc, child, true))
        })
    }
}

/// Glyph outlines of one text style.
pub struct TextGeom {
    pub mesh: Arc<Mesh>,
    pub fills: Vec<d::Paint>,
    path: sk::Path,
    /// Centered stroke outline of the glyphs (text strokes are always centered).
    stroke: RefCell<Option<Option<Arc<Mesh>>>>,
}

type StrokeMesh = Option<(Arc<Mesh>, StrokeClip)>;

static NEXT_GEOM_ID: AtomicU64 = AtomicU64::new(1);

/// Cached geometry of one node at one zoom bucket.
pub struct NodeGeom {
    /// Unique per build: equal ids mean identical meshes (cache keys of rendered results).
    pub id: u64,
    pub shape: Option<NodeShape>,
    /// Fill mesh of `shape.paths`.
    pub fill: Option<Arc<Mesh>>,
    stroke: RefCell<Option<StrokeMesh>>,
    pub text: Vec<TextGeom>,
    /// Device scale the meshes were flattened for.
    pub scale: f32,
    lazy: RefCell<Lazy>,
}

#[derive(Default)]
struct Lazy {
    /// (width bits, mesh) of silhouette outsets and growth outlines.
    outlines: Vec<(u32, Option<Arc<Mesh>>)>,
}

impl NodeGeom {
    /// Stroke outline mesh and how it is limited to the shape (built on first use).
    pub fn stroke_mesh(&self, n: &Node) -> Option<(Arc<Mesh>, StrokeClip)> {
        if let Some(c) = self.stroke.borrow().as_ref() {
            return c.clone();
        }
        let built = self
            .shape
            .as_ref()
            .and_then(|sh| stroke_plan(n, sh, self.scale))
            .and_then(|plan| tessellate(&plan.outline, 0.1 / self.scale).map(|m| (Arc::new(m), plan.clip)));
        *self.stroke.borrow_mut() = Some(built.clone());
        built
    }

    /// Centered stroke outline of text style `i`.
    pub fn text_stroke(&self, i: usize, n: &Node) -> Option<Arc<Mesh>> {
        let t = self.text.get(i)?;
        if let Some(c) = t.stroke.borrow().as_ref() {
            return c.clone();
        }
        let st = desigma_render::node_stroke_style(n, n.stroke.weight);
        let built = stroke_outline(&t.path, &st, self.scale).and_then(|p| tessellate(&[(p, sk::FillRule::Winding)], 0.1 / self.scale)).map(Arc::new);
        *t.stroke.borrow_mut() = Some(built.clone());
        built
    }

    /// Mitered outline `width` wide around the shape's paths (drop-shadow
    /// silhouettes: stroke footprint and spread). Cached per width.
    pub fn grow_outline(&self, width: f32) -> Option<Arc<Mesh>> {
        let sh = self.shape.as_ref()?;
        let key = width.to_bits();
        if let Some((_, m)) = self.lazy.borrow().outlines.iter().find(|(k, _)| *k == key) {
            return m.clone();
        }
        let tol = 0.1 / self.scale;
        let outline: Vec<_> =
            sh.paths.iter().filter_map(|(p, _)| desigma_render::miter_outline(p, width, self.scale)).map(|p| (p, sk::FillRule::Winding)).collect();
        let mesh = tessellate(&outline, tol).map(Arc::new);
        self.lazy.borrow_mut().outlines.push((key, mesh.clone()));
        mesh
    }

    /// Mesh of a precomputed stroke outline used as silhouette footprint.
    pub fn stroke_paths_mesh(&self) -> Option<Arc<Mesh>> {
        let sh = self.shape.as_ref()?;
        let sp = sh.stroke_paths.as_ref()?;
        let key = u32::MAX;
        if let Some((_, m)) = self.lazy.borrow().outlines.iter().find(|(k, _)| *k == key) {
            return m.clone();
        }
        let mesh = tessellate(sp, 0.1 / self.scale).map(Arc::new);
        self.lazy.borrow_mut().outlines.push((key, mesh.clone()));
        mesh
    }
}

struct Entry {
    src: GeomSrc,
    bucket: i32,
    rev: u64,
    geom: Rc<NodeGeom>,
}

/// Cache of [`NodeGeom`] per node.
#[derive(Default)]
pub struct GeomCache {
    entries: HashMap<NodeId, Entry>,
    /// Statistics: geometry builds since creation.
    pub builds: u64,
    last_trim: u64,
    /// Time one frame may spend re-flattening geometry for a new zoom bucket;
    /// past it, nodes keep their previous (coarser or finer) meshes until the
    /// next frame. `None`: unlimited.
    pub budget: Option<std::time::Duration>,
    frame_start: Option<std::time::Instant>,
    /// Nodes that kept stale-bucket geometry in the current frame.
    pub stale_hits: u32,
}

/// Zoom bucket for a device scale: the power of two at or above it.
pub fn bucket_of(scale: f32) -> i32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    (s.log2().ceil() as i32).clamp(-8, 10)
}

pub fn bucket_scale(bucket: i32) -> f32 {
    (bucket as f32).exp2()
}

impl GeomCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Starts a frame: resets the re-tessellation budget.
    pub fn begin_frame(&mut self) {
        self.frame_start = Some(std::time::Instant::now());
        self.stale_hits = 0;
    }

    fn over_budget(&self) -> bool {
        match (self.budget, self.frame_start) {
            (Some(b), Some(s)) => s.elapsed() > b,
            _ => false,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Geometry of `n` (already variable-resolved) for the given bucket.
    /// `rev` is the document revision: unchanged entries are not re-checked
    /// within the same revision.
    pub fn get(&mut self, doc: &Document, n: &Node, bucket: i32, rev: u64) -> Rc<NodeGeom> {
        let over = self.over_budget();
        if let Some(e) = self.entries.get_mut(&n.id) {
            if e.bucket == bucket && e.rev == rev {
                return e.geom.clone();
            }
            if e.bucket != bucket && e.rev == rev && over {
                // same document state, only the zoom bucket moved: reuse until budget allows
                self.stale_hits += 1;
                return e.geom.clone();
            }
            if e.src.same(doc, n, false) {
                if e.bucket == bucket {
                    e.rev = rev;
                    return e.geom.clone();
                }
                if over {
                    e.rev = rev;
                    self.stale_hits += 1;
                    return e.geom.clone();
                }
            }
        }
        self.builds += 1;
        let scale = bucket_scale(bucket);
        let geom = Rc::new(build_geom(doc, n, scale));
        self.entries.insert(n.id, Entry { src: GeomSrc::build(doc, n, false), bucket, rev, geom: geom.clone() });
        geom
    }

    /// Drops entries not touched since `rev - keep` revisions (nodes deleted from
    /// the document, or far from the viewport for a long time).
    pub fn trim(&mut self, rev: u64) {
        if self.entries.len() > 20_000 && rev != self.last_trim {
            self.last_trim = rev;
            self.entries.retain(|_, e| e.rev + 64 >= rev);
        }
    }
}

fn build_geom(doc: &Document, n: &Node, scale: f32) -> NodeGeom {
    let tol = 0.1 / scale;
    let id = NEXT_GEOM_ID.fetch_add(1, Ordering::Relaxed);
    let mut g = NodeGeom { id, shape: None, fill: None, stroke: RefCell::default(), text: Vec::new(), scale, lazy: RefCell::default() };
    if let NodeKind::Text(tp) = &n.kind {
        for o in text_outlines(tp, n.size) {
            let Some(mesh) = tessellate(&[(o.path.clone(), sk::FillRule::Winding)], tol) else { continue };
            g.text.push(TextGeom { mesh: Arc::new(mesh), fills: o.fills, path: o.path, stroke: RefCell::default() });
        }
        return g;
    }
    // boolean operations flatten with a tolerance tied to the device scale
    let Some(shape) = outline_of(doc, n, 0.15 / scale) else { return g };
    g.fill = tessellate(&shape.paths, tol).map(Arc::new);
    g.shape = Some(shape);
    g
}
