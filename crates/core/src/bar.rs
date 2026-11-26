//! The command bar: one line of text becomes a timer, a reminder, a sum or a unit conversion.
//!
//! ```text
//! 10m tea              timer           remind 20m call mom     a reminder (also: tra 20m / ricordami 15:30 ...)
//! pomodoro | stopwatch focus 45        dnd on | dnd off
//! 12*7+3 | sqrt(2)     a sum           5 km in mi | 70 f to c  a conversion
//! 20% of 150           a share         80 + 15% | 80 - 20%     a price with a rate
//! pw 24 | uuid | roll 2d6 | coin | random 1-100     something generated (Enter copies)
//! days until 3 feb | today + 45 days | week | 20 usd in eur     dates and money
//! split 84.5 by 3      a bill          remind at 9pm / tomorrow 8:30 call   a clock time
//! ```

/// What a line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// A countdown, with an optional name (`10m tea`).
    Timer { secs: u64, label: String },
    Pomodoro,
    Stopwatch,
    Dnd(bool),
    Focus { minutes: u32 },
    /// Remind after `secs` seconds (a clock time has already been turned into a delay).
    Remind { secs: i64, text: String },
    /// A countdown to five minutes before the next event (`until class`): study in the gap, not past it.
    UntilNext,
    /// Show tomorrow now (the evening card, on demand).
    Tomorrow,
    /// Show what is left of today now (the morning card, on demand).
    Today,
    /// A date to count down to (an exam, a project): `deadline 3 feb Reti exam`. `day` counts days since 1970-01-01.
    /// `course` and `est_min` come from `#course` and `~20h`: what to study and how long it will take.
    Deadline { day: i64, text: String, course: String, est_min: u32, cfu: u32 },
    /// Pin the usual train (`train pisa centrale 17:12 verso lucca`), unpin it (`train off`), set home (`home 43.7, 10.4` or an address).
    Train(String),
    TrainOff,
    Home(String),
    /// Something to do, for the list on the Today page (`todo buy milk`, `todo report due fri #algoritmi ~2h`,
    /// `todo gym every mon and thu`).
    Todo(crate::task::Spec),
    /// A reminder tied to an event (`remind 10m before lab`): it rings that long before the next event with those words.
    RemindBefore { secs: i64, query: String },
    /// A reminder that comes back (`remind every monday 9 gym`): `at_secs` is the time of day.
    RemindEvery { repeat: crate::task::Repeat, at_secs: i64, text: String },
    /// An event for the calendar (`event fri 15-16:30 office hours`): the day (since 1970), minutes after midnight, length.
    Event { day: i64, start_min: u32, dur_min: u32, title: String },
    /// Moves a task's day, or an event (`move report to fri`, `sposta lezione a lunedì 10`): by the words in its title.
    /// `day` is None when only the hour changes; `dur_min` None keeps the length.
    Move { query: String, day: Option<i64>, start_min: Option<u32>, dur_min: Option<u32> },
    /// Changes what a task says (`edit report -> due mon ~3h #algoritmi`): only what the right-hand side names.
    EditTask { query: String, spec: crate::task::Spec },
    /// Takes a task off the list (`rm report`, `togli report`).
    DropTask(String),
    /// Cancels the next event with these words in its title (`cancel office hours`).
    CancelEvent(String),
    /// A line for the notes folder (`note ask about the exam format`).
    Note(String),
    /// Look for words across events, tasks, deadlines, pins, notes and files (`find compilers`).
    Find(String),
    /// Study time done (`log 90m algoritmi`): it counts against the work still to do for that course.
    Study { minutes: u32, course: String },
    /// A Pomodoro that is for a course (`pomodoro algoritmi`): its focus time is logged there.
    PomodoroFor(String),
    /// Something to take along (`bring laptop for reti`: only on days with an event whose title has `reti`; without `for`,
    /// every day the card is made). It stays until `unbring`.
    Bring { text: String, course: String },
    /// Takes a bring entry off (`unbring laptop`, `unbring all`).
    Unbring(String),
    /// Read the text of the image on the clipboard and copy it (`ocr`).
    Ocr,
    /// Open a page by name (`go phone`, `go meteo`).
    Go(crate::face::Page),
    /// A note that stays on the Today page until it is removed (`pin ssh 10.0.0.5`); a click there copies it.
    Pin(String),
    /// Takes a pin off (`unpin ssh`: the first one containing it; `unpin all`; `unpin`: the newest).
    Unpin(String),
    /// Something to generate (a password, dice): made when Enter is pressed, never while typing.
    Gen(crate::quick::Gen),
    /// A sum or a conversion: the text to show and to copy.
    Value(String),
}

/// Examples shown, one set after the other, while the bar is empty: what it can do.
pub const HINTS: [&str; 9] = [
    "10m tea · remind at 9pm call mom · todo buy milk",
    "pw 24 · roll 2d6 · uuid · coin · random 1-100",
    "days until 3 feb · today + 45 days · week",
    "20 usd in eur · 5 km in mi · 20% of 150 · 12*7",
    "recap · agenda · pomodoro · focus 45 · dnd on",
    "go phone · go meteo · ocr (the image on the clipboard)",
    "todo report due fri #algoritmi ~2h · todo gym every mon",
    "event fri 15-16:30 office hours · note ask about the exam",
    "move report to sat · move lab to mon 10 · cancel dentist",
];

/// How to write a command whose first word was recognised but whose rest was not.
pub fn usage(line: &str) -> Option<&'static str> {
    let first = line.split_whitespace().next()?.to_lowercase();
    Some(match first.as_str() {
        "remind" | "reminder" | "ricorda" | "ricordami" | "promemoria" | "avvisami" => "remind 20m | at 9pm | tomorrow 8:30 | friday 18 · what to remember",
        "timer" => "timer 25m [name]  (a bare number is minutes)",
        "focus" | "studio" => "focus 45  (minutes, or 2h)",
        "dnd" | "silenzio" => "dnd on | dnd off",
        "todo" | "task" | "compito" => "todo <what> [due fri] [#course] [~2h] [every mon]",
        "event" | "evento" | "appuntamento" => "event [fri | 3 feb] 15[-16:30] [for 1h] <title>",
        "move" | "sposta" | "postpone" | "rinvia" => "move <words> to <day> [15[-16:30]]  (a task's day, or an event)",
        "edit" | "modifica" => "edit <words> -> [due fri] [#course] [~2h] [every mon]",
        "rm" | "togli" | "elimina" | "drop" => "rm <words in a task>",
        "cancel" | "annulla" | "disdici" => "cancel <words in an event title>",
        "note" | "nota" => "note <a line for your notes folder>",
        "find" | "cerca" | "search" | "trova" => "find <words>: events, tasks, notes, files",
        "log" | "studied" | "studiato" => "log 90m [course]  (study time done)",
        "bring" | "porta" => "bring <thing> [for <course>]  ·  unbring <thing | all>",
        "go" | "vai" | "apri" => "go <page>: phone, today, map, weather, calendar, system, sound, options...",
        "pin" | "appunta" => "pin <text to keep on Today>  ·  unpin [text | all]",
        "split" | "dividi" => "split 84.5 by 3",
        "days" | "giorni" => "days until 3 feb | days since 2027-01-01",
        "roll" | "dice" | "dado" | "random" | "pw" | "password" => "roll 2d6 | d20 | random 1-100 | pw 24",
        _ => return None,
    })
}

/// What the bar tells while typing: the preview line and whether Enter does something.
pub fn preview(cmd: &Cmd) -> String {
    match cmd {
        Cmd::Timer { secs, label } => format!("Start a timer · {}{}", crate::duration::fmt_remaining(*secs as i64 * 1000), if label.is_empty() { String::new() } else { format!(" · {label}") }),
        Cmd::Pomodoro => "Start a Pomodoro".into(),
        Cmd::Stopwatch => "Start the stopwatch".into(),
        Cmd::Dnd(on) => format!("Do not disturb {}", if *on { "on" } else { "off" }),
        Cmd::Focus { minutes } => format!("Focus mode · {minutes} min"),
        Cmd::Remind { secs, text } => format!("Remind in {} · {text}", human_delay(*secs)),
        Cmd::UntilNext => "Timer until 5 min before your next event".into(),
        Cmd::Tomorrow => "Show tomorrow".into(),
        Cmd::Today => "Show today".into(),
        Cmd::Deadline { day, text, course, est_min, cfu } => {
            let extra: String = [(!course.is_empty()).then(|| format!(" · #{course}")), (*cfu > 0).then(|| format!(" · {cfu} CFU")), (*est_min > 0).then(|| format!(" · {} to prepare", crate::task::fmt_minutes(*est_min)))].into_iter().flatten().collect();
            format!("Deadline · {} · {text}{extra}", crate::quick::fmt_date(crate::calendar::civil_from_days(*day)))
        }
        Cmd::Train(q) => format!("Pin the train · {q}"),
        Cmd::TrainOff => "Unpin the train".into(),
        Cmd::Home(h) => format!("Set home · {h}"),
        Cmd::Todo(t) => {
            let sum = t.summary(None);
            let due = t.due.map(|d| format!(" · {}", crate::quick::fmt_date(crate::calendar::civil_from_days(d)))).unwrap_or_default();
            format!("Add to your list · {}{due}{}", t.text, if sum.is_empty() { String::new() } else { format!(" · {sum}") })
        }
        Cmd::RemindBefore { secs, query } => format!("Remind {} before · {query}", human_delay(*secs)),
        Cmd::RemindEvery { repeat, at_secs, text } => format!("Remind {} at {:02}:{:02} · {text}", repeat.label(), at_secs / 3600, at_secs % 3600 / 60),
        Cmd::Move { query, day, start_min, .. } => {
            let to = [day.map(|d| crate::quick::fmt_date(crate::calendar::civil_from_days(d))), start_min.map(|m| format!("{:02}:{:02}", m / 60, m % 60))].into_iter().flatten().collect::<Vec<_>>().join(" ");
            format!("Move · {query} → {to}")
        }
        Cmd::EditTask { query, spec } => format!("Edit · {query} → {}", spec.summary(None)),
        Cmd::DropTask(q) => format!("Remove task · {q}"),
        Cmd::CancelEvent(q) => format!("Cancel event · {q}"),
        Cmd::Event { day, dur_min: 1440, title, .. } => format!("Event · {} · all day · {title}", crate::quick::fmt_date(crate::calendar::civil_from_days(*day))),
        Cmd::Event { day, start_min, dur_min, title } => format!("Event · {} {:02}:{:02} · {} · {title}", crate::quick::fmt_date(crate::calendar::civil_from_days(*day)), start_min / 60, start_min % 60, crate::task::fmt_minutes(*dur_min)),
        Cmd::Note(t) => format!("Note · {t}"),
        Cmd::Find(q) => format!("Find · {q}"),
        Cmd::Study { minutes, course } => format!("Studied {}{}", crate::task::fmt_minutes(*minutes), if course.is_empty() { String::new() } else { format!(" · {course}") }),
        Cmd::PomodoroFor(c) => format!("Start a Pomodoro · {c}"),
        Cmd::Bring { text, course } if course.is_empty() => format!("Bring · {text}"),
        Cmd::Bring { text, course } => format!("Bring · {text} · for {course}"),
        Cmd::Unbring(t) => format!("Stop bringing · {}", if t.is_empty() { "the newest" } else { t }),
        Cmd::Ocr => "Read the text of the image on the clipboard and copy it".into(),
        Cmd::Go(p) => format!("Go to {}", p.label()),
        Cmd::Pin(t) => format!("Pin to Today · {t}"),
        Cmd::Unpin(t) if t.is_empty() => "Unpin the newest".into(),
        Cmd::Unpin(t) => format!("Unpin · {t}"),
        Cmd::Gen(g) => crate::quick::preview_gen(g),
        Cmd::Value(v) => format!("{v}   (Enter copies)"),
    }
}

fn human_delay(secs: i64) -> String {
    match secs {
        s if s < 90 => format!("{s} s"),
        s if s < 5400 => format!("{} min", (s + 30) / 60),
        s => {
            let (h, m) = (s / 3600, (s % 3600 + 30) / 60);
            if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") }
        }
    }
}

/// Parses a line. `local_secs` is the time of day now (seconds since midnight, local), to turn `15:30` into a delay.
pub fn parse(line: &str, local_secs: i64) -> Option<Cmd> {
    parse_on(line, local_secs, None)
}

/// Like `parse`, knowing the day of the week (0 Monday .. 6 Sunday), so that `remind friday 9:00 ...` works.
pub fn parse_on(line: &str, local_secs: i64, weekday: Option<u8>) -> Option<Cmd> {
    parse_ctx(line, &Ctx { local_secs, weekday, today: None, rates: &[] })
}

/// Takes the credits of an exam (`9cfu`, `9 cfu`) out of its words; 0 when none are said.
fn take_cfu<'a>(words: &[&'a str]) -> (Vec<&'a str>, u32) {
    let credits = |w: &str| w.parse::<u32>().ok().filter(|n| (1..=60).contains(n));
    let (mut rest, mut cfu) = (Vec::with_capacity(words.len()), 0);
    for w in words {
        let low = w.to_lowercase();
        if low == "cfu" {
            if let Some(n) = rest.last().and_then(|n: &&str| credits(n)) {
                rest.pop();
                cfu = n;
                continue;
            }
        } else if let Some(n) = low.strip_suffix("cfu").and_then(credits) {
            cfu = n;
            continue;
        }
        rest.push(*w);
    }
    (rest, cfu)
}

/// What the parser may need to know about now: the time of day, the weekday, the date, the currency rates.
pub struct Ctx<'a> {
    pub local_secs: i64,
    /// 0 Monday .. 6 Sunday
    pub weekday: Option<u8>,
    /// (year, month, day) in local time
    pub today: Option<crate::quick::Ymd>,
    /// Units of each currency per euro (the ECB's rates)
    pub rates: &'a [(String, f64)],
}

pub fn parse_ctx(line: &str, ctx: &Ctx) -> Option<Cmd> {
    let (local_secs, weekday) = (ctx.local_secs, ctx.weekday);
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let lower = line.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let first = words[0];
    if let Some(g) = crate::quick::parse_gen(&words) {
        return Some(Cmd::Gen(g));
    }
    match first {
        "tomorrow" | "domani" | "recap" | "riepilogo" if words.len() == 1 => return Some(Cmd::Tomorrow),
        "today" | "oggi" | "agenda" if words.len() == 1 => return Some(Cmd::Today),
        "until" | "fino" if words.len() <= 3 && matches!(words.get(1).copied(), Some("next" | "class" | "lecture" | "event" | "lezione" | "alla" | "prossima")) => return Some(Cmd::UntilNext),
        "deadline" | "scadenza" | "esame" | "exam" if ctx.today.is_some() => {
            let today = ctx.today?;
            let (date, used) = crate::quick::parse_date(&words[1..], today)?;
            let orig: Vec<&str> = line.split_whitespace().collect();
            let (rest, cfu) = take_cfu(&orig[(1 + used).min(orig.len())..]);
            let spec = crate::task::parse_spec(&rest, None);
            // an exam is called by what it is, even when only the word was typed
            let text = if spec.text.is_empty() { if first == "esame" || first == "exam" { "Exam".to_string() } else { return None } } else { spec.text };
            return Some(Cmd::Deadline { day: crate::calendar::days_from_civil(date.0, date.1, date.2), text, course: spec.course, est_min: spec.est_min, cfu });
        }
        "train" | "treno" => {
            let rest = line.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            return match rest.to_lowercase().as_str() {
                "off" | "unpin" | "none" | "rimuovi" | "basta" => Some(Cmd::TrainOff),
                _ => crate::train::parse_pin_query(&rest).map(|_| Cmd::Train(rest)),
            };
        }
        "home" | "casa" if words.len() >= 2 => return Some(Cmd::Home(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "todo" | "task" | "compito" => {
            let orig: Vec<&str> = line.split_whitespace().collect();
            let spec = crate::task::parse_spec(&orig[1..], ctx.today);
            return (!spec.text.is_empty()).then_some(Cmd::Todo(spec));
        }
        "move" | "sposta" | "postpone" | "rinvia" if words.len() >= 3 && ctx.today.is_some() => return parse_move(line, &words, ctx.today?),
        "edit" | "modifica" if words.len() >= 3 && ctx.today.is_some() => {
            let orig: Vec<&str> = line.split_whitespace().skip(1).collect();
            let at = orig.iter().position(|w| matches!(*w, "->" | "=>" | "→"))?;
            let query = orig[..at].join(" ");
            let spec = crate::task::parse_spec(&orig[at + 1..], ctx.today);
            return (!query.is_empty() && spec != crate::task::Spec::default()).then_some(Cmd::EditTask { query, spec });
        }
        "rm" | "togli" | "elimina" | "drop" if words.len() >= 2 => return Some(Cmd::DropTask(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "cancel" | "annulla" | "disdici" if words.len() >= 2 => return Some(Cmd::CancelEvent(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "event" | "evento" | "appuntamento" if ctx.today.is_some() => return parse_event(line, &words, ctx.today?),
        "note" | "nota" if words.len() >= 2 => return Some(Cmd::Note(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "find" | "cerca" | "search" | "trova" if words.len() >= 2 => return Some(Cmd::Find(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "log" | "studied" | "studiato" if words.len() >= 2 => {
            let minutes = crate::task::parse_minutes(words[1])?;
            return Some(Cmd::Study { minutes, course: line.split_whitespace().skip(2).collect::<Vec<_>>().join(" ") });
        }
        "bring" | "porta" if words.len() >= 2 => {
            let rest: Vec<&str> = line.split_whitespace().skip(1).collect();
            // "for" / "per" before the last word group names the course
            let at = rest.iter().rposition(|w| matches!(w.to_lowercase().as_str(), "for" | "per"));
            let (text, course) = match at {
                Some(i) if i > 0 && i + 1 < rest.len() => (rest[..i].join(" "), rest[i + 1..].join(" ")),
                _ => (rest.join(" "), String::new()),
            };
            return Some(Cmd::Bring { text, course });
        }
        "unbring" | "nonportare" => return Some(Cmd::Unbring(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "ocr" | "leggi" if words.len() == 1 || words.get(1) == Some(&"testo") => return Some(Cmd::Ocr),
        "go" | "vai" | "apri" if words.len() >= 2 => return crate::face::page_by_name(&words[1..].join(" ")).map(Cmd::Go),
        "pin" | "appunta" if words.len() >= 2 => return Some(Cmd::Pin(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "unpin" | "stacca" => return Some(Cmd::Unpin(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "pomodoro" | "pomo" if words.len() == 1 => return Some(Cmd::Pomodoro),
        "pomodoro" | "pomo" => return Some(Cmd::PomodoroFor(line.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))),
        "stopwatch" | "cronometro" | "sw" => return Some(Cmd::Stopwatch),
        "dnd" | "silenzio" => {
            return match words.get(1).copied() {
                Some("on" | "si" | "sì") | None => Some(Cmd::Dnd(true)),
                Some("off" | "no") => Some(Cmd::Dnd(false)),
                _ => None,
            }
        }
        "focus" | "studio" => {
            let m = words.get(1).and_then(|w| take_duration(&[*w])).map(|(s, _)| s / 60).or(words.get(1).and_then(|w| w.parse::<i64>().ok())).unwrap_or(25);
            return (1..=600).contains(&m).then_some(Cmd::Focus { minutes: m as u32 });
        }
        "timer" => {
            let (secs, used) = take_duration_bare(&words[1..])?;
            return Some(Cmd::Timer { secs: secs as u64, label: label_after(line, 1 + used) });
        }
        "remind" | "reminder" | "ricorda" | "ricordami" | "promemoria" | "avvisami" | "in" | "tra" | "fra" => {
            let mut rest = &words[1..];
            if first != "in" && first != "tra" && first != "fra" {
                // remind me to ... in 20m / remind me in 20m ...
                if matches!(rest.first(), Some(&("me" | "mi"))) {
                    rest = &rest[1..];
                }
                if matches!(rest.first(), Some(&("in" | "tra" | "fra" | "to" | "di"))) && rest.get(1).is_some_and(|_| delay_or_clock(&rest[1..], local_secs, weekday).is_some()) {
                    rest = &rest[1..];
                }
            }
            // "10m before lab", "30 minuti prima di lezione": tied to an event, not to the clock
            if let Some((secs, used)) = take_duration(rest).filter(|(s, _)| *s > 0) {
                if matches!(rest.get(used), Some(&("before" | "prima"))) {
                    let mut at = used + 1;
                    while matches!(rest.get(at), Some(&("of" | "the" | "di" | "del" | "della" | "dello" | "dell'" | "la" | "il" | "lo"))) {
                        at += 1;
                    }
                    let orig: Vec<&str> = line.split_whitespace().collect();
                    let query = orig[(words.len() - rest.len() + at).min(orig.len())..].join(" ");
                    return (!query.is_empty()).then_some(Cmd::RemindBefore { secs, query });
                }
            }
            // "every monday 9 gym", "ogni giorno alle 8:30 vitamine": a reminder that comes back
            if let Some((repeat, used)) = crate::task::parse_repeat(rest) {
                let mut i = used;
                if matches!(rest.get(i), Some(&("at" | "alle" | "@"))) {
                    i += 1;
                }
                let (at_secs, took) = match rest.get(i).and_then(|w| clock_secs(w, true)) {
                    Some(a) => (a, 1),
                    None => (9 * 3600, 0),
                };
                let orig: Vec<&str> = line.split_whitespace().collect();
                let text = orig[(words.len() - rest.len() + i + took).min(orig.len())..].join(" ");
                return Some(Cmd::RemindEvery { repeat, at_secs, text: if text.is_empty() { "Reminder".into() } else { text } });
            }
            // "at 9", "alle 21", "at 9pm": the number is a time of day, not minutes
            let at_clock = matches!(rest.first(), Some(&("at" | "alle" | "@"))).then(|| clock_delay(rest.get(1)?, local_secs, true)).flatten();
            let (secs, used) = match at_clock {
                Some(d) => (d, 2),
                None => delay_or_clock(rest, local_secs, weekday)?,
            };
            // the text keeps its own capitals: cut the same number of words from the original line
            let orig: Vec<&str> = line.split_whitespace().collect();
            let skip = words.len() - rest.len() + used;
            let text = orig[skip.min(orig.len())..].join(" ");
            let mut text = text.as_str();
            for lead in ["to ", "di ", "that "] {
                text = text.strip_prefix(lead).unwrap_or(text);
            }
            let text = text.trim().to_string();
            return Some(Cmd::Remind { secs, text: if text.is_empty() { "Reminder".into() } else { text } });
        }
        _ => {}
    }
    // "10m tea": a duration with a unit first
    if let Some((secs, used)) = take_duration(&words) {
        if words.len() == 1 || !matches!(words.get(2).copied(), Some("in" | "to" | "as" | "->")) {
            return Some(Cmd::Timer { secs: secs as u64, label: label_after(line, used) });
        }
    }
    if matches!(first, "split" | "dividi") {
        return split(&words).map(Cmd::Value);
    }
    if let Some(t) = ctx.today {
        if let Some(v) = crate::quick::date_calc(&lower, t) {
            return Some(Cmd::Value(v));
        }
    }
    if let Some(v) = crate::quick::convert_currency(&lower, ctx.rates) {
        return Some(Cmd::Value(v));
    }
    if let Some(v) = convert(&lower) {
        return Some(Cmd::Value(v));
    }
    if let Some(v) = percent(&lower) {
        return Some(Cmd::Value(v));
    }
    let expr = lower.strip_prefix('=').unwrap_or(&lower);
    eval(expr).map(|v| Cmd::Value(format_number(v)))
}

/// `event fri 15-16:30 office hours`, `evento domani 10 dentista`, `event 3 feb 14:00 for 2h exam prep`: [day] time[-time]
/// [for length] title. A time is needed; the length is one hour when neither an end nor `for` is given.
fn parse_event(line: &str, words: &[&str], today: crate::quick::Ymd) -> Option<Cmd> {
    let orig: Vec<&str> = line.split_whitespace().collect();
    let t0 = crate::calendar::days_from_civil(today.0, today.1, today.2);
    let (mut i, mut day) = (1, t0);
    if let Some((d, used)) = crate::task::parse_day(&words[1..], today) {
        day = d;
        i += used;
    }
    // the whole day: one entry without hours (start 0, length of a day)
    let whole = match (words.get(i).copied(), words.get(i + 1).copied(), words.get(i + 2).copied()) {
        (Some("all"), Some("day"), _) => Some(2),
        (Some("all-day" | "allday" | "giornata"), _, _) => Some(1),
        (Some("tutto"), Some("il"), Some("giorno")) => Some(3),
        _ => None,
    };
    if let Some(n) = whole {
        let title = orig[(i + n).min(orig.len())..].join(" ");
        return (!title.is_empty()).then_some(Cmd::Event { day, start_min: 0, dur_min: 1440, title });
    }
    if matches!(words.get(i), Some(&("at" | "alle" | "@" | "dalle" | "from"))) {
        i += 1;
    }
    let range = words.get(i)?;
    let (a, b) = match range.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (*range, None),
    };
    let start = clock_secs(a, true)?;
    let mut dur = match b {
        Some(b) => {
            let end = clock_secs(b, true)?;
            // 10-11 is an hour; an end at or before the start makes no event
            (end > start).then_some((end - start) / 60)?
        }
        None => 60,
    };
    i += 1;
    if matches!(words.get(i), Some(&("for" | "per" | "durata"))) {
        if let Some(m) = words.get(i + 1).and_then(|w| crate::task::parse_minutes(w)) {
            dur = m as i64;
            i += 2;
        }
    }
    let title = orig[i.min(orig.len())..].join(" ");
    (!title.is_empty()).then_some(Cmd::Event { day, start_min: (start / 60) as u32, dur_min: dur as u32, title })
}

/// `move <words> to <day> [at] [15[-16:30]]`: the day, the hour, or both.
fn parse_move(line: &str, words: &[&str], today: crate::quick::Ymd) -> Option<Cmd> {
    let orig: Vec<&str> = line.split_whitespace().collect();
    let sep = (2..words.len()).rev().find(|&i| matches!(words[i], "to" | "a" | "al" | "su" | "->" | "→" | "alle" | "at"))?;
    let query = orig[1..sep].join(" ");
    let rest = &words[sep + 1..];
    let (day, used) = match crate::task::parse_day(rest, today) {
        Some((d, n)) => (Some(d), n),
        None => (None, 0),
    };
    let mut i = used;
    if matches!(rest.get(i), Some(&("at" | "alle" | "@" | "dalle"))) {
        i += 1;
    }
    let mut times = (None, None);
    if let Some(range) = rest.get(i) {
        let (a, b) = range.split_once('-').map_or((*range, None), |(a, b)| (a, Some(b)));
        let start = clock_secs(a, true)?;
        let dur = match b {
            Some(b) => Some(((clock_secs(b, true)? - start) / 60).try_into().ok().filter(|m: &u32| *m > 0)?),
            None => None,
        };
        times = (Some((start / 60) as u32), dur);
        i += 1;
    }
    (!query.is_empty() && i == rest.len() && (day.is_some() || times.0.is_some())).then_some(Cmd::Move { query, day, start_min: times.0, dur_min: times.1 })
}

/// 0 Monday .. 6 Sunday, from the English or Italian name (full or three letters).
pub(crate) fn weekday_of(w: &str) -> Option<u8> {
    const DAYS: [&[&str]; 7] = [
        &["monday", "mon", "lunedi", "lunedì", "lun"],
        &["tuesday", "tue", "martedi", "martedì", "mar"],
        &["wednesday", "wed", "mercoledi", "mercoledì", "mer"],
        &["thursday", "thu", "giovedi", "giovedì", "gio"],
        &["friday", "fri", "venerdi", "venerdì", "ven"],
        &["saturday", "sat", "sabato", "sab"],
        &["sunday", "sun", "domenica", "dom"],
    ];
    DAYS.iter().position(|d| d.contains(&w)).map(|i| i as u8)
}

fn delay_or_clock(words: &[&str], local_secs: i64, weekday: Option<u8>) -> Option<(i64, usize)> {
    // "friday 9:30" / "venerdì alle 9" / "friday": that day, at that time (nine o'clock when none is given)
    if let (Some(today), Some(target)) = (weekday, words.first().and_then(|w| weekday_of(w))) {
        let skip = usize::from(matches!(words.get(1), Some(&("at" | "alle" | "@")))) + 1;
        let (at, used) = match words.get(skip).and_then(|w| clock_secs(w, true)) {
            Some(a) => (a, skip + 1),
            None => (9 * 3600, 1),
        };
        let mut days = (target + 7 - today) % 7;
        if days == 0 && at <= local_secs {
            days = 7;
        }
        return Some((days as i64 * 86_400 - local_secs + at, used));
    }
    // "tomorrow 8:30" / "domani 9": that clock time, on the next day
    if matches!(words.first(), Some(&("tomorrow" | "domani"))) {
        let at = clock_secs(words.get(1)?, true)?;
        return Some((86_400 - local_secs + at, 2));
    }
    if let Some((secs, n)) = take_duration_bare(words) {
        return Some((secs, n));
    }
    // 15:30 or 9.05
    let w = words.first()?;
    clock_delay(w, local_secs, false).map(|d| (d, 1))
}

/// A time of day as seconds since midnight: `15:30`, `9.05`, and with `bare` also `9`, `9pm`, `9:30am`.
fn clock_secs(w: &str, bare: bool) -> Option<i64> {
    let (body, pm) = match (w.strip_suffix("pm"), w.strip_suffix("am")) {
        (Some(b), _) => (b, Some(true)),
        (_, Some(b)) => (b, Some(false)),
        _ => (w, None),
    };
    let (h, m): (i64, i64) = match body.split_once([':', '.']) {
        Some((h, m)) => (h.parse().ok()?, m.parse().ok()?),
        None if bare || pm.is_some() => (body.parse().ok()?, 0),
        None => return None,
    };
    let h = match pm {
        Some(true) if (1..=12).contains(&h) => h % 12 + 12,
        Some(false) if (1..=12).contains(&h) => h % 12,
        Some(_) => return None,
        None => h,
    };
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 3600 + m * 60)
}

/// Seconds until the next time the clock shows `w`.
fn clock_delay(w: &str, local_secs: i64, bare: bool) -> Option<i64> {
    let mut d = clock_secs(w, bare)? - local_secs;
    if d <= 0 {
        d += 86_400;
    }
    Some(d)
}

/// What is left of the line after its first `skip` words, with its own capitals.
fn label_after(line: &str, skip: usize) -> String {
    line.split_whitespace().skip(skip).collect::<Vec<_>>().join(" ")
}

/// A plain number or a sum (`eval` alone wants an operator).
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    s.parse().ok().or_else(|| eval(s))
}

/// `20% of 150`, `80 + 15%`, `80 - 20%`: a share of an amount, or an amount raised or cut by a rate.
fn percent(lower: &str) -> Option<String> {
    let t = lower.trim();
    if let Some((rate, base)) = t.split_once("% of ").or_else(|| t.split_once("%of ")).or_else(|| t.split_once(" % of ")) {
        let (r, b) = (number(rate)?, number(base)?);
        return Some(format_number(r / 100.0 * b));
    }
    let body = t.strip_suffix('%')?;
    let at = body.rfind(['+', '-'])?;
    let (base, rate) = (number(&body[..at])?, number(&body[at + 1..])?);
    Some(format_number(if body.as_bytes()[at] == b'+' { base * (1.0 + rate / 100.0) } else { base * (1.0 - rate / 100.0) }))
}

/// `split 84.5 by 3`: what each person pays.
fn split(words: &[&str]) -> Option<String> {
    let rest: Vec<&str> = words.iter().skip(1).copied().filter(|w| !matches!(*w, "by" | "per" | "tra" | "in" | "for")).collect();
    let (total, people): (f64, f64) = (rest.first()?.parse().ok()?, rest.get(1)?.parse().ok()?);
    (rest.len() == 2 && people >= 1.0 && people.fract() == 0.0).then(|| format!("{} each", format_number((total / people * 100.0).round() / 100.0)))
}

/// A duration that needs a unit (`10m`, `1h30m`, `20 minutes`): seconds and how many words it used.
fn take_duration(words: &[&str]) -> Option<(i64, usize)> {
    take(words, false)
}

/// A duration where a bare number counts as minutes (`timer 25`).
fn take_duration_bare(words: &[&str]) -> Option<(i64, usize)> {
    take(words, true)
}

fn unit_secs(u: &str) -> Option<f64> {
    Some(match u {
        "s" | "sec" | "secs" | "second" | "seconds" | "secondo" | "secondi" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" | "minuto" | "minuti" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" | "ora" | "ore" => 3600.0,
        _ => return None,
    })
}

fn take(words: &[&str], bare_minutes: bool) -> Option<(i64, usize)> {
    let mut total = 0.0f64;
    let mut used = 0;
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        // split "1h30m" into pieces: number, unit, number, unit...
        let mut rest = w;
        let mut got_piece = false;
        while !rest.is_empty() {
            let num_end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(rest.len());
            if num_end == 0 {
                break;
            }
            let n: f64 = rest[..num_end].replace(',', ".").parse().ok()?;
            let after = &rest[num_end..];
            let unit_end = after.find(|c: char| c.is_ascii_digit()).unwrap_or(after.len());
            let unit = &after[..unit_end];
            let mult = if unit.is_empty() {
                // the unit may be the next word ("20 minutes")
                match words.get(i + 1).and_then(|u| unit_secs(u)) {
                    Some(m) if rest.len() == num_end => {
                        i += 1;
                        used += 1;
                        m
                    }
                    _ if bare_minutes && i == 0 && used == 0 => 60.0,
                    _ => return if got_piece { Some((total.round() as i64, used + 1)).filter(|(s, _)| *s > 0) } else { None },
                }
            } else {
                unit_secs(unit)?
            };
            total += n * mult;
            got_piece = true;
            rest = &after[unit_end..];
        }
        if !got_piece {
            break;
        }
        used += 1;
        i += 1;
        // "1h 30m": keep going while the next word is also number+unit
        if !words.get(i).is_some_and(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit())) {
            break;
        }
    }
    let secs = total.round() as i64;
    (used > 0 && secs > 0).then_some((secs, used))
}

// --- the calculator -------------------------------------------------------------------------------------------

/// Evaluates `+ - * / ^ %` (modulo), brackets, `pi`, `e` and a few functions. None when it is not a sum.
pub fn eval(text: &str) -> Option<f64> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == ',' { '.' } else { c }).collect();
    // a plain number is not worth a result line
    if t.is_empty() || !t.chars().any(|c| "+-*/^%(x×÷".contains(c) || c.is_alphabetic()) {
        return None;
    }
    let mut p = Parser { s: t.chars().collect(), i: 0 };
    let v = p.sum()?;
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(c @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            if c == '+' { v += r } else { v -= r }
        }
        Some(v)
    }
    fn product(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(c @ ('*' | '/' | '%' | 'x' | '×' | '÷')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            match c {
                '*' | 'x' | '×' => v *= r,
                '/' | '÷' => v /= r,
                _ => v %= r,
            }
        }
        Some(v)
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            let e = self.power()?; // right to left
            return Some(base.powf(e));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            '+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Option<f64> {
        let c = self.peek()?;
        if c == '(' {
            self.i += 1;
            let v = self.sum()?;
            if self.peek() != Some(')') {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        if c.is_ascii_digit() || c == '.' {
            let start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                self.i += 1;
            }
            return self.s[start..self.i].iter().collect::<String>().parse().ok();
        }
        if c.is_alphabetic() {
            let start = self.i;
            while self.peek().is_some_and(|c| c.is_alphabetic()) {
                self.i += 1;
            }
            let name: String = self.s[start..self.i].iter().collect();
            match name.as_str() {
                "pi" => return Some(std::f64::consts::PI),
                "e" => return Some(std::f64::consts::E),
                _ => {}
            }
            if self.peek() != Some('(') {
                return None;
            }
            self.i += 1;
            let a = self.sum()?;
            if self.peek() != Some(')') {
                return None;
            }
            self.i += 1;
            return Some(match name.as_str() {
                "sqrt" => a.sqrt(),
                "sin" => a.to_radians().sin(),
                "cos" => a.to_radians().cos(),
                "tan" => a.to_radians().tan(),
                "ln" => a.ln(),
                "log" => a.log10(),
                "abs" => a.abs(),
                "round" => a.round(),
                "floor" => a.floor(),
                "ceil" => a.ceil(),
                _ => return None,
            });
        }
        None
    }
}

/// At most ten significant digits, no trailing zeros.
pub fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.abs() >= 1e15 || v.abs() < 1e-9 {
        return format!("{v:.6e}");
    }
    let digits = 10 - (v.abs().log10().floor() as i32 + 1).clamp(-5, 10);
    let s = format!("{:.*}", digits.clamp(0, 12) as usize, v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

// --- units ----------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Dim {
    Length,
    Mass,
    Volume,
    Speed,
    Data,
    Temp,
}

/// (names, dimension, factor to the base unit: m, g, l, m/s, byte)
fn unit(name: &str) -> Option<(Dim, f64)> {
    use Dim::*;
    Some(match name {
        "m" | "meter" | "meters" | "metre" | "metres" | "metri" => (Length, 1.0),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" | "chilometri" => (Length, 1000.0),
        "cm" | "centimeter" | "centimeters" | "centimetri" => (Length, 0.01),
        "mm" | "millimeter" | "millimeters" | "millimetri" => (Length, 0.001),
        "mi" | "mile" | "miles" | "miglia" => (Length, 1609.344),
        "ft" | "foot" | "feet" | "piedi" => (Length, 0.3048),
        "in" | "inch" | "inches" | "pollici" => (Length, 0.0254),
        "yd" | "yard" | "yards" => (Length, 0.9144),
        "nmi" => (Length, 1852.0),
        "kg" | "kilo" | "kilos" | "kilogram" | "kilograms" | "chili" => (Mass, 1000.0),
        "g" | "gram" | "grams" | "grammi" => (Mass, 1.0),
        "mg" => (Mass, 0.001),
        "lb" | "lbs" | "pound" | "pounds" | "libbre" => (Mass, 453.592_37),
        "oz" | "ounce" | "ounces" | "once" => (Mass, 28.349_523_125),
        "t" | "ton" | "tons" | "tonne" | "tonnes" => (Mass, 1_000_000.0),
        "l" | "lt" | "liter" | "liters" | "litre" | "litres" | "litri" => (Volume, 1.0),
        "ml" => (Volume, 0.001),
        "cl" => (Volume, 0.01),
        "gal" | "gallon" | "gallons" => (Volume, 3.785_411_784),
        "floz" => (Volume, 0.029_573_529_562_5),
        "cup" | "cups" => (Volume, 0.236_588_236_5),
        "tsp" => (Volume, 0.004_928_921_593_75),
        "tbsp" => (Volume, 0.014_786_764_781_25),
        "km/h" | "kmh" | "kph" => (Speed, 1000.0 / 3600.0),
        "mph" => (Speed, 0.447_04),
        "m/s" | "ms" => (Speed, 1.0),
        "kn" | "kt" | "knot" | "knots" | "nodi" => (Speed, 1852.0 / 3600.0),
        "b" | "byte" | "bytes" => (Data, 1.0),
        "kb" => (Data, 1e3),
        "mb" => (Data, 1e6),
        "gb" => (Data, 1e9),
        "tb" => (Data, 1e12),
        "kib" => (Data, 1024.0),
        "mib" => (Data, 1_048_576.0),
        "gib" => (Data, 1_073_741_824.0),
        "tib" => (Data, 1_099_511_627_776.0),
        "c" | "°c" | "celsius" => (Temp, 0.0),
        "f" | "°f" | "fahrenheit" => (Temp, 1.0),
        "k" | "kelvin" => (Temp, 2.0),
        _ => return None,
    })
}

fn to_celsius(v: f64, which: u8) -> f64 {
    match which {
        0 => v,
        1 => (v - 32.0) * 5.0 / 9.0,
        _ => v - 273.15,
    }
}

fn from_celsius(c: f64, which: u8) -> f64 {
    match which {
        0 => c,
        1 => c * 9.0 / 5.0 + 32.0,
        _ => c + 273.15,
    }
}

/// `5 km in mi` -> `5 km = 3.106856 mi`.
pub fn convert(lower: &str) -> Option<String> {
    let t = lower.trim();
    // number, then unit (attached or not), then in|to|as|->, then unit
    let num_end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' && t.starts_with('-'))).unwrap_or(t.len());
    if num_end == 0 {
        return None;
    }
    let n: f64 = t[..num_end].replace(',', ".").parse().ok()?;
    let rest: Vec<&str> = t[num_end..].split_whitespace().collect();
    let (from, to) = match rest.as_slice() {
        [from, sep, to] if matches!(*sep, "in" | "to" | "as" | "->" | "=" | "a" | "en") => (*from, *to),
        _ => return None,
    };
    let ((df, ff), (dt, ft)) = (unit(from)?, unit(to)?);
    if df != dt {
        return None;
    }
    let out = if df == Dim::Temp { from_celsius(to_celsius(n, ff as u8), ft as u8) } else { n * ff / ft };
    Some(format!("{} {} = {} {}", format_number(n), from, format_number(out), to))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: i64 = 12 * 3600;
    fn p(s: &str) -> Option<Cmd> {
        parse(s, NOON)
    }

    #[test]
    fn a_duration_first_is_a_timer() {
        assert_eq!(p("10m tea"), Some(Cmd::Timer { secs: 600, label: "tea".into() }));
        assert_eq!(p("25m"), Some(Cmd::Timer { secs: 1500, label: String::new() }));
        assert_eq!(p("1h30m"), Some(Cmd::Timer { secs: 5400, label: String::new() }));
        assert_eq!(p("90s"), Some(Cmd::Timer { secs: 90, label: String::new() }));
        assert_eq!(p("timer 20 minutes"), Some(Cmd::Timer { secs: 1200, label: String::new() }));
        assert_eq!(p("timer 25"), Some(Cmd::Timer { secs: 1500, label: String::new() }), "a bare number is minutes");
        assert_eq!(p("2 ore"), Some(Cmd::Timer { secs: 7200, label: String::new() }));
        assert_eq!(p("1.5h"), Some(Cmd::Timer { secs: 5400, label: String::new() }));
    }

    #[test]
    fn the_short_commands() {
        assert_eq!(p("pomodoro"), Some(Cmd::Pomodoro));
        assert_eq!(p("Stopwatch"), Some(Cmd::Stopwatch));
        assert_eq!(p("dnd on"), Some(Cmd::Dnd(true)));
        assert_eq!(p("dnd off"), Some(Cmd::Dnd(false)));
        assert_eq!(p("dnd maybe"), None);
        assert_eq!(p("focus 45"), Some(Cmd::Focus { minutes: 45 }));
        assert_eq!(p("focus 2h"), Some(Cmd::Focus { minutes: 120 }));
        assert_eq!(p("focus"), Some(Cmd::Focus { minutes: 25 }));
        assert_eq!(p("focus 0"), None);
    }

    #[test]
    fn a_time_of_day_after_at_is_a_clock_not_minutes() {
        // p() runs at 12:00
        assert_eq!(p("remind at 9pm dentist"), Some(Cmd::Remind { secs: 9 * 3600, text: "dentist".into() }));
        assert_eq!(p("remind me at 14 call"), Some(Cmd::Remind { secs: 2 * 3600, text: "call".into() }));
        assert_eq!(p("ricordami alle 9 palestra"), Some(Cmd::Remind { secs: 21 * 3600, text: "palestra".into() }), "9 already past: tomorrow");
        assert_eq!(p("remind at 12:30am x"), Some(Cmd::Remind { secs: 12 * 3600 + 1800, text: "x".into() }));
        assert_eq!(p("remind tomorrow 8:30 gym"), Some(Cmd::Remind { secs: 12 * 3600 + 8 * 3600 + 1800, text: "gym".into() }));
        assert_eq!(p("remind domani 9 spesa"), Some(Cmd::Remind { secs: 12 * 3600 + 9 * 3600, text: "spesa".into() }));
        assert_eq!(p("remind 9 tea"), Some(Cmd::Remind { secs: 540, text: "tea".into() }), "without at, a bare number is still minutes");
        assert_eq!(p("remind at 25 x"), None);
    }

    #[test]
    fn a_timer_can_have_a_name() {
        assert_eq!(p("10m Tea"), Some(Cmd::Timer { secs: 600, label: "Tea".into() }));
        assert_eq!(p("timer 25 pasta al forno"), Some(Cmd::Timer { secs: 1500, label: "pasta al forno".into() }));
        assert_eq!(p("1h30m"), Some(Cmd::Timer { secs: 5400, label: String::new() }));
        assert_eq!(preview(&Cmd::Timer { secs: 600, label: "tea".into() }), "Start a timer · 10:00 · tea");
    }

    #[test]
    fn a_day_of_the_week_is_the_next_one() {
        // NOON is 12:00; weekday 2 is Wednesday
        let on = |l: &str| parse_on(l, NOON, Some(2));
        assert_eq!(on("remind friday 9:30 dentist"), Some(Cmd::Remind { secs: 2 * 86_400 - NOON + 9 * 3600 + 1800, text: "dentist".into() }));
        assert_eq!(on("ricordami venerdì alle 18 cena"), Some(Cmd::Remind { secs: 2 * 86_400 - NOON + 18 * 3600, text: "cena".into() }));
        assert_eq!(on("remind monday call"), Some(Cmd::Remind { secs: 5 * 86_400 - NOON + 9 * 3600, text: "call".into() }), "no time: nine o'clock");
        assert_eq!(on("remind wednesday 9 standup"), Some(Cmd::Remind { secs: 7 * 86_400 - NOON + 9 * 3600, text: "standup".into() }), "today at 9 is past: next week");
        assert_eq!(on("remind wednesday 18 standup"), Some(Cmd::Remind { secs: 6 * 3600, text: "standup".into() }), "today at 18 is still ahead");
        assert_eq!(p("remind friday 9 x"), None, "without knowing the weekday it is not understood");
    }

    #[test]
    fn the_bar_generates_computes_dates_and_converts_money() {
        let rates = vec![("USD".to_string(), 1.08)];
        let ctx = Ctx { local_secs: NOON, weekday: Some(4), today: Some((2027, 1, 15)), rates: &rates };
        let q = |l: &str| parse_ctx(l, &ctx);
        assert_eq!(q("pw 24"), Some(Cmd::Gen(crate::quick::Gen::Password(24))));
        assert_eq!(q("d20"), Some(Cmd::Gen(crate::quick::Gen::Dice { n: 1, sides: 20 })));
        assert!(matches!(q("days until 3 feb"), Some(Cmd::Value(v)) if v.ends_with("in 19 days (2 weeks 5 days)")));
        assert!(matches!(q("today + 2 weeks"), Some(Cmd::Value(v)) if v.ends_with("= Fri 29 Jan 2027")));
        assert_eq!(q("20 usd in eur"), Some(Cmd::Value("20 USD = 18.519 EUR".into())));
        assert_eq!(q("5 km in mi").map(|c| matches!(c, Cmd::Value(_))), Some(true), "units still work");
        assert!(matches!(q("2m"), Some(Cmd::Timer { secs: 120, .. })), "a duration is still a timer");
        assert!(matches!(q("10m tea"), Some(Cmd::Timer { .. })));
        assert!(matches!(q("12*7"), Some(Cmd::Value(_))));
        assert_eq!(parse("pw", NOON), Some(Cmd::Gen(crate::quick::Gen::Password(20))), "no date needed for those");
        assert_eq!(parse("days until 3 feb", NOON), None, "without today a date means nothing");
        assert_eq!(preview(&Cmd::Gen(crate::quick::Gen::Coin)), "Flip a coin");
    }

    #[test]
    fn a_half_written_command_gets_its_usage_and_the_empty_bar_its_examples() {
        assert!(usage("remind").unwrap().contains("at 9pm"));
        assert!(usage("Timer banana").unwrap().starts_with("timer 25m"));
        assert_eq!(usage("hello world"), None);
        assert_eq!(usage(""), None);
        assert!(HINTS.iter().all(|h| h.chars().count() <= 60), "one line of the bar");
        // every example is something the parser understands (the first words of each, at least)
        for ex in ["10m tea", "remind at 9pm call mom", "todo buy milk", "pw 24", "roll 2d6", "uuid", "coin", "random 1-100", "5 km in mi", "20% of 150", "12*7", "recap", "agenda", "pomodoro", "focus 45", "dnd on"] {
            assert!(parse(ex, NOON).is_some(), "{ex}");
        }
    }

    #[test]
    fn tomorrow_alone_shows_tomorrow_and_with_more_words_is_something_else() {
        assert_eq!(p("tomorrow"), Some(Cmd::Tomorrow));
        assert_eq!(p("domani"), Some(Cmd::Tomorrow));
        assert_eq!(p("recap"), Some(Cmd::Tomorrow));
        assert!(matches!(p("remind domani 9 spesa"), Some(Cmd::Remind { .. })), "more words: a reminder");
        assert_eq!(preview(&Cmd::Tomorrow), "Show tomorrow");
        assert_eq!((p("today"), p("oggi"), p("agenda")), (Some(Cmd::Today), Some(Cmd::Today), Some(Cmd::Today)));
        assert_eq!(preview(&Cmd::Today), "Show today");
    }

    #[test]
    fn the_bar_pins_a_train_and_sets_home() {
        assert_eq!(p("treno pisa centrale 17:12 verso lucca"), Some(Cmd::Train("pisa centrale 17:12 verso lucca".into())));
        assert_eq!(p("train from Pisa Centrale 7:05"), Some(Cmd::Train("from Pisa Centrale 7:05".into())));
        assert_eq!(p("train pisa centrale"), None, "a train needs its time");
        assert_eq!(p("train off"), Some(Cmd::TrainOff));
        assert_eq!(p("treno basta"), Some(Cmd::TrainOff));
        assert_eq!(p("home 43.7085, 10.4036"), Some(Cmd::Home("43.7085, 10.4036".into())));
        assert_eq!(p("casa Via Roma 1 Pisa"), Some(Cmd::Home("Via Roma 1 Pisa".into())));
        assert_eq!(p("home"), None, "home alone is not a command");
        assert_eq!(preview(&Cmd::TrainOff), "Unpin the train");
    }

    #[test]
    fn until_the_next_class() {
        assert_eq!((p("until class"), p("until next"), p("fino alla lezione")), (Some(Cmd::UntilNext), Some(Cmd::UntilNext), Some(Cmd::UntilNext)));
        assert_eq!(p("until banana"), None);
        assert_eq!(preview(&Cmd::UntilNext), "Timer until 5 min before your next event");
    }

    #[test]
    fn a_deadline_is_a_date_and_a_name() {
        let ctx = Ctx { local_secs: NOON, weekday: Some(4), today: Some((2027, 1, 15)), rates: &[] };
        let d = |l: &str| parse_ctx(l, &ctx);
        let feb3 = crate::calendar::days_from_civil(2027, 2, 3);
        assert_eq!(d("deadline 3 feb Reti exam"), Some(Cmd::Deadline { day: feb3, text: "Reti exam".into(), course: String::new(), est_min: 0, cfu: 0 }));
        assert_eq!(d("esame 2027-02-03 Algoritmi e Strutture"), Some(Cmd::Deadline { day: feb3, text: "Algoritmi e Strutture".into(), course: String::new(), est_min: 0, cfu: 0 }));
        assert_eq!(d("scadenza 3/2 consegna progetto"), Some(Cmd::Deadline { day: feb3, text: "consegna progetto".into(), course: String::new(), est_min: 0, cfu: 0 }));
        assert_eq!(d("exam 3 feb"), Some(Cmd::Deadline { day: feb3, text: "Exam".into(), course: String::new(), est_min: 0, cfu: 0 }));
        assert_eq!(d("deadline 3 feb"), None, "a deadline needs a name");
        assert_eq!(d("deadline banana x"), None);
        assert_eq!(parse("deadline 3 feb x", NOON), None, "no date known: no deadline");
        assert_eq!(preview(&Cmd::Deadline { day: feb3, text: "x".into(), course: String::new(), est_min: 0, cfu: 0 }), "Deadline · Wed 3 Feb 2027 · x");
        assert_eq!(d("esame 3 feb Algoritmi #algoritmi ~20h"), Some(Cmd::Deadline { day: feb3, text: "Algoritmi".into(), course: "algoritmi".into(), est_min: 1200, cfu: 0 }), "the course and the work to do ride along");
        assert_eq!(preview(&d("esame 3 feb Algoritmi ~20h").unwrap()), "Deadline · Wed 3 Feb 2027 · Algoritmi · 20h to prepare");
        let nine = Cmd::Deadline { day: feb3, text: "AE".into(), course: String::new(), est_min: 0, cfu: 9 };
        assert_eq!(d("exam 3 feb AE 9cfu").as_ref(), Some(&nine));
        assert_eq!(d("esame 3 feb AE 9 CFU").as_ref(), Some(&nine));
        assert_eq!(d("exam 3 feb Analisi 2"), Some(Cmd::Deadline { day: feb3, text: "Analisi 2".into(), course: String::new(), est_min: 0, cfu: 0 }), "a number alone is part of the name");
        assert_eq!(preview(&nine), "Deadline · Wed 3 Feb 2027 · AE · 9 CFU");
    }

    #[test]
    fn a_pin_keeps_its_text_and_unpin_names_what_to_take_off() {
        assert_eq!(p("pin ssh 10.0.0.5 -p 2222"), Some(Cmd::Pin("ssh 10.0.0.5 -p 2222".into())));
        assert_eq!(p("pin Aula B3"), Some(Cmd::Pin("Aula B3".into())));
        assert_eq!(p("pin"), None);
        assert_eq!(p("unpin"), Some(Cmd::Unpin(String::new())));
        assert_eq!(p("unpin ssh"), Some(Cmd::Unpin("ssh".into())));
        assert_eq!(p("unpin all"), Some(Cmd::Unpin("all".into())));
        assert_eq!(preview(&Cmd::Pin("x".into())), "Pin to Today · x");
    }

    fn todo(text: &str) -> crate::task::Spec {
        crate::task::Spec { text: text.into(), ..Default::default() }
    }

    #[test]
    fn a_todo_keeps_its_capitals() {
        assert_eq!(p("todo Buy milk"), Some(Cmd::Todo(todo("Buy milk"))));
        assert_eq!(p("compito  chiamare   Anna"), Some(Cmd::Todo(todo("chiamare Anna"))));
        assert_eq!(p("todo"), None);
        assert_eq!(p("todo daily"), None, "only a repetition is no task");
        assert_eq!(preview(&Cmd::Todo(todo("x"))), "Add to your list · x");
    }

    #[test]
    fn a_todo_carries_its_day_course_estimate_and_repetition() {
        use crate::task::Repeat;
        // Sunday 22 November 2026
        let ctx = Ctx { local_secs: NOON, weekday: Some(6), today: Some((2026, 11, 22)), rates: &[] };
        let t = |l: &str| match parse_ctx(l, &ctx) {
            Some(Cmd::Todo(s)) => s,
            other => panic!("{l}: {other:?}"),
        };
        let fri = crate::calendar::days_from_civil(2026, 11, 27);
        let s = t("todo Report due fri #algoritmi ~2h");
        assert_eq!((s.text.as_str(), s.due, s.course.as_str(), s.est_min), ("Report", Some(fri), "algoritmi", 120));
        let daily = t("todo daily Bring the charger");
        assert_eq!((daily.repeat, daily.text.as_str()), (Repeat::Daily, "Bring the charger"), "the old daily form still works");
        assert_eq!(t("todo ogni giorno portare il caricatore").repeat, Repeat::Daily);
        assert_eq!(t("todo every day lunch").text, "lunch");
        assert_eq!(t("todo gym every mon and thu").repeat, Repeat::Weekly(0b0001001));
        assert_eq!(preview(&parse_ctx("todo Report due fri ~2h", &ctx).unwrap()), "Add to your list · Report · Fri 27 Nov 2026 · 2h");
    }

    #[test]
    fn a_reminder_can_come_back() {
        use crate::task::Repeat;
        let r = |l: &str| p(l);
        assert_eq!(r("remind every monday 9 gym"), Some(Cmd::RemindEvery { repeat: Repeat::Weekly(1), at_secs: 9 * 3600, text: "gym".into() }));
        assert_eq!(r("ricordami ogni giorno alle 8:30 prendere le vitamine"), Some(Cmd::RemindEvery { repeat: Repeat::Daily, at_secs: 8 * 3600 + 1800, text: "prendere le vitamine".into() }));
        assert_eq!(r("remind every month on the 27th pay rent"), Some(Cmd::RemindEvery { repeat: Repeat::Monthly(27), at_secs: 9 * 3600, text: "pay rent".into() }), "nine o'clock when no time is given");
        assert_eq!(r("remind every tue and thu at 18 lab"), Some(Cmd::RemindEvery { repeat: Repeat::Weekly(0b0001010), at_secs: 18 * 3600, text: "lab".into() }));
        assert!(matches!(r("remind 20m call mom"), Some(Cmd::Remind { .. })), "one-off reminders are unchanged");
        assert_eq!(r("remind 10m before lab"), Some(Cmd::RemindBefore { secs: 600, query: "lab".into() }));
        assert_eq!(r("ricordami 30 minuti prima della lezione di reti"), Some(Cmd::RemindBefore { secs: 1800, query: "lezione di reti".into() }));
        assert_eq!(r("remind 10m before"), None, "an event is needed");
        assert_eq!(preview(&r("remind every monday 9 gym").unwrap()), "Remind every Mon at 09:00 · gym");
    }

    #[test]
    fn an_event_has_a_day_a_time_and_a_length() {
        // Sunday 22 November 2026
        let ctx = Ctx { local_secs: NOON, weekday: Some(6), today: Some((2026, 11, 22)), rates: &[] };
        let e = |l: &str| parse_ctx(l, &ctx);
        let day = |d: i64| crate::calendar::days_from_civil(2026, 11, d);
        assert_eq!(e("event fri 15-16:30 Office hours"), Some(Cmd::Event { day: day(27), start_min: 900, dur_min: 90, title: "Office hours".into() }));
        assert_eq!(e("evento domani 10 dentista"), Some(Cmd::Event { day: day(23), start_min: 600, dur_min: 60, title: "dentista".into() }), "an hour when no end is given");
        assert_eq!(e("event 3 feb 14:00 for 2h exam prep"), Some(Cmd::Event { day: crate::calendar::days_from_civil(2027, 2, 3), start_min: 840, dur_min: 120, title: "exam prep".into() }));
        assert_eq!(e("event fri all day conference"), Some(Cmd::Event { day: day(27), start_min: 0, dur_min: 1440, title: "conference".into() }));
        assert_eq!(e("evento domani tutto il giorno gita"), Some(Cmd::Event { day: day(23), start_min: 0, dur_min: 1440, title: "gita".into() }));
        let m = |l: &str| parse_ctx(l, &ctx);
        assert_eq!(m("move report to fri"), Some(Cmd::Move { query: "report".into(), day: Some(day(27)), start_min: None, dur_min: None }));
        assert_eq!(m("sposta office hours a domani 10-11:30"), Some(Cmd::Move { query: "office hours".into(), day: Some(day(23)), start_min: Some(600), dur_min: Some(90) }));
        assert_eq!(m("move standup to 10"), Some(Cmd::Move { query: "standup".into(), day: None, start_min: Some(600), dur_min: None }));
        assert_eq!(m("move report"), None);
        assert_eq!(m("cancel dentist"), Some(Cmd::CancelEvent("dentist".into())));
        assert_eq!(m("rm buy milk"), Some(Cmd::DropTask("buy milk".into())));
        let ed = m("edit report -> due mon ~3h").unwrap();
        assert!(matches!(ed, Cmd::EditTask { ref query, ref spec } if query == "report" && spec.est_min == 180 && spec.due.is_some()));
        assert_eq!(m("edit report -> "), None);
        assert_eq!(e("event 9:30 standup"), Some(Cmd::Event { day: day(22), start_min: 570, dur_min: 60, title: "standup".into() }), "today when no day is given");
        assert_eq!(e("event fri 15 "), None, "a title is needed");
        assert_eq!(e("event fri 16-15 backwards"), None, "an end before the start is no event");
        assert_eq!(e("event fri lunch"), None, "a time is needed");
        assert_eq!(parse("event fri 15 x", NOON), None, "no date known: no event");
        assert_eq!(preview(&e("event fri 15-16:30 Office hours").unwrap()), "Event · Fri 27 Nov 2026 15:00 · 1h30 · Office hours");
    }

    #[test]
    fn notes_searches_and_study_time() {
        assert_eq!(p("note ask about the exam format"), Some(Cmd::Note("ask about the exam format".into())));
        assert_eq!(p("nota Chiedere del Progetto"), Some(Cmd::Note("Chiedere del Progetto".into())));
        assert_eq!(p("note"), None);
        assert_eq!(p("find compilers"), Some(Cmd::Find("compilers".into())));
        assert_eq!(p("cerca reti di calcolatori"), Some(Cmd::Find("reti di calcolatori".into())));
        assert_eq!(p("log 90m algoritmi"), Some(Cmd::Study { minutes: 90, course: "algoritmi".into() }));
        assert_eq!(p("studiato 1h30"), Some(Cmd::Study { minutes: 90, course: String::new() }));
        assert_eq!(p("log banana"), None);
        assert_eq!(p("pomodoro"), Some(Cmd::Pomodoro));
        assert_eq!(p("pomodoro Algoritmi"), Some(Cmd::PomodoroFor("Algoritmi".into())));
        assert_eq!(preview(&p("log 90m algoritmi").unwrap()), "Studied 1h30 · algoritmi");
    }

    #[test]
    fn shares_rates_and_bills() {
        assert_eq!(p("20% of 150"), Some(Cmd::Value("30".into())));
        assert_eq!(p("15 % of 80"), Some(Cmd::Value("12".into())));
        assert_eq!(p("80 + 15%"), Some(Cmd::Value("92".into())));
        assert_eq!(p("80 - 20%"), Some(Cmd::Value("64".into())));
        assert_eq!(p("split 84.5 by 3"), Some(Cmd::Value("28.17 each".into())));
        assert_eq!(p("dividi 90 3"), Some(Cmd::Value("30 each".into())));
        assert_eq!(p("split 90 0"), None);
        assert_eq!(p("10 % 3"), Some(Cmd::Value("1".into())), "modulo still works");
    }

    #[test]
    fn reminders_in_english_and_italian_keep_their_capitals() {
        assert_eq!(p("remind 20m Call Mom"), Some(Cmd::Remind { secs: 1200, text: "Call Mom".into() }));
        assert_eq!(p("remind me in 20 minutes to call mom"), Some(Cmd::Remind { secs: 1200, text: "call mom".into() }));
        assert_eq!(p("in 5m stand up"), Some(Cmd::Remind { secs: 300, text: "stand up".into() }));
        assert_eq!(p("ricordami tra 2 ore di chiamare Anna"), Some(Cmd::Remind { secs: 7200, text: "chiamare Anna".into() }));
        assert_eq!(p("tra 10m pasta"), Some(Cmd::Remind { secs: 600, text: "pasta".into() }));
        assert_eq!(p("remind 15:30 meeting prep"), Some(Cmd::Remind { secs: 3 * 3600 + 1800, text: "meeting prep".into() }), "a clock time ahead today");
        assert_eq!(p("remind 09:00 gym"), Some(Cmd::Remind { secs: 21 * 3600, text: "gym".into() }), "a clock time already past means tomorrow");
        assert_eq!(p("remind 20m"), Some(Cmd::Remind { secs: 1200, text: "Reminder".into() }));
        assert_eq!(p("remind soon"), None);
    }

    #[test]
    fn sums() {
        let v = |s: &str| match p(s) {
            Some(Cmd::Value(v)) => v,
            other => panic!("{s}: {other:?}"),
        };
        assert_eq!(v("12*7+3"), "87");
        assert_eq!(v("= 2^10"), "1024");
        assert_eq!(v("2^3^2"), "512", "powers go right to left");
        assert_eq!(v("(1+2)*3"), "9");
        assert_eq!(v("-4 + 10"), "6");
        assert_eq!(v("10/4"), "2.5");
        assert_eq!(v("1/3"), "0.3333333333");
        assert_eq!(v("sqrt(2)"), "1.414213562");
        assert_eq!(v("3,5*2"), "7", "a decimal comma");
        assert_eq!(v("10 % 3"), "1");
        assert_eq!(v("pi*2"), "6.283185307");
    }

    #[test]
    fn not_a_sum() {
        assert_eq!(p("42"), None, "a lone number says nothing");
        assert_eq!(p("hello world"), None);
        assert_eq!(p("1/0"), None, "infinity");
        assert_eq!(p("2+"), None);
        assert_eq!(p("(1+2"), None);
        assert_eq!(p("sqrt 4"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn conversions() {
        let v = |s: &str| match p(s) {
            Some(Cmd::Value(v)) => v,
            other => panic!("{s}: {other:?}"),
        };
        assert_eq!(v("5 km in mi"), "5 km = 3.106855961 mi");
        assert_eq!(v("70 f to c"), "70 f = 21.11111111 c");
        assert_eq!(v("0 c in k"), "0 c = 273.15 k");
        assert_eq!(v("1 gib to mb"), "1 gib = 1073.741824 mb");
        assert_eq!(v("3 kg in lb"), "3 kg = 6.613867866 lb");
        assert_eq!(v("100 km/h in mph"), "100 km/h = 62.13711922 mph");
        assert_eq!(v("5 in in cm"), "5 in = 12.7 cm");
        assert_eq!(v("2 l in ml"), "2 l = 2000 ml");
        assert_eq!(p("5 km in kg"), None, "different kinds of unit");
        assert_eq!(p("5 foo in bar"), None);
    }

    #[test]
    fn a_conversion_is_not_mistaken_for_a_timer() {
        assert_eq!(p("5 m in cm"), Some(Cmd::Value("5 m = 500 cm".into())), "m is metres here, minutes only without `in`");
    }

    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1e20), "1.000000e20");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(123456.789), "123456.789");
        assert_eq!(format_number(1234567890123.0), "1234567890123");
    }

    #[test]
    fn previews_say_what_enter_will_do() {
        assert_eq!(preview(&Cmd::Timer { secs: 600, label: String::new() }), "Start a timer · 10:00");
        assert_eq!(preview(&Cmd::Remind { secs: 1200, text: "x".into() }), "Remind in 20 min · x");
        assert_eq!(preview(&Cmd::Remind { secs: 7500, text: "y".into() }), "Remind in 2 h 5 min · y");
        assert_eq!(preview(&Cmd::Dnd(false)), "Do not disturb off");
    }

    #[test]
    fn go_opens_a_page_by_name() {
        use crate::face::Page;
        assert_eq!(p("go phone"), Some(Cmd::Go(Page::Phone)));
        assert_eq!(p("vai meteo"), Some(Cmd::Go(Page::Weather)));
        assert_eq!(p("go all"), Some(Cmd::Go(Page::Grid)));
        assert_eq!(p("go"), None);
        assert_eq!(p("go nowhere"), None);
        assert_eq!(preview(&Cmd::Go(Page::Phone)), "Go to iPhone");
    }

    #[test]
    fn ocr_reads_the_clipboard_image() {
        assert_eq!(p("ocr"), Some(Cmd::Ocr));
        assert_eq!(p("leggi testo"), Some(Cmd::Ocr));
        assert_eq!(p("ocr something"), None);
        assert!(preview(&Cmd::Ocr).contains("clipboard"));
    }

    #[test]
    fn bring_names_a_thing_and_maybe_a_course() {
        assert_eq!(p("bring laptop and charger for Reti di calcolatori"), Some(Cmd::Bring { text: "laptop and charger".into(), course: "Reti di calcolatori".into() }));
        assert_eq!(p("porta il libro per analisi"), Some(Cmd::Bring { text: "il libro".into(), course: "analisi".into() }));
        assert_eq!(p("bring umbrella"), Some(Cmd::Bring { text: "umbrella".into(), course: String::new() }));
        assert_eq!(p("bring for reti"), Some(Cmd::Bring { text: "for reti".into(), course: String::new() }), "nothing before for: all of it is the thing");
        assert_eq!(p("bring"), None);
        assert_eq!(p("unbring"), Some(Cmd::Unbring(String::new())));
        assert_eq!(p("unbring all"), Some(Cmd::Unbring("all".into())));
        assert_eq!(preview(&Cmd::Bring { text: "x".into(), course: "reti".into() }), "Bring · x · for reti");
    }
}
