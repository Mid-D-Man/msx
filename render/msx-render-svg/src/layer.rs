// render/msx-render-svg/src/layer.rs
use msx_ast::{fmt_f64, Effect, Layer};

use crate::{render_element, write_attr, write_id, write_transform, Ctx};

/// `blend_mode` → CSS `mix-blend-mode` (`Subtract`/`Divide` have no CSS
/// equivalent and fall back to Normal here; `msx-render-cpu`/`-gpu`
/// implement the signed-buffer math those actually need). `effects` → a
/// generated `<filter>` chaining one SVG filter primitive per effect.
/// `clip` has no cheap SVG 1.1 fallback without a matching
/// `clipPath`/bbox computation, so it's left unenforced here.
pub(crate) fn render_layer(ctx: &mut Ctx, layer: &Layer) {
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

    ctx.push(">");
    for child in &layer.children {
        render_element(ctx, child);
    }
    ctx.push("</g>");
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
