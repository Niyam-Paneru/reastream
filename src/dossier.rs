//! Dossier: the whole job, from a reader to a compact document.
//!
//! A dossier is what the streaming engine emits. It carries the evidence
//! envelope, the projected records, and what was dropped — because a projection
//! that cannot say how much it skipped cannot be trusted by its reader.

use std::io::Read;

use crate::decoding::{DecodingReader, Encoding};
use crate::lexer::{Lexer, Token};
use crate::path::Rule;
use crate::projector::{ProjectionError, Projector};
/// What happened to the bytes that were read but not projected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skipped {
    /// Total bytes read from the source.
    pub bytes_in: usize,
    /// Total UTF-8 bytes decoded.
    pub bytes_decoded: usize,
    /// Tokens the lexer produced.
    pub tokens: usize,
    /// Values that matched no rule.
    pub dropped_values: usize,
    /// Bytes of string text that exceeded the lexer's cap.
    pub oversized_text: usize,
}

/// A projection of one document.
#[derive(Debug, Clone, Default)]
pub struct Dossier {
    /// The encoding the source was read in.
    pub encoding: Option<Encoding>,
    /// Records produced by the projection, in document order.
    pub records: Vec<crate::projector::Record>,
    /// How much was read and not kept.
    pub skipped: Skipped,
    /// Whether the document closed cleanly.
    pub complete: bool,
    /// Rule paths that were declared, in order.
    pub rule_paths: Vec<String>,
}

/// Why a dossier could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DossierError {
    Read(String),
    Lexical(String),
    Rules(String),
}

impl std::fmt::Display for DossierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DossierError::Read(message) => write!(f, "read failed: {message}"),
            DossierError::Lexical(message) => write!(f, "malformed JSON: {message}"),
            DossierError::Rules(message) => write!(f, "invalid rules: {message}"),
        }
    }
}

impl std::error::Error for DossierError {}

/// The default chunk size for streaming: 1 MiB.
pub const CHUNK: usize = 1 << 20;

impl Dossier {
    /// Project a whole reader.
    pub fn open<R: Read>(reader: R) -> Result<Dossier, DossierError> {
        let rules = crate::application_rules();
        Self::with_rules(reader, &rules)
    }

    /// Project a reader with caller-supplied rules.
    pub fn with_rules<R: Read>(reader: R, rules: &[Rule]) -> Result<Dossier, DossierError> {
        let mut projector = Projector::new(rules).map_err(|error| match error {
            ProjectionError::EmptyPath(message) | ProjectionError::EmptyFieldName(message) => {
                DossierError::Rules(message.to_string())
            }
            ProjectionError::DuplicateFieldName => {
                DossierError::Rules("two rules emit the same field name".to_string())
            }
        })?;

        let mut decoder = DecodingReader::new(reader);
        let mut lexer = Lexer::new();
        let mut chunk = vec![0u8; CHUNK];
        let mut oversized_text = 0usize;

        loop {
            let read = decoder
                .read(&mut chunk)
                .map_err(|error| DossierError::Read(error.to_string()))?;
            if read == 0 {
                break;
            }
            let mut tokens = Vec::new();
            lexer
                .feed(&chunk[..read], &mut tokens)
                .map_err(|error| DossierError::Lexical(format!("{error:?}")))?;
            for token in tokens {
                if let Token::TextTooLarge { bytes } = &token {
                    oversized_text += bytes;
                }
                projector.push(&token);
            }
        }
        lexer
            .finish()
            .map_err(|error| DossierError::Lexical(format!("{error:?}")))?;

        Ok(Dossier {
            encoding: decoder.encoding(),
            records: projector.records,
            skipped: Skipped {
                bytes_in: decoder.bytes_in(),
                bytes_decoded: decoder.bytes_out(),
                tokens: projector.walked,
                dropped_values: 0,
                oversized_text,
            },
            complete: lexer.closed(),
            rule_paths: rules.iter().map(|rule| rule.at.dotted()).collect(),
        })
    }

    /// A short human-readable summary of the projection.
    pub fn summary(&self) -> DossierSummary {
        DossierSummary {
            encoding: self.encoding,
            records: self.records.len(),
            bytes_in: self.skipped.bytes_in,
            bytes_decoded: self.skipped.bytes_decoded,
            complete: self.complete,
            kept_ratio: if self.skipped.bytes_decoded == 0 {
                0.0
            } else {
                self.kept_bytes() as f64 / self.skipped.bytes_decoded as f64
            },
        }
    }

    /// Bytes retained, measured by the serialised records.
    pub fn kept_bytes(&self) -> usize {
        self.records
            .iter()
            .map(|record| {
                record
                    .fields
                    .iter()
                    .map(|(name, value)| name.len() + value.len() + 4)
                    .sum::<usize>()
            })
            .sum()
    }
}

/// What [`Dossier::summary`] reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DossierSummary {
    pub encoding: Option<Encoding>,
    pub records: usize,
    pub bytes_in: usize,
    pub bytes_decoded: usize,
    pub complete: bool,
    pub kept_ratio: f64,
}
