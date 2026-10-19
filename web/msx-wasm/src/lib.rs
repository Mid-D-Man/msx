// web/msx-wasm/src/lib.rs
//! WASM bindings for the site playground (site/playground/). Every
//! function here is a thin wrapper over the real `msx` facade crate's
//! own `parse_scene`/`render`/`compile`/`decode` — the exact same code
//! path `apps/msx-cli` and every other consumer uses, not a
//! reimplementation for the browser. If a scene renders differently in
//! the playground than it would via the CLI, that's a real bug in one
//! of them, not an expected difference between two separate engines.
//!
//! ## Verification status
//! This crate depends on `msx`, which depends on `msx-parser`, which
//! depends on `dixscript` — the same crate that has blocked EVERY local
//! sandbox compilation attempt across this project's history (needs
//! cargo's `edition2024` feature, stabilized around rustc 1.85; the
//! sandbox floor here is 1.75, fixed via `apt-get install rustc cargo`
//! on Ubuntu 24, with no network path to a newer toolchain). This crate
//! could not be compiled — for `wasm32-unknown-unknown` or any other
//! target — anywhere in this delivery's own verification. Every
//! function signature and `wasm-bindgen` attribute usage below was
//! written by direct analogy to `apps/msx-cli/src/main.rs`'s already-
//! real, already-used calls into the same `msx` crate functions (see
//! each function's own comment for the exact mirrored call), not
//! guessed — but "same shape as already-proven-correct code" is
//! necessarily weaker than an actual compile, and this needs real CI
//! (with a `wasm32-unknown-unknown` target and `wasm-bindgen` installed)
//! before it can be trusted the way the rest of this project's
//! `..._if_a_gpu_adapter_is_available`-style tests are once CI has run
//! them. Flagging this plainly rather than presenting it as more solid
//! than it is.

use std::cell::RefCell;

use msx_ast::{LoopMode, Scene};
use wasm_bindgen::prelude::*;

thread_local! {
    /// The scene `render_and_inspect` parsed last. `render_frame` and
    /// `local_time` read it, so playback re-renders a frame without
    /// parsing the source again. WASM here is single threaded.
    static LOADED: RefCell<Option<Scene>> = const { RefCell::new(None) };
}

/// Parses `source` and renders it straight to SVG — the minimal single-
/// purpose export, for any caller that only wants the picture and
/// nothing else. Mirrors `msx::parse_scene` + `msx::render`, the exact
/// pair `apps/msx-cli`'s own `cmd_render` calls.
#[wasm_bindgen]
pub fn render_scene(source: &str) -> Result<String, JsValue> {
    let scene = msx::parse_scene(source).map_err(|e| JsValue::from_str(&e))?;
    Ok(msx::render(&scene))
}

/// Parses, renders, AND round-trips `source` through the real binary
/// codec (`msx::compile` -> `msx::decode`, MBFA-compressed — the same
/// `compress = true` `apps/msx-cli`'s own `cmd_compile` always passes),
/// returning one JSON string rather than a struct: this crate
/// deliberately carries no `serde`/`serde-wasm-bindgen` dependency, so
/// the JSON here is hand-built via `format!` and the caller (`site/
/// playground/build_playground.py`'s embedded JS) parses it with a
/// plain `JSON.parse`. `svg` is escaped for safe embedding as a JSON
/// string value (backslash, double-quote, and newline — the three
/// characters real SVG/DixScript output could plausibly contain that
/// would otherwise break the surrounding JSON).
///
/// Fields, matching exactly what the playground page reads:
/// - `svg`: the rendered SVG (from the freshly-parsed scene, not the
///   round-tripped one — what the user sees is always what they typed,
///   never silently substituted with the post-roundtrip version even
///   if the two differ)
/// - `source_bytes` / `binary_bytes`: `source.len()` and the compiled
///   binary's byte length — the same two numbers `apps/msx-cli`'s own
///   `roundtrip`/`compile` subcommands already report
/// - `element_count`: `scene.elements.len()` (top-level count only, not
///   recursive through `Group`/`Layer` children — matches how nothing
///   else in this project currently counts elements either)
/// - `roundtrip_ok`: whether re-rendering the DECODED scene produces
///   byte-identical SVG to the original render — the exact same check
///   `tests/roundtrip.rs`'s `check_roundtrip` performs, just surfaced
///   live instead of as a test assertion
/// - `animated`: whether the scene has at least one keyframe track
/// - `duration`: `scene.effective_duration()` in seconds, the timeline
///   length the playback bar scrubs across
/// - `loop_mode`: `once`, `loop` or `ping_pong`
///
/// The parsed scene is kept for `render_frame` and `local_time`.
#[wasm_bindgen]
pub fn render_and_inspect(source: &str) -> Result<String, JsValue> {
    let scene = msx::parse_scene(source).map_err(|e| JsValue::from_str(&e))?;
    let svg = msx::render(&scene);

    let binary = msx::compile(&scene, true).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let decoded = msx::decode(&binary).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let roundtrip_svg = msx::render(&decoded);
    let roundtrip_ok = svg == roundtrip_svg;

    let svg_escaped = json_escape(&svg);

    let animated = !scene.animations.is_empty();
    let duration = scene.effective_duration();
    let loop_mode = scene.loop_mode.to_dixscript_str();

    let json = format!(
        r#"{{"svg":"{svg}","source_bytes":{source_bytes},"binary_bytes":{binary_bytes},"element_count":{element_count},"roundtrip_ok":{roundtrip_ok},"animated":{animated},"duration":{duration},"loop_mode":"{loop_mode}"}}"#,
        svg = svg_escaped,
        source_bytes = source.len(),
        binary_bytes = binary.len(),
        element_count = scene.elements.len(),
        roundtrip_ok = roundtrip_ok,
        animated = animated,
        duration = duration,
        loop_mode = loop_mode,
    );

    LOADED.with(|cell| *cell.borrow_mut() = Some(scene));
    Ok(json)
}

/// Escapes `s` for use inside a JSON string literal: backslash, double
/// quote and every control character below U+0020.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Imports an SVG document and returns one JSON string for the converter
/// page: the `.msx` source the writer produced, an SVG render of that
/// source parsed back (what the file will really look like), the importer
/// warnings, and a few sizes.
///
/// Fields:
/// - `msx`: the scene as `.msx` source text, from `msx_parser::scene_to_msx`
/// - `svg`: the render of `msx` after parsing it again, so the preview shows
///   the written file and not the in-memory scene
/// - `warnings`: every importer warning, in order
/// - `svg_bytes` / `msx_bytes`: input size and output source size
/// - `element_count`: top level elements of the imported scene
/// - `reparse_matches`: whether rendering the in-memory scene gives the same
///   SVG as rendering the reparsed source (a writer or parser gap shows here)
///
/// Errors are plain strings: invalid XML, not an SVG, content the writer
/// cannot express yet, or a written file that does not parse.
#[wasm_bindgen]
pub fn import_svg(svg: &str) -> Result<String, JsValue> {
    let imported = msx_svg_import::import_svg(svg).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let source = msx_parser::scene_to_msx(&imported.scene).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let reparsed = msx::parse_scene(&source).map_err(|e| JsValue::from_str(&format!("the converted source does not parse: {e}")))?;
    let rendered = msx::render(&reparsed);
    let reparse_matches = rendered == msx::render(&imported.scene);

    let warnings = imported
        .warnings
        .iter()
        .map(|w| format!("\"{}\"", json_escape(w)))
        .collect::<Vec<_>>()
        .join(",");

    Ok(format!(
        r#"{{"msx":"{msx}","svg":"{svg}","warnings":[{warnings}],"svg_bytes":{svg_bytes},"msx_bytes":{msx_bytes},"element_count":{element_count},"reparse_matches":{reparse_matches}}}"#,
        msx = json_escape(&source),
        svg = json_escape(&rendered),
        warnings = warnings,
        svg_bytes = svg.len(),
        msx_bytes = source.len(),
        element_count = imported.scene.elements.len(),
        reparse_matches = reparse_matches,
    ))
}

/// Compiles `.msx` source to the compressed binary form, the same call
/// `msx compile` makes (`compress = true`). Returns the bytes for download.
#[wasm_bindgen]
pub fn compile_source(source: &str) -> Result<Vec<u8>, JsValue> {
    let scene = msx::parse_scene(source).map_err(|e| JsValue::from_str(&e))?;
    msx::compile(&scene, true).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Renders the loaded scene at `t` seconds of elapsed playback time and
/// returns the SVG. `t` is raw elapsed time, not a position inside the
/// timeline: `msx_anim::resolve_at_time` applies the scene's own loop
/// mode, so a `loop` scene wraps and a `ping_pong` scene reverses without
/// the caller doing any arithmetic. The same resolver drives `msx animate`
/// and the native viewer, so a frame here matches a frame there.
#[wasm_bindgen]
pub fn render_frame(t: f64) -> Result<String, JsValue> {
    LOADED.with(|cell| {
        let loaded = cell.borrow();
        let scene = loaded
            .as_ref()
            .ok_or_else(|| JsValue::from_str("render_frame called before a scene was loaded"))?;
        let frame = msx_anim::resolve_at_time(scene, t);
        Ok(msx::render(&frame))
    })
}

/// Maps raw elapsed time `t` onto the timeline the way `render_frame`
/// does, so the playback bar can show the position the frame was drawn
/// at. Returns 0 when no scene is loaded or the scene has no duration.
#[wasm_bindgen]
pub fn local_time(t: f64) -> f64 {
    LOADED.with(|cell| {
        let loaded = cell.borrow();
        match loaded.as_ref() {
            Some(scene) => {
                let mode: LoopMode = scene.loop_mode;
                mode.resolve_time(t, scene.effective_duration())
            }
            None => 0.0,
        }
    })
}

// A panic anywhere in `msx`'s own dependency chain would otherwise
// vanish into an opaque "unreachable executed" WASM trap with no
// message — this forwards it to the browser console as a real error
// instead, the standard `console_error_panic_hook` pattern. Installed
// once, lazily, on first call rather than requiring the JS side to
// remember a separate init step.
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

#[cfg(test)]
mod tests {
    use super::*;

    const BADGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80">
  <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#4a9eff"/><stop offset="1" stop-color="#f2545b"/></linearGradient></defs>
  <rect x="5" y="5" width="110" height="70" rx="8" fill="url(#g)"/>
  <circle cx="40" cy="40" r="15" fill="#ffffff" fill-opacity=".8"/>
</svg>"##;

    #[test]
    fn json_escape_covers_quotes_backslashes_and_control_characters() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("x\ny\r\tz"), "x\\ny\\r\\tz");
        assert_eq!(json_escape("\u{1}"), "\\u0001");
        assert_eq!(json_escape("plain text"), "plain text");
    }

    #[test]
    fn import_svg_returns_source_render_and_clean_warnings() {
        let json = import_svg(BADGE).expect("the badge imports");
        assert!(json.starts_with("{\"msx\":\""), "{json}");
        assert!(json.contains("\"warnings\":[]"), "{json}");
        assert!(json.contains("\"reparse_matches\":true"), "{json}");
        assert!(json.contains("\"element_count\":2"), "{json}");
        assert!(!json.contains('\n'), "a raw newline would break JSON.parse");
    }

    #[test]
    fn import_svg_reports_warnings_as_escaped_strings() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><rect id="r" width="40" height="40" clip-path="url(#c)"/></svg>"##;
        let json = import_svg(svg).expect("the clip-path file imports");
        assert!(json.contains("clip-path"), "{json}");
        assert!(json.contains("\\\"r\\\""), "the quotes in the warning label are escaped: {json}");
    }

    #[test]
    fn compile_source_accepts_what_import_svg_wrote() {
        let json = import_svg(BADGE).expect("the badge imports");
        let imported = msx_svg_import::import_svg(BADGE).expect("import");
        let source = msx_parser::scene_to_msx(&imported.scene).expect("write");
        assert!(json.contains(&json_escape(&source)));
        let bytes = compile_source(&source).expect("compiles");
        assert!(bytes.len() > 8);
    }
}
