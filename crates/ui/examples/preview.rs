//! Renders the island's faces to PNG files, headless (no window): `cargo run -p island-ui --example preview -- out_dir`.
//! Needs a GPU (or a software Vulkan/GL adapter) that wgpu can use.

use island_core::calendar::Event;
use island_core::weather::Slot;
use island_ui::model::*;
use island_ui::text::Fonts;
use island_ui::{Config, Island};
use std::sync::Arc;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

const NOW: i64 = 1_797_600_000_000; // 2026-12-18 ... arbitrary fixed instant, local offset +1 h

fn art() -> Image {
    let (w, h) = (96u32, 96u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            px.extend_from_slice(&[(60.0 + 160.0 * fx) as u8, (90.0 + 100.0 * (1.0 - fy)) as u8, (200.0 - 120.0 * fx) as u8, 255]);
        }
    }
    Image { id: 1, width: w, height: h, rgba: Arc::new(px) }
}

fn data() -> Data {
    let mut d = Data::default();
    d.utc_offset_secs = 3600;
    d.volume = Some(0.62);
    d.wifi = Some(true);
    d.bluetooth = Some(false);
    d.profile = Some("balanced".into());
    d.sinks = vec![Sink { name: "alsa_output.pci".into(), label: "Built-in Audio Speaker".into() }, Sink { name: "bluez_output.x".into(), label: "WH-1000XM5".into() }];
    d.sink = Some("alsa_output.pci".into());
    d.system = Some(SystemStats {
        cpu_percent: 23.0,
        cpu_temp_c: Some(58.0),
        cpu_ghz: Some(3.4),
        cores: vec![12.0, 30.0, 5.0, 80.0, 22.0, 91.0, 3.0, 40.0, 10.0, 15.0, 60.0, 7.0],
        memory: Some(island_core::sysmon::Memory { total_mb: 15900.0, used_mb: 7800.0, swap_used_mb: 120.0 }),
        gpu: Some(island_ui::model::Gpu { pstate: "P8".into(), watts: 4.2, temp_c: 41.0, util: 3.0 }),
        battery_watts: Some(8.4),
        battery_eta_min: Some(310),
        net_rx: 1.4 * 1048576.0,
        net_tx: 52.0 * 1024.0,
        disk: Some((304000.0, 466943.0)),
        load: Some([0.52, 0.4, 0.31]),
    });
    d.brightness = Some(0.8);
    d.battery = Some(Battery { percent: 79.0, charging: false, on_ac: false });
    d.weather = Some(Weather { temp_c: 14.0, feels_c: 12.0, code: 2, day: true, high_c: 17.0, low_c: 9.0, rain_slots: vec![Slot { offset_min: 0, mm: 0.0 }, Slot { offset_min: 30, mm: 0.0 }] });
    d.events = vec![
        Event { id: "a".into(), title: "Reti di calcolatori — lezione".into(), start_ms: NOW + 3 * 3_600_000, end_ms: NOW + 4 * 3_600_000, url: Some("https://meet.example/x".into()), ..Default::default() },
        Event { id: "b".into(), title: "Ricevimento".into(), start_ms: NOW + 5 * 3_600_000, end_ms: NOW + 6 * 3_600_000, ..Default::default() },
    ];
    d
}

fn media(d: &mut Data, playing: bool) {
    d.media = Some(Media {
        track_id: "t1".into(),
        title: "Touch (feat. Paul Williams)".into(),
        artist: "Daft Punk".into(),
        playing,
        position_ms: 96_000.0,
        sampled_at_ms: NOW,
        length_ms: 498_000.0,
        art: Some(art()),
        can_seek: true,
        players: 2,
        player: "Spotify".into(),
    });
    d.lyrics_lrc = Some("[00:01.00] Touch\n[01:30.00] Hold on, if love is the answer, you're home\n[01:50.00] Hold on\n[02:10.00] If love is the answer".into());
}

fn settle(i: &mut Island, d: &Data, frames: usize, hold_open: bool) -> island_ui::Frame {
    let mut t = NOW;
    let mut f = i.tick(t, 8.0);
    for _ in 0..frames {
        t += 8;
        i.data = d.clone();
        if hold_open {
            i.push(t, Input::Peek);
        }
        f = i.tick(t, 8.0);
    }
    f
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Gpu {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("no GPU adapter");
    eprintln!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("device");
    let renderer = Renderer::new(&device, RendererOptions::default()).expect("vello renderer");
    Gpu { device, queue, renderer }
}

fn render(g: &mut Gpu, scene: &Scene, w: u32, h: u32, path: &std::path::Path) {
    let tex = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    g.renderer
        .render_to_texture(&g.device, &g.queue, scene, &view, &RenderParams { base_color: Color::from_rgba8(0xf6, 0xed, 0xd0, 255), width: w, height: h, antialiasing_method: AaConfig::Area })
        .expect("render");
    let bpr = (w * 4).div_ceil(256) * 256;
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    g.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let data = slice.get_mapped_range().expect("mapped");
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h {
        rgba.extend_from_slice(&data[(row * bpr) as usize..(row * bpr + w * 4) as usize]);
    }
    let mut enc = png::Encoder::new(std::fs::File::create(path).unwrap(), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    eprintln!("wrote {}", path.display());
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "preview".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let fonts = Fonts::load_system();
    assert!(fonts.has_text_font(), "no text font found");
    let mut g = gpu();
    let scale = 2.0;

    type Setup = Box<dyn Fn(&mut Island, &mut Data)>;
    let cases: Vec<(&str, Setup)> = vec![
        ("idle", Box::new(|_, _| {})),
        ("idle-mic", Box::new(|_, d| d.privacy.mic = true)),
        ("media-mini", Box::new(|_, d| media(d, true))),
        ("media-full", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); i.cfg.lyrics = false; })),
        ("media-lyrics", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); })),
        ("media-transition", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); i.cfg.lyrics = false; })),
        ("media-timer-bubble", Box::new(|i, d| { media(d, true); i.push(NOW, Input::StartTimer(600)); })),
        ("timer-mini", Box::new(|i, _| i.push(NOW, Input::StartTimer(1500)))),
        ("timer-full", Box::new(|i, _| { i.push(NOW, Input::StartTimer(1500)); i.push(NOW, Input::Peek); })),
        ("progress-card", Box::new(|i, _| i.push(NOW, Input::Progress("64 rsync backup".into())))),
        ("progress-bubble", Box::new(|i, _| i.push(NOW - 5000, Input::Progress("64 rsync backup".into())))),
        ("osd", Box::new(|i, _| i.push(NOW, Input::Osd { kind: OsdKind::Volume, level: 0.62, muted: false }))),
        ("power-low", Box::new(|i, _| i.push(NOW, Input::Power(PowerEvent::Low)))),
        ("notif", Box::new(|i, _| i.push(NOW, Input::Notification(Incoming { id: 1, app: "Telegram".into(), summary: "Marco".into(), body: "Ci vediamo alle 18 in aula?".into(), ..Default::default() })))),
        ("notif-actions", Box::new(|i, _| i.push(NOW, Input::Notification(Incoming { id: 2, app: "Mail".into(), summary: "Esame di Reti".into(), body: "Il risultato è disponibile sul portale.".into(), actions: vec![NotifAction { key: "open".into(), label: "Apri".into() }, NotifAction { key: "later".into(), label: "Più tardi".into() }], ..Default::default() })))),
        ("meeting", Box::new(|_, d| { d.events[0].start_ms = NOW + 7 * 60_000; d.events[0].end_ms = NOW + 97 * 60_000; })),
        ("meeting-mini", Box::new(|i, d| { d.events[0].start_ms = NOW + 7 * 60_000; d.events[0].end_ms = NOW + 97 * 60_000; i.restore_announced(["a:".to_string() + &(NOW + 7 * 60_000).to_string()]); })),
        ("rain", Box::new(|_, d| { d.weather.as_mut().unwrap().rain_slots[1].mm = 0.6; })),
        ("home", Box::new(|i, _| { i.push(NOW, Input::Peek); })),
        ("page-cal", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Cal)))),
        ("page-clip", Box::new(|i, d| { d.clipboard = vec!["cargo test --workspace".into(), "https://example.org/a/very/long/link/that/needs/an/ellipsis?with=query&and=more".into(), "Reti di calcolatori\n  lezione 12".into()]; i.push(NOW, Input::OpenPage(island_core::face::Page::Clip)); })),
        ("focus-mini", Box::new(|_, d| d.focus = Some(Focus { active: true, end_epoch: Some(NOW / 1000 + 23 * 60) }))),
        ("focus-full", Box::new(|i, d| { d.focus = Some(Focus { active: true, end_epoch: Some(NOW / 1000 + 23 * 60) }); i.push(NOW, Input::OpenPage(island_core::face::Page::Focus)); })),
        ("page-sys-opening", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Sys)))),
        ("page-qs", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Qs)))),
        ("page-sys", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Sys)))),
        ("page-notifs", Box::new(|i, _| {
            for n in 0..3u32 {
                i.push(NOW, Input::Notification(Incoming { id: 10 + n, app: format!("App {n}"), summary: format!("Message {n}"), body: "Body of the message".into(), ..Default::default() }));
            }
            i.push(NOW, Input::OpenPage(island_core::face::Page::Notifs));
        })),
    ];
    for (name, setup) in cases {
        let mut i = Island::new(Config::default());
        let mut d = data();
        setup(&mut i, &mut d);
        i.data = d.clone();
        // pages need a page request; "home" and "meeting" go through the hover-less path
        let hold = name == "page-sys-opening" || name == "media-transition" || name.contains("full") || name.starts_with("focus-full") || name.contains("lyrics") || name == "home" || name.starts_with("page-");
        let frames = if name == "media-transition" { 26 } else if name == "page-sys-opening" { 38 } else { 400 };
        let f = settle(&mut i, &d, frames, hold);
        let (sw, sh) = (f.surface.0.ceil().max(520.0) as u32, f.surface.1.ceil().max(90.0) as u32);
        let mut scene = Scene::new();
        scene.fill(vello::peniko::Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0xf6, 0xed, 0xd0, 255), None, &vello::kurbo::Rect::new(0.0, 0.0, 4000.0, 4000.0));
        i.draw(&mut scene, &fonts, (sw as f64, sh as f64), scale);
        render(&mut g, &scene, (sw as f64 * scale) as u32, (sh as f64 * scale) as u32, &dir.join(format!("{name}.png")));
    }
}
