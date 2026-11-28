//! LocalSend (https://localsend.org, protocol v2): the pure parts of sending files to a device on the local network and
//! of receiving them.
//! Nothing here does I/O: the app finds the devices and moves the bytes, these functions say what to ask and read the answers.

use serde_json::{json, Value};

pub const PORT: u16 = 53317;

/// A device that answered on the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub alias: String,
    pub ip: String,
    pub port: u16,
    pub model: String,
    /// "https" (the default of LocalSend) or "http".
    pub https: bool,
    /// "mobile", "desktop", "web", "headless", "server"
    pub kind: String,
    /// Reached through Tailscale's Taildrop (`tailscale file cp`), not LocalSend: `ip` is its tailnet address.
    pub taildrop: bool,
}

/// The reply of `GET /api/localsend/v2/info`.
pub fn parse_info(ip: &str, port: u16, https: bool, body: &str) -> Option<Peer> {
    let v: Value = serde_json::from_str(body).ok()?;
    let alias = v.get("alias")?.as_str()?.trim();
    if alias.is_empty() {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(Peer { alias: alias.chars().take(40).collect(), ip: ip.into(), port, https, model: s("deviceModel"), kind: s("deviceType"), taildrop: false })
}

/// Every other address of the /24 network the address is on.
pub fn neighbours(own: [u8; 4]) -> Vec<[u8; 4]> {
    (1..=254u8).filter(|h| *h != own[3]).map(|h| [own[0], own[1], own[2], h]).collect()
}

/// What this device says about itself (the `info` part of a request).
pub fn own_info(alias: &str, fingerprint: &str) -> Value {
    json!({ "alias": alias, "version": "2.0", "deviceModel": "open-island", "deviceType": "desktop", "fingerprint": fingerprint, "port": PORT, "protocol": "https", "download": false })
}

/// A file to send: its id in this transfer, name, size and type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub size: u64,
}

pub fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("txt" | "md" | "log") => "text/plain",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// The body of `POST /api/localsend/v2/prepare-upload`.
pub fn prepare_body(info: &Value, files: &[FileInfo]) -> Value {
    let files: serde_json::Map<String, Value> = files.iter().map(|f| (f.id.clone(), json!({ "id": f.id, "fileName": f.name, "size": f.size, "fileType": mime(&f.name) }))).collect();
    json!({ "info": info, "files": files })
}

/// The answer to prepare-upload: the session and the token of each file.
#[derive(Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub tokens: Vec<(String, String)>,
}

pub fn parse_session(body: &str) -> Option<Session> {
    let v: Value = serde_json::from_str(body).ok()?;
    let id = v.get("sessionId")?.as_str()?.to_string();
    let mut tokens: Vec<(String, String)> = v.get("files")?.as_object()?.iter().filter_map(|(k, t)| Some((k.clone(), t.as_str()?.to_string()))).collect();
    tokens.sort();
    Some(Session { id, tokens })
}

/// Why a prepare-upload was refused, in words for the user.
pub fn refusal(status: u16) -> &'static str {
    match status {
        403 => "declined",
        409 => "busy with another transfer",
        401 => "needs a PIN",
        429 => "too many requests",
        _ => "failed",
    }
}

// ---- receiving

pub const MULTICAST: [u8; 4] = [224, 0, 0, 167];
/// The most files one transfer may offer.
const MAX_FILES: usize = 500;
/// The longest message (text sent as it is, not as a file) that is taken.
const MAX_MESSAGE: usize = 100_000;

/// A file another device wants to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub id: String,
    /// As the sender wrote it: see `safe_name` before it touches the disk.
    pub name: String,
    pub size: u64,
    /// In lower case, when the sender gives it: the file received must match.
    pub sha256: Option<String>,
    /// The whole text, when what is sent is a message and not a file.
    pub message: Option<String>,
}

/// What a device asks to send (the body of `prepare-upload`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub from: String,
    pub files: Vec<Offered>,
}

/// One line of text from outside: no control characters or direction overrides, and not too long.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')).take(max).collect::<String>().trim().to_string()
}

pub fn parse_offer(body: &str) -> Option<Offer> {
    let v: Value = serde_json::from_str(body).ok()?;
    let from = clean(v.get("info")?.get("alias")?.as_str()?, 40);
    let list = v.get("files")?.as_object()?;
    if list.is_empty() || list.len() > MAX_FILES {
        return None;
    }
    let mut files = list
        .iter()
        .map(|(id, f)| {
            let size = f.get("size")?.as_u64()?;
            let text = |k: &str| f.get(k).and_then(Value::as_str);
            // a message is a text whose preview is all of it
            let message = (list.len() == 1 && text("fileType").is_some_and(|t| t.starts_with("text")))
                .then(|| text("preview"))
                .flatten()
                .filter(|p| p.len() as u64 == size && p.len() <= MAX_MESSAGE)
                .map(str::to_string);
            Some(Offered { id: id.clone(), name: text("fileName")?.to_string(), size, sha256: text("sha256").map(str::to_ascii_lowercase).filter(|h| h.len() == 64), message })
        })
        .collect::<Option<Vec<_>>>()?;
    files.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Some(Offer { from: if from.is_empty() { "A device".into() } else { from }, files })
}

/// Cuts at a character boundary, `max` bytes at most.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `report.pdf` -> (`report`, `.pdf`); a name without a short extension is all stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 16 => name.split_at(i),
        _ => (name, ""),
    }
}

/// The name under which a received file may be written: the last part of what the sender said, never a path, never
/// hidden, without characters that mean something to a file system, and short enough for one.
pub fn safe_name(sent: &str) -> String {
    let last = sent.rsplit(['/', '\\']).next().unwrap_or("");
    let plain: String = clean(last, 1000).chars().map(|c| if ":*?\"<>|".contains(c) { '_' } else { c }).collect();
    let name = plain.trim_start_matches(['.', ' ']).trim_end_matches(['.', ' ']);
    if name.is_empty() {
        return "file".into();
    }
    let (stem, ext) = split_ext(name);
    format!("{}{ext}", cut(stem, 200 - ext.len()))
}

/// The name for the `n`th file of this name: `notes.pdf`, `notes (2).pdf`...
pub fn numbered(name: &str, n: u32) -> String {
    if n < 2 {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    format!("{stem} ({n}){ext}")
}

/// What the card says of an offer: how much, and the first names.
pub fn offer_lines(offer: &Offer) -> String {
    let total = offer.files.iter().fold(0u64, |sum, f| sum.saturating_add(f.size));
    let count = if offer.files.len() == 1 { "1 file".to_string() } else { format!("{} files", offer.files.len()) };
    let mut names: Vec<String> = offer.files.iter().take(3).map(|f| safe_name(&f.name)).collect();
    if offer.files.len() > 3 {
        names.push(format!("+{} more", offer.files.len() - 3));
    }
    format!("{count} · {}\n{}", size_text(total), names.join(", "))
}

/// The value of `key` in the query of a request, percent-decoded.
pub fn query_value(query: &str, key: &str) -> Option<String> {
    let raw = query.split('&').find_map(|pair| pair.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))?;
    let (bytes, mut out, mut i) = (raw.as_bytes(), Vec::with_capacity(raw.len()), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' && bytes.len() >= i + 3 {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// The multicast message: this device's `own_info`, announcing itself (the others answer) or answering one.
pub fn announcement(info: &Value, announce: bool) -> Value {
    let mut v = info.clone();
    v["announce"] = json!(announce);
    v
}

/// Another device saying it is there, and whether it wants an answer: (the device, its fingerprint, wants an answer).
pub fn parse_announcement(ip: &str, body: &str) -> Option<(Peer, String, bool)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let port = v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0).unwrap_or(PORT);
    let https = v.get("protocol").and_then(Value::as_str) != Some("http");
    let peer = parse_info(ip, port, https, body)?;
    let fingerprint = v.get("fingerprint").and_then(Value::as_str).unwrap_or("").to_string();
    let wants = v.get("announce").or_else(|| v.get("announcement")).and_then(Value::as_bool).unwrap_or(false);
    Some((peer, fingerprint, wants))
}

/// Paths as the `text/uri-list` another program takes a drop of: one percent-encoded `file://` address a line.
pub fn uri_list(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| {
            let encoded: String = p.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
            format!("file://{encoded}\r\n")
        })
        .collect()
}

/// The files in a `text/uri-list` (what a file manager gives when a file is dropped): `file://` lines, percent-decoded.
pub fn uri_list_paths(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let rest = l.strip_prefix("file://")?;
            // an optional host (empty or localhost) comes before the path
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            if !path.starts_with('/') {
                return None;
            }
            let (bytes, mut out, mut i) = (path.as_bytes(), Vec::with_capacity(path.len()), 0);
            while i < bytes.len() {
                if bytes[i] == b'%' && bytes.len() >= i + 3 {
                    if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            String::from_utf8(out).ok()
        })
        .collect()
}

/// `1.4 MB`, `812 kB`, `37 B`.
pub fn size_text(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "kB", "MB", "GB"];
    let (mut v, mut i) = (bytes as f64, 0);
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 {
        format!("{:.0} {}", v, U[i])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_read_from_its_info() {
        let p = parse_info("192.168.1.9", PORT, true, r#"{"alias":"Pixel","version":"2.0","deviceModel":"Pixel 8","deviceType":"mobile","fingerprint":"x","download":false}"#).unwrap();
        assert_eq!(p, Peer { alias: "Pixel".into(), ip: "192.168.1.9".into(), port: PORT, https: true, model: "Pixel 8".into(), kind: "mobile".into(), taildrop: false });
        assert!(parse_info("1.2.3.4", PORT, true, r#"{"alias":"  "}"#).is_none());
        assert!(parse_info("1.2.3.4", PORT, true, "<html>").is_none());
    }

    #[test]
    fn the_scan_covers_the_network_but_not_this_machine() {
        let n = neighbours([192, 168, 1, 20]);
        assert_eq!(n.len(), 253);
        assert!(!n.contains(&[192, 168, 1, 20]) && n.contains(&[192, 168, 1, 1]) && n.contains(&[192, 168, 1, 254]));
    }

    #[test]
    fn the_prepare_request_lists_each_file_with_its_type() {
        let info = own_info("laptop", "abc");
        let body = prepare_body(&info, &[FileInfo { id: "1".into(), name: "notes.PDF".into(), size: 2048 }]);
        assert_eq!(body["info"]["alias"], "laptop");
        assert_eq!(body["info"]["protocol"], "https");
        assert_eq!(body["files"]["1"]["fileName"], "notes.PDF");
        assert_eq!(body["files"]["1"]["size"], 2048);
        assert_eq!(body["files"]["1"]["fileType"], "application/pdf");
        assert_eq!(mime("blob"), "application/octet-stream");
    }

    #[test]
    fn a_session_carries_a_token_per_file() {
        let s = parse_session(r#"{"sessionId":"s1","files":{"b":"t2","a":"t1"}}"#).unwrap();
        assert_eq!(s, Session { id: "s1".into(), tokens: vec![("a".into(), "t1".into()), ("b".into(), "t2".into())] });
        assert!(parse_session("{}").is_none());
        assert_eq!(refusal(403), "declined");
        assert_eq!(refusal(500), "failed");
    }

    #[test]
    fn an_offer_lists_the_files_and_tells_a_message_apart() {
        let o = parse_offer(r#"{"info":{"alias":"iPhone\u202e\n"},"files":{"b":{"id":"b","fileName":"z.jpg","size":2000,"fileType":"image/jpeg","preview":"abcd","sha256":"AA"},"a":{"id":"a","fileName":"a.pdf","size":1000,"fileType":"application/pdf"}}}"#).unwrap();
        assert_eq!(o.from, "iPhone");
        assert_eq!(o.files.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(o.files.iter().all(|f| f.message.is_none() && f.sha256.is_none()), "a thumbnail is no message, a short hash no hash");
        assert_eq!(offer_lines(&o), "2 files · 3.0 kB\na.pdf, z.jpg");
        let m = parse_offer(r#"{"info":{"alias":"Pixel"},"files":{"t":{"id":"t","fileName":"x.txt","size":5,"fileType":"text/plain","preview":"hello"}}}"#).unwrap();
        assert_eq!(m.files[0].message.as_deref(), Some("hello"));
        // the start of a long text is a preview of a file, not a message
        let f = parse_offer(r#"{"info":{"alias":"Pixel"},"files":{"t":{"id":"t","fileName":"x.txt","size":500,"fileType":"text/plain","preview":"hello"}}}"#).unwrap();
        assert!(f.files[0].message.is_none());
        for bad in ["{}", r#"{"info":{"alias":"x"},"files":{}}"#, r#"{"info":{"alias":"x"},"files":{"a":{"fileName":"a","size":-1}}}"#, "nonsense"] {
            assert!(parse_offer(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_received_name_stays_in_its_folder() {
        assert_eq!(safe_name("notes.pdf"), "notes.pdf");
        assert_eq!(safe_name("../../.ssh/authorized_keys"), "authorized_keys");
        assert_eq!(safe_name("C:\\Users\\x\\a.txt"), "a.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name(" .. "), "file");
        assert_eq!(safe_name("a/"), "file");
        assert_eq!(safe_name("a\u{0}b:c?.png"), "ab_c_.png");
        let long = safe_name(&format!("{}.jpeg", "è".repeat(300)));
        assert!(long.len() <= 200 && long.ends_with(".jpeg"), "{}", long.len());
        assert_eq!(numbered("a.pdf", 1), "a.pdf");
        assert_eq!(numbered("a.pdf", 2), "a (2).pdf");
        assert_eq!(numbered("Makefile", 3), "Makefile (3)");
    }

    #[test]
    fn announcements_go_both_ways() {
        let info = own_info("laptop", "ABC");
        let (peer, fingerprint, wants) = parse_announcement("192.168.1.2", &announcement(&info, true).to_string()).unwrap();
        assert_eq!((peer.alias.as_str(), peer.port, peer.https, fingerprint.as_str(), wants), ("laptop", PORT, true, "ABC", true));
        let (peer, _, wants) = parse_announcement("1.2.3.4", r#"{"alias":"Old","protocol":"http","port":8000}"#).unwrap();
        assert_eq!((peer.port, peer.https, wants), (8000, false, false));
        assert_eq!(query_value("sessionId=s1&fileId=a%20b&token=", "fileId").as_deref(), Some("a b"));
        assert_eq!(query_value("sessionId=s1", "token"), None);
    }

    #[test]
    fn a_dropped_uri_list_gives_the_paths() {
        let t = "# a comment\r\nfile:///home/me/My%20Doc%C3%A8.pdf\r\nfile://localhost/tmp/a.txt\r\nhttps://example.org/x\r\nfile://otherhost/x\r\nfile:///100%25\r\n";
        assert_eq!(uri_list_paths(t), ["/home/me/My Docè.pdf", "/tmp/a.txt", "/100%"]);
        assert!(uri_list_paths("").is_empty());
        assert_eq!(uri_list_paths("file:///bad%zz%"), ["/bad%zz%"], "an escape that is not one stays as it is");
        let odd = vec!["/home/me/My Docè (1).pdf".to_string(), "/tmp/100% #2?.txt".to_string()];
        assert_eq!(uri_list(&odd), "file:///home/me/My%20Doc%C3%A8%20%281%29.pdf\r\nfile:///tmp/100%25%20%232%3F.txt\r\n");
        assert_eq!(uri_list_paths(&uri_list(&odd)), odd, "and read back as they were");
        assert_eq!(uri_list_paths("file:///a%é.txt"), ["/a%é.txt"], "no cut inside a character");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size_text(37), "37 B");
        assert_eq!(size_text(812_000), "812 kB");
        assert_eq!(size_text(1_400_000), "1.4 MB");
        assert_eq!(size_text(2_500_000_000), "2.5 GB");
    }
}
