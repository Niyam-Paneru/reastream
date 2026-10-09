//! Transparent decoding of REA result documents.
//!
//! REA writes UTF-8. Windows shell redirection writes UTF-16LE, which a naive
//! capture then produces by accident, and that document is twice the length of
//! its JSON and unreadable as UTF-8. This reader normalises both to UTF-8 so
//! the lexer only ever sees one encoding.
//!
//! The byte order mark is consumed rather than pushed into the JSON stream,
//! because `JSON.parse` and a strict lexer both reject it as leading garbage.

use std::io::{self, Read};

/// What the first bytes of the stream declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// No BOM was present; UTF-8 is assumed.
    Utf8,
    /// A UTF-8 BOM was present and is stripped.
    Utf8WithBom,
    /// A UTF-16LE BOM was present and the body is transcoded.
    Utf16Le,
    /// A UTF-16BE BOM was present and the body is transcoded.
    Utf16Be,
}

impl Encoding {
    /// True when the body is more than one byte per character.
    pub fn is_utf16(self) -> bool {
        matches!(self, Encoding::Utf16Le | Encoding::Utf16Be)
    }
}

/// Detect the encoding from the first bytes of a stream.
///
/// Fewer than two bytes cannot declare a BOM, so they are read as UTF-8.
pub fn detect(first: &[u8]) -> Encoding {
    match first {
        [0xff, 0xfe, ..] => Encoding::Utf16Le,
        [0xfe, 0xff, ..] => Encoding::Utf16Be,
        [0xef, 0xbb, 0xbf, ..] => Encoding::Utf8WithBom,
        _ => Encoding::Utf8,
    }
}

/// A reader that yields UTF-8 bytes regardless of the encoding it reads.
///
/// The first [`BOM_PREFIX`] bytes are inspected to decide the encoding, then
/// every later byte is transcoded. Decoding straddles the underlying read
/// boundary: an odd UTF-16 byte is carried over to the next read.
pub struct DecodingReader<R> {
    inner: R,
    encoding: Encoding,
    /// Bytes read but not yet transcoded.
    carry: Vec<u8>,
    /// True once the encoding has been decided.
    decided: bool,
    /// Decoded output waiting to be handed out.
    out: Vec<u8>,
    /// Read position within `out`.
    out_at: usize,
    done: bool,
    chunks: usize,
    bytes_in: usize,
    bytes_out: usize,
}

/// How many bytes are needed to decide the encoding.
pub const BOM_PREFIX: usize = 4;

impl<R: Read> DecodingReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            encoding: Encoding::Utf8,
            carry: Vec::new(),
            decided: false,
            out: Vec::new(),
            out_at: 0,
            done: false,
            chunks: 0,
            bytes_in: 0,
            bytes_out: 0,
        }
    }

    /// The encoding that was detected, once the stream has been read.
    pub fn encoding(&self) -> Option<Encoding> {
        self.decided.then_some(self.encoding)
    }

    /// Total bytes read from the underlying reader.
    pub fn bytes_in(&self) -> usize {
        self.bytes_in
    }

    /// Total UTF-8 bytes yielded so far.
    pub fn bytes_out(&self) -> usize {
        self.bytes_out
    }
}

impl<R: Read> Read for DecodingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 64 {
                // Defensive termination: transcoding must always make progress
                // for inputs with at least two bytes available.
                return Ok(0);
            }

            if self.out_at < self.out.len() {
                let available = self.out.len() - self.out_at;
                let take = available.min(buf.len());
                buf[..take].copy_from_slice(&self.out[self.out_at..self.out_at + take]);
                self.out_at += take;
                self.bytes_out += take;
                return Ok(take);
            }
            if self.done {
                return Ok(0);
            }

            // Deceive and refill.
            self.out.clear();
            self.out_at = 0;

            let want = if self.decided && self.encoding.is_utf16() {
                // Read whole code units so no unit straddles a read boundary.
                (buf.len().max(2) / 2) * 2
            } else {
                buf.len().max(BOM_PREFIX)
            };
            let mut raw = vec![0u8; want];
            let read = self.inner.read(&mut raw)?;
            if read == 0 {
                self.done = true;
                self.transcode(&raw[..read]);
                if self.out.is_empty() {
                    return Ok(0);
                }
                continue;
            }
            raw.truncate(read);
            self.bytes_in += read;
            self.chunks += 1;

            if !self.decided {
                // The BOM needs up to BOM_PREFIX bytes, so keep reading until
                // there are enough, or the stream ends.
                self.carry.extend_from_slice(&raw);
                if self.carry.len() < BOM_PREFIX && !self.started_after_bom() {
                    continue;
                }
                self.encoding = detect(&self.carry);
                self.decided = true;
                let skip = match self.encoding {
                    Encoding::Utf8WithBom => 3,
                    Encoding::Utf16Le | Encoding::Utf16Be => 2,
                    Encoding::Utf8 => 0,
                };
                self.carry.drain(..skip.min(self.carry.len()));
                let body = std::mem::take(&mut self.carry);
                self.transcode(&body);
                continue;
            }

            self.transcode(&raw);
        }
    }
}

impl<R: Read> DecodingReader<R> {
    fn started_after_bom(&self) -> bool {
        // A two-byte UTF-16 BOM plus at least one full code unit is enough.
        self.carry.len() >= 4 && (self.carry[0] == 0xff || self.carry[0] == 0xfe)
    }

    fn transcode(&mut self, raw: &[u8]) {
        let mut bytes = Vec::with_capacity(raw.len() + self.carry.len());
        bytes.extend_from_slice(&self.carry);
        self.carry.clear();
        bytes.extend_from_slice(raw);

        match self.encoding {
            Encoding::Utf8 => {
                self.out.extend_from_slice(&bytes);
                self.carry.clear();
            }
            Encoding::Utf8WithBom => {
                // The BOM was already stripped by the caller.
                self.out.extend_from_slice(&bytes);
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                // Keep a dangling byte for the next read.
                let usable = bytes.len() / 2;
                self.carry.extend_from_slice(&bytes[usable * 2..]);
                for unit in 0..usable {
                    let low = bytes[unit * 2];
                    let high = bytes[unit * 2 + 1];
                    let code = if self.encoding == Encoding::Utf16Le {
                        (high as u16) << 8 | low as u16
                    } else {
                        (low as u16) << 8 | high as u16
                    };
                    push_code_unit(&mut self.out, code);
                }
            }
        }
    }
}

/// Encode one UTF-16 code unit as UTF-8, passing surrogates through unpaired.
///
/// A lone surrogate is not valid UTF-8, so it is encoded as the replacement
/// character rather than silently dropped; the lexer rejects invalid input in
/// its own terms and the caller decides what that means.
fn push_code_unit(out: &mut Vec<u8>, unit: u16) {
    if let Some(decoded) = char::from_u32(unit as u32) {
        let mut buffer = [0u8; 4];
        out.extend_from_slice(decoded.encode_utf8(&mut buffer).as_bytes());
    } else {
        out.extend_from_slice("\u{fffd}".as_bytes());
    }
}

/// Read a whole stream into memory, reporting its encoding and size.
///
/// Used only where the caller genuinely needs the text; the streaming path
/// never calls this.
pub fn read_all<R: Read>(reader: R) -> io::Result<(Vec<u8>, Encoding)> {
    let mut decoder = DecodingReader::new(reader);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    let encoding = decoder.encoding().unwrap_or(Encoding::Utf8);
    Ok((out, encoding))
}
