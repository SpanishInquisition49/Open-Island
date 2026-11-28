# open-island: the guide

Every feature, option and command in detail. For a quick tour, see the [README](../README.md).

A macOS-style **dynamic island** for any desktop, in Rust. A black pill at the top of the screen that
morphs between the clock, music (cover, progress, synced lyrics), volume and brightness, timers (countdown,
Pomodoro, stopwatch), long-running operations (a progress bar your scripts can drive) and privacy indicators, while
what happens (notifications with actions, charging, rain and meeting heads-ups, a job that ended) drops under it as cards. Around that it grew pages of its own: a map
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
| slide away from a window over its place | niri | every window (built) | **not yet** | **not yet** |
| map and position, trips to events, place search and routes | GeoClue + OpenStreetMap, OSRM, Transitous | the same services, the position from Windows' location service (built; the settings location when it is off) | the same, **no position** | the same as Linux |
| sound page, heavy-program card, network card | pactl, /proc, nmcli | Core Audio sessions, sysinfo (pause with NtSuspendProcess), a ping of the router and Windows' own connectivity probe (built) | heavy-program card only (built) | yes |
| audio follows headphones | pactl | **not yet** | **not yet** | yes |
| who uses the microphone and the camera (with Mute / Quit) | pactl, /dev/video | the names from the privacy registry, Quit only (built) | **not yet** | yes |
| activity page, auto focus, away recap | niri focus events, `ext-idle-notify` | the window in front (built); no away recap | **not yet** | **not yet** |
| LocalSend (send files), one-time codes, clipboard actions | yes | sending yes | sending yes | sending yes |
| LocalSend (receive files and messages) | yes | **not yet** | **not yet** | yes |
| keyboard: Caps Lock and layout notices | sysfs; the layout from niri only | **not yet** | **not yet** | Caps Lock only |
| shelf: drop files on the pill and drag them out | yes (Wayland) | the list only, filled by `open-island shelf` (built) | the same (built) | yes |

"built" means it type-checks for `x86_64-pc-windows-msvc` / `aarch64-apple-darwin` in `cargo check`; none of it has been
run on those systems. The update-check card of the Luau original is not ported (it was specific to Noctalia). Complex
scripts (Arabic, Hindi, ...) are not shaped: text is laid out glyph by glyph.

## Install (Linux)

```sh
cargo install --path crates/app
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
    {"type": "icloud", "user": "me@icloud.com"},
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
old `calendar_urls` list still works and is migrated).

The calendars are always read from two months back to two months ahead, again every five minutes. Turn the Calendar page to a
month outside that and it is read then, with the months beside it ("Reading this month…"); it is kept while the island runs and
read again after ten minutes or on refresh. A feed only gives what its publisher still lists: an old month can be empty because
the events are gone at the source.

Under the month, the events of the chosen day are listed four at a time. On a busier day the wheel moves through them, and the
button under the list ("1–4 of 7 · more") goes a page further and back to the top.

**Passwords and API keys** (CalDAV accounts, the map tiles) are kept in the system's keyring, not in `config.json`: the
Secret Service on Linux (GNOME Keyring, KWallet, KeePassXC), the Credential Manager on Windows, the Keychain on macOS. One
written into the file by hand (`"password"`, `"map_api_key"`) is moved to the keyring while the island runs, and leaves the
file only once the keyring holds it. Without a keyring, adding an account from the menu is refused; `password_cmd` (a command
that prints the password, from a password manager) works everywhere. This protects the file, its copies and backups: other
programs of the same user can still ask an unlocked keyring. A secret feed address is a secret too, and is still written in
the file (which is owner-only). `open-island doctor` says whether the keyring answers.

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

**What is remembered.** The options are in this file and apply from the start of the next run. "Do not disturb" and "Keep awake" are written to the state folder as
soon as they are switched, so they come back after a restart or a crash. Whatever keeps the machine awake (`systemd-inhibit`, `caffeinate`) ends with the island, so
nothing is left holding it while the island is not running.

## The player

Shuffle and repeat sit beside the skip buttons, for the players that offer them (MPRIS `Shuffle` and `LoopStatus`; on
Windows what the system's media controls allow). A dot marks what is on; repeat goes from off to the whole list to the
current track (the icon with a 1) and back. Not on macOS yet.

## Out of the way of windows

While a window is over the island's place (the middle of the top edge of its screen) the island slides up out of sight.
Move the pointer to the very top of the screen there and it comes back, and it stays while the pointer does. It also comes
out by itself for what it has to show: a notification or any other card, the volume bar, a password request, the command
bar and the pages you ask for; then it leaves again. A timer or a song in the small pill goes away with it. Out of sight
it draws nothing and does not listen to the sound for the equalizer. `hide_when_covered` (Options > Island > "Hide under
windows", on) switches it off.

- **Windows:** any window in front of the place counts, except the taskbar, the desktop and click-through overlays. The
  primary monitor only.
- **niri:** a floating window counts where it is. niri does not say where a tiled one is, so this is worked out: tiled
  windows count when they reach the top (no bar keeps them below the island; a bar reserving less than 32 px counts as
  none) and the columns on show are wide enough to reach the middle. A bar at the bottom with none at the top is
  mistaken for one at the top. The overview shows the island.
- Other compositors, X11 and macOS do not say where windows are: the island stays.

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
ring becomes snooze; a click on the card opens the sender, a right click anywhere on it takes it away. A card without a picture or an app icon shows its kind's mark
(information, success, warning, failure; a critical one is a red warning, and the `category` hint `*.complete` or `*.error` makes it a success or a failure). Up to three cards stack, each new one dripping from the last: the pointer on any of them holds them all,
a card that goes is drawn back into the one above and those below move up, and a small drop under the stack counts the ones waiting for a place. The motion and the look are
ported from [drip](https://github.com/jrnxf/drip) (see `THIRD-PARTY-NOTICES.md`).

## Cards from scripts

The command line follows [drip](https://drip.jrnxf.co/)'s:

```sh
open-island ok "Build finished" -d "246 passed in 3.2s"       # also: fail, warn, info, notify
open-island fail "Deploy failed" --stay                        # until clicked, ten minutes at most
open-island info "Deploying" --id deploy; sleep 2
open-island ok "Deployed" --id deploy                          # takes the first card's place
open-island ok "Report ready" --open report.html --button "View report"
open-island warn "Merge #23?" --yes Merge --no Later && gh pr merge 23
open-island run make test                                      # a card when it ends, with the time it took
make log | tail -1 | open-island info                          # the first line piped in is the title
open-island dismiss [--id deploy]
```

A question waits for its button: exit status 0 for yes, 1 for no, 5 when nobody answers (it went, was dismissed, or was replaced), 3 when no island is
running, so only a clear yes goes on after `&&`. A question stays until answered unless `--seconds` (0.5 to 60) gives it a countdown; one is shown even in do not
disturb, other cards from scripts are held back then. A card that stays shows a white dot instead of the ring, and gives way when others are waiting for a place.
`--icon <name>` (the island's icons) and `--color <name|#rrggbb>` change the look; `--sound <file>` plays a sound. Looks you use often can be named:

```json
"card_presets": { "deploy": { "kind": "ok", "icon": "bolt", "color": "purple", "seconds": 8, "sound": "~/Music/t.ogg" } }
```

then `open-island notify --preset deploy "Shipped"`.

**Coding agents.** With the hooks of `open-island agent setup claude` in `~/.claude/settings.json`, a session that needs you gets a card named after its folder;
one asking for a permission or an answer stays until the agent goes on (`PostToolUse` is what clears it as soon as an approved tool has run). `open-island agent --done`
in the `Stop` hook also says when a turn ends. The card only tells: permissions are answered in the terminal.

## The battery

Plugging in or unplugging drops a card with the battery as liquid: it pours up to its level from empty on power and falls to
it from full off it. A couple of seconds later the line under it says "Full in 1h10" or "About 3h20 left", from the power the
battery reports at that moment (an estimate, in steps of five minutes). The low battery card says the same. Options > Alerts >
"Battery alerts" switches the cards off.

## A second screen (Linux, niri)

When a screen is plugged in, a card offers **Extend**, **External** (the laptop's panel off) and **Laptop** (the other one
off); `monitor` in the command bar brings it back. niri cannot mirror one screen on another, so there is no such choice. A
choice that switches a screen off asks to be kept: without **Keep** within 15 seconds everything comes back on. A screen goes
off only after niri says another is on, and whenever every connected screen is off (the projector pulled out while the
laptop's panel was off) the panel is switched back on, also when the island starts. If the island is not running at that
moment, `niri msg output eDP-1 on` does it. The changes are temporary: niri forgets them when its configuration changes.
The island moves to the screen that is left when its own goes away.

## Administrator passwords (Linux, off by default)

With `"polkit_agent": true` in `config.json` (then restart the island) the island is the session's polkit agent: when a
program needs an administrator's password, a drop falls from the pill to the middle of the screen and opens into the request,
over fullscreen windows too; the pill shows a lock meanwhile. It shows the action
as the system names it, the system's message, the program that asked, and whose password is wanted, with their picture (the
one the login screens show, in `/var/lib/AccountsService/icons`; for you also `~/.face`); Enter confirms, Esc
cancels, three refused attempts end the request. Only one agent can serve a session: switch the other off first (Noctalia:
`polkit_agent = false`). `open-island doctor` has a section on it. Needs polkit 126 or later (the helper socket) and the
Wayland layer-shell backend.

How the password is treated:

- the island never checks it and never stores it. It goes to polkit's own helper, a root service, after checking that the
  other end of the socket really is root, and only when you confirm: a request dismissed before anything was sent reaches
  nobody (and so counts no failed attempt against the account). It is given only to a prompt that asks for a password;
  anything else (a PIN) is shown and asked for;
- it is held in one buffer of fixed size, wiped after use, kept out of the swap where the system allows, never printed or
  logged, drawn only as dots; no paste, no key repeat, and keys are ignored for half a second after the request appears;
- keys and clicks sent by programs (`open-island msg key ...`) are dropped while it is up, no card shows beside it, and a
  request is accepted only from polkit itself;
- while the agent is on, other programs of your user cannot trace the island or read its memory.

### Face recognition (Gaze)

If polkit's PAM stack runs [Gaze](https://gaze.gundulabs.com), a request can be answered with your face. On Arch, create
`/etc/pam.d/polkit-1` (this takes the place of the package's file in `/usr/lib/pam.d`):

```
#%PAM-1.0
auth       requisite    pam_faillock.so preauth
auth       sufficient   pam_gaze.so
auth       include      system-auth
account    include      system-auth
password   include      system-auth
session    include      system-auth
```

Without options the camera looks first (a second or two) and a password typed meanwhile goes through when it is done; Gaze
says why a face was not accepted, and the request shows it. With `simultaneous` the password goes straight through, but Gaze
then says nothing when it does not recognise a face: the request notices only after six seconds. Keep a root shell open
while you change PAM files, and check `gaze doctor` and `open-island doctor`.

- **When it looks:** Settings > Passwords > "Face unlock" (shown while `polkit_agent` is on), or `polkit_face` in
  `config.json`. **When asked** (`"ask"`, the default): only on Enter on the empty field or **Use face**. **Automatic**
  (`"auto"`): as soon as the request opens, kept only when Gaze has `require_confirmation_elevation = true`, else it asks.
  **Off** (`"off"`): the island does not offer it; the PAM file still decides whether Gaze runs when a password is sent, so
  to keep the camera off altogether take the Gaze line out of it. A change applies from the next request. The password can
  be typed at any moment. `open-island msg set` cannot change this.
- **Confirming:** with that Gaze setting on (its default), a recognised face does nothing until you press Enter or
  **Confirm**, at the earliest 0.7 s after it appears. With it off, a face you asked for authorises at once.
- The island never sees the camera or a picture: Gaze runs inside polkit's helper, as root. The island only shows where it is
  and passes on your yes (a fixed word). It tells Gaze that it knows its message codes, which lasts while it runs.
- A face is easier to fake or to use without you than a password: Gaze has its own liveness check, and the choice to accept
  it for administrator actions is made in the PAM file, which is yours.
- One face check per request: another can be asked for only after a refused attempt, because the one under way cannot be
  restarted without ending the conversation, and ending it counts.
- While the account is locked nothing runs, the camera included: the island says "Account temporarily locked" and closes
  the request.
- A recognised face does not reset the count of failed attempts (a typed password that is accepted does, through
  `pam_faillock`'s last step; a `sufficient` module that succeeds stops before it). `faillock --user you` shows the count.
- Dismissing a request after the camera or the system has started counts as a failed attempt against the account
  (`faillock`; three in a quarter of an hour lock it for ten minutes by default; `faillock --reset --user you` as root
  clears it). Dismissing before you asked for anything counts nothing.

What no agent can do: protect you from a program that already runs as your user and imitates the request, takes polkit's
place before the island does, or rewrites the island's program. For the last one, install the binary where only root can
change it: `sudo install -m755 target/release/open-island /usr/local/bin/` and start that one. The graphics toolkit keeps
short-lived copies of the characters typed that the island cannot wipe. A real request of yours shows the action's name under
the title; if a "password request" turns up as an ordinary notification card, it is not one.

## Liquid: events, small pills and the filling drop

The pill shows how things are; **what happens drops from it as a card**, stacked with the notifications:

- a countdown or a Pomodoro phase that ran out ("+1 min" gives a countdown another minute), a meeting about to start ("Join"), rain on the way,
  a Bluetooth device connecting or running low, plugging, unplugging and a low battery (with the charge);
- the end of a long job (`progress`, an activity, a long shell command, a download with "Open" and "Show in folder", a file sent);
- what the command bar worked out (`12*7`, a conversion): already on the clipboard, a click copies it again;
- a copied link, e-mail or address, with a button that opens it (`link_cards`, needs the clipboard history; not while quiet).

The **small pills** beside the pill leave it on a liquid neck and go back into it. While a job with a percentage or a countdown runs, a **drop hangs
at the left of the pill**: it fills up with the job (blue or the job's colour) and drains with a countdown (orange: full at the start). A click opens
its page; it gives way to open pages.
Switch it off with `progress_drop` (Options > Island > "Progress drop"). Volume and brightness still change the pill itself.

## Little animations

Each plays once, for the event that starts it, and asks for no frames afterwards.

- **The drop becomes the card.** When a countdown or a job with a percentage ends, the drop that hung at the left of the pill lets go, slides under it and opens into
  the card that says how it went, carrying the colour of the end until it does (a failed one trembles as it opens). Behind another card the new one drops from it as
  usual.
- **Files.** A file dragged over the island (Wayland) is reached for: the pill says "Drop on the shelf" and stretches a neck with a drop towards the pointer; let go and it
  is drawn in, the pill swells, and the shelf opens with the file on it. When files have been sent, a small drop leaves the right end of the pill.
- **One-time codes** roll in a digit at a time; a click on the card sends them into the pill, which says "Copied".
- **Back from do not disturb** (or from any of the automatic quiet states), what was held comes out of the pill drop by drop, each with its app's icon or initial (six
  at most), and the drops gather into the card that sums them up. Coming back to the machine is told by that card alone.
- **A device that connects** shows its battery as a ring that fills round its icon while the number counts up (green, orange at 40%, red at 20%); a battery that
  arrives a few seconds after the connection brings the card up to date.

`cargo run -p island-ui --example preview -- out/ fx` renders moments of each to PNG.

## Conversations, not piles of cards

Notifications are grouped by conversation (the same app and the same title: a chat or a sender, a site).

- **On screen**: a new message of the conversation whose card is showing joins that card instead of taking a card of its own: the card shows the latest words and
  "Telegram · 3 new", its timer starts again, and the earlier messages are closed towards their sender (so five messages in a lecture are one card, not five in a row).
  Messages of other conversations of the same app have cards of their own, and a critical one is never folded into another.
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

The socket takes messages from your own user (and root) only; on Linux and macOS each side checks who the other is.

`open-island msg <event> [payload]`: `peek`, `open [page]` (keyboard navigation: arrows move a focus ring, Enter
activates, Tab / Shift+Tab change page, Escape closes), `page next|prev`, `page <home|today|map|cal|qs|sys|activity|audio|send|shelf|notifs|media|timer|clip|focus|settings>`,
`dnd on|off|1h|until 18:00|until class ends`, `mic [toggle|off|on]`, `shelf <file>...`, `timer 25m`,
`pomodoro`, `stopwatch`, `timer-cancel`, `reply` (reply to the notification on screen: bind it to a key), `send <file>...` (see LocalSend below), and for long operations `open-island progress run [-l label] -- command...` (runs the command on a pseudo-terminal, so rsync, curl, pacman... still draw
their own progress, and shows the last `NN%` they print; not for commands that ask questions: `sudo -v` first), or by hand `progress <percent|?> <label>`,
`progress-done [label]`, `progress-fail [label]`, `progress-clear`: a `64%` small pill beside the pill while it runs,
and a card under it when it ends (red when it failed).

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

A screenshot's card (the compositor's "Screenshot captured") becomes one with buttons: **Text** copies the text in it, as below, and
when the picture's file is found (the one the notification names, or the newest picture written in the last seconds to niri's
screenshot folder or to Pictures) also **Send** (to a nearby device), **Folder** and **Trash** (it goes to the bin, from where it can be
taken back). The text button is switched off with `ocr_screenshots` (Options > Alerts > "Text from screenshots"). Linux.

`ocr` in the command bar (or `open-island msg ocr`) reads the text of the image on the clipboard (a screenshot of a slide, a
scanned page) with Tesseract and puts the text on the clipboard; a card shows how it starts. It asks for Italian and English
when both are installed (`tesseract-data-ita`), joins words cut by a hyphen at the end of a line and drops the empty lines in a
row. Offline: the image goes to the local `tesseract` and is written, for that moment only, to a private file in the runtime
folder. Linux (wl-clipboard + tesseract); other systems say it is not there yet. Not done: reading by itself after every
screenshot.

### Downloads

A browser download shows as an activity with what has arrived and how fast ("48 MB · 2.1 MB/s"); when the file is complete a card
says its size and the time, with "Open" and "Show in folder". The drop beside the pill sloshes while data arrives (the
browser does not say how big the file is, so there is nothing to fill it up to), goes back when the download stalls, and
becomes the card at the end. A download that is over before the folder is looked at (every four seconds when nothing is
going on) is not seen. No extension: the island looks at the download folder (the XDG one, else `~/Downloads`) for
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

A command that runs longer than 10 seconds becomes a progress pill with its name; when it ends a green card says how
long it took, or a red one its exit code. Editors, pagers, `ssh`, `top`, REPLs and similar are skipped. Set
`OPEN_ISLAND_HOOK_SECONDS=30` to change the threshold and `OPEN_ISLAND_HOOK_IGNORE="make,go"` to skip more programs.
A shell that is closed mid-command takes its pill with it.

## Coding agents

An agent (or any script) can use the island as its dashboard without a plugin: its turns show as activities, it can start
timers, and it can call you. Several sessions run side by side, each under its own id; the pill shows one, the Today page
lists all of them with their status line, and an activity that ends (or fails) leaves with a card.

```sh
open-island agent setup claude        # prints the hooks block for ~/.claude/settings.json (merge it in yourself)
```

With those hooks Claude Code needs nothing else: a prompt starts a turn ("Claude · repo: thinking"), each tool call updates
the status line ("Edit lib.rs", "Bash cargo test"), `Stop` ends the turn, a `Notification` (a permission question, a wait for
input) becomes a card (and only a card: the pill does not open for it too), `SessionEnd` forgets the session. The hook always exits 0 and never waits for the island, so it cannot
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
| `find compilers`, `cerca reti` | a search through events (those to come, and the last three that are over, from the two months back), tasks, deadlines, pins, notes and study files (the Find page) |
| `log 90m algoritmi`, `studiato 1h30 reti`, `pomodoro algoritmi` | study time done; it comes off the work still to do for that course. A Pomodoro with a course is a study session: quiet during the focus, logged by itself |
| `esame 3 feb Algoritmi`, `deadline 3/2 project` | a date to count down to (see "For a commuting student") |
| `until class` | a timer that ends five minutes before the next event |
| `pomodoro`, `stopwatch`, `focus 45`, `dnd on` / `dnd off` | the matching mode |
| `dnd 1h`, `dnd for 30m`, `dnd until 18:00`, `dnd until class ends`, `dnd fino alle 18`, `dnd fino a fine lezione` | do not disturb that ends by itself; the quick settings chip says when, and it is kept across restarts |
| `mic`, `mic off`, `mic on` (also `microfono …`) | mutes or unmutes the microphone |
| `remind me when i get home water the plants`, `remind me when i leave uni call Sam`, `ricordami quando arrivo a casa …`, `ricordami quando esco da uni …` | a reminder tied to a place: see "Reminders tied to a place" |
| `place uni`, `unplace uni` | saves where you are now under a name, or forgets it |
| `countdown 45`, `countdown off` (also `conto …`) | how many days ahead the pill counts down to an exam or deadline |
| `12*7+3`, `sqrt(2)`, `2^10`, `(1+2)*3` | a sum; Enter copies the result |
| `20% of 150`, `80 + 15%`, `80 - 20%`, `split 84.5 by 3` | a share, a price with a rate, what each person of a bill pays; Enter copies it |
| `emoji fire`, `emoji cuore`, `symbol implies`, `simbolo freccia` | up to ten emoji or symbols side by side: Enter copies the lit one, Tab and Shift+Tab move, a click copies that one. Emoji are found by their English names and shortcodes, the common ones in Italian too; flags and other emoji made of several characters are not offered (the island could not draw them whole) |
| `monitor` | the card of the other screen again (see "A second screen") |
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
  days before it (in steps of five minutes; all that is left on the day itself). Each day of the week has its **study hours**
  (9:00 to 17:00 to begin with; the **Hours** button on the Plan page, where a day can also be off or have a lunch pause that
  cuts it in two, or `study_hours` in the configuration): the plan gives a day what its timed events leave of that time, and
  events before or after it, or during the pause, count for nothing, here and in the week shown on the page. So a day full of lectures gets little and an empty one much; what the nearest
  deadline takes is not free for the next, and the same work is never
  planned on two days. The study plan counts on two months of calendar ahead: past that a day counts like the same weekday of the last week known.
  The plan is always one that can be followed: no day is given more than it has free, and today only what is left of its
  study hours once the events still to come and what you already studied are taken out. When the free time before a date is not enough, the rest is
  not planned and the page says how much of it "won't fit". The morning and evening cards end with "Study 1h20: Algoritmi exam 45m, Report 35m".
  `log 90m algoritmi` (or a study session, below) counts for the task or exam named exactly so, else for the nearest-due one of that
  course, and comes off today's part first, so having studied does not ask for more the same day. An exam's hours are the total: what is studied for it
  is counted next to them ("4h of 20h"), also when it was logged before the hours, or the exam, were written down.
- **Study sessions**: the ▶ on a row of today's study (Plan page), or `pomodoro algoritmi`, starts a Pomodoro for it. While a
  focus runs, notifications are held (the break lets them out, with the card of what was held back); a lecture about to start, the end of an
  event and the nudge about distracting apps still come, and `dnd off` lifts the quiet for that focus. Its time is logged by itself:
  all of it at the end, what ran of it when it is skipped, stopped or replaced by another timer (under five minutes: nothing).
  The running row shows the clock, and its cross stops it. A Pomodoro started without a name is only a Pomodoro: nothing is held.
  Quitting the island in the middle of a focus loses that focus.
- **Pace**: under each exam or hand-in with hours, where the last days lead: "10h20 short: 31% by then · 26m a day, 1h25 needed", or
  "On track". It is the time studied for it a study day (since you first studied for it, over at least 7 and at most 14 days; days off
  count only if you studied), carried over the study days left before the date. It does not look at how full those days are: that is
  what "won't fit" says. One with credits and no hours has no pace, as there is no total to measure against.
- **Credits (CFU)**: `exam 15 dec AE 9cfu` (or `9 cfu`) gives an exam a weight instead of a number of hours. Whatever each day leaves
  free, after the events and the work with hours, is suggested to the exams that have credits and no hours, each in proportion to
  its credits over the days it has left: a bigger exam gets more, and so does a nearer one. These are indications, in quarters of an
  hour, marked `≈`; they never make a day "heavy", and what was studied on earlier days does not change the proportions. What you
  study today comes off today's part. Hours said for an exam (`~40h`) take over from its credits.
- **The Plan page** (`go plan`, `go week`; it is on the page grid, and in the dock once you open it), from the top: what to study
  today, item by item, with the day each is due and the time already studied; the next seven days, each with a bar (each calendar's events in its
  colour, the study in grey, a legend naming them), the hours of both together (a moment two events share counts once, here and in
  the study plan), a flame with the number of things due, and in orange when the day is heavy (six hours of
  events, three things due, or nine hours with the planned study); the deadlines (the next five) with their date, the days to go and the
  work left; how the study of the next seven days splits among the exams; and the time studied this week, per course. **+ Exam** opens the bar to add one; a click on a deadline opens its line with
  the cursor after the date, to move it or change its hours; the cross removes it.
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
  away a day after the date. An exam has one day: naming it again (`esame 10 feb Algoritmi`) moves it, keeping its course and hours.
- **Every-day to-dos**: `todo daily bring the charger` (also `todo ogni giorno ...`) is ticked off for the day and is back tomorrow.

## The morning and the evening card


If the pinned train is late or cancelled and has not left yet (the board read in the last 20 minutes), the evening card opens with "18:50 train +12 min" before anything else; on time is not news.
Set a time (Options > Day > "Evening", or `"evening_recap_time": "18:30"` in the configuration, or
`open-island msg set evening_recap_time 18:30`) and at that time, once a day, a card shows tomorrow: how many events,
reminders and to-dos under the heading, then one line each for all of them with their times, the deadlines of the week and the rest. The
card grows to fit: nothing on it is cut or summed up as "+2 more". It stays
45 seconds (longer with the pointer moving on it), a click opens the Today page. It comes on weekdays only unless "Recap on weekends" is on, it is skipped when
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

The focus of a study session (see the organizer) holds them too.

The DND button is lit whenever notifications are held, by you or by the island (the three above, the study focus, auto
focus, a study session), and a press on a lit button always puts it out, as `dnd off` does: the reasons there are at that moment are set
aside until each of them ends, so the next meeting or call holds them again.

Do not disturb can also end by itself: `dnd 1h`, `dnd until 18:00`, or `dnd until class ends` (the event that is on, or the next one
today).

The bell with a z on a notification card (under the pointer) hides it and brings it back after ten minutes. When the island stops being
quiet (a meeting ends, a call, `dnd off`, the study focus) one card says how much was held back and from whom; a click
opens the list. The focus mode of `~/.local/bin/focus` counts as quiet too.

## The microphone

While the microphone is in use its icon shows in the pill; muted, a red crossed microphone stays there. A click on either, `mic` in the
bar, or `open-island msg mic toggle` mutes and unmutes the default input, and a short card says which. To have it on a key, in niri:

```kdl
Mod+M { spawn "open-island" "msg" "mic" "toggle"; }
```

Linux (pactl) and Windows.

## The keyboard

A change of Caps Lock, or of the keyboard layout, shows for a moment where the volume bar does ("Caps Lock on", "English (US)").
Caps Lock is read from the keyboards' lights (`/sys/class/leds`, four times a second); the layout comes from niri's event
stream, so other compositors show Caps Lock only. Switch both off with `keyboard_alerts` (Options > Alerts > "Keyboard
changes"). Linux.

## Status of anything a script watches

```sh
open-island activity run  "CI main" --detail "building 3/5" --icon git-branch --color orange
open-island activity run  "CI main" --progress 60 --detail "tests"       # a bar instead of the status line
open-island activity done "CI main" --detail "all green"                  # green card, and the small pill goes
open-island activity fail "CI main" --detail "2 tests failed"             # red card
open-island activity clear
```

Like `progress`, an activity is a small pill beside the pill with its icon and status. Colours: red orange yellow green teal blue purple pink white gray or `#rrggbb`;
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

### The weather in the pill

The small pill shows the weather where you are behind what it says:

- **clear:** by day a sun at the left of the clock, whose rays turn as a gleam crosses the pill; by night the moon in
  tonight's phase (worked out from the date, mirrored south of the equator) among stars that twinkle, when one falls;
- **partly cloudy:** the sun or the moon with a cloud before it;
- **overcast:** clouds across the pill;
- **fog:** soft banks, each drifting the other way from the next;
- **falling:** drizzle, rain with the droplets it throws up as it lands, snowflakes that sway as they fall, and in a
  thunderstorm a soft flash now and then; how heavy follows the forecast's code.

What falls leaves its mark. While it snows, a layer of snow piles up at the bottom of the pill, in some 24 seconds, and
melts in 20 once it stops; while it rains, drops stay on the glass, run down now and then, and slide off in a few seconds
after the rain. On battery, between two falls, snow and drops stay where they are, still. They are shown in the small
pill only, and `weather_in_pill` switches them off with the rest.

When the pill has more in it (a song, a timer, an icon, a countdown) the disc of the sun or the moon gives way, and only
its light stays. Day and night are as the forecast says, read every ten minutes. All but what falls stands still and
moves for five seconds every half a minute (every two minutes on battery); a still sky costs nothing. What falls moves
all the time on power, at 30 frames a second, and on battery for six seconds as it begins and then every two minutes.
Rain costs some 4 to 5 % of one processor core while it moves (measured in a nested niri on a Ryzen 6600HS), against
well under 1 % at rest. It all stops when a page is open and when the island is out of sight. `weather_in_pill` (Options
> Places > "Weather in the pill", on) switches it off. To see how one looks, whatever the weather is, `open-island msg
sky <name>` shows it for half a minute (`sun`, `night`, `partly`, `cloudy`, `fog`, `drizzle`, `rain`, `heavy`, `snow`,
`storm`); `msg sky off` ends it.

## Building with `just`

`just build` asks for the target (Linux, Windows MSVC, Windows GNU, macOS check) and leaves the result in `dist/<target>/`;
`just build windows` skips the menu, `just windows-zip` makes `dist/open-island-windows-x64.zip` to send to someone.
Cross-building for Windows from Linux needs `cargo install cargo-xwin` (MSVC, recommended) or mingw-w64 (GNU).
`just check-all` runs the tests, clippy and the Windows (cargo-xwin) and macOS type-checks.

On Windows the island slides away from windows over its place (see "Out of the way of windows"), and its window is hidden
once it is gone.
Windows draws through DirectX 12 into a transparent DirectComposition surface; `OPEN_ISLAND_OPAQUE=1` falls back to
cutting the window to the island's shape. The window has one size (large enough for every page) and is not resized while
the island animates: resizing it and its swap chain was what made it stutter and flash a rectangle. Only the box the
island is in is rendered and copied into that window, and the window keeps no picture of its own
(`WS_EX_NOREDIRECTIONBITMAP`), so nothing of one can show around the island. Clicking a
notification brings the window of its app to the front.

## Smoothness

`OPEN_ISLAND_PERF=1 open-island` logs one line per animation: frames, the interval between them (8.3 ms is a steady
120 Hz), the slowest one, how many took over 20 ms, and what drawing cost. The island asks for the low-power GPU
(`WGPU_POWER_PREF=low`, change it in the environment); on a laptop with a discrete GPU, installing the integrated GPU's
Vulkan driver (`vulkan-radeon`, `vulkan-intel`) lets it run there instead of keeping the big GPU awake. On such a laptop
NVIDIA's driver is then not even loaded (it takes some 80 MB and holds the card open); only Vulkan is asked, unless it has
no hardware to offer. `WGPU_BACKEND`, `WGPU_ADAPTER_NAME`, `WGPU_POWER_PREF` and the Vulkan loader's driver variables are
respected. Font files are mapped into memory, not copied, and a line sliding because it is too long is drawn 60 times a
second at most, and not at all while it pauses. On Wayland and Windows a frame equal to the one on screen is not rendered
again (it is repainted every 30 seconds at the latest), and news from the services that arrive within 15 ms of each other
are drawn together. Covers and map tiles are sent to the graphics card once.

## Options

The settings are written to `config.json` whole-file-atomically and one change at a time (a temporary file renamed over it, under a lock), so two changes made together - the home that is looked up while you toggle an option - cannot undo each other, and an option you switched on (the precise position from Wi-Fi, say) stays on across restarts and updates: nothing but you ever writes it, and only an absent key takes its default.

The same settings as the Noctalia island plugin plus this island's own, on the Options page (the toggle icon in the dock), grouped by what they are
for: **Island** (how it looks and behaves), **Alerts** (what it shows you and for how long), **Quiet** (when it keeps its notifications to
itself), **Day** (cards at set times, the way to lectures), **Places** (where you are, what it asks the network), **Music**, **Rules**
(notification rules). Resting the pointer on an option says what it does; an empty tab says what it is for. The same keys work in `config.json`, or from
a script with `open-island msg set <option> <value>`:

| tab | option | default |
|---|---|---|
| Island | `clock_in_pill`, `status_icons`, `hide_on_fullscreen`, `clipboard_history`, `timer_sound`, `auto_audio_output`, `progress_drop` | on |
| Island | `mini_pills` (1-4, 2): how many small pills may sit beside the main one; a timer, focus, an event and each running activity (an agent's turn, a build) get their own, in that order; 1 is a single pill beside it | 2 |
| Island | `dock_icons` (4-12, 7): how many page icons the bottom bar shows (Home, what is live, the pages used most, the page you are on); the rest are on the All pages grid | 7 |
| Island | `timer_presets` (["5m","10m","25m"]), `screen` (`focused` or a name such as `eDP-1`; pinning by connector name, on Wayland too; an unknown name is logged and the compositor chooses), `timer_sound_file` (the sound file the timer plays) | |
| Alerts | `show_notifications`, `show_osd`, `battery_alerts`, `meeting_alerts`, `rain_alerts`, `culprit_alerts`, `ocr_screenshots`, `keyboard_alerts` | on |
| Alerts | `notification_seconds` (2-30, 5), `osd_seconds` (0.5-5, 1.6), `low_battery_percent` (5-50, 15), `meeting_lead_minutes` (1-60, 10: the card), `meeting_pill_minutes` (1-120, 10: the pill on the side) | |
| Quiet | `quiet_in_meetings`, `quiet_on_mic`, `quiet_on_screen_share`, `away_recap` | on |
| Day | `link_cards` (a copied link offers to open it) | on |
| Quiet | `deep_work_minutes` (0-120, 20: auto focus), `break_minutes` (0-240, 0: off), `deep_work_apps` (a list in `config.json`) | |
| Quiet | `distraction_minutes` (0-120, 20; 0: off), `distraction_apps` (the apps marked on the Activity page) | 20, none |
| Day | `travel_times`, `end_alerts` (five minutes before an event of half an hour to five hours ends: what comes next, and where) | on |
| Day | `countdown_days` (0-365, 21: the clock pill counts down to the next exam or deadline within that many days, orange in the last three; 0: off. Also `countdown 45` / `countdown off` in the bar) | 21 |
| Day | `evening_weekends` (both cards on Saturday and Sunday) | off |
| Day | `morning_recap_time`, `evening_recap_time` (`"08:00"`, `"18:30"`, or minutes; `off`), `commute_minutes` (0 = off), `travel_margin_minutes` (0-30, 5), `study_hours` (seven days, Monday first: `"09:00-17:00"`, `"09:00-13:00+14:00-18:00"`, `"off 09:00-17:00"`) | off, off, 0, 5, 9-17 |
| Places | `weather`, `weather_in_pill`, `follow_position` | on |
| Places | `currency_rates` (the ECB's daily rates for the bar) | on |
| Places | `receive_files` (Linux: take files other devices send with LocalSend; `receive_dir` in `config.json` says where they go) | **off** |
| Places | `wifi_position` (Linux: a position within metres from the Wi-Fi networks around you, looked up at beacondb.net; a card says so when you turn it on) | **off** |
| Places | `map_tile_url` (a preset name or a URL; see the map), `map_dark`, `map_api_key` (Settings page; kept in the keyring), `travel_mode` (`car`, `walk`, `transit`, `train`: on the Today page and the map) | OpenStreetMap, on, none, car |
| Music (a title or artist too long for its place goes round to the left, followed by itself and pausing each time its start is back, instead of being cut) | `media_auto_expand`, `lyrics`, `lyrics_in_pill` (off; the compact pill widens, up to 640 px, to show the line whole), `art_tint` | on |

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

## Taking back a removal

What a cross, a menu or a line removes can be brought back for a few seconds with **Undo** on the card that says it went: a
task (on the CalDAV account too, with `tasks_sync`), an exam or hand-in with the time studied for it, a pinned note, files
taken off the shelf (also all at once) and a reminder waiting for a place. Only the last removal can be taken back. A calendar
event cancelled with `cancel` cannot: it is deleted at its source.

## Reminders tied to a place

`remind me when i get home water the plants` (or `... when i leave uni ...`, `at home ...`; in Italian `quando arrivo a casa`,
`quando esco da ...`) waits until the computer gets there or goes away, then rings with a card. Home is the place set for the map
(`home <address>`); any other place is saved while you are there with `place uni`. What is waiting is listed on the Today page,
where the cross takes it off.

- It needs **Follow my position** on. While something waits, the position is asked every two minutes.
- It needs a position sure within 500 m: the one from the internet address is often far rougher, so on a laptop turn on the Wi-Fi
  position (below, off by default). The island says so when the reading is too rough.
- "Getting there" is within 200 m (more when the reading is less sure); being away is half as far again, so a reading that wavers at
  the edge rings nothing. A reminder set while you are already there waits for the next arrival.
- It only works while the computer is on and has a position: it is not a phone's geofence.

Places and what waits for them are kept in `places.json` in the state folder, readable only by you.

## The shelf

A place to set files down for a moment. Drop files on the pill (Wayland), or `open-island shelf <file>...`: they stay on the Shelf page,
across restarts, until you take them off (twelve at most; the files themselves are never moved or deleted). Drag a row to another
window to drop a copy there (Wayland), open it, or send everything to a nearby device with **Send**. A small tray icon in the pill
says something is on it; a file that no longer exists is dropped from the list at the next start.

## Send files to another device (LocalSend)

`open-island send report.pdf photo.jpg` opens a page that lists the devices on your network running
[LocalSend](https://localsend.org) (phones, other computers; the app must be open there), found by looking at the /24 network
for its port, and sends the files to the one you click. The progress pill shows it (the other side has to accept first), then a
card says how it went. It speaks LocalSend's protocol v2 (self-signed HTTPS, which is why the certificate is not checked), so
nothing else has to be installed on this machine. Files dropped on the pill go to the shelf (above), whose **Send** button opens the
same page.

### Receiving (Linux)

Off until you switch it on: **Options > Places > "Receive files"**, or `open-island msg set receive_files on`. The island then
shows up in LocalSend on the other devices, under the computer's name. When one of them sends, a card says who, how much and
the first file names, with **Accept** and **Decline**; nothing is written before you accept, and a card nobody answers for a
minute is a no. The progress drop fills while the files arrive, the card at the end opens the file or its folder, and what came
is also on the shelf. A text message shows on its card with **Copy**. The card appears during do not disturb too, since you are
usually the one sending.

Files go to the download folder, or to `receive_dir` in the configuration. A name is cut to its last part (no folders, no
hidden files) and a file never replaces one that is there: it becomes `name (2)`. A file longer than announced, cut short, or
not matching its checksum is thrown away.

While the option is on the island listens on port 53317 (TCP and UDP), with its own certificate (kept in the state folder), and
answers only addresses of the local network and of your tailnet. Anyone on the same network can see the computer's name and
ask to send; a PIN is not supported. **The LocalSend app on the same computer uses the same port**: only one of the two can
receive (the island says when the port is taken). With a firewall, let TCP and UDP 53317 in.

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
pointer for two minutes. Nothing leaves the machine; the totals per app of today and of the thirteen days before are written to the
state directory (`attention.json`), once a minute.

- **Activity page** (dock, after the system page; `open-island msg page activity`). *Today*: the five apps that held the
  attention, with share and time, and how long you have been on the current one. *Week*: the last seven days, how long each, and
  the apps that took most of them. *Rewind*: the last half hour in one list, newest
  first: windows (an app that had the focus for less than ten seconds is left out), notifications, tracks, and what you copied
  (a click puts it back on the clipboard).
- **Auto focus**: after `deep_work_minutes` (default 20, Options > Quiet "Auto focus", 0 = off) in a row on work apps (editors,
  terminals, PDF and note readers: `deep_work_apps` in `config.json` replaces the list; a detour shorter than a minute does not
  break it) notifications are held like in a meeting, a bell-off icon shows in the pill, and a recap card comes when you leave.
  Reminders and critical notifications still get through.
- **Break nudge**: `break_minutes` (default 0 = off) gives a card after that long on one app, then again after another period.
- **Distracting apps**: the crossed circle on a row of *Today* marks an app (a click on its chip below takes the mark off). In your
  study hours, after `distraction_minutes` (default 20) in a row on marked apps a card says so, then again after another period.
  It only says it: nothing is closed or blocked.

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
Settings > Map > "API key" or `open-island msg set map_api_key <key>`: kept in the keyring like the calendar passwords,
drawn as dots, sent only to the tile server in that URL). Presets, all fetched and looked at: `osm` (default), `esri-dark` (Esri's dark gray canvas, no key, no labels),
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
| receiving files (**off by default**) | devices on your network | the computer's name, to whoever looks for LocalSend devices | leave `receive_files` off (Options > Places) |
| network check | NetworkManager's own connectivity probe, a ping to your router | nothing of yours | `nmcli` absent |

The clipboard history is in memory (only pinned entries are written, mode 0600), one-time codes are never kept, and the activity
page keeps only daily totals per app (two weeks of them) in the state directory. Saved places and the reminders waiting for them
stay on the machine. Nothing is sent to an account or an analytics service.

### Position within metres (Linux)

GeoClue's Wi-Fi source no longer works (Mozilla's location service is closed), so without a GPS the position is a GeoIP guess
of about 25 km. With `wifi_position` on, the island looks up the Wi-Fi networks around you at the open
[BeaconDB](https://beacondb.net) instead. It only knows networks that people have contributed, so where nobody has, the answer
is "not found" and the GeoIP guess stays. You can contribute scans yourself with an Android app such as NeoStumbler (upload
to BeaconDB in its settings). Windows uses the system's own location service.
