// render/msx-render-svg/src/splat.rs
use msx_ast::{fmt_f64, Color, Def, GaussianSplat, Paint};

use crate::{write_attr, write_id, Ctx};

/// Approximate the Gaussian falloff as a 3-stop radial gradient fading from
/// full color/opacity at the center to transparent at the edge — a static
/// preview, not the real math (`msx-splat::evaluate_at` is the source of
/// truth, sampled per-pixel by `msx-render-cpu` / per-fragment by
/// `msx-render-gpu`). `2.4 * sigma` is roughly where a Gaussian's opacity
/// has fallen under ~4%, a reasonable visual cutoff for the ellipse radius.
///
/// `fill`, when set, takes priority over the plain `color` field — same
/// as every other renderer. Note this is genuinely different from how an
/// ordinary shape's `fill` is handled elsewhere in this crate: a `rect`'s
/// `Paint::Ref` is passed through untouched as a raw `url(#id)` string
/// (see `shape_referencing_a_shader_def_still_gets_its_url_fill_attribute`
/// in `lib.rs`'s tests) because SVG can resolve a *real* gradient
/// reference natively. A splat's synthetic radial gradient can't nest
/// another gradient/shader reference inside its own stops the same way —
/// there's no SVG mechanism for "this stop's color is itself a gradient"
/// — so splat resolves `fill` down to one flat representative color first
/// (gradient average, or a shader's `fallback_color`) and uses that as
/// the falloff's peak color instead. This keeps the Gaussian-blob shape
/// approximation intact while still visibly reflecting what the splat
/// actually references, rather than silently ignoring `fill` and always
/// falling back to `color`.
pub(crate) fn render_splat(ctx: &mut Ctx, s: &GaussianSplat, defs: &[Def]) {
    let gradient_id = ctx.fresh_id("splat-grad-");
    let rx = s.sigma_x * 2.4;
    let ry = s.sigma_y * 2.4;
    let resolved = match &s.fill {
        Some(paint) => resolve_flat_color(paint, defs),
        None => s.color,
    };
    let hex = resolved.to_svg_hex();
    let peak = s.opacity.clamp(0.0, 1.0);

    ctx.extra_defs.push_str(&format!(
        r#"<radialGradient id="{id}" cx="0.5" cy="0.5" r="0.5"><stop offset="0%" stop-color="{hex}" stop-opacity="{op}"/><stop offset="55%" stop-color="{hex}" stop-opacity="{mid}"/><stop offset="100%" stop-color="{hex}" stop-opacity="0"/></radialGradient>"#,
        id = gradient_id,
        hex = hex,
        op = fmt_f64(peak),
        mid = fmt_f64(peak * 0.35),
    ));

    ctx.push("<ellipse");
    write_attr(ctx, "cx", fmt_f64(s.x));
    write_attr(ctx, "cy", fmt_f64(s.y));
    write_attr(ctx, "rx", fmt_f64(rx));
    write_attr(ctx, "ry", fmt_f64(ry));
    write_attr(ctx, "fill", format!("url(#{})", gradient_id));
    write_id(ctx, s.id.as_deref());
    if s.rotation != 0.0 {
        let degrees = s.rotation.to_degrees();
        write_attr(ctx, "transform", format!("rotate({},{},{})", fmt_f64(degrees), fmt_f64(s.x), fmt_f64(s.y)));
    }
    ctx.push("/>");
}

/// Resolves any `Paint` to one flat, representative `Color` — the
/// gradient-average/shader-fallback convention `msx-render-cpu`'s
/// `average_stop_color` and `msx-render-gpu`'s function of the same name
/// both already use, reimplemented here since this crate has never needed
/// it before (every other element just re-exports its `Paint` verbatim
/// into the `fill` attribute — see this module's doc comment for why
/// splat is the one exception).
fn resolve_flat_color(paint: &Paint, defs: &[Def]) -> Color {
    match paint {
        Paint::Color(c) => *c,
        Paint::CurrentColor => Color::BLACK,
        Paint::None => Color::rgba(0, 0, 0, 0),
        Paint::Ref(reference) => {
            let Some(id) = reference.strip_prefix("url(#").and_then(|s| s.strip_suffix(')')) else {
                return Color::rgba(0, 0, 0, 0);
            };
            let Some(def) = defs.iter().find(|d| d.id() == id) else {
                return Color::rgba(0, 0, 0, 0);
            };
            average_stop_color(def)
        }
    }
}

fn average_stop_color(def: &Def) -> Color {
    let stops: &[msx_ast::Stop] = match def {
        Def::LinearGradient(g) => &g.stops,
        Def::RadialGradient(g) => &g.stops,
        Def::ConicGradient(g) => &g.stops,
        Def::Shader(s) => return s.fallback_color,
        Def::Audio(_) => return Color::rgba(0, 0, 0, 0),
    };
    if stops.is_empty() {
        return Color::BLACK;
    }
    let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
    for stop in stops {
        r += stop.color.r as u32;
        g += stop.color.g as u32;
        b += stop.color.b as u32;
        a += stop.color.a as u32;
    }
    let n = stops.len() as u32;
    Color::rgba((r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8)
}
