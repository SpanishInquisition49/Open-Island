//! The notification daemon (org.freedesktop.Notifications) and its fallback, the bus watcher.
//!
//! The island answers the whole spec: ids and replacement, synchronous tags, pictures (`image-data`,
//! `image-path`, the app icon), timeouts, urgency, progress values, resident notifications, sounds,
//! actions, inline replies (`NotificationReplied`, as KDE and others define it) and the closing signals.

use super::{art, icons, sound, Reporter};
use crate::config::Settings;
use island_ui::model::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use zbus::zvariant::{OwnedValue, Value};

const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

pub enum Msg {
    Cmd(Command),
    /// Sent by the daemon's own methods (the bus calls CloseNotification).
    Closed(u32, u32),
}

struct Shared {
    next_id: u32,
    tags: HashMap<String, u32>,
}

struct Daemon {
    rep: Reporter,
    tx: Sender<Msg>,
    shared: Arc<Mutex<Shared>>,
    dnd: Arc<AtomicBool>,
    /// Only used for the first read; every notification reads the file again so a new sound applies at once.
    _settings: Arc<Settings>,
}

/// Everything a `Notify` call carries, decoded.
#[derive(Default, Debug, PartialEq)]
pub struct Hints {
    pub critical: bool,
    pub resident: bool,
    pub suppress_sound: bool,
    pub sound_file: Option<String>,
    pub desktop_entry: String,
    pub progress: Option<u8>,
    pub image_path: Option<String>,
    pub tag: Option<String>,
}

pub fn read_hints(h: &HashMap<String, OwnedValue>) -> Hints {
    let s = |k: &str| match h.get(k).map(|v| &**v) {
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    };
    let b = |k: &str| matches!(h.get(k).map(|v| &**v), Some(Value::Bool(true)));
    let int = |k: &str| match h.get(k).map(|v| &**v) {
        Some(Value::I32(v)) => Some(*v as i64),
        Some(Value::U32(v)) => Some(*v as i64),
        Some(Value::I64(v)) => Some(*v),
        Some(Value::U8(v)) => Some(*v as i64),
        _ => None,
    };
    Hints {
        critical: matches!(h.get("urgency").map(|v| &**v), Some(Value::U8(2))),
        resident: b("resident"),
        suppress_sound: b("suppress-sound"),
        sound_file: s("sound-file"),
        desktop_entry: s("desktop-entry").unwrap_or_default(),
        progress: int("value").map(|v| v.clamp(0, 100) as u8),
        image_path: s("image-path").or_else(|| s("image_path")),
        tag: s("x-canonical-private-synchronous").or_else(|| s("synchronous")).or_else(|| s("private-synchronous")),
    }
}

/// The raw-pixels hint `(iiibiiay)`: width, height, rowstride, has_alpha, bits per sample, channels, data.
pub fn image_from_hint(v: &Value) -> Option<Image> {
    let Value::Structure(st) = v else { return None };
    let f = st.fields();
    let int = |i: usize| match f.get(i)? {
        Value::I32(v) => Some(*v),
        _ => None,
    };
    let (w, h, stride, bits, ch) = (int(0)?, int(1)?, int(2)?, int(4)?, int(5)?);
    let Value::Array(a) = f.get(6)? else { return None };
    let data: Vec<u8> = a.iter().filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None }).collect();
    rgba_from_raw(w, h, stride, bits, ch, &data)
}

/// Raw pixels (3 or 4 channels, 8 bits) to straight RGBA8, honouring the row stride.
pub fn rgba_from_raw(w: i32, h: i32, stride: i32, bits: i32, channels: i32, data: &[u8]) -> Option<Image> {
    if w <= 0 || h <= 0 || bits != 8 || !(channels == 3 || channels == 4) || w > 4096 || h > 4096 || stride < w * channels {
        return None;
    }
    let (w, h, stride, ch) = (w as usize, h as usize, stride as usize, channels as usize);
    if data.len() < stride * (h - 1) + w * ch {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let p = &data[y * stride + x * ch..];
            rgba.extend_from_slice(&[p[0], p[1], p[2], if ch == 4 { p[3] } else { 255 }]);
        }
    }
    let img = Image { id: 0, width: w as u32, height: h as u32, rgba: Arc::new(rgba) };
    Some(art::reid(img))
}

pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Daemon {
    fn get_capabilities(&self) -> Vec<String> {
        ["body", "actions", "persistence", "inline-reply", "icon-static", "sound", "x-canonical-private-synchronous"].iter().map(|s| s.to_string()).collect()
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("open-island".into(), "open-island".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(&mut self, app_name: String, replaces_id: u32, app_icon: String, summary: String, body: String, actions: Vec<String>, hints: HashMap<String, OwnedValue>, timeout: i32) -> u32 {
        let hs = read_hints(&hints);
        let body = strip_markup(&body);
        let id = {
            let mut sh = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let tagged = hs.tag.as_ref().and_then(|t| sh.tags.get(t).copied());
            let id = if replaces_id != 0 {
                replaces_id
            } else if let Some(id) = tagged {
                id
            } else {
                sh.next_id += 1;
                sh.next_id
            };
            if let Some(t) = &hs.tag {
                sh.tags.insert(t.clone(), id);
            }
            id
        };
        let settings = crate::config::load();
        // filters: a notification can be dropped altogether
        if settings.filters.iter().any(|f| f.matches(&app_name, &summary, &body) && !f.show) {
            return id;
        }
        let dnd = self.dnd.load(Ordering::Relaxed);
        if let Some(file) = sound::choose(&settings, &app_name, &hs.desktop_entry, &summary, &body, hs.sound_file.as_deref(), hs.suppress_sound, dnd, hs.critical) {
            sound::play(&file);
        }
        let image = ["image-data", "image_data", "icon_data"]
            .iter()
            .find_map(|k| hints.get(*k).and_then(|v| image_from_hint(v)))
            .or_else(|| hs.image_path.as_deref().and_then(|p| art::load(p)));
        let icon_name = if app_icon.is_empty() { hs.desktop_entry.clone() } else { app_icon };
        let app_icon = icons::load(&icon_name);
        let acts = actions.chunks(2).filter(|c| c.len() == 2).map(|c| NotifAction { key: c[0].clone(), label: c[1].clone() }).collect();
        self.rep.input(Input::Notification(Incoming {
            id,
            app: app_name,
            summary,
            body,
            critical: hs.critical,
            actions: acts,
            image,
            app_icon,
            expire_ms: match timeout {
                t if t < 0 => None,
                t => Some(t as i64),
            },
            progress: hs.progress,
            resident: hs.resident,
        }));
        id
    }

    fn close_notification(&self, id: u32) {
        self.rep.input(Input::NotificationClosed(id));
        let _ = self.tx.send(Msg::Closed(id, CloseReason::Closed as u32));
    }
}

/// Runs the daemon when the name is free (never queueing for it), the bus watcher otherwise.
pub fn start(rep: Reporter, cmd_rx: Receiver<Command>, settings: Arc<Settings>, initial_dnd: bool) {
    std::thread::spawn(move || {
        let dnd = Arc::new(AtomicBool::new(initial_dnd));
        let (tx, rx) = channel::<Msg>();
        let daemon = Daemon { rep: rep.clone(), tx: tx.clone(), shared: Arc::new(Mutex::new(Shared { next_id: 0, tags: HashMap::new() })), dnd: dnd.clone(), _settings: settings };
        let built = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.serve_at(PATH, daemon))
            .and_then(|b| b.build())
            .and_then(|conn| {
                conn.request_name_with_flags(IFACE, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
                Ok(conn)
            });
        match built {
            Ok(conn) => {
                log::info!("open-island is the notification daemon");
                std::thread::spawn(move || {
                    for c in cmd_rx {
                        let _ = tx.send(Msg::Cmd(c));
                    }
                });
                for m in rx {
                    macro_rules! emit {
                        ($name:expr, $body:expr) => {
                            let _ = conn.emit_signal(None::<&str>, PATH, IFACE, $name, $body);
                        };
                    }
                    match m {
                        Msg::Closed(id, reason) => { emit!("NotificationClosed", &(id, reason)); }
                        Msg::Cmd(Command::NotificationClosed { id, reason }) => { emit!("NotificationClosed", &(id, reason as u32)); }
                        Msg::Cmd(Command::InvokeAction { id, key }) => { emit!("ActionInvoked", &(id, key)); }
                        Msg::Cmd(Command::NotificationReplied { id, text }) => { emit!("NotificationReplied", &(id, text)); }
                        Msg::Cmd(Command::SetDnd(on)) => dnd.store(on, Ordering::Relaxed),
                        _ => {}
                    }
                }
            }
            Err(e) => {
                log::info!("another notification daemon is running ({e}): watching the bus instead (no actions)");
                watch_bus(rep);
            }
        }
    });
}

/// Fallback: eavesdrop on `Notify` calls so the island can at least show them.
fn watch_bus(rep: Reporter) {
    let rule = "type='method_call',interface='org.freedesktop.Notifications',member='Notify'";
    let Ok(monitor_conn) = zbus::blocking::Connection::session() else { return };
    let Ok(mon) = zbus::blocking::Proxy::new(&monitor_conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus.Monitoring") else { return };
    if mon.call_method("BecomeMonitor", &(vec![rule], 0u32)).is_err() {
        log::warn!("cannot monitor the bus: notifications will not appear on the island");
        return;
    }
    let mut next_id = 1_000_000u32;
    for msg in zbus::blocking::MessageIterator::from(monitor_conn) {
        let Ok(msg) = msg else { continue };
        let Ok((app, _replaces, icon, summary, body, _actions, hints, timeout)) =
            msg.body().deserialize::<(String, u32, String, String, String, Vec<String>, HashMap<String, OwnedValue>, i32)>()
        else {
            continue;
        };
        next_id += 1;
        let hs = read_hints(&hints);
        let image = ["image-data", "image_data", "icon_data"].iter().find_map(|k| hints.get(*k).and_then(|v| image_from_hint(v))).or_else(|| hs.image_path.as_deref().and_then(art::load));
        rep.input(Input::Notification(Incoming {
            id: next_id,
            app,
            summary,
            body: strip_markup(&body),
            critical: hs.critical,
            actions: vec![],
            image,
            app_icon: icons::load(&icon),
            expire_ms: (timeout >= 0).then_some(timeout as i64),
            progress: hs.progress,
            resident: hs.resident,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::StructureBuilder;

    fn hints(pairs: Vec<(&str, Value<'static>)>) -> HashMap<String, OwnedValue> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), OwnedValue::try_from(v).unwrap())).collect()
    }

    #[test]
    fn reads_the_hints() {
        let h = read_hints(&hints(vec![
            ("urgency", Value::U8(2)),
            ("resident", Value::Bool(true)),
            ("value", Value::I32(140)),
            ("desktop-entry", Value::from("org.telegram.desktop")),
            ("x-canonical-private-synchronous", Value::from("volume")),
            ("sound-file", Value::from("/s/a.ogg")),
        ]));
        assert!(h.critical && h.resident && !h.suppress_sound);
        assert_eq!(h.progress, Some(100), "clamped");
        assert_eq!(h.desktop_entry, "org.telegram.desktop");
        assert_eq!(h.tag.as_deref(), Some("volume"));
        assert_eq!(h.sound_file.as_deref(), Some("/s/a.ogg"));
        assert_eq!(read_hints(&HashMap::new()), Hints::default());
    }

    #[test]
    fn decodes_raw_pixels_with_a_row_stride() {
        // 2x2 RGB, stride 8 (2 padding bytes per row)
        let data = [255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255, 9, 9];
        let img = rgba_from_raw(2, 2, 8, 8, 3, &data).unwrap();
        assert_eq!(&img.rgba[..], &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);
        assert!(rgba_from_raw(2, 2, 8, 8, 3, &data[..10]).is_none(), "short data");
        assert!(rgba_from_raw(0, 2, 8, 8, 3, &data).is_none());
        assert!(rgba_from_raw(2, 2, 8, 16, 3, &data).is_none(), "only 8 bits");
        assert!(rgba_from_raw(2, 2, 4, 8, 3, &data).is_none(), "stride below the row size");
    }

    #[test]
    fn decodes_the_image_data_hint() {
        let st = StructureBuilder::new()
            .add_field(1i32)
            .add_field(1i32)
            .add_field(4i32)
            .add_field(true)
            .add_field(8i32)
            .add_field(4i32)
            .add_field(vec![10u8, 20, 30, 40])
            .build()
            .unwrap();
        let img = image_from_hint(&Value::Structure(st)).unwrap();
        assert_eq!((img.width, img.height, &img.rgba[..]), (1, 1, &[10u8, 20, 30, 40][..]));
        assert!(image_from_hint(&Value::from("nope")).is_none());
    }

    #[test]
    fn strips_markup() {
        assert_eq!(strip_markup("<b>Hi</b> &amp; bye"), "Hi & bye");
    }
}
