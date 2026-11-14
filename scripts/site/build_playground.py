#!/usr/bin/env python3
"""
build_playground.py
The interactive playground: a DixScript editor and a live SVG preview,
driven entirely client-side by a WASM build of the real `msx` crate
(web/msx-wasm — the same parse_scene/render/compile/decode the CLI and
every other renderer use, not a reimplementation). No server-side
rendering — this is a static page, and Cloudflare Pages serves it as
one.

`window.__msxRenderAndInspect` is the one JS/WASM contract this page
depends on: a function taking a DixScript source string and returning a
JSON string (see web/msx-wasm/src/lib.rs's `render_and_inspect` for the
exact shape) or throwing/rejecting with an error string. Animated scenes
also use `window.__msxRenderFrame(t)` (SVG for elapsed time `t` seconds)
and `window.__msxLocalTime(t)` (the timeline position that frame shows),
both optional: without them the page renders a single still frame.
The examples in the dropdown are fetched when picked, from
`./examples/manifest.json` and `./examples/<file>` (written by
build_examples.py from the repository's examples/ directory), so the page
needs no list of its own. A link such as `/playground/#example=blend_modes`
opens an example directly.
`wasm-loader.js`
(a separate, tiny file — see its own header comment) is responsible for
loading the actual `.wasm` binary and installing that global; this file
never touches wasm-bindgen's generated JS glue directly, so the loader
can change independently of the page markup.
"""

from templates import page

# Shown until an example is picked, and when the examples cannot be fetched.
DEFAULT_SOURCE = """@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 300, height = 300, background = #0d1117 }
  elements::
    { type = "circle", cx = 150, cy = 150, r = 100,
      style = { fill = #4a9eff, stroke = #1a2438, stroke_width = 4.0, opacity = 1.0 } }
)"""

BODY = r"""<h1>Playground</h1>
<p class="lede">DixScript in, SVG out, rendered live in your browser by a WebAssembly build of the real <code>msx</code> crate. Scenes with keyframe tracks get a play bar. Pick an example to start from, or write your own. Nothing is sent to a server.</p>

<div class="pg-toolbar">
  <label for="pg-example">Example</label>
  <button id="pg-prev" type="button" class="pg-step" title="Previous example" aria-label="Previous example" disabled>&lsaquo;</button>
  <select id="pg-example" class="pg-select" disabled><option>Loading examples…</option></select>
  <button id="pg-next" type="button" class="pg-step" title="Next example" aria-label="Next example" disabled>&rsaquo;</button>
  <button id="pg-copy-link" type="button" class="pg-step pg-wide">Copy link</button>
</div>
<p id="pg-example-desc" class="pg-example-desc"></p>
<p id="pg-example-note" class="pg-example-note" hidden></p>

<div class="pg-shell">
  <div class="pg-pane">
    <div class="pg-pane-header">
      <span>DixScript source</span>
      <span id="pg-status" class="pg-status loading">loading engine…</span>
    </div>
    <textarea id="pg-editor" spellcheck="false"></textarea>
  </div>
  <div class="pg-pane">
    <div class="pg-pane-header">
      <span>Preview</span>
      <span id="pg-inspect" class="pg-status"></span>
    </div>
    <div id="pg-preview" class="pg-preview-body"><span style="color:var(--muted)">Loading render engine…</span></div>
    <div id="pg-transport" class="pg-transport" hidden>
      <button id="pg-play" type="button">Pause</button>
      <input id="pg-scrub" type="range" min="0" max="1000" value="0" step="1" aria-label="Animation position">
      <span id="pg-time" class="pg-time">0.00 / 0.00 s</span>
    </div>
  </div>
</div>
<p style="margin-top:20px;color:var(--muted);font-size:13px;">
  Full syntax reference: <a href="/docs/format-spec/">Format Specification</a>. The same examples, rendered natively, are on the <a href="/samples/">samples</a> page.
</p>

<script>
const DEFAULT_SOURCE = __DEFAULT_SOURCE__;
const EXAMPLE_BASE = '/playground/examples/';
const editor  = document.getElementById('pg-editor');
const preview = document.getElementById('pg-preview');
// `status` is a window property, so the element is named statusEl.
const statusEl = document.getElementById('pg-status');
const inspect = document.getElementById('pg-inspect');
const transport = document.getElementById('pg-transport');
const playBtn   = document.getElementById('pg-play');
const scrub     = document.getElementById('pg-scrub');
const timeLabel = document.getElementById('pg-time');
const exSelect  = document.getElementById('pg-example');
const exPrev    = document.getElementById('pg-prev');
const exNext    = document.getElementById('pg-next');
const exCopy    = document.getElementById('pg-copy-link');
const exDesc    = document.getElementById('pg-example-desc');
const exNote    = document.getElementById('pg-example-note');
const reducedMotion = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

function setStatus(el, text, kind) {
  el.textContent = text;
  el.className = 'pg-status' + (kind ? ' ' + kind : '');
}

// Playback. `pos` is raw elapsed seconds; the engine applies the scene's loop
// mode itself (see render_frame in web/msx-wasm), so a loop or ping_pong scene
// keeps counting up here and the frames wrap or mirror on their own.
const FRAME_MS = 33;
const player = { animated: false, duration: 0, loopMode: 'once', playing: false, pos: 0, startWall: 0, startPos: 0, raf: 0, lastDraw: 0 };

function atEnd() {
  return player.loopMode === 'once' && player.duration > 0 && player.pos >= player.duration;
}

function refreshButton() {
  playBtn.textContent = player.playing ? 'Pause' : (atEnd() ? 'Replay' : 'Play');
}

function drawFrame() {
  preview.innerHTML = window.__msxRenderFrame(player.pos);
  const local = (typeof window.__msxLocalTime === 'function') ? window.__msxLocalTime(player.pos) : player.pos;
  scrub.value = player.duration > 0 ? Math.round(local / player.duration * 1000) : 0;
  timeLabel.textContent = local.toFixed(2) + ' / ' + player.duration.toFixed(2) + ' s';
}

function showFrameError(err) {
  setPlaying(false);
  const msg = (err && err.message) ? err.message : String(err);
  preview.innerHTML = `<div class="pg-error-pane">${msg.replace(/&/g,'&amp;').replace(/</g,'&lt;')}</div>`;
  setStatus(statusEl, 'error', 'err');
}

function setPlaying(on) {
  player.playing = on;
  if (player.raf) { cancelAnimationFrame(player.raf); player.raf = 0; }
  if (on) {
    player.startWall = performance.now();
    player.startPos = player.pos;
    player.raf = requestAnimationFrame(tick);
  }
  refreshButton();
}

function tick(now) {
  if (!player.playing) return;
  player.pos = player.startPos + (now - player.startWall) / 1000;
  const finished = player.loopMode === 'once' && player.pos >= player.duration;
  if (finished) player.pos = player.duration;
  if (finished || now - player.lastDraw >= FRAME_MS) {
    player.lastDraw = now;
    try { drawFrame(); } catch (err) { showFrameError(err); return; }
  }
  if (finished) { setPlaying(false); return; }
  player.raf = requestAnimationFrame(tick);
}

playBtn.addEventListener('click', () => {
  if (!player.animated) return;
  if (player.playing) { setPlaying(false); return; }
  if (atEnd()) player.pos = 0;
  setPlaying(true);
});

scrub.addEventListener('input', () => {
  if (!player.animated) return;
  setPlaying(false);
  player.pos = (Number(scrub.value) / 1000) * player.duration;
  try { drawFrame(); } catch (err) { showFrameError(err); }
  refreshButton();
});

// Called after every successful render. A scene that just became animated
// starts playing (unless the visitor asked for reduced motion). Editing a
// scene that was already playing keeps it playing from the same position.
function applyInfo(info) {
  const wasAnimated = player.animated;
  const canPlay = !!info.animated && typeof window.__msxRenderFrame === 'function';
  player.animated = canPlay;
  player.duration = Number(info.duration) || 0;
  player.loopMode = info.loop_mode || 'once';
  transport.hidden = !canPlay;
  if (!canPlay) {
    setPlaying(false);
    preview.innerHTML = info.svg;
    return;
  }
  if (!wasAnimated) player.pos = 0;
  if (player.loopMode === 'once' && player.pos > player.duration) player.pos = player.duration;
  try { drawFrame(); } catch (err) { showFrameError(err); return; }
  if (!wasAnimated) { setPlaying(!reducedMotion); }
  else if (player.playing) { setPlaying(true); }
  else { refreshButton(); }
}

let renderTimer = null;
function scheduleRender() {
  clearTimeout(renderTimer);
  renderTimer = setTimeout(doRender, 200); // debounce — avoid re-rendering on every keystroke
}

function doRender() {
  clearTimeout(renderTimer);
  if (typeof window.__msxRenderAndInspect !== 'function') {
    return; // engine not loaded yet; wasm-loader.js will call doRender() once it is
  }
  const source = editor.value;
  try {
    const json = window.__msxRenderAndInspect(source);
    const info = JSON.parse(json);
    applyInfo(info);
    setStatus(statusEl, 'ready', 'ok');
    setStatus(inspect, `${info.source_bytes}B src -> ${info.binary_bytes}B bin · ${info.element_count} element(s) · roundtrip ${info.roundtrip_ok ? 'OK' : 'MISMATCH'}`, info.roundtrip_ok ? 'ok' : 'err');
  } catch (err) {
    const msg = (err && err.message) ? err.message : String(err);
    player.animated = false;
    transport.hidden = true;
    setPlaying(false);
    preview.innerHTML = `<div class="pg-error-pane">${msg.replace(/&/g,'&amp;').replace(/</g,'&lt;')}</div>`;
    setStatus(statusEl, 'error', 'err');
    setStatus(inspect, '', '');
  }
}

// Called by wasm-loader.js once the module finishes loading.
window.__msxEngineReady = function() {
  setStatus(statusEl, 'ready', 'ok');
  doRender();
};

// ---- Examples ------------------------------------------------------------
// The list is examples/manifest.json, written at build time from the
// repository's examples/ directory. Nothing here is a copy of it.
let examples = [];
let current = null;   // the manifest entry that is in the editor, or null once the visitor edits it
let loadedFrom = null;

function findExample(key) {
  if (!key) return null;
  return examples.find(e => e.id === key)
      || examples.find(e => e.file === key || e.file.replace(/\.msx$/, '') === key)
      || null;
}

// `#example=name`, `#name` and `?example=name` all name an example.
function keyFromLocation() {
  const hash = location.hash.replace(/^#/, '');
  if (hash) {
    const params = new URLSearchParams(hash);
    if (params.has('example')) return params.get('example');
    if (!hash.includes('=')) return decodeURIComponent(hash);
  }
  return new URLSearchParams(location.search).get('example') || '';
}

function describe(entry) {
  exDesc.textContent = entry ? entry.description + (entry.animated ? ' Animated.' : '') : '';
  exNote.hidden = !(entry && entry.note);
  exNote.textContent = entry && entry.note ? entry.note : '';
}

function updateStepButtons() {
  const i = current ? examples.indexOf(current) : -1;
  exPrev.disabled = examples.length === 0;
  exNext.disabled = examples.length === 0;
  exSelect.value = current ? current.id : '';
  exSelect.dataset.edited = current ? '' : '1';
}

async function loadExample(entry, updateHash) {
  setStatus(statusEl, 'loading example…', 'loading');
  let text;
  try {
    const res = await fetch(EXAMPLE_BASE + encodeURIComponent(entry.file));
    if (!res.ok) throw new Error('HTTP ' + res.status);
    text = await res.text();
  } catch (err) {
    setStatus(statusEl, 'could not load the example', 'err');
    return false;
  }
  editor.value = text;
  current = entry;
  loadedFrom = entry;
  describe(entry);
  updateStepButtons();
  // A new example starts from the beginning and plays, even if the last one was playing.
  player.pos = 0;
  player.animated = false;
  if (updateHash) history.replaceState(null, '', '#example=' + encodeURIComponent(entry.id));
  doRender();
  return true;
}

function step(delta) {
  if (!examples.length) return;
  const i = current ? examples.indexOf(current) : (delta > 0 ? -1 : 0);
  const next = examples[(i + delta + examples.length) % examples.length];
  loadExample(next, true);
}

exSelect.addEventListener('change', () => {
  const entry = findExample(exSelect.value);
  if (entry) loadExample(entry, true);
});
exPrev.addEventListener('click', () => step(-1));
exNext.addEventListener('click', () => step(1));

exCopy.addEventListener('click', async () => {
  const base = location.origin + location.pathname;
  const url = current ? base + '#example=' + encodeURIComponent(current.id) : base;
  try {
    await navigator.clipboard.writeText(url);
    exCopy.textContent = 'Copied';
    setTimeout(() => { exCopy.textContent = 'Copy link'; }, 1500);
  } catch (err) {
    window.prompt('Copy this link', url);
  }
});

// Editing detaches the editor from the example it came from.
editor.addEventListener('input', () => {
  if (current) {
    exDesc.textContent = 'Edited from "' + current.title + '".';
    exNote.hidden = true;
    current = null;
    exSelect.value = '';
    history.replaceState(null, '', location.pathname + location.search);
  }
  scheduleRender();
});

window.addEventListener('hashchange', () => {
  const entry = findExample(keyFromLocation());
  if (entry && entry !== current) loadExample(entry, false);
});

function fillSelect(manifest) {
  exSelect.innerHTML = '<option value="">Choose an example…</option>';
  for (const cat of manifest.categories) {
    const group = document.createElement('optgroup');
    group.label = cat.title;
    for (const e of manifest.examples.filter(x => x.category === cat.id)) {
      const opt = document.createElement('option');
      opt.value = e.id;
      opt.textContent = (e.animated ? '\u25B6 ' : '') + e.title;
      group.appendChild(opt);
    }
    exSelect.appendChild(group);
  }
  exSelect.disabled = false;
}

async function initExamples() {
  try {
    const res = await fetch(EXAMPLE_BASE + 'manifest.json');
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const manifest = await res.json();
    // The order of the dropdown is the order a visitor steps through with the arrows.
    examples = manifest.categories.flatMap(c => manifest.examples.filter(e => e.category === c.id));
    fillSelect(manifest);
  } catch (err) {
    exSelect.innerHTML = '<option>Examples unavailable</option>';
    return;
  }
  updateStepButtons();
  const entry = findExample(keyFromLocation());
  if (entry) await loadExample(entry, false);
}

editor.value = DEFAULT_SOURCE;
initExamples();
</script>
<script type="module" src="/playground/wasm-loader.js"></script>"""


def build_playground_page() -> str:
    import json
    body = BODY.replace("__DEFAULT_SOURCE__", json.dumps(DEFAULT_SOURCE))
    return page(title="Playground", active="playground", body=body, wide=True)


if __name__ == "__main__":
    print(build_playground_page())
