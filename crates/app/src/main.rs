//! open-island: a macOS-style dynamic island for any desktop.

mod app;
mod backend_winit;
mod config;
mod gpu;
mod services;

fn usage() {
    eprintln!(
        "open-island {}\n\nUSAGE:\n  open-island [--backend winit|wayland]   run the island\n  open-island msg <event> [payload]       talk to a running island\n\nEVENTS (for msg):\n  peek | reply | page <home|cal|qs|sys|notifs|media|timer> | timer <25m> | pomodoro | stopwatch | timer-cancel\n  progress <percent|?> <label> | progress-done [label] | progress-fail [label] | progress-clear\n\nCONFIG: {}",
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
        Some("-h") | Some("--help") | Some("help") => usage(),
        _ => {
            let backend = args.iter().position(|a| a == "--backend").and_then(|i| args.get(i + 1)).map(String::as_str);
            run(backend);
        }
    }
}

fn run(backend: Option<&str>) {
    let _ = backend;
    backend_winit::run();
}

#[allow(dead_code)]
fn fail(msg: &str) -> ! {
    eprintln!("open-island: {msg}");
    std::process::exit(1);
}
