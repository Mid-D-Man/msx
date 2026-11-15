// core/msx-anim/src/delta.rs
//! `AnimatedDelta`: the animatable channels (TranslateX/Y, ScaleX/Y,
//! Rotate, Opacity, ZIndex, and the ten style channels for fill and stroke
//! color, stroke width and dash offset) evaluated at a single point in
//! time and reduced to one value per channel, ready to fold onto an
//! element's existing static state.

use msx_ast::{AnimatedProperty, AnimationTrack, Matrix2D, Transform};

const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatedDelta {
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale_x:     f64,
    pub scale_y:     f64,
    /// Degrees — matches `AnimatedProperty::Rotate`'s doc convention.
    pub rotate:      f64,
    pub opacity:     f64,
    /// `None` when no track targets this channel — leave the element's
    /// existing static `z_index` alone. `Some(v)` overrides it to `v`
    /// outright. Deliberately *not* folded into the plain-`f64`-with-
    /// identity-value model the other six channels share: z-index is a
    /// sort key, not something additive/multiplicative composition makes
    /// sense for, and "untouched" and "keyframed to 0.0" need to stay
    /// distinguishable — a plain `f64` defaulting to 0.0 can't tell those
    /// apart, `Option<f64>` can.
    pub z_index:     Option<f64>,
    /// Red, green, blue and alpha of the fill, 0 to 255, `None` where no
    /// track targets the channel. Style channels replace the value on the
    /// shape (see `apply_style` in the resolver), so like `z_index` they
    /// stay `Option` and an untouched channel keeps the shape's own value.
    pub fill:         [Option<f64>; 4],
    /// The same four channels for the stroke.
    pub stroke:       [Option<f64>; 4],
    pub stroke_width: Option<f64>,
    pub dash_offset:  Option<f64>,
}

impl Default for AnimatedDelta {
    /// Every channel starts at its `AnimatedProperty::identity_value()` —
    /// a target with only some of its properties animated must leave the
    /// untouched ones inert (0 translate/rotate, 1 scale/opacity).
    /// `z_index` starts at `None` (see its own doc) rather than going
    /// through `identity_value()` at all.
    fn default() -> Self {
        AnimatedDelta {
            translate_x: AnimatedProperty::TranslateX.identity_value(),
            translate_y: AnimatedProperty::TranslateY.identity_value(),
            scale_x:     AnimatedProperty::ScaleX.identity_value(),
            scale_y:     AnimatedProperty::ScaleY.identity_value(),
            rotate:      AnimatedProperty::Rotate.identity_value(),
            opacity:     AnimatedProperty::Opacity.identity_value(),
            z_index:     None,
            fill:         [None; 4],
            stroke:       [None; 4],
            stroke_width: None,
            dash_offset:  None,
        }
    }
}

impl AnimatedDelta {
    /// Evaluates every track targeting one element at time `t` and folds
    /// them into a single delta. Multiple tracks writing the same
    /// property (author error) resolve last-write-wins in track order.
    pub fn from_tracks(tracks: &[&AnimationTrack], t: f64) -> Self {
        let mut delta = AnimatedDelta::default();
        for track in tracks {
            if let Some(value) = track.evaluate(t) {
                delta.set(track.property, value);
            }
        }
        delta
    }

    fn set(&mut self, property: AnimatedProperty, value: f64) {
        match property {
            AnimatedProperty::TranslateX => self.translate_x = value,
            AnimatedProperty::TranslateY => self.translate_y = value,
            AnimatedProperty::ScaleX     => self.scale_x = value,
            AnimatedProperty::ScaleY     => self.scale_y = value,
            AnimatedProperty::Rotate     => self.rotate = value,
            AnimatedProperty::Opacity    => self.opacity = value,
            AnimatedProperty::ZIndex     => self.z_index = Some(value),
            AnimatedProperty::FillR       => self.fill[0] = Some(value),
            AnimatedProperty::FillG       => self.fill[1] = Some(value),
            AnimatedProperty::FillB       => self.fill[2] = Some(value),
            AnimatedProperty::FillA       => self.fill[3] = Some(value),
            AnimatedProperty::StrokeR     => self.stroke[0] = Some(value),
            AnimatedProperty::StrokeG     => self.stroke[1] = Some(value),
            AnimatedProperty::StrokeB     => self.stroke[2] = Some(value),
            AnimatedProperty::StrokeA     => self.stroke[3] = Some(value),
            AnimatedProperty::StrokeWidth => self.stroke_width = Some(value),
            AnimatedProperty::DashOffset  => self.dash_offset = Some(value),
        }
    }

    /// True when every channel is at its identity value — lets the
    /// resolver skip untouched elements without allocating anything.
    pub fn is_identity(&self) -> bool {
        self.translate_x.abs() < EPSILON
            && self.translate_y.abs() < EPSILON
            && (self.scale_x - 1.0).abs() < EPSILON
            && (self.scale_y - 1.0).abs() < EPSILON
            && self.rotate.abs() < EPSILON
            && (self.opacity - 1.0).abs() < EPSILON
            && self.z_index.is_none()
            && !self.has_style()
    }

    /// True when a track targets any of the ten style channels.
    pub fn has_style(&self) -> bool {
        self.fill.iter().chain(self.stroke.iter()).any(Option::is_some)
            || self.stroke_width.is_some()
            || self.dash_offset.is_some()
    }

    /// TRS matrix for this delta alone, standard scale→rotate→translate
    /// order (point transforms as `translate(rotate(scale(p)))`).
    pub fn to_matrix(self) -> Matrix2D {
        Matrix2D::translate(self.translate_x, self.translate_y)
            .concat(Matrix2D::rotate_deg(self.rotate))
            .concat(Matrix2D::scale(self.scale_x, self.scale_y))
    }

    /// Folds this delta onto an element's existing static transform. The
    /// delta wraps the static transform from the outside — same as
    /// nesting the element one group deeper and animating that group —
    /// so a static offset and an animated offset simply add, and a
    /// rotate/scale delta pivots the *whole already-positioned element*
    /// around the parent origin rather than the element's own local one.
    pub fn compose_transform(self, existing: Option<&Transform>) -> Transform {
        let static_matrix = existing.map(|t| t.to_matrix()).unwrap_or_else(Matrix2D::identity);
        Transform::Matrix(self.to_matrix().concat(static_matrix))
    }
}
