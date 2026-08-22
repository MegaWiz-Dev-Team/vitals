# embla-proof

**Proof of Competence — verifiable clinical-skill credentials on Solana.**

> Status: design / kickoff 2026-08-22 · v0.1.0 (reserved) · Not yet implemented.
> Spin-off of [Embla](../Embla) for the Colosseum / Solana hackathon track.
> License: **AGPL-3.0 + Commercial** (Asgard policy).

---

## The problem

A medical student's skill is invisible until an exam board says otherwise, and that
verdict arrives once a year, costs a fortune to administer, and travels badly across
institutions and borders.

Meanwhile the *practice* that actually builds the skill — hundreds of patient encounters,
scored against a rubric — produces a rich, continuous competency signal that nobody can
verify, port, or trust. It sits in one school's database, and everyone downstream
(the next school, the hospital, the licensing board, the employer) throws it away and
starts over.

Digital credentials have been tried. They fail for two reasons:

1. **Nobody trusts a self-reported score.** A PDF certificate, or an NFT minted by the
   same app that produced the score, proves nothing about whether the exam was fair.
2. **The economics never worked.** Anchoring millions of individual attempts, and paying
   the educators who author cases a few satang per attempt, is impossible on card rails
   and impossible on most chains.

## What embla-proof is

An open protocol that turns clinical-skill practice into **portable, tamper-evident,
independently re-scorable competency records** — and pays case authors per attempt.

Three onchain primitives, one off-chain engine:

| Layer | What it does | Solana primitive |
|---|---|---|
| **Case Registry** | Authors publish cases; content stays off-chain, only `case_id + content_hash + rubric_hash + price + royalty split` goes onchain. Any front-end can read it. | Anchor program + Token-2022 |
| **Attempt Anchor** | Commit–reveal per attempt: pre-commit before the encounter, hash of `(transcript, rubric result, engine version, model id)` after. Nothing personal onchain — hashes only. | State compression (Bubblegum concurrent merkle tree) |
| **Progression** | XP, levels, Dreyfus skill trees and badges — minted **permissionlessly**, because the program recomputes the predicate from anchored attempts instead of trusting a server. Soulbound. | Token-2022 NonTransferable + cNFT |
| **Competency Credential** | Clear N attempts above threshold → an accredited issuer attests "OSCE-Cardio-L2" to the student's wallet. Reusable across apps without exposing the underlying data. | [Solana Attestation Service](https://solana.com/news/solana-attestation-service) |
| *(off-chain)* **Verifier** | Re-runs the deterministic rubric over a revealed transcript and confirms it reproduces the anchored score. | `embla-engine` (Rust) |

## One anchored record, three resolutions

Everything here is the same attempt data read at three zoom levels — not three products:

| Resolution | Stakes | Who can mint |
|---|---|---|
| **Attempt** — the anchored leaf | raw evidence | verifier |
| **Progression** — level, skill tree, badge | low, continuous | **anyone; the program checks the maths** |
| **Credential** — competency attestation | high, institutional | accredited issuer |

If a feature cannot be introduced as a consequence of that sentence, it does not belong in the pitch.

## Why this is not "certificate NFT" or "achievement badges"

The novelty is **exam integrity as a mechanism**, not storage:

- **Commit–reveal kills the retry cheat.** The client commits `hash(case_id ‖ student ‖ nonce)`
  *before* the encounter starts. You cannot practise five times and then anchor only the good
  run — the chain already knows how many times you started.
- **Deterministic scoring makes the score disputable.** Embla's rubric scorer is deterministic
  and the engine/model version is pinned in the anchor, so a second party can re-run the same
  transcript and must get the same number. A credential you can *re-derive* is worth more than
  a credential you can only *read*.
- **The student cannot self-sign.** Scoring runs in a verifier that holds the issuer key
  (the school's Embla box, or a protocol verifier node). High-stakes attempts can require an
  n-of-m verifier quorum.
- **Progression is computed, not granted.** Embla's `xp_for`, `level_for` and `dreyfus` are pure
  functions over attempt history with unit-tested thresholds. An integer twin of them runs *inside
  the Anchor program*: `claim_progress` takes merkle proofs, the program recomputes the level, and
  mints only if its own arithmetic agrees. Every other achievement NFT is minted because a server
  said so — ours because the chain checked. See [docs/GAMIFICATION.md](docs/GAMIFICATION.md).
- **The hash chain already exists.** `embla-engine` ships a hash-chained append-only audit log
  (`engine/src/audit.rs`) with a canonical hash recipe cross-validated between its Rust and
  Python implementations. Today it is tamper-evident *to whoever holds the log*. Anchoring the
  chain head to Solana makes it tamper-evident *to the world*. That is the whole delta.

## What is provable, and what is only attested

Embla's rubric is **40 points deterministic and 60 points LLM-judged**. So the anchor carries two
labelled numbers, not one:

- `det_score` (40) — **re-derivable.** Re-run `embla-engine` at the pinned version, get the same
  bytes. This is the strong claim.
- `judged_score` (60) — **verifier-quorum attested.** Signers state which model at which version
  produced the dimension scores. More signers means more confidence; it never means re-derivable.

Escrow-backed badge predicates are expressible over `det_score` alone. We state this split up front
rather than letting someone find it — see [docs/RISKS.md](docs/RISKS.md) §3.

## Why Solana specifically

Not decoration — the numbers only close on this chain:

- **Volume.** ~25k Thai medical students × ~200 cases = millions of attempt anchors per year,
  and that is one country. Per-write cost has to be ~$0.0001 or the model is dead.
  Solana state compression mints ~1M compressed records for roughly **$110** total.
- **Micropayments.** Case authors earn ฿0.5–2 per attempt. Card rails eat that whole amount in
  fees; Solana settles it with change left over. This is what makes an open case marketplace
  possible at all.
- **Latency is UX.** The after-action report appears seconds after the encounter ends. The
  attestation has to land inside that window or the credential feels like paperwork instead of
  a result.
- **Composability.** Built on SAS + Bubblegum + Token-2022 against an open case registry, so a
  competing trainer can read the same cases and issue against the same credential schema.
  We want to be a protocol with one reference client, not a walled app.
- **Gasless for students.** Fee-payer relayer — a med student never touches SOL, never sees a
  seed phrase, and does not need to know any of this is happening.

## What already exists (the head start)

`embla-proof` is a new repo, but it is not starting from zero. Reused from Embla:

- **`embla-engine`** (Rust) — virtual-patient encounter state machine, deterministic rubric
  scoring (`sop_score.rs`, `meq_grade.rs`, `examiner.rs`), competency derivation with Dreyfus
  levels (`competency.rs`), psychometrics, and the hash-chained audit log (`audit.rs`).
- **205 authored OSCE cases** across 12 specialties, plus JSON schemas for cases, SCEs,
  exams, and competency blueprints.
- **A designed game layer** — XP curve, Dreyfus skill trees, badges, ranked/practice modes, and
  anti-cheese guardrails (distinct-case and difficulty gates, a variance cap) that turn out to
  double as anti-farming rules once badges carry escrow money.
- A working client, scoring standard, and a real user population to test with.

Most hackathon teams spend four weeks building the thing that produces the signal.
We already have it; the sprint is spent making the signal verifiable.

## Boundaries (deliberate)

- **Nothing tradeable.** All progression tokens are non-transferable by construction. A market for
  "Expert in Cardiology" would make the whole system worthless, so we forgo the volume.
- **No patient data, ever.** Virtual patients are synthetic (DDXPlus, CC-BY-4.0). No PHI
  exists in this system to leak.
- **No student PII onchain.** Hashes and pubkeys only. Student performance data is personal
  data under PDPA — it stays off-chain, under the student's control, revealed selectively.
- **Inference stays local.** Clinical reasoning runs on the local Heimdall gateway. The
  onchain layer never requires shipping clinical content to a cloud LLM.
- **Institutions keep fiat rails.** Thai medical schools are not going to pay in USDC.
  Crypto rails serve the open protocol and independent case authors; the B2B contract stays
  boring on purpose.

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — protocol design, accounts, flows, open questions
- [docs/GAMIFICATION.md](docs/GAMIFICATION.md) — progression as onchain computation, soulbound design, escrow, anti-farming
- [docs/SPRINT_PLAN.md](docs/SPRINT_PLAN.md) — the 4-week build
- [docs/COLOSSEUM_FIT.md](docs/COLOSSEUM_FIT.md) — mapping to the six judging criteria
- [docs/RISKS.md](docs/RISKS.md) — what could sink this, including the ones we caused ourselves
