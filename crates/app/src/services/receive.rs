//! Receiving files from other devices with the LocalSend protocol (see island_core::localsend): an https server on
//! LocalSend's port, the multicast messages that let the others find this machine, and the writing of what they send.
//! Option `receive_files`, off by default.
//!
//! What it accepts: only devices of the local network (or the tailnet), one transfer at a time, and nothing is written
//! before the person says yes on the card. A file goes into one folder under a name the island makes safe, never over
//! another file, and never longer than announced.

use super::{net, Reporter};
use island_core::localsend::{self as ls, Offer, Offered};
use island_core::progress::{Activity, State};
use island_ui::model::{offer_card, Command, Input};
use ring::rand::SecureRandom;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long the card waits for an answer.
const ANSWER_TIME: Duration = Duration::from_secs(60);
/// A transfer in which nothing happens for this long is over.
const IDLE: Duration = Duration::from_secs(60);
/// After a no, how long the same device is not asked about again.
const QUIET_AFTER_NO: Duration = Duration::from_secs(20);
const MAX_CONNECTIONS: usize = 16;
const MAX_HEAD: u64 = 16 * 1024;
const MAX_BODY: u64 = 1024 * 1024;

type Tls = BufReader<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Answer {
    Yes,
    No,
    /// The sender gave up while the card was up.
    Withdrawn,
}

/// The offers whose card is waiting for an answer.
static ASKED: Mutex<Vec<(u32, Sender<Answer>)>> = Mutex::new(Vec::new());
static NEXT_OFFER: AtomicU32 = AtomicU32::new(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the person said on the card of an offer. A second answer to the same one changes nothing.
pub fn answer(offer: u32, accept: bool) {
    reply(offer, if accept { Answer::Yes } else { Answer::No });
}

fn reply(offer: u32, with: Answer) {
    let mut asked = lock(&ASKED);
    if let Some(i) = asked.iter().position(|(id, _)| *id == offer) {
        let _ = asked.remove(i).1.send(with);
    }
}

// ---- who this machine is

/// The certificate of this machine and its key, kept so that the others recognise it the next time.
pub struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
}

impl Identity {
    fn generate() -> Result<Self, String> {
        let made = rcgen::generate_simple_self_signed(vec!["open-island".to_string()]).map_err(|e| e.to_string())?;
        Ok(Self { cert: made.cert.der().to_vec(), key: made.signing_key.serialize_der() })
    }

    /// The one kept in these files, or a new one written to them (the key for the owner's eyes only).
    fn load_or_make(cert: &Path, key: &Path) -> Result<Self, String> {
        if let (Ok(c), Ok(k)) = (std::fs::read(cert), std::fs::read(key)) {
            let id = Self { cert: c, key: k };
            if id.tls().is_ok() {
                return Ok(id);
            }
        }
        let id = Self::generate()?;
        for (path, data) in [(cert, &id.cert), (key, &id.key)] {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::remove_file(path);
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).and_then(|mut f| f.write_all(data)).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(id)
    }

    /// In https the fingerprint a device announces is the SHA-256 of its certificate.
    fn fingerprint(&self) -> String {
        hex(ring::digest::digest(&ring::digest::SHA256, &self.cert).as_ref()).to_ascii_uppercase()
    }

    fn tls(&self) -> Result<Arc<rustls::ServerConfig>, String> {
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(self.cert.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())))
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(config))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A word nobody can guess, for sessions and the files in them.
fn secret() -> Option<String> {
    let mut b = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut b).ok()?;
    Some(hex(&b))
}

/// Devices of the local network and of the tailnet: nobody from further away is listened to.
fn nearby(ip: IpAddr) -> bool {
    let IpAddr::V4(a) = ip else { return false };
    let [x, y, ..] = a.octets();
    a.is_private() || a.is_loopback() || a.is_link_local() || (x == 100 && (64..128).contains(&y))
}

// ---- the server

#[derive(PartialEq)]
enum Step {
    Waiting,
    Receiving,
    Done,
}

struct Slot {
    token: String,
    file: Offered,
    step: Step,
}

/// A transfer that was accepted.
struct Session {
    id: String,
    ip: IpAddr,
    from: String,
    folder: PathBuf,
    files: HashMap<String, Slot>,
    total: u64,
    received: u64,
    /// Uploads running now.
    busy: u32,
    last: Instant,
    shown: Instant,
    saved: Vec<PathBuf>,
}

#[derive(Default)]
struct Shared {
    /// The offer on a card, and who made it.
    asking: Option<(u32, IpAddr)>,
    session: Option<Session>,
    refused: Option<(IpAddr, Instant)>,
    /// Receiving was switched off: a connection still open is not listened to.
    closed: bool,
}

pub struct Server {
    rep: Reporter,
    tls: Arc<rustls::ServerConfig>,
    alias: String,
    fingerprint: String,
    /// Where files go, asked at each offer (the setting may change).
    folder: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    answer_time: Duration,
    shared: Mutex<Shared>,
    connections: AtomicUsize,
}

/// A status and a body to answer with.
type Reply = (u16, String);

fn status(code: u16) -> Reply {
    (code, String::new())
}

fn activity(label: String, detail: String, frac: Option<f64>, file: Option<String>) -> Activity {
    Activity { id: "receive".into(), label, detail, icon: Some("download".into()), tint: None, frac, file, busy: false }
}

impl Server {
    pub fn new(rep: Reporter, identity: &Identity, alias: String, folder: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>) -> Result<Arc<Self>, String> {
        Self::with_answer_time(rep, identity, alias, folder, ANSWER_TIME)
    }

    fn with_answer_time(rep: Reporter, identity: &Identity, alias: String, folder: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>, answer_time: Duration) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self { rep, tls: identity.tls()?, alias, fingerprint: identity.fingerprint(), folder, answer_time, shared: Mutex::default(), connections: AtomicUsize::new(0) }))
    }

    fn info(&self) -> serde_json::Value {
        ls::own_info(&self.alias, &self.fingerprint)
    }

    /// Takes a connection on a thread of its own (dropped at once when there are already too many).
    pub fn accept(self: &Arc<Self>, stream: TcpStream, from: SocketAddr) {
        if !nearby(from.ip()) {
            return;
        }
        if self.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            self.connections.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        let server = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = server.serve(stream, from.ip()) {
                log::debug!("receive: {}: {e}", from.ip());
            }
            server.connections.fetch_sub(1, Ordering::SeqCst);
        });
    }

    /// Answers the requests of one connection until it ends.
    fn serve(&self, stream: TcpStream, ip: IpAddr) -> std::io::Result<()> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        let conn = rustls::ServerConnection::new(self.tls.clone()).map_err(std::io::Error::other)?;
        let mut tls: Tls = BufReader::with_capacity(64 * 1024, rustls::StreamOwned::new(conn, stream));
        while let Some(head) = read_head(&mut tls)? {
            if head.expects_continue {
                tls.get_mut().write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
                tls.get_mut().flush()?;
            }
            let (reply, whole) = self.route(&head, &mut tls, ip);
            // what was not read of a request would be taken for the start of the next one
            let keep = whole && !head.close;
            let (code, body) = reply;
            let out = tls.get_mut();
            write!(out, "HTTP/1.1 {code} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}\r\n{body}", reason(code), body.len(), if keep { "" } else { "Connection: close\r\n" })?;
            out.flush()?;
            if !keep {
                break;
            }
        }
        close(tls.into_inner());
        Ok(())
    }

    /// The answer to a request, and whether its body was read to the end.
    fn route(&self, head: &Head, tls: &mut Tls, ip: IpAddr) -> (Reply, bool) {
        let path = head.path.strip_prefix("/api/localsend/v2/").unwrap_or("");
        if head.method == "POST" && path == "upload" {
            let mut body = Body::new(tls, head);
            let reply = self.upload(&head.query, &mut body, ip);
            let whole = body.finished();
            return (reply, whole);
        }
        // everything else is small
        let mut body = Body::new(tls, head);
        let mut text = String::new();
        if (&mut body).take(MAX_BODY).read_to_string(&mut text).is_err() || !body.finished() {
            return (status(400), false);
        }
        let reply = match (head.method.as_str(), path) {
            ("GET", "info") | ("POST", "register") => (200, self.info().to_string()),
            ("POST", "prepare-upload") => self.prepare(&text, tls, ip),
            ("POST", "cancel") => self.cancel(&head.query, ip),
            _ => status(404),
        };
        (reply, true)
    }

    /// A device asks to send: a card, and what the person says.
    fn prepare(&self, body: &str, tls: &mut Tls, ip: IpAddr) -> Reply {
        let Some(offer) = ls::parse_offer(body) else { return status(400) };
        let id = NEXT_OFFER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        {
            let mut s = lock(&self.shared);
            if s.closed {
                return status(403);
            }
            if s.asking.is_some() || s.session.is_some() {
                return status(409);
            }
            if s.refused.is_some_and(|(who, at)| who == ip && at.elapsed() < QUIET_AFTER_NO) {
                return status(429);
            }
            s.asking = Some((id, ip));
        }
        lock(&ASKED).push((id, tx));
        let message = offer.files.first().and_then(|f| f.message.clone());
        let (summary, text) = match &message {
            Some(m) => (format!("Message from {}", offer.from), m.clone()),
            None => (format!("{} wants to send", offer.from), ls::offer_lines(&offer)),
        };
        self.rep.input(Input::FileOffer { id, summary, body: text, message: message.is_some() });
        let until = Instant::now() + self.answer_time;
        let said = loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(a) => break a,
                Err(RecvTimeoutError::Disconnected) => break Answer::Withdrawn,
                Err(RecvTimeoutError::Timeout) if Instant::now() >= until || gone(tls) => break Answer::Withdrawn,
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        lock(&ASKED).retain(|(n, _)| *n != id);
        lock(&self.shared).asking = None;
        match said {
            Answer::Yes => match message {
                Some(m) => {
                    self.rep.exec(Command::RestoreClipboard(m));
                    status(204)
                }
                None => self.begin(offer, ip),
            },
            Answer::No => {
                lock(&self.shared).refused = Some((ip, Instant::now()));
                status(403)
            }
            Answer::Withdrawn => {
                self.rep.input(Input::NotificationClosed(offer_card(id)));
                status(403)
            }
        }
    }

    /// An accepted offer: a token for each file.
    fn begin(&self, offer: Offer, ip: IpAddr) -> Reply {
        let folder = (self.folder)().filter(|f| std::fs::create_dir_all(f).is_ok());
        let Some(folder) = folder else {
            self.rep.input(Input::Toast { summary: "Cannot receive".into(), body: "The folder for received files cannot be made".into() });
            return status(500);
        };
        let Some(id) = secret() else { return status(500) };
        let total = offer.files.iter().fold(0u64, |sum, f| sum.saturating_add(f.size));
        let mut files = HashMap::new();
        let mut tokens = serde_json::Map::new();
        for file in offer.files {
            let Some(token) = secret() else { return status(500) };
            tokens.insert(file.id.clone(), token.clone().into());
            files.insert(file.id.clone(), Slot { token, file, step: Step::Waiting });
        }
        let now = Instant::now();
        let reply = serde_json::json!({ "sessionId": id, "files": tokens }).to_string();
        self.rep.input(Input::Activity(State::Run, activity(format!("Receiving from {}", offer.from), String::new(), Some(0.0), None)));
        lock(&self.shared).session = Some(Session { id, ip, from: offer.from, folder, files, total, received: 0, busy: 0, last: now, shown: now, saved: vec![] });
        (200, reply)
    }

    /// One file of the accepted transfer.
    fn upload(&self, query: &str, body: &mut impl Read, ip: IpAddr) -> Reply {
        let (Some(session), Some(file_id), Some(token)) = (ls::query_value(query, "sessionId"), ls::query_value(query, "fileId"), ls::query_value(query, "token")) else { return status(400) };
        let (file, folder) = {
            let mut s = lock(&self.shared);
            let Some(s) = s.session.as_mut().filter(|s| s.id == session && s.ip == ip) else { return status(403) };
            let Some(slot) = s.files.get_mut(&file_id).filter(|f| f.token == token) else { return status(403) };
            if slot.step != Step::Waiting {
                return status(409);
            }
            slot.step = Step::Receiving;
            s.busy += 1;
            s.last = Instant::now();
            (slot.file.clone(), s.folder.clone())
        };
        let partial = folder.join(format!(".{token}.incoming"));
        let result = self.store(body, &file, &partial, &session).and_then(|()| keep(&partial, &folder, &ls::safe_name(&file.name)).map_err(|e| (500, e.to_string())));
        match result {
            Ok(path) => {
                let mut shared = lock(&self.shared);
                let Some(s) = shared.session.as_mut().filter(|s| s.id == session) else { return status(200) };
                s.busy -= 1;
                s.last = Instant::now();
                s.saved.push(path);
                if let Some(slot) = s.files.get_mut(&file_id) {
                    slot.step = Step::Done;
                }
                if s.files.values().all(|f| f.step == Step::Done) {
                    if let Some(s) = shared.session.take() {
                        self.end(s, None);
                    }
                }
                status(200)
            }
            Err((code, why)) => {
                let _ = std::fs::remove_file(&partial);
                let mut shared = lock(&self.shared);
                if shared.session.as_ref().is_some_and(|s| s.id == session) {
                    if let Some(s) = shared.session.take() {
                        self.end(s, Some(format!("{}: {why}", ls::safe_name(&file.name))));
                    }
                }
                status(code)
            }
        }
    }

    /// Writes the body of an upload to `partial`: exactly as long as announced, and matching its checksum if it has one.
    fn store(&self, body: &mut impl Read, file: &Offered, partial: &Path, session: &str) -> Result<(), (u16, String)> {
        let disk = |e: std::io::Error| (500, e.to_string());
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(partial).map_err(disk)?;
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            let n = body.read(&mut buf).map_err(|_| (500, "the connection broke".to_string()))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > file.size {
                return Err((400, "longer than announced".into()));
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(disk)?;
            let mut shared = lock(&self.shared);
            let Some(s) = shared.session.as_mut().filter(|s| s.id == session) else { return Err((409, "cancelled".into())) };
            s.received += n as u64;
            s.last = Instant::now();
            if s.shown.elapsed() > Duration::from_millis(100) {
                s.shown = Instant::now();
                let frac = s.received as f64 / s.total.max(1) as f64;
                self.rep.input(Input::Activity(State::Run, activity(format!("Receiving from {}", s.from), ls::safe_name(&file.name), Some(frac.min(1.0)), None)));
            }
        }
        if written != file.size {
            return Err((500, "cut short".into()));
        }
        if file.sha256.as_ref().is_some_and(|want| *want != hex(hash.finish().as_ref())) {
            return Err((422, "arrived damaged".into()));
        }
        out.flush().map_err(disk)
    }

    /// The sender takes it back.
    fn cancel(&self, query: &str, ip: IpAddr) -> Reply {
        let named = ls::query_value(query, "sessionId");
        let mut shared = lock(&self.shared);
        if shared.session.as_ref().is_some_and(|s| s.ip == ip && named.as_deref() == Some(s.id.as_str())) {
            if let Some(s) = shared.session.take() {
                self.end(s, Some("cancelled by the sender".into()));
            }
        } else if let Some((offer, _)) = shared.asking.filter(|(_, who)| *who == ip) {
            reply(offer, Answer::Withdrawn);
        }
        status(200)
    }

    /// Tells how a transfer ended; what arrived goes on the shelf.
    fn end(&self, s: Session, failed: Option<String>) {
        let saved: Vec<String> = s.saved.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if !saved.is_empty() {
            self.rep.input(Input::ShelveQuietly(saved.clone()));
        }
        match failed {
            Some(why) => self.rep.input(Input::Activity(State::Fail, activity("Receive failed".into(), why, Some(1.0), None))),
            None => {
                let name = |p: &PathBuf| p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                let (what, file) = match s.saved.as_slice() {
                    [one] => (name(one), saved.first().cloned()),
                    // a folder ends in a slash: its card only shows it
                    many => (format!("{} files", many.len()), Some(format!("{}/", s.folder.to_string_lossy().trim_end_matches('/')))),
                };
                self.rep.input(Input::Activity(State::Done, activity(format!("Received from {}", s.from), what, Some(1.0), file)));
            }
        }
    }

    /// Ends a transfer in which nothing has happened for a while.
    pub fn sweep(&self, idle: Duration) {
        let mut shared = lock(&self.shared);
        if shared.session.as_ref().is_some_and(|s| s.busy == 0 && s.last.elapsed() >= idle) {
            if let Some(s) = shared.session.take() {
                self.end(s, Some("the sender stopped".into()));
            }
        }
    }

    /// Receiving is switched off: what is going on ends.
    fn close(&self) {
        let mut shared = lock(&self.shared);
        shared.closed = true;
        if let Some((offer, _)) = shared.asking {
            reply(offer, Answer::Withdrawn);
        }
        if let Some(s) = shared.session.take() {
            self.end(s, Some("receiving was switched off".into()));
        }
    }
}

/// Gives the finished file its name, taking another one when that is taken: nothing is ever written over.
fn keep(partial: &Path, folder: &Path, name: &str) -> std::io::Result<PathBuf> {
    for n in 1..10_000 {
        let path = folder.join(ls::numbered(name, n));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => {
                return std::fs::rename(partial, &path).map(|()| path.clone()).inspect_err(|_| {
                    let _ = std::fs::remove_file(&path);
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many files of this name"))
}

/// Ends a connection so that the last answer arrives: closing with bytes still unread (the rest of a refused upload)
/// resets it, so they are read and thrown away first, within limits.
fn close(mut s: rustls::StreamOwned<rustls::ServerConnection, TcpStream>) {
    s.conn.send_close_notify();
    let _ = s.flush();
    let _ = s.sock.shutdown(std::net::Shutdown::Write);
    let _ = s.sock.set_read_timeout(Some(Duration::from_secs(1)));
    let until = Instant::now() + Duration::from_secs(3);
    let mut sink = [0u8; 16 * 1024];
    let mut left = 8 * 1024 * 1024usize;
    while left > 0 && Instant::now() < until {
        match s.sock.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

/// Whether the other end has closed the connection while it waits for an answer.
fn gone(tls: &mut Tls) -> bool {
    let s = tls.get_mut();
    if s.sock.set_nonblocking(true).is_err() {
        return false;
    }
    let closed = match s.conn.read_tls(&mut s.sock) {
        Ok(0) => true,
        Ok(_) => s.conn.process_new_packets().map_or(true, |state| state.peer_has_closed()),
        Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
    };
    let _ = s.sock.set_nonblocking(false);
    closed
}

// ---- as much of HTTP/1.1 as the protocol needs

struct Head {
    method: String,
    path: String,
    query: String,
    length: u64,
    chunked: bool,
    expects_continue: bool,
    close: bool,
}

/// A line without its end; None when the connection ends before one.
fn read_line(r: &mut impl BufRead, left: &mut u64) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    let n = r.take(*left).read_until(b'\n', &mut line)?;
    *left -= n as u64;
    if !line.ends_with(b"\n") {
        return if n == 0 && *left > 0 { Ok(None) } else { Err(std::io::ErrorKind::InvalidData.into()) };
    }
    Ok(Some(String::from_utf8_lossy(&line).trim_end().to_string()))
}

/// The request line and headers; None when the connection is over.
fn read_head(r: &mut impl BufRead) -> std::io::Result<Option<Head>> {
    let mut left = MAX_HEAD;
    // an idle connection that is dropped is not an error
    let first = match read_line(r, &mut left) {
        Ok(Some(l)) => l,
        Ok(None) => return Ok(None),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::UnexpectedEof) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut words = first.split(' ');
    let (Some(method), Some(target)) = (words.next(), words.next()) else { return Err(std::io::ErrorKind::InvalidData.into()) };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut head = Head { method: method.to_string(), path: path.to_string(), query: query.to_string(), length: 0, chunked: false, expects_continue: false, close: false };
    loop {
        let line = read_line(r, &mut left)?.ok_or(std::io::ErrorKind::UnexpectedEof)?;
        if line.is_empty() {
            return Ok(Some(head));
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_ascii_lowercase();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => head.length = value.parse().map_err(|_| std::io::ErrorKind::InvalidData)?,
            "transfer-encoding" => head.chunked = value.contains("chunked"),
            "expect" => head.expects_continue = value.contains("100-continue"),
            "connection" => head.close = value.contains("close"),
            _ => {}
        }
    }
}

/// The body of a request, whether its length is given or it comes in chunks.
struct Body<'a, R: BufRead> {
    r: &'a mut R,
    /// Bytes left of the body, or of the chunk being read.
    left: u64,
    chunked: bool,
    done: bool,
}

impl<'a, R: BufRead> Body<'a, R> {
    fn new(r: &'a mut R, head: &Head) -> Self {
        let left = if head.chunked { 0 } else { head.length };
        Self { r, left, chunked: head.chunked, done: !head.chunked && head.length == 0 }
    }

    fn finished(&self) -> bool {
        self.done
    }

    /// The size line of the next chunk; the last one (0) is followed by trailers and an empty line.
    fn next_chunk(&mut self) -> std::io::Result<()> {
        let mut room = MAX_HEAD;
        let line = read_line(self.r, &mut room)?.ok_or(std::io::ErrorKind::UnexpectedEof)?;
        let size = line.split(';').next().unwrap_or("").trim();
        self.left = u64::from_str_radix(size, 16).map_err(|_| std::io::ErrorKind::InvalidData)?;
        if self.left == 0 {
            while !read_line(self.r, &mut room)?.ok_or(std::io::ErrorKind::UnexpectedEof)?.is_empty() {}
            self.done = true;
        }
        Ok(())
    }
}

impl<R: BufRead> Read for Body<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            self.next_chunk()?;
            if self.done {
                return Ok(0);
            }
        }
        let max = (buf.len() as u64).min(self.left) as usize;
        let n = self.r.read(&mut buf[..max])?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n as u64;
        if self.left == 0 {
            if self.chunked {
                let mut room = 2;
                read_line(self.r, &mut room)?;
            } else {
                self.done = true;
            }
        }
        Ok(n)
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    }
}

// ---- being found

const GROUP: Ipv4Addr = Ipv4Addr::new(ls::MULTICAST[0], ls::MULTICAST[1], ls::MULTICAST[2], ls::MULTICAST[3]);

/// The socket of the multicast messages, joined on the network of this address.
fn multicast(on: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ls::PORT))?;
    s.join_multicast_v4(&GROUP, &on)?;
    s.set_nonblocking(true)?;
    Ok(s)
}

impl Server {
    fn announce(&self, udp: &UdpSocket, wants_answer: bool) {
        let _ = udp.send_to(ls::announcement(&self.info(), wants_answer).to_string().as_bytes(), (GROUP, ls::PORT));
    }

    /// Another device says it is there: it is told that this one is too.
    fn heard(self: &Arc<Self>, udp: &UdpSocket, text: &[u8], from: IpAddr, answered: &mut HashMap<IpAddr, Instant>) {
        if !nearby(from) {
            return;
        }
        let Some((peer, fingerprint, wants)) = std::str::from_utf8(text).ok().and_then(|t| ls::parse_announcement(&from.to_string(), t)) else { return };
        if !wants || fingerprint.eq_ignore_ascii_case(&self.fingerprint) || answered.get(&from).is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
            return;
        }
        answered.retain(|_, at| at.elapsed() < Duration::from_secs(2));
        if answered.len() >= 32 {
            return;
        }
        answered.insert(from, Instant::now());
        let (server, udp) = (self.clone(), udp.try_clone());
        std::thread::spawn(move || {
            let url = format!("{}://{}:{}/api/localsend/v2/register", if peer.https { "https" } else { "http" }, peer.ip, peer.port);
            let told = net::lan_agent(Duration::from_secs(3)).post(&url).header("Content-Type", "application/json").send(server.info().to_string()).is_ok_and(|r| r.status().is_success());
            if let (false, Ok(udp)) = (told, udp) {
                server.announce(&udp, false);
            }
        });
    }
}

/// Waits until one of the sockets has something, or `ms` have passed.
fn wait_readable(fds: &[i32], ms: i32) {
    let mut set: Vec<libc::pollfd> = fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    // SAFETY: `set` is a valid array of `set.len()` entries for the length of the call
    unsafe { libc::poll(set.as_mut_ptr(), set.len() as libc::nfds_t, ms) };
}

fn wanted() -> bool {
    crate::config::load().ui.receive_files
}

/// Where received files go: `receive_dir`, else the download folder.
pub fn folder() -> Option<PathBuf> {
    crate::config::load().receive_dir.or_else(super::downloads::folder)
}

/// Listens while the option is on.
pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut told_busy = false;
        loop {
            if !wanted() {
                told_busy = false;
                super::wait_for(3600, &[&super::CONFIG_GEN]);
                continue;
            }
            net::init_tls();
            let server = super::state_file("localsend.cert")
                .zip(super::state_file("localsend.key"))
                .ok_or_else(|| "no state folder".to_string())
                .and_then(|(c, k)| Identity::load_or_make(&c, &k))
                .and_then(|id| Server::new(rep.clone(), &id, super::send::alias(), Box::new(folder)));
            let server = match server {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("receive: {e}");
                    super::wait_for(3600, &[&super::CONFIG_GEN]);
                    continue;
                }
            };
            let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, ls::PORT)).and_then(|l| l.set_nonblocking(true).map(|()| l)) {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("receive: port {}: {e}", ls::PORT);
                    if !told_busy {
                        told_busy = true;
                        rep.input(Input::Toast { summary: "Cannot receive files".into(), body: format!("Port {} is taken: is LocalSend open?", ls::PORT) });
                    }
                    super::wait_for(30, &[&super::CONFIG_GEN]);
                    continue;
                }
            };
            told_busy = false;
            run(&server, &listener);
            server.close();
        }
    });
}

/// Serves until the option is switched off.
fn run(server: &Arc<Server>, listener: &TcpListener) {
    let mut udp: Option<(Ipv4Addr, UdpSocket)> = None;
    let (mut looked, mut joined, mut greetings) = (None::<Instant>, Instant::now(), 0);
    let mut answered = HashMap::new();
    let mut config = super::CONFIG_GEN.load(Ordering::Relaxed);
    loop {
        // the network may have changed (or not been there at all): the group is joined again on the one there is
        if looked.is_none_or(|at| at.elapsed() > Duration::from_secs(15)) {
            looked = Some(Instant::now());
            let ip = super::send::local_ip();
            if udp.as_ref().map(|(a, _)| *a) != ip {
                // the old socket goes first: it holds the port
                drop(udp.take());
                udp = ip.and_then(|ip| multicast(ip).map_err(|e| log::debug!("receive: multicast: {e}")).ok().map(|s| (ip, s)));
                (joined, greetings) = (Instant::now(), 0);
            }
        }
        // said a few times, since a datagram may be lost
        if let Some((_, s)) = &udp {
            if greetings < 3 && joined.elapsed() >= Duration::from_secs([0, 1, 4][greetings]) {
                server.announce(s, true);
                greetings += 1;
            }
        }
        let mut fds = vec![listener.as_raw_fd()];
        fds.extend(udp.as_ref().map(|(_, s)| s.as_raw_fd()));
        wait_readable(&fds, 1000);
        while let Ok((stream, from)) = listener.accept() {
            server.accept(stream, from);
        }
        if let Some((_, s)) = &udp {
            let mut buf = [0u8; 4096];
            // a bounded number a turn, so that a flood cannot keep the thread from the connections
            for _ in 0..32 {
                let Ok((n, from)) = s.recv_from(&mut buf) else { break };
                server.heard(s, &buf[..n], from.ip(), &mut answered);
            }
        }
        server.sweep(IDLE);
        let now = super::CONFIG_GEN.load(Ordering::Relaxed);
        if now != config {
            config = now;
            if !wanted() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{send, Msg};
    use super::*;
    use island_core::localsend::Peer;
    use std::sync::mpsc::Receiver;

    struct Bench {
        server: Arc<Server>,
        msgs: Mutex<Receiver<Msg>>,
        port: u16,
        dir: PathBuf,
    }

    impl Bench {
        fn new(name: &str, answer_time: Duration) -> Self {
            net::init_tls();
            let dir = std::env::temp_dir().join(format!("oi-receive-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("out")).unwrap();
            let (tx, msgs) = channel();
            let rep = Reporter { tx, waker: Arc::new(|| {}) };
            let id = Identity::load_or_make(&dir.join("cert"), &dir.join("key")).unwrap();
            let into = dir.join("in");
            let server = Server::with_answer_time(rep, &id, "laptop".into(), Box::new(move || Some(into.clone())), answer_time).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let s = server.clone();
            std::thread::spawn(move || {
                while let Ok((stream, from)) = listener.accept() {
                    s.accept(stream, from);
                }
            });
            Self { server, msgs: Mutex::new(msgs), port, dir }
        }

        fn peer(&self) -> Peer {
            Peer { alias: "laptop".into(), ip: "127.0.0.1".into(), port: self.port, https: true, model: String::new(), kind: "desktop".into(), taildrop: false }
        }

        /// The next thing the service says that `pick` wants.
        fn next<T>(&self, mut pick: impl FnMut(Msg) -> Option<T>) -> T {
            loop {
                if let Some(t) = pick(lock(&self.msgs).recv_timeout(Duration::from_secs(10)).expect("the service said nothing")) {
                    return t;
                }
            }
        }

        /// The offer on the card.
        fn offer(&self) -> (u32, String, String, bool) {
            self.next(|m| match m {
                Msg::Input(Input::FileOffer { id, summary, body, message }) => Some((id, summary, body, message)),
                _ => None,
            })
        }

        /// How the transfer ended, and what went to the shelf before it.
        fn outcome(&self) -> (State, Activity, Vec<String>) {
            let mut shelf = vec![];
            self.next(|m| match m {
                Msg::Input(Input::ShelveQuietly(p)) => {
                    shelf = p;
                    None
                }
                Msg::Input(Input::Activity(s, a)) if s != State::Run => Some((s, a, shelf.clone())),
                _ => None,
            })
        }

        fn post(&self, path: &str, body: &[u8]) -> (u16, String) {
            let url = format!("https://127.0.0.1:{}/api/localsend/v2/{path}", self.port);
            let mut r = net::lan_agent(Duration::from_secs(10)).post(&url).send(body).unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap_or_default())
        }

        /// Asks to send these files and accepts; the session and the tokens.
        fn accepted(&self, files: &str) -> ls::Session {
            let body = format!(r#"{{"info":{{"alias":"Pixel"}},"files":{files}}}"#);
            std::thread::scope(|scope| {
                let ask = scope.spawn(|| self.post("prepare-upload", body.as_bytes()));
                answer(self.offer().0, true);
                let (code, reply) = ask.join().unwrap();
                assert_eq!(code, 200, "{reply}");
                ls::parse_session(&reply).unwrap()
            })
        }

        fn received(&self) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(self.dir.join("in")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            v.sort();
            v
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn what_the_island_sends_it_can_receive() {
        let b = Bench::new("whole", ANSWER_TIME);
        let out = b.dir.join("out");
        std::fs::write(out.join("a.txt"), b"hello").unwrap();
        std::fs::write(out.join("b.bin"), vec![7u8; 700_000]).unwrap();
        std::fs::create_dir_all(b.dir.join("in")).unwrap();
        std::fs::write(b.dir.join("in/a.txt"), b"mine").unwrap();
        let paths: Vec<String> = ["a.txt", "b.bin"].iter().map(|n| out.join(n).to_string_lossy().into_owned()).collect();
        std::thread::scope(|scope| {
            let sent = scope.spawn(|| send::send_files(&b.peer(), &paths, |_| {}));
            let (id, summary, body, message) = b.offer();
            assert!(summary.ends_with("wants to send") && body.starts_with("2 files · 700 kB") && !message, "{summary} / {body}");
            assert_eq!(b.received(), ["a.txt"], "nothing is written before the yes");
            answer(id, true);
            assert_eq!(sent.join().unwrap(), Ok(2));
        });
        let (state, a, shelf) = b.outcome();
        assert_eq!((state, a.detail.as_str()), (State::Done, "2 files"));
        assert!(a.file.unwrap().ends_with("/in/"), "several files: the card shows their folder");
        assert_eq!(shelf.len(), 2);
        assert_eq!(b.received(), ["a (2).txt", "a.txt", "b.bin"], "nothing written over, no part left");
        assert_eq!(std::fs::read(b.dir.join("in/a.txt")).unwrap(), b"mine");
        assert_eq!(std::fs::read(b.dir.join("in/a (2).txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(b.dir.join("in/b.bin")).unwrap().len(), 700_000);
    }

    #[test]
    fn a_no_writes_nothing_and_is_not_asked_again_at_once() {
        let b = Bench::new("no", ANSWER_TIME);
        let file = b.dir.join("out/a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let paths = vec![file.to_string_lossy().into_owned()];
        std::thread::scope(|scope| {
            let sent = scope.spawn(|| send::send_files(&b.peer(), &paths, |_| {}));
            let id = b.offer().0;
            answer(id, false);
            answer(id, true);
            assert_eq!(sent.join().unwrap(), Err("laptop declined".to_string()));
        });
        assert!(b.received().is_empty());
        assert_eq!(send::send_files(&b.peer(), &paths, |_| {}), Err("laptop too many requests".to_string()));
    }

    #[test]
    fn nobody_answering_takes_the_card_back() {
        let b = Bench::new("silent", Duration::from_millis(300));
        let (code, _) = b.post("prepare-upload", br#"{"info":{"alias":"Pixel"},"files":{"f":{"fileName":"a","size":1}}}"#);
        assert_eq!(code, 403);
        let id = b.offer().0;
        let closed = b.next(|m| match m {
            Msg::Input(Input::NotificationClosed(id)) => Some(id),
            _ => None,
        });
        assert_eq!(closed, offer_card(id));
        assert_eq!(b.post("prepare-upload", b"nonsense").0, 400);
        assert_eq!(b.post("nothing", b"").0, 404);
    }

    #[test]
    fn a_sender_that_hangs_up_takes_the_card_back() {
        let b = Bench::new("hangup", ANSWER_TIME);
        let url = format!("https://127.0.0.1:{}/api/localsend/v2/prepare-upload", b.port);
        let impatient = std::thread::spawn(move || net::lan_agent(Duration::from_millis(600)).post(&url).send(&br#"{"info":{"alias":"Pixel"},"files":{"f":{"fileName":"a","size":1}}}"#[..]).is_err());
        let id = b.offer().0;
        let closed = b.next(|m| match m {
            Msg::Input(Input::NotificationClosed(id)) => Some(id),
            _ => None,
        });
        assert_eq!(closed, offer_card(id));
        assert!(impatient.join().unwrap());
    }

    #[test]
    fn a_message_is_copied_and_nothing_is_written() {
        let b = Bench::new("message", ANSWER_TIME);
        std::thread::scope(|scope| {
            let ask = scope.spawn(|| b.post("prepare-upload", br#"{"info":{"alias":"Pixel"},"files":{"t":{"fileName":"t.txt","size":5,"fileType":"text/plain","preview":"hello"}}}"#));
            let (id, summary, body, message) = b.offer();
            assert_eq!((summary.as_str(), body.as_str(), message), ("Message from Pixel", "hello", true));
            answer(id, true);
            assert_eq!(ask.join().unwrap().0, 204);
        });
        let copied = b.next(|m| match m {
            Msg::Exec(Command::RestoreClipboard(t)) => Some(t),
            _ => None,
        });
        assert_eq!(copied, "hello");
        assert!(b.received().is_empty());
    }

    #[test]
    fn only_the_announced_bytes_with_the_right_token_are_kept() {
        let b = Bench::new("strict", ANSWER_TIME);
        let s = b.accepted(r#"{"f":{"fileName":"../../x.bin","size":4}}"#);
        let token = s.tokens[0].1.clone();
        assert_eq!(b.post(&format!("upload?sessionId={}&fileId=f&token=guess", s.id), b"abcd").0, 403);
        assert_eq!(b.post(&format!("upload?sessionId=other&fileId=f&token={token}"), b"abcd").0, 403);
        assert_eq!(b.post("upload?fileId=f", b"abcd").0, 400);
        // a second offer waits for this one to be over
        assert_eq!(b.post("prepare-upload", br#"{"info":{"alias":"Other"},"files":{"f":{"fileName":"a","size":1}}}"#).0, 409);
        assert_eq!(b.post(&format!("upload?sessionId={}&fileId=f&token={token}", s.id), b"abcdefgh").0, 400);
        let (state, a, shelf) = b.outcome();
        assert_eq!((state, a.detail.as_str()), (State::Fail, "x.bin: longer than announced"));
        assert!(shelf.is_empty() && b.received().is_empty(), "{:?}", b.received());

        let s = b.accepted(r#"{"f":{"fileName":"short","size":9}}"#);
        assert_eq!(b.post(&format!("upload?sessionId={}&fileId=f&token={}", s.id, s.tokens[0].1), b"abc").0, 500);
        assert_eq!(b.outcome().1.detail, "short: cut short");

        let s = b.accepted(&format!(r#"{{"f":{{"fileName":"sum","size":3,"sha256":"{}"}}}}"#, "0".repeat(64)));
        assert_eq!(b.post(&format!("upload?sessionId={}&fileId=f&token={}", s.id, s.tokens[0].1), b"abc").0, 422);
        assert_eq!(b.outcome().0, State::Fail);
        assert!(b.received().is_empty());

        let sum = hex(ring::digest::digest(&ring::digest::SHA256, b"abc").as_ref()).to_uppercase();
        let s = b.accepted(&format!(r#"{{"f":{{"fileName":"sum","size":3,"sha256":"{sum}"}}}}"#));
        assert_eq!(b.post(&format!("upload?sessionId={}&fileId=f&token={}", s.id, s.tokens[0].1), b"abc").0, 200);
        let (state, a, _) = b.outcome();
        assert_eq!((state, a.detail.as_str()), (State::Done, "sum"));
        assert!(a.file.unwrap().ends_with("/in/sum"));
    }

    #[test]
    fn a_transfer_ends_when_the_sender_cancels_or_goes_quiet() {
        let b = Bench::new("cancel", ANSWER_TIME);
        let s = b.accepted(r#"{"f":{"fileName":"a","size":4}}"#);
        assert_eq!(b.post("cancel?sessionId=wrong", b"").0, 200);
        b.server.sweep(IDLE);
        assert_eq!(b.post(&format!("cancel?sessionId={}", s.id), b"").0, 200);
        assert_eq!(b.outcome().1.detail, "cancelled by the sender");
        b.accepted(r#"{"f":{"fileName":"a","size":4}}"#);
        b.server.sweep(Duration::ZERO);
        assert_eq!(b.outcome().1.detail, "the sender stopped");
    }

    #[test]
    fn once_switched_off_nothing_more_is_asked() {
        let b = Bench::new("off", Duration::from_millis(300));
        b.server.close();
        assert_eq!(b.post("prepare-upload", br#"{"info":{"alias":"Pixel"},"files":{"f":{"fileName":"a","size":1}}}"#).0, 403);
        assert!(lock(&b.msgs).try_recv().is_err(), "no card");
    }

    #[test]
    fn a_body_in_chunks_is_put_back_together() {
        let head = |chunked, length| Head { method: "POST".into(), path: String::new(), query: String::new(), length, chunked, expects_continue: false, close: false };
        let read = |data: &[u8], chunked, length| {
            let mut r = BufReader::new(data);
            let mut body = Body::new(&mut r, &head(chunked, length));
            let mut out = vec![];
            let ok = body.read_to_end(&mut out).is_ok() && body.finished();
            let mut rest = vec![];
            r.read_to_end(&mut rest).unwrap();
            (ok, out, rest)
        };
        assert_eq!(read(b"4;x=y\r\nabcd\r\n1\r\ne\r\n0\r\nT: 1\r\n\r\nNEXT", true, 0), (true, b"abcde".to_vec(), b"NEXT".to_vec()));
        assert_eq!(read(b"abcdNEXT", false, 4), (true, b"abcd".to_vec(), b"NEXT".to_vec()));
        assert!(read(b"", false, 0).0);
        assert!(!read(b"ab", false, 4).0, "cut short");
        assert!(!read(b"zz\r\n", true, 0).0);
        assert!(!read(b"4\r\nab", true, 0).0);
    }

    #[test]
    fn only_the_neighbourhood_is_listened_to() {
        for ip in ["127.0.0.1", "192.168.1.9", "10.0.0.2", "172.20.1.1", "169.254.3.3", "100.101.102.103"] {
            assert!(nearby(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "100.200.0.1", "::1"] {
            assert!(!nearby(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn the_identity_stays_the_same() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oi-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (c, k) = (dir.join("cert"), dir.join("key"));
        let first = Identity::load_or_make(&c, &k).unwrap().fingerprint();
        assert_eq!(Identity::load_or_make(&c, &k).unwrap().fingerprint(), first);
        assert_eq!(first.len(), 64);
        assert_eq!(std::fs::metadata(&k).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::write(&k, b"broken").unwrap();
        assert_ne!(Identity::load_or_make(&c, &k).unwrap().fingerprint(), first, "a spoilt one is made again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
