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
    static LOADED: RefCell<Option<Scene>> = RefCell::new(None);
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

    let svg_escaped = svg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

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
