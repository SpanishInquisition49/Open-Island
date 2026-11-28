#![allow(clippy::field_reassign_with_default)]
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

#[allow(clippy::field_reassign_with_default)]
/// Two days of hours around NOW (local midnight of NOW's day + offset), a wet afternoon.
fn day_hours() -> Vec<island_core::weather::Hour> {
    let off = 3600;
    let mid = (NOW / 1000 + off).div_euclid(86_400) * 86_400 - off;
    (0..48)
        .map(|i| {
            let h = (i % 24) as f32;
            let wet = (15..=19).contains(&(i % 24));
            island_core::weather::Hour {
                time_ms: (mid + i * 3600) * 1000,
                temp_c: 9.0 + 8.0 * (1.0 - ((h - 15.0) / 9.0).powi(2)).max(0.0),
                code: if wet { 61 } else if (8..=14).contains(&(i % 24)) { 2 } else { 3 },
                is_day: (7..18).contains(&(i % 24)),
                rain_prob: if wet { 60 + (i % 24 - 15) as u8 * 8 } else { 5 },
                mm: if wet { 0.8 } else { 0.0 },
            }
        })
        .collect()
}

fn data() -> Data {
    let mut d = Data::default();
    d.utc_offset_secs = 3600;
    d.volume = Some(0.62);
    d.events_synced_at = NOW - 3 * 60_000;
    d.config.calendars = vec![CalendarView { name: "Università".into(), detail: "calendar.google.com".into(), color: (0xff, 0x9f, 0x0a) }];
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
    d.weather = Some(Weather { temp_c: 14.0, feels_c: 12.0, code: 2, day: true, high_c: 17.0, low_c: 9.0, rain_slots: vec![Slot { offset_min: 0, mm: 0.0 }, Slot { offset_min: 30, mm: 0.0 }], hours: day_hours() });
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
        shuffle: Some(true),
        repeat: Some(island_ui::model::LoopMode::One),
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

fn card_case(i: &mut Island, _: &mut Data) {
    i.push(NOW, Input::Notification(Incoming { id: 1, app: "Telegram".into(), summary: "Marco".into(), body: "Ci vediamo alle 18 in aula?".into(), ..Default::default() }));
}

fn stack_case(i: &mut Island, _: &mut Data) {
    for (id, app, who, text) in [(1, "Telegram", "Marco", "Ci vediamo alle 18 in aula?"), (2, "Mail", "Segreteria", "Il calendario degli appelli è stato aggiornato."), (3, "Signal", "Anna", "Porto io il caffè"), (4, "Slack", "CI", "build verde")] {
        i.push(NOW, Input::Notification(Incoming { id, app: app.into(), summary: who.into(), body: text.into(), expire_ms: Some(30_000), ..Default::default() }));
    }
}

/// The cards a script asks for: the four kinds, then a question, a button, an icon of one's own and a card that waits.
fn script_case(i: &mut Island, which: u32) {
    use island_core::notify::Kind;
    let card = |n: u32, kind: Kind, summary: &str, body: &str| Incoming { id: island_ui::island::SCRIPT_ID_BASE + n, app: "open-island".into(), kind: Some(kind), summary: summary.into(), body: body.into(), expire_ms: Some(30_000), ..Default::default() };
    let act = |k: &str, l: &str| island_ui::model::NotifAction { key: k.into(), label: l.into() };
    let cards = match which {
        0 => vec![card(1, Kind::Ok, "Build finished", "make test: 246 passed in 3.2s"), card(2, Kind::Fail, "Deploy failed", "staging: health check timed out after 30s"), card(3, Kind::Warn, "Disk almost full", "12 GB left")],
        1 => vec![
            Incoming { stay: true, expire_ms: None, actions: vec![act("no", "Later"), act("yes", "Merge")], ..card(1, Kind::Ok, "Merge drip #23?", "checks green, 1 approval") },
            Incoming { actions: vec![act("open", "View run")], ..card(2, Kind::Info, "Release 1.4 is out", "api · 2m 14s") },
            Incoming { stay: true, expire_ms: None, icon: Some("bolt".into()), tint: Some((0xd9, 0x77, 0x57)), ..card(3, Kind::Info, "api needs you", "Claude needs your permission to use Bash") },
        ],
        _ => vec![
            Incoming { icon: Some("bolt".into()), tint: Some((0xbf, 0x5a, 0xf2)), ..card(1, Kind::Ok, "Shipped to prod", "api 1.4.2, 14 files changed") },
            Incoming { id: 2, app: "Mail".into(), kind: None, ..card(2, Kind::Info, "Segreteria", "Il calendario è stato aggiornato.") },
        ],
    };
    for c in cards {
        i.push(NOW, Input::Notification(c));
    }
}

/// Brings an island to the start of one of the short animations, runs it `at` ms on and renders that instant.
fn fx_case(g: &mut Gpu, fonts: &Fonts, scale: f64, dir: &std::path::Path, name: &str, kind: u32, at: i64) {
    let mut i = Island::new(Config::default());
    let mut d = data();
    d.events = vec![];
    i.data = d.clone();
    let mut t = NOW;
    let mut run = |i: &mut Island, ms: i64| {
        let end = t + ms;
        let mut frame = i.tick(t, 0.0);
        i.draw(&mut Scene::new(), fonts, (frame.surface.0.max(560.0), frame.surface.1), scale);
        while t < end {
            t = (t + 8).min(end);
            frame = i.tick(t, 8.0);
            i.draw(&mut Scene::new(), fonts, (frame.surface.0.max(560.0), frame.surface.1), scale);
        }
        frame
    };
    run(&mut i, 600);
    let act = |label: &str| island_core::progress::Activity { id: "send".into(), label: label.into(), detail: "2 files to Pixel".into(), icon: Some("send".into()), tint: None, frac: Some(0.7), file: None, busy: false };
    let code = || Incoming { id: 7, app: "Banca".into(), summary: "Codice di accesso".into(), body: "Your verification code is 482913".into(), expire_ms: Some(20_000), ..Default::default() };
    match kind {
        0 | 1 => {
            i.push(i.now, Input::Progress("62 rsync backup".into()));
            run(&mut i, 2000);
            let done = if kind == 0 { Input::ProgressDone("backup".into()) } else { Input::ProgressFail("backup".into()) };
            i.push(i.now, done);
        }
        2 => {
            i.push(i.now, Input::SetDnd(true));
            run(&mut i, 600);
            for (n, app) in ["Telegram", "Mail", "Signal", "Calendar", "Zen"].iter().enumerate() {
                i.push(i.now, Input::Notification(Incoming { id: n as u32 + 1, app: app.to_string(), summary: format!("m{n}"), ..Default::default() }));
            }
            i.push(i.now, Input::SetDnd(false));
        }
        3 => {
            i.push(i.now, Input::Notification(code()));
            run(&mut i, 1800);
            // a click in the middle of the card
            let (x, y, w, h) = i.input_rects()[1];
            let now = i.now;
            i.pointer_moved(now, x + w / 2.0, y + h / 2.0);
            i.pointer_down(now, x + w / 2.0, y + h / 2.0);
            i.pointer_up(now);
        }
        4 => i.push(i.now, Input::Notification(code())),
        5 => i.push(i.now, Input::DeviceConnected(Device { id: "x".into(), name: "Nothing Ear".into(), kind: island_core::bluetooth::Kind::Earbuds, battery: Some(85) })),
        6 | 7 => {
            let (x, y, w, h) = i.input_rects()[0];
            i.push(i.now, Input::FileDrag(Some((x + w * 0.3, y + h))));
            run(&mut i, 500);
            let (x, y, w, h) = i.input_rects()[0];
            i.push(i.now, Input::FileDrag(Some((x + w * 0.3, y + h + 34.0))));
            if kind == 7 {
                run(&mut i, 600);
                i.push(i.now, Input::FileDropped);
                i.push(i.now, Input::SendFiles(vec!["/tmp/slides.pdf".into()]));
            }
        }
        _ => {
            i.push(i.now, Input::Activity(island_core::progress::State::Run, act("Sending to Pixel")));
            run(&mut i, 2000);
            i.push(i.now, Input::Activity(island_core::progress::State::Done, act("Sent")));
        }
    }
    let frame = run(&mut i, at);
    let (sw, sh) = (frame.surface.0.ceil().max(560.0) as u32, frame.surface.1.ceil().max(110.0) as u32);
    let mut scene = Scene::new();
    scene.fill(vello::peniko::Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0xf6, 0xed, 0xd0, 255), None, &vello::kurbo::Rect::new(0.0, 0.0, 4000.0, 4000.0));
    i.draw(&mut scene, fonts, (sw as f64, sh as f64), scale);
    render(g, &scene, (sw as f64 * scale) as u32, (sh as f64 * scale) as u32, &dir.join(format!("fx-{name}-{at:04}.png")));
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "preview".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let fonts = Fonts::load_system(std::env::var("ISLAND_FONT").ok().as_deref());
    assert!(fonts.has_text_font(), "no text font found");
    let mut g = gpu();
    let scale = 2.0;

    type Setup = Box<dyn Fn(&mut Island, &mut Data)>;
    let cases: Vec<(&str, Setup)> = vec![
        ("idle", Box::new(|_, _| {})),
        ("idle-mic", Box::new(|_, d| d.privacy.mic = true)),
        ("idle-mic-muted", Box::new(|i, d| { d.mic_muted = Some(true); i.restore_shelf(vec!["/home/u/Documents/notes.pdf".into()]); })),
        ("idle-countdown", Box::new(|i, _| i.push(NOW, Input::RunLine("exam 22 dec Computer Architecture".into())))),
        ("page-shelf", Box::new(|i, d| {
            d.can_drag_files = true;
            i.restore_shelf(vec!["/home/u/Downloads/Compilation Techniques slides 04.pdf".into(), "/home/u/Pictures/Screenshots/Screenshot from 2026-12-10 11-02-14.png".into(), "/home/u/Documents/report-draft.odt".into()]);
            i.push(NOW, Input::OpenPage(island_core::face::Page::Shelf));
        })),
        ("card-screenshot", Box::new(|i, _| i.push(NOW, Input::Notification(island_ui::model::Incoming { id: 1, app: "niri".into(), summary: "Screenshot captured".into(), body: "You can paste the image from the clipboard.".into(), file: Some("/home/u/Pictures/Screenshots/s.png".into()), ..Default::default() })))),
        ("page-today-places", Box::new(|i, _| {
            i.restore_places(vec![], vec![
                island_core::places::Waiting { place: "home".into(), leaving: false, text: "water the plants".into(), armed: true },
                island_core::places::Waiting { place: "uni".into(), leaving: true, text: "give back the library book".into(), armed: false },
            ]);
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-week", Box::new(|i, _| {
            // local midnight, an hour east of Greenwich as the preview is
            let today = (NOW + 3_600_000).div_euclid(86_400_000) * 86_400_000 - 3_600_000;
            let day = |n: i64, h: [i64; 3]| (today - n * 86_400_000, vec![("code".to_string(), h[0] * 60_000), ("zen".to_string(), h[1] * 60_000), ("telegram".to_string(), h[2] * 60_000)]);
            i.restore_attention_history(vec![day(1, [210, 80, 25]), day(2, [95, 140, 40]), day(3, [260, 60, 15]), day(5, [30, 55, 70]), day(6, [180, 90, 20])]);
            i.push(NOW - 50 * 60_000, Input::Focused { app: "code".into(), title: "island.rs".into() });
            i.push(NOW - 10 * 60_000, Input::Focused { app: "zen".into(), title: "docs".into() });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Activity));
        })),
        ("media-mini", Box::new(|_, d| media(d, true))),
        ("media-mini-lyric", Box::new(|i, d| { media(d, true); i.cfg.lyrics_in_pill = true; })),
        ("media-long-title", Box::new(|_, d| { media(d, true); d.media.as_mut().unwrap().title = "Sinfonia n. 9 in re minore, op. 125: IV. Finale (Presto - Allegro assai)".into(); })),
        ("media-full", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); i.cfg.lyrics = false; })),
        ("media-lyrics", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); })),
        ("media-transition", Box::new(|i, d| { media(d, true); i.push(NOW, Input::Peek); i.cfg.lyrics = false; })),
        ("lyrics-scroll", Box::new(|i, d| { media(d, true); d.media.as_mut().unwrap().position_ms = 109_600.0; i.push(NOW, Input::Peek); })),
        ("media-timer-bubble", Box::new(|i, d| { media(d, true); i.push(NOW, Input::StartTimer(600)); })),
        ("pills-many", Box::new(|i, d| {
            media(d, true);
            d.focus = Some(Focus { active: true, end_epoch: Some(NOW / 1000 + 25 * 60) });
            i.cfg.mini_pills = 4;
            i.push(NOW, Input::StartTimer(600));
            for (id, label, detail) in [("a1", "Claude · site", "Edit lib.rs"), ("a2", "Codex · api", "thinking")] {
                i.push(NOW, Input::Activity(island_core::progress::State::Run, island_core::progress::Activity { id: id.into(), label: label.into(), detail: detail.into(), ..Default::default() }));
            }
        })),
        ("timer-mini", Box::new(|i, _| i.push(NOW, Input::StartTimer(1500)))),
        ("timer-parallel", Box::new(|i, _| { i.push(NOW, Input::StartTimer(1500)); i.push(NOW, Input::StartTimer(180)); i.push(NOW, Input::StartTimer(900)); i.push(NOW, Input::OpenPage(island_core::face::Page::Timer)); })),
        ("timer-full", Box::new(|i, _| { i.push(NOW, Input::StartTimer(1500)); i.push(NOW, Input::Peek); })),
        ("progress-card", Box::new(|i, _| i.push(NOW, Input::Progress("64 rsync backup".into())))),
        ("progress-bubble", Box::new(|i, _| i.push(NOW - 5000, Input::Progress("64 rsync backup".into())))),
        ("device", Box::new(|i, _| i.push(NOW, Input::DeviceConnected(island_ui::model::Device { id: "x".into(), name: "Sony XM5".into(), kind: island_core::bluetooth::Kind::Headphones, battery: Some(85) })))),
        ("device-low", Box::new(|i, _| i.push(NOW, Input::DeviceLow(island_ui::model::Device { id: "x".into(), name: "Nothing Ear".into(), kind: island_core::bluetooth::Kind::Earbuds, battery: Some(12) })))),
        ("qs-devices", Box::new(|i, d| { d.devices = vec![island_ui::model::Device { id: "x".into(), name: "Sony XM5".into(), kind: island_core::bluetooth::Kind::Headphones, battery: Some(85) }, island_ui::model::Device { id: "y".into(), name: "Nothing Ear".into(), kind: island_core::bluetooth::Kind::Earbuds, battery: Some(12) }, island_ui::model::Device { id: "z".into(), name: "MX Master".into(), kind: island_core::bluetooth::Kind::Mouse, battery: None }]; i.push(NOW, Input::OpenPage(island_core::face::Page::Qs)) })),
        ("progress-done", Box::new(|i, _| { i.push(NOW, Input::Progress("64 rsync backup".into())); i.push(NOW, Input::ProgressDone("rsync backup".into())) })),
        ("activity-run", Box::new(|i, _| i.push(NOW, Input::Activity(island_core::progress::State::Run, island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "building 3/5".into(), icon: Some("git-branch".into()), tint: Some((255, 159, 10)), frac: None, file: None, busy: false })))),
        ("activity-fail", Box::new(|i, _| { i.push(NOW, Input::Activity(island_core::progress::State::Run, island_core::progress::Activity { id: String::new(), label: "CI main".into(), icon: Some("git-branch".into()), ..Default::default() })); i.push(NOW, Input::Activity(island_core::progress::State::Fail, island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "2 tests failed".into(), ..Default::default() })) })),
        // the small pill on its way out of the main one, and the drop that fills at the other side
        ("liquid-bubble-a", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-bubble-b", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-bubble-c", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        // the pill opening over the small pill and the drop
        ("liquid-open-a", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-open-b", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-open-c", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-download", Box::new(|i, _| i.push(NOW, Input::Activity(island_core::progress::State::Run, island_core::progress::Activity { id: "dl:thesis.pdf.part".into(), label: "thesis.pdf".into(), detail: "48 MB · 2.1 MB/s".into(), icon: Some("download".into()), busy: true, ..Default::default() })))),
        ("liquid-fill", Box::new(|i, _| i.push(NOW, Input::Progress("62 rsync backup".into())))),
        ("liquid-fill-timer", Box::new(|i, d| { media(d, true); i.push(NOW - 900_000, Input::StartTimer(1500)); })),
        ("event-cards", Box::new(|i, _| {
            i.push(NOW, Input::StartNamedTimer(0, "tea".into()));
            i.push(NOW, Input::Power(PowerEvent::Low));
            i.push(NOW, Input::Activity(island_core::progress::State::Done, island_core::progress::Activity { id: "dl:x".into(), label: "slides-03.pdf".into(), detail: "12 MB · 8s".into(), icon: Some("download".into()), file: Some("/home/u/Downloads/slides-03.pdf".into()), ..Default::default() }));
        })),
        ("osd", Box::new(|i, _| i.push(NOW, Input::Osd { kind: OsdKind::Volume, level: 0.62, muted: false }))),
        ("power-low", Box::new(|i, _| i.push(NOW, Input::Power(PowerEvent::Low)))),
        ("notif", Box::new(|i, _| i.push(NOW, Input::Notification(Incoming { id: 1, app: "Telegram".into(), summary: "Marco".into(), body: "Ci vediamo alle 18 in aula?".into(), ..Default::default() })))),
        // the card as it drops out of the pill and goes back, and with the pointer on it
        ("notif-drop-a", Box::new(card_case)),
        ("notif-drop-b", Box::new(card_case)),
        ("notif-drop-c", Box::new(card_case)),
        ("notif-drop-d", Box::new(card_case)),
        ("notif-leave-a", Box::new(card_case)),
        ("notif-leave-b", Box::new(card_case)),
        ("notif-hover", Box::new(card_case)),
        ("notif-long", Box::new(|i, _| {
            i.push(NOW, Input::Notification(Incoming { id: 1, app: "Mail".into(), summary: "Segreteria studenti: aggiornamento sul calendario degli appelli straordinari".into(), body: "Gentile studente, ti informiamo che il calendario degli appelli è stato aggiornato: controlla le nuove date sul portale prima di venerdì, perché le iscrizioni chiudono una settimana prima di ogni prova e non sarà possibile iscriversi dopo.".into(), ..Default::default() }));
            i.push(NOW, Input::Notification(Incoming { id: 2, app: "Telegram".into(), summary: "Anna".into(), critical: true, ..Default::default() }));
        })),
        ("script-kinds", Box::new(|i, _| script_case(i, 0))),
        ("script-ask", Box::new(|i, _| script_case(i, 1))),
        ("script-icon", Box::new(|i, _| script_case(i, 2))),
        // three cards stacked and one waiting; the second and third dropping in; the middle one going
        ("notif-stack", Box::new(stack_case)),
        ("notif-stack-in-a", Box::new(stack_case)),
        ("notif-stack-in-b", Box::new(stack_case)),
        ("notif-stack-in-c", Box::new(stack_case)),
        ("notif-stack-leave-a", Box::new(stack_case)),
        ("notif-stack-leave-b", Box::new(stack_case)),
        ("notif-stack-leave-c", Box::new(stack_case)),
        ("bar-empty", Box::new(|i, _| i.push(NOW, Input::OpenBar))),
        ("bar-remind", Box::new(|i, _| { i.push(NOW, Input::OpenBar); i.push(NOW, Input::Key(KeyInput::Text("remind 20m Call Anna".into()))) })),
        ("bar-sum", Box::new(|i, _| { i.push(NOW, Input::OpenBar); i.push(NOW, Input::Key(KeyInput::Text("5 km in mi".into()))) })),
        ("notif-actions", Box::new(|i, _| i.push(NOW, Input::Notification(Incoming { id: 2, app: "Mail".into(), summary: "Esame di Reti".into(), body: "Il risultato è disponibile sul portale.".into(), actions: vec![NotifAction { key: "open".into(), label: "Apri".into() }, NotifAction { key: "later".into(), label: "Più tardi".into() }], ..Default::default() })))),
        ("meeting", Box::new(|_, d| { d.events[0].start_ms = NOW + 7 * 60_000; d.events[0].end_ms = NOW + 97 * 60_000; })),
        ("meeting-mini", Box::new(|i, d| { d.events[0].start_ms = NOW + 7 * 60_000; d.events[0].end_ms = NOW + 97 * 60_000; i.restore_announced(["a:".to_string() + &(NOW + 7 * 60_000).to_string()]); })),
        ("rain", Box::new(|_, d| { d.weather.as_mut().unwrap().rain_slots[1].mm = 0.6; })),
        ("home", Box::new(|i, _| { i.push(NOW, Input::Peek); })),
        ("page-cal", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Cal)))),
        ("page-today", Box::new(|i, d| {
            let day = 1_700_000_000_000i64 / 86_400_000 * 86_400_000;
            let _ = day;
            let ev = |t: &str, h: i64, m: i64, dur: i64, url: bool| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: NOW / 86_400_000 * 86_400_000 + (h * 60 + m) * 60_000, end_ms: NOW / 86_400_000 * 86_400_000 + (h * 60 + m + dur) * 60_000, all_day: false, url: url.then(|| "https://meet.example/x".to_string()), color: Some((90, 160, 255)), calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Reti di calcolatori", 8, 30, 120, false), ev("Pranzo", 12, 30, 60, false), ev("Ricevimento", 15, 0, 45, true), ev("Studio di gruppo", 17, 0, 90, false)];
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-today-rowmenu", Box::new(|i, d| {
            d.events = vec![];
            let t = |u: &str, text: &str, off: i64, est: u32| island_ui::island::Todo { uid: u.into(), text: text.into(), due: Some(NOW / 86_400_000 + off), est_min: est, ..Default::default() };
            i.restore_todos(vec![t("a", "Relazione Reti", 0, 120), t("b", "Lab report", 1, 0), t("c", "Slides", 3, 60)]);
            i.row_menu = Some("b".into());
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-today-running", Box::new(|i, d| {
            let ev = |t: &str, h: i64, m: i64, dur: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: NOW / 86_400_000 * 86_400_000 + (h * 60 + m) * 60_000, end_ms: NOW / 86_400_000 * 86_400_000 + (h * 60 + m + dur) * 60_000, all_day: false, url: None, color: Some((90, 160, 255)), calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Reti di calcolatori", 8, 30, 120), ev("Pranzo", 12, 30, 60)];
            i.push(NOW, Input::StartTimer(1500));
            i.push(NOW, Input::Progress("64 rsync backup".into()));
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-today-agents", Box::new(|i, _| {
            let act = |id: &str, label: &str, detail: &str, f: Option<f64>| island_core::progress::Activity { id: id.into(), label: label.into(), detail: detail.into(), icon: Some("bolt".into()), frac: f, ..Default::default() };
            i.push(NOW, Input::Activity(island_core::progress::State::Run, act("s1", "Claude · site", "Edit lib.rs", None)));
            i.push(NOW, Input::Activity(island_core::progress::State::Run, act("s2", "Codex · api", "running tests", Some(0.6))));
            i.push(NOW, Input::StartNamedTimer(900, "pasta".into()));
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-today-evening", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64, dur: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + 86_400_000 + (h * 60 + m) * 60_000, end_ms: day + 86_400_000 + (h * 60 + m + dur) * 60_000, all_day: false, url: None, color: Some((90, 160, 255)), calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Reti di calcolatori", 8, 30, 120), ev("Algorithm Engineering", 11, 0, 90), ev("Laboratorio", 14, 0, 120)];
            i.restore_todos(["Comprare il latte", "Chiamare Anna", "Restituire il libro in biblioteca"].iter().map(|t| island_ui::island::Todo { text: t.to_string(), ..Default::default() }).collect());
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("evening-card", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000 + 86_400_000;
            let ev = |t: &str, h: i64, m: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + 90) * 60_000, all_day: false, url: None, color: None, calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Reti di calcolatori", 8, 30), ev("Algorithm Engineering", 11, 0), ev("Laboratorio", 14, 0)];
            i.restore_todos(vec![island_ui::island::Todo { text: "Chiamare Anna".into(), ..Default::default() }]);
            // the card comes by itself on the first frame after the data is in
            i.cfg.evening_recap_min = 60;
            i.cfg.evening_weekends = true;
        })),
        // a full day: the morning card names everything, to the end
        ("morning-card", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + 90) * 60_000, all_day: false, url: None, color: None, calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Reti di calcolatori", 15, 30), ev("Algorithm Engineering, esercitazione in laboratorio con consegna del progetto", 17, 0), ev("Models for Programming Paradigms", 18, 30), ev("Ricevimento", 20, 0), ev("Calcetto", 21, 30)];
            i.restore_todos(["Consegnare il report di laboratorio", "Restituire il libro in biblioteca", "Scrivere al relatore per fissare l'incontro", "Comprare il latte"].iter().map(|t| island_ui::island::Todo { text: t.to_string(), due: Some(NOW / 86_400_000 - 1), ..Default::default() }).collect());
            // the morning card comes only before 14:00: here it is six hours earlier than elsewhere
            d.utc_offset_secs -= 6 * 3600;
            i.cfg.morning_recap_min = 60;
            i.cfg.evening_weekends = true;
        })),
        ("page-today-student", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64, dur: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + dur) * 60_000, all_day: false, url: None, color: Some((90, 160, 255)), calendar: "Uni".into(), location: "Aula 3".into() };
            d.events = vec![ev("Reti di calcolatori", 15, 30, 120), ev("Laboratorio", 19, 0, 120)];
            d.events[0].location = "Aula 3, Polo Fibonacci, Largo Bruno Pontecorvo 3, Pisa".into();
            i.cfg.commute_minutes = 70;
            i.cfg.travel_margin_ms = 10 * 60_000;
            i.restore_deadlines(vec![island_ui::island::Deadline { day: NOW / 86_400_000 + 2, text: "Esame Algoritmi".into(), est_min: 600, ..Default::default() }, island_ui::island::Deadline { day: NOW / 86_400_000 + 9, text: "Consegna progetto".into(), ..Default::default() }]);
            i.restore_todos(vec![island_ui::island::Todo { text: "Portare il caricatore".into(), repeat: island_core::task::Repeat::Daily, due: Some(NOW / 86_400_000), ..Default::default() }, island_ui::island::Todo { text: "Restituire il libro".into(), due: Some(NOW / 86_400_000), est_min: 30, ..Default::default() }, island_ui::island::Todo { text: "Relazione Reti".into(), due: Some(NOW / 86_400_000 + 3), est_min: 120, ..Default::default() }]);
            i.restore_pins(vec!["ssh pi@10.0.0.5 -p 2222".into(), "Aula B3, piano 2".into()]);
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("notif-screenshot", Box::new(|i, _| {
            i.push(NOW, Input::Notification(Incoming { id: 7, app: "niri".into(), summary: "Screenshot captured".into(), body: "You can paste the image from the clipboard.".into(), ..Default::default() }));
        })),
        ("card-offer", Box::new(|i, _| i.push(NOW, Input::FileOffer { id: 1, summary: "iPhone wants to send".into(), body: "4 files · 8.2 MB\nIMG_2041.jpg, IMG_2042.jpg, IMG_2043.jpg, +1 more".into(), message: false }))),
        ("card-offer-message", Box::new(|i, _| i.push(NOW, Input::FileOffer { id: 2, summary: "Message from iPhone".into(), body: "The room changed: we are in B3, second floor, from 14:00.".into(), message: true }))),
        ("osd-layout", Box::new(|i, _| { i.cfg.osd_ms = 60_000; i.push(NOW, Input::KeyboardLayout("English (US)".into())) })),
        ("osd-capslock", Box::new(|i, _| { i.cfg.osd_ms = 60_000; i.push(NOW, Input::CapsLock(true)) })),
        ("page-grid", Box::new(|i, d| {
            media(d, true);
            d.phone = Some(Phone { link: island_core::phone::Link { present: true, messages: true, battery: Some(80), ..Default::default() }, ..Default::default() });
            i.push(NOW, Input::StartTimer(600));
            i.push(NOW, Input::OpenPage(island_core::face::Page::Grid));
        })),
        ("page-phone", Box::new(|i, d| {
            use island_core::phone::{Link, Thread};
            let th = |n: &str, k: &str, w: &str, p: &str| Thread { name: n.into(), key: k.into(), when: w.into(), preview: p.into() };
            d.phone = Some(Phone { link: Link { present: true, messages: true, contacts: true, battery: Some(80), ..Default::default() }, threads: vec![th("Enrica", "tel:+39", "2026-11-21 18:06", "Se vuoi possiamo chiedere anche a Giovanni"), th("iliad", "sender:iliad", "2026-10-26 09:23", "La tua offerta si rinnova il 27/10"), th("Corriere", "tel:+38", "2026-09-03 15:59", "Ciao, ho lasciato il pacco")], messages: None, at_ms: NOW, ..Default::default() });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Phone));
        })),
        ("page-phone-call", Box::new(|i, d| {
            use island_core::phone::{Call, Link, Thread};
            let th = |n: &str, k: &str, w: &str, p: &str| Thread { name: n.into(), key: k.into(), when: w.into(), preview: p.into() };
            d.phone = Some(Phone { link: Link { present: true, messages: true, contacts: true, calls: true, battery: Some(18), ..Default::default() }, threads: vec![th("Enrica", "tel:+39", "2026-11-21 18:06", "Stasera ci sei?"), th("iliad", "sender:iliad", "2026-10-26 09:23", "La tua offerta si rinnova")], calls: vec![Call { state: "ringing".into(), number: "+39347".into(), name: "Enrica".into() }], at_ms: NOW, ..Default::default() });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Phone));
        })),
        ("page-phone-chat", Box::new(|i, d| {
            use island_core::phone::{Link, Msg, Thread};
            let m = |mine: bool, t: &str| Msg { when: "2026-11-21 18:06".into(), mine, text: t.into() };
            d.phone = Some(Phone { link: Link { present: true, messages: true, contacts: true, battery: Some(80), ..Default::default() }, threads: vec![Thread { name: "Enrica".into(), key: "tel:+39".into(), when: "2026-11-21 18:06".into(), preview: String::new() }], messages: Some(("tel:+39".into(), vec![m(false, "Stasera ci sei?"), m(true, "Sì, arrivo alle 19"), m(false, "Perfetto, porta il pane"), m(true, "Ok")])), at_ms: NOW, ..Default::default() });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Phone));
            i.set_preview_phone_open("tel:+39");
        })),
        ("page-today-files", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64, dur: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + dur) * 60_000, all_day: false, url: None, color: Some((90, 160, 255)), calendar: "Uni".into(), location: "Aula 3".into() };
            d.events = vec![ev("Algorithm Engineering", 15, 30, 120)];
            let f = |n: &str, days: i64| island_core::materials::Material { path: format!("/home/u/Uni/{n}"), name: n.into(), modified_ms: NOW - days * 86_400_000 };
            d.materials.insert(island_core::materials::fold("Algorithm Engineering"), (NOW, vec![f("AlgorithmEngineering-2027_c1.pdf", 0), f("Pearls of Algorithm Engineering.pdf", 41), f("AE_250715.pdf", 128)]));
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-today-train", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64, dur: i64| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + dur) * 60_000, all_day: false, url: None, color: Some((90, 160, 255)), calendar: "Uni".into(), location: String::new() };
            d.events = vec![ev("Laboratorio", 19, 0, 120)];
            d.train = Some(island_core::train::TrainPin { from_id: "S06500".into(), from_name: "Pisa Centrale".into(), time: "17:12".into(), number: "18567".into(), category: "REG".into(), to_name: "Lucca".into(), arrival: "17:52".into(), ..Default::default() });
            d.train_status = Some(island_core::train::TrainStatus { delay_min: Some(6), cancelled: false, platform: Some("3".into()), at_ms: 0, next: vec![island_core::train::NextTrain { time: "17:42".into(), label: "REG 4177".into(), delay_min: None }, island_core::train::NextTrain { time: "18:12".into(), label: "RV 4181".into(), delay_min: Some(4) }] });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("notif-grouped", Box::new(|i, _| {
            for (id, text) in [(1, "Ci sei per il lab?"), (2, "Ho trovato l'aula"), (3, "Terzo piano, aula 3")] {
                i.push(NOW, Input::Notification(island_ui::model::Incoming { id, app: "Telegram".into(), summary: "Anna".into(), body: text.into(), ..Default::default() }));
            }
        })),
        ("page-notifs-grouped", Box::new(|i, _| {
            // held by the quiet mode: they go straight to the history, no card in the way
            i.push(NOW, Input::SetDnd(true));
            for (id, from, text) in [(1u32, "Anna", "Ci sei per il lab?"), (2, "Anna", "Ho trovato l'aula"), (3, "Anna", "Terzo piano, aula 3"), (4, "Marco", "Mi mandi gli appunti?"), (5, "Gruppo Reti", "Domani niente lezione")] {
                i.push(NOW, Input::Notification(island_ui::model::Incoming { id, app: "Telegram".into(), summary: from.into(), body: text.into(), ..Default::default() }));
            }
            i.push(NOW + 20_000, Input::OpenPage(island_core::face::Page::Notifs));
        })),
        ("page-send", Box::new(|i, d| {
            let peer = |a: &str, m: &str, k: &str| island_core::localsend::Peer { alias: a.into(), ip: "192.168.1.9".into(), port: 53317, https: true, model: m.into(), kind: k.into(), taildrop: false };
            d.peers = vec![peer("Pixel di Gabriele", "Pixel 8", "mobile"), peer("Studio-PC", "Windows", "desktop")];
            d.peers_scanning = true;
            i.push(NOW, Input::SendFiles(vec!["/tmp/preview-report.pdf".into()]));
        })),
        ("notif-code", Box::new(|i, _| {
            i.push(NOW, Input::Notification(island_ui::model::Incoming { id: 1, app: "Poste Italiane".into(), summary: "Your verification code".into(), body: "482913 is your code. Do not share it.".into(), ..Default::default() }));
        })),
        ("page-today-trip", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            let ev = |t: &str, h: i64, m: i64, loc: &str| island_core::calendar::Event { id: t.into(), title: t.into(), start_ms: day + (h * 60 + m) * 60_000, end_ms: day + (h * 60 + m + 60) * 60_000, color: Some((90, 160, 255)), calendar: "Uni".into(), location: loc.into(), ..Default::default() };
            d.events = vec![ev("Reti di calcolatori", 8, 30, "Aula 12, Polo Fibonacci, Pisa"), ev("Studio di gruppo", 17, 0, "Biblioteca, Via Santa Maria 36, Pisa")];
            d.trips = vec![island_ui::model::TripInfo { event_id: "Studio di gruppo".into(), mode: island_core::travel::Mode::Car, trip: Some(island_core::travel::Trip { secs: 840, meters: Some(5200), leave_ms: Some(day + (16 * 60 + 41) * 60_000), transfers: None }), error: None, at_ms: NOW, dest: None }];
            i.push(NOW, Input::OpenPage(island_core::face::Page::Today));
        })),
        ("page-activity", Box::new(|i, d| {
            d.utc_offset_secs = 3600;
            i.data.utc_offset_secs = 3600;
            let day = NOW / 86_400_000 * 86_400_000;
            let t = |m: i64| day + m * 60_000;
            let ds = day - 3_600_000; // local midnight: the preview's offset is +1 h
            i.restore_attention(ds, vec![("Code".into(), 3 * 3_600_000 + 12 * 60_000), ("Zen".into(), 95 * 60_000), ("Telegram".into(), 22 * 60_000), ("Spotify".into(), 9 * 60_000)]);
            i.push(NOW - 25 * 60_000, Input::Focused { app: "code".into(), title: "island.rs".into() });
            let _ = t;
            i.push(NOW - 9 * 60_000, Input::Notification(island_ui::model::Incoming { id: 1, app: "Telegram".into(), summary: "Lunch?".into(), ..Default::default() }));
            i.push(NOW, Input::OpenPage(island_core::face::Page::Activity));
        })),
        ("page-plan", Box::new(|i, d| {
            d.utc_offset_secs = 3600;
            i.data.utc_offset_secs = 3600;
            let day = NOW / 86_400_000 * 86_400_000 - 3_600_000;
            let ev = |t: &str, dd: i64, h: i64, mins: i64| island_core::calendar::Event { id: format!("{t}{dd}{h}"), title: t.into(), start_ms: day + dd * 86_400_000 + h * 3_600_000, end_ms: day + dd * 86_400_000 + h * 3_600_000 + mins * 60_000, color: Some((90, 160, 255)), calendar: "Uni".into(), ..Default::default() };
            let mine = |t: &str, dd: i64, h: i64, mins: i64| island_core::calendar::Event { color: Some((191, 90, 242)), calendar: "Personal".into(), ..ev(t, dd, h, mins) };
            d.events = vec![ev("Reti", 0, 9, 120), ev("Algoritmi", 0, 14, 90), ev("Compilazione", 1, 8, 120), ev("Lab", 1, 10, 120), ev("Algoritmi", 1, 13, 120), ev("Seminario", 1, 16, 90), ev("Reti", 2, 9, 120), ev("Lab", 4, 14, 180), mine("Palestra", 0, 18, 90), mine("Dentista", 2, 10, 120), mine("Cena", 5, 20, 150)];
            i.restore_deadlines(vec![island_ui::island::Deadline { day: NOW / 86_400_000 + 1, text: "Consegna progetto".into(), est_min: 180, ..Default::default() }, island_ui::island::Deadline { day: NOW / 86_400_000 + 12, text: "Esame Algoritmi".into(), course: "algoritmi".into(), est_min: 900, ..Default::default() }, island_ui::island::Deadline { day: NOW / 86_400_000 + 20, text: "AE".into(), cfu: 9, ..Default::default() }, island_ui::island::Deadline { day: NOW / 86_400_000 + 45, text: "mpp".into(), cfu: 6, ..Default::default() }]);
            i.restore_todos(vec![island_ui::island::Todo { uid: "a".into(), text: "Relazione Reti".into(), due: Some(NOW / 86_400_000 + 1), est_min: 120, ..Default::default() }, island_ui::island::Todo { uid: "b".into(), text: "Lab report".into(), due: Some(NOW / 86_400_000 + 1), ..Default::default() }]);
            i.restore_study_log(vec![(NOW / 86_400_000 - 1, "algoritmi".into(), 120), (NOW / 86_400_000, "reti".into(), 45), (NOW / 86_400_000, "algoritmi".into(), 60)]);
            i.push(NOW, Input::OpenPage(island_core::face::Page::Plan));
        })),
        ("page-find", Box::new(|i, d| {
            d.utc_offset_secs = 3600;
            i.data.utc_offset_secs = 3600;
            d.events = vec![island_core::calendar::Event { id: "c".into(), title: "Compilation Techniques".into(), start_ms: NOW + 3_600_000, end_ms: NOW + 7_200_000, calendar: "Uni".into(), ..Default::default() }];
            i.restore_todos(vec![island_ui::island::Todo { uid: "a".into(), text: "Read the compilers book".into(), due: Some(NOW / 86_400_000 + 3), ..Default::default() }]);
            i.push(NOW, Input::Find("compil".into()));
            i.push(NOW, Input::FoundMore { query: "compil".into(), hits: vec![island_core::plan::Hit { kind: island_core::plan::HitKind::Note, title: "compilers: ask about LLVM passes".into(), detail: "2026-12-10.md".into(), target: "/n/a.md".into() }, island_core::plan::Hit { kind: island_core::plan::HitKind::File, title: "Compilation Techniques slides 04.pdf".into(), detail: "Uni".into(), target: "/n/b.pdf".into() }] });
        })),
        ("page-rewind", Box::new(|i, _| {
            i.push(NOW - 20 * 60_000, Input::Focused { app: "code".into(), title: "island.rs".into() });
            i.push(NOW - 12 * 60_000, Input::Focused { app: "zen".into(), title: "Nominatim usage policy".into() });
            i.push(NOW - 9 * 60_000, Input::Notification(island_ui::model::Incoming { id: 1, app: "Telegram".into(), summary: "Lunch?".into(), ..Default::default() }));
            i.push(NOW - 3 * 60_000, Input::Focused { app: "kitty".into(), title: "cargo test".into() });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Activity));
            i.show_activity_tab(1);
        })),
        ("media-teams", Box::new(|i, d| {
            d.media = Some(Media { track_id: "z|/org/mpris/MediaPlayer2/firefox|Teams|".into(), title: "Teams and Channels | 26 - decorators and OOP in Python | Microsoft Teams".into(), artist: String::new(), playing: false, position_ms: 0.0, sampled_at_ms: NOW, length_ms: 7000.0, art: None, can_seek: true, players: 3, player: "Zen".into(), shuffle: None, repeat: None });
            d.lyrics_lrc = None;
            i.push(NOW, Input::OpenPage(island_core::face::Page::Media));
        })),
        ("media-teams-mini", Box::new(|_, d| {
            d.media = Some(Media { track_id: "z".into(), title: "Teams and Channels | 26 - decorators and OOP in Python | Microsoft Teams".into(), artist: String::new(), playing: true, position_ms: 9000.0, sampled_at_ms: NOW, length_ms: 7000.0, art: None, can_seek: true, players: 3, player: "Zen".into(), shuffle: None, repeat: None });
        })),
        ("page-map", Box::new(|i, d| {
            let day = NOW / 86_400_000 * 86_400_000;
            d.utc_offset_secs = 3600;
            i.data.utc_offset_secs = 3600;
            d.position = Some(island_ui::model::Position { lat: 43.7255, lon: 10.3908, accuracy_m: 380.0, at_ms: NOW, source: island_ui::model::PosSource::Geoclue });
            d.config.home_pos = Some((43.7262, 10.3995));
            d.events = vec![island_core::calendar::Event { id: "e".into(), title: "Studio di gruppo".into(), start_ms: day + 17 * 3_600_000, end_ms: day + 18 * 3_600_000, location: "Biblioteca, Via Santa Maria 36, Pisa".into(), ..Default::default() }];
            d.trips = vec![island_ui::model::TripInfo { event_id: "e".into(), mode: island_core::travel::Mode::Car, trip: Some(island_core::travel::Trip { secs: 480, meters: Some(2300), leave_ms: Some(day + 16 * 3_600_000), transfers: None }), error: None, at_ms: NOW, dest: Some((43.7167, 10.4017)) }];
            // stand-in tiles: a dark ground with a street grid, so the layout can be judged without the network
            let tile = |shade: u8| {
                let mut rgba = vec![0u8; 512 * 512 * 4];
                for yy in 0..512usize {
                    for xx in 0..512usize {
                        let street = xx % 128 < 6 || yy % 160 < 6;
                        let v = if street { 70 } else { 22 + shade };
                        let o = (yy * 512 + xx) * 4;
                        rgba[o..o + 4].copy_from_slice(&[v, v, v + 4, 255]);
                    }
                }
                island_ui::model::Image { id: 9000 + shade as u64, width: 512, height: 512, rgba: std::sync::Arc::new(rgba) }
            };
            i.push(NOW, Input::OpenPage(island_core::face::Page::Map));
            i.data = d.clone();
            let v = i.map_view_for_preview();
            for (n, t) in island_core::map::tiles(v.0, v.1, v.2, island_ui::island::MAP_W, island_ui::island::MAP_H).into_iter().enumerate() {
                i.push(NOW, Input::Tile { z: t.z, x: t.x, y: t.y, image: tile((n % 3) as u8 * 3) });
            }
        })),
        ("page-map-route", Box::new(|i, d| {
            use island_core::travel::{Mode, Place, Segment, Trip};
            d.utc_offset_secs = 3600;
            i.data.utc_offset_secs = 3600;
            d.position = Some(island_ui::model::Position { lat: 43.7255, lon: 10.3908, accuracy_m: 60.0, at_ms: NOW, source: island_ui::model::PosSource::Geoclue });
            let dest = Place { name: "Pisa Centrale".into(), detail: "Piazza della Stazione".into(), lat: 43.7079, lon: 10.3984 };
            d.route = Some(island_ui::model::RouteInfo { dest: dest.clone(), mode: Mode::Transit, trip: Some(Trip { secs: 600, meters: None, leave_ms: Some(NOW), transfers: Some(0) }), error: None, segments: vec![
                Segment { points: vec![(43.7255, 10.3908), (43.7230, 10.3925)], walk: true },
                Segment { points: vec![(43.7230, 10.3925), (43.7160, 10.3890), (43.7110, 10.3950), (43.7085, 10.3975)], walk: false },
                Segment { points: vec![(43.7085, 10.3975), (43.7079, 10.3984)], walk: true }] });
            i.push(NOW, Input::OpenPage(island_core::face::Page::Map));
            i.data = d.clone();
            i.set_preview_map_dest(dest);
            i.cfg.travel_mode = Mode::Transit;
        })),
        ("page-map-search", Box::new(|i, d| {
            use island_core::travel::Place;
            d.position = Some(island_ui::model::Position { lat: 43.7255, lon: 10.3908, accuracy_m: 60.0, at_ms: NOW, source: island_ui::model::PosSource::Geoclue });
            d.places = Some(vec![Place { name: "Pisa Centrale".into(), detail: "Piazza della Stazione, San Giusto".into(), lat: 43.7079, lon: 10.3984 }, Place { name: "Stazione San Rossore".into(), detail: "Via Pietrasantina, Pisa".into(), lat: 43.7311, lon: 10.3697 }, Place { name: "Stazione Pisa Aeroporto".into(), detail: "Pisa".into(), lat: 43.6844, lon: 10.3949 }]);
            i.push(NOW, Input::OpenPage(island_core::face::Page::Map));
        })),
        ("page-audio", Box::new(|i, d| {
            let sink = |n: &str, l: &str| island_ui::model::Sink { name: n.into(), label: l.into() };
            d.sinks = vec![sink("speaker", "Built-in Speakers"), sink("bluez_output.x", "WH-1000XM5")];
            d.sink = Some("bluez_output.x".into());
            d.sources = vec![sink("mic", "Internal Microphone")];
            d.source = Some("mic".into());
            d.streams = vec![island_ui::model::AppStream { id: 1, name: "Spotify".into(), volume: 62, muted: false }, island_ui::model::AppStream { id: 2, name: "Zen".into(), volume: 100, muted: true }];
            i.push(NOW, Input::OpenPage(island_core::face::Page::Audio));
        })),
        ("page-clip", Box::new(|i, d| { d.clipboard = vec!["cargo test --workspace".into(), "https://example.org/ciao".into(), "#ff8000".into(), "Via Santa Maria 36, Pisa".into(), "https://example.org/a/very/long/link/that/needs/an/ellipsis?with=query&and=more".into(), "Reti di calcolatori\n  lezione 12".into()]; i.push(NOW, Input::OpenPage(island_core::face::Page::Clip)); })),
        ("focus-mini", Box::new(|_, d| d.focus = Some(Focus { active: true, end_epoch: Some(NOW / 1000 + 23 * 60) }))),
        ("focus-full", Box::new(|i, d| { d.focus = Some(Focus { active: true, end_epoch: Some(NOW / 1000 + 23 * 60) }); i.push(NOW, Input::OpenPage(island_core::face::Page::Focus)); })),
        ("page-sys-opening", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Sys)))),
        ("page-settings", Box::new(|i, d| { d.config = ConfigView { font: "Inter".into(), font_missing: false, calendars: vec![CalendarView { name: "Università".into(), detail: "calendar.google.com".into(), color: (0xff, 0x9f, 0x0a) }, CalendarView { name: String::new(), detail: "p01-caldav.icloud.com".into(), color: (0x30, 0xd1, 0x58) }, CalendarView { name: "iCloud".into(), detail: "iCloud · me@icloud.com".into(), color: (0x0a, 0x84, 0xff) }], latitude: Some(43.7228), longitude: Some(10.4017), notification_sound: "/home/me/Music/UWU.ogg".into(), filters: vec![], timer_presets: "5 min, 10 min, 25 min".into(), screen: "focused".into(), map_tile_url: String::new(), map_key_set: false, home: "Via Roma 1, Pisa".into(), home_pos: Some((43.7255, 10.3908)), face: Some(island_ui::model::FaceChoice { chosen: island_ui::model::AuthFaceUse::Auto, note: "The camera starts with the request; a recognised face still waits for Enter".into() }) }; i.push(NOW, Input::OpenPage(island_core::face::Page::Settings)); })),
        ("page-weather", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Weather)))),
        ("home-rain", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Home)))),
        ("page-options", Box::new(|i, d| { d.config.timer_presets = "5 min, 10 min, 25 min".into(); d.config.screen = "focused".into(); i.push(NOW, Input::OpenPage(island_core::face::Page::Options)) })),
        ("page-options-rules", Box::new(|i, d| { d.config.filters = vec![island_ui::model::FilterView { app: "Zen Browser".into(), content: String::new(), show: false, sound: false }, island_ui::model::FilterView { app: "Telegram".into(), content: "unseen".into(), show: true, sound: false }]; i.push(NOW, Input::OpenPage(island_core::face::Page::Options)); i.show_options_tab(3); })),
        ("page-options-quiet", Box::new(|i, _| { i.push(NOW, Input::OpenPage(island_core::face::Page::Options)); i.show_options_tab(2); })),
        ("page-options-day", Box::new(|i, _| { i.push(NOW, Input::OpenPage(island_core::face::Page::Options)); i.show_options_tab(3); })),
        ("page-options-places", Box::new(|i, d| { d.config.map_tile_url = String::new(); i.push(NOW, Input::OpenPage(island_core::face::Page::Options)); i.show_options_tab(4); })),
        ("page-options-alerts", Box::new(|i, _| { i.push(NOW, Input::OpenPage(island_core::face::Page::Options)); i.show_options_tab(1); })),
        ("media-japanese", Box::new(|i, d| { media(d, true); let m = d.media.as_mut().unwrap(); m.title = "夜に駆ける (Racing into the Night)".into(); m.artist = "YOASOBI · 요아소비 · 约会".into(); i.push(NOW, Input::OpenPage(island_core::face::Page::Media)); })),
        ("page-qs", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Qs)))),
        ("page-sys", Box::new(|i, _| i.push(NOW, Input::OpenPage(island_core::face::Page::Sys)))),
        ("page-notifs", Box::new(|i, _| {
            for n in 0..3u32 {
                i.push(NOW, Input::Notification(Incoming { id: 10 + n, app: format!("App {n}"), summary: format!("Message {n}"), body: "Body of the message".into(), ..Default::default() }));
            }
            i.push(NOW, Input::OpenPage(island_core::face::Page::Notifs));
        })),
    ];
    // the short animations, each caught at several moments (`fx_case`)
    if let Some(only) = std::env::args().nth(2).filter(|a| a == "fx") {
        let _ = only;
        for (name, kind, times) in [
            ("drop-done", 0, &[8, 120, 200, 300, 420, 900][..]),
            ("drop-fail", 1, &[8, 200, 520, 580, 1000][..]),
            ("burst", 2, &[60, 180, 600, 930, 1070, 1200, 1900][..]),
            ("code", 3, &[0, 100, 220, 360, 520, 900][..]),
            ("code-roll", 4, &[300, 420, 520, 900][..]),
            ("ring", 5, &[300, 500, 800, 1600][..]),
            ("catch", 6, &[60, 600][..]),
            ("gulp", 7, &[100, 250, 320, 900][..]),
            ("shot", 8, &[200, 330, 480, 650, 750][..]),
        ] {
            for at in times {
                fx_case(&mut g, &fonts, scale, &dir, name, kind, *at);
            }
        }
        return;
    }
    for (name, setup) in cases {
        let mut i = Island::new(Config::default());
        let mut d = data();
        setup(&mut i, &mut d);
        i.data = d.clone();
        // pages need a page request; "home" and "meeting" go through the hover-less path
        let hold = name == "lyrics-scroll" || name == "page-sys-opening" || name == "media-transition" || name.contains("full") || name.starts_with("focus-full") || name.contains("lyrics") || name == "home" || name == "home-rain" || name == "qs-devices" || name.starts_with("page-");
        let frames = if name == "media-transition" { 26 } else if name == "page-sys-opening" { 38 } else if name == "lyrics-scroll" { 40 } else { 400 };
        let frames = match name { "liquid-bubble-a" => 14, "liquid-bubble-b" => 24, "liquid-bubble-c" => 40, "liquid-open-a" | "liquid-open-b" | "liquid-open-c" => 300, "event-cards" => 330, "liquid-fill" => 220, "liquid-download" => 520, "notif-stack-in-a" => 100, "notif-stack-in-b" => 122, "notif-stack-in-c" => 195, "notif-drop-a" => 12, "notif-drop-b" => 24, "notif-drop-c" => 36, "notif-drop-d" => 52, _ => frames };
        let mut f = settle(&mut i, &d, frames, hold);
        if let Some(after) = match name { "liquid-open-a" => Some(8), "liquid-open-b" => Some(16), "liquid-open-c" => Some(26), _ => None } {
            let mut t = NOW + frames as i64 * 8;
            let size = (f.surface.0.ceil().max(520.0), f.surface.1.ceil());
            i.draw(&mut Scene::new(), &fonts, size, scale);
            i.push(t, Input::Peek);
            for _ in 0..after {
                t += 8;
                f = i.tick(t, 8.0);
                i.draw(&mut Scene::new(), &fonts, size, scale);
            }
        }
        // these need the card to have been on screen before something happens to it
        if name.starts_with("notif-leave") || name.starts_with("notif-stack-leave") || name == "notif-hover" {
            let size = (f.surface.0.ceil().max(520.0), f.surface.1.ceil());
            i.draw(&mut Scene::new(), &fonts, size, scale);
            let mut t = NOW + frames as i64 * 8;
            if name == "notif-hover" {
                let (x, y, w, h) = i.input_rects()[1];
                i.pointer_moved(t, x + w - 24.0, y + h / 2.0);
            } else {
                i.push(t, Input::NotificationClosed(if name.starts_with("notif-stack") { 2 } else { 1 }));
            }
            for _ in 0..match name { "notif-leave-a" => 18, "notif-leave-b" => 40, "notif-stack-leave-a" => 26, "notif-stack-leave-b" => 44, "notif-stack-leave-c" => 220, _ => 4 } {
                t += 8;
                f = i.tick(t, 8.0);
                i.draw(&mut Scene::new(), &fonts, size, scale);
            }
        }
        // a card's height is known once it has been drawn: the next frame gives it room
        if i.has_card() {
            let t = i.now;
            i.draw(&mut Scene::new(), &fonts, (f.surface.0.ceil().max(520.0), f.surface.1.ceil()), scale);
            f = i.tick(t, 0.0);
        }
        let (sw, sh) = (f.surface.0.ceil().max(520.0) as u32, f.surface.1.ceil().max(90.0) as u32);
        let mut scene = Scene::new();
        scene.fill(vello::peniko::Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0xf6, 0xed, 0xd0, 255), None, &vello::kurbo::Rect::new(0.0, 0.0, 4000.0, 4000.0));
        i.draw(&mut scene, &fonts, (sw as f64, sh as f64), scale);
        render(&mut g, &scene, (sw as f64 * scale) as u32, (sh as f64 * scale) as u32, &dir.join(format!("{name}.png")));
        // the week of the Activity page, by its tab
        if name == "page-week" {
            let (x, y, w, _) = i.input_rects()[0];
            let mut t = i.now;
            i.pointer_down(t, x + w - 118.0, y + 31.0);
            i.pointer_up(t);
            let mut f = i.tick(t, 8.0);
            for _ in 0..300 {
                t += 8;
                f = i.tick(t, 8.0);
                i.draw(&mut Scene::new(), &fonts, (f.surface.0.ceil().max(520.0), f.surface.1.ceil()), scale);
            }
            let (sw, sh) = (f.surface.0.ceil().max(520.0) as u32, f.surface.1.ceil().max(90.0) as u32);
            let mut scene = Scene::new();
            scene.fill(vello::peniko::Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0xf6, 0xed, 0xd0, 255), None, &vello::kurbo::Rect::new(0.0, 0.0, 4000.0, 4000.0));
            i.draw(&mut scene, &fonts, (sw as f64, sh as f64), scale);
            render(&mut g, &scene, (sw as f64 * scale) as u32, (sh as f64 * scale) as u32, &dir.join("page-week.png"));
        }
        // the editor of the study hours, opened as it is opened: by its button on the Plan page
        if name == "page-plan" {
            let (x, y, w, _) = i.input_rects()[0];
            let mut t = i.now;
            i.pointer_down(t, x + w - 135.0, y + 31.0);
            i.pointer_up(t);
            i.cfg.study_week[0].to = 18 * 60;
            i.cfg.study_week[0].toggle_pause();
            i.cfg.study_week[2].to = 19 * 60;
            i.cfg.study_week[2].toggle_pause();
            i.cfg.study_week[5].to = 13 * 60;
            i.cfg.study_week[6].on = false;
            let mut f = i.tick(t, 8.0);
            for _ in 0..300 {
                t += 8;
                f = i.tick(t, 8.0);
                i.draw(&mut Scene::new(), &fonts, (f.surface.0.ceil().max(520.0), f.surface.1.ceil()), scale);
            }
            let (sw, sh) = (f.surface.0.ceil().max(520.0) as u32, f.surface.1.ceil().max(90.0) as u32);
            let mut scene = Scene::new();
            scene.fill(vello::peniko::Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0xf6, 0xed, 0xd0, 255), None, &vello::kurbo::Rect::new(0.0, 0.0, 4000.0, 4000.0));
            i.draw(&mut scene, &fonts, (sw as f64, sh as f64), scale);
            render(&mut g, &scene, (sw as f64 * scale) as u32, (sh as f64 * scale) as u32, &dir.join("page-plan-hours.png"));
        }
    }
}
