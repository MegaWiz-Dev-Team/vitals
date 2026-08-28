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
`INDEX.json` mapping hash → the path it was archived from → its length. The server serves them at

```
GET /api/sce/<sce_hash>     →  the exact bytes whose sha256 is that hash
```

which anyone can check with `sha256sum` on the reply. It is verified before it is sent: bytes that
do not hash to the requested value are a 404, never a wrong answer.

**Append only. Never delete a file here** — deleting one destroys the evidence for every run that
was anchored against it. Adding a version is a file drop plus a row in `INDEX.json`; nothing in
the server needs editing.

This directory is under `conformance/` and not under `docs/` deliberately: `docs/internal/` is in
both `.gitignore` and `.dockerignore`, so an archive kept there reaches no clone and no image, and
the endpoint would 404 for every historical hash in production. `crates/vitals-web/src/archive.rs`
has a test that pins the Dockerfile's `COPY conformance`.
