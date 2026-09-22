// The review form, built the way a browser builds it, and read back as data.
//
// `review.html` is one inline script that reads `location.search`, picks a role or an addressed
// instance, and writes the question cards into the page. Nothing type-checks it, and the thing
// that matters most about an addressed instance — *which* questions it puts in front of *which*
// reviewer — is decided at runtime by that script. So the script runs here, against a document
// small enough to read in one sitting, and prints what it built:
//
//   node review_logic.mjs <review.html> <for-id or ''> [role to click]
//
// Prints one JSON object: whether the role picker is hidden, the greeting, the title, which role
// button is pressed, the group headings and the question ids in the order they were built.
// `tests/page.rs` asserts on it; this file decides nothing.

import { readFileSync } from 'node:fs';

const [, , htmlPath, forId = '', click = '', lang = 'th'] = process.argv;
const html = readFileSync(htmlPath, 'utf8');

// ── a document, just large enough ───────────────────────────────────────────────────────────

class El {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.attrs = {};
    this.dataset = {};
    this.listeners = {};
    this.parentNode = null;
    this.hidden = false;
    this.className = '';
    this.value = '';
    this.checked = false;
    this.disabled = false;
    this.style = {};
    this._text = '';
    this._html = '';
  }
  appendChild(c) { c.parentNode = this; this.children.push(c); return c; }
  remove() { if (this.parentNode) this.parentNode.children = this.parentNode.children.filter(x => x !== this); }
  get textContent() { return this._text + this.children.map(c => c.textContent).join(''); }
  set textContent(t) { this._text = String(t); this.children = []; }
  get innerHTML() { return this._html; }
  set innerHTML(h) { this._html = String(h); this.children = []; }
  setAttribute(k, v) { this.attrs[k] = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) { return k in this.attrs ? this.attrs[k] : null; }
  removeAttribute(k) { delete this.attrs[k]; }
  addEventListener(t, f) { (this.listeners[t] ||= []).push(f); }
  click() { (this.listeners.click || []).forEach(f => f({})); }
  scrollIntoView() {}
  focus() {}
  select() {}
  get maxLength() { return this._max; }
  set maxLength(n) { this._max = n; }
  *walk() { for (const c of this.children) { yield c; yield* c.walk(); } }
  querySelectorAll(sel) {
    const parts = sel.trim().split(/\s+/).map(parsePart);
    const out = [];
    for (const e of this.walk()) {
      if (!matches(e, parts[parts.length - 1])) continue;
      // Descendant combinator: every earlier part must match some ancestor, innermost first.
      let ok = true, anc = e.parentNode;
      for (let i = parts.length - 2; i >= 0 && ok; i--) {
        while (anc && anc !== this && !matches(anc, parts[i])) anc = anc.parentNode;
        if (!anc || anc === this) ok = false; else anc = anc.parentNode;
      }
      if (ok) out.push(e);
    }
    return out;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
}

function parsePart(p) {
  const m = p.match(/^([a-z][a-z0-9]*)?((?:\.[\w-]+)*)((?:\[[^\]]+\])*)$/i);
  if (!m) throw new Error(`selector not understood: ${p}`);
  const classes = m[2].split('.').filter(Boolean);
  const attrs = [...m[3].matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)].map(a => [a[1], a[2]]);
  return { tag: (m[1] || '').toUpperCase(), classes, attrs };
}
function matches(e, part) {
  if (part.tag && e.tagName !== part.tag) return false;
  const cls = e.className.split(/\s+/);
  if (!part.classes.every(c => cls.includes(c))) return false;
  for (const [k, v] of part.attrs) {
    const got = k.startsWith('data-') ? e.dataset[k.slice(5)] : e.attrs[k];
    if (got === undefined) return false;
    if (v !== undefined && got !== v) return false;
  }
  return true;
}

// Every element the markup gives an id, by that id, hung flat under one body so a search from
// the document finds what the script appends under any of them.
const body = new El('body');
const byId = {};
for (const m of html.matchAll(/<([a-z0-9]+)\b[^>]*\bid="([^"]+)"[^>]*>/gi)) {
  const e = new El(m[1]);
  e.attrs.id = m[2];
  if (/\bhidden\b/.test(m[0])) e.hidden = true;
  byId[m[2]] = e;
  body.appendChild(e);
}
const meta = new El('meta');
meta.content = 'test-build';   // served by us: the send button posts, and the form id travels
// The root element. The page stamps the reader's language on it, and a document without one is a
// document no browser has ever served — the page should not have to check for it.
const documentElement = new El('html');
const document = {
  documentElement,
  getElementById: id => byId[id] || null,
  createElement: tag => new El(tag),
  querySelector: sel => sel === 'meta[name="build"]' ? meta : body.querySelector(sel),
  querySelectorAll: sel => body.querySelectorAll(sel),
  body,
  execCommand: () => true,
};
// Nothing pre-seeded. The language comes from `navigator` below, the way it does for a reader who
// has never touched the switch, and this store starts empty so that a test asking what the form
// *writes* to a browser sees only what the form wrote.
const store = {};
const localStorage = {
  getItem: k => (k in store ? store[k] : null),
  setItem: (k, v) => { store[k] = String(v); },
  removeItem: k => { delete store[k]; },
};
const window = { scrollTo() {}, claude: undefined };
const location = { search: forId ? `?for=${forId}` : '' };
const navigator = { language: lang === 'en' ? 'en-GB' : 'th-TH',
                    languages: [lang === 'en' ? 'en-GB' : 'th-TH'] };
const URL = { createObjectURL: () => 'blob:', revokeObjectURL() {} };
class Blob { constructor() {} }
const fetch = () => Promise.reject(new Error('not here'));

// ── run the page's script ───────────────────────────────────────────────────────────────────

const a = html.indexOf('<script>') + '<script>'.length;
const b = html.indexOf('</script>', a);
const script = html.slice(a, b);
new Function('document', 'window', 'localStorage', 'location', 'navigator', 'URL', 'Blob', 'fetch',
             'setTimeout', script)(document, window, localStorage, location, navigator, URL, Blob,
             fetch, () => {});

if (click) byId[`r-${click}`].click();

// ── what it built ───────────────────────────────────────────────────────────────────────────

const questions = byId.questions;
const out = {
  pick_hidden: byId.pick.hidden,
  greeting: byId.addressed.hidden ? '' : byId.addressed.textContent,
  title: byId.title.textContent,
  pressed: ['student', 'physician'].filter(r => byId[`r-${r}`].getAttribute('aria-pressed') === 'true'),
  groups: questions.querySelectorAll('.grp h2').map(h => h.textContent),
  ids: questions.querySelectorAll('textarea[data-q]').map(t => t.dataset.q),
  optionless: questions.querySelectorAll('section.q')
    .filter(c => !c.querySelector('.opts'))
    .map(c => c.querySelector('textarea[data-q]').dataset.q),
  // English-against-Thai rows per card, in order — the language review's whole content.
  rows: questions.querySelectorAll('section.q').map(c => c.querySelectorAll('.lines tr').length),
  // The three labels the script writes onto every card. They are not elements in the markup, so
  // they have no Thai to be read off the page — and on 22 ก.ย. that fallback fell through to the
  // English string and headed every card of the Thai form in English. Reported here so the rule
  // that catches it is a rule about these three labels rather than a coincidence.
  ctx_labels: [...new Set(questions.querySelectorAll('.ctx dt').map(d => d.textContent))],
  draft_keys: Object.keys(store),
};
process.stdout.write(JSON.stringify(out));
