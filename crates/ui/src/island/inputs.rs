//! Part of `Island` (see `mod.rs`).

use super::*;

impl Island {
    // ---- inputs -------------------------------------------------------------------------------

    pub fn push(&mut self, now: i64, input: Input) {
        self.now = now;
        match input {
            Input::Notification(n) => self.on_notification(n),
            Input::NotificationClosed(id) => {
                // closed by the sender: no signal back
                if self.notif.as_ref().is_some_and(|n| n.id == id) {
                    self.notif = None;
                    self.notif_until = 0;
                }
                if self.reply.as_ref().is_some_and(|r| r.id == id) {
                    self.reply = None;
                }
                self.waiting.retain(|n| n.id != id);
            }
            Input::Key(k) => self.on_key(k),
            Input::StartReply => self.start_reply(),
            Input::OpenBar => self.bar = Some(Bar { until: now + BAR_IDLE_MS, ..Default::default() }),
            Input::Click(x, y) => {
                let cmds = self.pointer_down(now, x, y);
                self.pending.extend(cmds);
                self.pointer_up(now);
            }
            Input::DumpHits => {
                let (ox, oy) = self.pill_origin;
                log::info!("pill at ({ox:.0}, {oy:.0}), {:.0} x {:.0}; face {:?}", self.spring.w.pos, self.spring.h.pos, self.shown);
                for h in &self.hits {
                    log::info!("  hit {:?} at surface ({:.0}, {:.0}) size {:.0} x {:.0}", h.action, h.rect.center().x + ox, h.rect.center().y + oy, h.rect.width(), h.rect.height());
                }
            }
            Input::CyclePage(dir) => {
                self.cycle_page(dir);
                self.expand_until = now + 5000;
            }
            Input::SettingsNote(t) => self.note = Some((t, now + 5000)),
            Input::OpenKeyboard(p) => {
                self.kb = Some(Kb { index: 0, last_key: now });
                self.page = p.or(Some(Page::Home));
                self.hover_page = self.page;
                self.expand_until = now + KB_IDLE_MS;
            }
            Input::Tile { z, x, y, image } => {
                if self.tiles.len() >= 80 {
                    // the oldest are not tracked: dropping everything is cheap, the visible ones come back at once
                    self.tiles.clear();
                    self.tiles_asked.clear();
                }
                self.tiles.insert((z, x, y), image);
            }
            Input::Focused { app, title } => {
                let ds = self.day_start(now);
                self.ledger.focus(now, ds, &island_core::activity::pretty_app(&app), &title);
            }
            Input::Idled { after_ms } => {
                let ds = self.day_start(now);
                self.ledger.idle(now - after_ms, ds);
            }
            Input::Returned { away_ms } => {
                let ds = self.day_start(now);
                self.ledger.resume(now, ds);
                // a short pause is not an absence
                if self.cfg.away_recap && away_ms >= 60_000 {
                    self.recap(now - away_ms, "while you were away");
                }
            }
            Input::SetDnd(on) => {
                self.data.dnd = on;
                self.dnd_sent = self.quiet();
                self.pending.push(Command::SetDnd(self.dnd_sent));
            }
            Input::Spectrum(v) => {
                self.spectrum = v;
                self.spectrum_at = now;
            }
            Input::Osd { .. } if !self.cfg.show_osd => {}
            Input::Osd { kind, level, muted } => {
                // a bar that was not on screen starts from the level it had before, and slides to the new one
                let before = match kind {
                    OsdKind::Volume => self.data.volume,
                    OsdKind::Brightness => self.data.brightness,
                };
                if self.osd.as_ref().is_none_or(|o| o.kind != kind) {
                    self.osd_shown = before.unwrap_or(level) as f64;
                }
                self.osd = Some(Osd { kind, level, muted });
                self.osd_until = now + self.cfg.osd_ms;
            }
            Input::Power(p) => {
                self.power = Some(p);
                self.power_until = now + 3500;
            }
            Input::Progress(p) => self.ops.report(now, &p),
            Input::ProgressDone(l) => self.ops.finish(now, progress::State::Done, &l),
            Input::ProgressFail(l) => self.ops.finish(now, progress::State::Fail, &l),
            Input::ProgressClear => self.ops.clear(),
            Input::DeviceConnected(d) => {
                self.device_ev = Some((d, false));
                self.device_until = now + 3500;
            }
            Input::DeviceLow(d) => {
                if !self.device_low_told.contains(&d.id) {
                    self.device_low_told.push(d.id.clone());
                    self.device_ev = Some((d, true));
                    self.device_until = now + 6000;
                }
            }
            Input::Activity(state, a) => match state {
                progress::State::Run => self.ops.run_activity(now, &a),
                s => self.ops.finish_activity(now, s, &a),
            },
            Input::CommandStart { id, label, after_secs } => {
                self.cmds.retain(|c| c.id != id);
                self.cmds.truncate(31);
                self.cmds.push(ShellCmd { id, label, start_ms: now, after_ms: after_secs as i64 * 1000, shown: false });
            }
            Input::CommandGone(id) => self.cmds.retain(|c| c.id != id),
            Input::CommandEnd { id, code } => {
                if let Some(at) = self.cmds.iter().position(|c| c.id == id) {
                    let c = self.cmds.remove(at);
                    if c.shown {
                        let took = duration::fmt_took(now - c.start_ms);
                        if code == 0 {
                            self.ops.finish(now, progress::State::Done, &format!("{} · {took}", c.label));
                        } else {
                            self.ops.finish(now, progress::State::Fail, &format!("{} · exit {code} · {took}", c.label));
                        }
                    }
                }
            }
            Input::StartTimer(secs) => self.start_timer(secs),
            Input::TodoAdd(text) => self.add_todo(text),
            Input::PinAdd(text) => self.pin(&text),
            Input::PinRemove(what) => self.unpin(&what),
            Input::StartNamedTimer(secs, label) => self.start_named_timer(secs, label),
            Input::CancelTimerNamed(label) => {
                let same = |t: &Timer| t.label.eq_ignore_ascii_case(&label);
                self.extra_timers.retain(|t| !same(t));
                if self.timer.as_ref().is_some_and(same) {
                    self.cancel_timer();
                }
            }
            Input::ActivityClear(id) => self.ops.clear_id(&id),
            Input::StartPomodoro => self.start_pomodoro(),
            Input::StartStopwatch => {
                self.keep_countdown();
                self.timer = Some(Timer { mode: TimerMode::Stopwatch, start_ms: now, end_ms: None, total_ms: 0, phase: Phase::Focus, round: 1, label: String::new() })
            }
            Input::CancelTimer => self.cancel_timer(),
            Input::Peek => self.expand_until = now + 3000,
            Input::OpenPage(p) => {
                self.activate(Action::Open(p));
                self.expand_until = now + 5000;
            }
            Input::SendFiles(paths) => {
                if !paths.is_empty() {
                    self.send_files = paths;
                    self.activate(Action::Open(Page::Send));
                    self.expand_until = now + 30_000;
                    self.pending.push(Command::ScanPeers);
                }
            }
        }
    }

    /// A screenshot taken by the compositor or a tool: its card has no use for the island's own, so it is told apart by its words.
    fn is_screenshot(n: &Incoming) -> bool {
        let s = n.summary.to_lowercase();
        !is_own(n) && n.app != "open-island" && (s.contains("screenshot") || s.contains("schermata") || s.contains("screen capture"))
    }

    pub(super) fn on_notification(&mut self, n: Incoming) {
        log::debug!("notification {} from {:?}: dnd={} showing={}", n.id, n.app, self.data.dnd, self.notif.is_some());
        if !(is_own(&n) && n.app == "open-island") {
            self.history.insert(0, (n.clone(), self.now));
            self.history.truncate(30);
            if !is_own(&n) {
                let text = if n.summary.is_empty() { n.app.clone() } else { format!("{}: {}", n.app, n.summary) };
                self.log_event(LogKind::Notification, text);
            }
        }
        // a screenshot's card becomes one that offers the text in the picture
        let n = if self.cfg.ocr_screenshots && self.cfg.show_notifications && !self.quiet() && Self::is_screenshot(&n) {
            self.next_own_id += 1;
            Incoming { id: self.next_own_id, app: "open-island".into(), summary: n.summary.clone(), body: "Copy the text in it?".into(), actions: vec![NotifAction { key: "open-island:ocr".into(), label: "Copy text".into() }], expire_ms: Some(8000), ..Default::default() }
        } else {
            n
        };
        if (self.quiet() || !self.cfg.show_notifications) && !n.critical && !is_own(&n) {
            return;
        }
        if self.notif.as_ref().is_some_and(|c| c.id == n.id) {
            self.notif = Some(n); // an update replaces the card on screen
            self.notif_until = self.now + self.cfg.notification_ms;
            return;
        }
        // another message of the conversation on screen joins its card instead of waiting for its turn
        if let Some(cur) = self.notif.as_ref().filter(|c| !n.critical && !is_own(&n) && !is_own(c) && c.id != n.id && notify::same_conversation(&c.app, &c.summary, &n.app, &n.summary)) {
            let old = cur.id;
            self.pending.push(Command::NotificationClosed { id: old, reason: CloseReason::Expired });
            let count = self.notif_group + 1;
            self.show_notif(n);
            self.notif_group = count;
            return;
        }
        if self.notif.is_none() {
            self.show_notif(n);
        } else {
            let folded = notify::enqueue(
                &mut self.queue,
                notify::Notification { id: n.id, app: n.app.clone(), summary: n.summary.clone(), body: n.body.clone(), critical: n.critical, group_count: 1 },
                8,
            );
            if folded {
                self.waiting.retain(|w| w.app != n.app);
            }
            self.waiting.push_back(n);
        }
    }

    /// The history as the page shows it: groups of one conversation (position of the newest, how many), within half an hour.
    pub(crate) fn history_groups(&self) -> Vec<(usize, usize)> {
        let entries: Vec<(&str, &str, i64)> = self.history.iter().map(|(n, at)| (n.app.as_str(), n.summary.as_str(), *at)).collect();
        notify::group_history(&entries, 30 * 60_000)
    }

    /// How many messages the row that starts at `i` stands for.
    pub(super) fn history_group_at(&self, i: usize) -> usize {
        self.history_groups().into_iter().find(|(start, _)| *start == i).map_or(1, |(_, c)| c)
    }

    pub(super) fn show_notif(&mut self, n: Incoming) {
        let default = if n.critical { self.cfg.notification_ms * 2 } else { self.cfg.notification_ms };
        // the sender's timeout wins, within reason: 0 (never) means "until dismissed", capped at 30 s on screen
        let ms = match n.expire_ms {
            Some(0) => 30_000,
            Some(ms) if ms > 0 => ms.clamp(2000, 30_000),
            _ => default,
        };
        self.notif_until = self.now + ms;
        self.notif = Some(n);
        self.notif_group = 1;
    }

    /// What a click on a notification does: the sender's default action (while the notification is live), and
    /// the app's window to the front.
    pub(super) fn open_commands(n: &Incoming, live: bool) -> Vec<Command> {
        let mut c = vec![];
        if live && n.actions.iter().any(|a| a.key == "default") {
            c.push(Command::InvokeAction { id: n.id, key: "default".into() });
        }
        c.push(Command::FocusApp { desktop_entry: n.desktop_entry.clone(), app: n.app.clone() });
        c
    }

    /// Takes the notification off the screen and tells its sender why.
    pub(super) fn close_notif(&mut self, reason: CloseReason) {
        if let Some(n) = self.notif.take() {
            self.pending.push(Command::NotificationClosed { id: n.id, reason });
        }
        self.notif_until = 0;
    }

    pub(super) fn start_reply(&mut self) {
        let Some(n) = self.notif.clone() else { return };
        let Some(a) = n.reply_action() else { return };
        self.reply = Some(Reply {
            id: n.id,
            app: n.app.clone(),
            summary: n.summary.clone(),
            body: n.body.clone(),
            placeholder: if a.label.is_empty() { "Reply".into() } else { a.label.clone() },
            text: String::new(),
            cursor: 0,
            until: self.now + REPLY_IDLE_MS,
        });
    }

    pub(super) fn end_reply(&mut self, send: bool) {
        let Some(r) = self.reply.take() else { return };
        if send && !r.text.trim().is_empty() {
            self.pending.push(Command::NotificationReplied { id: r.id, text: r.text.trim().to_string() });
            self.pending.push(Command::NotificationClosed { id: r.id, reason: CloseReason::Dismissed });
        }
        if self.notif.as_ref().is_some_and(|n| n.id == r.id) {
            self.notif = None;
            self.notif_until = 0;
        }
    }

    /// A message from the island itself (a confirmation, a copied result).
    pub(super) fn say(&mut self, summary: &str, body: &str) {
        self.next_own_id += 1;
        let id = self.next_own_id;
        let n = Incoming { id, app: "open-island".into(), summary: summary.into(), body: body.into(), expire_ms: Some(3000), ..Default::default() };
        self.push(self.now, Input::Notification(n));
    }

    /// 0 Monday .. 6 Sunday, for a moment in time (1970-01-01 was a Thursday).
    pub(crate) fn weekday_at(&self, ms: i64) -> u8 {
        (((ms / 1000 + self.data.utc_offset_secs as i64).div_euclid(86_400) + 3).rem_euclid(7)) as u8
    }

    /// The local day number (days since 1970) of a moment.
    pub(crate) fn day_number(&self, ms: i64) -> i64 {
        (ms / 1000 + self.data.utc_offset_secs as i64).div_euclid(86_400)
    }

    /// A headline and a line for a list of items and the open to-dos ("Tomorrow · 3 events, 1 to do"). None when there is nothing.
    fn agenda_card(&self, heading: &str, items: &[island_core::timeline::Item]) -> Option<(String, String)> {
        let off = self.data.utc_offset_secs;
        let events = items.iter().filter(|i| i.kind == island_core::timeline::Kind::Event).count();
        let reminders = items.len() - events;
        let todos = self.open_todos().len();
        let soon: Vec<&Deadline> = self.deadlines_soon(7);
        if items.is_empty() && todos == 0 && soon.is_empty() {
            return None;
        }
        let when = |i: &island_core::timeline::Item| if i.all_day { "all day".to_string() } else { crate::clock::civil(i.at_ms, off).hhmm() };
        // the weather of that day, when it is known: rain is news (an umbrella), the rest is a line at the end
        let day_start = {
            let t = crate::clock::civil(self.now, off);
            let d = if heading == "Tomorrow" { t.day + 1 } else { t.day };
            crate::clock::midnight_ms(t.year, t.month, d, off)
        };
        let forecast = self.data.weather.as_ref().and_then(|w| island_core::weather::day_forecast(&w.hours, day_start));
        let mut head = vec![];
        if events > 0 {
            head.push(format!("{events} event{}", if events == 1 { "" } else { "s" }));
        }
        if reminders > 0 {
            head.push(format!("{reminders} reminder{}", if reminders == 1 { "" } else { "s" }));
        }
        if todos > 0 {
            head.push(format!("{todos} to do"));
        }
        if !soon.is_empty() {
            head.push(format!("{} deadline{}", soon.len(), if soon.len() == 1 { "" } else { "s" }));
        }
        if let Some(t) = forecast.as_ref().and_then(|f| f.rain_from_ms) {
            head.push(format!("rain from {}", crate::clock::civil(t, off).hhmm()));
        }
        let mut lines: Vec<String> = items.iter().take(3).map(|i| format!("{} {}", when(i), i.title)).collect();
        if items.len() > 3 {
            lines.push(format!("+{} more", items.len() - 3));
        }
        // a commuter wants the time to leave before anything else, when it is still ahead
        if let Some((leave, _)) = self.leave_plan(items).filter(|(l, _)| *l > self.now) {
            lines.insert(0, format!("Leave {}", crate::clock::civil(leave, off).hhmm()));
        }
        // the way home tonight, when the usual train is not on time: that is the news of the evening
        if heading == "Tomorrow" {
            if let Some(t) = self.train_tonight() {
                lines.insert(0, t);
            }
        }
        // the nearest deadline, as what it is: "Reti exam in 3 days"
        if let Some(d) = soon.first() {
            let left = d.day - self.day_number(self.now);
            lines.push(format!("{} {}", d.text, match left { i64::MIN..=-1 => "was yesterday".to_string(), 0 => "is today".to_string(), 1 => "is tomorrow".to_string(), n => format!("in {n} days") }));
        }
        if lines.is_empty() {
            lines.push(self.open_todos().iter().take(2).map(|(_, t)| t.text.clone()).collect::<Vec<_>>().join(" · "));
        }
        if let Some(f) = &forecast {
            lines.push(island_core::weather::forecast_line(f, |ms| crate::clock::civil(ms, off).hhmm()));
        }
        Some((format!("{heading} · {}", head.join(", ")), lines.join(" · ")))
    }

    /// "17:42 train +12 min" when the pinned train has not left yet, the board was read in the last 20 minutes and it is late or
    /// cancelled; None when it is on time, unknown or gone.
    pub(crate) fn train_tonight(&self) -> Option<String> {
        let (pin, st) = (self.data.train.as_ref()?, self.data.train_status.as_ref()?);
        let local = crate::clock::civil(self.now, self.data.utc_offset_secs).hhmm();
        if pin.time.as_str() <= local.as_str() || self.now - st.at_ms > 20 * 60_000 {
            return None;
        }
        match (st.cancelled, st.delay_min) {
            (true, _) => Some(format!("{} train cancelled", pin.time)),
            (_, Some(d)) if d >= 3 => Some(format!("{} train +{d} min", pin.time)),
            _ => None,
        }
    }

    /// When to leave for the first lecture-like event of `items` (see `timeline::leave_plan`); None without a commute set.
    pub(crate) fn leave_plan<'a>(&self, items: &'a [island_core::timeline::Item]) -> Option<(i64, &'a island_core::timeline::Item)> {
        island_core::timeline::leave_plan(items, self.cfg.commute_minutes * 60_000, self.cfg.travel_margin_ms)
    }

    /// All of today's items, past ones included.
    pub(crate) fn today_all(&self) -> Vec<island_core::timeline::Item> {
        let off = self.data.utc_offset_secs;
        let t = crate::clock::civil(self.now, off);
        let (start, end) = (crate::clock::midnight_ms(t.year, t.month, t.day, off), crate::clock::midnight_ms(t.year, t.month, t.day + 1, off));
        let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
        island_core::timeline::build(&self.data.events, &reminders, start, end)
    }

    /// The pinned train is late or cancelled: a card, once for each piece of news.
    pub(super) fn check_train(&mut self, now: i64) {
        let Some(pin) = self.data.train.clone() else { return };
        let Some((key, words)) = island_core::train::alert(self.data.train_status.as_ref()) else { return };
        let key = format!("{}-{}-{}", pin.number, pin.time, key);
        if self.train_alerted.as_deref() == Some(&key) {
            return;
        }
        self.train_alerted = Some(key);
        let plat = self.data.train_status.as_ref().and_then(|s| s.platform.clone()).map(|p| format!(" · track {p}")).unwrap_or_default();
        self.next_own_id += 1;
        let n = Incoming { id: self.next_own_id, app: "open-island".into(), summary: format!("{}: {words}", pin.label()), body: format!("{} {} → {}{plat}", pin.time, pin.from_name, pin.to_name), expire_ms: Some(20_000), ..Default::default() };
        self.push(now, Input::Notification(n));
        self.pending.push(Command::PlaySound);
    }

    /// At the time to leave, once per event, and not when it is long past: a card that says so.
    pub(super) fn check_departure(&mut self, now: i64) {
        if self.cfg.commute_minutes <= 0 || !self.recap_ready(now) {
            return;
        }
        let items = self.today_all();
        let Some((leave, ev)) = self.leave_plan(&items) else { return };
        let key = (ev.title.clone(), ev.at_ms);
        if now < leave || now - leave > 15 * 60_000 || now >= ev.at_ms || self.left_for.as_ref() == Some(&key) {
            return;
        }
        let off = self.data.utc_offset_secs;
        let place = items.iter().find(|i| i.at_ms == ev.at_ms && i.title == ev.title).and_then(|_| self.data.events.iter().find(|e| e.start_ms == ev.at_ms && e.title == ev.title)).map(|e| e.location.trim().to_string()).filter(|l| !l.is_empty());
        let body = match place {
            Some(p) => format!("{} at {} · {p}", ev.title, crate::clock::civil(ev.at_ms, off).hhmm()),
            None => format!("{} at {}", ev.title, crate::clock::civil(ev.at_ms, off).hhmm()),
        };
        self.left_for = Some(key);
        self.next_own_id += 1;
        self.leave_id = self.next_own_id;
        let n = Incoming { id: self.leave_id, app: "open-island".into(), summary: "Time to leave".into(), body, expire_ms: Some(60_000), ..Default::default() };
        self.push(now, Input::Notification(n));
        self.pending.push(Command::PlaySound);
    }

    /// What the evening card says: tomorrow.
    pub(crate) fn evening_card(&self) -> Option<(String, String)> {
        self.agenda_card("Tomorrow", &self.tomorrow_items())
    }

    /// What the morning card says: what is left of today.
    pub(crate) fn morning_card(&self) -> Option<(String, String)> {
        self.agenda_card("Today", &self.today_left())
    }

    /// Today's items that have not ended yet (all-day ones stay).
    pub(crate) fn today_left(&self) -> Vec<island_core::timeline::Item> {
        let off = self.data.utc_offset_secs;
        let t = crate::clock::civil(self.now, off);
        let (start, end) = (crate::clock::midnight_ms(t.year, t.month, t.day, off), crate::clock::midnight_ms(t.year, t.month, t.day + 1, off));
        let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
        island_core::timeline::build(&self.data.events, &reminders, start, end).into_iter().filter(|i| i.all_day || i.end_ms.max(i.at_ms) > self.now).collect()
    }

    /// The card with tomorrow, held on screen long enough to read; false when there is nothing to say.
    pub fn show_evening(&mut self) -> bool {
        let card = self.evening_card();
        self.show_card(card, false)
    }

    /// Once a day, at the chosen time (or the first moment after it, until 23:30), the card with tomorrow.
    pub(super) fn check_evening(&mut self, now: i64) {
        let at = self.cfg.evening_recap_min;
        if at <= 0 {
            return;
        }
        let (day, local_min) = (self.day_number(now), ((now / 1000 + self.data.utc_offset_secs as i64).rem_euclid(86_400)) / 60);
        if self.evening_day == day || local_min < at || local_min >= 23 * 60 + 30 {
            return;
        }
        if !self.cfg.evening_weekends && self.weekday_at(now) >= 5 {
            return;
        }
        if !self.recap_ready(now) {
            return;
        }
        // asked for once: even when there is nothing to say today, it does not ask again every frame
        self.evening_day = day;
        self.show_evening();
    }

    /// The calendars arrive a few seconds after the island starts: a card made before would miss the events. It waits for
    /// some, or for 30 seconds (a day with none is still a day).
    fn recap_ready(&mut self, now: i64) -> bool {
        let since = *self.first_tick_ms.get_or_insert(now);
        !self.data.events.is_empty() || now - since >= 30_000
    }

    fn show_card(&mut self, card: Option<(String, String)>, morning: bool) -> bool {
        let Some((summary, body)) = card else { return false };
        self.next_own_id += 1;
        if morning {
            self.morning_id = self.next_own_id;
        } else {
            self.evening_id = self.next_own_id;
        }
        let n = Incoming { id: self.next_own_id, app: "open-island".into(), summary, body, expire_ms: Some(45_000), ..Default::default() };
        self.push(self.now, Input::Notification(n));
        true
    }

    /// The card with today, held on screen long enough to read; false when there is nothing to say.
    pub fn show_morning(&mut self) -> bool {
        let card = self.morning_card();
        self.show_card(card, true)
    }

    /// Once a day, at the first moment from the chosen time until 14:00 (the machine may be opened late), the card with today.
    pub(super) fn check_morning(&mut self, now: i64) {
        let at = self.cfg.morning_recap_min;
        if at <= 0 {
            return;
        }
        let (day, local_min) = (self.day_number(now), ((now / 1000 + self.data.utc_offset_secs as i64).rem_euclid(86_400)) / 60);
        if self.morning_day == day || local_min < at || local_min >= 14 * 60 {
            return;
        }
        if !self.cfg.evening_weekends && self.weekday_at(now) >= 5 {
            return;
        }
        if !self.recap_ready(now) {
            return;
        }
        self.morning_day = day;
        self.show_morning();
    }

    pub fn morning_day(&self) -> i64 {
        self.morning_day
    }

    pub fn restore_morning_day(&mut self, day: i64) {
        self.morning_day = day;
    }

    /// The last day the evening card came (kept across restarts by the platform layer).
    pub fn evening_day(&self) -> i64 {
        self.evening_day
    }

    pub fn restore_evening_day(&mut self, day: i64) {
        self.evening_day = day;
    }

    pub(crate) fn bar_cmd(&self) -> Option<island_core::bar::Cmd> {
        let b = self.bar.as_ref()?;
        let local = (self.now / 1000 + self.data.utc_offset_secs as i64).rem_euclid(86_400);
        let t = crate::clock::civil(self.now, self.data.utc_offset_secs);
        let ctx = island_core::bar::Ctx { local_secs: local, weekday: Some(self.weekday_at(self.now)), today: Some((t.year as i64, t.month as i64, t.day as i64)), rates: &self.data.rates };
        island_core::bar::parse_ctx(&b.text, &ctx)
    }

    pub(super) fn bar_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(b) = self.bar.as_mut() else { return };
        b.until = now + BAR_IDLE_MS;
        if edit_text(&mut b.text, &mut b.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Escape => self.bar = None,
            KeyInput::Up | KeyInput::Down => self.bar_recall(matches!(k, KeyInput::Up)),
            KeyInput::Enter => {
                // nothing understood: the bar stays, its preview line says so
                if let Some(c) = self.bar_cmd() {
                    let line = self.bar.as_ref().map(|b| b.text.trim().to_string()).unwrap_or_default();
                    self.bar = None;
                    // a line that made a password is not kept
                    if !matches!(c, island_core::bar::Cmd::Gen(island_core::quick::Gen::Password(_))) {
                        self.bar_history.retain(|h| *h != line);
                        self.bar_history.push(line);
                        let extra = self.bar_history.len().saturating_sub(30);
                        self.bar_history.drain(..extra);
                    }
                    self.run_bar(c);
                }
            }
            _ => {}
        }
    }

    /// Up brings back the line run before, Down goes forward again and ends on what was being typed.
    fn bar_recall(&mut self, older: bool) {
        let n = self.bar_history.len();
        let Some(b) = self.bar.as_mut() else { return };
        let next = match (b.recall, older) {
            (None, true) if n > 0 => {
                b.draft = b.text.clone();
                Some(n - 1)
            }
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < n => Some(i + 1),
            (Some(_), false) => None,
            _ => return,
        };
        b.recall = next;
        b.text = next.map_or_else(|| b.draft.clone(), |i| self.bar_history[i].clone());
        b.cursor = b.text.chars().count();
    }

    pub fn bar_history(&self) -> &[String] {
        &self.bar_history
    }

    pub fn restore_bar_history(&mut self, h: Vec<String>) {
        self.bar_history = h;
    }

    pub(super) fn run_bar(&mut self, c: island_core::bar::Cmd) {
        use island_core::bar::Cmd;
        let now = self.now;
        match c {
            Cmd::Timer { secs, label } => self.start_named_timer(secs, label),
            Cmd::Pomodoro => self.start_pomodoro(),
            Cmd::Stopwatch => self.push(now, Input::StartStopwatch),
            Cmd::Dnd(on) => self.pending.push(Command::SetDnd(on)),
            Cmd::Focus { minutes } => self.pending.push(Command::StartFocus(minutes)),
            Cmd::Remind { secs, text } => {
                self.reminders.push(Reminder { due_ms: now + secs * 1000, text: text.clone() });
                self.reminders.sort_by_key(|r| r.due_ms);
                self.reminders.truncate(50);
                self.say("Reminder set", &format!("in {} · {text}", island_core::duration::fmt_took(secs * 1000).replace(' ', "")));
            }
            Cmd::Ocr => self.pending.push(Command::Ocr),
            Cmd::Go(page) => self.push(now, Input::OpenPage(page)),
            Cmd::Pin(text) => self.pin(&text),
            Cmd::Unpin(what) => self.unpin(&what),
            Cmd::Deadline { day, text } => {
                self.deadlines.push(Deadline { day, text: text.chars().take(80).collect() });
                self.deadlines.sort_by_key(|d| d.day);
                self.deadlines.truncate(20);
                let left = day - self.day_number(now);
                self.say("Deadline added", &format!("{text} · {}", if left <= 0 { "today".to_string() } else { format!("in {left} day{}", if left == 1 { "" } else { "s" }) }));
            }
            Cmd::Train(q) => {
                self.say("Looking for the train", "Asking Trenitalia");
                self.pending.push(Command::PinTrain(q));
            }
            Cmd::TrainOff => {
                self.data.train = None;
                self.data.train_status = None;
                self.pending.push(Command::UnpinTrain);
                self.say("Train unpinned", "");
            }
            Cmd::Home(h) => {
                self.say("Setting home", &h);
                self.pending.push(Command::SetHome(h));
            }
            Cmd::Todo(text) => self.add_todo(text),
            Cmd::TodoDaily(text) => self.add_daily_todo(text),
            Cmd::UntilNext => {
                let next = self.data.events.iter().filter(|e| !e.all_day && e.start_ms > now).min_by_key(|e| e.start_ms).map(|e| (e.start_ms, e.title.clone()));
                match next {
                    Some((start, title)) if start - now > 6 * 60_000 => self.start_named_timer(((start - now) / 1000 - 300) as u64, format!("Before {title}")),
                    Some((start, title)) => self.say("Almost time", &format!("{title} starts in {} min", ((start - now) / 60_000).max(1))),
                    None => self.say("No next event", "Nothing else is planned"),
                }
            }
            Cmd::Today => {
                if !self.show_morning() {
                    self.say("Nothing left today", "No events, reminders or to-dos");
                }
            }
            Cmd::Tomorrow => {
                if !self.show_evening() {
                    self.say("Nothing planned tomorrow", "No events, reminders or to-dos");
                }
            }
            Cmd::Gen(g) => {
                let mut rng = os_random;
                let v = island_core::quick::generate(&g, &mut rng);
                let copy = v.rsplit_once(" = ").map_or(v.clone(), |(_, r)| r.to_string());
                if matches!(g, island_core::quick::Gen::Password(_)) {
                    // like a verification code: out of the clipboard history, cleared from the clipboard after 30 s
                    self.pending.push(Command::CopyCode(copy));
                    self.say("Password copied", "Cleared from the clipboard in 30 s");
                } else {
                    self.pending.push(Command::RestoreClipboard(copy.clone()));
                    self.say("Copied", &copy);
                }
            }
            Cmd::Value(v) => {
                let copy = v.rsplit_once(" = ").map_or(v.clone(), |(_, r)| r.to_string());
                self.pending.push(Command::RestoreClipboard(copy.clone()));
                self.say("Copied", &copy);
            }
        }
    }

    fn add_todo_item(&mut self, t: Todo) {
        self.todos.push(t);
        self.todos.truncate(30);
    }

    /// A to-do that comes back every day (the charger, the pass, lunch): ticking it hides it until tomorrow.
    pub(super) fn add_daily_todo(&mut self, text: String) {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
        if text.is_empty() || self.todos.iter().any(|t| t.text == text) {
            return;
        }
        self.add_todo_item(Todo { text: text.clone(), daily: true, done_day: i64::MIN });
        self.say("Added, every day", &text);
    }

    /// The to-dos still to do today, with their position in the list: a daily one that was ticked today waits for tomorrow.
    pub(crate) fn open_todos(&self) -> Vec<(usize, &Todo)> {
        let today = self.day_number(self.now);
        self.todos.iter().enumerate().filter(|(_, t)| !(t.daily && t.done_day == today)).collect()
    }

    pub(super) fn add_todo(&mut self, text: String) {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
        if text.is_empty() || self.todos.iter().any(|t| t.text == text) {
            return;
        }
        self.add_todo_item(Todo { text: text.clone(), ..Default::default() });
        self.say("Added to your list", &text);
    }

    /// Deadlines that have not passed (one is kept for the day after, so that "yesterday" is seen), soonest first.
    pub fn deadlines(&self) -> &[Deadline] {
        &self.deadlines
    }

    pub fn restore_deadlines(&mut self, d: Vec<Deadline>) {
        self.deadlines = d;
        self.deadlines.sort_by_key(|d| d.day);
    }

    /// Drops what is more than a day behind.
    pub(super) fn expire_deadlines(&mut self, now: i64) {
        let today = self.day_number(now);
        self.deadlines.retain(|d| d.day >= today - 1);
    }

    /// The deadlines that matter now: within `days` days (or just passed), soonest first.
    pub(crate) fn deadlines_soon(&self, days: i64) -> Vec<&Deadline> {
        let today = self.day_number(self.now);
        self.deadlines.iter().filter(|d| d.day - today <= days).collect()
    }

    pub fn todos(&self) -> &[Todo] {
        &self.todos
    }

    /// Keeps a note on the Today page (newest first, at most 8; the same text again only moves it up).
    pub fn pin(&mut self, text: &str) {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
        if text.is_empty() {
            return;
        }
        self.pins.retain(|p| *p != text);
        self.pins.insert(0, text.clone());
        self.pins.truncate(8);
        self.say("Pinned to Today", &text);
    }

    /// Takes pins off: "" the newest, "all" every one, else the first containing the words (any case).
    pub fn unpin(&mut self, what: &str) {
        let what = what.trim().to_lowercase();
        let gone = match what.as_str() {
            "" => (!self.pins.is_empty()).then(|| vec![self.pins.remove(0)]),
            "all" | "tutti" => Some(std::mem::take(&mut self.pins)),
            w => self.pins.iter().position(|p| p.to_lowercase().contains(w)).map(|i| vec![self.pins.remove(i)]),
        };
        match gone {
            Some(g) if !g.is_empty() => self.say("Unpinned", &if g.len() == 1 { g[0].clone() } else { format!("{} notes", g.len()) }),
            _ => self.say("Nothing to unpin", ""),
        }
    }

    /// How often each page was opened, by page key, to be kept between runs.
    pub fn page_usage(&self) -> Vec<(String, u32)> {
        let mut v: Vec<(String, u32)> = self.page_uses.iter().map(|(p, n)| (p.key().to_string(), *n)).collect();
        v.sort();
        v
    }

    pub fn restore_page_usage(&mut self, v: Vec<(String, u32)>) {
        self.page_uses = v.into_iter().filter_map(|(k, n)| Some((island_core::face::page_by_name(&k)?, n))).collect();
    }

    pub fn pins(&self) -> &[String] {
        &self.pins
    }

    pub fn restore_pins(&mut self, p: Vec<String>) {
        self.pins = p;
        self.pins.truncate(8);
    }

    pub fn restore_todos(&mut self, t: Vec<Todo>) {
        self.todos = t;
    }

    /// Reminders that are due become notifications.
    pub(super) fn fire_reminders(&mut self, now: i64) {
        while self.reminders.first().is_some_and(|r| r.due_ms <= now) {
            let r = self.reminders.remove(0);
            self.next_own_id += 1;
            let n = Incoming { id: self.next_own_id, app: "Reminder".into(), summary: r.text, body: String::new(), expire_ms: Some(15_000), ..Default::default() };
            self.pending.push(Command::PlaySound);
            self.push(now, Input::Notification(n));
        }
    }

    pub fn reminders(&self) -> &[Reminder] {
        &self.reminders
    }

    pub fn restore_reminders(&mut self, r: Vec<Reminder>) {
        self.reminders = r;
        self.reminders.truncate(50);
    }

    pub(super) fn on_key(&mut self, k: KeyInput) {
        let now = self.now;
        if self.edit.is_some() {
            self.edit_key(k);
            return;
        }
        if self.bar.is_some() {
            self.bar_key(k);
            return;
        }
        if self.reply.is_none() {
            self.kb_key(k);
            return;
        }
        let Some(r) = self.reply.as_mut() else { return };
        r.until = now + REPLY_IDLE_MS;
        if edit_text(&mut r.text, &mut r.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Enter => self.end_reply(true),
            KeyInput::Escape => self.end_reply(false),
            _ => {}
        }
    }

    /// Opens a tab of the options page (0 Island, 1 Alerts, 2 Music).
    pub fn show_options_tab(&mut self, tab: usize) {
        self.options_tab = tab.min(crate::options::TABS.len() - 1);
    }

    /// Opens an iPhone conversation on the Phone page (for the preview tool).
    pub fn set_preview_phone_open(&mut self, key: &str) {
        self.phone_open = Some(key.into());
    }

    /// Picks a place on the map page (for the preview tool).
    pub fn set_preview_map_dest(&mut self, p: island_core::travel::Place) {
        self.map_dest = Some(p);
    }

    /// The centre and zoom of the map page (for the preview tool).
    pub fn map_view_for_preview(&self) -> (f64, f64, u8) {
        self.map_view().map_or((0.0, 0.0, 3), |v| (v.center.0, v.center.1, v.zoom))
    }

    /// Opens a tab of the activity page (0 today, 1 rewind).
    pub fn show_activity_tab(&mut self, tab: usize) {
        self.activity_tab = tab.min(1);
    }

    pub fn history(&self) -> &[(Incoming, i64)] {
        &self.history
    }

    pub fn restore_history(&mut self, h: Vec<(Incoming, i64)>) {
        self.history = h;
        self.history.truncate(30);
    }

    /// The new do-not-disturb state when it changed since the last call (to persist it).
    pub fn dnd_changed(&mut self) -> Option<bool> {
        (self.dnd_seen != self.data.dnd).then(|| {
            self.dnd_seen = self.data.dnd;
            self.data.dnd
        })
    }

    /// What the island asks the platform to do, since the last call.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending)
    }

    /// The controls the keyboard can reach: every drawn hit except the ones that cover the whole face.
    pub(crate) fn nav_hits(&self) -> Vec<usize> {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        self.hits.iter().enumerate().filter(|(_, hit)| !(hit.rect.width() > w * 0.9 && hit.rect.height() > h * 0.5)).map(|(i, _)| i).collect()
    }

    /// A new tile provider or key: the tiles in memory are of the old one.
    pub(super) fn forget_tiles(&mut self) {
        self.tiles.clear();
        self.tiles_asked.clear();
    }

    pub(super) fn start_edit(&mut self, field: Field) {
        let c = &self.data.config;
        let text = match field {
            Field::Calendar | Field::ICloudUser | Field::ICloudPassword | Field::CalDavServer | Field::CalDavUser | Field::CalDavPassword => String::new(),
            Field::CalendarName(i) => c.calendars.get(i).map(|x| x.name.clone()).unwrap_or_default(),
            Field::Font => c.font.clone(),
            Field::Latitude => c.latitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Longitude => c.longitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Home | Field::PhoneReply | Field::PhoneSearch => String::new(),
            // the pinned train as the line that would pin it again, to be edited
            Field::Train => self.data.train.as_ref().map(|t| format!("{} {}{}", t.from_name, t.time, if t.to_name.is_empty() { String::new() } else { format!(" verso {}", t.to_name) })).unwrap_or_default(),
            Field::Sound => c.notification_sound.clone(),
            Field::RuleHide | Field::RuleSilence => String::new(),
            Field::TimerPresets => c.timer_presets.clone(),
            Field::Screen => if c.screen == "focused" { String::new() } else { c.screen.clone() },
            Field::MapTiles => c.map_tile_url.clone(),
            Field::MapKey | Field::MapSearch => String::new(),
        };
        let cursor = text.chars().count();
        self.edit = Some(Edit { field, text, cursor, until: self.now + EDIT_IDLE_MS });
        self.expand_until = self.now + EDIT_IDLE_MS;
    }

    pub(super) fn edit_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(e) = self.edit.as_mut() else { return };
        e.until = now + EDIT_IDLE_MS;
        self.expand_until = now + EDIT_IDLE_MS;
        if edit_text(&mut e.text, &mut e.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Escape => {
                self.edit = None;
                self.icloud_user = None;
            }
            KeyInput::Enter => {
                let Some(e) = self.edit.take() else { return };
                let t = e.text.trim().to_string();
                let note = |s: &str| Some((s.to_string(), now + 5000));
                match e.field {
                    Field::Calendar if crate::config_check::is_calendar_source(&t) => self.pending.push(Command::AddCalendar(t)),
                    Field::Calendar => {
                        self.note = note("Paste an https:// or webcal:// calendar address");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::CalendarName(i) => self.pending.push(Command::RenameCalendar { index: i, name: t }),
                    Field::ICloudUser if t.contains('@') && t.len() > 3 => {
                        self.icloud_user = Some(t);
                        self.note = Some(("Now the app-specific password (appleid.apple.com > Sign-In and Security)".into(), now + 8000));
                        self.start_edit(Field::ICloudPassword);
                    }
                    Field::ICloudUser => {
                        self.note = note("Type the Apple ID (an email address)");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::ICloudPassword if !t.is_empty() => {
                        if let Some(user) = self.icloud_user.take() {
                            self.pending.push(Command::AddICloud { user, password: t });
                            self.note = note("Signing in to iCloud...");
                        }
                    }
                    Field::ICloudPassword => {
                        self.note = note("An app-specific password is needed (not the Apple ID password)");
                        self.edit = Some(e);
                    }
                    Field::CalDavServer if crate::config_check::is_calendar_url(&t) => {
                        self.caldav_draft = (Some(t), None);
                        self.note = Some(("Now the user name of the account".into(), now + 8000));
                        self.start_edit(Field::CalDavUser);
                    }
                    Field::CalDavServer => {
                        self.note = note("The address of the server, e.g. https://cloud.example.org/remote.php/dav/");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::CalDavUser if !t.is_empty() => {
                        self.caldav_draft.1 = Some(t);
                        self.note = Some(("Now the password (an app password when the server offers them)".into(), now + 8000));
                        self.start_edit(Field::CalDavPassword);
                    }
                    Field::CalDavUser => {
                        self.note = note("Type the user name");
                        self.edit = Some(e);
                    }
                    Field::CalDavPassword if !t.is_empty() => {
                        if let (Some(server), Some(user)) = std::mem::take(&mut self.caldav_draft) {
                            self.pending.push(Command::AddCalDav { server, user, password: t });
                            self.note = note("Signing in...");
                        }
                    }
                    Field::CalDavPassword => {
                        self.note = note("A password is needed");
                        self.edit = Some(e);
                    }
                    Field::PhoneSearch => {
                        if !t.trim().is_empty() {
                            self.phone_search = true;
                            self.pending.push(Command::PhoneSearch(t.trim().to_string()));
                        }
                    }
                    Field::PhoneReply => {
                        if let (Some(key), false) = (self.phone_open.clone(), t.trim().is_empty()) {
                            self.say("Sending", &t);
                            self.pending.push(Command::PhoneSend { key, text: t });
                        }
                    }
                    Field::Home => {
                        self.say("Setting home", if t.is_empty() { "removing it" } else { &t });
                        self.pending.push(Command::SetHome(t));
                    }
                    Field::Train if t.is_empty() => {
                        self.data.train = None;
                        self.data.train_status = None;
                        self.pending.push(Command::UnpinTrain);
                    }
                    Field::Train if island_core::train::parse_pin_query(&t).is_some() => {
                        self.say("Looking for the train", "Asking Trenitalia");
                        self.pending.push(Command::PinTrain(t));
                    }
                    Field::Train => {
                        self.note = note("Say the station and the time: pisa centrale 17:12 verso lucca");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::Font => self.pending.push(Command::SetFont(t)),
                    Field::Sound => self.pending.push(Command::SetNotificationSound(t)),
                    Field::RuleHide | Field::RuleSilence if t.is_empty() => {}
                    Field::RuleHide | Field::RuleSilence => {
                        // a word is matched against the app and the text: the app first
                        self.pending.push(Command::AddFilter { app: t, content: String::new(), hide: e.field == Field::RuleHide });
                    }
                    Field::TimerPresets => self.pending.push(Command::SetOption { key: "timer_presets".into(), value: OptionValue::Text(t) }),
                    Field::Screen => self.pending.push(Command::SetOption { key: "screen".into(), value: OptionValue::Text(t) }),
                    Field::MapTiles => {
                        // a preset name stands for its URL
                        let t = if t.trim().is_empty() { String::new() } else { island_core::map::resolve(&t).unwrap_or_else(|| t.trim().to_string()) };
                        if !t.is_empty() && !island_core::map::valid_template(&t) {
                            self.note = note("A preset (esri-dark, osm...) or an https URL with {z}, {x} and {y}");
                            self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                        } else {
                            self.cfg.map_tile_url = if t.is_empty() { island_core::map::DEFAULT_TILES.to_string() } else { t.clone() };
                            self.data.config.map_tile_url = t.clone();
                            self.forget_tiles();
                            self.pending.push(Command::SetOption { key: "map_tile_url".into(), value: OptionValue::Text(t) });
                        }
                    }
                    Field::MapSearch => {
                        let q = t.trim().to_string();
                        if !q.is_empty() {
                            self.data.places = Some(vec![]);
                            self.data.places_searching = true;
                            let near = self.data.position.as_ref().map(|p| (p.lat, p.lon));
                            self.pending.push(Command::SearchPlaces { query: q, near });
                        }
                    }
                    Field::MapKey => {
                        let t = t.trim().to_string();
                        if t.chars().any(char::is_whitespace) {
                            self.note = note("An API key has no spaces");
                            self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                        } else {
                            self.data.config.map_key_set = !t.is_empty();
                            self.forget_tiles();
                            self.pending.push(Command::SetOption { key: "map_api_key".into(), value: OptionValue::Text(t) });
                        }
                    }
                    Field::Latitude | Field::Longitude => {
                        let (range, lat) = if e.field == Field::Latitude { (-90.0..=90.0, true) } else { (-180.0..=180.0, false) };
                        match t.replace(',', ".").parse::<f64>() {
                            Ok(v) if range.contains(&v) => self.pending.push(if lat { Command::SetLatitude(v) } else { Command::SetLongitude(v) }),
                            _ => {
                                self.note = note(if lat { "Latitude is a number from -90 to 90" } else { "Longitude is a number from -180 to 180" });
                                self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn kb_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(mut kb) = self.kb else { return };
        kb.last_key = now;
        self.expand_until = now + KB_IDLE_MS;
        let nav = self.nav_hits();
        let n = nav.len().max(1);
        match k {
            KeyInput::NextPage | KeyInput::PrevPage => {
                self.cycle_page(if k == KeyInput::NextPage { 1 } else { -1 });
                kb.index = 0;
            }
            KeyInput::Right | KeyInput::Down => kb.index = (kb.index + 1) % n,
            KeyInput::Left | KeyInput::Up => kb.index = (kb.index + n - 1) % n,
            KeyInput::Enter => {
                if let Some(action) = nav.get(kb.index).and_then(|i| self.hits.get(*i)).map(|h| h.action.clone()) {
                    match action {
                        Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {}
                        a => {
                            let cmds = self.activate(a);
                            self.pending.extend(cmds);
                        }
                    }
                }
            }
            KeyInput::Escape => {
                self.kb = None;
                self.expand_until = 0;
                self.page = None;
                self.hover_page = None;
                return;
            }
            _ => {}
        }
        self.kb = Some(kb);
    }

    /// True while the island needs the keyboard (an inline reply is being typed).
    /// True when the equalizer is on screen with music playing: the audio analysis is worth running.
    pub fn wants_spectrum(&self) -> bool {
        let playing = self.data.media.as_ref().is_some_and(|m| m.playing);
        let on_ac = self.data.battery.is_none_or(|b| b.on_ac);
        playing && on_ac && matches!(self.shown, Face::MediaMini | Face::MediaFull | Face::MediaLyric)
    }

    pub fn wants_keyboard(&self) -> bool {
        self.reply.is_some() || self.kb.is_some() || self.edit.is_some() || self.bar.is_some()
    }

    pub(super) fn start_timer(&mut self, secs: u64) {
        self.start_named_timer(secs, String::new());
    }

    /// A countdown joins the ones already running: the one that ends first is the main one (the pill, the timer page),
    /// the others run beside it. A Pomodoro or the stopwatch stays the main one while it lasts.
    pub(super) fn start_named_timer(&mut self, secs: u64, label: String) {
        let total = secs as i64 * 1000;
        let t = Timer { mode: TimerMode::Countdown, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, phase: Phase::Focus, round: 1, label };
        self.extra_timers.push(t);
        self.rebalance_timers();
    }

    pub(super) fn start_pomodoro(&mut self) {
        let total = pomodoro::FOCUS as i64 * 1000;
        self.keep_countdown();
        self.timer = Some(Timer { mode: TimerMode::Pomodoro, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, phase: Phase::Focus, round: 1, label: String::new() });
    }

    /// A countdown that is about to lose the main place is kept beside it instead of being dropped.
    fn keep_countdown(&mut self) {
        if let Some(t) = self.timer.take_if(|t| t.mode == TimerMode::Countdown) {
            self.extra_timers.push(t);
            self.extra_timers.sort_by_key(|t| t.end_ms.unwrap_or(i64::MAX));
        }
    }

    /// Main timer = the countdown that ends first, unless a Pomodoro or the stopwatch holds the place.
    pub(super) fn rebalance_timers(&mut self) {
        self.extra_timers.sort_by_key(|t| t.end_ms.unwrap_or(i64::MAX));
        match &self.timer {
            None if !self.extra_timers.is_empty() => self.timer = Some(self.extra_timers.remove(0)),
            Some(m) if m.mode == TimerMode::Countdown && self.extra_timers.first().is_some_and(|e| e.end_ms < m.end_ms) => {
                let old = self.timer.replace(self.extra_timers.remove(0));
                self.extra_timers.extend(old);
                self.extra_timers.sort_by_key(|t| t.end_ms.unwrap_or(i64::MAX));
            }
            _ => {}
        }
    }

    /// Ends the main timer; the next countdown takes its place.
    pub(super) fn cancel_timer(&mut self) {
        self.timer = None;
        self.rebalance_timers();
    }

    /// Countdowns beside the main one that have run out: each rings once.
    pub(super) fn ring_extra_timers(&mut self) {
        let now = self.now;
        let (done, running): (Vec<Timer>, Vec<Timer>) = std::mem::take(&mut self.extra_timers).into_iter().partition(|t| t.end_ms.is_some_and(|e| now >= e));
        self.extra_timers = running;
        for t in done {
            self.pending.push(Command::PlaySound);
            self.timer_done_text = ("Time's up".into(), if t.label.is_empty() { duration::preset_label((t.total_ms / 1000) as u64) } else { t.label });
            self.timer_done_until = now + 6000;
        }
    }

    pub fn remaining_ms(&self) -> i64 {
        self.timer.as_ref().and_then(|t| t.end_ms).map_or(0, |e| (e - self.now).max(0))
    }

    pub(super) fn on_timer_end(&mut self) {
        let Some(t) = self.timer.clone() else { return };
        self.pending.push(Command::PlaySound);
        match t.mode {
            TimerMode::Pomodoro => {
                let (phase, round, secs) = pomodoro::next(t.phase, t.round);
                self.timer_done_text = match t.phase {
                    Phase::Focus => ("Focus done".into(), format!("Break: {}", duration::preset_label(secs))),
                    Phase::Break => ("Break over".into(), format!("Round {round}: back to focus")),
                };
                let total = secs as i64 * 1000;
                self.timer = Some(Timer { phase, round, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, ..t });
            }
            _ => {
                self.timer_done_text = ("Time's up".into(), if t.label.is_empty() { duration::preset_label((t.total_ms / 1000) as u64) } else { t.label.clone() });
                self.cancel_timer();
            }
        }
        self.timer_done_until = self.now + 6000;
    }

}

/// 64 random bits from the operating system (the clock only if it fails, which is no place for a password but still a die).
fn os_random() -> u64 {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        return t.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (t >> 17);
    }
    u64::from_le_bytes(b)
}

