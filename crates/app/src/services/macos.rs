//! macOS services through osascript: the master volume and the track of Spotify or Music. (macOS has no public
//! now-playing API for other players; MediaRemote is private.) Not run on a Mac yet: type-checked with
//! `cargo check --target aarch64-apple-darwin`.

use super::parse::{mac_volume, now_playing, NowPlaying};
use super::{art, Reporter};
use island_ui::model::{Command, Input, Media, OsdKind};
use std::process::{Child, Command as Proc, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

static TX: Mutex<Option<Sender<Command>>> = Mutex::new(None);

fn osascript(script: &str) -> Option<String> {
    let out = Proc::new("osascript").args(["-e", script]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn start(rep: Reporter) {
    let (tx, rx) = channel();
    *TX.lock().unwrap() = Some(tx);
    std::thread::spawn(move || run(rep, rx));
}

pub fn command(c: Command) {
    if let Some(t) = TX.lock().unwrap().as_ref() {
        let _ = t.send(c);
    }
}

/// The player to ask: the first of Spotify and Music that is running and has a track.
fn track() -> Option<(&'static str, NowPlaying)> {
    for (app, ms) in [("Spotify", true), ("Music", false)] {
        let script = format!(
            "if application \"{app}\" is running then\n tell application \"{app}\"\n if player state is stopped then return \"\"\n return (player state as text) & \"||\" & (name of current track) & \"||\" & (artist of current track) & \"||\" & (player position as text) & \"||\" & ((duration of current track) as text){}\n end tell\nend if",
            if ms { " & \"||\" & (artwork url of current track)" } else { "" }
        );
        if let Some(np) = osascript(&script).and_then(|t| now_playing(&t, ms)) {
            return Some((app, np));
        }
    }
    None
}

fn run(rep: Reporter, rx: Receiver<Command>) {
    let mut keep_awake: Option<Child> = None;
    let (mut last_vol, mut last_track, mut art_url, mut art_cache) = (None::<(f32, bool)>, String::new(), None::<String>, None);
    let mut last_sent: Option<Media> = None;
    loop {
        while let Ok(c) = rx.try_recv() {
            match c {
                Command::SetVolume(v) => drop(osascript(&format!("set volume output volume {}", (v.clamp(0.0, 1.0) * 100.0).round() as i32))),
                Command::ToggleMute => drop(osascript("set volume output muted (not (output muted of (get volume settings)))")),
                Command::MediaPlayPause | Command::MediaNext | Command::MediaPrevious | Command::MediaSeek { .. } => {
                    if let Some((app, _)) = track() {
                        let verb = match c {
                            Command::MediaPlayPause => "playpause".to_string(),
                            Command::MediaNext => "next track".to_string(),
                            Command::MediaPrevious => "previous track".to_string(),
                            Command::MediaSeek { position_ms } => format!("set player position to {}", position_ms / 1000.0),
                            _ => continue,
                        };
                        drop(osascript(&format!("tell application \"{app}\" to {verb}")));
                    }
                }
                Command::ToggleKeepAwake => {
                    match keep_awake.take() {
                        Some(mut k) => {
                            let _ = k.kill();
                            let _ = k.wait();
                        }
                        None => keep_awake = Proc::new("caffeinate").args(["-di"]).stdin(Stdio::null()).spawn().ok(),
                    }
                    let on = keep_awake.is_some();
                    rep.patch(move |d| d.keep_awake = on);
                }
                _ => {}
            }
        }
        if let Some(v) = osascript("get volume settings").and_then(|t| mac_volume(&t)) {
            if Some(v) != last_vol {
                if last_vol.is_some() {
                    rep.input(Input::Osd { kind: OsdKind::Volume, level: v.0, muted: v.1 });
                }
                last_vol = Some(v);
                rep.patch(move |d| {
                    d.volume = Some(v.0);
                    d.muted = v.1;
                });
            }
        }
        match track() {
            Some((app, np)) => {
                let id = format!("{app}|{}|{}", np.title, np.artist);
                if id != last_track {
                    last_track = id.clone();
                    if np.art_url != art_url {
                        art_url = np.art_url.clone();
                        art_cache = art_url.as_deref().and_then(art::load);
                    }
                }
                let now = chrono::Utc::now().timestamp_millis();
                let m = Media { track_id: id, title: np.title, artist: np.artist, playing: np.playing, position_ms: np.position_ms, sampled_at_ms: now, length_ms: np.length_ms, art: art_cache.clone(), can_seek: true, players: 1, player: app.to_string() };
                let worth = last_sent.as_ref().is_none_or(|l| {
                    let expected = if l.playing { l.position_ms + (now - l.sampled_at_ms) as f64 } else { l.position_ms };
                    l.track_id != m.track_id || l.playing != m.playing || l.length_ms != m.length_ms || (m.position_ms - expected).abs() > 1500.0 || l.art.as_ref().map(|a| a.id) != m.art.as_ref().map(|a| a.id)
                });
                if worth {
                    last_sent = Some(m.clone());
                    rep.patch(move |d| d.media = Some(m));
                }
            }
            None => {
                if last_sent.take().is_some() {
                    rep.patch(|d| d.media = None);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
}
