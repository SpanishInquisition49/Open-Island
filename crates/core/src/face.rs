//! What the island shows: the faces, their sizes and the choice between them.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Idle,
    IdleHover,
    Qs,
    FocusMini,
    FocusFull,
    Cal,
    Sys,
    Notifs,
    Settings,
    Options,
    Audio,
    Today,
    Send,
    Weather,
    Update,
    Clip,
    MeetMini,
    Meeting,
    Rain,
    Progress,
    MediaMini,
    MediaFull,
    MediaLyric,
    Osd,
    Power,
    Device,
    TimerMini,
    TimerFull,
    TimerDone,
    Notif,
    NotifAct,
    Reply,
    Bar,
}

impl Face {
    /// Base size in logical pixels (width, height). Compact faces grow with their status icons.
    pub fn size(self) -> (f64, f64) {
        use Face::*;
        match self {
            Idle => (150.0, 28.0),
            IdleHover => (450.0, 104.0),
            Qs => (410.0, 314.0),
            FocusMini => (230.0, 28.0),
            FocusFull => (340.0, 116.0),
            Cal => (420.0, 300.0),
            Sys => (420.0, 286.0),
            Notifs => (420.0, 120.0),
            Settings => (440.0, 380.0),
            Options => (440.0, 300.0),
            Audio => (410.0, 300.0),
            Today => (410.0, 200.0),
            Send => (410.0, 220.0),
            Weather => (420.0, 318.0),
            Update => (400.0, 64.0),
            Clip => (400.0, 236.0),
            MeetMini => (270.0, 28.0),
            Meeting => (410.0, 66.0),
            Rain => (330.0, 64.0),
            Progress => (360.0, 64.0),
            MediaMini => (290.0, 28.0),
            MediaFull => (420.0, 160.0),
            MediaLyric => (420.0, 234.0),
            Osd => (270.0, 46.0),
            Power => (300.0, 46.0),
            Device => (330.0, 46.0),
            TimerMini => (210.0, 28.0),
            TimerFull => (320.0, 112.0),
            TimerDone => (330.0, 64.0),
            Notif => (410.0, 82.0),
            NotifAct => (410.0, 150.0),
            Reply => (430.0, 168.0),
            Bar => (430.0, 92.0),
        }
    }

    /// Compact faces are the pill at rest (they can carry a side bubble).
    pub fn is_compact(self) -> bool {
        matches!(self, Face::Idle | Face::MediaMini | Face::TimerMini | Face::MeetMini | Face::FocusMini)
    }
}

/// The second activity shown in a bubble next to the compact pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Timer,
    Meeting,
    Focus,
    Media,
    Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Clip,
    Timer,
    Meeting,
    Media,
    Qs,
    Notifs,
    Settings,
    Options,
    Audio,
    Today,
    Send,
    Weather,
    Cal,
    Sys,
    Focus,
    Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifFace {
    Plain,
    Actions,
}

/// Snapshot of the state the choice depends on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub barring: bool,
    pub replying: bool,
    pub osd: bool,
    pub notif: Option<NotifFaceOpt>,
    pub timer_done: bool,
    pub power: bool,
    pub device: bool,
    pub op: bool,
    pub op_flash: bool,
    pub meeting: bool,
    pub rain: bool,
    pub update: bool,
    pub big: bool,
    pub page: Option<Page>,
    pub clipboard: bool,
    pub media: bool,
    pub lyrics: bool,
    pub timer: bool,
    pub focus: bool,
    pub soon: bool,
}

pub type NotifFaceOpt = NotifFace;

/// Chooses what the island shows. Returns the face and, in the compact state, an optional second
/// activity for the side bubble. Hovering the side bubble opens that activity: the caller sets `page`.
pub fn choose(s: &Snapshot) -> (Face, Option<Side>) {
    use Face::*;
    if s.barring {
        return (Bar, None);
    }
    if s.replying {
        return (Reply, None);
    }
    // on the quick settings page the sliders change volume/brightness: their OSD would replace the page
    if s.osd && !(s.big && s.page == Some(Page::Qs)) {
        return (Osd, None);
    }
    match s.notif {
        Some(NotifFace::Plain) => return (Notif, None),
        Some(NotifFace::Actions) => return (NotifAct, None),
        None => {}
    }
    if s.timer_done {
        return (TimerDone, None);
    }
    if s.power {
        return (Power, None);
    }
    if s.device {
        return (Device, None);
    }
    if s.op_flash && s.op {
        return (Progress, None);
    }
    if s.meeting {
        return (Meeting, None);
    }
    if s.rain {
        return (Rain, None);
    }
    if s.update {
        return (Update, None);
    }
    let media_face = if s.lyrics { MediaLyric } else { MediaFull };
    if s.big {
        let p = s.page;
        let face = match p {
            Some(Page::Clip) if s.clipboard => Some(Clip),
            Some(Page::Home) => Some(IdleHover),
            Some(Page::Qs) => Some(Qs),
            Some(Page::Cal) => Some(Cal),
            Some(Page::Sys) => Some(Sys),
            Some(Page::Notifs) => Some(Notifs),
            Some(Page::Settings) => Some(Settings),
            Some(Page::Options) => Some(Options),
            Some(Page::Audio) => Some(Audio),
            Some(Page::Today) => Some(Today),
            Some(Page::Send) => Some(Send),
            Some(Page::Weather) => Some(Weather),
            Some(Page::Media) if s.media => Some(media_face),
            Some(Page::Timer) if s.timer => Some(TimerFull),
            Some(Page::Focus) if s.focus => Some(FocusFull),
            Some(Page::Meeting) if s.soon => Some(Meeting),
            Some(Page::Op) if s.op => Some(Progress),
            _ => None,
        };
        if let Some(f) = face {
            return (f, None);
        }
        if s.media {
            return (media_face, None);
        }
        if s.timer {
            return (TimerFull, None);
        }
        return (IdleHover, None);
    }
    let op = s.op.then_some(Side::Op);
    if s.media {
        let side = if s.timer {
            Some(Side::Timer)
        } else if s.focus {
            Some(Side::Focus)
        } else if s.soon {
            Some(Side::Meeting)
        } else {
            op
        };
        return (MediaMini, side);
    }
    if s.timer {
        let side = if s.focus {
            Some(Side::Focus)
        } else if s.soon {
            Some(Side::Meeting)
        } else {
            op
        };
        return (TimerMini, side);
    }
    if s.focus {
        return (FocusMini, if s.soon { Some(Side::Meeting) } else { op });
    }
    if s.soon {
        return (MeetMini, op);
    }
    (Idle, op)
}

/// Pages a horizontal swipe cycles through while the island is open: the live activities first, then
/// the fixed pages.
pub fn swipe_pages(media: bool, timer: bool, focus: bool, soon: bool, op: bool, notifs: bool, weather: bool) -> Vec<Page> {
    let mut out = vec![];
    if media {
        out.push(Page::Media);
    }
    if timer {
        out.push(Page::Timer);
    }
    if focus {
        out.push(Page::Focus);
    }
    if soon {
        out.push(Page::Meeting);
    }
    if op {
        out.push(Page::Op);
    }
    out.extend([Page::Home, Page::Cal]);
    if weather {
        out.push(Page::Weather);
    }
    out.extend([Page::Qs, Page::Sys]);
    if notifs {
        out.push(Page::Notifs);
    }
    out
}

/// The page after (dir = 1) or before (dir = -1) `current`, wrapping around. An unknown current page
/// starts from the first (or the last going back).
pub fn cycle_page(pages: &[Page], current: Option<Page>, dir: i32) -> Option<Page> {
    if pages.is_empty() {
        return None;
    }
    let idx = current.and_then(|c| pages.iter().position(|p| *p == c));
    Some(match idx {
        None => {
            if dir < 0 {
                pages[pages.len() - 1]
            } else {
                pages[0]
            }
        }
        Some(i) => pages[(i as i32 + dir).rem_euclid(pages.len() as i32) as usize],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(s: Snapshot) -> Face {
        choose(&s).0
    }
    fn snap() -> Snapshot {
        Snapshot::default()
    }

    #[test]
    fn rest_and_hover() {
        assert_eq!(face(snap()), Face::Idle);
        assert_eq!(face(Snapshot { big: true, ..snap() }), Face::IdleHover);
        assert_eq!(face(Snapshot { big: true, media: true, ..snap() }), Face::MediaFull);
        assert_eq!(face(Snapshot { big: true, media: true, lyrics: true, ..snap() }), Face::MediaLyric);
        assert_eq!(face(Snapshot { big: true, media: true, page: Some(Page::Home), ..snap() }), Face::IdleHover);
        assert_eq!(face(Snapshot { big: true, timer: true, ..snap() }), Face::TimerFull);
        assert_eq!(face(Snapshot { big: true, page: Some(Page::Clip), clipboard: true, ..snap() }), Face::Clip);
        assert_eq!(face(Snapshot { big: true, page: Some(Page::Clip), ..snap() }), Face::IdleHover);
        assert_eq!(face(Snapshot { big: true, media: true, page: Some(Page::Timer), ..snap() }), Face::MediaFull);
        assert_eq!(face(Snapshot { big: true, media: true, soon: true, page: Some(Page::Meeting), ..snap() }), Face::Meeting);
    }

    #[test]
    fn priorities() {
        assert_eq!(face(Snapshot { replying: true, osd: true, ..snap() }), Face::Reply);
        assert_eq!(face(Snapshot { osd: true, notif: Some(NotifFace::Plain), ..snap() }), Face::Osd);
        assert_eq!(face(Snapshot { notif: Some(NotifFace::Actions), power: true, ..snap() }), Face::NotifAct);
        assert_eq!(face(Snapshot { big: true, page: Some(Page::Qs), osd: true, ..snap() }), Face::Qs);
        assert_eq!(face(Snapshot { power: true, meeting: true, ..snap() }), Face::Power);
        assert_eq!(face(Snapshot { device: true, meeting: true, ..snap() }), Face::Device);
        assert_eq!(face(Snapshot { notif: Some(NotifFace::Plain), device: true, ..snap() }), Face::Notif, "a notification wins");
        assert_eq!(face(Snapshot { meeting: true, rain: true, ..snap() }), Face::Meeting);
    }

    #[test]
    fn bubbles() {
        assert_eq!(choose(&Snapshot { media: true, ..snap() }), (Face::MediaMini, None));
        assert_eq!(choose(&Snapshot { media: true, timer: true, soon: true, ..snap() }), (Face::MediaMini, Some(Side::Timer)));
        assert_eq!(choose(&Snapshot { timer: true, soon: true, ..snap() }), (Face::TimerMini, Some(Side::Meeting)));
        assert_eq!(choose(&Snapshot { soon: true, ..snap() }), (Face::MeetMini, None));
        assert_eq!(choose(&Snapshot { focus: true, soon: true, ..snap() }), (Face::FocusMini, Some(Side::Meeting)));
    }

    #[test]
    fn long_operations() {
        assert_eq!(face(Snapshot { op: true, op_flash: true, ..snap() }), Face::Progress);
        assert_eq!(face(Snapshot { op_flash: true, ..snap() }), Face::Idle);
        assert_eq!(choose(&Snapshot { op: true, ..snap() }), (Face::Idle, Some(Side::Op)));
        assert_eq!(choose(&Snapshot { op: true, media: true, timer: true, ..snap() }).1, Some(Side::Timer));
        assert_eq!(choose(&Snapshot { op: true, media: true, ..snap() }), (Face::MediaMini, Some(Side::Op)));
        assert_eq!(face(Snapshot { op: true, big: true, page: Some(Page::Op), ..snap() }), Face::Progress);
    }

    #[test]
    fn pages_cycle() {
        let pages = swipe_pages(true, false, false, true, false, true, false);
        assert_eq!(pages, vec![Page::Media, Page::Meeting, Page::Home, Page::Cal, Page::Qs, Page::Sys, Page::Notifs]);
        assert_eq!(swipe_pages(false, false, false, false, false, false, true), vec![Page::Home, Page::Cal, Page::Weather, Page::Qs, Page::Sys], "the weather sits after the calendar when there is a forecast");
        assert_eq!(cycle_page(&pages, Some(Page::Notifs), 1), Some(Page::Media));
        assert_eq!(cycle_page(&pages, Some(Page::Media), -1), Some(Page::Notifs));
        assert_eq!(cycle_page(&pages, None, 1), Some(Page::Media));
        assert_eq!(cycle_page(&pages, None, -1), Some(Page::Notifs));
        assert_eq!(cycle_page(&[], None, 1), None);
    }

    #[test]
    fn every_face_has_a_sane_size() {
        for f in [Face::Idle, Face::Qs, Face::Reply, Face::Progress, Face::MediaLyric] {
            let (w, h) = f.size();
            assert!(w >= 100.0 && h >= 20.0);
        }
        assert!(Face::Idle.is_compact() && !Face::Qs.is_compact());
    }
}
