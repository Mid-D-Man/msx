// site/playground/wasm-loader.js
//
// Loads the wasm-bindgen-generated glue for web/msx-wasm and installs
// the two globals build_playground.py's embedded script depends on:
//   window.__msxRenderAndInspect(source) -> JSON string (see
//     web/msx-wasm/src/lib.rs's `render_and_inspect` for the exact
//     shape) — throws on a parse/compile error, same as the underlying
//     Rust `Result::Err` becoming a thrown JsValue.
//   window.__msxEngineReady() — build_playground.py's own script
//     defines this one; this file only CALLS it, once, after the wasm
//     module finishes initializing.
//
// Deliberately separated from build_playground.py's inline script: this
// is the one file that changes if the wasm-bindgen output path/API
// changes, without touching the page markup at all.
//
// Path note: `./msx_wasm.js` is wasm-bindgen's own generated glue
// module (produced by `wasm-pack build --target web`, see
// .github/workflows/cloudflare-pages.yml's build step) — it does not
// exist in this source tree and is never hand-written; it is a build
// artifact copied into site/playground/ alongside this file and the
// .wasm binary itself.
//
// The glue is pulled in with a dynamic `import()` inside a try/catch,
// not a top-level `import` statement. A failed static import (404, wrong
// MIME type, a missing export) is a module-load error no handler in this
// file can see, which left the page on "loading engine…" with nothing
// visible. Every failure below is written onto the page instead, because
// opening a devtools console is not an option on every device.

function showFailure(stage, err) {
  const msg = (err && err.message) ? err.message : String(err);
  const status = document.getElementById('pg-status');
  if (status) {
    status.textContent = 'engine failed to load';
    status.className = 'pg-status err';
  }
  const preview = document.getElementById('pg-preview');
  if (preview) {
    const box = document.createElement('div');
    box.className = 'pg-error-pane';
    box.textContent = 'Could not load the render engine (' + stage + ').\n\n' + msg;
    preview.replaceChildren(box);
  }
  console.error('msx-wasm failed to load while ' + stage + ':', err);
}

(async () => {
  let glue;
  try {
    glue = await import('./msx_wasm.js');
  } catch (err) {
    showFailure('importing ./msx_wasm.js', err);
    return;
  }
  try {
    await glue.default();
    glue.init_panic_hook();
    window.__msxRenderAndInspect = glue.render_and_inspect;
    // Playback entry points. A wasm build without them still renders stills.
    if (typeof glue.render_frame === 'function') window.__msxRenderFrame = glue.render_frame;
    if (typeof glue.local_time === 'function')   window.__msxLocalTime = glue.local_time;
    if (typeof window.__msxEngineReady === 'function') {
      window.__msxEngineReady();
    }
  } catch (err) {
    showFailure('initializing the wasm module', err);
  }
})();
