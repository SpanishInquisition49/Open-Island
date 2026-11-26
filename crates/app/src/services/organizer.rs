//! What the organizer needs from the machine and the network: the notes folder, the search through it and the study
//! folders, new events (a CalDAV calendar, or a calendar file of the island's own) and the tasks kept on a CalDAV account.
//!
//! Nothing here knows any other program or project: the notes folder is any folder, the account is any CalDAV account.

use super::Reporter;
use island_core::plan::{self, Hit, HitKind};
use island_core::task::{self, Task};
use island_ui::model::{Command, Input};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ---- notes

/// The folder `note` writes to.
pub fn notes_dir() -> Option<PathBuf> {
    let set = crate::config::load().notes_dir;
    if set.is_empty() { super::state_file("notes") } else { Some(PathBuf::from(crate::config::expand_home(&set))) }
}

/// Adds a line to the note of the day (`<dir>/2027-01-15.md`, started with a heading) and says where it went.
pub fn save_note(dir: &Path, text: &str, now: chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    use std::io::Write;
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return Err("nothing to write".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.md", now.format("%Y-%m-%d")));
    let fresh = !path.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if fresh {
        writeln!(f, "# {}\n", now.format("%Y-%m-%d")).map_err(|e| e.to_string())?;
    }
    writeln!(f, "- {} {line}", now.format("%H:%M")).map_err(|e| e.to_string())?;
    Ok(path)
}

const MAX_ENTRIES: usize = 8_000;
const MAX_FILE: u64 = 1_000_000;
const MAX_TIME: Duration = Duration::from_millis(1500);

/// The lines of the notes (.md and .txt, three folders deep, newest files first) that have every word of the query.
pub fn search_notes(dir: &Path, query: &str, max: usize) -> Vec<Hit> {
    let began = Instant::now();
    let mut files: Vec<PathBuf> = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                break;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 3 && !name.starts_with('.') {
                    stack.push((e.path(), depth + 1));
                }
            } else if ft.is_file() && !name.starts_with('.') && (name.ends_with(".md") || name.ends_with(".txt")) && e.metadata().is_ok_and(|m| m.len() <= MAX_FILE) {
                files.push(e.path());
            }
        }
    }
    files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    let mut out = vec![];
    for f in files {
        if began.elapsed() > MAX_TIME || out.len() >= max {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for line in text.lines().filter(|l| plan::matches(l, query)) {
            out.push(Hit { kind: HitKind::Note, title: line.trim().trim_start_matches(['-', '*', '#', ' ']).chars().take(90).collect(), detail: name.clone(), target: f.to_string_lossy().into_owned() });
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Starts a search of the notes and of the study folders (file names only); the answer comes back as `FoundMore`.
fn find(rep: &Reporter, query: String) {
    let rep = rep.clone();
    std::thread::spawn(move || {
        let mut hits = notes_dir().map(|d| search_notes(&d, &query, 12)).unwrap_or_default();
        let kws = island_core::materials::keywords(&query);
        if !kws.is_empty() {
            for m in island_core::materials::rank(super::materials::scan(&super::materials::folders(), &kws), 6) {
                let folder = Path::new(&m.path).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                hits.push(Hit { kind: HitKind::File, title: m.name, detail: folder, target: m.path });
            }
        }
        rep.input(Input::FoundMore { query, hits });
    });
}

// ---- events

/// The calendar file the island keeps its own events in, when there is no account to hold them.
pub fn local_events_path() -> Option<PathBuf> {
    super::state_file("local-events.ics")
}

/// The file as a calendar source, once it exists (so that what was made with `event` shows up).
pub fn local_calendar() -> Option<crate::config::CalendarSource> {
    let p = local_events_path().filter(|p| p.is_file())?;
    Some(crate::config::CalendarSource { name: "Island".into(), kind: crate::config::CalendarKind::File { path: p.to_string_lossy().into_owned() }, color: None })
}

/// Adds an event to the island's own calendar file.
pub fn append_local_event(path: &Path, one: &str) -> Result<(), String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let vevent: String = one.lines().skip_while(|l| *l != "BEGIN:VEVENT").take_while(|l| *l != "END:VCALENDAR").map(|l| format!("{l}\r\n")).collect();
    let head = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//open-island//events//EN\r\nX-WR-CALNAME:Island\r\n";
    let body = old.trim_end().strip_suffix("END:VCALENDAR").map(|b| b.trim_end().to_string() + "\r\n").unwrap_or_else(|| head.to_string());
    std::fs::write(path, format!("{body}{vevent}END:VCALENDAR\r\n")).map_err(|e| format!("{}: {e}", path.display()))
}

/// The document of an event: with hours, or (`whole_day` is a civil day) without.
fn event_ics(uid: &str, title: &str, start_ms: i64, end_ms: i64, whole_day: Option<i64>) -> String {
    match whole_day {
        Some(day) => island_core::ics::to_vevent_all_day(uid, title, day, &stamp_now()),
        None => island_core::ics::to_vevent(uid, title, start_ms, end_ms, &stamp_now()),
    }
}

fn stamp_now() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// The CalDAV source a setting names: by the account's name or user, else the first account.
fn pick_account(sources: &[crate::config::CalendarSource], name: &str) -> Option<crate::config::CalendarSource> {
    use crate::config::CalendarKind;
    let dav: Vec<&crate::config::CalendarSource> = sources.iter().filter(|c| matches!(c.kind, CalendarKind::CalDav { .. })).collect();
    let by_name = |c: &&&crate::config::CalendarSource| {
        let user = match &c.kind {
            CalendarKind::CalDav { user, .. } => user.as_str(),
            _ => "",
        };
        !name.is_empty() && (c.name.eq_ignore_ascii_case(name) || user.eq_ignore_ascii_case(name))
    };
    dav.iter().find(by_name).or(dav.first()).map(|c| (**c).clone())
}

fn uid_file(uid: &str) -> String {
    uid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

/// Makes the event: on the CalDAV calendar when there is one, otherwise (or when the server will not have it) in the
/// island's own file. Says what went wrong, if anything.
fn create_event(rep: &Reporter, title: String, start_ms: i64, end_ms: i64, whole_day: Option<i64>) {
    let rep = rep.clone();
    std::thread::spawn(move || {
        let s = crate::config::load();
        let uid = format!("oi-ev-{:x}", chrono::Utc::now().timestamp_micros());
        let ics = event_ics(&uid, &title, start_ms, end_ms, whole_day);
        let remote = pick_account(&s.calendars, s.event_calendar.split('/').next().unwrap_or("")).map(|src| -> Result<(), String> {
            let acct = super::net::discover(&src)?;
            let cals = super::net::dav_calendars(&acct, "VEVENT")?;
            let want = s.event_calendar.rsplit('/').next().unwrap_or("");
            let cal = cals.iter().find(|c| !want.is_empty() && c.name.eq_ignore_ascii_case(want)).or(cals.first()).ok_or("no calendar to write to")?;
            let url = format!("{}{}.ics", super::caldav::resolve(&acct.home_url, &cal.href).trim_end_matches('/').to_string() + "/", uid_file(&uid));
            super::net::dav_put(&url, &acct.auth, &ics)
        });
        match remote {
            Some(Ok(())) => {}
            other => {
                let why = other.and_then(|r| r.err());
                let saved = local_events_path().ok_or_else(|| "no place to keep it".to_string()).and_then(|p| append_local_event(&p, &ics));
                match (saved, why) {
                    (Ok(()), Some(e)) => rep.input(Input::Toast { summary: "Event kept on this computer".into(), body: format!("The calendar said: {e}") }),
                    (Err(e), _) => rep.input(Input::Toast { summary: "Could not save the event".into(), body: e }),
                    _ => {}
                }
            }
        }
        super::CALENDAR_REFRESH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
}

/// Moves or cancels an event: in the island's own file when the event is there, else on the account's calendars.
/// The id is the calendar's own (`<uid>@<start>`); what repeats is left to the calendar app.
fn change_event(rep: &Reporter, id: String, title: String, around_ms: i64, ch: island_core::ics::Change) {
    let rep = rep.clone();
    std::thread::spawn(move || {
        let uid = id.rsplit_once('@').map_or(id.as_str(), |(u, _)| u).to_string();
        let res = (|| -> Result<(), String> {
            // 1. the island's own file
            if let Some(p) = local_events_path().filter(|p| p.is_file()) {
                let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
                if text.contains(&format!("UID:{uid}")) {
                    let new = island_core::ics::change_event(&text, &uid, &ch)?.unwrap_or_else(|| "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//open-island//events//EN\r\nX-WR-CALNAME:Island\r\nEND:VCALENDAR\r\n".into());
                    return std::fs::write(&p, new).map_err(|e| e.to_string());
                }
            }
            // 2. the account
            let s = crate::config::load();
            let src = pick_account(&s.calendars, s.event_calendar.split('/').next().unwrap_or("")).ok_or("the event is not in a calendar the island can write to")?;
            let acct = super::net::discover(&src)?;
            let fmt = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y%m%dT%H%M%SZ").to_string();
            let body = super::caldav::query_body(&fmt(around_ms - 86_400_000), &fmt(around_ms + 2 * 86_400_000));
            for cal in super::net::dav_calendars(&acct, "VEVENT")? {
                let base = super::caldav::resolve(&acct.home_url, &cal.href);
                let Ok(xml) = super::net::dav_report(&base, &acct.auth, &body) else { continue };
                for (href, ics) in super::caldav::parse_event_report(&xml) {
                    if !ics.contains(&format!("UID:{uid}")) {
                        continue;
                    }
                    let url = super::caldav::resolve(&base, &href);
                    return match island_core::ics::change_event(&ics, &uid, &ch)? {
                        Some(new) => super::net::dav_put(&url, &acct.auth, &new),
                        None => super::net::dav_delete(&url, &acct.auth),
                    };
                }
            }
            Err("the event was not found in the calendars".into())
        })();
        if let Err(e) = res {
            rep.input(Input::Toast { summary: format!("Could not change \"{title}\""), body: e });
        }
        super::CALENDAR_REFRESH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
}

// ---- tasks on a CalDAV account

enum Op {
    Pull,
    Delete(String),
    Push(Task, bool),
}

static TASKS: OnceLock<Mutex<Sender<Op>>> = OnceLock::new();

/// A signed-in to-do list.
struct List {
    auth: String,
    url: String,
}

fn connect(s: &crate::config::Settings) -> Result<List, String> {
    let src = pick_account(&s.calendars, &s.tasks_account).ok_or("no CalDAV account for the tasks")?;
    let acct = super::net::discover(&src)?;
    let lists = super::net::dav_calendars(&acct, "VTODO")?;
    let list = lists.iter().find(|l| !s.task_list.is_empty() && l.name.eq_ignore_ascii_case(&s.task_list)).or(lists.first()).ok_or("the account has no to-do list")?;
    Ok(List { auth: acct.auth, url: super::caldav::resolve(&acct.home_url, &list.href) })
}

fn pull(list: &List, hrefs: &mut HashMap<String, String>) -> Result<Vec<(Task, bool)>, String> {
    let xml = super::net::dav_report(&list.url, &list.auth, &super::caldav::todo_query_body())?;
    let mut out = vec![];
    for (href, ics) in super::caldav::parse_todo_report(&xml) {
        for (t, done) in task::from_vtodo(&ics) {
            hrefs.insert(t.uid.clone(), href.clone());
            out.push((t, done));
        }
    }
    out.truncate(500);
    Ok(out)
}

fn push(list: &List, hrefs: &HashMap<String, String>, t: &Task, done: bool) -> Result<(), String> {
    let url = match hrefs.get(&t.uid).filter(|h| !h.is_empty()) {
        Some(h) => super::caldav::resolve(&list.url, h),
        None => format!("{}{}.ics", list.url.trim_end_matches('/').to_string() + "/", uid_file(&t.uid)),
    };
    super::net::dav_put(&url, &list.auth, &task::to_vtodo(t, done, &stamp_now()))
}

/// The thread that keeps the tasks of the island and of the account level: a pull every five minutes and after every change.
pub fn start(rep: Reporter) {
    let (tx, rx) = channel::<Op>();
    let _ = TASKS.set(Mutex::new(tx.clone()));
    let _ = tx.send(Op::Pull);
    std::thread::spawn(move || {
        let (mut conn, mut hrefs): (Option<List>, HashMap<String, String>) = (None, HashMap::new());
        let mut last_error = String::new();
        loop {
            let first = match rx.recv_timeout(Duration::from_secs(300)) {
                Ok(op) => op,
                Err(RecvTimeoutError::Timeout) => Op::Pull,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let s = crate::config::load();
            if !s.tasks_sync {
                conn = None;
                continue;
            }
            let mut ops = vec![first];
            while let Ok(op) = rx.try_recv() {
                ops.push(op);
            }
            let result = (|| -> Result<Option<Vec<(Task, bool)>>, String> {
                if conn.is_none() {
                    conn = Some(connect(&s)?);
                }
                let list = conn.as_ref().ok_or("not connected")?;
                // one task the server will not take must not keep the others (or the pull) from going through
                let mut refused = vec![];
                for op in &ops {
                    match op {
                        Op::Push(t, done) => {
                            if let Err(e) = push(list, &hrefs, t, *done) {
                                refused.push(format!("{}: {e}", t.text));
                            }
                        }
                        Op::Delete(uid) => {
                            let url = match hrefs.get(uid).filter(|h| !h.is_empty()) {
                                Some(h) => super::caldav::resolve(&list.url, h),
                                None => format!("{}{}.ics", list.url.trim_end_matches('/').to_string() + "/", uid_file(uid)),
                            };
                            match super::net::dav_delete(&url, &list.auth) {
                                Ok(()) => {
                                    hrefs.remove(uid);
                                }
                                Err(e) => refused.push(format!("delete: {e}")),
                            }
                        }
                        Op::Pull => {}
                    }
                }
                let pulled = pull(list, &mut hrefs)?;
                if !refused.is_empty() {
                    rep.input(Input::Toast { summary: "A task was not saved on the server".into(), body: refused.join("; ") });
                }
                Ok(Some(pulled))
            })();
            match result {
                Ok(Some(tasks)) => {
                    last_error.clear();
                    rep.input(Input::TasksRemote(tasks));
                }
                Ok(None) => {}
                Err(e) => {
                    conn = None;
                    // say it once, not every five minutes
                    if e != last_error {
                        rep.input(Input::Toast { summary: "Tasks not synced".into(), body: e.clone() });
                        last_error = e;
                    }
                }
            }
        }
    });
}

/// Runs the commands of the organizer; false for any other command.
pub fn command(rep: &Reporter, c: &Command) -> bool {
    match c.clone() {
        Command::SaveNote(text) => {
            let rep = rep.clone();
            std::thread::spawn(move || {
                let res = notes_dir().ok_or_else(|| "no place for notes".to_string()).and_then(|d| save_note(&d, &text, chrono::Local::now()));
                if let Err(e) = res {
                    rep.input(Input::Toast { summary: "Could not save the note".into(), body: e });
                }
            });
        }
        Command::Find(q) => find(rep, q),
        Command::CreateEvent { title, start_ms, end_ms, whole_day } => create_event(rep, title, start_ms, end_ms, whole_day),
        Command::PushTask { task, done } => {
            if let Some(tx) = TASKS.get().and_then(|t| t.lock().ok()) {
                let _ = tx.send(Op::Push(task, done));
            }
        }
        Command::DeleteTask { uid } => {
            if let Some(tx) = TASKS.get().and_then(|t| t.lock().ok()) {
                let _ = tx.send(Op::Delete(uid));
            }
        }
        Command::MoveEvent { id, title, old_start_ms, start_ms, end_ms, whole_day } => change_event(rep, id, title, old_start_ms, island_core::ics::Change::Move { start_ms, end_ms, whole_day }),
        Command::CancelEvent { id, title, start_ms } => change_event(rep, id, title, start_ms, island_core::ics::Change::Cancel),
        Command::SyncTasks => {
            if let Some(tx) = TASKS.get().and_then(|t| t.lock().ok()) {
                let _ = tx.send(Op::Pull);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oi-org-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_note_is_a_line_in_the_file_of_the_day() {
        let dir = tmp("notes");
        let at = chrono::Local.with_ymd_and_hms(2027, 1, 15, 9, 5, 0).unwrap();
        let p = save_note(&dir, "ask   about the\nexam format", at).unwrap();
        save_note(&dir, "second one", chrono::Local.with_ymd_and_hms(2027, 1, 15, 10, 0, 0).unwrap()).unwrap();
        assert_eq!(p.file_name().unwrap(), "2027-01-15.md");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# 2027-01-15\n\n- 09:05 ask about the exam format\n- 10:00 second one\n", "a heading once, one line a note");
        assert!(save_note(&dir, "  ", at).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_search_reads_notes_in_any_folder_newest_first_and_needs_every_word() {
        let dir = tmp("search");
        std::fs::create_dir_all(dir.join("uni")).unwrap();
        std::fs::write(dir.join("2027-01-10.md"), "# 2027-01-10\n\n- 09:00 compilers: ask about LLVM\n- 10:00 buy milk\n").unwrap();
        std::fs::write(dir.join("uni/2027-01-12.md"), "compilers lecture on parsing\nunrelated\n").unwrap();
        std::fs::write(dir.join("image.png"), "compilers").unwrap();
        std::fs::write(dir.join(".hidden.md"), "compilers").unwrap();
        let hits = search_notes(&dir, "compilers", 10);
        assert_eq!(hits.iter().map(|h| (h.title.as_str(), h.detail.as_str())).collect::<Vec<_>>(), [("compilers lecture on parsing", "2027-01-12.md"), ("09:00 compilers: ask about LLVM", "2027-01-10.md")]);
        assert!(hits.iter().all(|h| h.kind == HitKind::Note && Path::new(&h.target).is_file()));
        assert_eq!(search_notes(&dir, "compilers llvm", 10).len(), 1);
        assert_eq!(search_notes(&dir, "compilers", 1).len(), 1, "the cap holds");
        assert!(search_notes(&dir.join("missing"), "x", 5).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_islands_own_calendar_file_collects_events_and_is_readable() {
        let dir = tmp("ics");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("local-events.ics");
        let ev = |uid: &str, title: &str, a: i64, b: i64| island_core::ics::to_vevent(uid, title, a, b, "20270115T080000Z");
        append_local_event(&p, &ev("a", "Office hours", 1_800_000_000_000, 1_800_000_000_000 + 3_600_000)).unwrap();
        append_local_event(&p, &ev("b", "Dentist; 2nd visit", 1_800_000_000_000 + 86_400_000, 1_800_000_000_000 + 90_000_000)).unwrap();
        append_local_event(&p, &island_core::ics::to_vevent_all_day("c", "Conference", 20_500, "20270115T080000Z")).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.matches("BEGIN:VCALENDAR").count(), 1, "one calendar, three events");
        let evs = island_core::ics::parse(&text, 0, 0, i64::MAX);
        assert_eq!(evs.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Conference", "Office hours", "Dentist; 2nd visit"], "in order of day");
        assert!(evs[0].all_day, "a day without hours stays one");
        assert_eq!(island_core::ics::calendar_name(&text).as_deref(), Some("Island"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_account_is_picked_by_name_or_user_and_else_the_first() {
        use crate::config::{CalendarKind, CalendarSource};
        let dav = |name: &str, user: &str| CalendarSource { name: name.into(), kind: CalendarKind::CalDav { server: "https://s/".into(), user: user.into(), password: String::new(), password_cmd: String::new() }, color: None };
        let src = vec![CalendarSource { name: "feed".into(), kind: CalendarKind::Ics { url: "https://x".into() }, color: None }, dav("Uni", "me@uni.it"), dav("Home", "me@icloud.com")];
        assert_eq!(pick_account(&src, "").map(|c| c.name), Some("Uni".into()));
        assert_eq!(pick_account(&src, "me@icloud.com").map(|c| c.name), Some("Home".into()));
        assert_eq!(pick_account(&src, "home").map(|c| c.name), Some("Home".into()));
        assert_eq!(pick_account(&src, "nobody").map(|c| c.name), Some("Uni".into()), "an unknown name falls back on the first");
        assert!(pick_account(&src[..1], "").is_none(), "a feed is no account");
    }

    /// A tiny CalDAV server with one to-do list: it keeps what is PUT and answers the REPORT with it.
    type Puts = std::sync::Arc<Mutex<Vec<(String, String)>>>;

    fn mock() -> (String, Puts) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let puts: Puts = Default::default();
        let store = puts.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (mut stream, store) = (stream, store.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let head_end = loop {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                    while buf.len() < head_end + len {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let body_in = String::from_utf8_lossy(&buf[head_end..]).to_string();
                    let first = head.lines().next().unwrap_or("").to_string();
                    let (status, body) = if first.starts_with("PROPFIND / ") {
                        ("207 Multi-Status", r#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:current-user-principal><d:href>/u/principal/</d:href></d:current-user-principal></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
                    } else if first.starts_with("PROPFIND /u/principal/ ") {
                        ("207 Multi-Status", r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:propstat><d:prop><c:calendar-home-set><d:href>/u/calendars/</d:href></c:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
                    } else if first.starts_with("PROPFIND /u/calendars/ ") {
                        ("207 Multi-Status", r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/u/calendars/events/</d:href><d:propstat><d:prop><d:displayname>Events</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set></d:prop></d:propstat></d:response><d:response><d:href>/u/calendars/tasks/</d:href><d:propstat><d:prop><d:displayname>Tasks</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
                    } else if first.starts_with("REPORT /u/calendars/tasks/ ") {
                        ("207 Multi-Status", "<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:response><d:href>/u/calendars/tasks/phone-1.ics</d:href><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:phone-1\nSUMMARY:Buy stamps\nDUE;VALUE=DATE:20270120\nEND:VTODO\nEND:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>".to_string())
                    } else if first.starts_with("REPORT /u/calendars/events/ ") {
                        ("207 Multi-Status", "<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:response><d:href>/u/calendars/events/lab-1.ics</d:href><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:lab-1\nSUMMARY:Lab\nDTSTART:20270120T090000Z\nDTEND:20270120T110000Z\nEND:VEVENT\nEND:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>".to_string())
                    } else if first.starts_with("DELETE /u/calendars/") {
                        store.lock().unwrap().push((format!("DELETE {}", first.split(' ').nth(1).unwrap_or("")), String::new()));
                        ("204 No Content", String::new())
                    } else if first.starts_with("PUT /u/calendars/") {
                        store.lock().unwrap().push((first.split(' ').nth(1).unwrap_or("").to_string(), body_in));
                        ("201 Created", String::new())
                    } else {
                        ("404 Not Found", String::new())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                });
            }
        });
        (format!("http://127.0.0.1:{port}/"), puts)
    }

    fn settings(server: &str, task_list: &str) -> crate::config::Settings {
        use crate::config::{CalendarKind, CalendarSource};
        crate::config::Settings { tasks_sync: true, task_list: task_list.into(), calendars: vec![CalendarSource { name: "Cloud".into(), kind: CalendarKind::CalDav { server: server.into(), user: "me".into(), password: "pw".into(), password_cmd: String::new() }, color: None }], ..Default::default() }
    }

    #[test]
    fn tasks_are_read_from_and_written_to_the_to_do_list_of_an_account() {
        let (server, puts) = mock();
        let list = connect(&settings(&server, "")).expect("finds the list that holds to-dos, not the one that holds events");
        assert!(list.url.ends_with("/u/calendars/tasks/"), "{}", list.url);
        let mut hrefs = HashMap::new();
        let remote = pull(&list, &mut hrefs).unwrap();
        assert_eq!(remote.len(), 1);
        assert_eq!((remote[0].0.uid.as_str(), remote[0].0.text.as_str(), remote[0].1), ("phone-1", "Buy stamps", false));
        assert!(remote[0].0.synced && remote[0].0.due.is_some());
        // finishing it writes back to the address the server gave it; a new one goes to a file named by its id
        push(&list, &hrefs, &remote[0].0, true).unwrap();
        let new = Task { uid: "oi-9".into(), text: "Report".into(), due: Some(20_470), course: "algoritmi".into(), est_min: 120, ..Default::default() };
        push(&list, &hrefs, &new, false).unwrap();
        let sent = puts.lock().unwrap().clone();
        assert_eq!(sent[0].0, "/u/calendars/tasks/phone-1.ics");
        assert!(sent[0].1.contains("STATUS:COMPLETED") && sent[0].1.contains("UID:phone-1"));
        assert_eq!(sent[1].0, "/u/calendars/tasks/oi-9.ics");
        assert!(sent[1].1.contains("SUMMARY:Report") && sent[1].1.contains("CATEGORIES:algoritmi") && sent[1].1.contains("X-OPEN-ISLAND-ESTIMATE:120") && sent[1].1.contains("STATUS:NEEDS-ACTION"));
        assert!(connect(&settings(&server, "Nope")).is_ok(), "an unknown list name falls back on the first that holds to-dos");
    }

    #[test]
    fn a_new_event_is_put_on_the_accounts_calendar() {
        let (server, puts) = mock();
        let s = settings(&server, "");
        let src = pick_account(&s.calendars, "").unwrap();
        let acct = super::super::net::discover(&src).unwrap();
        let cals = super::super::net::dav_calendars(&acct, "VEVENT").unwrap();
        assert_eq!(cals.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Events"], "the to-do list is not offered for events");
        let ics = island_core::ics::to_vevent("oi-ev-1", "Office hours", 1_800_000_000_000, 1_800_003_600_000, "20270115T080000Z");
        super::super::net::dav_put(&format!("{}oi-ev-1.ics", super::super::caldav::resolve(&acct.home_url, &cals[0].href)), &acct.auth, &ics).unwrap();
        assert_eq!(puts.lock().unwrap()[0].0, "/u/calendars/events/oi-ev-1.ics");
    }

    #[test]
    fn an_event_is_found_by_its_uid_moved_on_the_server_and_a_task_is_deleted() {
        let (server, puts) = mock();
        let acct = super::super::net::discover(&pick_account(&settings(&server, "").calendars, "").unwrap()).unwrap();
        let cal = &super::super::net::dav_calendars(&acct, "VEVENT").unwrap()[0];
        let base = super::super::caldav::resolve(&acct.home_url, &cal.href);
        let xml = super::super::net::dav_report(&base, &acct.auth, &super::super::caldav::query_body("20270119T000000Z", "20270122T000000Z")).unwrap();
        let found = super::super::caldav::parse_event_report(&xml);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "/u/calendars/events/lab-1.ics");
        let moved = island_core::ics::change_event(&found[0].1, "lab-1", &island_core::ics::Change::Move { start_ms: 1_800_000_000_000, end_ms: 1_800_003_600_000, whole_day: None }).unwrap().unwrap();
        super::super::net::dav_put(&super::super::caldav::resolve(&base, &found[0].0), &acct.auth, &moved).unwrap();
        super::super::net::dav_delete(&format!("{}phone-1.ics", super::super::caldav::resolve(&acct.home_url, "tasks/")), &acct.auth).unwrap();
        let sent = puts.lock().unwrap().clone();
        assert!(sent[0].0.ends_with("/lab-1.ics") && sent[0].1.contains("SUMMARY:Lab") && !sent[0].1.contains("20270120T090000Z"));
        assert_eq!(sent[1].0, "DELETE /u/calendars/tasks/phone-1.ics");
    }
}
