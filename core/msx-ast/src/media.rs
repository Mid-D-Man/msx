// core/msx-ast/src/media.rs
//! Shared infrastructure for anything that carries embedded or
//! externally-referenced binary media — currently `Element::Image` and
//! `Def::Audio`, and any future media-carrying construct without
//! duplicating the embed/file-ref choice per type.
//!
//! Two deliberate design calls, both worth stating explicitly rather
//! than leaving implicit:
//!
//! 1. **No stored format field.** A `MediaSource` never says "this is a
//!    PNG" or "this is a WAV" — the format is sniffed from the blob's
//!    own magic bytes wherever it's actually needed (see `ImageFormat::
//!    sniff` below), the same way `msx-cli`'s own `load_scene_bytes`
//!    already tells a compiled MSX binary apart from DixScript source
//!    text by its first four bytes (`b"MSX\0"`), rather than needing a
//!    caller to say which one it's looking at. A `FileRef`'s bytes
//!    aren't even available until a renderer actually reads the file at
//!    render time, so a format field on `MediaSource` itself couldn't be
//!    populated at parse time for that variant anyway — sniffing at the
//!    point of use is the only option that works uniformly for both
//!    variants.
//! 2. **Base64 only ever exists at the DixScript-source-text boundary.**
//!    `MediaSource::Embedded` holds already-decoded raw bytes — base64
//!    decoding happens once, in `msx-parser`, turning a DixScript string
//!    literal into a `Vec<u8>`. Neither this crate nor the compiled
//!    binary format (`msx-binary`) ever touches base64: the binary
//!    format writes/reads these raw bytes directly (their own
//!    u32-length-prefixed block, deliberately not the shared string
//!    pool — see `msx-binary::compiler::encode_media_source`'s own doc
//!    comment for why: the same u16-per-entry length-prefix truncation
//!    risk this project already found and fixed once for `Path::d_raw`).
//!    This crate has zero external dependencies by design (see this
//!    crate's own `Cargo.toml`) — base64 handling living one layer up in
//!    `msx-parser` instead of here is what keeps that true.

/// Where a piece of embedded binary media actually lives.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaSource {
    /// Raw bytes, decoded from an embedded base64 DixScript string
    /// literal at parse time. Self-contained — the `.msx` file, and its
    /// compiled binary, carry everything needed to render this, no
    /// external file required alongside it.
    Embedded(Vec<u8>),
    /// A file path, resolved against a base directory the renderer
    /// supplies at render/decode time — the SAME convention
    /// `Def::Shader`'s own `source_ref` already uses (same field name,
    /// on purpose, for the same reason: a large external asset
    /// referenced by path instead of bloated into the `.msx` file
    /// itself). Not embedded in the `.msx` file or its compiled binary
    /// at all; the referenced file must be present alongside wherever
    /// the `.msx` file is loaded from.
    FileRef(String),
}

/// Sniffed from a blob's own leading bytes — see this module's doc
/// comment for why nothing stores this explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    /// `None` if `bytes` doesn't start with a recognized PNG or JPEG
    /// signature — callers decide what "unrecognized" means for them
    /// (a hard parse error for an `Embedded` blob that's supposed to be
    /// an image; a softer fallback for a `FileRef` a renderer is about
    /// to hand to a real decoder anyway).
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        if bytes.starts_with(&PNG_SIG) {
            Some(ImageFormat::Png)
        } else if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            Some(ImageFormat::Jpeg)
        } else {
            None
        }
    }

    /// For SVG's `data:` URI — unlike the `image` crate's own internal
    /// format sniffing (which `msx-render-cpu` leans on directly, see
    /// its `image.rs`), a browser does NOT sniff a `data:` URI's actual
    /// bytes to figure out the image type; the MIME type in the URI
    /// itself is authoritative. This is the one place in the whole
    /// pipeline where knowing Png-vs-Jpeg explicitly, rather than just
    /// handing raw bytes to a real decoder, is actually required.
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
}

/// Best-effort, deliberately not exhaustive — see `Def::Audio`'s own doc
/// comment for why audio's format detection stays intentionally light
/// (no renderer in this project plays sound; this exists for basic
/// validation, not to back real audio decoding the way `ImageFormat`
/// backs real image decoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Ogg,
    /// ID3-tagged MP3 specifically — a bare MP3 frame stream with no ID3
    /// tag has no reliable fixed magic-byte signature to sniff (frame
    /// sync bytes look too similar to other content to trust), so that
    /// case sniffs as `None` rather than guessing.
    Mp3,
}

impl AudioFormat {
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            Some(AudioFormat::Wav)
        } else if bytes.starts_with(b"OggS") {
            Some(AudioFormat::Ogg)
        } else if bytes.starts_with(b"ID3") {
            Some(AudioFormat::Mp3)
        } else {
            None
        }
    }
}

/// A 9-point anchor, the same convention most design/layout tools use —
/// deliberately not a full flexbox-style layout system, just "which
/// point of the image's own bounding box lands on `(x, y)`."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// DixScript-facing string form — same naming convention as
    /// `LoopMode`/`Easing`'s own `to_dixscript_str`/`parse` pairs
    /// (snake_case, e.g. `"top_left"`, `"bottom_right"`).
    pub fn parse(s: &str) -> Self {
        match s {
            "top" => Anchor::Top,
            "top_right" => Anchor::TopRight,
            "left" => Anchor::Left,
            "center" => Anchor::Center,
            "right" => Anchor::Right,
            "bottom_left" => Anchor::BottomLeft,
            "bottom" => Anchor::Bottom,
            "bottom_right" => Anchor::BottomRight,
            // "top_left" and anything unrecognized both fall back to the
            // default — same "don't hard-error on a typo'd enum-ish
            // string" convention `FillRule::parse` already uses.
            _ => Anchor::TopLeft,
        }
    }

    pub fn to_dixscript_str(self) -> &'static str {
        match self {
            Anchor::TopLeft => "top_left",
            Anchor::Top => "top",
            Anchor::TopRight => "top_right",
            Anchor::Left => "left",
            Anchor::Center => "center",
            Anchor::Right => "right",
            Anchor::BottomLeft => "bottom_left",
            Anchor::Bottom => "bottom",
            Anchor::BottomRight => "bottom_right",
        }
    }

    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Anchor::Top,
            2 => Anchor::TopRight,
            3 => Anchor::Left,
            4 => Anchor::Center,
            5 => Anchor::Right,
            6 => Anchor::BottomLeft,
            7 => Anchor::Bottom,
            8 => Anchor::BottomRight,
            _ => Anchor::TopLeft,
        }
    }

    /// The actual top-left rendering position for a `width` × `height`
    /// image anchored at `(x, y)` — every renderer calls this SAME
    /// function rather than each reimplementing the anchor math, so
    /// "center", "bottom_right", etc. can't silently drift apart between
    /// SVG/CPU/GPU the way something reimplemented three times could.
    pub fn top_left_for(self, x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
        let (fx, fy) = match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::Top => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::Left => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::Right => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::Bottom => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        };
        (x - fx * width, y - fy * height)
    }
}

/// A named audio resource — modeled as a `Def` (like `Def::Shader`), not
/// an `Element`: audio has no meaningful canvas position, so it doesn't
/// belong in the positioned-visual-element tree the way `Element::Image`
/// does. Round-trips fully through parsing and the binary format via the
/// exact same `MediaSource` machinery `Element::Image` uses — genuine
/// shared infrastructure, not a stub.
///
/// It is, deliberately, a complete no-op in every renderer, the same
/// established way `Element::Text` already is (see
/// `msx-render-gpu/src/lib.rs`'s own module doc: `"Text` is a deliberate
/// no-op everywhere in this project"`). Nothing in this project has any
/// notion of "trigger this sound" — no element has a field referencing
/// an audio def's `id` the way a shape's `fill` can reference a
/// gradient's — so today this is inert, declarable, round-trippable
/// associated data with no consumer. That's the honest answer to
/// "feasible": the plumbing is real and complete, the playback isn't,
/// because nothing in this project plays audio yet.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDef {
    pub id: String,
    pub source: MediaSource,
}

impl AudioDef {
    pub fn new(id: impl Into<String>, source: MediaSource) -> Self {
        AudioDef { id: id.into(), source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_signature_sniffs_as_png() {
        let bytes = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0];
        assert_eq!(ImageFormat::sniff(&bytes), Some(ImageFormat::Png));
    }

    #[test]
    fn jpeg_signature_sniffs_as_jpeg() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
        assert_eq!(ImageFormat::sniff(&bytes), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn garbage_sniffs_as_none() {
        assert_eq!(ImageFormat::sniff(b"not an image"), None);
    }

    #[test]
    fn short_input_does_not_panic() {
        assert_eq!(ImageFormat::sniff(&[0xFF]), None);
        assert_eq!(ImageFormat::sniff(&[]), None);
    }

    #[test]
    fn wav_signature_sniffs_as_wav() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]); // chunk size, irrelevant here
        bytes.extend_from_slice(b"WAVE");
        assert_eq!(AudioFormat::sniff(&bytes), Some(AudioFormat::Wav));
    }

    #[test]
    fn ogg_signature_sniffs_as_ogg() {
        assert_eq!(AudioFormat::sniff(b"OggS0000"), Some(AudioFormat::Ogg));
    }

    #[test]
    fn id3_tagged_mp3_sniffs_as_mp3() {
        assert_eq!(AudioFormat::sniff(b"ID3\x03\x00\x00\x00"), Some(AudioFormat::Mp3));
    }

    #[test]
    fn bare_mp3_frame_sync_sniffs_as_none() {
        // No ID3 tag — deliberately NOT guessed at, see this type's own
        // doc comment.
        assert_eq!(AudioFormat::sniff(&[0xFF, 0xFB, 0x90, 0x00]), None);
    }

    #[test]
    fn anchor_byte_roundtrip() {
        for a in [
            Anchor::TopLeft, Anchor::Top, Anchor::TopRight,
            Anchor::Left, Anchor::Center, Anchor::Right,
            Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight,
        ] {
            assert_eq!(Anchor::from_byte(a.to_byte()), a);
        }
    }

    #[test]
    fn anchor_dixscript_str_roundtrip() {
        for a in [
            Anchor::TopLeft, Anchor::Top, Anchor::TopRight,
            Anchor::Left, Anchor::Center, Anchor::Right,
            Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight,
        ] {
            assert_eq!(Anchor::parse(a.to_dixscript_str()), a);
        }
    }

    #[test]
    fn top_left_anchor_is_the_identity() {
        assert_eq!(Anchor::TopLeft.top_left_for(10.0, 20.0, 100.0, 50.0), (10.0, 20.0));
    }

    #[test]
    fn center_anchor_offsets_by_half_extent() {
        assert_eq!(Anchor::Center.top_left_for(100.0, 100.0, 40.0, 20.0), (80.0, 90.0));
    }

    #[test]
    fn bottom_right_anchor_offsets_by_full_extent() {
        assert_eq!(Anchor::BottomRight.top_left_for(100.0, 100.0, 40.0, 20.0), (60.0, 80.0));
    }

    #[test]
    fn unrecognized_anchor_string_falls_back_to_top_left() {
        assert_eq!(Anchor::parse("not_a_real_anchor"), Anchor::TopLeft);
    }
}
