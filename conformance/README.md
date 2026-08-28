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

### `GET /api/sce/<sce_hash>` — retired versions only

```
GET /api/sce/<sce_hash>     →  200, the exact bytes whose sha256 is that hash — if that version
                               has been retired
                            →  404 {"error":"that scenario is in active use"} — if the case it
                               names can still be sat
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
Retirement is what publishes a case. Edit a scenario or withdraw it and the hash the old leaves
name stops being sittable, so the bytes go out and those leaves stay checkable forever. At the time
of writing nothing is retired, so the endpoint publishes nothing and the startup line says so
(`0 publishable · 17 live and withheld`) rather than printing a count that reads like a working
endpoint. That is the behaviour, not an outage.

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
