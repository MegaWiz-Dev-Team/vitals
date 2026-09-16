// The ward shift's own small decisions, pulled out of the page that ships and run here.
//
// Three of them, and every one is a bug the founder found in a browser on 16 ก.ย.:
//
//   * a question pressed before the head is taken was accepted, written into the transcript at
//     0:00 and answered by nobody — the page has to refuse it, and say why
//   * the card called Park Ji-woo "F 6" while the ward, the board and the rail said 8: the case's
//     authored age was leaking into a bed it does not belong to
//   * and the page had no word for "you have not taken this shift yet", so the disabled ask bar
//     looked like a broken ask bar
//
// Pulled by brace matching from `bay.js` — the source that ships, never a paraphrase — so a
// rename fails here loudly instead of testing a copy nobody runs.
//
//   node shift_logic.mjs <bay.js>

import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const script = readFileSync(process.argv[2], 'utf8');

function grab(name) {
  for (const head of [`function ${name}(`, `const ${name}=`, `let ${name}=`]) {
    const i = script.indexOf(head);
    if (i < 0) continue;
    let depth = 0, started = false;
    for (let k = i; k < script.length; k++) {
      const c = script[k];
      if (c === '{') { depth++; started = true; }
      else if (c === '}') { depth--; if (started && depth === 0) return script.slice(i, k + 1); }
    }
  }
  throw new Error(`${name} is not in the page any more — renamed, or deleted with its test left behind`);
}

const { takeFirst, wardWho, wardAged } = new Function(
  [grab('takeFirst'), grab('wardWho'), grab('wardAged'),
   'return { takeFirst, wardWho, wardAged };'].join('\n'))();

// ── the sentence, and when there is one ──────────────────────────────────────
assert.equal(takeFirst(null, null, 'her'), null, 'the Eternal bay is not a ward and is never gated');
assert.equal(takeFirst(null, 'run-7', 'her'), null);
assert.equal(takeFirst('1789528326', null, 'her'), 'take the shift to treat her',
             'on the ward with no head taken, every control has to say this rather than sit inert');
assert.equal(takeFirst('1789528327', null, 'him'), 'take the shift to treat him',
             'and the ward admits men — the pronoun is the patient\u2019s, never the sentence\u2019s');
assert.equal(takeFirst('1789528326', null, null), 'take the shift to treat the patient',
             'a page that does not yet know who is in the bed says so rather than guessing');
assert.equal(takeFirst('1789528326', 'run-7', 'her'), null,
             'and once the head is theirs the page gets out of the way');

// ── whose age is on the card ─────────────────────────────────────────────────
// The door refuses a pack whose sex or band contradicts the case, so only the number moves.
assert.equal(wardWho('Fon · F 6', 'Park Ji-woo', 8), 'Park Ji-woo · F 8',
             'the person in the bed is the ward’s, and so is her age');
assert.equal(wardWho('Somchai · M 71', 'Rafael Moreira', 63), 'Rafael Moreira · M 63');
assert.equal(wardWho('Fon · F 6', 'Park Ji-woo', null), 'Park Ji-woo · F 6',
             'a ward that does not know her age leaves the case’s alone rather than inventing one');
assert.equal(wardWho('Fon · F 6', null, 8), 'Fon · F 8');
assert.equal(wardWho('', 'Park Ji-woo', 8), 'Park Ji-woo', 'no case line, no invented one');

// ── and in the authored title, which carries it too ──────────────────────────
assert.equal(wardAged('Barking cough and drooling, worse at night — F 6', 8),
             'Barking cough and drooling, worse at night — F 8');
assert.equal(wardAged('Chest pain to the jaw — M 58', 63), 'Chest pain to the jaw — M 63');
assert.equal(wardAged('A barking cough — F 6', null), 'A barking cough — F 6', 'no age, no rewrite');
assert.equal(wardAged('F 6 and F 7 in one line', 8), 'F 8 and F 8 in one line',
             'every age in the string, because one left behind is the contradiction we are fixing');
assert.equal(wardAged('Fever 39 for 2 days', 8), 'Fever 39 for 2 days',
             'a number that is not an age is not touched');

console.log('shift_logic: ok');
