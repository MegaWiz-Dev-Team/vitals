# The world model — and why it solves the problem the rubric couldn't

> This supersedes the "40 proven / 60 attested" compromise in [RISKS.md](RISKS.md) §3.
> The rubric was never the right thing to anchor. The world model is.

---

## 1. Clinical State Machine Transition Model

```mermaid
stateDiagram-v2
    [*] --> AcuteState: Patient Ingests Allergen (T0)
    
    state AcuteState {
        direction LR
        AirwayClosing --> ShockDizzy: SBP 88 / HR 128
    }

    AcuteState --> ArrestDeath: Airway SpO2 < 0.15 (Untreated)
    AcuteState --> RecoveredState: Adrenaline IM Ordered (Timely)

    state RecoveredState {
        direction TB
        AdrenalineHeld --> Observation
    }

    RecoveredState --> HarmCollapse: Patient Stood Up / Walked (Harm Event Logged)
    HarmCollapse --> RecoveredState: Patient Laid Supine + Fluids

    RecoveredState --> BiphasicWin: Admitted for 6h Observation (WinDischarge)
    RecoveredState --> BiphasicDecline: Discharged Prematurely (Fatal Relapse)

    ArrestDeath --> [*]
    BiphasicDecline --> [*]
    BiphasicWin --> [*]
```

---

## 2. What Embla already runs

`engine/src/sce_runtime.rs` is a **data-driven hybrid automaton** — a physiology engine loaded from JSON, not hardcoded:

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

That enum is the format. The test harness for the physiology engine is, without anyone intending it, an **encounter replay format**.

---

## 3. The LLM vs Automaton Split

```mermaid
flowchart LR
    subgraph NonDeterministic ["Experience Layer (Off Proof-Path)"]
        LLM["Local LLM Gateway<br/><i>(Heimdall)</i>"]
        Dialogue["Patient Dialogue & History-Taking<br/><i>(Decides what player LEARNS)</i>"]
        LLM --> Dialogue
    end

    subgraph Deterministic ["Evidence Layer (On-Chain Proof-Path)"]
        Tape["Action Orders & Timestamps"]
        Automaton["Physiology Automaton<br/><i>(Decides what HAPPENS)</i>"]
        Leaf["Single Leaf Hash on Solana<br/><code>sha256(sce_hash ‖ tape ‖ outcome ‖ harm)</code>"]
        Tape --> Automaton --> Leaf
    end

    Dialogue -.->|Influences Player Choice| Tape

    style NonDeterministic fill:#F8FAFA,stroke:#C9D6D6
    style Deterministic fill:#E6F1ED,stroke:#0A5E4B,stroke-width:2px
```

| Part | Nature | In the proof path? |
|---|---|---|
| The patient's dialogue | LLM roleplay, non-deterministic | **No** |
| The world model — vitals, transitions, harm, outcome | deterministic automaton | **Yes** |

Score the player on **what they did to the world**, not on how well they chatted. Then:

```
leaf = hash(sce_id ‖ sce_hash ‖ action_trace ‖ outcome ‖ harm_events ‖ beats)
```

A verifier loads the same SCE JSON, replays the same tape, and must reach the same outcome. **No LLM anywhere in the verification path.**
