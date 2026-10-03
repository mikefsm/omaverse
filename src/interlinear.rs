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

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Anything else this sheet has chosen to record: discourse function,
    /// textual variants, whatever the passage asks for. Written out beside the
    /// four standard fields and read back the same way.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// The fields every sheet understands, whether or not it shows them.
pub const STANDARD_FIELDS: [&str; 4] = ["gloss", "lemma", "parse", "note"];

impl Word {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Word {
        Word {
            id: id.into(),
            text: text.into(),
            gloss: None,
            lemma: None,
            parse: None,
            note: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        match name {
            "gloss" => self.gloss.as_deref(),
            "lemma" => self.lemma.as_deref(),
            "parse" => self.parse.as_deref(),
            "note" => self.note.as_deref(),
            other => self.extra.get(other).map(|s| s.as_str()),
        }
    }

    pub fn set_field(&mut self, name: &str, value: &str) {
        let value = (!value.trim().is_empty()).then(|| value.trim().to_string());
        match name {
            "gloss" => self.gloss = value,
            "lemma" => self.lemma = value,
            "parse" => self.parse = value,
            "note" => self.note = value,
            other => match value {
                Some(v) => {
                    self.extra.insert(other.to_string(), v);
                }
                None => {
                    self.extra.remove(other);
                }
            },
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
    /// The next id to hand out, when it cannot be worked out from the words
    /// present. Only written once a word has been removed, since until then
    /// the highest id in the file says the same thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_id: Option<u32>,
    #[serde(default)]
    pub words: Vec<Word>,
}

impl Interlinear {
    pub fn new(reference: impl Into<String>, language: Language) -> Interlinear {
        Interlinear {
            reference: reference.into(),
            language,
            rows: default_rows(),
            next_id: None,
            words: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Interlinear, toml::de::Error> {
        let mut sheet: Interlinear = toml::from_str(text)?;
        sheet.tidy();
        Ok(sheet)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// One past the highest `wN` present, which is what the next id would be if
    /// no word had ever been removed.
    fn implied_next(&self) -> u32 {
        self.words
            .iter()
            .filter_map(|w| w.id.strip_prefix('w')?.parse::<u32>().ok())
            .max()
            .map_or(1, |n| n + 1)
    }

    /// Drop the stored counter when the words themselves already imply it, so
    /// an ordinary file carries no bookkeeping.
    fn tidy(&mut self) {
        let implied = self.implied_next();
        self.next_id = self.next_id.filter(|&n| n > implied);
    }

    pub fn word(&self, id: &str) -> Option<&Word> {
        self.words.iter().find(|w| w.id == id)
    }

    /// Every field this sheet could show: the ones it has chosen, then the
    /// standard ones it has not, so the editor offers all of them in an order
    /// that puts what the sheet cares about first.
    pub fn fields(&self) -> Vec<String> {
        let mut names = self.rows.clone();
        for standard in STANDARD_FIELDS {
            if !names.iter().any(|n| n == standard) {
                names.push(standard.to_string());
            }
        }
        // A word may carry something no row names, from a file edited by hand.
        for word in &self.words {
            for name in word.extra.keys() {
                if !names.iter().any(|n| n == name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// Change which fields are shown beneath each word. Names are trimmed, the
    /// empty ones dropped, and duplicates collapsed.
    pub fn set_rows(&mut self, names: &[String]) {
        let mut rows: Vec<String> = Vec::new();
        for n in names {
            let n = n.trim();
            if !n.is_empty() && !rows.iter().any(|r| r == n) {
                rows.push(n.to_string());
            }
        }
        self.rows = rows;
    }

    pub fn word_mut(&mut self, id: &str) -> Option<&mut Word> {
        self.words.iter_mut().find(|w| w.id == id)
    }

    /// Take a word out, handing back where it was so it can be put back.
    ///
    /// Pasted passages carry verse numbers and footnote marks that are not
    /// words at all, and they have to go.
    pub fn remove_word(&mut self, id: &str) -> Option<(usize, Word)> {
        let at = self.words.iter().position(|w| w.id == id)?;
        // Remember how far the numbering had got before the word leaves, or
        // removing the highest would free its id for reuse.
        let high = self.next_id.unwrap_or(0).max(self.implied_next());
        let word = self.words.remove(at);
        self.next_id = Some(high);
        self.tidy();
        Some((at, word))
    }

    /// Put a removed word back where it came from.
    pub fn insert_word(&mut self, at: usize, word: Word) {
        let at = at.min(self.words.len());
        self.words.insert(at, word);
        self.tidy();
    }

    /// An id never used before in this sheet.
    pub fn fresh_id(&mut self) -> String {
        let mut n = self.next_id.unwrap_or(0).max(self.implied_next());
        loop {
            let id = format!("w{n}");
            n += 1;
            // A hand-written file may use ids of its own shape; step over
            // anything already taken.
            if self.word(&id).is_none() {
                self.next_id = Some(n);
                return id;
            }
        }
    }

    /// Add the words of a pasted passage, keeping anything already here.
    pub fn append_text(&mut self, text: &str) {
        for token in tokenize(text) {
            let id = self.fresh_id();
            self.words.push(Word::new(id, token));
        }
        self.tidy();
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
    fn removing_a_word_can_be_undone_exactly() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha 4 beta");
        sheet.word_mut("w2").unwrap().set_field("note", "a verse number");

        let (at, word) = sheet.remove_word("w2").expect("the word is there");
        assert_eq!(at, 1);
        let ids: Vec<&str> = sheet.words.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w1", "w3"], "the others keep their ids");

        sheet.insert_word(at, word);
        let ids: Vec<&str> = sheet.words.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w1", "w2", "w3"], "back in its own place");
        assert_eq!(sheet.word("w2").unwrap().field("note"), Some("a verse number"));
    }

    #[test]
    fn removing_a_word_that_is_not_there_changes_nothing() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha");
        assert!(sheet.remove_word("w9").is_none());
        assert_eq!(sheet.words.len(), 1);
    }

    #[test]
    fn a_removed_id_is_not_handed_out_again() {
        // Diagrams point at words by id, so a freed id must stay free.
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta");
        sheet.remove_word("w1");
        sheet.append_text("gamma");
        let ids: Vec<&str> = sheet.words.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w2", "w3"], "w1 is gone for good");
    }

    #[test]
    fn removing_the_last_word_does_not_free_its_id() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta");
        sheet.remove_word("w2");
        // The counter has to outlive the word, so it is written down.
        let text = sheet.to_toml();
        assert!(text.contains("next_id = 3"), "got: {text}");

        let mut back = Interlinear::parse(&text).expect("should parse");
        back.append_text("gamma");
        let ids: Vec<&str> = back.words.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w1", "w3"], "across a save and reload too");
    }

    #[test]
    fn an_ordinary_sheet_carries_no_counter() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta");
        assert!(!sheet.to_toml().contains("next_id"), "nothing to record yet");
    }

    #[test]
    fn editing_a_word_keeps_its_id() {
        // Diagrams will point at words by id, so an edit must never renumber.
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha beta");
        let word = sheet.word_mut("w2").expect("the word is there");
        word.set_field("gloss", "second");
        word.text = "beta corrected".into();
        assert_eq!(sheet.words[1].id, "w2");
        assert_eq!(sheet.word("w2").unwrap().text, "beta corrected");
    }

    #[test]
    fn a_field_outside_rows_is_still_kept() {
        // The editor offers every field; `rows` only decides what is shown.
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.rows = vec!["gloss".into()];
        sheet.append_text("alpha");
        sheet.word_mut("w1").unwrap().set_field("note", "worth saying");
        let back = Interlinear::parse(&sheet.to_toml()).expect("should parse");
        assert_eq!(back.word("w1").unwrap().field("note"), Some("worth saying"));
        assert_eq!(back.rows, ["gloss"]);
    }

    #[test]
    fn a_sheet_can_name_its_own_fields() {
        let mut sheet = Interlinear::new("Romans 8:4", Language::Greek);
        sheet.set_rows(&["gloss".into(), "discourse".into(), "variant".into()]);
        sheet.append_text("alpha");
        let w = sheet.word_mut("w1").unwrap();
        w.set_field("gloss", "first");
        w.set_field("discourse", "topic shift");
        w.set_field("variant", "P46 omits");

        assert_eq!(sheet.word("w1").unwrap().field("discourse"), Some("topic shift"));
        let text = sheet.to_toml();
        assert!(text.contains("discourse = \"topic shift\""), "got: {text}");

        let back = Interlinear::parse(&text).expect("should parse");
        assert_eq!(back, sheet, "a field it invented survives the round trip");
        assert_eq!(back.rows, ["gloss", "discourse", "variant"]);
    }

    #[test]
    fn an_invented_field_is_cleared_like_any_other() {
        let mut sheet = Interlinear::new("Romans 8:4", Language::Greek);
        sheet.append_text("alpha");
        let w = sheet.word_mut("w1").unwrap();
        w.set_field("discourse", "topic shift");
        assert_eq!(w.extra.len(), 1);
        w.set_field("discourse", "   ");
        assert_eq!(w.field("discourse"), None);
        assert!(w.extra.is_empty(), "blanking removes it rather than storing nothing");
        assert!(!sheet.to_toml().contains("discourse"));
    }

    #[test]
    fn rows_are_tidied_and_the_editor_offers_the_standard_four_as_well() {
        let mut sheet = Interlinear::new("Romans 8:4", Language::Greek);
        sheet.set_rows(&[" discourse ".into(), "".into(), "discourse".into(), "gloss".into()]);
        assert_eq!(sheet.rows, ["discourse", "gloss"], "trimmed, deduped, no blanks");

        // What the sheet shows comes first; the rest are still fillable.
        assert_eq!(sheet.fields(), ["discourse", "gloss", "lemma", "parse", "note"]);
    }

    #[test]
    fn a_field_only_a_word_carries_is_still_offered() {
        // A file edited by hand can name a field no row mentions.
        let sheet = Interlinear::parse(
            "reference = \"Jude 4\"\nlanguage = \"greek\"\nrows = [\"gloss\"]\n\n             [[words]]\nid = \"w1\"\ntext = \"alpha\"\nsyntax = \"predicate\"\n",
        )
        .expect("should parse");
        assert_eq!(sheet.word("w1").unwrap().field("syntax"), Some("predicate"));
        assert!(sheet.fields().contains(&"syntax".to_string()));
    }

    #[test]
    fn a_sheet_that_names_nothing_extra_is_written_exactly_as_before() {
        let mut sheet = Interlinear::new("Jude 4", Language::Greek);
        sheet.append_text("alpha");
        sheet.word_mut("w1").unwrap().set_field("gloss", "first");
        let text = sheet.to_toml();
        assert!(!text.contains("extra"), "the map is not a key in the file");
        assert_eq!(Interlinear::parse(&text).unwrap(), sheet);
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
