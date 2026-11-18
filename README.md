# open-island

A macOS-style **dynamic island** for any desktop, in Rust. A black pill at the top of the screen that
morphs between the clock, music (cover, progress, synced lyrics), notifications (with actions), volume and
brightness, charging, timers (countdown, Pomodoro, stopwatch), weather and rain heads-up, calendar meetings,
long-running operations (a progress bar your scripts can drive) and privacy indicators.

It is a port of a Noctalia (Luau) plugin to a standalone program: one look (the macOS one), no shell
dependency.

```
crates/core   pure logic, no OS access: faces, shape springs, timers, lyrics, weather, iCal, notifications  (40 tests)
crates/ui     the state machine and the vello renderer, no windowing: draws every face, headless-testable
crates/app    window backends (Wayland layer-shell, winit), the OS services, the `open-island` binary
```

## Status

| | Linux (Wayland, wlr-layer-shell) | Linux (X11) / Windows / macOS (winit window) |
|---|---|---|
| island, springs, all faces, hover/click/scroll | yes | yes (built and type-checked; **not yet run** on Windows/macOS) |
| fractional scaling, click-through margin | yes | HiDPI through winit; click-through margin: no |
| clock, timers, Pomodoro, stopwatch | yes | yes |
| control socket (`open-island msg ...`) | yes | yes (named pipe on Windows) |
| weather (Open-Meteo), calendars (iCal URLs), lyrics (LRCLIB) | yes | yes |
| media (MPRIS) with cover art, several players (switch button) | yes | **not yet** (SMTC / MediaRemote) |
| notifications | yes: full daemon (pictures, app icons, timeouts, urgency, progress, resident, sync tags, per-app sounds, filters, actions, inline reply, closing signals, persistent history); watches the bus when another daemon owns the name | **not yet** |
| volume / brightness OSD, battery, privacy dots | yes (wpctl, sysfs, pactl) | **not yet** |
| hide under fullscreen windows | niri | **not yet** |

The faces for clipboard history, focus mode and the update check of the Luau original are not ported yet.
Complex scripts (Arabic, Hindi, ...) are not shaped: text is laid out glyph by glyph.

On X11 with the proprietary NVIDIA driver the window cannot be transparent (the driver offers no alpha
mode), so a black rectangle shows around the pill; Wayland has no such limit. A clipping shape for that case
is planned.

## Build and run

```sh
cargo run --release                      # the island
cargo run --release -- msg progress "40 Backup"   # talk to a running island
cargo test --workspace
cargo run -p island-ui --example preview -- out/  # render every face to PNG, headless
```

Needs a Vulkan/Metal/DX12 GPU (vello renders on the GPU). Font: SF Pro if installed (not redistributable),
else Helvetica Neue, Inter, Segoe UI, Roboto, Noto Sans, DejaVu Sans, Arial. Icons are Tabler (MIT, bundled,
17 kB subset; `tools/subset_icons.sh` rebuilds it).

## Configuration

`~/.config/open-island/config.json` (`%APPDATA%\open-island\config.json` on Windows,
`~/Library/Application Support/open-island/config.json` on macOS):

```json
{ "latitude": 43.72, "longitude": 10.40,
  "calendar_urls": ["https://example.org/calendar.ics"],
  "timer_presets": ["5m", "10m", "25m"], "meeting_lead_minutes": 10,
  "notification_seconds": 5, "low_battery_percent": 15, "lyrics": true,
  "notification_sound": "~/Music/uwu.ogg", "app_sounds": {"telegram": "~/Music/t.ogg"},
  "volume_sound": "~/Music/bloop.mp3", "timer_sound": "~/Music/bloop.mp3",
  "notification_filters": [{"app": "zen browser", "content": "unseen", "show": false}] }
```

Weather needs a location: the island never guesses one and calls out to nothing you did not configure.

## Controlling it

`open-island msg <event> [payload]`: `peek`, `page <home|cal|qs|sys|notifs|media|timer>`, `timer 25m`,
`pomodoro`, `stopwatch`, `timer-cancel`, `reply` (reply to the notification on screen: bind it to a key), and for long operations `progress <percent|?> <label>`,
`progress-done [label]`, `progress-fail [label]`, `progress-clear`: a card for 3 s, then a `64%` bubble
beside the pill, then a card again when it ends (red when it failed).

## Design notes

- **Springs** (`core/src/spring.rs`): the shape is two springs with *implicit damping*. The Luau original
  used explicit damping, which is unstable for any step above 40 ms on the closing spring (each step
  multiplies the velocity by `1 - c·dt`, below -1 for `c = 50`, `dt = 50 ms`). After a screen lock the frame
  tick drops to 1 Hz, `dt` clamps to 50 ms, the shape diverged to infinity and the shell disabled the layer,
  leaving a blank rectangle on screen. Time steps are clamped to 0..50 ms and a non-finite state resets to
  its target; both are unit-tested.
- **Meeting heads-up** fires once per event and is persisted, so a restart does not repeat it.
- **Fullscreen**: on niri the island drops from the overlay to the top layer while the focused window covers
  the output (niri has no fullscreen flag in its IPC, so the size is compared with the output's).
- The UI never blocks: every service runs on its own thread and reports through a channel.

## Licence

MIT. Tabler icons are MIT (`crates/ui/assets/TABLER-LICENSE.txt`).
