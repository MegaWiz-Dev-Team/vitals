// **A sentence a stranger reads in order to press something is twelve words or fewer.**
//
// UX review, 17 ก.ย., G3: the pages are written in long literary sentences — "Nothing here is this
// server's word for it", "a stay that ended is not one anybody can add to". That reads well to the
// engineer who wrote it and badly to a medical student in their second language deciding what to
// do in three seconds. The rule the director took from it, and asked for as a test: **every string
// a user must read to decide a press — the words on a control, the strip's own sentences, and the
// refusal that stands between them and the control — is at most twelve words.**
//
// Three collections, all read out of what the server actually serves:
//
//   * the words on a control: every <button> and every .btn link, in the markup and in the markup
//     the script writes;
//   * the strip: everything `wardSay` puts in front of a stranger at the bedside;
//   * the gate: the sentence `takeFirst` hands to every control that is closed until the head is
//     taken, which is the one a stranger reads *because* a press did nothing.
//
// Not in scope, deliberately: prose that explains rather than directs — the receipt's account of
// why a hash resolves to nothing, the policy notes on the globe, the footers. A reader meets those
// with nothing in their hand to press. The rule is about the moment of pressing.
//
//   node press_words.mjs <bay.js> <bay-surface.html> [more pages…]

import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const LIMIT = 12;
const files = process.argv.slice(2);
assert.ok(files.length >= 2, 'usage: press_words.mjs <bay.js> <bay-surface.html> [pages…]');

/** Source with its prose taken out, so a comment about a sentence is not read as one. */
function uncommented(src) {
  let out = '', i = 0;
  while (i < src.length) {
    if (src.startsWith('//', i) && src[i - 1] !== ':') {
      const j = src.indexOf('\n', i); i = j < 0 ? src.length : j;
    } else if (src.startsWith('/*', i)) {
      const j = src.indexOf('*/', i); i = j < 0 ? src.length : j + 2;
    } else if (src.startsWith('<!--', i)) {
      const j = src.indexOf('-->', i); i = j < 0 ? src.length : j + 3;
    } else { out += src[i]; i++; }
  }
  return out;
}

/** What a reader sees of a JavaScript expression: its literals, with every value as one word. */
function asRead(expr) {
  let out = '', i = 0;
  while (i < expr.length) {
    const c = expr[i];
    if (c === "'" || c === '"' || c === '`') {
      let j = i + 1, buf = '';
      while (j < expr.length) {
        if (expr[j] === '\\') { buf += expr[j + 1]; j += 2; continue; }
        if (expr[j] === c) break;
        buf += expr[j]; j++;
      }
      // A template's own holes are values too.
      out += buf.replace(/\$\{[^}]*\}/g, ' value '); i = j + 1;
    } else {
      let k = i;
      while (k < expr.length && !"'\"`".includes(expr[k])) k++;
      if (expr.slice(i, k).replace(/[\s+]/g, '')) out += ' value ';
      i = k;
    }
  }
  return out;
}

/** Every call of `name(…)`, as the words its argument puts on the screen. */
function callsOf(src, name) {
  const out = [];
  const re = new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\\(', 'g');
  let m;
  while ((m = re.exec(src))) {
    let i = m.index + m[0].length, depth = 1, j = i;
    while (j < src.length && depth) {
      if (src[j] === '(') depth++;
      else if (src[j] === ')') depth--;
      j++;
    }
    out.push({ at: m.index, text: asRead(src.slice(i, j - 1)) });
  }
  return out;
}

// A word is a thing with a letter or a digit in it. An em dash between two clauses is
// punctuation, and a reader does not read it as a word.
const words = t => t.trim().split(/\s+/).filter(w => /[\p{L}\p{N}]/u.test(w)).length;

/** A string as its sentences: tags out, one line, split where a reader stops.
 *
 * A tag boundary is one of those places. "…she went home.<a>the receipt</a> · <a>back to the
 * globe</a>" is a sentence and two links, and reading it as one long sentence is reading the
 * markup rather than the page. */
function sentences(t) {
  return String(t)
    .replace(/<[^>]*>/g, ' \u00b6 ')
    .replace(/&[a-z]+;/g, ' ')
    .replace(/\s+/g, ' ')
    // One value is one word to a reader, however many expressions the page joined to make it:
    // "shift 2 of 3" is built out of five pieces and read as four words.
    .replace(/(?:\bvalue\b[\s.,;:'’—-]*){2,}/g, 'value ')
    .split(/\u00b6|(?<=[.!?;])\s+/)
    .map(s => s.trim())
    .filter(Boolean);
}

const tooLong = [];
const check = (where, text) => {
  for (const s of sentences(text)) {
    if (words(s) > LIMIT) tooLong.push(`${where}: ${words(s)} words — ${JSON.stringify(s)}`);
  }
};

const sources = files.map(f => ({ name: f.split('/').pop(), src: uncommented(readFileSync(f, 'utf8')) }));

// ── the words on a control ───────────────────────────────────────────────────
let controls = 0;
for (const { name, src } of sources) {
  for (const m of src.matchAll(/<button[^>]*>([\s\S]{0,200}?)<\/button>/g)) {
    controls++; check(`${name} button`, asReadIfCode(m[1]));
  }
  for (const m of src.matchAll(/<a[^>]*class=["'][^"']*\bbtn\b[^"']*["'][^>]*>([\s\S]{0,200}?)<\/a>/g)) {
    controls++; check(`${name} link-button`, asReadIfCode(m[1]));
  }
}
// A label written inside a script is a string with holes in it; one in markup is plain text.
function asReadIfCode(label) {
  return /['"`]/.test(label) ? asRead(label) : label.replace(/\$\{[^}]*\}/g, ' value ');
}
assert.ok(controls > 20, `the scan found only ${controls} controls — the markup has changed shape`);

// ── the strip ────────────────────────────────────────────────────────────────
const said = callsOf(sources[0].src, 'wardSay');
assert.ok(said.length > 8, `only ${said.length} strip sentences found — has wardSay been renamed?`);
for (const s of said) check('the strip', s.text);

// ── the gate ─────────────────────────────────────────────────────────────────
const gate = sources[0].src.slice(sources[0].src.indexOf('function takeFirst('));
check('the gate', asRead(gate.slice(0, gate.indexOf('\n}'))));

if (tooLong.length) {
  console.error('sentences a stranger reads to decide a press, over ' + LIMIT + ' words:\n  ' +
                tooLong.join('\n  '));
  assert.fail(`${tooLong.length} of them`);
}
console.log(`press_words: ok (${controls} controls, ${said.length} strip sentences, none over ${LIMIT} words)`);
