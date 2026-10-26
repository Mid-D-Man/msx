// core/msx-svg-import/src/parse.rs
//! Lexical helpers for SVG attribute values: numbers, lengths, CSS colors,
//! transform lists and a small stylesheet engine.

use msx_ast::{Color, Transform};

// ── Numbers ──────────────────────────────────────────────────────────────────

/// Scan one number at the start of `s`. Returns the value and the number of
/// bytes used. Grammar: optional sign, digits with at most one decimal
/// point, optional exponent. A second `.` or a sign starts the next number.
pub fn scan_number(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.get(i), Some(b'-') | Some(b'+')) {
        i += 1;
    }
    let mut digits = 0;
    while matches!(b.get(i), Some(b'0'..=b'9')) {
        i += 1;
        digits += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while matches!(b.get(i), Some(b'0'..=b'9')) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e') | Some(b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'-') | Some(b'+')) {
            j += 1;
        }
        if matches!(b.get(j), Some(b'0'..=b'9')) {
            while matches!(b.get(j), Some(b'0'..=b'9')) {
                j += 1;
            }
            i = j;
        }
    }
    s[..i].parse::<f64>().ok().map(|v| (v, i))
}

/// A whole string that is exactly one number.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    let (v, n) = scan_number(t)?;
    (n == t.len()).then_some(v)
}

/// Numbers separated by whitespace and commas. Stops at the first token
/// that is not a number.
pub fn number_list(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        match scan_number(rest) {
            Some((v, n)) => {
                out.push(v);
                rest = &rest[n..];
            }
            None => break,
        }
    }
    out
}

// ── Lengths ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    User,
    Px,
    Pt,
    Pc,
    Mm,
    Cm,
    In,
    Em,
    Ex,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: Unit,
}

pub fn parse_length(s: &str) -> Option<Length> {
    let t = s.trim();
    let (value, n) = scan_number(t)?;
    let unit = match t[n..].trim() {
        "" => Unit::User,
        "px" => Unit::Px,
        "pt" => Unit::Pt,
        "pc" => Unit::Pc,
        "mm" => Unit::Mm,
        "cm" => Unit::Cm,
        "in" => Unit::In,
        "em" => Unit::Em,
        "ex" => Unit::Ex,
        "%" => Unit::Percent,
        _ => return None,
    };
    Some(Length { value, unit })
}

impl Length {
    /// Value in user units. `font_size` resolves `em` and `ex`,
    /// `percent_of` resolves `%`.
    pub fn to_px(self, font_size: f64, percent_of: f64) -> f64 {
        let v = self.value;
        match self.unit {
            Unit::User | Unit::Px => v,
            Unit::Pt => v * 96.0 / 72.0,
            Unit::Pc => v * 16.0,
            Unit::Mm => v * 96.0 / 25.4,
            Unit::Cm => v * 96.0 / 2.54,
            Unit::In => v * 96.0,
            Unit::Em => v * font_size,
            Unit::Ex => v * font_size * 0.5,
            Unit::Percent => v / 100.0 * percent_of,
        }
    }
}

// ── Colors ───────────────────────────────────────────────────────────────────

/// CSS color: hex (3, 4, 6, 8 digits), `rgb()`/`rgba()`, `hsl()`/`hsla()`,
/// named colors and `transparent`. `currentColor` is the caller's job.
pub fn parse_color(s: &str) -> Option<Color> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix('#') {
        return parse_hex(hex);
    }
    let lower = t.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(Color::rgba(0, 0, 0, 0));
    }
    for name in ["rgba", "rgb", "hsla", "hsl"] {
        if let Some(inner) = lower.strip_prefix(name).and_then(|r| r.trim_start().strip_prefix('(')).and_then(|r| r.trim_end().strip_suffix(')')) {
            return if name.starts_with("rgb") { parse_rgb_args(inner) } else { parse_hsl_args(inner) };
        }
    }
    named_color(&lower).map(|rgb| Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
}

fn parse_hex(hex: &str) -> Option<Color> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some(Color::rgb(d(0)?, d(1)?, d(2)?)),
        4 => Some(Color::rgba(d(0)?, d(1)?, d(2)?, d(3)?)),
        6 => Some(Color::rgb(p(0)?, p(2)?, p(4)?)),
        8 => Some(Color::rgba(p(0)?, p(2)?, p(4)?, p(6)?)),
        _ => None,
    }
}

fn split_args(inner: &str) -> Vec<&str> {
    inner.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|t| !t.is_empty()).collect()
}

/// One color channel: a number, or a percentage of 255.
fn channel(t: &str) -> Option<u8> {
    let v = match t.strip_suffix('%') {
        Some(p) => parse_number(p)? * 255.0 / 100.0,
        None => parse_number(t)?,
    };
    Some(v.round().clamp(0.0, 255.0) as u8)
}

/// Alpha: a number in 0..1, or a percentage.
fn alpha(t: &str) -> Option<u8> {
    let v = match t.strip_suffix('%') {
        Some(p) => parse_number(p)? / 100.0,
        None => parse_number(t)?,
    };
    Some((v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn parse_rgb_args(inner: &str) -> Option<Color> {
    let a = split_args(inner);
    if a.len() != 3 && a.len() != 4 {
        return None;
    }
    // The legacy syntax cannot mix numbers and percentages.
    let percents = a[..3].iter().filter(|t| t.ends_with('%')).count();
    if percents != 0 && percents != 3 {
        return None;
    }
    let al = if a.len() == 4 { alpha(a[3])? } else { 255 };
    Some(Color::rgba(channel(a[0])?, channel(a[1])?, channel(a[2])?, al))
}

fn parse_hsl_args(inner: &str) -> Option<Color> {
    let a = split_args(inner);
    if a.len() != 3 && a.len() != 4 {
        return None;
    }
    let h = parse_number(a[0].trim_end_matches("deg"))?;
    let s = parse_number(a[1].strip_suffix('%')?)? / 100.0;
    let l = parse_number(a[2].strip_suffix('%')?)? / 100.0;
    let al = if a.len() == 4 { alpha(a[3])? } else { 255 };
    let (r, g, b) = hsl_to_rgb(h.rem_euclid(360.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    Some(Color::rgba(r, g, b, al))
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(r), q(g), q(b))
}

fn named_color(name: &str) -> Option<u32> {
    NAMED.binary_search_by(|(n, _)| n.cmp(&name)).ok().map(|i| NAMED[i].1)
}

/// CSS named colors, sorted by name for binary search.
static NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

// ── Transforms ───────────────────────────────────────────────────────────────

/// Parse an SVG transform list. Returns `Ok(None)` for an empty or
/// identity list. Handles newlines, tabs, missing separators and every
/// function in the SVG grammar. An unknown function or a wrong argument
/// count is an error.
pub fn parse_transform(s: &str) -> Result<Option<Transform>, String> {
    let mut rest = s.trim();
    let mut list: Vec<Transform> = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let open = rest.find('(').ok_or_else(|| format!("expected '(' in transform \"{s}\""))?;
        let name = rest[..open].trim();
        let after = &rest[open + 1..];
        let close = after.find(')').ok_or_else(|| format!("missing ')' in transform \"{s}\""))?;
        let args = number_list(&after[..close]);
        rest = &after[close + 1..];
        let bad = || format!("bad arguments for {name}() in transform \"{s}\"");
        let t = match (name, args.len()) {
            ("translate", 1) => Transform::Translate { x: args[0], y: 0.0 },
            ("translate", 2) => Transform::Translate { x: args[0], y: args[1] },
            ("scale", 1) => Transform::Scale { x: args[0], y: args[0] },
            ("scale", 2) => Transform::Scale { x: args[0], y: args[1] },
            ("rotate", 1) => Transform::Rotate { angle: args[0], cx: None, cy: None },
            ("rotate", 3) => Transform::Rotate { angle: args[0], cx: Some(args[1]), cy: Some(args[2]) },
            ("skewX", 1) => Transform::SkewX(args[0]),
            ("skewY", 1) => Transform::SkewY(args[0]),
            ("matrix", 6) => Transform::Matrix(msx_ast::Matrix2D { a: args[0], b: args[1], c: args[2], d: args[3], e: args[4], f: args[5] }),
            ("translate" | "scale" | "rotate" | "skewX" | "skewY" | "matrix", _) => return Err(bad()),
            _ => return Err(format!("unknown transform function \"{name}\"")),
        };
        if !is_identity(&t) {
            list.push(t);
        }
    }
    Ok(match list.len() {
        0 => None,
        1 => list.pop(),
        _ => Some(Transform::Multiple(list)),
    })
}

/// A CSS angle in degrees. A bare number counts as degrees.
fn css_angle(tok: &str) -> Option<f64> {
    let t = tok.trim();
    let (v, n) = scan_number(t)?;
    match &t[n..] {
        "" | "deg" => Some(v),
        "rad" => Some(v.to_degrees()),
        "grad" => Some(v * 0.9),
        "turn" => Some(v * 360.0),
        _ => None,
    }
}

/// A CSS length in pixels. Only `px` and bare numbers, which is all a
/// transform needs here.
fn css_px(tok: &str) -> Option<f64> {
    let t = tok.trim();
    let (v, n) = scan_number(t)?;
    matches!(&t[n..], "" | "px").then_some(v)
}

/// Parse the value of the CSS `transform` property: functions separated by
/// whitespace, arguments separated by commas, with units on angles and
/// lengths. Returns `Ok(None)` for `none` or an identity.
pub fn parse_css_transform(s: &str) -> Result<Option<Transform>, String> {
    let mut rest = s.trim();
    if rest.is_empty() || rest == "none" {
        return Ok(None);
    }
    let mut list: Vec<Transform> = Vec::new();
    while !rest.is_empty() {
        let open = rest.find('(').ok_or_else(|| format!("expected '(' in transform \"{s}\""))?;
        let name = rest[..open].trim();
        let after = &rest[open + 1..];
        let close = after.find(')').ok_or_else(|| format!("missing ')' in transform \"{s}\""))?;
        let args: Vec<&str> = after[..close].split(',').map(str::trim).filter(|a| !a.is_empty()).collect();
        rest = after[close + 1..].trim_start();
        let bad = || format!("bad arguments for {name}() in transform \"{s}\"");
        let px = |i: usize| args.get(i).and_then(|a| css_px(a));
        let num = |i: usize| args.get(i).and_then(|a| parse_number(a));
        let ang = |i: usize| args.get(i).and_then(|a| css_angle(a));
        let t = match (name, args.len()) {
            ("translate", 1) => Transform::Translate { x: px(0).ok_or_else(bad)?, y: 0.0 },
            ("translate", 2) => Transform::Translate { x: px(0).ok_or_else(bad)?, y: px(1).ok_or_else(bad)? },
            ("translateX", 1) => Transform::Translate { x: px(0).ok_or_else(bad)?, y: 0.0 },
            ("translateY", 1) => Transform::Translate { x: 0.0, y: px(0).ok_or_else(bad)? },
            ("scale", 1) => Transform::Scale { x: num(0).ok_or_else(bad)?, y: num(0).ok_or_else(bad)? },
            ("scale", 2) => Transform::Scale { x: num(0).ok_or_else(bad)?, y: num(1).ok_or_else(bad)? },
            ("scaleX", 1) => Transform::Scale { x: num(0).ok_or_else(bad)?, y: 1.0 },
            ("scaleY", 1) => Transform::Scale { x: 1.0, y: num(0).ok_or_else(bad)? },
            ("rotate", 1) => Transform::Rotate { angle: ang(0).ok_or_else(bad)?, cx: None, cy: None },
            ("skewX", 1) => Transform::SkewX(ang(0).ok_or_else(bad)?),
            ("skewY", 1) => Transform::SkewY(ang(0).ok_or_else(bad)?),
            ("skew", 1) => Transform::SkewX(ang(0).ok_or_else(bad)?),
            ("skew", 2) => {
                list.push(Transform::SkewX(ang(0).ok_or_else(bad)?));
                Transform::SkewY(ang(1).ok_or_else(bad)?)
            }
            ("matrix", 6) => {
                let n: Vec<f64> = (0..6).filter_map(num).collect();
                if n.len() != 6 {
                    return Err(bad());
                }
                Transform::Matrix(msx_ast::Matrix2D { a: n[0], b: n[1], c: n[2], d: n[3], e: n[4], f: n[5] })
            }
            _ => return Err(format!("unsupported CSS transform function \"{name}\" in \"{s}\"")),
        };
        if !is_identity(&t) {
            list.push(t);
        }
    }
    Ok(match list.len() {
        0 => None,
        1 => list.pop(),
        _ => Some(Transform::Multiple(list)),
    })
}

/// `transform-origin` against a reference box of `w` by `h`. One to three
/// tokens, keywords or lengths or percentages. Returns `None` when invalid.
pub fn parse_origin(v: &str, w: f64, h: f64) -> Option<(f64, f64)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Axis {
        X,
        Y,
        Any,
    }
    let toks: Vec<&str> = v.split_whitespace().collect();
    if toks.is_empty() || toks.len() > 3 {
        return None;
    }
    enum Pos {
        Frac(f64),
        Len(Length),
    }
    let resolve = |t: &str| -> Option<(Axis, Pos)> {
        Some(match t {
            "left" => (Axis::X, Pos::Frac(0.0)),
            "right" => (Axis::X, Pos::Frac(1.0)),
            "top" => (Axis::Y, Pos::Frac(0.0)),
            "bottom" => (Axis::Y, Pos::Frac(1.0)),
            "center" => (Axis::Any, Pos::Frac(0.5)),
            other => (Axis::Any, Pos::Len(parse_length(other)?)),
        })
    };
    let value = |p: &Pos, size: f64| match p {
        Pos::Frac(f) => f * size,
        Pos::Len(l) => l.to_px(16.0, size),
    };
    let first = resolve(toks[0])?;
    let second = if toks.len() >= 2 { resolve(toks[1])? } else { (Axis::Any, Pos::Frac(0.5)) };
    let (a, b) = if first.0 == Axis::Y || second.0 == Axis::X { (second, first) } else { (first, second) };
    Some((value(&a.1, w), value(&b.1, h)))
}

fn is_identity(t: &Transform) -> bool {
    match t {
        Transform::Translate { x, y } => *x == 0.0 && *y == 0.0,
        Transform::Scale { x, y } => *x == 1.0 && *y == 1.0,
        Transform::Rotate { angle, .. } => *angle == 0.0,
        Transform::SkewX(a) | Transform::SkewY(a) => *a == 0.0,
        Transform::Matrix(m) => m.is_identity(),
        Transform::Multiple(v) => v.iter().all(is_identity),
        Transform::None => true,
    }
}

// ── CSS ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    Includes,
    Dash,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Debug, Clone, PartialEq)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
}

impl AttrSel {
    fn matches(&self, node: &roxmltree::Node) -> bool {
        let Some(have) = node.attribute(self.name.as_str()) else { return false };
        let want = self.value.as_str();
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => have == want,
            AttrOp::Includes => have.split_whitespace().any(|w| w == want),
            AttrOp::Dash => have == want || have.strip_prefix(want).is_some_and(|r| r.starts_with('-')),
            AttrOp::Prefix => !want.is_empty() && have.starts_with(want),
            AttrOp::Suffix => !want.is_empty() && have.ends_with(want),
            AttrOp::Substring => !want.is_empty() && have.contains(want),
        }
    }
}

/// A compound selector: optional type, optional id, classes, attribute
/// selectors and `:first-child`.
#[derive(Debug, Clone, PartialEq, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSel>,
    first_child: bool,
}

impl Compound {
    fn matches(&self, node: &roxmltree::Node) -> bool {
        node.is_element()
            && self.tag.as_deref().is_none_or(|t| t == node.tag_name().name())
            && self.id.as_deref().is_none_or(|want| node.attribute("id") == Some(want))
            && (self.classes.is_empty() || {
                let have: Vec<&str> = node.attribute("class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
                self.classes.iter().all(|c| have.contains(&c.as_str()))
            })
            && self.attrs.iter().all(|a| a.matches(node))
            && (!self.first_child || !node.prev_siblings().skip(1).any(|n| n.is_element()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

/// A complex selector: compounds joined by descendant or child combinators.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// Left to right. At least one.
    compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    combinators: Vec<Combinator>,
}

impl Selector {
    /// `(ids, classes plus attributes plus pseudo-classes, types)` summed over
    /// the compounds and compared lexicographically.
    pub fn specificity(&self) -> (u32, u32, u32) {
        self.compounds.iter().fold((0, 0, 0), |(a, b, c), k| {
            (a + k.id.is_some() as u32, b + (k.classes.len() + k.attrs.len()) as u32 + k.first_child as u32, c + k.tag.is_some() as u32)
        })
    }

    /// Whether the selector matches `node`, looking at its ancestors and
    /// previous siblings where the selector asks for them.
    pub fn matches_node(&self, node: &roxmltree::Node) -> bool {
        self.matches_from(self.compounds.len() - 1, node)
    }

    fn matches_from(&self, idx: usize, node: &roxmltree::Node) -> bool {
        if !self.compounds[idx].matches(node) {
            return false;
        }
        if idx == 0 {
            return true;
        }
        match self.combinators[idx - 1] {
            Combinator::Child => node.parent().is_some_and(|p| self.matches_from(idx - 1, &p)),
            Combinator::Descendant => node.ancestors().skip(1).any(|a| self.matches_from(idx - 1, &a)),
        }
    }

    /// The last compound only, from the parts a plain element name, id and
    /// class list carry. Ancestors and attributes are not looked at.
    #[cfg(test)]
    pub fn matches(&self, tag: &str, id: Option<&str>, classes: &[&str]) -> bool {
        let k = &self.compounds[self.compounds.len() - 1];
        k.tag.as_deref().is_none_or(|t| t == tag) && k.id.as_deref().is_none_or(|want| id == Some(want)) && k.classes.iter().all(|c| classes.contains(&c.as_str()))
    }
}

/// One stop of an `@keyframes` rule: an offset in `0..=1` and its declarations.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeStop {
    pub offset: f64,
    pub decls: Vec<Decl>,
}

#[derive(Debug, Default)]
pub struct Stylesheet {
    /// `(selector, declarations, source order)`
    pub rules: Vec<(Selector, Vec<Decl>, usize)>,
    /// Selectors this importer cannot match, for a warning.
    pub unsupported: Vec<String>,
    /// `@keyframes` rules by name, stops sorted by offset. A later rule of the
    /// same name replaces an earlier one.
    pub keyframes: std::collections::HashMap<String, Vec<KeyframeStop>>,
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Split on `;` outside parentheses and quotes.
fn split_decls(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0usize);
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ';') if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

/// Declarations from a `style` attribute or a rule body.
pub fn parse_declarations(body: &str) -> Vec<Decl> {
    let clean = strip_comments(body);
    split_decls(&clean)
        .into_iter()
        .filter_map(|d| {
            let (name, value) = d.split_once(':')?;
            let mut value = value.trim().to_string();
            let mut important = false;
            if let Some(v) = value.strip_suffix("!important") {
                value = v.trim().to_string();
                important = true;
            }
            let name = name.trim().to_ascii_lowercase();
            (!name.is_empty() && !value.is_empty()).then_some(Decl { name, value, important })
        })
        .collect()
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// One attribute selector, the text between the brackets.
fn parse_attr_selector(inner: &str) -> Option<AttrSel> {
    let inner = inner.trim();
    let Some(eq) = inner.find('=') else {
        return is_ident(inner).then(|| AttrSel { name: inner.to_string(), op: AttrOp::Exists, value: String::new() });
    };
    let (op, name_end) = match inner[..eq].chars().last() {
        Some('~') => (AttrOp::Includes, eq - 1),
        Some('|') => (AttrOp::Dash, eq - 1),
        Some('^') => (AttrOp::Prefix, eq - 1),
        Some('$') => (AttrOp::Suffix, eq - 1),
        Some('*') => (AttrOp::Substring, eq - 1),
        _ => (AttrOp::Equals, eq),
    };
    let name = inner[..name_end].trim();
    let raw = inner[eq + 1..].trim();
    let value = match raw.chars().next() {
        Some(q @ ('"' | '\'')) => raw.strip_prefix(q)?.strip_suffix(q)?,
        _ if raw.contains(char::is_whitespace) => return None,
        _ => raw,
    };
    (is_ident(name) && !value.contains(['"', '\''])).then(|| AttrSel { name: name.to_string(), op, value: value.to_string() })
}

fn parse_compound(text: &str) -> Option<Compound> {
    let mut k = Compound::default();
    let mut rest = text;
    let tag_end = rest.find(['.', '#', '[', ':']).unwrap_or(rest.len());
    let tag = &rest[..tag_end];
    if tag == "*" {
    } else if is_ident(tag) {
        k.tag = Some(tag.to_string());
    } else if !tag.is_empty() {
        return None;
    }
    rest = &rest[tag_end..];
    while !rest.is_empty() {
        let kind = rest.as_bytes()[0];
        if kind == b'[' {
            let close = rest.find(']')?;
            k.attrs.push(parse_attr_selector(&rest[1..close])?);
            rest = &rest[close + 1..];
            continue;
        }
        let end = rest[1..].find(['.', '#', '[', ':']).map(|i| i + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        match kind {
            b'.' if is_ident(name) => k.classes.push(name.to_string()),
            b'#' if is_ident(name) => k.id = Some(name.to_string()),
            b':' if name == "first-child" => k.first_child = true,
            _ => return None,
        }
        rest = &rest[end..];
    }
    Some(k)
}

/// A selector of compounds joined by whitespace (descendant) or `>` (child).
/// Sibling combinators, other pseudo-classes and pseudo-elements are not supported.
fn parse_selector(text: &str) -> Option<Selector> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut cur = String::new();
    let mut sep: Option<Combinator> = None;
    let mut in_bracket = false;
    let mut quote: Option<char> = None;
    let flush = |cur: &mut String, sep: &mut Option<Combinator>, compounds: &mut Vec<Compound>, combinators: &mut Vec<Combinator>| -> Option<()> {
        if cur.is_empty() {
            return Some(());
        }
        if !compounds.is_empty() {
            combinators.push(sep.take().unwrap_or(Combinator::Descendant));
        }
        compounds.push(parse_compound(cur)?);
        cur.clear();
        *sep = None;
        Some(())
    };
    for c in t.chars() {
        if in_bracket {
            cur.push(c);
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (None, '"') | (None, '\'') => quote = Some(c),
                (None, ']') => in_bracket = false,
                _ => {}
            }
            continue;
        }
        match c {
            '[' => {
                in_bracket = true;
                cur.push(c);
            }
            '>' => {
                if cur.is_empty() && sep == Some(Combinator::Child) {
                    return None;
                }
                flush(&mut cur, &mut sep, &mut compounds, &mut combinators)?;
                if compounds.is_empty() {
                    return None;
                }
                sep = Some(Combinator::Child);
            }
            '+' | '~' => return None,
            c if c.is_whitespace() => {
                flush(&mut cur, &mut sep, &mut compounds, &mut combinators)?;
                if !compounds.is_empty() && sep.is_none() {
                    sep = Some(Combinator::Descendant);
                }
            }
            c => cur.push(c),
        }
    }
    flush(&mut cur, &mut sep, &mut compounds, &mut combinators)?;
    // A dangling `>` has no compound after it.
    if compounds.is_empty() || sep == Some(Combinator::Child) {
        return None;
    }
    Some(Selector { compounds, combinators })
}

/// The stops of an `@keyframes` body: `from`, `to` and percentages, in lists.
fn parse_keyframe_stops(body: &str) -> Vec<KeyframeStop> {
    let mut stops = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start();
        let Some(open) = rest.find('{') else { break };
        let Some(close) = rest[open..].find('}') else { break };
        let decls = parse_declarations(&rest[open + 1..open + close]);
        for sel in rest[..open].split(',') {
            let offset = match sel.trim().to_ascii_lowercase().as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                other => other.strip_suffix('%').and_then(|p| p.trim().parse::<f64>().ok()).filter(|p| (0.0..=100.0).contains(p)).map(|p| p / 100.0),
            };
            if let Some(offset) = offset {
                stops.push(KeyframeStop { offset, decls: decls.clone() });
            }
        }
        rest = &rest[open + close + 1..];
    }
    // A stable sort keeps source order among equal offsets, so the later one wins.
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    stops
}

/// Add the rules of one `<style>` element's text to `sheet`.
pub fn parse_stylesheet(css: &str, sheet: &mut Stylesheet) {
    let css = strip_comments(css);
    let mut rest = css.as_str();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if rest.starts_with('@') {
            let keyword_end = rest.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(rest.len());
            let keyword = rest[..keyword_end].to_ascii_lowercase();
            if keyword == "@keyframes" || keyword == "@-webkit-keyframes" {
                if let Some(open) = rest.find('{') {
                    let mut depth = 0;
                    let mut end = None;
                    for (i, c) in rest[open..].char_indices() {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = Some(open + i);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let end = end.unwrap_or(rest.len());
                    let name = rest[keyword_end..open].trim().trim_matches(['"', '\'']).to_string();
                    if !name.is_empty() {
                        sheet.keyframes.insert(name, parse_keyframe_stops(&rest[open + 1..end]));
                    }
                    rest = rest.get(end + 1..).unwrap_or("");
                    continue;
                }
            }
            // Skip an at-rule: either up to `;` or a balanced `{...}` block.
            let semi = rest.find(';');
            let brace = rest.find('{');
            match (semi, brace) {
                (Some(s), Some(b)) if s < b => rest = &rest[s + 1..],
                (Some(s), None) => rest = &rest[s + 1..],
                (_, Some(b)) => {
                    let mut depth = 0;
                    let mut end = rest.len();
                    for (i, c) in rest[b..].char_indices() {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = b + i + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    rest = &rest[end..];
                }
                (None, None) => break,
            }
            continue;
        }
        let Some(open) = rest.find('{') else { break };
        let Some(close) = rest[open..].find('}') else { break };
        let selectors = &rest[..open];
        let decls = parse_declarations(&rest[open + 1..open + close]);
        for text in selectors.split(',') {
            match parse_selector(text) {
                Some(sel) => {
                    let order = sheet.rules.len();
                    sheet.rules.push((sel, decls.clone(), order));
                }
                None if !text.trim().is_empty() => sheet.unsupported.push(text.trim().to_string()),
                None => {}
            }
        }
        rest = &rest[open + close + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_are_collected_by_name_with_sorted_stops() {
        let mut sheet = Stylesheet::default();
        parse_stylesheet(
            "@keyframes spin { to { transform: rotate(360deg) } from { transform: rotate(0deg); opacity: 1 } }
             @-webkit-keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: .2 } }
             @media (min-width: 1px) { .x { fill: red } }
             .a { animation: spin 2s infinite }",
            &mut sheet,
        );
        let spin = &sheet.keyframes["spin"];
        assert_eq!(spin.iter().map(|s| s.offset).collect::<Vec<_>>(), vec![0.0, 1.0]);
        assert_eq!(spin[0].decls.len(), 2);
        let pulse = &sheet.keyframes["pulse"];
        assert_eq!(pulse.iter().map(|s| s.offset).collect::<Vec<_>>(), vec![0.0, 0.5, 1.0]);
        assert_eq!(pulse[1].decls[0].value, ".2");
        assert_eq!(sheet.rules.len(), 1, "the rule after the at-rules is still read");
        assert!(sheet.unsupported.is_empty());
    }

    #[test]
    fn a_later_keyframes_rule_replaces_an_earlier_one_and_bad_stops_are_dropped() {
        let mut sheet = Stylesheet::default();
        parse_stylesheet("@keyframes k { from { opacity: 0 } to { opacity: 1 } } @keyframes k { 50%, 150%, bogus { opacity: .5 } }", &mut sheet);
        let k = &sheet.keyframes["k"];
        assert_eq!(k.len(), 1);
        assert_eq!(k[0].offset, 0.5);
    }

    #[test]
    fn numbers_follow_the_svg_grammar() {
        assert_eq!(scan_number("1.5.5"), Some((1.5, 3)));
        assert_eq!(scan_number("-.5x"), Some((-0.5, 3)));
        assert_eq!(scan_number("1e2px"), Some((100.0, 3)));
        assert_eq!(scan_number("1em"), Some((1.0, 1)));
        assert_eq!(scan_number("+7"), Some((7.0, 2)));
        assert_eq!(scan_number("."), None);
        assert_eq!(scan_number("-"), None);
        assert_eq!(number_list("10,20 30-40 .5.5"), vec![10.0, 20.0, 30.0, -40.0, 0.5, 0.5]);
        assert_eq!(parse_number(" 3.25 "), Some(3.25));
        assert_eq!(parse_number("3.25x"), None);
    }

    #[test]
    fn lengths_resolve_units() {
        let px = |s: &str| parse_length(s).unwrap().to_px(20.0, 200.0);
        assert_eq!(px("10"), 10.0);
        assert_eq!(px("10px"), 10.0);
        assert!((px("72pt") - 96.0).abs() < 1e-9);
        assert_eq!(px("1pc"), 16.0);
        assert!((px("1in") - 96.0).abs() < 1e-9);
        assert!((px("25.4mm") - 96.0).abs() < 1e-9);
        assert!((px("2.54cm") - 96.0).abs() < 1e-9);
        assert_eq!(px("2em"), 40.0);
        assert_eq!(px("1ex"), 10.0);
        assert_eq!(px("50%"), 100.0);
        assert!(parse_length("5furlongs").is_none());
    }

    #[test]
    fn colors_cover_the_css_forms() {
        assert_eq!(parse_color("#f80"), Some(Color::rgb(255, 136, 0)));
        assert_eq!(parse_color("#f808"), Some(Color::rgba(255, 136, 0, 136)));
        assert_eq!(parse_color("#E94560"), Some(Color::rgb(233, 69, 96)));
        assert_eq!(parse_color("#11223344"), Some(Color::rgba(0x11, 0x22, 0x33, 0x44)));
        assert_eq!(parse_color("rgb(255, 0, 128)"), Some(Color::rgb(255, 0, 128)));
        assert_eq!(parse_color("rgb(100% 0% 50%)"), Some(Color::rgb(255, 0, 128)));
        assert_eq!(parse_color("rgba(0,0,0,.5)"), Some(Color::rgba(0, 0, 0, 128)));
        assert_eq!(parse_color("rgb(0 0 0 / 25%)"), Some(Color::rgba(0, 0, 0, 64)));
        assert_eq!(parse_color("hsl(120, 100%, 25%)"), Some(Color::rgb(0, 128, 0)));
        assert_eq!(parse_color("hsla(0, 100%, 50%, 0.5)"), Some(Color::rgba(255, 0, 0, 128)));
        assert_eq!(parse_color("RebeccaPurple"), Some(Color::rgb(0x66, 0x33, 0x99)));
        assert_eq!(parse_color("transparent"), Some(Color::rgba(0, 0, 0, 0)));
        assert_eq!(parse_color("aliceblue"), Some(Color::rgb(0xf0, 0xf8, 0xff)));
        assert_eq!(parse_color("rgba(0, 50%, 0, 0.5)"), None, "numbers and percentages cannot be mixed");
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("#ggg"), None);
    }

    #[test]
    fn named_colors_are_sorted_for_binary_search() {
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(NAMED.len(), 148);
    }

    #[test]
    fn transforms_parse_every_function() {
        use Transform::*;
        assert_eq!(parse_transform("translate(10,20)").unwrap(), Some(Translate { x: 10.0, y: 20.0 }));
        assert_eq!(parse_transform("translate(5)").unwrap(), Some(Translate { x: 5.0, y: 0.0 }));
        assert_eq!(parse_transform("scale(2)").unwrap(), Some(Scale { x: 2.0, y: 2.0 }));
        assert_eq!(parse_transform("rotate(45 10 20)").unwrap(), Some(Rotate { angle: 45.0, cx: Some(10.0), cy: Some(20.0) }));
        assert_eq!(parse_transform("skewX(10) ").unwrap(), Some(SkewX(10.0)));
        match parse_transform("matrix(1 0 0 1 5 6)").unwrap() {
            Some(Matrix(m)) => assert_eq!((m.e, m.f), (5.0, 6.0)),
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    #[test]
    fn transforms_survive_real_world_whitespace() {
        let t = parse_transform("matrix(1 0\n\t0 1\n 10-20)translate(1,2)rotate(90)").unwrap();
        match t {
            Some(Transform::Multiple(v)) => assert_eq!(v.len(), 3),
            other => panic!("expected a chain, got {other:?}"),
        }
        assert_eq!(parse_transform("translate(10-5)").unwrap(), Some(Transform::Translate { x: 10.0, y: -5.0 }));
    }

    #[test]
    fn identity_and_empty_transforms_vanish() {
        assert_eq!(parse_transform("").unwrap(), None);
        assert_eq!(parse_transform("  ").unwrap(), None);
        assert_eq!(parse_transform("translate(0 0) scale(1) rotate(0)").unwrap(), None);
        assert_eq!(parse_transform("matrix(1 0 0 1 0 0)").unwrap(), None);
    }

    #[test]
    fn css_transforms_take_units() {
        assert_eq!(parse_css_transform("scale(2)").unwrap(), Some(Transform::Scale { x: 2.0, y: 2.0 }));
        assert_eq!(parse_css_transform("rotate(90deg)").unwrap(), Some(Transform::Rotate { angle: 90.0, cx: None, cy: None }));
        assert_eq!(parse_css_transform("translate(10px, 20px)").unwrap(), Some(Transform::Translate { x: 10.0, y: 20.0 }));
        assert_eq!(parse_css_transform("translateY(5px)").unwrap(), Some(Transform::Translate { x: 0.0, y: 5.0 }));
        match parse_css_transform("rotate(0.5turn) scale(1, 2)").unwrap() {
            Some(Transform::Multiple(v)) => {
                assert_eq!(v[0], Transform::Rotate { angle: 180.0, cx: None, cy: None });
                assert_eq!(v[1], Transform::Scale { x: 1.0, y: 2.0 });
            }
            other => panic!("expected a chain, got {other:?}"),
        }
        assert_eq!(parse_css_transform("none").unwrap(), None);
        assert!(parse_css_transform("translate(1em)").is_err());
        assert!(parse_css_transform("perspective(3)").is_err());
    }

    #[test]
    fn transform_origin_keywords_lengths_and_percentages() {
        assert_eq!(parse_origin("center", 100.0, 50.0), Some((50.0, 25.0)));
        assert_eq!(parse_origin("left top", 100.0, 50.0), Some((0.0, 0.0)));
        assert_eq!(parse_origin("top left", 100.0, 50.0), Some((0.0, 0.0)), "keyword order is free");
        assert_eq!(parse_origin("right bottom", 100.0, 50.0), Some((100.0, 50.0)));
        assert_eq!(parse_origin("bottom", 100.0, 50.0), Some((50.0, 50.0)));
        assert_eq!(parse_origin("10px 20%", 100.0, 50.0), Some((10.0, 10.0)));
        assert_eq!(parse_origin("25%", 100.0, 50.0), Some((25.0, 25.0)));
        assert_eq!(parse_origin("", 1.0, 1.0), None);
        assert_eq!(parse_origin("nonsense", 1.0, 1.0), None);
    }

    #[test]
    fn bad_transforms_are_errors() {
        assert!(parse_transform("wobble(3)").is_err());
        assert!(parse_transform("translate(1 2 3)").is_err());
        assert!(parse_transform("matrix(1 2 3)").is_err());
        assert!(parse_transform("rotate(45 10)").is_err());
        assert!(parse_transform("translate(1").is_err());
    }

    #[test]
    fn declarations_handle_urls_quotes_and_important() {
        let d = parse_declarations("fill: red; stroke:url(data:image/png;base64,AAA) ; font-family: 'A;B'; opacity: .5 !important; /* c */ broken");
        assert_eq!(d.len(), 4);
        assert_eq!(d[0], Decl { name: "fill".into(), value: "red".into(), important: false });
        assert_eq!(d[1].value, "url(data:image/png;base64,AAA)");
        assert_eq!(d[2].value, "'A;B'");
        assert!(d[3].important && d[3].value == ".5");
    }

    #[test]
    fn stylesheet_selectors_and_at_rules() {
        let mut sheet = Stylesheet::default();
        parse_stylesheet(
            "@import url(x.css);\n.a, #b, rect.c.d { fill: red } @media print { .z { fill: blue } } * { stroke: none } g .nested { fill: green } path:hover { fill: pink }",
            &mut sheet,
        );
        assert_eq!(sheet.rules.len(), 5);
        assert_eq!(sheet.unsupported, vec!["path:hover".to_string()]);
        let (sel, ..) = &sheet.rules[2];
        assert!(sel.matches("rect", None, &["c", "d", "x"]));
        assert!(!sel.matches("rect", None, &["c"]));
        assert!(!sel.matches("circle", None, &["c", "d"]));
        assert_eq!(sel.specificity(), (0, 2, 1));
        let (id_sel, ..) = &sheet.rules[1];
        assert!(id_sel.matches("anything", Some("b"), &[]));
        assert_eq!(id_sel.specificity(), (1, 0, 0));
        let (star, ..) = &sheet.rules[3];
        assert!(star.matches("g", None, &[]));
        assert_eq!(star.specificity(), (0, 0, 0));
        let (nested, ..) = &sheet.rules[4];
        assert_eq!(nested.specificity(), (0, 1, 1));
    }

    /// The ids of the elements of `xml` that `selector` matches, in document order.
    fn matched(xml: &str, selector: &str) -> Vec<String> {
        let doc = roxmltree::Document::parse(xml).unwrap();
        let sel = parse_selector(selector).unwrap_or_else(|| panic!("{selector} should parse"));
        doc.descendants().filter(|n| n.is_element() && sel.matches_node(n)).filter_map(|n| n.attribute("id").map(String::from)).collect()
    }

    const TREE: &str = r#"<svg id="root" xmlns="http://www.w3.org/2000/svg">
        <g id="g1" class="a b"><g id="g2"><rect id="r1" x="1" lang="en-US"/><rect id="r2" class="k" data-n="foo bar"/></g><rect id="r3"/></g>
        <rect id="r4" x="2" data-n="foobar"/>
    </svg>"#;

    #[test]
    fn child_and_descendant_combinators() {
        assert_eq!(matched(TREE, "svg > rect"), vec!["r4"]);
        assert_eq!(matched(TREE, "svg rect"), vec!["r1", "r2", "r3", "r4"]);
        assert_eq!(matched(TREE, "g > rect"), vec!["r1", "r2", "r3"]);
        assert_eq!(matched(TREE, "g > g > rect"), vec!["r1", "r2"]);
        assert_eq!(matched(TREE, "g>g>rect.k"), vec!["r2"]);
        assert_eq!(matched(TREE, ".a rect"), vec!["r1", "r2", "r3"]);
        assert_eq!(matched(TREE, "#g1 * rect"), vec!["r1", "r2"]);
        // Two elements must sit between g#g1 and the rect, which only a deeper tree has.
        assert_eq!(matched(TREE, "g#g1 *  *  rect"), Vec::<String>::new());
        let deep = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="g1"><g id="g2"><g id="g3"><rect id="r"/></g></g></g></svg>"#;
        assert_eq!(matched(deep, "g#g1 * * rect"), vec!["r"]);
        assert_eq!(matched(deep, "g#g1 * g * * rect"), Vec::<String>::new());
        assert_eq!(matched(TREE, "svg g g rect"), vec!["r1", "r2"]);
    }

    #[test]
    fn descendant_matching_backtracks() {
        // The nearest `g` is g2, which has no `g` above it matching `.a > g` only through g1.
        assert_eq!(matched(TREE, ".a g rect"), vec!["r1", "r2"]);
        assert_eq!(matched(TREE, "g g > rect"), vec!["r1", "r2"]);
        assert_eq!(matched(TREE, "g > g rect"), vec!["r1", "r2"]);
    }

    #[test]
    fn attribute_selectors() {
        assert_eq!(matched(TREE, "[x]"), vec!["r1", "r4"]);
        assert_eq!(matched(TREE, "rect[x]"), vec!["r1", "r4"]);
        assert_eq!(matched(TREE, "[x=2]"), vec!["r4"]);
        assert_eq!(matched(TREE, "[x=\"2\"]"), vec!["r4"]);
        assert_eq!(matched(TREE, "[x='1']"), vec!["r1"]);
        assert_eq!(matched(TREE, "[data-n~=foo]"), vec!["r2"]);
        assert_eq!(matched(TREE, "[data-n^=foo]"), vec!["r2", "r4"]);
        assert_eq!(matched(TREE, "[data-n$=bar]"), vec!["r2", "r4"]);
        assert_eq!(matched(TREE, "[data-n*=oba]"), vec!["r4"]);
        assert_eq!(matched(TREE, "[lang|=en]"), vec!["r1"]);
        assert_eq!(matched(TREE, "[x][lang]"), vec!["r1"]);
        assert_eq!(matched(TREE, "g > rect[x]"), vec!["r1"]);
        assert_eq!(matched(TREE, "[data-n^=\"\"]"), Vec::<String>::new(), "an empty prefix matches nothing");
    }

    #[test]
    fn first_child() {
        assert_eq!(matched(TREE, "rect:first-child"), vec!["r1"]);
        assert_eq!(matched(TREE, "g:first-child"), vec!["g1", "g2"]);
        assert_eq!(matched(TREE, "g > :first-child"), vec!["g2", "r1"]);
    }

    #[test]
    fn specificity_sums_over_the_chain() {
        let spec = |s: &str| parse_selector(s).unwrap().specificity();
        assert_eq!(spec("g#g1 * g * * rect"), (1, 0, 3));
        assert_eq!(spec("g#g1 * * rect"), (1, 0, 2));
        assert_eq!(spec("svg > rect"), (0, 0, 2));
        assert_eq!(spec("[x]"), (0, 1, 0));
        assert_eq!(spec("rect.a[x]:first-child"), (0, 3, 1));
        assert!(spec("g#g1 * g * * rect") > spec("g#g1 * * rect"));
    }

    #[test]
    fn selectors_that_are_not_supported_are_refused_whole() {
        for bad in ["a + b", "a ~ b", "> a", "a >", "a > > b", "a:hover", "a::before", "a:not(.b)", "[x", "[x=]b", "[=y]", "a[x=y i]", "rect:nth-child(2)", ".", "#", "a..b"] {
            assert!(parse_selector(bad).is_none(), "{bad}");
        }
    }
}
