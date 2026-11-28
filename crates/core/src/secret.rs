//! A password while it is being typed and handed over. It lives in one place of a fixed size (so no copy is left behind
//! by a buffer that grows), is wiped when dropped, cannot be cloned or printed, and on Unix its memory is kept out of
//! the swap when the system allows it.

use zeroize::Zeroize;

/// The longest answer polkit's helper reads (PAM_MAX_RESP_SIZE is 512, with the newline and the end of the string).
pub const MAX_SECRET: usize = 510;

pub struct Secret {
    bytes: Box<[u8; MAX_SECRET]>,
    len: usize,
}

impl Secret {
    pub fn new() -> Self {
        let s = Self { bytes: Box::new([0; MAX_SECRET]), len: 0 };
        #[cfg(unix)]
        // SAFETY: the range is the box just made; locking only changes how the kernel treats its pages. A refusal (the
        // limit on locked memory) changes nothing else.
        unsafe {
            libc::mlock(s.bytes.as_ptr().cast(), MAX_SECRET);
        }
        s
    }

    /// Adds what was typed. All of it or nothing: false when it would not fit, or holds a character that cannot be part of
    /// a password sent as one line (a control character).
    pub fn push(&mut self, typed: &str) -> bool {
        if typed.chars().any(char::is_control) || self.len + typed.len() > MAX_SECRET {
            return false;
        }
        self.bytes[self.len..self.len + typed.len()].copy_from_slice(typed.as_bytes());
        self.len += typed.len();
        true
    }

    /// Takes the last character off.
    pub fn pop(&mut self) {
        let Some(mut at) = self.len.checked_sub(1) else { return };
        // back to the first byte of the character
        while at > 0 && self.bytes[at] & 0xc0 == 0x80 {
            at -= 1;
        }
        self.bytes[at..self.len].zeroize();
        self.len = at;
    }

    pub fn clear(&mut self) {
        self.bytes.zeroize();
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many characters were typed (for the dots).
    pub fn chars(&self) -> usize {
        self.bytes[..self.len].iter().filter(|b| *b & 0xc0 != 0x80).count()
    }

    /// The password itself: only for handing it to whoever checks it.
    pub fn expose(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

/// The page stays locked: locks do not stack, so unlocking it would also unlock a secret that shares it.
impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Never the contents, nor their length.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

/// Text that came from outside and is about to be shown where a password is asked: control characters and the marks that
/// reorder or hide text are taken out, space is evened, and what is too long loses its middle (the end of a command says
/// as much as its beginning).
pub fn plain_text(text: &str, max: usize) -> String {
    let hidden = |c: char| c.is_control() || matches!(c, '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}' | '\u{1160}' | '\u{17b4}' | '\u{17b5}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{3164}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}' | '\u{ffa0}' | '\u{fff0}'..='\u{ffff}' | '\u{e0000}'..='\u{e0fff}');
    let even: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !hidden(*c)).collect();
    let even = even.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = even.chars().count();
    if count <= max {
        return even;
    }
    let head = max.saturating_sub(1) * 2 / 3;
    let tail = max.saturating_sub(1) - head;
    format!("{}…{}", even.chars().take(head).collect::<String>(), even.chars().skip(count - tail).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_typed_comes_back_and_goes_a_character_at_a_time() {
        let mut s = Secret::new();
        assert!(s.is_empty());
        assert!(s.push("pàss") && s.push("€"));
        assert_eq!((s.expose(), s.chars()), ("pàss€".as_bytes(), 5));
        s.pop();
        assert_eq!(s.expose(), "pàss".as_bytes());
        assert!(s.bytes[s.len..].iter().all(|b| *b == 0), "what was taken off is wiped");
        s.clear();
        assert!(s.is_empty() && s.bytes.iter().all(|b| *b == 0));
        s.pop();
        assert!(s.is_empty());
    }

    #[test]
    fn it_takes_one_line_of_a_bounded_length_or_nothing() {
        let mut s = Secret::new();
        assert!(!s.push("a\nb") && !s.push("\u{7f}") && !s.push("\0"));
        assert!(s.is_empty());
        assert!(s.push(&"x".repeat(MAX_SECRET)));
        assert!(!s.push("y"));
        assert_eq!(s.expose().len(), MAX_SECRET);
    }

    #[test]
    fn it_never_prints_what_it_holds() {
        let mut s = Secret::new();
        s.push("hunter2");
        assert_eq!(format!("{s:?} {:?}", Some(&s)), "Secret(hidden) Some(Secret(hidden))");
    }

    #[test]
    fn text_from_outside_cannot_reorder_or_hide_itself() {
        assert_eq!(plain_text("run `/bin/\u{202e}hs\u{202c}`\n\tas\u{200b} root\u{7}", 80), "run `/bin/hs` as root");
        assert_eq!(plain_text("  a   b  ", 80), "a b");
        let long = plain_text(&format!("start {} /usr/bin/evil --end", "x".repeat(300)), 60);
        assert!(long.chars().count() == 60 && long.starts_with("start") && long.ends_with("evil --end") && long.contains('…'), "{long}");
    }
}
