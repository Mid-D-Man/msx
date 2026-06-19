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
    scene.defs     = gradient::parse_defs(data, "", "defs")?;
    scene.elements = element::parse_elements(data, "", "elements")?;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use msx_ast::{BlendMode, Color, Element, Paint, Transform};

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
}
