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

const { takeFirst, wardWho, wardAged, stateSentence } = new Function(
  [grab('takeFirst'), grab('wardWho'), grab('wardAged'), grab('stateSentence'),
   'return { takeFirst, wardWho, wardAged, stateSentence };'].join('\n'))();

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

// ── what the chain says happened to her, in a sentence ───────────────────────
// "handed over. her chain is 2 shifts long and she is went home." The state is a word from the
// chain — went_home, died, on_ward — and a sentence needs the verb that word already contains.
const her = { s: 'she', o: 'her', p: 'her' };
const him = { s: 'he', o: 'him', p: 'his' };
assert.equal(stateSentence('went_home', her), 'she went home');
assert.equal(stateSentence('died', him), 'he died');
assert.equal(stateSentence('on_ward', her), 'she is on the ward',
             'still here is a state rather than an ending, and reads like one');
assert.equal(stateSentence('', her), 'she is on the ward',
             'an unknown word is not an ending either — the chain says what it says and the page \
does not invent a verb for it');
assert.equal(stateSentence('unrebuildable', him), 'he is on the ward');

console.log('shift_logic: ok (sentences too)');

// ── the case the page draws is the case the payload described ────────────────
//
// Opening a World-case patient showed EP1's name, EP1's questions and no title at all. Every word
// of case content on that page is looked up in the page's own table of the season's sixteen, and a
// compiled case is not in it: `SEASON.find(e=>e.id===sel.value)||SEASON[0]` answered "EP1" and
// everything downstream believed it — the header, the sheet, the six quick questions, the pronoun
// table and the monitor's age limits.
//
// The payload carries the case now (`case_view` on the server) and these two functions are where
// it lands: `wardCard` turns it into the card the bay already knows how to draw, and `epOf` is the
// one place that answers "which case is this" — the ward's, never the shelf's first entry.
const SEASON_FIXTURE = [
  { id: 'ep1', n: 'EP1', t: 'Nine Minutes', who: 'Ing · F 19', tier: 'student' },
  { id: 'ep2', n: 'EP2', t: 'Time Is Muscle', who: 'Somchai · M 71', tier: 'intern' },
];
const { epOf, wardCard } = new Function('SEASON',
  [grab('epOf'), grab('wardCard'), 'return { epOf, wardCard };'].join('\n'))(SEASON_FIXTURE);

// What the server sends about Nusrat Jahan's case, as `case_view` builds it.
const CONTENT = {
  case_id: 'embla-typhoid-bgd-1',
  title: 'Nine days of fever and abdominal pain — a woman of 64',
  presents: 'She has had a fever for nine days and cannot keep water down',
  story: 'A woman of 64 brought in by her son.',
  who: 'Nusrat Jahan · F 64',
  difficulty: 'resident', specialty: 'eir-emergency', care_setting: 'ER',
  setting: 'a district hospital ward', archetype: 'enteric fever',
  chips: {
    ask:   [{ id: 'ask_fever_days', label: 'Ask how long the fever' }],
    exam:  [{ id: 'exam_abdomen', label: 'Examine the abdomen' }],
    lab:   [{ id: 'ix_blood_culture', label: 'Blood culture' }],
    treat: [{ id: 'tx_ceftriaxone', label: 'Ceftriaxone 2 g IV' }],
    dx:    [{ id: 'dx_typhoid', label: 'Name the diagnosis' }],
  },
  voice: { ask_fever_days: 'Nine days now' },
  no_answer: '— she does not answer that, and the case does not say why',
};

const built = wardCard(CONTENT);
assert.equal(built.entry.id, 'embla-typhoid-bgd-1');
assert.equal(built.entry.t, CONTENT.title, 'the headline is the case’s own title');
assert.equal(built.entry.line, CONTENT.presents, 'and the briefing is its presenting line');
assert.equal(built.entry.who, 'Nusrat Jahan · F 64',
             'the card names the person in the bed, which is what ageOf and pro() read');
assert.equal(built.entry.tier, 'resident');

// The tray: the case's own interventions, in the rows the bay's kit already has. `tx_` is what the
// compiler writes and "drugs" is what the tray calls that row.
assert.deepEqual(built.chips.ask, ['ask_fever_days']);
assert.deepEqual(built.chips.exam, ['exam_abdomen']);
assert.deepEqual(built.chips.lab, ['ix_blood_culture']);
assert.deepEqual(built.chips.drug, ['tx_ceftriaxone']);
assert.deepEqual(built.chips.dx, ['dx_typhoid']);
assert.equal(built.chips.proc, undefined,
             'a row this case has nothing in is a row the tray does not draw, rather than an empty \
shelf of somebody else’s procedures');

// What the button fires is the intervention id — the thing the tape records and the rubric pays
// for — and what it *says* is the case's own label.
assert.equal(built.labels.tx_ceftriaxone, 'Ceftriaxone 2 g IV');
assert.equal(built.labels.ask_fever_days, 'Ask how long the fever');
assert.equal(JSON.stringify(built).includes('ep1'), false, 'and nothing of the season is in it');

assert.equal(wardCard(null), null,
             'no content, no card: the page says so rather than drawing another patient');
assert.equal(wardCard({}), null);
assert.equal(wardCard({ case_id: '' }), null);

// ── which case is this ───────────────────────────────────────────────────────
assert.equal(epOf(null, 'ep1').t, 'Nine Minutes', 'the shelf still answers for the season');
assert.equal(epOf(null, 'embla-typhoid-bgd-1').id, 'ep1',
             'and this is the bug, kept here on purpose: an id the shelf does not know falls \
through to its first entry, which is why a World patient wore EP1’s name');
assert.equal(epOf(built.entry, 'embla-typhoid-bgd-1').t, CONTENT.title,
             'so on the ward the card decides, and the shelf is never asked');
assert.equal(epOf(built.entry, '').id, 'embla-typhoid-bgd-1');

console.log('shift_logic: ok (and the case is the payload’s)');
