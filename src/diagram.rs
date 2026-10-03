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

/// A line held to another line. `t` says where along the host they touch and
/// `s` says where along this line, so a divider can be held by its middle while
/// a slant is held by its head.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub host: String,
    pub t: f64,
    pub s: f64,
}

/// A word sitting on a line, at `t` along it. Its position is worked out from
/// the line rather than stored, so it cannot drift out of place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rest {
    pub host: String,
    pub t: f64,
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
    /// Where the text sits when it is not on a line: the middle of its
    /// baseline. Ignored while `rest` is set.
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Rest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub stroke: Stroke,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint: Option<Joint>,
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
        // A file can be edited by hand, and a joint is the authority on where a
        // line sits, so put everything back where its joints say.
        d.reconcile();
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
            rest: None,
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
            rest: None,
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
            joint: None,
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
        // What it held is let go where it stands rather than vanishing with it.
        // This has to happen while the line is still here, since that is the
        // only thing that knows where those words are.
        let orphans: Vec<String> = self
            .labels
            .iter()
            .filter(|l| l.rest.as_ref().is_some_and(|r| r.host == id))
            .map(|l| l.id.clone())
            .collect();
        for orphan in orphans {
            self.free_label(&orphan);
        }
        let line = self.lines.remove(at);
        for other in self.lines.iter_mut() {
            if other.joint.as_ref().is_some_and(|j| j.host == id) {
                other.joint = None;
            }
        }
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

    // ---- attachment --------------------------------------------------------

    /// Every line held, directly or not, by this one.
    pub fn descendants(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut frontier = vec![id.to_string()];
        while let Some(current) = frontier.pop() {
            for line in &self.lines {
                let held = line.joint.as_ref().is_some_and(|j| j.host == current);
                if held && !out.iter().any(|x| x == &line.id) && line.id != id {
                    out.push(line.id.clone());
                    frontier.push(line.id.clone());
                }
            }
        }
        out
    }

    /// Would holding `child` to `host` close a loop?
    pub fn would_cycle(&self, child: &str, host: &str) -> bool {
        child == host || self.descendants(child).iter().any(|d| d == host)
    }

    /// Lines in an order where a host always comes before what it holds. A
    /// joint that forms a loop is dropped rather than followed.
    fn settled_order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        let mut left: Vec<&Line> = self.lines.iter().collect();
        while !left.is_empty() {
            let before = left.len();
            left.retain(|line| {
                let ready = match &line.joint {
                    None => true,
                    Some(j) => {
                        // A joint to a line that is not here holds nothing up.
                        self.line(&j.host).is_none() || order.iter().any(|x| x == &j.host)
                    }
                };
                if ready {
                    order.push(line.id.clone());
                }
                !ready
            });
            if left.len() == before {
                // Whatever is left is in a loop; leave it where it is.
                order.extend(left.iter().map(|l| l.id.clone()));
                break;
            }
        }
        order
    }

    /// Put every jointed line back where its host says it belongs.
    pub fn reconcile(&mut self) {
        for id in self.settled_order() {
            let Some(joint) = self.line(&id).and_then(|l| l.joint.clone()) else { continue };
            let Some(host) = self.line(&joint.host) else { continue };
            let (hx, hy) = point_at(host, joint.t);
            let Some(line) = self.line(&id) else { continue };
            let (cx, cy) = point_at(line, joint.s);
            let (dx, dy) = (hx - cx, hy - cy);
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            // Only this line: what it holds comes later in the order.
            if let Some(line) = self.line_mut(&id) {
                line.x1 += dx;
                line.y1 += dy;
                line.x2 += dx;
                line.y2 += dy;
            }
        }
    }

    /// Move a line and everything hanging off it.
    pub fn shift_line(&mut self, id: &str, dx: f64, dy: f64) {
        let mut moving = self.descendants(id);
        moving.push(id.to_string());
        for id in moving {
            if let Some(line) = self.line_mut(&id) {
                line.x1 += dx;
                line.y1 += dy;
                line.x2 += dx;
                line.y2 += dy;
            }
        }
    }

    /// After a line has been dragged, hold it to whatever it came to rest
    /// against — or let it go if it came to rest against nothing.
    ///
    /// Returns where the joint was made.
    pub fn settle_line(&mut self, id: &str, tol: f64) -> Option<(f64, f64)> {
        let Some(line) = self.line(id).cloned() else { return None };
        let mut best: Option<(String, f64, f64, f64)> = None;
        for host in &self.lines {
            // Never hold a line to itself or to anything it already holds.
            if self.would_cycle(id, &host.id) {
                continue;
            }
            let (s, t, d) = closest_params(
                (line.x1, line.y1, line.x2, line.y2),
                (host.x1, host.y1, host.x2, host.y2),
            );
            if d <= tol && best.as_ref().is_none_or(|(_, _, _, bd)| d < *bd) {
                best = Some((host.id.clone(), s, t, d));
            }
        }

        match best {
            Some((host_id, s, t, _)) => {
                let host = self.line(&host_id).expect("just found").clone();
                let (hx, hy) = point_at(&host, t);
                let (cx, cy) = point_at(&line, s);
                self.shift_line(id, hx - cx, hy - cy);
                if let Some(line) = self.line_mut(id) {
                    line.joint = Some(Joint { host: host_id, t, s });
                }
                Some((hx, hy))
            }
            None => {
                if let Some(line) = self.line_mut(id) {
                    line.joint = None;
                }
                None
            }
        }
    }

    /// After a word has been dragged, sit it on the nearest line, or leave it
    /// loose if there is none near.
    pub fn settle_label(&mut self, id: &str, tol: f64) -> bool {
        let Some((x, y)) = self.label(id).map(|l| (l.x, l.y)) else { return false };
        let mut best: Option<(String, f64, f64)> = None;
        for host in &self.lines {
            let (t, d) = nearest_param(host, x, y);
            if d <= tol && best.as_ref().is_none_or(|(_, _, bd)| d < *bd) {
                best = Some((host.id.clone(), t, d));
            }
        }
        let found = best.is_some();
        if let Some(label) = self.label_mut(id) {
            label.rest = best.map(|(host, t, _)| Rest { host, t });
        }
        found
    }

    /// Where a word is drawn: the middle of its baseline, and the angle to
    /// write it at. A word on a steep line stays upright, since nothing is
    /// read sideways.
    pub fn anchor_of(&self, label: &Label) -> (f64, f64, f64) {
        let Some(rest) = &label.rest else { return (label.x, label.y, 0.0) };
        let Some(host) = self.line(&rest.host) else { return (label.x, label.y, 0.0) };
        let (x, y) = point_at(host, rest.t);
        let mut angle = (host.y2 - host.y1).atan2(host.x2 - host.x1);
        // Read it left to right whichever way the line was drawn.
        if angle.abs() > std::f64::consts::FRAC_PI_2 {
            angle -= std::f64::consts::PI * angle.signum();
        }
        if angle.abs() > UPRIGHT_ABOVE {
            angle = 0.0;
        }
        (x, y, angle)
    }

    /// Hold a word to a line directly, as when it is dropped onto one.
    pub fn set_rest(&mut self, id: &str, host: &str, t: f64) {
        if self.line(host).is_none() {
            return;
        }
        if let Some(label) = self.label_mut(id) {
            label.rest = Some(Rest { host: host.to_string(), t });
        }
    }

    /// Let a word go, leaving it where it was drawn.
    pub fn free_label(&mut self, id: &str) {
        let Some(label) = self.label(id) else { return };
        let (x, y, _) = self.anchor_of(label);
        if let Some(label) = self.label_mut(id) {
            label.x = x;
            label.y = y;
            label.rest = None;
        }
    }

    /// The dots where things are held together.
    pub fn joints(&self) -> Vec<(f64, f64)> {
        self.lines
            .iter()
            .filter_map(|line| {
                let joint = line.joint.as_ref()?;
                let host = self.line(&joint.host)?;
                Some(point_at(host, joint.t))
            })
            .collect()
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

/// Beyond this angle a word is written upright rather than along its line:
/// nothing is read sideways, so a word on the subject divider stays level.
const UPRIGHT_ABOVE: f64 = 1.22; // ~70 degrees

/// A point along a line, by parameter.
pub fn point_at(line: &Line, t: f64) -> (f64, f64) {
    (
        line.x1 + (line.x2 - line.x1) * t,
        line.y1 + (line.y2 - line.y1) * t,
    )
}

/// Where along a line a point comes closest to it, and how far away it is.
pub fn nearest_param(line: &Line, px: f64, py: f64) -> (f64, f64) {
    let dx = line.x2 - line.x1;
    let dy = line.y2 - line.y1;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (0.0, (px - line.x1).hypot(py - line.y1));
    }
    let t = (((px - line.x1) * dx + (py - line.y1) * dy) / len2).clamp(0.0, 1.0);
    let (x, y) = (line.x1 + t * dx, line.y1 + t * dy);
    (t, (px - x).hypot(py - y))
}

/// Where two segments come closest, as a parameter along each and the distance
/// between those points. This is what lets a divider be held by its middle
/// while a slant is held by its head — the touching point is found, not assumed
/// to be an end.
pub fn closest_params(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64) {
    const EPS: f64 = 1e-9;
    let (ax1, ay1, ax2, ay2) = a;
    let (bx1, by1, bx2, by2) = b;
    let (d1x, d1y) = (ax2 - ax1, ay2 - ay1);
    let (d2x, d2y) = (bx2 - bx1, by2 - by1);
    let (rx, ry) = (ax1 - bx1, ay1 - by1);
    let aa = d1x * d1x + d1y * d1y;
    let e = d2x * d2x + d2y * d2y;
    let f = d2x * rx + d2y * ry;

    let (s, t) = if aa <= EPS && e <= EPS {
        (0.0, 0.0)
    } else if aa <= EPS {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1x * rx + d1y * ry;
        if e <= EPS {
            ((-c / aa).clamp(0.0, 1.0), 0.0)
        } else {
            let b_ = d1x * d2x + d1y * d2y;
            let denom = aa * e - b_ * b_;
            let mut s = if denom.abs() > EPS {
                ((b_ * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b_ * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / aa).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b_ - c) / aa).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    let (px, py) = (ax1 + d1x * s, ay1 + d1y * s);
    let (qx, qy) = (bx1 + d2x * t, by1 + d2y * t);
    (s, t, (px - qx).hypot(py - qy))
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

    /// A baseline with a divider across it and a slant hanging beneath.
    fn clause() -> (Diagram, String, String, String) {
        let mut d = Diagram::new("John 1:1");
        let base = d.add_line(Preset::Base, 200.0, 200.0); // (110,200)-(290,200)
        let divider = d.add_line(Preset::Divider, 180.0, 200.0);
        let slant = d.add_line(Preset::Slant, 140.0, 200.0);
        d.settle_line(&divider, 10.0);
        d.settle_line(&slant, 10.0);
        (d, base, divider, slant)
    }

    #[test]
    fn a_stroke_dropped_against_another_is_held_by_it() {
        let (d, base, divider, slant) = clause();
        assert_eq!(d.line(&divider).unwrap().joint.as_ref().unwrap().host, base);
        assert_eq!(d.line(&slant).unwrap().joint.as_ref().unwrap().host, base);

        // The divider crosses the baseline, so it is held by its middle; the
        // slant hangs from it, so it is held by its head.
        let across = d.line(&divider).unwrap().joint.as_ref().unwrap().s;
        assert!((0.3..0.8).contains(&across), "held mid-stroke, got {across}");
        assert_eq!(d.line(&slant).unwrap().joint.as_ref().unwrap().s, 0.0);
        assert_eq!(d.joints().len(), 2, "a dot for each");
    }

    #[test]
    fn a_stroke_too_far_from_anything_is_held_by_nothing() {
        let mut d = Diagram::new("John 1:1");
        d.add_line(Preset::Base, 200.0, 200.0);
        let loose = d.add_line(Preset::Slant, 200.0, 600.0);
        assert_eq!(d.settle_line(&loose, 10.0), None);
        assert!(d.line(&loose).unwrap().joint.is_none());
        assert!(d.joints().is_empty());
    }

    #[test]
    fn moving_a_host_carries_everything_it_holds() {
        let (mut d, base, divider, slant) = clause();
        let before = d.line(&slant).unwrap().clone();
        d.shift_line(&base, 40.0, -15.0);

        let after = d.line(&slant).unwrap();
        assert_eq!((after.x1 - before.x1, after.y1 - before.y1), (40.0, -15.0));
        assert_eq!(d.line(&divider).unwrap().x1, 180.0 + 40.0);
        // Still touching, exactly.
        let host = d.line(&base).unwrap();
        let joint = d.line(&slant).unwrap().joint.clone().unwrap();
        assert_eq!(point_at(host, joint.t), point_at(d.line(&slant).unwrap(), joint.s));
    }

    #[test]
    fn moving_what_is_held_slides_it_along_and_leaves_the_rest() {
        let (mut d, base, _divider, slant) = clause();
        let base_before = d.line(&base).unwrap().clone();
        let t_before = d.line(&slant).unwrap().joint.as_ref().unwrap().t;

        d.shift_line(&slant, 60.0, 3.0);
        d.settle_line(&slant, 10.0);

        assert_eq!(d.line(&base).unwrap(), &base_before, "the baseline stayed put");
        let joint = d.line(&slant).unwrap().joint.clone().unwrap();
        assert_eq!(joint.host, base, "still attached");
        assert!(joint.t > t_before, "and slid along it");
        assert_eq!(
            point_at(d.line(&base).unwrap(), joint.t),
            point_at(d.line(&slant).unwrap(), joint.s)
        );
    }

    #[test]
    fn dragging_what_is_held_right_away_lets_it_go() {
        let (mut d, _base, _divider, slant) = clause();
        d.shift_line(&slant, 0.0, 400.0);
        d.settle_line(&slant, 10.0);
        assert!(d.line(&slant).unwrap().joint.is_none());
    }

    #[test]
    fn a_chain_moves_from_the_top() {
        // baseline <- slant <- a second slant hanging off the first
        let (mut d, base, _divider, slant) = clause();
        let hanging = {
            let head = point_at(d.line(&slant).unwrap(), 1.0);
            let id = d.add_line(Preset::Slant, head.0, head.1);
            d.settle_line(&id, 10.0);
            id
        };
        assert_eq!(d.line(&hanging).unwrap().joint.as_ref().unwrap().host, slant);

        let before = d.line(&hanging).unwrap().clone();
        d.shift_line(&base, 25.0, 0.0);
        assert_eq!(d.line(&hanging).unwrap().x1 - before.x1, 25.0, "the chain came too");
    }

    #[test]
    fn a_loop_cannot_be_made() {
        let (d, base, _divider, slant) = clause();
        assert!(d.would_cycle(&base, &slant), "the baseline already holds the slant");
        assert!(d.would_cycle(&base, &base));
        assert!(!d.would_cycle(&slant, &base));
    }

    #[test]
    fn a_word_dropped_on_a_stroke_sits_on_it_and_travels_with_it() {
        let (mut d, base, _divider, _slant) = clause();
        d.words.push(BankWord { id: "w1".into(), text: "alpha".into(), gloss: None });
        let label = d.place("w1", 150.0, 203.0).unwrap();
        assert!(d.settle_label(&label, 10.0));

        let (x, y, angle) = d.anchor_of(d.label(&label).unwrap());
        assert_eq!(y, 200.0, "pulled onto the line");
        assert!((x - 150.0).abs() < 0.001, "and left where it was along it");
        assert_eq!(angle, 0.0);

        d.shift_line(&base, 0.0, 30.0);
        let (_, y, _) = d.anchor_of(d.label(&label).unwrap());
        assert_eq!(y, 230.0, "it moved with the line");
    }

    #[test]
    fn a_word_on_a_slant_is_written_along_it_but_never_sideways() {
        let mut d = Diagram::new("John 1:1");
        let slant = d.add_line(Preset::Slant, 100.0, 100.0);
        let upright = d.add_line(Preset::Divider, 400.0, 100.0);
        let on_slant = d.add_free_label("mod", 0.0, 0.0);
        let on_upright = d.add_free_label("and", 0.0, 0.0);
        d.set_rest(&on_slant, &slant, 0.5);
        d.set_rest(&on_upright, &upright, 0.5);

        let (_, _, a) = d.anchor_of(d.label(&on_slant).unwrap());
        assert!(a > 0.5 && a < 1.2, "written along the slant, got {a}");
        let (_, _, b) = d.anchor_of(d.label(&on_upright).unwrap());
        assert_eq!(b, 0.0, "but a steep line keeps its word level");
    }

    #[test]
    fn removing_a_stroke_lets_go_of_what_it_held_without_taking_it_away() {
        let (mut d, base, divider, slant) = clause();
        let label = d.add_free_label("alpha", 0.0, 0.0);
        d.set_rest(&label, &base, 0.25);
        let (x, y, _) = d.anchor_of(d.label(&label).unwrap());

        d.remove_line(&base);
        assert!(d.line(&divider).unwrap().joint.is_none());
        assert!(d.line(&slant).unwrap().joint.is_none());
        let still = d.label(&label).unwrap();
        assert!(still.rest.is_none());
        assert_eq!((still.x, still.y), (x, y), "the word stayed where it was");
    }

    #[test]
    fn joints_survive_a_save_and_a_hand_edit() {
        let (mut d, base, _divider, slant) = clause();
        let text = d.to_toml();
        let back = Diagram::parse(&text).expect("should parse");
        assert_eq!(back, d);

        // Someone moves the baseline in a text editor; the rest follows on load.
        if let Some(line) = d.line_mut(&base) {
            line.y1 += 100.0;
            line.y2 += 100.0;
        }
        let moved = Diagram::parse(&d.to_toml()).expect("should parse");
        let joint = moved.line(&slant).unwrap().joint.clone().unwrap();
        assert_eq!(
            point_at(moved.line(&base).unwrap(), joint.t),
            point_at(moved.line(&slant).unwrap(), joint.s),
            "put back against its host"
        );
    }

    #[test]
    fn two_segments_meet_where_they_are_nearest() {
        // A cross: nearest points are both mid-segment.
        let (s, t, d) = closest_params((0.0, 10.0, 20.0, 10.0), (10.0, 0.0, 10.0, 20.0));
        assert!((s - 0.5).abs() < 1e-9);
        assert!((t - 0.5).abs() < 1e-9);
        assert!(d < 1e-9);

        // Parallel and apart: distance is the gap.
        let (_, _, d) = closest_params((0.0, 0.0, 10.0, 0.0), (0.0, 5.0, 10.0, 5.0));
        assert!((d - 5.0).abs() < 1e-9);

        // End to end.
        let (s, t, d) = closest_params((0.0, 0.0, 10.0, 0.0), (13.0, 0.0, 23.0, 0.0));
        assert!((s - 1.0).abs() < 1e-9);
        assert!(t.abs() < 1e-9);
        assert!((d - 3.0).abs() < 1e-9);
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
