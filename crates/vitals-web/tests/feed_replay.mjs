// Replay a recorded run through the page's own feed code and print the transcript.
//
// The transcript is the encounter feed — the column a candidate reads while the case runs and
// the examiner reads afterwards. It is built entirely in the browser, out of two things the
// server never sees together: the beats it sends, and the order lines the page writes the
// moment a candidate presses send. Nothing on the server can assert about it.
//
// So this pulls the real functions out of index.html — by name, by brace matching, never a
// paraphrase — evaluates them against a DOM small enough to be obviously correct, and replays a
// captured sequence of `/api/step` replies. Two builds replayed this way produce two
// transcripts, and those can be compared.
//
//   node feed_replay.mjs <index.html> <run.json>
//
// `run.json` is `[{order|ask|null, v}]`: what the candidate did, and the reply that came back.

import { readFileSync } from 'node:fs';

const [pagePath, runPath, mode] = process.argv.slice(2);
const EXAM = mode !== 'practice';   // a station is an exam by definition; an episode is not
const html = readFileSync(pagePath, 'utf8');
// The page opens with a small boot block; the feed lives in the second and much larger one.
// Taking the longest rather than the first, so a third block added tomorrow changes nothing.
const script = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)]
  .map(m => m[1]).sort((a, b) => b.length - a.length)[0];

// ── pulling a function out of the page ──────────────────────────────────────
// Brace matching from the header, so what runs here is the source that ships. A rename or a
// deletion fails loudly rather than silently testing nothing.
function grab(name) {
  for (const head of [`function ${name}(`, `const ${name}=`, `let ${name}=`]) {
    const i = script.indexOf(head);
    if (i < 0) continue;
    // A scanner rather than a regex: half these are arrows whose bodies are template literals
    // full of `${...}`, and counting raw braces reads those as blocks and truncates the source.
    let depth = 0, started = false;
    for (let k = i; k < script.length; k++) {
      const c = script[k];
      if (c === '/' && script[k + 1] === '/') { while (k < script.length && script[k] !== '\n') k++; continue; }
      if (c === '/' && script[k + 1] === '*') { k = script.indexOf('*/', k) + 1; continue; }
      if (c === '"' || c === "'") { const q = c; for (k++; k < script.length && script[k] !== q; k++) if (script[k] === '\\') k++; continue; }
      if (c === '`') {                                  // template: only `${` opens real code
        for (k++; k < script.length && script[k] !== '`'; k++) {
          if (script[k] === '\\') { k++; continue; }
          if (script[k] === '$' && script[k + 1] === '{') { let d = 1; k += 2;
            while (k < script.length && d) { if (script[k] === '{') d++; else if (script[k] === '}') d--; k++; } k--; }
        }
        continue;
      }
      if (c === '{') { depth++; started = true; }
      else if (c === '}') { if (--depth === 0 && started && head.startsWith('function')) return script.slice(i, k + 1); }
      else if (c === '(' || c === '[') depth++;
      else if (c === ')' || c === ']') depth--;
      else if (c === ';' && depth === 0 && !head.startsWith('function')) return script.slice(i, k + 1);
    }
  }
  throw new Error(`${name} is not in the page any more`);
}

// ── a DOM with just enough in it ────────────────────────────────────────────
class El {
  constructor(tag) {
    this.tag = tag; this.className = ''; this.children = []; this.parentNode = null;
    this.title = ''; this._text = '';
    // A real `dataset` stores strings whatever you assign it, and `[data-bn="4"]` is matched
    // against that string. Storing the number here would make the page look broken when it is
    // the stand-in that is wrong.
    this.dataset = new Proxy({}, { set: (o, k, v) => { o[k] = String(v); return true; } });
  }
  set textContent(t) { this._text = t; }
  get textContent() { return this._text; }
  set innerHTML(h) {
    this.children = [];
    for (const m of h.matchAll(/<span class="([a-z-]+)">([^<]*)<\/span>/g)) {
      const s = new El('span'); s.className = m[1]; s._text = m[2]; s.parentNode = this;
      this.children.push(s);
    }
  }
  get classList() {
    const self = this;
    return {
      add: c => { if (!self.className.split(' ').includes(c)) self.className += ' ' + c; },
      remove: c => { self.className = self.className.split(' ').filter(x => x && x !== c).join(' '); },
      contains: c => self.className.split(' ').includes(c),
    };
  }
  appendChild(n) { n.parentNode = this; this.children.push(n); return n; }
  insertBefore(n, ref) {
    if (!ref) return this.appendChild(n);
    const i = this.children.indexOf(ref);
    n.parentNode = this; this.children.splice(i < 0 ? this.children.length : i, 0, n); return n;
  }
  get lastElementChild() { return this.children[this.children.length - 1] || null; }
  get nextSibling() {
    if (!this.parentNode) return null;
    return this.parentNode.children[this.parentNode.children.indexOf(this) + 1] || null;
  }
  matches(sel) {
    const m = sel.match(/^\.ev\[data-bn="(\d+)"\]$/);
    if (m) return this.className.split(' ').includes('ev') && this.dataset.bn === m[1];
    return sel.split('.').filter(Boolean).every(c => this.className.split(' ').includes(c));
  }
  querySelector(sel) {
    if (sel.includes('[')) return this.children.find(x => x.matches(sel)) || null;
    const c = sel.replace(/^\./, '');
    return this.children.find(x => x.className.split(' ').includes(c)) || null;
  }
  querySelectorAll(sel) { return this.children.filter(x => x.matches(sel)); }
}

const chat = new El('div');
globalThis.document = {
  createElement: t => new El(t),
  querySelector: sel => (sel.startsWith('#chat ') ? chat.querySelectorAll(sel.slice(6))[0] || null : chat),
  querySelectorAll: sel => (sel.startsWith('#chat ') ? chat.querySelectorAll(sel.slice(6)) : []),
};
globalThis.$ = () => chat;

// ── the page's own code, and the smallest surround it needs ────────────────
globalThis.TR = {};
globalThis.fillPro = x => x;
globalThis.examMode = () => EXAM;
globalThis.wake = () => {};
globalThis.seen = 0;
globalThis.CHARTSEEN = 0;
globalThis.PAINTS = [];
globalThis.clockNow = 0;
globalThis.HARM_SEALED = '⚠ harm recorded';   // the old page's redaction; unused by the new one

const src = ['SAY', 'say', 'fmt', 'BEAT_DECLINED', 'BEAT_NOTED', 'ev', 'drainBeats', 'unsealHarm', 'unsealBeats']
  .map(n => { try { return grab(n); } catch { return ''; } })
  .filter(Boolean);
try { src.unshift(grab('visBeats')); } catch { /* the old page has no such thing */ }

// `let`/`const` at eval top level would not be visible to the functions, so everything is
// hoisted onto the global object by hand.
const decl = src.join('\n').replace(/^(const|let)\s+/gm, 'globalThis.').replace(/^function (\w+)/gm, 'globalThis.$1 = function $1');
(0, eval)(decl);

// ── the replay ─────────────────────────────────────────────────────────────
let over = false;
for (const step of JSON.parse(readFileSync(runPath, 'utf8'))) {
  if (step.order) globalThis.ev('order', '▸', step.order);
  const v = step.v;
  globalThis.clockNow = v.elapsed;
  const ended = over || !!v.outcome;
  globalThis.drainBeats(v, ended, false);
  if (v.outcome && !over) { over = true; globalThis.unsealHarm(v); globalThis.unsealBeats(v); }
}

// className is normalised: `sealed` is a transient the bell clears, and the old page and the
// new one arrive at the same line by different routes.
const line = n => ({
  kind: n.className.split(' ').filter(c => c && c !== 'ev' && c !== 'sealed').join(' '),
  t: (n.querySelector('.t') || {}).textContent || '',
  text: (n.querySelector('.tx') || {}).textContent || '',
  title: n.title || '',
});
console.log(JSON.stringify(chat.children.map(line), null, 1));
