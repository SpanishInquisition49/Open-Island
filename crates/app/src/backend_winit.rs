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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

struct Wake;

struct Shell {
    app: App,
    gpu: Gpu,
    window: Option<Arc<Window>>,
    surface_logical: (f64, f64),
    animating: bool,
    last_redraw: Instant,
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
        let Some(m) = window.current_monitor().or_else(|| window.primary_monitor()) else { return };
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
        let scale = window.scale_factor();
        if let Err(e) = self.gpu.draw(&mut self.app.island, &self.app.fonts, self.surface_logical, scale) {
            log::error!("{e}");
        }
        self.last_redraw = Instant::now();
        if !window.is_visible().unwrap_or(true) {
            window.set_visible(true);
        }
    }

    fn schedule(&self, el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            if self.animating {
                w.request_redraw();
                el.set_control_flow(ControlFlow::Wait);
            } else {
                // at rest: wake once a second for the clock and the countdowns
                el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(1000)));
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
        window.request_redraw();
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, _e: Wake) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if !self.animating && self.last_redraw.elapsed() >= Duration::from_millis(1000) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        self.schedule(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        match event {
            WindowEvent::CloseRequested => el.exit(),
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
    let mut shell = Shell { app, gpu: Gpu::new(), window: None, surface_logical: (0.0, 0.0), animating: true, last_redraw: Instant::now() };
    let _ = (TOP_MARGIN, LogicalPosition::new(0.0, 0.0));
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("open-island: {e}");
    }
}
