// src/lib.rs — workspace façade.
// All heavy logic lives in sub-crates. This re-export surface keeps backward
// compatibility with existing tests, benches, and CI scripts.

pub use msx_ast::*;
pub use msx_binary::{compile, decode, MsxHeader, HEADER_SIZE};
pub use msx_parser::{parse_scene, parse_scene_file, parse_scene_from_data};
pub use msx_render_svg::render;

/// Parse MSX source and render directly to SVG.
pub fn source_to_svg(source: &str) -> Result<String, String> {
    let scene = parse_scene(source)?;
    Ok(render(&scene))
}

/// Parse MSX file from disk and render to SVG.
pub fn file_to_svg(path: &str) -> Result<String, String> {
    let scene = parse_scene_file(path)?;
    Ok(render(&scene))
}

/// Parse MSX source and compile to binary.
pub fn source_to_binary(source: &str, compress: bool) -> Result<Vec<u8>, String> {
    let scene = parse_scene(source)?;
    compile(&scene, compress).map_err(|e| format!("compile error: {}", e))
}
