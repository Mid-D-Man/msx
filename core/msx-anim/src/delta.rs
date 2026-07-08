// core/msx-anim/src/delta.rs
//! `AnimatedDelta` — the six animatable channels (TranslateX/Y, ScaleX/Y,
//! Rotate, Opacity) evaluated at a single point in time and reduced to one
//! value per channel, ready to fold onto an element's existing static
//! state.

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
}

impl Default for AnimatedDelta {
    /// Every channel starts at its `AnimatedProperty::identity_value()` —
    /// a target with only some of its properties animated must leave the
    /// untouched ones inert (0 translate/rotate, 1 scale/opacity).
    fn default() -> Self {
        AnimatedDelta {
            translate_x: AnimatedProperty::TranslateX.identity_value(),
            translate_y: AnimatedProperty::TranslateY.identity_value(),
            scale_x:     AnimatedProperty::ScaleX.identity_value(),
            scale_y:     AnimatedProperty::ScaleY.identity_value(),
            rotate:      AnimatedProperty::Rotate.identity_value(),
            opacity:     AnimatedProperty::Opacity.identity_value(),
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
