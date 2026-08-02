// core/msx-ast/src/animation.rs
//! Keyframe/timeline animation data — pure data only, zero external deps.
//! See module-level doc for full design rationale.
//!
//! ## Key design decisions (summary)
//!
//! - Tracks live on `Scene.animations[]`, targeting elements by id string
//!   — avoids touching all 13 element structs just to add animation.
//! - Channels are TRS+opacity only — matches Lottie/AE/Rive convention.
//! - Easing describes arriving *into* a keyframe (destination keyframe's
//!   easing applies). First keyframe's easing is never consulted.
//! - Time outside [first, last] keyframe holds at that endpoint's value.
//! - duration=0.0 means "no timeline" — effective_duration() infers from
//!   tracks if author omits explicit duration.

use crate::primitives::fmt_f64;

// ── Easing ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear    => t,
            Easing::EaseIn    => t * t,
            Easing::EaseOut   => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 { 2.0 * t * t }
                else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 }
            }
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Easing::Linear    => 0,
            Easing::EaseIn    => 1,
            Easing::EaseOut   => 2,
            Easing::EaseInOut => 3,
        }
    }

    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Easing::EaseIn,
            2 => Easing::EaseOut,
            3 => Easing::EaseInOut,
            _ => Easing::Linear,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "ease_in"  | "ease-in"      => Easing::EaseIn,
            "ease_out" | "ease-out"     => Easing::EaseOut,
            "ease_in_out" | "ease-in-out" => Easing::EaseInOut,
            _                           => Easing::Linear,
        }
    }

    pub fn to_dixscript_str(self) -> &'static str {
        match self {
            Easing::Linear    => "linear",
            Easing::EaseIn    => "ease_in",
            Easing::EaseOut   => "ease_out",
            Easing::EaseInOut => "ease_in_out",
        }
    }
}

// ── AnimatedProperty ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimatedProperty {
    TranslateX,
    TranslateY,
    ScaleX,
    ScaleY,
    /// Degrees, around local-space origin (0,0).
    Rotate,
    Opacity,
    /// `Layer`-only today (see `Layer::z_index`'s own doc). Composes as a
    /// plain override, not additive/multiplicative like every other
    /// channel here — see `msx-anim`'s `AnimatedDelta` for why that
    /// makes `identity_value()` below a nominal placeholder for this one
    /// variant rather than a real neutral value ever folded through the
    /// usual per-channel arithmetic.
    ZIndex,
}

impl AnimatedProperty {
    pub fn to_byte(self) -> u8 {
        match self {
            AnimatedProperty::TranslateX => 0,
            AnimatedProperty::TranslateY => 1,
            AnimatedProperty::ScaleX     => 2,
            AnimatedProperty::ScaleY     => 3,
            AnimatedProperty::Rotate     => 4,
            AnimatedProperty::Opacity    => 5,
            AnimatedProperty::ZIndex     => 6,
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AnimatedProperty::TranslateX),
            1 => Some(AnimatedProperty::TranslateY),
            2 => Some(AnimatedProperty::ScaleX),
            3 => Some(AnimatedProperty::ScaleY),
            4 => Some(AnimatedProperty::Rotate),
            5 => Some(AnimatedProperty::Opacity),
            6 => Some(AnimatedProperty::ZIndex),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "translate_x" => Some(AnimatedProperty::TranslateX),
            "translate_y" => Some(AnimatedProperty::TranslateY),
            "scale_x"     => Some(AnimatedProperty::ScaleX),
            "scale_y"     => Some(AnimatedProperty::ScaleY),
            "rotate"      => Some(AnimatedProperty::Rotate),
            "opacity"     => Some(AnimatedProperty::Opacity),
            "z_index"     => Some(AnimatedProperty::ZIndex),
            _             => None,
        }
    }

    pub fn to_dixscript_str(self) -> &'static str {
        match self {
            AnimatedProperty::TranslateX => "translate_x",
            AnimatedProperty::TranslateY => "translate_y",
            AnimatedProperty::ScaleX     => "scale_x",
            AnimatedProperty::ScaleY     => "scale_y",
            AnimatedProperty::Rotate     => "rotate",
            AnimatedProperty::Opacity    => "opacity",
            AnimatedProperty::ZIndex     => "z_index",
        }
    }

    /// The value an unanimated channel implicitly has — 1.0 for scale/opacity
    /// (neutral state), 0.0 for everything else.
    ///
    /// `ZIndex` returns 0.0 here purely to keep this a total function —
    /// it's never actually consulted for `ZIndex` in practice.
    /// `AnimatedDelta` (msx-anim) special-cases `ZIndex` as an
    /// `Option<f64>` override rather than routing it through this
    /// identity-then-compose model every additive/multiplicative channel
    /// uses, because "no keyframe touched this" and "keyframed to
    /// exactly 0.0" are different, both-meaningful states for a sort key
    /// in a way they aren't for a translate/rotate/scale/opacity delta.
    pub fn identity_value(self) -> f64 {
        match self {
            AnimatedProperty::ScaleX | AnimatedProperty::ScaleY | AnimatedProperty::Opacity => 1.0,
            _ => 0.0,
        }
    }
}

// ── Keyframe ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    pub time:   f64,
    pub value:  f64,
    pub easing: Easing,
}

impl Keyframe {
    pub fn new(time: f64, value: f64, easing: Easing) -> Self {
        Keyframe { time, value, easing }
    }

    pub fn linear(time: f64, value: f64) -> Self {
        Keyframe::new(time, value, Easing::Linear)
    }
}

impl std::fmt::Display for Keyframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}s({})", fmt_f64(self.value), fmt_f64(self.time), self.easing.to_dixscript_str())
    }
}

// ── AnimationTrack ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationTrack {
    pub target_id: String,
    pub property:  AnimatedProperty,
    /// Not required to be time-sorted — evaluate() handles arbitrary order.
    pub keyframes: Vec<Keyframe>,
}

impl AnimationTrack {
    pub fn new(target_id: impl Into<String>, property: AnimatedProperty, keyframes: Vec<Keyframe>) -> Self {
        AnimationTrack { target_id: target_id.into(), property, keyframes }
    }

    /// Evaluates the track at time `t`. Returns None only for an empty
    /// track. Holds at the first/last keyframe's value outside their range.
    pub fn evaluate(&self, t: f64) -> Option<f64> {
        if self.keyframes.is_empty() {
            return None;
        }

        let mut prev: Option<&Keyframe> = None;
        let mut next: Option<&Keyframe> = None;
        for kf in &self.keyframes {
            if kf.time <= t && prev.map(|p| kf.time > p.time).unwrap_or(true) {
                prev = Some(kf);
            }
            if kf.time > t && next.map(|n| kf.time < n.time).unwrap_or(true) {
                next = Some(kf);
            }
        }

        Some(match (prev, next) {
            (Some(p), Some(n)) => {
                let span   = n.time - p.time;
                let local  = if span > 0.0 { (t - p.time) / span } else { 1.0 };
                let eased  = n.easing.apply(local);
                p.value + (n.value - p.value) * eased
            }
            (Some(p), None) => p.value,
            (None, Some(n)) => n.value,
            (None, None)    => unreachable!("keyframes is non-empty"),
        })
    }

    pub fn max_time(&self) -> f64 {
        self.keyframes.iter().map(|k| k.time).fold(0.0, f64::max)
    }
}

// ── LoopMode ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    #[default]
    Once,
    Loop,
    PingPong,
}

impl LoopMode {
    pub fn to_byte(self) -> u8 {
        match self { LoopMode::Once => 0, LoopMode::Loop => 1, LoopMode::PingPong => 2 }
    }

    pub fn from_byte(b: u8) -> Self {
        match b { 1 => LoopMode::Loop, 2 => LoopMode::PingPong, _ => LoopMode::Once }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "loop"     => LoopMode::Loop,
            "ping_pong" | "ping-pong" | "pingpong" => LoopMode::PingPong,
            _          => LoopMode::Once,
        }
    }

    pub fn to_dixscript_str(self) -> &'static str {
        match self { LoopMode::Once => "once", LoopMode::Loop => "loop", LoopMode::PingPong => "ping_pong" }
    }

    pub fn resolve_time(self, elapsed: f64, duration: f64) -> f64 {
        if duration <= 0.0 { return 0.0; }
        match self {
            LoopMode::Once     => elapsed.clamp(0.0, duration),
            LoopMode::Loop     => elapsed.rem_euclid(duration),
            LoopMode::PingPong => {
                let cycle = duration * 2.0;
                let phase = elapsed.rem_euclid(cycle);
                if phase <= duration { phase } else { cycle - phase }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_boundaries_are_exact_for_every_curve() {
        for e in [Easing::Linear, Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            assert!((e.apply(0.0) - 0.0).abs() < 1e-9, "{e:?} should start at 0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-9, "{e:?} should end at 1");
        }
    }

    #[test]
    fn easing_curves_are_monotonic_increasing() {
        for e in [Easing::Linear, Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            let mut prev = e.apply(0.0);
            for i in 1..=20 {
                let v = e.apply(i as f64 / 20.0);
                assert!(v >= prev - 1e-9, "{e:?} dipped at t={}", i as f64 / 20.0);
                prev = v;
            }
        }
    }

    #[test]
    fn ease_in_out_is_continuous_at_the_midpoint() {
        let a = Easing::EaseInOut.apply(0.499999);
        let b = Easing::EaseInOut.apply(0.500001);
        assert!((a - b).abs() < 1e-3);
    }

    #[test]
    fn easing_byte_roundtrip() {
        for e in [Easing::Linear, Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            assert_eq!(Easing::from_byte(e.to_byte()), e);
        }
    }

    #[test]
    fn animated_property_byte_roundtrip() {
        let all = [
            AnimatedProperty::TranslateX, AnimatedProperty::TranslateY,
            AnimatedProperty::ScaleX,     AnimatedProperty::ScaleY,
            AnimatedProperty::Rotate,     AnimatedProperty::Opacity,
            AnimatedProperty::ZIndex,
        ];
        for p in all {
            assert_eq!(AnimatedProperty::from_byte(p.to_byte()), Some(p));
        }
        assert_eq!(AnimatedProperty::from_byte(99), None);
    }

    #[test]
    fn scale_and_opacity_identity_is_one_everything_else_is_zero() {
        assert_eq!(AnimatedProperty::ScaleX.identity_value(),    1.0);
        assert_eq!(AnimatedProperty::ScaleY.identity_value(),    1.0);
        assert_eq!(AnimatedProperty::Opacity.identity_value(),   1.0);
        assert_eq!(AnimatedProperty::TranslateX.identity_value(), 0.0);
        assert_eq!(AnimatedProperty::TranslateY.identity_value(), 0.0);
        assert_eq!(AnimatedProperty::Rotate.identity_value(),    0.0);
        // Nominal only — see identity_value's own doc. ZIndex never
        // actually routes through this value at runtime.
        assert_eq!(AnimatedProperty::ZIndex.identity_value(),    0.0);
    }

    #[test]
    fn z_index_parse_and_dixscript_str_roundtrip() {
        assert_eq!(AnimatedProperty::parse("z_index"), Some(AnimatedProperty::ZIndex));
        assert_eq!(AnimatedProperty::ZIndex.to_dixscript_str(), "z_index");
    }

    #[test]
    fn evaluate_empty_track_returns_none() {
        let t = AnimationTrack::new("box", AnimatedProperty::Opacity, vec![]);
        assert_eq!(t.evaluate(0.5), None);
    }

    #[test]
    fn evaluate_single_keyframe_is_constant_everywhere() {
        let t = AnimationTrack::new("box", AnimatedProperty::Opacity, vec![Keyframe::linear(2.0, 0.5)]);
        assert_eq!(t.evaluate(-10.0), Some(0.5));
        assert_eq!(t.evaluate(2.0),   Some(0.5));
        assert_eq!(t.evaluate(100.0), Some(0.5));
    }

    #[test]
    fn evaluate_holds_before_first_and_after_last_keyframe() {
        let t = AnimationTrack::new("box", AnimatedProperty::TranslateX, vec![
            Keyframe::linear(1.0, 10.0),
            Keyframe::linear(3.0, 50.0),
        ]);
        assert_eq!(t.evaluate(0.0), Some(10.0));
        assert_eq!(t.evaluate(5.0), Some(50.0));
    }

    #[test]
    fn evaluate_linear_midpoint_is_the_average() {
        let t = AnimationTrack::new("box", AnimatedProperty::TranslateX, vec![
            Keyframe::linear(0.0, 0.0),
            Keyframe::linear(2.0, 100.0),
        ]);
        assert!((t.evaluate(1.0).unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn evaluate_exact_keyframe_time_returns_its_exact_value() {
        let t = AnimationTrack::new("box", AnimatedProperty::Rotate, vec![
            Keyframe::linear(0.0, 0.0),
            Keyframe::linear(1.0, 90.0),
            Keyframe::linear(2.0, 180.0),
        ]);
        assert_eq!(t.evaluate(1.0), Some(90.0));
    }

    #[test]
    fn evaluate_applies_the_destination_keyframes_easing() {
        let t = AnimationTrack::new("box", AnimatedProperty::Opacity, vec![
            Keyframe::linear(0.0, 0.0),
            Keyframe::new(1.0, 100.0, Easing::EaseIn),
        ]);
        let v = t.evaluate(0.5).unwrap();
        assert!(v < 40.0, "ease-in at midpoint should be well below 50, got {v}");
    }

    #[test]
    fn evaluate_ignores_out_of_order_keyframe_input() {
        let t = AnimationTrack::new("box", AnimatedProperty::TranslateY, vec![
            Keyframe::linear(2.0, 100.0),
            Keyframe::linear(0.0, 0.0),
        ]);
        assert!((t.evaluate(1.0).unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn max_time_finds_the_latest_keyframe() {
        let t = AnimationTrack::new("box", AnimatedProperty::Opacity, vec![
            Keyframe::linear(0.0, 0.0),
            Keyframe::linear(4.5, 1.0),
            Keyframe::linear(2.0, 0.5),
        ]);
        assert!((t.max_time() - 4.5).abs() < 1e-9);
    }

    #[test]
    fn loop_mode_byte_roundtrip() {
        for m in [LoopMode::Once, LoopMode::Loop, LoopMode::PingPong] {
            assert_eq!(LoopMode::from_byte(m.to_byte()), m);
        }
    }

    #[test]
    fn zero_duration_always_resolves_to_zero() {
        for m in [LoopMode::Once, LoopMode::Loop, LoopMode::PingPong] {
            assert_eq!(m.resolve_time(5.0, 0.0), 0.0);
        }
    }

    #[test]
    fn once_clamps_at_the_end_instead_of_wrapping() {
        assert_eq!(LoopMode::Once.resolve_time(3.0, 10.0),  3.0);
        assert_eq!(LoopMode::Once.resolve_time(15.0, 10.0), 10.0);
        assert_eq!(LoopMode::Once.resolve_time(-5.0, 10.0), 0.0);
    }

    #[test]
    fn loop_wraps_back_to_zero_each_cycle() {
        assert_eq!(LoopMode::Loop.resolve_time(3.0, 10.0),  3.0);
        assert_eq!(LoopMode::Loop.resolve_time(15.0, 10.0), 5.0);
    }

    #[test]
    fn ping_pong_alternates_forward_and_backward_each_cycle() {
        let m = LoopMode::PingPong;
        assert_eq!(m.resolve_time(0.0, 10.0),  0.0);
        assert_eq!(m.resolve_time(5.0, 10.0),  5.0);
        assert_eq!(m.resolve_time(10.0, 10.0), 10.0);
        assert_eq!(m.resolve_time(15.0, 10.0), 5.0);
        assert_eq!(m.resolve_time(20.0, 10.0), 0.0);
    }
        }
