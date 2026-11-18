//! The Wayland backend: a wlr-layer-shell surface anchored to the top centre of the screen, drawn with
//! fractional scaling (wp-fractional-scale + wp-viewporter) and a click-through transparent margin.
//! Works on niri, sway, Hyprland, KDE and every other compositor with wlr-layer-shell; GNOME falls back
//! to the plain window backend.

use crate::app::{App, Waker};
use crate::gpu::Gpu;
use calloop::{channel, EventLoop};
use calloop_wayland_source::WaylandSource;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    delegate_keyboard,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

struct Wl {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer: LayerSurface,
    viewport: Option<WpViewport>,
    _fractional: Option<WpFractionalScaleV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    loop_handle: calloop::LoopHandle<'static, Wl>,
    keyboard_on: bool,
    ctrl: bool,
    app: App,
    gpu: Gpu,
    scale: f64,
    size: (u32, u32),
    configured: bool,
    dirty: bool,
    animating: bool,
    exit: bool,
    last_render: Instant,
    input_rect: Option<(i32, i32, i32, i32)>,
    on_top_layer: bool,
    /// OPEN_ISLAND_NO_POINTER=1: the surface takes no pointer input (screenshots and tests)
    no_pointer: bool,
}

impl Wl {
    fn render(&mut self, qh: &QueueHandle<Self>) {
        let f = self.app.tick();
        if self.app.quit {
            self.exit = true;
            return;
        }
        // fullscreen windows: drop under them (overlay -> top) so the island hides, and come back after
        if self.app.fullscreen != self.on_top_layer {
            self.on_top_layer = self.app.fullscreen;
            self.layer.set_layer(if self.on_top_layer { Layer::Top } else { Layer::Overlay });
        }
        // the keyboard is only taken while an inline reply is being typed
        if self.app.island.wants_keyboard() != self.keyboard_on {
            self.keyboard_on = !self.keyboard_on;
            self.layer.set_keyboard_interactivity(if self.keyboard_on { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
        }
        let (lw, lh) = (f.surface.0.ceil().max(1.0) as u32, f.surface.1.ceil().max(1.0) as u32);
        if (lw, lh) != self.size {
            self.size = (lw, lh);
            self.layer.set_size(lw, lh);
            if let Some(v) = &self.viewport {
                v.set_destination(lw as i32, lh as i32);
            }
        }
        let (bw, bh) = ((lw as f64 * self.scale).round() as u32, (lh as f64 * self.scale).round() as u32);
        self.gpu.resize(bw, bh);
        // only the island itself takes clicks
        let (x, y, w, h) = self.app.island.input_rect();
        let rect = (x.floor() as i32, y.floor() as i32, w.ceil() as i32, h.ceil() as i32);
        if self.input_rect != Some(rect) {
            self.input_rect = Some(rect);
            if let Ok(region) = Region::new(&self.compositor) {
                if !self.no_pointer {
                    region.add(rect.0, rect.1, rect.2, rect.3);
                }
                self.layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
        }
        if let Err(e) = self.gpu.draw(&mut self.app.island, &self.app.fonts, (lw as f64, lh as f64), self.scale) {
            log::error!("{e}");
        }
        self.animating = f.animating;
        self.dirty = false;
        self.last_render = Instant::now();
        let _ = qh;
    }
}

impl CompositorHandler for Wl {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Wl {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Wl {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.configured = true;
        self.dirty = true;
    }
}

impl SeatHandler for Wl {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            // with key repeat, so holding Backspace deletes
            self.keyboard = self
                .seat_state
                .get_keyboard_with_repeat(qh, &seat, None, self.loop_handle.clone(), Box::new(|state, _kbd, event| state.key_event(event)))
                .ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Wl {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = e.position;
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.app.pointer_moved(x, y),
                PointerEventKind::Leave { .. } => self.app.pointer_left(),
                PointerEventKind::Press { button, .. } => match button {
                    BTN_LEFT => self.app.pointer_down(x, y),
                    BTN_RIGHT => self.app.right_click(),
                    _ => {}
                },
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => self.app.pointer_up(),
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    // wl_pointer scroll: positive is down/right; the island treats positive dy as "up"
                    let (dx, dy) = (horizontal.absolute / 10.0, -vertical.absolute / 10.0);
                    self.app.scroll(dx, dy);
                }
                _ => {}
            }
        }
        self.dirty = true;
    }
}

impl Wl {
    fn key_event(&mut self, e: KeyEvent) {
        use island_ui::model::KeyInput;
        let k = match e.keysym {
            Keysym::BackSpace => Some(KeyInput::Backspace),
            Keysym::Delete => Some(KeyInput::Delete),
            Keysym::Left => Some(KeyInput::Left),
            Keysym::Right => Some(KeyInput::Right),
            Keysym::Home => Some(KeyInput::Home),
            Keysym::End => Some(KeyInput::End),
            Keysym::Return | Keysym::KP_Enter => Some(KeyInput::Enter),
            Keysym::Escape => Some(KeyInput::Escape),
            _ if self.ctrl => None,
            _ => e.utf8.filter(|t| !t.is_empty()).map(KeyInput::Text),
        };
        if let Some(k) = k {
            self.app.key(k);
            self.dirty = true;
        }
    }
}

impl KeyboardHandler for Wl {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key_event(event);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key_event(event);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: smithay_client_toolkit::seat::keyboard::RawModifiers, _: u32) {
        self.ctrl = modifiers.ctrl || modifiers.alt;
    }
}

impl ProvidesRegistryState for Wl {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for Wl {
    fn event(_: &mut Self, _: &WpFractionalScaleManagerV1, _: <WpFractionalScaleManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpFractionalScaleV1, ()> for Wl {
    fn event(state: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.scale = scale as f64 / 120.0;
            state.dirty = true;
        }
    }
}

impl Dispatch<WpViewporter, ()> for Wl {
    fn event(_: &mut Self, _: &WpViewporter, _: <WpViewporter as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpViewport, ()> for Wl {
    fn event(_: &mut Self, _: &WpViewport, _: <WpViewport as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_compositor!(Wl);
delegate_output!(Wl);
delegate_seat!(Wl);
delegate_pointer!(Wl);
delegate_keyboard!(Wl);
delegate_layer!(Wl);
delegate_registry!(Wl);

pub fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, event_queue) = registry_queue_init::<Wl>(&conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("no wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| "the compositor has no wlr-layer-shell".to_string())?;

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface.clone(), Layer::Overlay, Some("open-island"), None);
    layer.set_anchor(Anchor::TOP);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(200, 60);
    // OPEN_ISLAND_TOP_OFFSET (logical px) moves it down: for testing next to another island
    if let Some(dy) = std::env::var("OPEN_ISLAND_TOP_OFFSET").ok().and_then(|v| v.parse::<i32>().ok()) {
        layer.set_margin(dy, 0, 0, 0);
    }
    // fractional scaling and the viewport that maps the buffer to the logical size
    let viewport = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok().map(|v| v.get_viewport(&surface, &qh, ()));
    let fractional = globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok().map(|m| m.get_fractional_scale(&surface, &qh, ()));
    if viewport.is_none() || fractional.is_none() {
        log::warn!("no fractional scaling: the island will look soft on scaled displays");
    }
    layer.commit();

    let mut event_loop: EventLoop<Wl> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let (wake_tx, wake_rx) = channel::channel::<()>();
    let wake_tx = std::sync::Mutex::new(wake_tx);
    let waker: Waker = Arc::new(move || {
        if let Ok(tx) = wake_tx.lock() {
            let _ = tx.send(());
        }
    });
    event_loop
        .handle()
        .insert_source(wake_rx, |_, _, state: &mut Wl| state.dirty = true)
        .map_err(|e| e.to_string())?;
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle()).map_err(|e| e.to_string())?;

    let mut gpu = Gpu::new();
    let display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(conn.backend().display_ptr() as *mut _).ok_or("no display pointer")?));
    let window = RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(surface.id().as_ptr() as *mut _).ok_or("no surface pointer")?));
    // SAFETY: the display connection and the surface outlive `gpu` (both are owned by `state` below).
    unsafe { gpu.attach_raw(display, window, 200, 60)? };

    let mut state = Wl {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        layer,
        viewport,
        _fractional: fractional,
        pointer: None,
        keyboard: None,
        loop_handle: event_loop.handle(),
        keyboard_on: false,
        ctrl: false,
        app: App::new(waker),
        gpu,
        scale: 1.0,
        size: (200, 60),
        configured: false,
        dirty: true,
        animating: true,
        exit: false,
        last_render: Instant::now(),
        input_rect: None,
        on_top_layer: false,
        no_pointer: std::env::var_os("OPEN_ISLAND_NO_POINTER").is_some(),
    };
    log::info!("layer-shell surface created");
    loop {
        let timeout = if state.animating || state.dirty { Duration::ZERO } else { Duration::from_millis(1000) };
        event_loop.dispatch(timeout, &mut state).map_err(|e| e.to_string())?;
        if state.exit {
            return Ok(());
        }
        if state.configured && (state.animating || state.dirty || state.last_render.elapsed() >= Duration::from_millis(1000)) {
            state.render(&qh);
        }
    }
}
