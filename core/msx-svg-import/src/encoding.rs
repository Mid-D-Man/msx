// core/msx-svg-import/src/encoding.rs
//! Text encodings of SVG files.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Encodings". An SVG file is
//! XML, so it can be UTF-8, UTF-16 (with or without a byte order mark) or any
//! encoding its XML declaration names. This module turns the bytes into the
//! text the importer reads. It covers UTF-8, UTF-16, ASCII, ISO-8859-1,
//! ISO-8859-15, windows-1252 and windows-1251, and refuses other names
//! instead of guessing.

/// Whether `bytes` start like an XML document in a supported framing: a byte
/// order mark, or `<` as the first character.
pub fn looks_like_xml(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(b"<\0") || bytes.starts_with(b"\0<") {
        return true;
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

/// The text of an XML document, decoded from `bytes`.
pub fn decode_xml(bytes: &[u8]) -> Result<String, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return from_utf8(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return from_utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return from_utf16(rest, false);
    }
    if bytes.starts_with(b"<\0") {
        return from_utf16(bytes, true);
    }
    if bytes.starts_with(b"\0<") {
        return from_utf16(bytes, false);
    }
    let Some(name) = declared_encoding(bytes) else {
        return from_utf8(bytes);
    };
    let text = match name.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" => return from_utf8(bytes),
        "iso-8859-1" | "iso8859-1" | "latin1" | "l1" => bytes.iter().map(|&b| char::from(b)).collect::<String>(),
        "iso-8859-15" | "latin9" => bytes.iter().map(|&b| latin9(b)).collect(),
        "windows-1252" | "cp1252" => bytes.iter().map(|&b| windows_1252(b)).collect(),
        "windows-1251" | "cp1251" => bytes.iter().map(|&b| windows_1251(b)).collect(),
        "utf-16" | "utf-16le" | "utf-16be" => return Err(format!("the file declares {name} but has no byte order mark and is not UTF-16")),
        other => return Err(format!("the encoding {other} is not supported (UTF-8, UTF-16, ASCII, ISO-8859-1, ISO-8859-15, windows-1252 and windows-1251 are)")),
    };
    Ok(retag_utf8(&text))
}

fn from_utf8(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes).map(str::to_string).map_err(|_| "the text is not valid UTF-8".to_string())
}

fn from_utf16(bytes: &[u8], little_endian: bool) -> Result<String, String> {
    if bytes.len() % 2 != 0 {
        return Err("the UTF-16 text has an odd number of bytes".into());
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| if little_endian { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) })
        .collect();
    let text = String::from_utf16(&units).map_err(|_| "the text is not valid UTF-16".to_string())?;
    Ok(retag_utf8(&text))
}

/// The encoding named in the XML declaration, lower case.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(256)];
    let head: String = head.iter().map(|&b| if b.is_ascii() { char::from(b) } else { '?' }).collect();
    let decl_end = head.find("?>")?;
    let decl = head.strip_prefix("<?xml").map(|_| &head[..decl_end])?;
    let at = decl.find("encoding")?;
    let rest = decl[at + "encoding".len()..].trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &rest[1..];
    Some(value[..value.find(quote)?].trim().to_ascii_lowercase())
}

/// The text is UTF-8 now, so a declaration that names another encoding would
/// be wrong. It is rewritten.
fn retag_utf8(text: &str) -> String {
    let Some(end) = text.find("?>") else { return text.to_string() };
    if !text.trim_start().starts_with("<?xml") {
        return text.to_string();
    }
    let decl = &text[..end];
    let Some(at) = decl.find("encoding") else { return text.to_string() };
    let after = &decl[at + "encoding".len()..];
    let Some(eq) = after.find('=') else { return text.to_string() };
    let value_start = at + "encoding".len() + eq + 1;
    let tail = decl[value_start..].trim_start();
    let Some(quote) = tail.chars().next().filter(|c| *c == '"' || *c == '\'') else { return text.to_string() };
    let from = value_start + (decl[value_start..].len() - tail.len()) + 1;
    let Some(len) = decl[from..].find(quote) else { return text.to_string() };
    format!("{}UTF-8{}", &text[..from], &text[from + len..])
}

fn latin9(b: u8) -> char {
    match b {
        0xA4 => '\u{20AC}',
        0xA6 => '\u{0160}',
        0xA8 => '\u{0161}',
        0xB4 => '\u{017D}',
        0xB8 => '\u{017E}',
        0xBC => '\u{0152}',
        0xBD => '\u{0153}',
        0xBE => '\u{0178}',
        _ => char::from(b),
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [u32; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D, 0x017D, 0x008F,
        0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[usize::from(b - 0x80)]).unwrap_or('\u{FFFD}'),
        _ => char::from(b),
    }
}

fn windows_1251(b: u8) -> char {
    const LOW: [u32; 64] = [
        0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039, 0x040A, 0x040C, 0x040B, 0x040F,
        0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x0098, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F,
        0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
        0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,
    ];
    match b {
        0x80..=0xBF => char::from_u32(LOW[usize::from(b - 0x80)]).unwrap_or('\u{FFFD}'),
        0xC0..=0xFF => char::from_u32(0x0410 + u32::from(b - 0xC0)).unwrap_or('\u{FFFD}'),
        _ => char::from(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_with_and_without_a_mark() {
        assert_eq!(decode_xml("<svg>é</svg>".as_bytes()).unwrap(), "<svg>é</svg>");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice("<svg>é</svg>".as_bytes());
        assert_eq!(decode_xml(&with_bom).unwrap(), "<svg>é</svg>");
        assert!(decode_xml(&[b'<', b's', 0xFF, b'>']).is_err(), "invalid UTF-8 without a declaration is refused");
    }

    #[test]
    fn utf16_both_orders_with_and_without_a_mark() {
        let text = "<svg>é日</svg>";
        let units: Vec<u16> = text.encode_utf16().collect();
        let le: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        let be: Vec<u8> = units.iter().flat_map(|u| u.to_be_bytes()).collect();
        let with = |mark: [u8; 2], body: &[u8]| [mark.as_slice(), body].concat();
        assert_eq!(decode_xml(&with([0xFF, 0xFE], &le)).unwrap(), text);
        assert_eq!(decode_xml(&with([0xFE, 0xFF], &be)).unwrap(), text);
        assert_eq!(decode_xml(&le).unwrap(), text);
        assert_eq!(decode_xml(&be).unwrap(), text);
        assert!(decode_xml(&[0xFF, 0xFE, 0x3C]).is_err());
    }

    #[test]
    fn declared_single_byte_encodings() {
        let latin1 = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><svg>caf\xE9</svg>";
        assert_eq!(decode_xml(latin1).unwrap(), "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg>café</svg>");
        let cp1252 = b"<?xml version='1.0' encoding='windows-1252'?><svg>\x80 \x93x\x94</svg>";
        assert_eq!(decode_xml(cp1252).unwrap(), "<?xml version='1.0' encoding='UTF-8'?><svg>€ \u{201C}x\u{201D}</svg>");
        let latin9 = b"<?xml version=\"1.0\" encoding=\"iso-8859-15\"?><svg>\xA4</svg>";
        assert!(decode_xml(latin9).unwrap().contains('€'));
    }

    #[test]
    fn windows_1251_reads_cyrillic() {
        // "Привет ё" in windows-1251
        let bytes = b"<?xml version=\"1.0\" encoding=\"Windows-1251\"?><svg>\xCF\xF0\xE8\xE2\xE5\xF2 \xB8</svg>";
        assert_eq!(decode_xml(bytes).unwrap(), "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg>Привет ё</svg>");
    }

    #[test]
    fn unknown_encodings_are_refused() {
        let err = decode_xml(b"<?xml version=\"1.0\" encoding=\"Shift_JIS\"?><svg/>").unwrap_err();
        assert!(err.contains("shift_jis") && err.contains("not supported"), "{err}");
    }

    #[test]
    fn a_declaration_of_utf8_is_left_alone() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg>é</svg>";
        assert_eq!(decode_xml(text.as_bytes()).unwrap(), text);
    }

    #[test]
    fn xml_framing_is_recognised() {
        assert!(looks_like_xml(b"<svg/>"));
        assert!(looks_like_xml(b"  \n<?xml version=\"1.0\"?>"));
        assert!(looks_like_xml(&[0xEF, 0xBB, 0xBF, b'<']));
        assert!(looks_like_xml(&[0xFF, 0xFE, b'<', 0]));
        assert!(looks_like_xml(b"<\0s\0"));
        assert!(!looks_like_xml(b"@CONFIG(version -> \"1.0.0\")"));
        assert!(!looks_like_xml(b"MSX\0"));
    }
}
