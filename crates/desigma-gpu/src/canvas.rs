//! What an egui paint callback needs: the renderer plus the presenter.

use crate::exec::Renderer;
use crate::frame::Frame;
use crate::present::{PresentView, Presenter};

/// Stored in egui's callback resources: `prepare` runs a frame into the host
/// encoder, `paint` shows the result inside the host render pass.
pub struct Canvas {
    pub renderer: Renderer,
    presenter: Presenter,
}

impl Canvas {
    /// `target_format` / `samples` describe the host render pass the page is drawn into.
    pub fn new(dev: &wgpu::Device, queue: &wgpu::Queue, target_format: wgpu::TextureFormat, samples: u32) -> Result<Self, crate::GpuError> {
        let renderer = Renderer::new(dev, queue)?;
        Ok(Self { renderer, presenter: Presenter::new(dev, target_format, samples) })
    }

    /// Records the frame's passes into the host encoder.
    pub fn prepare(&mut self, dev: &wgpu::Device, enc: &mut wgpu::CommandEncoder, frame: &Frame) {
        let out = self.renderer.render(frame, enc);
        self.presenter.set_source(dev, &out.view);
    }

    /// How the last prepared frame maps onto the viewport (identity unless a zoom gesture
    /// shows a previous frame scaled).
    pub fn set_view(&self, queue: &wgpu::Queue, view: PresentView) {
        self.presenter.set_view(queue, view);
    }

    /// Draws the prepared frame into the host pass' viewport.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.presenter.draw(pass);
    }

    pub fn max_texture_dimension(&self) -> u32 {
        self.renderer.max_texture_dimension()
    }
}
