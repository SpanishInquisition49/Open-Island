//! Emoji and symbols for the command bar (`emoji fire`, `simbolo freccia`): found by their English name and shortcodes,
//! the common ones by Italian words too. Only emoji made of one character are offered: the island draws text one character
//! at a time, so a flag or a family would be shown in pieces.

/// What can be copied, and what it is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub text: &'static str,
    pub name: &'static str,
}

/// The most results of a search.
pub const MOST: usize = 10;

/// Shown for `emoji` alone.
const COMMON: [&str; MOST] = ["😂", "❤️", "👍", "🔥", "😊", "🙏", "🎉", "😅", "✅", "👀"];
const COMMON_SYMBOLS: [&str; MOST] = ["→", "⇒", "×", "≠", "≤", "≥", "∈", "∀", "λ", "€"];

/// How well the words of a thing answer one word asked for: 0 a whole word, 1 the beginning of one, None not at all.
fn score(words: &str, asked: &str) -> Option<u8> {
    let mut best = None;
    for w in words.split(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == ':').filter(|w| !w.is_empty()) {
        if w.eq_ignore_ascii_case(asked) {
            return Some(0);
        }
        if w.to_lowercase().starts_with(asked) {
            best = Some(1);
        }
    }
    best
}

/// Lower case, without the accents of Italian.
fn fold(s: &str) -> String {
    s.to_lowercase().replace(['à', 'á'], "a").replace(['è', 'é'], "e").replace(['ì', 'í'], "i").replace(['ò', 'ó'], "o").replace(['ù', 'ú'], "u")
}

fn one_character(s: &str) -> bool {
    s.chars().filter(|c| *c != '\u{fe0f}').count() == 1
}

fn italian(emoji: &str) -> &'static str {
    let bare = emoji.trim_end_matches('\u{fe0f}');
    ITALIAN.iter().find(|(e, _)| *e == bare).map_or("", |(_, w)| w)
}

/// The best `MOST` results for the words (all of them must be found), best first. No words: the common ones.
pub fn search(symbols: bool, query: &str) -> Vec<Found> {
    let asked: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    if asked.is_empty() {
        let pick = |s: &&str| {
            if symbols {
                SYMBOLS.iter().find(|(t, _)| t == s).map(|(t, n)| Found { text: t, name: n })
            } else {
                emojis::get(s).map(|e| Found { text: e.as_str(), name: e.name() })
            }
        };
        return if symbols { &COMMON_SYMBOLS } else { &COMMON }.iter().filter_map(pick).collect();
    }
    // the sum of the scores of the words; Italian and the name count the same
    let rank = |words: &[&str]| asked.iter().map(|a| words.iter().filter_map(|w| score(w, a)).min().map(u32::from)).sum::<Option<u32>>();
    let mut found: Vec<(u32, usize, Found)> = if symbols {
        SYMBOLS.iter().enumerate().filter_map(|(i, (t, n))| rank(&[n]).map(|r| (r, i, Found { text: t, name: n }))).collect()
    } else {
        emojis::iter()
            .enumerate()
            .filter(|(_, e)| one_character(e.as_str()))
            .filter_map(|(i, e)| {
                let it = italian(e.as_str());
                let codes = e.shortcodes().collect::<Vec<_>>().join(" ");
                // one with Italian words is a common one: it comes before the rest
                rank(&[e.name(), &codes, it]).map(|r| (r * 2 + u32::from(it.is_empty()), i, Found { text: e.as_str(), name: e.name() }))
            })
            .collect()
    };
    found.sort_by_key(|(r, i, _)| (*r, *i));
    found.into_iter().take(MOST).map(|(.., f)| f).collect()
}

/// Italian words for the emoji most asked for (written without the variation selector).
const ITALIAN: &[(&str, &str)] = &[
    ("😀", "sorriso faccina felice"),
    ("😂", "risata ridere lacrime"),
    ("🤣", "rotolo risate"),
    ("🙂", "sorrisetto"),
    ("😊", "contento arrossito"),
    ("😉", "occhiolino"),
    ("😍", "innamorato occhi cuore"),
    ("😘", "bacio"),
    ("😎", "occhiali sole figo"),
    ("🤔", "pensare pensieroso dubbio"),
    ("😅", "sudore sollievo"),
    ("😢", "triste lacrima piangere"),
    ("😭", "pianto disperato"),
    ("😡", "arrabbiato rabbia"),
    ("😱", "paura urlo"),
    ("😴", "dormire sonno"),
    ("🙄", "occhi al cielo"),
    ("😬", "imbarazzo"),
    ("🤯", "testa esplosa"),
    ("🥳", "festa festeggiare"),
    ("😇", "angelo"),
    ("🤝", "stretta mano accordo"),
    ("👍", "pollice su ok bene"),
    ("👎", "pollice giu male"),
    ("👏", "applauso bravo"),
    ("🙏", "grazie preghiera per favore"),
    ("💪", "forza muscolo"),
    ("👋", "ciao saluto mano"),
    ("🤞", "dita incrociate fortuna"),
    ("👀", "occhi guardare"),
    ("🧠", "cervello"),
    ("❤", "cuore amore rosso"),
    ("💔", "cuore spezzato"),
    ("🔥", "fuoco fiamma"),
    ("✨", "scintille brillantini"),
    ("⭐", "stella"),
    ("🎉", "coriandoli festa"),
    ("🎂", "torta compleanno"),
    ("🎁", "regalo"),
    ("🚀", "razzo lancio"),
    ("💡", "lampadina idea"),
    ("✅", "spunta fatto verde"),
    ("❌", "croce errore no"),
    ("⚠", "attenzione avviso pericolo"),
    ("❓", "domanda"),
    ("❗", "esclamativo"),
    ("💯", "cento"),
    ("📌", "puntina"),
    ("📎", "graffetta allegato"),
    ("📅", "calendario data"),
    ("⏰", "sveglia orologio"),
    ("📚", "libri studio"),
    ("📖", "libro leggere"),
    ("📝", "appunti nota scrivere"),
    ("✏", "matita"),
    ("💻", "portatile computer"),
    ("📱", "telefono cellulare"),
    ("📧", "email posta"),
    ("🔒", "lucchetto chiuso"),
    ("🔑", "chiave"),
    ("🔍", "lente cerca"),
    ("🐛", "bug insetto"),
    ("🔧", "chiave inglese attrezzo"),
    ("⚙", "ingranaggio impostazioni"),
    ("📈", "grafico crescita"),
    ("📉", "grafico calo"),
    ("💰", "soldi sacco"),
    ("🍕", "pizza"),
    ("🍝", "pasta spaghetti"),
    ("☕", "caffe tazza"),
    ("🍺", "birra"),
    ("🍷", "vino"),
    ("🌞", "sole"),
    ("🌙", "luna"),
    ("🌧", "pioggia"),
    ("❄", "neve fiocco"),
    ("🌈", "arcobaleno"),
    ("🐱", "gatto"),
    ("🐶", "cane"),
    ("🏠", "casa"),
    ("🚆", "treno"),
    ("🚗", "auto macchina"),
    ("🚲", "bici bicicletta"),
    ("✈", "aereo"),
    ("🎵", "musica nota"),
    ("🎧", "cuffie"),
    ("⚽", "calcio pallone"),
    ("🏆", "coppa trofeo vittoria"),
    ("🤷", "boh alzata spalle"),
    ("🤦", "facepalm mano in faccia"),
    ("💀", "teschio morto"),
    ("🤖", "robot"),
    ("👻", "fantasma"),
    ("💩", "cacca"),
];

const SYMBOLS: &[(&str, &str)] = &[
    ("→", "freccia destra arrow right"),
    ("←", "freccia sinistra arrow left"),
    ("↑", "freccia su arrow up"),
    ("↓", "freccia giu arrow down"),
    ("↔", "freccia doppia orizzontale arrow both"),
    ("↕", "freccia doppia verticale arrow up down"),
    ("⇒", "freccia implica implies arrow double right"),
    ("⇐", "freccia doppia sinistra arrow double left"),
    ("⇔", "freccia se e solo se iff equivalent arrow double"),
    ("↦", "freccia mappa maps to arrow"),
    ("⟶", "freccia lunga long arrow"),
    ("↩", "freccia ritorno return arrow"),
    ("⏎", "invio enter return"),
    ("×", "per moltiplicazione times multiply"),
    ("÷", "diviso divisione divide"),
    ("±", "piu meno plus minus"),
    ("≠", "diverso not equal"),
    ("≈", "circa quasi uguale approx almost equal"),
    ("≡", "identico equivalente congruent identical"),
    ("≤", "minore uguale less equal"),
    ("≥", "maggiore uguale greater equal"),
    ("≪", "molto minore much less"),
    ("≫", "molto maggiore much greater"),
    ("∞", "infinito infinity"),
    ("√", "radice root square"),
    ("∑", "sommatoria somma sum sigma"),
    ("∏", "produttoria prodotto product"),
    ("∫", "integrale integral"),
    ("∂", "derivata parziale partial"),
    ("∇", "nabla gradiente gradient"),
    ("∆", "delta differenza difference"),
    ("∈", "appartiene element in belongs"),
    ("∉", "non appartiene not in element"),
    ("⊂", "sottoinsieme subset"),
    ("⊆", "sottoinsieme uguale subset equal"),
    ("⊃", "sovrainsieme superset"),
    ("∪", "unione union"),
    ("∩", "intersezione intersection"),
    ("∅", "insieme vuoto empty set"),
    ("∀", "per ogni for all forall"),
    ("∃", "esiste exists"),
    ("∄", "non esiste not exists"),
    ("¬", "negazione non not"),
    ("∧", "and congiunzione e logico wedge"),
    ("∨", "or disgiunzione o logico vee"),
    ("⊕", "xor somma diretta direct sum"),
    ("⊤", "vero top true"),
    ("⊥", "falso bottom false perpendicolare"),
    ("⊢", "dimostra turnstile proves"),
    ("⊨", "modella models entails"),
    ("λ", "lambda greca greek"),
    ("α", "alfa alpha greca greek"),
    ("β", "beta greca greek"),
    ("γ", "gamma greca greek"),
    ("δ", "delta greca greek"),
    ("ε", "epsilon greca greek"),
    ("θ", "theta teta greca greek"),
    ("μ", "mu mi micro greca greek"),
    ("π", "pi greco greek"),
    ("ρ", "rho ro greca greek"),
    ("σ", "sigma greca greek"),
    ("τ", "tau greca greek"),
    ("φ", "phi fi greca greek"),
    ("ψ", "psi greca greek"),
    ("ω", "omega greca greek"),
    ("Γ", "gamma maiuscola capital greek"),
    ("Δ", "delta maiuscola capital greek"),
    ("Σ", "sigma maiuscola capital greek"),
    ("Φ", "phi maiuscola capital greek"),
    ("Ω", "omega maiuscola capital greek ohm"),
    ("ℕ", "naturali natural numbers"),
    ("ℤ", "interi integers numbers"),
    ("ℚ", "razionali rationals numbers"),
    ("ℝ", "reali real numbers"),
    ("ℂ", "complessi complex numbers"),
    ("°", "gradi grado degree"),
    ("·", "punto medio dot middle"),
    ("•", "pallino punto elenco bullet"),
    ("…", "puntini ellissi ellipsis"),
    ("—", "trattino lungo lineetta em dash"),
    ("–", "trattino medio en dash"),
    ("«", "virgolette aperte caporali quote left guillemet"),
    ("»", "virgolette chiuse caporali quote right guillemet"),
    ("“", "virgolette alte aperte quote left curly"),
    ("”", "virgolette alte chiuse quote right curly"),
    ("’", "apostrofo apostrophe"),
    ("§", "paragrafo section"),
    ("¶", "pilcrow paragraph capoverso"),
    ("©", "copyright"),
    ("®", "registrato registered"),
    ("™", "marchio trademark"),
    ("€", "euro valuta currency"),
    ("£", "sterlina pound currency"),
    ("¥", "yen valuta currency"),
    ("$", "dollaro dollar currency"),
    ("¢", "centesimo cent"),
    ("½", "mezzo half frazione"),
    ("⅓", "un terzo third frazione"),
    ("¼", "un quarto quarter frazione"),
    ("¾", "tre quarti quarters frazione"),
    ("²", "quadrato apice squared two superscript"),
    ("³", "cubo apice cubed three superscript"),
    ("₀", "pedice zero subscript"),
    ("₁", "pedice uno subscript one"),
    ("₂", "pedice due subscript two"),
    ("✓", "spunta check segno"),
    ("✗", "croce cross segno"),
    ("★", "stella piena star"),
    ("☆", "stella vuota star"),
    ("⌘", "command comando mac"),
    ("⌥", "option opzione alt mac"),
    ("⇧", "shift maiuscolo"),
    ("⌃", "control ctrl"),
    ("⌫", "backspace cancella delete"),
    ("⇥", "tab tabulazione"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn first(symbols: bool, q: &str) -> &'static str {
        search(symbols, q).first().map_or("", |f| f.text)
    }

    #[test]
    fn emoji_are_found_by_their_english_and_italian_names() {
        assert_eq!(first(false, "fire"), "🔥");
        assert_eq!(first(false, "fuoco"), "🔥");
        assert_eq!(first(false, "Caffè"), "☕");
        assert_eq!(first(false, "pollice su"), "👍");
        assert!(search(false, "heart").len() == MOST);
        assert!(search(false, "qwertyzz").is_empty());
        assert_eq!(search(false, "").len(), MOST, "the common ones");
        assert!(ITALIAN.iter().all(|(e, _)| emojis::iter().any(|x| x.as_str().trim_end_matches('\u{fe0f}') == *e)), "every Italian entry is an emoji");
    }

    #[test]
    fn only_emoji_that_can_be_drawn_whole_are_offered() {
        for q in ["flag", "family", "technologist", "bandiera"] {
            assert!(search(false, q).iter().all(|f| one_character(f.text)), "{q}");
        }
    }

    #[test]
    fn symbols_are_found_in_both_languages() {
        assert_eq!(first(true, "freccia destra"), "→");
        assert_eq!(first(true, "implies"), "⇒");
        assert_eq!(first(true, "appartiene"), "∈");
        assert_eq!(first(true, "lambda"), "λ");
        assert_eq!(search(true, "").len(), MOST);
        let mut all: Vec<&str> = SYMBOLS.iter().map(|(s, _)| *s).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), SYMBOLS.len(), "no symbol twice");
    }
}
