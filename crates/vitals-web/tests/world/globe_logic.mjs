// The globe's arithmetic, pulled out of world/index.html and run without a browser.
//
// Three things on the page are pure functions and are tested here, red before green:
//   countryId(alpha3)        — ISO 3166-1 alpha-3 → the numeric id world-atlas keys its shapes by
//   countryCounts(patients)  — per-country tallies the lit countries and their badges draw from
//   visible(patients, filter)— the difficulty toggles, and where a null-country patient goes
//
// Everything else on the page is drawing, and drawing is checked by the two screenshots in the
// report, not here. The functions are grabbed by name and brace-matching so what runs is the
// source that ships; a rename fails loudly.
//
//   node globe_logic.mjs <world/index.html>

import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const html = readFileSync(process.argv[2], 'utf8');
// The page's own code sits in the block marked id="globe"; the vendored libraries are the other
// (larger) blocks and must not be what runs here.
const script = (html.match(/<script id="globe">([\s\S]*?)<\/script>/) || [])[1];
assert.ok(script, 'the page has no <script id="globe"> block');

// The committed data files, reached from the page's path: both the pool and the physicians series
// are read here and held against what the page ships.
const dataFile = (name) => new URL(`../../data/${name}`,
  new URL('file://' + process.argv[2].replace(/^(?!\/)/, process.cwd() + '/')))
  .pathname.replace('/static/world/../../data', '/data');

function grab(name) {
  const i = script.indexOf(`function ${name}(`);
  assert.notEqual(i, -1, `${name} is not in the page`);
  let d = 0, j = script.indexOf('{', i);
  for (; j < script.length; j++) {
    if (script[j] === '{') d++;
    else if (script[j] === '}' && --d === 0) break;
  }
  return script.slice(i, j + 1);
}
const grabConst = (name) => {
  const m = script.match(new RegExp(`(const|var) ${name} *= *([\\s\\S]*?);\\n`));
  assert.ok(m, `${name} is not in the page`);
  return `${m[1]} ${name} = ${m[2]};`;
};

const sandbox = [grabConst('ALPHA3'), grabConst('FULL_NAME'), grab('displayName'), grabConst('STATE_LABEL'), grabConst('DOCTOR_BINS'), grab('countryId'), grab('countryCounts'), grab('visible'),
  grab('stateOf'), grab('onBoard'), grab('inBeds'), grab('canTakeShift'), grab('censusFigures'), grab('paintOf'), grab('hoverText'), grab('openingCountry'), grab('openingLongitude'), grab('countryGroups'), grab('countryHeading'), grab('waitingCounts'), grab('figuresFor'), grab('bedsEmptyWords'), grab('shouldReload'), grab('whenMs'), grab('relative'), grab('absolute'), grab('stateLine'),
  grab('peoplePerDoctor'), grab('latestOf'), grab('tenYearTrend'), grab('fmtTrend'), grab('fmtPeople'), grab('doctorLine'),
  grab('worldAverage'), grab('missionLine'), grab('doctorBin'),
  grab('yearValue'), grab('yearRange'), grab('doctorLineAt'), grab('worldAverageAt'), grab('missionLineAt'),
  grab('escapeHtml'), grab('portraitImg'), grab('onTheGlobe'),
  'return { displayName, FULL_NAME, countryId, countryCounts, visible, inBeds, canTakeShift, censusFigures, ALPHA3, openingCountry, openingLongitude, countryGroups, countryHeading, waitingCounts, figuresFor, bedsEmptyWords, shouldReload, whenMs, relative, absolute, stateLine, stateOf, onBoard, paintOf, hoverText, STATE_LABEL, DOCTOR_BINS, peoplePerDoctor, latestOf, tenYearTrend, fmtTrend, fmtPeople, doctorLine, worldAverage, missionLine, doctorBin, yearValue, yearRange, doctorLineAt, worldAverageAt, missionLineAt, portraitImg, onTheGlobe };'].join('\n');
const { displayName, FULL_NAME, countryId, countryCounts, inBeds, canTakeShift, censusFigures, visible, ALPHA3, openingCountry, openingLongitude, countryGroups, countryHeading, waitingCounts, figuresFor, bedsEmptyWords, shouldReload, whenMs, relative, absolute, stateLine,
  stateOf, onBoard, paintOf, hoverText, STATE_LABEL,
  DOCTOR_BINS, peoplePerDoctor, latestOf, tenYearTrend, fmtTrend, fmtPeople, doctorLine, worldAverage, missionLine, doctorBin,
  yearValue, yearRange, doctorLineAt, worldAverageAt, missionLineAt, portraitImg, onTheGlobe } = new Function(sandbox)();

// ── countryId ────────────────────────────────────────────────────────────────
// world-atlas 110m keys its shapes by ISO numeric, as strings ("764"); the ward sends alpha-3.
assert.equal(countryId('THA'), '764', 'Thailand');
assert.equal(countryId('IDN'), '360', 'Indonesia');
assert.equal(countryId('JPN'), '392', 'Japan');
assert.equal(countryId('KOR'), '410', 'South Korea');
assert.equal(countryId('CHN'), '156', 'China');
assert.equal(countryId('HKG'), '344', 'Hong Kong');
assert.equal(countryId('USA'), '840', 'United States');
assert.equal(countryId('tha'), '764', 'case-insensitive: a lower-case code still resolves');
assert.equal(countryId('XXX'), null, 'an unknown code is null, never a wrong country');
assert.equal(countryId(null), null, 'null in, null out — the origin-unknown patient');
assert.equal(countryId(''), null);
// Every shape in the vendored atlas must be reachable from some alpha-3, or a lit country could
// never light. The atlas ids are read from the page's own embedded topology.
const topo = JSON.parse(html.match(/<script id="atlas" type="application\/json">([\s\S]*?)<\/script>/)[1]);
const atlasIds = new Set(topo.objects.countries.geometries.map(g => String(g.id)));
// What each polygon is labelled in the file, before the product has a say.
const polygonName = new Map(topo.objects.countries.geometries
  .filter(g => g.id !== undefined && g.id !== null)
  .map(g => [String(g.id), (g.properties || {}).name]));
const reachable = new Set(Object.values(ALPHA3).map(String));
const orphans = [...atlasIds].filter(id => !reachable.has(id));
// 110m draws a few territories with no ISO alpha-3 of their own (e.g. N. Cyprus, Somaliland,
// Kosovo as -99); those may stay unreachable, but real countries may not.
assert.ok(orphans.length <= 5, `atlas shapes no alpha-3 reaches: ${orphans.join(', ')}`);
assert.ok(atlasIds.has('764') && atlasIds.has('360') && atlasIds.has('156'), 'TH, ID and CN are drawn');
// 110m has no polygon for Hong Kong (it is inside China's outline at that scale). A patient from
// HKG must still resolve — to the tray, named — rather than fall off the map as an unknown code.
assert.equal(countryId('HKG'), '344');
assert.ok(!atlasIds.has('344'), 'if 110m ever draws HK on its own, the tray rule for HKG should be revisited');

// ── the product says the full name; the atlas abbreviates to fit a label ──────
//
// 110m shortens ten of its labels so they fit on a map: "S. Sudan", "Dem. Rep. Congo",
// "Bosnia and Herz.", "Eq. Guinea". That is cartography, not what a country is called, and it was
// reaching people — the queue chip read "S. Sudan" while the bedside read "South Sudan", the same
// person under two names in one session. Worse, the first version of the pool-label rule below
// pinned `place` to the raw polygon string, which would have pushed the abbreviation the other way
// and put "a man from Dem. Rep. Congo" into a portrait prompt.
//
// So the expansion lives in one place and `nameOf` is its only reader: nothing downstream needs to
// know the atlas abbreviates at all.
assert.equal(displayName('728', 'S. Sudan'), 'South Sudan');
assert.equal(displayName('180', 'Dem. Rep. Congo'), 'Democratic Republic of the Congo');
assert.equal(displayName('070', 'Bosnia and Herz.'), 'Bosnia and Herzegovina');
assert.equal(displayName('764', 'Thailand'), 'Thailand', 'a name that needs nothing is untouched');
assert.equal(displayName('999', undefined), '999',
             'and an id the atlas does not draw still answers with something');

// **No name this product can reach is an abbreviation.** The assertion that catches the next atlas
// bump: a new "St. Vincent" arrives, and the expansion table has to grow with it or this fails.
const shortened = [...new Set(Object.values(ALPHA3))]
  .map(id => displayName(id, polygonName.get(id)))
  .filter(n => n && /\.(\s|$)/.test(n));
assert.deepEqual(shortened, [],
                 `the product would say these abbreviated: ${shortened.join(', ')}`);

// ── the pool's label says what the screen says ────────────────────────────────
//
// Three files name a country. `personas.json` said `KOR: "Korea"` while the globe said South
// Korea, and an hour went into asking which was wrong. Every join is on the alpha-3 through
// ALPHA3, so a disagreeing name cannot break a lookup — which is why nothing had caught it.
//
// **Both names are on screens.** The panel heading is the atlas polygon's own `properties.name`
// (`nameOf`, then `countryHeading`). The pool's `place` is published as `country_name` and the bay
// prints it — "· from South Korea" — and the no-JS pages, the meta description and the factory's
// portrait prompt all say it too. `ward.rs` claimed `place` was for a reader of the file rather
// than for the product; it was not, and that stale sentence is what made two people look for the
// defect in the wrong file. So this is not a tidying rule: a patient can be from Korea in the bay
// and from South Korea on the globe, in the same session, over the same person.
//
// The World Bank names in physicians.json are deliberately **not** held to this. "Korea, Rep." is
// an indicator label from a source the footer credits, and editing it would misquote that source.
//
// This will speak up at the cwf/factory merge after 26 ก.ย., where a 74-country pool arrives
// (the founder's "patients from the whole world", 16 ก.ย.). That is the point of it: any label
// that differs is one string, and the alternative is a merge resolving the divergence silently.
const poolPath = dataFile('personas.json');
const pool = JSON.parse(readFileSync(poolPath, 'utf8')).countries;
assert.ok(pool.length >= 20, `the pool has its countries: ${pool.length}`);
for (const e of pool) {
  const id = countryId(e.country);
  assert.notEqual(id, null, `${e.country}: a code the globe can place`);
  // The **displayed** name, not the polygon's label. Pinning the pool to the raw string would
  // force the atlas's label-fitting abbreviations into the bedside and the portrait prompt.
  const drawn = polygonName.has(id) ? displayName(id, polygonName.get(id)) : undefined;
  // A code with no polygon at this scale (HKG inside China's outline) has no on-screen name to
  // agree with — she is named in the tray and the label is free. Only a country the map draws is
  // held to what the map calls it.
  if (drawn === undefined) continue;
  assert.equal(e.place, drawn,
               `${e.country}: the pool calls it ${JSON.stringify(e.place)} and the globe says ` +
               `${JSON.stringify(drawn)} — one of them is what a reader sees, and it is the globe`);
}

// ── countryCounts ────────────────────────────────────────────────────────────
const P = (id, country, extra = {}) => ({ patient_id: id, name: `p${id}`, country, state: 'on_ward',
  difficulty: 'student', endemic: false, ...extra });
const cc = countryCounts([P(1, 'THA'), P(2, 'THA'), P(3, 'IDN'), P(4, null), P(5, 'tha')]);
assert.deepEqual(cc.byId, { '764': 3, '360': 1 }, 'tallied by atlas id, case-folded, null excluded');
assert.equal(cc.unknown, 1, 'the null-country patient is counted, not dropped');
assert.deepEqual(countryCounts([]), { byId: {}, treatingById: {}, unknown: 0 },
                 'an empty ward is an empty tally');
assert.deepEqual(countryCounts([P(1, 'XXX')]).byId, {}, 'an unknown code lights nothing');
assert.equal(countryCounts([P(1, 'XXX')]).unknown, 1, '…but she is still on the ward, in the tray');
// A real code the atlas has no polygon for (HKG at 110m) must also go to the tray: a patient who
// resolves to an id nothing can draw would otherwise be on the ward and on no screen.
const hk = countryCounts([P(1, 'HKG')], atlasIds);
assert.deepEqual(hk.byId, {}, 'HKG lights no polygon (there is none)');
assert.equal(hk.unknown, 1, 'HKG goes to the tray');
assert.equal(countryCounts([P(1, 'THA')], atlasIds).unknown, 0, 'a drawable country never goes to the tray');

// ── the rings are beds, and one of them is being worked in ───────────────────
//
// The globe showed USA 1, MEX 1, BRA 1, EGY 1 and ETH 3 on a board whose beds held KOR, ETH and JPN
// (director, 17 ก.ย., staging 00033). Every ring but one was a patient who had gone home, died, or
// was never the ward's to open: the tally counted the whole chain's history. A ring means "there is
// somebody here you can treat", and the whole point of the page is that the number is small and
// true.
//
// `inBeds` is the one place that decides, and `treatingById` is what makes "being treated right
// now" drawable — the founder's "ดูยากว่าใครกำลังรักษาคนไข้อยู่".
const bedded = [P(1, 'THA', { bed: 2 }), P(2, 'THA', { bed: 1, state: 'on_shift' }),
                P(3, 'IDN', { state: 'went_home' }), P(4, 'IDN', { state: 'died' }),
                P(5, 'IDN', { state: 'off_ward' }),
                // On the chain, open, and with no bed: the ward cannot rebuild her chart, so
                // nobody can take a shift on her and she is not in a bed. She was in this list and
                // in the panel's heading, over a figure that said three — two definitions of "in
                // beds" on one screen, which is the thing this page must never do.
                P(6, 'ETH', { bed: null })];
assert.deepEqual(inBeds(bedded).map(p => p.patient_id), [2, 1],
                 'a bed is a bed: the state and a bed number, listed in bed order');
const beds = countryCounts(inBeds(bedded));
assert.deepEqual(beds.byId, { '764': 2 }, 'Indonesia is not ringed for three patients who left');
assert.deepEqual(beds.treatingById, { '764': 1 }, 'and one of the two Thai beds is being worked in');
assert.equal(beds.byId['231'], undefined, 'nor Ethiopia for a patient nobody can open');
// The figure and the list are one number: whatever the panel lists, the heading counts, and
// the census rail publishes.
assert.equal(inBeds(bedded).length, censusFigures({}, bedded, inBeds(bedded).length)[0][1],
             'the panel\'s count and the figure under the headline are the same number');
assert.deepEqual(countryCounts(inBeds([P(1, 'THA')])).treatingById, {},
                 'a bed nobody is in right now is a ring without the second mark');

// ── who can actually be taken ────────────────────────────────────────────────
//
// The tray offered "take a shift" on a patient the ward cannot rebuild, and pressing it landed on a
// page that refuses (director, 17 ก.ย., A6). A button that cannot do what it says is worse than no
// button: it spends the one gesture the front page is asking for.
//
// A shift can be taken on somebody in a bed whose chart this ward can open. A patient with no bed —
// admitted outside the ward, or on the chain with a chart nobody here can rebuild — is on the board
// with her own sentence, and the link says "view".
assert.equal(canTakeShift(P(1, 'THA', { bed: 2 })), true);
assert.equal(canTakeShift(P(1, 'THA', { bed: null })), false, 'no bed, nothing to take');
assert.equal(canTakeShift(P(1, 'THA', { bed: 2, state: 'on_shift' })), false,
             'somebody is already in the room with her');
assert.equal(canTakeShift(P(1, 'THA', { bed: null, state: 'off_ward' })), false);
assert.equal(canTakeShift(P(1, 'THA', { bed: 1, state: 'went_home' })), false);

// ── the figures under the headline ───────────────────────────────────────────
//
// The numbers the founder wants followed week to week were 11 px of monospace in a corner (A5). They
// are the page's second sentence now, and they carry the one the board could not say before:
// how many of the beds have somebody in them right now.
const figs = censusFigures({ shifts: 25, went_home: 1, died: 7 }, [
  P(1, 'THA', { bed: 1 }), P(2, 'KOR', { bed: 2, state: 'on_shift' }), P(3, 'BRA', { bed: 3 }),
  P(4, 'IDN', { state: 'went_home' }),
], 3);
assert.deepEqual(figs, [['in beds', 3], ['on shift now', 1], ['shifts', 25], ['went home', 1], ['died', 7]],
                 'the five numbers, in the order a reader meets them');
// A census that does not carry a number is not a zero: the figure is left out rather than invented.
assert.deepEqual(censusFigures({}, [], 0).map(([k]) => k), ['in beds', 'on shift now'],
                 'what the ward itself counts is always there; the chain\'s numbers are not faked');

// ── visible ──────────────────────────────────────────────────────────────────
const ward = [P(1, 'THA', { difficulty: 'student' }), P(2, 'THA', { difficulty: 'intern' }),
  P(3, 'IDN', { difficulty: 'resident' }), P(4, null, { difficulty: 'intern' })];
const all = { student: true, intern: true, resident: true };
assert.deepEqual(visible(ward, all).map(p => p.patient_id), [1, 2, 3, 4], 'all three on = everyone');
assert.deepEqual(visible(ward, { ...all, intern: false }).map(p => p.patient_id), [1, 3],
  'a toggle off removes that difficulty everywhere, tray included');
assert.deepEqual(visible(ward, { student: false, intern: false, resident: false }), [],
  'all off = nobody, and the page must say so rather than show a lit globe');
assert.deepEqual(visible([P(9, 'THA', { difficulty: 'weird' })], all).map(p => p.patient_id), [9],
  'an unknown difficulty is shown, not silently filtered — a new tier must not vanish patients');

// ── time ─────────────────────────────────────────────────────────────────────
// Every timestamp the page shows is a moment the *chain* dated. The payload carries the block
// time of the slot an event landed in, as a Z-suffixed UTC string (or unix seconds); a slot count
// is never turned into a time here. The board said "admitted 3 days ago" over a patient admitted
// yesterday for exactly that reason — 0.4 s a slot, extrapolated from the read's own slot across
// two days of a devnet that does not keep to its nominal rate, was 38 hours out.
const NOW = Date.UTC(2026, 8, 16, 12, 0, 0);          // 16 Sep 2026 12:00:00Z
assert.equal(whenMs({ unix: NOW / 1000 - 360 }, NOW), NOW - 360_000, 'unix seconds → ms');
assert.equal(whenMs({ iso: '2026-09-16T11:54:00Z' }, NOW), NOW - 360_000, 'a Z string is UTC');
assert.equal(whenMs({ iso: '2026-09-16T11:54:00' }, NOW), null, 'a string with no zone is not a time the page will guess at');
assert.equal(whenMs({ slot: 499_139_724 }, NOW), null, 'a slot is a fact about the chain, and this page does not date it');
assert.equal(whenMs({ unix: 0 }, NOW), null, 'zero is the program\'s "never", not 1970');
assert.equal(whenMs({}, NOW), null);
assert.ok(!/SLOT_MS/.test(script), 'no slot-to-milliseconds constant is left on the page to tempt the next reader');

assert.equal(relative(NOW - 10_000, NOW), 'just now');
assert.equal(relative(NOW - 6 * 60_000, NOW), '6 min');
assert.equal(relative(NOW - 89 * 60_000, NOW), '89 min');
assert.equal(relative(NOW - 95 * 60_000, NOW), '2 h', 'past ninety minutes it is hours, rounded');
assert.equal(relative(NOW - 6 * 3_600_000, NOW), '6 h');
assert.equal(relative(NOW - 35 * 3_600_000, NOW), '35 h');
assert.equal(relative(NOW - 3 * 86_400_000, NOW), '3 days');
assert.equal(relative(NOW - 86_400_000 * 1.6, NOW), '2 days');
assert.equal(relative(NOW + 60_000, NOW), 'just now', 'a clock ahead of ours is now, not "-1 min"');

const abs = absolute(NOW);
assert.equal(typeof abs, 'string');
assert.ok(abs.includes('2026'), `the absolute time carries the date: ${abs}`);
const hours = new Date(NOW).getHours();                // this machine's zone, which is the viewer's
assert.ok(abs.includes(String(hours)) || abs.includes(String(hours % 12 || 12)),
  `rendered in the viewer's own zone (Intl), not the server's: ${abs} vs local hour ${hours}`);

// stateLine: the words under a patient, and the title behind them. Every branch reads a time the
// payload carries; a row with the slot and no block time for it says the state and stops.
//
// Short on purpose. The pill read "on the ward · handed over 30 h ago" in a 300 px panel and ran
// off the row's right edge — the founder's screenshot, 17 ก.ย. What a reader needs at a glance is
// the state and how long ago; *which* event the time is about is a detail, and details go in the
// title where a hover or a long press finds them.
const line = (p) => stateLine(p, NOW);
const iso = ms => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
const onShift = line({ state: 'on_shift', on_shift_since: NOW / 1000 - 360, admitted_at: iso(NOW - 9_000_000) });
assert.equal(onShift.text, 'on shift · 6 min');
assert.equal(onShift.title, `since ${absolute(NOW - 360_000)}`);
const waiting = line({ state: 'on_ward', on_shift_since: null, admitted_at: iso(NOW - 3 * 86_400_000) });
assert.equal(waiting.text, 'on the ward · 3 days');
assert.equal(waiting.title, `admitted 3 days ago · ${absolute(NOW - 3 * 86_400_000)}`,
  'the word that says which event it was lives in the title, with the moment itself');
const handed = line({ state: 'on_ward', handed_over: iso(NOW - 30 * 3_600_000), admitted_at: iso(NOW - 3 * 86_400_000) });
assert.equal(handed.text, 'on the ward · 30 h', 'the row the founder screenshotted, in the width it has');
assert.equal(handed.title, `handed over 30 h ago · ${absolute(NOW - 30 * 3_600_000)}`);
const home = line({ state: 'went_home', admitted_at: iso(NOW - 900_000_00), closed_at: iso(NOW - 6 * 3_600_000) });
assert.equal(home.text, 'went home · 6 h');
assert.equal(home.title, `left 6 h ago · ${absolute(NOW - 6 * 3_600_000)}`);
const died = line({ state: 'died', admitted_at: iso(NOW - 900_000_00), closed_at: iso(NOW - 20_000) });
assert.equal(died.text, 'died · just now');
const bare = line({ state: 'on_ward' });
assert.equal(bare.text, 'on the ward', 'no time in the payload, no time on the page');
assert.equal(bare.title, '');
// The slot is on the row and the block time is not: the ward has not looked that slot up yet.
const undated = line({ state: 'on_ward', admitted_slot: 499_139_724, closed_slot: 0 });
assert.equal(undated.text, 'on the ward',
  'a slot the chain has not dated for us is not a time we invent from the read\'s own slot');

// ── where the globe opens ────────────────────────────────────────────────────
//
// UX review A2: the globe opens on a fixed rotation over Asia. It was chosen when the first
// patients happened to be there, and it is a guess about where the ward is that stops being true
// the moment the queue changes — a viewer in Lagos or São Paulo is shown somebody else's continent
// and has to find their own.
//
// So it opens on the ward: the country of the patient in the first bed, or of the first person
// waiting for the door when no bed is filled yet. With neither — a ward with nothing on it — it
// opens on the viewer's own longitude, which their clock already tells us and which needs nothing
// asked of them.
assert.equal(openingCountry([{ bed: 2, country: "KEN", state: "on_ward" },
                             { bed: 1, country: "BGD", state: "on_ward" }], []),
             "BGD", "the first bed, not the first row");
assert.equal(openingCountry([{ bed: null, country: "KEN", state: "went_home" }],
                            [{ country: "THA" }]),
             "THA", "nobody in a bed: the first person waiting for one");
assert.equal(openingCountry([], [{ country: "PAK" }, { country: "THA" }]), "PAK");
assert.equal(openingCountry([], []), null, "and with neither, it is not a country's business");
assert.equal(openingCountry(null, null), null);

// The viewer's own longitude, from the offset their browser reports. `getTimezoneOffset` is
// minutes *behind* UTC, so Bangkok (UTC+7) reports −420 and sits at +105°.
assert.equal(openingLongitude(-420), 105, "Bangkok");
assert.equal(openingLongitude(0), 0, "London in winter");
assert.equal(openingLongitude(300), -75, "New York in winter");
assert.equal(openingLongitude(-660), 165, "and the far side of the date line");
assert.equal(openingLongitude(null), 100,
             "and a browser that reports no offset leaves the globe where it has always opened — \
              100°E, which is the rotation this page was born with");

// ── the controls read as controls ────────────────────────────────────────────
//
// UX review A3/A4: the two layer switches and the three level switches are pill buttons whose only
// state is a colour, and they are labelled with the names of data sets ("doctors", "ward") rather
// than with what a viewer would see. The year slider has an invisible label and a bare number
// beside it. All five are switches; none of them says so.
//
// What is asserted here is the part that lives in the file — the words, the switch role, and a
// stylesheet that paints both states — and it is read off the *page*, markup and CSS included,
// rather than off the script the rest of this file pulls functions out of. The remaining half,
// that a click flips one, is driven in a browser.
assert.match(html, /doctor shortage/, 'the layer is named by what a viewer sees on it');
assert.match(html, /patients in beds</, 'and so is the other one');
assert.ok(!/>doctors</.test(html) && !/>ward</.test(html),
  'neither is named by the data set it comes from any more');
assert.match(html, /role="switch"/, 'a switch says so to a screen reader, not only to an eye');
assert.match(html, /aria-pressed="true"/, 'and keeps the pressed state the page already reads');
// Both states painted, and differently: a control whose off state is "the absence of a colour" is
// a control nobody can read in a screenshot, which is how the founder met these.
assert.match(html, /\[aria-pressed="true"\]/);
assert.match(html, /\[aria-pressed="false"\]/);
// The year control says what it is and what it is set to.
assert.match(html, /Year ·/, 'a labelled control, not a number beside a slider');
assert.ok(!/<label for="year" class="sr">/.test(html),
  'the label is visible now — an invisible one is a label for everybody except the person looking');

// ── a country's panel, in two groups ─────────────────────────────────────────
//
// The founder, 18 ก.ย.: South Korea's ring says 1 and the panel lists two — Ji-woo in bed 1 and
// Lee Seo-yeon, who died sixteen hours ago. Both are true and the pair is unreadable: the ring
// counts beds, and a stay that ended is not a bed. So the panel says which is which, in the order
// a reader wants them — who is here now, then who has been here.
assert.deepEqual(countryGroups([]), { inBeds: [], left: [] });
{
  const jiwoo = { patient_id: 1, state: "on_ward", bed: 1, difficulty: "resident" };
  const seoyeon = { patient_id: 2, state: "died", bed: null, difficulty: "intern" };
  const adrift = { patient_id: 3, state: "off_ward", bed: null };
  const g = countryGroups([seoyeon, jiwoo, adrift]);
  assert.deepEqual(g.inBeds.map(p => p.patient_id), [1], "a bed is a state and a bed, as the ring counts it");
  assert.deepEqual(g.left.map(p => p.patient_id), [2, 3],
    "everybody else: went home, died, or on the chain with no bed the ward can offer");
}

// The heading carries both counts, and omits the half that is zero — "0 in beds" is a number
// nobody asked for on a page about a country.
assert.equal(countryHeading("South Korea", 1, 1), "South Korea · 1 in a bed · 1 left");
assert.equal(countryHeading("South Korea", 2, 3), "South Korea · 2 in beds · 3 left");
assert.equal(countryHeading("Nepal", 1, 0), "Nepal · 1 in a bed");
assert.equal(countryHeading("Nepal", 0, 2), "Nepal · 2 left");
assert.equal(countryHeading("Nepal", 0, 0), "Nepal");

// ── the door, and the people behind it ───────────────────────────────────────
//
// Founder's ruling, 18 ก.ย.: a third door between closed and open. In `preview` the factory fills
// the queue and the board publishes who is in it — so the globe rings the countries they are from,
// the panel lists them, and nothing on the page offers to treat anybody. Production spent the week
// before the fair behind a closed door with an empty board, which proved nothing to anybody who
// opened it.
//
// The rings first: a waiting patient is not in a bed, so she is not in the ring that counts beds.
// Her country gets its own count, and the legend says which is which.
assert.deepEqual(waitingCounts([]), {});
assert.deepEqual(
  waitingCounts([{ country: "BGD" }, { country: "THA" }, { country: "BGD" }, { country: "ZZZ" }]),
  { "050": 2, "764": 1 },
  "counted by the same country ids the beds ring uses, and a country the atlas cannot place is \
   left out of the ring rather than drawn somewhere wrong");

// The figures row says what the door is doing. In preview the beds are empty and saying "0 in
// beds" alone reads as a ward that lost its patients.
assert.deepEqual(figuresFor("open", { shifts: 14, went_home: 0, died: 0 }, [], 3, 20),
  [["in beds", 3], ["on shift now", 0], ["shifts", 14], ["went home", 0], ["died", 0]],
  "an open ward's figures are what they always were");
assert.deepEqual(figuresFor("preview", { shifts: 0, went_home: 0, died: 0 }, [], 0, 20),
  [["in beds", 0], ["waiting", 20], ["opening", "soon"]],
  "and a preview ward says what it is: nobody in a bed, twenty waiting, opening soon");
assert.deepEqual(figuresFor("closed", {}, [], 0, 0), [],
  "a closed ward publishes no queue and has no figures to give");

// What the panel says when there is nobody in a bed. Three different facts, and the founder read
// the wrong one on production: "the ward admits from its queue every minute" over a door that was
// shut and a queue that was empty.
assert.equal(bedsEmptyWords("closed", 0), "The door is closed · opening soon");
assert.equal(bedsEmptyWords("preview", 20), "20 patients are waiting · the ward opens soon");
assert.equal(bedsEmptyWords("preview", 0), "The ward opens soon · the queue is filling");
assert.equal(bedsEmptyWords("open", 0), "No patient in a bed · the queue is empty");
assert.equal(bedsEmptyWords("open", 4), "No patient in a bed · the ward admits from its queue every minute");
assert.equal(bedsEmptyWords("open", null), "No patient in a bed · the queue is empty",
  "a queue the ward could not count is not a queue with somebody in it");

// ── the build under an open tab ───────────────────────────────────────────────
// The founder read a ward three hours out of date in his own tab: the page had been fixed, his
// browser had the old one, and nothing in the answer had told it to ask. Pages are `no-cache` now,
// which fixes the next visit — and this is the tab that never navigates again. Every board the
// page receives carries the revision that served it; when that changes under an open tab, the page
// reloads itself, once.
//
// Once, and only between two revisions it has actually seen. A rollout serves two revisions at the
// same time and a board can arrive from either; a page that reloaded on every difference would
// bounce between them for as long as the rollout lasted.
assert.equal(shouldReload("00044", "00044"), false, "the same build is not a new build");
assert.equal(shouldReload("00044", "00045"), true);
assert.equal(shouldReload("", "00045"), false, "a page that never learned its own build stays put");
assert.equal(shouldReload("00044", ""), false, "and a board that does not say is not an answer");
assert.equal(shouldReload("00044", null), false);
assert.equal(shouldReload(null, null), false);

// ── the wire ─────────────────────────────────────────────────────────────────
// Pushed, and still polled: the 30-second refetch stays for browsers and proxies that drop the
// stream, and the stream's `ward` event goes to the same consumer the fetch feeds.
assert.match(script, /setInterval\(load, 30000\)/, 'the 30-second refetch stays');
assert.match(script, /new EventSource\("\/api\/ward\/stream"\)/, 'the stream is opened');
assert.match(script, /addEventListener\("ward", /, 'and its ward event is listened for');
assert.match(script, /function render\(j\)/, 'one consumer, render(j), for both the fetch and the stream');
assert.ok(!/toLocaleTimeString|toLocaleDateString|toLocaleString/.test(script), 'every absolute time goes through Intl.DateTimeFormat');

// ── doctors ──────────────────────────────────────────────────────────────────
// The globe's second layer: people per doctor, from one open series (World Bank SH.MED.PHYS.ZS,
// physicians per 1,000 people, WHO's numbers republished, CC BY 4.0). Every figure the page shows
// is a conversion of that series and nothing else: no value means "no data", never a guess.
assert.equal(peoplePerDoctor(0.159), 6290, '1000 / 0.159 = 6289.3, to the nearest ten');
assert.equal(peoplePerDoctor(3.681), 270, '271.7 → 270');
assert.equal(peoplePerDoctor(1.8609), 540, 'the world, 2022');
assert.equal(peoplePerDoctor(0.541), 1850, 'Thailand 2021 by this series — the deck\'s MOPH 1:1,487 is another source, and the page says which it uses');
assert.equal(peoplePerDoctor(0), null, 'zero physicians is not a number of people per doctor');
assert.equal(peoplePerDoctor(null), null);
assert.equal(peoplePerDoctor(undefined), null);
assert.equal(peoplePerDoctor(-1), null);

assert.deepEqual(latestOf([[2000, 1.0], [2010, 1.2], [2021, 0.541]]), [2021, 0.541]);
assert.deepEqual(latestOf([[2021, 0.541], [2000, 1.0]]), [2021, 0.541], 'sorted, whatever order it came in');
assert.equal(latestOf([]), null);
assert.equal(latestOf(null), null);
assert.equal(latestOf([[2020, 0]]), null, 'a zero is not a value');

// The ~10-year change, as % per year of PEOPLE PER DOCTOR (down is the mission's direction), between
// the latest value and the value nearest to ten years before it — whichever year exists, provided one
// exists at least eight years back. Years differ by country (Myanmar 2019, India 2020, Ethiopia
// 2023), so every figure carries its own years, and a short series is "too short", not a trend.
let t = tenYearTrend([[2011, 0.4], [2016, 0.45], [2021, 0.541]]);
assert.deepEqual([t.from, t.to, t.years], [[2011, 0.4], [2021, 0.541], 10]);
assert.equal(t.pct_per_year.toFixed(2), '-2.97', 'people per doctor fell from 2,500 to 1,850 over ten years');
t = tenYearTrend([[2005, 0.3], [2014, 0.35], [2023, 0.5]]);
assert.deepEqual([t.from, t.to, t.years], [[2014, 0.35], [2023, 0.5], 9], 'uneven: 2014 is nearer to 2013 than 2005 is');
assert.equal(t.pct_per_year.toFixed(2), '-3.89');
t = tenYearTrend([[2008, 0.5], [2015, 0.5], [2023, 0.5]]);
assert.deepEqual([t.from, t.years], [[2015, 0.5], 8], '2013 is nearer to 2015 (two years) than to 2008 (five); eight years back is the floor, and it counts');
assert.equal(t.pct_per_year, 0);
t = tenYearTrend([[2011, 0.5], [2015, 0.5], [2023, 0.5]]);
assert.deepEqual(t.from, [2011, 0.5], 'a tie (2011 and 2015 both two years from 2013) goes to the earlier year');
assert.equal(tenYearTrend([[2020, 1.0], [2023, 1.1]]), null, 'three years of history is a series too short for a trend');
assert.equal(tenYearTrend([[2016, 1.0], [2023, 1.1]]), null, 'seven years back is still too short');
assert.equal(tenYearTrend([[2021, 0.5]]), null, 'one point is not a trend');
assert.equal(tenYearTrend([]), null);
assert.equal(tenYearTrend(null), null);
t = tenYearTrend([[2000, 2.0], [2010, 1.0]]);
assert.equal(t.pct_per_year.toFixed(2), '7.18', 'fewer doctors → more people per doctor → positive');

assert.equal(fmtTrend(-1.234), '\u22121.2%/yr', 'a real minus sign');
assert.equal(fmtTrend(0.8), '+0.8%/yr');
assert.equal(fmtTrend(0), '0.0%/yr');
assert.equal(fmtTrend(-0.04), '0.0%/yr', 'what rounds to nothing is nothing, not "−0.0"');
assert.equal(fmtTrend(null), 'trend: series too short', 'and no trend says why');
assert.equal(fmtPeople(6290), '6,290');
assert.equal(fmtPeople(540), '540');
assert.equal(fmtPeople(12345), '12,345');

assert.equal(doctorLine('Kenya', [[2019, 0.15], [2020, 0.159]]), 'Kenya · 1 doctor per 6,290 people (2020)');
assert.equal(doctorLine('Hong Kong', []), 'Hong Kong · no data');
assert.equal(doctorLine('Hong Kong', null), 'Hong Kong · no data');
assert.equal(doctorLine('Nowhere', [[2020, 0]]), 'Nowhere · no data');

const WORLD = { source: 's', licence: 'CC BY 4.0', fetched: '2026-09-16', indicator: 'SH.MED.PHYS.ZS — Physicians (per 1,000 people)', note: '',
  countries: { WLD: { name: 'World', series: [[2000, 1.523], [2010, 1.491], [2022, 1.861]] },
               THA: { name: 'Thailand', series: [[2021, 0.541]] } } };
assert.deepEqual(worldAverage(WORLD), { people: 540, year: 2022 }, '1000 / 1.861 = 537.3, to the nearest ten like every other figure');
assert.equal(worldAverage({ countries: { THA: WORLD.countries.THA } }), null, 'no WLD, no average — never computed from the countries');
assert.equal(worldAverage(null), null);
assert.equal(missionLine(WORLD),
  'One doctor for every 540 people, world average (World Bank, 2022). We exist to bring that number down by 1% a year.',
  'the founder\'s sentence, exactly, with the two numbers from the series');
assert.equal(missionLine({ countries: { THA: WORLD.countries.THA } }), null, 'no world figure, no sentence — never a guess');

// The colour bins are a log-ish ladder, stated in the legend as they are here, spanning the series'
// extremes (Cuba 1:105 in 2021, Niger 1:26,316 in 2023).
assert.deepEqual(DOCTOR_BINS, [250, 500, 1000, 2000, 5000, 10000]);
assert.equal(doctorBin(105), 0);
assert.equal(doctorBin(249), 0);
assert.equal(doctorBin(250), 1);
assert.equal(doctorBin(540), 2);
assert.equal(doctorBin(1850), 3);
assert.equal(doctorBin(6290), 5);
assert.equal(doctorBin(10000), 6);
assert.equal(doctorBin(26320), 6);
assert.equal(doctorBin(null), null, 'no data is not a bin');

// The data the page ships: the fetch script's output, committed, and inlined into the page so
// it stays one file. Both are read here and held equal.
const data = JSON.parse(readFileSync(dataFile('physicians.json'), 'utf8'));
for (const k of ['indicator', 'source', 'licence', 'fetched', 'note']) assert.equal(typeof data[k], 'string', `${k} is stated`);
assert.match(data.indicator, /^SH\.MED\.PHYS\.ZS/);
assert.equal(data.licence, 'CC BY 4.0');
assert.match(data.fetched, /^\d{4}-\d{2}-\d{2}$/);
assert.deepEqual(Object.keys(data).sort(), ['countries', 'fetched', 'indicator', 'licence', 'note', 'source'], 'the reference shape, nothing else at the top');
const codes = Object.keys(data.countries);
assert.ok(codes.length >= 150, `enough countries to colour a globe: ${codes.length}`);
assert.ok(codes.every(c => /^[A-Z]{3}$/.test(c)), 'World Bank codes, three upper-case letters');
assert.ok(codes.includes('WLD') && codes.includes('THA') && codes.includes('KEN'));
assert.equal(data.countries.THA.name, 'Thailand');
for (const c of codes) {
  const s = data.countries[c].series;
  assert.ok(Array.isArray(s) && s.length > 0, `${c}: a series with values`);
  for (let i = 0; i < s.length; i++) {
    assert.ok(Number.isInteger(s[i][0]) && s[i][0] >= 2000 && s[i][0] <= 2024, `${c}: years in range`);
    assert.ok(typeof s[i][1] === 'number' && s[i][1] > 0, `${c}: values only, never null`);
    if (i) assert.ok(s[i][0] > s[i - 1][0], `${c}: ascending by year, one value per year`);
  }
}
const inlined = html.match(/<script id="physicians" type="application\/json">([\s\S]*?)<\/script>/);
assert.ok(inlined, 'the series is inlined into the page like the atlas');
assert.deepEqual(JSON.parse(inlined[1]), data, 'and it is the same data as the committed file — regenerate both with scripts/fetch-physicians.py');
assert.notEqual(worldAverage(data), null, 'the real series has a world average');
assert.match(html, /id="mission"/, 'the mission line has its place under the headline');
assert.match(html, /data-layer="doctors"/, 'the layer toggle is on the page');
assert.match(html, /people per doctor · World Bank\/WHO, latest year/, 'the legend says what the colours are and whose numbers');
assert.match(html, /population per physician/, 'and says once that it is population per physician, not patients');

// ── the year ─────────────────────────────────────────────────────────────────
// A slider from 2000 to the latest year in the file, with ▶ stepping a year a second. Everything
// the doctors layer shows follows the chosen year: the choropleth, the hover figure, the panel's
// highlighted point, the mission line's world average. A country with no value in that year uses
// its latest earlier value and says "(value from YEAR)"; with no earlier value it is hatched. The
// ward's ring and badges are today's whatever the year, and the legend says so. Default: latest.
const S = [[2001, 0.298], [2004, 0.289], [2010, 0.383], [2021, 0.541]];
assert.deepEqual(yearValue(S, 2010), { year: 2010, per_1000: 0.383, exact: true }, 'a year with a value');
assert.deepEqual(yearValue(S, 2015), { year: 2010, per_1000: 0.383, exact: false }, 'no value in 2015: the latest earlier one, and it says which');
assert.deepEqual(yearValue(S, 2023), { year: 2021, per_1000: 0.541, exact: false }, 'past the end: the latest');
assert.deepEqual(yearValue(S, 2021), { year: 2021, per_1000: 0.541, exact: true });
assert.equal(yearValue(S, 2000), null, 'before the first value: nothing, never a later value read backwards');
assert.equal(yearValue([], 2010), null);
assert.equal(yearValue(null, 2010), null);
assert.equal(yearValue([[2010, 0]], 2010), null, 'a zero is not a value');
assert.deepEqual(yearValue([[2021, 0.541], [2001, 0.298]], 2005), { year: 2001, per_1000: 0.298, exact: false }, 'in any order');

assert.deepEqual(yearRange(WORLD), { min: 2000, max: 2022 }, 'from 2000 to the latest year any country has');
assert.deepEqual(yearRange({ countries: { X: { series: [[2003, 1]] }, Y: { series: [[2019, 1]] } } }), { min: 2000, max: 2019 });
assert.deepEqual(yearRange({ countries: {} }), { min: 2000, max: 2000 }, 'no data is a one-year range, not a crash');

assert.equal(doctorLineAt('Kenya', [[2019, 0.15], [2020, 0.159]], 2020), 'Kenya · 1 doctor per 6,290 people (2020)');
assert.equal(doctorLineAt('Kenya', [[2019, 0.15], [2020, 0.159]], 2023), 'Kenya · 1 doctor per 6,290 people (value from 2020)', 'a fallback says which year it is from');
assert.equal(doctorLineAt('Kenya', [[2019, 0.15], [2020, 0.159]], 2005), 'Kenya · no data for 2005', 'nothing earlier: no data, with the year');
assert.equal(doctorLineAt('Hong Kong', null, 2020), 'Hong Kong · no data for 2020');
assert.equal(doctorLineAt('Kenya', [[2019, 0.15], [2020, 0.159]], 2020), doctorLine('Kenya', [[2019, 0.15], [2020, 0.159]]), 'at the latest year the two lines agree');

assert.deepEqual(worldAverageAt(WORLD, 2022), { people: 540, year: 2022, exact: true });
assert.deepEqual(worldAverageAt(WORLD, 2005), { people: 660, year: 2000, exact: false }, '1000 / 1.523 = 656.6 → 660, from 2000');
assert.deepEqual(worldAverageAt(WORLD, 2015), { people: 670, year: 2010, exact: false }, '1000 / 1.491 = 670.7');
assert.equal(worldAverageAt(WORLD, 1999), null);
assert.equal(worldAverageAt({ countries: {} }, 2010), null);
assert.deepEqual(worldAverageAt(WORLD, 2022), { ...worldAverage(WORLD), exact: true }, 'at the latest year the two agree');
assert.equal(missionLineAt(WORLD, 2005),
  'One doctor for every 660 people, world average (World Bank, 2000). We exist to bring that number down by 1% a year.',
  'the sentence names the year the value is from, never the year on the slider');
assert.equal(missionLineAt(WORLD, 2022), missionLine(WORLD));
assert.equal(missionLineAt(WORLD, 1999), null, 'no world figure, no sentence');

assert.match(html, /<input[^>]*type="range"[^>]*id="year"/, 'the year slider');
assert.match(html, /id="play"/, 'and its ▶');
assert.match(html, /ward[^<]{0,80}(today|now)[^<]{0,80}whatever the year|whatever the year[^<]{0,80}ward/i, 'the legend says the ward does not follow the year');
assert.ok(!/<input[^>]*id="year"[^>]*value="20(0\d|1\d)"/.test(html), 'the slider does not start in the past: the page sets it to the latest year');

// ── the face in the tray and the panel ───────────────────────────────────────
// Since 4920a43 the board's `portrait` is the 256 px sibling when one exists. The page shows it as
// served — the address is the ward's to choose — with the box sized for a 256 px picture and the
// browser told what it is loading; no size logic here, ever.
const T = 'https://storage.googleapis.com/vitals-world-portraits/' + 'a'.repeat(64) + '-256.webp';
const F = 'https://storage.googleapis.com/vitals-world-portraits/' + 'b'.repeat(64) + '.webp';
assert.equal(portraitImg({ portrait: T }), `<img src="${T}" width="56" height="56" loading="lazy" decoding="async" alt="">`, 'the thumbnail, as served');
assert.equal(portraitImg({ portrait: F }), `<img src="${F}" width="56" height="56" loading="lazy" decoding="async" alt="">`, 'a full picture, as served — the page does not rewrite it');
// A placeholder, and one that says whose it is. An em dash says "something is absent" and stops
// there; her initials say the patient is here and the picture is not — the distinction the founder
// had to make himself on production, from a gap, on 22 ก.ย.
assert.equal(portraitImg({ portrait: null, name: 'Haruto Sasaki' }),
             '<span class="ph" title="no picture yet" aria-label="Haruto Sasaki — no picture yet">HS</span>',
             'no picture is a placeholder carrying her initials, not a broken image and not a dash');
assert.equal(portraitImg({ name: 'Sagal' }),
             '<span class="ph" title="no picture yet" aria-label="Sagal — no picture yet">S</span>');
assert.equal(portraitImg({}),
             '<span class="ph" title="no picture yet" aria-label="this patient — no picture yet">?</span>',
             'and a row with no name at all still fills the square');
assert.equal(portraitImg({ portrait: 'x"y' }), '<img src="x&quot;y" width="56" height="56" loading="lazy" decoding="async" alt="">', 'the address is escaped, never trusted');
assert.ok(!/-256|_256/.test(script), 'no size logic in the page: the ward chooses the address');
assert.ok(!/\$\{l\[0\] === at \? "" : ""\}/.test(script), 'the no-op ternary is gone');

console.log('globe_logic: ok');

// ── a patient the ward cannot describe ────────────────────────────────────────────────────────
// She is on the chain and in no bed (CWF_PLAN ruling 5). The page knew four states and fell back
// to "on the ward" for anything else, so these appeared as ordinary patients somebody could take
// a shift on — and nobody can: the ward has no case for them. The label has to say what they are,
// and the row has to carry the sentence the endpoint sends.
{
  const adrift = {
    patient_id: 7, name: null, country: null, bed: null, state: "off_ward",
    note: "admitted outside the ward · no bed — she is on the chain and the ward has no pack for her",
  };
  assert.equal(stateOf(adrift), "off_ward",
    "a state the endpoint sends must not fall back to on_ward: a stranger would try to treat her");
  assert.equal(STATE_LABEL.off_ward, "admitted outside the ward",
    "and the label says what she is, in the endpoint's own terms");

  const line = stateLine(adrift, Date.parse("2026-09-16T12:00:00Z"));
  assert.ok(line.text.includes("admitted outside the ward"), `the row says it: ${line.text}`);

  // An unknown state still falls back, because the page must render something for a word it has
  // never heard of — what it must not do is pretend that word means "on the ward".
  assert.equal(stateOf({ state: "something-new" }), "on_ward");
}

// ── a patient the ward cannot describe is not on the board ────────────────────────────────────
// The founder opened staging, saw three chain-only test patients in the tray offering "take a
// shift", and called them dummies. He is right: they are on the chain and the ward has no case for
// them, so a stranger who clicks one gets a refusal and a product that wasted their time.
//
// They stay in /api/ward — chain truth, anyone can read it, with the sentence saying what they are
// — and they appear nowhere a stranger looks. `onBoard` is the one place that decides, so nothing
// downstream can put them back: not the counts, not the tray, not the panel, not the rail.
{
  const packed = { patient_id: 1, name: "Anita Shrestha", country: "NPL", bed: 1, state: "on_ward" };
  const adrift = { patient_id: 2, name: null, country: null, bed: null, state: "off_ward" };
  const gone = { patient_id: 3, name: "Fon", country: "THA", bed: null, state: "went_home" };

  const board = onBoard([packed, adrift, gone]);
  assert.deepEqual(board.map(p => p.patient_id), [1, 3],
    "a patient the ward cannot describe is not on the board at all — no row, no badge, no count");

  // And the counts the globe lights from never see her either, whatever her country said.
  const withCountry = { ...adrift, country: "NPL" };
  const counts = countryCounts(onBoard([packed, withCountry]), null);
  assert.equal(counts.byId[countryId("NPL")], 1,
    "one patient in Nepal, not two: the chain-only one is not a patient anybody can take");
}

// ── both layers at once ───────────────────────────────────────────────────────────────────────
// The founder, 16 ก.ย.: "ต้องการให้ default ทั้ง ward และ doctor เลย" — both, by default. The two
// facts are one sentence, not two modes: this country is short of doctors, and this country has
// somebody on the ward right now. Reading them together is the argument the product is making.
//
// A lit fill cannot survive over the ramp, so "lit" becomes an outline ring, and the count badge
// stays. The checkboxes are independent — a viewer can still isolate either — and never a radio,
// because a radio is what made the two facts alternatives in the first place.
{
  const both = { doctors: true, ward: true };
  const shortage = { fill: "ramp", ring: true };

  // A country with a shortage figure and somebody on the ward: coloured by the ramp, ringed for
  // the ward.
  const kenya = paintOf(3460, 1, both);
  assert.equal(kenya.fill, shortage.fill, "the ramp is the base fill when the doctors layer is on");
  assert.ok(kenya.bin !== null, "and it has a bin to colour with");
  assert.equal(kenya.ring, true, "a country with somebody on the ward is ringed, not filled");

  // No shortage figure: hatched, and still ringed if somebody is there.
  const nodata = paintOf(null, 2, both);
  assert.equal(nodata.fill, "hatch", "no figure is hatched rather than coloured as though it were zero");
  assert.equal(nodata.ring, true);

  // Ward alone: the old behaviour, a lit fill, because there is no ramp to sit on.
  const wardOnly = paintOf(3460, 1, { doctors: false, ward: true });
  assert.equal(wardOnly.fill, "lit", "with no ramp under it, lit is a fill again");
  assert.equal(wardOnly.ring, false, "and a ring on a lit fill would say the same thing twice");

  // Doctors alone: no ring, whoever is on the ward.
  const docsOnly = paintOf(3460, 3, { doctors: true, ward: false });
  assert.equal(docsOnly.ring, false, "the ward layer is off, so the ward is not drawn");
  assert.equal(docsOnly.fill, "ramp");

  // Neither: plain land. The page still draws a globe rather than nothing.
  assert.equal(paintOf(3460, 1, { doctors: false, ward: false }).fill, "land");

  // The hover says both facts in one line when both are on.
  const series = [[2023, 0.289]];   // years are integers, as the file has them
  const line = hoverText("Kenya", series, 1, both);
  assert.ok(line.includes("1 doctor per"), `the shortage: ${line}`);
  assert.ok(line.includes("1 on the ward"), `and the ward: ${line}`);
  assert.equal(hoverText("Kenya", series, 0, both).includes("on the ward"), false,
    "a country with nobody on the ward says nothing about the ward");
  assert.equal(hoverText("Kenya", series, 1, { doctors: false, ward: true }), "Kenya · 1 on the ward",
    "with the doctors layer off the line is the ward's alone");
}


// ── the way back to the beds ─────────────────────────────────────────────────
//
// Demo capture, item 3: a country panel on a desktop had no way out of it. The page ships a
// control and wires it (`$("close")` → `clearPanel`), and the stylesheet hid it at every width —
// `.panel .close{display:none}` in the sheet and again in the phone block — so the only way back
// to the beds was to find the country again on a globe that had turned since.
//
// The resting state of this panel *is* the beds, so the control is shown when there is somewhere
// to go back from: when a country is selected and the panel carries `.open`.
assert.ok(!html.includes('.panel .close{display:none}'),
          'nothing may hide the way out of the panel outright — it is the whole control');

const openClose = html.match(/\.panel\.open \.close\{([^}]*)\}/);
assert.ok(openClose, 'a country panel has to show its way back to the beds, at every width');
assert.ok(!/display: *none/.test(openClose[1]),
          `and showing it means showing it: ${openClose[1]}`);

const closeBtn = html.match(/<button[^>]*id="close"[^>]*>([^<]*)<\/button>/);
assert.ok(closeBtn, 'the panel ships the control');
assert.match(closeBtn[1], /beds/,
             'it says where it goes rather than "close": the beds are what it goes back to');
assert.match(closeBtn[0], /aria-label="[^"]*beds/i,
             'and a screen reader is told the same thing, not "Close"');

// A keyboard gets out too, at every width, and only when there is something to get out of.
assert.match(script, /Escape[\s\S]{0,160}clearPanel/,
             'Escape returns to the beds — a panel with one way out has one way out for a mouse');

console.log('globe_logic: ok (and the panel can be left)');

// ── a pointer off the globe is over the page, not over Antarctica ───────────
//
// Demo capture, item 6: hovering below the globe named Antarctica. d3's orthographic `invert`
// answers for points outside the disc too — it hands back the nearest point on the limb — and the
// limb below a globe tilted the way this one is tilted is the far south. The guard that was there
// compared the inverted point against the centre, and a limb point is exactly π/2 away, which is
// not *greater than* π/2.
//
// So the question is asked in the space the pointer is actually in: the disc the globe is drawn
// in, which is the projection's translate and scale.
assert.equal(onTheGlobe([200, 200], [200, 200], 150), true, 'the middle of the world');
assert.equal(onTheGlobe([200, 349], [200, 200], 150), true, 'a pixel inside the limb is still land or sea');
assert.equal(onTheGlobe([200, 350], [200, 200], 150), true, 'the limb itself belongs to the globe');
assert.equal(onTheGlobe([200, 351], [200, 200], 150), false, 'and one pixel past it is the page');
assert.equal(onTheGlobe([200, 420], [200, 200], 150), false, 'seventy px south of the limb is not Antarctica');
assert.equal(onTheGlobe([80, 80], [200, 200], 150), false, 'the corners of the box are not the globe either');
assert.equal(onTheGlobe([94, 94], [200, 200], 150), true, 'and a corner inside the disc is');

assert.match(grab('countryAt'), /onTheGlobe/,
             'the hit test has to ask it — a guard nothing calls is a comment');

console.log('globe_logic: ok (and the page is not Antarctica)');

// ── a bed the ward cannot open is not offered ───────────────────────────────
//
// Park Ji-woo and Yonas Haile sat in beds 1 and 2 on staging with "take a shift" beside them, and
// the ward could draw neither case: both rows were doors onto "Not on this page yet". The board now
// says `openable: false` with the reason, and the page has to believe it — a row offering a shift
// the server has already said cannot be taken is the worst kind of disagreement, because the
// stranger finds out after they have committed.
assert.equal(canTakeShift({ state: 'on_ward', bed: 1, openable: false }), false,
             'the server says this bed cannot be opened, so the page does not offer it');
assert.equal(canTakeShift({ state: 'on_ward', bed: 1, openable: true }), true);
assert.equal(canTakeShift({ state: 'on_ward', bed: 1 }), true,
             'a board that does not carry the field is an older ward, and its beds are takeable');

console.log('globe_logic: ok (and a bed that cannot be opened is not offered)');

// ── the film on the front page ──────────────────────────────────────────────
//
// Founder's ruling, 18 September: the film is an embed on the globe page, not a button. It is also
// the only thing on this page that fetches from somebody else's server, which is why every rule
// about it is checked here rather than trusted.
const iframes = html.match(/<iframe\b[^>]*>/g) || [];
assert.equal(iframes.length, 1, `the page carries exactly one embed: ${iframes.length}`);
const film = iframes[0];

assert.match(film, /src="https:\/\/www\.youtube-nocookie\.com\/embed\/uOpv-_d7-s0[?"]/,
             `from the no-cookie host, and the film the founder named: ${film}`);
assert.match(film, /[?&](amp;)?rel=0/, `no other channel's videos at the end of ours: ${film}`);
// `&amp;` is how an ampersand is written in an attribute, so the separator is either.
assert.match(film, /[?&](amp;)?modestbranding=1/, film);
assert.ok(!/autoplay/i.test(film),
          `nothing plays at somebody who has not asked for it: ${film}`);
assert.match(film, /loading="lazy"/,
             `and nothing is fetched from Google until the box is near the screen: ${film}`);
assert.match(film, /title="[^"]*Vital Signs[^"]*"/,
             `a screen reader is told what the frame is: ${film}`);
assert.ok(!/allow="[^"]*autoplay/.test(film), `not even permitted to: ${film}`);

assert.match(html, /Vital Signs · 3 min · the patients are simulated, the shortage is not\./,
             'the caption says what it is and how long it takes, in the ward\'s own sentence');

console.log('globe_logic: ok (and the film is embedded, not autoplayed)');

// ── a patient the ward cannot open is still a patient on the page ───────────
//
// The first caseless fix reused `off_ward`, and this page runs its whole patient list through
// `onBoard` — which drops every `off_ward` row, a rule written for the accounts that reached the
// chain outside the queue and have no pack, no name and no face. So Park Ji-woo and Yonas Haile
// vanished from the globe page altogether: two patients with names, charts and, in Yonas's case,
// an anchored shift. A dead-end link was traded for a disappeared patient, which is worse — the
// founder's first words when a patient went missing this week were "คนไข้ไปไหนละอ่ะ".
//
// So it is its own word. `off_ward` means the ward knows nothing about her, and says so in its
// label: "admitted outside the ward". `caseless` is nearly the opposite — the ward has her pack,
// her chart and her receipts, and cannot draw her case.
assert.equal(STATE_LABEL.caseless, 'no case to open',
             'the page has a word for her, and it is not the one that means the ward never knew her');
assert.equal(stateOf({ state: 'caseless' }), 'caseless', 'and it survives the state reader');

const shut = { patient_id: 1789528326, name: 'Park Ji-woo', state: 'caseless', bed: null,
               openable: false, why_not: 'the ward no longer holds this case' };
const gone = { patient_id: 1789490620, state: 'off_ward', bed: null };
const kept = onBoard([shut, gone, { patient_id: 7, state: 'on_ward', bed: 1 }]);
assert.deepEqual(kept.map(p => p.patient_id), [1789528326, 7],
                 'she stays on the page; the account the ward never knew still does not');

assert.equal(canTakeShift(shut), false, 'nobody is offered her, as before');

// And her row says why, with no bed to hang it on. `row` builds DOM, so what is asserted here is
// the rule in its source — that the sentence is not gated on a bed she no longer holds — and the
// rendering itself is driven in a browser against a board that carries her.
const rowSrc = grab('row');
assert.match(rowSrc, /openable === false/,
             'the row asks the board whether this patient can be opened');
assert.ok(!/openable === false && p\.bed/.test(rowSrc),
          `and does not hide the reason behind a bed she has already given back: ${rowSrc}`);

console.log('globe_logic: ok (and a patient the ward cannot open is still on the page)');

// ── a stale history is said on the row ───────────────────────────────────────
//
// /api/ward now carries `history: "history not refreshed this minute"` on a patient whose signature
// listing failed this read — her account was read, her chart may be a shift behind. The board says
// it; the row has to show it, where `why_not` already is, or the payload knows something the page
// does not and a stranger opens a bedside a shift behind without being told. `row` builds DOM, so
// what is asserted is the rule in its source, the same way the `openable === false` rule is.
const rowSrcStale = grab('row');
assert.match(rowSrcStale, /p\.history/,
             'the row reads the history note off the board');
assert.ok(!/p\.history\s*&&\s*p\.openable/.test(rowSrcStale) && !/p\.openable\s*&&\s*p\.history/.test(rowSrcStale),
          `and does not tie it to openable — a stale chart is not a shut bed: ${rowSrcStale}`);
console.log('globe_logic: ok (and a stale history is said on the row)');
