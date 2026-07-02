// core/msx-ast/src/canvas.rs

use crate::animation::{AnimationTrack, LoopMode};
use crate::color::Color;
use crate::element::Element;
use crate::gradient::Def;
use crate::primitives::ViewBox;

#[derive(Debug, Clone)]
pub struct Canvas {
    pub width:      f64,
    pub height:     f64,
    pub background: Color,
    pub viewbox:    Option<ViewBox>,
}

impl Canvas {
    pub fn new(width: f64, height: f64, background: Color) -> Self {
        Canvas { width, height, background, viewbox: None }
    }
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub canvas:     Canvas,
    pub defs:       Vec<Def>,
    pub elements:   Vec<Element>,
    /// Empty for a static scene — every existing Scene::new() call
    /// defaults this to vec![], so no existing call site breaks.
    pub animations: Vec<AnimationTrack>,
    /// Composition length in seconds. 0.0 = no animation timeline.
    pub duration:   f64,
    pub loop_mode:  LoopMode,
}

impl Scene {
    pub fn new(canvas: Canvas) -> Self {
        Scene {
            canvas,
            defs: Vec::new(),
            elements: Vec::new(),
            animations: Vec::new(),
            duration: 0.0,
            loop_mode: LoopMode::default(),
        }
    }

    pub fn element_count(&self) -> usize {
        count_recursive(&self.elements)
    }

    pub fn is_animated(&self) -> bool {
        !self.animations.is_empty() && self.effective_duration() > 0.0
    }

    /// Returns explicit duration if set, otherwise infers from latest
    /// keyframe across all tracks.
    pub fn effective_duration(&self) -> f64 {
        if self.duration > 0.0 {
            return self.duration;
        }
        self.animations.iter().map(|t| t.max_time()).fold(0.0, f64::max)
    }
}

fn count_recursive(elements: &[Element]) -> usize {
    elements.iter().map(|e| match e {
        Element::Group(g) => 1 + count_recursive(&g.children),
        Element::Layer(l) => 1 + count_recursive(&l.children),
        _ => 1,
    }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{AnimatedProperty, AnimationTrack, Keyframe};

    fn blank_scene() -> Scene {
        Scene::new(Canvas::new(100.0, 100.0, Color::WHITE))
    }

    #[test]
    fn new_scene_has_no_animation_by_default() {
        let scene = blank_scene();
        assert!(scene.animations.is_empty());
        assert_eq!(scene.duration, 0.0);
        assert_eq!(scene.loop_mode, LoopMode::Once);
        assert!(!scene.is_animated());
    }

    #[test]
    fn explicit_duration_with_no_tracks_is_not_animated() {
        let mut scene = blank_scene();
        scene.duration = 5.0;
        assert!(!scene.is_animated());
    }

    #[test]
    fn tracks_with_explicit_duration_are_animated() {
        let mut scene = blank_scene();
        scene.duration = 2.0;
        scene.animations.push(AnimationTrack::new(
            "box", AnimatedProperty::Opacity,
            vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(2.0, 1.0)],
        ));
        assert!(scene.is_animated());
        assert_eq!(scene.effective_duration(), 2.0);
    }

    #[test]
    fn tracks_without_explicit_duration_infer_it_from_keyframes() {
        let mut scene = blank_scene();
        scene.animations.push(AnimationTrack::new(
            "box", AnimatedProperty::TranslateX,
            vec![Keyframe::linear(0.0, 0.0), Keyframe::linear(3.5, 100.0)],
        ));
        assert!(scene.is_animated());
        assert!((scene.effective_duration() - 3.5).abs() < 1e-9);
    }
            }
