//! An incremental JSON lexer that never holds the document.
//!
//! REA emits one very large result document. A conventional parser must hold
//! that document, so this crate reads it as a stream of tokens instead. The
//! lexer is deliberately not a parser: it reports structure and keys, and hands
//! scalar text to the caller without deciding what it means.
//!
//! The invariant that makes the design work: a string in key position *is* a
//! key. JSON only permits a member name immediately after `{` or `,` inside an
//! object, so the lexer can label it from its own state without looking ahead
//! past the closing quote. That is what keeps the lexer free of buffering.
//!
//! Text is owned, but bounded: a string longer than the configured cap is
//! reported as [`Token::TextTooLarge`] rather than accumulated, so a huge value
//! inside a subtree the caller wants to skip cannot grow the process.

use std::io::Read;

/// What kind of JSON value a token refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Object,
    Array,
}

/// One lexical event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// The first byte of an object or array.
    Open(TokenKind),
    /// The matching close of the most recent open.
    Close(TokenKind),
    /// A member name. The value it introduces is the next token.
    Key(String),
    /// Scalar text, complete: a string, number, `true`, `false` or `null`.
    Text(String),
    /// A string whose text exceeded the cap. Its length is reported instead.
    TextTooLarge { bytes: usize },
}

/// Why a feed did not complete a well-formed document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexError {
    NotJson(&'static str),
    UnterminatedString,
    InvalidUtf8,
}

/// Where in a container the lexer is standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    /// A value must start here.
    Value,
    /// A member name (or the object's close) must come here.
    Key,
    /// Only a comma or the container's close are allowed.
    AfterValue,
}

/// The default cap on retained string text.
pub const DEFAULT_TEXT_CAP: usize = 1 << 20; // 1 MiB

/// A streaming JSON lexer over already-decoded UTF-8 bytes.
pub struct Lexer {
    /// Bytes received but not yet turned into tokens.
    pending: Vec<u8>,
    position: Position,
    /// Stack of open containers, innermost last.
    depth: Vec<TokenKind>,
    /// Lexing is inside a string literal.
    in_string: bool,
    /// The string currently being read is a member name.
    string_is_key: bool,
    /// A backslash has just been seen inside a string.
    escaped: bool,
    /// Accumulating scalar text.
    text: Vec<u8>,
    /// A literal left incomplete at the end of a chunk, awaiting more bytes.
    in_literal: bool,
    /// True while the string being read is a *value* rather than a member name.
    ///
    /// A member name is never unescaped, because its text is a key the caller
    /// matches literally, and the path they wrote is also literal.
    in_string_value: bool,
    /// Set once `text` reaches the cap; further bytes are counted, not stored.
    dropped_text: usize,
    text_cap: usize,
    /// True once a complete top-level value has been closed.
    closed: bool,
}

impl Lexer {
    pub fn new() -> Self {
        Self::with_text_cap(DEFAULT_TEXT_CAP)
    }

    pub fn with_text_cap(text_cap: usize) -> Self {
        Self {
            pending: Vec::new(),
            position: Position::Value,
            depth: Vec::new(),
            in_string: false,
            string_is_key: false,
            escaped: false,
            text: Vec::new(),
            in_literal: false,
            in_string_value: false,
            dropped_text: 0,
            text_cap: text_cap.max(16),
            closed: false,
        }
    }

    /// Feed one chunk of bytes. Returns the tokens they complete.
    pub fn feed(&mut self, chunk: &[u8], out: &mut Vec<Token>) -> Result<(), LexError> {
        self.pending.extend_from_slice(chunk);

        // A literal may be left incomplete by the previous chunk. If the bytes
        // that arrive next are a delimiter, the literal was finished at the
        // chunk boundary; otherwise they continue it.
        if self.in_literal {
            let continues = match self.pending.first() {
                None => true,
                Some(byte) => !matches!(
                    byte,
                    b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']' | b':'
                ),
            };
            if continues {
                self.position = Position::Value;
            } else {
                self.in_literal = false;
                self.check_literal()?;
                self.finish_text(out);
            }
        }

        let mut consumed = 0;
        while consumed < self.pending.len() {
            let byte = self.pending[consumed];

            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                    self.push_text_byte(byte);
                    consumed += 1;
                    continue;
                }
                match byte {
                    b'\\' => {
                        self.escaped = true;
                        self.push_text_byte(byte);
                        consumed += 1;
                    }
                    b'"' => {
                        self.in_string = false;
                        consumed += 1;
                        self.finish_text(out);
                    }
                    _ => {
                        self.push_text_byte(byte);
                        consumed += 1;
                    }
                }
                continue;
            }

            match byte {
                b' ' | b'\t' | b'\r' | b'\n' => consumed += 1,
                b'"' => {
                    // A string here is a key only in key position; that is what
                    // makes lookahead unnecessary.
                    self.string_is_key = self.position == Position::Key;
                    self.in_string_value = !self.string_is_key;
                    self.in_string = true;
                    self.text.clear();
                    self.dropped_text = 0;
                    consumed += 1;
                }
                b'{' => {
                    self.emit(Token::Open(TokenKind::Object), out);
                    self.depth.push(TokenKind::Object);
                    self.position = Position::Key;
                    consumed += 1;
                }
                b'[' => {
                    self.emit(Token::Open(TokenKind::Array), out);
                    self.depth.push(TokenKind::Array);
                    self.position = Position::Value;
                    consumed += 1;
                }
                b'}' => {
                    // A close is legal at the end of a member list, and an
                    // absent member list leaves the object in key position.
                    let legal = match self.depth.last() {
                        Some(TokenKind::Object) => {
                            matches!(self.position, Position::AfterValue | Position::Key)
                        }
                        _ => false,
                    };
                    if legal {
                        self.depth.pop();
                        self.emit(Token::Close(TokenKind::Object), out);
                        consumed += 1;
                        self.position = self.after_close();
                    } else {
                        return Err(LexError::NotJson("closing brace without an object"));
                    }
                }
                b']' => {
                    let legal = match self.depth.last() {
                        Some(TokenKind::Array) => {
                            matches!(self.position, Position::AfterValue | Position::Value)
                        }
                        _ => false,
                    };
                    if legal {
                        self.depth.pop();
                        self.emit(Token::Close(TokenKind::Array), out);
                        consumed += 1;
                        self.position = self.after_close();
                    } else {
                        return Err(LexError::NotJson("closing bracket without an array"));
                    }
                }
                b':' => {
                    // The value a member name introduces starts here.
                    if self.string_is_key {
                        return Err(LexError::NotJson("colon outside a member name"));
                    }
                    self.position = Position::Value;
                    consumed += 1;
                }
                b',' => {
                    self.position = match self.depth.last() {
                        Some(TokenKind::Object) => Position::Key,
                        Some(TokenKind::Array) => Position::Value,
                        None => return Err(LexError::NotJson("comma outside a container")),
                    };
                    consumed += 1;
                }
                _ => {
                    // A number or a bare literal, ending at the next delimiter.
                    let start = consumed;
                    while consumed < self.pending.len()
                        && !matches!(
                            self.pending[consumed],
                            b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']' | b':'
                        )
                    {
                        consumed += 1;
                    }
                    let literal = self.pending[start..consumed].to_vec();
                    if self.in_literal {
                        self.text.extend_from_slice(&literal);
                    } else {
                        // A fresh literal must not append to a previous one.
                        self.text = literal;
                    }
                    if consumed == self.pending.len() && self.position == Position::Value {
                        // The literal may continue in the next chunk, so hold it
                        // and wait rather than emitting a partial token.
                        self.in_literal = true;
                        break;
                    }
                    self.in_literal = false;
                    self.check_literal()?;
                    self.finish_text(out);
                }
            }
        }

        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        Ok(())
    }

    /// Position after closing a container: the container's own value has ended.
    fn after_close(&mut self) -> Position {
        if self.depth.is_empty() {
            self.closed = true;
        }
        Position::AfterValue
    }

    fn emit(&self, token: Token, out: &mut Vec<Token>) {
        let _ = self;
        out.push(token);
    }

    fn check_literal(&self) -> Result<(), LexError> {
        let text = String::from_utf8(self.text.clone()).map_err(|_| LexError::InvalidUtf8)?;
        match text.as_str() {
            "true" | "false" | "null" => return Ok(()),
            _ => {}
        }
        if is_json_number(&text) {
            Ok(())
        } else {
            Err(LexError::NotJson("not a valid JSON number or literal"))
        }
    }

    fn push_text_byte(&mut self, byte: u8) {
        if self.text.len() < self.text_cap {
            self.text.push(byte);
        } else {
            self.dropped_text += 1;
        }
    }

    fn finish_text(&mut self, out: &mut Vec<Token>) {
        let bytes = self.text.len() + self.dropped_text;
        if self.dropped_text > 0 {
            self.text.clear();
            self.dropped_text = 0;
            out.push(Token::TextTooLarge { bytes });
            self.string_is_key = false;
            self.position = Position::AfterValue;
            return;
        }
        let mut text = String::from_utf8(std::mem::take(&mut self.text))
            .map_err(|_| LexError::InvalidUtf8)
            .unwrap_or_default();
        if !self.string_is_key {
            // A string value carries escapes that must be resolved before the
            // caller sees the text: a Windows path arrives as `C:\\dir`, and a
            // record quoting it verbatim would be wrong twice over.
            if self.in_string_value {
                text = unescape(&text);
            }
        }
        self.in_string_value = false;
        if self.string_is_key {
            out.push(Token::Key(text));
            self.string_is_key = false;
            self.position = Position::Value;
        } else {
            out.push(Token::Text(text));
            self.position = Position::AfterValue;
        }
    }

    /// Report that no more bytes will arrive.
    pub fn finish(&mut self) -> Result<(), LexError> {
        if self.in_string {
            return Err(LexError::UnterminatedString);
        }
        if !self.pending.is_empty() {
            return Err(LexError::NotJson("input ended inside an incomplete token"));
        }
        if !self.depth.is_empty() {
            return Err(LexError::NotJson(
                "input ended inside an unclosed container",
            ));
        }
        Ok(())
    }

    /// True once a complete top-level value has been closed.
    pub fn closed(&self) -> bool {
        self.closed
    }
}

impl Default for Lexer {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve the escape sequences in a JSON string value.
///
/// A value's raw text still carries its escapes, so `C:\dir` is still
/// `C:\\dir` and a newline is still the two characters `\` `n`. A record that
/// quoted those back verbatim would be wrong in a way the reader cannot see.
///
/// An unknown escape is left as it was found rather than dropped, because the
/// caller should be able to tell that it happened.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let first: String = characters.by_ref().take(4).collect();
                let mut code = match u32::from_str_radix(&first, 16) {
                    Ok(code) => code,
                    Err(_) => {
                        out.push_str("\\u");
                        out.push_str(&first);
                        continue;
                    }
                };
                // A surrogate pair is two escapes long; resolving one half alone
                // would produce a replacement character instead of the text.
                if (0xd800..0xdc00).contains(&code) {
                    let mut pair = characters.clone();
                    if pair.next() == Some('\\') && pair.next() == Some('u') {
                        let second: String = pair.by_ref().take(4).collect();
                        if let Ok(low) = u32::from_str_radix(&second, 16) {
                            if (0xdc00..0xe000).contains(&low) {
                                let combined = 0x1_0000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                                characters = pair;
                                code = combined;
                            }
                        }
                    }
                }
                match char::from_u32(code) {
                    Some(decoded) => out.push(decoded),
                    None => out.push('\u{fffd}'),
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Whether `literal` is a JSON number per RFC 8259.
fn is_json_number(literal: &str) -> bool {
    let bytes = literal.as_bytes();
    let mut index = 0;
    if bytes.first() == Some(&b'-') {
        index += 1;
    }
    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == integer_start {
        return false;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e') | Some(b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

/// Read a whole reader into tokens, owning all text.
pub fn scan<R: Read>(reader: R) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new();
    let mut reader = reader;
    let mut tokens = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| LexError::NotJson("read failed"))?;
        if read == 0 {
            break;
        }
        lexer.feed(&chunk[..read], &mut tokens)?;
    }
    lexer.finish()?;
    Ok(tokens)
}
