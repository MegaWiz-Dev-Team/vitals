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

const { takeFirst, wardAged, stateSentence } = new Function(
  [grab('takeFirst'), grab('wardAged'), grab('stateSentence'),
   'return { takeFirst, wardAged, stateSentence };'].join('\n'))();

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

// ── whose age is on the card ─────────────────────────────────────────
// `wardWho` — which retold a season card's "Fon · F 6" as "Park Ji-woo · F 8" — is gone with the
// last thing that drew a season card on the ward. The card is the payload's now and says who is in
// the bed outright (`case_view`'s `who`, from the persona), so there is nothing left to retell:
// see `wardCard` below. What remains is the authored *title*, which carries an age too.

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
// The same card, read on the reviewer's page: the bar says what the run is, and "the ward" over a
// case nobody is in is the wrong half of the truth there.
assert.equal(wardCard(CONTENT, true).entry.n, 'reviewing');
assert.equal(built.entry.n, 'the ward');
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

// ── what the transcript reads when a chip is pressed ─────────────────────────
//
// At a station the ask chips *are* the questions — "any allergies?" is both the button and the
// line — so the transcript writes what was fired and the translated label stays a coat over the
// button: the chart is written in the language the order is written in, and a Thai sentence in it
// would be the page quietly deciding otherwise.
//
// A compiled case's ask chips are not sentences. They are intervention ids — `ask_black_tarry_stool`
// — because that is what the tape records and the rubric pays for. Writing that into the transcript
// puts a database key in the middle of a conversation at a bedside, so on the ward the line is the
// case author's own label for it.
const { askedShown } = new Function(
  [grab('askedShown'), 'return { askedShown };'].join('\n'))();

assert.equal(askedShown('1789554596', 'ask', 'ask_black_tarry_stool', 'Ask: Black tarry stool'),
             'Ask: Black tarry stool',
             'on the ward the transcript reads the question in the case’s own words');
assert.equal(askedShown(null, 'ask', 'any allergies?', 'แพ้ยาอะไรไหม'), null,
             'in the bay the chart stays the language the order is written in — the chip’s \
translation is a coat over the button and nothing else');
assert.equal(askedShown(null, 'drug', 'adrenaline IM', 'adrenaline IM'), 'adrenaline IM',
             'and an order always says what it was');
assert.equal(askedShown('1789554596', 'dx', 'dx_typhoid', 'Name the diagnosis'), 'Name the diagnosis');

console.log('shift_logic: ok (and the transcript reads like a conversation)');

// ── the strip's own sentence ─────────────────────────────────────────────────
//
// Which bed she is in belongs in it: "bed 3" is how a person at a ward says which patient they
// mean, and the board already prints it in her row. The number comes from the board and not from
// the shift payload on purpose — the payload would have to recompute it from every patient on the
// ward to get the same answer, and two independently computed bed numbers is how a page comes to
// disagree with the board it is standing next to.
//
// A ward that does not know the bed says the rest of the sentence rather than "bed null".
const { shiftLine } = new Function([grab('shiftLine'), 'return { shiftLine };'].join('\n'))();

// Short: the button under her face carries the instruction now, and this line is the context beside
// it. It used to be one sentence ending in "take the shift to treat her", and a stranger read past
// all of it looking for something to press.
assert.equal(shiftLine(3, 1, 0, her), 'bed 3 \u00b7 shift 1 \u00b7 chart rebuilt from 0 anchored shifts');
assert.equal(shiftLine(null, 1, 0, her), 'shift 1 \u00b7 chart rebuilt from 0 anchored shifts',
  'a ward that does not know the bed says the rest of it rather than "bed null"');
assert.equal(shiftLine(0, 1, 0, her).startsWith('shift'), true, 'and bed zero is not a bed');
assert.equal(shiftLine(2, 4, 1, him), 'bed 2 \u00b7 shift 4 \u00b7 chart rebuilt from 1 anchored shift',
  'one shift is one shift');
assert.ok(shiftLine(2, 4, 1, him).split(' ').length <= 12,
  'twelve words is the limit for anything a stranger reads to decide a press');

console.log('shift_logic: ok (and the strip says which bed)');

// ── the one button that ends a shift ─────────────────────────────────────────
//
// On the ward it says "hand over" in the markup, and then the bay's own code renames it. Press it
// once: it arms, and the line under it becomes "The case plays out from where you leave it" — the
// season's warning, about a station, on a public ward. Six seconds later `disarmEnd` puts back what
// it thinks the label is, and the button reads "I have finished", which is not a thing a stranger
// at a bed has done or can do.
//
// The line under it was worse: served in the markup as "writes it to her chain. She stays on the
// ward" — over whoever is actually in the bed.
const { endWords } = new Function([grab('endWords'), 'return { endWords };'].join('\n'))();

assert.equal(endWords(null, false, her), null, 'in the bay the words are the pack’s, untouched');
assert.equal(endWords(null, true, her), null);

const rest = endWords('1789554596', false, her);
assert.equal(rest.label, 'hand over');
assert.match(rest.note, /writes it to her chain/, 'the patient’s own pronoun, not the sentence’s');
assert.match(rest.note, /^Ends your shift/);
assert.match(rest.note, /She stays on the ward/, 'and capitalised where the sentence starts');
assert.equal(rest.note.includes('I have finished'), false);

const armed = endWords('1789554596', true, him);
assert.match(armed.label, /hand over/, 'the second press is the same act, said again');
assert.equal(armed.label.includes('end'), false, '"end" is the station’s word: a stay does not end here');
assert.match(armed.note, /his chain/);
assert.equal(armed.note.includes('her'), false, 'the ward admits men');

// Nothing of the station in either.
for (const w of [rest, armed]) {
  for (const season of ['the attempt', 'the case plays out', 'marks are computed', 'station']) {
    assert.equal(w.note.toLowerCase().includes(season), false,
                 `the ward's own button says ${season!==undefined?JSON.stringify(season):''}: ${w.note}`);
  }
}

console.log('shift_logic: ok (and the hand-over button is the ward’s own)');

// ── the one button the page is asking you to press ───────────────────────────
//
// The founder, on staging: "ปุ่มเข้ารักษาคนไข้มันไม่ค่อยเด่น" — the button that puts you at the
// bedside is not prominent. It was a small control at the right end of a grey strip, the same size
// and colour as "← the globe" beside it.
//
// So the patient block carries one primary button and it says what pressing it does, in the
// patient's own words: take her, then hand her back. `primaryLabel` is that sentence, and it is a
// function of the two facts that decide it — whether the head is yours, and whether the engine has
// finished with her.
const { primaryLabel } = new Function([grab('primaryLabel'), 'return { primaryLabel };'].join('\n'))();

assert.equal(primaryLabel(false, false, 'Nusrat Jahan', her),
             'Take the shift · treat Nusrat Jahan',
             'before the head is taken, the page has one thing to ask');
assert.equal(primaryLabel(false, false, '', her),
             'Take the shift · treat her',
             'and a page that does not know her name yet still says what the press does');
assert.equal(primaryLabel(true, false, 'Nusrat Jahan', her),
             'Hand over · record this shift',
             'once it is yours the one action is the one that writes it to the chain');
assert.equal(primaryLabel(true, true, 'Rafael Moreira', him),
             'Hand over · record this shift',
             'and a finished shift is exactly the one with something left to do');
assert.equal(primaryLabel(true, false, 'Rafael Moreira', him).includes('her'),
             false, 'the ward admits men');

console.log('shift_logic: ok (and the page has one button)');

// ── what a chip says ─────────────────────────────────────────────────────────
//
// The compiler writes a label for the row it belongs to: "Ask: Prolonged stepwise fever for 3
// weeks", "Examine: Abdominal examination". Under a tab that already says ASK, eleven to twenty
// chips each beginning "Ask:" is a column of the same word (director, 17 ก.ย., B4) — and at the
// sizes a learner reads them in, the first three words are what they see.
//
// The prefix comes off the *display* and nothing else: `data-x` is the intervention id, and what
// fires, lands on the tape and is marked is untouched.
const { chipText } = new Function([grab('chipText'), 'return { chipText };'].join('\n'))();

assert.equal(chipText('Ask: Prolonged stepwise fever for 3 weeks'), 'Prolonged stepwise fever for 3 weeks');
assert.equal(chipText('Examine: Abdominal examination'), 'Abdominal examination');
assert.equal(chipText('Ask about the shoulder tip pain'), 'Ask about the shoulder tip pain',
             'only the compiler’s own prefix, not any sentence that starts with the word');
assert.equal(chipText('Crystalloid bolus, reassessed'), 'Crystalloid bolus, reassessed');
assert.equal(chipText('ask: black tarry stool'), 'black tarry stool', 'however it was cased');
assert.equal(chipText(''), '');
assert.equal(chipText('Ask:'), 'Ask:', 'a label that is only the prefix keeps it rather than vanishing');

console.log('shift_logic: ok (and a chip says the question)');

// ── the strip is on the page before the page waits for anything ──────────────
//
// `#wardbar` is the only way back to the globe a shift page has, and — until the case lands — the
// only thing on it a stranger can act on at all. It was built after `await identity()`, so on a
// ward that answers slowly the page was a header over nothing, with no way out of it: staging
// blocked for 49 seconds on 17 ก.ย. while the chain was read, and a scrape of /ward/<id> taken in
// that window found the bay's shell and no strip. So the order is the assertion — the strip is
// painted first, and every await on the page happens behind it.
//
// This reads the shipped source rather than a DOM, because the property is an ordering one: a
// `wardBar()` that has moved below an `await` is the bug, however the page renders afterwards.
for (const name of ['openShift', 'openReview']) {
  const body = grab(name);
  const bar = body.indexOf('wardBar()');
  const wait = body.indexOf('await');
  assert.ok(bar >= 0, `${name} no longer paints the strip at all`);
  assert.ok(wait < 0 || bar < wait,
            `${name} waits before it paints the strip — a page with no way back until a fetch returns`);
}

console.log('shift_logic: ok (and the way back is painted before the waiting)');
