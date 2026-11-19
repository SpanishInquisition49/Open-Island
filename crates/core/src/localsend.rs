//! LocalSend (https://localsend.org, protocol v2): the pure parts of sending files to a device on the local network.
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
}

/// The reply of `GET /api/localsend/v2/info`.
pub fn parse_info(ip: &str, port: u16, https: bool, body: &str) -> Option<Peer> {
    let v: Value = serde_json::from_str(body).ok()?;
    let alias = v.get("alias")?.as_str()?.trim();
    if alias.is_empty() {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(Peer { alias: alias.chars().take(40).collect(), ip: ip.into(), port, https, model: s("deviceModel"), kind: s("deviceType") })
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
        assert_eq!(p, Peer { alias: "Pixel".into(), ip: "192.168.1.9".into(), port: PORT, https: true, model: "Pixel 8".into(), kind: "mobile".into() });
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
    fn sizes_read_naturally() {
        assert_eq!(size_text(37), "37 B");
        assert_eq!(size_text(812_000), "812 kB");
        assert_eq!(size_text(1_400_000), "1.4 MB");
        assert_eq!(size_text(2_500_000_000), "2.5 GB");
    }
}
