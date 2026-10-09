//! Tests for projection.
//!
//! These are the tests that matter most, because projection is where a
//! document that is far too large to hold becomes a small set of records. The
//! behaviour worth pinning is the one that makes the whole design work: a
//! record is emitted, and only that record is kept, regardless of how many
//! siblings surround it.

use reastream::path::{field, parse_dotted, Missing, Path, Rule};
use reastream::projector::{ProjectionError, Projector, Record};

fn project(input: &str, rules: &[Rule]) -> Vec<Record> {
    let mut projector = Projector::new(rules).expect("rules rejected");
    let mut out = Vec::new();
    // Feed in small pieces on purpose: a record must not depend on the chunk
    // boundaries happening to fall between values.
    let mut lexer = reastream::lexer::Lexer::new();
    let bytes = input.as_bytes();
    for chunk in bytes.chunks(3) {
        lexer.feed(chunk, &mut out).expect("feed failed");
        for token in &out {
            projector.push(&token);
        }
        out.clear();
    }
    lexer.finish().expect("finish failed");
    projector.records
}

#[test]
fn no_rules_yields_no_records() {
    assert!(project("{\"a\":1}", &[]).is_empty());
}

#[test]
fn a_scalar_rule_captures_the_value_itself() {
    let rules = vec![Rule::fields(
        Path::key("evidence_id"),
        vec![field("evidence_id", "")],
    )];
    let records = project(r#"{"evidence_id":"ev_abc123"}"#, &rules);
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].fields,
        vec![("evidence_id".to_string(), "ev_abc123".to_string())]
    );
}

#[test]
fn a_nested_scalar_rule_captures_below_an_object() {
    let rules = vec![Rule::fields(
        Path::keys(["subject", "name"]),
        vec![field("subject_name", "")],
    )];
    let records = project(
        r#"{"subject":{"name":"C:/app/src","digest":{"sha256":"ff"}}}"#,
        &rules,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].fields,
        vec![("subject_name".to_string(), "C:/app/src".to_string())]
    );
}

#[test]
fn one_record_per_array_element() {
    let rules = vec![Rule::fields(
        parse_dotted("nodes[*]"),
        vec![field("node_id", "node_id"), field("kind", "kind")],
    )];
    let records = project(
        r#"{"nodes":[{"node_id":"a","kind":"file"},{"node_id":"b","kind":"dir"}]}"#,
        &rules,
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].index, Some(0));
    assert_eq!(records[1].index, Some(1));
    assert_eq!(records[0].fields[0].1, "a");
    assert_eq!(records[1].fields[0].1, "b");
}

#[test]
fn a_field_below_the_array_element_is_captured() {
    // `nodes[*].identity.namespace` is the shape that makes this crate worth
    // anything: the field sits two levels below the matched value.
    let rules = vec![Rule::fields(
        parse_dotted("normalized_result.graph.nodes[*]"),
        vec![
            field("node_id", "node_id"),
            field("identity_namespace", "identity.namespace"),
            field("identity_key", "identity.key"),
            field("semantic_role", "semantic_role"),
            field("state", "state"),
        ],
    )];
    let records = project(
        r#"{"normalized_result":{"graph":{"nodes":[
            {"node_id":"n1","kind":"file","identity":{"namespace":"ns","key":"k1"},"semantic_role":"src","state":"ok"},
            {"node_id":"n2","kind":"dir","identity":{"namespace":"ns","key":"k2"},"semantic_role":"dir","state":"ok"}
        ]}}}"#,
        &rules,
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].fields[0].1, "n1");
    assert_eq!(records[0].fields[1].1, "ns");
    assert_eq!(records[0].fields[2].1, "k1");
    assert_eq!(records[1].fields[0].1, "n2");
    assert_eq!(records[1].fields[2].1, "k2");
}

#[test]
fn an_absent_field_is_null_rather_than_dropped() {
    let rules = vec![Rule::fields(
        parse_dotted("nodes[*]"),
        vec![field("node_id", "node_id"), field("state", "state")],
    )];
    let records = project(
        r#"{"nodes":[{"node_id":"a"},{"node_id":"b","state":"ok"}]}"#,
        &rules,
    );
    assert_eq!(records.len(), 2);
    // The first node has no state, and must not lose its record because of it.
    assert_eq!(
        records[0].fields[1],
        ("state".to_string(), "null".to_string())
    );
    assert_eq!(
        records[1].fields[1],
        ("state".to_string(), "ok".to_string())
    );
}

#[test]
fn an_absent_field_can_be_omitted() {
    let spec = reastream::path::FieldSpec {
        as_name: "state".to_string(),
        from: parse_dotted("state"),
        on_missing: Missing::Skip,
    };
    let rules = vec![Rule::fields(parse_dotted("nodes[*]"), vec![spec])];
    let records = project(r#"{"nodes":[{"other":1}]}"#, &rules);
    assert_eq!(records.len(), 1);
    assert!(records[0].fields.is_empty());
    assert_eq!(records[0].missing, vec!["state".to_string()]);
}

#[test]
fn a_number_field_is_captured_as_text() {
    let rules = vec![Rule::fields(
        parse_dotted("nodes[*]"),
        vec![field("confidence", "confidence")],
    )];
    let records = project(
        r#"{"nodes":[{"confidence":0.9},{"confidence":1.0e-3}]}"#,
        &rules,
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].fields[0].1, "0.9");
    assert_eq!(records[1].fields[0].1, "1.0e-3");
}

#[test]
fn sibling_arrays_are_not_confused() {
    let rules = vec![Rule::fields(parse_dotted("a[*]"), vec![field("v", "v")])];
    let records = project(
        r#"{"a":[{"v":"first"},{"v":"second"}],"b":[{"v":"other"}]}"#,
        &rules,
    );
    assert_eq!(
        records.len(),
        2,
        "the rule is scoped to `a`, not to every array"
    );
    assert_eq!(records[0].fields[0].1, "first");
    assert_eq!(records[1].fields[0].1, "second");
}

#[test]
fn a_string_value_is_not_mistaken_for_a_key() {
    // An array of strings has no keys, so no phantom records may appear.
    let rules = vec![Rule::fields(
        parse_dotted("nodes[*]"),
        vec![field("k", "k")],
    )];
    let records = project(r#"{"nodes":["alpha","beta"]}"#, &rules);
    assert!(records.is_empty());
}

#[test]
fn chunk_boundaries_do_not_create_or_hide_records() {
    // Every chunk size must give the same answer; this is the property that
    // would silently break on a document larger than the buffer.
    let input = r#"{"normalized_result":{"graph":{"nodes":[
        {"node_id":"n1","identity":{"namespace":"a","key":"k1"}},
        {"node_id":"n2","identity":{"namespace":"b","key":"k2"}},
        {"node_id":"n3","identity":{"namespace":"c","key":"k3"}}
    ]}}}"#;
    let rules = vec![Rule::fields(
        parse_dotted("normalized_result.graph.nodes[*]"),
        vec![
            field("node_id", "node_id"),
            field("identity_key", "identity.key"),
        ],
    )];

    for size in [1usize, 2, 3, 5, 7, 11, 13, 17, 64, 1024] {
        let mut lexer = reastream::lexer::Lexer::new();
        let mut projector = Projector::new(&rules).expect("rules rejected");
        let mut out = Vec::new();
        for chunk in input.as_bytes().chunks(size) {
            lexer.feed(chunk, &mut out).expect("feed failed");
            for token in &out {
                projector.push(&token);
            }
            out.clear();
        }
        lexer.finish().expect("finish failed");
        let records: Vec<&str> = projector
            .records
            .iter()
            .map(|record| record.fields[0].1.as_str())
            .collect();
        assert_eq!(
            records,
            vec!["n1", "n2", "n3"],
            "chunk size {size} changed the result"
        );
    }
}

#[test]
fn an_empty_array_yields_no_records() {
    let rules = vec![Rule::fields(
        parse_dotted("nodes[*]"),
        vec![field("k", "k")],
    )];
    assert!(project(r#"{"nodes":[]}"#, &rules).is_empty());
}

#[test]
fn invalid_rules_are_rejected() {
    let rules: &[Rule] = &[Rule::fields(Path::new(), vec![field("a", "a")])];
    assert_eq!(
        Projector::new(rules).unwrap_err(),
        ProjectionError::EmptyPath("rule path must not be empty")
    );

    let rules = vec![Rule::fields(
        Path::key("x"),
        vec![field("a", "a"), field("a", "b")],
    )];
    assert_eq!(
        Projector::new(&rules).unwrap_err(),
        ProjectionError::DuplicateFieldName
    );
}
