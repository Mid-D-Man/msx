// render/msx-render-svg/src/splat.rs
use msx_ast::{fmt_f64, GaussianSplat};

use crate::{write_attr, write_id, Ctx};

/// Approximate the Gaussian falloff as a 3-stop radial gradient fading from
/// full color/opacity at the center to transparent at the edge — a static
/// preview, not the real math (`msx-splat::evaluate_at` is the source of
/// truth, sampled per-pixel by `msx-render-cpu` / per-fragment by
/// `msx-render-gpu`). `2.4 * sigma` is roughly where a Gaussian's opacity
/// has fallen under ~4%, a reasonable visual cutoff for the ellipse radius.
pub(crate) fn render_splat(ctx: &mut Ctx, s: &GaussianSplat) {
    let gradient_id = ctx.fresh_id("splat-grad-");
    let rx = s.sigma_x * 2.4;
    let ry = s.sigma_y * 2.4;
    let hex = s.color.to_svg_hex();
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
