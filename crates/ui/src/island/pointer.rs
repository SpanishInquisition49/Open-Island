//! Part of `Island` (see `mod.rs`).

use super::*;

impl Island {
    // ---- pointer ------------------------------------------------------------------------------

    pub(super) fn local(&self, x: f64, y: f64) -> (f64, f64) {
        (x - self.pill_origin.0, y - self.pill_origin.1)
    }

    /// The width the small pills take: a first one and `more` after it.
    pub(crate) fn side_slot(&self, first: bool, more: usize) -> f64 {
        if first { (1 + more) as f64 * (BUBBLE_W + BUBBLE_GAP) } else { 0.0 }
    }

    /// The small pills on show, first one first.
    pub(crate) fn sides_shown(&self) -> Vec<Side> {
        self.shown_side.into_iter().chain(self.shown_more.iter().copied()).collect()
    }

    /// The small pill under this x (surface coordinates), when it is over one.
    fn side_at(&self, x: f64) -> Option<Side> {
        let rel = x - (self.pill_origin.0 + self.spring.w.pos + BUBBLE_GAP);
        if rel < 0.0 {
            return None;
        }
        self.sides_shown().get((rel / (BUBBLE_W + BUBBLE_GAP)) as usize).copied()
    }

    pub(super) fn pill_rect(&self) -> (f64, f64, f64, f64) {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        let side = (self.side_slot(self.shown_side.is_some(), self.shown_more.len()) * self.bubble_k).round();
        (self.pill_origin.0, self.pill_origin.1, w + side, h)
    }

    pub fn pointer_moved(&mut self, now: i64, x: f64, y: f64) -> Vec<Command> {
        self.now = now;
        self.pointer = Some((x, y));
        self.card_pointer_moved();
        let (px, py, pw, ph) = self.pill_rect();
        let inside = x >= px && x <= px + pw && y >= py && y <= py + ph;
        if inside && !self.hovering {
            self.hovering = true;
            self.hover_since = now;
        } else if !inside && self.hovering {
            self.hovering = false;
            self.hover_left = now;
            self.hover_page = None;
        }
        // what opens depends on where the pointer rests: the side bubble opens its own activity, the pill its own
        if inside && self.shown.is_compact() {
            let over_bubble = self.shown_side.is_some() && x > px + self.spring.w.pos;
            self.hover_page = if over_bubble { self.side_at(x).map(side_page) } else { main_page(self.shown) };
        }
        if self.dragging == Some(Action::MapPan) {
            let (dx, dy) = (x - self.map_drag_last.0, y - self.map_drag_last.1);
            self.map_drag_last = (x, y);
            if let Some(v) = self.map_view() {
                self.map_center = Some(island_core::map::pan(v.center, v.zoom, dx, dy));
                self.map_fixed_zoom = Some(v.zoom);
            }
            return std::mem::take(&mut self.pending);
        }
        if let Some(Action::SeekBar | Action::VolumeBar | Action::BrightnessBar) = self.dragging.clone() {
            return self.slider_at(x, y);
        }
        std::mem::take(&mut self.pending)
    }

    pub fn pointer_left(&mut self, now: i64) {
        self.now = now;
        self.pointer = None;
        if self.hovering {
            self.hovering = false;
            self.hover_left = now;
            self.hover_page = None;
        }
        self.dragging = None;
    }

    pub(super) fn hit_at(&self, x: f64, y: f64) -> Option<&Hit<Action>> {
        let (lx, ly) = self.local(x, y);
        // the most specific control wins: a button on a row beats the row, whatever was drawn first
        let at = vello::kurbo::Point::new(lx, ly);
        self.hits.iter().rev().filter(|h| h.rect.contains(at)).min_by(|a, b| (a.rect.width() * a.rect.height()).total_cmp(&(b.rect.width() * b.rect.height())))
    }

    pub(super) fn slider_at(&mut self, x: f64, _y: f64) -> Vec<Command> {
        let Some(action) = self.dragging.clone() else { return vec![] };
        let (lx, _) = self.local(x, 0.0);
        let Some(h) = self.hits.iter().rev().find(|h| h.action == action) else { return vec![] };
        let frac = ((lx - h.rect.x0) / h.rect.width()).clamp(0.0, 1.0);
        let mut cmds = vec![];
        match action {
            Action::SeekBar => {
                if let Some(m) = self.data.media.as_ref().filter(|m| m.can_seek && m.length_ms > 0.0) {
                    cmds.push(Command::MediaSeek { position_ms: frac * m.length_ms });
                }
            }
            Action::VolumeBar => {
                self.data.volume = Some(frac as f32);
                cmds.push(Command::SetVolume(frac as f32));
            }
            Action::BrightnessBar => {
                self.data.brightness = Some(frac as f32);
                cmds.push(Command::SetBrightness(frac as f32));
            }
            _ => {}
        }
        cmds
    }

    pub fn pointer_down(&mut self, now: i64, x: f64, y: f64) -> Vec<Command> {
        self.now = now;
        self.pointer = Some((x, y));
        let Some(action) = self.hit_at(x, y).map(|h| h.action.clone()) else {
            return self.click_background(x, y);
        };
        match action {
            Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {
                self.dragging = Some(action);
                self.slider_at(x, y)
            }
            Action::MapPan => {
                self.dragging = Some(action);
                self.map_drag_last = (x, y);
                vec![]
            }
            a => self.activate(a),
        }
    }

    pub fn pointer_up(&mut self, now: i64) {
        self.now = now;
        self.dragging = None;
    }

    pub(super) fn click_background(&mut self, x: f64, y: f64) -> Vec<Command> {
        let (px, py, pw, ph) = self.pill_rect();
        if !(x >= px && x <= px + pw && y >= py && y <= py + ph) {
            return vec![];
        }
        let now = self.now;
        // a click on the side bubble opens its own activity
        let main_w = self.spring.w.pos;
        if self.shown_side.is_some() && x > px + main_w {
            self.hover_page = self.side_at(x).map(side_page);
            self.page = self.hover_page;
            self.expand_until = now + 6000;
            return vec![];
        }
        match self.shown {
            Face::TimerDone => self.timer_done_until = 0,
            Face::Power => self.power_until = 0,
            Face::Device => self.device_until = 0,
            Face::Meeting => self.meeting_until = 0,
            Face::Rain => self.rain_until = 0,
            Face::Progress => {
                if self.ops.op.as_ref().is_some_and(|o| o.state != progress::State::Run) {
                    self.ops.clear();
                }
                self.ops.flash_until = 0;
            }
            f if f.is_compact() => self.expand_until = now + 6000,
            _ => {}
        }
        vec![]
    }

    /// The middle button on the small pill mutes the sound.
    pub fn middle_click(&mut self, now: i64) -> Vec<Command> {
        self.now = now;
        if self.shown.is_compact() {
            return vec![Command::ToggleMute];
        }
        vec![]
    }

    pub fn right_click(&mut self, now: i64) {
        self.now = now;
        // on a to-do row: its menu, not "close the page"
        if let Some((x, y)) = self.pointer {
            if let Some(Action::TodoDone(n)) = self.hit_at(x, y).map(|h| h.action.clone()) {
                self.row_menu = self.todos.get(n).map(|t| t.uid.clone());
                return;
            }
        }
        if self.row_menu.take().is_some() {
            return;
        }
        // close an open page, or swap the main activity and the bubble
        if self.shown.is_compact() && self.shown_side.is_some() {
            self.swapped = !self.swapped;
        } else {
            self.expand_until = 0;
            self.page = None;
            self.hover_page = None;
        }
    }

    pub fn scroll(&mut self, now: i64, dx: f64, dy: f64) -> Vec<Command> {
        self.now = now;
        // the card under the pill is not the pill
        if self.over_card() {
            return vec![];
        }
        // over the map the wheel zooms (and a sideways swipe does not turn the page under the finger)
        if self.shown == Face::Map {
            self.map_scroll_acc += dy;
            while self.map_scroll_acc.abs() >= 1.0 {
                self.map_zoom(if self.map_scroll_acc > 0.0 { 1 } else { -1 });
                self.map_scroll_acc -= self.map_scroll_acc.signum();
            }
            self.expand_until = now + 6000;
            return vec![];
        }
        if dx.abs() > dy.abs() && dx.abs() > 0.5 {
            // horizontal swipe: next/previous page
            if self.shown.size().1 > 40.0 {
                self.cycle_page(if dx > 0.0 { 1 } else { -1 });
                self.expand_until = now + 4000;
                return vec![];
            }
            // over the small pill with music: next / previous track (a swipe is many small events: add them up,
            // act once past a threshold, then leave a moment before the next one)
            if self.data.media.is_some() && now >= self.swipe_until {
                self.swipe_acc += dx;
                if self.swipe_acc.abs() >= SWIPE_TRACK {
                    let next = self.swipe_acc > 0.0;
                    self.swipe_acc = 0.0;
                    self.swipe_until = now + SWIPE_COOLDOWN_MS;
                    return vec![if next { Command::MediaNext } else { Command::MediaPrevious }];
                }
            }
            return vec![];
        }
        // vertical scroll over the pill or a media face changes the volume
        if self.shown.is_compact() || matches!(self.shown, Face::MediaFull | Face::MediaLyric | Face::MediaMini | Face::Osd | Face::Qs) {
            let v = (self.data.volume.unwrap_or(0.5) + (dy as f32) * 0.02).clamp(0.0, 1.0);
            self.data.volume = Some(v);
            return vec![Command::SetVolume(v)];
        }
        vec![]
    }

    /// Does `action` on the card of the notification `id`: that card takes the first place while it is done, so that
    /// everything acts on it, and the first one comes back after.
    fn on_card(&mut self, id: u32, action: Action) -> Vec<Command> {
        if self.notif.as_ref().is_some_and(|n| n.id == id) {
            return self.on_first_card(action);
        }
        let Some(k) = self.below.iter().position(|b| b.n.id == id) else { return vec![] };
        let target = self.below.remove(k);
        let first = self.take_front();
        self.set_front(Some(target));
        let cmds = self.on_first_card(action);
        let after = self.take_front();
        // a reply being typed stays first: that is where it is sent from
        let replying = self.reply.as_ref().is_some_and(|r| r.id == id);
        match after {
            Some(card) if card.n.id == id && replying => {
                self.set_front(Some(card));
                if let Some(first) = first {
                    self.below.insert(k, first);
                }
            }
            other => {
                self.set_front(first);
                // the card itself, still there, goes back to its place; a card the action made goes to the end
                match other {
                    Some(card) if card.n.id == id => self.below.insert(k.min(self.below.len()), card),
                    Some(card) => self.below.push(card),
                    None => {}
                }
            }
        }
        cmds
    }

    /// A press on a card is not a press on the pill: the pill opens only when the press leads to one of its pages.
    fn on_first_card(&mut self, action: Action) -> Vec<Command> {
        let (page, expand_until) = (self.page, self.expand_until);
        let cmds = self.activate(action);
        if self.page == page {
            self.expand_until = expand_until;
        }
        cmds
    }

    /// Moves to the next (1) or previous (-1) page in the dock's order, wrapping around.
    pub(crate) fn cycle_page(&mut self, dir: i32) {
        let pages = self.pages();
        let current = self.page.or_else(|| page_of(self.shown));
        self.page = face::cycle_page(&pages, current, dir);
        self.hover_page = self.page;
    }

    /// The pages the dock shows and a swipe cycles through: the live activities first, then the fixed pages.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let mut v = face::swipe_pages(
            self.data.media.is_some(),
            self.timer.is_some(),
            self.focus_active(),
            self.soon().is_some(),
            self.ops.op.is_some(),
            !self.history.is_empty(),
            self.data.weather.is_some(),
        );
        if let Some(at) = v.iter().position(|p| *p == Page::Home) {
            v.insert(at + 1, Page::Today);
            v.insert(at + 2, Page::Map);
            if self.data.phone.as_ref().is_some_and(|p| p.link.present) {
                v.insert(at + 3, Page::Phone);
            }
        }
        if self.cfg.clipboard_history {
            v.push(Page::Clip);
        }
        if let Some(at) = v.iter().position(|p| *p == Page::Sys) {
            v.insert(at + 1, Page::Activity);
        }
        // the week ahead earns a place in the dock by being opened; until then it is on the grid
        v.push(Page::Plan);
        v.push(Page::Options);
        v.push(Page::Settings);
        v
    }

    /// The pages the dock shows: Home, what is live now, the ones opened most, and the page on screen; the rest are on the grid.
    pub(crate) fn dock_pages(&self) -> Vec<Page> {
        let all = self.pages();
        let room = (self.cfg.dock_icons as usize).clamp(4, 12);
        let live = |p: &Page| matches!(p, Page::Media | Page::Timer | Page::Focus | Page::Meeting | Page::Op);
        let mut ranked: Vec<Page> = all.iter().copied().filter(|p| *p != Page::Home).collect();
        ranked.sort_by_key(|p| (!live(p), std::cmp::Reverse(self.page_uses.get(p).copied().unwrap_or(0))));
        let mut keep: Vec<Page> = ranked.into_iter().take(room - 1).collect();
        // the page you are on stays in the bar, so that it can be seen where you are
        if let Some(cur) = self.page.or_else(|| page_of(self.shown)).filter(|c| all.contains(c) && *c != Page::Home) {
            if !keep.contains(&cur) {
                keep.pop();
                keep.push(cur);
            }
        }
        all.into_iter().filter(|p| *p == Page::Home || keep.contains(p)).collect()
    }

    /// When it rains next, from the forecast.
    pub(crate) fn rain_outlook(&self) -> Option<weather::Outlook> {
        let w = self.data.weather.as_ref()?;
        Some(weather::outlook(&w.rain_slots, &w.hours, self.now))
    }

    pub(super) fn soon(&self) -> Option<&calendar::Event> {
        if !self.cfg.meeting_alerts {
            return None;
        }
        calendar::soon(&self.data.events, self.now, self.cfg.meeting_pill_ms)
    }

    pub(crate) fn focus_active(&self) -> bool {
        self.data.focus.is_some_and(|f| f.active)
    }

    /// Minutes of focus left, when the end is known.
    pub(crate) fn focus_minutes_left(&self) -> Option<i64> {
        let f = self.data.focus.filter(|f| f.active)?;
        let end = f.end_epoch?;
        Some(((end * 1000 - self.now) as f64 / 60_000.0).ceil().max(0.0) as i64)
    }

    pub(crate) fn next_event(&self) -> Option<&calendar::Event> {
        calendar::next_event(&self.data.events, self.now)
    }

    /// Makes a place the destination of the map and asks for the way from where the machine is.
    fn map_go_to(&mut self, place: island_core::travel::Place) -> Vec<Command> {
        let Some(from) = self.data.position.as_ref().map(|p| (p.lat, p.lon)) else {
            self.say("No position yet", "The way needs to know where you are");
            return vec![];
        };
        self.data.places = None;
        self.data.places_searching = false;
        self.data.route = None;
        self.data.route_pending = true;
        self.map_dest = Some(place.clone());
        self.map_both = true;
        self.map_recenter();
        vec![Command::PlanRoute { dest: place, mode: self.cfg.travel_mode, from }]
    }

    pub(super) fn activate(&mut self, a: Action) -> Vec<Command> {
        let now = self.now;
        if !matches!(a, Action::OnCard(..)) {
            self.expand_until = self.expand_until.max(now + 4000);
        }
        match a {
            Action::Cmd(c @ (Command::MediaNext | Command::MediaPrevious)) => {
                self.skip_press = if c == Command::MediaNext { 1.0 } else { -1.0 };
                return vec![c];
            }
            Action::Cmd(Command::MediaPlayPause) => {
                // show the answer now; the player's own report comes a moment later and agrees
                self.play_override = Some((!self.playing_shown(), now + PLAY_CONFIRM_MS));
                self.disc_press = 1.0;
                return vec![Command::MediaPlayPause];
            }
            Action::Cmd(c) => {
                if c == Command::RefreshCalendars {
                    self.last_refresh_req = now;
                    self.note = Some(("Reading the calendars…".into(), now + 3000));
                }
                // the chips answer at once; the next sample confirms
                match &c {
                    Command::ToggleWifi => self.data.wifi = self.data.wifi.map(|v| !v),
                    Command::ToggleBluetooth => self.data.bluetooth = self.data.bluetooth.map(|v| !v),
                    Command::ToggleKeepAwake => self.data.keep_awake = !self.data.keep_awake,
                    Command::ToggleNightLight => self.data.night_light = self.data.night_light.map(|v| !v),
                    _ => {}
                }
                return vec![c];
            }
            Action::Back => {
                // back to the page that led here, else to whatever the island shows by default
                let prev = loop {
                    match self.page_stack.pop() {
                        Some(p) if Some(p) == self.page => continue,
                        other => break other,
                    }
                };
                self.page = prev;
                self.hover_page = prev;
            }
            Action::Goto(p) => {
                self.page = Some(p);
                self.hover_page = Some(p);
            }
            Action::TimerStart(s) => self.start_timer(s),
            Action::PomodoroStart => self.start_pomodoro(),
            Action::StopwatchStart => self.push(now, Input::StartStopwatch),
            Action::TimerCancel => self.cancel_timer(),
            Action::DeadlineDone(n) => {
                if n < self.deadlines.len() {
                    self.deadlines.remove(n);
                }
            }
            Action::PhoneThread(n) => {
                if let Some(t) = self.data.phone.as_ref().and_then(|p| p.threads.get(n)).cloned() {
                    self.phone_open = Some(t.key.clone());
                    return vec![Command::PhoneOpen(t.key)];
                }
            }
            Action::PhoneList => {
                self.phone_open = None;
                self.phone_search = false;
            }
            Action::PhoneContact(n) => {
                let number = self.data.phone.as_ref().and_then(|p| p.contacts.as_ref()).and_then(|c| c.get(n)).and_then(|c| c.numbers.first()).cloned();
                if let Some(number) = number {
                    let key = island_core::phone::key_for_number(&number);
                    self.phone_search = false;
                    self.phone_open = Some(key.clone());
                    return vec![Command::PhoneOpen(key)];
                }
            }
            Action::PhoneCallDo(c) => return vec![Command::PhoneCall(c)],
            Action::OpenFile(path) => return vec![Command::OpenFile(path)],
            Action::PhoneAudioDo(here) => return vec![Command::PhoneAudio(here)],
            Action::PinCopy(n) => {
                if let Some(t) = self.pins.get(n).cloned() {
                    self.say("Copied", &t);
                    return vec![Command::RestoreClipboard(t)];
                }
            }
            Action::PinRemove(n) => {
                if n < self.pins.len() {
                    self.pins.remove(n);
                }
            }
            Action::TodoDone(n) => self.complete_task(n),
            Action::TodoPostpone(n, days) => {
                self.row_menu = None;
                let day = self.today_day() + days;
                self.postpone(&[n], day);
            }
            Action::TodoEdit(n) => {
                self.row_menu = None;
                if let Some(t) = self.todos.get(n) {
                    let words: Vec<&str> = t.text.split_whitespace().take(3).collect();
                    let text = format!("edit {} -> ", words.join(" "));
                    self.bar = Some(Bar { until: self.now + BAR_IDLE_MS, cursor: text.chars().count(), text, ..Default::default() });
                }
            }
            Action::TodoRemove(n) => {
                self.row_menu = None;
                self.remove_task(n);
            }
            Action::TodoMenuClose => self.row_menu = None,
            Action::FindOpen(i) => self.open_hit(i),
            Action::TimerCancelExtra(n) => {
                if n < self.extra_timers.len() {
                    self.extra_timers.remove(n);
                }
            }
            Action::TimerAddMinute => {
                if let Some(t) = self.timer.as_mut() {
                    if let Some(e) = t.end_ms.as_mut() {
                        *e += 60_000;
                        t.total_ms += 60_000;
                    }
                }
            }
            Action::PomodoroSkip => {
                if let Some(t) = self.timer.as_mut() {
                    t.end_ms = Some(now);
                }
            }
            Action::Dismiss | Action::NextNotif => self.close_notif(CloseReason::Dismissed),
            Action::OnCard(id, inner) => return self.on_card(id, *inner),
            Action::OptionsTab(t) => self.options_tab = t.min(crate::options::TABS.len() - 1),
            Action::MuteApp(n) => {
                if let Some((inc, _)) = self.history.get(n) {
                    let app = inc.app.clone();
                    self.say("Notifications hidden", &format!("{app} (Options > Rules to undo)"));
                    return vec![Command::AddFilter { app, content: String::new(), hide: true }];
                }
            }
            Action::RemoveRule(n) => return vec![Command::RemoveFilter(n)],
            Action::Toggle(key) => {
                let on = !crate::options::get_bool(&self.cfg, key);
                crate::options::set_bool(&mut self.cfg, key, on);
                if key == "wifi_position" {
                    // said once, plainly, when it is turned on: this is the one option that sends where you are
                    if on {
                        self.say("Wi-Fi position on", "Nearby Wi-Fi addresses go to beacondb.net");
                    } else {
                        self.say("Wi-Fi position off", "Nothing about your networks is sent");
                    }
                }
                return vec![Command::SetOption { key: key.into(), value: OptionValue::Bool(on) }];
            }
            Action::Step(key, dir) => {
                if let Some((_, _, _, step, min, max)) = (0..crate::options::TABS.len()).flat_map(crate::options::steppers).find(|s| s.0 == key) {
                    let cur = crate::options::get_num(&self.cfg, key);
                    // from "off", the first step lands on a sensible evening instead of 00:15
                    let first = match key {
                        "evening_recap_time" => Some(18.0 * 60.0),
                        "morning_recap_time" => Some(8.0 * 60.0),
                        _ => None,
                    };
                    let next = match first {
                        Some(f) if cur == 0.0 && dir > 0 => f,
                        _ => cur + dir as f64 * step,
                    };
                    let v = (next.clamp(*min, *max) * 10.0).round() / 10.0;
                    crate::options::set_num(&mut self.cfg, key, v);
                    return vec![Command::SetOption { key: key.into(), value: OptionValue::Num(v) }];
                }
            }
            Action::Snooze => {
                if let Some(n) = self.notif.clone() {
                    let text = match (n.summary.is_empty(), n.app.is_empty()) {
                        (false, false) => format!("{}: {}", n.app, n.summary),
                        (false, true) => n.summary.clone(),
                        _ => n.app.clone(),
                    };
                    self.reminders.push(Reminder { due_ms: now + SNOOZE_MS, text, ..Default::default() });
                    self.reminders.sort_by_key(|r| r.due_ms);
                    self.close_notif(CloseReason::Dismissed);
                }
            }
            Action::NotifAction(key) if key == "open-island:ocr" => {
                self.close_notif(CloseReason::Dismissed);
                return vec![Command::Ocr];
            }
            Action::NotifAction(key) if key.starts_with("open-island:") => {
                let summary = self.notif.as_ref().map(|n| n.summary.clone()).unwrap_or_default();
                self.own_card_action(&key, &summary);
                self.close_notif(CloseReason::Dismissed);
            }
            Action::NotifAction(key) => {
                if key == "inline-reply" {
                    self.start_reply();
                } else if let Some(n) = self.notif.clone() {
                    self.pending.push(Command::InvokeAction { id: n.id, key });
                    if n.resident {
                        self.notif = None;
                        self.notif_until = 0;
                    } else {
                        self.close_notif(CloseReason::Dismissed);
                    }
                }
            }
            Action::ReplySend => self.end_reply(true),
            Action::ReplyCancel => self.end_reply(false),
            Action::CyclePlayer => return vec![Command::MediaCyclePlayer],
            Action::Open(p) => {
                if let Some(cur) = self.page.or_else(|| page_of(self.shown)).filter(|c| *c != p) {
                    if self.page_stack.last() != Some(&cur) {
                        self.page_stack.push(cur);
                    }
                    self.page_stack.truncate(8);
                }
                self.page = Some(p);
                self.hover_page = Some(p);
                if p == Page::Cal {
                    let c = crate::clock::civil(now, self.data.utc_offset_secs);
                    self.cal = Cal { year: c.year, month: c.month, day: c.day };
                }
            }
            Action::CalPrev | Action::CalNext => {
                let (y, m) = calendar::add_months(self.cal.year, self.cal.month, if a == Action::CalNext { 1 } else { -1 });
                self.cal = Cal { year: y, month: m, day: 1 };
            }
            Action::CalDay(d) => self.cal.day = d,
            Action::Join(url) => {
                self.meeting_until = 0;
                return vec![Command::OpenUrl(url)];
            }
            Action::ClearHistory => self.history.clear(),
            Action::EditField(f) => {
                self.picking = None;
                self.start_edit(f)
            }
            Action::PickColor(i) => self.picking = if self.picking == Some(i) { None } else { Some(i) },
            Action::SetColor(i, color) => {
                self.picking = None;
                return vec![Command::SetCalendarColor { index: i, color }];
            }
            Action::RemoveCalendar(i) => return vec![Command::RemoveCalendar(i)],
            Action::CalToday => {
                let c = crate::clock::civil(now, self.data.utc_offset_secs);
                self.cal = Cal { year: c.year, month: c.month, day: c.day };
            }
            // a row of the page is a conversation: removing or opening it takes all of its messages
            Action::HistoryRemove(i) => {
                let count = self.history_group_at(i);
                if i < self.history.len() {
                    self.history.drain(i..(i + count).min(self.history.len()));
                }
            }
            Action::HistoryOpen(i) => {
                let count = self.history_group_at(i);
                if i < self.history.len() {
                    let (n, _) = self.history[i].clone();
                    self.history.drain(i..(i + count).min(self.history.len()));
                    self.pending.extend(Self::open_commands(&n, false));
                }
            }
            Action::NotifOpen if self.notif.as_ref().is_some_and(|n| n.id == self.net_id && self.net_id != 0) => {
                self.notif = None;
                self.notif_until = 0;
                self.say("Reconnecting…", "");
                return vec![Command::ReconnectNetwork];
            }
            Action::NotifOpen if self.notif.as_ref().is_some_and(|n| n.id == self.culprit_id && self.culprit_id != 0) => {
                self.notif = None;
                self.notif_until = 0;
                return self.activate(Action::Open(Page::Sys));
            }
            Action::NotifOpen if self.notif.as_ref().is_some_and(|n| (n.id == self.evening_id && self.evening_id != 0) || (n.id == self.morning_id && self.morning_id != 0) || (n.id == self.leave_id && self.leave_id != 0)) => {
                self.notif = None;
                self.notif_until = 0;
                return self.activate(Action::Open(Page::Today));
            }
            Action::NotifOpen if self.notif.as_ref().is_some_and(|n| n.id == self.recap_id && self.recap_id != 0) => {
                self.notif = None;
                self.notif_until = 0;
                return self.activate(Action::Open(Page::Notifs));
            }
            Action::NotifOpen => {
                if let Some(n) = self.notif.clone() {
                    self.pending.extend(Self::open_commands(&n, true));
                    if n.resident {
                        self.notif = None;
                        self.notif_until = 0;
                    } else {
                        self.close_notif(CloseReason::Dismissed);
                    }
                }
            }
            Action::ClipRestore(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    self.copied = Some((i, now + 1500));
                    return vec![Command::RestoreClipboard(t)];
                }
            }
            Action::SendTo(i) => {
                if let Some(peer) = self.data.peers.get(i).cloned() {
                    let paths = std::mem::take(&mut self.send_files);
                    let mut out = vec![Command::SendFiles { peer, paths }];
                    out.extend(self.activate(Action::Back));
                    return out;
                }
            }
            Action::CopyCode(code) => {
                self.code_copied = self.notif.as_ref().map(|n| (n.id, now + 2500));
                self.notif_until = self.notif_until.max(now + 2500);
                return vec![Command::CopyCode(code)];
            }
            Action::ClipAct(i) => {
                if let Some(act) = self.data.clipboard.get(i).and_then(|t| island_core::clipact::classify(t)) {
                    return vec![Command::OpenUrl(act.target)];
                }
            }
            Action::TravelMode(m) => {
                self.cfg.travel_mode = m;
                self.trip_asked.retain(|(_, mode), _| *mode == m);
                let mut out = vec![Command::SetOption { key: "travel_mode".into(), value: OptionValue::Text(m.name().into()) }];
                // a place picked on the map gets its route again for the new way
                if let (Some(dest), Some(p)) = (self.map_dest.clone(), self.data.position.as_ref()) {
                    self.data.route_pending = true;
                    out.push(Command::PlanRoute { dest, mode: m, from: (p.lat, p.lon) });
                }
                return out;
            }
            Action::ActivityTab(t) => self.activity_tab = t.min(1),
            Action::RewindCopy(i) => {
                if let Some(t) = self.rewind_rows().get(i).and_then(|r| r.copy.clone()) {
                    return vec![Command::RestoreClipboard(t)];
                }
            }
            Action::MapZoom(d) => self.map_zoom(d),
            Action::MapRecenter => self.map_recenter(),
            Action::MapPan => {}
            Action::MapPick(n) => {
                let Some(place) = self.data.places.as_ref().and_then(|p| p.get(n)).cloned() else { return vec![] };
                return self.map_go_to(place);
            }
            Action::MapHome => {
                let Some((lat, lon)) = self.data.config.home_pos else { return vec![] };
                let name = if self.data.config.home.is_empty() { "Home".to_string() } else { self.data.config.home.clone() };
                return self.map_go_to(island_core::travel::Place { name, detail: "Home".into(), lat, lon });
            }
            Action::MapClearDest => {
                self.map_dest = None;
                self.data.route = None;
                self.data.route_pending = false;
                self.data.places = None;
                self.data.places_searching = false;
                self.map_recenter();
            }
            Action::MapView => {
                self.map_both = !self.map_both;
                self.map_recenter();
            }
            Action::MapRefresh => {
                self.map_asked_at = now;
                return vec![Command::RefreshPosition];
            }
            Action::MapCopy => {
                if let Some(v) = self.map_view() {
                    let text = format!("{:.6}, {:.6}", v.me.0, v.me.1);
                    self.say("Position copied", &text);
                    return vec![Command::RestoreClipboard(text)];
                }
            }
            Action::MapOpen => {
                if let Some(v) = self.map_view() {
                    return vec![Command::OpenUrl(island_core::map::osm_url(v.me.0, v.me.1, v.zoom))];
                }
            }
            Action::MapRoute => {
                if let Some(v) = self.map_view() {
                    if let Some((d, _)) = v.dest {
                        let engine = if self.cfg.travel_mode == island_core::travel::Mode::Car { "car" } else { "foot" };
                        return vec![Command::OpenUrl(island_core::map::directions_url(engine, v.me, d))];
                    }
                }
            }
            Action::ClipPin(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    return vec![Command::PinClipboard(t)];
                }
            }
            Action::ClipForget(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    return vec![Command::ForgetClipboard(t)];
                }
            }
            Action::ToggleDnd => {
                self.data.dnd = !self.data.dnd;
                self.dnd_sent = self.quiet();
                return vec![Command::SetDnd(self.dnd_sent)];
            }
            Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {}
        }
        vec![]
    }

}
