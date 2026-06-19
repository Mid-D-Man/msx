// core/msx-parser/src/style.rs
use dixscript::Runtime::{DixData, dix_path};
use msx_ast::{FillRule, FontWeight, LineCap, LineJoin, Style, TextAnchor};

use crate::dix_helpers::{enum_discriminant, opt, paint_from, raw};

/// Parse the `style` block on the element at `element_prefix`
/// (e.g. `"elements[2]"` → reads `"elements[2].style.*"`).
///
/// Starts from `Style::default()` (fill black, stroke none, stroke_width 1,
/// opacity 1 — the SVG defaults) and overrides exactly the fields the source
/// provides, so an element that omits `style` entirely still renders sanely.
pub fn parse_style(data: &DixData, element_prefix: &str) -> Result<Style, String> {
    let prefix = dix_path(element_prefix, "style");
    let mut style = Style::default();

    if let Some(p) = paint_from(raw(data, &prefix, "fill"))   { style.fill = Some(p); }
    if let Some(p) = paint_from(raw(data, &prefix, "stroke")) { style.stroke = Some(p); }
    if let Some(w) = opt::<f64>(data, &prefix, "stroke_width")?   { style.stroke_width = Some(w); }
    if let Some(o) = opt::<f64>(data, &prefix, "opacity")?        { style.opacity = Some(o); }
    if let Some(o) = opt::<f64>(data, &prefix, "fill_opacity")?   { style.fill_opacity = Some(o); }
    if let Some(o) = opt::<f64>(data, &prefix, "stroke_opacity")? { style.stroke_opacity = Some(o); }

    if let Some(fr) = parse_fill_rule(data, &prefix)?  { style.fill_rule = Some(fr); }
    if let Some(lc) = parse_line_cap(data, &prefix)?   { style.stroke_linecap = Some(lc); }
    if let Some(lj) = parse_line_join(data, &prefix)?  { style.stroke_linejoin = Some(lj); }
    if let Some(ml) = opt::<f64>(data, &prefix, "stroke_miterlimit")? { style.stroke_miterlimit = Some(ml); }

    if let Some(arr) = opt::<Vec<f64>>(data, &prefix, "stroke_dasharray")? { style.stroke_dasharray = Some(arr); }
    if let Some(off) = opt::<f64>(data, &prefix, "stroke_dashoffset")?     { style.stroke_dashoffset = Some(off); }

    if let Some(fs) = opt::<f64>(data, &prefix, "font_size")?      { style.font_size = Some(fs); }
    if let Some(ff) = opt::<String>(data, &prefix, "font_family")? { style.font_family = Some(ff); }
    if let Some(fw) = parse_font_weight(data, &prefix)?            { style.font_weight = Some(fw); }
    if let Some(ta) = parse_text_anchor(data, &prefix)?            { style.text_anchor = Some(ta); }
    if let Some(db) = opt::<String>(data, &prefix, "dominant_baseline")? { style.dominant_baseline = Some(db); }

    if let Some(v) = opt::<String>(data, &prefix, "visibility")? { style.visibility_hidden = v == "hidden"; }
    if let Some(d) = opt::<String>(data, &prefix, "display")?    { style.display_none = d == "none"; }

    Ok(style)
}

// Each of these accepts EITHER a DixScript `@ENUMS` value (e.g.
// `fill_rule<enum> = FillRule.EvenOdd`) OR a plain string (`"evenodd"`),
// since std/enums.msx hasn't shipped to every existing example yet —
// both forms keep working forever, the enum form just reads cleaner.

fn parse_fill_rule(data: &DixData, prefix: &str) -> Result<Option<FillRule>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "fill_rule")) {
        return Ok(Some(FillRule::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "fill_rule")?.map(|s| FillRule::parse(&s)))
}

fn parse_line_cap(data: &DixData, prefix: &str) -> Result<Option<LineCap>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "stroke_linecap")) {
        return Ok(Some(LineCap::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "stroke_linecap")?.map(|s| LineCap::parse(&s)))
}

fn parse_line_join(data: &DixData, prefix: &str) -> Result<Option<LineJoin>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "stroke_linejoin")) {
        return Ok(Some(LineJoin::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "stroke_linejoin")?.map(|s| LineJoin::parse(&s)))
}

fn parse_text_anchor(data: &DixData, prefix: &str) -> Result<Option<TextAnchor>, String> {
    if let Some(v) = enum_discriminant(raw(data, prefix, "text_anchor")) {
        return Ok(Some(TextAnchor::from_byte(v as u8)));
    }
    Ok(opt::<String>(data, prefix, "text_anchor")?.map(|s| TextAnchor::parse(&s)))
}

fn parse_font_weight(data: &DixData, prefix: &str) -> Result<Option<FontWeight>, String> {
    Ok(opt::<String>(data, prefix, "font_weight")?.map(|s| FontWeight::parse(&s)))
                                                                  }
