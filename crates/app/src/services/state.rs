//! What the island keeps between runs, one file each in the state folder. A file is written only when what it holds
//! changes (the app asks after every frame), whole or not at all, and for the owner's eyes only.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// What was last written of one list, so that the disk is touched only when it changes.
type Last<T> = Mutex<Option<Vec<T>>>;

/// Whether `now` is not what was seen last, which it then is. An empty list never seen before is no news: there is no
/// file to make for it.
fn changed<T: PartialEq + Clone>(last: &Last<T>, now: &[T]) -> bool {
    let Ok(mut last) = last.lock() else { return false };
    if last.as_deref() == Some(now) || (last.is_none() && now.is_empty()) {
        return false;
    }
    *last = Some(now.to_vec());
    true
}

/// Writes a file next to itself and renames it into place, so that a crash or a full disk leaves the one there was. It
/// is the owner's only from the start. A link stays a link: what it points to is replaced.
pub(crate) fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut part = path.clone().into_os_string();
    part.push(format!(".{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let part = PathBuf::from(part);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let done = options.open(&part).and_then(|mut f| f.write_all(text.as_bytes())).and_then(|()| std::fs::rename(&part, &path));
    if done.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    done
}

/// `write_whole` for a file of the state folder; a failure is logged, the island goes on.
fn save(name: &str, text: &str) {
    if let Some(p) = state_file(name) {
        if let Err(e) = write_whole(&p, text) {
            log::warn!("{}: {e}", p.display());
        }
    }
}

fn save_json(name: &str, value: serde_json::Value) {
    save(name, &value.to_string());
}

pub(crate) fn state_file(name: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))?;
    let dir = base.join("open-island");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

/// Meetings already announced, remembered across restarts (a restart must not announce one twice).
pub fn load_announced() -> Vec<String> {
    state_file("meeting-announced").and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}

pub fn save_announced_if_changed(keys: &[String]) {
    static LAST: Last<String> = Last::new(None);
    if changed(&LAST, keys) {
        save("meeting-announced", &keys.join("\n"));
    }
}

/// "on", "off" or "on 1795700000000" (on until then): whether do not disturb is on at `now`, and its end if it has one.
pub fn parse_dnd(text: &str, now: i64) -> (bool, Option<i64>) {
    match text.trim().split_once(' ') {
        Some(("on", until)) => match until.trim().parse::<i64>() {
            Ok(until) if until > now => (true, Some(until)),
            _ => (false, None),
        },
        _ => (text.trim() == "on", None),
    }
}

pub fn load_dnd(now: i64) -> (bool, Option<i64>) {
    state_file("dnd").and_then(|p| std::fs::read_to_string(p).ok()).map_or((false, None), |t| parse_dnd(&t, now))
}

pub fn save_dnd((on, until): (bool, Option<i64>)) {
    let text = match until {
        Some(until) if on => format!("on {until}"),
        _ => if on { "on".into() } else { "off".to_string() },
    };
    save("dnd", &text);
}

#[cfg(test)]
mod dnd_tests {
    use super::parse_dnd;

    #[test]
    fn a_do_not_disturb_with_an_end_comes_back_only_until_then() {
        assert_eq!(parse_dnd("on\n", 100), (true, None));
        assert_eq!(parse_dnd("off", 100), (false, None));
        assert_eq!(parse_dnd("on 200", 100), (true, Some(200)));
        assert_eq!(parse_dnd("on 100", 100), (false, None), "its time is up");
        assert_eq!(parse_dnd("on soon", 100), (false, None));
    }
}

/// Today's attention per app, kept between runs (day start in epoch ms, then app and milliseconds).
pub fn load_attention() -> Option<(i64, Vec<(String, i64)>)> {
    let v: serde_json::Value = serde_json::from_str(&state_file("attention.json").and_then(|p| std::fs::read_to_string(p).ok())?).ok()?;
    let day = v.get("day")?.as_i64()?;
    let apps = v.get("apps")?.as_array()?.iter().filter_map(|a| Some((a.get(0)?.as_str()?.to_string(), a.get(1)?.as_i64()?))).collect();
    Some((day, apps))
}

/// The totals of the days before the saved one.
pub fn load_attention_history() -> Vec<island_core::activity::DayTotals> {
    let Some(v) = state_file("attention.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return vec![] };
    v.get("history").and_then(|h| serde_json::from_value(h.clone()).ok()).unwrap_or_default()
}

pub fn save_attention(day: i64, apps: &[(String, i64)], history: &[island_core::activity::DayTotals]) {
    save("attention.json", &serde_json::json!({"day": day, "apps": apps, "history": history}).to_string());
}

/// The reminders set from the command bar, kept between runs.
pub fn load_reminders() -> Vec<island_ui::island::Reminder> {
    let Some(text) = state_file("reminders.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items
        .iter()
        .filter_map(|v| Some(island_ui::island::Reminder { due_ms: v.get("due")?.as_i64()?, text: v.get("text")?.as_str()?.to_string(), repeat: v.get("repeat").and_then(|r| r.as_str()).map_or(island_core::task::Repeat::Never, island_core::task::Repeat::from_rrule) }))
        .collect()
}

pub fn save_reminders_if_changed(reminders: &[island_ui::island::Reminder]) {
    static LAST: Last<island_ui::island::Reminder> = Last::new(None);
    if !changed(&LAST, reminders) {
        return;
    }
    let items: Vec<serde_json::Value> = reminders.iter().map(|r| match r.repeat.to_rrule() {
        Some(rule) => serde_json::json!({"due": r.due_ms, "text": r.text, "repeat": rule}),
        None => serde_json::json!({"due": r.due_ms, "text": r.text}),
    }).collect();
    save_json("reminders.json", items.into());
}

/// The last day a recap card ("evening-recap", "morning-recap") came, so that a restart does not show it again.
pub fn load_recap_day(name: &str) -> Option<i64> {
    state_file(name).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| s.trim().parse().ok())
}

pub fn save_recap_day_if_changed(name: &'static str, day: i64) {
    use std::collections::HashMap;
    static LAST: Mutex<Option<HashMap<&'static str, i64>>> = Mutex::new(None);
    if day == i64::MIN {
        return;
    }
    let Ok(mut g) = LAST.lock() else { return };
    if g.get_or_insert_with(HashMap::new).insert(name, day) != Some(day) {
        save(name, &day.to_string());
    }
}

/// The lines run from the command bar, kept between runs (Up recalls them).
pub fn load_bar_history() -> Vec<String> {
    let Some(text) = state_file("bar-history.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

pub fn save_bar_history_if_changed(history: &[String]) {
    static LAST: Last<String> = Last::new(None);
    if changed(&LAST, history) {
        save_json("bar-history.json", history.into());
    }
}

/// Exams and hand-ins, kept between runs.
pub fn load_deadlines() -> Vec<island_ui::island::Deadline> {
    let Some(text) = state_file("deadlines.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items
        .iter()
        .filter_map(|v| Some(island_ui::island::Deadline { day: v.get("day")?.as_i64()?, text: v.get("text")?.as_str()?.to_string(), course: v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(), est_min: v.get("est").and_then(|e| e.as_u64()).unwrap_or(0) as u32, done_min: v.get("done").and_then(|e| e.as_u64()).unwrap_or(0) as u32, cfu: v.get("cfu").and_then(|e| e.as_u64()).unwrap_or(0) as u32 }))
        .collect()
}

pub fn save_deadlines_if_changed(d: &[island_ui::island::Deadline]) {
    static LAST: Last<island_ui::island::Deadline> = Last::new(None);
    if !changed(&LAST, d) {
        return;
    }
    let items: Vec<serde_json::Value> = d.iter().map(|x| serde_json::json!({"day": x.day, "text": x.text, "course": x.course, "est": x.est_min, "done": x.done_min, "cfu": x.cfu})).collect();
    save_json("deadlines.json", items.into());
}

/// How often each page was opened (by page key), kept between runs for the dock.
pub fn load_page_usage() -> Vec<(String, u32)> {
    let Some(text) = state_file("page-usage.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<std::collections::BTreeMap<String, u32>>(&text).map(|m| m.into_iter().collect()).unwrap_or_default()
}

pub fn save_page_usage_if_changed(u: &[(String, u32)]) {
    static LAST: Last<(String, u32)> = Last::new(None);
    if changed(&LAST, u) {
        let map: std::collections::BTreeMap<&str, u32> = u.iter().map(|(k, n)| (k.as_str(), *n)).collect();
        save_json("page-usage.json", serde_json::json!(map));
    }
}

/// How fast the battery drains (percent per hour), learned from use, kept between runs.
pub fn load_drain() -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state_file("power.json")?).ok()?).ok()?;
    v.get("drain")?.as_f64()
}

pub fn save_drain_if_changed(rate: Option<f64>) {
    static LAST: Mutex<Option<Option<u64>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    // a tenth of a percent is the same drain
    let key = rate.map(|r| (r * 10.0).round() as u64);
    if *last == Some(key) || (last.is_none() && key.is_none()) {
        return;
    }
    *last = Some(key);
    if let Some(r) = rate {
        save_json("power.json", serde_json::json!({"drain": (r * 10.0).round() / 10.0}));
    }
}

/// What to take along, kept between runs: (thing, course).
pub fn load_bring() -> Vec<island_ui::island::Bring> {
    let Some(text) = state_file("bring.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items.iter().filter_map(|v| Some(island_ui::island::Bring { text: v.get("text")?.as_str()?.to_string(), course: v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string() })).collect()
}

pub fn save_bring_if_changed(b: &[island_ui::island::Bring]) {
    static LAST: Last<island_ui::island::Bring> = Last::new(None);
    if !changed(&LAST, b) {
        return;
    }
    let items: Vec<serde_json::Value> = b.iter().map(|x| serde_json::json!({"text": x.text, "course": x.course})).collect();
    save_json("bring.json", items.into());
}

/// The notes pinned to the Today page, kept between runs.
pub fn load_pins() -> Vec<String> {
    let Some(text) = state_file("pins.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

pub fn save_pins_if_changed(pins: &[String]) {
    static LAST: Last<String> = Last::new(None);
    if changed(&LAST, pins) {
        save_json("pins.json", pins.into());
    }
}

/// Saved places and the reminders waiting for them, kept between runs (`places.json`).
pub type Places = (Vec<(String, f64, f64)>, Vec<island_core::places::Waiting>);

fn places_json((places, waiting): (&[(String, f64, f64)], &[island_core::places::Waiting])) -> serde_json::Value {
    let waiting: Vec<_> = waiting.iter().map(|w| serde_json::json!({"place": w.place, "leaving": w.leaving, "text": w.text, "armed": w.armed})).collect();
    serde_json::json!({"places": places, "waiting": waiting})
}

fn places_from(v: &serde_json::Value) -> Places {
    let places = v.get("places").and_then(|p| serde_json::from_value(p.clone()).ok()).unwrap_or_default();
    let waiting = v
        .get("waiting")
        .and_then(|w| w.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|w| Some(island_core::places::Waiting { place: w.get("place")?.as_str()?.to_string(), leaving: w.get("leaving")?.as_bool()?, text: w.get("text")?.as_str()?.to_string(), armed: w.get("armed").and_then(|a| a.as_bool()).unwrap_or(false) }))
                .collect()
        })
        .unwrap_or_default();
    (places, waiting)
}

pub fn load_places() -> Places {
    state_file("places.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).map_or_else(Default::default, |v| places_from(&v))
}

pub fn save_places_if_changed(places: &[(String, f64, f64)], waiting: &[island_core::places::Waiting]) {
    static PLACES: Last<(String, f64, f64)> = Last::new(None);
    static WAITING: Last<island_core::places::Waiting> = Last::new(None);
    // both are looked at, so that both are remembered
    if changed(&PLACES, places) | changed(&WAITING, waiting) {
        save_json("places.json", places_json((places, waiting)));
    }
}

#[cfg(test)]
mod places_tests {
    use super::{places_from, places_json};

    #[test]
    fn places_and_what_waits_for_them_come_back() {
        let places = vec![("uni".to_string(), 43.7, 10.4)];
        let waiting = vec![island_core::places::Waiting { place: "uni".into(), leaving: true, text: "call".into(), armed: true }];
        assert_eq!(places_from(&places_json((&places, &waiting))), (places, waiting));
        assert_eq!(places_from(&serde_json::json!({"waiting": [{"place": "x"}]})), (vec![], vec![]));
    }
}

/// The files on the shelf, kept between runs; those that are gone meanwhile are left out.
pub fn load_shelf() -> Vec<String> {
    let Some(text) = state_file("shelf.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default().into_iter().filter(|p| std::path::Path::new(p).exists()).collect()
}

pub fn save_shelf_if_changed(shelf: &[String]) {
    static LAST: Last<String> = Last::new(None);
    if changed(&LAST, shelf) {
        save_json("shelf.json", shelf.into());
    }
}

/// One task as it is kept on disk (`todos.json`).
pub fn todo_to_json(t: &island_ui::island::Todo) -> serde_json::Value {
    let mut o = serde_json::json!({"uid": t.uid, "text": t.text});
    let m = o.as_object_mut().expect("an object");
    if let Some(d) = t.due {
        m.insert("due".into(), d.into());
    }
    if !t.course.is_empty() {
        m.insert("course".into(), t.course.clone().into());
    }
    if t.est_min > 0 {
        m.insert("est".into(), t.est_min.into());
    }
    if let Some(r) = t.repeat.to_rrule() {
        m.insert("repeat".into(), r.into());
    }
    if t.synced {
        m.insert("synced".into(), true.into());
    }
    o
}

/// Reads a task from `todos.json`: the current form, or an older one (a bare string, or `daily` with the day it was last
/// ticked), which becomes a task that repeats every day and rests until tomorrow when it was ticked today.
pub fn todo_from_json(v: &serde_json::Value, today: i64, index: usize) -> Option<island_ui::island::Todo> {
    use island_core::task::Repeat;
    let uid = |v: &serde_json::Value| v.get("uid").and_then(|u| u.as_str()).filter(|u| !u.is_empty()).map_or_else(|| format!("oi-old-{today:x}-{index}-{}", std::process::id()), str::to_string);
    match v {
        serde_json::Value::String(t) => Some(island_ui::island::Todo { uid: uid(v), text: t.clone(), ..Default::default() }),
        o => {
            let text = o.get("text")?.as_str()?.to_string();
            if o.get("daily").and_then(|d| d.as_bool()).unwrap_or(false) {
                let done = o.get("done_day").and_then(|d| d.as_i64()).unwrap_or(i64::MIN);
                return Some(island_ui::island::Todo { uid: uid(v), text, repeat: Repeat::Daily, due: Some(if done >= today { done + 1 } else { today }), ..Default::default() });
            }
            Some(island_ui::island::Todo {
                uid: uid(v),
                text,
                due: o.get("due").and_then(|d| d.as_i64()),
                course: o.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(),
                est_min: o.get("est").and_then(|e| e.as_u64()).unwrap_or(0) as u32,
                repeat: o.get("repeat").and_then(|r| r.as_str()).map_or(Repeat::Never, Repeat::from_rrule),
                synced: o.get("synced").and_then(|s| s.as_bool()).unwrap_or(false),
            })
        }
    }
}

/// Today as days since 1970 (local).
pub fn local_day() -> i64 {
    use chrono::Datelike;
    chrono::Local::now().date_naive().num_days_from_ce() as i64 - 719_163
}

/// The to-do list, kept between runs.
pub fn load_todos() -> Vec<island_ui::island::Todo> {
    let Some(text) = state_file("todos.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let today = local_day();
    items.iter().enumerate().filter_map(|(i, v)| todo_from_json(v, today, i)).collect()
}

pub fn save_todos_if_changed(todos: &[island_ui::island::Todo]) {
    static LAST: Last<island_ui::island::Todo> = Last::new(None);
    if changed(&LAST, todos) {
        save_json("todos.json", todos.iter().map(todo_to_json).collect());
    }
}

/// Study time done (day, course, minutes), kept between runs.
pub fn load_study_log() -> Vec<(i64, String, u32)> {
    let Some(text) = state_file("study-log.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items.iter().filter_map(|v| Some((v.get("day")?.as_i64()?, v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(), v.get("min")?.as_u64()? as u32))).collect()
}

pub fn save_study_log_if_changed(log: &[(i64, String, u32)]) {
    static LAST: Last<(i64, String, u32)> = Last::new(None);
    if !changed(&LAST, log) {
        return;
    }
    let items: Vec<serde_json::Value> = log.iter().map(|(d, c, m)| serde_json::json!({"day": d, "course": c, "min": m})).collect();
    save_json("study-log.json", items.into());
}

/// The notification history, kept between runs: (id, app, summary, body, epoch ms). Pictures are not kept.
pub fn load_history() -> Vec<(island_ui::model::Incoming, i64)> {
    let Some(text) = state_file("notification-history.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    items
        .iter()
        .map(|v| (island_ui::model::Incoming { id: v.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u32, app: s(v, "app"), summary: s(v, "summary"), body: s(v, "body"), desktop_entry: s(v, "desktop_entry"), ..Default::default() }, v.get("at").and_then(|x| x.as_i64()).unwrap_or(0)))
        .collect()
}

pub fn save_history_if_changed(history: &[(island_ui::model::Incoming, i64)]) {
    // which notifications are in it, and when they came: comparing those costs nothing, unlike the whole of them
    static LAST: Mutex<Vec<(u32, i64)>> = Mutex::new(Vec::new());
    let Ok(mut last) = LAST.lock() else { return };
    if last.iter().copied().eq(history.iter().map(|(n, at)| (n.id, *at))) {
        return;
    }
    *last = history.iter().map(|(n, at)| (n.id, *at)).collect();
    let items: Vec<serde_json::Value> = history.iter().map(|(n, at)| serde_json::json!({"id": n.id, "app": n.app, "summary": n.summary, "body": n.body, "desktop_entry": n.desktop_entry, "at": at})).collect();
    save_json("notification-history.json", items.into());
}

#[cfg(test)]
mod organizer_state_tests {
    use super::*;
    use island_core::task::Repeat;

    #[test]
    fn a_task_survives_the_disk() {
        let t = island_ui::island::Todo { uid: "oi-1".into(), text: "Report".into(), due: Some(20_470), course: "algoritmi".into(), est_min: 120, repeat: Repeat::Weekly(0b0001010), synced: true };
        assert_eq!(todo_from_json(&todo_to_json(&t), 20_000, 0), Some(t));
        let plain = island_ui::island::Todo { uid: "oi-2".into(), text: "x".into(), ..Default::default() };
        assert_eq!(todo_to_json(&plain).as_object().unwrap().len(), 2, "only what is set is written");
    }

    #[test]
    fn the_older_forms_of_the_list_still_load() {
        let today = 20_000;
        let bare = todo_from_json(&serde_json::json!("Buy milk"), today, 3).unwrap();
        assert_eq!((bare.text.as_str(), bare.repeat, bare.due), ("Buy milk", Repeat::Never, None));
        assert!(!bare.uid.is_empty(), "it gets an id, so that it can be kept on a server");
        let ticked = todo_from_json(&serde_json::json!({"text": "charger", "daily": true, "done_day": today}), today, 0).unwrap();
        assert_eq!((ticked.repeat, ticked.due), (Repeat::Daily, Some(today + 1)), "ticked today: it rests until tomorrow");
        let waiting = todo_from_json(&serde_json::json!({"text": "charger", "daily": true, "done_day": today - 5}), today, 0).unwrap();
        assert_eq!(waiting.due, Some(today), "not ticked today: it is due today");
        assert!(todo_from_json(&serde_json::json!({"nope": 1}), today, 0).is_none());
        assert_ne!(bare.uid, todo_from_json(&serde_json::json!("Other"), today, 4).unwrap().uid, "each gets its own id");
    }
}

#[cfg(test)]
mod writing_tests {
    use super::*;

    #[test]
    fn a_list_is_news_when_it_changes_and_an_empty_first_one_is_not() {
        let last: Last<u32> = Last::new(None);
        assert!(!changed(&last, &[]));
        assert!(changed(&last, &[1]));
        assert!(!changed(&last, &[1]));
        assert!(changed(&last, &[]), "emptied: the file must say so");
        assert!(!changed(&last, &[]));
    }

    #[test]
    fn a_file_is_replaced_whole_for_the_owner_only_and_a_link_stays_one() {
        let dir = std::env::temp_dir().join(format!("oi-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        let file = dir.join("todos.json");
        write_whole(&file, "one").unwrap();
        write_whole(&file, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "nothing left beside it");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
            let (target, link) = (dir.join("elsewhere/pins.json"), dir.join("pins.json"));
            std::fs::write(&target, "old").unwrap();
            std::os::unix::fs::symlink(&target, &link).unwrap();
            write_whole(&link, "new").unwrap();
            assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        }
        // a write that cannot be made leaves nothing behind and says so
        assert!(write_whole(&dir.join("no/such/folder"), "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
