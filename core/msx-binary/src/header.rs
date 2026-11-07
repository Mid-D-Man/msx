// core/msx-binary/src/header.rs

pub const MAGIC: [u8; 4] = [0x4D, 0x53, 0x58, 0x00]; // "MSX\0"
/// The oldest wire version this crate reads, and the one it writes for a scene
/// that needs nothing newer. v2 added the Sdf/Splat/Layer/ConicGradient tags.
pub const MIN_VERSION: u8 = 2;
/// The newest wire version this crate reads. v3 adds the style extension block
/// (`fill_opacity`, `stroke_opacity`, `dominant_baseline`). A scene is written
/// with the lowest version that can hold it, see `version.rs`.
pub const VERSION: u8 = 3;
pub const HEADER_SIZE: usize = 32;

// compress field
pub const COMPRESS_NONE: u8 = 0;
pub const COMPRESS_MBFA: u8 = 1;

// flags bits
pub const FLAG_HAS_VIEWBOX:     u8 = 0b0000_0001;
pub const FLAG_HAS_METADATA:    u8 = 0b0000_0010;
pub const FLAG_HAS_DEFS:        u8 = 0b0000_0100;
pub const FLAG_HAS_ANIMATIONS:  u8 = 0b0000_1000; // v0.5 — animation section

#[derive(Debug, Clone)]
pub struct MsxHeader {
    pub version:      u8,
    pub compress:     u8,
    pub flags:        u8,
    pub width:        f32,
    pub height:       f32,
    pub elem_count:   u32,
    pub str_pool_len: u32,
    pub def_count:    u32,
}

impl MsxHeader {
    pub fn new(width: f32, height: f32) -> Self {
        MsxHeader {
            version:      MIN_VERSION,
            compress:     COMPRESS_MBFA,
            flags:        0,
            width,
            height,
            elem_count:   0,
            str_pool_len: 0,
            def_count:    0,
        }
    }

    pub fn has_viewbox(&self)    -> bool { self.flags & FLAG_HAS_VIEWBOX    != 0 }
    pub fn has_metadata(&self)   -> bool { self.flags & FLAG_HAS_METADATA   != 0 }
    pub fn has_defs(&self)       -> bool { self.flags & FLAG_HAS_DEFS       != 0 }
    pub fn has_animations(&self) -> bool { self.flags & FLAG_HAS_ANIMATIONS != 0 }

    pub fn set_viewbox(&mut self,    v: bool) { set_flag(&mut self.flags, FLAG_HAS_VIEWBOX,    v); }
    pub fn set_metadata(&mut self,   v: bool) { set_flag(&mut self.flags, FLAG_HAS_METADATA,   v); }
    pub fn set_defs(&mut self,       v: bool) { set_flag(&mut self.flags, FLAG_HAS_DEFS,       v); }
    pub fn set_animations(&mut self, v: bool) { set_flag(&mut self.flags, FLAG_HAS_ANIMATIONS, v); }

    pub fn serialize(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4] = self.version;
        buf[5] = self.compress;
        buf[6] = self.flags;
        buf[7] = 0; // reserved
        buf[8..12].copy_from_slice(&self.width.to_le_bytes());
        buf[12..16].copy_from_slice(&self.height.to_le_bytes());
        buf[16..20].copy_from_slice(&self.elem_count.to_le_bytes());
        buf[20..24].copy_from_slice(&self.str_pool_len.to_le_bytes());
        buf[24..28].copy_from_slice(&self.def_count.to_le_bytes());
        // buf[28..32] stays zero (reserved)
        buf
    }

    pub fn parse(data: &[u8]) -> std::io::Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(e("MSX header too short"));
        }
        if data[0..4] != MAGIC {
            return Err(e("not an MSX file (bad magic)"));
        }
        if !(MIN_VERSION..=VERSION).contains(&data[4]) {
            return Err(e(format!("unsupported MSX version {} (this reader handles {} to {})", data[4], MIN_VERSION, VERSION)));
        }
        let compress = data[5];
        if compress != COMPRESS_NONE && compress != COMPRESS_MBFA {
            return Err(e(format!("unknown compress mode {}", compress)));
        }
        Ok(MsxHeader {
            version:      data[4],
            compress:     data[5],
            flags:        data[6],
            width:        f32::from_le_bytes(data[8..12].try_into().unwrap()),
            height:       f32::from_le_bytes(data[12..16].try_into().unwrap()),
            elem_count:   u32::from_le_bytes(data[16..20].try_into().unwrap()),
            str_pool_len: u32::from_le_bytes(data[20..24].try_into().unwrap()),
            def_count:    u32::from_le_bytes(data[24..28].try_into().unwrap()),
        })
    }
}

#[inline]
fn set_flag(flags: &mut u8, bit: u8, value: bool) {
    if value { *flags |= bit; } else { *flags &= !bit; }
}

#[inline]
fn e(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_parse_roundtrip() {
        let mut h = MsxHeader::new(640.0, 480.0);
        h.elem_count = 5;
        h.set_viewbox(true);
        let bytes = h.serialize();
        let h2 = MsxHeader::parse(&bytes).unwrap();
        assert_eq!(h2.version, MIN_VERSION);
        assert!((h2.width - 640.0).abs() < 1e-4);
        assert_eq!(h2.elem_count, 5);
        assert!(h2.has_viewbox());
    }

    #[test]
    fn animations_flag_roundtrips() {
        let mut h = MsxHeader::new(320.0, 240.0);
        h.set_animations(true);
        let bytes = h.serialize();
        let h2 = MsxHeader::parse(&bytes).unwrap();
        assert!(h2.has_animations());
        assert!(!h2.has_viewbox(), "setting animations must not set unrelated flag bits");
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0] = 0x00;
        assert!(MsxHeader::parse(&bytes).is_err());
    }

    #[test]
    fn bad_version_rejected() {
        let mut h = MsxHeader::new(100.0, 100.0).serialize();
        h[4] = VERSION + 1;
        assert!(MsxHeader::parse(&h).is_err());
        h[4] = MIN_VERSION - 1;
        assert!(MsxHeader::parse(&h).is_err());
    }

    #[test]
    fn every_version_from_the_oldest_to_the_newest_is_read() {
        for v in MIN_VERSION..=VERSION {
            let mut h = MsxHeader::new(100.0, 100.0).serialize();
            h[4] = v;
            assert_eq!(MsxHeader::parse(&h).unwrap().version, v);
        }
    }
}
