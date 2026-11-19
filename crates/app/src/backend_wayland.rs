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
    delegate_keyboard, delegate_data_device,
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::DataSourceHandler,
        WritePipe,
        DataDeviceManagerState,
    },
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
    protocol::{wl_data_device::WlDataDevice, wl_data_source::WlDataSource, wl_data_device_manager::DndAction, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

/// What a file manager offers when a file is dragged.
const URI_LIST: &str = "text/uri-list";
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

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
    idle_manager: Option<ExtIdleNotifierV1>,
    idle: Option<ExtIdleNotificationV1>,
    /// When the person went idle (no keyboard or pointer for `idle_secs`), until they come back.
    idle_since: Option<Instant>,
    idle_secs: u32,
    data_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    /// Paths of files dropped on the island (read on another thread).
    drop_tx: channel::Sender<Vec<String>>,
    loop_handle: calloop::LoopHandle<'static, Wl>,
    keyboard_on: bool,
    ctrl: bool,
    alt: bool,
    app: App,
    gpu: Gpu,
    scale: f64,
    size: (u32, u32),
    configured: bool,
    dirty: bool,
    animating: bool,
    next_ms: u32,
    exit: bool,
    last_render: Instant,
    input_rect: Option<(i32, i32, i32, i32)>,
    on_top_layer: bool,
    /// OPEN_ISLAND_NO_POINTER=1: the surface takes no pointer input (screenshots and tests)
    no_pointer: bool,
    perf: crate::perf::Perf,
}

impl Wl {
    fn render(&mut self, qh: &QueueHandle<Self>) {
        let f = self.app.tick();
        if self.app.quit {
            self.exit = true;
            return;
        }
        // fullscreen windows: drop under them (overlay -> top) so the island hides, and come back after
        let under = self.app.fullscreen_hides();
        if under != self.on_top_layer {
            self.on_top_layer = under;
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
        self.perf.frame(f.animating, self.gpu.last_work);
        self.animating = f.animating;
        self.next_ms = f.next_ms;
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
        // told when the person has not touched the keyboard or the pointer for a while, and when they are back
        if self.idle.is_none() {
            if let Some(m) = &self.idle_manager {
                let ms = self.idle_secs * 1000;
                log::debug!("watching for idleness after {} s", self.idle_secs);
                self.idle = Some(if m.version() >= 2 { m.get_input_idle_notification(ms, &seat, qh, ()) } else { m.get_idle_notification(ms, &seat, qh, ()) });
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            // files dragged onto the island
            if self.data_device.is_none() {
                self.data_device = self.data_manager.as_ref().map(|m| m.get_data_device(qh, &seat));
            }
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
                    BTN_MIDDLE => self.app.middle_click(),
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

/// The text on the clipboard, for Ctrl+V in a text field (single line, at most 2000 characters).
fn paste() -> Option<String> {
    let out = std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text"]).stderr(std::process::Stdio::null()).output().ok()?;
    let t: String = String::from_utf8_lossy(&out.stdout).chars().take(2000).collect();
    (!t.is_empty()).then_some(t)
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
            Keysym::Tab | Keysym::Page_Down => Some(KeyInput::NextPage),
            Keysym::ISO_Left_Tab | Keysym::Page_Up => Some(KeyInput::PrevPage),
            Keysym::Up => Some(KeyInput::Up),
            Keysym::Down => Some(KeyInput::Down),
            Keysym::Return | Keysym::KP_Enter => Some(KeyInput::Enter),
            Keysym::Escape => Some(KeyInput::Escape),
            Keysym::v | Keysym::V if self.ctrl && !self.alt => paste().map(KeyInput::Text),
            _ if self.ctrl || self.alt => None,
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
        self.ctrl = modifiers.ctrl;
        self.alt = modifiers.alt;
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

impl Dispatch<ExtIdleNotifierV1, ()> for Wl {
    fn event(_: &mut Self, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotificationV1, ()> for Wl {
    fn event(state: &mut Self, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                log::debug!("idle");
                state.idle_since = Some(Instant::now());
                state.app.input(island_ui::model::Input::Idled { after_ms: state.idle_secs as i64 * 1000 });
            }
            ext_idle_notification_v1::Event::Resumed => {
                if let Some(since) = state.idle_since.take() {
                    let away_ms = since.elapsed().as_millis() as i64 + state.idle_secs as i64 * 1000;
                    log::debug!("back after {away_ms} ms");
                    state.app.input(island_ui::model::Input::Returned { away_ms });
                    state.dirty = true;
                }
            }
            _ => {}
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
delegate_data_device!(Wl);

impl Wl {
    /// The file drag now over the island, when it carries files: accepted as a copy.
    fn drag_over(&mut self) {
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().drag_offer()) else { return };
        if offer.with_mime_types(|m| m.iter().any(|t| t == URI_LIST)) {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
            offer.accept_mime_type(offer.serial, Some(URI_LIST.to_string()));
            // the pill opens so the drop has a target to see
            self.app.input(island_ui::model::Input::Peek);
            self.dirty = true;
        }
    }
}

impl DataDeviceHandler for Wl {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {
        self.drag_over();
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {
        self.drag_over();
    }
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().drag_offer()) else { return };
        let Ok(mut pipe) = offer.receive(URI_LIST.to_string()) else { return };
        let tx = self.drop_tx.clone();
        // the file manager writes the list when it is ready: read it away from the event loop
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
            offer.finish();
            let _ = tx.send(island_core::localsend::uri_list_paths(&text));
        });
    }
}

impl DataOfferHandler for Wl {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

/// Reads the outputs on a queue of its own (the main one has no state to take their events yet) and returns the one
/// whose connector name is `name` (`eDP-1`); matching is not case sensitive.
fn pick_output(conn: &Connection, name: &str) -> Option<wl_output::WlOutput> {
    struct Probe {
        registry_state: RegistryState,
        output_state: OutputState,
    }
    impl OutputHandler for Probe {
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.output_state
        }
        fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
        fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
        fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    }
    impl ProvidesRegistryState for Probe {
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry_state
        }
        registry_handlers![OutputState];
    }
    delegate_output!(Probe);
    delegate_registry!(Probe);

    let (globals, mut queue) = registry_queue_init::<Probe>(conn).ok()?;
    let mut probe = Probe { registry_state: RegistryState::new(&globals), output_state: OutputState::new(&globals, &queue.handle()) };
    // the outputs announce themselves, then describe themselves: two round trips
    queue.roundtrip(&mut probe).ok()?;
    queue.roundtrip(&mut probe).ok()?;
    let found = probe.output_state.outputs().find(|o| probe.output_state.info(o).and_then(|i| i.name).is_some_and(|n| n.eq_ignore_ascii_case(name)));
    if found.is_none() {
        let names: Vec<String> = probe.output_state.outputs().filter_map(|o| probe.output_state.info(&o).and_then(|i| i.name)).collect();
        log::warn!("the screens are: {}", names.join(", "));
    } else {
        log::info!("the island is pinned to {name}");
    }
    found
}

pub fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, event_queue) = registry_queue_init::<Wl>(&conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("no wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| "the compositor has no wlr-layer-shell".to_string())?;

    let surface = compositor.create_surface(&qh);
    // the `screen` option: a connector name pins the island to that screen, "focused" lets the compositor choose
    let screen = crate::config::load().screen;
    let output = if screen == "focused" { None } else { pick_output(&conn, &screen) };
    if output.is_none() && screen != "focused" {
        log::warn!("screen {screen:?} not found: the island goes where the compositor puts it");
    }
    let layer = layer_shell.create_layer_surface(&qh, surface.clone(), Layer::Overlay, Some("open-island"), output.as_ref());
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
    let (drop_tx, drop_rx) = channel::channel::<Vec<String>>();
    event_loop
        .handle()
        .insert_source(drop_rx, |ev, _, state: &mut Wl| {
            if let channel::Event::Msg(paths) = ev {
                if !paths.is_empty() {
                    state.app.input(island_ui::model::Input::SendFiles(paths));
                }
                state.dirty = true;
            }
        })
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
        idle_manager: globals.bind::<ExtIdleNotifierV1, _, _>(&qh, 1..=2, ()).ok(),
        idle: None,
        idle_since: None,
        idle_secs: std::env::var("OPEN_ISLAND_IDLE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(120),
        data_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        drop_tx,
        loop_handle: event_loop.handle(),
        keyboard_on: false,
        ctrl: false,
        alt: false,
        app: App::new(waker),
        gpu,
        scale: 1.0,
        size: (200, 60),
        configured: false,
        dirty: true,
        animating: true,
        next_ms: 0,
        exit: false,
        last_render: Instant::now(),
        input_rect: None,
        on_top_layer: false,
        no_pointer: std::env::var_os("OPEN_ISLAND_NO_POINTER").is_some(),
        perf: crate::perf::Perf::from_env(),
    };
    log::info!("layer-shell surface created; away detection {}", if state.idle_manager.is_some() { "on" } else { "unavailable (no ext-idle-notify)" });
    loop {
        // frames only when something needs them: at vsync while the shape moves, otherwise when `next_ms` has passed
        let due = Duration::from_millis(state.next_ms as u64);
        let timeout = if state.animating || state.dirty { Duration::ZERO } else { due.saturating_sub(state.last_render.elapsed()) };
        event_loop.dispatch(timeout, &mut state).map_err(|e| e.to_string())?;
        if state.exit {
            return Ok(());
        }
        if state.configured && (state.animating || state.dirty || state.last_render.elapsed() >= due) {
            state.render(&qh);
        }
    }
}

/// The island only receives drags; the trait is needed by the delegate and nothing is ever offered.
impl DataSourceHandler for Wl {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: String, _: WritePipe) {}
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}
