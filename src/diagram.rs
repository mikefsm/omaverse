//! Sentence diagrams: a free canvas carrying the words of a passage and the
//! Reed-Kellogg strokes they sit on.
//!
//! Nothing here snaps or checks grammar. The diagram is a drawing: words have a
//! position, lines have two ends, and both are moved by hand. What the file
//! does keep is the link back to the interlinear — every placed word remembers
//! the word id it came from, so a diagram can be read against the sheet it was
//! made from even after the text has been glossed further.

use crate::interlinear::Interlinear;
use serde::{Deserialize, Serialize};

fn diagram_kind() -> String {
    "diagram".to_string()
}

/// Is this TOML a diagram rather than an interlinear? Both live in `.toml`
/// files, so the file says which it is in its first key.
pub fn is_diagram(head: &str) -> bool {
    head.lines().take(10).any(|line| {
        let line = line.trim();
        line.starts_with("kind")
            && line
                .split_once('=')
                .is_some_and(|(_, v)| v.trim().trim_matches('"').trim() == "diagram")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stroke {
    Solid,
    /// Conjunctions and the lines joining a subordinate clause to what it
    /// modifies are drawn broken.
    Dotted,
}

/// The standard strokes, offered as a palette. Each one is only a starting
/// shape: once placed, both ends can be dragged anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// The horizontal line a clause sits on.
    Base,
    /// The full vertical between subject and predicate, crossing the base.
    Divider,
    /// The shorter vertical before a direct object, which rests on the base
    /// without crossing it.
    Stop,
    /// The backslash before a predicate noun or adjective, leaning back toward
    /// the subject.
    Lean,
    /// The slant a modifier hangs on.
    Slant,
    /// The short upright that lifts a participle or infinitive off its line.
    Riser,
    /// The broken upright joining compound elements, carrying the conjunction.
    DottedDrop,
    /// The broken slant joining a subordinate clause to the word it modifies.
    DottedSlant,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::Base => "Baseline",
            Preset::Divider => "Subject divider",
            Preset::Stop => "Object stop",
            Preset::Lean => "Complement lean",
            Preset::Slant => "Modifier slant",
            Preset::Riser => "Verbal riser",
            Preset::DottedDrop => "Conjunction",
            Preset::DottedSlant => "Clause link",
        }
    }

    pub fn stroke(self) -> Stroke {
        match self {
            Preset::DottedDrop | Preset::DottedSlant => Stroke::Dotted,
            _ => Stroke::Solid,
        }
    }

    /// The shape, as offsets from the point it is dropped at.
    pub fn offsets(self) -> (f64, f64, f64, f64) {
        match self {
            Preset::Base => (-90.0, 0.0, 90.0, 0.0),
            Preset::Divider => (0.0, -26.0, 0.0, 16.0),
            Preset::Stop => (0.0, -26.0, 0.0, 0.0),
            // Up and to the left: back toward the subject.
            Preset::Lean => (8.0, 0.0, -8.0, -26.0),
            Preset::Slant => (0.0, 0.0, 24.0, 36.0),
            Preset::Riser => (0.0, 0.0, 0.0, -26.0),
            Preset::DottedDrop => (0.0, -30.0, 0.0, 30.0),
            Preset::DottedSlant => (0.0, 0.0, 28.0, 44.0),
        }
    }

    /// Every preset, in the order they are offered.
    pub fn all() -> [Preset; 8] {
        [
            Preset::Base,
            Preset::Divider,
            Preset::Stop,
            Preset::Lean,
            Preset::Slant,
            Preset::Riser,
            Preset::DottedDrop,
            Preset::DottedSlant,
        ]
    }
}

/// A word carried over from the interlinear, whether placed yet or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankWord {
    /// The interlinear's word id. The link back to the sheet.
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gloss: Option<String>,
}

/// Something written on the canvas: a word from the bank, or a free label for
/// anything the words do not cover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub id: String,
    pub text: String,
    /// The word in the bank this came from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word: Option<String>,
    /// Where the text rests: the left end of the line it sits on.
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub stroke: Stroke,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    /// Always "diagram". Interlinears and diagrams are both TOML, and this is
    /// what tells them apart without parsing the whole file.
    #[serde(default = "diagram_kind")]
    pub kind: String,
    /// The passage, as `Romans 8:4`.
    pub reference: String,
    /// The interlinear this came from, as a file name beside it where possible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Every word of the passage, in order. Those not placed make up the bank.
    #[serde(default)]
    pub words: Vec<BankWord>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_id: Option<u32>,
}

impl Diagram {
    pub fn new(reference: impl Into<String>) -> Diagram {
        Diagram {
            kind: diagram_kind(),
            reference: reference.into(),
            source: None,
            words: Vec::new(),
            labels: Vec::new(),
            lines: Vec::new(),
            next_id: None,
        }
    }

    /// Start a diagram from a sheet: the same passage, every word to hand.
    pub fn from_interlinear(sheet: &Interlinear, source: Option<String>) -> Diagram {
        let mut d = Diagram::new(sheet.reference.clone());
        d.source = source;
        d.words = sheet
            .words
            .iter()
            .map(|w| BankWord {
                id: w.id.clone(),
                text: w.text.clone(),
                gloss: w.gloss.clone(),
            })
            .collect();
        d
    }

    pub fn parse(text: &str) -> Result<Diagram, toml::de::Error> {
        let mut d: Diagram = toml::from_str(text)?;
        d.tidy();
        Ok(d)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// The words still waiting to be placed, in the order they read.
    pub fn bank(&self) -> Vec<&BankWord> {
        self.words.iter().filter(|w| !self.is_placed(&w.id)).collect()
    }

    pub fn is_placed(&self, word_id: &str) -> bool {
        self.labels.iter().any(|l| l.word.as_deref() == Some(word_id))
    }

    /// Put a word from the bank onto the canvas. A word is placed once; asking
    /// again gives back the label already there.
    pub fn place(&mut self, word_id: &str, x: f64, y: f64) -> Option<String> {
        if let Some(existing) = self
            .labels
            .iter()
            .find(|l| l.word.as_deref() == Some(word_id))
        {
            return Some(existing.id.clone());
        }
        let text = self.words.iter().find(|w| w.id == word_id)?.text.clone();
        let id = self.fresh_id();
        self.labels.push(Label {
            id: id.clone(),
            text,
            word: Some(word_id.to_string()),
            x,
            y,
        });
        self.tidy();
        Some(id)
    }

    /// Write something that is not one of the passage's words.
    pub fn add_free_label(&mut self, text: impl Into<String>, x: f64, y: f64) -> String {
        let id = self.fresh_id();
        self.labels.push(Label {
            id: id.clone(),
            text: text.into(),
            word: None,
            x,
            y,
        });
        self.tidy();
        id
    }

    pub fn add_line(&mut self, preset: Preset, x: f64, y: f64) -> String {
        let (dx1, dy1, dx2, dy2) = preset.offsets();
        let id = self.fresh_id();
        self.lines.push(Line {
            id: id.clone(),
            x1: x + dx1,
            y1: y + dy1,
            x2: x + dx2,
            y2: y + dy2,
            stroke: preset.stroke(),
        });
        self.tidy();
        id
    }

    pub fn label(&self, id: &str) -> Option<&Label> {
        self.labels.iter().find(|l| l.id == id)
    }

    pub fn label_mut(&mut self, id: &str) -> Option<&mut Label> {
        self.labels.iter_mut().find(|l| l.id == id)
    }

    pub fn line(&self, id: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.id == id)
    }

    pub fn line_mut(&mut self, id: &str) -> Option<&mut Line> {
        self.lines.iter_mut().find(|l| l.id == id)
    }

    /// Take a label off the canvas. A word returns to the bank by doing so.
    pub fn remove_label(&mut self, id: &str) -> Option<Label> {
        let at = self.labels.iter().position(|l| l.id == id)?;
        let high = self.high_water();
        let label = self.labels.remove(at);
        self.mark(high);
        Some(label)
    }

    pub fn remove_line(&mut self, id: &str) -> Option<Line> {
        let at = self.lines.iter().position(|l| l.id == id)?;
        let high = self.high_water();
        let line = self.lines.remove(at);
        self.mark(high);
        Some(line)
    }

    /// How far the numbering has got, before anything is taken away.
    fn high_water(&self) -> u32 {
        self.next_id.unwrap_or(0).max(self.implied_next())
    }

    fn mark(&mut self, high: u32) {
        self.next_id = Some(high);
        self.tidy();
    }

    /// The line nearest a point, within `tol`. Ends are preferred over middles
    /// so a handle can be grabbed where two lines meet.
    pub fn line_at(&self, x: f64, y: f64, tol: f64) -> Option<&Line> {
        self.lines
            .iter()
            .rev()
            .map(|l| (l, distance_to_segment(x, y, l.x1, l.y1, l.x2, l.y2)))
            .filter(|(_, d)| *d <= tol)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l)
    }

    /// Which end of a line a point is grabbing, if either.
    pub fn end_at(&self, line: &Line, x: f64, y: f64, tol: f64) -> Option<End> {
        let a = (x - line.x1).hypot(y - line.y1);
        let b = (x - line.x2).hypot(y - line.y2);
        if a <= tol && a <= b {
            Some(End::First)
        } else if b <= tol {
            Some(End::Second)
        } else {
            None
        }
    }

    fn implied_next(&self) -> u32 {
        let ids = self
            .labels
            .iter()
            .map(|l| l.id.as_str())
            .chain(self.lines.iter().map(|l| l.id.as_str()));
        ids.filter_map(|id| id.strip_prefix('e')?.parse::<u32>().ok())
            .max()
            .map_or(1, |n| n + 1)
    }

    fn tidy(&mut self) {
        let implied = self.implied_next();
        self.next_id = self.next_id.filter(|&n| n > implied);
    }

    /// An id never used before in this diagram, so a line and the label on it
    /// can always be told apart.
    fn fresh_id(&mut self) -> String {
        let mut n = self.next_id.unwrap_or(0).max(self.implied_next());
        loop {
            let id = format!("e{n}");
            n += 1;
            if self.label(&id).is_none() && self.line(&id).is_none() {
                self.next_id = Some(n);
                return id;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    First,
    Second,
}

/// How far a point lies from a line segment. Used for picking a line out from
/// under the pointer, so it has to be the true distance and not the distance to
/// the infinite line through it.
pub fn distance_to_segment(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (px - x1).hypot(py - y1);
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len2).clamp(0.0, 1.0);
    (px - (x1 + t * dx)).hypot(py - (y1 + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interlinear::Language;

    fn sheet() -> Interlinear {
        let mut s = Interlinear::new("John 1:1", Language::Greek);
        s.append_text("alpha beta gamma");
        s.word_mut("w1").unwrap().set_field("gloss", "first");
        s
    }

    #[test]
    fn a_diagram_starts_with_every_word_in_the_bank() {
        let d = Diagram::from_interlinear(&sheet(), Some("john_1_1.toml".into()));
        assert_eq!(d.reference, "John 1:1");
        assert_eq!(d.source.as_deref(), Some("john_1_1.toml"));
        let bank: Vec<&str> = d.bank().iter().map(|w| w.text.as_str()).collect();
        assert_eq!(bank, ["alpha", "beta", "gamma"]);
        assert_eq!(d.words[0].gloss.as_deref(), Some("first"), "the gloss comes too");
        assert!(d.labels.is_empty());
    }

    #[test]
    fn placing_a_word_takes_it_out_of_the_bank_and_putting_it_back_restores_order() {
        let mut d = Diagram::from_interlinear(&sheet(), None);
        let id = d.place("w2", 100.0, 50.0).expect("w2 is in the bank");
        let bank: Vec<&str> = d.bank().iter().map(|w| w.text.as_str()).collect();
        assert_eq!(bank, ["alpha", "gamma"]);
        assert_eq!(d.label(&id).unwrap().word.as_deref(), Some("w2"));

        d.remove_label(&id);
        let bank: Vec<&str> = d.bank().iter().map(|w| w.text.as_str()).collect();
        assert_eq!(bank, ["alpha", "beta", "gamma"], "back in its own place");
    }

    #[test]
    fn a_word_is_only_placed_once() {
        let mut d = Diagram::from_interlinear(&sheet(), None);
        let first = d.place("w1", 10.0, 10.0).unwrap();
        let again = d.place("w1", 90.0, 90.0).unwrap();
        assert_eq!(first, again, "the same label, not a second copy");
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.label(&first).unwrap().x, 10.0, "and it did not move");
    }

    #[test]
    fn a_word_that_is_not_in_the_bank_cannot_be_placed() {
        let mut d = Diagram::from_interlinear(&sheet(), None);
        assert!(d.place("w9", 0.0, 0.0).is_none());
        assert!(d.labels.is_empty());
    }

    #[test]
    fn free_labels_sit_alongside_words() {
        let mut d = Diagram::new("John 1:1");
        let id = d.add_free_label("(understood)", 20.0, 30.0);
        assert_eq!(d.label(&id).unwrap().word, None);
        assert!(d.bank().is_empty());
    }

    #[test]
    fn a_preset_lands_where_it_is_dropped() {
        let mut d = Diagram::new("John 1:1");
        let id = d.add_line(Preset::Base, 200.0, 100.0);
        let line = d.line(&id).unwrap();
        assert_eq!((line.x1, line.y1), (110.0, 100.0));
        assert_eq!((line.x2, line.y2), (290.0, 100.0));
        assert_eq!(line.stroke, Stroke::Solid);

        let id = d.add_line(Preset::DottedDrop, 50.0, 50.0);
        assert_eq!(d.line(&id).unwrap().stroke, Stroke::Dotted);
    }

    #[test]
    fn the_complement_lean_slants_back_toward_the_subject() {
        // Its foot is to the right of its head, which is what distinguishes it
        // from an ordinary modifier slant.
        let (x1, y1, x2, y2) = Preset::Lean.offsets();
        assert!(x1 > x2, "foot right of head");
        assert!(y1 > y2, "foot below head");
        let (sx1, _, sx2, sy2) = Preset::Slant.offsets();
        assert!(sx2 > sx1 && sy2 > 0.0, "a modifier slant falls away instead");
    }

    #[test]
    fn lines_and_labels_never_share_an_id() {
        let mut d = Diagram::new("John 1:1");
        let a = d.add_free_label("x", 0.0, 0.0);
        let b = d.add_line(Preset::Base, 0.0, 0.0);
        let c = d.add_free_label("y", 0.0, 0.0);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn a_removed_id_is_not_handed_out_again() {
        let mut d = Diagram::new("John 1:1");
        let a = d.add_free_label("x", 0.0, 0.0);
        d.remove_label(&a);
        let b = d.add_free_label("y", 0.0, 0.0);
        assert_ne!(a, b, "ids are not recycled");

        // And the counter survives a save, since nothing on the canvas implies
        // it any more.
        let text = d.to_toml();
        let mut back = Diagram::parse(&text).expect("should parse");
        let c = back.add_free_label("z", 0.0, 0.0);
        assert_ne!(c, b);
        assert_ne!(c, a);
    }

    #[test]
    fn distance_is_to_the_segment_not_the_line_through_it() {
        // Straight out from the middle.
        assert!((distance_to_segment(50.0, 10.0, 0.0, 0.0, 100.0, 0.0) - 10.0).abs() < 1e-9);
        // Beyond the end: measured from the end, not from the projection.
        assert!((distance_to_segment(130.0, 0.0, 0.0, 0.0, 100.0, 0.0) - 30.0).abs() < 1e-9);
        // A segment of no length is just its point.
        assert!((distance_to_segment(3.0, 4.0, 0.0, 0.0, 0.0, 0.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_pointer_picks_the_nearest_line_and_then_its_end() {
        let mut d = Diagram::new("John 1:1");
        let base = d.add_line(Preset::Base, 100.0, 100.0); // (10,100)-(190,100)
        d.add_line(Preset::Base, 100.0, 300.0);

        assert_eq!(d.line_at(100.0, 104.0, 8.0).map(|l| l.id.clone()), Some(base.clone()));
        assert!(d.line_at(100.0, 200.0, 8.0).is_none(), "nothing within reach");

        let line = d.line(&base).unwrap().clone();
        assert_eq!(d.end_at(&line, 12.0, 100.0, 10.0), Some(End::First));
        assert_eq!(d.end_at(&line, 188.0, 100.0, 10.0), Some(End::Second));
        assert_eq!(d.end_at(&line, 100.0, 100.0, 10.0), None, "the middle is not an end");
    }

    #[test]
    fn a_diagram_round_trips_through_toml() {
        let mut d = Diagram::from_interlinear(&sheet(), Some("john.toml".into()));
        d.place("w1", 40.0, 120.0);
        d.add_line(Preset::Base, 100.0, 120.0);
        d.add_line(Preset::DottedDrop, 100.0, 120.0);
        d.add_free_label("(you)", 10.0, 10.0);

        let text = d.to_toml();
        let back = Diagram::parse(&text).expect("should parse");
        assert_eq!(back, d);
        assert_eq!(back.to_toml(), text, "stable through a second pass");
    }

    #[test]
    fn a_diagram_file_announces_itself() {
        let d = Diagram::new("John 1:1");
        let text = d.to_toml();
        assert!(is_diagram(&text), "got: {text}");

        let sheet = sheet().to_toml();
        assert!(!is_diagram(&sheet), "an interlinear is not a diagram");
        assert!(!is_diagram(""), "and neither is nothing");
    }
}
