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
// exist in this source tree and is never hand-written; it's a build
// artifact copied into site/playground/ alongside this file and the
// .wasm binary itself, both untracked (see the same workflow's
// .gitignore-equivalent build-output handling).
import init, { render_and_inspect, init_panic_hook } from './msx_wasm.js';

(async () => {
  try {
    await init();
    init_panic_hook();
    window.__msxRenderAndInspect = render_and_inspect;
    if (typeof window.__msxEngineReady === 'function') {
      window.__msxEngineReady();
    }
  } catch (err) {
    const status = document.getElementById('pg-status');
    if (status) {
      status.textContent = 'engine failed to load';
      status.className = 'pg-status err';
    }
    console.error('msx-wasm failed to load:', err);
  }
})();
