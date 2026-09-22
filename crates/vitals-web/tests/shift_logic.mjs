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
assert.equal(rest.label, 'hand over (press twice)',
             'the two-press latch is a property of the button and the button says so: a stranger \
              who presses once and walks away has recorded nothing, and nothing told them');

assert.match(rest.note, /writes it to her chain/, 'the patient’s own pronoun, not the sentence’s');
assert.match(rest.note, /^Ends your shift/);
assert.match(rest.note, /She stays on the ward/, 'and capitalised where the sentence starts');
assert.equal(rest.note.includes('I have finished'), false);

const armed = endWords('1789554596', true, him);
assert.equal(armed.label, 'press again to record',
             'and the armed label names what the second press achieves. The rule this replaces \
              asked for the same verb twice, so that nobody read the second press as a different \
              act; "record" is the same act named by its consequence, and it is the word \
              `primaryLabel` already uses for it.');
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

// The two buttons that hand over share the word the armed strip button uses, so a stranger who
// reads "press again to record" does not read it as a different act from "hand over".
assert.ok(/record/i.test(primaryLabel(true, false, 'Nusrat Jahan', her)));
assert.ok(/record/i.test(endWords('1789554596', true, her).label));

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
// Comments are stripped first: the sentence above `wardBar()` explains what it must not sit
// behind, and the word it explains would otherwise be the first `await` in the function.
const uncommented = src => src.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/(^|[^:])\/\/[^\n]*/g, '$1');
for (const name of ['openShift', 'openReview']) {
  const body = uncommented(grab(name));
  const bar = body.indexOf('wardBar()');
  const wait = body.indexOf('await');
  assert.ok(bar >= 0, `${name} no longer paints the strip at all`);
  assert.ok(wait < 0 || bar < wait,
            `${name} waits before it paints the strip — a page with no way back until a fetch returns`);
}

console.log('shift_logic: ok (and the way back is painted before the waiting)');

// ── the two ways out of a shift, named by what they do ───────────────────────
//
// UX review C4: "hand her back" and "hand over" are one word apart and opposite in consequence —
// one writes the shift to the chain under the stranger's key, the other throws it away. A learner
// who reads the wrong one loses everything they just did to a patient, and there is nothing on the
// page to get it back with.
//
// So the exits are named by what they do, not by what happens to the patient: "Hand over · record
// this shift" and "Leave without recording". The leave asks once before it acts, in the page's own
// press-again idiom — the same one the end button has used all along, rather than a browser
// dialogue a stranger dismisses without reading.
const { leaveWords } = new Function([grab('leaveWords'), 'return { leaveWords };'].join('\n'))();

assert.equal(leaveWords(false).label, 'Leave without recording',
             'what the button does, in the words of what it does');
assert.equal(leaveWords(false).say, '', 'and nothing said until it is pressed');
assert.equal(leaveWords(true).label, 'press again to leave');
assert.equal(leaveWords(true).say,
             'this throws away everything you did — hand over instead?',
             'asked once, in words, where the strip already speaks — and naming the thing to do \
              instead, because the one stranger who took a shift on opening night pressed this \
              and lost five minutes of work that the chain would have paid for');
// The alternative is named in the question, never on the button: C4 below is about the labels, and
// two buttons that both say "hand" are the bug it exists to prevent.
assert.equal(leaveWords(true).label.toLowerCase().includes('hand'), false);
assert.equal(leaveWords(false).label.toLowerCase().includes('hand'), false);

// The two exits must not read alike. This is the whole of C4: one of them is irreversible and the
// other is a loss, and a stranger has to tell them apart at a glance.
const both = [primaryLabel(true, false, 'Nusrat Jahan', her), leaveWords(false).label];
assert.ok(!/leave/i.test(both[0]), `the recording exit never says leave: ${both[0]}`);
assert.ok(!/hand/i.test(both[1]), `the discarding exit never says hand: ${both[1]}`);
assert.ok(!/her|him|she|he\b/i.test(both[1]),
          `and it names no patient — it is about the shift, not about who is in the bed: ${both[1]}`);

console.log('shift_logic: ok (and the two exits say what they do)');

// ── how long the head is yours ───────────────────────────────────────────────
//
// The program holds a lease for 3,450 slots and the page said "23 minutes", because 3,450 × 0.4 s
// is 23 minutes. Devnet was producing slots at 0.166 s on 17 ก.ย., so the lease was nine and a
// half — which is when the director's abandoned bed freed itself, nine minutes before the page
// said it would.
//
// The ward measures the rate and publishes the minutes; this page repeats them. A ward that has
// not measured says nothing, and so does the page: a sentence with no number in it is better than
// a number that is wrong about the chain it is describing.
const { leaseWords } = new Function([grab('leaseWords'), 'return { leaseWords };'].join('\n'))();

assert.equal(leaseWords(10), 'The lease runs about 10 minutes today.');
assert.equal(leaseWords(23), 'The lease runs about 23 minutes today.');
assert.equal(leaseWords(null), '', 'not measured, not said');
assert.equal(leaseWords(0), '', 'and a zero is a measurement that failed, not a lease of no time');
assert.equal(leaseWords(undefined), '');

console.log('shift_logic: ok (and the lease is the one the chain is keeping today)');

// ── what the clock is counting ───────────────────────────────────────────────
//
// UX review C1: the clock runs and says nothing about what it is measuring. A stranger has no idea
// the head is theirs for a fixed span, or when to hand over — and the span is not cosmetic: the
// program refuses an anchor once the lease has run out (`anchor_shift`: `slot >=
// patient.lease_until_slot` → NotLeaseHolder), so a shift that runs past it cannot be recorded at
// all.
//
// So the line says the time left, and in the last five minutes it says what to do with it. At zero
// it says the truth: the bed is free for the next stranger, and this shift is no longer theirs to
// record.
const { leaseLine } = new Function([grab('leaseLine'), 'return { leaseLine };'].join('\n'))();

assert.equal(leaseLine(1351), 'shift ends in 22:31');
assert.equal(leaseLine(600), 'shift ends in 10:00');
assert.equal(leaseLine(361), 'shift ends in 6:01', 'seconds are padded, minutes are not');
assert.equal(leaseLine(9), 'shift ends in 0:09 — hand over now, or this shift is lost',
             'nine seconds left is inside the last two, where the line says what is at stake \
              rather than what to do: the program refuses an anchor past the lease, so a shift \
              that runs out is work that cannot be recorded at all');

// The last five minutes carry the instruction, because that is when it is actionable.
assert.equal(leaseLine(252), 'shift ends in 4:12 — hand over to record it');
assert.equal(leaseLine(300), 'shift ends in 5:00 — hand over to record it', 'five minutes is inside it');
assert.equal(leaseLine(301), 'shift ends in 5:01', 'and a second more is not');

// And the last two minutes are stronger than the last five, because by then the instruction has
// been on screen for three minutes and has not worked. This is the copy the founder asked for
// after opening night: the only shift taken ran five minutes and was thrown away.
assert.equal(leaseLine(120), 'shift ends in 2:00 — hand over now, or this shift is lost',
             'two minutes is inside it');
assert.equal(leaseLine(121), 'shift ends in 2:01 — hand over to record it',
             'and a second more is still the gentler line');

// Zero and past it: the bed is free and the shift cannot be anchored. Both halves are true and the
// second is the one a stranger needs — their work is not on the chain and will not go there.
assert.equal(leaseLine(0), 'the lease has run out — the bed is free');
assert.equal(leaseLine(-90), 'the lease has run out — the bed is free');
// And a ward that has not told the page how long a lease is says nothing at all.
assert.equal(leaseLine(null), '');
assert.equal(leaseLine(undefined), '');

console.log('shift_logic: ok (and the clock says what it is counting)');

// ── the second press of Hand over ───────────────────────────────────────────
//
// Demo capture, item 2: the founder pressed the bedside "Hand over" twice — the first press
// anchored the shift, the second sent the same leaf at the same head, and the chain refused it as a
// head that had moved. Which it had: we moved it. The page showed a stranger a refusal for the
// work it had just recorded for them.
//
// So the press is a decision the page can make on its own, and it is made here: while a hand-over
// is in flight or finished, a press does nothing at all. The bedside button and the strip's button
// are the same act, so the latch is not the button's own state — it is the shift's.
const { pressPrimary } = new Function([grab('pressPrimary'), 'return { pressPrimary };'].join('\n'))();

assert.equal(pressPrimary(false, false), 'take', 'no head taken: the one thing to ask for');
assert.equal(pressPrimary(true, false), 'handover', 'the head is theirs: the one act left');
assert.equal(pressPrimary(true, true), 'nothing',
             'and the second press of a hand-over already under way does nothing — that press is \
              the one that earned a refusal for work the chain had already taken');
assert.equal(pressPrimary(false, true), 'nothing',
             'a latched shift is latched whatever else the page believes about the head');

console.log('shift_logic: ok (and Hand over is pressed once)');

// ── a clock that never says :60 ─────────────────────────────────────────────
//
// Demo capture, item 6: the chart printed `15:60`. The minutes were floored and the seconds were
// *rounded*, so 959.6 s read as 15 minutes and a sixtieth second — a minute the clock never rolled.
// Floor both, and there is no remainder that can reach 60.
//
// One function for every clock on the page (the chart's `fmt`, the debrief's `F`, the mark sheet's
// `mmss` were three copies of the same arithmetic, and only one of them had this bug), so there is
// one place left for it to be wrong.
// Asked before it is pulled out, so a page that still has the arithmetic inline fails with the
// reason rather than with a parse error from a half-grabbed arrow.
assert.match(script, /function fmt\(/,
             'the chart clock is a function of its own — one clock on this page, and one place for \
              it to be wrong');
const { fmt } = new Function([grab('fmt'), 'return { fmt };'].join('\n'))();

assert.equal(fmt(959.6), '15:59', 'the capture printed 15:60 for this very number');
assert.equal(fmt(59.999), '0:59', 'and nothing rolls a minute early');
assert.equal(fmt(60), '1:00');
assert.equal(fmt(0), '0:00');
assert.equal(fmt(3599.9), '59:59');
assert.equal(fmt(3600), '60:00', 'an hour on the ward is 60:00 — the chart counts minutes, not hours');
assert.equal(fmt(-3), '0:00', 'a clock before the clock started reads zero, never -1:-3');

// And the page keeps one copy of it: a second formatter is a second chance to round a remainder.
const clocks = (uncommented(script).match(/Math\.(floor|round)\(\w+ ?% ?60\)/g) || []);
assert.ok(clocks.length <= 2,
          `the page has ${clocks.length} copies of minutes-and-seconds arithmetic: ${clocks}`);

// ── the room goes red for somebody who is holding her ───────────────────────
//
// Demo capture, item 6: the alarm vignette pulsed over a Critical patient before the head was
// taken. Nobody is treating her yet, nothing the reader does can answer it, and the page's own
// rule two hundred lines below already says this about the nudge — "not before the head is taken".
const { shouldAlarm } = new Function([grab('shouldAlarm'), 'return { shouldAlarm };'].join('\n'))();

assert.equal(shouldAlarm(3, 1, false, true), true, 'she got worse on a shift somebody is holding');
assert.equal(shouldAlarm(3, 1, false, false), false,
             'and not before the head is taken — the room going red is a call to act, and there \
              is nothing this reader may do to her yet');
assert.equal(shouldAlarm(1, 0, false, true), false, 'only past the second rank: a rank 1 is not an alarm');
assert.equal(shouldAlarm(3, 3, false, true), false, 'only when it moves — a fact that has not changed is furniture');
assert.equal(shouldAlarm(1, 3, false, true), false, 'and only downward: an alarm on good news is one nobody reads');
assert.equal(shouldAlarm(3, 1, true, true), false, 'never after the bell — the border is a warning, not a verdict');

console.log('shift_logic: ok (and the clock rolls its own minutes)');

// ── a chart page does not contradict the board ──────────────────────────────
//
// Park Ji-woo's row on the globe read `off_ward` with "the ward no longer holds this case", and her
// own chart page read "She is on the ward" — the board and the page disagreeing about the same
// patient, in the same minute, which is the class of bug the whole day went on removing. The page's
// sentence comes from the payload's own words now, and `openable: false` is the one that wins:
// whatever the chain says about her stay, this ward cannot put anybody at her bedside.
const { chartState } = new Function([grab('chartState'), 'return { chartState };'].join('\n'))();
const she = { s: 'she', o: 'her', p: 'her' };
const he = { s: 'he', o: 'him', p: 'his' };

assert.equal(chartState({ state: 'on_ward' }, she), 'is on the ward');
assert.equal(chartState({ state: 'went_home' }, she), 'went home');
assert.equal(chartState({ state: 'died' }, he), 'died');

const shut = chartState({ state: 'off_ward', openable: false,
                          why_not: 'the ward no longer holds this case' }, she);
assert.match(shut, /off the ward/, `it says where she is: ${shut}`);
assert.match(shut, /chain/, `and that the chain is untouched, which is the part that matters: ${shut}`);
assert.ok(!/is on the ward/.test(shut), `and never the thing her row denies: ${shut}`);

// The case that caused it: the chain still calls her open, and the page must not read that out as
// "on the ward" when the board has already said nobody can open her.
const arguing = chartState({ state: 'on_ward', openable: false,
                             why_not: 'the ward no longer holds this case' }, she);
assert.ok(!/is on the ward/.test(arguing),
          `the page agreed with the chain and contradicted the board: ${arguing}`);

console.log('shift_logic: ok (and a chart page agrees with the board)');

// ── a shift that cannot be rebuilt lets go of the bed ───────────────────────
//
// Sessions are rebuilt when their id is asked for rather than at boot, so the first thing a page
// hears after a deploy can be "this one will not replay". If it keeps beating, the head stays held
// for the whole ten-minute lease by somebody who cannot play on it; if it stops, the server frees
// the bed after seventy-five seconds of silence. So the page has to hear that answer and act on it
// — and say, at the bedside, that nothing it did was recorded.
const { shouldStopBeating } = new Function([grab('shouldStopBeating'), 'return { shouldStopBeating };'].join('\n'))();

assert.equal(shouldStopBeating({ stop_beating: true, error: 'the tape is not here' }), true,
             'the server said this shift is gone: stop holding the bed');
assert.equal(shouldStopBeating({ error: 'no such session' }), false,
             'an ordinary refusal is not a reason to drop a head somebody may still be holding');
assert.equal(shouldStopBeating({}), false);
assert.equal(shouldStopBeating(null), false, 'and a torn answer is not an instruction');
assert.equal(shouldStopBeating({ stop_beating: false }), false);

console.log('shift_logic: ok (and a shift that cannot be rebuilt lets go)');

// ── the three steps of a shift, said on the page ─────────────────────────────
//
// Opening night: 381 people opened the front page, one took a shift, and that shift ran five
// minutes and ended in `/api/ward/left`. Somebody treated a patient and then threw the work away.
// Nothing on the page had told them that pressing Hand over is the thing that makes it count, and
// nothing had told them what a shift consists of.
//
// So the strip carries the three steps while the head is theirs, with the live one marked. `at` is
// a function of what has actually happened — what has been asked, and what has been ordered —
// because a strip that says "order something" to somebody who has not looked at the patient yet is
// giving instructions in the wrong order, and a strip that marks nothing is decoration.
const { wardGuide } = new Function([grab('wardGuide'), 'return { wardGuide };'].join('\n'))();

assert.equal(wardGuide(false, 0, 0), null, 'before the head is taken there is no shift to guide');

const fresh = wardGuide(true, 0, 0);
assert.equal(fresh.steps.length, 3, 'three steps, always all three: it is a map, not a wizard');
assert.equal(fresh.at, 1, 'a shift with nothing done yet is at the first step');
assert.match(fresh.steps[0], /ask|examine/i);
assert.match(fresh.steps[1], /order/i);
assert.match(fresh.steps[2], /hand over/i, 'and the third step is the one that was never pressed');

assert.equal(wardGuide(true, 3, 0).at, 2, 'somebody who has asked is being asked to order');
assert.equal(wardGuide(true, 3, 1).at, 3, 'and one order in, the remaining step is to record it');
assert.equal(wardGuide(true, 0, 1).at, 3,
             'an order without a question still counts — the steps mark what has happened, they \
              do not police the order it happened in');

// The steps never change what they say, only which one is live. A strip whose words move under a
// reader is a strip they stop reading.
assert.deepEqual(wardGuide(true, 0, 0).steps, wardGuide(true, 9, 9).steps);

console.log('shift_logic: ok (and the page says what a shift consists of)');

// ── what the page says before a head is taken ────────────────────────────────
//
// A.4, and the founder's ruling behind it: nothing counts until the shift is handed over, and the
// page has to say so before somebody starts rather than after they have lost it.
//
// The lease is the one number here that must not be typed. `leaseWords` above exists because
// devnet ran at 0.166 s a slot on 17 ก.ย., which made the lease nine and a half minutes rather
// than the twenty-three its nominal rate implies — so a page that says "you have 10 minutes" is a
// page that will lie to somebody the day the rate moves. The measured figure or nothing.
const { beforeTake } = new Function([grab('beforeTake'), 'return { beforeTake };'].join('\n'))();

const unknown = beforeTake(null);
assert.match(unknown, /nothing counts until you hand over/,
             'the sentence that would have saved opening night\u2019s one shift');
assert.equal(/\d/.test(unknown), false,
             `a ward that has not measured its rate says no number at all: ${unknown}`);
assert.equal(/10 minutes|ten minutes/i.test(unknown), false,
             'and never the nominal one, which is the figure that was wrong by a factor of two');

const known = beforeTake(9);
assert.match(known, /9 minutes/, 'measured, it says the measured figure');
assert.match(known, /nothing counts until you hand over/, 'and still says the part that matters');
assert.match(beforeTake(23), /23 minutes/, 'whatever the chain is doing today');

console.log('shift_logic: ok (and it says so before the shift, not after)');

// ── the way to the guide, for somebody who has never done this ───────────────
//
// A.6. One link, to the page the server serves at /start, in the words of the question somebody
// asks themselves rather than the name of a feature.
const { guideLink } = new Function([grab('guideLink'), 'return { guideLink };'].join('\n'))();

const link = guideLink();
assert.match(link, /href="\/start"/, 'the guide the ward serves, same origin, no request leaves');
assert.match(link, /first time/i, 'addressed to the person who needs it');
assert.match(link, /two-minute|2-minute/i, 'and honest about what it costs them to read it');
assert.equal(/http/i.test(link), false, 'nothing off this origin');

console.log('shift_logic: ok (and there is a way to the guide)');

// ── and the strip a stranger actually reads ──────────────────────────────────
//
// The three above are the words. This is the arrangement, which is the part a stranger reads — and
// it is pure for exactly that reason: a sentence that exists in a function nothing paints is a
// sentence nobody has been told.
const { guideHtml } = new Function(
  [grab('beforeTake'), grab('guideLink'), grab('guideHtml'), 'return { guideHtml };'].join('\n'))();

// Before the head is taken: what to expect, and the way to the guide.
const waiting = guideHtml(null, null);
assert.match(waiting, /nothing counts until you hand over/);
assert.match(waiting, /href="\/start"/, 'the guide is one press away from the bed, before the take');
assert.equal(/\d/.test(waiting), false, 'and still no unmeasured number in it');
assert.match(guideHtml(null, 9), /9 minutes/, 'measured, the wait says how long the head is held');

// Once it is theirs: three steps, with one lit.
const held = guideHtml(wardGuide(true, 2, 0), 9);
assert.equal((held.match(/class="gd/g) || []).length, 3, 'all three steps, always');
assert.equal((held.match(/class="gd at"/g) || []).length, 1, 'and exactly one of them is live');
assert.match(held, /class="gd at"><b>2<\/b>/, 'somebody who has asked is lit at the ordering step');
assert.match(guideHtml(wardGuide(true, 2, 1), 9), /class="gd at"><b>3<\/b>/,
             'and one order in, at the press that records it');
assert.equal(/href="\/start"/.test(held), false,
             'the guide link is for somebody deciding whether to start, not for somebody mid-shift \
              with a countdown running');

console.log('shift_logic: ok (and the strip reads as three steps)');


// ── the shift that records itself, and the one that cannot ──────────────────
//
// A.7, and the honest scope of it. `anchor_shift` needs the player's signature, so the server can
// never record a shift on a stranger's behalf — the only thing it can do at lease end is free the
// bed, which it already does. What *can* record is the page, while it is still open. So the page
// does it at 0:00, and the policy text says exactly that and no more.
//
// Only where there is something to record. A shift with no orders in it is left as it is today:
// there is nothing on the chain worth a transaction, and a stranger who opened a bed and walked
// away has not treated anybody.
const { autoHandOver } = new Function([grab('autoHandOver'), 'return { autoHandOver };'].join('\n'))();

assert.equal(autoHandOver(0, 1, true, false), true,
             'the lease is out, the head is theirs and there is one order in it: record it');
assert.equal(autoHandOver(-3, 4, true, false), true, 'and a clock that went past zero still counts');

assert.equal(autoHandOver(0, 0, true, false), false,
             'nothing was ordered, so there is nothing to record and no transaction worth sending');
assert.equal(autoHandOver(1, 3, true, false), false, 'a second left is a second to keep playing');
assert.equal(autoHandOver(0, 3, false, false), false, 'no head taken, no shift to record');
assert.equal(autoHandOver(0, 3, true, true), false,
             'and never twice: a hand-over already going is the one that records it');
assert.equal(autoHandOver(null, 3, true, false), false,
             'a page that does not know the lease does not act on it');

console.log('shift_logic: ok (and a shift with work in it records itself at 0:00)');


// ── an age, said the way a person says one ──────────────────────────────────
//
// Seen on staging, 22 Sep, at 1280 and at phone width: the bedside identity line read
// "FADUMO JAMA · 2 · FROM SOM" and the receipt carried a line holding nothing but "2". A number
// with no unit standing between two other facts reads as a stray digit at any age, and at two it
// reads as a typo — which is the worst of it, because two is exactly the age a reader is most
// likely to disbelieve.
//
// The patient block's "F 2" is a different convention and stays: there the letter carries the sex
// and the number is plainly the age beside it. This is for the places where the number stands
// alone.
const { agePhrase } = new Function([grab('agePhrase'), 'return { agePhrase };'].join('\n'))();

assert.equal(agePhrase(2), '2 years old', 'the unit is what makes it a fact rather than a digit');
assert.equal(agePhrase(64), '64 years old');
assert.equal(agePhrase(1), '1 year old', 'and one of anything is not ones');
assert.equal(agePhrase(0), 'under 1 year old',
             'a ward that admits infants has ages of zero, and "0 years old" is not how anybody \
              says it — nor is hiding the age of the youngest patient on the ward');
assert.equal(agePhrase(null), '', 'and an age nobody knows is not invented');
assert.equal(agePhrase(undefined), '');

console.log('shift_logic: ok (and an age says what it is)');

// ── the season's counters are not the ward's ────────────────────────────────
//
// `#chainstate` and `#tally` ship as "—" in bay-surface.html and are filled by the season, which
// counts an account's attempts. On the ward there is no account and no attempts, so they stay at
// their placeholder — and at phone width they wrap onto a line of their own, where a stranger
// standing at a bed is given a symbol that decodes to nothing.
//
// A placeholder is a promise that a value is coming. Where none is, it is furniture.
const { showsTally } = new Function([grab('showsTally'), 'return { showsTally };'].join('\n'))();

assert.equal(showsTally(true, '—'), false, 'on the ward, an unfilled counter is not shown at all');
assert.equal(showsTally(true, ''), false, 'nor an empty one');
assert.equal(showsTally(true, '3 of 5'), true, 'and a counter with something in it is shown');
assert.equal(showsTally(false, '—'), true,
             'the Eternal bay keeps its own placeholder — the season fills it, and its layout was \
              designed around it being there');

console.log('shift_logic: ok (and the ward shows no empty furniture)');
