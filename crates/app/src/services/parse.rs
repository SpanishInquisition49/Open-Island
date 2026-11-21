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

/// The power plan a Windows machine runs, from `powercfg /getactivescheme` ("Power Scheme GUID: <guid>  (Balanced)").
/// The plans Windows ships are known by their GUID, whatever the language of the name; others are not mapped.
pub fn win_power_profile(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    [
        ("a1841308-3541-4fab-bc81-f71556f20b4a", "powersave"),
        ("381b4222-f694-41f0-9685-ff5bb260df2e", "balanced"),
        ("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c", "performance"),
        ("e9a42b02-d5df-448d-aa00-03f14749eb61", "performance"),
    ]
    .iter()
    .find(|(g, _)| t.contains(g))
    .map(|(_, p)| *p)
}

/// The GUID of the plan Windows ships for a profile name (the Linux names the island uses everywhere).
pub fn win_power_guid(profile: &str) -> Option<&'static str> {
    match profile {
        "powersave" => Some("a1841308-3541-4fab-bc81-f71556f20b4a"),
        "balanced" => Some("381b4222-f694-41f0-9685-ff5bb260df2e"),
        "performance" => Some("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
        _ => None,
    }
}

/// The app a Windows consent-store key stands for: a desktop program is its path with `\` written as `#`
/// (`C:#Users#me#AppData#Local#Discord#Discord.exe` -> `Discord`), a packaged app its package name
/// (`Microsoft.WindowsCamera_8wekyb3d8bbwe` -> `WindowsCamera`). The second tells whether it has a process to name.
pub fn consent_app(key: &str) -> (String, bool) {
    if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        return (file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string(), true);
    }
    let package = key.split('_').next().unwrap_or(key);
    (package.rsplit('.').next().unwrap_or(package).to_string(), false)
}

/// What a Windows toast says (the XML the notification database keeps).
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub title: String,
    pub body: String,
    /// The line some senders add to say where it comes from (browsers put the site there).
    pub attribution: Option<String>,
    /// A local file with the picture, when the toast names one.
    pub image: Option<String>,
}

/// `<toast><visual><binding template="ToastGeneric"><text>Title</text><text>Body</text>...`
pub fn parse_toast(xml: &str) -> Option<Toast> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}').trim()).ok()?;
    let binding = doc.descendants().find(|n| n.has_tag_name("binding"))?;
    let (mut texts, mut attribution) = (vec![], None);
    for t in binding.children().filter(|n| n.has_tag_name("text")) {
        let s = t.text().unwrap_or("").trim().to_string();
        if t.attribute("placement") == Some("attribution") {
            attribution = Some(s).filter(|s| !s.is_empty());
        } else if !s.is_empty() {
            texts.push(s);
        }
    }
    if texts.is_empty() {
        return None;
    }
    let title = texts.remove(0);
    let image = binding
        .children()
        .filter(|n| n.has_tag_name("image"))
        .find(|n| n.attribute("placement") == Some("appLogoOverride") || n.attribute("placement").is_none())
        .and_then(|n| n.attribute("src"))
        .map(|s| s.strip_prefix("file:///").unwrap_or(s).replace('/', "\\"))
        .filter(|s| !s.starts_with("http"));
    Some(Toast { title, body: texts.join("\n"), attribution, image })
}

/// The payload column: UTF-8 XML, or UTF-16 when a sender stored it that way.
pub fn decode_payload(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && (bytes[..2] == [0xff, 0xfe] || bytes.iter().skip(1).step_by(2).take(8).all(|b| *b == 0)) {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').to_string();
    }
    String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string()
}

/// A nicer name for an application user model id (`Chrome`, `MSEdge`, `Microsoft.WindowsStore_8wekyb3d8bbwe!App`).
pub fn app_name_from_aumid(id: &str) -> String {
    let id = id.trim();
    match id.to_ascii_lowercase().as_str() {
        "chrome" | "google chrome" => return "Chrome".into(),
        "msedge" | "microsoft edge" => return "Edge".into(),
        "firefox" | "mozilla firefox" => return "Firefox".into(),
        _ => {}
    }
    let tail = id.rsplit('!').next().unwrap_or(id);
    let name = tail.rsplit('.').next().unwrap_or(tail);
    let name = name.split('_').next().unwrap_or(name);
    if name.is_empty() { id.to_string() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_toast() {
        let xml = r#"<toast launch="x"><visual><binding template="ToastGeneric"><image placement="appLogoOverride" src="file:///C:/Users/a/AppData/Local/Temp/icon.png"/><text>Anna</text><text>Ci vediamo alle 18?</text><text placement="attribution">web.whatsapp.com</text></binding></visual></toast>"#;
        let t = parse_toast(xml).unwrap();
        assert_eq!((t.title.as_str(), t.body.as_str(), t.attribution.as_deref()), ("Anna", "Ci vediamo alle 18?", Some("web.whatsapp.com")));
        assert_eq!(t.image.as_deref(), Some("C:\\Users\\a\\AppData\\Local\\Temp\\icon.png"));
    }

    #[test]
    fn a_toast_with_one_text_or_several() {
        let one = parse_toast(r#"<toast><visual><binding template="ToastGeneric"><text>Backup done</text></binding></visual></toast>"#).unwrap();
        assert_eq!((one.title.as_str(), one.body.as_str(), one.attribution), ("Backup done", "", None));
        let many = parse_toast(r#"<toast><visual><binding template="ToastGeneric"><text>T</text><text>line 1</text><text>line 2</text></binding></visual></toast>"#).unwrap();
        assert_eq!(many.body, "line 1\nline 2");
        let entities = parse_toast(r#"<toast><visual><binding><text>A &amp; B</text><text>&lt;ok&gt;</text></binding></visual></toast>"#).unwrap();
        assert_eq!((entities.title.as_str(), entities.body.as_str()), ("A & B", "<ok>"));
    }

    #[test]
    fn not_a_toast() {
        assert!(parse_toast("").is_none());
        assert!(parse_toast("not xml").is_none());
        assert!(parse_toast("<toast><visual><binding></binding></visual></toast>").is_none(), "no text");
        assert!(parse_toast(r#"<toast><visual><binding><image src="http://x/y.png"/><text>T</text></binding></visual></toast>"#).unwrap().image.is_none(), "web pictures are not fetched");
        assert!(parse_toast("\u{feff}<toast><visual><binding><text>BOM</text></binding></visual></toast>").is_some(), "a byte order mark is fine");
    }

    #[test]
    fn decodes_both_encodings_of_the_payload() {
        assert_eq!(decode_payload(b"<toast/>"), "<toast/>");
        let utf16: Vec<u8> = "<toast/>".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_payload(&utf16), "<toast/>");
        let with_bom: Vec<u8> = [0xff, 0xfe].into_iter().chain(utf16.iter().copied()).collect();
        assert_eq!(decode_payload(&with_bom), "<toast/>");
        assert_eq!(decode_payload(b""), "");
    }

    #[test]
    fn names_an_app_from_its_id() {
        assert_eq!(app_name_from_aumid("Chrome"), "Chrome");
        assert_eq!(app_name_from_aumid("MSEdge"), "Edge");
        assert_eq!(app_name_from_aumid("Microsoft.WindowsStore_8wekyb3d8bbwe!App"), "App");
        assert_eq!(app_name_from_aumid("com.squirrel.Telegram.Desktop"), "Desktop");
        assert_eq!(app_name_from_aumid("308046B0AF4A39CB"), "308046B0AF4A39CB");
        assert_eq!(app_name_from_aumid(""), "");
    }


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

    #[test]
    fn the_windows_power_plan_is_told_by_its_guid() {
        assert_eq!(win_power_profile("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)"), Some("balanced"));
        assert_eq!(win_power_profile("GUID combinazione per il risparmio energia: A1841308-3541-4FAB-BC81-F71556F20B4A  (Risparmio di energia)"), Some("powersave"));
        assert_eq!(win_power_profile("Power Scheme GUID: 11111111-2222-3333-4444-555555555555  (Mine)"), None);
        assert_eq!(win_power_guid("performance"), Some("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"));
        assert_eq!(win_power_guid("turbo"), None);
    }

    #[test]
    fn the_app_behind_a_consent_key_is_named() {
        assert_eq!(consent_app(r"C:#Users#me#AppData#Local#Discord#app-1.0#Discord.exe"), ("Discord".to_string(), true));
        assert_eq!(consent_app("Microsoft.WindowsCamera_8wekyb3d8bbwe"), ("WindowsCamera".to_string(), false));
        assert_eq!(consent_app("Zoom"), ("Zoom".to_string(), false));
    }
}
