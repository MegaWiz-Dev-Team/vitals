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

const sandbox = [grabConst('ALPHA3'), grabConst('SLOT_MS'), grab('countryId'), grab('countryCounts'), grab('visible'),
  grab('whenMs'), grab('relative'), grab('absolute'), grab('stateLine'),
  'return { countryId, countryCounts, visible, ALPHA3, SLOT_MS, whenMs, relative, absolute, stateLine };'].join('\n');
const { countryId, countryCounts, visible, ALPHA3, SLOT_MS, whenMs, relative, absolute, stateLine } = new Function(sandbox)();

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
const died = line({ state: 'died', admitted_slot: AS_OF - 900_000, closed_slot: AS_OF - 150 });
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

console.log('globe_logic: ok');
