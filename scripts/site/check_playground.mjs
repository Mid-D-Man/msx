// scripts/site/check_playground.mjs
// Drives the built playground page like a visitor, with the engine stubbed out, to
// check the example loader: the dropdown, deep links, the step buttons, editing,
// copy link and the failure paths. The compiled WASM engine is not needed.
//
// Usage (needs `npm install jsdom` somewhere Node can find it):
//   python3 scripts/site/build_site.py --repo-root . --out /tmp/site
//   node scripts/site/check_playground.mjs /tmp/site
import { JSDOM } from 'jsdom';
import fs from 'node:fs';
import path from 'node:path';

const site = path.resolve(process.argv[2] || 'site');
const html = fs.readFileSync(path.join(site, 'playground', 'index.html'), 'utf8');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
function check(cond, what) {
  if (!cond) { failures++; console.log('FAIL  ' + what); } else { console.log('ok    ' + what); }
}

async function open(url, { manifestFails = false } = {}) {
  const calls = [];
  const dom = new JSDOM(html, {
    url,
    runScripts: 'dangerously',
    pretendToBeVisual: true,
    beforeParse(window) {
      window.fetch = async (u) => {
        const rel = new URL(u, window.location.href).pathname.replace(/^\/playground\//, '');
        calls.push(rel);
        const file = path.join(site, 'playground', decodeURIComponent(rel));
        if ((manifestFails && rel.endsWith('manifest.json')) || !fs.existsSync(file)) {
          return { ok: false, status: 404, text: async () => '', json: async () => ({}) };
        }
        const body = fs.readFileSync(file, 'utf8');
        return { ok: true, status: 200, text: async () => body, json: async () => JSON.parse(body) };
      };
      window.__rendered = [];
      window.__msxRenderAndInspect = (src) => {
        window.__rendered.push(src);
        return JSON.stringify({ animated: /^\s*animations\s*::/m.test(src), duration: 2, loop_mode: 'loop', svg: '<svg/>', source_bytes: src.length, binary_bytes: 1, element_count: 1, roundtrip_ok: true });
      };
      window.__msxRenderFrame = (t) => `<svg data-t="${t}"/>`;
      window.__msxLocalTime = (t) => t % 2;
      window.requestAnimationFrame = () => 0;
    },
  });
  const w = dom.window;
  w.__msxEngineReady();
  await sleep(150);
  return { dom, w, d: w.document, calls };
}

const manifest = JSON.parse(fs.readFileSync(path.join(site, 'playground', 'examples', 'manifest.json'), 'utf8'));
const src = (file) => fs.readFileSync(path.join(site, 'playground', 'examples', file), 'utf8');

// 1. Plain visit: dropdown filled from the manifest, default source kept.
{
  const { w, d } = await open('https://msx.test/playground/');
  const sel = d.getElementById('pg-example');
  check(!sel.disabled, 'the dropdown is enabled once the manifest loads');
  check(d.querySelectorAll('#pg-example optgroup').length === manifest.categories.length, 'one group per category');
  check(d.querySelectorAll('#pg-example option[value]:not([value=""])').length === manifest.examples.length, 'one option per example');
  check(d.getElementById('pg-editor').value.includes('"circle"') && d.getElementById('pg-editor').value.includes('r = 100'), 'the default scene stays until an example is picked');
  const animatedOpt = [...d.querySelectorAll('#pg-example option')].find((o) => o.value === 'orbit_pulse');
  check(animatedOpt && animatedOpt.textContent.startsWith('\u25B6'), 'animated examples are marked');
}

// 2. Deep links: #example=id, #id, ?example=id, and a file stem for an entry whose id differs.
for (const [url, id, why] of [
  ['https://msx.test/playground/#example=blend_modes', 'blend_modes', '#example=id'],
  ['https://msx.test/playground/#gradient_spread', 'gradient_spread', '#id'],
  ['https://msx.test/playground/?example=clip_and_mask', 'clip_and_mask', '?example=id'],
  ['https://msx.test/playground/#example=sdf_splat_orbit(3)', 'sdf_splat_orbit', 'a file stem'],
]) {
  const { d } = await open(url);
  const entry = manifest.examples.find((e) => e.id === id);
  check(d.getElementById('pg-editor').value === src(entry.file), `deep link ${why} loads ${id}`);
  check(d.getElementById('pg-example').value === id, `deep link ${why} selects ${id}`);
  check(d.getElementById('pg-example-desc').textContent.startsWith(entry.description), `deep link ${why} shows its description`);
}

// 3. Unknown deep link: no crash, default stays.
{
  const { d } = await open('https://msx.test/playground/#example=nothing_like_it');
  check(d.getElementById('pg-editor').value.includes('r = 100'), 'an unknown example leaves the default scene');
}

// 4. Picking from the dropdown loads, renders, and updates the address.
{
  const { w, d } = await open('https://msx.test/playground/');
  const sel = d.getElementById('pg-example');
  sel.value = 'effects_gallery';
  sel.dispatchEvent(new w.Event('change'));
  await sleep(150);
  check(d.getElementById('pg-editor').value === src('effects_gallery.msx'), 'picking loads the file');
  check(w.location.hash === '#example=effects_gallery', 'picking updates the hash');
  check(w.__rendered.at(-1) === src('effects_gallery.msx'), 'picking renders at once, without the typing delay');
  const note = d.getElementById('pg-example-note');
  check(!note.hidden && note.textContent.includes('GPU'), 'an example with a note shows it');
  sel.value = 'blend_modes';
  sel.dispatchEvent(new w.Event('change'));
  await sleep(150);
  check(d.getElementById('pg-example-note').hidden, 'the note goes away with the next example');
}

// 5. Animated example shows the transport and starts from zero.
{
  const { d } = await open('https://msx.test/playground/#example=anim_clock');
  check(!d.getElementById('pg-transport').hidden, 'an animated example shows the play bar');
  const { d: d2 } = await open('https://msx.test/playground/#example=blend_modes');
  check(d2.getElementById('pg-transport').hidden, 'a still example hides it');
}

// 6. Previous and next walk the dropdown order and wrap.
{
  const { w, d } = await open('https://msx.test/playground/');
  const order = manifest.categories.flatMap((c) => manifest.examples.filter((e) => e.category === c.id));
  d.getElementById('pg-next').click(); await sleep(120);
  check(d.getElementById('pg-example').value === order[0].id, 'next from the default goes to the first example');
  d.getElementById('pg-prev').click(); await sleep(120);
  check(d.getElementById('pg-example').value === order.at(-1).id, 'previous from the first wraps to the last');
  d.getElementById('pg-next').click(); await sleep(120);
  check(d.getElementById('pg-example').value === order[0].id, 'next from the last wraps to the first');
}

// 7. Editing detaches from the example and clears the link.
{
  const { w, d } = await open('https://msx.test/playground/#example=gradient_demo');
  const ed = d.getElementById('pg-editor');
  ed.value = ed.value + '\n// edited';
  ed.dispatchEvent(new w.Event('input'));
  check(d.getElementById('pg-example-desc').textContent.startsWith('Edited from'), 'editing says the source was edited');
  check(w.location.hash === '', 'editing clears the example from the address');
  check(d.getElementById('pg-example').value === '', 'editing clears the dropdown choice');
}

// 8. Browser back/forward and in-page links: a hash change loads the example.
{
  const { w, d } = await open('https://msx.test/playground/#example=gradients');
  w.location.hash = '#example=text_demo';
  await sleep(200);
  check(d.getElementById('pg-editor').value === src('text_demo.msx'), 'a hash change loads that example');
}

// 9. Copy link.
{
  const { w, d } = await open('https://msx.test/playground/#example=stroke_styles');
  let copied = '';
  Object.defineProperty(w.navigator, 'clipboard', { value: { writeText: async (t) => { copied = t; } }, configurable: true });
  d.getElementById('pg-copy-link').click();
  await sleep(50);
  check(copied === 'https://msx.test/playground/#example=stroke_styles', 'copy link gives the address of the loaded example');
}

// 10. No manifest: the page still works.
{
  const { d } = await open('https://msx.test/playground/#example=blend_modes', { manifestFails: true });
  check(d.getElementById('pg-example').textContent.includes('Examples unavailable'), 'a missing manifest says so');
  check(d.getElementById('pg-editor').value.includes('r = 100'), 'and the editor keeps working');
}

console.log(failures ? `\n${failures} check(s) failed` : '\nall checks passed');
process.exit(failures ? 1 : 0);
