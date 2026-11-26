//! GPU rendering: a vello renderer drawing into a window surface that can be transparent.

use island_ui::text::Fonts;
use island_ui::Island;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::util::TextureBlitter;
use vello::wgpu::{self, CompositeAlphaMode, PresentMode, SurfaceTarget};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

/// The alpha mode that lets what is behind the window show through, if the surface has one.
fn transparent_mode(offered: &[CompositeAlphaMode]) -> Option<CompositeAlphaMode> {
    [CompositeAlphaMode::PreMultiplied, CompositeAlphaMode::PostMultiplied, CompositeAlphaMode::Inherit].into_iter().find(|m| offered.contains(m))
}

/// The texture vello draws into, copied to the surface afterwards (as vello's own helper makes it).
fn target(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        format: wgpu::TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub struct Gpu {
    ctx: RenderContext,
    renderer: Option<Renderer>,
    surface: Option<RenderSurface<'static>>,
    scene: Scene,
    /// The surface really is transparent (otherwise the window must be cut to the island's shape).
    pub transparent: bool,
    /// What the last frame cost without waiting for the display (the scene, the render and the submit).
    pub last_work: std::time::Duration,
}

impl Gpu {
    pub fn new() -> Self {
        Self { ctx: RenderContext::new(), renderer: None, surface: None, scene: Scene::new(), transparent: false, last_work: Default::default() }
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

    /// Configures the surface once, with the alpha mode it will keep. Not through vello's `create_render_surface`,
    /// which configures with `Auto` first: DirectX 12 fixes the alpha mode when it creates the swap chain and a later
    /// configure only resizes it, so on Windows the surface stayed opaque while the island believed it transparent
    /// (a black rectangle behind it while it moved, cut round when it stopped).
    fn finish_attach(&mut self, surface: wgpu::Surface<'static>, width: u32, height: u32) -> Result<(), String> {
        const CANNOT: &str = "cannot create the GPU surface";
        let dev_id = pollster::block_on(self.ctx.device(Some(&surface))).ok_or_else(|| format!("{CANNOT}: no graphics adapter can show it"))?;
        let handle = &self.ctx.devices[dev_id];
        let caps = surface.get_capabilities(handle.adapter());
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm))
            .ok_or_else(|| format!("{CANNOT}: no 8-bit format"))?;
        let mode = transparent_mode(&caps.alpha_modes);
        match mode {
            Some(m) => log::info!("transparent surface ({m:?}) on {:?}", handle.adapter().get_info()),
            None if cfg!(target_os = "windows") => log::info!("the window surface is opaque on {:?}: the window is cut to the island's shape instead", handle.adapter().get_info()),
            None => log::warn!("no transparent surface mode: the island will have a visible rectangle behind it"),
        }
        self.transparent = mode.is_some();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::AutoVsync,
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 2,
            alpha_mode: mode.unwrap_or(CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        let (target_texture, target_view) = target(&handle.device, config.width, config.height);
        let blitter = TextureBlitter::new(&handle.device, format);
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(&handle.device, RendererOptions::default()).map_err(|e| format!("cannot start the renderer: {e}"))?);
        }
        let surface = RenderSurface { surface, config, dev_id, format, target_texture, target_view, blitter };
        self.ctx.configure_surface(&surface);
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
        let started = std::time::Instant::now();
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
        self.last_work = started.elapsed();
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

#[cfg(test)]
mod tests {
    use super::*;
    use CompositeAlphaMode::*;

    /// What wgpu 30 offers for the two kinds of surface DirectX 12 has.
    #[test]
    fn the_first_configure_already_asks_for_transparency_where_there_is_some() {
        assert_eq!(transparent_mode(&[Auto, Inherit, Opaque, PostMultiplied, PreMultiplied]), Some(PreMultiplied), "DirectComposition");
        assert_eq!(transparent_mode(&[Opaque]), None, "a plain window");
        assert_eq!(transparent_mode(&[Opaque, Inherit]), Some(Inherit));
    }
}
