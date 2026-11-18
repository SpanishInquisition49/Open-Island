//! GPU rendering: a vello renderer drawing into a window surface that can be transparent.

use island_ui::text::Fonts;
use island_ui::Island;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::{self, CompositeAlphaMode, PresentMode, SurfaceTarget};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

pub struct Gpu {
    ctx: RenderContext,
    renderer: Option<Renderer>,
    surface: Option<RenderSurface<'static>>,
    scene: Scene,
}

impl Gpu {
    pub fn new() -> Self {
        Self { ctx: RenderContext::new(), renderer: None, surface: None, scene: Scene::new() }
    }

    /// Attaches the window surface. `target` is anything wgpu can make a surface of (a winit window).
    pub fn attach(&mut self, target: impl Into<SurfaceTarget<'static>>, width: u32, height: u32) -> Result<(), String> {
        let surface = self.ctx.instance.create_surface(target.into()).map_err(|e| format!("cannot create the GPU surface: {e}"))?;
        self.finish_attach(surface, width, height)
    }

    /// Attaches a surface made from raw window-system handles (the Wayland layer-shell backend).
    ///
    /// # Safety
    /// The handles must stay valid for as long as this `Gpu` draws.
    #[cfg(target_os = "linux")]
    pub unsafe fn attach_raw(&mut self, display: raw_window_handle::RawDisplayHandle, window: raw_window_handle::RawWindowHandle, width: u32, height: u32) -> Result<(), String> {
        let surface = self
            .ctx
            .instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: window })
            .map_err(|e| format!("cannot create the GPU surface: {e}"))?;
        self.finish_attach(surface, width, height)
    }

    fn finish_attach(&mut self, surface: wgpu::Surface<'static>, width: u32, height: u32) -> Result<(), String> {
        let mut surface = pollster::block_on(self.ctx.create_render_surface(surface, width.max(1), height.max(1), PresentMode::AutoVsync))
            .map_err(|e| format!("cannot create the GPU surface: {e}"))?;
        // a transparent window needs an alpha mode that is not opaque
        let caps = surface.surface.get_capabilities(self.ctx.devices[surface.dev_id].adapter());
        let mode = [CompositeAlphaMode::PreMultiplied, CompositeAlphaMode::PostMultiplied, CompositeAlphaMode::Inherit]
            .into_iter()
            .find(|m| caps.alpha_modes.contains(m));
        match mode {
            Some(m) => {
                surface.config.alpha_mode = m;
                self.ctx.configure_surface(&surface);
            }
            None => log::warn!("no transparent surface mode: the island will have a visible rectangle behind it"),
        }
        let device = &self.ctx.devices[surface.dev_id].device;
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(device, RendererOptions::default()).map_err(|e| format!("cannot start the renderer: {e}"))?);
        }
        self.surface = Some(surface);
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(s) = self.surface.as_mut() {
            if (s.config.width, s.config.height) != (width.max(1), height.max(1)) {
                self.ctx.resize_surface(s, width.max(1), height.max(1));
            }
        }
    }

    /// Draws the island into the surface at `scale` (physical pixels per logical pixel).
    pub fn draw(&mut self, island: &mut Island, fonts: &Fonts, logical: (f64, f64), scale: f64) -> Result<(), String> {
        let (Some(surface), Some(renderer)) = (self.surface.as_mut(), self.renderer.as_mut()) else { return Ok(()) };
        self.scene.reset();
        island.draw(&mut self.scene, fonts, logical, scale);
        let (w, h) = (surface.config.width, surface.config.height);
        let handle = &self.ctx.devices[surface.dev_id];
        renderer
            .render_to_texture(
                &handle.device,
                &handle.queue,
                &self.scene,
                &surface.target_view,
                &RenderParams { base_color: Color::from_rgba8(0, 0, 0, 0), width: w, height: h, antialiasing_method: AaConfig::Area },
            )
            .map_err(|e| format!("render failed: {e}"))?;
        let frame = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.ctx.configure_surface(surface);
                return Ok(());
            }
            _ => return Ok(()), // timeout, occluded: skip this frame
        };
        let mut enc = handle.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("blit") });
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        surface.blitter.copy(&handle.device, &mut enc, &surface.target_view, &view);
        handle.queue.submit([enc.finish()]);
        handle.queue.present(frame);
        let _ = Affine::IDENTITY;
        Ok(())
    }
}
