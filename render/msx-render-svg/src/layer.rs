// render/msx-render-svg/src/layer.rs
use msx_ast::{fmt_f64, Def, Effect, Layer};

use crate::{render_element, write_attr, write_id, write_transform, Ctx};

/// `blend_mode` → CSS `mix-blend-mode` (`Subtract`/`Divide` have no CSS
/// equivalent and fall back to Normal here; `msx-render-cpu`/`-gpu`
/// implement the signed-buffer math those actually need). `effects` → a
/// generated `<filter>` chaining one SVG filter primitive per effect.
/// `mask` → a generated `<mask>` of the same elements, same way. `clip_path` →
/// a generated `<clipPath>` of the same shapes, referenced
/// from the layer's `<g>`. `clip` (the pixel footprint flag) has no cheap
/// SVG 1.1 fallback without a bbox computation, so it's left unenforced
/// here.
pub(crate) fn render_layer(ctx: &mut Ctx, layer: &Layer, defs: &[Def]) {
    ctx.push("<g");

    write_id(ctx, layer.id.as_deref());
    write_transform(ctx, layer.transform.as_ref());

    if (layer.opacity - 1.0).abs() > 1e-6 {
        write_attr(ctx, "opacity", fmt_f64(layer.opacity));
    }

    if let Some(css) = layer.blend_mode.to_css_blend_mode() {
        write_attr(ctx, "style", format!("mix-blend-mode:{}", css));
    }

    if !layer.effects.is_empty() {
        let filter_id = render_effects_filter(ctx, &layer.effects);
        write_attr(ctx, "filter", format!("url(#{})", filter_id));
    }

    if let Some(shapes) = &layer.clip_path {
        let clip_id = render_clip_path(ctx, shapes, defs);
        write_attr(ctx, "clip-path", format!("url(#{})", clip_id));
    }

    if let Some(elements) = &layer.mask {
        let mask_id = render_mask(ctx, elements, layer.mask_type, defs);
        write_attr(ctx, "mask", format!("url(#{})", mask_id));
    }

    ctx.push(">");
    for child in msx_ast::layer_reordered(&layer.children) {
        render_element(ctx, child, defs);
    }
    ctx.push("</g>");
}

/// Renders `shapes` into a `<clipPath>` in the synthesized defs and returns
/// its id. The shapes are drawn by the normal element renderer into a side
/// buffer, then `fill-rule` is renamed to `clip-rule`, the property SVG reads
/// inside a clip path. Fill, stroke and opacity attributes are ignored there.
/// An empty list gives an empty `<clipPath>`, which clips everything away.
fn render_clip_path(ctx: &mut Ctx, shapes: &[msx_ast::Element], defs: &[Def]) -> String {
    let id = ctx.fresh_id("layer-clip-");
    let outer = std::mem::take(&mut ctx.out);
    for shape in shapes {
        render_element(ctx, shape, defs);
    }
    let body = std::mem::replace(&mut ctx.out, outer).replace(" fill-rule=\"", " clip-rule=\"");
    ctx.extra_defs.push_str(&format!("<clipPath id=\"{id}\">{body}</clipPath>"));
    id
}

/// Renders `elements` into a `<mask>` in the synthesized defs and returns its
/// id. The region is far larger than any canvas, so the layer is never cut by
/// the default 120 percent box of a mask, since a mask that needs a region
/// carries it as a clip of its own content. `alpha` masks write
/// `mask-type: alpha`, and an empty list gives an empty `<mask>`, which hides
/// the whole layer.
fn render_mask(ctx: &mut Ctx, elements: &[msx_ast::Element], kind: msx_ast::MaskType, defs: &[Def]) -> String {
    let id = ctx.fresh_id("layer-mask-");
    let outer = std::mem::take(&mut ctx.out);
    for el in elements {
        render_element(ctx, el, defs);
    }
    let body = std::mem::replace(&mut ctx.out, outer);
    let kind_attr = if kind == msx_ast::MaskType::Alpha { " style=\"mask-type:alpha\"" } else { "" };
    ctx.extra_defs.push_str(&format!(
        "<mask id=\"{id}\" maskUnits=\"userSpaceOnUse\" x=\"-1000000\" y=\"-1000000\" width=\"2000000\" height=\"2000000\"{kind_attr}>{body}</mask>"
    ));
    id
}

fn render_effects_filter(ctx: &mut Ctx, effects: &[Effect]) -> String {
    let filter_id = ctx.fresh_id("layer-fx-");
    let mut body = String::new();
    let mut input = "SourceGraphic".to_string();

    for (i, effect) in effects.iter().enumerate() {
        let result = format!("fx{}-out", i);
        body.push_str(&effect_primitive(effect, &input, &result, i));
        input = result;
    }

    ctx.extra_defs.push_str(&format!(
        r#"<filter id="{id}" x="-50%" y="-50%" width="200%" height="200%">{body}</filter>"#,
        id = filter_id,
        body = body,
    ));
    filter_id
}

/// `idx` scopes each effect's intermediate `result=` names (`fx{idx}-...`)
/// so chained effects on the same layer never collide.
fn effect_primitive(effect: &Effect, input: &str, result: &str, idx: usize) -> String {
    let p = format!("fx{}-", idx);
    match effect {
        Effect::Blur { radius } => format!(
            r#"<feGaussianBlur in="{input}" stdDeviation="{r}" result="{result}"/>"#,
            input = input,
            r = fmt_f64(radius.max(0.0)),
            result = result,
        ),

        // Two numbers give a deviation per axis. A zero on one axis blurs only
        // the other.
        Effect::BlurXY { rx, ry } => format!(
            r#"<feGaussianBlur in="{input}" stdDeviation="{rx} {ry}" result="{result}"/>"#,
            input = input,
            rx = fmt_f64(rx.max(0.0)),
            ry = fmt_f64(ry.max(0.0)),
            result = result,
        ),

        // The color space is written on the primitive, because the filter
        // default (linearRGB) would otherwise apply to every matrix.
        Effect::ColorMatrix { values, linear } => format!(
            r#"<feColorMatrix in="{input}" type="matrix" values="{values}" color-interpolation-filters="{space}" result="{result}"/>"#,
            input = input,
            values = values.iter().map(|v| fmt_matrix_value(*v)).collect::<Vec<_>>().join(" "),
            space = if *linear { "linearRGB" } else { "sRGB" },
            result = result,
        ),

        Effect::Graph(graph) => graph_primitives(graph, input, result, &p),

        Effect::DropShadow { offset_x, offset_y, blur_radius, color, opacity } => format!(
            r#"<feDropShadow in="{input}" dx="{dx}" dy="{dy}" stdDeviation="{blur}" flood-color="{color}" flood-opacity="{op}" result="{result}"/>"#,
            input = input,
            dx = fmt_f64(*offset_x),
            dy = fmt_f64(*offset_y),
            blur = fmt_f64(blur_radius.max(0.0)),
            color = color.to_svg_hex(),
            op = fmt_f64(opacity.clamp(0.0, 1.0)),
            result = result,
        ),

        // Classic blur-the-alpha / flood / composite-in / merge-on-top glow
        // recipe, with `feMorphology dilate` standing in for `spread`.
        Effect::OuterGlow { color, blur_radius, spread, opacity } => format!(
            r#"<feMorphology in="{input}" operator="dilate" radius="{spread}" result="{p}dilate"/><feGaussianBlur in="{p}dilate" stdDeviation="{blur}" result="{p}blur"/><feFlood flood-color="{color}" flood-opacity="{op}" result="{p}color"/><feComposite in="{p}color" in2="{p}blur" operator="in" result="{p}glow"/><feMerge result="{result}"><feMergeNode in="{p}glow"/><feMergeNode in="{input}"/></feMerge>"#,
            input = input,
            p = p,
            spread = fmt_f64(spread.max(0.0)),
            blur = fmt_f64(blur_radius.max(0.0)),
            color = color.to_svg_hex(),
            op = fmt_f64(opacity.clamp(0.0, 1.0)),
            result = result,
        ),

        // Invert alpha (feFuncA "1 0"), blur, flood, composite-in, clip back
        // to the original silhouette, merge under the source.
        Effect::InnerGlow { color, blur_radius, opacity } => format!(
            r#"<feComponentTransfer in="{input}" result="{p}inv"><feFuncA type="table" tableValues="1 0"/></feComponentTransfer><feGaussianBlur in="{p}inv" stdDeviation="{blur}" result="{p}blur"/><feFlood flood-color="{color}" flood-opacity="{op}" result="{p}color"/><feComposite in="{p}color" in2="{p}blur" operator="in" result="{p}tint"/><feComposite in="{p}tint" in2="{input}" operator="in" result="{p}clip"/><feMerge result="{result}"><feMergeNode in="{input}"/><feMergeNode in="{p}clip"/></feMerge>"#,
            input = input,
            p = p,
            blur = fmt_f64(blur_radius.max(0.0)),
            color = color.to_svg_hex(),
            op = fmt_f64(opacity.clamp(0.0, 1.0)),
            result = result,
        ),

        // Same as InnerGlow but the inverted-alpha mask is offset before
        // blurring, giving the shadow a direction instead of an even halo.
        Effect::InnerShadow { offset_x, offset_y, blur_radius, color, opacity } => format!(
            r#"<feComponentTransfer in="{input}" result="{p}inv"><feFuncA type="table" tableValues="1 0"/></feComponentTransfer><feOffset in="{p}inv" dx="{dx}" dy="{dy}" result="{p}off"/><feGaussianBlur in="{p}off" stdDeviation="{blur}" result="{p}blur"/><feFlood flood-color="{color}" flood-opacity="{op}" result="{p}color"/><feComposite in="{p}color" in2="{p}blur" operator="in" result="{p}tint"/><feComposite in="{p}tint" in2="{input}" operator="in" result="{p}clip"/><feMerge result="{result}"><feMergeNode in="{input}"/><feMergeNode in="{p}clip"/></feMerge>"#,
            input = input,
            p = p,
            dx = fmt_f64(*offset_x),
            dy = fmt_f64(*offset_y),
            blur = fmt_f64(blur_radius.max(0.0)),
            color = color.to_svg_hex(),
            op = fmt_f64(opacity.clamp(0.0, 1.0)),
            result = result,
        ),
    }
        }

/// A color matrix entry with six decimals, because `fmt_f64` keeps four and a
/// luminance weight such as 0.2126 needs more than that to match the
/// other renderers.
fn fmt_matrix_value(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

/// One SVG primitive per node of a filter graph. The node `j` is named
/// `{prefix}n{j}` and the last one takes `result`, so the graph chains with the
/// effects around it. The source graphic is `input`, the result of the effect
/// before, and its alpha is `SourceAlpha` when that is the layer itself or a
/// color matrix that keeps the alpha and sets the color to black otherwise.
/// Every primitive writes its color space, because the filter default
/// (linearRGB) would otherwise apply to all of them.
fn graph_primitives(graph: &msx_ast::FilterGraph, input: &str, result: &str, prefix: &str) -> String {
    use msx_ast::{CompositeOp, FilterInput, FilterOp, TransferFn};

    let mut out = String::new();
    let needs_alpha = graph.nodes.iter().any(|n| n.op.inputs().contains(&FilterInput::SourceAlpha));
    let alpha_name = if input == "SourceGraphic" {
        "SourceAlpha".to_string()
    } else {
        let name = format!("{prefix}alpha");
        if needs_alpha {
            out.push_str(&format!(
                r#"<feColorMatrix in="{input}" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0" color-interpolation-filters="sRGB" result="{name}"/>"#
            ));
        }
        name
    };
    let name_of = |i: FilterInput| match i {
        FilterInput::SourceGraphic => input.to_string(),
        FilterInput::SourceAlpha => alpha_name.clone(),
        FilterInput::Node(j) => format!("{prefix}n{j}"),
    };
    if graph.nodes.is_empty() {
        return format!(r#"<feOffset in="{input}" dx="0" dy="0" result="{result}"/>"#);
    }
    let num = |v: f64| fmt_matrix_value(v);
    let list = |vs: &[f64]| vs.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ");
    for (j, node) in graph.nodes.iter().enumerate() {
        let name = if j + 1 == graph.nodes.len() { result.to_string() } else { format!("{prefix}n{j}") };
        let mut attrs = String::new();
        if let Some(r) = node.region {
            attrs.push_str(&format!(r#" x="{}" y="{}" width="{}" height="{}""#, num(r.x), num(r.y), num(r.width), num(r.height)));
        }
        attrs.push_str(&format!(r#" color-interpolation-filters="{}" result="{name}""#, if node.linear { "linearRGB" } else { "sRGB" }));
        let prim = match &node.op {
            FilterOp::Flood { color } => format!(
                r#"<feFlood flood-color="rgb({},{},{})" flood-opacity="{}"{attrs}/>"#,
                color.r,
                color.g,
                color.b,
                num(f64::from(color.a) / 255.0)
            ),
            FilterOp::Composite { a, b, op } => {
                let k = match op {
                    CompositeOp::Arithmetic { k1, k2, k3, k4 } => format!(r#" k1="{}" k2="{}" k3="{}" k4="{}""#, num(*k1), num(*k2), num(*k3), num(*k4)),
                    _ => String::new(),
                };
                format!(r#"<feComposite in="{}" in2="{}" operator="{}"{k}{attrs}/>"#, name_of(*a), name_of(*b), op.source_name())
            }
            FilterOp::Merge { inputs } => {
                let nodes: String = inputs.iter().map(|i| format!(r#"<feMergeNode in="{}"/>"#, name_of(*i))).collect();
                format!("<feMerge{attrs}>{nodes}</feMerge>")
            }
            FilterOp::Blend { a, b, mode } => {
                format!(r#"<feBlend in="{}" in2="{}" mode="{}"{attrs}/>"#, name_of(*a), name_of(*b), mode.to_css_blend_mode().unwrap_or("normal"))
            }
            FilterOp::Offset { input: i, dx, dy } => format!(r#"<feOffset in="{}" dx="{}" dy="{}"{attrs}/>"#, name_of(*i), num(*dx), num(*dy)),
            FilterOp::Blur { input: i, rx, ry } => {
                format!(r#"<feGaussianBlur in="{}" stdDeviation="{} {}"{attrs}/>"#, name_of(*i), num(rx.max(0.0)), num(ry.max(0.0)))
            }
            FilterOp::ColorMatrix { input: i, values } => format!(r#"<feColorMatrix in="{}" type="matrix" values="{}"{attrs}/>"#, name_of(*i), list(values)),
            FilterOp::ComponentTransfer { input: i, funcs } => {
                let mut body = String::new();
                for (func, tag) in funcs.iter().zip(["R", "G", "B", "A"]) {
                    match func {
                        TransferFn::Identity => {}
                        TransferFn::Table(t) | TransferFn::Discrete(t) => {
                            body.push_str(&format!(r#"<feFunc{tag} type="{}" tableValues="{}"/>"#, func.source_name(), list(t)));
                        }
                        TransferFn::Linear { slope, intercept } => {
                            body.push_str(&format!(r#"<feFunc{tag} type="linear" slope="{}" intercept="{}"/>"#, num(*slope), num(*intercept)));
                        }
                        TransferFn::Gamma { amplitude, exponent, offset } => body.push_str(&format!(
                            r#"<feFunc{tag} type="gamma" amplitude="{}" exponent="{}" offset="{}"/>"#,
                            num(*amplitude),
                            num(*exponent),
                            num(*offset)
                        )),
                    }
                }
                format!(r#"<feComponentTransfer in="{}"{attrs}>{body}</feComponentTransfer>"#, name_of(*i))
            }
            FilterOp::Morphology { input: i, dilate, rx, ry } => format!(
                r#"<feMorphology in="{}" operator="{}" radius="{} {}"{attrs}/>"#,
                name_of(*i),
                if *dilate { "dilate" } else { "erode" },
                num(*rx),
                num(*ry)
            ),
            FilterOp::Tile { input: i } => format!(r#"<feTile in="{}"{attrs}/>"#, name_of(*i)),
            FilterOp::ConvolveMatrix { input: i, columns, rows, kernel, divisor, bias, target_x, target_y, edge, preserve_alpha } => format!(
                r#"<feConvolveMatrix in="{}" order="{columns} {rows}" kernelMatrix="{}" divisor="{}" bias="{}" targetX="{target_x}" targetY="{target_y}" edgeMode="{}" preserveAlpha="{preserve_alpha}"{attrs}/>"#,
                name_of(*i),
                list(kernel),
                num(*divisor),
                num(*bias),
                edge.source_name()
            ),
        };
        out.push_str(&prim);
    }
    out
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    use msx_ast::{CompositeOp, FilterGraph, FilterInput as In, FilterNode, FilterOp, FilterRegion, TransferFn};

    fn emit(graph: FilterGraph, input: &str) -> String {
        graph_primitives(&graph, input, "out", "fx3-")
    }

    #[test]
    fn nodes_chain_by_name_and_the_last_takes_the_result_name() {
        let g = FilterGraph {
            nodes: vec![
                FilterNode::new(FilterOp::Flood { color: msx_ast::Color::rgba(255, 0, 0, 128) }),
                FilterNode::new(FilterOp::Composite { a: In::Node(0), b: In::SourceAlpha, op: CompositeOp::In }),
                FilterNode::new(FilterOp::Merge { inputs: vec![In::Node(1), In::SourceGraphic] }),
            ],
        };
        let svg = emit(g, "SourceGraphic");
        assert!(svg.contains(r#"<feFlood flood-color="rgb(255,0,0)" flood-opacity="0.501961" color-interpolation-filters="sRGB" result="fx3-n0"/>"#), "{svg}");
        assert!(svg.contains(r#"<feComposite in="fx3-n0" in2="SourceAlpha" operator="in""#));
        assert!(svg.contains(r#"<feMerge color-interpolation-filters="sRGB" result="out"><feMergeNode in="fx3-n1"/><feMergeNode in="SourceGraphic"/></feMerge>"#));
    }

    #[test]
    fn morphology_tile_and_convolve_matrix_are_written_as_their_primitives() {
        let mut tile = FilterNode::new(FilterOp::Tile { input: In::Node(0) });
        tile.region = Some(FilterRegion { x: 5.0, y: 6.0, width: 70.0, height: 80.0 });
        let g = FilterGraph {
            nodes: vec![
                FilterNode::new(FilterOp::Morphology { input: In::SourceGraphic, dilate: true, rx: 2.0, ry: 0.5 }),
                tile,
                FilterNode::new(FilterOp::ConvolveMatrix {
                    input: In::Node(1),
                    columns: 3,
                    rows: 1,
                    kernel: vec![0.25, 0.5, 0.25],
                    divisor: 1.0,
                    bias: 0.125,
                    target_x: 1,
                    target_y: 0,
                    edge: msx_ast::ConvolveEdge::Wrap,
                    preserve_alpha: true,
                }),
            ],
        };
        let svg = emit(g, "SourceGraphic");
        assert!(svg.contains(r#"<feMorphology in="SourceGraphic" operator="dilate" radius="2 0.5" color-interpolation-filters="sRGB" result="fx3-n0"/>"#), "{svg}");
        assert!(svg.contains(r#"<feTile in="fx3-n0" x="5" y="6" width="70" height="80""#), "{svg}");
        assert!(svg.contains(r#"<feConvolveMatrix in="fx3-n1" order="3 1" kernelMatrix="0.25 0.5 0.25" divisor="1" bias="0.125" targetX="1" targetY="0" edgeMode="wrap" preserveAlpha="true""#), "{svg}");
        let erode = emit(FilterGraph { nodes: vec![FilterNode::new(FilterOp::Morphology { input: In::SourceGraphic, dilate: false, rx: 1.0, ry: 1.0 })] }, "SourceGraphic");
        assert!(erode.contains(r#"operator="erode" radius="1 1""#), "{erode}");
    }

    #[test]
    fn after_another_effect_the_alpha_comes_from_a_matrix() {
        let g = FilterGraph { nodes: vec![FilterNode::new(FilterOp::Offset { input: In::SourceAlpha, dx: 2.0, dy: 3.0 })] };
        let svg = emit(g, "fx2-out");
        assert!(svg.starts_with(r#"<feColorMatrix in="fx2-out" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0""#), "{svg}");
        assert!(svg.contains(r#"<feOffset in="fx3-alpha" dx="2" dy="3""#), "{svg}");
        // A graph that does not read the alpha does not add the matrix.
        let plain = emit(FilterGraph { nodes: vec![FilterNode::new(FilterOp::Offset { input: In::SourceGraphic, dx: 2.0, dy: 3.0 })] }, "fx2-out");
        assert!(!plain.contains("feColorMatrix"), "{plain}");
        assert!(plain.contains(r#"<feOffset in="fx2-out""#));
    }

    #[test]
    fn regions_spaces_and_function_channels_are_written() {
        let mut blur = FilterNode::new(FilterOp::Blur { input: In::SourceGraphic, rx: 4.0, ry: 0.0 });
        blur.linear = true;
        blur.region = Some(FilterRegion { x: 1.0, y: 2.5, width: 30.0, height: 40.0 });
        let ct = FilterNode::new(FilterOp::ComponentTransfer {
            input: In::Node(0),
            funcs: [TransferFn::Identity, TransferFn::Table(vec![0.0, 0.5, 1.0]), TransferFn::Linear { slope: 2.0, intercept: -0.5 }, TransferFn::Gamma { amplitude: 1.0, exponent: 2.0, offset: 0.0 }],
        });
        let svg = emit(FilterGraph { nodes: vec![blur, ct] }, "SourceGraphic");
        assert!(svg.contains(r#"<feGaussianBlur in="SourceGraphic" stdDeviation="4 0" x="1" y="2.5" width="30" height="40" color-interpolation-filters="linearRGB" result="fx3-n0"/>"#), "{svg}");
        assert!(!svg.contains("feFuncR"), "an identity channel writes nothing");
        assert!(svg.contains(r#"<feFuncG type="table" tableValues="0 0.5 1"/>"#));
        assert!(svg.contains(r#"<feFuncB type="linear" slope="2" intercept="-0.5"/>"#));
        assert!(svg.contains(r#"<feFuncA type="gamma" amplitude="1" exponent="2" offset="0"/>"#));
    }

    #[test]
    fn blend_modes_use_the_css_names_and_arithmetic_writes_its_constants() {
        let blend = FilterNode::new(FilterOp::Blend { a: In::SourceGraphic, b: In::SourceAlpha, mode: msx_ast::BlendMode::Multiply });
        let arith = FilterNode::new(FilterOp::Composite { a: In::Node(0), b: In::SourceGraphic, op: CompositeOp::Arithmetic { k1: 0.0, k2: 1.0, k3: 0.5, k4: -0.25 } });
        let svg = emit(FilterGraph { nodes: vec![blend, arith] }, "SourceGraphic");
        assert!(svg.contains(r#"mode="multiply""#));
        assert!(svg.contains(r#"operator="arithmetic" k1="0" k2="1" k3="0.5" k4="-0.25""#), "{svg}");
        let normal = emit(FilterGraph { nodes: vec![FilterNode::new(FilterOp::Blend { a: In::SourceGraphic, b: In::SourceGraphic, mode: msx_ast::BlendMode::Subtract })] }, "SourceGraphic");
        assert!(normal.contains(r#"mode="normal""#), "a mode with no CSS name falls back to normal");
    }

    #[test]
    fn an_empty_graph_passes_the_input_through() {
        assert_eq!(emit(FilterGraph::default(), "SourceGraphic"), r#"<feOffset in="SourceGraphic" dx="0" dy="0" result="out"/>"#);
    }
}
