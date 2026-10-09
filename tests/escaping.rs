//! Tests for JSON string escaping.
//!
//! A value's raw text still carries its escapes, and a record that quotes them
//! back verbatim is wrong in a way no reader can see. These tests pin the
//! resolution of every escape, including the two-byte escapes that are the
//! easiest to get wrong.

use reastream::lexer::{Lexer, Token};

fn text_of(input: &str) -> Vec<String> {
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    lexer.feed(input.as_bytes(), &mut out).expect("feed failed");
    lexer.finish().expect("finish failed");
    out.iter()
        .filter_map(|token| match token {
            Token::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn control_characters_are_decoded() {
    assert_eq!(
        text_of(r#"["\n\t\r\b\f"]"#),
        vec!["\n\t\r\u{8}\u{c}".to_string()]
    );
}

#[test]
fn a_unicode_escape_is_decoded() {
    assert_eq!(text_of(r#"["caf\u00e9"]"#), vec!["caf\u{e9}".to_string()]);
}

#[test]
fn a_surrogate_pair_is_one_character() {
    // U+1F600 is one emoji, encoded as two escapes.
    assert_eq!(
        text_of(r#"["\ud83d\ude00"]"#),
        vec!["\u{1f600}".to_string()]
    );
}

#[test]
fn an_unpaired_surrogate_becomes_a_replacement_character() {
    assert_eq!(text_of(r#"["\ud83d"]"#), vec!["\u{fffd}".to_string()]);
}

#[test]
fn a_member_name_is_not_unescaped() {
    // A name is matched literally, and the path a caller writes is also
    // literal, so the two must be comparable.
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    lexer.feed(br#"{"a\nb":1}"#, &mut out).expect("feed failed");
    lexer.finish().expect("finish failed");
    assert_eq!(out[1], Token::Key("a\\nb".to_string()));
}

#[test]
fn an_unknown_escape_is_left_visible() {
    assert_eq!(text_of(r#"["a\qb"]"#), vec!["a\\qb".to_string()]);
}

#[test]
fn an_empty_string_value_is_preserved() {
    assert_eq!(text_of(r#"{"a":""}"#), vec![String::new()]);
}
