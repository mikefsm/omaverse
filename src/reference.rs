//! Scripture references, which are how documents find each other.
//!
//! An outline of Romans, an interlinear of 8:4 and a diagram of one sentence in
//! it are three separate files. What relates them is not their names, which
//! drift, but the passage each one covers. A reference that can be parsed,
//! compared and tested for overlap turns "what else do I have on this verse"
//! into a question with an answer.

use std::fmt;

/// A chapter and verse. Single-chapter books use chapter 1 throughout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Verse {
    pub chapter: u16,
    pub verse: u16,
}

impl Verse {
    pub fn new(chapter: u16, verse: u16) -> Self {
        Verse { chapter, verse }
    }
}

/// A span of text: a book, and a range within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// None when the book is implied by the document it sits in.
    pub book: Option<String>,
    pub start: Verse,
    pub end: Verse,
}

impl Reference {
    /// Parse `1:1-2:3`, `8:1-17`, `4`, `4-6`, or any of those with a book in
    /// front. A bare number is a verse in a single-chapter book, which is how
    /// Jude, Philemon, 2 and 3 John and Obadiah are cited.
    pub fn parse(text: &str) -> Option<Reference> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (book, rest) = split_book(text);
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }

        let (left, right) = match rest.split_once('-') {
            Some((l, r)) => (l.trim(), Some(r.trim())),
            None => (rest, None),
        };

        let start = parse_verse(left)?;
        let end = match right {
            None => start,
            Some(r) => match parse_verse(r) {
                // `8:1-17` -- the right side is a bare number, which
                // parse_verse returns as verse-of-chapter-1, so take its verse.
                Some(v) if !r.contains(':') => Verse::new(start.chapter, v.verse),
                Some(v) => v,
                None => return None,
            },
        };
        if end < start {
            return None;
        }
        Some(Reference { book, start, end })
    }

    /// True when the two cover any of the same ground. Only comparable when the
    /// books agree; a reference with no book takes the other's.
    pub fn overlaps(&self, other: &Reference) -> bool {
        if !books_agree(self.book.as_deref(), other.book.as_deref()) {
            return false;
        }
        self.start <= other.end && other.start <= self.end
    }

    /// True when `other` falls entirely inside this one.
    pub fn contains(&self, other: &Reference) -> bool {
        books_agree(self.book.as_deref(), other.book.as_deref())
            && self.start <= other.start
            && other.end <= self.end
    }

    /// The range without the book, as it would be written inside a document
    /// about that book.
    pub fn range(&self) -> String {
        let single = self.start.chapter == self.end.chapter;
        match (single, self.start == self.end) {
            (_, true) => format!("{}:{}", self.start.chapter, self.start.verse),
            (true, _) => format!(
                "{}:{}-{}",
                self.start.chapter, self.start.verse, self.end.verse
            ),
            _ => format!(
                "{}:{}-{}:{}",
                self.start.chapter, self.start.verse, self.end.chapter, self.end.verse
            ),
        }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.book {
            Some(book) => write!(f, "{book} {}", self.range()),
            None => write!(f, "{}", self.range()),
        }
    }
}

/// Split a leading book name off the front. A book name may begin with a digit
/// (`1 Peter`, `2 John`), so a leading number only counts as a book when a word
/// follows it.
fn split_book(text: &str) -> (Option<String>, &str) {
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    // An optional leading ordinal.
    if bytes.first().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        let mut j = 0;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        // `1:1` is a chapter, not a book.
        if bytes.get(j) == Some(&':') || j == bytes.len() {
            return (None, text);
        }
        i = j;
    }
    while i < bytes.len() && (bytes[i].is_alphabetic() || bytes[i] == ' ' || bytes[i] == '.') {
        i += 1;
    }
    let name: String = bytes[..i].iter().collect();
    let name = name.trim();
    if name.is_empty() || !name.chars().any(|c| c.is_alphabetic()) {
        return (None, text);
    }
    let rest: String = bytes[i..].iter().collect();
    // Keep the remainder borrowed from the original for cheapness.
    let offset = text.len() - rest.len();
    (Some(normalise_book(name)), &text[offset..])
}

fn parse_verse(text: &str) -> Option<Verse> {
    match text.split_once(':') {
        Some((c, v)) => Some(Verse::new(c.trim().parse().ok()?, v.trim().parse().ok()?)),
        // A bare number: a verse in a single-chapter book.
        None => Some(Verse::new(1, text.trim().parse().ok()?)),
    }
}

fn books_agree(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        // One side unqualified means "the book this document is about".
        _ => true,
    }
}

/// Fold common abbreviations onto one spelling so `Rom`, `Rom.` and `Romans`
/// compare equal. Unknown names are kept as written rather than guessed at.
fn normalise_book(name: &str) -> String {
    let key: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    const BOOKS: &[(&str, &str)] = &[
        ("gen", "Genesis"), ("genesis", "Genesis"),
        ("exod", "Exodus"), ("ex", "Exodus"), ("exodus", "Exodus"),
        ("ps", "Psalms"), ("psa", "Psalms"), ("psalm", "Psalms"), ("psalms", "Psalms"),
        ("isa", "Isaiah"), ("isaiah", "Isaiah"),
        ("matt", "Matthew"), ("mt", "Matthew"), ("matthew", "Matthew"),
        ("mark", "Mark"), ("mk", "Mark"),
        ("luke", "Luke"), ("lk", "Luke"),
        ("john", "John"), ("jn", "John"),
        ("acts", "Acts"),
        ("rom", "Romans"), ("romans", "Romans"),
        ("1cor", "1 Corinthians"), ("2cor", "2 Corinthians"),
        ("gal", "Galatians"), ("galatians", "Galatians"),
        ("eph", "Ephesians"), ("ephesians", "Ephesians"),
        ("phil", "Philippians"), ("philippians", "Philippians"),
        ("col", "Colossians"), ("colossians", "Colossians"),
        ("heb", "Hebrews"), ("hebrews", "Hebrews"),
        ("jas", "James"), ("james", "James"),
        ("1pet", "1 Peter"), ("2pet", "2 Peter"),
        ("1john", "1 John"), ("2john", "2 John"), ("3john", "3 John"),
        ("jude", "Jude"),
        ("rev", "Revelation"), ("revelation", "Revelation"),
    ];
    BOOKS
        .iter()
        .find(|(abbr, _)| *abbr == key)
        .map(|(_, full)| full.to_string())
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(text: &str) -> Reference {
        Reference::parse(text).unwrap_or_else(|| panic!("should parse: {text:?}"))
    }

    #[test]
    fn a_chapter_and_verse_range() {
        let v = r("1:1-2:3");
        assert_eq!(v.book, None);
        assert_eq!(v.start, Verse::new(1, 1));
        assert_eq!(v.end, Verse::new(2, 3));
    }

    #[test]
    fn a_range_inside_one_chapter() {
        // The right-hand number is a verse, not a chapter.
        let v = r("8:1-17");
        assert_eq!(v.start, Verse::new(8, 1));
        assert_eq!(v.end, Verse::new(8, 17));
    }

    #[test]
    fn a_single_verse() {
        let v = r("1:1");
        assert_eq!(v.start, v.end);
        assert_eq!(v.start, Verse::new(1, 1));
    }

    #[test]
    fn a_bare_number_is_a_verse_in_a_one_chapter_book() {
        // How Jude, Philemon and 3 John are cited.
        let v = r("4");
        assert_eq!(v.start, Verse::new(1, 4));
        assert_eq!(v.end, Verse::new(1, 4));
        assert_eq!(r("4-6").end, Verse::new(1, 6));
    }

    #[test]
    fn a_book_in_front_is_recognised() {
        assert_eq!(r("Romans 8:1-17").book.as_deref(), Some("Romans"));
        assert_eq!(r("Rom 8:1").book.as_deref(), Some("Romans"));
        assert_eq!(r("Rom. 8:1").book.as_deref(), Some("Romans"));
    }

    #[test]
    fn books_beginning_with_a_number_still_parse() {
        assert_eq!(r("3 John 4").book.as_deref(), Some("3 John"));
        assert_eq!(r("1 Peter 2:9").book.as_deref(), Some("1 Peter"));
        assert_eq!(r("1 Peter 2:9").start, Verse::new(2, 9));
    }

    #[test]
    fn a_leading_chapter_is_not_mistaken_for_a_book() {
        assert_eq!(r("1:1").book, None);
        assert_eq!(r("12:3").book, None);
    }

    #[test]
    fn an_unknown_book_is_kept_as_written() {
        assert_eq!(r("Enoch 1:1").book.as_deref(), Some("Enoch"));
    }

    #[test]
    fn nonsense_does_not_parse() {
        assert!(Reference::parse("").is_none());
        assert!(Reference::parse("not a reference").is_none());
        assert!(Reference::parse("1:").is_none());
        assert!(Reference::parse("2:3-1:1").is_none(), "backwards range");
    }

    #[test]
    fn overlap_is_what_links_documents() {
        let outline = r("Romans 8:1-17");
        assert!(outline.overlaps(&r("Romans 8:4")), "a verse inside it");
        assert!(outline.overlaps(&r("Romans 8:17-25")), "an overlapping range");
        assert!(!outline.overlaps(&r("Romans 9:1")), "past the end");
        assert!(!outline.overlaps(&r("Galatians 8:4")), "a different book");
    }

    #[test]
    fn an_unqualified_range_takes_the_other_book() {
        // Inside a document about Romans, sections are written as bare ranges.
        assert!(r("Romans 8:1-17").overlaps(&r("8:4")));
        assert!(r("8:1-17").contains(&r("8:4")));
    }

    #[test]
    fn containment_is_stricter_than_overlap() {
        let section = r("1:1-2:3");
        assert!(section.contains(&r("1:5")));
        assert!(!section.contains(&r("2:1-3:1")));
        assert!(section.overlaps(&r("2:1-3:1")));
    }

    #[test]
    fn a_reference_writes_back_the_way_it_is_cited() {
        assert_eq!(r("1:1-2:3").to_string(), "1:1-2:3");
        assert_eq!(r("8:1-17").to_string(), "8:1-17");
        assert_eq!(r("1:1").to_string(), "1:1");
        assert_eq!(r("Romans 8:1-17").to_string(), "Romans 8:1-17");
        assert_eq!(r("Rom 8:1").to_string(), "Romans 8:1");
    }

    #[test]
    fn round_trips_through_its_own_output() {
        for text in ["1:1-2:3", "8:1-17", "Romans 8:4", "3 John 4", "1 Peter 2:9-10"] {
            let once = r(text).to_string();
            assert_eq!(r(&once).to_string(), once, "not stable for {text:?}");
        }
    }
}
