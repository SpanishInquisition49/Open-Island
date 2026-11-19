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
8. Brightness (laptop only): the slider on the quick-settings page changes the panel; a desktop monitor has none.
9. `open-island progress run -l Ping -- ping -n 6 127.0.0.1`: a progress card, then a green "done".
   (`ping` prints no percentage, so the bar stays indeterminate; try `curl -O` of a big file for a real one.)
10. Open a fullscreen video or game: the island hides; leave it: it comes back.
11. Use the microphone (a call, the voice recorder): the orange dot shows; stop: it goes.
12. `open-island autostart on`, sign out and in: it starts by itself. `autostart off` undoes it.
13. Settings page: the font field, add a calendar address (an iCloud account needs an app-specific password).
14. Leave it running for an hour: memory stays flat, CPU near zero when idle.

Known gaps on Windows: Wi-Fi and Bluetooth switches, night light, going to the app behind a notification (there are
no desktop notifications from other apps to click anyway), the audio equalizer (it shows a plain animation).

If nothing appears: run it from a terminal (PowerShell) as `$env:RUST_LOG="debug"; .\open-island.exe` and send the output;
the lines "window created", "showing the island" and "hiding the island (fullscreen window: ...)" say how far it got. Windows
surfaces are opaque, so the line "the window surface is opaque" is normal: the window is cut to the island's shape.

Smoothness and the edge: on Windows the island draws through DirectX 12 into a transparent DirectComposition surface
(log line "transparent surface"). If that misbehaves (black rectangle, nothing shown), start it with
`$env:OPEN_ISLAND_OPAQUE=1` to use the old cut-to-shape way and tell me which of the two worked. With a maximized
window in front the island hides; moving the pointer to the top edge, around the middle of the screen, brings it
back, and it goes again a moment after the pointer leaves. A notification, the volume or a timer still show.
