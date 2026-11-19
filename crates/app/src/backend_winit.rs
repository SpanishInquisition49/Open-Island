//! The generic window backend: a borderless transparent always-on-top window at the top centre of the
//! primary monitor. Used on Windows, macOS and X11 (and on Wayland compositors without layer-shell).

use crate::app::App;
use crate::gpu::Gpu;
use island_ui::island::TOP_MARGIN;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

struct Wake;

struct Shell {
    app: App,
    gpu: Gpu,
    window: Option<Arc<Window>>,
    surface_logical: (f64, f64),
    animating: bool,
    next_ms: u32,
    keyboard_on: bool,
    hidden: bool,
    mods: ModifiersState,
    shape: Vec<(i32, i32, i32, i32, i32)>,
    last_redraw: Instant,
    perf: crate::perf::Perf,
}

/// Under a fullscreen window the island is out of sight. Under a maximized one it is too, while it rests, unless
/// the pointer is at the top edge asking for it; a card or a page (not resting) always shows.
fn should_hide(fullscreen: bool, maximized: bool, resting: bool, pointer_near: bool) -> bool {
    fullscreen || (maximized && resting && !pointer_near)
}

#[allow(unused_mut)]
fn attributes() -> WindowAttributes {
    let mut a = Window::default_attributes()
        .with_title("open-island")
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(LogicalSize::new(200.0, 60.0))
        .with_visible(false);
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::WindowAttributesExtWindows;
        a = a.with_skip_taskbar(true).with_undecorated_shadow(false);
    }
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::{WindowAttributesExtX11, WindowType};
        a = a.with_x11_window_type(vec![WindowType::Dock]).with_override_redirect(true);
    }
    a
}

impl Shell {
    fn place(&self, window: &Window) {
        // the screen option pins the island to a screen by name; otherwise the one it is on (the primary at first)
        let pinned = match self.app.island.data.config.screen.as_str() {
            "" | "focused" => None,
            name => window.available_monitors().find(|m| m.name().as_deref() == Some(name)),
        };
        let Some(m) = pinned.or_else(|| window.current_monitor()).or_else(|| window.primary_monitor()) else { return };
        let scale = m.scale_factor();
        let size = window.outer_size();
        let mp = m.position();
        let ms = m.size();
        let x = mp.x as f64 + (ms.width as f64 - size.width as f64) / 2.0;
        // OPEN_ISLAND_TOP_OFFSET (physical px) moves it down: for testing next to another island
        let dy = std::env::var("OPEN_ISLAND_TOP_OFFSET").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        window.set_outer_position(PhysicalPosition::new(x.round(), mp.y as f64 + dy));
        let _ = scale;
    }

    fn resize_to(&mut self, window: &Window, logical: (f64, f64)) {
        if logical == self.surface_logical {
            return;
        }
        self.surface_logical = logical;
        let _ = window.request_inner_size(LogicalSize::new(logical.0, logical.1));
        let scale = window.scale_factor();
        self.gpu.resize((logical.0 * scale).ceil() as u32, (logical.1 * scale).ceil() as u32);
        self.place(window);
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else { return };
        let f = self.app.tick();
        if self.app.quit {
            el.exit();
            return;
        }
        self.resize_to(&window, f.surface);
        self.animating = f.animating;
        self.next_ms = f.next_ms;
        let scale = window.scale_factor();
        if let Err(e) = self.gpu.draw(&mut self.app.island, &self.app.fonts, self.surface_logical, scale) {
            log::error!("{e}");
        }
        self.perf.frame(f.animating, self.gpu.last_work);
        self.last_redraw = Instant::now();
        // cut the window to the island's outline before it is shown, so no rectangle flashes
        self.cut_to_shape(&window);
        // under a fullscreen window the island hides (the platform service says when); under a maximized one it
        // hides too, except while it has something to say (a card, a page) or the pointer is at the top edge
        let resting = self.app.island.current_face().is_compact();
        let want_hidden = should_hide(self.app.fullscreen_hides(), self.app.maximized, resting, self.app.pointer_near);
        if want_hidden != self.hidden {
            self.hidden = want_hidden;
            log::info!("{} the island (fullscreen: {}, maximized window: {}, pointer at the edge: {})", if self.hidden { "hiding" } else { "showing" }, self.app.fullscreen, self.app.maximized, self.app.pointer_near);
            window.set_visible(!self.hidden);
        } else if !self.hidden && !window.is_visible().unwrap_or(true) {
            log::info!("showing the island");
            window.set_visible(true);
        }
        // a text field needs the keyboard: ask for the focus when one opens
        let kb = self.app.island.wants_keyboard();
        if kb && !self.keyboard_on {
            window.focus_window();
        }
        self.keyboard_on = kb;
    }

    /// Windows (and any platform without a transparent surface): cut the window to the island's outline.
    #[cfg(target_os = "windows")]
    fn cut_to_shape(&mut self, window: &Window) {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let scale = window.scale_factor();
        // A transparent window only needs the shape for the clicks (what is not the island lets them through):
        // while it moves the whole window takes them, and the exact shape is set when it rests. An opaque one is
        // cut to the shape all the time.
        let transparent = self.gpu.transparent;
        let pad = if transparent { 2.0 } else { 0.0 }; // room for the anti-aliased edge
        let rects: Vec<(i32, i32, i32, i32, i32)> = if transparent && self.animating {
            vec![]
        } else {
            self.app.island.shape_rects().iter().map(|&(x, y, w, h, r)| (((x - pad) * scale).round() as i32, ((y - pad) * scale).round() as i32, ((w + 2.0 * pad) * scale).round() as i32, ((h + 2.0 * pad) * scale).round() as i32, ((r + pad) * scale).round() as i32)).collect()
        };
        if rects == self.shape {
            return;
        }
        if let Ok(h) = window.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                crate::services::windows::set_window_shape(w.hwnd.get(), &rects);
                self.shape = rects;
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn cut_to_shape(&mut self, _window: &Window) {
        let _ = &self.shape;
    }

    /// Asks for a frame. A hidden window gets no redraw requests (Windows sends none), so its frames are run
    /// directly: that is how it notices that it should show itself again.
    fn redraw(&mut self, el: &ActiveEventLoop) {
        let Some(w) = self.window.clone() else { return };
        if self.hidden {
            self.frame(el);
        } else {
            w.request_redraw();
        }
    }

    fn schedule(&self, el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            if self.hidden {
                el.set_control_flow(ControlFlow::WaitUntil(self.last_redraw + Duration::from_millis(200)));
            } else if self.animating {
                w.request_redraw();
                el.set_control_flow(ControlFlow::Wait);
            } else {
                // at rest: wake once a second for the clock and the countdowns
                el.set_control_flow(ControlFlow::WaitUntil(self.last_redraw + Duration::from_millis(self.next_ms.max(16) as u64)));
            }
        }
    }
}

impl ApplicationHandler<Wake> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match el.create_window(attributes()) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("open-island: cannot create the window: {e}");
                el.exit();
                return;
            }
        };
        let scale = window.scale_factor();
        let size = window.inner_size();
        if let Err(e) = self.gpu.attach(window.clone(), size.width, size.height) {
            eprintln!("open-island: {e}");
            el.exit();
            return;
        }
        self.surface_logical = (size.width as f64 / scale, size.height as f64 / scale);
        self.window = Some(window.clone());
        self.place(&window);
        log::info!("window created: {}x{} physical px, scale {scale}", size.width, size.height);
        // The window starts hidden and Windows sends no redraw to a hidden window, so the first frame is drawn
        // here (it shows the window once it has something on it) rather than waited for.
        self.frame(el);
        if !self.hidden {
            window.set_visible(true); // whatever is_visible() says
        }
        window.request_redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, _e: Wake) {
        self.redraw(el);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // hidden: a frame five times a second keeps the clock, the timers and the reveal alive
        let every = if self.hidden { 200 } else { self.next_ms.max(16) as u64 };
        if (self.hidden || !self.animating) && self.last_redraw.elapsed() >= Duration::from_millis(every) {
            self.redraw(el);
        }
        self.schedule(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(k) = key_input(&event.logical_key, event.text.as_deref(), self.mods) {
                    self.app.key(k);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(el),
            WindowEvent::Resized(s) => {
                self.gpu.resize(s.width, s.height);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.app.pointer_moved(position.x / scale, position.y / scale);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.pointer_left();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => {
                        // the pointer position comes from the last CursorMoved
                        self.app.pointer_down_at_last();
                    }
                    (ElementState::Released, MouseButton::Left) => self.app.pointer_up(),
                    (ElementState::Pressed, MouseButton::Right) => self.app.right_click(),
                    _ => {}
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64 * 2.0, y as f64 * 2.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x / 20.0, p.y / 20.0),
                };
                self.app.scroll(dx, dy);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// A key press as the island's text fields and navigation understand it.
fn key_input(key: &Key, text: Option<&str>, mods: ModifiersState) -> Option<island_ui::model::KeyInput> {
    use island_ui::model::KeyInput;
    Some(match key {
        Key::Named(NamedKey::Backspace) => KeyInput::Backspace,
        Key::Named(NamedKey::Delete) => KeyInput::Delete,
        Key::Named(NamedKey::ArrowLeft) => KeyInput::Left,
        Key::Named(NamedKey::ArrowRight) => KeyInput::Right,
        Key::Named(NamedKey::ArrowUp) => KeyInput::Up,
        Key::Named(NamedKey::ArrowDown) => KeyInput::Down,
        Key::Named(NamedKey::Home) => KeyInput::Home,
        Key::Named(NamedKey::End) => KeyInput::End,
        Key::Named(NamedKey::Enter) => KeyInput::Enter,
        Key::Named(NamedKey::Escape) => KeyInput::Escape,
        Key::Named(NamedKey::Tab) if mods.shift_key() => KeyInput::PrevPage,
        Key::Named(NamedKey::Tab) | Key::Named(NamedKey::PageDown) => KeyInput::NextPage,
        Key::Named(NamedKey::PageUp) => KeyInput::PrevPage,
        Key::Character(c) if mods.control_key() && c.eq_ignore_ascii_case("v") => KeyInput::Text(paste()?),
        _ if mods.control_key() || mods.alt_key() || mods.super_key() => return None,
        _ => KeyInput::Text(text.filter(|t| !t.is_empty())?.to_string()),
    })
}

/// The clipboard's text for Ctrl+V (single line, at most 2000 characters).
fn paste() -> Option<String> {
    #[cfg(not(target_os = "linux"))]
    {
        let t: String = arboard::Clipboard::new().ok()?.get_text().ok()?.chars().take(2000).collect();
        (!t.is_empty()).then_some(t)
    }
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text"]).stderr(std::process::Stdio::null()).output().ok()?;
        let t: String = String::from_utf8_lossy(&out.stdout).chars().take(2000).collect();
        (!t.is_empty()).then_some(t)
    }
}

pub fn run() {
    let event_loop: EventLoop<Wake> = match EventLoop::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("open-island: no display available: {e}");
            std::process::exit(1);
        }
    };
    let proxy: EventLoopProxy<Wake> = event_loop.create_proxy();
    let waker: crate::app::Waker = Arc::new(move || {
        let _ = proxy.send_event(Wake);
    });
    let app = App::new(waker);
    let mut shell = Shell { app, gpu: Gpu::new(), window: None, surface_logical: (0.0, 0.0), animating: true, next_ms: 0, keyboard_on: false, hidden: false, mods: ModifiersState::empty(), shape: vec![], last_redraw: Instant::now(), perf: crate::perf::Perf::from_env() };
    let _ = (TOP_MARGIN, LogicalPosition::new(0.0, 0.0));
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("open-island: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::should_hide;

    #[test]
    fn hides_under_a_maximized_window_unless_it_has_something_to_show_or_is_asked_for() {
        assert!(!should_hide(false, false, true, false), "nothing covers the screen");
        assert!(should_hide(false, true, true, false), "maximized and resting: out of sight");
        assert!(!should_hide(false, true, true, true), "the pointer at the top edge brings it back");
        assert!(!should_hide(false, true, false, false), "a notification or a page shows");
        assert!(should_hide(true, false, true, false), "fullscreen hides");
        assert!(should_hide(true, true, false, true), "and nothing overrides a fullscreen window");
    }
}
