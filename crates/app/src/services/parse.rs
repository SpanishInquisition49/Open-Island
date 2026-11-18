//! Parsers for what the macOS and Windows helper tools print (osascript, PowerShell). They are plain functions,
//! so they are tested on every platform even though only one of them runs them.
#![cfg_attr(target_os = "linux", allow(dead_code))]

/// `get volume settings` -> (output volume 0..=1, muted).
pub fn mac_volume(text: &str) -> Option<(f32, bool)> {
    let field = |k: &str| text.split(',').find_map(|p| p.trim().strip_prefix(k)).map(str::trim);
    let vol: f32 = field("output volume:")?.parse().ok()?; // "missing value" (no volume control) fails here
    let muted = field("output muted:").is_some_and(|m| m == "true");
    Some((vol / 100.0, muted))
}

/// What a player reports.
#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub position_ms: f64,
    pub length_ms: f64,
    pub art_url: Option<String>,
}

/// The script's answer `state||title||artist||position seconds||duration||artwork url`. Spotify's duration is in
/// milliseconds, Music's in seconds (`duration_in_ms`).
pub fn now_playing(text: &str, duration_in_ms: bool) -> Option<NowPlaying> {
    let f: Vec<&str> = text.trim().split("||").collect();
    if f.len() < 5 || f[1].is_empty() {
        return None;
    }
    let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
    Some(NowPlaying {
        playing: f[0].trim() == "playing",
        title: f[1].to_string(),
        artist: f[2].to_string(),
        position_ms: num(f[3])? * 1000.0,
        length_ms: num(f[4])? * if duration_in_ms { 1.0 } else { 1000.0 },
        art_url: f.get(5).map(|u| u.trim()).filter(|u| u.starts_with("http")).map(str::to_string),
    })
}

/// `(Get-CimInstance ... WmiMonitorBrightness).CurrentBrightness` -> 0..=1.
pub fn win_brightness(text: &str) -> Option<f32> {
    let v: f32 = text.lines().find(|l| !l.trim().is_empty())?.trim().parse().ok()?;
    (0.0..=100.0).contains(&v).then_some(v / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_macos_volume() {
        assert_eq!(mac_volume("output volume:50, input volume:75, alert volume:100, output muted:false"), Some((0.5, false)));
        assert_eq!(mac_volume("output volume:0, input volume:75, alert volume:100, output muted:true"), Some((0.0, true)));
        assert_eq!(mac_volume("output volume:missing value, input volume:75, alert volume:100, output muted:missing value"), None);
        assert_eq!(mac_volume("nonsense"), None);
    }

    #[test]
    fn reads_what_a_player_reports() {
        let spotify = now_playing("playing||Touch||Daft Punk||96.5||498000||https://i.scdn.co/image/abc\n", true).unwrap();
        assert_eq!((spotify.playing, spotify.title.as_str(), spotify.position_ms, spotify.length_ms), (true, "Touch", 96_500.0, 498_000.0));
        assert_eq!(spotify.art_url.as_deref(), Some("https://i.scdn.co/image/abc"));
        let music = now_playing("paused||Song||Band||12,5||240", false).unwrap();
        assert_eq!((music.playing, music.position_ms, music.length_ms, music.art_url), (false, 12_500.0, 240_000.0, None), "Music: seconds, a decimal comma");
        assert!(now_playing("stopped||||||0||0", true).is_none(), "no track");
        assert!(now_playing("garbage", true).is_none());
    }

    #[test]
    fn reads_the_windows_brightness() {
        assert_eq!(win_brightness("70\r\n"), Some(0.7));
        assert_eq!(win_brightness("\r\n  100 \r\n"), Some(1.0));
        assert_eq!(win_brightness("Get-CimInstance : Not supported"), None);
        assert_eq!(win_brightness("250"), None);
        assert_eq!(win_brightness(""), None);
    }
}
