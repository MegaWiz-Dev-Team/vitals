# Conformance vectors

`ep1-vectors.json` is the contract between Vitals and Embla's reference engine.

Embla's `engine/src/sce_runtime.rs` is where these semantics were first implemented and proven
against a retired hardcoded physiology engine. `crates/vitals-sce` is a second, independent
implementation of the same semantics with almost no dependencies — no database, no HTTP server,
no clock — so it can be built from a fresh clone, audited by a stranger, and compiled to wasm.

**The two are held together by these vectors, not by a shared build.** Neither crate depends on
the other. Eight tapes cover every terminal outcome the scenario can reach, plus the harm paths
and a fine-grained tick schedule:

| vector | outcome | covers |
|---|---|---|
| `treated` | WinDischarge | the textbook run |
| `stood_up` | WinDischarge | harm recorded on a surviving patient |
| `no_adrenaline` | DeathArrest | antihistamine-and-hope |
| `iv_push_harm` | *none* | intervention-level harm, no terminal state |
| `intubate_then_admit` | WinDischarge | airway path |
| `discharged_early` | DeathBiphasic | the biphasic relapse |
| `nothing_at_all` | *none* | pure decay, no actions |
| `fine_grained_ticks` | *none* | many small ticks vs few large ones |

## Regenerating

**A failing conformance test is never fixed by regenerating the vectors.** If the two
implementations disagree, one of them is wrong, and the vectors are how you find out which.

Regenerate only when the scenario or the specification changes deliberately. The generator lives
in Embla, because that is where the reference implementation lives — it is not in this repo on
purpose, so that no change here can quietly move the goalposts.

`sce-anaphylaxis-ep1.json` is the scenario the vectors were frozen against, copied in so this
repo builds and tests standalone. Its sha256 is recorded in `ep1-vectors.json`; if the file
changes, the vectors are stale by definition.

## `sce-archive/` — every scenario version an anchored run was played against

A run's identity on chain is `sce_hash = sha256(<the whole scenario file>)`. Rewriting a scenario
therefore orphans every leaf that named the old one — which is correct, and was also unusable:
the disk held the current file and nothing else, so the hash in a leaf resolved to nothing a
stranger could fetch. "Deterministic, re-derivable by anyone" quietly meant "re-derivable by
whoever has our repository and guesses the right commit".

`sce-archive/<sce_hash>.json` is the copy of each version, named by its own digest, with
`INDEX.json` mapping hash → the path it was archived from → its length. Because the archive is
committed, the route that resolves a hash needs no server at all — from the root of a clone:

```bash
grep -A1 <sce_hash> conformance/sce-archive/INDEX.json
shasum -a 256 conformance/sce-archive/<sce_hash>.json     # → <sce_hash>
```

That is the check `verify_player` prints beside every attempt it reads off the chain, and the one
`VERIFICATION.md` §5 walks through. It is the stronger of the two routes, not the fallback: the
archive travels in a git history a stranger can diff, and we cannot serve one reader a different
copy of it than we serve anyone else.

### `GET /api/sce/<sce_hash>` — versions of retired **cases** only

```
GET /api/sce/<sce_hash>     →  200, the exact bytes whose sha256 is that hash — if the CASE that
                               version belongs to has left the shelf
                            →  404 "that scenario is in active use" — these bytes are on the
                               shelf right now
                            →  404 "…is a version of a case in active use" — an earlier version
                               of a case that can still be sat
                            →  404 "…cannot be attributed to a case" — archived with no
                               INDEX.json row, so the server cannot tell which case it is of
```

Anyone can check a 200 with `sha256sum` on the reply. It is verified before it is sent: bytes that
do not hash to the requested value are a 404, never a wrong answer.

**Why retirement gates it.** The same file that lets an outsider re-derive a score is the file that
contains the answers. A scenario carries every intervention id, every matcher keyword in every
language, every `(HARM)` the author wrote beside a wrong turn, the trigger thresholds that decide
the outcome, and `_note` fields that name the diagnosis outright. The first cut of this route
resolved through the live shelf as well as the archive — "a run anchored ten minutes ago names a
file nobody has archived yet, and it has to resolve" — so a candidate could open a station, read
`sce_hash` off their own screen, and GET the whole mark sheet in one unauthenticated request while
the clock was still running. A star measured that way measures nothing.

So the shelf is a **deny list**, checked first, read fresh on every request: a hash is refused
while its case is playable, whether or not the archive also holds a copy — and it holds one for
every case in the season, which is exactly why "serve from the archive" is not by itself the fix.

**Retirement is a fact about a case, not about a byte sequence.** The first cut of this rule tested
byte equality against the shelf, and that is not the same question. A scenario's identity is its own
sha256, so *any* edit mints a new hash and leaves the old one matching nothing on the shelf — which
that rule read as "retired" and published. Nothing had retired; one file in a live case had rotated,
and the version it replaced was still that case's mark sheet in every respect that matters. Measured
on the real thing: `ep2`'s previous version differs from the live one by three lines of `rhythm` —
the same ten intervention ids, the same matcher keywords, the same `nitrate in RV infarct` trap at
the same threshold. Under the old rule, fixing a live case would have opened the endpoint that
exists to keep a candidate off their own mark sheet.

So the deny list asks the question retirement actually poses — *can this case still be sat?* — and
answers it from `INDEX.json`, which already recorded the file each version was archived from. Every
version of a live case is withheld together, and they all publish on the same day: the day that case
comes off the shelf. **Editing a case publishes nothing.** A version with no `INDEX.json` row is
refused too, because an unattributed version cannot be shown not to be a live case's answer key —
adding the row is what publishes it.

At the time of writing no case has retired, so the endpoint publishes nothing and the startup line
says so (`0 publishable · 21 live and withheld`) rather than printing a count that reads like a
working endpoint. That is the behaviour, not an outage.

**How to verify a live case in the meantime:** the `shasum` above, on your own clone. It proves
exactly what the fetch would have proved, with no server in the loop — so for a case in the current
season the repository is not the backup, it is the route.

**Append only. Never delete a file here** — deleting one destroys the evidence for every run that
was anchored against it. Adding a version is a file drop plus a row in `INDEX.json`; nothing in
the server needs editing.

This directory is under `conformance/` and not under `docs/` deliberately: `docs/internal/` is in
both `.gitignore` and `.dockerignore`, so an archive kept there reaches no clone and no image, and
the endpoint would 404 for every historical hash in production. `crates/vitals-web/src/archive.rs`
has a test that pins the Dockerfile's `COPY conformance`.

## Authoring rules that are not in the schema

`Sce::validate` catches what is malformed — an effect naming an outcome the file never declares, a
transition to a state that does not exist. What follows is the other kind: a scenario that
validates, runs, scores correctly, and still leaks. There is nothing in the engine that can enforce
these, because from the engine's side nothing is wrong.

### Every intervention gets a narrative beat — including the harmful ones

Every entry in `interventions` carries a `{"beat": "..."}` effect. Not only the right answer: the
trap, the wrong dose, the premature discharge, the order that changes nothing. If a harmful order
genuinely has nothing to narrate, **the beat still has to exist and it has to read as
unremarkable** — the room noting what was done, in the register the file already uses for a
correct order with no physiological consequence.

**Why:** the engine emits exactly the beats the file declares, so an intervention with no beat
returns a reply one line shorter than every other order — and if the silent orders are the harmful
ones, silence is the answer key. Silence must never correlate with wrongness.

That is measured, not theorised. Across the twelve stations shipped in August 2026 a reply with no
new beat was roughly 14× more likely to follow a harmful order than a harmless one, entirely
because the traps had been written without beats while the right answers had them —
`docs/RISKS.md` §11 has the numbers, before and after. The twelve stations were re-issued in
September 2026 under the retirement flow below, and since then no order in any station replies
with nothing. The rule is pinned by `crates/vitals-replay/tests/trap_silence.rs`, which reads
every station off disk: a station that declares an intervention with a silent effect path, or a
trigger that records a harm and narrates nothing, fails the build — so it holds for week nine's
author exactly as it held for week two's.

Two things the rule does **not** ask for. The beat must not announce the harm: *"the tongue
depressor goes in — she screams"* is what the `harm` field is for, and `harm` is withheld until the
bell in exam mode. And the beat must not read differently from the ones around it — a line that is
conspicuously flat is the same signal with one extra step in front of it.

### Retiring and re-issuing a station

A scenario is edited when it is retired and re-issued, and not before: any edit changes
`sce_hash`, and `sce_hash` is the case's identity on chain, so editing a live case orphans every
proof anchored against it. Retirement is therefore also the only moment at which the rules above
can be applied to a case that is already on the shelf, and the audit belongs there.

Before a re-issued version goes back on the shelf:

1. **Every declared harm has a beat.** Walk `interventions` and the `harm` effects in `triggers`:
   each one that can be reached by a candidate must leave a narrative beat behind it, and that beat
   must be indistinguishable in tone from the beats a correct order produces. This is the step that
   closes `docs/RISKS.md` §11 for that station, and it is the reason the station is being touched.
2. **Archive the outgoing version** — copy it to `sce-archive/<its own sha256>.json` and add its
   row to `INDEX.json`. Append only; the old file stays forever, because the leaves that name it do.
3. **Check the new hash is not already archived** — `shasum -a 256` the new file and grep
   `INDEX.json`. A collision means nothing changed and the re-issue is a no-op.
4. **Neither hash answers `GET /api/sce/<sce_hash>` yet**, and that is correct: re-issuing a
   station rotates its hash, it does not retire the case. Both versions stay withheld — together —
   until the case itself leaves the shelf, at which point every version of it publishes at once.
   Nothing in the server needs editing for either.
