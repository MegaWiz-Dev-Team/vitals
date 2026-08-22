# The world model — and why it solves the problem the rubric couldn't

> This supersedes the "40 proven / 60 attested" compromise in [RISKS.md](RISKS.md) §3.
> The rubric was never the right thing to anchor. The world model is.

## 1. What Embla already runs

`engine/src/sce_runtime.rs` is a **data-driven hybrid automaton** — a physiology engine loaded
from JSON, not hardcoded:

- continuous per-second **vital dynamics** inside the current state (`hr, sbp, dbp, spo2, rr, temp, gcs`, plus declared custom axes)
- global **triggers**
- discrete **state transitions**
- free-text **interventions**
- emitted **narrative beats**, accumulated **harm events**, and a terminal **outcome**

Its API is two calls:

```rust
SceState::tick(&mut self, dt: f64) -> Vec<NarrativeBeat>
SceState::apply(&mut self, action: &Action) -> Vec<NarrativeBeat>
```

And its own golden tests already drive it with a replay tape:

```rust
enum Step { Tick(f64), Do(Action) }
```

That enum is the format. The test harness for the physiology engine is, without anyone intending
it, an **encounter replay format**.

## 2. Why this changes the whole project

The verification problem was never the patient conversation. It was that we tried to anchor a
*rubric score*, 60% of which an LLM produced — so "anyone can re-derive this" was only ever 40%
true, and no amount of pinning model versions fixes a hosted model that can move under you.

Split the encounter instead:

| Part | Nature | In the proof path? |
|---|---|---|
| The patient's dialogue | LLM roleplay, non-deterministic | **No** |
| The world model — vitals, transitions, harm, outcome | deterministic automaton | **Yes** |

Score the player on **what they did to the world**, not on how well they chatted. Then:

```
leaf = hash(sce_id ‖ sce_hash ‖ action_trace ‖ outcome ‖ harm_events ‖ beats)
```

A verifier loads the same SCE JSON, replays the same tape, and must reach the same outcome.
**No LLM anywhere in the verification path.** The transcript never has to be revealed at all —
which also disposes of the privacy problem, since an action trace carries far less about a person
than a conversation does.

This is verifiable game replay: the pattern chess engines and speedrun leaderboards have used for
decades, applied to a patient who is dying on a clock. Judges recognise it in one sentence.

## 3. The honest caveat, again — and how it is smaller this time

The dynamics are `f64`, and the golden tests compare with a `1e-6` tolerance. Bit-identical float
replay across machines, compilers and architectures is not something to promise.

So anchor the **discrete** facts, which are robust to that noise:

- terminal `Outcome`
- the set of `harm_events`
- the ordered `NarrativeBeat`s
- coarse clinical `status` at each transition
- the action trace itself, with its tick schedule

Those are what the score should depend on anyway — *did the patient survive, were they harmed,
what did you do and when*. A 1e-6 wobble in diastolic pressure cannot flip any of them. If a
future version wants bit-exact trajectories, the interpreter moves to fixed-point, the same way
`proof-progress` did for the competency arithmetic.

State this the way we state everything else: the trajectory is simulated, the **outcome** is proven.

## 4. Agents stay outside the proof path — deliberately

`engine/src/agents.rs` gives the encounter its LLM surfaces: the patient persona and the
"phone-a-specialist" consult lifelines. Both stay off the proof path, but they are not
unaccounted for:

- **Consults are actions.** A consult appears in the action trace like any other move — which
  specialist, when, and what it cost. Using one well is rewarded; leaning on them is visible.
  The *cost accounting* is deterministic even though the *advice* is not.
- **The patient's words are not scored.** They shape what the player learns, which shapes what the
  player does, which is what gets scored. The LLM stays in the experience and out of the evidence.

That division is the point: **the model makes it worth playing, the automaton makes it worth
proving.**

## 5. What this unlocks for a general audience

A rubric that grades history-taking technique only means something to a clinician. A patient who
deteriorates on a clock while you decide what to do is legible to anyone — it is a game, and it
happens to be a real one.

- The **stakes are real, not arbitrary**: the deterioration is physiology, not a fake timer.
- **Nothing needs a medical vocabulary to be dramatic** — a falling SpO₂ reads on any screen.
- Every play is anchored the same way, whether the player is a resident or a curious teenager, so
  the same protocol serves a licensure-grade record and a leaderboard run.

The credential story and the game story stop being two products. They are the same replay,
read at different resolutions — which is the sentence this project has been organised around
from the start.

## 6. Consequences for the sprint

- The demo scores from the **action trace**, not the rubric. Simpler, and fully re-derivable.
- `proof-replay` joins `proof-progress` as a shared Rust crate: load SCE, replay tape, emit the
  discrete facts. Compiles into the app, the verifier, the wasm verify page.
- The three demo cases need **SCE definitions**, not just rubrics — the resident-tier dissection
  is the one that earns it, because it is the case where waiting is fatal.
- `docs/RISKS.md` §3 is downgraded from "the thing most likely to sink the pitch" to a footnote
  about float tolerance.
