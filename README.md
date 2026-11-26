<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mosaic-horizontal-dark.svg">
    <img src="assets/logo/mosaic-horizontal.svg" width="380" alt="open-island logo">
  </picture>
</p>

<p align="center">
  <b>A dynamic island for your desktop.</b><br>
  A little black pill at the top of the screen that shows what matters right now, and gets out of the way the rest of the time.
</p>

<p align="center">
  <img src="docs/images/music.png" width="430" alt="The island open on the music player, with the cover, the controls and the lyrics">
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#what-it-does">Features</a> ·
  <a href="#works-on">Platforms</a> ·
  <a href="#your-data">Privacy</a> ·
  <a href="docs/guide.md">Full guide</a>
</p>

At rest it is a clock. When something happens it changes shape to show it: the song you are playing, a message that just
arrived, the timer running out. Hover it or click it to open it.

## What it does

### Music at a glance

The cover, the controls and the lyrics in time with the song. Closed, it keeps the title and a small equalizer in view.

<p align="center">
  <img src="docs/images/pill.png" width="430" alt="The closed island showing the song title, an equalizer and the time">
</p>

### Notifications that drip out of the island

Messages drop out as soft cards, up to three at a time, with their buttons and replies. One-time codes show large, and a
click copies them. In a meeting, on a call or while sharing your screen the island stays quiet, then tells you what you
missed.

<table>
  <tr>
    <td width="50%"><img src="docs/images/notifications.png" alt="Three notification cards stacked under the island"></td>
    <td width="50%"><img src="docs/images/one-time-code.png" alt="A card showing a verification code in large digits"></td>
  </tr>
</table>

### Your day, in one place

Classes, appointments, deadlines and to-dos in order, with the time you need to leave to get there. If you like, a card
in the morning and one in the evening sum it up, weather included.

<table>
  <tr>
    <td width="50%" valign="top"><img src="docs/images/today.png" alt="The Today page with events, pinned notes, deadlines and to-dos"></td>
    <td width="50%" valign="top"><img src="docs/images/study-plan.png" alt="The Plan page with the study of the day, the week and the exams"></td>
  </tr>
</table>

### A study plan that fits your calendar

Add your exams with their date and either their credits or the hours you expect to need. The island suggests what to study
each day: more on free days, less on busy ones, and never more than the day can hold. You choose when your study day
starts and ends, weekday by weekday, when you break for lunch, and which days are off. Each of your calendars keeps its own colour.

### Timers, and a bar you can talk to

Several countdowns at once, a Pomodoro and a stopwatch. Open the command bar (give `open-island msg bar` a keyboard
shortcut) and just type:

| Type | And you get |
|---|---|
| `10m tea` | a ten-minute timer |
| `remind friday 9:00 call the office` | a reminder |
| `todo report due fri` | a task for your list |
| `exam 15 dec Algorithms 9cfu` | an exam in your study plan |
| `20% of 150` | the answer, ready to paste |
| `5 km in mi` | a conversion |

<table>
  <tr>
    <td width="50%"><img src="docs/images/timer.png" alt="A countdown with buttons to add a minute or cancel"></td>
    <td width="50%"><img src="docs/images/command-bar.png" alt="The command bar converting kilometres to miles"></td>
  </tr>
</table>

### Weather and the way there

Rain warnings before you leave, the forecast for the day, and a map with the way to your next event: by car, on foot, by
public transport or by train. If you take the same train every day (Trenitalia, in Italy), it tells you when it is late.

<table>
  <tr>
    <td width="50%"><img src="docs/images/home.png" alt="The home page with the time, the weather, quick timers and the next event"></td>
    <td width="50%"><img src="docs/images/weather.png" alt="The weather page with the forecast and the chance of rain"></td>
  </tr>
</table>

### Controls within reach

Volume, brightness, Wi-Fi, Bluetooth, do not disturb and keep awake. Choose where the sound plays, set the volume of each
app, and see your headphones' battery when they connect.

<table>
  <tr>
    <td width="50%"><img src="docs/images/quick-settings.png" alt="The quick settings with switches and sliders"></td>
    <td width="50%"><img src="docs/images/sound.png" alt="The sound page with outputs, inputs and the volume of each app"></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/headphones.png" alt="A card saying the headphones are connected, with their battery"></td>
    <td width="50%"><img src="docs/images/send-files.png" alt="The list of nearby devices a file can be sent to"></td>
  </tr>
</table>

### And the small things

- **Send files** to your phone or another computer nearby (with [LocalSend](https://localsend.org)).
- **Clipboard history**, with the entries you want to keep pinned.
- **Downloads and long jobs** beside the island, as a drop of liquid: it fills up with a job that says how far it is,
  and sloshes while your browser downloads a file.
- **A heads-up** when a program is slowing the computer down, or the network stops working.
- **Camera and microphone dots**, so you know when they are in use.
- **Make it yours:** most things can be switched on or off from the options page.

<p align="center">
  <img src="docs/images/progress.png" width="430" alt="A job at 62 percent beside the island, filling like a drop">
</p>

### For people who like the terminal

Your scripts can talk to the island: show progress, ask a yes-or-no question, or say that a job is done. Coding agents
such as Claude Code can use it to tell you when they need you.

```sh
open-island ok "Build finished"
open-island run make test
```

<p align="center">
  <img src="docs/images/script-cards.png" width="430" alt="Cards sent by scripts, one of them asking a question with two buttons">
</p>

## Works on

| System | Status |
|---|---|
| **Linux** (Wayland) | Ready for daily use. Developed on [niri](https://github.com/YaLTeR/niri); other desktops may behave differently. |
| **Windows** | In testing. Most features are there; a few are Linux only. |
| **macOS** | Experimental: it has never been tried on a Mac, and several features are missing. |

The [full guide](docs/guide.md#status) says what works where, feature by feature.

## Install

There are no ready-made downloads yet, so you build it yourself. You need [Rust](https://rustup.rs) and a graphics card
with up-to-date drivers; on Windows also the Visual Studio C++ build tools, which the Rust installer offers.

### Linux

```sh
git clone https://github.com/SpanishInquisition49/Open-Island.git
cd Open-Island
cargo install --path crates/app

open-island doctor          # checks what works on your computer and what is missing
open-island autostart on    # start it with your session
open-island                 # start it now
```

> **Note:** the island takes over your desktop notifications and stops the program that was showing them. To keep your
> current one, add `"take_over_notifications": false` to `~/.config/open-island/config.json`.

### Windows

```powershell
git clone https://github.com/SpanishInquisition49/Open-Island.git
cd Open-Island
cargo build --release
.\target\release\open-island.exe
```

Something looks wrong? The [Windows test notes](docs/windows-test.md) say what to check and what to report.

## Set it up

Open the island and click the **gear** to add your calendars (iCloud, Google, Outlook and most others), choose your
weather location and pick a sound. The **options** page switches features on and off.

## Your data

- No sign-up, no ads, no tracking.
- Some features ask public services for what they need: weather, maps, directions, trains, lyrics and exchange rates.
  Each can be switched off or left unused.
- The weather and the map follow your computer's location. You can set a fixed place instead.
- Your clipboard history stays in memory, apart from what you pin, and one-time codes are kept out of it.

The guide lists exactly [what leaves your computer](docs/guide.md#what-leaves-the-machine).

## Credits

open-island began as a port of a dynamic island plugin for the Noctalia shell. The notification cards are modelled on
[drip](https://github.com/jrnxf/drip), and the icons are [Tabler](https://tabler.io/icons). See
[third-party notices](THIRD-PARTY-NOTICES.md).

Released under the [MIT licence](LICENSE).
