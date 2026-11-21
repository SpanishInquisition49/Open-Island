//! Small utilities for the command bar: things to generate (a password, dice), date arithmetic (`days until 3 feb`,
//! `today + 6 weeks`, `week`) and currency conversion with the ECB's daily rates. Pure: the caller brings the date, the
//! rates and the randomness.

use crate::calendar::{civil_from_days, days_from_civil, days_in_month, weekday_of_days};

// ---- things to generate ----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gen {
    Uuid,
    Password(usize),
    Dice { n: u32, sides: u32 },
    Coin,
    Range(i64, i64),
}

/// `uuid`, `pw 24`, `roll 2d6`, `d20`, `coin`, `random 1-100`: what the words ask to generate.
pub fn parse_gen(words: &[&str]) -> Option<Gen> {
    let first = *words.first()?;
    let arg = words.get(1).copied();
    match first {
        "uuid" | "guid" if words.len() == 1 => Some(Gen::Uuid),
        "pw" | "pass" | "password" | "psw" => {
            let n = match arg {
                Some(a) => a.parse::<usize>().ok()?,
                None => 20,
            };
            (words.len() <= 2).then_some(Gen::Password(n.clamp(8, 128)))
        }
        "coin" | "flip" | "moneta" if words.len() == 1 => Some(Gen::Coin),
        "roll" | "dice" | "dado" | "dadi" => {
            let (n, sides) = match arg {
                Some(a) => dice_spec(a)?,
                None => (1, 6),
            };
            (words.len() <= 2).then_some(Gen::Dice { n, sides })
        }
        w if words.len() == 1 && w.contains('d') => dice_spec(w).map(|(n, sides)| Gen::Dice { n, sides }),
        "random" | "rand" | "casuale" | "numero" => {
            let rest: Vec<&str> = words[1..].iter().flat_map(|w| w.split(['-', '–', ',', ':'])).filter(|p| !p.is_empty()).collect();
            let nums: Vec<i64> = rest.iter().map(|p| p.parse().ok()).collect::<Option<_>>()?;
            match nums.as_slice() {
                [hi] if *hi >= 1 => Some(Gen::Range(1, *hi)),
                [lo, hi] if lo < hi => Some(Gen::Range(*lo, *hi)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `2d6`, `d20`, `6` (one die of that many sides).
fn dice_spec(a: &str) -> Option<(u32, u32)> {
    let (n, sides) = match a.split_once('d') {
        Some((n, s)) => (if n.is_empty() { 1 } else { n.parse().ok()? }, s.parse().ok()?),
        None => (1, a.parse().ok()?),
    };
    ((1..=20).contains(&n) && (2..=1000).contains(&sides)).then_some((n, sides))
}

pub fn preview_gen(g: &Gen) -> String {
    match g {
        Gen::Uuid => "Generate a UUID (Enter copies)".into(),
        Gen::Password(n) => format!("Generate a password · {n} characters (Enter copies)"),
        Gen::Dice { n, sides } => format!("Roll {}d{sides}", if *n == 1 { String::new() } else { n.to_string() }),
        Gen::Coin => "Flip a coin".into(),
        Gen::Range(lo, hi) => format!("A random number from {lo} to {hi}"),
    }
}

/// The result, written like the sums are: "text = value" (what follows ` = ` is what gets copied).
pub fn generate(g: &Gen, rng: &mut dyn FnMut() -> u64) -> String {
    let mut below = |n: u64| rng() % n.max(1);
    match g {
        Gen::Uuid => {
            let (a, b) = (below(u64::MAX), below(u64::MAX));
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&a.to_le_bytes());
            bytes[8..].copy_from_slice(&b.to_le_bytes());
            bytes[6] = bytes[6] & 0x0f | 0x40; // version 4
            bytes[8] = bytes[8] & 0x3f | 0x80; // RFC 4122 variant
            let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("UUID = {}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
        }
        Gen::Password(n) => {
            const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
            const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
            const DIGIT: &[u8] = b"23456789";
            const SYMBOL: &[u8] = b"!@#$%&*-_+=?";
            let all: Vec<u8> = [LOWER, UPPER, DIGIT, SYMBOL].concat();
            // at least one of each kind, whatever the length is: a policy that asks for them is met
            let mut pw: Vec<u8> = [LOWER, UPPER, DIGIT, SYMBOL].iter().map(|set| set[below(set.len() as u64) as usize]).collect();
            while pw.len() < *n {
                pw.push(all[below(all.len() as u64) as usize]);
            }
            // a shuffle (Fisher-Yates), so that the first four are not always one of each kind
            for i in (1..pw.len()).rev() {
                pw.swap(i, below(i as u64 + 1) as usize);
            }
            format!("Password = {}", String::from_utf8_lossy(&pw[..*n]))
        }
        Gen::Dice { n, sides } => {
            let rolls: Vec<u64> = (0..*n).map(|_| below(*sides as u64) + 1).collect();
            let label = format!("{}d{sides}", if *n == 1 { String::new() } else { n.to_string() });
            if *n == 1 {
                format!("{label} = {}", rolls[0])
            } else {
                format!("{label}: {} = {}", rolls.iter().map(u64::to_string).collect::<Vec<_>>().join(" + "), rolls.iter().sum::<u64>())
            }
        }
        Gen::Coin => format!("Coin = {}", if below(2) == 0 { "Heads" } else { "Tails" }),
        Gen::Range(lo, hi) => format!("{lo}–{hi} = {}", lo + below((hi - lo + 1) as u64) as i64),
    }
}

// ---- dates -------------------------------------------------------------------------------------------------

const MONTHS: [&[&str]; 12] = [
    &["jan", "january", "gen", "gennaio"],
    &["feb", "february", "febbraio"],
    &["mar", "march", "marzo"],
    &["apr", "april", "aprile"],
    &["may", "mag", "maggio"],
    &["jun", "june", "giu", "giugno"],
    &["jul", "july", "lug", "luglio"],
    &["aug", "august", "ago", "agosto"],
    &["sep", "sept", "september", "set", "settembre"],
    &["oct", "october", "ott", "ottobre"],
    &["nov", "november", "novembre"],
    &["dec", "december", "dic", "dicembre"],
];

const MONTH_ABBR: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAY_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn month_of(w: &str) -> Option<i64> {
    MONTHS.iter().position(|m| m.contains(&w)).map(|i| i as i64 + 1)
}

pub type Ymd = (i64, i64, i64);

fn valid(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && (1900..=2200).contains(&y) && d >= 1 && d <= days_in_month(y as i32, m as u32) as i64
}

/// The next time a day and month come round (this year when still ahead or today, else next year).
fn next_occurrence(today: Ymd, m: i64, d: i64) -> Option<Ymd> {
    let year = if (m, d) >= (today.1, today.2) { today.0 } else { today.0 + 1 };
    valid(year, m, d).then_some((year, m, d))
}

/// A date at the start of `words`, and how many words it took: `today`, `tomorrow`, `2027-02-03`, `3/2`, `3/2/2027`,
/// `3 feb`, `feb 3`, `3 feb 2027`. A day and month without a year is the next one to come.
pub fn parse_date(words: &[&str], today: Ymd) -> Option<(Ymd, usize)> {
    let w = *words.first()?;
    let day = days_from_civil(today.0, today.1, today.2);
    let from_days = |n: i64| {
        let (y, m, d) = civil_from_days(n);
        (y, m, d)
    };
    match w {
        "today" | "oggi" => return Some((today, 1)),
        "tomorrow" | "domani" => return Some((from_days(day + 1), 1)),
        "yesterday" | "ieri" => return Some((from_days(day - 1), 1)),
        _ => {}
    }
    // 2027-02-03
    let iso: Vec<&str> = w.split('-').collect();
    if iso.len() == 3 && iso[0].len() == 4 {
        let (y, m, d) = (iso[0].parse().ok()?, iso[1].parse().ok()?, iso[2].parse().ok()?);
        return valid(y, m, d).then_some(((y, m, d), 1));
    }
    // 3/2 or 3/2/2027 (day first)
    if w.contains('/') {
        let p: Vec<&str> = w.split('/').collect();
        let (d, m): (i64, i64) = (p.first()?.parse().ok()?, p.get(1)?.parse().ok()?);
        return match p.get(2) {
            Some(y) => {
                let y: i64 = y.parse().ok()?;
                let y = if y < 100 { 2000 + y } else { y };
                valid(y, m, d).then_some(((y, m, d), 1))
            }
            None => next_occurrence(today, m, d).map(|x| (x, 1)),
        };
    }
    let year_at = |i: usize| words.get(i).and_then(|y| y.parse::<i64>().ok()).filter(|y| (1900..=2200).contains(y));
    // 3 feb [2027]
    if let (Ok(d), Some(m)) = (w.trim_end_matches(['.', 'º', '°']).parse::<i64>(), words.get(1).and_then(|m| month_of(m))) {
        return match year_at(2) {
            Some(y) => valid(y, m, d).then_some(((y, m, d), 3)),
            None => next_occurrence(today, m, d).map(|x| (x, 2)),
        };
    }
    // feb 3 [2027]
    if let (Some(m), Some(d)) = (month_of(w), words.get(1).and_then(|d| d.trim_end_matches(['.', ',']).parse::<i64>().ok())) {
        return match year_at(2) {
            Some(y) => valid(y, m, d).then_some(((y, m, d), 3)),
            None => next_occurrence(today, m, d).map(|x| (x, 2)),
        };
    }
    None
}

/// "Wed 3 Feb 2027".
pub fn fmt_date((y, m, d): Ymd) -> String {
    format!("{} {d} {} {y}", DAY_ABBR[weekday_of_days(days_from_civil(y, m, d)) as usize], MONTH_ABBR[(m - 1) as usize])
}

fn add_months((y, m, d): Ymd, n: i64) -> Ymd {
    let idx = y * 12 + (m - 1) + n;
    let (ny, nm) = (idx.div_euclid(12), idx.rem_euclid(12) + 1);
    (ny, nm, d.min(days_in_month(ny as i32, nm as u32) as i64))
}

/// "47 days", "1 day", "6 weeks 5 days": a count of days in words.
fn days_words(n: i64) -> String {
    let n = n.abs();
    let plural = |k: i64, one: &str| if k == 1 { format!("{k} {one}") } else { format!("{k} {one}s") };
    if n >= 14 {
        let (w, d) = (n / 7, n % 7);
        if d == 0 { format!("{} ({})", plural(n, "day"), plural(w, "week")) } else { format!("{} ({} {})", plural(n, "day"), plural(w, "week"), plural(d, "day")) }
    } else {
        plural(n, "day")
    }
}

/// ISO 8601 week number and its year.
pub fn iso_week(today: Ymd) -> (i64, i64) {
    let days = days_from_civil(today.0, today.1, today.2);
    // the Thursday of this week decides the year
    let thursday = days - weekday_of_days(days) + 3;
    let (y, _, _) = civil_from_days(thursday);
    let jan1 = days_from_civil(y, 1, 1);
    (y, (thursday - jan1) / 7 + 1)
}

/// `days until 3 feb`, `giorni a natale`-style phrases, `15 jan + 45 days`, `today - 2 weeks`, `week`: the answer as
/// "something = value" (the value is what gets copied), or None when the line is not about dates.
pub fn date_calc(lower: &str, today: Ymd) -> Option<String> {
    let words: Vec<&str> = lower.split_whitespace().collect();
    let first = *words.first()?;
    if words.len() == 1 && matches!(first, "week" | "weeknumber" | "settimana") {
        let (y, w) = iso_week(today);
        return Some(format!("Week = {w} of {y}"));
    }
    let day_of = |t: Ymd| days_from_civil(t.0, t.1, t.2);
    // days until <date> / days since <date>
    if matches!(first, "days" | "day" | "giorni" | "giorno") {
        let (kind, rest) = match words.get(1).copied()? {
            "until" | "to" | "till" | "a" | "al" | "fino" | "mancano" => (1, &words[2..]),
            "since" | "from" | "da" | "dal" => (-1, &words[2..]),
            _ => return None,
        };
        let rest = if matches!(rest.first(), Some(&("a" | "al" | "il" | "to"))) { &rest[1..] } else { rest };
        let (date, used) = parse_date(rest, today)?;
        if used != rest.len() {
            return None;
        }
        let diff = day_of(date) - day_of(today);
        let value = match (diff, kind) {
            (0, _) => "today".to_string(),
            (d, 1) if d > 0 => format!("in {}", days_words(d)),
            (d, 1) => format!("{} ago", days_words(d)),
            (d, _) if d < 0 => format!("{} ago", days_words(d)),
            (d, _) => format!("in {}", days_words(d)),
        };
        return Some(format!("{} = {value}", fmt_date(date)));
    }
    // <date> (+|-) N unit
    let (date, used) = parse_date(&words, today)?;
    let rest = &words[used..];
    if rest.is_empty() {
        // a bare date is shown only when it is written with a month name or as an ISO date: "25/12" is a division too
        let written = words[..used].iter().any(|w| month_of(w.trim_end_matches(['.', ','])).is_some() || w.len() == 10 && w.matches('-').count() == 2);
        if !written {
            return None;
        }
        let diff = day_of(date) - day_of(today);
        let value = match diff {
            0 => "today".to_string(),
            d if d > 0 => format!("in {}", days_words(d)),
            d => format!("{} ago", days_words(d)),
        };
        return Some(format!("{} = {value}", fmt_date(date)));
    }
    let (sign, n_word, unit) = match rest {
        [s, n, u] if matches!(*s, "+" | "-") => (if *s == "+" { 1 } else { -1 }, *n, *u),
        [sn, u] if sn.starts_with(['+', '-']) => (if sn.starts_with('+') { 1 } else { -1 }, &sn[1..], *u),
        _ => return None,
    };
    let n: i64 = n_word.parse().ok().filter(|n| (0..=100_000).contains(n))?;
    let result = match unit {
        "d" | "day" | "days" | "g" | "giorno" | "giorni" => {
            let (y, m, d) = civil_from_days(day_of(date) + sign * n);
            (y, m, d)
        }
        "w" | "week" | "weeks" | "settimana" | "settimane" => {
            let (y, m, d) = civil_from_days(day_of(date) + sign * n * 7);
            (y, m, d)
        }
        "m" | "month" | "months" | "mese" | "mesi" => add_months(date, sign * n),
        "y" | "year" | "years" | "anno" | "anni" => add_months(date, sign * n * 12),
        _ => return None,
    };
    if !valid(result.0, result.1, result.2) {
        return None;
    }
    Some(format!("{} {} {n} {unit} = {}", fmt_date(date), if sign > 0 { "+" } else { "-" }, fmt_date(result)))
}

// ---- currency ----------------------------------------------------------------------------------------------

/// The ECB's `eurofxref-daily.xml`: every `currency='USD' rate='1.0873'` as (code, units per euro).
pub fn parse_ecb(xml: &str) -> Vec<(String, f64)> {
    let mut out = vec![];
    let mut rest = xml;
    while let Some(at) = rest.find("currency=") {
        rest = &rest[at + "currency=".len()..];
        let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"');
        let Some(q) = quote else { continue };
        let Some(end) = rest[1..].find(q) else { break };
        let code = rest[1..1 + end].to_string();
        rest = &rest[end + 2..];
        if let Some(r) = rest.find("rate=") {
            let after = &rest[r + 5..];
            if let Some(q2) = after.chars().next().filter(|c| *c == '\'' || *c == '"') {
                if let Some(e2) = after[1..].find(q2) {
                    if let Ok(v) = after[1..1 + e2].parse::<f64>() {
                        if code.len() == 3 && v > 0.0 {
                            out.push((code.to_uppercase(), v));
                        }
                    }
                }
            }
        }
    }
    out
}

fn rate_of(rates: &[(String, f64)], code: &str) -> Option<f64> {
    let c = code.to_uppercase();
    if c == "EUR" {
        return Some(1.0);
    }
    rates.iter().find(|(k, _)| *k == c).map(|(_, v)| *v)
}

/// `20 usd in eur`, `20usd to gbp`, `100 eur = jpy`: both codes must be known.
pub fn convert_currency(lower: &str, rates: &[(String, f64)]) -> Option<String> {
    let mut words: Vec<String> = lower.split_whitespace().map(str::to_string).collect();
    // "20usd" -> "20", "usd"
    if let Some(w) = words.first().cloned() {
        if let Some(at) = w.find(|c: char| c.is_ascii_alphabetic()).filter(|at| *at > 0) {
            words[0] = w[at..].to_string();
            words.insert(0, w[..at].to_string());
        }
    }
    let (amount, from, to) = match words.as_slice() {
        [a, f, k, t] if matches!(k.as_str(), "in" | "to" | "into" | "=" | "->" | "a" | "in:") => (a.replace(',', ".").parse::<f64>().ok()?, f, t),
        _ => return None,
    };
    if !amount.is_finite() || from.len() != 3 || to.len() != 3 {
        return None;
    }
    let (rf, rt) = (rate_of(rates, from)?, rate_of(rates, to)?);
    let v = amount / rf * rt;
    let shown = if v.abs() >= 100.0 { format!("{v:.2}") } else if v.abs() >= 1.0 { format!("{v:.3}") } else { format!("{v:.5}") };
    Some(format!("{} {} = {} {}", crate::bar::format_number(amount), from.to_uppercase(), shown.trim_end_matches('0').trim_end_matches('.'), to.to_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Ymd = (2027, 1, 15); // a Friday

    fn counter() -> impl FnMut() -> u64 {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        }
    }

    fn g(s: &str) -> Option<Gen> {
        parse_gen(&s.split_whitespace().collect::<Vec<_>>())
    }

    #[test]
    fn what_to_generate_is_read_from_the_words() {
        assert_eq!(g("uuid"), Some(Gen::Uuid));
        assert_eq!(g("pw"), Some(Gen::Password(20)));
        assert_eq!(g("password 32"), Some(Gen::Password(32)));
        assert_eq!(g("pw 3"), Some(Gen::Password(8)), "short ones are raised");
        assert_eq!(g("roll 2d6"), Some(Gen::Dice { n: 2, sides: 6 }));
        assert_eq!(g("d20"), Some(Gen::Dice { n: 1, sides: 20 }));
        assert_eq!(g("3d8"), Some(Gen::Dice { n: 3, sides: 8 }));
        assert_eq!(g("roll"), Some(Gen::Dice { n: 1, sides: 6 }));
        assert_eq!(g("dado 12"), Some(Gen::Dice { n: 1, sides: 12 }));
        assert_eq!(g("coin"), Some(Gen::Coin));
        assert_eq!(g("random 100"), Some(Gen::Range(1, 100)));
        assert_eq!(g("random 5-10"), Some(Gen::Range(5, 10)));
        assert_eq!(g("random 5 10"), Some(Gen::Range(5, 10)));
        assert_eq!(g("random 10 5"), None);
        assert_eq!((g("2d"), g("roll 0d6"), g("d1"), g("pw x"), g("uuid now"), g("dinner"), g("")), (None, None, None, None, None, None, None));
    }

    #[test]
    fn generated_values_have_the_right_shape() {
        let mut r = counter();
        let u = generate(&Gen::Uuid, &mut r);
        let id = u.strip_prefix("UUID = ").unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_eq!(&id[14..15], "4", "version 4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "variant");
        for n in [8, 20, 64] {
            let pw = generate(&Gen::Password(n), &mut r);
            let pw = pw.strip_prefix("Password = ").unwrap();
            assert_eq!(pw.chars().count(), n);
            assert!(pw.chars().any(|c| c.is_ascii_lowercase()) && pw.chars().any(|c| c.is_ascii_uppercase()) && pw.chars().any(|c| c.is_ascii_digit()) && pw.chars().any(|c| "!@#$%&*-_+=?".contains(c)), "{pw}");
            assert!(!pw.contains(['l', 'I', 'O', '0', '1']), "no look-alikes: {pw}");
        }
        let d = generate(&Gen::Dice { n: 3, sides: 6 }, &mut r);
        let (shown, total) = d.rsplit_once(" = ").unwrap();
        let sum: u64 = shown.trim_start_matches("3d6: ").split(" + ").map(|x| x.parse::<u64>().unwrap()).sum();
        assert_eq!(sum, total.parse::<u64>().unwrap());
        for _ in 0..200 {
            let v: i64 = generate(&Gen::Range(-3, 4), &mut r).rsplit_once(" = ").unwrap().1.parse().unwrap();
            assert!((-3..=4).contains(&v));
            let one: u64 = generate(&Gen::Dice { n: 1, sides: 20 }, &mut r).rsplit_once(" = ").unwrap().1.parse().unwrap();
            assert!((1..=20).contains(&one));
        }
        assert!(matches!(generate(&Gen::Coin, &mut r).as_str(), "Coin = Heads" | "Coin = Tails"));
    }

    #[test]
    fn dates_are_read_in_several_ways() {
        let p = |s: &str| parse_date(&s.split_whitespace().collect::<Vec<_>>(), TODAY);
        assert_eq!(p("2027-02-03"), Some(((2027, 2, 3), 1)));
        assert_eq!(p("3 feb"), Some(((2027, 2, 3), 2)));
        assert_eq!(p("3 feb 2028"), Some(((2028, 2, 3), 3)));
        assert_eq!(p("feb 3"), Some(((2027, 2, 3), 2)));
        assert_eq!(p("3/2"), Some(((2027, 2, 3), 1)), "day first");
        assert_eq!(p("3/2/28"), Some(((2028, 2, 3), 1)));
        assert_eq!(p("10 jan"), Some(((2028, 1, 10), 2)), "already past this year: next year");
        assert_eq!(p("15 gen"), Some(((2027, 1, 15), 2)), "today itself is today");
        assert_eq!(p("30 feb"), None);
        assert_eq!(p("tomorrow"), Some(((2027, 1, 16), 1)));
        assert_eq!(p("ieri"), Some(((2027, 1, 14), 1)));
        assert_eq!(p("banana"), None);
    }

    #[test]
    fn how_many_days_and_what_day() {
        assert_eq!(date_calc("days until 3 feb", TODAY).as_deref(), Some("Wed 3 Feb 2027 = in 19 days (2 weeks 5 days)"));
        assert_eq!(date_calc("days to 16 jan", TODAY).as_deref(), Some("Sat 16 Jan 2027 = in 1 day"));
        assert_eq!(date_calc("giorni al 25 dic", TODAY).as_deref(), Some("Sat 25 Dec 2027 = in 344 days (49 weeks 1 day)"));
        assert_eq!(date_calc("days since 2027-01-01", TODAY).as_deref(), Some("Fri 1 Jan 2027 = 14 days (2 weeks) ago"));
        assert_eq!(date_calc("days until today", TODAY).as_deref(), Some("Fri 15 Jan 2027 = today"));
        assert_eq!(date_calc("days until banana", TODAY), None);
        assert_eq!(date_calc("days", TODAY), None);
        // a date alone says what day it is and how far
        assert_eq!(date_calc("3 feb", TODAY).as_deref(), Some("Wed 3 Feb 2027 = in 19 days (2 weeks 5 days)"));
        assert_eq!(date_calc("2027-03-01", TODAY).as_deref(), Some("Mon 1 Mar 2027 = in 45 days (6 weeks 3 days)"));
        assert_eq!(date_calc("25/12", TODAY), None, "that is a division as well");
    }

    #[test]
    fn dates_move_by_days_weeks_months_and_years() {
        assert_eq!(date_calc("today + 45 days", TODAY).as_deref(), Some("Fri 15 Jan 2027 + 45 days = Mon 1 Mar 2027"));
        assert_eq!(date_calc("today +45 days", TODAY).as_deref(), Some("Fri 15 Jan 2027 + 45 days = Mon 1 Mar 2027"));
        assert_eq!(date_calc("3 feb - 2 weeks", TODAY).as_deref(), Some("Wed 3 Feb 2027 - 2 weeks = Wed 20 Jan 2027"));
        assert_eq!(date_calc("31 jan + 1 month", (2027, 1, 15)).as_deref(), Some("Sun 31 Jan 2027 + 1 month = Sun 28 Feb 2027"), "the day is cut to the month");
        assert_eq!(date_calc("2027-01-15 + 1 year", TODAY).as_deref(), Some("Fri 15 Jan 2027 + 1 year = Sat 15 Jan 2028"));
        assert_eq!(date_calc("today + 3 parsecs", TODAY), None);
        assert_eq!(date_calc("5 + 3 days", TODAY), None, "a sum is not a date");
    }

    #[test]
    fn the_iso_week() {
        assert_eq!(iso_week((2027, 1, 15)), (2027, 2));
        assert_eq!(iso_week((2027, 1, 1)), (2026, 53), "the first days can belong to the last week of the year before");
        assert_eq!(iso_week((2026, 12, 31)), (2026, 53));
        assert_eq!(iso_week((2025, 12, 29)), (2026, 1), "and the last ones to week 1 of the next");
        assert_eq!(date_calc("week", TODAY).as_deref(), Some("Week = 2 of 2027"));
    }

    const ECB: &str = "<gesmes:Envelope><Cube><Cube time='2026-11-20'><Cube currency='USD' rate='1.0800'/><Cube currency=\"GBP\" rate=\"0.8400\"/><Cube currency='JPY' rate='160.00'/><Cube currency='XX' rate='1'/></Cube></Cube></gesmes:Envelope>";

    #[test]
    fn the_ecb_file_is_read() {
        let r = parse_ecb(ECB);
        assert_eq!(r, vec![("USD".to_string(), 1.08), ("GBP".to_string(), 0.84), ("JPY".to_string(), 160.0)]);
        assert!(parse_ecb("nothing").is_empty());
    }

    #[test]
    fn money_is_converted_through_the_euro() {
        let r = parse_ecb(ECB);
        assert_eq!(convert_currency("20 usd in eur", &r).as_deref(), Some("20 USD = 18.519 EUR"));
        assert_eq!(convert_currency("100 eur to jpy", &r).as_deref(), Some("100 EUR = 16000 JPY"));
        assert_eq!(convert_currency("20usd to gbp", &r).as_deref(), Some("20 USD = 15.556 GBP"));
        assert_eq!(convert_currency("1,5 eur in usd", &r).as_deref(), Some("1.5 EUR = 1.62 USD"));
        assert_eq!(convert_currency("20 usd in zzz", &r), None);
        assert_eq!(convert_currency("5 km in mi", &r), None, "units are not money");
        assert_eq!(convert_currency("20 usd in eur", &[]), None, "no rates yet");
    }
}
