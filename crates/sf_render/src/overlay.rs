//! The egui overlay layer (WP1.11): painting only.
//!
//! `sf_render` owns the wgpu device, so it owns the egui *painter*
//! (`egui-wgpu`). Building the UI and feeding it window events is the
//! caller's job (`sf_game::debug_ui` uses `egui` and `egui-winit`), which keeps
//! this crate free of winit and of any panel logic. The caller hands over one
//! [`OverlayFrame`] per frame with [`crate::Renderer::set_overlay`]; the
//! renderer draws it in a second render pass on top of the scene.
//!
//! Costs nothing while unused: the painter (shader, pipeline, buffers) is
//! created on the first `set_overlay`, and a renderer without an overlay
//! records no extra pass.

/// One frame of egui output, ready to paint.
pub struct OverlayFrame {
    /// Tessellated shapes (`egui::Context::tessellate`).
    pub jobs: Vec<egui::ClippedPrimitive>,
    /// Texture uploads and frees of this frame (`FullOutput::textures_delta`).
    pub textures_delta: egui::TexturesDelta,
    /// Physical pixels per egui point (`FullOutput::pixels_per_point`).
    pub pixels_per_point: f32,
}

/// The painter plus the frame to draw. Lives inside the renderer.
pub(crate) struct Overlay {
    painter: egui_wgpu::Renderer,
    jobs: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
    /// Textures to free once the frame that may still use them was submitted.
    pending_free: Vec<egui::TextureId>,
}

impl Overlay {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            painter: egui_wgpu::Renderer::new(
                device,
                format,
                egui_wgpu::RendererOptions::default(),
            ),
            jobs: Vec::new(),
            pixels_per_point: 1.0,
            pending_free: Vec::new(),
        }
    }

    /// Take over `frame`: upload texture changes now, keep the jobs for the
    /// next draw.
    pub(crate) fn set(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: OverlayFrame) {
        let mut textures = frame.textures_delta;
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.painter.update_texture(device, queue, *id, delta);
            }
        }
        self.pending_free.extend(textures.free.iter().copied());
        // Applied above; `TexturesDelta` asserts (debug) that it is emptied.
        textures.clear();
        self.jobs = frame.jobs;
        self.pixels_per_point = frame.pixels_per_point;
    }

    /// Nothing to draw (the panels are off). Textures stay resident, so
    /// reopening is instant.
    pub(crate) fn clear(&mut self) {
        self.jobs.clear();
    }

    pub(crate) fn has_jobs(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Upload vertex/index data; call before [`Overlay::paint`].
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
    ) -> Vec<wgpu::CommandBuffer> {
        let screen = self.screen(size);
        self.painter
            .update_buffers(device, queue, encoder, &self.jobs, &screen)
    }

    /// Record the overlay pass: loads the colour target, no depth.
    pub(crate) fn paint(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let screen = self.screen(size);
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // egui-wgpu wants a 'static pass (paint callbacks may keep resources).
        self.painter
            .render(&mut pass.forget_lifetime(), &self.jobs, &screen);
    }

    /// Free textures egui no longer needs (after the submit that used them).
    pub(crate) fn free_pending(&mut self) {
        for id in self.pending_free.drain(..) {
            self.painter.free_texture(&id);
        }
    }

    fn screen(&self, (w, h): (u32, u32)) -> egui_wgpu::ScreenDescriptor {
        egui_wgpu::ScreenDescriptor {
            size_in_pixels: [w, h],
            pixels_per_point: self.pixels_per_point,
        }
    }
}
