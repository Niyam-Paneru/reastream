//! Paths through a JSON document, and the rules that select values in them.
//!
//! A path is a sequence of object keys and array positions. Any position may be
//! `AnyIndex`, which matches whatever index actually appears there; that is what
//! makes `nodes[].node_id` expressible without knowing how many nodes exist.

use std::fmt;

/// One step down a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// An object member name.
    Key(String),
    /// Any array index at this level.
    AnyIndex,
    /// A particular array index.
    Index(usize),
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Segment::Key(key) => f.write_str(key),
            Segment::AnyIndex => f.write_str("[*]"),
            Segment::Index(index) => write!(f, "[{index}]"),
        }
    }
}

/// A path from the document root to a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Segment>,
}

impl Path {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// A path from key names, with no array steps.
    pub fn keys<K: AsRef<str>>(keys: impl IntoIterator<Item = K>) -> Self {
        Self {
            segments: keys
                .into_iter()
                .map(|key| Segment::Key(key.as_ref().to_string()))
                .collect(),
        }
    }

    /// A single object member.
    pub fn key<K: Into<String>>(key: K) -> Self {
        Self {
            segments: vec![Segment::Key(key.into())],
        }
    }

    pub fn push_key<K: Into<String>>(&mut self, key: K) {
        self.segments.push(Segment::Key(key.into()));
    }

    pub fn push_index(&mut self, index: usize) {
        self.segments.push(Segment::Index(index));
    }

    pub fn pop(&mut self) {
        self.segments.pop();
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Whether this pattern matches `path`, step by step.
    ///
    /// `self` is the pattern and `path` is the value's actual path, so a segment
    /// on `self` decides what to accept: a name must match, `AnyIndex` accepts
    /// any index, and a numbered index must match exactly.
    ///
    /// A pattern shorter than the path matches a prefix, so a rule aimed at
    /// `nodes[*]` also matches `nodes[*].identity` while looking for a field
    /// beneath it.
    pub fn matches(&self, path: &Path) -> bool {
        if self.segments.len() > path.segments.len() {
            return false;
        }
        self.segments
            .iter()
            .zip(path.segments.iter())
            .all(|(expected, actual)| match expected {
                Segment::Key(key) => actual == &Segment::Key(key.clone()),
                Segment::AnyIndex => matches!(actual, Segment::Index(_)),
                Segment::Index(index) => actual == &Segment::Index(*index),
            })
    }

    /// Render with dots, as in `normalized_result.graph.nodes[*].node_id`.
    pub fn dotted(&self) -> String {
        let mut out = String::new();
        for (position, segment) in self.segments.iter().enumerate() {
            match segment {
                Segment::Key(key) => {
                    if position > 0 {
                        out.push('.');
                    }
                    out.push_str(key);
                }
                Segment::AnyIndex => out.push_str("[*]"),
                Segment::Index(index) => out.push_str(&format!("[{index}]")),
            }
        }
        out
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.dotted())
    }
}

/// A single value pulled from a matched object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    /// Key the value is reported under.
    pub as_name: String,
    /// Path relative to the matched value. Empty captures the value itself.
    pub from: Path,
    /// What to do when the field is absent.
    pub on_missing: Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Report the field as JSON `null`.
    Null,
    /// Omit the field from the record entirely.
    Skip,
}

/// Where to look and what to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Path of the value to read fields from.
    pub at: Path,
    pub fields: Vec<FieldSpec>,
}

impl Rule {
    /// A rule reading named fields out of each value at `at`.
    pub fn fields(at: Path, fields: Vec<FieldSpec>) -> Self {
        Self { at, fields }
    }
}

/// Build a [`FieldSpec`] from a dotted path.
pub fn field(name: &str, from: &str) -> FieldSpec {
    FieldSpec {
        as_name: name.to_string(),
        from: parse_dotted(from),
        on_missing: Missing::Null,
    }
}

/// Parse `a.b[*].c` into a [`Path`].
///
/// A bracketed part may be attached to the name it indexes, as in `nodes[*]`,
/// which is the same as `nodes[*]`. Splitting on `.` alone would leave
/// `nodes[*]` as a single name, so the brackets are peeled off first.
pub fn parse_dotted(path: &str) -> Path {
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        // Peel a leading `name[` off, so `name[*]` yields both segments.
        let (name, brackets) = match part.find('[') {
            Some(open) => (Some(&part[..open]), &part[open..]),
            None => (Some(part), ""),
        };
        if let Some(name) = name {
            if !name.is_empty() {
                segments.push(Segment::Key(name.to_string()));
            }
        }
        for piece in brackets.split(']') {
            let piece = piece.trim_start_matches('[');
            match piece {
                "" => {}
                "*" => segments.push(Segment::AnyIndex),
                digits => match digits.parse::<usize>() {
                    Ok(index) => segments.push(Segment::Index(index)),
                    Err(_) => segments.push(Segment::Key(format!("[{digits}]"))),
                },
            }
        }
    }
    Path { segments }
}
