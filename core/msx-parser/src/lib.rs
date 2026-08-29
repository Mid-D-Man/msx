// core/msx-parser/src/lib.rs
//! MSX DixScript source parser.
//!
//! Turns a `.msx` DixScript source file into an `msx_ast::Scene` by reading
//! a fully-resolved `dixscript::Runtime::DixData` (QuickFuncs already
//! evaluated, `@IMPORTS` already inlined by the dixscript compiler pipeline
//! before this crate ever sees the data).
//!
//! Orphan-rule note: `DixDeserialize` is a foreign trait (dixscript) and
//! `Scene` / `Element` / `Style` / etc. are foreign types (msx-ast) — this
//! crate owns neither, so `impl DixDeserialize for X` is illegal here.
//! Every AST node gets a plain `parse_*(data, prefix) -> Result<T, String>`
//! function instead. Scalar/array leaf reads (`f64`, `String`, `bool`,
//! `Vec<f64>`, `Option<T>`, ...) still go through dixscript's own
//! `DixDeserialize` / `TryFrom<DixValue>` impls via the `dix_helpers::opt`
//! wrapper, so none of the ergonomics are lost — only the outer struct-level
//! dispatch is hand-written, which type-tagged enums like `Element` and
//! `SdfTree` need anyway.

pub mod animation;
pub mod canvas;
pub mod dix_helpers;
pub mod element;
pub mod gradient;
pub mod layer;
pub mod schema;
pub mod sdf;
pub mod splat;
pub mod style;
pub mod transform;

pub use animation::{parse_animations, parse_duration, parse_loop_mode};
pub use canvas::parse_canvas;
pub use element::{parse_element, parse_elements};
pub use gradient::{parse_defs, parse_stops};
pub use layer::parse_layer;
pub use sdf::parse_sdf_node;
pub use splat::parse_splat;
pub use style::parse_style;
pub use transform::parse_transform;

use dixscript::Runtime::{DixData, DixLoadOptions, DixLoader};
use msx_ast::Scene;

/// Parse MSX source text directly (no file on disk required).
pub fn parse_scene(source: &str) -> Result<Scene, String> {
    let loader = DixLoader::new();
    let data = loader.load_from_str(source, &DixLoadOptions::new())?;
    parse_scene_from_data(&data)
}

/// Parse an `.msx` file from disk.
pub fn parse_scene_file(path: &str) -> Result<Scene, String> {
    let loader = DixLoader::new();
    let data = loader.load_text(path, &DixLoadOptions::new())?;
    parse_scene_from_data(&data)
}

/// Build a `Scene` from an already-loaded `DixData` — e.g. from a custom
/// `DixLoadOptions`, an encrypted `.mdix.enc`, or an `MdixMerger` multi-file
/// load. This is the actual parsing entry point; `parse_scene` /
/// `parse_scene_file` are thin convenience wrappers around it.
pub fn parse_scene_from_data(data: &DixData) -> Result<Scene, String> {
    schema::validate(data)?;

    let canvas = canvas::parse_canvas(data)?;
    let mut scene = Scene::new(canvas);
    scene.defs       = gradient::parse_defs(data, "", "defs")?;
    scene.elements   = element::parse_elements(data, "", "elements")?;
    scene.animations = animation::parse_animations(data, "", "animations")?;
    scene.duration   = animation::parse_duration(data)?;
    scene.loop_mode  = animation::parse_loop_mode(data)?;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{AnimatedProperty, BlendMode, Color, Def, Easing, Element, LoopMode, Paint, ShaderUniformValue, Transform};

    #[test]
    fn basic_rect_and_circle() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 200, height = 100, background = #ffffff }
  elements::
    { type = "rect", x = 0, y = 0, width = 50, height = 50,
      style = { fill = #ff0000, stroke = "none", stroke_width = 0, opacity = 1.0 } }
    { type = "circle", cx = 100, cy = 50, r = 20,
      style = { fill = #00ff00, stroke = "none", stroke_width = 0, opacity = 0.5 } }
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert!((scene.canvas.width - 200.0).abs() < 1e-9);
        assert_eq!(scene.elements.len(), 2);
        match &scene.elements[0] {
            Element::Rect(r) => {
                assert_eq!(r.style.fill, Some(Paint::Color(Color::rgb(255, 0, 0))));
            }
            other => panic!("expected Rect, got {:?}", other),
        }
    }

    #[test]
    fn transform_string_form() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #000000 }
  elements::
    { type = "rect", x = 0, y = 0, width = 10, height = 10,
      transform = "translate(10,20) rotate(45)",
      style = { fill = #ffffff, stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.elements[0] {
            Element::Rect(r) => assert!(matches!(r.transform, Some(Transform::Multiple(_)))),
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn transform_object_form() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #000000 }
  elements::
    { type = "circle", cx = 0, cy = 0, r = 5,
      transform = { type = "translate", x = 30, y = 40 },
      style = { fill = #ffffff, stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.elements[0] {
            Element::Circle(c) => {
                assert_eq!(c.transform, Some(Transform::Translate { x: 30.0, y: 40.0 }));
            }
            _ => panic!("expected Circle"),
        }
    }

    #[test]
    fn linear_gradient_def() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #ffffff }
  defs::
    { type = "linear_gradient", id = "g1", x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
      stops = [ { offset = 0.0, color = #ff0000, opacity = 1.0 },
                { offset = 1.0, color = #0000ff, opacity = 1.0 } ] }
  elements::
    { type = "rect", x = 0, y = 0, width = 100, height = 100,
      style = { fill = "url(#g1)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert_eq!(scene.defs.len(), 1);
        match &scene.elements[0] {
            Element::Rect(r) => assert_eq!(r.style.fill, Some(Paint::Ref("url(#g1)".into()))),
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn sdf_splat_and_layer() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 200, height = 200, background = #000000 }
  elements::
    { type = "sdf", fill = #ff8800,
      tree = { type = "smooth_union", k = 0.3,
        children = [ { type = "circle", cx = 0, cy = 0, r = 40 },
                     { type = "circle", cx = 50, cy = 0, r = 40 } ] } }
    { type = "splat", x = 100, y = 100, sigma_x = 30, sigma_y = 30,
      color = #ffffff, opacity = 0.6 }
    { type = "layer", blend_mode = "multiply", opacity = 0.8, clip = true,
      effects = [ { type = "blur", radius = 6.0 } ],
      elements = [ { type = "rect", x = 0, y = 0, width = 20, height = 20,
        style = { fill = #112233, stroke = "none", stroke_width = 0, opacity = 1.0 } } ] }
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert_eq!(scene.elements.len(), 3);
        assert!(matches!(scene.elements[0], Element::Sdf(_)));
        assert!(matches!(scene.elements[1], Element::Splat(_)));
        if let Element::Layer(layer) = &scene.elements[2] {
            assert_eq!(layer.blend_mode, BlendMode::Multiply);
            assert_eq!(layer.effects.len(), 1);
            assert_eq!(layer.children.len(), 1);
        } else {
            panic!("expected Layer");
        }
    }

    #[test]
    fn animation_tracks_duration_and_loop_mode() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #000000 }
  duration = 2.0
  loop_mode = "ping_pong"
  elements::
    { type = "circle", id = "dot", cx = 0, cy = 0, r = 5,
      style = { fill = #ffffff, stroke = "none", stroke_width = 0, opacity = 1.0 } }
  animations::
    { target_id = "dot", property = "translate_x",
      keyframes = [ { time = 0.0, value = 0.0 },
                    { time = 2.0, value = 50.0, easing = "ease_in_out" } ] }
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert_eq!(scene.duration, 2.0);
        assert_eq!(scene.loop_mode, LoopMode::PingPong);
        assert_eq!(scene.animations.len(), 1);

        let track = &scene.animations[0];
        assert_eq!(track.target_id, "dot");
        assert_eq!(track.property, AnimatedProperty::TranslateX);
        assert_eq!(track.keyframes.len(), 2);
        assert_eq!(track.keyframes[1].easing, Easing::EaseInOut);
        assert!(scene.is_animated());
    }

    #[test]
    fn animations_default_to_empty_and_once() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #ffffff }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert!(scene.animations.is_empty());
        assert_eq!(scene.duration, 0.0);
        assert_eq!(scene.loop_mode, LoopMode::Once);
        assert!(!scene.is_animated());
    }

    #[test]
    fn animation_track_rejects_unknown_property() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #ffffff }
  elements::
  animations::
    { target_id = "dot", property = "skew_x",
      keyframes = [ { time = 0.0, value = 0.0 } ] }
)
"#;
        assert!(parse_scene(src).is_err());
    }

    #[test]
    fn shader_def_with_uniforms() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 400, height = 300, background = #000000 }
  defs::
    { type = "shader", id = "plasma_1", source_ref = "shaders/plasma.wgsl",
      entry_point = "main_fs", fallback_color = #6b46ff,
      uniforms = [ { name = "speed", type = "float", value = 1.5 },
                   { name = "resolution", type = "vec2", value = [800.0, 600.0] } ] }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse");
        assert_eq!(scene.defs.len(), 1);
        match &scene.defs[0] {
            Def::Shader(s) => {
                assert_eq!(s.id, "plasma_1");
                assert_eq!(s.source_ref, "shaders/plasma.wgsl");
                assert_eq!(s.entry_point, "main_fs");
                assert_eq!(s.fallback_color, Color::rgb(0x6b, 0x46, 0xff));
                assert_eq!(s.uniforms.len(), 2);
                assert_eq!(s.uniforms[0].value, ShaderUniformValue::Float(1.5));
                assert_eq!(s.uniforms[1].value, ShaderUniformValue::Vec2(800.0, 600.0));
            }
            other => panic!("expected Def::Shader, got {:?}", other),
        }
    }

    #[test]
    fn shader_def_defaults_entry_point_and_uniforms() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "shader", id = "s", source_ref = "a.wgsl", fallback_color = #ff0000 }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.defs[0] {
            Def::Shader(s) => {
                assert_eq!(s.entry_point, "fs_main");
                assert!(s.uniforms.is_empty());
            }
            other => panic!("expected Def::Shader, got {:?}", other),
        }
    }

    #[test]
    fn shader_def_requires_fallback_color() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "shader", id = "s", source_ref = "a.wgsl" }
  elements::
)
"#;
        assert!(parse_scene(src).is_err());
    }

    #[test]
    fn shader_uniform_rejects_wrong_arity() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "shader", id = "s", source_ref = "a.wgsl", fallback_color = #ff0000,
      uniforms = [ { name = "bad", type = "vec3", value = [1.0, 2.0] } ] }
  elements::
)
"#;
        assert!(parse_scene(src).is_err());
    }

    #[test]
    fn image_element_with_embedded_data_parses_and_decodes() {
        // base64 of an 8-byte PNG signature + 20 zero bytes — verified
        // independently (outside this crate, since dixscript makes this
        // crate itself unexecutable in the environment these tests were
        // authored in) to decode to exactly that.
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #ffffff }
  elements::
    { type = "image", x = 10, y = 10, width = 50, height = 40,
      data = "iVBORw0KGgoAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.elements[0] {
            Element::Image(img) => {
                match &img.source {
                    msx_ast::MediaSource::Embedded(bytes) => {
                        assert_eq!(&bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
                    }
                    other => panic!("expected MediaSource::Embedded, got {:?}", other),
                }
                assert_eq!((img.x, img.y, img.width, img.height), (10.0, 10.0, 50.0, 40.0));
                assert_eq!(img.anchor, msx_ast::Anchor::TopLeft, "anchor should default when omitted");
            }
            other => panic!("expected Element::Image, got {:?}", other),
        }
    }

    #[test]
    fn image_element_with_source_ref_does_not_touch_base64_at_all() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 100, height = 100, background = #ffffff }
  elements::
    { type = "image", x = 0, y = 0, width = 20, height = 20,
      anchor = "center", source_ref = "assets/logo.png" }
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.elements[0] {
            Element::Image(img) => {
                assert_eq!(img.source, msx_ast::MediaSource::FileRef("assets/logo.png".to_string()));
                assert_eq!(img.anchor, msx_ast::Anchor::Center);
            }
            other => panic!("expected Element::Image, got {:?}", other),
        }
    }

    #[test]
    fn image_element_requires_exactly_one_of_data_or_source_ref() {
        let neither = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  elements::
    { type = "image", x = 0, y = 0, width = 5, height = 5 }
)
"#;
        assert!(parse_scene(neither).is_err(), "neither data nor source_ref given should be a parse error");

        let both = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  elements::
    { type = "image", x = 0, y = 0, width = 5, height = 5,
      data = "iVBORw0KGgoAAAAAAAAAAAAAAAAAAAAAAAAAAA==", source_ref = "a.png" }
)
"#;
        assert!(parse_scene(both).is_err(), "both data AND source_ref given should be a parse error");
    }

    #[test]
    fn image_element_rejects_data_that_does_not_sniff_as_a_known_image_format() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  elements::
    { type = "image", x = 0, y = 0, width = 5, height = 5,
      data = "dGhpcyBpcyBkZWZpbml0ZWx5IG5vdCBhbiBpbWFnZSwganVzdCB0ZXh0" }
)
"#;
        assert!(parse_scene(src).is_err(), "base64 that decodes to plain text, not PNG/JPEG, should be a parse error");
    }

    #[test]
    fn audio_def_parses_via_the_same_media_source_machinery() {
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "audio", id = "chime", source_ref = "assets/chime.wav" }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.defs[0] {
            Def::Audio(a) => {
                assert_eq!(a.id, "chime");
                assert_eq!(a.source, msx_ast::MediaSource::FileRef("assets/chime.wav".to_string()));
            }
            other => panic!("expected Def::Audio, got {:?}", other),
        }
    }

    #[test]
    fn audio_def_with_embedded_data_decodes_base64() {
        // The one real coverage gap flagged when this project last asked
        // "how do we even test the audio exactly": everything else
        // (`AudioFormat::sniff`'s own unit tests, this file's own
        // `FileRef` test above, msx-binary's `audio_def_roundtrips`)
        // covers either format-sniffing in isolation or the `source_ref`
        // path — nothing exercised the embedded-base64 path at the
        // parser level specifically, even though it's the ONLY place in
        // the whole pipeline that ever decodes audio base64 at all (see
        // `dix_helpers::parse_media_source`'s own doc comment).
        //
        // base64 of a 52-byte synthetic RIFF/WAVE header — 4 zero bytes
        // for the (unchecked, for this test's purposes) chunk size, then
        // 40 zero bytes of body — the same fixture shape
        // `msx-binary::compiler`'s own `audio_def_roundtrips` test
        // already uses, so a WAV-sniffing failure here and a roundtrip
        // failure there would be recognizably the same underlying bytes.
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "audio", id = "chime",
      data = "UklGRgAAAABXQVZFAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse");
        match &scene.defs[0] {
            Def::Audio(a) => {
                assert_eq!(a.id, "chime");
                match &a.source {
                    msx_ast::MediaSource::Embedded(bytes) => {
                        assert_eq!(bytes.len(), 52);
                        assert_eq!(&bytes[0..4], b"RIFF");
                        assert_eq!(&bytes[8..12], b"WAVE");
                        assert_eq!(
                            msx_ast::AudioFormat::sniff(bytes),
                            Some(msx_ast::AudioFormat::Wav),
                            "decoded bytes should sniff as WAV, confirming this is real audio-shaped data, not just any bytes"
                        );
                    }
                    other => panic!("expected MediaSource::Embedded, got {:?}", other),
                }
            }
            other => panic!("expected Def::Audio, got {:?}", other),
        }
    }

    #[test]
    fn audio_def_with_embedded_data_that_does_not_sniff_as_audio_still_parses() {
        // Deliberate asymmetry with images (see
        // `image_element_rejects_data_that_does_not_sniff_as_a_known_image_format`
        // above): `gradient.rs`'s own "audio" arm does NOT call
        // `AudioFormat::sniff` at parse time at all — see its comment on
        // why (nothing plays audio yet, so there's no render-time
        // failure mode format validation would be protecting against).
        // Plain text that decodes fine as base64 but sniffs as `None`
        // must still parse successfully; this pins that asymmetry down
        // as an explicit, tested behaviour rather than something only
        // documented in a comment.
        let src = r#"
@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 10, height = 10, background = #000000 }
  defs::
    { type = "audio", id = "not_really_audio",
      data = "dGhpcyBpcyBkZWZpbml0ZWx5IG5vdCBhbiBhdWRpbyBmaWxlLCBqdXN0IHRleHQ=" }
  elements::
)
"#;
        let scene = parse_scene(src).expect("parse should succeed — audio def parsing does not format-sniff");
        match &scene.defs[0] {
            Def::Audio(a) => match &a.source {
                msx_ast::MediaSource::Embedded(bytes) => {
                    assert_eq!(msx_ast::AudioFormat::sniff(bytes), None);
                }
                other => panic!("expected MediaSource::Embedded, got {:?}", other),
            },
            other => panic!("expected Def::Audio, got {:?}", other),
        }
    }
}
