//! Draws the rendered page texture into a host render pass (egui's).

/// How the page texture maps onto the pass viewport (see `present.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PresentView {
    /// Source uv = destination uv * `scale` + `offset`.
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

impl PresentView {
    pub const IDENTITY: PresentView = PresentView { scale: [1.0; 2], offset: [0.0; 2] };
}

pub struct Presenter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    view_buf: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
}

impl Presenter {
    /// `target_format` and `samples` must match the pass the page is drawn into.
    pub fn new(dev: &wgpu::Device, target_format: wgpu::TextureFormat, samples: u32) -> Self {
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/present.wgsl").into()),
        });
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pl =
            dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let view_buf = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("present view"),
            size: std::mem::size_of::<PresentView>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, layout, view_buf, bind: None }
    }

    /// Selects the texture to draw (call once per frame, before `draw`).
    pub fn set_source(&mut self, dev: &wgpu::Device, view: &wgpu::TextureView) {
        self.bind = Some(dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.view_buf.as_entire_binding() },
            ],
        }));
    }

    /// Sets how the texture maps onto the viewport for the next `draw`.
    pub fn set_view(&self, queue: &wgpu::Queue, view: PresentView) {
        queue.write_buffer(&self.view_buf, 0, bytemuck::bytes_of(&view));
    }

    /// Draws the page into the pass' current viewport.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(b) = &self.bind {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, b, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
