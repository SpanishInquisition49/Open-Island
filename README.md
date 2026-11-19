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

| | Linux (Wayland, wlr-layer-shell) | Windows | macOS | Linux X11 |
|---|---|---|---|---|
| island, springs, all faces, hover / click / scroll / keyboard | yes | built, **not run** | built, **not run** | yes (a visible rectangle on NVIDIA) |
| window | layer surface, fractional scale, click-through margin | opaque window cut to the island's shape (`SetWindowRgn`), top-most, no taskbar button | winit window, transparent | winit window |
| clock, timers, Pomodoro, stopwatch, control socket, `notify`, `progress run`, `autostart` | yes | yes | yes | yes |
| weather, calendars (feeds, iCloud / CalDAV), lyrics | yes | yes | yes | yes |
| media with cover art, several players | MPRIS | SMTC (built) | **not yet** | MPRIS |
| notifications | the island is the daemon (replaces mako, dunst, Noctalia's) | other apps' toasts are read from Windows' notification database (a plain program cannot be handed them), plus `open-island notify` | **only `open-island notify`** | the island is the daemon |
| volume card, volume / mute | wpctl, pactl | Core Audio (built) | **not yet** | yes |
| brightness, Wi-Fi / Bluetooth switches, power profile | brightnessctl, nmcli, bluetoothctl, TLP / powerprofilesctl | **not yet** (the chips hide) | **not yet** | yes |
| battery, system page | sysfs, /proc, nvidia-smi | sysinfo + starship-battery (built) | the same (built) | yes |
| privacy dots (microphone, camera, screen share) | pactl, /dev/video, pw-dump | the privacy registry (built) | **not yet** | yes |
| clipboard history | wl-clipboard | arboard polling (built) | arboard polling (built) | **not yet** |
| keep awake, night light | systemd-inhibit, wlsunset | SetThreadExecutionState; no night light | **not yet** | yes |
| hide under fullscreen windows | niri | shell notification state (built) | **not yet** | **not yet** |

"built" means it type-checks for `x86_64-pc-windows-msvc` / `aarch64-apple-darwin` in `cargo check`; none of it has been
run on those systems. The update-check card of the Luau original is not ported (it was specific to Noctalia). Complex
scripts (Arabic, Hindi, ...) are not shaped: text is laid out glyph by glyph.

## Install (Linux)

```sh
cargo install --path crates/app          # or: makepkg -si -D contrib   (contrib/PKGBUILD)
open-island doctor                       # what works here, what is missing
cp contrib/open-island.service ~/.config/systemd/user/ && systemctl --user enable --now open-island
```

On niri, `contrib/niri-snippet.kdl` has `spawn-at-startup` and key bindings (open, quick settings, notifications, reply,
clipboard). The island replaces the notification daemon at start (see below); at rest it uses under 1% of a core.
On the clipboard page a star keeps an entry on top (pinned entries are the only ones written to disk, in the state
directory, mode 0600) and the x forgets it. A copied link, e-mail address, street address or pair of coordinates gets a button
that opens it (the browser, the mail client, OpenStreetMap), and a `#rrggbb` colour shows its swatch.

## Windows and macOS

```sh
cargo build --release          # the island: target\release\open-island.exe
open-island autostart on       # Run key (Windows) / LaunchAgent (macOS)
open-island notify "Build done" "120 tests passed"
```

Everything marked "built" above was written against the platform APIs and checked by the compiler for those targets, with
no machine to run it on: expect rough edges, and please report them. Windows cannot hand other applications' toasts to a
program that is not a packaged app, so on Windows and macOS notifications come from `open-island notify`, `progress run`
and the calendar and timers.

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

Everything below can be changed from the island itself: the gear on the home page (or `open-island msg page settings`) opens a
menu to add and remove calendars (paste an .ics or webcal:// address, Ctrl+V works), set the weather location, the font and the
notification sound. It writes the file below, keeping the keys it does not know, and weather and calendars refresh at once.

`~/.config/open-island/config.json` (`%APPDATA%\open-island\config.json` on Windows,
`~/Library/Application Support/open-island/config.json` on macOS):

```json
{ "latitude": 43.72, "longitude": 10.40,
  "calendars": [
    {"name": "University", "url": "https://example.org/calendar.ics", "color": "#ff9f0a"},
    {"type": "icloud", "user": "me@icloud.com", "password": "abcd-efgh-ijkl-mnop"},
    {"type": "caldav", "server": "https://cloud.example.org/remote.php/dav/", "user": "me", "password_cmd": "pass show dav"} ],
  "timer_presets": ["5m", "10m", "25m"], "meeting_lead_minutes": 10,
  "notification_seconds": 5, "font": "Inter", "low_battery_percent": 15, "lyrics": true,
  "notification_sound": "~/Music/uwu.ogg", "app_sounds": {"telegram": "~/Music/t.ogg"},
  "volume_sound": "~/Music/bloop.mp3", "timer_sound": "~/Music/bloop.mp3",
  "notification_filters": [{"app": "zen browser", "content": "unseen", "show": false}] }
```

Calendars: a feed (`.ics` / `webcal://`, public or secret address) or a CalDAV account. **iCloud** needs the Apple ID and an
app-specific password (appleid.apple.com > Sign-In and Security > App-Specific Passwords), not the Apple ID password: every
calendar of the account is read with its own name and colour. Each calendar can be renamed and recoloured from the menu (the
old `calendar_urls` list still works and is migrated). `password_cmd` prints the password (a password manager) instead of
keeping it in the file, which is owner-only anyway.

Weather needs a location: the island never guesses one and calls out to nothing you did not configure.

## Moving between pages

Every page carries a row of page icons at the bottom (media and timer first when active, then home, calendar, quick settings,
system, clipboard, settings): click one to jump straight to it. A horizontal swipe, Tab / Shift+Tab (keyboard mode) and
`open-island msg page next|prev` step through the same list, and the content slides the way the pages are ordered. The back arrow
returns to the page that led there.

## One notification daemon

Only one program can own `org.freedesktop.Notifications`; another daemon (mako, dunst, swaync...) showing popups next to the
island is why every notification appeared twice. At start the island therefore stops the daemon it finds (its systemd unit
when it is named after it, else SIGTERM) and takes the name. Set `"take_over_notifications": false` to keep the other one (the
island then only watches, without actions or replies). D-Bus can start the old daemon again when a notification arrives while
the island is not running: `systemctl --user mask mako` stops that. Noctalia is never killed: the island asks it, with `noctalia msg config-set notification.enable_daemon false`, to stop
claiming notifications (undo with `... true`). Any other owner is left alone and named in the log.

## Controlling it

`open-island msg <event> [payload]`: `peek`, `open [page]` (keyboard navigation: arrows move a focus ring, Enter
activates, Tab / Shift+Tab change page, Escape closes), `page next|prev`, `page <home|cal|qs|sys|notifs|media|timer|clip|focus>`, `dnd on|off`, `timer 25m`,
`pomodoro`, `stopwatch`, `timer-cancel`, `reply` (reply to the notification on screen: bind it to a key), and for long operations `open-island progress run [-l label] -- command...` (runs the command on a pseudo-terminal, so rsync, curl, pacman... still draw
their own progress, and shows the last `NN%` they print; not for commands that ask questions: `sudo -v` first), or by hand `progress <percent|?> <label>`,
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

## A pill for every long shell command

```sh
eval "$(open-island hook zsh)"      # in ~/.zshrc  (bash: ~/.bashrc, fish: `open-island hook fish | source` in config.fish)
```

A command that runs longer than 10 seconds becomes a progress pill with its name; when it ends it turns green with how
long it took, or red with its exit code. Editors, pagers, `ssh`, `top`, REPLs and similar are skipped. Set
`OPEN_ISLAND_HOOK_SECONDS=30` to change the threshold and `OPEN_ISLAND_HOOK_IGNORE="make,go"` to skip more programs.
A shell that is closed mid-command takes its pill with it.

## The command bar

Bind `open-island msg bar` to a key (niri: `Mod+Space { spawn "open-island" "msg" "bar"; }`). Type, Enter runs it, Esc closes:

| type | does |
|---|---|
| `10m tea`, `1h30m`, `timer 25` | a timer |
| `remind 20m call mom`, `in 2h x`, `ricordami tra 10m …`, `remind 15:30 …` | a reminder (a notification when due; kept across restarts) |
| `pomodoro`, `stopwatch`, `focus 45`, `dnd on` / `dnd off` | the matching mode |
| `12*7+3`, `sqrt(2)`, `2^10`, `(1+2)*3` | a sum; Enter copies the result |
| `5 km in mi`, `70 f to c`, `1 gib to mb` | a conversion (length, mass, volume, speed, data, temperature); Enter copies it |

## Quiet by itself

Notifications are held (they stay in the history, and the sound stays off) while a calendar event is on (timed, up to
3 hours), while the microphone is in use and while the screen is shared; a small crossed bell shows it in the pill and
`critical` notifications still come through. Switch any of the three off in the configuration:
`"quiet_in_meetings": false`, `"quiet_on_mic": false`, `"quiet_on_screen_share": false`.

The bell with a z on a notification card hides it and brings it back after ten minutes. When the island stops being
quiet (a meeting ends, a call, `dnd off`, the study focus) one card says how much was held back and from whom; a click
opens the list. The focus mode of `~/.local/bin/focus` counts as quiet too.

## Status of anything a script watches

```sh
open-island activity run  "CI main" --detail "building 3/5" --icon git-branch --color orange
open-island activity run  "CI main" --progress 60 --detail "tests"       # a bar instead of the status line
open-island activity done "CI main" --detail "all green"                  # green card
open-island activity fail "CI main" --detail "2 tests failed"             # red card
open-island activity clear
```

Like `progress`, one activity is on show at a time (the latest): a card for three seconds when it starts, then a bubble
beside the pill with its icon and status. Colours: red orange yellow green teal blue purple pink white gray or `#rrggbb`;
icons are the ones bundled (Tabler names such as `git-branch`, `bolt`, `cloud`, `download`, `music`, `mail-forward`...).

## Bluetooth devices (Linux)

Through `bluetoothctl`: a card when headphones, earbuds, a mouse or a keyboard connect (with the battery when the
device reports it), a warning once when it falls to 15%, and the connected ones with their battery on the Quick settings and System pages.

## Weather

A Weather page (after the calendar in the dock, or click the weather on the home page): the day in three-hour steps
with a glyph and temperature each (hours already gone are dimmed, the current block is lit), the chance of rain hour by
hour, and one line saying when it rains next ("Rain in 40 min", "Rain around 18:00 · in 3 h", "Raining · clears in 25
min", or that none is expected in the next 24 hours). When rain is due within six hours, or it is raining, the home page
shows that line in place of today's high and low. Needs the latitude and longitude (settings page). Data: Open-Meteo.

## Building with `just`

`just build` asks for the target (Linux, Windows MSVC, Windows GNU, macOS check) and leaves the result in `dist/<target>/`;
`just build windows` skips the menu, `just windows-zip` makes `dist/open-island-windows-x64.zip` to send to someone.
Cross-building for Windows from Linux needs `cargo install cargo-xwin` (MSVC, recommended) or mingw-w64 (GNU).
`just check-all` runs the tests, clippy and the Windows (cargo-xwin) and macOS type-checks.

On Windows (and anywhere the island is a plain window) it hides while a maximized window is in front, and comes back when
the pointer touches the top edge near the middle of the screen; a notification, a timer or the volume still show over it.
Windows draws through DirectX 12 into a transparent DirectComposition surface; `OPEN_ISLAND_OPAQUE=1` falls back to
cutting the window to the island's shape.

## Smoothness

`OPEN_ISLAND_PERF=1 open-island` logs one line per animation: frames, the interval between them (8.3 ms is a steady
120 Hz), the slowest one, how many took over 20 ms, and what drawing cost. The island asks for the low-power GPU
(`WGPU_POWER_PREF=low`, change it in the environment); on a laptop with a discrete GPU, installing the integrated GPU's
Vulkan driver (`vulkan-radeon`, `vulkan-intel`) lets it run there instead of keeping the big GPU awake.

## Options

The same settings as the Noctalia island plugin, on the Options page (the toggle icon in the dock: tabs Island, Alerts, Music)
or in `config.json`, or from a script with `open-island msg set <option> <value>`:

| tab | option | default |
|---|---|---|
| Island | `clock_in_pill`, `status_icons`, `clipboard_history`, `hide_on_fullscreen`, `weather`, `timer_sound` | on |
| Island | `timer_presets` (`["5m","10m","25m"]`), `screen` (`focused` or a name such as `eDP-1`; pinning by connector name, on Wayland too; an unknown name is logged and the compositor chooses), `timer_sound_file` | |
| Alerts | `show_notifications`, `show_osd`, `battery_alerts`, `meeting_alerts`, `rain_alerts` | on |
| Alerts | `quiet_in_meetings`, `quiet_on_mic`, `quiet_on_screen_share` (extras of this island) | on |
| Alerts | `notification_seconds` (2-30, 5), `osd_seconds` (0.5-5, 1.6), `low_battery_percent` (5-50, 15), `meeting_lead_minutes` (1-60, 10: the card), `meeting_pill_minutes` (1-120, 10: the pill on the side) | |
| Music | `media_auto_expand`, `lyrics`, `lyrics_in_pill` (off), `art_tint` | on |

Not carried over, because they are about running inside Noctalia: `style` (the island is always black), `update_check`.

## Notification rules from the island

On the Notifications page every row has a crossed bell: it hides that app's notifications from then on. The Rules tab of
the Options page lists the rules (hidden or silent), removes them, and adds one by app name (hide the notifications, or
only silence their sound). The same rules can be written by hand in `config.json` as `notification_filters` (with a
`content` word to match, too).

## Fonts and scripts

The UI font (the configured one, else SF Pro, Inter, Segoe UI...) draws the text. A character it lacks is looked for in the
other installed fonts, by script: kana and kanji in a Japanese font (Noto Sans CJK JP, Yu Gothic, Hiragino...), Hangul in a
Korean one, Chinese in a Chinese one, then Arabic, Thai, symbols and the like in a Noto font or any font that has it. The
font is loaded the first time it is needed. On Linux install `noto-fonts-cjk`; Windows and macOS ship with such fonts. There
is no shaping, so Arabic and Indic scripts come out letter by letter, not joined. Colour emoji are not drawn.

## Audio output follows the headphones (Linux)

When Bluetooth or USB headphones connect (or a headphone jack is plugged in) the island makes them the default output and shows
a short "Audio output" card; when they go, the output falls back to the best one left. WirePlumber does that on its own until
something pins a default output (the sink button of the quick settings, or a mixer, stores it as
`default.configured.audio.sink`), and from then on new headphones are ignored: that is what this works around. Switch it off with
`auto_audio_output` (Options > Island > "Audio to headphones").

## The sound page (Linux)

The output row of the quick settings opens it (`open-island msg page audio`): every output and input to pick from, and each
app that is playing with its own volume (minus / plus by ten, the speaker icon mutes just that app). It reads `pactl` when
PipeWire reports a change, so it follows headphones coming and going.

## Today

The clock icon in the dock (right after home, `open-island msg page today`) lists the day in one column: calendar events and
reminders in time order, what is over dimmed, and a line for "now" between it and what is still to come. The header says what
is next and in how long; an event with a link has a join button on its row.

## Send files to another device (LocalSend)

`open-island send report.pdf photo.jpg` opens a page that lists the devices on your network running
[LocalSend](https://localsend.org) (phones, other computers; the app must be open there), found by looking at the /24 network
for its port, and sends the files to the one you click. The progress pill shows it (the other side has to accept first), then a
card says how it went. It speaks LocalSend's protocol v2 (self-signed HTTPS, which is why the certificate is not checked), so
nothing else has to be installed on this machine. On Wayland you can also drag files from a file manager onto the pill: it
opens, and dropping opens the same page (the drop target is the island's own area, so the pill must be under the pointer).
Not done: receiving, and dropping on Windows and macOS.

## One-time codes

A notification that says it carries a code ("verification code", "codice", "OTP", "PIN"...) next to 4 to 8 digits shows the
code large; a click copies it. The code is never put in the clipboard history, and it leaves the clipboard after thirty seconds
if it is still there. Times, amounts, dates and phone numbers are not taken for codes.
