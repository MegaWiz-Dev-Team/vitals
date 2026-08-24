# AttemptRecord `vt02` — the record change, specified

> Written 2026-08-24 in answer to: *does the anchored record carry evidence that a commitment
> existed?* **Yes. Reasoning and exact layout below.**

---

## 1. The format was already built for this

`record.rs` ends its encoding with a version tag and says why:

```rust
// Version tag last, so a future encoding can be told apart from this one.
out[133..137].copy_from_slice(b"vt01");
```

So this is **`vt02`, not a rewrite.** A verifier dispatches on the tag; `vt01` leaves stay
verifiable under `vt01` rules forever. The comment that *"change it and every leaf ever anchored
stops verifying"* applies to changing field order **within** a version — which is exactly what the
tag exists to avoid.

In practice it is moot today: devnet has zero leaves. But it means the change does not have to be
perfect on the first try, which is worth knowing before designing under pressure.

---

## 2. Why the commitment belongs in the leaf

**The decisive reason is the upgrade authority.** The program is upgradeable
(`AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiq`). "The program enforced a commitment at anchor
time" is therefore only as strong as *"I can prove which program ran at that slot."* A later
version that anchors without commitments would make old and new records indistinguishable.

**The practical reason is retention.** The commitment account is consumed at anchor. Closed
accounts leave current state, and most RPC providers prune transaction history — so a verifier
arriving a year later with a standard RPC **cannot reconstruct whether a commitment existed.**
The leaf is the only durable place.

That difference is precisely *re-derivable* versus *take our word for it*, which is the axis the
whole project sits on.

---

## 3. `vt02` layout

Existing bytes `0..133` keep their order and meaning. New fields append; the tag stays last.

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0..32 | 32 | `player` | unchanged |
| 32..64 | 32 | `sce_hash` | unchanged |
| 64..96 | 32 | `case` | **now the full SHA-256 content hash** from `embla-cases`, not the truncated form |
| 96..128 | 32 | `run_hash` | unchanged |
| 128 | 1 | `difficulty` | unchanged |
| 129 | 1 | `exam_mode` | unchanged |
| 130 | 1 | `outcome` | unchanged |
| 131..133 | 2 | `harm_count` | unchanged |
| **133..165** | **32** | **`commitment`** | `hash(case ‖ player ‖ nonce)` — *which* commitment |
| **165..173** | **8** | **`committed_slot`** | u64 LE — *when*, and that it came first |
| **173..205** | **32** | **`rubric_hash`** | scorer inputs pinned separately from presentation content; zero for SCE runs |
| **205..207** | **2** | **`det_score`** | u16 LE |
| **207..209** | **2** | **`det_max`** | u16 LE |
| **209..211** | **2** | **`judged_score`** | u16 LE |
| **211..213** | **2** | **`judged_max`** | u16 LE |
| **213..217** | **4** | version tag | `b"vt02"` |

**217 bytes.**

### Why both `commitment` and `committed_slot`

The hash binds *which* commitment; the slot binds *that it came first*. Either alone is
insufficient.

### Why four score fields rather than one

`README.md` already specifies it: *"the anchor carries two labelled numbers, not one"* — `det_score`
is re-derivable by re-running the pinned engine; `judged_score` is verifier-attested. Storing the
maxima as well removes any future ambiguity about what a number was out of.

**It falls out cleanly for both modes:**

| | `det_score` | `det_max` | `judged_score` | `judged_max` |
|---|---|---|---|---|
| SCE (story) — already fully deterministic | `score()` | 100 | 0 | **0** |
| OSCE (practice) | rubric deterministic part | 40 | LLM-judged part | 60 |

A `judged_max` of zero says *"nothing here required a witness"*, which is a true and useful thing
for a story-mode leaf to state about itself.

**This is the change that cannot be retrofitted.** If the two are ever summed into one field, the
question *"does the deterministic 40 alone predict passing?"* becomes unanswerable — and if it does
predict, that is the strongest result the project can produce, because a third party can recompute
it without trusting our model, our version, or us.

---

## 4. The trap to close in the same change

The claim is *"the chain already knows how many times you started."* It depends on **unused
commitments remaining countable.**

**If a player can close an unused commitment to reclaim rent, they erase the evidence of the runs
they did not like** — commit five times, play five times, anchor the best, tidy away the other
four.

Two ways out, pick one now rather than later:

- commitments cannot be closed at all — simplest, costs the player rent
- **closable, but decrementing nothing: a per-player monotonic `started` counter that only ever
  increases** — rent comes back, the count survives. **Recommended**

Without one of these the headline claim is decorative.

---

## 5. Downstream

- `crates/vitals-progress/src/record.rs` — `AttemptRecord`, `encode`, `leaf`, `to_attempt`
- `crates/vitals-program/src/lib.rs` — `RecordWire` gains the same fields; `ProvenAttempt` derives
  `score`/`max` as `det + judged` for progression while the split stays in the leaf
- New: commitment account + instruction; `AnchorReplay` requires and consumes one and copies
  `commitment` and `committed_slot` into the record **from the account, never from the caller**
- Tests bound to leaf hashes; the `chain_flow` suite now running 4/4 on devnet
- `conformance/ep1-vectors.json` is unaffected — it binds physiology semantics, not leaf encoding

---

## 6. What not to do

- **Do not let the client supply `commitment` or `committed_slot`.** They are read from the
  commitment account by the program. A caller-supplied field proves nothing
- **Do not truncate the commitment hash.** 32 bytes; a shortened hash weakens the binding for
  nothing
- **Do not derive `judged_score` from `det_score`,** or reuse one field for both
- **Do not claim more than this proves.** It establishes that a commitment naming this case and
  player existed before the anchor. It does not establish that it preceded the *gameplay* — for
  that the run's start would have to be bound into `run_hash`. The property that matters is intact:
  five practice runs need five visible commitments
