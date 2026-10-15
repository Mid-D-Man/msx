// render/msx-render-gpu/src/gradient.rs
//! Gradient paint for the GPU renderer.
//!
//! A gradient shape is tessellated into the same vertex batch as flat
//! shapes. Each of its vertices carries `uv`, the vertex position in
//! `objectBoundingBox` units, and `grad`, an index into a storage buffer of
//! [`GradientDesc`] records. The fragment stage in `shaders/vector.wgsl`
//! turns `uv` into the gradient parameter and looks the color up in the
//! record. Flat shapes use [`NO_GRADIENT`] and keep their baked color, so
//! flat and gradient shapes share one draw in document order.
//!
//! `uv` is affine in position, so the rasterizer interpolates it exactly
//! across each triangle.
//!
//! Conic convention: angle 0 starts at the top and runs clockwise, like CSS
//! `conic-gradient`, measured in local space so the sweep is not distorted
//! by the bounding box aspect ratio. This matches the CPU renderer.

use bytemuck::{Pod, Zeroable};
use lyon::algorithms::aabb::bounding_box;
use lyon::path::Path as LyonPath;
use msx_ast::{Def, Stop};

/// Most stops a record holds. A gradient with more stops is resampled to
/// this many evenly spaced stops, which softens hard stops past the limit.
pub(crate) const MAX_STOPS: usize = 32;

/// `grad` value of a vertex that uses its own baked color.
pub(crate) const NO_GRADIENT: u32 = u32::MAX;

pub(crate) const KIND_SOLID: u32 = 0;
pub(crate) const KIND_LINEAR: u32 = 1;
pub(crate) const KIND_RADIAL: u32 = 2;
pub(crate) const KIND_CONIC: u32 = 3;

/// One gradient record, mirrored by `GradientDesc` in `vector.wgsl`.
///
/// Field use by `kind`:
/// - linear: `a = (x1, y1, x2, y2)`
/// - radial: `a = (cx, cy, fx, fy)`, `b.x = r`
/// - conic: `a = (cx, cy, angle_deg, 0)`, `b = (bbox width, bbox height, 0, 0)`
///
/// Colors are straight (not premultiplied) with the element opacity folded
/// into alpha. Offsets are packed four per `vec4`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GradientDesc {
    pub(crate) kind: u32,
    pub(crate) count: u32,
    pub(crate) pad: [u32; 2],
    pub(crate) a: [f32; 4],
    pub(crate) b: [f32; 4],
    pub(crate) colors: [[f32; 4]; MAX_STOPS],
    pub(crate) offsets: [[f32; 4]; MAX_STOPS / 4],
}

/// Bounding box of the geometry being painted, in local (pre-transform) space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bbox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bbox {
    pub fn from_path(path: &LyonPath) -> Option<Bbox> {
        path.iter().next()?;
        let b = bounding_box(path.iter());
        Some(Bbox { x: b.min.x, y: b.min.y, w: b.width(), h: b.height() })
    }

    /// `(left, top, width, height)` as produced by `msx_text`.
    pub fn from_text_box(b: (f64, f64, f64, f64)) -> Bbox {
        Bbox { x: b.0 as f32, y: b.1 as f32, w: b.2 as f32, h: b.3 as f32 }
    }

    /// A gradient in `objectBoundingBox` units needs a box with area.
    pub fn usable(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }

    /// Position in bounding-box units. Only meaningful when `usable()`.
    pub fn uv(&self, x: f32, y: f32) -> [f32; 2] {
        [(x - self.x) / self.w, (y - self.y) / self.h]
    }
}

/// Outcome of resolving a gradient reference.
pub(crate) enum Resolved {
    /// Boxed: the record is 688 bytes and `Nothing` carries none.
    Gradient(Box<GradientDesc>),
    /// A valid reference that paints nothing, for example an empty bounding box.
    Nothing,
}

/// Resolve a gradient definition. Returns `None` for defs that are not
/// gradients (shaders, audio), so the caller can use its own fallback.
pub(crate) fn resolve(def: &Def, bbox: Option<Bbox>, opacity: f32) -> Option<Resolved> {
    let stops = match def {
        Def::LinearGradient(g) => &g.stops,
        Def::RadialGradient(g) => &g.stops,
        Def::ConicGradient(g) => &g.stops,
        Def::Shader(_) | Def::Audio(_) => return None,
    };
    let norm = fit_stops(normalize_stops(stops, opacity));
    if norm.is_empty() {
        return Some(Resolved::Nothing);
    }
    let Some(b) = bbox.filter(Bbox::usable) else {
        return Some(Resolved::Nothing);
    };
    let last = norm[norm.len() - 1].1;

    let desc = match def {
        Def::LinearGradient(g) => {
            let (x1, y1, x2, y2) = (g.x1 as f32, g.y1 as f32, g.x2 as f32, g.y2 as f32);
            let (dx, dy) = (x2 - x1, y2 - y1);
            if dx * dx + dy * dy <= 1e-12 {
                // SVG paints the last stop color when both points coincide.
                solid(last)
            } else {
                build(KIND_LINEAR, [x1, y1, x2, y2], [0.0; 4], &norm)
            }
        }
        Def::RadialGradient(g) => {
            let r = g.r as f32;
            if r <= 0.0 {
                solid(last)
            } else {
                let (cx, cy) = (g.cx as f32, g.cy as f32);
                let (mut fx, mut fy) = (g.fx as f32, g.fy as f32);
                let (dx, dy) = (fx - cx, fy - cy);
                let dist = (dx * dx + dy * dy).sqrt();
                let max = 0.999 * r;
                if dist > max {
                    let k = max / dist;
                    fx = cx + dx * k;
                    fy = cy + dy * k;
                }
                build(KIND_RADIAL, [cx, cy, fx, fy], [r, 0.0, 0.0, 0.0], &norm)
            }
        }
        Def::ConicGradient(g) => build(KIND_CONIC, [g.cx as f32, g.cy as f32, g.angle as f32, 0.0], [b.w, b.h, 0.0, 0.0], &norm),
        Def::Shader(_) | Def::Audio(_) => return None,
    };
    Some(Resolved::Gradient(Box::new(desc)))
}

fn solid(color: [f32; 4]) -> GradientDesc {
    build(KIND_SOLID, [0.0; 4], [0.0; 4], &[(0.0, color)])
}

fn build(kind: u32, a: [f32; 4], b: [f32; 4], stops: &[(f32, [f32; 4])]) -> GradientDesc {
    let mut d = GradientDesc::zeroed();
    d.kind = kind;
    d.count = stops.len() as u32;
    d.a = a;
    d.b = b;
    let last = stops[stops.len() - 1];
    // Unused slots repeat the last stop so no lookup ever reads zeros.
    for i in 0..MAX_STOPS {
        let (o, c) = stops.get(i).copied().unwrap_or(last);
        d.colors[i] = c;
        d.offsets[i / 4][i % 4] = o;
    }
    d
}

/// Offsets clamped to `0..=1` and made non-decreasing; element opacity is
/// folded into each stop alpha.
pub(crate) fn normalize_stops(stops: &[Stop], opacity: f32) -> Vec<(f32, [f32; 4])> {
    let op = opacity.clamp(0.0, 1.0);
    let mut floor = 0.0f32;
    stops
        .iter()
        .map(|s| {
            let raw = s.offset as f32;
            let off = if raw.is_finite() { raw.clamp(0.0, 1.0) } else { 0.0 }.max(floor);
            floor = off;
            let c = s.color;
            (off, [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0 * op])
        })
        .collect()
}

/// Resample to `MAX_STOPS` evenly spaced stops when there are more.
fn fit_stops(stops: Vec<(f32, [f32; 4])>) -> Vec<(f32, [f32; 4])> {
    if stops.len() <= MAX_STOPS {
        return stops;
    }
    (0..MAX_STOPS)
        .map(|i| {
            let t = i as f32 / (MAX_STOPS - 1) as f32;
            (t, color_at(&stops, t))
        })
        .collect()
}

/// Reference color lookup, the Rust twin of `ramp` in `vector.wgsl`.
pub(crate) fn color_at(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = b.0 - a.0;
            let k = if span > 1e-6 { (t - a.0) / span } else { 1.0 };
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

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{Color, ConicGradient, LinearGradient, RadialGradient};

    fn red_blue() -> Vec<Stop> {
        vec![Stop::new(0.0, Color::rgb(255, 0, 0)), Stop::new(1.0, Color::rgb(0, 0, 255))]
    }

    fn unit() -> Option<Bbox> {
        Some(Bbox { x: 0.0, y: 0.0, w: 10.0, h: 10.0 })
    }

    #[test]
    fn desc_matches_the_wgsl_struct_size() {
        // 16 header + 16 a + 16 b + 32 * 16 colors + 8 * 16 offsets.
        assert_eq!(std::mem::size_of::<GradientDesc>(), 688);
        assert_eq!(std::mem::size_of::<GradientDesc>() % 16, 0);
        assert_eq!(std::mem::offset_of!(GradientDesc, colors), 48);
        assert_eq!(std::mem::offset_of!(GradientDesc, offsets), 560);
    }

    #[test]
    fn linear_record_carries_endpoints_and_stops() {
        let def = Def::LinearGradient(LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.5, red_blue()));
        let Some(Resolved::Gradient(d)) = resolve(&def, unit(), 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.kind, KIND_LINEAR);
        assert_eq!(d.count, 2);
        assert_eq!(d.a, [0.0, 0.0, 1.0, 0.5]);
        assert_eq!(d.colors[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(d.colors[1], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(d.offsets[0][1], 1.0);
    }

    #[test]
    fn coincident_linear_points_paint_the_last_stop() {
        let def = Def::LinearGradient(LinearGradient::new("g".into(), 0.3, 0.3, 0.3, 0.3, red_blue()));
        let Some(Resolved::Gradient(d)) = resolve(&def, unit(), 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.kind, KIND_SOLID);
        assert_eq!(d.colors[0], [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn radial_focal_point_is_clamped_inside_the_circle() {
        let def = Def::RadialGradient(RadialGradient::new("g".into(), 0.5, 0.5, 0.5, 5.0, 0.5, red_blue()));
        let Some(Resolved::Gradient(d)) = resolve(&def, unit(), 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.kind, KIND_RADIAL);
        let (dx, dy) = (d.a[2] - d.a[0], d.a[3] - d.a[1]);
        assert!((dx * dx + dy * dy).sqrt() <= 0.5 * 0.999 + 1e-6);
    }

    #[test]
    fn radial_with_zero_radius_paints_the_last_stop() {
        let def = Def::RadialGradient(RadialGradient::new("g".into(), 0.5, 0.5, 0.0, 0.5, 0.5, red_blue()));
        let Some(Resolved::Gradient(d)) = resolve(&def, unit(), 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.kind, KIND_SOLID);
        assert_eq!(d.colors[0], [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn conic_record_carries_center_angle_and_box_size() {
        let def = Def::ConicGradient(ConicGradient::new("g".into(), 0.25, 0.75, 30.0, red_blue()));
        let bbox = Some(Bbox { x: 0.0, y: 0.0, w: 40.0, h: 20.0 });
        let Some(Resolved::Gradient(d)) = resolve(&def, bbox, 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.kind, KIND_CONIC);
        assert_eq!(d.a, [0.25, 0.75, 30.0, 0.0]);
        assert_eq!(&d.b[..2], &[40.0, 20.0]);
    }

    #[test]
    fn empty_box_or_no_stops_paints_nothing() {
        let def = Def::LinearGradient(LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, red_blue()));
        let flat = Some(Bbox { x: 0.0, y: 0.0, w: 10.0, h: 0.0 });
        assert!(matches!(resolve(&def, flat, 1.0), Some(Resolved::Nothing)));
        assert!(matches!(resolve(&def, None, 1.0), Some(Resolved::Nothing)));
        let empty = Def::LinearGradient(LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, vec![]));
        assert!(matches!(resolve(&empty, unit(), 1.0), Some(Resolved::Nothing)));
    }

    #[test]
    fn non_gradient_defs_are_left_to_the_caller() {
        let def = Def::Shader(msx_ast::ShaderDef::new("s", "x.wgsl", Color::rgb(1, 2, 3)));
        assert!(resolve(&def, unit(), 1.0).is_none());
    }

    #[test]
    fn opacity_folds_into_alpha_and_offsets_stay_monotonic() {
        let stops = vec![Stop::new(0.6, Color::rgb(255, 0, 0)), Stop::new(0.2, Color::rgb(0, 255, 0)), Stop::new(2.0, Color::rgba(0, 0, 255, 128))];
        let n = normalize_stops(&stops, 0.5);
        assert_eq!(n.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0.6, 0.6, 1.0]);
        assert!((n[0].1[3] - 0.5).abs() < 1e-6);
        assert!((n[2].1[3] - (128.0 / 255.0) * 0.5).abs() < 1e-6);
    }

    #[test]
    fn more_than_max_stops_are_resampled() {
        let stops: Vec<Stop> = (0..50).map(|i| Stop::new(i as f64 / 49.0, Color::rgb((i * 5) as u8, 0, 0))).collect();
        let def = Def::LinearGradient(LinearGradient::new("g".into(), 0.0, 0.0, 1.0, 0.0, stops));
        let Some(Resolved::Gradient(d)) = resolve(&def, unit(), 1.0) else { panic!("expected a gradient") };
        assert_eq!(d.count as usize, MAX_STOPS);
        assert_eq!(d.offsets[0][0], 0.0);
        assert!((d.offsets[MAX_STOPS / 4 - 1][3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn color_at_interpolates_clamps_and_handles_hard_stops() {
        let s = vec![(0.0, [1.0, 0.0, 0.0, 1.0]), (0.5, [1.0, 0.0, 0.0, 1.0]), (0.5, [0.0, 0.0, 1.0, 1.0]), (1.0, [0.0, 0.0, 1.0, 1.0])];
        assert_eq!(color_at(&s, 0.25), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(color_at(&s, 0.75), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(color_at(&s, 0.0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(color_at(&s, 1.0), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn bbox_uv_maps_corners_to_the_unit_square() {
        let b = Bbox { x: 100.0, y: 50.0, w: 200.0, h: 40.0 };
        assert_eq!(b.uv(100.0, 50.0), [0.0, 0.0]);
        assert_eq!(b.uv(300.0, 90.0), [1.0, 1.0]);
    }
}
