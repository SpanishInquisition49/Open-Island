//! open-island: a macOS-style dynamic island for any desktop.

mod app;
mod cli;
mod doctor;
mod backend_winit;
#[cfg(target_os = "linux")]
mod backend_wayland;
mod config;
mod gpu;
mod services;

fn usage() {
    eprintln!(
        "open-island {}\n\nUSAGE:\n  open-island [--backend winit|wayland]   run the island\n  open-island msg <event> [payload]       talk to a running island
  open-island doctor                      what works on this machine, what is missing
  open-island --version\n\nEVENTS (for msg):\n  peek | open [page] | reply | dnd <on|off> | page <home|cal|qs|sys|notifs|media|timer|clip> | timer <25m> | pomodoro | stopwatch | timer-cancel\n  refresh (read the calendars now) | font <family|file> | calendar add <url> | calendar remove <n> | sound <file>   (settings, same as the menu)
  progress run [-l label] -- command...   (shows a command's progress)
  progress <percent|?> <label> | progress-done [label] | progress-fail [label] | progress-clear\n\nCONFIG: {}",
        env!("CARGO_PKG_VERSION"),
        config::path().map(|p| p.display().to_string()).unwrap_or_else(|| "(no config directory)".into())
    );
}

fn main() {
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
        Some("-V") | Some("--version") | Some("version") => println!("open-island {}", env!("CARGO_PKG_VERSION")),
        Some("progress") => std::process::exit(cli::progress(&args[1..])),
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
