// A sentence the page writes about a patient takes the patient's pronoun from the board — never a
// literal one. On 25 Sep 2026 the front-page card said "the ward will close her" about a
// seven-year-old boy: the server's plain-words gate refuses a fixed "she" in ward.rs and
// ward_chain.rs, and nothing covered index.html, the one place a stranger reads the sentence.
//
// The rule is narrow on purpose. Prose and comments may say "her". What may not is a *generated
// sentence* — a template literal with an interpolation in it — that also carries a bare pronoun
// of its own: a sentence with a variable in it is written about somebody the page does not know
// in advance, and its pronoun comes from theirs()/pro() or it fails here. (The first draft looked
// only for templates naming the patient, and missed the very card this is for: "the ward will
// close her in about ${when}" names no patient, only a time.)
//
// What it misses, written here rather than assumed away: a sentence assembled in pieces across
// statements, and a pronoun in a plain quoted string concatenated onto a template (`...` + '... she
// ...'). Founding case checked both ways: index.html at 50ac124 (the card before the fix) fails
// on two lines; at 741824f it passes. First run also found bay.js:2524, the mark sheet's head —
// "a station where she dies" — which a male ward patient reads about himself.
//
//   node crates/vitals-web/tests/world/patient_words.mjs crates/vitals-web/static/world/index.html crates/vitals-web/static/bay.js
import { readFileSync } from "node:fs";

const files = process.argv.slice(2);
if (!files.length) { console.error("usage: patient_words.mjs <page.html|script.js> ..."); process.exit(2); }

const HAS_AN_INTERPOLATION = /\$\{[^}]*\}/;
const BARE_PRONOUN = /(^|[^\w${.])(she|he|her|him|hers|his)([^\w}]|$)/i;

let bad = 0;
for (const f of files) {
  const src = readFileSync(f, "utf8");
  // every template literal, with its line number; nested ${} are walked as text
  const re = /`((?:[^`\\]|\\.)*)`/g;
  let m;
  while ((m = re.exec(src))) {
    const lit = m[1];
    if (!HAS_AN_INTERPOLATION.test(lit)) continue;
    // strip the interpolations before looking for a bare pronoun: `${g.her}` is the right way
    const text = lit.replace(/\$\{[^}]*\}/g, " ");
    const hit = BARE_PRONOUN.exec(text);
    if (hit) {
      const line = src.slice(0, m.index).split("\n").length;
      bad++;
      console.log(`  FAIL  ${f}:${line} a template about a patient carries a fixed "${hit[2]}": ${text.trim().replace(/\s+/g, " ").slice(0, 100)}`);
    }
  }
}
if (bad) { console.log(`${bad} template(s) write a pronoun the board did not give`); process.exit(1); }
console.log(`patient words: every template about a patient takes its pronoun from the board (${files.length} file(s))`);
