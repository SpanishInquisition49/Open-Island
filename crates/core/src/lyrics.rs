//! Synced lyrics: LRC parsing, the line being sung, and picking the best of several LRCLIB versions.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub ms: f64,
    pub text: String,
}

/// LRC text -> lines sorted by time, or None when it has no timed lines.
pub fn parse_lrc(text: &str) -> Option<Vec<Line>> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let Some(rest) = raw.strip_prefix('[') else { continue };
        let Some((stamp, text)) = rest.split_once(']') else { continue };
        let Some((mm, ss)) = stamp.split_once(':') else { continue };
        let (Ok(mm), Ok(ss)) = (mm.parse::<f64>(), ss.parse::<f64>()) else { continue };
        if !mm.is_finite() || !ss.is_finite() {
            continue;
        }
        out.push(Line { ms: (mm * 60.0 + ss) * 1000.0, text: text.trim().to_string() });
    }
    out.sort_by(|a, b| a.ms.total_cmp(&b.ms));
    (!out.is_empty()).then_some(out)
}

/// Index (1-based, 0 = before the first line) of the line at `pos_ms`. `lead_ms` shows a line slightly
/// early, which reads better than slightly late.
pub fn index_at(lines: &[Line], pos_ms: f64, lead_ms: f64) -> usize {
    let pos = pos_ms + lead_ms;
    lines.iter().take_while(|l| l.ms <= pos).count()
}

/// The line being sung ("" before the first line).
pub fn line_at(lines: &[Line], pos_ms: f64, lead_ms: f64) -> &str {
    match index_at(lines, pos_ms, lead_ms) {
        0 => "",
        i => &lines[i - 1].text,
    }
}

/// Previous, current and next lines ("" where there is none) and the current index.
pub fn window(lines: &[Line], pos_ms: f64, lead_ms: f64) -> (&str, &str, &str, usize) {
    let idx = index_at(lines, pos_ms, lead_ms);
    let at = |i: isize| -> &str {
        if i >= 1 && (i as usize) <= lines.len() {
            &lines[i as usize - 1].text
        } else {
            ""
        }
    };
    let i = idx as isize;
    (at(i - 1), at(i), at(i + 1), idx)
}

/// Font size that fits `text` in `width`: `base` when it fits, smaller down to `min` (about 0.52 em
/// per character).
pub fn fit_font(text: &str, width: f32, base: f32, min: f32) -> f32 {
    let n = text.chars().count();
    if n == 0 {
        return base;
    }
    let fit = width / (n as f32 * 0.52);
    min.max(base.min((fit * 2.0).floor() / 2.0))
}

/// Title for the lyrics search: without "(feat. ...)", "[feat ...]", "(with ...)" or " - Remastered ..." parts.
pub fn clean_title(title: &str) -> String {
    let mut t = title.to_string();
    for open in ['(', '['] {
        let close = if open == '(' { ')' } else { ']' };
        while let Some(start) = t.find(open) {
            let Some(end) = t[start..].find(close).map(|e| start + e) else { break };
            let inner = t[start + 1..end].to_lowercase();
            let is_feat = inner.starts_with("feat") || inner.starts_with("ft.") || inner.starts_with("with ");
            if !is_feat {
                // keep this bracket: blank it so the loop moves on, restore afterwards
                t.replace_range(start..start + 1, "\u{0}");
                continue;
            }
            let from = t[..start].trim_end().len();
            t.replace_range(from..=end, "");
        }
    }
    let mut t = t.replace('\u{0}', "(");
    if let Some(pos) = t.find(" - ") {
        if t[pos..].to_lowercase().contains("remaster") && !t[pos + 3..].contains(" - ") {
            t.truncate(pos);
        }
    }
    let t = t.trim().to_string();
    if t.is_empty() {
        title.to_string()
    } else {
        t
    }
}

/// Lyrics text without timestamps, case and punctuation: versions that only differ in timing or in
/// commas count as the same transcription.
pub fn lyrics_key(lrc: &str) -> String {
    let mut out = String::with_capacity(lrc.len());
    let mut in_stamp = false;
    for c in lrc.chars() {
        match c {
            '[' => in_stamp = true,
            ']' if in_stamp => in_stamp = false,
            _ if in_stamp && (c.is_ascii_digit() || c == ':' || c == '.') => {}
            _ => {
                if in_stamp {
                    // not a timestamp after all: "[" was text
                    in_stamp = false;
                }
                if c.is_alphanumeric() {
                    out.extend(c.to_lowercase());
                }
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct Version {
    pub id: Option<u64>,
    pub duration: f64,
    pub synced: String,
}

/// Picks synced lyrics from LRCLIB search results: among versions whose duration matches the track
/// (within `tolerance` s) the transcription most of them agree on wins, then the closest duration
/// within it. Duplicate ids count once; the first seen wins a tie.
pub fn pick(results: &[Version], track_secs: f64, tolerance: f64) -> Option<String> {
    struct Group {
        count: usize,
        best: usize,
        best_diff: f64,
    }
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, v) in results.iter().enumerate() {
        if v.synced.is_empty() {
            continue;
        }
        if let Some(id) = v.id {
            if !seen.insert(id) {
                continue;
            }
        }
        let diff = if track_secs > 0.0 { (v.duration - track_secs).abs() } else { 0.0 };
        if track_secs > 0.0 && diff > tolerance {
            continue;
        }
        let key = lyrics_key(&v.synced);
        let g = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Group { count: 0, best: i, best_diff: f64::INFINITY }
        });
        g.count += 1;
        if diff < g.best_diff {
            g.best = i;
            g.best_diff = diff;
        }
    }
    let mut winner: Option<&Group> = None;
    for k in &order {
        let g = &groups[k];
        if winner.is_none_or(|w| g.count > w.count) {
            winner = Some(g);
        }
    }
    winner.map(|g| results[g.best].synced.clone())
}

/// How long the strip takes to scroll one line, in ms.
pub const SCROLL_MS: f64 = 460.0;

/// Cubic ease-out: fast at first, settling softly.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The look of a lyric line `slot` rows away from the current one (a fraction while the strip scrolls):
/// (emphasis 0..=1 for size and weight, opacity 0..=1). The current line is full, its neighbours dim, and
/// lines farther away fade out so nothing pops in or out at the edges.
pub fn line_look(slot: f32) -> (f32, f32) {
    let a = slot.abs();
    let emphasis = (1.0 - a).max(0.0);
    let opacity = if a <= 1.0 { 0.4 + 0.6 * emphasis } else { (0.4 * (2.0 - a)).max(0.0) };
    (emphasis, opacity)
}

/// The lines to draw while the strip scrolls: (1-based line number, slot) around the current line `cur`
/// (0 = before the first line). `progress` runs 0..=1 from the start of the scroll; `dir` is 1 when the song
/// moved forward (the strip starts one row lower and slides up) and -1 when it moved back.
pub fn strip(cur: usize, progress: f64, dir: f64) -> Vec<(i64, f32)> {
    let shift = (dir.signum() * (1.0 - ease_out(progress))) as f32;
    (-2i64..=2).map(|d| (cur as i64 + d, d as f32 + shift)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc() {
        let l = parse_lrc("[00:12.50] two\n[00:01.00] one \n[bad] x\nnot a line").unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], Line { ms: 1000.0, text: "one".into() });
        assert_eq!(l[1].ms, 12500.0);
        assert!(parse_lrc("nothing").is_none());
    }

    #[test]
    fn current_line() {
        let l = parse_lrc("[00:01.00] a\n[00:02.00] b").unwrap();
        assert_eq!(line_at(&l, 500.0, 0.0), "");
        assert_eq!(line_at(&l, 1500.0, 0.0), "a");
        assert_eq!(line_at(&l, 1900.0, 200.0), "b");
        let (p, c, n, i) = window(&l, 1500.0, 0.0);
        assert_eq!((p, c, n, i), ("", "a", "b", 1));
        let (p, c, n, i) = window(&l, 500.0, 0.0);
        assert_eq!((p, c, n, i), ("", "", "a", 0));
        let (_, c, n, _) = window(&l, 5000.0, 0.0);
        assert_eq!((c, n), ("b", ""));
    }

    #[test]
    fn the_strip_scrolls_up_and_settles() {
        let start = strip(5, 0.0, 1.0);
        assert_eq!(start[2], (5, 1.0), "the new current line starts one row lower");
        assert_eq!(start[1], (4, 0.0), "the old current line is where the current one will be... in the middle");
        let end = strip(5, 1.0, 1.0);
        assert_eq!(end.iter().map(|(_, s)| *s).collect::<Vec<_>>(), vec![-2.0, -1.0, 0.0, 1.0, 2.0]);
        let back = strip(5, 0.0, -1.0);
        assert_eq!(back[2].1, -1.0, "going back slides down");
        let mid = strip(5, 0.3, 1.0);
        assert!(mid[2].1 > 0.0 && mid[2].1 < 1.0);
    }

    #[test]
    fn looks_fade_smoothly() {
        assert_eq!(line_look(0.0), (1.0, 1.0));
        assert_eq!(line_look(1.0), (0.0, 0.4));
        assert_eq!(line_look(-1.0), (0.0, 0.4));
        assert_eq!(line_look(2.0), (0.0, 0.0), "gone at the edge");
        let (e, o) = line_look(0.5);
        assert!((e - 0.5).abs() < 1e-6 && (o - 0.7).abs() < 1e-6);
        let mut last = 1.0;
        for i in 0..40 {
            let (_, o) = line_look(i as f32 * 0.05);
            assert!(o <= last + 1e-6, "opacity never rises with distance");
            last = o;
        }
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5 && ease_out(-3.0) == 0.0 && ease_out(9.0) == 1.0);
    }

    #[test]
    fn fonts() {
        assert_eq!(fit_font("short line", 388.0, 13.0, 10.0), 13.0);
        let long = "How could I ever be in peace when nothing else matters to me";
        let fs = fit_font(long, 388.0, 13.0, 10.0);
        assert!((10.0..13.0).contains(&fs));
        assert!(long.len() as f32 * fs * 0.52 <= 388.0);
        assert_eq!(fit_font(&"x".repeat(200), 388.0, 13.0, 10.0), 10.0);
        assert_eq!(fit_font("", 388.0, 13.0, 10.0), 13.0);
    }

    #[test]
    fn titles() {
        assert_eq!(clean_title("Touch (feat. Paul Williams)"), "Touch");
        assert_eq!(clean_title("Song [Feat. X]"), "Song");
        assert_eq!(clean_title("Song (with Y)"), "Song");
        assert_eq!(clean_title("Song - 2011 Remastered Version"), "Song");
        assert_eq!(clean_title("Song (Live)"), "Song (Live)");
        assert_eq!(clean_title("(feat. X)"), "(feat. X)");
        assert_eq!(clean_title("A - B"), "A - B");
    }

    #[test]
    fn keys() {
        assert_eq!(lyrics_key("[00:01.00] Hello, World!"), "helloworld");
        assert_eq!(lyrics_key("[01:20.5]hello world"), "helloworld");
    }

    fn v(id: u64, dur: f64, text: &str) -> Version {
        Version { id: Some(id), duration: dur, synced: text.into() }
    }

    #[test]
    fn picks_the_majority_transcription() {
        let home = "[00:01.00] Hold on, if love is the answer, you're home";
        let split = "[00:01.00] Hold on\n[00:02.00] If love is the answer\n[00:03.00] Hold on";
        let versions = [
            v(1, 498.0, split),
            v(2, 499.0, home),
            v(3, 497.5, home),
            v(4, 498.5, home),
            v(5, 498.0, split),
            v(2, 499.0, home), // duplicate id from the second search
        ];
        assert_eq!(pick(&versions, 498.2, 5.0).as_deref(), Some(home));
        assert_eq!(pick(&versions[..2], 498.2, 5.0).as_deref(), Some(split), "a tie keeps the first seen");
        assert_eq!(pick(&[v(9, 300.0, home)], 498.2, 5.0), None, "duration outside tolerance");
    }
}
