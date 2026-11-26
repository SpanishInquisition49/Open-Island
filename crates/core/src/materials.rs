//! The files for a lecture: from the name of an event ("Reti di calcolatori") find the files whose path says the same
//! words (a folder `Reti/`, `slides-reti-03.pdf`), so that they are one click away when the lecture is next.

/// Words of an event title worth looking for: letters and digits, lower case, accents folded, at least four characters,
/// no words that every title has.
pub fn keywords(title: &str) -> Vec<String> {
    const COMMON: [&str; 22] = [
        "lezione", "lezioni", "lecture", "class", "corso", "aula", "online", "teams", "zoom", "meet", "della", "delle", "dello", "degli", "dell", "esame", "exam", "laboratorio", "with", "from", "that", "this",
    ];
    let mut out: Vec<String> = vec![];
    for w in title.split(|c: char| !c.is_alphanumeric()) {
        let w = fold(w);
        if w.chars().count() >= 4 && !COMMON.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Lower case with the common Italian accents removed, so that "Probabilità" finds "probabilita".
pub fn fold(s: &str) -> String {
    s.to_lowercase().chars().map(|c| match c { 'à' | 'á' => 'a', 'è' | 'é' => 'e', 'ì' | 'í' => 'i', 'ò' | 'ó' => 'o', 'ù' | 'ú' => 'u', c => c }).collect()
}

/// Files that are study material: documents, slides, notes.
pub fn is_study_file(name: &str) -> bool {
    const EXT: [&str; 12] = ["pdf", "epub", "md", "txt", "pptx", "ppt", "odp", "docx", "doc", "odt", "ipynb", "tex"];
    // the files every project folder has are not course material
    const BOILERPLATE: [&str; 6] = ["readme", "todo", "license", "licence", "changelog", "notes"];
    name.rsplit_once('.').is_some_and(|(stem, e)| !stem.is_empty() && EXT.contains(&e.to_lowercase().as_str()) && !BOILERPLATE.contains(&stem.to_lowercase().as_str()))
}

/// Slides and books are what one opens for a lecture; notes and sources come after.
pub fn kind_weight(name: &str) -> usize {
    match name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).as_deref() {
        Some("pdf" | "pptx" | "ppt" | "odp" | "epub") => 2,
        Some("docx" | "doc" | "odt" | "md") => 1,
        _ => 0,
    }
}

/// How many of the keywords the path mentions (folders and file name), 0 when none.
pub fn score(path: &str, keywords: &[String]) -> usize {
    let p = fold(path);
    keywords.iter().filter(|k| p.contains(k.as_str())).count()
}

/// Folders that are never worth walking.
pub fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "venv" | "__pycache__" | "build" | "dist" | "site-packages" | "Trash" | "snap")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub path: String,
    pub name: String,
    pub modified_ms: i64,
}

/// The best files out of candidates (path, modified): those that match, the ones matching most words first and, among
/// them, the newest; at most `max`.
pub fn rank(mut found: Vec<(Material, usize)>, max: usize) -> Vec<Material> {
    found.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.modified_ms.cmp(&a.0.modified_ms)).then(a.0.path.cmp(&b.0.path)));
    found.into_iter().take(max).map(|(m, _)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_gives_its_useful_words() {
        assert_eq!(keywords("Reti di calcolatori"), ["reti", "calcolatori"]);
        assert_eq!(keywords("Lezione di Probabilità (Aula B3, Teams)"), ["probabilita"]);
        assert_eq!(keywords("Lab"), Vec::<String>::new(), "too short to look for");
        assert_eq!(keywords("Reti reti RETI"), ["reti"]);
    }

    #[test]
    fn paths_are_matched_by_the_words_and_study_files_by_their_kind() {
        let k = keywords("Reti di calcolatori");
        assert_eq!(score("/home/u/Documents/Uni/Reti/slides-03.pdf", &k), 1);
        assert_eq!(score("/home/u/Downloads/Calcolatori-reti-appunti.pdf", &k), 2);
        assert_eq!(score("/home/u/Documents/Cucina/ricette.pdf", &k), 0);
        assert!(is_study_file("slides.PDF") && is_study_file("my-notes.md") && !is_study_file("photo.jpg") && !is_study_file(".pdf") && !is_study_file("README"));
        assert!(!is_study_file("README.md") && !is_study_file("TODO.md") && !is_study_file("notes.md"), "project boilerplate");
        assert!(kind_weight("a.pdf") > kind_weight("a.md") && kind_weight("a.md") > kind_weight("a.tex"));
        assert!(skip_dir(".git") && skip_dir("node_modules") && !skip_dir("Reti"));
        assert_eq!(fold("Perché"), "perche");
    }

    #[test]
    fn the_best_match_comes_first_then_the_newest() {
        let m = |p: &str, t: i64| Material { path: p.into(), name: p.rsplit('/').next().unwrap().into(), modified_ms: t };
        let r = rank(vec![(m("/a/old.pdf", 1), 2), (m("/a/new.pdf", 9), 1), (m("/a/newer2.pdf", 8), 2), (m("/a/x.pdf", 7), 1)], 3);
        assert_eq!(r.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["newer2.pdf", "old.pdf", "new.pdf"]);
    }
}
