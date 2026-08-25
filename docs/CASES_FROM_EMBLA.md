# Consuming cases from `embla-cases`


> Written 2026-08-24. Scope: **how Vitals gets its OSCE cases.** Nothing else.
>
> The short version: **nothing migrates.** `embla-cases` is already the single source of truth,
> already consumed as a pinned dependency by Embla and Askr, and already has the machinery for a
> third consumer. Vitals registers as a deployment target. That is the whole design.

---

## 1. What already exists — checked, not assumed

```
cases/<id>/case.json        content · `hidden` block = exam layer (answers, rubric)
cases/<id>/case.meta.yaml   version · content_hash · lifecycle · specialty · deployments
registry.jsonl              catalog surface
deployments.jsonl           who deployed what version, at which hash, and whether it drifted
tools/deploy-strip.py       removes `hidden` → dist/deploy-safe/   (Askr Gate-0)
tools/deploy-track.py       backfill | drift | list | record
tools/validate.py           CI gate
```

Real values, from `auth-acne-vulgaris-y4-1`:

```yaml
version: 1.0.0
content_hash: "sha256:e38e4aa032a8b8df"
lifecycle: validated
deployments: [cloud]
```

```json
{"case_id":"auth-acne-vulgaris-y4-1","target":"cloud","deployed_version":"1.0.0",
 "deployed_hash":"sha256:e38e4aa032a8b8df","deployed_at":"2026-08-18","status":"active"}
```

**`deployments.jsonl` already models exactly what Vitals needs**: a named target, a pinned version,
the hash that was actually shipped, and `deploy-track.py drift` to detect when a deployed case no
longer matches source. Vitals becomes `target: "vitals"`. No new format, no new tool.

---

## 2. The join that makes this worth doing

`ProvenAttempt.case` is `[u8; 32]` — a 32-byte case identifier already in the on-chain record.

**Make it the `embla-cases` content hash.** Then an anchored attempt names the exact case content
that produced the score, and a verifier can confirm they scored the same bytes the learner faced.
This is the Case Registry from `README.md` — *"content stays off-chain, only case_id +
content_hash + rubric_hash goes onchain"* — and `embla-cases` has already been carrying the
content hash all along.

**One concrete mismatch to fix:** `content_hash` is stored truncated — `sha256:e38e4aa032a8b8df`
is 16 hex characters, 8 bytes. The on-chain field is 32. **Put the full SHA-256 digest on chain**
and keep the truncated form as the human-readable label. A truncated hash is fine for a filename
and not fine as the thing a credential rests on.

Also worth adding while touching this: `rubric_hash` alongside `content_hash`, so the deterministic
scorer's inputs are pinned separately from the presentation content.

---

## 3. The split that keeps exam security intact

`deploy-strip.py` removes the whole `hidden` block — `correct_diagnosis`, `differential`,
`expected_workup`, `red_flags` — with a `leaks()` defence-in-depth check and a `--check` CI gate.

But the deterministic 40 **needs** that block: `grade_diagnosis` needs the correct diagnosis,
`investigation_score` needs the expected workup, and red flags are scored dimensions.

**So the split is not "what ships" — it is "where the code runs".**

```
learner's browser   ← deploy-safe bundle (hidden stripped)     presentation only, never scores
scoring server /    ← full case.json including hidden          produces the score
  verifier
chain               ← content_hash + rubric_hash + score       no content, ever
open repo           ← the algorithm                            never the cases
```

**Open algorithm, controlled content.** The scorer being public is the point; the cases being
public would end Askr as an exam product on the same day.

This also settles a question the code leaves ambiguous: `vitals-sce` compiles to wasm so an
**auditor or verifier** can re-derive an outcome from a fresh clone — *not* so the game client can
score itself. A client-side scorer would ship the rubric to the learner.

---

## 4. **Trap** — OSCE conformance vectors would publish answer keys

`conformance/ep1-vectors.json` is safe because an SCE scenario is a physiology spec with no answer
key. **OSCE vectors are not the same thing.** A vector that binds `vitals-osce` to Embla's scorer
has to contain a case *and* its expected rubric output — which is an answer key, in a repo intended
to be readable by strangers.

**Fix: author one public specimen case for conformance.** Real enough to exercise
`score_sop()` and `score_deterministic()` end to end, deliberately published, and **never used in
any exam or deployed to any learner cohort.** Mark it in `case.meta.yaml` with a lifecycle or flag
that `deploy-track.py` refuses to deploy anywhere.

Cost: one case. It buys auditability without spending exam security, and there is no other way to
have both.

---

## 5. What to build

1. **Register `vitals` as a deployment target** in `embla-cases` — `deploy-track.py record`, and
   add `vitals` to the `deployments:` list on the cases in scope. *No code.*
2. **Widen `content_hash` to the full digest** in `case.meta.yaml`, keeping the short form as a
   label. Add `rubric_hash`. Run `registry-build.py`.
3. **Case loader in Vitals** reading the pinned `deploy-safe` bundle for presentation, and the full
   case for the scorer, pinned by hash. Refuse to run on a case whose hash does not match what is
   recorded in `deployments.jsonl` — drift should fail loudly, not score quietly.
4. **`ProvenAttempt.case` = full content hash.** Small change, large consequence.
5. **The public specimen case** for OSCE conformance. §4.

Start with the subset the demo needs, not all 433. `deploy-track.py list` and the `lifecycle:
validated` field already give the selection criteria.

---

## 6. What is deliberately not happening

- Cases are **not copied into the Vitals repo.** They are a pinned external dependency, as they are
  for Embla and Askr. A copy would fork the source of truth on day one
- The `hidden` block **never enters the Vitals repository**, public or private
- `grounding/corpus.jsonl` is internal and contains answers — it is not an input to anything here
- No change to how Embla or Askr consume the same library
