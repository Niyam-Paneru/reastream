//! Tests for the decoding layer.
//!
//! Windows shell redirection produces UTF-16LE, which is why this layer exists
//! at all. The property worth testing is exactness: a stream must come out the
//! same length and content it went in as, in both encodings, regardless of how
//! the reads happen to be sized.

use std::io::{Cursor, Read};

use reastream::decoding::{detect, read_all, DecodingReader, Encoding, BOM_PREFIX};

fn read_all_bytes(input: &[u8], chunk: usize) -> (Vec<u8>, Option<Encoding>) {
    let mut reader = DecodingReader::new(Cursor::new(input.to_vec()));
    let mut out = Vec::new();
    let mut buffer = vec![0u8; chunk];
    loop {
        let read = reader.read(&mut buffer).expect("read failed");
        if read == 0 {
            break;
        }
        out.extend_from_slice(&buffer[..read]);
    }
    (out, reader.encoding())
}

#[test]
fn utf8_passes_through_unchanged() {
    let input = br#"{"a":"b","c":[1,2,3]}"#;
    for chunk in [1usize, 2, 3, 7, BOM_PREFIX, 4096] {
        let (out, encoding) = read_all_bytes(input, chunk);
        assert_eq!(out, input, "chunk {chunk} changed the bytes");
        assert_eq!(encoding, Some(Encoding::Utf8));
    }
}

#[test]
fn a_utf8_bom_is_stripped() {
    let mut input = vec![0xef, 0xbb, 0xbf];
    input.extend_from_slice(br#"{"a":1}"#);
    let (out, encoding) = read_all_bytes(&input, 4096);
    assert_eq!(out, br#"{"a":1}"#);
    assert_eq!(encoding, Some(Encoding::Utf8WithBom));
}

#[test]
fn utf16le_is_transcoded_to_utf8() {
    // This is the shape a captured REA document actually arrives in.
    let text = r#"{"a":"b"}"#;
    let mut input = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        input.extend_from_slice(&unit.to_le_bytes());
    }
    for chunk in [2usize, 3, 5, 8, 64, 4096] {
        let (out, encoding) = read_all_bytes(&input, chunk);
        assert_eq!(
            out,
            text.as_bytes(),
            "chunk {chunk} changed the transcoding"
        );
        assert_eq!(encoding, Some(Encoding::Utf16Le));
    }
}

#[test]
fn utf16be_is_transcoded_to_utf8() {
    let text = r#"{"a":"b"}"#;
    let mut input = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        input.extend_from_slice(&unit.to_be_bytes());
    }
    let (out, encoding) = read_all_bytes(&input, 4096);
    assert_eq!(out, text.as_bytes());
    assert_eq!(encoding, Some(Encoding::Utf16Be));
}

#[test]
fn non_ascii_survives_transcoding() {
    // The byte count is what the caller reports as "bytes kept", so a character
    // that survives as the wrong length would misreport the whole projection.
    let text = r#"{"name":"café — ☃"}"#;
    let mut input = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        input.extend_from_slice(&unit.to_le_bytes());
    }
    let (out, _) = read_all_bytes(&input, 4);
    assert_eq!(out, text.as_bytes());
}

#[test]
fn detect_reports_what_the_declared_bom_says() {
    assert_eq!(detect(b"\xff\xfe"), Encoding::Utf16Le);
    assert_eq!(detect(b"\xfe\xff"), Encoding::Utf16Be);
    assert_eq!(detect(b"\xef\xbb\xbf"), Encoding::Utf8WithBom);
    // Two bytes cannot declare a BOM, so UTF-8 is the only honest answer.
    assert_eq!(detect(b"{"), Encoding::Utf8);
    assert_eq!(detect(&[]), Encoding::Utf8);
}

#[test]
fn read_all_reports_length_and_encoding() {
    let text = r#"{"a":1}"#;
    let mut input = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        input.extend_from_slice(&unit.to_le_bytes());
    }
    let (out, encoding) = read_all(Cursor::new(input)).expect("read_all");
    assert_eq!(out, text.as_bytes());
    assert_eq!(encoding, Encoding::Utf16Le);
}

#[test]
fn a_reader_that_delivers_one_byte_at_a_time_still_decodes() {
    // Byte-at-a-time is the hardest case for the code-unit split.
    let text = r#"{"a":"b"}"#;
    let mut input = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        input.extend_from_slice(&unit.to_le_bytes());
    }
    let (out, _) = read_all_bytes(&input, 1);
    assert_eq!(out, text.as_bytes());
}

#[test]
fn unpaired_surrogates_become_replacement_characters() {
    // A lone surrogate half is not valid UTF-8; dropping it silently would move
    // every later byte, so it is replaced instead.
    let input = [0xff, 0xfe, 0x00, 0xd8, 0x41, 0x00];
    let (out, encoding) = read_all_bytes(&input, 4);
    assert_eq!(encoding, Some(Encoding::Utf16Le));
    assert_eq!(out, "\u{fffd}A".as_bytes());
}
