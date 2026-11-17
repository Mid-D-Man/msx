// core/msx-binary/src/filter_codec.rs
//! Binary encode/decode for the filter graph of `Effect::Graph` (effect tag 0x07,
//! wire version 3).
//!
//! NOTICE: see docs/format-spec.md, section "Layer effects". The layout is
//! `[u16 node count]` and then each node as `[u8 op] [u8 flags] [4 x f32 region
//! when flag bit 1] [op payload]`. Flag bit 0 is the linear light flag.
//! An input is `[u8 kind]` (0 source graphic, 1 source alpha, 2 node) and for a
//! node `[u16 index]`. The reader checks the result with `FilterGraph::validate`,
//! so a node that reads a later node, too many nodes and lists that are too
//! long are refused here and not in the renderers.

use std::io;

use msx_ast::{BlendMode, Color, CompositeOp, FilterGraph, FilterInput, FilterNode, FilterOp, FilterRegion, TransferFn, MAX_FILTER_NODES, MAX_MERGE_INPUTS, MAX_TRANSFER_VALUES};

use crate::decoder::{read_f32, read_u16, read_u8};
use crate::encoder::{write_f32, write_u16, write_u8};

const OP_FLOOD: u8 = 0;
const OP_COMPOSITE: u8 = 1;
const OP_MERGE: u8 = 2;
const OP_BLEND: u8 = 3;
const OP_OFFSET: u8 = 4;
const OP_BLUR: u8 = 5;
const OP_COLOR_MATRIX: u8 = 6;
const OP_COMPONENT_TRANSFER: u8 = 7;

const FLAG_LINEAR: u8 = 1;
const FLAG_REGION: u8 = 2;

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn write_input(out: &mut Vec<u8>, input: FilterInput) {
    match input {
        FilterInput::SourceGraphic => write_u8(out, 0),
        FilterInput::SourceAlpha => write_u8(out, 1),
        FilterInput::Node(i) => {
            write_u8(out, 2);
            write_u16(out, i);
        }
    }
}

fn read_input(data: &[u8], cursor: &mut usize) -> io::Result<FilterInput> {
    match read_u8(data, cursor)? {
        0 => Ok(FilterInput::SourceGraphic),
        1 => Ok(FilterInput::SourceAlpha),
        2 => Ok(FilterInput::Node(read_u16(data, cursor)?)),
        other => Err(bad(format!("unknown filter input kind {other}"))),
    }
}

fn write_fn(out: &mut Vec<u8>, f: &TransferFn) {
    match f {
        TransferFn::Identity => write_u8(out, 0),
        TransferFn::Table(t) | TransferFn::Discrete(t) => {
            write_u8(out, if matches!(f, TransferFn::Table(_)) { 1 } else { 2 });
            write_u16(out, t.len().min(MAX_TRANSFER_VALUES) as u16);
            for v in t.iter().take(MAX_TRANSFER_VALUES) {
                write_f32(out, *v);
            }
        }
        TransferFn::Linear { slope, intercept } => {
            write_u8(out, 3);
            write_f32(out, *slope);
            write_f32(out, *intercept);
        }
        TransferFn::Gamma { amplitude, exponent, offset } => {
            write_u8(out, 4);
            write_f32(out, *amplitude);
            write_f32(out, *exponent);
            write_f32(out, *offset);
        }
    }
}

fn read_fn(data: &[u8], cursor: &mut usize) -> io::Result<TransferFn> {
    match read_u8(data, cursor)? {
        0 => Ok(TransferFn::Identity),
        tag @ (1 | 2) => {
            let n = read_u16(data, cursor)? as usize;
            if n > MAX_TRANSFER_VALUES {
                return Err(bad(format!("a transfer table has {n} values, the limit is {MAX_TRANSFER_VALUES}")));
            }
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(read_f32(data, cursor)?);
            }
            Ok(if tag == 1 { TransferFn::Table(v) } else { TransferFn::Discrete(v) })
        }
        3 => Ok(TransferFn::Linear { slope: read_f32(data, cursor)?, intercept: read_f32(data, cursor)? }),
        4 => Ok(TransferFn::Gamma { amplitude: read_f32(data, cursor)?, exponent: read_f32(data, cursor)?, offset: read_f32(data, cursor)? }),
        other => Err(bad(format!("unknown transfer function tag {other}"))),
    }
}

pub fn write_graph(out: &mut Vec<u8>, graph: &FilterGraph) {
    write_u16(out, graph.nodes.len().min(MAX_FILTER_NODES) as u16);
    for node in graph.nodes.iter().take(MAX_FILTER_NODES) {
        let op = match &node.op {
            FilterOp::Flood { .. } => OP_FLOOD,
            FilterOp::Composite { .. } => OP_COMPOSITE,
            FilterOp::Merge { .. } => OP_MERGE,
            FilterOp::Blend { .. } => OP_BLEND,
            FilterOp::Offset { .. } => OP_OFFSET,
            FilterOp::Blur { .. } => OP_BLUR,
            FilterOp::ColorMatrix { .. } => OP_COLOR_MATRIX,
            FilterOp::ComponentTransfer { .. } => OP_COMPONENT_TRANSFER,
        };
        write_u8(out, op);
        write_u8(out, if node.linear { FLAG_LINEAR } else { 0 } | if node.region.is_some() { FLAG_REGION } else { 0 });
        if let Some(r) = node.region {
            for v in [r.x, r.y, r.width, r.height] {
                write_f32(out, v);
            }
        }
        match &node.op {
            FilterOp::Flood { color } => out.extend_from_slice(&[color.r, color.g, color.b, color.a]),
            FilterOp::Composite { a, b, op } => {
                write_input(out, *a);
                write_input(out, *b);
                match op {
                    CompositeOp::Over => write_u8(out, 0),
                    CompositeOp::In => write_u8(out, 1),
                    CompositeOp::Out => write_u8(out, 2),
                    CompositeOp::Atop => write_u8(out, 3),
                    CompositeOp::Xor => write_u8(out, 4),
                    CompositeOp::Arithmetic { k1, k2, k3, k4 } => {
                        write_u8(out, 5);
                        for k in [k1, k2, k3, k4] {
                            write_f32(out, *k);
                        }
                    }
                }
            }
            FilterOp::Merge { inputs } => {
                write_u8(out, inputs.len().min(MAX_MERGE_INPUTS) as u8);
                for i in inputs.iter().take(MAX_MERGE_INPUTS) {
                    write_input(out, *i);
                }
            }
            FilterOp::Blend { a, b, mode } => {
                write_input(out, *a);
                write_input(out, *b);
                write_u8(out, mode.to_byte());
            }
            FilterOp::Offset { input, dx, dy } => {
                write_input(out, *input);
                write_f32(out, *dx);
                write_f32(out, *dy);
            }
            FilterOp::Blur { input, rx, ry } => {
                write_input(out, *input);
                write_f32(out, *rx);
                write_f32(out, *ry);
            }
            FilterOp::ColorMatrix { input, values } => {
                write_input(out, *input);
                for v in values {
                    write_f32(out, *v);
                }
            }
            FilterOp::ComponentTransfer { input, funcs } => {
                write_input(out, *input);
                for f in funcs {
                    write_fn(out, f);
                }
            }
        }
    }
}

pub fn read_graph(data: &[u8], cursor: &mut usize) -> io::Result<FilterGraph> {
    let count = read_u16(data, cursor)? as usize;
    if count > MAX_FILTER_NODES {
        return Err(bad(format!("a filter graph has {count} nodes, the limit is {MAX_FILTER_NODES}")));
    }
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let op = read_u8(data, cursor)?;
        let flags = read_u8(data, cursor)?;
        if flags & !(FLAG_LINEAR | FLAG_REGION) != 0 {
            return Err(bad(format!("unknown filter node flags 0x{flags:02x}")));
        }
        let region = if flags & FLAG_REGION != 0 {
            Some(FilterRegion { x: read_f32(data, cursor)?, y: read_f32(data, cursor)?, width: read_f32(data, cursor)?, height: read_f32(data, cursor)? })
        } else {
            None
        };
        let op = match op {
            OP_FLOOD => {
                let r = read_u8(data, cursor)?;
                let g = read_u8(data, cursor)?;
                let b = read_u8(data, cursor)?;
                let a = read_u8(data, cursor)?;
                FilterOp::Flood { color: Color::rgba(r, g, b, a) }
            }
            OP_COMPOSITE => {
                let a = read_input(data, cursor)?;
                let b = read_input(data, cursor)?;
                let op = match read_u8(data, cursor)? {
                    0 => CompositeOp::Over,
                    1 => CompositeOp::In,
                    2 => CompositeOp::Out,
                    3 => CompositeOp::Atop,
                    4 => CompositeOp::Xor,
                    5 => CompositeOp::Arithmetic { k1: read_f32(data, cursor)?, k2: read_f32(data, cursor)?, k3: read_f32(data, cursor)?, k4: read_f32(data, cursor)? },
                    other => return Err(bad(format!("unknown composite operator {other}"))),
                };
                FilterOp::Composite { a, b, op }
            }
            OP_MERGE => {
                let n = read_u8(data, cursor)? as usize;
                if n > MAX_MERGE_INPUTS {
                    return Err(bad(format!("a merge has {n} inputs, the limit is {MAX_MERGE_INPUTS}")));
                }
                let mut inputs = Vec::with_capacity(n);
                for _ in 0..n {
                    inputs.push(read_input(data, cursor)?);
                }
                FilterOp::Merge { inputs }
            }
            OP_BLEND => {
                let a = read_input(data, cursor)?;
                let b = read_input(data, cursor)?;
                FilterOp::Blend { a, b, mode: BlendMode::from_byte(read_u8(data, cursor)?) }
            }
            OP_OFFSET => FilterOp::Offset { input: read_input(data, cursor)?, dx: read_f32(data, cursor)?, dy: read_f32(data, cursor)? },
            OP_BLUR => FilterOp::Blur { input: read_input(data, cursor)?, rx: read_f32(data, cursor)?, ry: read_f32(data, cursor)? },
            OP_COLOR_MATRIX => {
                let input = read_input(data, cursor)?;
                let mut values = [0.0; 20];
                for v in values.iter_mut() {
                    *v = read_f32(data, cursor)?;
                }
                FilterOp::ColorMatrix { input, values }
            }
            OP_COMPONENT_TRANSFER => {
                let input = read_input(data, cursor)?;
                let funcs = [read_fn(data, cursor)?, read_fn(data, cursor)?, read_fn(data, cursor)?, read_fn(data, cursor)?];
                FilterOp::ComponentTransfer { input, funcs }
            }
            other => return Err(bad(format!("unknown filter op {other}"))),
        };
        nodes.push(FilterNode { op, region, linear: flags & FLAG_LINEAR != 0 });
    }
    let graph = FilterGraph { nodes };
    graph.validate().map_err(bad)?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FilterGraph {
        let mut values = [0.0; 20];
        for (i, v) in values.iter_mut().enumerate() {
            *v = i as f64 * 0.5 - 4.0;
        }
        let mut flood = FilterNode::new(FilterOp::Flood { color: Color::rgba(10, 20, 30, 128) });
        flood.region = Some(FilterRegion { x: 1.5, y: 2.0, width: 30.0, height: 40.0 });
        let mut comp = FilterNode::new(FilterOp::Composite { a: FilterInput::Node(0), b: FilterInput::SourceAlpha, op: CompositeOp::Arithmetic { k1: 0.0, k2: 1.0, k3: 0.5, k4: -0.25 } });
        comp.linear = true;
        FilterGraph {
            nodes: vec![
                flood,
                comp,
                FilterNode::new(FilterOp::Composite { a: FilterInput::SourceGraphic, b: FilterInput::Node(1), op: CompositeOp::Xor }),
                FilterNode::new(FilterOp::Merge { inputs: vec![FilterInput::Node(1), FilterInput::Node(2), FilterInput::SourceGraphic] }),
                FilterNode::new(FilterOp::Blend { a: FilterInput::Node(3), b: FilterInput::SourceGraphic, mode: BlendMode::Multiply }),
                FilterNode::new(FilterOp::Offset { input: FilterInput::Node(4), dx: 3.0, dy: -2.5 }),
                FilterNode::new(FilterOp::Blur { input: FilterInput::Node(5), rx: 2.0, ry: 0.0 }),
                FilterNode::new(FilterOp::ColorMatrix { input: FilterInput::Node(6), values }),
                FilterNode::new(FilterOp::ComponentTransfer {
                    input: FilterInput::Node(7),
                    funcs: [
                        TransferFn::Identity,
                        TransferFn::Table(vec![0.0, 0.5, 1.0]),
                        TransferFn::Discrete(vec![0.25, 0.75]),
                        TransferFn::Linear { slope: 0.5, intercept: 0.25 },
                    ],
                }),
                FilterNode::new(FilterOp::ComponentTransfer {
                    input: FilterInput::SourceGraphic,
                    funcs: [TransferFn::Gamma { amplitude: 1.0, exponent: 2.0, offset: 0.0 }, TransferFn::Identity, TransferFn::Identity, TransferFn::Identity],
                }),
            ],
        }
    }

    #[test]
    fn every_op_roundtrips() {
        let g = sample();
        let mut buf = Vec::new();
        write_graph(&mut buf, &g);
        let mut cursor = 0;
        let back = read_graph(&buf, &mut cursor).unwrap();
        assert_eq!(cursor, buf.len());
        assert_eq!(back, g, "all values here are exact in f32");
    }

    #[test]
    fn an_empty_graph_is_two_bytes() {
        let mut buf = Vec::new();
        write_graph(&mut buf, &FilterGraph::default());
        assert_eq!(buf, vec![0, 0]);
        assert_eq!(read_graph(&buf, &mut 0).unwrap(), FilterGraph::default());
    }

    #[test]
    fn a_reader_refuses_what_the_validator_refuses() {
        // Node 0 reads node 0.
        let mut buf = Vec::new();
        write_u16(&mut buf, 1);
        write_u8(&mut buf, OP_OFFSET);
        write_u8(&mut buf, 0);
        write_input(&mut buf, FilterInput::Node(0));
        write_f32(&mut buf, 1.0);
        write_f32(&mut buf, 1.0);
        assert!(read_graph(&buf, &mut 0).is_err());
        // Too many nodes, before anything is allocated for them.
        let mut many = Vec::new();
        write_u16(&mut many, (MAX_FILTER_NODES + 1) as u16);
        assert!(read_graph(&many, &mut 0).is_err());
        // Unknown op, flags, input kind and transfer tag.
        assert!(read_graph(&[1, 0, 99, 0], &mut 0).is_err());
        assert!(read_graph(&[1, 0, OP_FLOOD, 0x80, 0, 0, 0, 0], &mut 0).is_err());
        assert!(read_graph(&[1, 0, OP_OFFSET, 0, 9], &mut 0).is_err());
        assert!(read_graph(&[1, 0, OP_COMPONENT_TRANSFER, 0, 0, 9], &mut 0).is_err());
        // A region without area.
        let mut flat = Vec::new();
        write_u16(&mut flat, 1);
        write_u8(&mut flat, OP_FLOOD);
        write_u8(&mut flat, FLAG_REGION);
        for v in [0.0, 0.0, 0.0, 5.0] {
            write_f32(&mut flat, v);
        }
        flat.extend_from_slice(&[1, 2, 3, 4]);
        assert!(read_graph(&flat, &mut 0).is_err());
        // Cut short.
        let mut full = Vec::new();
        write_graph(&mut full, &sample());
        for cut in [1, 5, full.len() / 2, full.len() - 1] {
            assert!(read_graph(&full[..cut], &mut 0).is_err(), "cut at {cut}");
        }
    }
}
