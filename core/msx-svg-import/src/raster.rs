// core/msx-svg-import/src/raster.rs
//! Raster images for `<image>`: reading a `data:` URI, telling PNG and JPEG
//! from everything else, measuring them without decoding, and working out how
//! `preserveAspectRatio` places them in the box the SVG gave.

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};

/// What the first bytes of an image say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    /// A format MSX cannot hold, named for the warning.
    Other(&'static str),
}

/// Bytes carried by a `data:` URI, or `None` when `href` is not one. A
/// malformed URI is `Some(Err(..))`. Base64 may contain line breaks and spaces,
/// and any padding, as in hand written and exported files.
pub(crate) fn decode_data_uri(href: &str) -> Option<Result<Vec<u8>, String>> {
    let href = href.trim();
    if !href.get(..5)?.eq_ignore_ascii_case("data:") {
        return None;
    }
    let Some((meta, payload)) = href[5..].split_once(',') else {
        return Some(Err("the data URI has no comma".into()));
    };
    let base64 = meta.split(';').any(|p| p.trim().eq_ignore_ascii_case("base64"));
    if !base64 {
        return Some(Ok(percent_decode(payload)));
    }
    let clean: String = payload.chars().filter(|c| !c.is_whitespace() && *c != '=').collect();
    Some(STANDARD_NO_PAD.decode(clean).map_err(|e| format!("invalid base64 ({e})")))
}

/// Decodes `%XX` escapes. A stray `%` stays as it is.
pub(crate) fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() && hex(b[i + 1]).is_some() && hex(b[i + 2]).is_some() {
            out.push((hex(b[i + 1]).unwrap() * 16 + hex(b[i + 2]).unwrap()) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// The format of `b`, from its first bytes.
pub(crate) fn sniff(b: &[u8]) -> Format {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Format::Png
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Format::Jpeg
    } else if b.starts_with(b"GIF8") {
        Format::Other("GIF")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Format::Other("WebP")
    } else if b.starts_with(&[0x1F, 0x8B]) {
        Format::Other("compressed SVG")
    } else if String::from_utf8_lossy(&b[..b.len().min(2048)]).contains("<svg") {
        Format::Other("SVG")
    } else {
        Format::Other("unknown")
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be16(b: &[u8]) -> u32 {
    u16::from_be_bytes([b[0], b[1]]) as u32
}

/// Width and height in pixels from the header, without decoding the image.
pub(crate) fn dimensions(b: &[u8], format: Format) -> Option<(u32, u32)> {
    let (w, h) = match format {
        Format::Png => {
            if b.len() < 24 || &b[12..16] != b"IHDR" {
                return None;
            }
            (be32(&b[16..20]), be32(&b[20..24]))
        }
        Format::Jpeg => jpeg_size(b)?,
        Format::Other(_) => return None,
    };
    (w > 0 && h > 0).then_some((w, h))
}

/// Walks the JPEG segments to the first start-of-frame marker.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        i += 2;
        // Markers without a length: standalone, restart and the start of image.
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            continue;
        }
        if i + 2 > b.len() {
            return None;
        }
        let len = be16(&b[i..]) as usize;
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            if i + 7 > b.len() {
                return None;
            }
            return Some((be16(&b[i + 5..]), be16(&b[i + 3..])));
        }
        i += len.max(2);
    }
    None
}

/// A parsed `preserveAspectRatio`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AspectRatio {
    /// Where the image sits in the spare room, 0 to 1 across and down, or
    /// `None` for `none`, which stretches.
    pub align: Option<(f64, f64)>,
    pub slice: bool,
}

pub(crate) fn parse_aspect_ratio(v: &str) -> AspectRatio {
    let mut tokens = v.split_whitespace().filter(|t| *t != "defer");
    let align = tokens.next().unwrap_or("xMidYMid");
    let slice = tokens.next() == Some("slice");
    let axis = |s: &str| match s {
        "Min" => 0.0,
        "Max" => 1.0,
        _ => 0.5,
    };
    let align = if align == "none" {
        None
    } else if align.len() == 8 && align.starts_with('x') && &align[4..5] == "Y" {
        Some((axis(&align[1..4]), axis(&align[5..8])))
    } else {
        Some((0.5, 0.5))
    };
    AspectRatio { align, slice }
}

/// Where the image goes inside the box `(x, y, w, h)`, as `(x, y, w, h)`, and
/// whether it spills over the box and so needs a clip.
pub(crate) fn fit(bx: (f64, f64, f64, f64), intrinsic: (f64, f64), par: AspectRatio) -> ((f64, f64, f64, f64), bool) {
    let (x, y, w, h) = bx;
    let Some((ax, ay)) = par.align else { return (bx, false) };
    let (sx, sy) = (w / intrinsic.0, h / intrinsic.1);
    let s = if par.slice { sx.max(sy) } else { sx.min(sy) };
    let (dw, dh) = (intrinsic.0 * s, intrinsic.1 * s);
    let spill = dw > w + 1e-9 || dh > h + 1e-9;
    ((x + (w - dw) * ax, y + (h - dh) * ay, dw, dh), spill)
}

/// Builders for fake image bytes, shared with the importer tests.
#[cfg(test)]
pub(crate) mod tests_support {
    /// A PNG signature and an IHDR chunk, enough to sniff and measure.
    pub(crate) fn fake_png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    /// A JPEG with a JFIF segment and a baseline start-of-frame segment.
    pub(crate) fn fake_jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        b.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{fake_jpeg, fake_png};
    use super::*;

    #[test]
    fn data_uri_decodes_base64_with_breaks_spaces_and_any_padding() {
        let ok = |s: &str| decode_data_uri(s).unwrap().unwrap();
        assert_eq!(ok("data:image/png;base64,aGVsbG8="), b"hello");
        assert_eq!(ok("data:image/png;base64,aGVs\n  bG8"), b"hello", "line breaks, spaces and missing padding");
        assert_eq!(ok("DATA:;BASE64,aGk="), b"hi");
        assert_eq!(ok("data:text/plain,a%20b%2"), b"a b%2");
        assert!(decode_data_uri("image.png").is_none());
        assert!(decode_data_uri("data:image/png;base64").unwrap().is_err());
        assert!(decode_data_uri("data:image/png;base64,@@@").unwrap().is_err());
    }

    #[test]
    fn sniff_tells_the_formats_apart() {
        assert_eq!(sniff(&fake_png(1, 1)), Format::Png);
        assert_eq!(sniff(&fake_jpeg(1, 1)), Format::Jpeg);
        assert_eq!(sniff(b"GIF89a...."), Format::Other("GIF"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Format::Other("WebP"));
        assert_eq!(sniff(b"<?xml version='1.0'?><svg xmlns='x'/>"), Format::Other("SVG"));
        assert_eq!(sniff(&[0x1F, 0x8B, 8]), Format::Other("compressed SVG"));
        assert_eq!(sniff(b"hello"), Format::Other("unknown"));
    }

    #[test]
    fn dimensions_come_from_the_header_of_both_formats() {
        assert_eq!(dimensions(&fake_png(40, 20), Format::Png), Some((40, 20)));
        assert_eq!(dimensions(&fake_jpeg(300, 200), Format::Jpeg), Some((300, 200)));
        assert_eq!(dimensions(&fake_png(0, 20), Format::Png), None, "a zero side is not a size");
        assert_eq!(dimensions(&[0x89, b'P', b'N', b'G'], Format::Png), None);
        assert_eq!(dimensions(&[0xFF, 0xD8, 0xFF, 0xE0], Format::Jpeg), None);
        assert_eq!(dimensions(b"GIF89a", Format::Other("GIF")), None);
    }

    #[test]
    fn aspect_ratio_parses_alignment_and_mode() {
        assert_eq!(parse_aspect_ratio(""), AspectRatio { align: Some((0.5, 0.5)), slice: false });
        assert_eq!(parse_aspect_ratio("none"), AspectRatio { align: None, slice: false });
        assert_eq!(parse_aspect_ratio("xMaxYMin slice"), AspectRatio { align: Some((1.0, 0.0)), slice: true });
        assert_eq!(parse_aspect_ratio("defer xMinYMax meet"), AspectRatio { align: Some((0.0, 1.0)), slice: false });
        assert_eq!(parse_aspect_ratio("garbage"), AspectRatio { align: Some((0.5, 0.5)), slice: false });
    }

    #[test]
    fn fit_places_the_image_in_the_box() {
        let bx = (10.0, 20.0, 100.0, 100.0);
        let wide = (40.0, 20.0);
        let meet = |a: &str| fit(bx, wide, parse_aspect_ratio(a));
        assert_eq!(meet("xMidYMid"), ((10.0, 45.0, 100.0, 50.0), false));
        assert_eq!(meet("xMinYMin"), ((10.0, 20.0, 100.0, 50.0), false));
        assert_eq!(meet("xMaxYMax"), ((10.0, 70.0, 100.0, 50.0), false));
        assert_eq!(meet("none"), (bx, false));
        let slice = fit(bx, wide, parse_aspect_ratio("xMidYMid slice"));
        assert_eq!(slice, ((-40.0, 20.0, 200.0, 100.0), true), "scaled to cover, centered, spilling");
        let tall = fit(bx, (20.0, 40.0), parse_aspect_ratio("xMaxYMax slice"));
        assert_eq!(tall, ((10.0, -80.0, 100.0, 200.0), true));
    }
}
