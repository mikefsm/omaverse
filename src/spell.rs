//! Spell checking against the system dictionary.
//!
//! Deliberately not libspelling: that needs GtkSourceView, which would mean
//! swapping the document's widget and buffer for GtkSourceView's own. Those
//! bring their own undo implementation and their own gutter -- the two pieces
//! most recently and most delicately got working here. A misspelling is just
//! another tag, which is a thing this already knows how to do.

use std::cell::RefCell;
use std::collections::HashMap;

pub struct Speller {
    dict: RefCell<enchant::Dict>,
    /// Looked-up words, so retyping a paragraph does not re-check every word on
    /// every restyle.
    seen: RefCell<HashMap<String, bool>>,
}

impl Speller {
    pub fn new() -> Option<Speller> {
        let mut broker = enchant::Broker::new();
        let dict = broker
            .request_dict("en_US")
            .or_else(|_| broker.request_dict("en"))
            .ok()?;
        Some(Speller { dict: RefCell::new(dict), seen: RefCell::new(HashMap::new()) })
    }

    pub fn is_correct(&self, word: &str) -> bool {
        if let Some(known) = self.seen.borrow().get(word) {
            return *known;
        }
        let ok = self.dict.borrow_mut().check(word).unwrap_or(true);
        self.seen.borrow_mut().insert(word.to_string(), ok);
        ok
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        self.dict.borrow_mut().suggest(word).into_iter().take(8).collect()
    }

    /// Teach the dictionary a word for good. Biblical names are not in a
    /// general dictionary, and without this the marking is noise.
    pub fn learn(&self, word: &str) {
        self.dict.borrow_mut().add(word);
        self.seen.borrow_mut().insert(word.to_string(), true);
    }
}

/// Words in a line worth checking, as character ranges.
///
/// Anything carrying a digit is skipped, which covers verse references like
/// `1:1-2:3`, and so is anything inside a note anchor's markup.
pub fn words(line: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let skip = word.chars().any(|c| c.is_ascii_digit())
            || word.chars().all(|c| !c.is_alphabetic())
            || word.len() < 2;
        if !skip {
            out.push((start, i));
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        words(line)
            .into_iter()
            .map(|(a, b)| chars[a..b].iter().collect())
            .collect()
    }

    #[test]
    fn ordinary_words_are_picked_out() {
        assert_eq!(picked("Paul calls himself a servant"), ["Paul", "calls", "himself", "servant"]);
    }

    #[test]
    fn apostrophes_stay_inside_a_word() {
        assert_eq!(picked("God's own people"), ["God's", "own", "people"]);
        assert_eq!(picked("God\u{2019}s own"), ["God\u{2019}s", "own"]);
    }

    #[test]
    fn verse_references_are_not_words() {
        assert!(picked("1:1-2:3").is_empty());
        assert_eq!(picked("Introduction 1:1-17"), ["Introduction"]);
    }

    #[test]
    fn markup_characters_do_not_join_words() {
        assert_eq!(picked("a **bold** word"), ["bold", "word"]);
        assert_eq!(picked("==servant==[^n1] first"), ["servant", "first"]);
    }

    #[test]
    fn single_letters_and_punctuation_are_skipped() {
        assert!(picked("a I - \u{2014} .").is_empty());
    }

    #[test]
    fn offsets_count_characters() {
        let line = "\u{3b4}\u{3bf}\u{1fe6}\u{3bb}\u{3bf}\u{3c2} \u{2014} servant";
        let chars: Vec<char> = line.chars().collect();
        let last = *words(line).last().unwrap();
        let text: String = chars[last.0..last.1].iter().collect();
        assert_eq!(text, "servant");
    }
}
