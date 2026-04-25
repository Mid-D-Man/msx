// src/header.rs

pub const MAGIC: [u8; 4] = [0x4D, 0x53, 0x58, 0x00]; // "MSX\0"
pub const VERSION: u8 = 1;
pub const HEADER_SIZE: usize = 32;

// compress field
pub const COMPRESS_NONE: u8 = 0;
pub const COMPRESS_MBFA: u8 = 1;

// flags bits
pub const FLAG_HAS_VIEWBOX:   u8 = 0b0000_0001;
pub const FLAG_HAS_METADATA:  u8 = 0b0000_0010;
pub const FLAG_HAS_DEFS:      u8 = 0b0000_0100;

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
            version:      VERSION,
            compress:     COMPRESS_MBFA,
            flags:        0,
            width,
            height,
            elem_count:   0,
            str_pool_len: 0,
            def_count:    0,
        }
    }

    pub fn has_viewbox(&self)  -> bool { self.flags & FLAG_HAS_VIEWBOX  != 0 }
    pub fn has_metadata(&self) -> bool { self.flags & FLAG_HAS_METADATA != 0 }
    pub fn has_defs(&self)     -> bool { self.flags & FLAG_HAS_DEFS     != 0 }

    pub fn set_viewbox(&mut self,  v: bool) { set_flag(&mut self.flags, FLAG_HAS_VIEWBOX,  v); }
    pub fn set_metadata(&mut self, v: bool) { set_flag(&mut self.flags, FLAG_HAS_METADATA, v); }
    pub fn set_defs(&mut self,     v: bool) { set_flag(&mut self.flags, FLAG_HAS_DEFS,     v); }

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
        if data[4] != VERSION {
            return Err(e(format!("unsupported MSX version {}", data[4])));
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
