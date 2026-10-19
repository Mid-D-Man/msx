#!/usr/bin/env python3
"""
build_convert.py
The SVG to MSX converter page. An SVG is pasted, dropped or opened from disk,
and the page shows the converted .msx source next to a side by side render of
the original and of the converted scene. Everything runs in the browser: the
same WASM build that drives the playground (web/msx-wasm) exposes
`import_svg` (the real msx-svg-import crate plus the msx-parser writer) and
`compile_source` (the real binary codec), so the page output matches
`msx import` and `msx compile` byte for byte.

JS/WASM contract, installed by /playground/wasm-loader.js:
  window.__msxImportSvg(svgText) -> JSON string, or throws an error string
      {msx, svg, warnings: [..], svg_bytes, msx_bytes, element_count, reparse_matches}
  window.__msxCompile(msxSource) -> Uint8Array holding the compressed binary
The page reuses the element ids `pg-status` and `pg-preview` because the
loader writes its failure message into them.
"""

import json

from templates import page

EXAMPLES = {
    "gradient badge": """<svg xmlns="http://www.w3.org/2000/svg" width="240" height="160" viewBox="0 0 240 160">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#4a9eff"/>
      <stop offset="1" stop-color="#f2545b"/>
    </linearGradient>
  </defs>
  <rect x="10" y="10" width="220" height="140" rx="18" fill="url(#g)"/>
  <circle cx="70" cy="80" r="28" fill="#ffffff" fill-opacity=".85"/>
  <text x="115" y="88" font-size="26" fill="#ffffff" font-family="sans-serif">MSX</text>
</svg>
""",
    "stroke icon": """<svg xmlns="http://www.w3.org/2000/svg" width="160" height="160" viewBox="0 0 24 24" fill="none" stroke="#f2545b" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
</svg>
""",
    "css and use": """<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="240" height="120" viewBox="0 0 240 120">
  <style>.dot { fill: #f5a623 } .ring { fill: none; stroke: #4a9eff; stroke-width: 4 }</style>
  <defs>
    <g id="pair"><circle class="dot" cx="20" cy="20" r="12"/><circle class="ring" cx="20" cy="20" r="18"/></g>
  </defs>
  <use href="#pair" x="10" y="10"/>
  <use href="#pair" x="90" y="40" opacity=".6"/>
  <g transform="translate(170 60) rotate(20)"><rect width="50" height="30" fill="#7ad17a"/></g>
</svg>
""",
}

DEFAULT_EXAMPLE = "gradient badge"

CSS = """
.cv-bar { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin: 0 0 14px; }
.cv-bar button, .cv-bar label.cv-btn, .cv-actions button {
  font: inherit; font-size: 12px; padding: 5px 10px; border-radius: 6px;
  background: var(--surface2); border: 1px solid var(--border); color: var(--muted); cursor: pointer;
}
.cv-bar button:hover, .cv-bar label.cv-btn:hover, .cv-actions button:hover:not(:disabled) { color: var(--text); border-color: var(--accent-dim); }
.cv-actions button:disabled { opacity: .45; cursor: default; }
.cv-bar .cv-label { color: var(--muted); font-size: 12px; margin-right: 4px; }
.cv-bar input[type=file] { display: none; }
.cv-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--border); border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }
.cv-grid .pg-pane { height: 360px; }
.cv-grid textarea { flex: 1; width: 100%; background: var(--bg-raised); color: var(--text); border: none; resize: none; font-family: var(--mono); font-size: 13px; line-height: 1.55; padding: 14px; outline: none; }
.cv-grid .pg-preview-body img { max-width: 100%; max-height: 100%; background: #fff0; }
.cv-actions { display: flex; gap: 6px; }
.cv-warnings { margin: 16px 0 0; padding: 12px 16px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg-raised); font-size: 13px; }
.cv-warnings h3 { margin: 0 0 6px; font-size: 13px; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); }
.cv-warnings ul { margin: 0; padding-left: 18px; font-family: var(--mono); font-size: 12.5px; }
.cv-warnings.clean { color: var(--good); }
.cv-drop { outline: 2px dashed var(--accent); outline-offset: -6px; }
@media (max-width: 860px) { .cv-grid { grid-template-columns: 1fr; } }
"""

SCRIPT = r"""
const CV_EXAMPLES = __EXAMPLES__;
const $ = (id) => document.getElementById(id);
const svgIn = $('cv-svg'), msxOut = $('cv-msx'), statusEl = $('pg-status');
const origBox = $('cv-orig'), msxBox = $('pg-preview'), warnBox = $('cv-warnings'), statsEl = $('cv-stats');
const copyBtn = $('cv-copy'), dlSrc = $('cv-dl-src'), dlBin = $('cv-dl-bin'), fileIn = $('cv-file');
let lastSource = '';
let baseName = 'converted';
const urls = { orig: null, msx: null };

function setStatus(text, kind) { statusEl.textContent = text; statusEl.className = 'pg-status' + (kind ? ' ' + kind : ''); }

function showSvg(box, key, text) {
  if (urls[key]) URL.revokeObjectURL(urls[key]);
  urls[key] = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
  const img = document.createElement('img');
  img.alt = key === 'orig' ? 'The original SVG' : 'The converted MSX scene';
  img.src = urls[key];
  box.replaceChildren(img);
}

function showError(msg) {
  const pane = document.createElement('div');
  pane.className = 'pg-error-pane';
  pane.textContent = msg;
  msxBox.replaceChildren(pane);
  msxOut.value = '';
  lastSource = '';
  copyBtn.disabled = dlSrc.disabled = dlBin.disabled = true;
  statsEl.textContent = '';
  warnBox.hidden = true;
  setStatus('error', 'err');
}

function showWarnings(list) {
  warnBox.hidden = false;
  warnBox.replaceChildren();
  if (!list.length) {
    warnBox.className = 'cv-warnings clean';
    warnBox.textContent = 'Converted with no warnings.';
    return;
  }
  warnBox.className = 'cv-warnings';
  const h = document.createElement('h3');
  h.textContent = list.length + (list.length === 1 ? ' warning' : ' warnings') + ' from the importer';
  const ul = document.createElement('ul');
  for (const w of list) { const li = document.createElement('li'); li.textContent = w; ul.appendChild(li); }
  warnBox.append(h, ul);
}

function convert() {
  if (typeof window.__msxImportSvg !== 'function') return;
  const src = svgIn.value;
  if (!src.trim()) {
    msxOut.value = ''; lastSource = '';
    origBox.replaceChildren(); msxBox.replaceChildren();
    warnBox.hidden = true; statsEl.textContent = '';
    copyBtn.disabled = dlSrc.disabled = dlBin.disabled = true;
    setStatus('ready', 'ok');
    return;
  }
  try {
    const info = JSON.parse(window.__msxImportSvg(src));
    lastSource = info.msx;
    msxOut.value = info.msx;
    showSvg(origBox, 'orig', src);
    showSvg(msxBox, 'msx', info.svg);
    showWarnings(info.warnings);
    statsEl.textContent = info.svg_bytes + ' B svg -> ' + info.msx_bytes + ' B msx source · ' + info.element_count + ' element(s)' + (info.reparse_matches ? '' : ' · reparse differs');
    copyBtn.disabled = dlSrc.disabled = dlBin.disabled = false;
    setStatus('ready', 'ok');
  } catch (err) {
    showError((err && err.message) ? err.message : String(err));
  }
}

let timer = null;
function schedule() { clearTimeout(timer); timer = setTimeout(convert, 200); }

function download(name, data, type) {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function loadFile(file) {
  if (!file) return;
  baseName = file.name.replace(/\.[^.]*$/, '') || 'converted';
  const reader = new FileReader();
  reader.onload = () => { svgIn.value = String(reader.result); schedule(); };
  reader.readAsText(file);
}

svgIn.addEventListener('input', schedule);
fileIn.addEventListener('change', () => loadFile(fileIn.files[0]));
['dragenter', 'dragover'].forEach(t => svgIn.addEventListener(t, (e) => { e.preventDefault(); svgIn.classList.add('cv-drop'); }));
['dragleave', 'drop'].forEach(t => svgIn.addEventListener(t, (e) => { e.preventDefault(); svgIn.classList.remove('cv-drop'); }));
svgIn.addEventListener('drop', (e) => loadFile(e.dataTransfer.files[0]));

copyBtn.addEventListener('click', async () => {
  try { await navigator.clipboard.writeText(lastSource); copyBtn.textContent = 'Copied'; }
  catch (_) { msxOut.select(); copyBtn.textContent = 'Press copy now'; }
  setTimeout(() => { copyBtn.textContent = 'Copy'; }, 1500);
});
dlSrc.addEventListener('click', () => download(baseName + '.msx', lastSource, 'text/plain'));
dlBin.addEventListener('click', () => {
  try { download(baseName + '.compiled.msx', window.__msxCompile(lastSource), 'application/octet-stream'); }
  catch (err) { showError((err && err.message) ? err.message : String(err)); }
});

document.querySelectorAll('.cv-bar button[data-example]').forEach(btn => {
  btn.addEventListener('click', () => { baseName = btn.dataset.example.replace(/\s+/g, '-'); svgIn.value = CV_EXAMPLES[btn.dataset.example]; schedule(); });
});

window.__msxEngineReady = function () { setStatus('ready', 'ok'); convert(); };
svgIn.value = CV_EXAMPLES[__DEFAULT__];
baseName = __DEFAULT__.replace(/\s+/g, '-');
"""


def build_convert_page() -> str:
    buttons = "".join(
        f'<button type="button" data-example="{name}">{name}</button>' for name in EXAMPLES
    )
    script = (
        SCRIPT.replace("__EXAMPLES__", json.dumps(EXAMPLES))
        .replace("__DEFAULT__", json.dumps(DEFAULT_EXAMPLE))
    )
    body = f'''<h1>Convert SVG to MSX</h1>
<p class="lede">Paste an SVG, drop a file on the editor or open one from disk. The page converts it to MSX source with the same importer as <code>msx import</code>, then draws the original and the converted scene next to each other. Nothing is sent to a server.</p>

<div class="cv-bar">
  <label class="cv-btn" for="cv-file">Open SVG file</label>
  <input id="cv-file" type="file" accept=".svg,image/svg+xml">
  <span class="cv-label">Examples:</span>
  {buttons}
  <span id="pg-status" class="pg-status loading" style="margin-left:auto">loading engine…</span>
</div>

<div class="cv-grid">
  <div class="pg-pane">
    <div class="pg-pane-header"><span>SVG source</span></div>
    <textarea id="cv-svg" spellcheck="false" aria-label="SVG source"></textarea>
  </div>
  <div class="pg-pane">
    <div class="pg-pane-header">
      <span>MSX source</span>
      <span class="cv-actions">
        <button id="cv-copy" type="button" disabled>Copy</button>
        <button id="cv-dl-src" type="button" disabled>Download .msx</button>
        <button id="cv-dl-bin" type="button" disabled>Download binary</button>
      </span>
    </div>
    <textarea id="cv-msx" spellcheck="false" readonly aria-label="Converted MSX source"></textarea>
  </div>
  <div class="pg-pane">
    <div class="pg-pane-header"><span>Original (your browser)</span></div>
    <div id="cv-orig" class="pg-preview-body"></div>
  </div>
  <div class="pg-pane">
    <div class="pg-pane-header"><span>Converted (MSX)</span><span id="cv-stats" class="pg-status"></span></div>
    <div id="pg-preview" class="pg-preview-body"><span style="color:var(--muted)">Loading render engine…</span></div>
  </div>
</div>
<div id="cv-warnings" class="cv-warnings" hidden></div>

<p style="margin-top:20px;color:var(--muted);font-size:13px;">
  What the importer resolves and what it reports: <a href="/docs/svg-import/">Importing SVG</a>.
  Feature by feature status: <a href="/docs/svg-parity/">SVG parity</a>.
</p>

<script>{script}</script>
<script type="module" src="/playground/wasm-loader.js"></script>'''
    return page(title="Convert", active="convert", body=body, wide=True, extra_head=f"<style>{CSS}</style>")


if __name__ == "__main__":
    print(build_convert_page())
