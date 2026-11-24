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

/// `font-style`. The raster renderers slant the upright face for `Italic` and
/// `Oblique`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle { Normal, Italic, Oblique }

impl FontStyle {
    pub fn to_svg(self) -> &'static str {
        match self { FontStyle::Normal => "normal", FontStyle::Italic => "italic", FontStyle::Oblique => "oblique" }
    }
    /// Reads the first word, so `oblique 10deg` is `Oblique`. Unknown words are `Normal`.
    pub fn parse(s: &str) -> Self {
        match s.split_whitespace().next() { Some("italic") => FontStyle::Italic, Some("oblique") => FontStyle::Oblique, _ => FontStyle::Normal }
    }
    pub fn to_byte(self)    -> u8 { match self { FontStyle::Normal => 0, FontStyle::Italic => 1, FontStyle::Oblique => 2 } }
    pub fn from_byte(b: u8) -> Self { match b { 1 => FontStyle::Italic, 2 => FontStyle::Oblique, _ => FontStyle::Normal } }
    /// Whether the glyphs are drawn slanted.
    pub fn is_slanted(self) -> bool { self != FontStyle::Normal }
}

/// `writing-mode`. Vertical text runs top to bottom and turns Latin glyphs
/// a quarter turn clockwise, as in a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingMode { Horizontal, Vertical }

impl WritingMode {
    /// The SVG 2 word for the mode.
    pub fn to_svg(self) -> &'static str {
        match self { WritingMode::Horizontal => "horizontal-tb", WritingMode::Vertical => "vertical-rl" }
    }
    /// Reads the SVG 1.1 and SVG 2 words. `rl` and `rl-tb` are horizontal because
    /// they only set the direction. Anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "tb" | "tb-rl" | "vertical-rl" | "vertical-lr" => Some(WritingMode::Vertical),
            "lr" | "lr-tb" | "rl" | "rl-tb" | "horizontal-tb" => Some(WritingMode::Horizontal),
            _ => None,
        }
    }
    pub fn to_byte(self)    -> u8 { match self { WritingMode::Horizontal => 0, WritingMode::Vertical => 1 } }
    pub fn from_byte(b: u8) -> Self { if b == 1 { WritingMode::Vertical } else { WritingMode::Horizontal } }
}

/// The lines of `text-decoration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextDecoration {
    pub underline:    bool,
    pub overline:     bool,
    pub line_through: bool,
}

impl TextDecoration {
    pub fn is_none(self) -> bool { !(self.underline || self.overline || self.line_through) }
    /// Reads a list of `underline`, `overline` and `line-through`. Other words are ignored.
    pub fn parse(s: &str) -> Self {
        let mut d = TextDecoration::default();
        for w in s.split_whitespace() {
            match w {
                "underline"    => d.underline = true,
                "overline"     => d.overline = true,
                "line-through" => d.line_through = true,
                _ => {}
            }
        }
        d
    }
    /// The SVG value, `none` when no line is set.
    pub fn to_svg(self) -> String {
        let mut w: Vec<&str> = Vec::new();
        if self.underline    { w.push("underline"); }
        if self.overline     { w.push("overline"); }
        if self.line_through { w.push("line-through"); }
        if w.is_empty() { "none".into() } else { w.join(" ") }
    }
    pub fn to_byte(self) -> u8 { (self.underline as u8) | ((self.overline as u8) << 1) | ((self.line_through as u8) << 2) }
    pub fn from_byte(b: u8) -> Self { TextDecoration { underline: b & 1 != 0, overline: b & 2 != 0, line_through: b & 4 != 0 } }
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
    /// Extra space after every glyph, in user units.
    pub letter_spacing:     Option<f64>,
    pub text_decoration:    Option<TextDecoration>,
    pub font_style:         Option<FontStyle>,
    pub writing_mode:       Option<WritingMode>,
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
            letter_spacing:    None,
            text_decoration:   None,
            font_style:        None,
            writing_mode:      None,
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
            letter_spacing: None, text_decoration: None, font_style: None, writing_mode: None,
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
        if let Some(fs) = self.font_style {
            // Suppress "normal", the SVG default.
            if fs != FontStyle::Normal { a.push(("font-style", fs.to_svg().into())); }
        }
        if let Some(ls) = self.letter_spacing {
            if ls != 0.0 { a.push(("letter-spacing", fmt_f64(ls))); }
        }
        if let Some(td) = self.text_decoration {
            if !td.is_none() { a.push(("text-decoration", td.to_svg())); }
        }
        if self.writing_mode == Some(WritingMode::Vertical) {
            a.push(("writing-mode", WritingMode::Vertical.to_svg().into()));
        }
        if self.visibility_hidden { a.push(("visibility", "hidden".into())); }
        if self.display_none      { a.push(("display",    "none".into()));   }
        a
    }

    /// Whether the style sets a field that only wire version 3 can hold:
    /// `fill_opacity`, `stroke_opacity`, `dominant_baseline`, `letter_spacing`,
    /// `text_decoration`, `font_style` or `writing_mode`.
    pub fn has_v3_fields(&self) -> bool {
        self.fill_opacity.is_some() || self.stroke_opacity.is_some() || self.dominant_baseline.is_some()
            || self.letter_spacing.is_some() || self.text_decoration.is_some() || self.font_style.is_some()
            || self.writing_mode.is_some()
    }

    /// Bit flags indicating which fields are present (for binary encoding).
    /// Layout must remain stable across versions — msx-binary depends on it.
    /// Bit 7 covers `visibility_hidden`, `display_none` and, from wire version 3,
    /// the extension block of `has_v3_fields`.
    pub fn present_flags(&self) -> u8 {
        let mut f = 0u8;
        if self.fill.is_some()                                                                                   { f |= 1 << 0; }
        if self.stroke.is_some()                                                                                 { f |= 1 << 1; }
        if self.opacity.is_some()                                                                                { f |= 1 << 2; }
        if self.stroke_width.is_some()                                                                           { f |= 1 << 3; }
        if self.fill_rule.is_some() || self.stroke_linecap.is_some() || self.stroke_linejoin.is_some() || self.stroke_miterlimit.is_some() { f |= 1 << 4; }
        if self.font_size.is_some() || self.font_family.is_some() || self.font_weight.is_some() || self.text_anchor.is_some() { f |= 1 << 5; }
        if self.stroke_dasharray.is_some() || self.stroke_dashoffset.is_some()                                   { f |= 1 << 6; }
        if self.visibility_hidden || self.display_none || self.has_v3_fields()                                    { f |= 1 << 7; }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{Color, Paint};

    #[test] fn fillrule_rt()     { assert_eq!(FillRule::from_byte(FillRule::EvenOdd.to_byte()), FillRule::EvenOdd); }
    #[test] fn linecap_all()     { for lc in [LineCap::Butt, LineCap::Round, LineCap::Square] { assert_eq!(LineCap::from_byte(lc.to_byte()), lc); } }
    #[test] fn v3_fields_set_the_last_flag_bit() {
        let mut s = Style::empty();
        assert!(!s.has_v3_fields() && s.present_flags() & (1 << 7) == 0);
        s.fill_opacity = Some(0.5);
        assert!(s.has_v3_fields() && s.present_flags() & (1 << 7) != 0);
        let mut s = Style::empty(); s.stroke_opacity = Some(0.5); assert!(s.has_v3_fields());
        let mut s = Style::empty(); s.dominant_baseline = Some("middle".into()); assert!(s.has_v3_fields());
        let mut s = Style::empty(); s.letter_spacing = Some(2.0); assert!(s.has_v3_fields());
        let mut s = Style::empty(); s.text_decoration = Some(TextDecoration::default()); assert!(s.has_v3_fields());
        let mut s = Style::empty(); s.font_style = Some(FontStyle::Italic); assert!(s.has_v3_fields());
        let mut s = Style::empty(); s.writing_mode = Some(WritingMode::Horizontal); assert!(s.has_v3_fields());
    }
    #[test] fn writing_mode_words_and_bytes() {
        for w in ["tb", "tb-rl", "vertical-rl", "vertical-lr"] { assert_eq!(WritingMode::parse(w), Some(WritingMode::Vertical), "{w}"); }
        for w in ["lr", "lr-tb", "rl", "rl-tb", "horizontal-tb"] { assert_eq!(WritingMode::parse(w), Some(WritingMode::Horizontal), "{w}"); }
        assert_eq!(WritingMode::parse("random"), None);
        for m in [WritingMode::Horizontal, WritingMode::Vertical] { assert_eq!(WritingMode::from_byte(m.to_byte()), m); }
        let mut s = Style::empty();
        s.writing_mode = Some(WritingMode::Horizontal);
        assert!(!s.to_svg_attrs().iter().any(|(k, _)| *k == "writing-mode"));
        s.writing_mode = Some(WritingMode::Vertical);
        assert!(s.to_svg_attrs().contains(&("writing-mode", "vertical-rl".to_string())));
    }
    #[test] fn font_style_words_and_bytes() {
        assert_eq!(FontStyle::parse("italic"), FontStyle::Italic);
        assert_eq!(FontStyle::parse("oblique 10deg"), FontStyle::Oblique);
        assert_eq!(FontStyle::parse("slanted"), FontStyle::Normal);
        for f in [FontStyle::Normal, FontStyle::Italic, FontStyle::Oblique] { assert_eq!(FontStyle::from_byte(f.to_byte()), f); }
        assert!(FontStyle::Oblique.is_slanted() && !FontStyle::Normal.is_slanted());
    }
    #[test] fn text_decoration_words_and_bytes() {
        let d = TextDecoration::parse("underline  line-through");
        assert!(d.underline && d.line_through && !d.overline);
        assert_eq!(d.to_svg(), "underline line-through");
        assert_eq!(TextDecoration::from_byte(d.to_byte()), d);
        assert!(TextDecoration::parse("none").is_none());
        assert_eq!(TextDecoration::default().to_svg(), "none");
    }
    #[test] fn text_attrs_skip_the_defaults() {
        let mut s = Style::empty();
        s.font_style = Some(FontStyle::Normal); s.letter_spacing = Some(0.0); s.text_decoration = Some(TextDecoration::default());
        let a = s.to_svg_attrs();
        assert!(!a.iter().any(|(k, _)| matches!(*k, "font-style" | "letter-spacing" | "text-decoration")));
        s.font_style = Some(FontStyle::Italic); s.letter_spacing = Some(1.5); s.text_decoration = Some(TextDecoration::parse("underline"));
        let a = s.to_svg_attrs();
        assert!(a.contains(&("font-style", "italic".to_string())));
        assert!(a.contains(&("letter-spacing", "1.5".to_string())));
        assert!(a.contains(&("text-decoration", "underline".to_string())));
    }
    #[test] fn flags_fill_only() { let mut s = Style::empty(); s.fill = Some(Paint::Color(Color::BLACK)); assert_eq!(s.present_flags() & 1, 1); assert_eq!(s.present_flags() & 2, 0); }
    #[test] fn opacity_1_omitted() { let mut s = Style::empty(); s.opacity = Some(1.0); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "opacity")); }
    #[test] fn opacity_half_present() { let mut s = Style::empty(); s.opacity = Some(0.5); assert!(s.to_svg_attrs().iter().any(|(k,v)| *k == "opacity" && v == "0.5")); }
    #[test] fn fw_normal_omitted() { let mut s = Style::empty(); s.font_weight = Some(FontWeight::Normal); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "font-weight")); }
    #[test] fn ta_start_omitted() { let mut s = Style::empty(); s.text_anchor = Some(TextAnchor::Start); assert!(!s.to_svg_attrs().iter().any(|(k,_)| *k == "text-anchor")); }
                                                                                            }
