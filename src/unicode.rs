//! Unicode text-file decoding helpers for NOVA Editor.
//!
//! NOVA stores text as UTF-8. When opening existing text files, UTF-8 (with or
//! without BOM) and UTF-16 LE/BE files are decoded without losing Vietnamese
//! combining marks or other Unicode characters.

use std::io;

pub fn decode_text_file(bytes: &[u8]) -> io::Result<String> {
    if let Some(payload) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(payload.to_vec()).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8 text: {error}"))
        });
    }

    if let Some(payload) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(payload, true);
    }
    if let Some(payload) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(payload, false);
    }

    String::from_utf8(bytes.to_vec()).map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Unsupported text encoding (expected UTF-8 or UTF-16 with BOM): {error}"))
    })
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> io::Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "UTF-16 text contains an incomplete code unit."));
    }
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-16 text: {error}")))
}

#[cfg(test)]
mod tests {
    use super::decode_text_file;

    #[test]
    fn decodes_vietnamese_utf8_without_bom() {
        assert_eq!(decode_text_file("Tiếng Việt – xin chào".as_bytes()).unwrap(), "Tiếng Việt – xin chào");
    }

    #[test]
    fn decodes_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("NOVA tiếng Việt".as_bytes());
        assert_eq!(decode_text_file(&bytes).unwrap(), "NOVA tiếng Việt");
    }

    #[test]
    fn decodes_utf16_little_and_big_endian() {
        let text = "Tiếng Việt 日本語";
        let mut little = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() { little.extend_from_slice(&unit.to_le_bytes()); }
        let mut big = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() { big.extend_from_slice(&unit.to_be_bytes()); }
        assert_eq!(decode_text_file(&little).unwrap(), text);
        assert_eq!(decode_text_file(&big).unwrap(), text);
    }

    #[test]
    fn decodes_non_bmp_unicode_surrogate_pairs() {
        let text = "NOVA 🚀 Tiếng Việt";
        let mut little = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() { little.extend_from_slice(&unit.to_le_bytes()); }
        let mut big = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() { big.extend_from_slice(&unit.to_be_bytes()); }
        assert_eq!(decode_text_file(&little).unwrap(), text);
        assert_eq!(decode_text_file(&big).unwrap(), text);
    }

    #[test]
    fn rejects_truncated_or_invalid_unicode() {
        assert!(decode_text_file(&[0xFF, 0xFE, 0x41]).is_err());
        assert!(decode_text_file(&[0xFF, 0xFE, 0x00, 0xD8]).is_err());
        assert!(decode_text_file(&[0xFF, 0xFE, 0x41, 0x00]).is_ok());
    }
}
