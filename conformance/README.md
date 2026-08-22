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
