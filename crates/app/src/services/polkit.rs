//! The island as the session's polkit agent: what asks for an administrator's password when a program needs one.
//! Off unless `"polkit_agent": true` is in the settings file (the options page and the control socket cannot switch it on).
//!
//! What the island does and does not do:
//! * A face check (Gaze, when polkit's PAM stack has it) happens inside the same helper: the island only shows where it is
//!   and passes on the person's yes. See `polkit_talk` and `polkit_face`.
//! * It never checks a password and never stores one. The password goes to polkit's own helper (a root service behind
//!   `/run/polkit/agent-helper.socket`), which runs PAM and tells polkitd. The island checks that the other end of that
//!   socket is root before it writes anything, and writes only when the person confirms.
//! * Requests are taken only from the connection that owns polkit's name on the system bus (the bus policy already lets
//!   no one else call; this checks it again).
//! * It runs only where the keyboard can be taken exclusively (the Wayland layer-shell backend), so that what is typed
//!   cannot land in another window.
//! * While it is on, the process cannot be traced or dumped by other programs of the same user.
//! * It cannot protect from a program that already runs as the same user and imitates the request, replaces the island's
//!   binary or registers an agent of its own: no agent can.

use super::polkit_talk::{Act, Talk, Tell};
use super::{avatar, peer, polkit_face, Reporter};
use island_core::secret::{Secret, MAX_SECRET};
use island_ui::model::{AuthFaceUse, AuthRequest, AuthUpdate, Command, Input};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::Message;
use zeroize::Zeroize;

const PATH: &str = "/org/openisland/PolkitAgent";
const AGENT: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
const POLKIT: &str = "org.freedesktop.PolicyKit1";
pub const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
/// How many wrong passwords end a request (the system may lock an account after a few).
const TRIES: u32 = 3;
/// How long the helper may stay silent: it waits for the person after a face check, so longer than the island leaves a
/// request open.
const HELPER_PATIENCE: Duration = Duration::from_secs(240);
/// The longest line taken from the helper.
const LINE: u64 = 4096;

static KEYBOARD_EXCLUSIVE: AtomicBool = AtomicBool::new(false);
static ASKED: OnceLock<Sender<Event>> = OnceLock::new();

/// Called by a backend that takes the keyboard for itself while a password is typed, before the services start.
pub fn keyboard_is_exclusive() {
    KEYBOARD_EXCLUSIVE.store(true, Ordering::Relaxed);
}

/// What the island's buttons answer.
pub fn command(c: Command) {
    if let Some(tx) = ASKED.get() {
        let _ = tx.send(Event::Answer(c));
    }
}

enum Event {
    Bus(Message),
    Answer(Command),
    /// A line from the helper of the request `id` (or why there are no more); `session` tells an older attempt from the
    /// current one.
    Helper { id: u64, session: u64, line: Result<String, String> },
}

// ---- the lines of polkit's helper ----------------------------------------------------------------------

/// Undoes the escaping the helper writes its messages with (`g_strescape`).
fn unescape(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes().peekable();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match bytes.next() {
            Some(b'n') => out.push(b'\n'),
            Some(b't') => out.push(b'\t'),
            Some(b'r' | b'f' | b'b' | b'v') => out.push(b' '),
            Some(d @ b'0'..=b'7') => {
                let mut n = u32::from(d - b'0');
                for _ in 0..2 {
                    let Some(d) = bytes.next_if(|d| (b'0'..=b'7').contains(d)) else { break };
                    n = n * 8 + u32::from(d - b'0');
                }
                out.push(n as u8);
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Writes the secret as one line, leaving no copy of it behind.
fn send_secret(to: &mut impl Write, secret: &Secret) -> std::io::Result<()> {
    let mut line = [0u8; MAX_SECRET + 1];
    let n = secret.expose().len();
    line[..n].copy_from_slice(secret.expose());
    line[n] = b'\n';
    let sent = to.write_all(&line[..=n]).and_then(|()| to.flush());
    line.zeroize();
    sent
}

/// Reads the helper's lines until it says how it went, or can say no more: `hear` gets each, unescaped, or why they
/// ended.
fn listen(stream: UnixStream, mut hear: impl FnMut(Result<String, String>)) {
    let mut input = BufReader::new(stream);
    loop {
        let mut line = Vec::new();
        match (&mut input).take(LINE).read_until(b'\n', &mut line) {
            Ok(0) => return hear(Err("polkit's helper closed the conversation".into())),
            Ok(_) if !line.ends_with(b"\n") => return hear(Err("polkit's helper sent a line without an end".into())),
            Ok(_) => {}
            Err(e) => return hear(Err(format!("polkit's helper: {e}"))),
        }
        let line = unescape(String::from_utf8_lossy(&line).trim_end_matches('\n'));
        let last = line == "SUCCESS" || line == "FAILURE";
        hear(Ok(line));
        if last {
            return;
        }
    }
}

/// The helper, only if what answers is root.
fn connect_helper(path: &str) -> Result<UnixStream, String> {
    let stream = UnixStream::connect(path).map_err(|e| format!("polkit's helper cannot be reached: {e}"))?;
    if peer::uid(stream.as_fd()) != Some(0) {
        return Err("what answers on polkit's socket is not the system: nothing was sent".into());
    }
    stream.set_read_timeout(Some(HELPER_PATIENCE)).and_then(|()| stream.set_write_timeout(Some(HELPER_PATIENCE))).map_err(|e| e.to_string())?;
    Ok(stream)
}

// ---- who is asking and who may answer ----------------------------------------------------------------

fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero passwd is a valid value to be filled in
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer refers to a live buffer of the length given
    let r = unsafe { libc::getpwuid_r(uid, &mut entry, buf.as_mut_ptr(), buf.len(), &mut found) };
    if r != 0 || found.is_null() {
        return None;
    }
    // SAFETY: on success `pw_name` is a string ending in zero inside `buf`
    let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }.to_str().ok()?;
    (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.to_string())
}

/// The users among the identities polkit allows, and the one to start with: the person's own when it is among them.
fn users_of(identities: &[(String, HashMap<String, OwnedValue>)], own: u32, name_of: impl Fn(u32) -> Option<String>) -> (Vec<String>, usize) {
    let found: Vec<(u32, String)> = identities
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, d)| d.get("uid").and_then(|v| u32::try_from(v).ok()))
        .filter_map(|uid| Some((uid, name_of(uid)?)))
        .collect();
    let chosen = found.iter().position(|(uid, _)| *uid == own).unwrap_or(0);
    (found.into_iter().map(|(_, n)| n).collect(), chosen)
}

/// "pkexec (pid 12)": the name a process goes by. The name is the program's own claim.
fn requester(details: &HashMap<String, String>) -> String {
    let Some(pid) = details.get("polkit.subject-pid").and_then(|p| p.parse::<u32>().ok()) else { return String::new() };
    match std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        Ok(name) => format!("{} (pid {pid})", name.trim()),
        Err(_) => format!("pid {pid}"),
    }
}

/// A cookie that can be handed to the helper as one line.
fn plain_cookie(cookie: &str) -> bool {
    !cookie.is_empty() && cookie.len() < LINE as usize && !cookie.chars().any(char::is_control)
}

// ---- the agent ----------------------------------------------------------------------------------------

type Begin = (String, String, String, HashMap<String, String>, String, Vec<(String, HashMap<String, OwnedValue>)>);

struct Helper {
    session: u64,
    stream: UnixStream,
    talk: Talk,
}

impl Drop for Helper {
    fn drop(&mut self) {
        // ends the conversation, and the thread that holds it
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

struct Pending {
    /// polkitd's call, answered when the request is over.
    call: Message,
    cookie: String,
    users: Vec<String>,
    /// The one the conversation under way, or the last, was for.
    asked: String,
    tries: u32,
    helper: Option<Helper>,
}

impl Helper {
    /// Writes what the conversation answers; what is for the person comes back.
    fn carry_out(&mut self, acts: Vec<Act>) -> std::io::Result<Vec<Tell>> {
        let mut told = vec![];
        for act in acts {
            match act {
                Act::Tell(t) => told.push(t),
                Act::Password(p) => send_secret(&mut self.stream, &p)?,
                Act::Confirm(word) => self.stream.write_all(format!("{word}\n").as_bytes())?,
            }
        }
        Ok(told)
    }
}

struct Agent {
    conn: Connection,
    rep: Reporter,
    tx: Sender<Event>,
    pending: HashMap<u64, Pending>,
    next: u64,
}

impl Agent {
    fn state(&self, id: u64, update: AuthUpdate) {
        self.rep.input(Input::AuthState { id, update });
    }

    /// Whether a message comes from polkitd.
    fn is_polkit(&self, sender: Option<&str>) -> bool {
        let owner: Result<String, _> = Proxy::new(&self.conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").and_then(|p| p.call("GetNameOwner", &(POLKIT,)));
        matches!((owner, sender), (Ok(o), Some(s)) if o == s)
    }

    fn refuse(&self, call: &Message, error: &str, why: &str) {
        let name = format!("org.freedesktop.PolicyKit1.Error.{error}");
        if let Err(e) = self.conn.reply_error(&call.header(), name.as_str(), &(why,)) {
            log::warn!("polkit: {e}");
        }
    }

    /// Ends a request: polkitd gets its answer and the island closes the prompt.
    fn finish(&mut self, id: u64, outcome: Result<(), (&str, &str)>) {
        let Some(p) = self.pending.remove(&id) else { return };
        match outcome {
            Ok(()) => {
                if let Err(e) = self.conn.reply(&p.call.header(), &()) {
                    log::warn!("polkit: {e}");
                }
            }
            Err((error, why)) => self.refuse(&p.call, error, why),
        }
        self.state(id, AuthUpdate::Gone);
    }

    fn call(&mut self, m: Message) {
        let header = m.header();
        let (path, interface, member) = (header.path().map(|p| p.as_str()), header.interface().map(|i| i.as_str()), header.member().map(|n| n.as_str().to_string()));
        if m.message_type() != Type::MethodCall || path != Some(PATH) || interface != Some(AGENT) {
            return;
        }
        if !self.is_polkit(header.sender().map(|s| s.as_str())) {
            log::warn!("polkit: a request from someone other than polkit was refused");
            return self.refuse(&m, "Failed", "only polkit may ask");
        }
        match member.as_deref() {
            Some("BeginAuthentication") => self.begin(m),
            Some("CancelAuthentication") => {
                let Ok(cookie) = m.body().deserialize::<String>() else { return self.refuse(&m, "Failed", "no cookie") };
                match self.pending.iter().find(|(_, p)| p.cookie == cookie).map(|(id, _)| *id) {
                    Some(id) => {
                        self.finish(id, Err(("Cancelled", "withdrawn")));
                        if let Err(e) = self.conn.reply(&m.header(), &()) {
                            log::warn!("polkit: {e}");
                        }
                    }
                    None => self.refuse(&m, "Failed", "no such request"),
                }
            }
            _ => self.refuse(&m, "Failed", "no such method"),
        }
    }

    fn begin(&mut self, m: Message) {
        let Ok((action, message, _icon, details, cookie, identities)) = m.body().deserialize::<Begin>() else { return self.refuse(&m, "Failed", "the request cannot be read") };
        // SAFETY: getuid cannot fail
        let (users, chosen) = users_of(&identities, unsafe { libc::getuid() }, user_name);
        if users.is_empty() || !plain_cookie(&cookie) {
            return self.refuse(&m, "Failed", "no user can answer this request");
        }
        self.next += 1;
        let id = self.next;
        log::info!("a password is asked for {action}");
        let requester = requester(&details);
        self.pending.insert(id, Pending { call: m, cookie, users: users.clone(), asked: String::new(), tries: TRIES, helper: None });
        // looked at for every request: the choice, the stack and Gaze's settings can change while the island runs
        let face = polkit_face::choose(&crate::config::load().polkit_face, polkit_face::in_stack(), polkit_face::confirms());
        if face != AuthFaceUse::No {
            // off this thread: a Gaze daemon that hangs must not hold up the request. The words in English are read too.
            let conn = self.conn.clone();
            std::thread::spawn(move || tell_gaze(&conn));
        }
        // SAFETY: getuid cannot fail
        let own = user_name(unsafe { libc::getuid() });
        let pictures = users.iter().map(|u| avatar::of(u, own.as_deref() == Some(u.as_str()))).collect();
        self.rep.input(Input::AuthAsk(AuthRequest { id, action, message, requester, users, chosen, pictures, face }));
    }

    fn answer(&mut self, c: Command) {
        match c {
            Command::AuthCancel(id) => self.finish(id, Err(("Cancelled", "dismissed"))),
            Command::AuthSubmit { id, user, secret } => match self.pending.get_mut(&id).map(|p| p.helper.as_mut()) {
                None => self.state(id, AuthUpdate::Gone),
                // a conversation is under way (a face check, or a question of its own): this is for it
                Some(Some(h)) => {
                    let acts = h.talk.typed(secret);
                    self.carry_out(id, acts);
                }
                Some(None) => self.attempt(id, user, Some(secret)),
            },
            Command::AuthFace { id, user } => {
                if self.pending.get(&id).is_some_and(|p| p.helper.is_none()) {
                    self.attempt(id, user, None);
                }
            }
            Command::AuthConfirm(id) => {
                if let Some(h) = self.pending.get_mut(&id).and_then(|p| p.helper.as_mut()) {
                    let acts = h.talk.confirmed();
                    self.carry_out(id, acts);
                }
            }
            _ => {}
        }
    }

    /// Starts a conversation with the helper for the user at `user`: with a password, or to let the face check run.
    fn attempt(&mut self, id: u64, user: usize, password: Option<Arc<Secret>>) {
        let Some(p) = self.pending.get_mut(&id) else { return };
        let Some(name) = p.users.get(user).cloned() else { return };
        let opened = connect_helper(HELPER_SOCKET).and_then(|mut s| {
            // the user and then the cookie, a line each
            s.write_all(format!("{name}\n{}\n", p.cookie).as_bytes()).map_err(|e| e.to_string())?;
            Ok((s.try_clone().map_err(|e| e.to_string())?, s))
        });
        let (keep, stream) = match opened {
            Ok(s) => s,
            Err(e) => return self.broken(id, e),
        };
        self.next += 1;
        let (session, tx) = (self.next, self.tx.clone());
        p.asked = name.clone();
        p.helper = Some(Helper { session, stream: keep, talk: Talk::new(password) });
        std::thread::spawn(move || {
            listen(stream, |line| {
                let _ = tx.send(Event::Helper { id, session, line });
            });
        });
    }

    fn broken(&mut self, id: u64, why: String) {
        log::error!("polkit: {why}");
        self.rep.input(Input::Toast { summary: "The password could not be checked".into(), body: why });
        self.finish(id, Err(("Failed", "the helper failed")));
    }

    /// Does what the conversation of a request asks: answers it, and tells the person.
    fn carry_out(&mut self, id: u64, acts: Vec<Act>) {
        let Some(h) = self.pending.get_mut(&id).and_then(|p| p.helper.as_mut()) else { return };
        match h.carry_out(acts) {
            Ok(told) => told.into_iter().for_each(|t| self.tell(id, t)),
            Err(e) => self.broken(id, format!("polkit's helper: {e}")),
        }
    }

    fn helper(&mut self, id: u64, session: u64, line: Result<String, String>) {
        // word from an attempt that is over is nothing
        let Some(h) = self.pending.get_mut(&id).and_then(|p| p.helper.as_mut()).filter(|h| h.session == session) else { return };
        match line {
            Ok(line) => {
                let acts = h.talk.heard(&line);
                self.carry_out(id, acts);
            }
            Err(why) => self.broken(id, why),
        }
    }

    fn tell(&mut self, id: u64, what: Tell) {
        let Some(p) = self.pending.get_mut(&id) else { return };
        match what {
            Tell::Asks(prompt) => self.state(id, AuthUpdate::Prompt(prompt)),
            Tell::Ready => self.state(id, AuthUpdate::Ready),
            Tell::Face(check) => self.state(id, AuthUpdate::Face(check)),
            Tell::Info(text) => self.state(id, AuthUpdate::Info(text)),
            Tell::Error(text) => self.state(id, AuthUpdate::Error(text)),
            Tell::Accepted => {
                log::info!("the authentication was accepted");
                self.finish(id, Ok(()));
            }
            Tell::Locked(why) => {
                let user = p.asked.clone();
                log::warn!("polkit: {why}");
                self.rep.input(Input::Toast { summary: "Account temporarily locked".into(), body: format!("Too many failed attempts. It opens by itself in a few minutes, or: faillock --user {user} --reset") });
                self.finish(id, Err(("Failed", "the account is locked")));
            }
            Tell::Refused => {
                p.helper = None;
                p.tries = p.tries.saturating_sub(1);
                match p.tries {
                    0 => {
                        self.rep.input(Input::Toast { summary: "Not authorised".into(), body: format!("{TRIES} attempts were refused") });
                        self.finish(id, Err(("Failed", "the authentication was refused")));
                    }
                    left => self.state(id, AuthUpdate::Wrong(left)),
                }
            }
            Tell::Broken(why) => self.broken(id, why),
        }
    }
}

/// Tells Gaze that polkit's requests are shown by a program that knows its fixed words, so that it sends those instead of
/// sentences in English. It holds while this connection lives. Without Gaze there is nobody to tell.
fn tell_gaze(conn: &Connection) {
    let told: zbus::Result<()> = Proxy::new(conn, "com.gundulabs.Gaze", "/com/gundulabs/Gaze", "com.gundulabs.Gaze").and_then(|p| p.call("AddPamInternal", &("polkit-1",)));
    if let Err(e) = told {
        log::debug!("Gaze was not told (its sentences are read instead): {e}");
    }
}

/// The login session the island answers for: the one on the person's display.
fn session_id(conn: &Connection) -> Option<String> {
    let found = || -> zbus::Result<String> {
        let manager = Proxy::new(conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;
        // SAFETY: getuid cannot fail
        let user: OwnedObjectPath = manager.call("GetUser", &(unsafe { libc::getuid() },))?;
        let (id, _): (String, OwnedObjectPath) = Proxy::new(conn, "org.freedesktop.login1", user.as_str(), "org.freedesktop.login1.User")?.get_property("Display")?;
        Ok(id)
    };
    found().ok().filter(|id| !id.is_empty()).or_else(|| std::env::var("XDG_SESSION_ID").ok().filter(|id| !id.is_empty()))
}

/// Whom the agent answers for: a login session (or, in the tests, one process).
type Subject = (&'static str, HashMap<&'static str, OwnedValue>);

fn register(conn: &Connection, subject: &Subject) -> zbus::Result<()> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "en_US.UTF-8".into());
    Proxy::new(conn, POLKIT, "/org/freedesktop/PolicyKit1/Authority", "org.freedesktop.PolicyKit1.Authority")?.call("RegisterAuthenticationAgent", &(subject, locale.as_str(), PATH))
}

pub fn start(rep: Reporter) {
    if !crate::config::load().polkit_agent {
        return;
    }
    if !KEYBOARD_EXCLUSIVE.load(Ordering::Relaxed) {
        log::warn!("polkit_agent needs the Wayland layer-shell backend, which can hold the keyboard: not started");
        return;
    }
    std::thread::spawn(move || {
        if let Err(e) = run(&rep) {
            log::error!("polkit agent: {e}");
            rep.input(Input::Toast { summary: "The island cannot ask for passwords".into(), body: e });
        }
    });
}

fn run(rep: &Reporter) -> Result<(), String> {
    if !std::path::Path::new(HELPER_SOCKET).exists() {
        return Err("this system has no polkit helper socket (polkit 126 or later is needed)".into());
    }
    // other programs of this user may not trace this process or read its memory, and it leaves no core dump
    // SAFETY: a call that only sets a flag of this process
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err("the process could not be protected from tracing".into());
    }
    let conn = Connection::system().map_err(|e| e.to_string())?;
    let session = session_id(&conn).ok_or("the login session cannot be told")?;
    let id = OwnedValue::try_from(Value::from(session.as_str())).map_err(|e| e.to_string())?;
    let (tx, rx) = channel();
    serve(conn, rep, ("unix-session", HashMap::from([("session-id", id)])), tx.clone(), rx, move || {
        log::info!("the island asks for administrator passwords (session {session})");
        let _ = ASKED.set(tx);
    })
}

/// Registers for `subject` and answers polkit until the events end. `ready` is called once registered.
fn serve(conn: Connection, rep: &Reporter, subject: Subject, tx: Sender<Event>, rx: Receiver<Event>, ready: impl FnOnce()) -> Result<(), String> {
    // listening starts before registering, so that no request is missed
    let messages = MessageIterator::from(conn.clone());
    Proxy::new(&conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus")
        .and_then(|p| p.call::<_, _, ()>("AddMatch", &(format!("type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged',arg0='{POLKIT}'"),)))
        .map_err(|e| e.to_string())?;
    let bus = tx.clone();
    std::thread::spawn(move || {
        for m in messages.flatten() {
            let wanted = m.message_type() == Type::MethodCall || (m.message_type() == Type::Signal && m.header().member().is_some_and(|n| n.as_str() == "NameOwnerChanged"));
            if wanted && bus.send(Event::Bus(m)).is_err() {
                return;
            }
        }
    });
    register(&conn, &subject).map_err(|e| {
        let e = e.to_string();
        if e.contains("already exists") {
            "another program asks for them already (in Noctalia: polkit_agent): switch it off and restart the island".to_string()
        } else {
            e.chars().take(120).collect()
        }
    })?;
    ready();
    let mut agent = Agent { conn, rep: rep.clone(), tx, pending: HashMap::new(), next: 0 };
    for event in rx {
        match event {
            Event::Bus(m) if m.message_type() == Type::Signal => {
                // polkitd came back: what was pending is gone with the old one, and the agent registers again
                let restarted = m.body().deserialize::<(String, String, String)>().is_ok_and(|(name, _, new)| name == POLKIT && !new.is_empty());
                if restarted && m.header().sender().is_some_and(|s| s.as_str() == "org.freedesktop.DBus") {
                    for id in agent.pending.keys().copied().collect::<Vec<_>>() {
                        agent.pending.remove(&id);
                        agent.state(id, AuthUpdate::Gone);
                    }
                    if let Err(e) = register(&agent.conn, &subject) {
                        log::error!("polkit agent: {e}");
                    }
                }
            }
            Event::Bus(m) => agent.call(m),
            Event::Answer(c) => agent.answer(c),
            Event::Helper { id, session, line } => agent.helper(id, session, line),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> Arc<Secret> {
        let mut x = Secret::new();
        assert!(x.push(s));
        Arc::new(x)
    }

    /// Plays the helper over a real socket: sends `lines`, reading an answer after each prompt, while the island's side
    /// runs as it does in the agent. Returns what was told and what the helper received.
    fn talk(lines: &[&str], password: Option<&str>) -> (Vec<Tell>, Vec<String>) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let script: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let helper = std::thread::spawn(move || {
            let mut out = theirs.try_clone().unwrap();
            let mut input = BufReader::new(theirs);
            let mut got = vec![];
            for l in script {
                let _ = writeln!(out, "{l}");
                if l.starts_with("PAM_PROMPT") {
                    let mut line = String::new();
                    if input.read_line(&mut line).unwrap_or(0) > 0 {
                        got.push(line.trim_end_matches('\n').to_string());
                    }
                }
            }
            got
        });
        let mut h = Helper { session: 1, stream: ours.try_clone().unwrap(), talk: Talk::new(password.map(secret)) };
        let mut said = vec![];
        listen(ours, |line| match line {
            Ok(line) => {
                let acts = h.talk.heard(&line);
                let told = h.carry_out(acts).unwrap();
                // a person who answers a question when one is put
                if told.iter().any(|t| matches!(t, Tell::Asks(_))) {
                    let acts = h.talk.typed(secret("0000"));
                    h.carry_out(acts).unwrap();
                }
                said.extend(told);
            }
            Err(e) => said.push(Tell::Broken(e)),
        });
        (said, helper.join().unwrap())
    }

    #[test]
    fn lines_go_both_ways_over_the_socket() {
        let (said, got) = talk(&["PAM_PROMPT_ECHO_OFF Password: ", "SUCCESS"], Some("hunter2 è"));
        assert_eq!(said, [Tell::Accepted]);
        assert_eq!(got, ["hunter2 è"]);
        let (said, _) = talk(&["PAM_PROMPT_ECHO_OFF Password: ", "PAM_ERROR_MSG Account\\tlocked \\303\\250", "FAILURE"], Some("bad"));
        assert_eq!(said, [Tell::Error("Account\tlocked è".into()), Tell::Refused]);
        // a PIN first: the password is not sent in its place
        let (said, got) = talk(&["PAM_PROMPT_ECHO_OFF Enter PIN: ", "SUCCESS"], Some("hunter2"));
        assert_eq!(said, [Tell::Asks("Enter PIN: ".into()), Tell::Accepted]);
        assert_eq!(got, ["0000"]);
    }

    #[test]
    fn a_helper_that_ends_oddly_is_told() {
        let (said, _) = talk(&[], None);
        assert!(matches!(&said[..], [Tell::Broken(w)] if w.contains("closed")), "{said:?}");
        let (said, _) = talk(&[&"A".repeat(10_000)], None);
        assert!(matches!(&said[..], [Tell::Broken(w)] if w.contains("without an end")), "{said:?}");
    }

    /// With the real Gaze daemon: what is told holds for this connection only, and goes with it.
    #[test]
    #[ignore = "talks to this system's Gaze daemon"]
    fn gaze_learns_that_polkit_requests_are_shown_by_a_program_that_knows_its_words() {
        let conn = Connection::system().unwrap();
        let known = |c: &Connection| -> Vec<String> { Proxy::new(c, "com.gundulabs.Gaze", "/com/gundulabs/Gaze", "com.gundulabs.Gaze").unwrap().get_property("PamInternal").unwrap() };
        println!("before: {:?}", known(&conn));
        tell_gaze(&conn);
        let other = Connection::system().unwrap();
        assert!(known(&other).iter().any(|s| s == "polkit-1"), "{:?}", known(&other));
        drop(conn);
        std::thread::sleep(Duration::from_millis(800));
        println!("after the connection is gone: {:?}", known(&Connection::system().unwrap()));
    }

    #[test]
    fn nothing_is_sent_to_a_socket_that_is_not_roots() {
        let dir = std::env::temp_dir().join(format!("oi-pk-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("s");
        let _ = std::fs::remove_file(&path);
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // SAFETY: getuid cannot fail
        if unsafe { libc::getuid() } != 0 {
            let e = connect_helper(path.to_str().unwrap()).unwrap_err();
            assert!(e.contains("not the system"), "{e}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_users_may_answer_and_the_persons_own_comes_first() {
        let user = |uid: u32| ("unix-user".to_string(), HashMap::from([("uid".to_string(), OwnedValue::from(uid))]));
        let group = ("unix-group".to_string(), HashMap::from([("gid".to_string(), OwnedValue::from(998u32))]));
        let name = |uid: u32| (uid != 7).then(|| format!("u{uid}"));
        assert_eq!(users_of(&[user(0), group.clone(), user(1000), user(7)], 1000, name), (vec!["u0".to_string(), "u1000".to_string()], 1));
        assert_eq!(users_of(&[user(0)], 1000, name), (vec!["u0".to_string()], 0));
        assert!(users_of(&[group], 1000, name).0.is_empty());
        assert_eq!(user_name(0).as_deref(), Some("root"));
        assert!(plain_cookie("3-abc") && !plain_cookie("a\nb") && !plain_cookie(""));
    }

    #[test]
    #[ignore = "talks to this system's polkit socket"]
    fn the_real_helper_socket_is_roots() {
        // connected and closed at once: the helper has no user to try, so nothing is counted against anyone (systemd
        // does keep that instance as failed: `sudo systemctl reset-failed 'polkit-agent-helper@*'`)
        connect_helper(HELPER_SOCKET).unwrap();
    }

    /// With the real polkitd, for one process of this test only (the session's own agent is not touched). A request is
    /// received and dismissed: no password is typed or sent.
    #[test]
    #[ignore = "registers an agent with this system's polkit"]
    fn polkit_asks_this_agent_and_takes_its_dismissal() {
        use std::process::Command as Run;
        let mut target = Run::new("sleep").arg("60").spawn().unwrap();
        let pid = target.id();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let started: u64 = stat.rsplit_once(')').unwrap().1.split_whitespace().nth(19).unwrap().parse().unwrap();
        // SAFETY: getuid cannot fail
        let uid = unsafe { libc::getuid() };
        let value = |v: Value<'static>| OwnedValue::try_from(v).unwrap();
        let subject: Subject = ("unix-process", HashMap::from([("pid", value(Value::from(pid))), ("start-time", value(Value::from(started))), ("uid", value(Value::from(uid as i32)))]));
        let (reports, heard) = channel();
        let rep = Reporter::for_tests(reports);
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let events = tx.clone();
        std::thread::spawn(move || {
            let conn = Connection::system().unwrap();
            if let Err(e) = serve(conn, &rep, subject, events, rx, move || ready_tx.send(()).unwrap()) {
                eprintln!("agent: {e}");
            }
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).expect("registered");
        let check = || Run::new("pkcheck").args(["--action-id", "org.freedesktop.policykit.exec", "--process", &pid.to_string(), "--allow-user-interaction"]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let next = || loop {
            if let crate::services::Msg::Input(i) = heard.recv_timeout(Duration::from_secs(20)).expect("word from the agent") {
                break i;
            }
        };
        // dismissed by the person
        let asking = check();
        let Input::AuthAsk(r) = next() else { panic!("a request was expected") };
        println!("asked: {r:?}");
        assert_eq!(r.action, "org.freedesktop.policykit.exec");
        assert!(r.requester.starts_with("sleep (pid") && !r.users.is_empty() && r.pictures.len() == r.users.len());
        tx.send(Event::Answer(Command::AuthCancel(r.id))).unwrap();
        let out = asking.wait_with_output().unwrap();
        println!("pkcheck: {:?} {}", out.status.code(), String::from_utf8_lossy(&out.stderr));
        assert!(!out.status.success());
        assert!(matches!(next(), Input::AuthState { id, update: AuthUpdate::Gone } if id == r.id));
        // withdrawn by the program that asked
        let mut asking = check();
        let Input::AuthAsk(r) = next() else { panic!("a second request was expected") };
        asking.kill().unwrap();
        let _ = asking.wait();
        assert!(matches!(next(), Input::AuthState { id, update: AuthUpdate::Gone } if id == r.id), "polkit's cancellation closes the prompt");
        target.kill().unwrap();
        let _ = target.wait();
    }
}
