// render/msx-render-cpu/src/gradient.rs
//! Gradient paint for the CPU renderer.
//!
//! Linear and radial gradients become real `tiny-skia` shaders. Their units
//! (bounding box or user space), `spread` and `transform` follow the SVG
//! output, and a gradient in bounding-box units needs a box with area. `tiny-skia` 0.11
//! has no conic shader, so conic gradients are drawn by rasterizing the
//! shape coverage into a scratch pixmap and recoloring each covered pixel.
//!
//! Conic convention: angle 0 starts at the top and runs clockwise, like CSS
//! `conic-gradient`, measured in local space so the sweep is not distorted
//! by the bounding box aspect ratio.

use msx_ast::{gradient_matrix, Color, Def, Matrix2D, SpreadMethod, Stop};
use tiny_skia::{
    Color as SkColor, FillRule as SkFillRule, GradientStop, LinearGradient, Paint as SkPaint, Path as SkPath,
    PixmapPaint, Point, RadialGradient, SpreadMode, Stroke, Transform,
};
use tiny_skia::Pixmap;

use crate::geom::{apply_matrix, invert_matrix};

/// Bounding box of the geometry being painted, in local (pre-transform) space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bbox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bbox {
    pub fn from_path(path: &SkPath) -> Option<Bbox> {
        let r = path.compute_tight_bounds()?;
        Some(Bbox { x: r.left(), y: r.top(), w: r.width(), h: r.height() })
    }

    /// `(left, top, width, height)` as produced by `msx_text`.
    pub fn from_text_box(b: (f64, f64, f64, f64)) -> Bbox {
        Bbox { x: b.0 as f32, y: b.1 as f32, w: b.2 as f32, h: b.3 as f32 }
    }

    /// A gradient in `objectBoundingBox` units needs a box with area.
    fn usable(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }
}

/// A paint ready to draw.
pub(crate) enum ResolvedPaint {
    /// Flat color or a `tiny-skia` gradient shader.
    Shader(SkPaint<'static>),
    /// Conic gradient, drawn through the recolor path.
    Conic(ConicPaint),
    /// Radial gradient with a focal circle, also drawn through the recolor path.
    TwoPoint(TwoPointPaint),
    /// Valid reference that paints nothing (for example an empty bounding box).
    Nothing,
}

pub(crate) struct ConicPaint {
    cx: f32,
    cy: f32,
    angle_deg: f32,
    /// Offsets in `0..=1`, non-decreasing; colors straight (not premultiplied)
    /// with the element opacity already folded into alpha.
    stops: Vec<(f32, [f32; 4])>,
}

impl ConicPaint {
    /// Gradient parameter in `0..1` for a local-space point.
    pub fn t_at(&self, lx: f32, ly: f32) -> f32 {
        let deg = (ly - self.cy).atan2(lx - self.cx).to_degrees() + 90.0 - self.angle_deg;
        (deg / 360.0).rem_euclid(1.0)
    }

    pub fn color_at(&self, t: f32) -> [f32; 4] {
        stops_color(&self.stops, t)
    }
}

/// The color at `t` between straight-alpha stops. Before the first stop and after
/// the last one the end colors hold.
fn stops_color(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t >= a.0 && t <= b.0 {
            let span = b.0 - a.0;
            let k = if span <= f32::EPSILON { 1.0 } else { (t - a.0) / span };
            return [
                a.1[0] + (b.1[0] - a.1[0]) * k,
                a.1[1] + (b.1[1] - a.1[1]) * k,
                a.1[2] + (b.1[2] - a.1[2]) * k,
                a.1[3] + (b.1[3] - a.1[3]) * k,
            ];
        }
    }
    last.1
}

/// A radial gradient between a focal circle (center `f`, radius `fr`) at `t = 0`
/// and the circle (center `c`, radius `r`) at `t = 1`. `tiny-skia` only takes a
/// focal point, so a gradient with `fr > 0` is evaluated per pixel here.
pub(crate) struct TwoPointPaint {
    /// Local space to gradient space.
    to_gradient: Matrix2D,
    c: (f64, f64),
    r: f64,
    f: (f64, f64),
    fr: f64,
    spread: SpreadMethod,
    stops: Vec<(f32, [f32; 4])>,
}

impl TwoPointPaint {
    /// The largest `t` whose circle has a positive radius and passes through the
    /// point, which is the rule the canvas and SVG 2 give. `None` outside the cone.
    fn parameter(&self, gx: f64, gy: f64) -> Option<f64> {
        let (cdx, cdy) = (self.c.0 - self.f.0, self.c.1 - self.f.1);
        let (pdx, pdy) = (gx - self.f.0, gy - self.f.1);
        let dr = self.r - self.fr;
        let a = cdx * cdx + cdy * cdy - dr * dr;
        let b = pdx * cdx + pdy * cdy + self.fr * dr;
        let c = pdx * pdx + pdy * pdy - self.fr * self.fr;
        let radius_ok = |t: f64| self.fr + t * dr >= 0.0;
        if a.abs() < 1e-9 {
            // The circles touch: one root.
            if b.abs() < 1e-12 {
                return None;
            }
            let t = c / (2.0 * b);
            return radius_ok(t).then_some(t);
        }
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        let (t1, t2) = ((b + root) / a, (b - root) / a);
        let (hi, lo) = (t1.max(t2), t1.min(t2));
        if radius_ok(hi) {
            Some(hi)
        } else if radius_ok(lo) {
            Some(lo)
        } else {
            None
        }
    }
}

/// A paint that colors a pixel from where it lies in local space.
pub(crate) trait Recolor {
    /// The straight-alpha color there, or `None` where the paint draws nothing.
    fn color_at_local(&self, lx: f32, ly: f32) -> Option<[f32; 4]>;
}

impl Recolor for ConicPaint {
    fn color_at_local(&self, lx: f32, ly: f32) -> Option<[f32; 4]> {
        Some(self.color_at(self.t_at(lx, ly)))
    }
}

impl Recolor for TwoPointPaint {
    fn color_at_local(&self, lx: f32, ly: f32) -> Option<[f32; 4]> {
        let (gx, gy) = apply_matrix(self.to_gradient, (lx, ly));
        let t = self.parameter(f64::from(gx), f64::from(gy))? as f32;
        let t = match self.spread {
            SpreadMethod::Pad => t.clamp(0.0, 1.0),
            SpreadMethod::Repeat => t - t.floor(),
            SpreadMethod::Reflect => {
                let m = t - 2.0 * (t * 0.5).floor();
                if m > 1.0 { 2.0 - m } else { m }
            }
        };
        Some(stops_color(&self.stops, t))
    }
}

/// Resolve a gradient definition. Returns `None` for defs that are not
/// gradients (shaders, audio), so the caller can use its own fallback.
pub(crate) fn resolve(def: &Def, bbox: Option<Bbox>, opacity: f32) -> Option<ResolvedPaint> {
    let stops = match def {
        Def::LinearGradient(g) => &g.stops,
        Def::RadialGradient(g) => &g.stops,
        Def::ConicGradient(g) => &g.stops,
        Def::Shader(_) | Def::Audio(_) => return None,
    };
    let norm = normalize_stops(stops, opacity);
    if norm.is_empty() {
        return Some(ResolvedPaint::Nothing);
    }
    let b = bbox.filter(Bbox::usable);
    let bbox_f64 = b.map(|b| (f64::from(b.x), f64::from(b.y), f64::from(b.w), f64::from(b.h)));
    // A gradient in bounding-box units has nothing to paint with a box that has no area;
    // one in user-space units does not need the box at all.
    let space = |units, transform: &Option<msx_ast::Transform>| gradient_matrix(units, transform, bbox_f64).map(sk_transform);

    match def {
        Def::LinearGradient(g) => {
            let Some(to_local) = space(g.units, &g.transform) else {
                return Some(ResolvedPaint::Nothing);
            };
            let start = Point::from_xy(g.x1 as f32, g.y1 as f32);
            let end = Point::from_xy(g.x2 as f32, g.y2 as f32);
            Some(shader_paint(LinearGradient::new(start, end, sk_stops(&norm), spread_mode(g.spread), to_local)))
        }
        Def::RadialGradient(g) => {
            let Some(to_local) = space(g.units, &g.transform) else {
                return Some(ResolvedPaint::Nothing);
            };
            let r = g.r as f32;
            if r <= 0.0 {
                // SVG paints the last stop color when the radius is zero.
                return Some(ResolvedPaint::Shader(solid(norm[norm.len() - 1].1)));
            }
            if g.fr > 0.0 {
                let Some(m) = gradient_matrix(g.units, &g.transform, bbox_f64) else {
                    return Some(ResolvedPaint::Nothing);
                };
                let Some(to_gradient) = invert_matrix(m) else {
                    return Some(ResolvedPaint::Nothing);
                };
                return Some(ResolvedPaint::TwoPoint(TwoPointPaint {
                    to_gradient,
                    c: (g.cx, g.cy),
                    r: g.r,
                    f: (g.fx, g.fy),
                    fr: g.fr,
                    spread: g.spread,
                    stops: norm,
                }));
            }
            // A focal point outside the circle makes a cone, with nothing painted outside it,
            // as SVG 2 and the two-point conical gradient of the shader library define it.
            let (cx, cy) = (g.cx as f32, g.cy as f32);
            let (fx, fy) = (g.fx as f32, g.fy as f32);
            Some(shader_paint(RadialGradient::new(
                Point::from_xy(fx, fy),
                Point::from_xy(cx, cy),
                r,
                sk_stops(&norm),
                spread_mode(g.spread),
                to_local,
            )))
        }
        Def::ConicGradient(g) => {
            let b = b?;
            Some(ResolvedPaint::Conic(ConicPaint {
                cx: b.x + g.cx as f32 * b.w,
                cy: b.y + g.cy as f32 * b.h,
                angle_deg: g.angle as f32,
                stops: norm,
            }))
        }
        Def::Shader(_) | Def::Audio(_) => None,
    }
}

/// A gradient-space matrix as a `tiny-skia` transform.
fn sk_transform(m: Matrix2D) -> Transform {
    Transform::from_row(m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32)
}

fn spread_mode(s: SpreadMethod) -> SpreadMode {
    match s {
        SpreadMethod::Pad => SpreadMode::Pad,
        SpreadMethod::Reflect => SpreadMode::Reflect,
        SpreadMethod::Repeat => SpreadMode::Repeat,
    }
}

fn shader_paint(shader: Option<tiny_skia::Shader<'static>>) -> ResolvedPaint {
    match shader {
        Some(shader) => ResolvedPaint::Shader(SkPaint { shader, anti_alias: true, ..SkPaint::default() }),
        None => ResolvedPaint::Nothing,
    }
}

fn solid(c: [f32; 4]) -> SkPaint<'static> {
    let mut p = SkPaint::default();
    p.set_color(SkColor::from_rgba(c[0], c[1], c[2], c[3]).unwrap_or(SkColor::TRANSPARENT));
    p.anti_alias = true;
    p
}

fn sk_stops(stops: &[(f32, [f32; 4])]) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|(o, c)| GradientStop::new(*o, SkColor::from_rgba(c[0], c[1], c[2], c[3]).unwrap_or(SkColor::TRANSPARENT)))
        .collect()
}

/// Offsets clamped to `0..=1` and made non-decreasing; element opacity is
/// folded into each stop alpha.
fn normalize_stops(stops: &[Stop], opacity: f32) -> Vec<(f32, [f32; 4])> {
    let op = opacity.clamp(0.0, 1.0);
    let mut floor = 0.0f32;
    stops
        .iter()
        .map(|s| {
            let raw = s.offset as f32;
            let off = if raw.is_finite() { raw.clamp(0.0, 1.0) } else { 0.0 }.max(floor);
            floor = off;
            let c: Color = s.color;
            (off, [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0 * op])
        })
        .collect()
}

/// Fill `path` with a paint that colors each pixel itself.
pub(crate) fn pixel_fill(target: &mut Pixmap, path: &SkPath, conic: &dyn Recolor, rule: SkFillRule, transform: Transform, matrix: Matrix2D) {
    let Some(mut cov) = Pixmap::new(target.width(), target.height()) else { return };
    cov.fill_path(path, &white(), rule, transform, None);
    recolor_and_composite(target, cov, conic, matrix);
}

/// Stroke `path` with a paint that colors each pixel itself.
pub(crate) fn pixel_stroke(target: &mut Pixmap, path: &SkPath, conic: &dyn Recolor, stroke: &Stroke, transform: Transform, matrix: Matrix2D) {
    let Some(mut cov) = Pixmap::new(target.width(), target.height()) else { return };
    cov.stroke_path(path, &white(), stroke, transform, None);
    recolor_and_composite(target, cov, conic, matrix);
}

fn white() -> SkPaint<'static> {
    let mut p = SkPaint::default();
    p.set_color_rgba8(255, 255, 255, 255);
    p.anti_alias = true;
    p
}

/// `cov` holds opaque white where the shape covers (alpha = coverage). Map
/// each covered pixel centre back to local space, look up the paint color
/// and write it premultiplied, then draw the result onto `target`.
fn recolor_and_composite(target: &mut Pixmap, mut cov: Pixmap, conic: &dyn Recolor, matrix: Matrix2D) {
    let Some(inv) = invert_matrix(matrix) else { return };
    let width = cov.width() as usize;
    for (i, px) in cov.data_mut().chunks_exact_mut(4).enumerate() {
        let coverage = px[3];
        if coverage == 0 {
            continue;
        }
        let x = (i % width) as f32 + 0.5;
        let y = (i / width) as f32 + 0.5;
        let (lx, ly) = apply_matrix(inv, (x, y));
        let Some([r, g, b, a]) = conic.color_at_local(lx, ly) else {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        };
        let alpha = a * (coverage as f32 / 255.0);
        let q = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        px[0] = q(r * alpha);
        px[1] = q(g * alpha);
        px[2] = q(b * alpha);
        px[3] = q(alpha);
    }
    target.draw_pixmap(0, 0, cov.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conic(stops: Vec<(f32, [f32; 4])>) -> ConicPaint {
        ConicPaint { cx: 0.0, cy: 0.0, angle_deg: 0.0, stops }
    }

    fn red_blue() -> ConicPaint {
        conic(vec![(0.0, [1.0, 0.0, 0.0, 1.0]), (1.0, [0.0, 0.0, 1.0, 1.0])])
    }

    #[test]
    fn t_starts_at_top_and_runs_clockwise() {
        let c = red_blue();
        assert!(c.t_at(0.0, -10.0) < 1e-4, "top should be t=0");
        assert!((c.t_at(10.0, 0.0) - 0.25).abs() < 1e-4, "right should be a quarter turn");
        assert!((c.t_at(0.0, 10.0) - 0.5).abs() < 1e-4, "bottom should be half");
        assert!((c.t_at(-10.0, 0.0) - 0.75).abs() < 1e-4, "left should be three quarters");
    }

    #[test]
    fn start_angle_rotates_the_sweep() {
        let mut c = red_blue();
        c.angle_deg = 90.0;
        assert!(c.t_at(10.0, 0.0) < 1e-4, "with angle 90 the right edge becomes t=0");
    }

    #[test]
    fn color_lookup_interpolates_and_clamps() {
        let c = red_blue();
        let mid = c.color_at(0.5);
        assert!((mid[0] - 0.5).abs() < 1e-5 && (mid[2] - 0.5).abs() < 1e-5);
        assert_eq!(c.color_at(0.0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(c.color_at(1.0), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn stops_outside_the_ends_extend_first_and_last_color() {
        let c = conic(vec![(0.25, [1.0, 0.0, 0.0, 1.0]), (0.75, [0.0, 0.0, 1.0, 1.0])]);
        assert_eq!(c.color_at(0.1), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(c.color_at(0.9), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn normalize_makes_offsets_monotonic_and_folds_opacity() {
        let stops = vec![
            Stop::new(0.6, Color::rgb(255, 0, 0)),
            Stop::new(0.2, Color::rgb(0, 255, 0)),
            Stop::new(2.0, Color::rgba(0, 0, 255, 128)),
        ];
        let n = normalize_stops(&stops, 0.5);
        assert_eq!(n.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0.6, 0.6, 1.0]);
        assert!((n[0].1[3] - 0.5).abs() < 1e-5);
        assert!((n[2].1[3] - (128.0 / 255.0) * 0.5).abs() < 1e-5);
    }

    #[test]
    fn bbox_without_area_is_unusable() {
        assert!(!Bbox { x: 0.0, y: 0.0, w: 10.0, h: 0.0 }.usable());
        assert!(!Bbox { x: 0.0, y: 0.0, w: 0.0, h: 10.0 }.usable());
        assert!(Bbox { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }.usable());
    }
}
