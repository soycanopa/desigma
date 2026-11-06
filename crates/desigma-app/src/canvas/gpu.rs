//! Canvas rendering on the GPU (`desigma-gpu`): the visible page is compiled
//! to a frame while the UI is painted and drawn by an egui paint callback.
//! Pan and zoom only change the view transform: geometry is cached per node,
//! nothing is re-rasterised on the CPU. A frame identical to the one on the
//! GPU is drawn again as it is, and during a zoom gesture the last frame is
//! shown scaled until the zoom settles (like the CPU tiles, see [`ZOOM_SETTLE`]).
//!
//! The CPU tile path (`tiles.rs`) stays the fallback: it is used when the GPU
//! cannot be initialised, when "Use GPU rendering" is off, and for
//! `--screenshot` unless `--gpu` is given (pixel diffs must be deterministic).

use super::tiles::ZOOM_SETTLE;
use crate::app::App;
use desigma_doc::NodeId;
use desigma_gpu::{Canvas, Frame, PresentView, SceneCompiler};
use desigma_render::Viewport;
use eframe::egui_wgpu;
use egui::{Rect as ERect, Ui};
use std::sync::Arc;
use std::time::Duration;

/// Everything the rendered page texture depends on.
#[derive(Clone, Copy, PartialEq)]
struct FrameKey {
    rev: u64,
    page: NodeId,
    offset: (f32, f32),
    zoom: f32,
    ppp: f32,
    size: (u32, u32),
    fonts: usize,
}

impl FrameKey {
    /// How the texture rendered for `self` maps onto the viewport of `now` (same page and size).
    fn view_for(&self, now: &FrameKey) -> PresentView {
        let k = self.zoom * self.ppp;
        let shift = |d: f32, size: u32| d * k / size.max(1) as f32;
        let s = self.zoom / now.zoom;
        PresentView { scale: [s, s], offset: [shift(now.offset.0 - self.offset.0, now.size.0), shift(now.offset.1 - self.offset.1, now.size.1)] }
    }
}

/// App-side GPU state.
pub struct GpuView {
    /// Preferences -> "Use GPU rendering".
    pub pref: bool,
    /// The device and renderer initialised.
    pub available: bool,
    max_texture: u32,
    compiler: SceneCompiler,
    /// The frame the renderer's output holds.
    out: Option<FrameKey>,
    /// `out` has no stale geometry, so it can be drawn again as it is.
    complete: bool,
    /// Zoom at the last paint, and when it last changed (zoom gesture detection).
    last_zoom: f32,
    zoom_changed_at: f64,
    /// Milliseconds the last frame took to compile (CPU side).
    pub last_compile_ms: f32,
    pub last_stats: Option<desigma_gpu::FrameStats>,
}

impl Default for GpuView {
    fn default() -> Self {
        let mut compiler = SceneCompiler::new();
        // re-flattening curves for a new zoom level may take at most this long per frame
        compiler.geom.budget = Some(Duration::from_millis(6));
        Self {
            pref: true,
            available: false,
            max_texture: 0,
            compiler,
            out: None,
            complete: false,
            last_zoom: 0.0,
            zoom_changed_at: 0.0,
            last_compile_ms: 0.0,
            last_stats: None,
        }
    }
}

impl GpuView {
    /// Whether the canvas is drawn by the GPU right now.
    pub fn active(&self) -> bool {
        self.pref && self.available
    }

    /// Forgets cached geometry (the document was replaced).
    pub fn reset(&mut self) {
        self.compiler.geom.clear();
        self.out = None;
    }

    /// Largest texture edge the device supports.
    pub fn set_max_texture(&mut self, max: u32) {
        self.compiler.opts.max_texture = max;
        self.max_texture = max;
    }
}

struct Cb {
    /// `None`: draw the renderer's current output again.
    frame: Option<Arc<Frame>>,
    view: PresentView,
}

impl egui_wgpu::CallbackTrait for Cb {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut egui_wgpu::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        if let Some(c) = resources.get_mut::<Canvas>() {
            if let Some(frame) = &self.frame {
                c.prepare(device, encoder, frame);
            }
            c.set_view(queue, self.view);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut egui_wgpu::wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(c) = resources.get::<Canvas>() {
            c.paint(pass);
        }
    }
}

impl App {
    /// Preferences -> "Use GPU rendering": switches between the GPU canvas and the CPU tiles.
    pub fn toggle_gpu(&mut self) {
        if !self.canvas.gpu.available {
            self.show_toast("GPU rendering is not available on this system", true);
            return;
        }
        self.canvas.gpu.pref = !self.canvas.gpu.pref;
        let on = self.canvas.gpu.pref;
        if let Some(lib) = &self.lib {
            if let Err(e) = crate::comments::settings::save_flag(lib.root(), "use_gpu", on) {
                self.show_toast(e, true);
            }
        }
        // the tile cache may be stale after a long GPU session
        self.canvas.tiles.invalidate_all();
    }
}

/// Creates the GPU renderer on eframe's device. Returns false (and the app keeps
/// using the CPU tiles) when that is not possible.
pub fn init(rs: &egui_wgpu::RenderState) -> Option<u32> {
    match Canvas::new(&rs.device, &rs.queue, rs.target_format, 1) {
        Ok(c) => {
            let max = c.max_texture_dimension();
            rs.renderer.write().callback_resources.insert(c);
            Some(max)
        }
        Err(e) => {
            eprintln!("desigma: GPU rendering unavailable, using the CPU renderer ({e})");
            None
        }
    }
}

/// Paints the page with the GPU into `rect`; false when the CPU path must do it.
pub fn paint(app: &mut App, ui: &Ui, rect: ERect) -> bool {
    if !app.canvas.gpu.active() {
        return false;
    }
    let ppp = ui.ctx().pixels_per_point();
    let size = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    if size.0 > app.canvas.gpu.max_texture.max(1) || size.1 > app.canvas.gpu.max_texture.max(1) {
        return false;
    }
    let rev = super::tiles::take_rev(app);
    let start = std::time::Instant::now();
    let vp = Viewport { offset: app.view.offset, zoom: app.view.zoom, dpr: ppp };
    let page = app.page;
    let key = FrameKey { rev, page, offset: (vp.offset.x, vp.offset.y), zoom: vp.zoom, ppp, size, fonts: desigma_render::font_generation() };
    let now = ui.input(|i| i.time);
    let bg = super::tiles::page_background(app);
    let board = app.is_board();
    let c = &mut app.canvas;
    let g = &mut c.gpu;
    if key.zoom != g.last_zoom {
        g.last_zoom = key.zoom;
        g.zoom_changed_at = now;
    }
    let zooming = now - g.zoom_changed_at < ZOOM_SETTLE;
    let painter = ui.painter().with_clip_rect(rect);
    let (frame, view) = match g.out {
        Some(out) if out == key && g.complete => (None, PresentView::IDENTITY),
        // zoom gesture: the last frame scaled, re-rendered once the zoom is still
        Some(out) if zooming && out.zoom != key.zoom && FrameKey { offset: out.offset, zoom: out.zoom, ..key } == out => {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(ZOOM_SETTLE - (now - g.zoom_changed_at) + 0.005));
            if !board {
                // what the old frame does not cover
                painter.rect_filled(rect, 0.0, bg);
            }
            (None, out.view_for(&key))
        }
        _ => {
            let doc = c.motion.document(app.motion.enabled, &app.doc);
            let frame = g.compiler.compile(doc, page, rev, &vp, size);
            g.last_stats = Some(frame.stats());
            g.last_compile_ms = start.elapsed().as_secs_f32() * 1000.0;
            g.out = Some(key);
            g.complete = g.compiler.geom.stale_hits == 0;
            if !g.complete {
                // some meshes still have the previous zoom's flattening: refine next frame
                ui.ctx().request_repaint();
            }
            (Some(Arc::new(frame)), PresentView::IDENTITY)
        }
    };
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, Cb { frame, view }));
    c.last_render_ms = start.elapsed().as_secs_f32() * 1000.0;
    true
}
