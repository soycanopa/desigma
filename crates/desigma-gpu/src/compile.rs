//! Scene compiler: walks the page exactly like the CPU renderer's
//! `draw_children` / `draw_node` / `draw_body` and records a [`Frame`].
//!
//! Where the CPU renderer draws into a pixmap, this records draws into the
//! current *program*; where it allocates a layer pixmap, this opens a new
//! program (bounded to the node's pixels) and composites it afterwards.

use crate::frame::*;
use crate::geom::{bucket_of, GeomCache, NodeGeom};
use crate::mesh::Mesh;
use crate::paintconv::{PaintCache, PaintInst};
use desigma_doc as d;
use desigma_doc::{resolve_node, Document, Effect, Node, NodeId, NodeKind, Vec2};
use desigma_render::board;
use desigma_render::tiny_skia as sk;
use desigma_render::{node_margin, render_node_into, SceneIndex, StrokeClip, Viewport};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Decides which nodes the CPU renderer draws instead of the GPU.
pub type FallbackFn = Arc<dyn Fn(&Node) -> bool + Send + Sync>;

/// Compile-time knobs.
#[derive(Clone, Default)]
pub struct CompileOptions {
    /// Largest texture edge the device supports (0 = unlimited).
    pub max_texture: u32,
    /// Nodes for which this returns true are rendered by the CPU renderer into
    /// a texture instead of by the GPU. Used by tests and as an escape hatch.
    pub force_fallback: Option<FallbackFn>,
    /// Render at this multiple of the output size and box-filter down (1 = off).
    /// 2 gives 16 samples per pixel together with the 4x MSAA.
    pub supersample: u32,
}

/// Persistent compiler state: geometry, paint and index caches.
#[derive(Default)]
pub struct SceneCompiler {
    pub geom: GeomCache,
    paints: PaintCache,
    index: SceneIndex,
    pub opts: CompileOptions,
    unit_rect: Option<Arc<Mesh>>,
}

impl SceneCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compiles the visible part of `page` seen through `vp` into a frame of
    /// `size` device pixels. `rev` must change whenever the document does.
    pub fn compile(&mut self, doc: &Document, page: NodeId, rev: u64, vp: &Viewport, size: (u32, u32)) -> Frame {
        self.index.ensure(doc, page, rev);
        let s = vp.zoom * vp.dpr;
        let ts = sk::Transform::from_row(s, 0.0, 0.0, s, -vp.offset.x * s, -vp.offset.y * s);
        self.compile_with(doc, page, rev, ts, size, false)
    }

    /// Compiles one node (a page: its children over the page background) with
    /// its own top-left at (0, 0), scaled by `scale`; the counterpart of
    /// `desigma_render::render_node` (used for Present frames and exports).
    pub fn compile_node(&mut self, doc: &Document, node: NodeId, rev: u64, scale: f32, size: (u32, u32)) -> Frame {
        let ts = sk::Transform::from_scale(scale, scale);
        self.compile_with(doc, node, rev, ts, size, true)
    }

    fn compile_with(&mut self, doc: &Document, root_node: NodeId, rev: u64, logical_ts: sk::Transform, size: (u32, u32), single: bool) -> Frame {
        self.geom.trim(rev);
        self.geom.begin_frame();
        let unit = self.unit_rect.get_or_insert_with(|| Arc::new(Mesh::rect(0.0, 0.0, 1.0, 1.0))).clone();
        let ss = self.opts.supersample.clamp(1, 4);
        let clampi = |v: f32| if v.is_finite() { v.clamp(-1.0e9, 1.0e9) as i32 } else { 0 };
        let grid = (clampi(-logical_ts.tx.round()), clampi(-logical_ts.ty.round()));
        let root_ts = logical_ts.post_scale(ss as f32, ss as f32);
        let out_size = size;
        let size = (size.0 * ss, size.1 * ss);
        let root_rect = IRect::new(0, 0, size.0 as i32, size.1 as i32);
        let mut cx = Ctx {
            doc,
            geom: &mut self.geom,
            paints: &mut self.paints,
            index: &self.index,
            opts: &self.opts,
            rev,
            root_ts,
            root_rect,
            grid,
            progs: Vec::new(),
            stack: Vec::new(),
            clips: Vec::new(),
            clip_ids: HashMap::new(),
            unit,
            cull: !single,
            ss,
        };
        let node = doc.get(root_node).map(|n| resolve_node(doc, n).into_owned());
        let bg = match node.as_ref().map(|n| &n.kind) {
            Some(NodeKind::Page { background }) => [background.r, background.g, background.b, background.a],
            _ if single => [0.0; 4],
            _ => [0xF5 as f32 / 255.0, 0xF5 as f32 / 255.0, 0xF5 as f32 / 255.0, 1.0],
        };
        let mut root = Program::new(root_rect);
        root.clear = [bg[0].clamp(0.0, 1.0) * bg[3], bg[1].clamp(0.0, 1.0) * bg[3], bg[2].clamp(0.0, 1.0) * bg[3], bg[3].clamp(0.0, 1.0)];
        cx.progs.push(root);
        cx.stack.push(0);
        let full = cx.clip_for(ClipDesc { scissor: root_rect, mask: None });
        if let Some(n) = node {
            if single && !matches!(n.kind, NodeKind::Page { .. }) {
                cx.draw_node(&n, root_ts, full);
            } else {
                let children = n.children.clone();
                cx.draw_children(&children, root_ts, full);
            }
        }
        Frame { size, out_size, ss, root: 0, programs: cx.progs, clips: cx.clips }
    }
}

// ---------------------------------------------------------------- helpers

type FBox = (f32, f32, f32, f32);

fn xform_bounds(ts: &sk::Transform, x: f32, y: f32, w: f32, h: f32) -> FBox {
    let pts = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (px, py) in pts {
        let tx = ts.sx * px + ts.kx * py + ts.tx;
        let ty = ts.ky * px + ts.sy * py + ts.ty;
        x0 = x0.min(tx);
        y0 = y0.min(ty);
        x1 = x1.max(tx);
        y1 = y1.max(ty);
    }
    (x0, y0, x1, y1)
}

fn ts_scale(ts: &sk::Transform) -> f32 {
    (ts.sx * ts.sy - ts.kx * ts.ky).abs().sqrt().max(1e-6)
}

fn to_sk(a: &d::Affine) -> sk::Transform {
    sk::Transform::from_row(a.a, a.b, a.c, a.d, a.tx, a.ty)
}

fn hits(bb: FBox, r: IRect) -> bool {
    !(r.is_empty() || bb.2 < r.x0 as f32 || bb.3 < r.y0 as f32 || bb.0 > r.x1 as f32 || bb.1 > r.y1 as f32)
}

fn draws_children(n: &Node) -> bool {
    !n.children.is_empty() && !matches!(n.kind, NodeKind::BooleanOp(_))
}

fn special_blend(b: d::BlendMode) -> bool {
    !matches!(b, d::BlendMode::Normal | d::BlendMode::PassThrough)
}

fn visible_fills(n: &Node) -> Vec<&d::Paint> {
    match &n.kind {
        NodeKind::Text(t) => t.style.fills.iter().filter(|p| p.visible).collect(),
        _ => n.fills.iter().filter(|p| p.visible).collect(),
    }
}

fn visible_strokes(n: &Node) -> Vec<&d::Paint> {
    if n.stroke.weight <= 0.0 && n.stroke.side_weights.is_none() {
        return Vec::new();
    }
    n.stroke.paints.iter().filter(|p| p.visible).collect()
}

fn identity_like(ts: &sk::Transform) -> bool {
    ts.kx.abs() < 1e-5 && ts.ky.abs() < 1e-5 && ts.sx > 0.0 && ts.sy > 0.0
}

struct Ctx<'a> {
    doc: &'a Document,
    geom: &'a mut GeomCache,
    paints: &'a mut PaintCache,
    index: &'a SceneIndex,
    opts: &'a CompileOptions,
    rev: u64,
    root_ts: sk::Transform,
    root_rect: IRect,
    grid: (i32, i32),
    progs: Vec<Program>,
    stack: Vec<ProgId>,
    clips: Vec<ClipDesc>,
    clip_ids: HashMap<ClipDesc, ClipId>,
    unit: Arc<Mesh>,
    /// Index-based culling is valid (not while drawing offset silhouettes).
    cull: bool,
    /// Supersampling factor of the target.
    ss: u32,
}

impl Ctx<'_> {
    // ----- programs and clips ---------------------------------------------

    fn cur(&self) -> ProgId {
        *self.stack.last().expect("program stack")
    }

    fn push_op(&mut self, op: Op) {
        let c = self.cur() as usize;
        self.progs[c].ops.push(op);
    }

    fn new_prog(&mut self, mut rect: IRect) -> ProgId {
        // never ask for a texture bigger than the device allows (extreme blur radii)
        let max = self.opts.max_texture as i32;
        if max > 0 {
            rect.x1 = rect.x1.min(rect.x0 + max);
            rect.y1 = rect.y1.min(rect.y0 + max);
        }
        self.progs.push(Program::new(rect));
        (self.progs.len() - 1) as ProgId
    }

    fn clip_for(&mut self, c: ClipDesc) -> ClipId {
        if let Some(id) = self.clip_ids.get(&c) {
            return *id;
        }
        let id = self.clips.len() as ClipId;
        self.clips.push(c);
        self.clip_ids.insert(c, id);
        id
    }

    fn full_clip(&mut self, rect: IRect) -> ClipId {
        self.clip_for(ClipDesc { scissor: rect, mask: None })
    }

    /// Area of the current program still visible through `clip`.
    fn visible(&self, clip: ClipId) -> IRect {
        self.progs[self.cur() as usize].rect.intersect(self.clips[clip as usize].scissor)
    }

    fn with_prog<R>(&mut self, id: ProgId, f: impl FnOnce(&mut Self) -> R) -> R {
        self.stack.push(id);
        let r = f(self);
        self.stack.pop();
        r
    }

    fn narrow_clip(&mut self, clip: ClipId, rect: IRect) -> ClipId {
        let c = self.clips[clip as usize];
        let s = c.scissor.intersect(rect);
        self.clip_for(ClipDesc { scissor: s, mask: c.mask })
    }

    /// A mask program covering `mesh` (through `ts`) restricted by `parent`;
    /// returns the clip that limits draws to it (`invert`: to its complement
    /// within `roi`). `None` when nothing can remain visible.
    fn mask_clip(&mut self, mesh: &Arc<Mesh>, ts: sk::Transform, parent: ClipId, invert: bool, roi: IRect) -> Option<ClipId> {
        let pdesc = self.clips[parent as usize];
        let bb = xform_bounds(&ts, mesh.bounds.0, mesh.bounds.1, mesh.bounds.2 - mesh.bounds.0, mesh.bounds.3 - mesh.bounds.1);
        let region = if invert { roi } else { IRect::outward(bb).intersect(roi) };
        let region = region.intersect(pdesc.scissor).intersect(self.progs[self.cur() as usize].rect);
        if region.is_empty() {
            return None;
        }
        let prog = self.new_prog(region);
        let white = solid_inst([1.0, 1.0, 1.0, 1.0]);
        let inner = self.full_clip(region);
        let with_parent = self.clip_for(ClipDesc { scissor: region, mask: pdesc.mask });
        self.with_prog(prog, |c| {
            if invert {
                // everything inside the parent clip ...
                c.draw_quad(region, white.clone(), DrawBlend::Over, with_parent);
                // ... minus the shape
                c.draw_mesh(mesh, ts, white, DrawBlend::Erase, inner);
            } else {
                c.draw_mesh(mesh, ts, white, DrawBlend::Over, with_parent);
            }
        });
        Some(self.clip_for(ClipDesc { scissor: region, mask: Some(prog) }))
    }

    // ----- low level draws --------------------------------------------------

    fn draw_mesh(&mut self, mesh: &Arc<Mesh>, ts: sk::Transform, mut pi: PaintInst, blend: DrawBlend, clip: ClipId) {
        let bb = xform_bounds(&ts, mesh.bounds.0, mesh.bounds.1, mesh.bounds.2 - mesh.bounds.0, mesh.bounds.3 - mesh.bounds.1);
        if mesh.is_empty() || !hits(bb, self.visible(clip)) {
            return;
        }
        pi.inst.m = [ts.sx, ts.ky, ts.kx, ts.sy];
        pi.inst.t[0] = ts.tx;
        pi.inst.t[1] = ts.ty;
        self.push_op(Op::Draw(Box::new(DrawOp { mesh: mesh.clone(), inst: pi.inst, blend, clip, image: pi.image, ramp: pi.ramp })));
    }

    /// Fills `rect` (root pixels) with a paint.
    fn draw_quad(&mut self, rect: IRect, pi: PaintInst, blend: DrawBlend, clip: ClipId) {
        let ts = sk::Transform::from_row(rect.w() as f32, 0.0, 0.0, rect.h() as f32, rect.x0 as f32, rect.y0 as f32);
        let unit = self.unit.clone();
        self.draw_mesh(&unit, ts, pi, blend, clip);
    }

    // ----- tree -------------------------------------------------------------

    fn draw_children(&mut self, children: &[NodeId], parent_ts: sk::Transform, clip: ClipId) {
        let doc = self.doc;
        for (i, cid) in children.iter().enumerate() {
            let Some(child) = board::get(doc, *cid) else { continue };
            let child = resolve_node(doc, &child);
            let child: &Node = &child;
            // lowered board children are not in the scene index
            if self.cull && !child.is_mask && !board::is_synthetic(*cid) {
                let vis = self.visible(clip);
                match self.index.bounds_of(*cid) {
                    Some(b) => {
                        let bb = xform_bounds(&self.root_ts, b.0, b.1, b.2 - b.0, b.3 - b.1);
                        if !hits(bb, vis) {
                            continue;
                        }
                    }
                    None => continue,
                }
            }
            let ts = parent_ts.pre_concat(to_sk(&child.transform));
            if child.is_mask {
                if !child.visible {
                    continue;
                }
                self.draw_mask(child, children, i, parent_ts, ts, clip);
                return;
            }
            self.draw_node(child, ts, clip);
        }
    }

    /// A mask node clips the siblings that follow it.
    fn draw_mask(&mut self, mask: &Node, siblings: &[NodeId], i: usize, parent_ts: sk::Transform, ts: sk::Transform, clip: ClipId) {
        let doc = self.doc;
        let vis = self.visible(clip);
        let mut rect = vis;
        if self.cull {
            if let Some(b) = self.index.bounds_of(mask.id) {
                rect = rect.intersect(IRect::outward(xform_bounds(&self.root_ts, b.0, b.1, b.2 - b.0, b.3 - b.1)).grow(1));
            }
        }
        if rect.is_empty() {
            return;
        }
        let rest = &siblings[i + 1..];
        let needs_back = rest.iter().any(|c| doc.get(*c).is_some_and(|c| self.has_bg_blur(c)));
        let layer = self.new_prog(rect);
        self.progs[layer as usize].needs_backdrop = needs_back;
        let lclip = self.narrow_clip(clip, rect);
        self.with_prog(layer, |c| c.draw_children(rest, parent_ts, lclip));
        let mut mask_node = mask.clone();
        mask_node.blend = d::BlendMode::Normal;
        mask_node.effects.clear();
        mask_node.stroke.paints.clear();
        if !matches!(mask_node.kind, NodeKind::Text(_) | NodeKind::Group) && !mask_node.fills.iter().any(|p| p.visible) {
            mask_node.fills = vec![d::Paint::solid(d::Color::BLACK)];
        }
        let mprog = self.new_prog(rect);
        let mclip = self.full_clip(rect);
        self.with_prog(mprog, |c| c.draw_node(&mask_node, ts, mclip));
        let pclip = self.full_clip(rect);
        self.push_op(Op::Composite(Box::new(CompositeOp {
            src: layer,
            rect,
            opacity: 1.0,
            tint: None,
            aux: Some((mprog, AuxMode::Mul)),
            mode: CompMode::Over,
            clip: pclip,
        })));
    }

    /// Whether `n` or any drawn descendant has a visible background blur.
    fn has_bg_blur(&self, n: &Node) -> bool {
        if !n.visible {
            return false;
        }
        n.effects.iter().any(|e| matches!(e, Effect::BackgroundBlur { visible: true, radius } if *radius > 0.0) || matches!(e, Effect::Glass(g) if g.visible))
            || (draws_children(n) && n.children.iter().any(|c| self.doc.get(*c).is_some_and(|c| self.has_bg_blur(c))))
    }

    fn bbox(&self, n: &Node, ts: &sk::Transform) -> FBox {
        let m = node_margin(n);
        xform_bounds(ts, -m, -m, n.size.x + 2.0 * m, n.size.y + 2.0 * m)
    }

    /// Pixels a layer for `n` needs: its drawn subtree (from the index) grown
    /// by `pad`, limited to what is visible (grown by `pad` too).
    fn layer_rect(&self, n: &Node, ts: &sk::Transform, clip: ClipId, pad: i32) -> IRect {
        let vis = self.visible(clip).grow(pad);
        let sub = if self.cull {
            match self.index.bounds_of(n.id) {
                Some(b) => IRect::outward(xform_bounds(&self.root_ts, b.0, b.1, b.2 - b.0, b.3 - b.1)).grow(1 + pad),
                None => vis,
            }
        } else {
            let bb = self.bbox(n, ts);
            let own = IRect::outward(bb).grow(1 + pad);
            if draws_children(n) {
                vis
            } else {
                own
            }
        };
        sub.intersect(vis)
    }

    fn draw_node(&mut self, n: &Node, ts: sk::Transform, clip: ClipId) {
        if !n.visible || matches!(n.kind, NodeKind::Unknown { .. } | NodeKind::Page { .. }) {
            return;
        }
        if board::lowers(self.doc, n) {
            // stickies, connectors, shapes with text, stamps and code blocks draw as their lowered form
            if let Some(body) = board::lower(self.doc, n) {
                self.draw_node(&body, ts, clip);
            }
            return;
        }
        // washi tape is drawn by the CPU renderer
        if matches!(n.brush, Some(desigma_doc::Brush::Washi(_))) && !self.has_bg_blur(n) {
            self.fallback(n, ts, clip);
            return;
        }
        if let Some(f) = &self.opts.force_fallback {
            if f(n) && !self.has_bg_blur(n) {
                self.fallback(n, ts, clip);
                return;
            }
        }
        let clips_content = n.kind.frame_props().map(|f| f.clips_content).unwrap_or(false);
        let bb = self.bbox(n, &ts);
        let has_kids = draws_children(n);
        let vis = self.visible(clip);
        if (!has_kids || clips_content) && !hits(bb, vis) {
            return;
        }
        let paints = visible_fills(n).len() + visible_strokes(n).len();
        let has_effects = n.effects.iter().any(|e| {
            matches!(e, Effect::DropShadow { visible: true, .. } | Effect::InnerShadow { visible: true, .. } | Effect::BackgroundBlur { visible: true, .. })
                || matches!(e, Effect::Glass(g) if g.visible)
                || matches!(e, Effect::Noise(n) if n.visible)
        });
        let textured = n.effects.iter().any(|e| matches!(e, Effect::Texture(t) if t.visible && t.radius > 0.0));
        let layer_blur = n
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::LayerBlur { radius, visible: true } if *radius > 0.0 => Some(*radius),
                _ => None,
            })
            .fold(0.0f32, f32::max);
        let special = special_blend(n.blend);
        let opacity = n.opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 && !special {
            return;
        }
        let need_layer = special || layer_blur > 0.0 || textured || (opacity < 1.0 && (has_kids || paints > 1 || has_effects));
        if !need_layer {
            self.draw_body(n, ts, clip, opacity);
            return;
        }
        let sigma = layer_blur * ts_scale(&ts) / 2.0;
        let pad = if layer_blur > 0.0 { (3.0 * sigma).ceil() as i32 + 1 } else { 0 };
        let tex_pad = n
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Texture(t) if t.visible && t.radius > 0.0 => Some((t.radius * ts_scale(&ts)).ceil() as i32 + 3),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let rect = self.layer_rect(n, &ts, clip, pad.max(tex_pad));
        if rect.is_empty() {
            return;
        }
        let layer = self.new_prog(rect);
        self.progs[layer as usize].needs_backdrop = self.has_bg_blur(n);
        // draws inside the layer keep the parent's clip (mask and scissor)
        let lclip = self.narrow_clip(clip, rect);
        self.with_prog(layer, |c| c.draw_body(n, ts, lclip, 1.0));
        if layer_blur > 0.0 {
            self.progs[layer as usize].post.push(Post::Blur { sigma, clamp: false });
        }
        let mode = if special { CompMode::Blend(n.blend) } else { CompMode::Over };
        let pclip = self.full_clip(rect);
        self.push_op(Op::Composite(Box::new(CompositeOp { src: layer, rect, opacity, tint: None, aux: None, mode, clip: pclip })));
    }

    /// CPU-rendered replacement for a whole node (never drops content).
    fn fallback(&mut self, n: &Node, ts: sk::Transform, clip: ClipId) {
        let rect = self.layer_rect(n, &ts, clip, 0);
        let (w, h) = (rect.w() as u32, rect.h() as u32);
        if w == 0 || h == 0 {
            return;
        }
        let Some(mut pm) = sk::Pixmap::new(w, h) else { return };
        let Some(inv) = to_sk(&n.transform).invert() else { return };
        let parent_ts = ts.pre_concat(inv).post_translate(-(rect.x0 as f32), -(rect.y0 as f32));
        render_node_into(self.doc, n.id, parent_ts, self.root_ts.post_translate(-(rect.x0 as f32), -(rect.y0 as f32)), &mut pm);
        let prog = self.new_prog(rect);
        self.progs[prog as usize].pixels = Some(Arc::new(PixelData { w, h, rgba: pm.data().to_vec() }));
        let mode = if special_blend(n.blend) { CompMode::Blend(n.blend) } else { CompMode::Over };
        // the node's clip applies to the whole fallback image
        self.push_op(Op::Composite(Box::new(CompositeOp { src: prog, rect, opacity: 1.0, tint: None, aux: None, mode, clip })));
    }

    // ----- node body --------------------------------------------------------

    fn draw_body(&mut self, n: &Node, ts: sk::Transform, clip: ClipId, alpha: f32) {
        self.draw_body_inner(n, ts, clip, alpha);
        for e in &n.effects {
            let Effect::Texture(t) = e else { continue };
            if !t.visible || t.radius <= 0.0 {
                continue;
            }
            let scale = ts_scale(&ts);
            let amp = t.radius.max(0.0) * scale;
            let g = self.geom_of(n, &ts);
            let rect = self.progs[self.cur() as usize].rect;
            let clip_to = if t.clip_to_shape {
                let r = IRect::outward(self.bbox(n, &ts)).grow(amp.ceil() as i32 + 3).intersect(rect);
                (!r.is_empty()).then(|| self.sil_program(n, &g, ts, r, 0.0, 0.0, 0.0, true))
            } else {
                None
            };
            let cell = (t.size.max(0.1) * scale / self.ss as f32).max(1.0);
            let c = self.cur() as usize;
            self.progs[c].post.push(Post::Displace { amp, cell, seed: t.seed, grid: self.grid, clip_to, ss: self.ss });
        }
    }

    fn geom_of(&mut self, n: &Node, ts: &sk::Transform) -> Rc<NodeGeom> {
        let bucket = bucket_of(ts_scale(ts));
        self.geom.get(self.doc, n, bucket, self.rev)
    }

    fn draw_body_inner(&mut self, n: &Node, ts: sk::Transform, clip: ClipId, alpha: f32) {
        let scale = ts_scale(&ts);
        let g = self.geom_of(n, &ts);
        let bb = self.bbox(n, &ts);
        let inner_bb = xform_bounds(&ts, 0.0, 0.0, n.size.x, n.size.y);

        // 1. drop shadows + background blur (behind the node)
        for e in &n.effects {
            match e {
                Effect::DropShadow { color, offset, radius, spread, visible: true, show_behind } => {
                    self.drop_shadow(n, &g, ts, clip, bb, scale, color, *offset, *radius, *spread, *show_behind, alpha);
                }
                Effect::BackgroundBlur { radius, visible: true } if *radius > 0.0 => {
                    self.background_blur(n, &g, ts, clip, inner_bb, scale, *radius);
                }
                Effect::Glass(gl) if gl.visible => self.glass(n, &g, ts, clip, inner_bb, scale, gl),
                _ => {}
            }
        }

        // 2. fills
        if matches!(n.kind, NodeKind::Text(_)) {
            self.draw_text(n, &g, ts, clip, alpha);
        } else if let Some(sh) = &g.shape {
            if sh.fillable {
                if let Some(mesh) = &g.fill {
                    for p in visible_fills(n) {
                        self.fill_with(mesh, ts, n.size, p, alpha, clip);
                    }
                }
            }
        }

        // 3. inner shadows
        for e in &n.effects {
            if let Effect::InnerShadow { color, offset, radius, spread, visible: true } = e {
                self.inner_shadow(n, &g, ts, clip, bb, scale, color, *offset, *radius, *spread, alpha);
            }
        }

        // 3b. noise grain over the fill
        for e in &n.effects {
            if let Effect::Noise(nz) = e {
                if nz.visible {
                    self.noise(n, &g, ts, clip, bb, scale, nz, alpha);
                }
            }
        }

        // 4. strokes
        if g.shape.is_some() {
            self.draw_strokes(n, &g, ts, clip, alpha);
        }

        // 5. children
        if draws_children(n) {
            let clips = n.kind.frame_props().map(|f| f.clips_content).unwrap_or(false);
            let children = n.children.clone();
            if clips {
                let Some(sh) = &g.shape else { return };
                let radii = if matches!(n.kind, NodeKind::Section) { [0.0; 4] } else { n.corner_radii };
                let rect_like = sh.is_rect && radii.iter().all(|r| *r <= 0.0) && identity_like(&ts);
                let child_clip = if rect_like {
                    // axis-aligned rectangle: a scissor is exact up to the edge pixels
                    let (x0, y0) = (ts.tx, ts.ty);
                    let (x1, y1) = (ts.tx + n.size.x * ts.sx, ts.ty + n.size.y * ts.sy);
                    let r = IRect::new(x0.round() as i32, y0.round() as i32, x1.round() as i32, y1.round() as i32);
                    Some(self.narrow_clip(clip, r))
                } else if let Some(mesh) = &g.fill {
                    let roi = self.visible(clip);
                    self.mask_clip(mesh, ts, clip, false, roi)
                } else {
                    None
                };
                let Some(cc) = child_clip else { return };
                if self.visible(cc).is_empty() {
                    return;
                }
                self.draw_children(&children, ts, cc);
            } else {
                self.draw_children(&children, ts, clip);
            }
        }
    }

    // ----- fills and strokes ---------------------------------------------------

    fn fill_with(&mut self, mesh: &Arc<Mesh>, ts: sk::Transform, size: Vec2, p: &d::Paint, alpha: f32, clip: ClipId) {
        let Some(pi) = self.paints.convert(self.doc, p, size, alpha) else { return };
        if special_blend(p.blend) {
            // a paint blended against what is beneath: isolate it and composite
            let bb = xform_bounds(&ts, mesh.bounds.0, mesh.bounds.1, mesh.bounds.2 - mesh.bounds.0, mesh.bounds.3 - mesh.bounds.1);
            let rect = IRect::outward(bb).grow(1).intersect(self.visible(clip));
            if rect.is_empty() {
                return;
            }
            let layer = self.new_prog(rect);
            let lclip = self.narrow_clip(clip, rect);
            self.with_prog(layer, |c| c.draw_mesh(mesh, ts, pi, DrawBlend::Over, lclip));
            let pclip = self.full_clip(rect);
            self.push_op(Op::Composite(Box::new(CompositeOp {
                src: layer,
                rect,
                opacity: 1.0,
                tint: None,
                aux: None,
                mode: CompMode::Blend(p.blend),
                clip: pclip,
            })));
            return;
        }
        self.draw_mesh(mesh, ts, pi, DrawBlend::Over, clip);
    }

    fn draw_text(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, clip: ClipId, alpha: f32) {
        let strokes = visible_strokes(n);
        for (i, t) in g.text.iter().enumerate() {
            for p in &t.fills {
                self.fill_with(&t.mesh, ts, n.size, p, alpha, clip);
            }
            if !strokes.is_empty() {
                if let Some(sm) = g.text_stroke(i, n) {
                    for p in &strokes {
                        self.fill_with(&sm, ts, n.size, p, alpha, clip);
                    }
                }
            }
        }
    }

    fn draw_strokes(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, clip: ClipId, alpha: f32) {
        let paints = visible_strokes(n);
        if paints.is_empty() {
            return;
        }
        let Some((mesh, how)) = g.stroke_mesh(n) else { return };
        let sclip = match how {
            StrokeClip::None => clip,
            StrokeClip::Inside | StrokeClip::Outside => {
                let Some(shape) = &g.fill else { return };
                let b = mesh.bounds;
                let roi = IRect::outward(xform_bounds(&ts, b.0, b.1, b.2 - b.0, b.3 - b.1)).grow(1);
                let Some(c) = self.mask_clip(shape, ts, clip, how == StrokeClip::Outside, roi) else { return };
                c
            }
        };
        for p in paints {
            self.fill_with(&mesh, ts, n.size, p, alpha, sclip);
        }
    }

    // ----- silhouettes ----------------------------------------------------------

    /// A program of `rect` holding the node silhouette (black, with the fill
    /// alpha unless `opaque`), shifted by (`dx`, `dy`) device pixels and grown
    /// (`grow` > 0) or eroded (< 0) by `grow` local units.
    #[allow(clippy::too_many_arguments)]
    fn sil_program(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, rect: IRect, dx: f32, dy: f32, grow: f32, opaque: bool) -> ProgId {
        let prog = self.new_prog(rect);
        let clip = self.full_clip(rect);
        self.with_prog(prog, |c| c.silhouette(n, g, ts, dx, dy, grow, opaque, clip));
        prog
    }

    /// Content key of [`Self::sil_program`] blurred by `sigma`: everything the silhouette pixels
    /// depend on, with its position relative to `rect` (quantized to 1/8 px). `None` when the
    /// silhouette is drawn from the node's children, which this key does not cover.
    #[allow(clippy::too_many_arguments)]
    fn sil_key(&self, n: &Node, g: &NodeGeom, ts: sk::Transform, rect: IRect, dx: f32, dy: f32, grow: f32, opaque: bool, sigma: f32) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        let fills = visible_fills(n);
        let strokes = visible_strokes(n);
        let text = matches!(n.kind, NodeKind::Text(_));
        let own_shape = g.shape.as_ref().is_some_and(|sh| (sh.fillable && !fills.is_empty()) || !strokes.is_empty() || (opaque && sh.fillable));
        if !text && !own_shape {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        g.id.hash(&mut h);
        opaque.hash(&mut h);
        for v in [grow, sigma, ts.sx, ts.ky, ts.kx, ts.sy, n.stroke.weight] {
            v.to_bits().hash(&mut h);
        }
        let q = |v: f32| (v * 8.0).round() as i64;
        (q(ts.tx + dx - rect.x0 as f32), q(ts.ty + dy - rect.y0 as f32), rect.w(), rect.h()).hash(&mut h);
        std::mem::discriminant(&n.stroke.align).hash(&mut h);
        strokes.is_empty().hash(&mut h);
        if !opaque {
            let alphas = if text {
                g.text.iter().map(|t| t.fills.iter().filter(|p| p.visible).map(|p| p.opacity).fold(0.0, f32::max)).collect::<Vec<_>>()
            } else {
                fills.iter().map(|p| if let d::PaintKind::Solid(c) = &p.kind { c.a * p.opacity } else { p.opacity }).collect()
            };
            for a in alphas {
                a.to_bits().hash(&mut h);
            }
        }
        Some(h.finish())
    }

    #[allow(clippy::too_many_arguments)]
    fn silhouette(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, dx: f32, dy: f32, grow: f32, opaque: bool, clip: ClipId) {
        let ts2 = ts.post_translate(dx, dy);
        let fills = visible_fills(n);
        let strokes = visible_strokes(n);
        let black = |a: f32| solid_inst([0.0, 0.0, 0.0, a.clamp(0.0, 1.0)]);
        if matches!(n.kind, NodeKind::Text(_)) {
            for (i, t) in g.text.iter().enumerate() {
                let _ = i;
                let a = if opaque { 1.0 } else { t.fills.iter().filter(|p| p.visible).map(|p| p.opacity).fold(0.0, f32::max) };
                self.draw_mesh(&t.mesh, ts2, black(a), DrawBlend::Over, clip);
            }
            return;
        }
        if let Some(sh) = &g.shape {
            if (sh.fillable && !fills.is_empty()) || !strokes.is_empty() || (opaque && sh.fillable) {
                if let Some(fill) = &g.fill {
                    if sh.fillable && opaque && fills.is_empty() {
                        self.draw_mesh(fill, ts2, black(1.0), DrawBlend::Over, clip);
                    } else if sh.fillable {
                        for p in &fills {
                            let a = if opaque {
                                1.0
                            } else {
                                let base = match &p.kind {
                                    d::PaintKind::Solid(c) => c.a,
                                    _ => 1.0,
                                };
                                base * p.opacity
                            };
                            self.draw_mesh(fill, ts2, black(a), DrawBlend::Over, clip);
                        }
                    }
                }
                // stroke footprint outside the fill
                let outset = match n.stroke.align {
                    d::StrokeAlign::Inside if sh.closed && sh.fillable && !fills.is_empty() => 0.0,
                    d::StrokeAlign::Outside if sh.closed => n.stroke.weight * 2.0,
                    _ => n.stroke.weight,
                };
                if let (Some(_), false) = (&sh.stroke_paths, strokes.is_empty()) {
                    if let Some(m) = g.stroke_paths_mesh() {
                        self.draw_mesh(&m, ts2, black(1.0), DrawBlend::Over, clip);
                    }
                } else if !strokes.is_empty() && outset > 0.0 {
                    if let Some(m) = g.grow_outline(outset) {
                        self.draw_mesh(&m, ts2, black(1.0), DrawBlend::Over, clip);
                    }
                }
                if grow != 0.0 {
                    if let Some(m) = g.grow_outline(grow.abs() * 2.0) {
                        let blend = if grow < 0.0 { DrawBlend::Erase } else { DrawBlend::Over };
                        self.draw_mesh(&m, ts2, black(1.0), blend, clip);
                    }
                }
                return;
            }
        }
        if draws_children(n) {
            // silhouettes are drawn offset from the page grid: no index culling
            let was = std::mem::replace(&mut self.cull, false);
            let children = n.children.clone();
            self.draw_children(&children, ts2, clip);
            self.cull = was;
        }
    }

    // ----- effects ----------------------------------------------------------------

    /// The shadow of a plain solid rounded rectangle, drawn analytically in one
    /// quad (the GPU counterpart of the CPU renderer's nine-patch fast path).
    #[allow(clippy::too_many_arguments)]
    fn drop_shadow_analytic(
        &mut self,
        n: &Node,
        g: &Rc<NodeGeom>,
        ts: sk::Transform,
        clip: ClipId,
        scale: f32,
        color: &d::Color,
        offset: Vec2,
        radius: f32,
        spread: f32,
        show_behind: bool,
        alpha: f32,
    ) -> bool {
        let Some(sh) = &g.shape else { return false };
        if !sh.is_rect
            || !sh.fillable
            || n.fill_geometry.is_some()
            || !visible_strokes(n).is_empty()
            || !identity_like(&ts)
            || (ts.sx - ts.sy).abs() > 1e-4 * ts.sx
        {
            return false;
        }
        let fills = visible_fills(n);
        let [fill] = fills[..] else { return false };
        let d::PaintKind::Solid(c) = &fill.kind else { return false };
        let radii = if matches!(n.kind, NodeKind::Section) { [0.0; 4] } else { n.corner_radii };
        if radii.iter().any(|r| (r - radii[0]).abs() > 1e-4) || (radii[0] > 0.0 && n.corner_smoothing > 0.0) {
            return false;
        }
        let sigma = radius.max(0.0) * scale / 2.0;
        if sigma < 0.3 || (spread != 0.0 && sigma < 8.0) || n.size.x <= 0.0 || n.size.y <= 0.0 {
            return false;
        }
        let rl = radii[0].max(0.0).min(n.size.x.min(n.size.y) / 2.0);
        let sp = spread * scale;
        let (x0, y0) = (ts.tx, ts.ty);
        let (x1, y1) = (ts.tx + n.size.x * ts.sx, ts.ty + n.size.y * ts.sy);
        let (ox, oy) = (offset.x * scale, offset.y * scale);
        let (a, b, cc, dd) = (x0 + ox - sp, y0 + oy - sp, x1 + ox + sp, y1 + oy + sp);
        let r = if rl > 0.0 { (rl * scale + sp).max(0.0) } else { 0.0 }.min((cc - a) / 2.0).min((dd - b) / 2.0);
        let margin = 3.0 * sigma + 2.0;
        let quad = IRect::outward((a - margin, b - margin, cc + margin, dd + margin)).intersect(self.visible(clip));
        if quad.is_empty() {
            return true;
        }
        let fill_alpha = (c.a * fill.opacity).clamp(0.0, 1.0);
        let col_a = (color.a * alpha).clamp(0.0, 1.0) * fill_alpha;
        let inst = Instance {
            t: [0.0, 0.0, sigma, r],
            c0: [color.r.clamp(0.0, 1.0), color.g.clamp(0.0, 1.0), color.b.clamp(0.0, 1.0), col_a],
            c1: [a, b, cc, dd],
            c2: [x0, y0, x1, y1],
            q: [KIND_SHADOW, 0, (!show_behind) as u32, (rl * scale).to_bits()],
            ..Default::default()
        };
        self.draw_quad(quad, PaintInst { inst, ramp: None, image: None }, DrawBlend::Over, clip);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn drop_shadow(
        &mut self,
        n: &Node,
        g: &Rc<NodeGeom>,
        ts: sk::Transform,
        clip: ClipId,
        bbox: FBox,
        scale: f32,
        color: &d::Color,
        offset: Vec2,
        radius: f32,
        spread: f32,
        show_behind: bool,
        alpha: f32,
    ) {
        if self.drop_shadow_analytic(n, g, ts, clip, scale, color, offset, radius, spread, show_behind, alpha) {
            return;
        }
        let sigma = radius.max(0.0) * scale / 2.0;
        let (ox, oy) = (offset.x * scale, offset.y * scale);
        let margin = 3.0 * sigma + ox.abs() + oy.abs() + (spread * scale).max(0.0) + 2.0;
        let reach = margin.ceil() as i32;
        let vis = self.visible(clip);
        let rect = IRect::outward((bbox.0 - margin, bbox.1 - margin, bbox.2 + margin, bbox.3 + margin)).intersect(vis.grow(reach));
        if rect.is_empty() {
            return;
        }
        let s = self.sil_program(n, g, ts, rect, ox, oy, spread, false);
        let blur = if sigma > 0.05 { sigma } else { 0.0 };
        if blur > 0.0 {
            self.progs[s as usize].post.push(Post::Blur { sigma, clamp: false });
        }
        self.progs[s as usize].cache = self.sil_key(n, g, ts, rect, ox, oy, spread, false, blur);
        let aux = (!show_behind).then(|| {
            let a = self.sil_program(n, g, ts, rect, 0.0, 0.0, 0.0, true);
            self.progs[a as usize].cache = self.sil_key(n, g, ts, rect, 0.0, 0.0, 0.0, true, 0.0);
            (a, AuxMode::MulInv)
        });
        let tint = [color.r.clamp(0.0, 1.0), color.g.clamp(0.0, 1.0), color.b.clamp(0.0, 1.0), (color.a * alpha).clamp(0.0, 1.0)];
        self.push_op(Op::Composite(Box::new(CompositeOp { src: s, rect, opacity: 1.0, tint: Some(tint), aux, mode: CompMode::Over, clip })));
    }

    #[allow(clippy::too_many_arguments)]
    fn inner_shadow(
        &mut self,
        n: &Node,
        g: &Rc<NodeGeom>,
        ts: sk::Transform,
        clip: ClipId,
        bbox: FBox,
        scale: f32,
        color: &d::Color,
        offset: Vec2,
        radius: f32,
        spread: f32,
        alpha: f32,
    ) {
        let sigma = radius.max(0.0) * scale / 2.0;
        let (ox, oy) = (offset.x * scale, offset.y * scale);
        let margin = 3.0 * sigma + ox.abs() + oy.abs() + (spread * scale).abs() + 2.0;
        let reach = margin.ceil() as i32;
        let vis = self.visible(clip);
        let rect = IRect::outward((bbox.0 - margin, bbox.1 - margin, bbox.2 + margin, bbox.3 + margin)).intersect(vis.grow(reach));
        if rect.is_empty() {
            return;
        }
        let hole = self.sil_program(n, g, ts, rect, ox, oy, -spread, false);
        let p = self.new_prog(rect);
        let inner = self.full_clip(rect);
        self.with_prog(p, |c| {
            c.draw_quad(rect, solid_inst([0.0, 0.0, 0.0, 1.0]), DrawBlend::Over, inner);
            c.push_op(Op::Composite(Box::new(CompositeOp { src: hole, rect, opacity: 1.0, tint: None, aux: None, mode: CompMode::Erase, clip: inner })));
        });
        if sigma > 0.05 {
            self.progs[p as usize].post.push(Post::Blur { sigma, clamp: false });
        }
        let shape = self.sil_program(n, g, ts, rect, 0.0, 0.0, 0.0, false);
        let tint = [color.r.clamp(0.0, 1.0), color.g.clamp(0.0, 1.0), color.b.clamp(0.0, 1.0), (color.a * alpha).clamp(0.0, 1.0)];
        self.push_op(Op::Composite(Box::new(CompositeOp {
            src: p,
            rect,
            opacity: 1.0,
            tint: Some(tint),
            aux: Some((shape, AuxMode::Mul)),
            mode: CompMode::Over,
            clip,
        })));
    }

    #[allow(clippy::too_many_arguments)]
    fn background_blur(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, clip: ClipId, inner_bb: FBox, scale: f32, radius: f32) {
        let sigma = radius.max(0.0) * scale / 2.0;
        let region = IRect::outward(inner_bb).intersect(self.root_rect);
        if region.is_empty() {
            return;
        }
        let pad = (3.0 * sigma).ceil() as i32 + 1;
        let padr = region.grow(pad).intersect(self.root_rect);
        let mask = self.sil_program(n, g, ts, region, 0.0, 0.0, 0.0, true);
        self.push_op(Op::Backdrop(Box::new(BackdropOp { kind: BackdropKind::Blur { sigma }, region, pad: padr, mask, clip, grid: self.grid })));
    }

    #[allow(clippy::too_many_arguments)]
    fn glass(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, clip: ClipId, inner_bb: FBox, scale: f32, e: &d::GlassEffect) {
        let region = IRect::outward(inner_bb).intersect(self.root_rect);
        if region.is_empty() {
            return;
        }
        let depth = e.depth.max(0.0) * scale;
        let sigma_f = e.frost.max(0.0) * scale / 2.0;
        let sigma_m = (depth / 3.0).max(0.6);
        let pad = (3.0 * sigma_f.max(sigma_m)).ceil() as i32 + (depth * 1.5).ceil() as i32 + 2;
        let padr = region.grow(pad).intersect(self.root_rect);
        let mask = self.sil_program(n, g, ts, padr, 0.0, 0.0, 0.0, true);
        let ang = e.light_angle.to_radians();
        let params = GlassParams {
            light: [ang.cos(), ang.sin()],
            intensity: e.light_intensity.clamp(0.0, 1.0),
            refraction: e.refraction.clamp(0.0, 1.0),
            dispersion: e.dispersion.clamp(0.0, 1.0),
            depth,
            sigma_frost: sigma_f,
            sigma_mask: sigma_m,
        };
        self.push_op(Op::Backdrop(Box::new(BackdropOp { kind: BackdropKind::Glass(params), region, pad: padr, mask, clip, grid: self.grid })));
    }

    #[allow(clippy::too_many_arguments)]
    fn noise(&mut self, n: &Node, g: &Rc<NodeGeom>, ts: sk::Transform, clip: ClipId, bbox: FBox, scale: f32, e: &d::NoiseEffect, alpha: f32) {
        let rect = IRect::outward(bbox).intersect(self.visible(clip));
        if rect.is_empty() {
            return;
        }
        let a = (e.opacity * alpha).clamp(0.0, 1.0);
        let rgba = |c: &d::Color| [c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0), (c.a * a).clamp(0.0, 1.0)];
        let (ca, cb, mode) = match &e.mode {
            d::NoiseMode::Mono { color } => (rgba(color), rgba(color), 0),
            d::NoiseMode::Duo { color, secondary } => (rgba(color), rgba(secondary), 1),
            d::NoiseMode::Multi => ([1.0, 1.0, 1.0, a], [1.0, 1.0, 1.0, a], 2),
        };
        let inst = Instance {
            q: [KIND_NOISE, e.seed, mode, 0],
            q2: [self.grid.0, self.grid.1, self.ss as i32, 0],
            c0: ca,
            c1: cb,
            c2: [(e.size.max(0.05) * scale / self.ss as f32).max(1.0), e.density.clamp(0.0, 1.0), 0.0, 0.0],
            t: [0.0, 0.0, 1.0, 0.0],
            ..Default::default()
        };
        let pi = PaintInst { inst, ramp: None, image: None };
        let normal = !special_blend(e.blend);
        // fast path: grain drawn straight over the shape's own geometry
        let direct = normal && matches!(n.kind, NodeKind::Text(_));
        if direct {
            for t in &g.text {
                self.draw_mesh(&t.mesh, ts, PaintInst { inst: pi.inst, ramp: None, image: None }, DrawBlend::Over, clip);
            }
            return;
        }
        if normal && g.shape.as_ref().is_some_and(|s| s.fillable) && visible_strokes(n).is_empty() {
            if let Some(m) = &g.fill {
                // opaque silhouette == fill geometry when the node has a fillable shape
                self.draw_mesh(m, ts, pi, DrawBlend::Over, clip);
                return;
            }
        }
        // general case: grain layer masked by the opaque silhouette
        let layer = self.new_prog(rect);
        let lclip = self.full_clip(rect);
        self.with_prog(layer, |c| c.draw_quad(rect, pi, DrawBlend::Over, lclip));
        let mask = self.sil_program(n, g, ts, rect, 0.0, 0.0, 0.0, true);
        let mode = if normal { CompMode::Over } else { CompMode::Blend(e.blend) };
        self.push_op(Op::Composite(Box::new(CompositeOp { src: layer, rect, opacity: 1.0, tint: None, aux: Some((mask, AuxMode::Mul)), mode, clip })));
    }
}

fn solid_inst(c: [f32; 4]) -> PaintInst {
    let inst = Instance { q: [KIND_SOLID, 0, 0, 0], c0: c, t: [0.0, 0.0, 1.0, 0.0], ..Default::default() };
    PaintInst { inst, ramp: None, image: None }
}
