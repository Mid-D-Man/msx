// core/msx-binary/src/sdf_codec.rs
//! Binary encode/decode for SdfTree — recursive signed-distance-field nodes.

use std::io;
use msx_ast::SdfTree;

use crate::decoder::{read_f32, read_u16, read_u8};
use crate::encoder::{write_f32, write_u16, write_u8};
use crate::tags::*;

pub fn write_sdf_tree(out: &mut Vec<u8>, tree: &SdfTree) {
    match tree {
        SdfTree::Circle { cx, cy, r } => {
            write_u8(out, SDF_TAG_CIRCLE);
            write_f32(out, *cx); write_f32(out, *cy); write_f32(out, *r);
        }
        SdfTree::Box { x, y, width, height, corner_radius } => {
            write_u8(out, SDF_TAG_BOX);
            write_f32(out, *x); write_f32(out, *y);
            write_f32(out, *width); write_f32(out, *height);
            write_f32(out, *corner_radius);
        }
        SdfTree::Line { x1, y1, x2, y2, thickness } => {
            write_u8(out, SDF_TAG_LINE);
            write_f32(out, *x1); write_f32(out, *y1);
            write_f32(out, *x2); write_f32(out, *y2);
            write_f32(out, *thickness);
        }
        SdfTree::Ring { cx, cy, r, thickness } => {
            write_u8(out, SDF_TAG_RING);
            write_f32(out, *cx); write_f32(out, *cy);
            write_f32(out, *r);  write_f32(out, *thickness);
        }
        SdfTree::Arc { cx, cy, r, angle_start, angle_end, thickness } => {
            write_u8(out, SDF_TAG_ARC);
            write_f32(out, *cx); write_f32(out, *cy); write_f32(out, *r);
            write_f32(out, *angle_start); write_f32(out, *angle_end);
            write_f32(out, *thickness);
        }
        SdfTree::Union(children) => {
            write_u8(out, SDF_TAG_UNION);
            write_u16(out, children.len() as u16);
            for c in children { write_sdf_tree(out, c); }
        }
        SdfTree::SmoothUnion { children, k } => {
            write_u8(out, SDF_TAG_SMOOTH_UNION);
            write_f32(out, *k);
            write_u16(out, children.len() as u16);
            for c in children { write_sdf_tree(out, c); }
        }
        SdfTree::Subtract { a, b } => {
            write_u8(out, SDF_TAG_SUBTRACT);
            write_sdf_tree(out, a);
            write_sdf_tree(out, b);
        }
        SdfTree::SmoothSubtract { a, b, k } => {
            write_u8(out, SDF_TAG_SMOOTH_SUBTRACT);
            write_f32(out, *k);
            write_sdf_tree(out, a);
            write_sdf_tree(out, b);
        }
        SdfTree::Intersect { a, b } => {
            write_u8(out, SDF_TAG_INTERSECT);
            write_sdf_tree(out, a);
            write_sdf_tree(out, b);
        }
        SdfTree::SmoothIntersect { a, b, k } => {
            write_u8(out, SDF_TAG_SMOOTH_INTERSECT);
            write_f32(out, *k);
            write_sdf_tree(out, a);
            write_sdf_tree(out, b);
        }
        SdfTree::Offset { child, amount } => {
            write_u8(out, SDF_TAG_OFFSET);
            write_f32(out, *amount);
            write_sdf_tree(out, child);
        }
    }
}

pub fn read_sdf_tree(data: &[u8], cursor: &mut usize) -> io::Result<SdfTree> {
    let tag = read_u8(data, cursor)?;
    match tag {
        SDF_TAG_CIRCLE => {
            let cx = read_f32(data, cursor)?;
            let cy = read_f32(data, cursor)?;
            let r  = read_f32(data, cursor)?;
            Ok(SdfTree::Circle { cx, cy, r })
        }
        SDF_TAG_BOX => {
            let x = read_f32(data, cursor)?;
            let y = read_f32(data, cursor)?;
            let width  = read_f32(data, cursor)?;
            let height = read_f32(data, cursor)?;
            let corner_radius = read_f32(data, cursor)?;
            Ok(SdfTree::Box { x, y, width, height, corner_radius })
        }
        SDF_TAG_LINE => {
            let x1 = read_f32(data, cursor)?;
            let y1 = read_f32(data, cursor)?;
            let x2 = read_f32(data, cursor)?;
            let y2 = read_f32(data, cursor)?;
            let thickness = read_f32(data, cursor)?;
            Ok(SdfTree::Line { x1, y1, x2, y2, thickness })
        }
        SDF_TAG_RING => {
            let cx = read_f32(data, cursor)?;
            let cy = read_f32(data, cursor)?;
            let r  = read_f32(data, cursor)?;
            let thickness = read_f32(data, cursor)?;
            Ok(SdfTree::Ring { cx, cy, r, thickness })
        }
        SDF_TAG_ARC => {
            let cx = read_f32(data, cursor)?;
            let cy = read_f32(data, cursor)?;
            let r  = read_f32(data, cursor)?;
            let angle_start = read_f32(data, cursor)?;
            let angle_end   = read_f32(data, cursor)?;
            let thickness   = read_f32(data, cursor)?;
            Ok(SdfTree::Arc { cx, cy, r, angle_start, angle_end, thickness })
        }
        SDF_TAG_UNION => {
            let count = read_u16(data, cursor)? as usize;
            let mut children = Vec::with_capacity(count);
            for _ in 0..count { children.push(read_sdf_tree(data, cursor)?); }
            Ok(SdfTree::Union(children))
        }
        SDF_TAG_SMOOTH_UNION => {
            let k = read_f32(data, cursor)?;
            let count = read_u16(data, cursor)? as usize;
            let mut children = Vec::with_capacity(count);
            for _ in 0..count { children.push(read_sdf_tree(data, cursor)?); }
            Ok(SdfTree::SmoothUnion { children, k })
        }
        SDF_TAG_SUBTRACT => {
            let a = read_sdf_tree(data, cursor)?;
            let b = read_sdf_tree(data, cursor)?;
            Ok(SdfTree::Subtract { a: Box::new(a), b: Box::new(b) })
        }
        SDF_TAG_SMOOTH_SUBTRACT => {
            let k = read_f32(data, cursor)?;
            let a = read_sdf_tree(data, cursor)?;
            let b = read_sdf_tree(data, cursor)?;
            Ok(SdfTree::SmoothSubtract { a: Box::new(a), b: Box::new(b), k })
        }
        SDF_TAG_INTERSECT => {
            let a = read_sdf_tree(data, cursor)?;
            let b = read_sdf_tree(data, cursor)?;
            Ok(SdfTree::Intersect { a: Box::new(a), b: Box::new(b) })
        }
        SDF_TAG_SMOOTH_INTERSECT => {
            let k = read_f32(data, cursor)?;
            let a = read_sdf_tree(data, cursor)?;
            let b = read_sdf_tree(data, cursor)?;
            Ok(SdfTree::SmoothIntersect { a: Box::new(a), b: Box::new(b), k })
        }
        SDF_TAG_OFFSET => {
            let amount = read_f32(data, cursor)?;
            let child  = read_sdf_tree(data, cursor)?;
            Ok(SdfTree::Offset { child: Box::new(child), amount })
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown SDF tag 0x{:02x}", other),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_roundtrip() {
        let t = SdfTree::Circle { cx: 10.0, cy: 20.0, r: 30.0 };
        let mut buf = Vec::new();
        write_sdf_tree(&mut buf, &t);
        let mut cursor = 0;
        let back = read_sdf_tree(&buf, &mut cursor).unwrap();
        assert_eq!(back, t);
        assert_eq!(cursor, buf.len());
    }

    #[test]
    fn smooth_union_roundtrip() {
        // k = 0.25 is exact in binary (2^-2), avoiding f32 round-trip noise
        // in this struct-equality test.
        let a = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 };
        let b = SdfTree::Circle { cx: 50.0, cy: 0.0, r: 30.0 };
        let t = SdfTree::SmoothUnion { children: vec![a, b], k: 0.25 };
        let mut buf = Vec::new();
        write_sdf_tree(&mut buf, &t);
        let mut cursor = 0;
        let back = read_sdf_tree(&buf, &mut cursor).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn nested_subtract_roundtrip() {
        let ring = SdfTree::Circle { cx: 0.0, cy: 0.0, r: 50.0 }
            .subtract(SdfTree::Circle { cx: 0.0, cy: 0.0, r: 30.0 });
        let mut buf = Vec::new();
        write_sdf_tree(&mut buf, &ring);
        let mut cursor = 0;
        let back = read_sdf_tree(&buf, &mut cursor).unwrap();
        assert_eq!(back, ring);
    }
  }
