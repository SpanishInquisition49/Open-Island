# open-island

A macOS-style **dynamic island** for any desktop, in Rust. A black pill at the top of the screen that
morphs between the clock, music (cover, progress, synced lyrics), notifications (with actions), volume and
brightness, charging, timers (countdown, Pomodoro, stopwatch), weather and rain heads-up, calendar meetings,
long-running operations (a progress bar your scripts can drive) and privacy indicators. Around that it grew pages of its own: a map
that knows where you are, a day timeline with the way to your events, a sound mixer, a clipboard with pins, an activity
ledger, file sending to other devices, and cards for what needs attention (a one-time code, a program hogging the CPU, a
network that broke, rain before you leave).

It is a port of a Noctalia (Luau) plugin to a standalone program: one look (the macOS one), no shell
dependency.

```
crates/core   pure logic, no OS access: faces, springs, timers, lyrics, weather, iCal, bar, map and route maths, activity ledger  (about 130 tests)
crates/ui     the state machine and the vello renderer, no windowing: draws every face, headless-testable  (about 125 tests)
crates/app    window backends (Wayland layer-shell, winit), the OS services, the `open-island` binary  (about 100 tests)
```

## Status

| | Linux (Wayland, wlr-layer-shell) | Windows | macOS | Linux X11 |
|---|---|---|---|---|
| island, springs, all faces, hover / click / scroll / keyboard | yes | built, **not run** | built, **not run** | yes (a visible rectangle on NVIDIA) |
| window | layer surface, fractional scale, click-through margin | one fixed-size transparent window (never resized while it animates), top-most, no taskbar button; its region decides what takes the pointer | winit window, transparent | winit window |
| clock, timers, Pomodoro, stopwatch, control socket, `notify`, `progress run`, `autostart` | yes | yes | yes | yes |
| weather, calendars (feeds, iCloud / CalDAV), lyrics | yes | yes | yes | yes |
| media with cover art, several players | MPRIS | SMTC (built) | **not yet** | MPRIS |
| notifications | the island is the daemon (replaces mako, dunst, Noctalia's) | other apps' toasts are read from Windows' notification database (a plain program cannot be handed them), plus `open-island notify` | **only `open-island notify`** | the island is the daemon |
| volume card, volume / mute | wpctl, pactl | Core Audio (built) | **not yet** | yes |
| brightness, Wi-Fi / Bluetooth switches, power profile | brightnessctl, nmcli, bluetoothctl, TLP / powerprofilesctl | brightness through WMI and the switches through the radio API, the power plan through powercfg (built) | **not yet** | yes |
| battery, system page | sysfs, /proc, nvidia-smi | sysinfo + starship-battery (built) | the same (built) | yes |
| privacy dots (microphone, camera, screen share) | pactl, /dev/video, pw-dump | the privacy registry (built) | **not yet** | yes |
| clipboard history | wl-clipboard | arboard polling (built) | arboard polling (built) | **not yet** |
| keep awake, night light | systemd-inhibit, wlsunset | SetThreadExecutionState; no night light (no public API) | **not yet** | yes |
| hide under fullscreen windows | niri | shell notification state (built) | **not yet** | **not yet** |
| map and position, trips to events, place search and routes | GeoClue + OpenStreetMap, OSRM, Transitous | the same services, the position from Windows' location service (built; the settings location when it is off) | the same, **no position** | the same as Linux |
| sound page, heavy-program card, network card | pactl, /proc, nmcli | Core Audio sessions, sysinfo (pause with NtSuspendProcess), a ping of the router and Windows' own connectivity probe (built) | heavy-program card only (built) | yes |
| audio follows headphones | pactl | **not yet** | **not yet** | yes |
| who uses the microphone and the camera (with Mute / Quit) | pactl, /dev/video | the names from the privacy registry, Quit only (built) | **not yet** | yes |
| activity page, auto focus, away recap | niri focus events, `ext-idle-notify` | the window in front (built); no away recap | **not yet** | **not yet** |
| LocalSend (send files), one-time codes, clipboard actions | yes (drag a file onto the pill on Wayland) | sending yes, drag no | sending yes, drag no | sending yes |

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
  "notification_filters": [{"app": "zen browser", "content": "unseen", "show": false}],
  "follow_position": true, "travel_mode": "car", "map_tile_url": "osm", "deep_work_minutes": 20 }
```

Calendars: a feed (`.ics` / `webcal://`, public or secret address) or a CalDAV account. **iCloud** needs the Apple ID and an
app-specific password (appleid.apple.com > Sign-In and Security > App-Specific Passwords), not the Apple ID password: every
calendar of the account is read with its own name and colour. Each calendar can be renamed and recoloured from the menu (the
old `calendar_urls` list still works and is migrated). `password_cmd` prints the password (a password manager) instead of
keeping it in the file, which is owner-only anyway.

The weather, the map and the trips use where the machine is (GeoClue, see "Following your position"); the latitude and longitude
above are the fallback, and the whole answer when `follow_position` is off. The island calls out only to the services a feature
needs: see "What leaves the machine" at the end.

## Finding a page

There are many pages, so three ways in. The bar at the bottom of a page holds Home, whatever is live (music, a timer, the next
event, a running activity), the pages you open most (it learns, `dock_icons` sets how many, 4-12, 7 by default) and the page you
are on; the last icon opens **All pages**, a grid of every page in groups (Day, Now, Messages, Device, Island), one click each.
And in the command bar `go <page>` (also `vai`, `apri`) opens one by name, in English or Italian, or by the start of a name when
only one page begins so: `go phone`, `go meteo`, `go sett`, `go all`. The control socket takes the same names: `msg open phone`,
`msg page all`. The swipe order is unchanged. Usage is counted in `page-usage.json` in the state folder.

## Moving between pages

Every page carries a row of page icons at the bottom (media and timer first when active, then home, calendar, quick settings,
system, activity, clipboard, options, settings; the map and today right after home): click one to jump straight to it. A horizontal swipe, Tab / Shift+Tab (keyboard mode) and
`open-island msg page next|prev` step through the same list, and the content slides the way the pages are ordered. The back arrow
returns to the page that led there.

## `open-island doctor`

Besides the session, the graphics and the tools, it now has a "Your day" section: where home is, the commute and the card times, the pinned train, how many events are
kept for offline use and when they were fetched, and the currency rates: what each day feature has to work with, and the command that sets it when it is missing.

## The notification card

A notification does not take over the pill: a drop forms inside it, falls on a liquid neck that pinches off and spreads into a card just below, then is gathered up and
swallowed again when it goes. The card shows the icon or picture, the title and the sender, up to three lines of text, and on the right a ring that drains with the time
left, or the buttons the sender offers. With the pointer on it the countdown waits (for up to 5 s after the pointer last moved), the icon becomes a close button and the
ring becomes snooze; a click on the card opens the sender. The motion and the look are
ported from [drip](https://github.com/jrnxf/drip) (see `THIRD-PARTY-NOTICES.md`).

## Conversations, not piles of cards

Notifications are grouped by conversation (the same app and the same title: a chat or a sender, a site).

- **On screen**: a new message of the conversation whose card is showing joins that card instead of waiting for its turn: the card shows the latest words and
  "Telegram · 3 new", its timer starts again, and the earlier messages are closed towards their sender (so five messages in a lecture are one card, not five in a row).
  Messages of other conversations of the same app still wait their turn, and a critical one is never folded into another.
- **In the history**: the Notifications page shows a conversation as one row ("Telegram · now · 3 messages", the sender and then the latest words). A click on the row opens the app
  and clears the whole conversation; the x removes all of it. A conversation is the consecutive messages, within half an hour of each other; a different sender or a long
  silence starts a new row.

## One notification daemon

Only one program can own `org.freedesktop.Notifications`; another daemon (mako, dunst, swaync...) showing popups next to the
island is why every notification appeared twice. At start the island therefore stops the daemon it finds (its systemd unit
when it is named after it, else SIGTERM) and takes the name. Set `"take_over_notifications": false` to keep the other one (the
island then only watches, without actions or replies). D-Bus can start the old daemon again when a notification arrives while
the island is not running: `systemctl --user mask mako` stops that. Noctalia is never killed: the island asks it, with `noctalia msg config-set notification.enable_daemon false`, to stop
claiming notifications (undo with `... true`). Any other owner is left alone and named in the log.

## Controlling it

`open-island msg <event> [payload]`: `peek`, `open [page]` (keyboard navigation: arrows move a focus ring, Enter
activates, Tab / Shift+Tab change page, Escape closes), `page next|prev`, `page <home|today|map|cal|qs|sys|activity|audio|send|notifs|media|timer|clip|focus|settings>`, `dnd on|off`, `timer 25m`,
`pomodoro`, `stopwatch`, `timer-cancel`, `reply` (reply to the notification on screen: bind it to a key), `send <file>...` (see LocalSend below), and for long operations `open-island progress run [-l label] -- command...` (runs the command on a pseudo-terminal, so rsync, curl, pacman... still draw
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

MIT. Tabler icons are MIT (`crates/ui/assets/TABLER-LICENSE.txt`). The notification card is ported from drip, MIT (`THIRD-PARTY-NOTICES.md`).

### The iPhone (through tether)

With [tether](https://github.com/zackb/tether) paired to your iPhone, the Phone page (`go phone`, `msg open phone`; after Today and
Map in the dock) shows:

- the phone's **battery** and what the Bluetooth link carries;
- **calls**, when call control is on (a button on the page turns it on): a ringing call is a card with a sound and a green and a red
  button on the page to answer or hang up, and in a conversation a phone button dials the number (the audio stays on the phone);
- the **message conversations** (SMS and iMessage), newest first; a click opens one (the last four messages) and "Reply" sends a
  reply through the phone;
- **a new message**: "New message…" looks a name up in the phone's contacts and writes to its first number;
- a **card for each new message** (checked every 20 seconds, calls every 4 while call control is on), since the iPhone's own
  notifications do not come through on every setup (see `tether --bt-connection`).

The island only runs the `tether` command (`--bt-connection`, `--bt-threads`, `--bt-messages`, `--bt-send`, `--bt-calls`,
`--bt-answer`, `--bt-hangup`, `--bt-call`, `--bt-calls-enable`, `--bt-contacts`), always as separate arguments and never through a
shell; a reply or a search that starts with a dash is changed so it cannot be read as an option, only plain numbers are dialled, and
only conversation keys from the list are used. The page is not offered when `tether` is missing or no phone is connected. Not done:
the AirPods, the iPhone's other notifications.

### Files to your own devices over Tailscale (Taildrop)

The Send page (`open-island send file...`) also lists the devices of your tailnet that Taildrop can reach right now (`tailscale file
cp --targets`), marked "Tailscale", next to the LocalSend ones: a click sends with `tailscale file cp`, the pill says where it goes
and a card says how it ended (Taildrop reports no progress). Receiving is not done here on purpose: whatever already empties the
Taildrop inbox (`tailscale file get`) keeps doing it, and two readers would take files from each other.

### The day, planned: battery and files

Two things the morning and evening cards, the Today page and a card now and then do for a student who spends the day away from a socket.

- **Will the battery last the day?** The island learns how fast your battery drains from its own use (percent over time on battery,
  kept in `power.json` and blended, so one compile does not rule) and knows when the day ends: the last timed event plus
  `commute_minutes`. If the charge now is not what the day needs to get you home with the low-battery level left, the morning card
  says "Battery 54%: the day needs ~62% (home ~19:25)", a card (once a day, with a sound) says when it would run out, and when it
  is plugged in and about to be left a card says to charge a little more. The evening card says what to charge to before leaving
  tomorrow ("Charge to 70% before you leave"). Nothing is said before a drain has been learned, and nothing when the battery will
  do. The "Battery alerts" switch applies.
- **The files for the next lecture.** For the next event (and tomorrow's first) the island looks in your study folders for
  documents whose path says the same words as the title ("Algorithm Engineering" finds `~/Uni/Algorithm-Engineering/slides/...`,
  `AlgorithmEngineering-2027_c1.pdf` in Downloads), best match first, slides and books before notes, the newest first, and shows the
  top three on the Today page ("Files · Algorithm Engineering", a click opens one) and one on the cards. Folders: `study_folders`
  in the configuration (`["~/Uni", "~/Documents/Corsi"]`), else Documents, Downloads and `Uni`, `Studio`, `Courses`... when they
  exist. Only file names and dates are read, never the contents; hidden and build folders are skipped, links are not followed;
  about 30 ms for a home folder. A title of short words ("Lab") has nothing to look for.

### What to take along

`bring laptop for reti` in the command bar (`porta ... per ...` too) makes the morning and evening cards say "Bring laptop" on the
days with an event whose title has "reti"; without `for`, the thing is on every card. It stays until `unbring laptop` (a word of the
thing or of the course), `unbring all` or just `unbring` (the newest). Kept in `bring.json`; thirty entries at most.

### The iPhone: battery and where its audio plays

The iPhone page offers a switch for where the phone's music and sounds play (this computer or the phone, `tether --bt-phone-audio`),
and a card says when the phone's battery drops to 20% (again only after it has been above 30%; the "Battery alerts" switch applies).

### Text from an image (OCR)

A screenshot's card (the compositor's "Screenshot captured") becomes one with a **Copy text** button that does the same; switch it off with
`ocr_screenshots` (Options > Alerts > "Text from screenshots").

`ocr` in the command bar (or `open-island msg ocr`) reads the text of the image on the clipboard (a screenshot of a slide, a
scanned page) with Tesseract and puts the text on the clipboard; a card shows how it starts. It asks for Italian and English
when both are installed (`tesseract-data-ita`), joins words cut by a hyphen at the end of a line and drops the empty lines in a
row. Offline: the image goes to the local `tesseract` and is written, for that moment only, to a private file in the runtime
folder. Linux (wl-clipboard + tesseract); other systems say it is not there yet. Not done: reading by itself after every
screenshot.

### Downloads

A browser download shows as an activity with what has arrived and how fast ("48 MB · 2.1 MB/s"), and turns green with the size and
the time when the file is complete. No extension: the island looks at the download folder (the XDG one, else `~/Downloads`) for
the files a browser writes meanwhile (`.part` Firefox, `.crdownload` Chrome / Edge / Brave, `.download` Safari, `.opdownload`
Opera). The total size is not known, so there is no percentage; a download that stops growing for 20 s says "stalled", one that
disappears without its file is dropped quietly. Only names and sizes are read. Off with `watch_downloads` (Options > Island > "Show
downloads"). Not done: torrent clients' own APIs.

### Notes and waiting, from the shell

```sh
open-island pin "ssh pi@10.0.0.5 -p 2222"     # or: echo 10.0.0.5 | open-island pin  ·  in the bar: pin <text>
open-island unpin ssh                         # a part of the text; no words = the newest; `unpin all`
open-island watch 4821 "render"               # a card when process 4821 ends, with how long it took
open-island watch ffmpeg                      # by program name: it ends when the last one is gone
```

A pin stays on the Today page (the newest four are drawn, eight are kept, they survive a restart); a click copies it,
the cross takes it off. `watch` also works for what you did not start through the shell hook; while it waits it shows as a
pill, and since the exit code of a process that is not its child cannot be read, it only says "done", never "failed".

## A pill for every long shell command

```sh
eval "$(open-island hook zsh)"      # in ~/.zshrc  (bash: ~/.bashrc, fish: `open-island hook fish | source` in config.fish)  # PowerShell: open-island hook powershell | Out-String | Invoke-Expression  (in $PROFILE)
```

A command that runs longer than 10 seconds becomes a progress pill with its name; when it ends it turns green with how
long it took, or red with its exit code. Editors, pagers, `ssh`, `top`, REPLs and similar are skipped. Set
`OPEN_ISLAND_HOOK_SECONDS=30` to change the threshold and `OPEN_ISLAND_HOOK_IGNORE="make,go"` to skip more programs.
A shell that is closed mid-command takes its pill with it.

## Coding agents

An agent (or any script) can use the island as its dashboard without a plugin: its turns show as activities, it can start
timers, and it can call you. Several sessions run side by side, each under its own id; the pill shows one, the Today page
lists all of them with their status line, and an activity that ends (or fails) is shown for a moment before the next takes over.

```sh
open-island agent setup claude        # prints the hooks block for ~/.claude/settings.json (merge it in yourself)
```

With those hooks Claude Code needs nothing else: a prompt starts a turn ("Claude · repo: thinking"), each tool call updates
the status line ("Edit lib.rs", "Bash cargo test"), `Stop` ends the turn, a `Notification` (a permission question, a wait for
input) becomes a card, `SessionEnd` forgets the session. The hook always exits 0 and never waits for the island, so it cannot
slow the agent down or break it when the island is not running. For any other agent:

```sh
open-island agent start --id cx1 --name Codex "reading the repo"
open-island agent tool  --id cx1 --name Codex "running tests"
open-island agent done  --id cx1 --name Codex "3 files changed"      # or: fail, end (forget it)
open-island activity run --id build --progress 40 --detail "linking" "make all"   # the generic form, with a percentage
open-island timer 25m review the diff        # a named countdown; several can run; msg timer-cancel review ends it
open-island notify "Tests failed" "api: 2 failures"
```

## The command bar

Bind `open-island msg bar` to a key (niri: `Mod+Space { spawn "open-island" "msg" "bar"; }`). Type, Enter runs it, Esc closes.
While the bar is empty it shows examples, a set every four seconds; Up and Down bring back the lines you ran before (the last 30, kept
across restarts, a password line is never kept); a command that starts right but is not finished ("remind", "timer", "days") shows
how to write it instead of "Not understood":

| type | does |
|---|---|
| `10m tea`, `1h30m`, `timer 25 pasta` | a timer (several can run at once: the one ending first is shown in the pill, the others on the timer page and on Today; a Pomodoro or the stopwatch keeps the main place and the countdowns run beside it); the words after the time name it (shown on the Today page and when it rings) |
| `remind 20m call mom`, `in 2h x`, `ricordami tra 10m …`, `remind 15:30 …`, `remind at 9pm …`, `ricordami alle 21 …`, `remind tomorrow 8:30 …`, `remind friday 18 …`, `ricordami venerdì alle 9 …` | a reminder (a notification when due; kept across restarts; a weekday is the next one, nine o'clock when no time is given); after `at` / `alle` a bare number is an hour, not minutes |
| `todo buy milk` (also `compito …`, or `open-island todo buy milk` from a script), `todo report due fri #algoritmi ~2h`, `todo gym every mon and thu`, `todo daily bring the charger` | something for the list on the Today page: a circle to tick off, kept across restarts. It can carry a day (`due fri`, `entro 3 feb`, `tomorrow`), a course (`#algoritmi`), the work it takes (`~2h`) and a repetition: see "The organizer" |
| `remind every monday 9 gym`, `ricordami ogni giorno alle 8:30 …`, `remind every month on the 27th pay rent` | a reminder that comes back: it rings and is set again for the next day it falls on (nine o'clock when no time is given) |
| `remind 10m before lab`, `ricordami 30 minuti prima della lezione di reti` | rings that long before the next event with those words (set for its time now: if it moves, say it again) |
| `move report to sat`, `move lab to mon 10`, `sposta lezione a domani 10-11:30` | a task's day, or an event (with hours, or when no task matches): the length stays; a task or event is found by the words of its title |
| `edit report -> due mon ~3h #algoritmi` | changes only what the right-hand side names |
| `rm report`, `cancel dentist` | removes a task (also on the server) / cancels the next event with those words (on its calendar) |
| `event fri all day conference`, `evento domani tutto il giorno gita` | an entry without hours (it blocks nothing, so it never flags an overlap) |
| `event fri 15-16:30 office hours`, `evento domani 10 dentista`, `event 3 feb 14:00 for 2h exam prep` | an event on the calendar: shown at once, written to the calendar, and a word when it overlaps another |
| `note ask about the exam format`, `open-island note …` | a line in the notes folder (one Markdown file a day) |
| `find compilers`, `cerca reti` | a search through events, tasks, deadlines, pins, notes and study files (the Find page) |
| `log 90m algoritmi`, `studiato 1h30 reti`, `pomodoro algoritmi` | study time done; it comes off the work still to do for that course. A Pomodoro with a course logs its focus time by itself |
| `esame 3 feb Algoritmi`, `deadline 3/2 project` | a date to count down to (see "For a commuting student") |
| `until class` | a timer that ends five minutes before the next event |
| `pomodoro`, `stopwatch`, `focus 45`, `dnd on` / `dnd off` | the matching mode |
| `12*7+3`, `sqrt(2)`, `2^10`, `(1+2)*3` | a sum; Enter copies the result |
| `20% of 150`, `80 + 15%`, `80 - 20%`, `split 84.5 by 3` | a share, a price with a rate, what each person of a bill pays; Enter copies it |
| `pw 24`, `uuid`, `roll 2d6`, `d20`, `coin`, `random 1-100` | something generated when you press Enter and copied (a password is copied like a verification code: out of the clipboard history, cleared after 30 s, not shown on the card) |
| `days until 3 feb`, `giorni al 25 dic`, `today + 45 days`, `3 feb - 2 weeks`, `31 jan + 1 month`, `week`, `3 feb` | date arithmetic and the day of the week (dates are day first; a day and month without a year is the next one to come; `week` is the ISO week) |
| `20 usd in eur`, `100 eur to jpy` | a currency conversion with the ECB's daily rates (needs them once; works offline with the last ones) |
| `5 km in mi`, `70 f to c`, `1 gib to mb` | a conversion (length, mass, volume, speed, data, temperature); Enter copies it |

## The organizer

Everything here is local first and works without an account. Nothing in it knows any other program or project: the notes
folder is any folder, the account is any CalDAV account.

- **Tasks** (`todo …`): a task has a text and, optionally, a **day** (`due fri`, `by 3 feb`, `entro domani`, or just `tomorrow` /
  `domani` anywhere in the line), a **course** (`#algoritmi`, `#reti_di_calcolatori`), the **work** it takes (`~2h`, `~90m`, `~1h30`)
  and a **repetition** (`daily`, `every weekday`, `every mon and thu`, `ogni martedì e giovedì`, `every month on the 27th`). The Today
  page orders the list: late ones first (in orange), then by day, then those without one; each row ends with its day and its estimate.
  A repeating task rests until its day comes; ticking it moves it to the next one instead of removing it.
- **Study plan**: an exam or hand-in with an estimate (`esame 3 feb Algoritmi #algoritmi ~20h`) or a task with one is spread over the
  days left (in steps of five minutes; all of it on the last day). The morning and evening cards end with "Study 1h20: Algoritmi exam 45m,
  Report 35m". `log 90m algoritmi` (or a Pomodoro `pomodoro algoritmi`) takes the time off the nearest-due item of that course, so the
  plan shrinks as you study.
- **The Plan page** (`go plan`, `go week`; it is on the page grid, and in the dock once you open it): the next seven days, each with a
  bar (lectures solid, study dim), "3 events · 4h30 · 2 due · study 1h20", and in orange when the day is heavy (six hours of events, three
  things due, or nine hours with the study). Below: today's study and the time studied this week, per course.
- **Changing what exists**: `move`, `edit`, `rm` and `cancel` act on what the island shows; the change goes to the server too (`PUT` of the changed event, `DELETE` of a removed task or event). An event that repeats is refused with a card, since "this one or all" is for the calendar app. The server is not asked whether the item changed meanwhere (no `If-Match`): the last write wins.
- **Without the bar**: right-click a row of the to-do list and it turns into a menu: Tomorrow, +1 week, Edit (opens the bar with `edit <words> -> `), Remove; right-click again closes it. A reminder that rings offers +1 hour, Tonight (until 19:30) and Tomorrow (09:00). The evening card names what is left of today and offers "To tomorrow"; the morning card names what is late and offers "Move to today".
- **A heavy day ahead**: the morning and evening cards say when today, tomorrow or the day after is heavy, and when a task due then could be done on a lighter day before it, which one ("Thu is lighter: move \"Report\" there?").
- **From scripts and shortcuts**: `open-island do "<any bar line>"` runs a line as if typed and sent (`do todo report due fri ~2h`); `open-island event …` and `open-island remind …` are shorthands. A line that is not understood shows a card.
- **Events from the bar** (`event fri 15-16:30 office hours`): the day (`fri`, `domani`, `3 feb`, today when omitted), a time or a range, an
  optional `for 1h`, then the title. Written to a CalDAV calendar when an account is set (`event_calendar`, below), else to a calendar file of
  the island's own (`local-events.ics` in the state folder, shown as the calendar "Island"). If the server refuses, it is kept there.
- **Notes** (`note …`, or `open-island note …` from a shell, which needs no running island): a line `- 09:05 text` in `<notes_dir>/YYYY-MM-DD.md`.
- **Find** (`find …`, `open-island find …`): every word must match (case and accents ignored). The island's own things come at once; the notes
  folder (`.md` and `.txt`, three levels deep, newest first) and the study folders (file names) follow. A click opens a file or a note, and
  copies the rest.
- **Tasks on a server**: with `tasks_sync` the tasks are kept on a CalDAV account too (VTODO), so the phone's reminders app and the island
  see the same list. The island sends a task when it is made, changed or ticked, and reads the list every five minutes; a task finished
  or deleted elsewhere goes from the island, one the server has never seen (made offline) is sent. A repeating task is kept with its RRULE
  (daily, weekly on given days, monthly on a day; a rule with an interval or a count is not claimed).

| setting (`config.json`) | meaning |
|---|---|
| `notes_dir` | where `note` writes (`~` is the home); empty: a `notes` folder in the island's state folder |
| `tasks_sync` | `true` keeps the tasks on a CalDAV account (needs a CalDAV account in the calendars) |
| `tasks_account` | the account's name or user for the tasks; empty: the first CalDAV account |
| `task_list` | the name of the to-do list on the account; empty: the first list that holds to-dos |
| `event_calendar` | where new events go: `"Personal"` (a calendar of the first account) or `"Uni/Personal"` (account / calendar); empty: the first calendar of the first account. No account: the island's own file |

## Other kinds of calendar

Beyond a feed address and an iCloud account, "Add a calendar" on the Settings page (or `open-island msg calendar add ...`) takes:

- **An .ics file or a folder of them** on this machine (`~/Documents/orario.ics`, `/path/to/folder`, `C:\Users\me\orario.ics`, `file://...`): a timetable
  exported by the university, a feed downloaded once. A folder is read one level deep (every `.ics`); the files are read again at every refresh, so
  replacing one is enough; a missing file shows as an error on the Settings page.
- **Public holidays**: `holidays` (or `festività`, optionally with a country code): worked out here, no network and no file (Easter included),
  as all-day events. Only Italy (`IT`) so far.
- **Any CalDAV account** (Nextcloud, Fastmail, Radicale, Synology, Baikal...): "Add a CalDAV account" asks for the server address, the user and the password
  in three steps; every calendar of the account is read, like iCloud's. Use an app password where the service has them. Only iCloud has been tried against a
  real server; the others rely on the same standard discovery, so a server that answers differently may need the address of its DAV root.
- **Google Calendar, Outlook / Microsoft 365, Proton, and the like** through their *secret iCal address*: Google: calendar settings > Integrate calendar > "Secret address in
  iCal format"; Outlook on the web: Settings > Calendar > Shared calendars > Publish a calendar > ICS link. Paste it as a feed. Signing in with an account (OAuth) is not built:
  it needs an application registered with each provider.

## Trains on the map

Beside car, on foot and public transport, the map (and the trip row of the Today page) has a **train** way, and it works as a person thinks of it:
on foot to the railway station **nearest to where you are**, the train, off at the station **nearest to where you are going**, on foot from
there. The line is drawn dashed on foot and solid on the train, and the time is door to door, the wait for the train included. Only
Trenitalia's own stations are considered (a station that other feeds list too is not offered twice); the train between the two stations is asked of the
timetable (Transitous), so the times are the programmed ones, not the live delays, which only the pinned train below has. When the two nearest
stations are the same one, or no train links them, the planner chooses the stations itself, rail only, with up to an hour of walking at each end.
`travel_mode` accepts `train` (also `treno`, `rail`).

## The usual train (Trenitalia)

Pin the train you usually take and the island follows it every day: its delay, its track, whether it runs.

```
train pisa centrale 17:12 verso lucca      # in the bar; also: treno ..., open-island msg train ...
train off                                  # unpin
home 43.7085, 10.4036                      # or an address: home Via Roma 1, Pisa
```

- **Pin**: the station, the time of the train and (optional) where it goes. The island asks the departure board of that station
  for the train leaving at that time (and, when a destination is known, checks that it stops there), and keeps the train
  number. Without a destination it uses the station nearest to **home**; with none it keeps the train's last stop.
- **Settings page**: "Home and train" has a row for each (click to edit; the train row starts from the line that would pin it again and an empty line unpins it, an empty home removes it).
- **On the map**: when home is set, a green pin with a house marks it; a click on it gives the way home from where you are (by car, on foot, by transit or by train, as for any place); out of view it sits on the edge of the map, dimmed, toward home. The search plate is opaque and edged so that it reads over any map.
- **Home** is set by hand and nothing follows it: coordinates need no network at all, an address is looked up once (Transitous'
  geocoder, OpenStreetMap data) and only the coordinates are kept (`home_latitude`, `home_longitude`, `home_name` in the
  configuration). It only serves to find the station near home.
- **What you see**: a row on the Today page, "17:12 Pisa Centrale → Lucca 17:52" with "On time · track 3", "+6 min" (orange) or
  "+12 min" / "Cancelled" (red). From 90 minutes before the train leaves until 10 minutes after, the board is read once a
  minute; outside that, only the schedule is shown. A card with a sound says when it is 10 minutes late or more (again every 5
  minutes later) or cancelled.
- **Where the data comes from**: ViaggiaTreno, the service behind Trenitalia's own departure boards (stations, boards, train
  status). It is not a documented API: it answers clients that say who they are (the island sends `open-island`) and turns
  generic ones away; it may change without notice, and then the row simply shows the schedule. Nothing is sent but the station
  and the train number. The station near home comes from Transitous (`api.transitous.org`), the open transit data the island already uses for trips.
- **When it is late or cancelled** the row adds "Next: 17:42 REG 4177 · 18:12 RV 4181 +4": the trains that leave after yours for the same destination (same board, not cancelled, no bus), so there is no need to look for another. A train pinned before this existed learns its destination from the first board it is found on.
- **Not done**: other operators (Italo, regional lines that are not Trenitalia's).

## The weather in the cards, the room in the day

- **Weather**: the morning card ends with the weather of today and the evening card with that of tomorrow, from the hourly forecast between 07:00 and 22:00
  ("9–14°, dry", "9–14°, rain from 16:00"). When rain is likely (50 % or more, or 0.3 mm in an hour) the headline says it ("Tomorrow · 3 events, rain from 16:00"),
  because that is the news worth an umbrella. Nothing is said when the weather is off or not known yet.
- **Room**: an event with a place shows it, dim, at the right of its row on Today: the room and the building ("Aula 3, Polo Fibonacci"), not the whole address and never a video-call link.

## For a commuting student

- **The schedule works offline.** Calendars are kept on disk (`calendar-cache.json` in the state folder, readable only by you,
  sources named by a hash so that no password or address is written): on a train or in a lecture hall without a network the island
  starts with the last schedule it fetched and says when that was.
- **Commute** (Options > Day > "Commute", door to door in minutes, 0 = off; "Leave early" adds a margin): the Today page says
  "Leave 08:05 · 09:30 Reti di calcolatori" in its header, the morning and evening cards start with "Leave 08:05", and at that moment, once,
  a card says "Time to leave: Reti at 09:30 · Aula 3" (not if you open the machine more than 15 minutes late). The event is the first
  timed one with no video-call link, so an online lecture does not count.
- **Free time**: a pause of 45 minutes or more between two events shows as "Free · 1h 30m" on Today. `until class` (or `until next`,
  `fino alla lezione`) in the bar starts a timer that ends five minutes before the next event: study in the gap, not past it.
- **Deadlines**: `esame 3 feb Algoritmi`, `deadline 3/2 Consegna progetto`, `scadenza 2027-02-03 Tesi` count down on Today
  (the next three within two weeks, orange from three days, a click removes one) and join the morning and evening cards within a week; they go
  away a day after the date.
- **Every-day to-dos**: `todo daily bring the charger` (also `todo ogni giorno ...`) is ticked off for the day and is back tomorrow.

## The morning and the evening card


If the pinned train is late or cancelled and has not left yet (the board read in the last 20 minutes), the evening card opens with "18:50 train +12 min" before anything else; on time is not news.
Set a time (Options > Day > "Evening", or `"evening_recap_time": "18:30"` in the configuration, or
`open-island msg set evening_recap_time 18:30`) and at that time, once a day, a card shows tomorrow: how many events,
reminders and to-dos, and the first ones with their times ("Tomorrow · 3 events, 1 to do: 09:30 Reti di calcolatori · ..."). It stays
45 seconds, a click opens the Today page. It comes on weekdays only unless "Recap on weekends" is on, it is skipped when
there is nothing to say, a restart does not repeat it, and when the machine was off at that time it comes at the first
moment after it (until 23:30). In the options the first step from "off" lands on 18:00. The bar's `recap` (or `tomorrow`,
`domani`) shows it on demand.

The **morning card** is the same for today: set "Morning" (first step from off: 08:00) and on the first use of the day from that time
(until 14:00, so a machine opened late still gets it) a card shows what is left of today ("Today · 2 events, 1 to do: 09:00 ..."),
events that are over left out. It waits for the calendars to load (up to 30 s) before it decides, so it never says "nothing" because
the feed was slow. `today`, `oggi` or `agenda` in the bar shows it on demand. Both cards share the weekend switch. Quiet modes (a meeting, a call, do not disturb) hold it like any card; it is then in the history.

## Quiet by itself

Notifications are held (they stay in the history, and the sound stays off) while a calendar event is on (timed, up to
3 hours), while the microphone is in use and while the screen is shared; a small crossed bell shows it in the pill and
`critical` notifications still come through. Switch any of the three off in the configuration:
`"quiet_in_meetings": false`, `"quiet_on_mic": false`, `"quiet_on_screen_share": false`.

The bell with a z on a notification card (under the pointer) hides it and brings it back after ten minutes. When the island stops being
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
shows that line in place of today's high and low. It is for where you are (see "Following your position"), or for the coordinates of the settings page. Data: Open-Meteo.

## Building with `just`

`just build` asks for the target (Linux, Windows MSVC, Windows GNU, macOS check) and leaves the result in `dist/<target>/`;
`just build windows` skips the menu, `just windows-zip` makes `dist/open-island-windows-x64.zip` to send to someone.
Cross-building for Windows from Linux needs `cargo install cargo-xwin` (MSVC, recommended) or mingw-w64 (GNU).
`just check-all` runs the tests, clippy and the Windows (cargo-xwin) and macOS type-checks.

On Windows (and anywhere the island is a plain window) it hides while a maximized window is in front, and comes back when
the pointer touches the top edge near the middle of the screen; a notification, a timer or the volume still show over it.
Windows draws through DirectX 12 into a transparent DirectComposition surface; `OPEN_ISLAND_OPAQUE=1` falls back to
cutting the window to the island's shape. The window has one size (large enough for every page) and is not resized while
the island animates: resizing it and its swap chain was what made it stutter and flash a rectangle. Clicking a notification
brings the window of its app to the front.

## Smoothness

`OPEN_ISLAND_PERF=1 open-island` logs one line per animation: frames, the interval between them (8.3 ms is a steady
120 Hz), the slowest one, how many took over 20 ms, and what drawing cost. The island asks for the low-power GPU
(`WGPU_POWER_PREF=low`, change it in the environment); on a laptop with a discrete GPU, installing the integrated GPU's
Vulkan driver (`vulkan-radeon`, `vulkan-intel`) lets it run there instead of keeping the big GPU awake.

## Options

The settings are written to `config.json` whole-file-atomically and one change at a time (a temporary file renamed over it, under a lock), so two changes made together - the home that is looked up while you toggle an option - cannot undo each other, and an option you switched on (the precise position from Wi-Fi, say) stays on across restarts and updates: nothing but you ever writes it, and only an absent key takes its default.

The same settings as the Noctalia island plugin plus this island's own, on the Options page (the toggle icon in the dock), grouped by what they are
for: **Island** (how it looks and behaves), **Alerts** (what it shows you and for how long), **Quiet** (when it keeps its notifications to
itself), **Day** (cards at set times, the way to lectures), **Places** (where you are, what it asks the network), **Music**, **Rules**
(notification rules). Resting the pointer on an option says what it does; an empty tab says what it is for. The same keys work in `config.json`, or from
a script with `open-island msg set <option> <value>`:

| tab | option | default |
|---|---|---|
| Island | `clock_in_pill`, `status_icons`, `hide_on_fullscreen`, `clipboard_history`, `timer_sound`, `auto_audio_output` | on |
| Island | `mini_pills` (1-4, 2): how many small pills may sit beside the main one; a timer, focus, an event and each running activity (an agent's turn, a build) get their own, in that order; 1 is a single pill beside it | 2 |
| Island | `dock_icons` (4-12, 7): how many page icons the bottom bar shows (Home, what is live, the pages used most, the page you are on); the rest are on the All pages grid | 7 |
| Island | `timer_presets` (["5m","10m","25m"]), `screen` (`focused` or a name such as `eDP-1`; pinning by connector name, on Wayland too; an unknown name is logged and the compositor chooses), `timer_sound_file` (the sound file the timer plays) | |
| Alerts | `show_notifications`, `show_osd`, `battery_alerts`, `meeting_alerts`, `rain_alerts`, `culprit_alerts` | on |
| Alerts | `notification_seconds` (2-30, 5), `osd_seconds` (0.5-5, 1.6), `low_battery_percent` (5-50, 15), `meeting_lead_minutes` (1-60, 10: the card), `meeting_pill_minutes` (1-120, 10: the pill on the side) | |
| Quiet | `quiet_in_meetings`, `quiet_on_mic`, `quiet_on_screen_share`, `away_recap` | on |
| Quiet | `deep_work_minutes` (0-120, 20: auto focus), `break_minutes` (0-240, 0: off), `deep_work_apps` (a list in `config.json`) | |
| Day | `travel_times` | on |
| Day | `evening_weekends` (both cards on Saturday and Sunday) | off |
| Day | `morning_recap_time`, `evening_recap_time` (`"08:00"`, `"18:30"`, or minutes; `off`), `commute_minutes` (0 = off), `travel_margin_minutes` (0-30, 5) | off, off, 0, 5 |
| Places | `weather`, `follow_position` | on |
| Places | `currency_rates` (the ECB's daily rates for the bar) | on |
| Places | `wifi_position` (Linux: a position within metres from the Wi-Fi networks around you, looked up at beacondb.net; a card says so when you turn it on) | **off** |
| Places | `map_tile_url` (a preset name or a URL; see the map), `map_dark`, `map_api_key` (Settings page), `travel_mode` (`car`, `walk`, `transit`, `train`: on the Today page and the map) | OpenStreetMap, on, none, car |
| Music (a title or artist too long for its place slides back and forth, pausing at each end, instead of being cut) | `media_auto_expand`, `lyrics`, `lyrics_in_pill` (off; the compact pill widens, up to 640 px, to show the line whole), `art_tint` | on |

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

## Recap when you are back

After two minutes without the keyboard or the pointer (`ext-idle-notify`, any Wayland compositor that has it; set
`OPEN_ISLAND_IDLE_SECS` to change the two minutes) and then a touch, a card says what came in meanwhile ("3 while you were
away: Telegram 2 · Mail") and a click opens the list. It covers a lock screen too, since nobody touches anything there.
Switch it off with `away_recap` (Options > Quiet > "Recap when back"). Linux (Wayland) only for now.

## Which program is hogging the machine (Linux)

When the CPU stays above 85% for fifteen seconds and one program (all its processes together: a browser is dozens) is using
at least a core and 40% of what the machine does, or when less than 8% of the memory is left, a card names it ("ffmpeg is using
9.8 cores"). A click opens the System page, where a row for it has **Pause** (SIGSTOP) and **Quit** (SIGTERM); a paused program
is listed with **Resume**. The programs the session lives on (the compositor, the shell, PipeWire, systemd...) are named but
never offered for stopping. The same page lists who is using the microphone (with **Mute** / **Unmute** for that app's stream, through
`pactl set-source-output-mute`) and who has the camera open (with **Quit**), read every two seconds while the page is open. A program the island froze and that is still frozen when the island ends is woken again the next
time the island starts. Switch it off with `culprit_alerts` (Options > Alerts > "Heavy program alerts"). The check reads
`/proc` every five seconds and does more only while the machine is loaded.

## When the network breaks (Linux, NetworkManager)

The link, the router (one ping) and the internet (NetworkManager's own connectivity check) are looked at every ten seconds, every
five while something is wrong. A change that holds for two readings gives a card that says where it breaks: "No network", "Router
not answering" (connected, but the router is silent), "No internet" (the router answers, the problem is beyond it), "Sign-in
needed" (a captive portal). A click on the card switches the Wi-Fi radio (or the wired device) off and on again. A red Wi-Fi
icon sits in the pill while it is broken, and a card says "Back online" afterwards. The network row of the System page adds
"VPN" when a tunnel interface (wireguard, tailscale, tun, ppp...) is up.

## Getting to an event

An event with a place (not a link or a video call) gets a row on the Today page: four buttons, car, on foot, public
transport and **train** (the choice is `travel_mode`, also `open-island msg set travel_mode train`), and next to them how long it takes and
when to leave ("14 min · leave 17:41"). A card says "Leave in 5 min for ..." once, five minutes before the time to leave, and
it gets through the quiet mode like a reminder. The island asks for the way when the page is open or the event is within three
hours, and again every ten minutes. Only today's events get a way: one tomorrow morning is not asked about tonight.

Where the data comes from, all public services without a key: the place is found by Nominatim (OpenStreetMap), the road trip is
timed by the OSRM demo server (no traffic), and the walk and the public transport (buses, trains, trams, with the departure that
arrives in time) by Transitous. It starts from the location set for the weather. What leaves the machine: the place of the
event and the two coordinates. Switch it off with `travel_times` (Options > Day > "Travel times"); `travel_margin_minutes`
(default 5) is how much earlier than needed you are told. A short trip has no bus worth taking: transit then shows the walk.
The OSRM demo server only knows roads, so on foot comes from Transitous (or a straight-line estimate when it does not answer).

## Before you go

Forty minutes before an event, if there is something worth knowing about it, one card says so: "Lecture in 30 min: rain in 25 min,
take an umbrella · battery 21%, bring the charger". It stays silent when there is nothing to say. When the event has a place and
a way to get there (see above), the same notes are added to the "Leave in ..." card instead. The rain comes from the forecast
used for the weather (rain that falls or starts before you would arrive); the battery matters below 30% and unplugged.

## Activity: where the attention goes, rewind, auto focus (niri)

The island follows the focused window (niri's event stream) and stops counting when nobody has touched the keyboard or the
pointer for two minutes. Nothing leaves the machine; only today's totals per app are written to the state directory
(`attention.json`), once a minute.

- **Activity page** (dock, after the system page; `open-island msg page activity`). *Today*: the five apps that held the
  attention, with share and time, and how long you have been on the current one. *Rewind*: the last half hour in one list, newest
  first: windows (an app that had the focus for less than ten seconds is left out), notifications, tracks, and what you copied
  (a click puts it back on the clipboard).
- **Auto focus**: after `deep_work_minutes` (default 20, Options > Quiet "Auto focus", 0 = off) in a row on work apps (editors,
  terminals, PDF and note readers: `deep_work_apps` in `config.json` replaces the list; a detour shorter than a minute does not
  break it) notifications are held like in a meeting, a bell-off icon shows in the pill, and a recap card comes when you leave.
  Reminders and critical notifications still get through.
- **Break nudge**: `break_minutes` (default 0 = off) gives a card after that long on one app, then again after another period.

Other compositors than niri give no focus events yet, so the page stays empty there (the options still apply).

## The map

The map icon in the dock (after Today; `open-island msg page map`) shows where the machine is on a dark map, with the circle of
doubt of the fix, and the place of the next event as an orange pin joined to you by a dashed line, with the distance and the
travel time of the chosen way (a click on that chip opens directions on openstreetmap.org). **+ / -** zoom, **Me / Both**
switches between around me and fitting both, and the buttons below copy the coordinates, open the spot in the browser and look
for a new fix. The map is dragged with the pointer (a click on it does nothing) and the wheel zooms; once it is away from you a pin button in the corner brings it back.

The position comes from GeoClue (the `where-am-i` client that ships with it, which is allowed to ask without a configuration
change): Wi-Fi when the machine has it, else GeoIP, which can be 25 km off; the circle shows how sure it is. Without GeoClue
the location set for the weather is used (the page says so). The trips to events now start from the fix too (a fix of the last
quarter hour, asked for when there is none), not from the weather location.

Tiles come from OpenStreetMap's own server (no key; its light style, which the island darkens itself with an invert and a hue
turn so water stays blue and parks green; `map_dark: false` keeps it light). 256 px at a time, kept in
`~/.cache/open-island/tiles` for two weeks, as OSM's tile policy asks, with the credit drawn on the map and a User-Agent of
`open-island`. What leaves the machine when the page is open is the tile addresses, which say roughly where you are looking.
`map_tile_url` (Options > Places > "Map tiles", or `open-island msg set map_tile_url <name or url>`) changes the provider. It takes
a preset name or an https URL with `{z}`, `{x}` and `{y}` (and `{key}` where the provider wants its API key, which goes in
Settings > Map > "API key", `map_api_key`: stored in `config.json` like the iCloud password, drawn as dots, sent only to the tile
server in that URL). Presets, all fetched and looked at: `osm` (default), `esri-dark` (Esri's dark gray canvas, no key, no labels),
`esri-light` and `esri-street` (Esri, darkened by the island), `opentopomap`, `cyclosm` (darkened too), and `carto-dark`
(`https://basemaps.cartocdn.com/rastertiles/dark_all/{z}/{x}/{y}.png?key={key}`, needs a free key from carto.com/basemaps/apikey).
A URL with "dark" in it is not darkened again, and the credit on the map follows the provider. Providers that want a key may answer
a key-less request with a picture that says so (CARTO's did), so look at the map after changing it. Esri's tiles answer without a
key today, but I found no published terms for it: fine for a personal island, check before sharing a build. OSM's server is meant for light use: heavy use should go to a provider of your own.
### Searching a place and getting the way

The search button on the map takes a name or an address ("pisa centrale", "via roma 12 pisa"): the answers (Nominatim, nearest to
you first) come as a list over the map, and picking one gives the way from where you are, leaving now: by car along the roads
(OSRM), on foot, or by public transport with its legs (Transitous: dashed on foot, solid on the vehicle). The three buttons at the
bottom left switch the way and the distance and time follow; the cross next to the picked place forgets it. A click on the
distance chip opens the same trip in OpenStreetMap's directions. What leaves the machine: the words searched and, for the route,
the two points.

## Following your position (Linux)

By default the map, the trips to events and the weather use where the machine is: a GeoClue fix is asked for when the island starts
and every ten minutes (and when the map page opens), and the weather is read again for the new place once you have moved a few
kilometres. Until the first fix, and when GeoClue gives none (or only a guess wider than 50 km), the location set in the settings
is used. To pin the coordinates instead, as before, switch off **Options > Places > "Follow my position"**
(`follow_position`): the map, the trips and the weather then all use Settings > Weather location.

## What leaves the machine

Nothing is sent unless a feature that needs it is in use, and each can be switched off:

| feature | goes to | what | off with |
|---|---|---|---|
| weather | Open-Meteo | the coordinates | `weather` |
| calendars, lyrics, cover art | the feed you configured, LRCLIB, the player's art URL | the feed address; a track and artist; an image URL | remove the feed, `lyrics` |
| trips, search and routes on the map | Nominatim, OSRM (demo server), Transitous | the event's place or the words you searched, and the two points | `travel_times` (the map search is yours to use or not) |
| map tiles | the tile provider (OpenStreetMap by default) | the tile addresses, which say roughly where you look; your API key, if the provider wants one | do not open the map |
| position | GeoClue, which asks its own sources (Wi-Fi, GeoIP) | what GeoClue sends; the island itself calls no location service unless `wifi_position` is on | `follow_position` |
| precise position (Linux, **off by default**) | [beacondb.net](https://beacondb.net), an open Wi-Fi database | the hardware addresses (BSSID) and signal strength of up to 15 Wi-Fi networks around you, never your IP address for positioning; networks named `..._nomap` are left out | leave `wifi_position` off (Options > Places) |
| currency rates | the European Central Bank, `ecb.europa.eu` (one XML file, at most every 12 h, kept on disk) | your IP address, as for any web page | switch `currency_rates` off (Options > Places) |
| the usual train (only when you pin one) | ViaggiaTreno, `viaggiatreno.it` (Trenitalia), and for the station near home Transitous | the station, the train number and the time; your IP address; an address you typed for home is sent once to the geocoder at `api.transitous.org` (coordinates are not sent anywhere but to find the near station) | do not pin a train; give home as coordinates |
| sending files | the device you pick, on your network | the files | do not send |
| network check | NetworkManager's own connectivity probe, a ping to your router | nothing of yours | `nmcli` absent |

The clipboard history is in memory (only pinned entries are written, mode 0600), one-time codes are never kept, and the activity
page keeps only today's totals per app in the state directory. Nothing is sent to an account or an analytics service.

### Position within metres (Linux)

GeoClue's Wi-Fi source no longer works (Mozilla's location service is closed), so without a GPS the position is a GeoIP guess
of about 25 km. With `wifi_position` on, the island looks up the Wi-Fi networks around you at the open
[BeaconDB](https://beacondb.net) instead. It only knows networks that people have contributed, so where nobody has, the answer
is "not found" and the GeoIP guess stays. You can contribute scans yourself with an Android app such as NeoStumbler (upload
to BeaconDB in its settings). Windows uses the system's own location service.
