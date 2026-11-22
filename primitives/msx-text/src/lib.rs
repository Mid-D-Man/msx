// primitives/msx-text/src/lib.rs
//! Text layout for the raster renderers. A `Text` element becomes absolute
//! path commands in user space (y down), so the CPU and GPU renderers draw
//! it through their normal path pipelines, gradients and shader fills
//! included.

use std::sync::OnceLock;

use msx_ast::{FontStyle, FontWeight, Path, PathCommand, Point, Style, Text, TextAnchor, TextDecoration};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

static REGULAR_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSans-subset.ttf");
static BOLD_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold-subset.ttf");

const DEFAULT_FONT_SIZE: f64 = 16.0;

/// Horizontal shift per unit of height for `italic` and `oblique`, the tangent
/// of a 12 degree slant.
const SLANT: f64 = 0.2126;

struct Faces {
    regular: Face<'static>,
    bold: Face<'static>,
}

fn faces() -> &'static Faces {
    static FACES: OnceLock<Faces> = OnceLock::new();
    FACES.get_or_init(|| Faces {
        regular: Face::parse(REGULAR_BYTES, 0).expect("embedded regular font parses"),
        bold: Face::parse(BOLD_BYTES, 0).expect("embedded bold font parses"),
    })
}

/// Result of laying out one `Text` element.
#[derive(Debug, Clone)]
pub struct TextLayout {
    /// Absolute `M`/`L`/`Q`/`C`/`Z` commands in user space, y down.
    pub commands: Vec<PathCommand>,
    /// Text box as `(left, top, width, height)` in user space. The width is
    /// the advance width and the height is ascender minus descender, which
    /// is the box SVG uses for gradient bounding boxes.
    pub bounds: (f64, f64, f64, f64),
}

/// Lay out a text element. Returns `None` for empty or whitespace-only text.
pub fn layout(text: &Text) -> Option<TextLayout> {
    if text.content.trim().is_empty() {
        return None;
    }

    let face = pick_face(&text.style);
    let size = text.style.font_size.unwrap_or(DEFAULT_FONT_SIZE);
    let scale = size / f64::from(face.units_per_em());

    let glyphs: Vec<GlyphId> = text.content.chars().map(|c| glyph_for(face, c)).collect();
    // Letter spacing follows every glyph, the last one included, as in SVG.
    let spacing = text.style.letter_spacing.unwrap_or(0.0) / scale;
    let advance_units = total_advance(face, &glyphs) + spacing * glyphs.len() as f64;
    let width = advance_units * scale;

    let ascender = f64::from(face.ascender());
    let descender = f64::from(face.descender());

    let x0 = text.x
        + match text.style.text_anchor.unwrap_or(TextAnchor::Start) {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => -width / 2.0,
            TextAnchor::End => -width,
        };
    let baseline_y = text.y + baseline_shift(face, text.style.dominant_baseline.as_deref(), scale);

    let slant = if text.style.font_style.is_some_and(FontStyle::is_slanted) { SLANT } else { 0.0 };
    let mut sink = Sink { commands: Vec::new(), ox: x0, oy: baseline_y, scale, slant, open: false };
    let mut pen = 0.0f64;
    for (i, &gid) in glyphs.iter().enumerate() {
        sink.ox = x0 + pen * scale;
        face.outline_glyph(gid, &mut sink);
        sink.finish_contour();
        pen += f64::from(face.glyph_hor_advance(gid).unwrap_or(0));
        if let Some(&next) = glyphs.get(i + 1) {
            pen += kerning(face, gid, next);
        }
        pen += spacing;
    }
    if let Some(d) = text.style.text_decoration {
        for (top_units, thickness_units) in decoration_bars(face, d) {
            let (y0, y1) = (baseline_y - top_units * scale, baseline_y - (top_units - thickness_units) * scale);
            sink.rect(x0, y0, x0 + width, y1);
        }
    }

    let top = baseline_y - ascender * scale;
    let height = (ascender - descender) * scale;

    Some(TextLayout { commands: sink.commands, bounds: (x0, top, width, height) })
}

/// Convert a text element into a path element carrying its style and
/// transform. Fill rule is forced to non-zero, which is what glyph outlines
/// are authored for.
pub fn to_path(text: &Text) -> Option<Path> {
    to_path_with_bounds(text).map(|(p, _)| p)
}

/// Like [`to_path`], also returning the text box `(left, top, width, height)`
/// used for gradient bounding boxes.
pub fn to_path_with_bounds(text: &Text) -> Option<(Path, (f64, f64, f64, f64))> {
    let lay = layout(text)?;
    if lay.commands.is_empty() {
        return None;
    }
    let mut style: Style = text.style.clone();
    style.fill_rule = Some(msx_ast::FillRule::NonZero);
    let d_raw = lay.commands.iter().map(PathCommand::to_svg_token).collect::<Vec<_>>().join(" ");
    let path = Path {
        commands: lay.commands,
        d_raw,
        id: text.id.clone(),
        transform: text.transform.clone(),
        style,
    };
    Some((path, lay.bounds))
}

fn pick_face(style: &Style) -> &'static Face<'static> {
    let f = faces();
    let bold = match &style.font_weight {
        Some(FontWeight::Bold) => true,
        Some(FontWeight::Numeric(n)) => *n >= 600,
        _ => false,
    };
    if bold { &f.bold } else { &f.regular }
}

fn glyph_for(face: &Face<'_>, c: char) -> GlyphId {
    // Control whitespace collapses to a space, like SVG's default handling.
    let c = if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c };
    face.glyph_index(c).unwrap_or(GlyphId(0))
}

fn total_advance(face: &Face<'_>, glyphs: &[GlyphId]) -> f64 {
    let mut sum = 0.0;
    for (i, &g) in glyphs.iter().enumerate() {
        sum += f64::from(face.glyph_hor_advance(g).unwrap_or(0));
        if let Some(&next) = glyphs.get(i + 1) {
            sum += kerning(face, g, next);
        }
    }
    sum
}

/// Kerning from the legacy `kern` table, in font units. Only plain
/// horizontal subtables are used; variable and state-machine subtables are
/// skipped.
fn kerning(face: &Face<'_>, left: GlyphId, right: GlyphId) -> f64 {
    let Some(kern) = face.tables().kern else { return 0.0 };
    let mut total = 0.0;
    for sub in kern.subtables {
        if sub.horizontal && !sub.variable && !sub.has_state_machine {
            if let Some(k) = sub.glyphs_kerning(left, right) {
                total += f64::from(k);
            }
        }
    }
    total
}

/// The decoration lines as `(top, thickness)` in font units, y up from the
/// baseline: underline, overline, then line-through. A face without the
/// metrics gets proportions of the em.
fn decoration_bars(face: &Face<'_>, d: TextDecoration) -> Vec<(f64, f64)> {
    let em = f64::from(face.units_per_em());
    let line = |m: Option<ttf_parser::LineMetrics>, fallback_top: f64| {
        m.map(|m| (f64::from(m.position), f64::from(m.thickness).max(1.0))).unwrap_or((fallback_top, em * 0.05))
    };
    let under = line(face.underline_metrics(), -em * 0.1);
    let mut bars = Vec::new();
    if d.underline {
        bars.push(under);
    }
    if d.overline {
        bars.push((f64::from(face.ascender()), under.1));
    }
    if d.line_through {
        bars.push(line(face.strikeout_metrics(), em * 0.3));
    }
    bars
}

/// Offset added to the text `y` so the requested baseline lands on it.
fn baseline_shift(face: &Face<'_>, baseline: Option<&str>, scale: f64) -> f64 {
    let asc = f64::from(face.ascender());
    let desc = f64::from(face.descender());
    match baseline.map(str::trim) {
        Some("middle") => {
            let xh = face.x_height().map(f64::from).unwrap_or(asc * 0.5);
            xh * 0.5 * scale
        }
        Some("central") => (asc + desc) / 2.0 * scale,
        Some("hanging") => asc * 0.8 * scale,
        Some("text-before-edge") | Some("text-top") => asc * scale,
        Some("text-after-edge") | Some("text-bottom") | Some("ideographic") => desc * scale,
        _ => 0.0,
    }
}

/// Collects glyph outlines as absolute path commands, flipping y.
struct Sink {
    commands: Vec<PathCommand>,
    ox: f64,
    oy: f64,
    scale: f64,
    /// Horizontal shift per unit of glyph height, zero for upright text.
    slant: f64,
    open: bool,
}

impl Sink {
    fn pt(&self, x: f32, y: f32) -> Point {
        Point::new(self.ox + (f64::from(x) + f64::from(y) * self.slant) * self.scale, self.oy - f64::from(y) * self.scale)
    }

    /// A closed box in user space, wound like the outer contours of a glyph so
    /// that it joins the glyphs under the non-zero rule.
    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.finish_contour();
        self.commands.push(PathCommand::MoveTo(Point::new(x0, y0)));
        self.commands.push(PathCommand::LineTo(Point::new(x1, y0)));
        self.commands.push(PathCommand::LineTo(Point::new(x1, y1)));
        self.commands.push(PathCommand::LineTo(Point::new(x0, y1)));
        self.commands.push(PathCommand::ClosePath);
    }

    fn finish_contour(&mut self) {
        if self.open {
            self.commands.push(PathCommand::ClosePath);
            self.open = false;
        }
    }
}

impl OutlineBuilder for Sink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        let p = self.pt(x, y);
        self.commands.push(PathCommand::MoveTo(p));
        self.open = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.commands.push(PathCommand::LineTo(p));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c = self.pt(x1, y1);
        let to = self.pt(x, y);
        self.commands.push(PathCommand::QuadBezier { c, to });
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.pt(x1, y1);
        let c2 = self.pt(x2, y2);
        let to = self.pt(x, y);
        self.commands.push(PathCommand::CubicBezier { c1, c2, to });
    }
    fn close(&mut self) {
        self.finish_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str, style: Style) -> Text {
        Text { x: 100.0, y: 50.0, content: content.to_string(), id: None, transform: None, style }
    }

    fn plain() -> Style {
        let mut s = Style::empty();
        s.font_size = Some(20.0);
        s
    }

    fn y_extent(cmds: &[PathCommand]) -> (f64, f64) {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        let mut visit = |p: &Point| {
            lo = lo.min(p.y);
            hi = hi.max(p.y);
        };
        for c in cmds {
            match c {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => visit(p),
                PathCommand::QuadBezier { c, to } => {
                    visit(c);
                    visit(to);
                }
                PathCommand::CubicBezier { c1, c2, to } => {
                    visit(c1);
                    visit(c2);
                    visit(to);
                }
                _ => {}
            }
        }
        (lo, hi)
    }

    #[test]
    fn empty_and_whitespace_return_none() {
        assert!(layout(&text("", plain())).is_none());
        assert!(layout(&text("   \n\t", plain())).is_none());
        assert!(to_path(&text("  ", plain())).is_none());
    }

    #[test]
    fn width_matches_advance_times_scale() {
        let l = layout(&text("A", plain())).unwrap();
        let f = &faces().regular;
        let gid = f.glyph_index('A').unwrap();
        let expect = f64::from(f.glyph_hor_advance(gid).unwrap()) * 20.0 / f64::from(f.units_per_em());
        assert!((l.bounds.2 - expect).abs() < 1e-9);
    }

    #[test]
    fn box_height_is_ascender_minus_descender() {
        let l = layout(&text("A", plain())).unwrap();
        let f = &faces().regular;
        let expect = f64::from(f.ascender() - f.descender()) * 20.0 / f64::from(f.units_per_em());
        assert!((l.bounds.3 - expect).abs() < 1e-9);
    }

    #[test]
    fn default_font_size_is_16() {
        let mut s = Style::empty();
        s.font_size = None;
        let a = layout(&text("Hi", s)).unwrap();
        let mut s16 = Style::empty();
        s16.font_size = Some(16.0);
        let b = layout(&text("Hi", s16)).unwrap();
        assert!((a.bounds.2 - b.bounds.2).abs() < 1e-9);
    }

    #[test]
    fn anchor_middle_and_end_shift_the_box() {
        let start = layout(&text("Hello", plain())).unwrap().bounds;
        let mut s = plain();
        s.text_anchor = Some(TextAnchor::Middle);
        let mid = layout(&text("Hello", s)).unwrap().bounds;
        let mut s = plain();
        s.text_anchor = Some(TextAnchor::End);
        let end = layout(&text("Hello", s)).unwrap().bounds;
        assert!((start.0 - 100.0).abs() < 1e-9);
        assert!((mid.0 - (100.0 - start.2 / 2.0)).abs() < 1e-9);
        assert!((end.0 - (100.0 - start.2)).abs() < 1e-9);
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let reg = layout(&text("MMM", plain())).unwrap().bounds.2;
        let mut s = plain();
        s.font_weight = Some(FontWeight::Bold);
        let bold = layout(&text("MMM", s)).unwrap().bounds.2;
        assert!(bold > reg);
    }

    #[test]
    fn numeric_weight_threshold_is_600() {
        let reg = layout(&text("MMM", plain())).unwrap().bounds.2;
        let mut s = plain();
        s.font_weight = Some(FontWeight::Numeric(500));
        assert!((layout(&text("MMM", s)).unwrap().bounds.2 - reg).abs() < 1e-9);
        let mut s = plain();
        s.font_weight = Some(FontWeight::Numeric(700));
        assert!(layout(&text("MMM", s)).unwrap().bounds.2 > reg);
    }

    #[test]
    fn alphabetic_baseline_sits_on_y() {
        let l = layout(&text("H", plain())).unwrap();
        let (top, bottom) = y_extent(&l.commands);
        assert!((bottom - 50.0).abs() < 0.01, "bottom {bottom}");
        assert!(top < 50.0 - 10.0, "cap height should rise above the baseline, top {top}");
    }

    #[test]
    fn hanging_baseline_moves_text_down() {
        let base = y_extent(&layout(&text("H", plain())).unwrap().commands).1;
        let mut s = plain();
        s.dominant_baseline = Some("hanging".into());
        let hang = y_extent(&layout(&text("H", s)).unwrap().commands).1;
        assert!(hang > base + 5.0);
    }

    #[test]
    fn central_baseline_centres_the_box_on_y() {
        let mut s = plain();
        s.dominant_baseline = Some("central".into());
        let b = layout(&text("H", s)).unwrap().bounds;
        let centre = b.1 + b.3 / 2.0;
        assert!((centre - 50.0).abs() < 0.01, "centre {centre}");
    }

    #[test]
    fn missing_glyph_draws_notdef() {
        let l = layout(&text("\u{4E2D}", plain())).unwrap();
        assert!(!l.commands.is_empty());
    }

    #[test]
    fn commands_are_absolute_only() {
        let l = layout(&text("Rust @ 2024", plain())).unwrap();
        assert!(l.commands.iter().all(|c| matches!(
            c,
            PathCommand::MoveTo(_) | PathCommand::LineTo(_) | PathCommand::QuadBezier { .. } | PathCommand::CubicBezier { .. } | PathCommand::ClosePath
        )));
        assert!(matches!(l.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(l.commands.last().unwrap(), PathCommand::ClosePath));
    }

    #[test]
    fn to_path_forces_nonzero_and_keeps_transform() {
        let mut s = plain();
        s.fill_rule = Some(msx_ast::FillRule::EvenOdd);
        let mut t = text("A", s);
        t.transform = Some(msx_ast::Transform::Translate { x: 5.0, y: 6.0 });
        t.id = Some("t1".into());
        let (p, _) = to_path_with_bounds(&t).unwrap();
        assert_eq!(p.style.fill_rule, Some(msx_ast::FillRule::NonZero));
        assert!(p.transform.is_some());
        assert_eq!(p.id.as_deref(), Some("t1"));
    }

    fn bars(style: Style) -> Vec<PathCommand> {
        let base = layout(&text("H", plain())).unwrap().commands.len();
        layout(&text("H", style)).unwrap().commands[base..].to_vec()
    }

    /// Shoelace sum of the first closed subpath made of straight lines.
    fn signed_area(cmds: &[PathCommand]) -> f64 {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for c in cmds {
            match c {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => pts.push((p.x, p.y)),
                PathCommand::ClosePath => break,
                _ => {}
            }
        }
        (0..pts.len()).map(|i| { let (a, b) = (pts[i], pts[(i + 1) % pts.len()]); a.0 * b.1 - b.0 * a.1 }).sum::<f64>() / 2.0
    }

    #[test]
    fn letter_spacing_follows_every_glyph() {
        let base = layout(&text("AB", plain())).unwrap().bounds.2;
        let mut s = plain();
        s.letter_spacing = Some(3.0);
        let spaced = layout(&text("AB", s)).unwrap().bounds.2;
        assert!((spaced - (base + 6.0)).abs() < 1e-9, "{spaced} vs {base}");
    }

    #[test]
    fn letter_spacing_moves_the_second_glyph() {
        let right = |ls: f64| {
            let mut s = plain();
            s.letter_spacing = Some(ls);
            let l = layout(&text("II", s)).unwrap();
            let mut hi = f64::MIN;
            for c in &l.commands {
                if let PathCommand::MoveTo(p) | PathCommand::LineTo(p) = c { hi = hi.max(p.x); }
            }
            hi
        };
        assert!((right(5.0) - right(0.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn underline_adds_one_box_below_the_baseline() {
        let mut s = plain();
        s.text_decoration = Some(TextDecoration::parse("underline"));
        let b = bars(s);
        assert_eq!(b.len(), 5);
        let (top, bottom) = y_extent(&b);
        assert!(top > 50.0 && bottom > top, "top {top} bottom {bottom}");
    }

    #[test]
    fn overline_and_line_through_sit_above_the_baseline() {
        let mut s = plain();
        s.text_decoration = Some(TextDecoration::parse("overline line-through"));
        let b = bars(s);
        assert_eq!(b.len(), 10);
        let (top, bottom) = y_extent(&b);
        assert!(bottom < 50.0, "bottom {bottom}");
        let plain_top = y_extent(&layout(&text("H", plain())).unwrap().commands).0;
        assert!(top < plain_top, "overline {top} should rise above the glyph {plain_top}");
    }

    #[test]
    fn decoration_spans_the_advance_width() {
        let mut s = plain();
        s.text_decoration = Some(TextDecoration::parse("underline"));
        let l = layout(&text("H", s)).unwrap();
        let b = &l.commands[l.commands.len() - 5..];
        let (PathCommand::MoveTo(a), PathCommand::LineTo(c)) = (&b[0], &b[1]) else { panic!("box starts with a line") };
        assert!((a.x - l.bounds.0).abs() < 1e-9 && ((c.x - a.x) - l.bounds.2).abs() < 1e-9);
    }

    #[test]
    fn decoration_box_is_wound_like_a_glyph_outline() {
        let outer = signed_area(&layout(&text("H", plain())).unwrap().commands);
        let mut s = plain();
        s.text_decoration = Some(TextDecoration::parse("underline"));
        let bar = signed_area(&bars(s));
        assert!(outer * bar > 0.0, "glyph {outer} bar {bar}: opposite winding would cut holes under non-zero");
    }

    #[test]
    fn no_decoration_words_add_nothing() {
        let mut s = plain();
        s.text_decoration = Some(TextDecoration::default());
        assert!(bars(s).is_empty());
    }

    #[test]
    fn italic_slants_the_top_to_the_right() {
        let right = |style: Option<FontStyle>| {
            let mut s = plain();
            s.font_style = style;
            let l = layout(&text("H", s)).unwrap();
            let mut hi = f64::MIN;
            for c in &l.commands {
                if let PathCommand::MoveTo(p) | PathCommand::LineTo(p) = c { hi = hi.max(p.x); }
            }
            hi
        };
        assert!(right(Some(FontStyle::Italic)) > right(None) + 2.0);
        assert!((right(Some(FontStyle::Oblique)) - right(Some(FontStyle::Italic))).abs() < 1e-9);
        assert!((right(Some(FontStyle::Normal)) - right(None)).abs() < 1e-9);
    }

    #[test]
    fn italic_keeps_the_baseline_and_the_box() {
        let up = layout(&text("H", plain())).unwrap();
        let mut s = plain();
        s.font_style = Some(FontStyle::Italic);
        let it = layout(&text("H", s)).unwrap();
        assert_eq!(it.bounds, up.bounds);
        assert!((y_extent(&it.commands).1 - 50.0).abs() < 0.01);
    }

    #[test]
    fn kerning_never_widens_a_known_pair() {
        let f = &faces().regular;
        let a = f.glyph_index('A').unwrap();
        let v = f.glyph_index('V').unwrap();
        let k = kerning(f, a, v);
        assert!(k <= 0.0, "AV kern should be zero or negative, got {k}");
    }
}
