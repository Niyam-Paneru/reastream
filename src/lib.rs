//! Streaming reader for REA analysis results.
//!
//! REA produces one very large result document per analysis. A REA application
//! analysis of the REA source tree produces a document whose semantic graph is
//! **100.00% of its length** — measured, not estimated (see
//! `docs/measurements/README.md`). The envelope a caller needs is under two
//! kilobytes at the very front of that document.
//!
//! This crate reads that document as a stream and emits only the projection a
//! caller asked for. It never holds the document.
//!
//! ## Making rules from outside
//!
//! A rule is a path and the fields to read there, so a caller can describe a
//! projection without depending on Rust types:
//!
//! ```
//! use reastream::{decode_rule_set, Dossier};
//!
//! // One row per graph node, keeping only its identity and role.
//! let rules = decode_rule_set(&[
//!     ("evidence_id", "evidence_id", ""),
//!     ("normalized_result.graph.nodes[*].node_id", "node_id", "node_id"),
//!     ("normalized_result.graph.nodes[*].identity.namespace", "identity_namespace", "identity.namespace"),
//!     ("normalized_result.graph.nodes[*].identity.key", "identity_key", "identity.key"),
//! ]).unwrap();
//!
//! assert_eq!(rules.rules.len(), 4);
//! ```
//!
//! Each tuple is `(rule path, output field, value path relative to the rule)`.

pub mod decoding;
pub mod dossier;
pub mod lexer;
pub mod path;
pub mod projector;

pub use decoding::{detect, DecodingReader, Encoding};
pub use dossier::{Dossier, DossierError, DossierSummary};
pub use lexer::{Lexer, Token, TokenKind};
pub use path::{field, parse_dotted, FieldSpec, Missing, Path, Rule, Segment};
pub use projector::{ProjectionError, Projector, Record};

/// A set of rules: which paths to collect, and what to call the result.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

/// Build a [`RuleSet`] from `(rule path, field name, relative path)` triples.
///
/// Adjacent triples sharing a rule path are merged into one rule, so a caller
/// may list one node's fields in any order and still get one record per node.
pub fn decode_rule_set<'a, I>(specs: I) -> Result<RuleSet, String>
where
    I: IntoIterator<Item = &'a (&'a str, &'a str, &'a str)>,
{
    let mut rules: Vec<Rule> = Vec::new();
    for (at, name, from) in specs {
        let at_path = parse_dotted(at);
        let spec = field(name, from);
        match rules.last_mut() {
            Some(rule) if rule.at == at_path => rule.fields.push(spec),
            _ => rules.push(Rule::fields(at_path, vec![spec])),
        }
    }
    if rules.is_empty() {
        return Err("rule set is empty".to_string());
    }
    Ok(RuleSet { rules })
}

/// Wrap rules in a [`RuleSet`].
pub fn rule_set(rules: Vec<Rule>) -> RuleSet {
    RuleSet { rules }
}

/// The REA application result projection.
///
/// Paths verified against REA 6.1.0 output on the REA source tree; see
/// `docs/measurements/README.md`. The envelope sits at the very front of the
/// document and the capability information lives inside `graph.nodes[]`, which
/// is where all of the length is.
///
/// `state` and `confidence` are included because a node's coverage is what makes
/// its identity usable. A node that does not carry them reports `null` rather
/// than being dropped.
pub fn application_rules() -> Vec<Rule> {
    vec![
        Rule::fields(Path::key("evidence_id"), vec![field("evidence_id", "")]),
        Rule::fields(Path::key("operation"), vec![field("operation", "")]),
        Rule::fields(
            Path::key("predicate_type"),
            vec![field("predicate_type", "")],
        ),
        Rule::fields(Path::keys(["subject", "name"]), vec![field("subject", "")]),
        Rule::fields(
            Path::keys(["normalized_result", "root_artifact_sha256"]),
            vec![field("root_artifact_sha256", "")],
        ),
        Rule::fields(
            Path::keys(["normalized_result", "inventory_manifest_id"]),
            vec![field("inventory_manifest_id", "")],
        ),
        Rule::fields(
            Path::keys(["normalized_result", "inventory_graph_sha256"]),
            vec![field("inventory_graph_sha256", "")],
        ),
        // One row per graph node. This is the rule that keeps memory bounded: a
        // node is projected and then released, so its siblings are never held.
        //
        // The field paths come from the real node shape, not from an assumption:
        // a node carries `node_id`, `kind`, an `identity` block, and an
        // `observations` array whose entries hold the human-readable label, the
        // semantic role, and the evidence's state and confidence.
        Rule::fields(
            parse_dotted("normalized_result.graph.nodes[*]"),
            vec![
                field("node_id", "node_id"),
                field("kind", "kind"),
                field("namespace", "identity.namespace"),
                field("key", "identity.key"),
                field("label", "observations[*].label"),
                field("semantic_role", "observations[*].properties.semantic_role"),
                field(
                    "resolution_status",
                    "observations[*].properties.resolution_status",
                ),
                field("state", "observations[*].evidence.state"),
                field("confidence", "observations[*].evidence.confidence"),
            ],
        ),
    ]
}
