// core/msx-binary/src/lib.rs
//! MSX binary format — Scene ↔ binary encode/decode, MBFA compression.

pub mod compiler;
pub mod decoder;
pub mod effect_codec;
pub mod encoder;
pub mod header;
pub mod path_codec;
pub mod scene_decode;
pub mod sdf_codec;
pub mod tags;

pub use compiler::{compile, compile_stats};
pub use header::{MsxHeader, COMPRESS_MBFA, COMPRESS_NONE, HEADER_SIZE, VERSION};
pub use scene_decode::decode;
