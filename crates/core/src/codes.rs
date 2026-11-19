//! One-time codes in notifications ("Your code is 482913"): found so the island can show them big and copy them.

const WORDS: &[&str] = &[
    "code", "codice", "otp", "verification", "verifica", "verify", "pin", "passcode", "password", "one-time", "monouso", "security", "sicurezza",
    "authentication", "autenticazione", "login", "log in", "sign in", "sign-in", "accesso", "2fa", "token", "conferma", "confirm",
];

/// The digits of a code in the text, or None. A code needs a word that says it is one, and 4 to 8 digits (a group like
/// `123-456` or `123 456` counts as one); times, amounts, dates and phone numbers are not codes.
pub fn extract(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if !WORDS.iter().any(|w| lower.contains(w)) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut best: Option<(usize, String)> = None;
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && (chars[i - 1].is_alphanumeric() && !chars[i - 1].is_alphabetic() || chars[i - 1] == '+')) {
            i += 1;
            continue;
        }
        // digits, allowing one space or dash between two groups of 3 (123-456)
        let start = i;
        let mut digits = String::new();
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                digits.push(chars[i]);
                i += 1;
            } else if (chars[i] == '-' || chars[i] == ' ') && digits.len() == 3 && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            } else {
                break;
            }
        }
        let before = if start > 0 { chars[start - 1] } else { ' ' };
        let after = chars.get(i).copied().unwrap_or(' ');
        let next = chars.get(i + 1).copied();
        // a letter glued to the digits is a word or an id, not a code
        let glued = after.is_alphabetic() || before.is_alphabetic();
        // a separator that makes it a time, amount, date, decimal or phone number (a full stop that ends a sentence is fine)
        let before_odd = matches!(before, ':' | '.' | ',' | '/' | '€' | '$' | '£' | '#') || before == '-' && start >= 2 && chars[start - 2].is_ascii_digit();
        let after_odd = matches!(after, ':' | ',' | '/' | '%' | '€' | '$' | '£')
            || after == '.' && next.is_some_and(|c| c.is_ascii_digit())
            || after == '-' && next.is_some_and(|c| c.is_ascii_digit());
        if (4..=8).contains(&digits.len()) && !glued && !before_odd && !after_odd {
            // 6 digits is by far the commonest length: prefer it, else the first found
            let score = if digits.len() == 6 { 0 } else { 1 };
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, digits));
            }
        }
    }
    best.map(|(_, d)| d)
}

/// How a code is shown: `123 456` for six digits, `1234 5678` for eight, the rest as they are.
pub fn display(code: &str) -> String {
    match code.len() {
        6 => format!("{} {}", &code[..3], &code[3..]),
        8 => format!("{} {}", &code[..4], &code[4..]),
        _ => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_usual_messages() {
        assert_eq!(extract("Your verification code is 482913"), Some("482913".into()));
        assert_eq!(extract("Il tuo codice di accesso: 5821. Non condividerlo."), Some("5821".into()));
        assert_eq!(extract("G-123456 is your Google verification code"), Some("123456".into()));
        assert_eq!(extract("Use 123-456 to log in"), Some("123456".into()));
        assert_eq!(extract("OTP 90817265 valid for 5 minutes"), Some("90817265".into()));
        assert_eq!(extract("Codice: 123 456"), Some("123456".into()));
    }

    #[test]
    fn what_is_not_a_code_is_left_alone() {
        assert_eq!(extract("Meeting at 10:30 in room 2048"), None, "no word that says code");
        assert_eq!(extract("Your code expires at 12:45"), None, "a time");
        assert_eq!(extract("Security alert: 2026-11-19 login from Pisa"), None, "a date");
        assert_eq!(extract("Password changed. Charged 1234.50 €"), None, "an amount");
        assert_eq!(extract("Call +39 050 1234567 for the code"), None, "a phone number");
        assert_eq!(extract("Your login code is 12"), None, "too short");
        assert_eq!(extract("Your login code is 123456789012"), None, "too long");
        assert_eq!(extract(""), None);
    }

    #[test]
    fn six_digits_win_over_other_numbers() {
        assert_eq!(extract("Order 20481 code 553821 for pickup"), Some("553821".into()));
    }

    #[test]
    fn a_code_is_shown_in_groups() {
        assert_eq!(display("482913"), "482 913");
        assert_eq!(display("90817265"), "9081 7265");
        assert_eq!(display("5821"), "5821");
    }
}
