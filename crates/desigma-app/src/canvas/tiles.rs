//! Canvas rendering through the renderer's tile cache: the page is painted as
//! 256 px tiles, each uploaded as its own texture only when its pixels
//! change, so panning reuses everything already drawn and an edit re-renders
//! just the tiles under the nodes it touched.
//!
//! Zoom gestures follow Figma: while the zoom keeps changing the tiles of the
//! last settled zoom are simply scaled by the GPU; once it has been still for
//! [`ZOOM_SETTLE`] seconds the page is re-rendered at the new scale (the old
//! tiles stay underneath until the new ones arrive).

use crate::app::App;
use desigma_doc::NodeKind;
use desigma_render::{Tile, TileBudget, TileCache, TileKey, Viewport, TILE_SIZE};
use egui::{Color32, Context, Rect as ERect, TextureHandle, TextureOptions, Ui};
use std::collections::HashMap;
use std::time::Duration;

/// Seconds without a zoom change before tiles are re-rendered at the new zoom.
pub const ZOOM_SETTLE: f64 = 0.15;

/// Time one frame may spend rendering tiles (a started batch always finishes).
const RENDER_BUDGET: Duration = Duration::from_millis(6);

struct TexEntry {
    handle: TextureHandle,
    version: u64,
    /// Sampled with linear filtering (tile is being scaled).
    linear: bool,
    seen: u64,
}

#[derive(Default)]
pub struct TileView {
    cache: TileCache,
    /// Document revision handed to the cache; bumped when `dirty` is consumed.
    rev: u64,
    textures: HashMap<TileKey, TexEntry>,
    frame_no: u64,
    /// Zoom the tiles are currently rendered at (lags `view.zoom` mid-gesture).
    settled_zoom: f32,
    last_zoom: f32,
    zoom_changed_at: f64,
    /// The document was replaced: the GPU geometry cache must be dropped.
    pub gpu_reset: bool,
}

impl TileView {
    /// Document revision, bumped whenever the document changed (autosave watches it).
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Re-renders everything (stale tiles stay visible until replaced).
    pub fn invalidate_all(&mut self) {
        self.cache.invalidate_all();
        self.gpu_reset = true;
    }
}

pub(super) fn page_background(app: &App) -> Color32 {
    match app.doc.get(app.page).map(|n| &n.kind) {
        Some(NodeKind::Page { background }) => {
            let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color32::from_rgb(f(background.r), f(background.g), f(background.b))
        }
        _ => Color32::from_rgb(0xF5, 0xF5, 0xF5),
    }
}

fn upload(ctx: &Context, tv: &mut TileView, tile: &Tile, linear: bool) -> egui::TextureId {
    let opts = if linear { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
    let side = TILE_SIZE as usize;
    let frame_no = tv.frame_no;
    if let Some(e) = tv.textures.get_mut(&tile.key) {
        e.seen = frame_no;
        if e.version != tile.version || e.linear != linear {
            let img = egui::ColorImage::from_rgba_premultiplied([side, side], tile.pixmap.data());
            e.handle.set(img, opts);
            e.version = tile.version;
            e.linear = linear;
        }
        return e.handle.id();
    }
    let img = egui::ColorImage::from_rgba_premultiplied([side, side], tile.pixmap.data());
    let name = format!("tile {} {} {}", tile.key.scale_bits, tile.key.x, tile.key.y);
    let handle = ctx.load_texture(name, img, opts);
    let id = handle.id();
    tv.textures.insert(tile.key, TexEntry { handle, version: tile.version, linear, seen: frame_no });
    id
}

/// Revision for the render caches this frame. It changes when the document did, and in Motion
/// mode also when the playhead moved (the page is then drawn evaluated at the playhead: see
/// [`crate::motion::playback`]), so the caches re-diff the page and re-render only what moved.
pub fn take_rev(app: &mut App) -> u64 {
    let c = &mut app.canvas;
    if c.dirty {
        c.dirty = false;
        c.tiles.rev += 1;
    }
    if std::mem::take(&mut c.tiles.gpu_reset) {
        c.gpu.reset();
        c.motion.reset();
    }
    c.motion.prepare(&app.motion, &app.doc, app.page, c.tiles.rev)
}

/// Renders (as far as the budget allows) and paints the page into `rect`.
pub fn paint(app: &mut App, ui: &Ui, rect: ERect) {
    if super::gpu::paint(app, ui, rect) {
        return;
    }
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let (w, h) = ((rect.width() * ppp).round().max(1.0), (rect.height() * ppp).round().max(1.0));
    let start = std::time::Instant::now();
    let bg = page_background(app);
    let painter = ui.painter().with_clip_rect(rect);
    if !app.is_board() {
        painter.rect_filled(rect, 0.0, bg);
    }

    let (page, offset, zoom) = (app.page, app.view.offset, app.view.zoom);
    let now = ctx.input(|i| i.time);
    let rev = take_rev(app);
    let c = &mut app.canvas;
    let shown = c.motion.document(app.motion.enabled, &app.doc);
    let tv = &mut c.tiles;
    tv.frame_no += 1;

    // Zoom gesture handling: keep rendering at the settled zoom until idle.
    if tv.settled_zoom <= 0.0 {
        tv.settled_zoom = zoom;
        tv.last_zoom = zoom;
    }
    if zoom != tv.last_zoom {
        tv.last_zoom = zoom;
        tv.zoom_changed_at = now;
    }
    if zoom != tv.settled_zoom {
        let idle = now - tv.zoom_changed_at;
        if idle >= ZOOM_SETTLE {
            tv.settled_zoom = zoom;
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(ZOOM_SETTLE - idle + 0.005));
        }
    }
    let rz = tv.settled_zoom;
    // device size of the visible page area when drawn at the settled zoom
    let ratio = rz / zoom;
    let vp = Viewport { offset, zoom: rz, dpr: ppp };
    let size = ((w * ratio).ceil().max(1.0) as u32, (h * ratio).ceil().max(1.0) as u32);
    let frame = tv.cache.render_tiles(shown, page, &vp, size, rev, TileBudget::time(RENDER_BUDGET));
    let pending = frame.pending() > 0 || frame.prefetch_pending > 0;

    // Tiles are laid out in device pixels of the actual zoom, snapped so
    // neighbours abut exactly (and match the tile grid 1:1 when not scaling).
    let a = zoom * ppp;
    let to_pt = |dev: f32| dev.round() / ppp;
    for t in frame.fallback.iter().chain(frame.tiles.iter()) {
        let s = t.key.scale();
        let (ox, oy) = t.key.device_origin();
        let side = TILE_SIZE as f32;
        let x0 = (ox as f32 / s - offset.x) * a;
        let y0 = (oy as f32 / s - offset.y) * a;
        let x1 = ((ox as f32 + side) / s - offset.x) * a;
        let y1 = ((oy as f32 + side) / s - offset.y) * a;
        let r = ERect::from_min_max(egui::pos2(rect.min.x + to_pt(x0), rect.min.y + to_pt(y0)), egui::pos2(rect.min.x + to_pt(x1), rect.min.y + to_pt(y1)));
        if !r.intersects(rect) {
            continue;
        }
        let linear = (s - a).abs() > 1e-4 * a;
        let id = upload(ctx, tv, t, linear);
        painter.image(id, r, ERect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }

    if pending {
        ctx.request_repaint();
    }
    // drop textures of evicted tiles now and then
    if tv.frame_no.is_multiple_of(60) {
        let cache = &tv.cache;
        tv.textures.retain(|k, _| cache.contains(k));
    }
    c.last_render_ms = start.elapsed().as_secs_f32() * 1000.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use desigma_doc::motion::{AnimProperty, AnimValue, Ease};
    use desigma_doc::{Color, Node, Paint, Rect, Vec2};

    /// A page of two tiles: a static square at x = 300 and a square sliding inside the first tile.
    fn app() -> App {
        let mut d = desigma_doc::Document::new("t");
        let page = d.add_page("p");
        d.get_mut(page).unwrap().kind = NodeKind::Page { background: Color::WHITE };
        let st = d.alloc_id();
        let mut n = Node::rectangle(st, Rect::new(300.0, 10.0, 40.0, 40.0));
        n.fills = vec![Paint::solid(Color::rgb(0, 0, 255))];
        d.insert(page, n).unwrap();
        let mv = d.alloc_id();
        let mut n = Node::rectangle(mv, Rect::new(0.0, 100.0, 40.0, 40.0));
        n.fills = vec![Paint::solid(Color::rgb(255, 0, 0))];
        let p = n.motion.ensure_track(AnimProperty::Position);
        p.set_key(0, AnimValue::Vec2(Vec2::new(0.0, 100.0)), Some(Ease::Linear));
        p.set_key(1000, AnimValue::Vec2(Vec2::new(200.0, 100.0)), None);
        d.insert(page, n).unwrap();
        App::new(d)
    }

    fn render(app: &mut App, cache: &mut TileCache) -> Vec<(i32, i32, u64, [u8; 4])> {
        let rev = take_rev(app);
        let shown = app.canvas.motion.document(app.motion.enabled, &app.doc);
        let vp = Viewport { offset: Vec2::ZERO, zoom: 1.0, dpr: 1.0 };
        let f = cache.render_tiles(shown, app.page, &vp, (512, 256), rev, TileBudget::unlimited());
        let mut out: Vec<_> = f
            .tiles
            .iter()
            .map(|t| {
                let (ox, oy) = t.key.device_origin();
                let c = t.pixmap.pixel(20, 120 - oy as u32 % 256).unwrap();
                (ox, oy, t.version, [c.red(), c.green(), c.blue(), c.alpha()])
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn canvas_shows_the_page_at_the_playhead_and_only_redraws_what_moved() {
        let mut app = app();
        let mut cache = TileCache::new();
        // Motion mode off: the stored document (the square sits at its stored place, x = 0)
        let before = render(&mut app, &mut cache);
        let at_x20 = |v: &Vec<(i32, i32, u64, [u8; 4])>| v.iter().find(|t| t.0 == 0 && t.1 == 0).unwrap().3;
        assert_eq!(at_x20(&before), [255, 0, 0, 255]);
        // Motion mode at 500 ms: the square is at x = 100..140, so x = 20 is background
        app.motion.enabled = true;
        app.motion.playhead_ms = 500;
        let mid = render(&mut app, &mut cache);
        assert_eq!(at_x20(&mid), [255, 255, 255, 255]);
        // Scrub to 600 ms: the moving tile is repainted, the static tile keeps its version
        app.motion.playhead_ms = 600;
        let later = render(&mut app, &mut cache);
        let ver = |v: &Vec<(i32, i32, u64, [u8; 4])>, x: i32| v.iter().find(|t| t.0 == x && t.1 == 0).unwrap().2;
        assert_ne!(ver(&later, 0), ver(&mid, 0), "the tile with the animated square is redrawn");
        assert_eq!(ver(&later, 256), ver(&mid, 256), "the tile that did not change is not");
        // The stored document never changed
        let st = app.doc.get(app.doc.get(app.page).unwrap().children[1]).unwrap();
        assert_eq!(st.transform.tx, 0.0);
        // Leaving Motion mode shows the stored document again
        app.motion.enabled = false;
        let back = render(&mut app, &mut cache);
        assert_eq!(at_x20(&back), [255, 0, 0, 255]);
    }
}
