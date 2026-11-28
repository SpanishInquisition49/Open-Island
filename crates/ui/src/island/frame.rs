//! Part of `Island` (see `mod.rs`).

use super::*;

/// What the summary of the held notifications says after their number.
const HELD_BACK: &str = "held back";
use super::card;

/// The longest between two frames while the edges of the pill move: a little more than the time between two new
/// spectra, which bring a frame by themselves.
const RIPPLE_STEP_MS: u32 = 40;

impl Island {
    // ---- the frame ----------------------------------------------------------------------------

    /// Size of a face, which can depend on the data it shows.
    pub fn face_size(&self, face: Face) -> (f64, f64) {
        let (mut w, mut h) = face.size();
        match face {
            Face::IdleHover => {
                h = 28.0 + 44.0 + 10.0 + 34.0 + 10.0;
                if self.next_event().is_some() {
                    h += 10.0 + 18.0 + 4.0;
                }
            }
            Face::Cal => {
                let weeks = calendar::month_grid(self.cal.year, self.cal.month).len() as f64;
                let n = self.day_events().len().clamp(1, CAL_ROWS) as f64;
                h = 14.0 + 34.0 + 4.0 + 16.0 + weeks * 30.0 + 10.0 + n * 24.0 + 20.0;
            }
            Face::Notifs => h = 14.0 + 34.0 + 6.0 + self.history_groups().len().clamp(1, 5) as f64 * 46.0 + if self.history.is_empty() { 20.0 } else { 8.0 },
            Face::Clip => h = 14.0 + 34.0 + 4.0 + self.data.clipboard.len().clamp(1, CLIP_ROWS) as f64 * 32.0 + 10.0,
            Face::FocusMini => w += self.icon_count(true) as f64 * ICON_W,
            Face::Settings => h = 14.0 + 34.0 + self.settings_content_height() + 14.0,
            Face::Options => h = 14.0 + 34.0 + 40.0 + crate::options::content_height(self.options_tab, self.data.config.filters.len()) + 14.0,
            Face::Qs => h = self.qs_layout().height,
            Face::Audio => h = self.audio_height(),
            Face::Today => h = self.today_height(),
            Face::Plan => h = self.plan_height(),
            Face::Find => h = self.find_height(),
            Face::Bar if !self.picks().is_empty() => h += crate::faces::PICK_ROW,
            Face::TimerFull => h += self.extra_timers.len().min(3) as f64 * 28.0 + if self.extra_timers.is_empty() { 0.0 } else { 8.0 },
            Face::Send => h = self.send_height(),
            Face::Phone => h = self.phone_height(),
            Face::Grid => h = self.grid_height(),
            Face::Map => h = 14.0 + 34.0 + MAP_H + 8.0 + 34.0 + 10.0,
            Face::Activity => h = self.activity_height(),
            Face::Shelf => h = self.shelf_height(),
            Face::Sys => h = 14.0 + 34.0 + self.sys_rows() as f64 * 28.0 + 14.0,
            Face::Idle => {
                w += self.icon_count(false) as f64 * ICON_W;
                // the countdown beside the clock: about 0.55 em a character at 12 px
                if let Some((text, _)) = self.countdown() {
                    w += text.chars().count() as f64 * 6.6 + 18.0;
                }
            }
            Face::Catch => {
                let swell = self.fx.catch.as_ref().map_or(0.0, |c| c.swell(self.now));
                (w, h) = (w + 40.0 * swell, h + 12.0 * swell);
            }
            Face::MediaMini | Face::TimerMini | Face::MeetMini => {
                w += self.icon_count(true) as f64 * ICON_W;
                if face == Face::MediaMini {
                    w = w.max(self.lyric_pill_width(w));
                }
            }
            _ => {}
        }
        if has_dock(face) {
            h += DOCK_H;
            w = w.max((self.dock_pages().len() + 1) as f64 * 30.0 + 28.0);
        }
        (w, h)
    }

    /// The width the compact pill needs to show the line being sung whole (the option puts it where the title is): left
    /// margin, the line (about 0.54 em a character at 12 px), the equalizer, the clock and the icons already counted in `w`.
    /// Never narrower than `w`, never wider than `LYRIC_PILL_MAX`.
    fn lyric_pill_width(&self, w: f64) -> f64 {
        if !self.cfg.lyrics_in_pill || !self.data.media.as_ref().is_some_and(|m| m.playing) {
            return w;
        }
        let Some((line, _)) = self.lyric_now() else { return w };
        let icons = if self.cfg.status_icons { w - Face::MediaMini.size().0 } else { 0.0 };
        let clock = if self.cfg.clock_in_pill { 50.0 } else { 0.0 };
        (38.0 + line.chars().count() as f64 * 12.0 * 0.54 + 26.0 + 14.0 + clock + icons).clamp(w, LYRIC_PILL_MAX.max(w))
    }

    /// Rows of the system page that have data.
    pub(crate) fn sys_rows(&self) -> usize {
        let Some(st) = &self.data.system else { return 1 };
        let mut n = 2; // cpu, cores
        n += st.memory.is_some() as usize + st.gpu.is_some() as usize + self.data.battery.is_some() as usize + st.disk.is_some() as usize;
        n + self.data.devices.len().min(4) + 1 + self.data.culprit.is_some() as usize + self.data.paused.len().min(3) + self.data.mic_users.len().min(2) + self.data.camera_users.len().min(2) // devices, network, the program to blame, the paused ones
    }

    /// Height of the settings page below its header: four sections and their rows.
    pub(crate) fn settings_content_height(&self) -> f64 {
        let cals = self.data.config.calendars.len().min(4) as f64;
        // section titles (6 x 22), calendar rows, the two add rows, latitude, longitude, home, train, font, sound, map key, the note line
        // and, while the island asks for passwords, the face choice with the line that says what it means
        let face = if self.data.config.face.is_some() { 22.0 + 34.0 + FACE_NOTE_H } else { 0.0 };
        6.0 * 22.0 + (cals + 3.0 + 2.0 + 2.0 + 1.0 + 1.0 + 1.0) * 34.0 + face + 20.0
    }

    pub(crate) fn day_events(&self) -> Vec<&calendar::Event> {
        let c = &self.cal;
        let start = self.local_midnight_ms(c.year, c.month, c.day);
        let end = self.local_midnight_ms(c.year, c.month, c.day + 1);
        calendar::events_between(self.calendar_events(), start, end)
    }

    /// The last position the list of the day can start from.
    pub(super) fn cal_last_first(&self) -> usize {
        self.day_events().len().saturating_sub(CAL_ROWS)
    }

    /// Where it starts now (the events may have changed since it was moved).
    pub(crate) fn cal_first(&self) -> usize {
        self.cal.first.min(self.cal_last_first())
    }

    /// The local offset from UTC at an instant: the calendar page goes where daylight saving is another.
    pub(crate) fn offset_at(&self, ms: i64) -> i32 {
        self.data.zone.map_or(self.data.utc_offset_secs, |zone| zone(ms))
    }

    /// Local midnight of a date (a day past the end of the month counts on into the next).
    pub(crate) fn local_midnight_ms(&self, y: i32, m: u32, d: u32) -> i64 {
        let near = crate::clock::midnight_ms(y, m, d, self.data.utc_offset_secs);
        let first = crate::clock::midnight_ms(y, m, d, self.offset_at(near));
        crate::clock::midnight_ms(y, m, d, self.offset_at(first))
    }

    /// Whether the stretch that is always read holds the whole of a month.
    fn month_covered(&self, y: i32, m: u32) -> bool {
        let (from, to) = self.data.events_span;
        from < to && from <= self.local_midnight_ms(y, m, 1) && self.local_midnight_ms(y, m, calendar::days_in_month(y, m) + 1) <= to
    }

    fn month_read(&self, y: i32, m: u32) -> Option<&MonthEvents> {
        self.data.months.iter().find(|r| (r.year, r.month) == (y, m))
    }

    /// The events the calendar page draws its month from, by start: those always read, and for a month that goes past
    /// them, what was fetched for it outside their stretch.
    pub(crate) fn calendar_events(&self) -> Vec<&calendar::Event> {
        let (y, m) = (self.cal.year, self.cal.month);
        let mut out: Vec<&calendar::Event> = self.data.events.iter().collect();
        if let Some(read) = self.month_read(y, m).filter(|_| !self.month_covered(y, m)) {
            let (from, to) = self.data.events_span;
            let more = read.events.iter().filter(|e| (e.end_ms <= from || e.start_ms >= to) && !self.data.events.iter().any(|k| k.start_ms == e.start_ms && k.title == e.title));
            out.extend(more);
            out.sort_by_key(|e| e.start_ms);
        }
        out
    }

    /// What to say under the month when the day shown has no event.
    pub(crate) fn no_events_text(&self) -> &'static str {
        let (y, m) = (self.cal.year, self.cal.month);
        if self.reads_months() && !self.month_covered(y, m) {
            return match self.month_read(y, m) {
                None => "Reading this month…",
                Some(r) if !r.complete => "Could not read the calendars",
                Some(_) => "No events",
            };
        }
        if self.data.events.is_empty() { "Calendar not synced yet" } else { "No events" }
    }

    /// There is a calendar to ask: one in the settings, or the stretch was read (the island's own file).
    fn reads_months(&self) -> bool {
        !self.data.config.calendars.is_empty() || self.data.events_span != (0, 0)
    }

    /// The calendar page shows a month that is not at hand, or was read long ago: ask for it, once until it answers.
    pub(super) fn ask_month(&mut self, now: i64) {
        let data = &self.data;
        self.months_asked.retain(|(k, at)| now - at < MONTH_RETRY_MS && !data.months.iter().any(|r| (r.year, r.month) == *k && r.read_at >= *at));
        let (y, m) = (self.cal.year, self.cal.month);
        if !self.reads_months() || self.month_covered(y, m) || self.months_asked.iter().any(|(k, _)| *k == (y, m)) {
            return;
        }
        if self.month_read(y, m).is_some_and(|r| r.read_at > 0 && now - r.read_at < if r.complete { MONTH_STALE_MS } else { MONTH_RETRY_MS }) {
            return;
        }
        self.months_asked.push(((y, m), now));
        self.pending.push(Command::LoadCalendarMonth { year: y, month: m });
    }

    pub(crate) fn icon_count(&self, compact_busy: bool) -> usize {
        if !self.cfg.status_icons {
            return 0;
        }
        let p = self.data.privacy;
        let _ = compact_busy;
        let mic = p.mic || self.data.mic_muted == Some(true);
        usize::from(!self.shelf.is_empty()) + mic as usize + p.camera as usize + p.screen as usize + self.quiet() as usize + (!self.data.net.is_ok()) as usize
    }

    pub(super) fn sync_lyrics(&mut self) {
        if !self.cfg.lyrics {
            self.lyric_lines = None;
            return;
        }
        if self.lyric_src != self.data.lyrics_lrc {
            self.lyric_src = self.data.lyrics_lrc.clone();
            self.lyric_lines = self.lyric_src.as_deref().and_then(lyrics::parse_lrc);
            self.lyric_idx = self.lyric_lines.as_ref().map_or(0, |l| lyrics::index_at(l, self.media_position(), LYRIC_LEAD_MS));
            self.lyric_at = -100_000;
        }
        if let Some(lines) = &self.lyric_lines {
            let idx = lyrics::index_at(lines, self.media_position(), LYRIC_LEAD_MS);
            if idx != self.lyric_idx {
                // one line on is a scroll; a jump (a seek, the island waking from a long pause) is just there
                let jump = idx.abs_diff(self.lyric_idx) > 1;
                self.lyric_dir = if idx > self.lyric_idx { 1.0 } else { -1.0 };
                self.lyric_idx = idx;
                self.lyric_at = if jump { -100_000 } else { self.now };
            }
        }
    }

    pub(crate) fn media_position(&self) -> f64 {
        match &self.data.media {
            Some(m) if m.playing => (m.position_ms + (self.now - m.sampled_at_ms).max(0) as f64).min(m.length_ms.max(m.position_ms)),
            Some(m) => m.position_ms,
            None => 0.0,
        }
    }

    /// The lyric lines to draw with their slot (rows from the current one, fractional while scrolling), when
    /// the track has synced lyrics.
    pub(crate) fn lyric_strip(&self) -> Option<Vec<(String, f32)>> {
        let lines = self.lyric_lines.as_ref()?;
        let progress = (self.now - self.lyric_at) as f64 / lyrics::SCROLL_MS;
        let text = |n: i64| if n >= 1 && (n as usize) <= lines.len() { lines[n as usize - 1].text.clone() } else { String::new() };
        Some(lyrics::strip(self.lyric_idx, progress, self.lyric_dir).into_iter().map(|(n, slot)| (text(n), slot)).collect())
    }

    /// The line being sung and how far its entrance has come (0..=1), when the track has synced lyrics and one is due.
    pub(crate) fn lyric_now(&self) -> Option<(String, f64)> {
        let lines = self.lyric_lines.as_ref()?;
        let line = lines.get(self.lyric_idx.checked_sub(1)?)?;
        (!line.text.is_empty()).then(|| (line.text.clone(), lyrics::ease_out((self.now - self.lyric_at) as f64 / 300.0)))
    }

    /// The strip is still scrolling: frames are needed.
    pub(super) fn lyrics_scrolling(&self) -> bool {
        self.lyric_lines.is_some() && ((self.now - self.lyric_at) as f64) < lyrics::SCROLL_MS && matches!(self.shown, Face::MediaLyric | Face::MediaMini)
    }

    pub(super) fn snapshot(&mut self) -> (Snapshot, bool) {
        let now = self.now;
        let hover_open = (self.hovering && now - self.hover_since >= HOVER_INTENT_MS) || now < self.hover_left + HOVER_GRACE_MS;
        let big = hover_open || now < self.expand_until;
        if !big {
            self.page = None;
            if !self.hovering {
                self.hover_page = None;
            }
        }
        if big && self.page.is_none() {
            self.page = self.hover_page;
        }
        let media = self.data.media.as_ref().is_some_and(|m| !m.title.is_empty());
        let snap = Snapshot {
            authing: self.auth.is_some(),
            barring: self.bar.is_some(),
            replying: self.reply.is_some(),
            catching: self.fx.catch.is_some(),
            osd: (self.osd.is_some() || self.notice.is_some()) && now < self.osd_until,
            op: self.ops.op.is_some(),
            update: false,
            big,
            page: self.page,
            clipboard: self.cfg.clipboard_history,
            media,
            lyrics: media && self.lyric_lines.is_some(),
            timer: self.timer.is_some(),
            focus: self.focus_active(),
            soon: self.soon().is_some(),
        };
        (snap, big)
    }

    /// Notifications are held: asked for, or one of the automatic reasons holds.
    pub fn quiet(&self) -> bool {
        self.data.dnd || self.auto_quiet.is_some()
    }

    /// Why the island holds notifications by itself right now.
    pub fn auto_quiet_reason(&self) -> Option<&'static str> {
        self.auto_quiet
    }

    /// One card for what was held while the island was quiet.
    pub(super) fn recap(&mut self, since: i64, what: &str) {
        let mut apps: Vec<(String, usize)> = vec![];
        for (n, at) in self.history.iter().filter(|(n, at)| *at >= since && !is_own(n)) {
            let _ = at;
            match apps.iter_mut().find(|(a, _)| *a == n.app) {
                Some((_, c)) => *c += 1,
                None => apps.push((n.app.clone(), 1)),
            }
        }
        let total: usize = apps.iter().map(|(_, c)| c).sum();
        if total == 0 {
            return;
        }
        apps.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        let body = apps.iter().take(4).map(|(a, c)| if *c > 1 { format!("{a} {c}") } else { a.clone() }).collect::<Vec<_>>().join(" · ");
        // each of the held ones as a drop of its own, in the order they came (the history is newest first)
        let mut held: Vec<effects::Held> = self.history.iter().filter(|(n, at)| *at >= since && !is_own(n)).take(effects::BURST_MAX).map(|(n, _)| effects::Held { app: n.app.clone(), icon: n.app_icon.clone().or_else(|| n.image.clone()) }).collect();
        held.reverse();
        self.next_own_id += 1;
        self.recap_id = self.next_own_id;
        let n = Incoming { id: self.recap_id, app: "open-island".into(), summary: format!("{total} {what}"), body, expire_ms: Some(8000), ..Default::default() };
        self.push(self.now, Input::Notification(n));
        // back from do not disturb they come out one by one (coming back to the machine is told by the card alone)
        if what == HELD_BACK {
            self.burst(self.recap_id, held);
        }
    }

    /// Every reason there is right now to hold notifications without being asked, the most telling first.
    fn quiet_reasons(&self, now: i64) -> Vec<&'static str> {
        let p = self.data.privacy;
        let in_meeting = self.cfg.quiet_in_meetings
            && self.data.events.iter().any(|e| !e.all_day && e.start_ms <= now && now < e.end_ms && e.end_ms - e.start_ms <= 3 * 3_600_000);
        [
            ("meeting", in_meeting),
            ("screen share", self.cfg.quiet_on_screen_share && p.screen),
            ("microphone", self.cfg.quiet_on_mic && p.mic),
            ("focus", self.data.focus.as_ref().is_some_and(|f| f.active)),
            ("deep work", self.deep_work(now)),
            // the focus of a study session; last, so that any other reason says what it holds back
            ("study", self.session().is_some_and(|t| t.phase == Phase::Focus)),
        ]
        .into_iter()
        .filter_map(|(reason, holds)| holds.then_some(reason))
        .collect()
    }

    /// Do not disturb switched by the person (the button, the command bar, the control socket). Off means off: the reasons
    /// the island has of its own right now are set aside too, for as long as each lasts. Returns what to tell the daemon.
    pub(super) fn set_dnd(&mut self, on: bool) -> Command {
        self.data.dnd = on;
        // switched by hand it lasts until it is switched again
        self.dnd_until = None;
        let holding = self.quiet_reasons(self.now);
        self.quiet_set_aside = if on { Vec::new() } else { holding.clone() };
        self.auto_quiet = holding.into_iter().find(|r| !self.quiet_set_aside.contains(r));
        self.dnd_sent = self.quiet();
        Command::SetDnd(self.dnd_sent)
    }

    /// Do not disturb for `secs` seconds, or (None) until what is on in the calendar is over. Returns what to tell the
    /// daemon, or None when there is nothing on to wait for.
    pub(super) fn set_dnd_for(&mut self, secs: Option<i64>) {
        let now = self.now;
        let until = match secs {
            Some(secs) => now + secs * 1000,
            // the last to end of what is on now (not what lasts half a day: that is no lecture)
            None => match self.data.events.iter().filter(|e| !e.all_day && e.start_ms <= now && now < e.end_ms && e.end_ms - e.start_ms <= 12 * 3_600_000).map(|e| e.end_ms).max() {
                Some(end) => end,
                None => {
                    self.say("Nothing is on now", "Say for how long: dnd 1h");
                    return;
                }
            },
        };
        let tell = self.set_dnd(true);
        self.dnd_until = Some(until);
        self.pending.push(tell);
        let at = crate::clock::civil(until, self.data.utc_offset_secs).hhmm();
        self.say("Do not disturb", &format!("until {at}"));
    }

    /// What a restart brings back: a do not disturb that has an end.
    pub fn restore_dnd_until(&mut self, until: i64) {
        self.data.dnd = true;
        self.dnd_until = Some(until);
        self.dnd_seen = (true, Some(until));
    }

    /// Mutes or unmutes the microphone; the island shows it at once and the platform confirms.
    pub(super) fn set_mic(&mut self, muted: Option<bool>) -> Command {
        let muted = muted.or(self.data.mic_muted.map(|m| !m));
        if muted.is_some() {
            self.data.mic_muted = muted;
        }
        Command::SetMic(muted)
    }

    /// A word when the microphone is muted or heard again, however that happened.
    fn mic_card(&mut self) {
        let now = self.data.mic_muted;
        if now == self.mic_seen {
            return;
        }
        let first = self.mic_seen.replace(false).is_none();
        self.mic_seen = now;
        match now {
            Some(muted) if !first => self.say(if muted { "Microphone muted" } else { "Microphone on" }, ""),
            _ => {}
        }
    }

    pub(super) fn update_quiet(&mut self, now: i64) {
        // its time is up: the manual choice goes, what the island holds for reasons of its own stays held
        if self.dnd_until.is_some_and(|until| until <= now) {
            self.dnd_until = None;
            self.data.dnd = false;
        }
        let holding = self.quiet_reasons(now);
        // one that has ended counts again the next time it comes
        self.quiet_set_aside.retain(|r| holding.contains(r));
        self.auto_quiet = holding.into_iter().find(|r| !self.quiet_set_aside.contains(r));
        match (self.quiet(), self.quiet_since) {
            (true, None) => self.quiet_since = Some(now),
            (false, Some(since)) => {
                self.quiet_since = None;
                self.recap(since, HELD_BACK);
            }
            _ => {}
        }
        // the daemon holds the sounds back too: tell it whenever the state it should keep changes
        if self.quiet() != self.dnd_sent {
            self.dnd_sent = self.quiet();
            self.pending.push(Command::SetDnd(self.dnd_sent));
        }
    }

    pub(super) fn day_start(&self, now: i64) -> i64 {
        let off = self.data.utc_offset_secs;
        let t = crate::clock::civil(now, off);
        crate::clock::midnight_ms(t.year, t.month, t.day, off)
    }

    pub(super) fn log_event(&mut self, kind: LogKind, text: String) {
        self.log.push_front(LogEntry { at_ms: self.now, kind, text });
        self.log.truncate(120);
    }

    /// Today's attention per app, to be kept between runs.
    pub fn attention_export(&self) -> (i64, Vec<(String, i64)>) {
        let ds = self.day_start(self.now);
        self.ledger.export(self.now, ds)
    }

    pub fn restore_attention(&mut self, day_start: i64, totals: Vec<(String, i64)>) {
        self.ledger.restore(day_start, totals);
    }

    /// The totals of the days before today, to be kept between runs.
    pub fn attention_history(&self) -> &[island_core::activity::DayTotals] {
        self.ledger.history()
    }

    pub fn restore_attention_history(&mut self, history: Vec<island_core::activity::DayTotals>) {
        self.ledger.restore_history(history);
    }

    pub(crate) fn is_distraction(&self, app: &str) -> bool {
        self.cfg.distraction_apps.iter().any(|a| a.eq_ignore_ascii_case(app))
    }

    /// During the study hours, a card after too long on the apps marked as distracting (going from one to another, or
    /// away for less than a minute, goes on counting); again after another such stretch. Held, like the other
    /// heads-ups, during a call or a shared screen.
    pub(super) fn distraction_card(&mut self, now: i64) {
        let limit = i64::from(self.cfg.distraction_minutes) * 60_000;
        let Some(app) = self.ledger.current().map(|(a, _, _)| a.to_string()) else { return };
        if limit <= 0 || !self.is_distraction(&app) || self.quiet_blocks_headsup() {
            return;
        }
        let today = self.today_day();
        let Some((began, ends)) = self.study_windows(today).into_iter().find(|(from, to)| *from <= now && now < *to) else { return };
        // counted from the start of this stretch of study, and from the last time it was said
        let on_them = self.ledger.streak(now, 60_000, |a| self.is_distraction(a)).min(now - began).min(self.distraction_told.map_or(i64::MAX, |told| now - told));
        if on_them < limit {
            return;
        }
        self.distraction_told = Some(now);
        let until = crate::clock::civil(ends, self.data.utc_offset_secs).hhmm();
        self.say(&format!("{} min on {app}", on_them / 60_000), &format!("These are study hours, until {until}"));
    }

    /// The last half hour in one list, newest first: where the attention was, what came in, what played, what was copied.
    pub(crate) fn rewind_rows(&self) -> Vec<RewindRow> {
        const WINDOW: i64 = 30 * 60_000;
        let now = self.now;
        let mut rows: Vec<RewindRow> = vec![];
        // a window that had the focus for less than ten seconds is a pass-through, not a place
        for s in self.ledger.recent(now, WINDOW).into_iter().filter(|s| s.end_ms - s.start_ms >= 10_000) {
            let text = if s.title.is_empty() { s.app.clone() } else { format!("{}: {}", s.app, s.title) };
            rows.push(RewindRow { at_ms: s.start_ms, icon: "device-desktop", text, copy: None });
        }
        for e in self.log.iter().filter(|e| now - e.at_ms <= WINDOW) {
            let (icon, copy) = match e.kind {
                LogKind::Notification => ("bell", None),
                LogKind::Track => ("music", None),
                LogKind::Copy => ("clipboard-text", Some(e.text.clone())),
            };
            rows.push(RewindRow { at_ms: e.at_ms, icon, text: e.text.clone(), copy });
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.at_ms));
        rows.truncate(6);
        rows
    }

    /// Whether the person has been on work apps long enough for the notifications to wait.
    pub(super) fn deep_work(&self, now: i64) -> bool {
        let mins = self.cfg.deep_work_minutes;
        if mins <= 0 || self.cfg.deep_work_apps.is_empty() {
            return false;
        }
        let apps: Vec<String> = self.cfg.deep_work_apps.iter().map(|a| a.to_lowercase()).collect();
        self.ledger.streak(now, 60_000, |app| {
            let a = app.to_lowercase();
            apps.iter().any(|w| a.contains(w.as_str()))
        }) >= mins * 60_000
    }

    /// A card when the person has been on one app for a long stretch: a break may be due.
    pub(super) fn break_card(&mut self, now: i64) {
        let mins = self.cfg.break_minutes;
        let Some((app, _, _)) = self.ledger.current().map(|(a, t, s)| (a.to_string(), t.to_string(), s)) else { return };
        if mins <= 0 {
            return;
        }
        let streak = self.ledger.streak(now, 60_000, |a| a == app);
        if streak < mins * 60_000 {
            return;
        }
        // told again only after another full period
        if self.break_told.is_some_and(|t| now - t < mins * 60_000) {
            return;
        }
        self.break_told = Some(now);
        self.next_own_id += 1;
        let n = Incoming { id: self.next_own_id, app: "open-island".into(), summary: format!("{} minutes on {app}", streak / 60_000), body: "A short break?".into(), expire_ms: Some(8000), ..Default::default() };
        self.push(now, Input::Notification(n));
    }

    pub(crate) fn map_is_panned(&self) -> bool {
        self.map_center.is_some()
    }

    /// Back to following the position, at the zoom that fits.
    pub(super) fn map_recenter(&mut self) {
        self.map_center = None;
        self.map_fixed_zoom = None;
        self.map_zoom_delta = 0;
    }

    pub(super) fn map_zoom(&mut self, d: i8) {
        match self.map_fixed_zoom {
            Some(z) => self.map_fixed_zoom = Some((z as i16 + d as i16).clamp(island_core::map::MIN_ZOOM as i16, island_core::map::MAX_ZOOM as i16) as u8),
            None => self.map_zoom_delta = (self.map_zoom_delta + d).clamp(-10, 10),
        }
    }

    /// What the map page shows: the centre and zoom, where the machine is, and the next event's place when it is known.
    pub(crate) fn map_view(&self) -> Option<MapView> {
        use island_core::map;
        let pos = self.data.position.as_ref()?;
        let me = (pos.lat, pos.lon);
        let dest = match &self.map_dest {
            Some(p) => Some(((p.lat, p.lon), p.name.clone(), String::new())),
            None => self.trip_event().and_then(|e| {
                let at = self.data.trips.iter().filter(|t| t.event_id == e.id).find_map(|t| t.dest)?;
                Some((at, e.title.clone(), e.id.clone()))
            }),
        };
        let (center, base) = match (&dest, self.map_both) {
            (Some((d, _, _)), true) => (map::midpoint(me, *d), map::zoom_to_fit(me, *d, MAP_W, MAP_H, 40.0)),
            _ => (me, map::zoom_for_accuracy(pos.accuracy_m)),
        };
        let zoom = self.map_fixed_zoom.unwrap_or((base as i16 + self.map_zoom_delta as i16).clamp(map::MIN_ZOOM as i16, map::MAX_ZOOM as i16) as u8);
        let center = self.map_center.unwrap_or(center);
        Some(MapView { center, zoom, me, accuracy_m: pos.accuracy_m, from_settings: pos.source == PosSource::Settings, dest: dest.map(|(d, t, _)| (d, t)) })
    }

    /// Asks for the map tiles in view, and for a position when there is none or it is old.
    pub(super) fn map_needs(&mut self, now: i64) {
        if self.page != Some(Page::Map) {
            return;
        }
        if self.cfg.follow_position && now - self.map_asked_at > 120_000 && self.data.position.as_ref().is_none_or(|p| p.source == PosSource::Settings || now - p.at_ms > 120_000) {
            self.map_asked_at = now;
            self.pending.push(Command::RefreshPosition);
        }
        if island_core::map::needs_key(&self.cfg.map_tile_url) && !self.data.config.map_key_set {
            return;
        }
        let Some(v) = self.map_view() else { return };
        // a ring of tiles beyond the view, so a drag finds them there
        for t in island_core::map::tiles(v.center.0, v.center.1, v.zoom, MAP_W + 256.0, MAP_H + 256.0) {
            let key = (t.z, t.x, t.y);
            if !self.tiles.contains_key(&key) && self.tiles_asked.insert(key) {
                self.pending.push(Command::LoadTile { z: t.z, x: t.x, y: t.y, template: self.cfg.map_tile_url.clone(), dark: self.cfg.map_dark && !island_core::map::is_dark_style(&self.cfg.map_tile_url) });
            }
        }
    }

    /// One card when a program starts to hog the machine (and again only after ten minutes, or for another program).
    pub(super) fn culprit_card(&mut self, now: i64) {
        let Some(c) = self.data.culprit.clone() else { return };
        if !self.cfg.culprit_alerts || self.quiet() || self.culprit_told.as_ref().is_some_and(|(n, at)| *n == c.name && now - at < 600_000) {
            return;
        }
        self.culprit_told = Some((c.name.clone(), now));
        let summary = if c.memory { format!("{} holds {} of memory", c.name, island_core::system::fmt_mb(c.mem_mb as f64)) } else { format!("{} is using {:.1} cores", c.name, c.cores) };
        self.next_own_id += 1;
        self.culprit_id = self.next_own_id;
        let n = Incoming { id: self.culprit_id, app: "open-island".into(), summary, body: if c.protected { "Tap for details".into() } else { "Tap to pause or quit it".into() }, expire_ms: Some(9000), ..Default::default() };
        self.push(now, Input::Notification(n));
    }

    /// A card when the network breaks (with where) and when it is back.
    pub(super) fn network_card(&mut self, now: i64) {
        let h = self.data.net;
        if h == self.net_seen {
            return;
        }
        let was_ok = self.net_seen.is_ok();
        self.net_seen = h;
        let (summary, body) = h.message();
        self.next_own_id += 1;
        let id = self.next_own_id;
        if h.is_ok() {
            self.net_id = 0;
            self.push(now, Input::Notification(Incoming { id, app: "open-island".into(), summary: summary.into(), expire_ms: Some(2500), ..Default::default() }));
        } else if h == island_core::netstate::Health::Portal {
            self.net_id = 0;
            self.push(now, Input::Notification(Incoming { id, app: "open-island".into(), summary: summary.into(), body: body.into(), expire_ms: Some(6000), ..Default::default() }));
        } else {
            self.net_id = id;
            let tail = if was_ok { "· tap to reconnect" } else { "" };
            self.push(now, Input::Notification(Incoming { id, app: "open-island".into(), summary: summary.into(), body: format!("{body} {tail}").trim().into(), expire_ms: Some(8000), ..Default::default() }));
        }
    }

    /// The next event that has a place to go to, today.
    pub(crate) fn trip_event(&self) -> Option<&calendar::Event> {
        if !self.cfg.travel_times {
            return None;
        }
        self.data
            .events
            .iter()
            .filter(|e| !e.all_day && e.start_ms > self.now && e.start_ms < self.day_start(self.now) + 24 * 3_600_000 && island_core::travel::is_physical(&e.location))
            .min_by_key(|e| e.start_ms)
    }

    pub(crate) fn trip_info(&self, event_id: &str, mode: island_core::travel::Mode) -> Option<&TripInfo> {
        self.data.trips.iter().find(|t| t.event_id == event_id && t.mode == mode)
    }

    /// What is worth knowing before leaving for an event: rain, a low battery.
    pub(super) fn before_you_go(&self, leave_in_min: i32, trip_min: i32) -> String {
        let rain = self.rain_outlook().and_then(|o| island_core::headsup::rain_note(o, leave_in_min, trip_min));
        let battery = self.data.battery.and_then(|b| island_core::headsup::battery_note(b.percent, b.on_ac));
        island_core::headsup::line([rain, battery])
    }

    /// Asks for the trip to the next event with a place when it matters (the page is open, or it is a few hours away),
    /// and again every ten minutes (public transport changes with the hour); tells when it is time to leave.
    pub(super) fn trips(&mut self, now: i64) {
        let Some(e) = self.trip_event().cloned() else { return };
        let mode = self.cfg.travel_mode;
        let wanted = self.page == Some(Page::Today) || e.start_ms - now <= 3 * 3_600_000;
        let key = (e.id.clone(), mode);
        if wanted && self.trip_asked.get(&key).is_none_or(|at| now - at > 600_000) {
            self.trip_asked.insert(key, now);
            self.pending.push(Command::PlanTrip { event_id: e.id.clone(), location: e.location.clone(), start_ms: e.start_ms, mode, margin_secs: self.cfg.travel_margin_ms / 1000 });
        }
        let leave = self.trip_info(&e.id, mode).and_then(|t| t.trip.as_ref()).and_then(|t| t.leave_ms.map(|l| (l, t.secs)));
        if let Some((leave_ms, secs)) = leave {
            if now >= leave_ms - 5 * 60_000 && !self.leave_told.contains(&e.id) {
                self.leave_told.insert(e.id.clone());
                let off = self.data.utc_offset_secs;
                let late = now >= leave_ms;
                let summary = if late { format!("Time to leave for {}", e.title) } else { format!("Leave in {} min for {}", ((leave_ms - now) as f64 / 60_000.0).ceil() as i64, e.title) };
                let mut body = format!("{} · {} · starts {}", mode.name(), island_core::travel::fmt_secs(secs), crate::clock::civil(e.start_ms, off).hhmm());
                let notes = self.before_you_go(((leave_ms - now) as f64 / 60_000.0).ceil() as i32, (secs / 60) as i32);
                if !notes.is_empty() {
                    body = format!("{body} · {notes}");
                }
                self.next_own_id += 1;
                let n = Incoming { id: self.next_own_id, app: "Reminder".into(), summary, body, expire_ms: Some(12_000), ..Default::default() };
                self.push(now, Input::Notification(n));
            }
        }
    }

    /// An event within the hour with something to know about it (rain, a low battery) and no way to tell it with the
    /// time to leave: one card.
    pub(super) fn heads_up(&mut self, now: i64) {
        let next = self.data.events.iter().filter(|e| !e.all_day && e.start_ms > now && e.start_ms - now <= 40 * 60_000).min_by_key(|e| e.start_ms).cloned();
        let Some(e) = next else { return };
        if self.headsup_told.contains(&e.id) || self.leave_told.contains(&e.id) || self.quiet_blocks_headsup() {
            return;
        }
        let min = ((e.start_ms - now) as f64 / 60_000.0).ceil() as i32;
        let notes = self.before_you_go(min, 15);
        if notes.is_empty() {
            return;
        }
        self.headsup_told.insert(e.id.clone());
        self.next_own_id += 1;
        let n = Incoming { id: self.next_own_id, app: "Reminder".into(), summary: format!("{} in {min} min", e.title), body: notes, expire_ms: Some(10_000), ..Default::default() };
        self.push(now, Input::Notification(n));
    }

    /// A call or a recording is not interrupted by a heads-up. A meeting and a study session are: the next lecture and the
    /// nudge away from a distracting app are what one wants to hear then.
    pub(super) fn quiet_blocks_headsup(&self) -> bool {
        self.auto_quiet.is_some_and(|r| r != "meeting" && r != "study")
    }

    /// What played and what was copied, for the rewind page.
    pub(super) fn note_changes(&mut self) {
        let track = self.data.media.as_ref().map(|m| if m.artist.is_empty() { m.title.clone() } else { format!("{} · {}", m.title, m.artist) }).unwrap_or_default();
        if track != self.log_track {
            if !track.is_empty() && !self.log_track.is_empty() {
                self.log_event(LogKind::Track, track.clone());
            }
            self.log_track = track;
        }
        let copy = self.data.clipboard.first().cloned().unwrap_or_default();
        if copy != self.last_copy {
            if !copy.is_empty() && !self.last_copy.is_empty() {
                self.log_event(LogKind::Copy, copy.clone());
                self.link_card(&copy);
            }
            self.last_copy = copy;
        }
    }

    pub(super) fn slow_checks(&mut self) {
        let now = self.now;
        self.note_changes();
        self.break_card(now);
        self.distraction_card(now);
        self.check_places(now);
        self.update_quiet(now);
        self.trips(now);
        self.map_needs(now);
        self.heads_up(now);
        self.late_battery();
        self.late_eta();
        self.culprit_card(now);
        self.sample_battery(now);
        self.power_card(now);
        self.ask_materials(now);
        self.network_card(now);
        // a device whose battery went back up may warn again next time
        let devs = &self.data.devices;
        self.device_low_told.retain(|id| devs.iter().any(|d| d.id == *id && d.battery.is_some_and(|b| b <= 20)));
        // meeting heads-up: once per event
        if self.cfg.meeting_alerts {
            if let Some(e) = self.announcer.new_heads_up(&self.data.events, now, self.cfg.meeting_lead_ms).cloned() {
                self.meeting_card(&e);
            }
        }
        // rain
        if self.cfg.rain_alerts {
            let soon = self.data.weather.as_ref().and_then(|w| weather::rain_soon(&w.rain_slots, 60));
            match soon {
                Some(m) if !self.rain_announced => {
                    self.rain_announced = true;
                    self.rain_card(m);
                }
                None => self.rain_announced = false,
                _ => {}
            }
        }
        // power
        if let Some(b) = self.data.battery {
            if let Some(prev) = self.last_plugged {
                if prev != b.on_ac && self.cfg.battery_alerts {
                    self.push(now, Input::Power(if b.on_ac { PowerEvent::Plugged } else { PowerEvent::Unplugged }));
                }
            }
            self.last_plugged = Some(b.on_ac);
            if !b.on_ac && b.percent <= self.cfg.low_battery {
                if !self.low_battery_sent && self.cfg.battery_alerts {
                    self.low_battery_sent = true;
                    self.push(now, Input::Power(PowerEvent::Low));
                }
            } else if b.percent > self.cfg.low_battery + 3.0 || b.on_ac {
                self.low_battery_sent = false;
            }
        }
    }

    /// A command that outlasts its threshold becomes the operation on show (when none other is); the one on show
    /// is refreshed so it is not dropped as stale.
    pub(super) fn shell_commands(&mut self, now: i64) {
        if self.cmds.is_empty() {
            return;
        }
        let running = matches!(&self.ops.op, Some(o) if o.state == progress::State::Run);
        let on_show = self.cmds.iter().find(|c| c.shown).map(|c| c.label.clone());
        match (running, on_show) {
            (true, Some(label)) => {
                if matches!(&self.ops.op, Some(o) if o.label == label && now - o.updated_ms >= 30_000) {
                    self.ops.report(now, &format!("? {label}"));
                }
            }
            (false, _) => {
                if let Some(c) = self.cmds.iter_mut().find(|c| !c.shown && now - c.start_ms >= c.after_ms) {
                    c.shown = true;
                    let label = c.label.clone();
                    self.ops.report(now, &format!("? {label}"));
                }
            }
            _ => {}
        }
    }

    /// Whether the music is shown as playing: what the player says, except just after a press of the button.
    pub(crate) fn playing_shown(&self) -> bool {
        let base = self.data.media.as_ref().is_some_and(|m| m.playing);
        match self.play_override {
            Some((v, until)) if self.now < until => v,
            _ => base,
        }
    }

    /// Moves the play/pause button: the icons cross-fade, the disc grows under the pointer and gives under a press.
    /// Returns true while something is still on its way.
    pub(super) fn ease_media_button(&mut self, dt_ms: f64) -> bool {
        let dt = if dt_ms.is_finite() { dt_ms.clamp(0.0, 100.0) } else { 0.0 };
        let base = self.data.media.as_ref().is_some_and(|m| m.playing);
        if matches!(self.play_override, Some((v, until)) if v == base || self.now >= until) {
            self.play_override = None; // the player agrees (or never did): back to what it says
        }
        let on_screen = matches!(self.shown, Face::MediaFull | Face::MediaLyric);
        let over = self.pointer.is_some_and(|(x, y)| {
            let (lx, ly) = self.local(x, y);
            self.hits.iter().any(|h| h.action == Action::Cmd(Command::MediaPlayPause) && h.rect.contains(vello::kurbo::Point::new(lx, ly)))
        });
        let mut moving = false;
        let mut ease = |v: &mut f64, target: f64, tau: f64| {
            if !on_screen || (target - *v).abs() < 0.004 {
                *v = target;
            } else {
                *v += (target - *v) * (1.0 - (-dt / tau).exp());
                moving = true;
            }
        };
        let target = if self.playing_shown() { 1.0 } else { 0.0 };
        ease(&mut self.play_anim, target, 70.0);
        ease(&mut self.disc_hover, if over { 1.0 } else { 0.0 }, 60.0);
        ease(&mut self.disc_press, 0.0, 90.0);
        ease(&mut self.skip_press, 0.0, 110.0);
        moving
    }

    /// Moves the levels the bars show towards the real ones: about a twentieth of a second to cover most of the
    /// distance, so the 10 or 30 changes a second that arrive look like a continuous slide at any refresh rate.
    /// A slider being dragged follows the pointer exactly. Returns true while something is still on its way.
    pub(super) fn ease_levels(&mut self, dt_ms: f64) -> bool {
        let k = 1.0 - (-(if dt_ms.is_finite() { dt_ms.clamp(0.0, 100.0) } else { 0.0 }) / 55.0).exp();
        let dragging = matches!(self.dragging, Some(Action::VolumeBar | Action::BrightnessBar));
        let mut moving = false;
        let mut ease = |shown: &mut f64, target: f64, snap: bool| {
            if snap || (target - *shown).abs() < 0.002 {
                *shown = target;
            } else {
                *shown += (target - *shown) * k;
                moving = true;
            }
        };
        let vol = self.data.volume.unwrap_or(0.0) as f64;
        let bri = self.data.brightness.unwrap_or(0.0) as f64;
        if !self.levels_init && (self.data.volume.is_some() || self.data.brightness.is_some()) {
            // the first reading is where the bars are, not something to slide to
            self.levels_init = true;
            (self.volume_shown, self.brightness_shown) = (vol, bri);
        }
        ease(&mut self.volume_shown, vol, dragging && self.dragging == Some(Action::VolumeBar));
        ease(&mut self.brightness_shown, bri, dragging && self.dragging == Some(Action::BrightnessBar));
        if let Some(o) = self.osd {
            ease(&mut self.osd_shown, if o.muted { 0.0 } else { o.level as f64 }, false);
        }
        moving
    }

    /// Advances the island to `now` (epoch ms) and says how big the surface has to be.
    pub fn tick(&mut self, now: i64, dt_ms: f64) -> Frame {
        self.now = now;
        self.ops.expire(now);
        self.shell_commands(now);
        self.fire_reminders(now);
        self.expire_deadlines(now);
        self.check_evening(now);
        self.check_morning(now);
        self.check_departure(now);
        self.check_event_end(now);
        self.check_train(now);
        self.auth_tick(now);
        if self.bar.as_ref().is_some_and(|b| now > b.until) {
            self.bar = None;
        }
        self.mic_card();
        self.slow_checks();
        self.sync_lyrics();
        // notifications: expiry and the next waiting one
        if self.edit.as_ref().is_some_and(|e| now > e.until) || (self.edit.is_some() && !matches!(self.page, Some(Page::Settings | Page::Options | Page::Map | Page::Phone))) {
            self.edit = None;
            self.icloud_user = None;
        }
        if self.page != Some(Page::Settings) {
            self.picking = None;
        }
        if self.note.as_ref().is_some_and(|(_, until)| now > *until) {
            self.note = None;
        }
        if self.kb.is_some_and(|k| now - k.last_key > KB_IDLE_MS) {
            self.kb = None;
        }
        if self.reply.as_ref().is_some_and(|r| now > r.until) {
            self.end_reply(false);
        }
        self.hold_cards();
        // expiry, the places in the stack, and the ones waiting for a place
        self.turn_over_cards();
        self.sync_cards();
        if self.timer.as_ref().and_then(|t| t.end_ms).is_some_and(|e| now >= e) {
            self.on_timer_end();
        }
        if self.extra_timers.iter().any(|t| t.end_ms.is_some_and(|e| now >= e)) {
            self.ring_extra_timers();
        }
        self.update_effects(dt_ms);
        // track change: peek the player
        let track = self.data.media.as_ref().map(|m| m.track_id.clone());
        if track != self.last_track {
            if self.last_track.is_some() && track.is_some() && self.cfg.media_auto_expand {
                self.expand_until = now + 3500;
            }
            if self.last_track.is_some() {
                self.track_changed_at = now;
            }
            self.last_track = track;
        }

        let (snap, _big) = self.snapshot();
        let (mut want, mut side) = face::choose(&snap);
        let mut more = face::more_sides(want, side, &snap, self.ops.also.len(), (self.cfg.mini_pills as usize).saturating_sub(1));
        if side.is_none() {
            self.swapped = false;
        } else if self.swapped {
            more.clear();
            let kind = match want {
                Face::MediaMini => Some(Side::Media),
                Face::TimerMini => Some(Side::Timer),
                _ => None,
            };
            match side {
                Some(Side::Timer) => want = Face::TimerMini,
                Some(Side::Meeting) => want = Face::MeetMini,
                _ => {}
            }
            side = kind;
        }
        // the calendar opens on today however it was reached (a swipe, the back button, a shortcut)
        if want == Face::Cal && (want != self.want || self.cal.year == 1970) {
            let c = crate::clock::civil(now, self.data.utc_offset_secs);
            self.cal = Cal { year: c.year, month: c.month, day: c.day, first: 0 };
        }
        // opening the calendar or the home page with stale events asks for a fresh read
        if want != self.want && matches!(want, Face::Cal | Face::IdleHover) && !self.data.config.calendars.is_empty() && now - self.data.events_synced_at > STALE_EVENTS_MS && now - self.last_refresh_req > 30_000 {
            self.last_refresh_req = now;
            self.pending.push(Command::RefreshCalendars);
        }
        if want == Face::Cal {
            self.ask_month(now);
        }
        if want != self.want {
            // between two pages the content slides the way the page order goes
            let pages = self.pages();
            let idx = |f: Face| page_of(f).and_then(|p| pages.iter().position(|x| *x == p));
            self.slide = match (idx(self.shown), idx(want)) {
                (Some(a), Some(b)) if a != b => if b > a { 1.0 } else { -1.0 },
                _ => 0.0,
            };
        }
        if want != Face::Plan {
            // the editor is left with the page
            self.plan_hours = false;
        }
        if want != self.want {
            if let Some(p) = page_of(want).filter(|p| !matches!(p, Page::Home | Page::Grid)) {
                *self.page_uses.entry(p).or_insert(0) += 1;
            }
        }
        if want != self.want {
            log::debug!("face {:?} -> {:?} (shown {:?}, notif {})", self.want, want, self.shown, self.notif.is_some());
        }
        self.want = want;
        self.want_side = side;
        self.want_more = more;

        // content crossfade: out fast, in once the shape is about halfway to its target
        let dt = if dt_ms.is_finite() { dt_ms.clamp(0.0, 1000.0) } else { 0.0 };
        let (mut tw, mut th) = self.face_size(self.want);
        if self.want != self.shown {
            self.alpha = (self.alpha - dt / FADE_OUT_MS).max(0.0);
            if self.alpha <= 0.0 {
                self.shown = self.want;
                self.shown_side = self.want_side;
                self.shown_more = self.want_more.clone();
                self.from_size = (self.spring.w.pos, self.spring.h.pos);
            }
        } else {
            if let Some(s) = self.want_side {
                self.shown_side = Some(s);
                self.shown_more = self.want_more.clone();
            }
            let span = (tw - self.from_size.0).abs().max((th - self.from_size.1).abs());
            let done = if span < 1.0 {
                1.0
            } else {
                1.0 - (tw - self.spring.w.pos).abs().max((th - self.spring.h.pos).abs()) / span
            };
            if done >= FADE_IN_AT {
                if self.alpha <= 0.0 && dt > 0.0 {
                    self.enter_at = now;
                }
                self.alpha = (self.alpha + dt / FADE_IN_MS).min(1.0);
            }
        }
        // while the old content fades out the shape holds still
        if self.want != self.shown && self.alpha > 0.3 {
            (tw, th) = self.face_size(self.shown);
        }
        let at_rest = self.spring.step(tw, th, dt);
        self.tuck_tick(now, dt);
        self.sky_tick(now);
        self.surface_tick(now);
        if self.shown_side.is_some() && self.want == self.shown && self.want_side.is_none() {
            self.shown_side = None;
            self.shown_more.clear();
        }
        // the small pills leave the main one, or go back into it; the drop hangs when there is room for it
        self.sync_bubbles();
        self.sync_fill();
        self.settled = at_rest && self.alpha >= 1.0 && self.want == self.shown && self.want_side == self.shown_side && self.want_more == self.shown_more;
        // what needs frames: motion at every vsync, an equalizer at 30 fps, things that expire a few times a second,
        // a timer twice a second, otherwise the clock once a second
        let levels_moving = self.ease_levels(dt_ms) | self.ease_media_button(dt_ms);
        let waiting_for_hover = (self.hovering && now - self.hover_since < HOVER_INTENT_MS + 60) || now < self.hover_left + HOVER_GRACE_MS + 60;
        // milliseconds until a sliding line moves (0: it is sliding)
        let marquee = self.marquee_wake.take().map(|at| (at - now).clamp(0, u32::MAX as i64) as u32);
        let moving = self.tuck_sliding() || self.auth_moving() || !self.settled || self.lyrics_scrolling() || waiting_for_hover || levels_moving || self.card_moving() || self.bubbles_moving() || self.fill_moving() || self.fx.moving();
        let equalizer = self.data.media.as_ref().is_some_and(|m| m.playing) && matches!(self.shown, Face::MediaMini | Face::MediaFull | Face::MediaLyric);
        let expiring = snap.osd || self.auth.is_some() || self.notif.is_some() || self.edit.is_some() || self.reply.is_some() || self.kb.is_some() || self.bar.is_some();
        let next_ms: u32 = if moving {
            0
        } else if equalizer {
            // live bars: every new spectrum (it comes 30 times a second) wakes a frame by itself, so no timer is needed
            // while they arrive (a timer at 16 ms drew each of them twice); the made-up animation needs one, at 30 fps
            if !self.spectrum.is_empty() && now - self.spectrum_at < 200 { 120 } else { 33 }
        } else if self.cards.any_ticking(now) {
            33 // the rings drain
        } else if expiring {
            120
        } else if self.timer.is_some() || self.ops.op.is_some_and_running() {
            500
        } else {
            1000
        };
        let next_ms = if moving { 0 } else { marquee.map_or(next_ms, |wait| next_ms.min(wait.max(MARQUEE_STEP_MS))) };
        let next_ms = if self.fill_sloshing() { next_ms.min(liquid::BUSY_STEP_MS) } else { next_ms };
        let next_ms = self.sky_step().map_or(next_ms, |step| next_ms.min(step));
        // the edges come to rest also when no more of the music is heard
        let next_ms = if self.rippling().is_some() { next_ms.min(RIPPLE_STEP_MS) } else { next_ms };
        // out of sight there is nothing that has to move; about to leave, the slide starts on time
        let next_ms = if self.out_of_sight() && !moving { next_ms.max(if self.timer.is_some() { 500 } else { 1000 }) } else { self.tuck_due_ms().map_or(next_ms, |due| next_ms.min(due.max(1))) };
        let next_ms = if moving { 0 } else { next_ms };
        let animating = next_ms == 0;
        // the surface: holds both shapes plus room for the overshoot while animating
        // (a small pill on its way back still needs its room)
        let beside = self.side_slot(self.shown_side.is_some(), self.shown_more.len()).max(self.bubbles.len() as f64 * (BUBBLE_W + BUBBLE_GAP));
        let group_w = self.spring.w.pos + beside;
        let (sw, sh) = if self.settled {
            self.anim_box = None;
            (group_w.ceil(), self.spring.h.pos.ceil())
        } else {
            let target_w = tw + self.side_slot(self.want_side.is_some(), self.want_more.len());
            let bx = match self.anim_box {
                Some((aw, ah, bw, bh)) if aw == target_w && ah == th => (bw, bh),
                _ => (group_w.max(target_w).ceil() + 24.0, self.spring.h.pos.max(th).ceil() + 16.0),
            };
            let bx = (bx.0.max(group_w.ceil() + 2.0), bx.1.max(self.spring.h.pos.ceil() + 2.0));
            self.anim_box = Some((target_w, th, bx.0, bx.1));
            bx
        };
        let (mut sw, mut sh) = (sw.max(40.0), sh.max(20.0));
        if self.has_card() {
            // room for the card under the pill, centred on the pill however many small pills sit beside it
            let slot = self.side_slot(self.shown_side.is_some() || self.want_side.is_some(), self.shown_more.len().max(self.want_more.len())).max(beside);
            sw = sw.max(card::CARD_MAX_W + slot);
            sh += card::zone_height(self.cards_shown().max(self.cards.stack.len()), self.cards.taller());
        }
        if self.fx.shot.is_some() {
            sw = sw.max(group_w + 2.0 * effects::SHOT_ROOM);
        }
        if self.fx.catch.is_some() {
            // the drop that reaches for the file, and the area that takes it
            sw = sw.max(group_w + 2.0 * (effects::REACH + 24.0));
            sh += effects::REACH + 30.0;
        }
        if self.may_ripple() && (self.data.media.as_ref().is_some_and(|m| m.playing) || self.rippling().is_some()) {
            // room below for the edge, kept for as long as it may move: the surface must not change at every beat
            sh = sh.max(self.spring.h.pos.ceil() + island_core::surface::SWELL);
        }
        if self.fill.is_some() {
            // the drop hangs at the left: as much room on the right keeps the pill where it is
            sw = sw.max(group_w + 2.0 * liquid::DROP_ROOM);
        }
        // the drop of a password request, down to the middle of the screen and back
        if let Some((wide, bottom)) = self.auth_reach() {
            sw = sw.max(wide);
            sh = sh.max(self.spring.h.pos.ceil() + bottom);
        }
        Frame { surface: (sw + 8.0, sh + TOP_MARGIN + 4.0), animating, next_ms }
    }

    /// The edges of the pill may move: in one of the small faces of the player, on screen.
    pub(crate) fn may_ripple(&self) -> bool {
        self.cfg.pill_pulse && matches!(self.shown, Face::MediaMini | Face::MediaLyric) && !self.out_of_sight()
    }

    /// The edges while they are not at rest.
    pub(crate) fn rippling(&self) -> Option<&island_core::surface::Surface> {
        (self.may_ripple() && !self.surface.still()).then_some(&self.surface)
    }

    /// Once a frame: the edges move on, or are laid flat where they may not move.
    fn surface_tick(&mut self, now: i64) {
        if self.may_ripple() {
            self.surface.step((now - self.surface_at) as f64);
        } else {
            self.surface = Default::default();
        }
        self.surface_at = now;
    }

    /// Content opacity for the crossfade (eased).
    pub(crate) fn content_opacity(&self) -> f32 {
        let a = self.alpha.clamp(0.0, 1.0);
        let op = if self.want != self.shown { a } else { 1.0 - (1.0 - a).powi(3) };
        op as f32
    }

    pub(crate) fn pointer_pos(&self) -> Option<(f64, f64)> {
        self.pointer
    }

    /// The parts of the surface that should take pointer input (the pill with its bubbles, and the cards under it), in
    /// surface coordinates: (x, y, width, height). The rest of the surface is transparent and must click through.
    pub fn input_rects(&self) -> Vec<(f64, f64, f64, f64)> {
        let mut v = vec![self.pill_rect()];
        v.extend(self.cards.rects().map(|(x, y, w, h, _)| (x, y, w, h)));
        v.extend(self.fill.as_ref().and_then(|f| f.rect));
        v.extend(self.catch_rect());
        v.extend(self.auth_rect());
        self.tuck_input(v)
    }

    /// The island's outline as rounded rectangles (x, y, width, height, radius) in surface coordinates: what is
    /// visible, for a platform that has to cut its window to that shape because it cannot be transparent.
    pub fn shape_rects(&self) -> Vec<(f64, f64, f64, f64, f64)> {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        let r = (h / 2.0).min(30.0);
        let mut v = vec![(self.pill_origin.0, self.pill_origin.1, w, h, r)];
        for (n, _) in self.bubbles.iter().enumerate().filter(|(_, b)| b.out(self.now) > 0.5) {
            v.push((self.pill_origin.0 + w + BUBBLE_GAP + n as f64 * (BUBBLE_W + BUBBLE_GAP), self.pill_origin.1, BUBBLE_W, h, r));
        }
        v.extend(self.fill.as_ref().and_then(|f| f.rect).map(|(x, y, w, h)| (x, y, w, h, w / 2.0)));
        v.extend(self.cards.rects());
        v.extend(self.auth_rect().map(|(x, y, w, h)| (x, y, w, h, 30.0)));
        if self.out_of_sight() {
            return vec![];
        }
        // on its way out it shows less: what is above the edge is gone
        let rise = self.tuck_rise();
        if rise > 0.0 {
            v = v.into_iter().filter(|(_, y, _, h, _)| y + h > rise).map(|(x, y, w, h, r)| (x, (y - rise).max(0.0), w, h - (rise - y).max(0.0), r)).collect();
        }
        v
    }

    pub fn current_face(&self) -> Face {
        self.shown
    }
}
