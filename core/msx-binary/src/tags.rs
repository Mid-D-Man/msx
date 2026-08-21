// core/msx-binary/src/tags.rs
//! Wire-format type tags. These values are part of the binary contract —
//! never renumber an existing tag; only append new ones at the end of
//! their respective groups.

// ── Element type tags ───────────────────────────────────────────────────────
pub const TAG_RECT:            u8 = 0x00;
pub const TAG_CIRCLE:          u8 = 0x01;
pub const TAG_ELLIPSE:         u8 = 0x02;
pub const TAG_LINE:            u8 = 0x03;
pub const TAG_POLYLINE:        u8 = 0x04;
pub const TAG_POLYGON:         u8 = 0x05;
pub const TAG_PATH:            u8 = 0x06;
pub const TAG_TEXT:            u8 = 0x07;
pub const TAG_GROUP:           u8 = 0x08;
pub const TAG_USE:             u8 = 0x09;
pub const TAG_LINEAR_GRADIENT: u8 = 0x0A; // def only
pub const TAG_RADIAL_GRADIENT: u8 = 0x0B; // def only
pub const TAG_CONIC_GRADIENT:  u8 = 0x0C; // def only — v0.2
pub const TAG_SDF:             u8 = 0x0D; // v0.2
pub const TAG_SPLAT:           u8 = 0x0E; // v0.2
pub const TAG_LAYER:           u8 = 0x0F; // v0.2
pub const TAG_SHADER:          u8 = 0x10; // def only — v0.3
pub const TAG_IMAGE:           u8 = 0x11; // v0.4
pub const TAG_AUDIO:           u8 = 0x12; // def only — v0.4
pub const TAG_END:             u8 = 0xFF;

// ── Path command tags ───────────────────────────────────────────────────────
pub const CMD_MOVE_TO:       u8 = 0x00;
pub const CMD_LINE_TO:       u8 = 0x01;
pub const CMD_H_LINE_TO:     u8 = 0x02;
pub const CMD_V_LINE_TO:     u8 = 0x03;
pub const CMD_CUBIC:         u8 = 0x04;
pub const CMD_SMOOTH_CUBIC:  u8 = 0x05;
pub const CMD_QUAD:          u8 = 0x06;
pub const CMD_SMOOTH_QUAD:   u8 = 0x07;
pub const CMD_ARC:           u8 = 0x08;
// Relative = absolute + 0x10
pub const CMD_REL_MOVE_TO:      u8 = 0x10;
pub const CMD_REL_LINE_TO:      u8 = 0x11;
pub const CMD_REL_H_LINE_TO:    u8 = 0x12;
pub const CMD_REL_V_LINE_TO:    u8 = 0x13;
pub const CMD_REL_CUBIC:        u8 = 0x14;
pub const CMD_REL_SMOOTH_CUBIC: u8 = 0x15;
pub const CMD_REL_QUAD:         u8 = 0x16;
pub const CMD_REL_SMOOTH_QUAD:  u8 = 0x17;
pub const CMD_REL_ARC:          u8 = 0x18;
pub const CMD_CLOSE:            u8 = 0xFF;

// ── SDF tree node tags (v0.2) ────────────────────────────────────────────────
pub const SDF_TAG_CIRCLE:          u8 = 0x00;
pub const SDF_TAG_BOX:             u8 = 0x01;
pub const SDF_TAG_LINE:            u8 = 0x02;
pub const SDF_TAG_RING:            u8 = 0x03;
pub const SDF_TAG_ARC:             u8 = 0x04;
pub const SDF_TAG_UNION:           u8 = 0x05;
pub const SDF_TAG_SMOOTH_UNION:    u8 = 0x06;
pub const SDF_TAG_SUBTRACT:        u8 = 0x07;
pub const SDF_TAG_SMOOTH_SUBTRACT: u8 = 0x08;
pub const SDF_TAG_INTERSECT:       u8 = 0x09;
pub const SDF_TAG_SMOOTH_INTERSECT: u8 = 0x0A;
pub const SDF_TAG_OFFSET:          u8 = 0x0B;

// ── Effect node tags (v0.2) — mirrors EffectType discriminant values ────────
pub const EFFECT_TAG_BLUR:         u8 = 0x00;
pub const EFFECT_TAG_DROP_SHADOW:  u8 = 0x01;
pub const EFFECT_TAG_INNER_SHADOW: u8 = 0x02;
pub const EFFECT_TAG_OUTER_GLOW:   u8 = 0x03;
pub const EFFECT_TAG_INNER_GLOW:   u8 = 0x04;
