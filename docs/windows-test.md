# Trying open-island on Windows

Nothing here has run on Windows yet; it only type-checks for `x86_64-pc-windows-msvc`. Work through the list and
note what fails (the text of the error, or what you saw instead). Build with `cargo build --release` (Rust and the
MSVC build tools needed) or take the `open-island.exe` someone built for you.

1. `open-island doctor`: a GPU line (DX12 or Vulkan), a font, `powershell` ok.
2. `open-island` (or double-click): a black pill appears at the top centre, above other windows, and clicks outside
   the pill reach the windows behind it. Without a GPU adapter it exits with a message: send it.
3. The clock face, then hover: the island opens to the home page; the dock icons switch pages.
4. `open-island msg notify "Test | body"`, then click the card: it goes away. The x dismisses it too.
5. `open-island timer 10`, `pomodoro`, `stopwatch`: bubble next to the pill, hover opens the timer page.
6. Music: play something in Spotify, the browser or the Windows media player: title, cover, play/pause/next work.
7. Volume: change it with the keys: a volume card appears; the slider on the quick-settings page moves it.
8. Brightness (laptop only): the slider on the quick-settings page changes the panel; a desktop monitor has none. It is read as the page opens and every five seconds while it stays open; while it is closed no `powershell` process should keep appearing in Task Manager (one at start is expected).
9. `open-island progress run -l Ping -- ping -n 6 127.0.0.1`: a progress card, then a green "done".
   (`ping` prints no percentage, so the bar stays indeterminate; try `curl -O` of a big file for a real one.)
10. Open a fullscreen video or game: the island hides; leave it: it comes back.
11. Use the microphone (a call, the voice recorder): the orange dot shows; stop: it goes.
12. `open-island autostart on`, sign out and in: it starts by itself. `autostart off` undoes it.
13. Settings page: the font field, add a calendar address (an iCloud account needs an app-specific password).
14. Leave it running for an hour: memory stays flat, CPU near zero when idle.

Added later, also never run: the Wi-Fi and Bluetooth switches on the quick-settings page (they hide when the machine
has no such radio; the first press may show Windows' permission question), the Map page and the weather following the
position (Settings > Privacy > Location must be on, else the coordinates of the settings are used), the Activity page
(the program in front, checked once a second), and a click on another app's notification bringing its window to the front.

Added on 2026-11-21, also never run: the heavy-program card (a program using most of the CPU for 15 s, or memory under 8 %:
try a busy loop in PowerShell, `while ($true) {}`, in a few windows; Pause / Resume / Quit on the System page; system
programs such as explorer and svchost are never offered), the network card (unplug the cable or turn Wi-Fi off and on: "no link"
or "no internet"; a click on it switches the Wi-Fi radio off and on), the power plan on the quick-settings page (cycles
performance, balanced, power saver; a custom plan hides it), the Sound page (outputs, inputs, volume and mute of each app that
is playing; choosing another output uses an undocumented Windows interface, so say if it does nothing or crashes), and the
equalizer following the music (WASAPI loopback of the default output).

`progress run` was reworked for Windows (the output is read on a thread because the pseudo-console keeps its pipe open after the
command ends; batch files such as `npm` and built-ins such as `dir` are retried through `cmd /C`): try
`open-island progress run -- cmd /C dir`, `... -- npm --version` and a long download. The PowerShell hook is new too:
`open-island hook powershell | Out-String | Invoke-Expression` in `$PROFILE`, then run `Start-Sleep 12`: a pill appears after
10 s and turns green. If it clashes with another PSReadLine Enter binding or a prompt framework (starship, oh-my-posh), say so.

Also: the System page now names the program using the microphone or the camera (from the privacy registry) with a Quit button; packaged apps show only their name.

Known gaps on Windows: night light, muting one microphone user, audio following the headphones.

If nothing appears: run it from a terminal (PowerShell) as `$env:RUST_LOG="debug"; .\open-island.exe` and send the output;
the lines "transparent surface", "window created", "showing the island" and "hiding the island (fullscreen: ...)" say how
far it got.

Smoothness and the edge (the window is one fixed size and does not change while the island animates; if it still
stutters, run with `$env:OPEN_ISLAND_PERF=1` and send the log lines): on Windows the island draws through DirectX 12 into a transparent DirectComposition surface
(log line "transparent surface (PreMultiplied) on ...", which also names the graphics adapter used). Until 2026-11-26 the
surface was in fact opaque although that line appeared: a black rectangle showed while the island moved and was cut round
when it stopped. If that, or anything like it, still happens (black rectangle, nothing shown), start it with
`$env:OPEN_ISLAND_OPAQUE=1` to use the old cut-to-shape way and tell me which of the two worked. With a maximized
window in front the island hides; moving the pointer to the top edge, around the middle of the screen, brings it
back, and it goes again a moment after the pointer leaves. A notification, the volume or a timer still show.

Notifications from other applications (the browser, Telegram...): Windows will not hand them to a plain program, so the island
reads the database where Windows keeps them (`%LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db`) every second
and a half and shows every new toast. `open-island doctor` says whether it found the file; the log says "watching the
notification database". Test: a site's notification in Chrome/Edge/Firefox, `Win+N` shows the same one. The Windows banner
appears as well; Settings > System > Notifications > (the app) > "Show notification banners" off leaves only the island.
This reads an undocumented file, so it can stop working with a Windows update.
