//! GPU rendering: a vello renderer drawing into a window surface that can be transparent.

use island_ui::text::Fonts;
use island_ui::Island;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::util::TextureBlitter;
use vello::wgpu::{self, CompositeAlphaMode, PresentMode, SurfaceTarget};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

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

/// On a laptop with two graphics processors the island draws on the integrated one (see `main`), yet merely listing the
/// adapters loads every installed driver. NVIDIA's costs some 80 MB and holds its device open: it is left out when a
/// card of another maker, with its Vulkan driver installed, can draw instead. `cards` are the vendor ids of the graphics
/// cards, `drivers` the names of the Vulkan driver manifests.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn nvidia_driver_can_be_left_out(cards: &[String], drivers: &[String]) -> bool {
    let card = |vendor: &str| cards.iter().any(|c| c.trim().eq_ignore_ascii_case(vendor));
    let driver = |names: &[&str]| drivers.iter().any(|d| names.iter().any(|n| d.to_lowercase().starts_with(n)));
    card("0x10de") && ((card("0x1002") && driver(&["radeon", "amd"])) || (card("0x8086") && driver(&["intel"])))
}

/// The graphics context, and whether it was narrowed (to Vulkan, perhaps without a driver) to spare memory. The other
/// graphics interfaces load drivers of their own (OpenGL brings in a whole shader compiler), so only Vulkan is asked
/// unless it has no hardware to offer or the environment says what to use.
///
/// Called while the program has one thread: it may set an environment variable for the length of the call.
#[cfg(target_os = "linux")]
fn context() -> (RenderContext, bool) {
    let said = |names: &[&str]| names.iter().any(|n| std::env::var_os(n).is_some());
    if said(&["WGPU_BACKEND", "WGPU_ADAPTER_NAME"]) {
        return (RenderContext::new(), false);
    }
    let names = |dir: &str| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
    let cards: Vec<String> = names("/sys/class/drm").iter().filter_map(|c| std::fs::read_to_string(format!("/sys/class/drm/{c}/device/vendor")).ok()).collect();
    let drivers: Vec<String> = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().flat_map(|d| names(d)).collect();
    let low_power = std::env::var("WGPU_POWER_PREF").is_ok_and(|p| p == "low");
    let chosen_drivers = said(&["VK_LOADER_DRIVERS_DISABLE", "VK_LOADER_DRIVERS_SELECT", "VK_ICD_FILENAMES", "VK_DRIVER_FILES"]);
    if low_power && !chosen_drivers && nvidia_driver_can_be_left_out(&cards, &drivers) {
        // read by the Vulkan loader when the instance is made; taken away again so that programs the island opens
        // do not inherit it
        std::env::set_var("VK_LOADER_DRIVERS_DISABLE", "*nvidia*");
        let narrowed = vulkan_with_hardware();
        std::env::remove_var("VK_LOADER_DRIVERS_DISABLE");
        if let Some(ctx) = narrowed {
            return (ctx, true);
        }
    }
    match vulkan_with_hardware() {
        Some(ctx) => (ctx, true),
        None => (RenderContext::new(), false),
    }
}

#[cfg(target_os = "linux")]
fn vulkan_with_hardware() -> Option<RenderContext> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::from_env().unwrap_or_default(), ..Default::default() })).ok()?;
    (adapter.get_info().device_type != wgpu::DeviceType::Cpu).then(|| RenderContext { instance, devices: vec![] })
}

#[cfg(not(target_os = "linux"))]
fn context() -> (RenderContext, bool) {
    (RenderContext::new(), false)
}

pub struct Gpu {
    ctx: RenderContext,
    /// The context only knows some of the drivers: widened if it cannot show the window.
    narrowed: bool,
    renderer: Option<Renderer>,
    surface: Option<RenderSurface<'static>>,
    scene: Scene,
    /// The surface really is transparent (otherwise the window must be cut to the island's shape).
    pub transparent: bool,
    /// What the last frame cost without waiting for the display (the scene, the render and the submit).
    pub last_work: std::time::Duration,
}

impl Gpu {
    /// Call before the program starts other threads (see `context`).
    pub fn new() -> Self {
        let (ctx, narrowed) = context();
        Self { ctx, narrowed, renderer: None, surface: None, scene: Scene::new(), transparent: false, last_work: Default::default() }
    }

    /// Makes the window's surface; if the narrowed context has no adapter that can show it, again with every driver.
    fn surface_for(&mut self, make: impl Fn(&wgpu::Instance) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError>) -> Result<wgpu::Surface<'static>, String> {
        let said = |e: wgpu::CreateSurfaceError| format!("cannot create the GPU surface: {e}");
        if self.narrowed {
            let surface = make(&self.ctx.instance);
            if let Ok(surface) = surface {
                if pollster::block_on(self.ctx.device(Some(&surface))).is_some() {
                    return Ok(surface);
                }
            }
            log::info!("the Vulkan drivers asked first cannot show the window: trying every one");
            (self.ctx, self.narrowed) = (RenderContext::new(), false);
        }
        make(&self.ctx.instance).map_err(said)
    }

    /// Attaches the window surface. `target` is anything wgpu can make a surface of (a winit window).
    pub fn attach(&mut self, target: impl Into<SurfaceTarget<'static>> + Clone, width: u32, height: u32) -> Result<(), String> {
        let surface = self.surface_for(|instance| instance.create_surface(target.clone().into()))?;
        self.finish_attach(surface, width, height)
    }

    /// Attaches a surface made from raw window-system handles (the Wayland layer-shell backend).
    ///
    /// # Safety
    /// The handles must stay valid for as long as this `Gpu` draws.
    #[cfg(target_os = "linux")]
    pub unsafe fn attach_raw(&mut self, display: raw_window_handle::RawDisplayHandle, window: raw_window_handle::RawWindowHandle, width: u32, height: u32) -> Result<(), String> {
        // SAFETY: the caller vouches for the handles
        let surface = self.surface_for(|instance| unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: window }) })?;
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
            self.renderer = Some(Renderer::new(&handle.device, RendererOptions { antialiasing_support: AaSupport::area_only(), ..Default::default() }).map_err(|e| format!("cannot start the renderer: {e}"))?);
        }
        let surface = RenderSurface { surface, config, dev_id, format, target_texture, target_view, blitter };
        self.ctx.configure_surface(&surface);
        self.surface = Some(surface);
        Ok(())
    }

    /// Lets go of the window surface, before the window itself goes.
    #[cfg(target_os = "linux")]
    pub fn detach(&mut self) {
        self.surface = None;
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
    fn nvidias_driver_is_only_left_out_when_another_card_can_draw() {
        let can = |cards: &[&str], drivers: &[&str]| nvidia_driver_can_be_left_out(&cards.iter().map(|c| format!("{c}\n")).collect::<Vec<_>>(), &drivers.iter().map(|d| d.to_string()).collect::<Vec<_>>());
        assert!(can(&["0x1002", "0x10de"], &["radeon_icd.json", "nvidia_icd.json"]), "AMD beside NVIDIA");
        assert!(can(&["0x10de", "0x8086"], &["intel_icd.x86_64.json", "nvidia_icd.json"]), "Intel beside NVIDIA");
        assert!(!can(&["0x10de"], &["nvidia_icd.json", "radeon_icd.json"]), "NVIDIA alone");
        assert!(!can(&["0x1002", "0x10de"], &["nvidia_icd.json", "lvp_icd.json"]), "no driver for the other card");
        assert!(!can(&["0x8086", "0x10de"], &["radeon_icd.json", "nvidia_icd.json"]), "a driver, but not its card's");
        assert!(!can(&["0x1002"], &["radeon_icd.json"]), "nothing to leave out");
    }

    #[test]
    fn the_first_configure_already_asks_for_transparency_where_there_is_some() {
        assert_eq!(transparent_mode(&[Auto, Inherit, Opaque, PostMultiplied, PreMultiplied]), Some(PreMultiplied), "DirectComposition");
        assert_eq!(transparent_mode(&[Opaque]), None, "a plain window");
        assert_eq!(transparent_mode(&[Opaque, Inherit]), Some(Inherit));
    }
}
