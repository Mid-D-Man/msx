// core/msx-parser/src/layer.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{BlendMode, Color, CompositeOp, Effect, FilterGraph, FilterInput, Layer, TransferFn};

use crate::dix_helpers::{array_len, color_from, enum_discriminant, opt, raw, type_tag};
use crate::element::parse_elements;
use crate::transform::parse_transform;

/// `{ type = "layer", blend_mode<enum> = BlendMode.Multiply (or "multiply"),
///    opacity=0.75, z_index=1.0, clip=true, clip_path=[...shapes], mask=[...elements],
///    mask_type="alpha", effects=[...], elements=[...] }`
///
/// `clip_path` and `mask` are optional. Absent means none, present and empty
/// hides the whole layer. `mask_type` is `luminance` (default) or `alpha`.
pub fn parse_layer(data: &DixData, prefix: &str) -> Result<Layer, String> {
    let id = opt::<String>(data, prefix, "id")?;
    let transform = parse_transform(data, prefix)?;
    let children = parse_elements(data, prefix, "elements")?;

    let mut layer = Layer::new(children);
    layer.id = id;
    layer.transform = transform;
    layer.blend_mode = parse_blend_mode(data, prefix)?.unwrap_or_default();
    layer.opacity = opt::<f64>(data, prefix, "opacity")?.unwrap_or(1.0);
    layer.z_index = opt::<f64>(data, prefix, "z_index")?.unwrap_or(0.0);
    layer.clip = opt::<bool>(data, prefix, "clip")?.unwrap_or(false);
    if raw(data, prefix, "clip_path").is_some() {
        layer.clip_path = Some(parse_elements(data, prefix, "clip_path")?);
    }
    if raw(data, prefix, "mask").is_some() {
        layer.mask = Some(parse_elements(data, prefix, "mask")?);
    }
    if let Some(name) = opt::<String>(data, prefix, "mask_type")? {
        layer.mask_type = msx_ast::MaskType::from_name(&name);
    }
    layer.effects = parse_effects(data, prefix, "effects")?;

    Ok(layer)
}

fn parse_blend_mode(data: &DixData, prefix: &str) -> Result<Option<BlendMode>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "blend_mode")) {
        return Ok(Some(BlendMode::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "blend_mode")?.map(|s| blend_mode_from_str(&s)))
}

fn blend_mode_from_str(s: &str) -> BlendMode {
    match s {
        "multiply"   => BlendMode::Multiply,
        "screen"     => BlendMode::Screen,
        "overlay"    => BlendMode::Overlay,
        "add"        => BlendMode::Add,
        "soft_light" => BlendMode::SoftLight,
        "hard_light" => BlendMode::HardLight,
        "difference" => BlendMode::Difference,
        "exclusion"  => BlendMode::Exclusion,
        "darken"     => BlendMode::Darken,
        "lighten"    => BlendMode::Lighten,
        "subtract"   => BlendMode::Subtract,
        "divide"     => BlendMode::Divide,
        "color_dodge" => BlendMode::ColorDodge,
        "color_burn" => BlendMode::ColorBurn,
        "hue"        => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color"      => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        "plus_lighter" => BlendMode::PlusLighter,
        _            => BlendMode::Normal,
    }
}

fn parse_effects(data: &DixData, parent_prefix: &str, field: &str) -> Result<Vec<Effect>, String> {
    let path = dix_path(parent_prefix, field);
    let count = array_len(data, parent_prefix, field)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(parse_effect(data, &format!("{}[{}]", path, i))?);
    }
    Ok(out)
}

fn parse_effect(data: &DixData, prefix: &str) -> Result<Effect, String> {
    let kind = type_tag(data, prefix)?;
    match kind.as_str() {
        "blur" => Ok(Effect::Blur {
            radius: opt::<f64>(data, prefix, "radius")?.unwrap_or(4.0),
        }),
        "drop_shadow" => Ok(Effect::DropShadow {
            offset_x: opt::<f64>(data, prefix, "offset_x")?.unwrap_or(0.0),
            offset_y: opt::<f64>(data, prefix, "offset_y")?.unwrap_or(0.0),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(4.0),
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::BLACK),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.5),
        }),
        "inner_shadow" => Ok(Effect::InnerShadow {
            offset_x: opt::<f64>(data, prefix, "offset_x")?.unwrap_or(0.0),
            offset_y: opt::<f64>(data, prefix, "offset_y")?.unwrap_or(0.0),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(4.0),
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::BLACK),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.5),
        }),
        "outer_glow" => Ok(Effect::OuterGlow {
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::WHITE),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(8.0),
            spread: opt::<f64>(data, prefix, "spread")?.unwrap_or(0.0),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.75),
        }),
        "inner_glow" => Ok(Effect::InnerGlow {
            color: color_from(raw(data, prefix, "color")).unwrap_or(Color::WHITE),
            blur_radius: opt::<f64>(data, prefix, "blur_radius")?.unwrap_or(8.0),
            opacity: opt::<f64>(data, prefix, "opacity")?.unwrap_or(0.75),
        }),
        "blur_xy" => Ok(Effect::BlurXY {
            rx: opt::<f64>(data, prefix, "rx")?.unwrap_or(4.0),
            ry: opt::<f64>(data, prefix, "ry")?.unwrap_or(4.0),
        }),
        "color_matrix" => {
            let list = opt::<Vec<f64>>(data, prefix, "values")?.ok_or_else(|| format!("{prefix}: a color_matrix needs `values`, 20 numbers"))?;
            let count = list.len();
            let values: [f64; 20] = list.try_into().map_err(|_| format!("{prefix}: a color_matrix needs 20 values, found {count}"))?;
            Ok(Effect::ColorMatrix { values, linear: opt::<bool>(data, prefix, "linear")?.unwrap_or(false) })
        }
        "graph" => parse_graph(data, prefix).map(Effect::Graph),
        other => Err(format!("{}: unknown effect type '{}'", prefix, other)),
    }
                 }

// ── Filter graph ─────────────────────────────────────────────────────────────

/// `{ type = "graph", nodes = [ { op = "...", ... }, ... ] }`. An input
/// (`input`, `input2`, and the entries of `inputs`) is `"source"`, `"alpha"` or
/// the index of an earlier node as text. A node without `input` reads the
/// node before it, or `"source"` when it is the first, which is what SVG does.
fn parse_graph(data: &DixData, prefix: &str) -> Result<FilterGraph, String> {
    use msx_ast::{FilterNode, FilterOp, FilterRegion};

    let count = array_len(data, prefix, "nodes")?;
    let mut nodes = Vec::with_capacity(count.min(msx_ast::MAX_FILTER_NODES));
    for i in 0..count {
        let p = format!("{}.nodes[{}]", prefix, i);
        let previous = if i == 0 { FilterInput::SourceGraphic } else { FilterInput::Node((i - 1) as u16) };
        let input = |data: &DixData, field: &str, default: FilterInput| -> Result<FilterInput, String> {
            match opt::<String>(data, &p, field)? {
                None => Ok(default),
                Some(name) => FilterInput::from_source_name(&name).ok_or_else(|| format!("{p}.{field}: '{name}' is not source, alpha or a node index")),
            }
        };
        let op = opt::<String>(data, &p, "op")?.ok_or_else(|| format!("{p}: a filter node needs `op`"))?;
        let kind = match op.as_str() {
            "flood" => FilterOp::Flood { color: color_from(raw(data, &p, "color")).unwrap_or(Color::BLACK) },
            "composite" => {
                let operator = opt::<String>(data, &p, "operator")?.unwrap_or_else(|| "over".to_string());
                let op = match operator.as_str() {
                    "over" => CompositeOp::Over,
                    "in" => CompositeOp::In,
                    "out" => CompositeOp::Out,
                    "atop" => CompositeOp::Atop,
                    "xor" => CompositeOp::Xor,
                    "arithmetic" => {
                        let k = opt::<Vec<f64>>(data, &p, "k")?.unwrap_or_default();
                        let get = |n: usize| k.get(n).copied().unwrap_or(0.0);
                        CompositeOp::Arithmetic { k1: get(0), k2: get(1), k3: get(2), k4: get(3) }
                    }
                    other => return Err(format!("{p}.operator: unknown composite operator '{other}'")),
                };
                FilterOp::Composite { a: input(data, "input", previous)?, b: input(data, "input2", FilterInput::SourceGraphic)?, op }
            }
            "merge" => {
                let names = opt::<Vec<String>>(data, &p, "inputs")?.unwrap_or_default();
                let mut inputs = Vec::with_capacity(names.len());
                for name in names {
                    inputs.push(FilterInput::from_source_name(&name).ok_or_else(|| format!("{p}.inputs: '{name}' is not source, alpha or a node index"))?);
                }
                FilterOp::Merge { inputs }
            }
            "blend" => FilterOp::Blend {
                a: input(data, "input", previous)?,
                b: input(data, "input2", FilterInput::SourceGraphic)?,
                mode: opt::<String>(data, &p, "mode")?.map(|m| blend_mode_from_str(&m)).unwrap_or_default(),
            },
            "offset" => FilterOp::Offset {
                input: input(data, "input", previous)?,
                dx: opt::<f64>(data, &p, "dx")?.unwrap_or(0.0),
                dy: opt::<f64>(data, &p, "dy")?.unwrap_or(0.0),
            },
            "blur" => FilterOp::Blur {
                input: input(data, "input", previous)?,
                rx: opt::<f64>(data, &p, "rx")?.unwrap_or(0.0),
                ry: opt::<f64>(data, &p, "ry")?.unwrap_or(0.0),
            },
            "color_matrix" => {
                let list = opt::<Vec<f64>>(data, &p, "values")?.ok_or_else(|| format!("{p}: a color_matrix needs `values`, 20 numbers"))?;
                let found = list.len();
                let values: [f64; 20] = list.try_into().map_err(|_| format!("{p}: a color_matrix needs 20 values, found {found}"))?;
                FilterOp::ColorMatrix { input: input(data, "input", previous)?, values }
            }
            "component_transfer" => {
                let mut funcs = [TransferFn::Identity, TransferFn::Identity, TransferFn::Identity, TransferFn::Identity];
                for (slot, channel) in funcs.iter_mut().zip(["r", "g", "b", "a"]) {
                    if raw(data, &p, channel).is_some() {
                        *slot = parse_transfer(data, &format!("{p}.{channel}"))?;
                    }
                }
                FilterOp::ComponentTransfer { input: input(data, "input", previous)?, funcs }
            }
            other => return Err(format!("{p}.op: unknown filter op '{other}'")),
        };
        let region = match opt::<Vec<f64>>(data, &p, "region")? {
            None => None,
            Some(r) if r.len() == 4 => Some(FilterRegion { x: r[0], y: r[1], width: r[2], height: r[3] }),
            Some(r) => return Err(format!("{p}.region: expected [x, y, width, height], found {} values", r.len())),
        };
        nodes.push(FilterNode { op: kind, region, linear: opt::<bool>(data, &p, "linear")?.unwrap_or(false) });
    }
    let graph = FilterGraph { nodes };
    graph.validate().map_err(|e| format!("{prefix}: {e}"))?;
    Ok(graph)
}

/// `{ kind = "table", values = [...] }`, `"discrete"`, `"linear"` (`slope`,
/// `intercept`), `"gamma"` (`amplitude`, `exponent`, `offset`) or `"identity"`.
fn parse_transfer(data: &DixData, p: &str) -> Result<TransferFn, String> {
    let kind = opt::<String>(data, p, "kind")?.unwrap_or_else(|| "identity".to_string());
    match kind.as_str() {
        "identity" => Ok(TransferFn::Identity),
        "table" => Ok(TransferFn::Table(opt::<Vec<f64>>(data, p, "values")?.unwrap_or_default())),
        "discrete" => Ok(TransferFn::Discrete(opt::<Vec<f64>>(data, p, "values")?.unwrap_or_default())),
        "linear" => Ok(TransferFn::Linear { slope: opt::<f64>(data, p, "slope")?.unwrap_or(1.0), intercept: opt::<f64>(data, p, "intercept")?.unwrap_or(0.0) }),
        "gamma" => Ok(TransferFn::Gamma {
            amplitude: opt::<f64>(data, p, "amplitude")?.unwrap_or(1.0),
            exponent: opt::<f64>(data, p, "exponent")?.unwrap_or(1.0),
            offset: opt::<f64>(data, p, "offset")?.unwrap_or(0.0),
        }),
        other => Err(format!("{p}.kind: unknown transfer function '{other}'")),
    }
}
