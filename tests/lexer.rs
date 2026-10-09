//! Tests for the streaming lexer.
//!
//! The lexer is the correctness boundary of this crate: everything downstream
//! trusts that it reports structure and keys without regard for what they mean.
//! These tests pin down the behaviour that is easy to break but hard to notice,
//! particularly the claim that memory does not depend on input length.

use reastream::lexer::{Lexer, Token, TokenKind};

fn tokens_of(input: &str) -> Vec<Token> {
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    // Feed in small pieces to exercise the chunk-boundary paths.
    let bytes = input.as_bytes();
    for chunk in bytes.chunks(3) {
        lexer.feed(chunk, &mut out).expect("feed failed");
    }
    lexer.finish().expect("finish failed");
    out
}

#[test]
fn empty_object_and_array() {
    assert_eq!(
        tokens_of("{}"),
        vec![
            Token::Open(TokenKind::Object),
            Token::Close(TokenKind::Object)
        ]
    );
    assert_eq!(
        tokens_of("[]"),
        vec![
            Token::Open(TokenKind::Array),
            Token::Close(TokenKind::Array)
        ]
    );
}

#[test]
fn key_is_distinguished_from_a_valued_string() {
    // This is the property that lets the lexer work without lookahead: in key
    // position, a string is a member name.
    assert_eq!(
        tokens_of(r#"{"a":1}"#),
        vec![
            Token::Open(TokenKind::Object),
            Token::Key("a".to_string()),
            Token::Text("1".to_string()),
            Token::Close(TokenKind::Object),
        ]
    );
}

#[test]
fn an_array_of_strings_has_no_keys() {
    assert_eq!(
        tokens_of(r#"["a","b"]"#),
        vec![
            Token::Open(TokenKind::Array),
            Token::Text("a".to_string()),
            Token::Text("b".to_string()),
            Token::Close(TokenKind::Array),
        ]
    );
}

#[test]
fn escapes_survive_chunk_boundaries() {
    // The backslash and the escaped quote may be split across feeds, and the
    // result is the resolved character, not the escape sequence.
    assert_eq!(
        tokens_of(r#"{"a":"x\"y"}"#),
        vec![
            Token::Open(TokenKind::Object),
            Token::Key("a".to_string()),
            Token::Text("x\"y".to_string()),
            Token::Close(TokenKind::Object),
        ]
    );
}

#[test]
fn nested_containers() {
    assert_eq!(
        tokens_of(r#"{"a":{"b":[1,2]}}"#),
        vec![
            Token::Open(TokenKind::Object),
            Token::Key("a".to_string()),
            Token::Open(TokenKind::Object),
            Token::Key("b".to_string()),
            Token::Open(TokenKind::Array),
            Token::Text("1".to_string()),
            Token::Text("2".to_string()),
            Token::Close(TokenKind::Array),
            Token::Close(TokenKind::Object),
            Token::Close(TokenKind::Object),
        ]
    );
}

#[test]
fn numbers_and_literals() {
    assert_eq!(
        tokens_of(r#"[0,-1,1.5,-2.5e10,true,false,null]"#),
        vec![
            Token::Open(TokenKind::Array),
            Token::Text("0".to_string()),
            Token::Text("-1".to_string()),
            Token::Text("1.5".to_string()),
            Token::Text("-2.5e10".to_string()),
            Token::Text("true".to_string()),
            Token::Text("false".to_string()),
            Token::Text("null".to_string()),
            Token::Close(TokenKind::Array),
        ]
    );
}

#[test]
fn a_number_split_across_chunks_is_one_token() {
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    lexer.feed(b"[12", &mut out).expect("feed");
    lexer.feed(b"34]", &mut out).expect("feed");
    lexer.finish().expect("finish");

    assert_eq!(
        out,
        vec![
            Token::Open(TokenKind::Array),
            Token::Text("1234".to_string()),
            Token::Close(TokenKind::Array),
        ]
    );
}

#[test]
fn whitespace_between_tokens_is_ignored() {
    assert_eq!(
        tokens_of("  {  \"a\"  :  [  1  ,  2  ]  }  "),
        vec![
            Token::Open(TokenKind::Object),
            Token::Key("a".to_string()),
            Token::Open(TokenKind::Array),
            Token::Text("1".to_string()),
            Token::Text("2".to_string()),
            Token::Close(TokenKind::Array),
            Token::Close(TokenKind::Object),
        ]
    );
}

#[test]
fn a_string_longer_than_the_cap_is_reported_not_buffered() {
    // A document full of one enormous string must not be held in memory. The
    // lexer reports its size and drops the bytes instead.
    let huge = format!("{{\"a\":\"{}\"}}", "x".repeat(64));
    let mut lexer = Lexer::with_text_cap(16);
    let mut out = Vec::new();
    lexer.feed(huge.as_bytes(), &mut out).expect("feed");
    lexer.finish().expect("finish");

    assert_eq!(out[0], Token::Open(TokenKind::Object));
    assert_eq!(out[1], Token::Key("a".to_string()));
    assert_eq!(out[2], Token::TextTooLarge { bytes: 64 });
    assert_eq!(out[3], Token::Close(TokenKind::Object));
}

#[test]
fn malformed_documents_are_rejected() {
    // An unclosed string.
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    lexer.feed(b"{\"a\":\"x", &mut out).expect("feed");
    assert!(lexer.finish().is_err());

    // A closing bracket with no array.
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    assert!(lexer.feed(b"[1]", &mut out).is_ok());

    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    assert!(lexer.feed(b"{\"a\":1]", &mut out).is_err());
}

#[test]
fn a_document_finishing_between_tokens_is_complete() {
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    lexer.feed(b"{\"a\":1}", &mut out).expect("feed");
    assert!(lexer.closed());
    lexer.finish().expect("finish");
}

#[test]
fn nul_bytes_in_a_string_are_text_not_structure() {
    // A string may legitimately contain a raw NUL byte in UTF-8 input.
    assert_eq!(
        tokens_of("{\"a\":\"x\u{0}y\"}"),
        vec![
            Token::Open(TokenKind::Object),
            Token::Key("a".to_string()),
            Token::Text("x\u{0}y".to_string()),
            Token::Close(TokenKind::Object),
        ]
    );
}
