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
`wasm-loader.js`
(a separate, tiny file — see its own header comment) is responsible for
loading the actual `.wasm` binary and installing that global; this file
never touches wasm-bindgen's generated JS glue directly, so the loader
can change independently of the page markup.
"""

from templates import page

EXAMPLES = {
    "circle": '''@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 300, height = 300, background = #0d1117 }
  elements::
    { type = "circle", cx = 150, cy = 150, r = 100,
      style = { fill = #4a9eff, stroke = #1a2438, stroke_width = 4.0, opacity = 1.0 } }
)''',
    "gradient": '''@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 300, height = 200, background = #ffffff }
  defs::
    { type = "linear_gradient", id = "g", x1 = 0.0, y1 = 0.0, x2 = 1.0, y2 = 0.0,
      stops = [ { offset = 0.0, color = #4a9eff, opacity = 1.0 },
                { offset = 1.0, color = #f2545b, opacity = 1.0 } ] }
  elements::
    { type = "rect", x = 20, y = 20, width = 260, height = 160,
      style = { fill = "url(#g)", stroke = "none", stroke_width = 0, opacity = 1.0 } }
)''',
    "animation": '''@CONFIG( version -> "1.0.0" )
@DATA(
  scene = { width = 200, height = 200, background = #0d1117 }
  duration = 2.0
  loop_mode = "ping_pong"
  elements::
    { type = "circle", id = "dot", cx = 40, cy = 100, r = 15,
      style = { fill = #f5a623, stroke = "none", stroke_width = 0, opacity = 1.0 } }
  animations::
    { target_id = "dot", property = "translate_x",
      keyframes = [ { time = 0.0, value = 0.0, easing = "ease_in_out" },
                    { time = 2.0, value = 120.0, easing = "ease_in_out" } ] }
)''',
}

DEFAULT_EXAMPLE = "circle"


def build_playground_page() -> str:
    example_buttons = "".join(
        f'<button data-example="{name}">{name}</button>' for name in EXAMPLES
    )
    examples_js = "{\n" + ",\n".join(
        f'  {name!r}: {src!r}' for name, src in EXAMPLES.items()
    ) + "\n}"

    body = f'''<h1>Playground</h1>
<p class="lede">DixScript in, SVG out, rendered live in your browser by a WebAssembly build of the real <code>msx</code> crate. Scenes with keyframe tracks get a play bar. Nothing is sent to a server.</p>

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
<div class="pg-examples">
  <span style="color:var(--muted);font-size:12px;align-self:center;margin-right:4px;">Examples:</span>
  {example_buttons}
</div>
<p style="margin-top:20px;color:var(--muted);font-size:13px;">
  Full syntax reference: <a href="/docs/format-spec/">Format Specification</a>.
</p>

<script>
const MSX_EXAMPLES = {examples_js};
const editor  = document.getElementById('pg-editor');
const preview = document.getElementById('pg-preview');
// `status` is a window property, so the element is named statusEl.
const statusEl = document.getElementById('pg-status');
const inspect = document.getElementById('pg-inspect');
const transport = document.getElementById('pg-transport');
const playBtn   = document.getElementById('pg-play');
const scrub     = document.getElementById('pg-scrub');
const timeLabel = document.getElementById('pg-time');
const reducedMotion = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

function setStatus(el, text, kind) {{
  el.textContent = text;
  el.className = 'pg-status' + (kind ? ' ' + kind : '');
}}

// Playback. `pos` is raw elapsed seconds; the engine applies the scene's loop
// mode itself (see render_frame in web/msx-wasm), so a loop or ping_pong scene
// keeps counting up here and the frames wrap or mirror on their own.
const FRAME_MS = 33;
const player = {{ animated: false, duration: 0, loopMode: 'once', playing: false, pos: 0, startWall: 0, startPos: 0, raf: 0, lastDraw: 0 }};

function atEnd() {{
  return player.loopMode === 'once' && player.duration > 0 && player.pos >= player.duration;
}}

function refreshButton() {{
  playBtn.textContent = player.playing ? 'Pause' : (atEnd() ? 'Replay' : 'Play');
}}

function drawFrame() {{
  preview.innerHTML = window.__msxRenderFrame(player.pos);
  const local = (typeof window.__msxLocalTime === 'function') ? window.__msxLocalTime(player.pos) : player.pos;
  scrub.value = player.duration > 0 ? Math.round(local / player.duration * 1000) : 0;
  timeLabel.textContent = local.toFixed(2) + ' / ' + player.duration.toFixed(2) + ' s';
}}

function showFrameError(err) {{
  setPlaying(false);
  const msg = (err && err.message) ? err.message : String(err);
  preview.innerHTML = `<div class="pg-error-pane">${{msg.replace(/&/g,'&amp;').replace(/</g,'&lt;')}}</div>`;
  setStatus(statusEl, 'error', 'err');
}}

function setPlaying(on) {{
  player.playing = on;
  if (player.raf) {{ cancelAnimationFrame(player.raf); player.raf = 0; }}
  if (on) {{
    player.startWall = performance.now();
    player.startPos = player.pos;
    player.raf = requestAnimationFrame(tick);
  }}
  refreshButton();
}}

function tick(now) {{
  if (!player.playing) return;
  player.pos = player.startPos + (now - player.startWall) / 1000;
  const finished = player.loopMode === 'once' && player.pos >= player.duration;
  if (finished) player.pos = player.duration;
  if (finished || now - player.lastDraw >= FRAME_MS) {{
    player.lastDraw = now;
    try {{ drawFrame(); }} catch (err) {{ showFrameError(err); return; }}
  }}
  if (finished) {{ setPlaying(false); return; }}
  player.raf = requestAnimationFrame(tick);
}}

playBtn.addEventListener('click', () => {{
  if (!player.animated) return;
  if (player.playing) {{ setPlaying(false); return; }}
  if (atEnd()) player.pos = 0;
  setPlaying(true);
}});

scrub.addEventListener('input', () => {{
  if (!player.animated) return;
  setPlaying(false);
  player.pos = (Number(scrub.value) / 1000) * player.duration;
  try {{ drawFrame(); }} catch (err) {{ showFrameError(err); }}
  refreshButton();
}});

// Called after every successful render. A scene that just became animated
// starts playing (unless the visitor asked for reduced motion). Editing a
// scene that was already playing keeps it playing from the same position.
function applyInfo(info) {{
  const wasAnimated = player.animated;
  const canPlay = !!info.animated && typeof window.__msxRenderFrame === 'function';
  player.animated = canPlay;
  player.duration = Number(info.duration) || 0;
  player.loopMode = info.loop_mode || 'once';
  transport.hidden = !canPlay;
  if (!canPlay) {{
    setPlaying(false);
    preview.innerHTML = info.svg;
    return;
  }}
  if (!wasAnimated) player.pos = 0;
  if (player.loopMode === 'once' && player.pos > player.duration) player.pos = player.duration;
  try {{ drawFrame(); }} catch (err) {{ showFrameError(err); return; }}
  if (!wasAnimated) {{ setPlaying(!reducedMotion); }}
  else if (player.playing) {{ setPlaying(true); }}
  else {{ refreshButton(); }}
}}

let renderTimer = null;
function scheduleRender() {{
  clearTimeout(renderTimer);
  renderTimer = setTimeout(doRender, 200); // debounce — avoid re-rendering on every keystroke
}}

function doRender() {{
  if (typeof window.__msxRenderAndInspect !== 'function') {{
    return; // engine not loaded yet; wasm-loader.js will call renderNow() once it is
  }}
  const source = editor.value;
  try {{
    const json = window.__msxRenderAndInspect(source);
    const info = JSON.parse(json);
    applyInfo(info);
    setStatus(statusEl, 'ready', 'ok');
    setStatus(inspect, `${{info.source_bytes}}B src -> ${{info.binary_bytes}}B bin · ${{info.element_count}} element(s) · roundtrip ${{info.roundtrip_ok ? 'OK' : 'MISMATCH'}}`, info.roundtrip_ok ? 'ok' : 'err');
  }} catch (err) {{
    const msg = (err && err.message) ? err.message : String(err);
    player.animated = false;
    transport.hidden = true;
    setPlaying(false);
    preview.innerHTML = `<div class="pg-error-pane">${{msg.replace(/&/g,'&amp;').replace(/</g,'&lt;')}}</div>`;
    setStatus(statusEl, 'error', 'err');
    setStatus(inspect, '', '');
  }}
}}

// Called by wasm-loader.js once the module finishes loading.
window.__msxEngineReady = function() {{
  setStatus(statusEl, 'ready', 'ok');
  doRender();
}};

editor.addEventListener('input', scheduleRender);

document.querySelectorAll('.pg-examples button').forEach(btn => {{
  btn.addEventListener('click', () => {{
    editor.value = MSX_EXAMPLES[btn.dataset.example];
    player.pos = 0;
    scheduleRender();
  }});
}});

editor.value = MSX_EXAMPLES[{DEFAULT_EXAMPLE!r}];
</script>
<script type="module" src="/playground/wasm-loader.js"></script>'''

    return page(title="Playground", active="playground", body=body, wide=True)


if __name__ == "__main__":
    print(build_playground_page())
