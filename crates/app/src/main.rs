//! open-island: a macOS-style dynamic island for any desktop.

// parts that only the Linux services use are dead code on the other platforms
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod app;
mod cli;
mod doctor;
mod agent;
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
  open-island ok|warn|fail|info <title> [-d detail] [-s secs] [--stay] [--id name] [--open url] [--yes l [--no l]]
                                          a card from a script; a question exits 0 yes, 1 no, 5 unanswered (--help)
  open-island run [--] <command>...       a card when the command ends: how it went and how long it took
  open-island dismiss [--id name]         take a card away
  open-island pin <text> | unpin [text|all]  keep a note on the Today page (a click copies it)
  open-island watch <pid|program> [label]  a card when that process ends
  open-island autostart on|off|status     start with the session
  open-island doctor                      what works on this machine, what is missing
  open-island --version\n\nEVENTS (for msg):\n  peek | open [page] | reply | dnd <on|off> | page <home|cal|qs|sys|notifs|media|timer|clip> | timer <25m> | pomodoro | stopwatch | timer-cancel\n  key enter|escape|backspace|left|right|up|down|home|end|tab|text <t> (type into the island, for tests)
  set <option> <value> (any option of the options page: lyrics off, notification_seconds 8, timer_presets 5m,20m) | refresh (read the calendars now) | font <family|file> | calendar add <url> | calendar remove <n> | sound <file>   (settings, same as the menu)
  bar                                     open the command bar (bind it to a key): 10m tea, remind 20m ..., 12*7, 5 km in mi
  progress run [-l label] -- command...   (shows a command's progress)
  send <file>...   pick a device on the network (LocalSend) and send it the files
  activity run|done|fail|clear [label] [--detail t] [--icon i] [--color c] [--progress n]   a status for a script (CI, build)
  timer <25m> [name]                     start a countdown (several can run at once; `msg timer-cancel [name]` ends them)
  todo <text>                            add something to the list on the Today page (tick it off there): todo report due fri #algoritmi ~2h, todo gym every mon
  note <text>                            add a line to the notes folder (`notes_dir` in the settings; works without the island running)
  find <words>                           search events, tasks, notes and files (opens the Find page)
  do <bar line>                          run any command-bar line (todo, event, remind, note...)
  event <when> <title>                   shorthand for do event ...
  agent                                  a coding agent reports its turns (Claude Code hook JSON on stdin); agent setup claude | agent start|tool|done|fail|end --id ID ...
  hook <zsh|bash|fish|powershell>        a pill for every long shell command (eval $(open-island hook zsh))
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
        Some(verb @ ("notify" | "ok" | "warn" | "fail" | "info")) => std::process::exit(cli::card(verb, &args[1..])),
        Some("run") => std::process::exit(cli::run_and_tell(&args[1..])),
        Some("dismiss") => std::process::exit(cli::dismiss(&args[1..])),
        Some("autostart") => std::process::exit(cli::autostart(args.get(1).map(String::as_str))),
        Some("-V") | Some("--version") | Some("version") => println!("open-island {}", env!("CARGO_PKG_VERSION")),
        Some("progress") => std::process::exit(cli::progress(&args[1..])),
        Some("send") => std::process::exit(cli::send_files(&args[1..])),
        Some("activity") => std::process::exit(cli::activity(&args[1..])),
        Some("hook") => std::process::exit(hook::run(&args[1..])),
        Some("todo") if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("todo {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some("note") if args.len() >= 2 => {
            let res = services::organizer::notes_dir().ok_or_else(|| "no place for notes".to_string()).and_then(|d| services::organizer::save_note(&d, &args[1..].join(" "), chrono::Local::now()));
            std::process::exit(match res {
                Ok(p) => {
                    println!("{}", p.display());
                    0
                }
                Err(e) => {
                    eprintln!("open-island: {e}");
                    1
                }
            })
        }
        // any line of the command bar, from a script or a shortcut: open-island do "event fri 15-16 office hours"
        Some("do") if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("do {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some(verb @ ("event" | "remind")) if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("do {verb} {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some("find") if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("find {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some("pin") => std::process::exit(cli::pin(&args[1..])),
        Some("unpin") => std::process::exit(if services::ipc::send(&format!("unpin {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some("watch") if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("watch {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
        Some("agent") => std::process::exit(agent::run(&args[1..])),
        Some("timer") if args.len() >= 2 => std::process::exit(if services::ipc::send(&format!("timer {}", args[1..].join(" "))) { 0 } else { eprintln!("open-island is not running"); 1 }),
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
