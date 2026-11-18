//! The command bar: one line of text becomes a timer, a reminder, a sum or a unit conversion.
//!
//! ```text
//! 10m tea              timer           remind 20m call mom     a reminder (also: tra 20m / ricordami 15:30 ...)
//! pomodoro | stopwatch focus 45        dnd on | dnd off
//! 12*7+3 | sqrt(2)     a sum           5 km in mi | 70 f to c  a conversion
//! ```

/// What a line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Timer { secs: u64 },
    Pomodoro,
    Stopwatch,
    Dnd(bool),
    Focus { minutes: u32 },
    /// Remind after `secs` seconds (a clock time has already been turned into a delay).
    Remind { secs: i64, text: String },
    /// A sum or a conversion: the text to show and to copy.
    Value(String),
}

/// What the bar tells while typing: the preview line and whether Enter does something.
pub fn preview(cmd: &Cmd) -> String {
    match cmd {
        Cmd::Timer { secs } => format!("Start a timer · {}", crate::duration::fmt_remaining(*secs as i64 * 1000)),
        Cmd::Pomodoro => "Start a Pomodoro".into(),
        Cmd::Stopwatch => "Start the stopwatch".into(),
        Cmd::Dnd(on) => format!("Do not disturb {}", if *on { "on" } else { "off" }),
        Cmd::Focus { minutes } => format!("Focus mode · {minutes} min"),
        Cmd::Remind { secs, text } => format!("Remind in {} · {text}", human_delay(*secs)),
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
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let lower = line.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let first = words[0];
    match first {
        "pomodoro" | "pomo" => return Some(Cmd::Pomodoro),
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
            let (secs, _) = take_duration_bare(&words[1..])?;
            return Some(Cmd::Timer { secs: secs as u64 });
        }
        "remind" | "reminder" | "ricorda" | "ricordami" | "promemoria" | "avvisami" | "in" | "tra" | "fra" => {
            let mut rest = &words[1..];
            if first != "in" && first != "tra" && first != "fra" {
                // remind me to ... in 20m / remind me in 20m ...
                if matches!(rest.first(), Some(&("me" | "mi"))) {
                    rest = &rest[1..];
                }
                if matches!(rest.first(), Some(&("in" | "tra" | "fra" | "at" | "alle" | "to" | "di"))) && rest.get(1).is_some_and(|w| delay_or_clock(&rest[1..], local_secs).is_some()) {
                    rest = &rest[1..];
                }
            }
            let (secs, used) = delay_or_clock(rest, local_secs)?;
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
    if let Some((secs, _)) = take_duration(&words) {
        if words.len() == 1 || !matches!(words.get(2).copied(), Some("in" | "to" | "as" | "->")) {
            return Some(Cmd::Timer { secs: secs as u64 });
        }
    }
    if let Some(v) = convert(&lower) {
        return Some(Cmd::Value(v));
    }
    let expr = lower.strip_prefix('=').unwrap_or(&lower);
    eval(expr).map(|v| Cmd::Value(format_number(v)))
}

fn delay_or_clock(words: &[&str], local_secs: i64) -> Option<(i64, usize)> {
    if let Some((secs, n)) = take_duration_bare(words) {
        return Some((secs, n));
    }
    // 15:30 or 9.05
    let w = words.first()?;
    let (h, m) = w.split_once([':', '.'])?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    let mut d = h * 3600 + m * 60 - local_secs;
    if d <= 0 {
        d += 86_400;
    }
    Some((d, 1))
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
        assert_eq!(p("10m tea"), Some(Cmd::Timer { secs: 600 }));
        assert_eq!(p("25m"), Some(Cmd::Timer { secs: 1500 }));
        assert_eq!(p("1h30m"), Some(Cmd::Timer { secs: 5400 }));
        assert_eq!(p("90s"), Some(Cmd::Timer { secs: 90 }));
        assert_eq!(p("timer 20 minutes"), Some(Cmd::Timer { secs: 1200 }));
        assert_eq!(p("timer 25"), Some(Cmd::Timer { secs: 1500 }), "a bare number is minutes");
        assert_eq!(p("2 ore"), Some(Cmd::Timer { secs: 7200 }));
        assert_eq!(p("1.5h"), Some(Cmd::Timer { secs: 5400 }));
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
        assert_eq!(preview(&Cmd::Timer { secs: 600 }), "Start a timer · 10:00");
        assert_eq!(preview(&Cmd::Remind { secs: 1200, text: "x".into() }), "Remind in 20 min · x");
        assert_eq!(preview(&Cmd::Remind { secs: 7500, text: "y".into() }), "Remind in 2 h 5 min · y");
        assert_eq!(preview(&Cmd::Dnd(false)), "Do not disturb off");
    }
}
