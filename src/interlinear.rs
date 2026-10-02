//! Interlinears: a passage in Hebrew or Greek, one word at a time, with
//! whatever you want written underneath each one.
//!
//! Stored as TOML beside the outline rather than inside it. An outline is prose
//! with a thin skeleton; this is a record set, with a row per word and a
//! variable set of fields hanging off each. Markdown would only be a costume.
//!
//! Every word keeps an id that never changes, because a diagram refers to words
//! by id and must not be broken by editing the glosses around them.

// Nothing consumes this yet: it is the data model the interlinear editor and
// then the diagrammer are built on, written and tested first because word
// identity has to be right before anything refers to a word.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Hebrew,
    Greek,
    /// Anything else, laid out left to right.
    Other,
}

impl Language {
    /// Hebrew runs right to left, which changes the order words are laid out in
    /// and nothing else about them.
    pub fn right_to_left(self) -> bool {
        matches!(self, Language::Hebrew)
    }

    pub fn of(name: &str) -> Language {
        match name.trim().to_lowercase().as_str() {
            "hebrew" | "he" | "hbo" => Language::Hebrew,
            "greek" | "el" | "grc" => Language::Greek,
            _ => Language::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    /// Stable for the life of the word; diagrams refer to it.
    pub id: String,
    /// The word as it stands in the text.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gloss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lemma: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<String>,
    /// Your own remark on this word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Word {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Word {
        Word {
            id: id.into(),
            text: text.into(),
            gloss: None,
            lemma: None,
            parse: None,
            note: None,
        }
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        match name {
            "gloss" => self.gloss.as_deref(),
            "lemma" => self.lemma.as_deref(),
            "parse" => self.parse.as_deref(),
            "note" => self.note.as_deref(),
            _ => None,
        }
    }

    pub fn set_field(&mut self, name: &str, value: &str) {
        let value = (!value.trim().is_empty()).then(|| value.trim().to_string());
        match name {
            "gloss" => self.gloss = value,
            "lemma" => self.lemma = value,
            "parse" => self.parse = value,
            "note" => self.note = value,
            _ => {}
        }
    }
}

/// The rows shown beneath each word, in order, when a file does not say.
pub fn default_rows() -> Vec<String> {
    vec!["gloss".to_string(), "parse".to_string()]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interlinear {
    /// The passage, as `Romans 8:4` or `4`.
    pub reference: String,
    pub language: Language,
    /// Which fields to show beneath each word, in order.
    #[serde(default = "default_rows")]
    pub rows: Vec<String>,
    #[serde(default)]
    pub words: Vec<Word>,
}

impl Interlinear {
    pub fn new(reference: impl Into<String>, language: Language) -> Interlinear {
        Interlinear {
            reference: reference.into(),
            language,
            rows: default_rows(),
            words: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Interlinear, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn word(&self, id: &str) -> Option<&Word> {
        self.words.iter().find(|w| w.id == id)
    }

    pub fn word_mut(&mut self, id: &str) -> Option<&mut Word> {
        self.words.iter_mut().find(|w| w.id == id)
    }

    /// An id not already taken.
    pub fn fresh_id(&self) -> String {
        (1..)
            .map(|n| format!("w{n}"))
            .find(|id| self.word(id).is_none())
            .unwrap_or_default()
    }

    /// Add the words of a pasted passage, keeping anything already here.
    pub fn append_text(&mut self, text: &str) {
        let mut next = self.words.len() + 1;
        for token in tokenize(text) {
            while self.word(&format!("w{next}")).is_some() {
                next += 1;
            }
            self.words.push(Word::new(format!("w{next}"), token));
            next += 1;
        }
    }
}

/// Split a pasted passage into words.
///
/// Whitespace separates words, and so does the Hebrew maqqef, which joins words
/// into one accentual unit while leaving them separate words. The maqqef stays
/// attached to the word before it so the text can be put back together.
pub fn tokenize(text: &str) -> Vec<String> {
    const MAQQEF: char = '\u{05BE}';
    let mut out = Vec::new();
    for chunk in text.split_whitespace() {
        let mut current = String::new();
        for c in chunk.chars() {
            current.push(c);
            if c == MAQQEF {
                out.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_decides_direction_and_nothing_else() {
        assert!(Language::of("hebrew").right_to_left());
        assert!(!Language::of("greek").right_to_left());
        assert!(!Language::of("english").right_to_left());
        assert_eq!(Language::of("GRC"), Language::Greek);
        assert_eq!(Language::of("wordlist"), Language::Other);
    }

    #[test]
    fn a_passage_splits_into_words() {
        assert_eq!(tokenize("one two  three"), ["one", "two", "three"]);
        assert_eq!(tokenize("   "), Vec::<String>::new());
    }

    #[test]
    fn a_maqqef_separates_words_but_stays_attached() {
        // Two words joined as one accentual unit: still two words, and the
        // joiner is kept so the text can be reassembled.
        let joined = format!("alpha{}beta", '\u{05BE}');
        let words = tokenize(&joined);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0], format!("alpha{}", '\u{05BE}'));
        assert_eq!(words[1], "beta");
        assert_eq!(words.concat(), joined, "the text reassembles exactly");
    }

    #[test]
    fn pasting_gives_every_word_an_id() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta gamma");
        assert_eq!(sheet.words.len(), 3);
        assert_eq!(sheet.words[0].id, "w1");
        assert_eq!(sheet.words[2].id, "w3");
        assert_eq!(sheet.words[2].text, "gamma");
    }

    #[test]
    fn pasting_more_does_not_reuse_an_id() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta");
        sheet.append_text("gamma");
        let ids: Vec<&str> = sheet.words.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w1", "w2", "w3"]);
        assert_eq!(sheet.fresh_id(), "w4");
    }

    #[test]
    fn fields_are_set_and_read_by_name() {
        let mut w = Word::new("w1", "alpha");
        w.set_field("gloss", "  a gloss  ");
        w.set_field("parse", "noun");
        assert_eq!(w.field("gloss"), Some("a gloss"), "trimmed");
        assert_eq!(w.field("parse"), Some("noun"));
        assert_eq!(w.field("lemma"), None);
        w.set_field("gloss", "   ");
        assert_eq!(w.field("gloss"), None, "blanking clears it");
    }

    #[test]
    fn a_sheet_round_trips_through_toml() {
        let mut sheet = Interlinear::new("Romans 8:4", Language::Greek);
        sheet.append_text("alpha beta");
        sheet.word_mut("w1").unwrap().set_field("gloss", "first");
        sheet.word_mut("w1").unwrap().set_field("parse", "noun nom sg");
        sheet.word_mut("w2").unwrap().set_field("note", "my own remark");

        let text = sheet.to_toml();
        let back = Interlinear::parse(&text).expect("should parse");
        assert_eq!(back, sheet);
        assert_eq!(back.to_toml(), text, "stable through a second pass");
    }

    #[test]
    fn empty_fields_are_not_written_out() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha");
        let text = sheet.to_toml();
        // `rows` names the fields in the header, so look only at the word entry.
        let entry = &text[text.find("[[words]]").expect("a word was written")..];
        assert!(!entry.contains("gloss ="), "an unset field is simply absent");
        assert!(!entry.contains("parse ="));
        assert!(entry.contains("w1"));
    }

    #[test]
    fn rows_default_when_a_file_does_not_say() {
        let sheet = Interlinear::parse("reference = \"Jude 4\"\nlanguage = \"greek\"\n")
            .expect("should parse");
        assert_eq!(sheet.rows, ["gloss", "parse"]);
        assert!(sheet.words.is_empty());
    }

    #[test]
    fn rows_are_honoured_when_given() {
        let sheet = Interlinear::parse(
            "reference = \"Jude 4\"\nlanguage = \"hebrew\"\nrows = [\"lemma\", \"note\"]\n",
        )
        .expect("should parse");
        assert_eq!(sheet.rows, ["lemma", "note"]);
        assert!(sheet.language.right_to_left());
    }

    #[test]
    fn a_hand_written_file_is_accepted() {
        // The format has to be pleasant to edit by hand, so this shape must work.
        let sheet = Interlinear::parse(
            r#"
reference = "Jude 4"
language = "greek"

[[words]]
id = "w1"
text = "alpha"
gloss = "first"

[[words]]
id = "w2"
text = "beta"
"#,
        )
        .expect("should parse");
        assert_eq!(sheet.words.len(), 2);
        assert_eq!(sheet.word("w1").unwrap().gloss.as_deref(), Some("first"));
        assert_eq!(sheet.word("w2").unwrap().gloss, None);
    }
}
