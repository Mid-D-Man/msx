// core/msx-ast/src/gradient.rs

use crate::color::Color;
use crate::media::AudioDef;
use crate::primitives::fmt_f64;
use crate::transform::{Matrix2D, Transform};

/// The coordinate system a linear or radial gradient's geometry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientUnits {
    /// Fractions of the bounding box of the shape that is painted. The default.
    #[default]
    ObjectBoundingBox,
    /// The local coordinates of the shape that is painted, so a gradient with
    /// these units stays put when the shape moves or has no area.
    UserSpace,
}

impl GradientUnits {
    /// The name used in `.msx` source.
    pub fn name(self) -> &'static str {
        match self {
            GradientUnits::ObjectBoundingBox => "object_bounding_box",
            GradientUnits::UserSpace => "user_space",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "object_bounding_box" => Some(GradientUnits::ObjectBoundingBox),
            "user_space" => Some(GradientUnits::UserSpace),
            _ => None,
        }
    }

    /// The value of the SVG `gradientUnits` attribute.
    pub fn svg_name(self) -> &'static str {
        match self {
            GradientUnits::ObjectBoundingBox => "objectBoundingBox",
            GradientUnits::UserSpace => "userSpaceOnUse",
        }
    }
}

/// What a gradient paints beyond its first and last stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpreadMethod {
    /// The end colors continue. The default.
    #[default]
    Pad,
    /// The gradient runs back and forth.
    Reflect,
    /// The gradient starts again.
    Repeat,
}

impl SpreadMethod {
    /// The name used in `.msx` source and in SVG `spreadMethod`.
    pub fn name(self) -> &'static str {
        match self {
            SpreadMethod::Pad => "pad",
            SpreadMethod::Reflect => "reflect",
            SpreadMethod::Repeat => "repeat",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "pad" => Some(SpreadMethod::Pad),
            "reflect" => Some(SpreadMethod::Reflect),
            "repeat" => Some(SpreadMethod::Repeat),
            _ => None,
        }
    }

    /// The wire byte, which is also the order of the variants.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(SpreadMethod::Pad),
            1 => Some(SpreadMethod::Reflect),
            2 => Some(SpreadMethod::Repeat),
            _ => None,
        }
    }
}

/// The matrix from a gradient's own space to the local space of the shape.
///
/// With `ObjectBoundingBox` units the gradient space is the unit square of
/// `bbox` (`x`, `y`, width, height), with `UserSpace` it is the local space
/// itself, and `transform` is applied inside either one, like SVG's
/// `gradientTransform`, unless it cannot be undone (see `usable_transform`).
/// `None` means a bounding box without area, which leaves nothing to paint
/// with `ObjectBoundingBox` units.
pub fn gradient_matrix(units: GradientUnits, transform: &Option<Transform>, bbox: Option<(f64, f64, f64, f64)>) -> Option<Matrix2D> {
    let base = match units {
        GradientUnits::UserSpace => Matrix2D::identity(),
        GradientUnits::ObjectBoundingBox => {
            let (x, y, w, h) = bbox?;
            if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
                return None;
            }
            Matrix2D { a: w, b: 0.0, c: 0.0, d: h, e: x, f: y }
        }
    };
    Some(match usable_transform(transform) {
        Some(m) => base.concat(m),
        None => base,
    })
}

/// The matrix of a gradient transform, or `None` when there is none or it
/// cannot be undone (a zero scale, or a value that is not a number). Such a
/// transform counts as no transform, which is what resvg draws for SVG's
/// `matrix(0 0 0 0 0 0)`, so every renderer treats it the same way.
pub fn usable_transform(transform: &Option<Transform>) -> Option<Matrix2D> {
    let m = transform.as_ref().filter(|t| !t.is_none())?.to_matrix();
    let det = m.a * m.d - m.b * m.c;
    let finite = [m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite());
    (finite && det.abs() > 1e-12).then_some(m)
}

/// Up to six decimals, for gradient geometry that must keep small values.
fn fmt_fine(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e10 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.6}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The attributes after `gradientUnits` that a gradient adds when it uses more
/// than the defaults. Empty for the defaults, so older output stays as it was.
fn extra_svg_attrs(spread: SpreadMethod, transform: &Option<Transform>) -> String {
    let mut out = String::new();
    if spread != SpreadMethod::Pad {
        out.push_str(&format!(r#" spreadMethod="{}""#, spread.name()));
    }
    if let Some(m) = usable_transform(transform) {
        out.push_str(&format!(
            r#" gradientTransform="matrix({} {} {} {} {} {})""#,
            fmt_fine(m.a), fmt_fine(m.b), fmt_fine(m.c), fmt_fine(m.d), fmt_fine(m.e), fmt_fine(m.f),
        ));
    }
    out
}

/// A single gradient color stop.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub offset: f64,  // 0.0..=1.0
    pub color:  Color,
}

impl Stop {
    pub fn new(offset: f64, color: Color) -> Self { Stop { offset, color } }

    /// 8-byte wire format: [f32 offset][u8 r g b a].
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..4].copy_from_slice(&(self.offset as f32).to_le_bytes());
        b[4..8].copy_from_slice(&self.color.to_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; 8]) -> Self {
        Stop {
            offset: f32::from_le_bytes(b[0..4].try_into().unwrap()) as f64,
            color:  Color::from_bytes(b[4..8].try_into().unwrap()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    pub id:    String,
    pub x1:    f64,
    pub y1:    f64,
    pub x2:    f64,
    pub y2:    f64,
    pub stops: Vec<Stop>,
    /// Coordinate system of the end points. Needs wire version 3 when not the default.
    pub units:     GradientUnits,
    /// What lies beyond the end points. Needs wire version 3 when not the default.
    pub spread:    SpreadMethod,
    /// Applied inside `units`, like SVG `gradientTransform`. Needs wire version 3.
    pub transform: Option<Transform>,
}

impl LinearGradient {
    pub fn new(id: String, x1: f64, y1: f64, x2: f64, y2: f64, stops: Vec<Stop>) -> Self {
        LinearGradient { id, x1, y1, x2, y2, stops, units: GradientUnits::default(), spread: SpreadMethod::default(), transform: None }
    }

    pub fn with_units(mut self, units: GradientUnits) -> Self { self.units = units; self }
    pub fn with_spread(mut self, spread: SpreadMethod) -> Self { self.spread = spread; self }
    pub fn with_transform(mut self, t: Transform) -> Self { self.transform = Some(t).filter(|t| !t.is_none()); self }

    /// Whether the gradient sets a field that only wire version 3 can hold.
    pub fn has_v3_fields(&self) -> bool {
        self.units != GradientUnits::default() || self.spread != SpreadMethod::default() || self.transform.as_ref().is_some_and(|t| !t.is_none())
    }

    pub fn to_svg(&self) -> String {
        let stops_svg: String = self.stops.iter().map(|s| format!(
            r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
            fmt_f64(s.offset), s.color.to_svg_hex(), fmt_f64(s.color.opacity()),
        )).collect::<Vec<_>>().join("");
        let fmt = if self.units == GradientUnits::UserSpace { fmt_fine } else { fmt_f64 };
        format!(
            r#"<linearGradient id="{}" x1="{}" y1="{}" x2="{}" y2="{}" gradientUnits="{}"{}>{}</linearGradient>"#,
            self.id, fmt(self.x1), fmt(self.y1), fmt(self.x2), fmt(self.y2), self.units.svg_name(),
            extra_svg_attrs(self.spread, &self.transform), stops_svg,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradient {
    pub id:    String,
    pub cx:    f64,
    pub cy:    f64,
    pub r:     f64,
    pub fx:    f64,
    pub fy:    f64,
    /// Radius of the focal circle, where the gradient starts. 0 is a focal point.
    /// Needs wire version 3 when it is above 0. Set it with [`RadialGradient::with_fr`].
    pub fr:    f64,
    pub stops: Vec<Stop>,
    /// Coordinate system of the circle and focal point. Needs wire version 3 when not the default.
    pub units:     GradientUnits,
    /// What lies beyond the circle. Needs wire version 3 when not the default.
    pub spread:    SpreadMethod,
    /// Applied inside `units`, like SVG `gradientTransform`. Needs wire version 3.
    pub transform: Option<Transform>,
}

impl RadialGradient {
    /// Sets `fr`. A value that is not above 0 or not a number counts as 0, which is what
    /// resvg draws for a negative `fr`, so every renderer sees only 0 or a real radius.
    pub fn with_fr(mut self, fr: f64) -> Self {
        self.fr = if fr.is_finite() && fr > 0.0 { fr } else { 0.0 };
        self
    }

    pub fn new(id: String, cx: f64, cy: f64, r: f64, fx: f64, fy: f64, stops: Vec<Stop>) -> Self {
        RadialGradient { id, cx, cy, r, fx, fy, fr: 0.0, stops, units: GradientUnits::default(), spread: SpreadMethod::default(), transform: None }
    }

    pub fn with_units(mut self, units: GradientUnits) -> Self { self.units = units; self }
    pub fn with_spread(mut self, spread: SpreadMethod) -> Self { self.spread = spread; self }
    pub fn with_transform(mut self, t: Transform) -> Self { self.transform = Some(t).filter(|t| !t.is_none()); self }

    /// Whether the gradient sets a field that only wire version 3 can hold.
    pub fn has_v3_fields(&self) -> bool {
        self.fr > 0.0 || self.units != GradientUnits::default() || self.spread != SpreadMethod::default() || self.transform.as_ref().is_some_and(|t| !t.is_none())
    }

    pub fn to_svg(&self) -> String {
        let stops_svg: String = self.stops.iter().map(|s| format!(
            r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
            fmt_f64(s.offset), s.color.to_svg_hex(), fmt_f64(s.color.opacity()),
        )).collect::<Vec<_>>().join("");
        let fmt = if self.units == GradientUnits::UserSpace { fmt_fine } else { fmt_f64 };
        format!(
            r#"<radialGradient id="{}" cx="{}" cy="{}" r="{}" fx="{}" fy="{}"{} gradientUnits="{}"{}>{}</radialGradient>"#,
            self.id, fmt(self.cx), fmt(self.cy), fmt(self.r),
            fmt(self.fx), fmt(self.fy),
            if self.fr > 0.0 { format!(r#" fr="{}""#, fmt(self.fr)) } else { String::new() },
            self.units.svg_name(),
            extra_svg_attrs(self.spread, &self.transform), stops_svg,
        )
    }
}

/// Conic gradient — native to CSS, approximated in SVG 1.1 export.
#[derive(Debug, Clone, PartialEq)]
pub struct ConicGradient {
    pub id:    String,
    pub cx:    f64,
    pub cy:    f64,
    /// Start angle in degrees.
    pub angle: f64,
    pub stops: Vec<Stop>,
}

impl ConicGradient {
    pub fn new(id: String, cx: f64, cy: f64, angle: f64, stops: Vec<Stop>) -> Self {
        ConicGradient { id, cx, cy, angle, stops }
    }

    pub fn to_svg(&self) -> String {
        // SVG 1.1 has no conic gradient; emit a comment.
        format!("<!-- conic gradient '{}': use SVG 2 / CSS for native support -->", self.id)
    }
}

/// A single named value fed into a shader as a uniform binding. Kept to
/// the handful of scalar/vector shapes WGSL uniforms actually need —
/// nothing here is animatable yet (see `ShaderDef` docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderUniformValue {
    Float(f32),
    Vec2(f32, f32),
    Vec3(f32, f32, f32),
    Vec4(f32, f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderUniform {
    pub name:  String,
    pub value: ShaderUniformValue,
}

impl ShaderUniform {
    pub fn new(name: impl Into<String>, value: ShaderUniformValue) -> Self {
        ShaderUniform { name: name.into(), value }
    }
}

/// A custom WGSL fill, referenced by paint `"url(#id)"` exactly like a
/// gradient — any element's `fill`/`stroke` can point at one.
///
/// `source_ref` is a path to a `.wgsl` file, relative to the `.msx`
/// source it was authored in — it is **never** resolved WGSL text.
/// DixScript has no raw/heredoc text-block syntax yet to embed shader
/// source inline without either escaping every quote/brace by hand or
/// pre-base64-encoding it through the existing `b:("...")` blob
/// constructor; an external file reference sidesteps both and keeps
/// compiled binaries small. `msx-cli compile` resolves and validates the
/// reference at compile time; a real WGSL-executing renderer would
/// resolve it again at render time.
///
/// FUTURE(raw-section): if/when DixScript grows a proper raw/heredoc
/// text-block section, inline shader source becomes viable without the
/// base64/escaping tradeoffs above. Migration path, so this doesn't need
/// another redesign when that lands:
///   1. Add `pub inline_source: Option<String>` here, alongside
///      `source_ref` (which stays — external references remain useful
///      even once inline is possible, e.g. for shared/reused shaders).
///   2. `msx-parser`'s `"shader"` arm in `gradient.rs` (search
///      `FUTURE(raw-section)` there too) gains a branch reading the new
///      raw section instead of `source_ref` when present.
///   3. Binary encode/decode (`msx-binary`'s `TAG_SHADER` arms in
///      `compiler.rs`/`scene_decode.rs`) need one new optional field —
///      everything else in this struct, and every renderer's
///      `fallback_color` handling, is unaffected.
///
/// `fallback_color` is not optional: every renderer that can't execute
/// WGSL (`msx-render-cpu`, `msx-render-svg`, and `msx-render-gpu` too,
/// until it actually grows a WGSL-executing path) paints this flat color
/// instead, so a shader-filled shape still renders *something* sane
/// everywhere rather than turning invisible or panicking.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderDef {
    pub id:             String,
    pub source_ref:     String,
    pub entry_point:    String,
    pub uniforms:       Vec<ShaderUniform>,
    pub fallback_color: Color,
}

impl ShaderDef {
    pub fn new(id: impl Into<String>, source_ref: impl Into<String>, fallback_color: Color) -> Self {
        ShaderDef {
            id: id.into(),
            source_ref: source_ref.into(),
            entry_point: "fs_main".to_string(),
            uniforms: Vec::new(),
            fallback_color,
        }
    }

    pub fn with_entry_point(mut self, entry_point: impl Into<String>) -> Self {
        self.entry_point = entry_point.into();
        self
    }

    pub fn with_uniforms(mut self, uniforms: Vec<ShaderUniform>) -> Self {
        self.uniforms = uniforms;
        self
    }
}

/// Scene-level definition — referenced by paint `"url(#id)"`.
#[derive(Debug, Clone)]
pub enum Def {
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    ConicGradient(ConicGradient),
    Shader(ShaderDef),
    Audio(AudioDef),
}

impl Def {
    pub fn id(&self) -> &str {
        match self {
            Def::LinearGradient(g) => &g.id,
            Def::RadialGradient(g) => &g.id,
            Def::ConicGradient(g)  => &g.id,
            Def::Shader(s)         => &s.id,
            Def::Audio(a)          => &a.id,
        }
    }

    pub fn to_svg(&self) -> String {
        match self {
            Def::LinearGradient(g) => g.to_svg(),
            Def::RadialGradient(g) => g.to_svg(),
            Def::ConicGradient(g)  => g.to_svg(),
            // SVG 1.1 can't execute WGSL and there's no paint-server
            // element to emit — same "document the gap" approach as
            // ConicGradient above. A shape whose fill/stroke references
            // this id will resolve to an unmatched `url(#id)` and render
            // as `none` in SVG export specifically; the CPU/GPU raster
            // paths use `fallback_color` instead (see their `Def::Shader`
            // handling), which is why SVG is documented as the export
            // path and native raster as the primary one.
            Def::Shader(s) => format!(
                "<!-- shader '{}': WGSL has no SVG equivalent, renders as none in SVG export \
                 (native raster paths use fallback_color {} instead) -->",
                s.id, s.fallback_color.to_svg_hex()
            ),
            Def::Audio(a) => format!("<!-- audio '{}': not renderable, no playback anywhere in this project yet -->", a.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    #[test]
    fn stop_roundtrip() {
        let s = Stop::new(0.5, Color::rgb(255, 128, 0));
        let s2 = Stop::from_bytes(&s.to_bytes());
        assert!((s.offset - s2.offset).abs() < 1e-4);
        assert_eq!(s.color, s2.color);
    }

    #[test]
    fn shader_def_id_and_svg_fallback() {
        let shader = ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(107, 70, 255))
            .with_entry_point("main_fs")
            .with_uniforms(vec![
                ShaderUniform::new("speed", ShaderUniformValue::Float(1.5)),
                ShaderUniform::new("resolution", ShaderUniformValue::Vec2(800.0, 600.0)),
            ]);
        let def = Def::Shader(shader);

        assert_eq!(def.id(), "plasma_1");
        let svg = def.to_svg();
        // No real paint-server element — just a documented, harmless comment.
        assert!(svg.starts_with("<!--"));
        assert!(svg.contains("plasma_1"));
    }

    fn two_stops() -> Vec<Stop> {
        vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(1.0, Color::rgb(0, 0, 255))]
    }

    #[test]
    fn a_default_gradient_writes_the_svg_it_always_did() {
        let l = LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, two_stops());
        assert!(l.to_svg().starts_with(r#"<linearGradient id="g" x1="0" y1="0" x2="1" y2="0" gradientUnits="objectBoundingBox">"#), "{}", l.to_svg());
        let r = RadialGradient::new("r".into(), 0.5, 0.5, 0.5, 0.5, 0.5, two_stops());
        assert!(r.to_svg().starts_with(r#"<radialGradient id="r" cx="0.5" cy="0.5" r="0.5" fx="0.5" fy="0.5" gradientUnits="objectBoundingBox">"#), "{}", r.to_svg());
        assert!(!l.has_v3_fields() && !r.has_v3_fields());
    }

    #[test]
    fn units_spread_and_transform_reach_the_svg() {
        let l = LinearGradient::new("g".into(), 10.0, 0.0, 90.5, 0.0, two_stops())
            .with_units(GradientUnits::UserSpace)
            .with_spread(SpreadMethod::Reflect)
            .with_transform(Transform::Matrix(Matrix2D { a: 0.00125, b: 0.5, c: -0.5, d: 1.0, e: 3.0, f: 4.25 }));
        let svg = l.to_svg();
        assert!(svg.contains(r#"gradientUnits="userSpaceOnUse""#), "{svg}");
        assert!(svg.contains(r#"spreadMethod="reflect""#), "{svg}");
        assert!(svg.contains(r#"gradientTransform="matrix(0.00125 0.5 -0.5 1 3 4.25)""#), "small matrix entries keep their digits: {svg}");
        assert!(svg.contains(r#"x2="90.5""#));
        assert!(l.has_v3_fields());
        let r = RadialGradient::new("r".into(), 0.5, 0.5, 0.5, 0.5, 0.5, two_stops()).with_spread(SpreadMethod::Repeat);
        assert!(r.to_svg().contains(r#"spreadMethod="repeat""#) && r.has_v3_fields());
    }

    #[test]
    fn a_transform_that_cannot_be_undone_counts_as_none() {
        let zero = Transform::Matrix(Matrix2D { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0, f: 0.0 });
        assert!(usable_transform(&Some(zero.clone())).is_none());
        assert!(usable_transform(&Some(Transform::Scale { x: 0.0, y: 1.0 })).is_none());
        assert!(usable_transform(&Some(Transform::Translate { x: f64::NAN, y: 0.0 })).is_none());
        assert!(usable_transform(&Some(Transform::Scale { x: 2.0, y: 3.0 })).is_some());
        // The gradient keeps the field, but it moves nothing and is not written to SVG.
        let l = LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, two_stops()).with_transform(zero);
        assert!(!l.to_svg().contains("gradientTransform"), "{}", l.to_svg());
        let m = gradient_matrix(GradientUnits::UserSpace, &l.transform, None).unwrap();
        assert!(m.is_identity());
    }

    #[test]
    fn fr_is_a_radius_or_nothing() {
        let g = |fr| RadialGradient::new("r".into(), 0.5, 0.5, 0.5, 0.5, 0.5, two_stops()).with_fr(fr);
        assert_eq!(g(0.2).fr, 0.2);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(g(bad).fr, 0.0, "{bad}");
        }
        assert!(g(0.2).has_v3_fields() && !g(0.0).has_v3_fields() && !g(-1.0).has_v3_fields());
        assert!(g(0.2).to_svg().contains(r#" fr="0.2" gradientUnits"#), "{}", g(0.2).to_svg());
        assert!(!g(-1.0).to_svg().contains("fr="), "a negative fr is not written");
    }

    #[test]
    fn an_empty_transform_counts_as_none() {
        let l = LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, two_stops()).with_transform(Transform::None);
        assert!(l.transform.is_none() && !l.has_v3_fields());
        assert!(!l.to_svg().contains("gradientTransform"));
    }

    #[test]
    fn names_and_bytes_roundtrip() {
        for s in [SpreadMethod::Pad, SpreadMethod::Reflect, SpreadMethod::Repeat] {
            assert_eq!(SpreadMethod::from_name(s.name()), Some(s));
            assert_eq!(SpreadMethod::from_byte(s.to_byte()), Some(s));
        }
        for u in [GradientUnits::ObjectBoundingBox, GradientUnits::UserSpace] {
            assert_eq!(GradientUnits::from_name(u.name()), Some(u));
        }
        assert_eq!(SpreadMethod::from_byte(3), None);
        assert_eq!(SpreadMethod::from_name("mirror"), None);
        assert_eq!(GradientUnits::from_name("userSpaceOnUse"), None, "source uses the snake case names");
    }

    #[test]
    fn the_gradient_matrix_maps_gradient_space_to_local_space() {
        let bbox = Some((10.0, 20.0, 100.0, 50.0));
        let m = gradient_matrix(GradientUnits::ObjectBoundingBox, &None, bbox).unwrap();
        let p = m.transform_point(crate::primitives::Point::new(0.5, 1.0));
        assert!((p.x - 60.0).abs() < 1e-9 && (p.y - 70.0).abs() < 1e-9);
        // The transform acts inside the box: here it moves the gradient half a box to the right.
        let t = Some(Transform::Translate { x: 0.5, y: 0.0 });
        let m = gradient_matrix(GradientUnits::ObjectBoundingBox, &t, bbox).unwrap();
        let p = m.transform_point(crate::primitives::Point::new(0.0, 0.0));
        assert!((p.x - 60.0).abs() < 1e-9 && (p.y - 20.0).abs() < 1e-9);
        // User space ignores the box, even one without area.
        let m = gradient_matrix(GradientUnits::UserSpace, &t, None).unwrap();
        assert!((m.e - 0.5).abs() < 1e-12 && m.a == 1.0);
        let m = gradient_matrix(GradientUnits::UserSpace, &None, Some((0.0, 0.0, 0.0, 0.0))).unwrap();
        assert!(m.is_identity());
    }

    #[test]
    fn a_box_without_area_leaves_nothing_to_paint_in_box_units() {
        assert!(gradient_matrix(GradientUnits::ObjectBoundingBox, &None, None).is_none());
        assert!(gradient_matrix(GradientUnits::ObjectBoundingBox, &None, Some((0.0, 0.0, 0.0, 5.0))).is_none());
        assert!(gradient_matrix(GradientUnits::ObjectBoundingBox, &None, Some((0.0, 0.0, f64::NAN, 5.0))).is_none());
    }
}
