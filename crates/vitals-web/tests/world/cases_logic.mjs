// How the reviewer's list is grouped, run against the page that ships.
//
// The ward held 138 cases on 18 ก.ย. and 60 of them were withdrawn — withdrawn cases sat in the
// level bands beside the live ones, in the same weight, most with Thai titles, and the seventeen a
// reviewer actually had to read were somewhere in the middle of them. A withdrawn case is not a
// fourth difficulty; it is a fact about a case's future, and it belongs under its own fold at the
// bottom where a reviewer can open it on purpose.
//
//   node cases_logic.mjs <world/review.html>

import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const html = readFileSync(process.argv[2], 'utf8');
const script = (html.match(/<script>([\s\S]*?)<\/script>/) || [])[1];
assert.ok(script, 'the page has no script block');

function grab(name) {
  const i = script.indexOf(`function ${name}(`);
  assert.notEqual(i, -1, `${name} is not in the page — renamed, or deleted with its test left behind`);
  let d = 0, j = script.indexOf('{', i);
  for (; j < script.length; j++) {
    if (script[j] === '{') d++;
    else if (script[j] === '}' && --d === 0) break;
  }
  return script.slice(i, j + 1);
}
const grabConst = (name) => {
  const m = script.match(new RegExp(`const ${name} *= *([\\s\\S]*?);\\n`));
  assert.ok(m, `${name} is not in the page`);
  return `const ${name} = ${m[1]};`;
};

const { row, groupsOf, reviewLede } = new Function(
  [grabConst('esc'), grabConst('LEVELS'), grab('row'), grab('groupsOf'), grab('reviewLede'),
   'return { row, groupsOf, reviewLede };'].join('\n'))();

// The ward as it stood on the capture: 138 cases, 60 of them withdrawn, Thai titles among them,
// and — read off /api/ward/cases on 18 ก.ย. — not one of the 138 clinically reviewed.
const LEVELS = ['student', 'intern', 'resident'];
const cases = [];
for (let i = 0; i < 138; i++) {
  const withdrawn = i < 60;
  cases.push({
    case_id: `case-${i}`,
    title: withdrawn ? `ผู้ป่วยหญิง ${20 + i} ปี มีไข้และไอ` : `A patient with a cough (${i})`,
    difficulty: LEVELS[i % 3],
    withdrawn,
    provisional: true,
    country: 'THA',
  });
}

const { bands, withdrawn } = groupsOf(cases);

assert.equal(withdrawn.length, 60, 'every withdrawn case is in the fold');
assert.equal(bands.reduce((n, [, list]) => n + list.length, 0), 78,
             'and none of them is in a level band: 138 − 60 = 78 rows a reviewer has to read');
for (const [level, list] of bands) {
  for (const c of list) {
    assert.ok(!c.withdrawn, `a withdrawn case is in the ${level} band — that is the 60 rows again`);
  }
}

// The fold: collapsed, at the bottom, and counted so a reviewer knows what is in it.
assert.match(html, /<details/, 'the withdrawn group is a fold, so it opens on purpose');
assert.ok(!/<details[^>]*\sopen[\s>]/.test(html), 'and it is closed until somebody opens it');
assert.match(script, /<summary>[\s\S]{0,120}withdrawn/i, 'the fold says what it holds');

// ── said once, at the top, rather than on all 138 rows ───────────────────────
// Every case the ward holds is provisional, so a "provisional" badge on every row distinguishes
// nothing: it is furniture by the second screenful. The page states it once, and a row speaks only
// where it differs — which today is nowhere, and the day a case is signed off is everywhere that
// matters.
// Count-driven, because the truth moves: the advisor's review of 18 cases arrived on the night of
// 20 ก.ย., and a fixed "no case here is reviewed" would have been false the morning after. Eleven
// words, inside the twelve the ward holds itself to.
assert.equal(reviewLede(cases), '0 of 78 reviewed by our clinical advisor; the rest provisional',
             'the whole catalogue in one sentence, counted at this read');
// The ward opened on a date; the page says so beneath the counts, in the sentence the API gives it
// — never a date typed into the page.
const status = 'provisional — under review by our clinical advisor · open for play since 21 Sep 2026';
assert.equal(reviewLede(cases, status),
             '0 of 78 reviewed by our clinical advisor; the rest provisional · open for play since 21 Sep 2026',
             'the opening rides on the same line, from the API');

const mixed = cases.map((c, i) => ({ ...c, provisional: i % 2 === 0 }));
assert.equal(reviewLede(mixed), '39 of 78 reviewed by our clinical advisor; the rest provisional',
             'once some are signed off the sentence says the split rather than a flat claim');
assert.ok(!/all provisional/.test(reviewLede(mixed)),
          `and stops saying "all" the moment it is untrue: ${reviewLede(mixed)}`);

const provisional = row({ case_id: 'x', title: 'y', difficulty: 'intern', provisional: true });
assert.ok(!/provisional/i.test(provisional),
          `a provisional row carries no badge — the page has already said it: ${provisional}`);
assert.ok(!/not reviewed/i.test(provisional),
          `and certainly not "NOT REVIEWED", which reads as a fault being shown: ${provisional}`);

const reviewed = row({ case_id: 'x', title: 'y', difficulty: 'intern', provisional: false });
assert.match(reviewed, />reviewed</,
             `the row that differs from the page is the one that speaks: ${reviewed}`);

console.log('cases_logic: ok (and the withdrawn are folded away)');
