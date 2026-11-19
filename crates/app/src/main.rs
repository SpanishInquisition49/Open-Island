//! open-island: a macOS-style dynamic island for any desktop.

// parts that only the Linux services use are dead code on the other platforms
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod app;
mod cli;
mod doctor;
mod hook;
mod perf;
mod backend_winit;
#[cfg(target_os = "linux")]
mod backend_wayland;
mod config;
mod gpu;
mod services;

fn usage() {
    eprintln!(
        "open-island {}\n\nUSAGE:\n  open-island [--backend winit|wayland]   run the island\n  open-island msg <event> [payload]       talk to a running island
  open-island notify <title> [body]       show a notification (any platform)
  open-island autostart on|off|status     start with the session
  open-island doctor                      what works on this machine, what is missing
  open-island --version\n\nEVENTS (for msg):\n  peek | open [page] | reply | dnd <on|off> | page <home|cal|qs|sys|notifs|media|timer|clip> | timer <25m> | pomodoro | stopwatch | timer-cancel\n  key enter|escape|backspace|left|right|up|down|home|end|tab|text <t> (type into the island, for tests)
  set <option> <value> (any option of the options page: lyrics off, notification_seconds 8, timer_presets 5m,20m) | refresh (read the calendars now) | font <family|file> | calendar add <url> | calendar remove <n> | sound <file>   (settings, same as the menu)
  bar                                     open the command bar (bind it to a key): 10m tea, remind 20m ..., 12*7, 5 km in mi
  progress run [-l label] -- command...   (shows a command's progress)
  send <file>...   pick a device on the network (LocalSend) and send it the files
  activity run|done|fail|clear [label] [--detail t] [--icon i] [--color c] [--progress n]   a status for a script (CI, build)
  hook <zsh|bash|fish>                   a pill for every long shell command (eval $(open-island hook zsh))
  progress <percent|?> <label> | progress-done [label] | progress-fail [label] | progress-clear\n\nCONFIG: {}",
        env!("CARGO_PKG_VERSION"),
        config::path().map(|p| p.display().to_string()).unwrap_or_else(|| "(no config directory)".into())
    );
}

/// Windows: draw through DirectX 12 into a DirectComposition swap chain, the one kind that can be transparent
/// (the plain window swap chain is opaque, which meant cutting the window to shape on every frame: slow and
/// jagged). `OPEN_ISLAND_OPAQUE=1` keeps the old way; a setting already made in the environment is respected.
#[cfg(windows)]
fn windows_graphics_setup() {
    if std::env::var_os("OPEN_ISLAND_OPAQUE").is_some() {
        return;
    }
    for (k, v) in [("WGPU_BACKEND", "dx12"), ("WGPU_DX12_PRESENTATION_SYSTEM", "DxgiFromVisual")] {
        if std::env::var_os(k).is_none() {
            std::env::set_var(k, v);
        }
    }
}

fn main() {
    #[cfg(windows)]
    windows_graphics_setup();
    // a small overlay has no business on the power-hungry GPU of a hybrid laptop (it keeps it awake)
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        std::env::set_var("WGPU_POWER_PREF", "low");
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,zbus=warn,wgpu=warn,naga=warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("msg") => {
            if args.len() < 2 {
                usage();
                std::process::exit(2);
            }
            if !services::ipc::send(&args[1..].join(" ")) {
                eprintln!("open-island is not running");
                std::process::exit(1);
            }
        }
        Some("doctor") => doctor::run(),
        Some("notify") if args.len() >= 2 => {
            let line = format!("notify {}", args[1..].join(" | "));
            if !services::ipc::send(&line) {
                eprintln!("open-island is not running");
                std::process::exit(1);
            }
        }
        Some("autostart") => std::process::exit(cli::autostart(args.get(1).map(String::as_str))),
        Some("-V") | Some("--version") | Some("version") => println!("open-island {}", env!("CARGO_PKG_VERSION")),
        Some("progress") => std::process::exit(cli::progress(&args[1..])),
        Some("send") => std::process::exit(cli::send_files(&args[1..])),
        Some("activity") => std::process::exit(cli::activity(&args[1..])),
        Some("hook") => std::process::exit(hook::run(&args[1..])),
        Some("-h") | Some("--help") | Some("help") => usage(),
        _ => {
            let backend = args.iter().position(|a| a == "--backend").and_then(|i| args.get(i + 1)).map(String::as_str);
            run(backend);
        }
    }
}

fn run(backend: Option<&str>) {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        match backend {
            Some("winit") => return backend_winit::run(),
            Some("wayland") => return backend_wayland::run().unwrap_or_else(|e| fail(&e)),
            _ if wayland => match backend_wayland::run() {
                Ok(()) => return,
                Err(e) => log::warn!("layer-shell backend unavailable ({e}); falling back to a plain window"),
            },
            _ => {}
        }
    }
    let _ = backend;
    backend_winit::run();
}

#[allow(dead_code)]
fn fail(msg: &str) -> ! {
    eprintln!("open-island: {msg}");
    std::process::exit(1);
}
