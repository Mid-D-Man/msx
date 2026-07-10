use msx_ast::{Canvas, Color, Def, Scene, ShaderDef, ShaderUniform, ShaderUniformValue};

#[test]
fn shader_def_survives_binary_roundtrip_uncompressed() {
    let mut scene = Scene::new(Canvas::new(400.0, 300.0, Color::BLACK));
    let shader = ShaderDef::new("plasma_1", "shaders/plasma.wgsl", Color::rgb(107, 70, 255))
        .with_entry_point("main_fs")
        .with_uniforms(vec![
            ShaderUniform::new("speed", ShaderUniformValue::Float(1.5)),
            ShaderUniform::new("resolution", ShaderUniformValue::Vec2(800.0, 600.5)),
            ShaderUniform::new("tint", ShaderUniformValue::Vec3(1.0, 0.5, 0.25)),
            ShaderUniform::new("rect", ShaderUniformValue::Vec4(0.0, 1.0, 2.0, 3.0)),
        ]);
    scene.defs.push(Def::Shader(shader));

    let bytes = msx_binary::compile(&scene, false).expect("compile");
    let decoded = msx_binary::decode(&bytes).expect("decode");

    assert_eq!(decoded.defs.len(), 1);
    match &decoded.defs[0] {
        Def::Shader(s) => {
            assert_eq!(s.id, "plasma_1");
            assert_eq!(s.source_ref, "shaders/plasma.wgsl");
            assert_eq!(s.entry_point, "main_fs");
            assert_eq!(s.fallback_color, Color::rgb(107, 70, 255));
            assert_eq!(s.uniforms.len(), 4);
            assert_eq!(s.uniforms[0].name, "speed");
            assert_eq!(s.uniforms[0].value, ShaderUniformValue::Float(1.5));
            assert_eq!(s.uniforms[1].value, ShaderUniformValue::Vec2(800.0, 600.5));
            assert_eq!(s.uniforms[2].value, ShaderUniformValue::Vec3(1.0, 0.5, 0.25));
            assert_eq!(s.uniforms[3].value, ShaderUniformValue::Vec4(0.0, 1.0, 2.0, 3.0));
        }
        other => panic!("expected Def::Shader, got {:?}", other),
    }
}

#[test]
fn shader_def_with_no_uniforms_roundtrips() {
    let mut scene = Scene::new(Canvas::new(10.0, 10.0, Color::WHITE));
    scene.defs.push(Def::Shader(ShaderDef::new("s", "a.wgsl", Color::BLACK)));
    let bytes = msx_binary::compile(&scene, false).expect("compile");
    let decoded = msx_binary::decode(&bytes).expect("decode");
    match &decoded.defs[0] {
        Def::Shader(s) => {
            assert!(s.uniforms.is_empty());
            assert_eq!(s.entry_point, "fs_main"); // ShaderDef::new default
        }
        other => panic!("expected Def::Shader, got {:?}", other),
    }
}

#[test]
fn shader_def_survives_alongside_gradients_and_elements() {
    use msx_ast::{Element, LinearGradient, Rect, Stop, Style};
    let mut scene = Scene::new(Canvas::new(100.0, 100.0, Color::BLACK));
    scene.defs.push(Def::LinearGradient(LinearGradient::new(
        "g1".into(), 0.0, 0.0, 1.0, 1.0,
        vec![Stop::new(0.0, Color::WHITE), Stop::new(1.0, Color::BLACK)],
    )));
    scene.defs.push(Def::Shader(ShaderDef::new("sh1", "b.wgsl", Color::rgb(1, 2, 3))));
    scene.elements.push(Element::Rect(Rect::new(0.0, 0.0, 10.0, 10.0, Style::default())));

    let bytes = msx_binary::compile(&scene, false).expect("compile");
    let decoded = msx_binary::decode(&bytes).expect("decode");
    assert_eq!(decoded.defs.len(), 2);
    assert_eq!(decoded.elements.len(), 1);
    assert!(matches!(decoded.defs[0], Def::LinearGradient(_)));
    assert!(matches!(decoded.defs[1], Def::Shader(_)));
}
