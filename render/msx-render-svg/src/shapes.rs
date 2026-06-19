// render/msx-render-svg/src/shapes.rs
use msx_ast::path::commands_to_d;
use msx_ast::{fmt_f64, Circle, Ellipse, Group, Line, Path, Polyline, Rect, Text, Use};

use crate::{escape_attr, escape_text, render_element, write_attr, write_id, write_style, write_transform, Ctx};

pub(crate) fn render_rect(ctx: &mut Ctx, r: &Rect) {
    ctx.push("<rect");
    write_attr(ctx, "x", fmt_f64(r.x));
    write_attr(ctx, "y", fmt_f64(r.y));
    write_attr(ctx, "width", fmt_f64(r.width));
    write_attr(ctx, "height", fmt_f64(r.height));
    if let Some(rx) = r.rx { write_attr(ctx, "rx", fmt_f64(rx)); }
    if let Some(ry) = r.ry { write_attr(ctx, "ry", fmt_f64(ry)); }
    write_id(ctx, r.id.as_deref());
    write_transform(ctx, r.transform.as_ref());
    write_style(ctx, &r.style);
    ctx.push("/>");
}

pub(crate) fn render_circle(ctx: &mut Ctx, c: &Circle) {
    ctx.push("<circle");
    write_attr(ctx, "cx", fmt_f64(c.cx));
    write_attr(ctx, "cy", fmt_f64(c.cy));
    write_attr(ctx, "r", fmt_f64(c.r));
    write_id(ctx, c.id.as_deref());
    write_transform(ctx, c.transform.as_ref());
    write_style(ctx, &c.style);
    ctx.push("/>");
}

pub(crate) fn render_ellipse(ctx: &mut Ctx, e: &Ellipse) {
    ctx.push("<ellipse");
    write_attr(ctx, "cx", fmt_f64(e.cx));
    write_attr(ctx, "cy", fmt_f64(e.cy));
    write_attr(ctx, "rx", fmt_f64(e.rx));
    write_attr(ctx, "ry", fmt_f64(e.ry));
    write_id(ctx, e.id.as_deref());
    write_transform(ctx, e.transform.as_ref());
    write_style(ctx, &e.style);
    ctx.push("/>");
}

pub(crate) fn render_line(ctx: &mut Ctx, l: &Line) {
    ctx.push("<line");
    write_attr(ctx, "x1", fmt_f64(l.x1));
    write_attr(ctx, "y1", fmt_f64(l.y1));
    write_attr(ctx, "x2", fmt_f64(l.x2));
    write_attr(ctx, "y2", fmt_f64(l.y2));
    write_id(ctx, l.id.as_deref());
    write_transform(ctx, l.transform.as_ref());
    write_style(ctx, &l.style);
    ctx.push("/>");
}

/// Shared by `Element::Polyline` and `Element::Polygon` — both wrap the same
/// `Polyline` struct (`Polygon` is a type alias, not a distinct shape), and
/// `p.closed` (not which enum variant matched) decides the tag name. This
/// matters: after a binary roundtrip the decoder always reconstructs
/// `Element::Polyline { closed, .. }` regardless of the original tag — so
/// dispatching on `closed` here, rather than the enum variant, is what keeps
/// `render(parsed_scene) == render(decoded_scene)`.
pub(crate) fn render_polyline(ctx: &mut Ctx, p: &Polyline) {
    let tag = if p.closed { "polygon" } else { "polyline" };
    ctx.push("<");
    ctx.push(tag);
    let points = p
        .points
        .iter()
        .map(|pt| format!("{},{}", fmt_f64(pt.x), fmt_f64(pt.y)))
        .collect::<Vec<_>>()
        .join(" ");
    write_attr(ctx, "points", points);
    write_id(ctx, p.id.as_deref());
    write_transform(ctx, p.transform.as_ref());
    write_style(ctx, &p.style);
    ctx.push("/>");
}

pub(crate) fn render_path(ctx: &mut Ctx, p: &Path) {
    ctx.push("<path");
    // Always re-derive `d` from `p.commands`, never from `p.d_raw` — after a
    // binary roundtrip there's no `d_raw` provenance to fall back to, so
    // using the canonical re-serialization on both sides is what guarantees
    // byte-identical SVG between a freshly-parsed scene and a decoded one.
    write_attr(ctx, "d", escape_attr(&commands_to_d(&p.commands)));
    write_id(ctx, p.id.as_deref());
    write_transform(ctx, p.transform.as_ref());
    write_style(ctx, &p.style);
    ctx.push("/>");
}

pub(crate) fn render_text(ctx: &mut Ctx, t: &Text) {
    ctx.push("<text");
    write_attr(ctx, "x", fmt_f64(t.x));
    write_attr(ctx, "y", fmt_f64(t.y));
    write_id(ctx, t.id.as_deref());
    write_transform(ctx, t.transform.as_ref());
    write_style(ctx, &t.style);
    ctx.push(">");
    ctx.push(&escape_text(&t.content));
    ctx.push("</text>");
}

pub(crate) fn render_group(ctx: &mut Ctx, g: &Group) {
    ctx.push("<g");
    write_id(ctx, g.id.as_deref());
    write_transform(ctx, g.transform.as_ref());
    if let Some(ref style) = g.style {
        write_style(ctx, style);
    }
    ctx.push(">");
    for child in &g.children {
        render_element(ctx, child);
    }
    ctx.push("</g>");
}

pub(crate) fn render_use(ctx: &mut Ctx, u: &Use) {
    ctx.push("<use");
    write_attr(ctx, "href", escape_attr(&u.href));
    write_attr(ctx, "x", fmt_f64(u.x));
    write_attr(ctx, "y", fmt_f64(u.y));
    write_id(ctx, u.id.as_deref());
    write_transform(ctx, u.transform.as_ref());
    ctx.push("/>");
  }
