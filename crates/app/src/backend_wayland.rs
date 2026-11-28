//! The Wayland backend: a wlr-layer-shell surface anchored to the top centre of the screen, drawn with
//! fractional scaling (wp-fractional-scale + wp-viewporter) and a click-through transparent margin.
//! Works on niri, sway, Hyprland, KDE and every other compositor with wlr-layer-shell; GNOME falls back
//! to the plain window backend.

use crate::app::{until_frame, App, Waker};
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
        data_source::{DataSourceHandler, DragSource},
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
const TEXT: &str = "text/plain;charset=utf-8";
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
    fractional: Option<WpFractionalScaleV1>,
    /// What a new surface is made with, when the one there was goes with its screen.
    conn: Connection,
    shell: Shell,
    /// The compositor took the surface away: a new one is made once there is a screen, not before `retry_at`.
    lost: bool,
    retry_at: Instant,
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
    /// The last press of a button (a drag starts from one), and the drag of the island's own files under way.
    press_serial: u32,
    outgoing: Option<(DragSource, Vec<String>)>,
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
    /// The compositor configured the surface since the last frame: the next one must be shown, even if it is the same.
    reshow: bool,
    input_rects: Vec<(i32, i32, i32, i32)>,
    on_top_layer: bool,
    /// The screen the surface is on, for its height.
    on_output: Option<wl_output::WlOutput>,
    /// OPEN_ISLAND_NO_POINTER=1: the surface takes no pointer input (screenshots and tests)
    no_pointer: bool,
    perf: crate::perf::Perf,
}

/// The globals a surface is made from.
struct Shell {
    layer: LayerShell,
    viewporter: Option<WpViewporter>,
    fractional: Option<WpFractionalScaleManagerV1>,
}

struct Made {
    layer: LayerSurface,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
}

/// The island's surface, anchored at the top of the screen asked for (or the one the compositor chooses).
fn make_surface(conn: &Connection, compositor: &CompositorState, shell: &Shell, qh: &QueueHandle<Wl>) -> Made {
    let surface = compositor.create_surface(qh);
    // the `screen` option: a connector name pins the island to that screen, "focused" lets the compositor choose
    let screen = crate::config::load().screen;
    let output = if screen == "focused" { None } else { pick_output(conn, &screen) };
    if output.is_none() && screen != "focused" {
        log::warn!("screen {screen:?} not found: the island goes where the compositor puts it");
    }
    let layer = shell.layer.create_layer_surface(qh, surface.clone(), Layer::Overlay, Some("open-island"), output.as_ref());
    layer.set_anchor(Anchor::TOP);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(200, 60);
    // OPEN_ISLAND_TOP_OFFSET (logical px) moves it down: for testing next to another island
    if let Some(dy) = std::env::var("OPEN_ISLAND_TOP_OFFSET").ok().and_then(|v| v.parse::<i32>().ok()) {
        layer.set_margin(dy, 0, 0, 0);
    }
    // fractional scaling and the viewport that maps the buffer to the logical size
    let viewport = shell.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, ()));
    let fractional = shell.fractional.as_ref().map(|m| m.get_fractional_scale(&surface, qh, ()));
    layer.commit();
    Made { layer, viewport, fractional }
}

/// Draws into the surface of `layer`.
fn attach(gpu: &mut Gpu, conn: &Connection, layer: &LayerSurface) -> Result<(), String> {
    let display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(conn.backend().display_ptr() as *mut _).ok_or("no display pointer")?));
    let window = RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).ok_or("no surface pointer")?));
    // SAFETY: the display connection and the surface outlive what the GPU draws into: both are kept beside it, and the
    // GPU surface is dropped (`detach`) before its Wayland surface is replaced.
    unsafe { gpu.attach_raw(display, window, 200, 60) }
}

impl Wl {
    /// A new surface in place of one the compositor closed (its screen was switched off or unplugged).
    fn rebuild(&mut self, qh: &QueueHandle<Self>) {
        self.retry_at = Instant::now() + Duration::from_secs(1);
        // the old one goes first: what draws into it, then what hangs from it, then the surface itself
        self.gpu.detach();
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        let made = make_surface(&self.conn, &self.compositor, &self.shell, qh);
        (self.layer, self.viewport, self.fractional) = (made.layer, made.viewport, made.fractional);
        self.configured = false;
        self.size = (200, 60);
        self.on_top_layer = false;
        self.keyboard_on = false;
        self.outgoing = None;
        self.on_output = None;
        self.input_rects.clear();
        self.app.pointer_left();
        self.app.island.keyboard_focus(false);
        match attach(&mut self.gpu, &self.conn, &self.layer) {
            Ok(()) => {
                log::info!("a new surface after the screen changed");
                self.lost = false;
                self.dirty = true;
            }
            Err(e) => log::warn!("{e}: trying again"),
        }
    }

    fn render(&mut self, qh: &QueueHandle<Self>) {
        // where the middle of the screen is, for what opens there
        let height = self.on_output.as_ref().and_then(|o| self.output_state.info(o)).and_then(|i| i.logical_size).map(|(_, h)| f64::from(h));
        self.app.island.set_screen_height(height);
        let f = self.app.tick();
        if self.app.quit {
            self.exit = true;
            return;
        }
        // fullscreen windows: drop under them (overlay -> top) so the island hides, and come back after
        let under = self.app.fullscreen_hides();
        // what changes about the surface takes effect with the next frame shown, so such a change shows one
        let mut changed = std::mem::take(&mut self.reshow);
        if under != self.on_top_layer {
            changed = true;
            self.on_top_layer = under;
            self.layer.set_layer(if self.on_top_layer { Layer::Top } else { Layer::Overlay });
        }
        // the keyboard is only taken while an inline reply is being typed
        if self.app.island.wants_keyboard() != self.keyboard_on {
            changed = true;
            self.keyboard_on = !self.keyboard_on;
            self.layer.set_keyboard_interactivity(if self.keyboard_on { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
        }
        let (lw, lh) = (f.surface.0.ceil().max(1.0) as u32, f.surface.1.ceil().max(1.0) as u32);
        if (lw, lh) != self.size {
            changed = true;
            self.size = (lw, lh);
            self.layer.set_size(lw, lh);
            if let Some(v) = &self.viewport {
                v.set_destination(lw as i32, lh as i32);
            }
        }
        let (bw, bh) = ((lw as f64 * self.scale).round() as u32, (lh as f64 * self.scale).round() as u32);
        self.gpu.resize(bw, bh);
        // only the island itself takes clicks
        let rects: Vec<(i32, i32, i32, i32)> = self.app.island.input_rects().into_iter().map(|(x, y, w, h)| (x.floor() as i32, y.floor() as i32, w.ceil() as i32, h.ceil() as i32)).collect();
        if self.input_rects != rects {
            changed = true;
            self.input_rects = rects.clone();
            if let Ok(region) = Region::new(&self.compositor) {
                if !self.no_pointer {
                    for (x, y, w, h) in rects {
                        region.add(x, y, w, h);
                    }
                }
                self.layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
        }
        // while the shape moves each frame waits for the display, which is what paces the loop: all of them are shown
        let may_keep = !changed && !f.animating;
        if let Err(e) = self.gpu.draw(&mut self.app.island, &self.app.fonts, (lw as f64, lh as f64), self.scale, may_keep) {
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
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, output: &wl_output::WlOutput) {
        if surface == self.layer.wl_surface() {
            self.on_output = Some(output.clone());
        }
    }
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, closed: &LayerSurface) {
        // word of a surface already replaced is nothing
        if closed.wl_surface() == self.layer.wl_surface() {
            log::info!("the surface was closed: its screen is gone");
            (self.lost, self.configured) = (true, false);
        }
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.configured = true;
        self.dirty = true;
        self.reshow = true;
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
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = e.position;
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.app.pointer_moved(x, y),
                PointerEventKind::Leave { .. } => self.app.pointer_left(),
                PointerEventKind::Press { button, serial, .. } => match button {
                    BTN_LEFT => {
                        self.press_serial = serial;
                        // a drag under way holds the pointer, so a press means there is none (the compositor may have
                        // refused the last one without a word)
                        self.outgoing = None;
                        self.app.pointer_down(x, y);
                    }
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
        if let Some(paths) = self.app.take_drag() {
            self.start_drag(qh, paths);
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
            // nothing is pasted into a password request: the clipboard is not read for it
            Keysym::v | Keysym::V if self.ctrl && !self.alt && !self.app.island.auth_open() => paste().map(KeyInput::Text),
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
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        if surface == self.layer.wl_surface() {
            self.app.island.keyboard_focus(true);
            self.dirty = true;
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if surface == self.layer.wl_surface() {
            self.app.island.keyboard_focus(false);
            self.dirty = true;
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key_event(event);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        // a key held down does not fill a password with itself
        if self.app.island.auth_open() && event.keysym != Keysym::BackSpace {
            return;
        }
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
    /// Starts a drag of files from the island, offered as a copy.
    fn start_drag(&mut self, qh: &QueueHandle<Self>, paths: Vec<String>) {
        let (Some(manager), Some(device)) = (self.data_manager.as_ref(), self.data_device.as_ref()) else { return };
        let source = manager.create_drag_and_drop_source(qh, [URI_LIST, TEXT], DndAction::Copy);
        source.start_drag(device, self.layer.wl_surface(), None, self.press_serial);
        self.outgoing = Some((source, paths));
    }

    /// The file drag now over the island, when it carries files: accepted as a copy. `at` is the pointer on the surface.
    fn drag_over(&mut self, at: (f64, f64)) {
        // the island's own files, on their way out, are not caught again
        if self.outgoing.is_some() {
            return;
        }
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().drag_offer()) else { return };
        if offer.with_mime_types(|m| m.iter().any(|t| t == URI_LIST)) {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
            offer.accept_mime_type(offer.serial, Some(URI_LIST.to_string()));
            // the pill reaches for it, so the drop has a target to see
            self.app.input(island_ui::model::Input::FileDrag(Some(at)));
            self.dirty = true;
        }
    }
}

impl DataDeviceHandler for Wl {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64, _: &wl_surface::WlSurface) {
        self.drag_over((x, y));
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        self.app.input(island_ui::model::Input::FileDrag(None));
        self.dirty = true;
    }
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        self.drag_over((x, y));
    }
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        if self.outgoing.is_some() {
            return;
        }
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().drag_offer()) else { return };
        let Ok(mut pipe) = offer.receive(URI_LIST.to_string()) else { return };
        self.app.input(island_ui::model::Input::FileDropped);
        self.dirty = true;
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

    let shell = Shell {
        layer: layer_shell,
        viewporter: globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok(),
        fractional: globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok(),
    };
    if shell.viewporter.is_none() || shell.fractional.is_none() {
        log::warn!("no fractional scaling: the island will look soft on scaled displays");
    }
    let Made { layer, viewport, fractional } = make_surface(&conn, &compositor, &shell, &qh);

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
                    state.app.input(island_ui::model::Input::Shelve(paths));
                }
                state.dirty = true;
            }
        })
        .map_err(|e| e.to_string())?;
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle()).map_err(|e| e.to_string())?;

    let mut gpu = Gpu::new();
    attach(&mut gpu, &conn, &layer)?;

    let mut state = Wl {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        layer,
        viewport,
        fractional,
        conn: conn.clone(),
        shell,
        lost: false,
        retry_at: Instant::now(),
        pointer: None,
        keyboard: None,
        idle_manager: globals.bind::<ExtIdleNotifierV1, _, _>(&qh, 1..=2, ()).ok(),
        idle: None,
        idle_since: None,
        idle_secs: std::env::var("OPEN_ISLAND_IDLE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(120),
        data_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        drop_tx,
        press_serial: 0,
        outgoing: None,
        loop_handle: event_loop.handle(),
        keyboard_on: false,
        ctrl: false,
        alt: false,
        app: {
            // the layer surface can hold the keyboard for itself: a password may be asked here
            crate::services::polkit::keyboard_is_exclusive();
            let mut app = App::new(waker);
            // files can be dragged out of the island here
            app.island.data.can_drag_files = true;
            app
        },
        gpu,
        scale: 1.0,
        size: (200, 60),
        configured: false,
        dirty: true,
        animating: true,
        next_ms: 0,
        exit: false,
        last_render: Instant::now(),
        reshow: true,
        input_rects: Vec::new(),
        on_top_layer: false,
        on_output: None,
        no_pointer: std::env::var_os("OPEN_ISLAND_NO_POINTER").is_some(),
        perf: crate::perf::Perf::from_env(),
    };
    log::info!("layer-shell surface created; away detection {}", if state.idle_manager.is_some() { "on" } else { "unavailable (no ext-idle-notify)" });
    loop {
        // frames only when something needs them: at vsync while the shape moves, otherwise when `next_ms` has passed
        let due = Duration::from_millis(state.next_ms as u64);
        let timeout = if state.lost {
            Duration::from_millis(500)
        } else if !state.configured {
            // nothing to draw into until the compositor says: its word wakes the loop
            Duration::from_millis(100)
        } else {
            until_frame(state.animating, state.dirty, state.last_render.elapsed(), due)
        };
        event_loop.dispatch(timeout, &mut state).map_err(|e| e.to_string())?;
        if state.exit {
            return Ok(());
        }
        if state.lost {
            // time goes on without a surface: timers ring, services are heard
            state.app.tick();
            if state.app.quit {
                return Ok(());
            }
            if Instant::now() >= state.retry_at && state.output_state.outputs().next().is_some() {
                state.rebuild(&qh);
            }
            continue;
        }
        if state.configured && until_frame(state.animating, state.dirty, state.last_render.elapsed(), due).is_zero() {
            state.render(&qh);
        }
    }
}

/// The island's side of a drag of its own files: the target asks for them, then says when it is done.
impl DataSourceHandler for Wl {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource, mime: String, mut pipe: WritePipe) {
        let Some((_, paths)) = self.outgoing.as_ref().filter(|(s, _)| s.inner() == source) else { return };
        let text = if mime == URI_LIST { island_core::localsend::uri_list(paths) } else { paths.join("\n") };
        // a handful of paths: far less than a pipe holds, so this does not wait for the reader
        if let Err(e) = std::io::Write::write_all(&mut pipe, text.as_bytes()) {
            log::warn!("drag: {e}");
        }
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        if self.outgoing.as_ref().is_some_and(|(s, _)| s.inner() == source) {
            self.outgoing = None;
        }
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        if self.outgoing.as_ref().is_some_and(|(s, _)| s.inner() == source) {
            self.outgoing = None;
        }
    }
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}
