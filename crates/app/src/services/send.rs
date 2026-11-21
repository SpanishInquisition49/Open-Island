//! Sending files to a device on the local network with the LocalSend protocol (see island_core::localsend): scanning
//! the network for devices, and the upload with its progress.

use super::{net, Reporter};
use island_core::localsend::{self as ls, FileInfo, Peer};
use island_core::progress::{Activity, State};
use island_ui::model::{Incoming, Input};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The address this machine uses on the network (no packet is sent).
fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(a) if !a.is_loopback() => Some(a),
        _ => None,
    }
}

/// Asks one address whether a LocalSend device answers there.
pub fn probe(ip: Ipv4Addr, port: u16, connect_timeout: Duration) -> Option<Peer> {
    TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(ip), port), connect_timeout).ok()?;
    let agent = net::lan_agent(Duration::from_secs(3));
    for https in [true, false] {
        let url = format!("{}://{ip}:{port}/api/localsend/v2/info", if https { "https" } else { "http" });
        if let Ok(mut r) = agent.get(&url).call() {
            if r.status().is_success() {
                if let Some(p) = r.body_mut().read_to_string().ok().and_then(|b| ls::parse_info(&ip.to_string(), port, https, &b)) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Runs `tailscale` and gives what it printed (None when it is not installed, not running, or slow).
fn tailscale(args: &[&str]) -> Option<String> {
    let mut child = Command::new("tailscale").args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let began = Instant::now();
    while child.try_wait().ok()?.is_none() {
        if began.elapsed() > Duration::from_secs(6) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

/// The devices of the tailnet that Taildrop can send to right now.
fn taildrop_peers() -> Vec<Peer> {
    let Some(targets) = tailscale(&["file", "cp", "--targets"]) else { return vec![] };
    let os = tailscale(&["status", "--json"]).map(|j| island_core::taildrop::parse_os(&j)).unwrap_or_default();
    island_core::taildrop::peers(&island_core::taildrop::parse_targets(&targets), &os)
}

/// Looks for devices on the /24 network: the list on the island grows as they answer.
pub fn scan(rep: Reporter) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        // the tailnet answers at once; the network is looked through after
        let tail = taildrop_peers();
        let first = tail.clone();
        rep.patch(move |d| {
            d.peers = first;
            d.peers_scanning = true;
        });
        if let Some(own) = local_ip() {
            let hosts = Arc::new(ls::neighbours(own.octets()));
            let (next, found) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(tail)));
            let workers: Vec<_> = (0..48)
                .map(|_| {
                    let (hosts, next, found, rep) = (hosts.clone(), next.clone(), found.clone(), rep.clone());
                    std::thread::spawn(move || loop {
                        let Some(h) = hosts.get(next.fetch_add(1, Ordering::Relaxed)) else { return };
                        if let Some(p) = probe(Ipv4Addr::from(*h), ls::PORT, Duration::from_millis(350)) {
                            let list = {
                                let mut f = found.lock().unwrap();
                                f.push(p);
                                f.sort_by(|a, b| a.alias.cmp(&b.alias));
                                f.clone()
                            };
                            rep.patch(move |d| d.peers = list);
                        }
                    })
                })
                .collect();
            for w in workers {
                let _ = w.join();
            }
        }
        rep.patch(|d| d.peers_scanning = false);
        BUSY.store(false, Ordering::SeqCst);
    });
}

fn fingerprint() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:032x}", n ^ ((std::process::id() as u128) << 64))
}

fn alias() -> String {
    std::env::var("HOSTNAME").ok().filter(|h| !h.is_empty()).or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|h| h.trim().to_string())).filter(|h| !h.is_empty()).unwrap_or_else(|| "open-island".into())
}

/// A reader that tells how many bytes went through.
struct Counting<R> {
    inner: R,
    sent: Arc<AtomicUsize>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.sent.fetch_add(n, Ordering::Relaxed);
        Ok(n)
    }
}

/// Files the user can send: regular files that exist, with their names.
pub fn collect(paths: &[String]) -> Vec<(PathBuf, FileInfo)> {
    paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((path, meta.len(), name))
        })
        .enumerate()
        .map(|(i, (path, size, name))| (path, FileInfo { id: format!("f{i}"), name, size }))
        .collect()
}

/// What a transfer reports while it runs.
pub enum Progress {
    /// Waiting for the other side to accept.
    Waiting,
    /// 0..=1 of everything, and the name of the file being sent.
    Sending(f64, String),
}

/// Sends the files to the device, one after the other. Blocks until it is done; the error says why it was not.
pub fn send_files(peer: &Peer, paths: &[String], mut on: impl FnMut(Progress)) -> Result<usize, String> {
    let files = collect(paths);
    if files.is_empty() {
        return Err("nothing to send".into());
    }
    let base = format!("{}://{}:{}/api/localsend/v2", if peer.https { "https" } else { "http" }, peer.ip, peer.port);
    // accepting can take as long as the person at the other end needs
    let agent = net::lan_agent(Duration::from_secs(180));
    on(Progress::Waiting);
    let infos: Vec<FileInfo> = files.iter().map(|(_, f)| f.clone()).collect();
    let body = ls::prepare_body(&ls::own_info(&alias(), &fingerprint()), &infos);
    let mut resp = agent.post(&format!("{base}/prepare-upload")).header("Content-Type", "application/json").send(body.to_string()).map_err(|e| format!("{}: {e}", peer.alias))?;
    let status = resp.status().as_u16();
    if status == 204 {
        return Ok(0);
    }
    if status != 200 {
        return Err(format!("{} {}", peer.alias, ls::refusal(status)));
    }
    let session = ls::parse_session(&resp.body_mut().read_to_string().map_err(|e| e.to_string())?).ok_or("unexpected answer")?;
    let total: u64 = files.iter().map(|(_, f)| f.size).sum::<u64>().max(1);
    let done = Arc::new(AtomicUsize::new(0));
    let mut sent_before = 0u64;
    for (path, f) in &files {
        let Some((_, token)) = session.tokens.iter().find(|(id, _)| *id == f.id) else { continue };
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", f.name))?;
        done.store(0, Ordering::Relaxed);
        let mut reader = Counting { inner: file, sent: done.clone() };
        let url = format!("{base}/upload?sessionId={}&fileId={}&token={}", session.id, f.id, token);
        let (name, size) = (f.name.clone(), f.size);
        let result = std::thread::scope(|scope| {
            let upload = scope.spawn(|| agent.post(&url).header("Content-Type", ls::mime(&name)).header("Content-Length", size.to_string()).send(ureq::SendBody::from_reader(&mut reader)));
            // this thread reports while the other one is busy inside the upload
            while !upload.is_finished() {
                on(Progress::Sending(((sent_before + done.load(Ordering::Relaxed) as u64) as f64 / total as f64).min(1.0), name.clone()));
                std::thread::sleep(Duration::from_millis(120));
            }
            upload.join()
        });
        match result {
            Ok(Ok(r)) if r.status().is_success() => {}
            Ok(Ok(r)) => return Err(format!("{name}: {}", ls::refusal(r.status().as_u16()))),
            Ok(Err(e)) => return Err(format!("{name}: {e}")),
            Err(_) => return Err(format!("{name}: failed")),
        }
        sent_before += size;
        on(Progress::Sending((sent_before as f64 / total as f64).min(1.0), name));
    }
    Ok(files.len())
}

/// Sends in the background: the progress pill shows it, and a card says how it ended.
pub fn send(rep: Reporter, peer: Peer, paths: Vec<String>) {
    if peer.taildrop {
        return send_taildrop(rep, peer, paths);
    }
    std::thread::spawn(move || {
        let act = |detail: String, frac: Option<f64>| Activity { id: "send".into(), label: format!("Sending to {}", peer.alias), detail, icon: Some("send".into()), tint: None, frac };
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let result = send_files(&peer, &paths, |p| {
            let (detail, frac) = match p {
                Progress::Waiting => ("Waiting for them to accept".to_string(), None),
                Progress::Sending(f, name) => (name, Some(f)),
            };
            let mut l = last.lock().unwrap();
            if l.elapsed() > Duration::from_millis(100) {
                *l = Instant::now();
                rep.input(Input::Activity(State::Run, act(detail, frac)));
            }
        });
        let (state, summary, body) = match &result {
            Ok(0) => (State::Done, "Nothing to send".to_string(), format!("{} has it already", peer.alias)),
            Ok(n) => (State::Done, "Sent".to_string(), format!("{} to {}", if *n == 1 { "1 file".to_string() } else { format!("{n} files") }, peer.alias)),
            Err(e) => (State::Fail, "Send failed".to_string(), e.clone()),
        };
        rep.input(Input::Activity(state, act(body.clone(), Some(1.0))));
        rep.input(Input::Notification(Incoming { id: 3_600_000 + (std::process::id() % 1000), app: "open-island".into(), summary, body, expire_ms: Some(3500), ..Default::default() }));
    });
}

/// Sends through Taildrop: `tailscale file cp <files> <address>:`. It reports no progress, so the pill only says where it goes.
fn send_taildrop(rep: Reporter, peer: Peer, paths: Vec<String>) {
    std::thread::spawn(move || {
        let act = |detail: String| Activity { id: "send".into(), label: format!("Sending to {}", peer.alias), detail, icon: Some("send".into()), tint: None, frac: None };
        rep.input(Input::Activity(State::Run, act("through Tailscale".into())));
        let files = collect(&paths);
        let mut cmd = Command::new("tailscale");
        cmd.args(["file", "cp", "--"]).args(files.iter().map(|(p, _)| p)).arg(format!("{}:", peer.ip)).stdin(std::process::Stdio::null());
        let result = if files.is_empty() { Ok(0) } else { cmd.output().map_err(|e| e.to_string()).and_then(|o| if o.status.success() { Ok(files.len()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("tailscale failed").trim().to_string()) }) };
        let (state, summary, body) = match &result {
            Ok(0) => (State::Done, "Nothing to send".to_string(), "no file".to_string()),
            Ok(n) => (State::Done, "Sent".to_string(), format!("{} to {} (Tailscale)", if *n == 1 { "1 file".to_string() } else { format!("{n} files") }, peer.alias)),
            Err(e) => (State::Fail, "Send failed".to_string(), e.clone()),
        };
        rep.input(Input::Activity(state, act(body.clone())));
        rep.input(Input::Notification(Incoming { id: 3_600_000 + (std::process::id() % 1000), app: "open-island".into(), summary, body, expire_ms: Some(3500), ..Default::default() }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// A receiver that behaves like LocalSend over plain http: what it was asked, and what it got.
    /// The request line and body of each request the receiver saw.
    type Seen = Vec<(String, Vec<u8>)>;

    fn receiver(accept: bool) -> (u16, std::thread::JoinHandle<Seen>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut seen = vec![];
            l.set_nonblocking(false).unwrap();
            // the sender asks at most: prepare-upload, then one upload per file
            while let Ok((mut s, _)) = l.accept() {
                let mut buf = vec![];
                let mut byte = [0u8; 1];
                while !buf.ends_with(b"\r\n\r\n") {
                    if s.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    buf.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).unwrap_or(0);
                let mut body = vec![0u8; len];
                s.read_exact(&mut body).unwrap();
                let (status, reply) = if line.contains("prepare-upload") {
                    if accept { ("200 OK", r#"{"sessionId":"S","files":{"f0":"T0","f1":"T1"}}"#.to_string()) } else { ("403 Forbidden", String::new()) }
                } else {
                    ("200 OK", String::new())
                };
                let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                seen.push((line.clone(), body));
                if !accept || line.contains("fileId=f1") {
                    break;
                }
            }
            seen
        });
        (port, h)
    }

    fn peer(port: u16) -> Peer {
        Peer { alias: "Pixel".into(), ip: "127.0.0.1".into(), port, https: false, model: String::new(), kind: "mobile".into(), taildrop: false }
    }

    fn files(dir: &std::path::Path) -> Vec<String> {
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.join("b.png"), vec![7u8; 300_000]).unwrap();
        vec![dir.join("a.txt").to_string_lossy().into(), dir.join("b.png").to_string_lossy().into(), dir.join("missing").to_string_lossy().into(), dir.to_string_lossy().into()]
    }

    #[test]
    fn files_go_up_one_by_one_with_the_token_of_each() {
        let dir = std::env::temp_dir().join(format!("oi-send-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = files(&dir);
        let (port, server) = receiver(true);
        let mut fractions = vec![];
        let n = send_files(&peer(port), &paths, |p| {
            if let Progress::Sending(f, _) = p {
                fractions.push(f);
            }
        })
        .unwrap();
        let seen = server.join().unwrap();
        assert_eq!(n, 2, "the missing file and the folder are left out");
        assert_eq!(seen.len(), 3);
        assert!(seen[0].0.starts_with("POST /api/localsend/v2/prepare-upload"));
        let ask: serde_json::Value = serde_json::from_slice(&seen[0].1).unwrap();
        assert_eq!(ask["files"]["f1"]["fileName"], "b.png");
        assert_eq!(ask["files"]["f1"]["fileType"], "image/png");
        assert!(seen[1].0.contains("sessionId=S") && seen[1].0.contains("fileId=f0") && seen[1].0.contains("token=T0"));
        assert_eq!(seen[1].1, b"hello");
        assert_eq!(seen[2].1.len(), 300_000);
        assert_eq!(fractions.last().copied(), Some(1.0));
        assert!(fractions.windows(2).all(|w| w[0] <= w[1]), "progress never goes back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_declined_transfer_says_so() {
        let dir = std::env::temp_dir().join(format!("oi-send-no-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = files(&dir);
        let (port, server) = receiver(false);
        let e = send_files(&peer(port), &paths, |_| {}).unwrap_err();
        assert_eq!(e, "Pixel declined");
        server.join().unwrap();
        assert!(send_files(&peer(1), &[], |_| {}).is_err(), "nothing to send");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_device_that_answers_is_found() {
        assert!(probe(Ipv4Addr::LOCALHOST, 1, Duration::from_millis(100)).is_none(), "nothing listens on port 1");
    }
}
