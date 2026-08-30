# Vitals

**A patient is dying on a clock and you decide what happens next.
Anyone can play. Nobody can fake the replay.**

> v0.6.0 · kickoff 2026-08-22 · Rust throughout · SPDX **`AGPL-3.0-or-later`**, plus a commercial
> licence (the same pair the workspace manifest declares).
> Built on the encounter engine and physiology automaton from **Embla** and **Embla Cloud**, which
> ship today at [embla.megawiz.co.th](https://embla.megawiz.co.th). Those are separate private
> repositories, not part of this one, so there is no link here to follow.
> Targeting the Colosseum / Solana track.

Vitals is two things that turn out to be the same thing:

- **A game.** A real-time clinical emergency, driven by a deterministic physiology
  simulation — vitals move, the patient deteriorates, and the clock is the patient
  rather than an arbitrary timer. No medical vocabulary required to feel the stakes.
- **A protocol.** Any run can be anchored on Solana as a replayable action trace — anchoring is
  opt-in, one press — and once it is, the outcome can be re-derived by anyone, forever, without us.
  Progression is granted because the chain recomputed the predicate — not because a server said so.

The same replay serves a curious teenager on a leaderboard and a medical graduate who
needs to prove competence to a residency programme in another country.

---

## 🏆 Traction — Embla's, not Vitals'

Vitals started on 2026-08-22. Every figure in this section belongs to **Embla**, the clinical
simulation engine Vitals is built on and reuses, which has been in production for months. It is
cited because it is why a scoreable signal exists at all, not because Vitals earned it.

- **1st Place @ NECTEC AI for Thai 2026** ([announced by NECTEC](https://www.facebook.com/NECTEC/posts/pfbid02jyg4DWDoWsGpnXC6Gi6CcpjsDpHf5mxh1vkrxmSSN8FaAwoW89dBMx1i5GR1tTLWl)) — awarded to Embla.
- **18 of Thailand’s 29 medical faculties have at least one active learner on Embla.** A faculty is
  counted only when a learner account is verified against it; self-reported signup entries are
  excluded, which is why this number is lower than the platform's raw institution count. **No faculty
  has an institutional agreement with us; every user is an individual.**
- **290+ clinicians and medical students on Embla.** Individual accounts rather than
  institutional seats — the same population the runs below come from. It is a floor and stays
  one: it is the standing headcount carried across our own documents, not a figure any current
  export re-derives, so no rate is computed from it.
- **671 scored runs on Embla (as of 25 Aug 2026) · a catalog of 433 authored clinical cases · 384
  of them validated · across 23 specialties.** Different denominators, so they are printed
  separately rather than collapsed: a run is not a case, and an authored case is not a validated
  one. Every one of the 671 is an encounter that ran all the way to grading — Embla persists an
  attempt row only after the examiner returns a score, so an abandoned, timed-out or ungraded
  encounter is never counted. The runs figure is a snapshot and only grows; the case counts come
  from the case registry.
- **Live production platform**: [embla.megawiz.co.th](https://embla.megawiz.co.th)

**Vitals' own usage is close to nothing, and it is published rather than omitted:** the demo bay
serves its raw counts at `/api/usage`, uncurated, including on the days they read one and one.
Anchoring is opt-in, so even the anchored count there is a floor rather than a total.

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

## What vitals is

An open protocol that turns clinical-skill practice into **portable, tamper-evident,
independently re-scorable competency records** — and pays case authors per attempt.

Five onchain primitives and one off-chain engine — with a column saying which of them you can
run today. Three of the six are designed and not built; they are kept here because the design is
the argument, but a reader who greps this repository for them should meet this sentence first.

| Layer | What it does | Built with | Status |
|---|---|---|---|
| **Attempt Anchor** | Commit–reveal per attempt: pre-commit before the encounter, then one leaf over `(player, sce_hash, run_hash, rubric_hash, outcome, harm, det and judged scores)`. The scenario and the rubric are pinned by hash; there is no `engine_ver` or `model_id` field, and the only version byte in the record is `vt02`, the encoding's own tag. The leaf's non-hash fields — outcome, harm, scores, tier, exam flag — are public; no name, email or institution ever is (see Boundaries). | `Commit` · `AnchorReplay` · `ProveAttempt`, over a fixed-depth incremental Merkle tree the program keeps itself | **shipped** |
| **Progression** | XP, level and a Dreyfus stage per specialty — granted **permissionlessly**, because the program recomputes the predicate from anchored attempts instead of trusting a server, and rejects a claim its own arithmetic disagrees with. Badges are not among them: the `Progress` account holds a stage, a distinct-case count, an attempt count and XP, and nothing else. | `ClaimProgress`, written to a `Progress` PDA at `["prog", id, specialty]`. Not a token: nothing here is transferable, so there is nothing to sell | **shipped** |
| *(off-chain)* **Verifier** | Re-runs the deterministic rubric over a revealed transcript and confirms it reproduces the anchored score. | `vitals-replay` (Rust), and the same arithmetic compiled to wasm on the verify page | **shipped** |
| **Achievement badges** | A predicate over anchored attempts — `Sharp Historian`, `Red-flag Hawk` — that a front-end could read and a sponsor could put money behind. | no account holds one today; the nearest built thing is the star tally the server derives off-chain from proven attempts | designed, not built |
| **Case Registry** | Authors publish cases; content stays off-chain, only `case_id + content_hash + rubric_hash + price + royalty split` goes onchain. Any front-end can read it. | a second Solana program + Token-2022 | designed, not built |
| **Competency Credential** | Clear N attempts above threshold → an accredited issuer attests "OSCE-Cardio-L2" to the student's wallet. Reusable across apps without exposing the underlying data. | [Solana Attestation Service](https://solana.com/news/solana-attestation-service) | designed, not built |

Two primitives are named here that this repository deliberately does *not* use. Production-scale
anchoring belongs on **Bubblegum**, but Bubblegum needs an indexer to read, and a demo that
cannot be verified on a laptop with no network is not a demo of verifiability — so the tree is
in the program, and a proof is checkable with nothing but hashes. Progression could have been a
**Token-2022 non-transferable mint**; it is a PDA instead, because the useful property was never
the token, it was that the chain recomputes the predicate and can refuse. Adding a mint would
add a thing to look at, not a thing to check.

## One anchored record, three resolutions

Everything here is the same attempt data read at three zoom levels — not three products:

| Resolution | Stakes | Who can write it |
|---|---|---|
| **Attempt** — the anchored leaf | raw evidence | verifier |
| **Progression** — level, skill tree, badge | low, continuous | **anyone; the program checks the maths** |
| **Credential** — competency attestation | high, institutional | accredited issuer |

If a feature cannot be introduced as a consequence of that sentence, it does not belong in the pitch.

## Why this is not "certificate NFT" or "achievement badges"

The novelty is **exam integrity as a mechanism**, not storage:

- **Commit–reveal kills the retry cheat.** The client commits `hash(case ‖ player ‖ nonce ‖ mode)`
  *before* the encounter starts. You cannot practise five times and then anchor only the good
  run — the chain already knows how many times you started, and the count lives in an account that
  is never closed for its rent, so it can only go up. The mode byte is inside the hash for the same
  reason: whether a run was an exam is decided before it is played, not after the outcome is known.
- **Deterministic scoring makes the score disputable.** Embla's rubric scorer is deterministic
  and the anchor pins the scenario and the rubric by hash, so a second party can re-run the same
  transcript against the same two and must get the same number. A credential you can *re-derive*
  is worth more than a credential you can only *read*.
- **The student cannot self-sign a leaf anyone reads.** An anchoring tree is a PDA derived from
  the operator's key, so a run is evidence only relative to whose tree it sits in: a student who
  funds their own tree anchors into an account no relying party looks at. The program holds no
  issuer key of its own — `ClaimProgress` is a pure function of proven attempts, so nobody can
  lean on it. An n-of-m verifier quorum over a single leaf is designed, not built.
- **Progression is computed, not granted.** Embla's `xp_for`, `level_for` and `dreyfus` are pure
  functions over attempt history with unit-tested thresholds. An integer twin of them runs *inside
  the program itself*: `claim_progress` takes merkle proofs, the program recomputes the level, and
  writes the record only if its own arithmetic agrees. Every other achievement NFT is minted
  because a server said so — ours is not minted at all: it is a PDA the program declines to write
  when the maths disagrees, and there is nothing to transfer. See [docs/GAMIFICATION.md](docs/GAMIFICATION.md).
- **The hash chain already exists.** `embla-engine` ships a hash-chained append-only audit log
  (`engine/src/audit.rs`) with a canonical hash recipe cross-validated between its Rust and
  Python implementations. Today it is tamper-evident *to whoever holds the log*. Anchoring the
  chain head to Solana makes it tamper-evident *to the world*. That is the whole delta.

## What is provable, and what is only attested

Embla's rubric is **40 points deterministic and 60 points LLM-judged**. So the anchor carries two
labelled numbers, not one:

- `det_score` (40) — **re-derivable.** The leaf pins `sce_hash` and `rubric_hash`, so re-running
  the same scenario against the same rubric reproduces the same bytes. This is the strong claim.
  Being exact about the limit: what is pinned is the *content* of the scenario and of the rubric,
  not the version of the engine that ran them — no field carries an engine version or a model id.
- `judged_score` (60) — **attested, not derived.** The record carries the number and what it was
  out of, and `judged_max == 0` states that nothing here needed a witness at all. Recording which
  model at which version signed it, and requiring more than one signer, is designed, not built:
  no field holds either today.

A badge predicate a sponsor could put money behind is expressible over `det_score` alone, so the
money would never ride on the attested half. *Coming next*, and stated as such: the program holds
no money today; that instruction is not written yet. We state the split up front rather than
letting someone find it — see [docs/RISKS.md](docs/RISKS.md) §3.

**And the rubric is public on purpose, not by oversight.** A score you cannot re-derive is not a
score — exam integrity here comes from commit–reveal, not from hiding the key. Publishing the rubric
is exactly what makes the 40 deterministic points checkable by a stranger; what stops the retry cheat
is the pre-encounter commitment, which works whether or not the candidate has read the rubric.

## Why Solana specifically

Not decoration — the numbers only close on this chain:

- **Volume.** ~25k Thai medical students × ~200 cases = millions of attempt anchors per year,
  and that is one country. Per-write cost has to be ~$0.0001 or the model is dead, and Solana
  state compression writes ~1M compressed records for roughly **$110** — about that figure.
  This demo does not reach it: it anchors through the program's own Merkle tree at 3 transactions
  a run, **30,000 lamports** — which is $0.0001 only if SOL is worth about three dollars. That is
  the gap the table above calls "production-scale anchoring belongs on Bubblegum", stated as a
  number rather than as a preference.
- **Micropayments.** Case authors earn ฿0.5–2 per attempt. Card rails eat that whole amount in
  fees; Solana settles it with change left over. This is what makes an open case marketplace
  possible at all.
- **Latency is UX.** The after-action report appears seconds after the encounter ends. The
  attestation has to land inside that window or the credential feels like paperwork instead of
  a result.
- **Composability.** The registry and the credential are meant to land on SAS, Bubblegum and
  Token-2022 — designed, not built, as the table above says — so that a competing trainer can
  read the same cases and issue against the same credential schema.
  We want to be a protocol with one reference client, not a walled app.
- **Gasless for students.** Fee-payer relayer — a med student never touches SOL, never sees a
  seed phrase, and does not need to know any of this is happening.

## What already exists (the head start)

`vitals` is a new repo, but it is not starting from zero. Reused from Embla:

- **`embla-engine`** (Rust) — virtual-patient encounter state machine, deterministic rubric
  scoring (`sop_score.rs`, `meq_grade.rs`, `examiner.rs`), competency derivation with Dreyfus
  levels (`competency.rs`), psychometrics, and the hash-chained audit log (`audit.rs`).
- **433 authored clinical cases across 23 specialties**, 384 of them validated, plus JSON schemas
  for cases, SCEs, exams, and competency blueprints.
- **A designed game layer** — XP curve, Dreyfus skill trees, badges, ranked/practice modes, and
  anti-cheese guardrails (distinct-case and difficulty gates, a variance cap) that turn out to
  double as anti-farming rules if a badge ever carries sponsor money.
- A working client, scoring standard, and a real user population to test with.

Most hackathon teams spend four weeks building the thing that produces the signal.
We already have it; the sprint is spent making the signal verifiable.

## Boundaries (deliberate)

- **Nothing tradeable.** Progression is a record the program owns, not a token — the table above
  says the same thing. There is nothing to transfer and nothing to sell, by construction rather
  than by policy: no instruction moves a `Progress` account, and no mint exists to hold a
  transfer flag in the first place. A market for "Expert in Cardiology" would make the whole
  system worthless, so we forgo the volume.
- **No patient data, ever.** Virtual patients are synthetic (DDXPlus, CC-BY-4.0). No PHI
  exists in this system to leak.
- **No identity onchain — but performance is onchain, in the clear.** This file used to claim
  "hashes and pubkeys only", and the program does not deliver that. An anchored run publishes,
  under the player's key and readable by anyone: the difficulty tier, the exam flag, the outcome
  *including whether the patient died*, the harm count, and the deterministic and judged scores —
  beside the hashes of the case, the scenario, the tape and the rubric. `ProvenAttempt` and
  `Progress` then keep score, level, distinct-case count, attempt count and XP as account data,
  and the commitment account keeps a count of every attempt ever declared, which only ever rises.
  That is performance data, and performance data is personal data under PDPA — so the honest
  framing is *published*, not *withheld*. What genuinely never reaches a chain is the identity
  around it: no name, no email, no institution, no patient data, and no transcript — the tape is
  committed to by hash and revealed selectively. A player key is a pseudonym that nothing here
  binds to a person; it stops being one the moment its holder tells someone it is theirs, which is
  the trade [the privacy policy](crates/vitals-web/static/privacy.html) puts in front of the
  button.
- **Inference stays local — with one recorded exception.** Clinical reasoning runs on the local
  Heimdall gateway, and the onchain layer never requires shipping clinical content to a cloud
  LLM. The public demo's synthetic-patient voice may fall back to a cloud model (see
  `crates/vitals-web/src/patient.rs`): the patient is synthetic, no PHI exists, and the local
  gateway takes over whenever it is reachable.
- **Institutions keep fiat rails.** Thai medical schools are not going to pay in USDC.
  Crypto rails serve the open protocol and independent case authors; the B2B contract stays
  boring on purpose.

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — protocol design, accounts, flows, open questions
- [docs/GAMIFICATION.md](docs/GAMIFICATION.md) — progression as onchain computation, badge design, sponsor-funded bounties (coming next, not built), anti-farming
- [docs/SPRINT_PLAN.md](docs/SPRINT_PLAN.md) — the 4-week build
- [docs/RISKS.md](docs/RISKS.md) — what could sink this, including the ones we caused ourselves
- [VERIFICATION.md](VERIFICATION.md) — clone, build, and re-derive a player's level from the chain
  yourself: every command, and the real output it printed, including a claim the program refused

## Security

Dependency advisories are triaged in [deny.toml](deny.toml), assessment included — the currently
ignored RUSTSEC entries sit in the Solana SDK's RPC-over-TLS client path, are not reachable from
the deployed program, and clear when the SDK bumps its `rustls` line. CI runs `cargo deny` on
every push.
