//! GPU rendering: a vello renderer drawing into a window surface that can be transparent.

use island_ui::text::Fonts;
use island_ui::Island;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::util::TextureBlitter;
use vello::wgpu::{self, CompositeAlphaMode, PresentMode, SurfaceTarget};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// A frame equal to the one on screen is not rendered again, but not for longer than this: should a surface be lost
/// without a word (a graphics processor back from sleep), it is found out when the next frame is shown.
const REPAINT: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether the frame about to be drawn (what it shows, its size) is already the one on screen.
fn on_screen(shown: Option<(Drawn, std::time::Instant)>, next: Drawn, now: std::time::Instant) -> bool {
    shown.is_some_and(|(drawn, at)| drawn == next && now.duration_since(at) < REPAINT)
}

/// What a frame shows (see `fingerprint`), the surface's width and height, and the part of it rendered.
type Drawn = (u64, (u32, u32), Option<Part>);

/// The sides of a rendered part come in steps of this many pixels, so that its texture is seldom made again.
const STEP: u32 = 64;

/// A box of the surface, centred at its top: where it starts, its width and height, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    x: u32,
    width: u32,
    height: u32,
}

/// The box that holds an island `area` big (logical pixels, centred at the top) on a surface of `surface` pixels.
fn part_of(surface: (u32, u32), area: (f64, f64), scale: f64) -> Part {
    // a little to spare on each side: the pill is put on whole logical pixels
    let side = |logical: f64, most: u32| (((logical + 4.0) * scale).ceil() as u32).next_multiple_of(STEP).min(most);
    let width = side(area.0, surface.0);
    Part { x: (surface.0 - width) / 2, width, height: side(area.1, surface.1) }
}

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
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
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
    /// The frame on screen, and when it was shown. None when there is none to count on.
    shown: Option<(Drawn, std::time::Instant)>,
    /// Render only the box the island is in (see `in_parts`), and whether the surface can take it.
    wants_parts: bool,
    parts: bool,
    /// The texture of that box, its size, and the scene moved into it.
    part: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    moved: Scene,
}

impl Gpu {
    /// Call before the program starts other threads (see `context`).
    pub fn new() -> Self {
        let (ctx, narrowed) = context();
        Self { ctx, narrowed, renderer: None, surface: None, scene: Scene::new(), transparent: false, last_work: Default::default(), shown: None, wants_parts: false, parts: false, part: None, moved: Scene::new() }
    }

    /// For a window much bigger than the island in it: call before attaching. Only the box the island is in is then
    /// rendered and copied to the surface, where the surface can take a copy; the rest is cleared, which costs next
    /// to nothing.
    pub fn in_parts(mut self, wanted: bool) -> Self {
        self.wants_parts = wanted;
        self
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
        // a part is copied as it is, so the surface must have the format it is rendered in and take copies
        self.parts = self.wants_parts && caps.formats.contains(&wgpu::TextureFormat::Rgba8Unorm) && caps.usages.contains(wgpu::TextureUsages::COPY_DST);
        let format = if self.parts {
            wgpu::TextureFormat::Rgba8Unorm
        } else {
            caps.formats
                .iter()
                .copied()
                .find(|f| matches!(f, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm))
                .ok_or_else(|| format!("{CANNOT}: no 8-bit format"))?
        };
        if self.wants_parts && !self.parts {
            log::warn!("the surface takes no copies: the whole window is rendered at every frame");
        }
        let mode = transparent_mode(&caps.alpha_modes);
        match mode {
            Some(m) => log::info!("transparent surface ({m:?}) on {:?}", handle.adapter().get_info()),
            None if cfg!(target_os = "windows") => log::info!("the window surface is opaque on {:?}: the window is cut to the island's shape instead", handle.adapter().get_info()),
            None => log::warn!("no transparent surface mode: the island will have a visible rectangle behind it"),
        }
        self.transparent = mode.is_some();
        let config = wgpu::SurfaceConfiguration {
            usage: if self.parts { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST } else { wgpu::TextureUsages::RENDER_ATTACHMENT },
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
        self.shown = None;
        Ok(())
    }

    /// Lets go of the window surface, before the window itself goes.
    #[cfg(target_os = "linux")]
    pub fn detach(&mut self) {
        self.surface = None;
        self.shown = None;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(s) = self.surface.as_mut() {
            if (s.config.width, s.config.height) != (width.max(1), height.max(1)) {
                self.ctx.resize_surface(s, width.max(1), height.max(1));
            }
        }
    }

    /// Draws the island into the surface at `scale` (physical pixels per logical pixel). With `may_keep`, a frame that
    /// is the one on screen is left there: pass false when the window itself needs a new one. `area` is the room the
    /// island asks for, centred at the top of the surface: all that is rendered when drawing in parts.
    pub fn draw(&mut self, island: &mut Island, fonts: &Fonts, logical: (f64, f64), scale: f64, area: (f64, f64), may_keep: bool) -> Result<(), String> {
        let (Some(surface), Some(renderer)) = (self.surface.as_mut(), self.renderer.as_mut()) else { return Ok(()) };
        let started = std::time::Instant::now();
        self.scene.reset();
        island.draw(&mut self.scene, fonts, logical, scale);
        let size = (surface.config.width, surface.config.height);
        let part = self.parts.then(|| part_of(size, area, scale));
        let next = (crate::fingerprint::of(&self.scene), size, part);
        if may_keep && on_screen(self.shown, next, started) {
            self.last_work = started.elapsed();
            return Ok(());
        }
        self.shown = None;
        let handle = &self.ctx.devices[surface.dev_id];
        let (scene, view, (width, height)) = match part {
            Some(p) => {
                if self.part.as_ref().is_none_or(|(.., made)| *made != (p.width, p.height)) {
                    let (texture, view) = target(&handle.device, p.width, p.height);
                    self.part = Some((texture, view, (p.width, p.height)));
                }
                let Some((_, view, _)) = self.part.as_ref() else { return Ok(()) };
                self.moved.reset();
                self.moved.append(&self.scene, Some(Affine::translate((-f64::from(p.x), 0.0))));
                (&self.moved, view, (p.width, p.height))
            }
            None => (&self.scene, &surface.target_view, size),
        };
        renderer
            .render_to_texture(
                &handle.device,
                &handle.queue,
                scene,
                view,
                &RenderParams { base_color: Color::from_rgba8(0, 0, 0, 0), width, height, antialiasing_method: AaConfig::Area },
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
        match (part, self.part.as_ref()) {
            (Some(p), Some((texture, ..))) => {
                clear(&mut enc, &view);
                let mut to = frame.texture.as_image_copy();
                to.origin.x = p.x;
                enc.copy_texture_to_texture(texture.as_image_copy(), to, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
            }
            _ => surface.blitter.copy(&handle.device, &mut enc, &surface.target_view, &view),
        }
        handle.queue.submit([enc.finish()]);
        handle.queue.present(frame);
        self.shown = Some((next, started));
        Ok(())
    }
}

/// Makes the whole of a surface see-through.
fn clear(enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let whole = wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } };
    let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("clear"), color_attachments: &[Some(whole)], ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use CompositeAlphaMode::*;

    #[test]
    fn a_frame_is_kept_only_while_it_is_the_same_the_same_size_and_recent() {
        let at = std::time::Instant::now();
        let drawn = (7, (200, 60), None);
        let shown = Some((drawn, at));
        assert!(on_screen(shown, drawn, at + REPAINT / 2));
        assert!(!on_screen(shown, (8, (200, 60), None), at), "it shows something else");
        assert!(!on_screen(shown, (7, (201, 60), None), at), "another size");
        assert!(!on_screen(shown, (7, (200, 60), Some(Part { x: 0, width: 64, height: 60 })), at), "another box of it");
        assert!(!on_screen(shown, drawn, at + REPAINT), "painted again now and then");
        assert!(!on_screen(None, drawn, at));
    }

    #[test]
    fn the_part_rendered_holds_the_island_stays_inside_the_surface_and_changes_in_steps() {
        // Windows' fixed window of 820 x 1080 at 150 %: the small pill is a sliver of it
        let surface = (1230, 1620);
        let p = part_of(surface, (158.0, 40.0), 1.5);
        assert_eq!((p.width, p.height), (256, 128));
        let (left, right) = ((1230.0 - 158.0 * 1.5) / 2.0, (1230.0 + 158.0 * 1.5) / 2.0);
        assert!(f64::from(p.x) <= left - 3.0 && f64::from(p.x + p.width) >= right + 3.0, "{p:?}");
        assert!(p.width * p.height * 50 < surface.0 * surface.1);
        assert_eq!(part_of(surface, (160.0, 41.0), 1.5), p, "a little more is the same box");
        // never beyond the surface
        let all = part_of(surface, (900.0, 2000.0), 1.5);
        assert_eq!(all, Part { x: 0, width: 1230, height: 1620 });
    }

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
