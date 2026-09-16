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

const sandbox = [grabConst('ALPHA3'), grabConst('SLOT_MS'), grabConst('STATE_LABEL'), grabConst('DOCTOR_BINS'), grab('countryId'), grab('countryCounts'), grab('visible'),
  grab('stateOf'), grab('whenMs'), grab('relative'), grab('absolute'), grab('stateLine'),
  grab('peoplePerDoctor'), grab('latestOf'), grab('tenYearTrend'), grab('fmtTrend'), grab('fmtPeople'), grab('doctorLine'),
  grab('worldAverage'), grab('missionLine'), grab('doctorBin'),
  'return { countryId, countryCounts, visible, ALPHA3, SLOT_MS, whenMs, relative, absolute, stateLine, DOCTOR_BINS, peoplePerDoctor, latestOf, tenYearTrend, fmtTrend, fmtPeople, doctorLine, worldAverage, missionLine, doctorBin };'].join('\n');
const { countryId, countryCounts, visible, ALPHA3, SLOT_MS, whenMs, relative, absolute, stateLine,
  DOCTOR_BINS, peoplePerDoctor, latestOf, tenYearTrend, fmtTrend, fmtPeople, doctorLine, worldAverage, missionLine, doctorBin } = new Function(sandbox)();

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

// ── countryCounts ────────────────────────────────────────────────────────────
const P = (id, country, extra = {}) => ({ patient_id: id, name: `p${id}`, country, state: 'on_ward',
  difficulty: 'student', endemic: false, ...extra });
const cc = countryCounts([P(1, 'THA'), P(2, 'THA'), P(3, 'IDN'), P(4, null), P(5, 'tha')]);
assert.deepEqual(cc.byId, { '764': 3, '360': 1 }, 'tallied by atlas id, case-folded, null excluded');
assert.equal(cc.unknown, 1, 'the null-country patient is counted, not dropped');
assert.deepEqual(countryCounts([]), { byId: {}, unknown: 0 }, 'an empty ward is an empty tally');
assert.deepEqual(countryCounts([P(1, 'XXX')]).byId, {}, 'an unknown code lights nothing');
assert.equal(countryCounts([P(1, 'XXX')]).unknown, 1, '…but she is still on the ward, in the tray');
// A real code the atlas has no polygon for (HKG at 110m) must also go to the tray: a patient who
// resolves to an id nothing can draw would otherwise be on the ward and on no screen.
const hk = countryCounts([P(1, 'HKG')], atlasIds);
assert.deepEqual(hk.byId, {}, 'HKG lights no polygon (there is none)');
assert.equal(hk.unknown, 1, 'HKG goes to the tray');
assert.equal(countryCounts([P(1, 'THA')], atlasIds).unknown, 0, 'a drawable country never goes to the tray');

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
// Every timestamp the page shows comes from the payload as a slot (relative to as_of_slot, 0.4 s
// each), unix seconds, or a Z-suffixed UTC string. The page renders the distance from now first
// and the absolute moment in the viewer's own zone on hover — never a server-side local time.
const NOW = Date.UTC(2026, 8, 16, 12, 0, 0);          // 16 Sep 2026 12:00:00Z
const AS_OF = 500_000_000;                             // the read's slot
assert.equal(SLOT_MS, 400, 'a slot is 0.4 s, as vitals_replay::SLOT_SECONDS says');
assert.equal(whenMs({ unix: NOW / 1000 - 360 }, AS_OF, NOW), NOW - 360_000, 'unix seconds → ms');
assert.equal(whenMs({ slot: AS_OF - 9000 }, AS_OF, NOW), NOW - 9000 * 400, 'a slot is as_of minus the gap, at 0.4 s a slot');
assert.equal(whenMs({ slot: AS_OF + 5 }, AS_OF, NOW), NOW, 'a slot past the read is now, never the future');
assert.equal(whenMs({ iso: '2026-09-16T11:54:00Z' }, AS_OF, NOW), NOW - 360_000, 'a Z string is UTC');
assert.equal(whenMs({ iso: '2026-09-16T11:54:00' }, AS_OF, NOW), null, 'a string with no zone is not a time the page will guess at');
assert.equal(whenMs({ slot: null }, AS_OF, NOW), null);
assert.equal(whenMs({ unix: 0 }, AS_OF, NOW), null, 'zero is the program\'s "never", not 1970');
assert.equal(whenMs({}, AS_OF, NOW), null);
assert.equal(whenMs({ slot: 10 }, null, NOW), null, 'a slot without the read\'s slot cannot be placed');

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

// stateLine: the words under a patient, and the title behind them.
const line = (p) => stateLine(p, AS_OF, NOW);
const onShift = line({ state: 'on_shift', on_shift_since: NOW / 1000 - 360, admitted_slot: AS_OF - 9000 });
assert.equal(onShift.text, 'on shift · 6 min');
assert.equal(onShift.title, `since ${absolute(NOW - 360_000)}`);
const waiting = line({ state: 'on_ward', on_shift_since: null, admitted_slot: AS_OF - 3 * 86_400_000 / 400 });
assert.equal(waiting.text, 'on the ward · admitted 3 days ago');
assert.equal(waiting.title, `admitted ${absolute(NOW - 3 * 86_400_000)}`);
const home = line({ state: 'went_home', admitted_slot: AS_OF - 900_000, closed_slot: AS_OF - 6 * 9000 });
assert.equal(home.text, 'went home · 6 h ago');
assert.equal(home.title, `left ${absolute(NOW - 6 * 3_600_000)}`);
const died = line({ state: 'died', admitted_slot: AS_OF - 900_000, closed_slot: AS_OF - 50 });   // 50 slots = 20 s
assert.equal(died.text, 'died · just now');
const bare = line({ state: 'on_ward' });
assert.equal(bare.text, 'on the ward', 'no time in the payload, no time on the page');
assert.equal(bare.title, '');
const future = line({ state: 'on_ward', handed_over: '2026-09-16T06:00:00Z' });
assert.equal(future.text, 'on the ward · handed over 6 h ago', 'a Z string the payload may carry later is read the same way');

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
const dataPath = new URL('../../data/physicians.json', new URL('file://' + process.argv[2].replace(/^(?!\/)/, process.cwd() + '/'))).pathname.replace('/static/world/../../data', '/data');
const data = JSON.parse(readFileSync(dataPath, 'utf8'));
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

console.log('globe_logic: ok');
