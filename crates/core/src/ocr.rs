//! Reading the text of an image on the clipboard (a screenshot of a slide, a scanned page) with Tesseract.
//! The pure parts: which clipboard type is an image, which languages to ask for, and tidying what comes out.

/// The image type to ask the clipboard for, out of `wl-paste --list-types` (one MIME type per line): PNG first.
pub fn pick_image_type(list: &str) -> Option<&str> {
    const WANTED: [&str; 5] = ["image/png", "image/jpeg", "image/webp", "image/bmp", "image/tiff"];
    let have: Vec<&str> = list.lines().map(str::trim).collect();
    WANTED.iter().find(|w| have.contains(w)).copied()
}

/// The `-l` argument for Tesseract: Italian and English when installed (out of `tesseract --list-langs`), else what is
/// there of them, else English (its default).
pub fn pick_langs(list: &str) -> String {
    let have: Vec<&str> = list.lines().map(str::trim).collect();
    let langs: Vec<&str> = ["ita", "eng"].into_iter().filter(|l| have.contains(l)).collect();
    if langs.is_empty() { "eng".to_string() } else { langs.join("+") }
}

/// What Tesseract printed, made fit to paste: no form feeds, no trailing spaces, at most one empty line in a row, and a
/// word cut by a hyphen at the end of a line joined again.
pub fn tidy(raw: &str) -> String {
    let mut lines: Vec<String> = vec![];
    for l in raw.replace('\u{c}', "").lines() {
        let l = l.trim_end();
        if l.is_empty() && lines.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        lines.push(l.to_string());
    }
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        // "informa-" + "zione": a lower-case letter before the hyphen and one after the break
        let joins = l.ends_with('-') && l.chars().rev().nth(1).is_some_and(|c| c.is_lowercase()) && lines.get(i + 1).and_then(|n| n.chars().next()).is_some_and(|c| c.is_lowercase());
        if joins {
            out.push_str(l.trim_end_matches('-'));
        } else {
            out.push_str(l);
            out.push('\n');
        }
        i += 1;
    }
    out.trim().to_string()
}

/// The start of the text for a card: one line, cut at `max` characters.
pub fn preview(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max).collect::<String>().trim_end()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_type_and_the_languages_are_picked() {
        assert_eq!(pick_image_type("text/plain\nimage/jpeg\nimage/png\n"), Some("image/png"));
        assert_eq!(pick_image_type("text/plain\nimage/jpeg"), Some("image/jpeg"));
        assert_eq!(pick_image_type("text/plain\ntext/html"), None);
        assert_eq!(pick_image_type(""), None);
        assert_eq!(pick_langs("List of available languages in \"/usr/share/tessdata/\" (4):\nafr\neng\nita\nosd\n"), "ita+eng");
        assert_eq!(pick_langs("eng\nosd"), "eng");
        assert_eq!(pick_langs("fra\nosd"), "eng");
    }

    #[test]
    fn what_comes_out_is_made_fit_to_paste() {
        let raw = "Le reti di calcolatori \u{c}\n\n\n\nsono un insieme di informa-\nzione e di nodi.\n\nCapitolo 2 - \n";
        assert_eq!(tidy(raw), "Le reti di calcolatori\n\nsono un insieme di informazione e di nodi.\n\nCapitolo 2 -");
        assert_eq!(tidy("  \n\n"), "");
        assert_eq!(tidy("well-\nKnown"), "well-\nKnown", "a capital after the break is not a cut word");
        assert_eq!(preview("a  b\nc d e f", 5), "a b c…");
        assert_eq!(preview("short", 50), "short");
    }
}
