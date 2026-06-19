// core/msx-ast/src/style.rs

use crate::color::Paint;
use crate::primitives::fmt_f64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule { NonZero, EvenOdd }

impl FillRule {
    pub fn to_svg(self)     -> &'static str { match self { FillRule::NonZero => "nonzero", FillRule::EvenOdd => "evenodd" } }
    pub fn parse(s: &str)   -> Self         { if s == "evenodd" { FillRule::EvenOdd } else { FillRule::NonZero } }
    pub fn to_byte(self)    -> u8           { self as u8 }
    pub fn from_byte(b: u8) -> Self         { if b == 1 { FillRule::EvenOdd } else { FillRule::NonZero } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap { Butt, Round, Square }

impl LineCap {
    pub fn to_svg(self) -> &'static str {
        match self { LineCap::Butt => "butt", LineCap::Round => "round", LineCap::Square => "square" }
    }
    pub fn parse(s: &str) -> Self {
        match s { "round" => LineCap::Round, "square" => LineCap::Square, _ => LineCap::Butt }
    }
    pub fn to_byte(self)    -> u8 { match self { LineCap::Butt => 0, LineCap::Round => 1, LineCap::Square => 2 } }
    pub fn from_byte(b: u8) -> Self { match b { 1 => LineCap::Round, 2 => LineCap::Square, _ => LineCap::Butt } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin { Miter, Round, Bevel }

impl LineJoin {
    pub fn to_svg(self) -> &'static str {
        match self { LineJoin::Miter => "miter", LineJoin::Round => "round", LineJoin::Bevel => "bevel" }
    }
    pub fn parse(s: &str) -> Self {
        match s { "round" => LineJoin::Round, "bevel" => LineJoin::Bevel, _ => LineJoin::Miter }
    }
    pub fn to_byte(self)    -> u8 { match self { LineJoin::Miter => 0, LineJoin::Round => 1, LineJoin::Bevel => 2 } }
    pub fn from_byte(b: u8) -> Self { match b { 1 => LineJoin::Round, 2 => LineJoin::Bevel, _ => LineJoin::Miter } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor { Start, Middle, End }

impl TextAnchor {
    pub fn to_svg(self) -> &'static str {
        match self { TextAnchor::Start => "start", TextAnchor::Middle => "middle", TextAnchor::End => "end" }
    }
    pub fn parse(s: &str) -> Self {
        match s { "middle" => TextAnchor::Middle, "end" => TextAnchor::End, _ => TextAnchor::Start }
    }
    /// Raw byte (0=Start 1=Middle 2=End). Encoder adds +1 so 0 means None on wire.
    pub fn to_byte(self) -> u8 { match self { TextAnchor::Start => 0, TextAnchor::Middle => 1, TextAnchor::End => 2 } }
    pub fn from_byte(b: u8) -> Self { match b { 1 => TextAnchor::Middle, 2 => TextAnchor::End, _ => TextAnchor::Start } }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FontWeight { Normal, Bold, Numeric(u16) }

impl FontWeight {
    pub fn to_svg(&self) -> String {
        match self { FontWeight::Normal => "normal".into(), FontWeight::Bold => "bold".into(), FontWeight::Numeric(n) => n.to_string() }
    }
    pub fn parse(s: &str) -> Self {
        match s { "normal" => FontWeight::Normal, "bold" => FontWeight::Bold, other => other.parse::<u16>().map(FontWeight::Numeric).unwrap_or(FontWeight::Normal) }
    }
    /// Raw byte (0=Normal 1=Bold 2..=Numeric/100). Encoder adds +1 so 0 = None on wire.
    pub fn to_byte(&self) -> u8 {
        match self { FontWeight::Normal => 0, FontWeight::Bold => 1, FontWeight::Numeric(n) => (*n / 100).min(9) as u8 + 2 }
    }
    pub fn from_byte(b: u8) -> Self {
        match b { 0 => FontWeight::Normal, 1 => FontWeight::Bold, n => FontWeight::Numeric((n - 2) as u16 * 100) }
    }
}

/// Per-element style properties. All fields are `Option` — None means "inherit / not set".
/// Binary encoding uses `present_flags()` to skip unset fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub fill:               Option<Paint>,
    pub stroke:             Option<Paint>,
    pub stroke_width:       Option<f64>,
    pub opacity:            Option<f64>,
    pub fill_opacity:       Option<f64>,
    pub stroke_opacity:     Option<f64>,
    pub fill_rule:          Option<FillRule>,
    pub stroke_linecap:     Option<LineCap>,
    pub stroke_linejoin:    Option<LineJoin>,
    pub stroke_miterlimit:  Option<f64>,
    pub stroke_dasharray:   Option<Vec<f64>>,
    pub stroke_dashoffset:  Option<f64>,
    pub font_size:          Option<f64>,
    pub font_family:        Option<String>,
    pub font_weight:        Option<FontWeight>,
    pub text_anchor:        Option<TextAnchor>,
    pub dominant_baseline:  Option<String>,
    pub visibility_hidden:  bool,
    pub display_none:       bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill:              Some(Paint::Color(crate::color::Color::BLACK)),
            stroke:            Some(Paint::None),
            stroke_width:      Some(1.0),
            opacity:           Some(1.0),
            fill_opacity:      None,
            stroke_opacity:    None,
            fill_rule:         None,
            stroke_linecap:    None,
            stroke_linejoin:   None,
            stroke_miterlimit: None,
            stroke_dasharray:  None,
            stroke_dashoffset: None,
            font_size:         None,
            font_family:       None,
            font_weight:       None,
            text_anchor:       None,
            dominant_baseline: None,
            visibility_hidden: false,
            display_none:      false,
        }
    }
}

impl Style {
    pub fn empty() -> Self {
        Style {
            fill: None, stroke: None, stroke_width: None, opacity: None,
            fill_opacity: None, stroke_opacity: None, fill_rule: None,
            stroke_linecap: None, stroke_linejoin: None, stroke_miterlimit: None,
            stroke_dasharray: None, stroke_dashoffset: None,
            font_size: None, font_family: None, font_weight: None,
            text_anchor: None, dominant_baseline: None,
            visibility_hidden: false, display_none: false,
        }
    }

    pub fn none() -> Self {
        Style { fill: Some(Paint::None), stroke: Some(Paint::None), stroke_width: Some(0.0), opacity: Some(1.0), ..Self::empty() }
    }

    /// Collect non-None fields as SVG attribute (key, value) pairs.
    /// SVG default values are suppressed to keep source→SVG identical to
    /// binary→SVG (roundtrip pixel-perfect).
    pub fn to_svg_attrs(&self) -> Vec<(&'static str, String)> {
        let mut a: Vec<(&'static str, String)> = Vec::new();

        if let Some(ref p) = self.fill   { a.push(("fill",   p.to_svg_value())); }
        if let Some(ref p) = self.stroke { a.push(("stroke", p.to_svg_value())); }
        if let Some(sw)    = self.stroke_width { a.push(("stroke-width", fmt_f64(sw))); }
        if let Some(op)    = self.opacity {
            if (op - 1.0).abs() > 1e-6 { a.push(("opacity", fmt_f64(op))); }
        }
        if let Some(fo) = self.fill_opacity   { a.push(("fill-opacity",   fmt_f64(fo))); }
        if let Some(so) = self.stroke_opacity  { a.push(("stroke-opacity", fmt_f64(so))); }
        if let Some(fr) = self.fill_rule {
            if fr != FillRule::NonZero { a.push(("fill-rule", fr.to_svg().into())); }
        }
        if let Some(lc) = self.stroke_linecap {
            if lc != LineCap::Butt { a.push(("stroke-linecap", lc.to_svg().into())); }
        }
        if let Some(lj) = self.stroke_linejoin {
            if lj != LineJoin::Miter { a.push(("stroke-linejoin", lj.to_svg().into())); }
        }
        if let Some(ml) = self.stroke_miterlimit { a.push(("stroke-miterlimit", fmt_f64(ml))); }
        if let Some(ref da) = self.stroke_dasharray {
            a.push(("stroke-dasharray", da.iter().map(|v| fmt_f64(*v)).collect::<Vec<_>>().join(" ")));
        }
        if let Some(do_) = self.stroke_dashoffset { a.push(("stroke-dashoffset", fmt_f64(do_))); }
        if let Some(fs)  = self.font_size          { a.push(("font-size",   fmt_f64(fs))); }
        if let Some(ref ff) = self.font_family     { a.push(("font-family", ff.clone())); }
        if let Some(ref fw) = self.font_weight {
            // Suppress "normal" — it is the SVG default.
            if *fw != FontWeight::Normal { a.push(("font-weight", fw.to_svg())); }
        }
        if let Some(ta) = self.text_anchor {
            // Suppress "start" — it is the SVG default.
            if ta != TextAnchor::Start { a.push(("text-anchor", ta.to_svg().into())); }
        }
        if let Some(ref db) = self.dominant_baseline { a.push(("dominant-baseline", db.clone())); }
        if self.visibility_hidden { a.push(("visibility", "hidden".into())); }
        if self.display_none      { a.push(("display",    "none".into()));   }
        a
    }

    /// Bit flags indicating which fields are present (for binary encoding).
    /// Layout must remain stable across versions — msx-binary depends on it.
    pub fn present_flags(&self) -> u8 {
        let mut f = 0u8;
        if self.fill.is_some()                                                                                   { f |= 1 << 0; }
        if self.stroke.is_some()                                                                                 { f |= 1 << 1; }
        if self.opacity.is_some()                                                                                { f |= 1 << 2; }
        if self.stroke_width.is_some()                                                                           { f |= 1 << 3; }
        if self.fill_rule.is_some() || self.stroke_linecap.is_some() || self.stroke_linejoin.is_some()          { f |= 1 << 4; }
        if self.font_size.is_some() || self.font_family.is_some() || self.font_weight.is_some() || self.text_anchor.is_some() { f |= 1 << 5; }
        if self.stroke_dasharray.is_some()                                                                       { f |= 1 << 6; }
        if self.visibility_hidden || self.display_none                                                           { f |= 1 << 7; }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{Color, Paint};

    #[test] fn fillrule_rt()     { assert_eq!(FillRule::from_byte(FillRule::EvenOdd.to_byte()), FillRule::EvenOdd); }
    #[test] fn linecap_all()     { for lc in [LineCap::Butt, LineCap::Round, LineCap::Square] { assert_eq!(LineCap::from_byte(lc.to_byte()), lc); } }
    #[test] fn flags_fill_only() { let mut s = Style::empty(); s.fill = Some(Paint::Color(Color::BLACK)); assert_eq!(s.present_flags() & 1, 1); assert_eq!(s.present_flags() & 2, 0); }
    #[test] fn opacity_1_omitted() { let mut s = Style::empty(); s.opacity = Some(1.0); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "opacity")); }
    #[test] fn opacity_half_present() { let mut s = Style::empty(); s.opacity = Some(0.5); assert!(s.to_svg_attrs().iter().any(|(k,v)| *k == "opacity" && v == "0.5")); }
    #[test] fn fw_normal_omitted() { let mut s = Style::empty(); s.font_weight = Some(FontWeight::Normal); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "font-weight")); }
    #[test] fn ta_start_omitted() { let mut s = Style::empty(); s.text_anchor = Some(TextAnchor::Start); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "text-anchor")); }
                                                                                            }
