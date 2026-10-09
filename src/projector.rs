//! Projection: walk a token stream once, emit only what was asked for.
//!
//! A REA application result is a semantic graph that accounts for essentially
//! all of its length. What a caller wants is one small record per node. This
//! module walks the stream, keeps the fields a rule names, and drops the rest.
//!
//! The consequence is the point: peak memory is the size of the largest single
//! value, not of the document. A rule keeping `node_id`, `kind`,
//! `identity.namespace`, `identity.key`, `semantic_role`, `properties.label`
//! and `state` collects kilobytes whether the document is 4 MB or 4 GB.
//!
//! ## How a record's lifetime is decided
//!
//! Each open container gets a stack frame carrying the path of that container.
//! When a rule matches the value at a path, a collector is opened and remembers
//! the depth of the frame that value lived in. That collector closes when a
//! `Close` returns to that same depth, which is exactly the token that ended the
//! value. Nothing is deferred, nothing is searched for, and a collector's
//! lifetime cannot outlive the value it belongs to.

use crate::lexer::{Token, TokenKind};
use crate::path::{Missing, Path, Rule, Segment};

/// One extracted record.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Index into the rule list that produced it.
    pub rule: usize,
    /// Array index the value came from, when the rule's path named one.
    pub index: Option<usize>,
    /// Present fields, in the order the rule declared them.
    pub fields: Vec<(String, String)>,
    /// Fields the rule declared that were absent, when they are omitted.
    pub missing: Vec<String>,
}

/// Why the projector rejected its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionError {
    EmptyPath(&'static str),
    EmptyFieldName(&'static str),
    DuplicateFieldName,
}

/// One open container.
#[derive(Debug, Clone)]
struct Frame {
    /// Path of this container's own value.
    path: Path,
    kind: TokenKind,
    /// Next index to hand an element of this array.
    next_index: usize,
}

/// A rule collecting one value.
#[derive(Debug, Clone)]
struct Collector {
    rule: usize,
    base: Path,
    values: Vec<Option<String>>,
    index: Option<usize>,
    /// Depth of the frame that holds the collected value.
    depth: usize,
}

/// Walks a lexer's tokens and applies rules to them.
#[derive(Debug)]
pub struct Projector<'a> {
    rules: &'a [Rule],
    /// Open containers, innermost last.
    stack: Vec<Frame>,
    /// Path of the value the next token belongs to.
    working: Path,
    active: Vec<Collector>,
    /// Records produced, in document order.
    pub records: Vec<Record>,
    /// Tokens walked.
    pub walked: usize,
}

impl<'a> Projector<'a> {
    pub fn new(rules: &'a [Rule]) -> Result<Self, ProjectionError> {
        for rule in rules {
            if rule.at.is_empty() {
                return Err(ProjectionError::EmptyPath("rule path must not be empty"));
            }
            let mut names = Vec::new();
            for spec in &rule.fields {
                if spec.as_name.is_empty() {
                    return Err(ProjectionError::EmptyFieldName(
                        "field name must not be empty",
                    ));
                }
                if names.contains(&spec.as_name) {
                    return Err(ProjectionError::DuplicateFieldName);
                }
                names.push(spec.as_name.clone());
            }
        }
        Ok(Self {
            rules,
            stack: Vec::new(),
            working: Path::new(),
            active: Vec::new(),
            records: Vec::new(),
            walked: 0,
        })
    }

    /// Consume one token.
    pub fn push(&mut self, token: &Token) {
        self.walked += 1;
        match token {
            Token::Open(kind) => {
                let value_path = self.working.clone();
                self.maybe_start_collector(&value_path);
                self.stack.push(Frame {
                    path: value_path.clone(),
                    kind: *kind,
                    next_index: 0,
                });
                self.working = value_path;
                if *kind == TokenKind::Array {
                    self.working.segments.push(Segment::Index(0));
                }
            }
            Token::Close(_) => {
                if self.stack.pop().is_none() {
                    return;
                }
                // Any collector held at this depth has had its value closed.
                let depth = self.stack.len();
                self.complete_at(depth);
                self.working = match self.stack.last() {
                    Some(parent) => {
                        let mut path = parent.path.clone();
                        if parent.kind == TokenKind::Array {
                            let next = parent.next_index + 1;
                            path.segments.push(Segment::Index(next));
                            if let Some(open) = self.stack.last_mut() {
                                open.next_index = next;
                            }
                        }
                        path
                    }
                    None => Path::new(),
                };
            }
            Token::Key(key) => {
                // A member's own path is its container's path plus this key.
                // The enclosing container is the frame on top of the stack, and
                // its `path` is that container's own path.
                let frame_path = match self.stack.last() {
                    Some(frame) => frame.path.clone(),
                    None => Path::new(),
                };
                self.working = frame_path;
                self.working.push_key(key.clone());
            }
            Token::Text(text) => {
                let path = self.working.clone();
                self.feed_value(&path, Some(text));
                self.match_scalar(&path, Some(text));
                self.truncate_to_container();
            }
            Token::TextTooLarge { .. } => {
                let path = self.working.clone();
                self.feed_value(&path, None);
                self.match_scalar(&path, None);
                self.truncate_to_container();
            }
        }
    }

    fn truncate_to_container(&mut self) {
        let depth = match self.stack.last() {
            Some(frame) => frame.path.len(),
            None => 0,
        };
        self.working.segments.truncate(depth);
    }

    /// A scalar matched a rule whose own path is the value's path.
    fn match_scalar(&mut self, path: &Path, text: Option<&String>) {
        for (rule_index, rule) in self.rules.iter().enumerate() {
            if !(rule.at.matches(path) && path.len() == rule.at.len()) {
                continue;
            }
            // A scalar can only supply a field aimed at the value itself.
            if rule.fields.len() != 1 || !rule.fields[0].from.is_empty() {
                continue;
            }
            let value = text.cloned().unwrap_or_else(|| "null".to_string());
            let index = index_of(path);
            self.records.push(Record {
                rule: rule_index,
                index,
                fields: vec![(rule.fields[0].as_name.clone(), value)],
                missing: Vec::new(),
            });
        }
    }

    fn maybe_start_collector(&mut self, path: &Path) {
        for (rule_index, rule) in self.rules.iter().enumerate() {
            if rule.at.matches(path) && path.len() == rule.at.len() {
                self.active.push(Collector {
                    rule: rule_index,
                    base: path.clone(),
                    values: vec![None; rule.fields.len()],
                    index: index_of(path),
                    depth: self.stack.len(),
                });
            }
        }
    }

    fn feed_value(&mut self, path: &Path, text: Option<&String>) {
        for collector in self.active.iter_mut() {
            let rest = &path.segments[collector.base.len()..];
            for (field_index, spec) in self.rules[collector.rule].fields.iter().enumerate() {
                if !spec_from_matches(spec, rest) {
                    continue;
                }
                // The first occurrence wins. A node can carry several
                // observations, and the record must be the same whichever one
                // the document happens to list first.
                if collector.values[field_index].is_none() {
                    collector.values[field_index] = text.cloned();
                }
            }
        }
    }

    fn complete_at(&mut self, depth: usize) {
        let finished: Vec<usize> = self
            .active
            .iter()
            .enumerate()
            .filter(|(_, collector)| collector.depth == depth)
            .map(|(position, _)| position)
            .collect();
        for position in finished.into_iter().rev() {
            let collector = self.active.remove(position);
            self.records.push(self.to_record(collector));
        }
    }

    fn to_record(&self, collector: Collector) -> Record {
        let rule = &self.rules[collector.rule];
        let mut fields = Vec::new();
        let mut missing = Vec::new();
        for (index, spec) in rule.fields.iter().enumerate() {
            match &collector.values[index] {
                Some(text) => fields.push((spec.as_name.clone(), text.clone())),
                None => match spec.on_missing {
                    Missing::Null => fields.push((spec.as_name.clone(), "null".to_string())),
                    Missing::Skip => missing.push(spec.as_name.clone()),
                },
            }
        }
        Record {
            rule: collector.rule,
            index: collector.index,
            fields,
            missing,
        }
    }
}

fn index_of(path: &Path) -> Option<usize> {
    path.segments
        .iter()
        .rev()
        .find_map(|segment| match segment {
            Segment::Index(index) => Some(*index),
            _ => None,
        })
}

/// Whether a field's path matches the remainder of a matched value's path.
///
/// The comparison is by pattern, not by equality, because a path under an array
/// element carries that element's actual index while the rule says `[*]`.
fn spec_from_matches(spec: &crate::path::FieldSpec, rest: &[Segment]) -> bool {
    if spec.from.segments.len() != rest.len() {
        return false;
    }
    spec.from
        .segments
        .iter()
        .zip(rest.iter())
        .all(|(expected, actual)| match expected {
            Segment::Key(key) => actual == &Segment::Key(key.clone()),
            Segment::AnyIndex => matches!(actual, Segment::Index(_)),
            Segment::Index(index) => actual == &Segment::Index(*index),
        })
}
