# vitals — Crypto World's Fair sprint plan

Colosseum's autumn hackathon, **Crypto World's Fair: 14 Sep 04:00 PDT → 12 Oct 2026**, read off
colosseum.com/hackathon on 11 Sep (DECISIONS.md #2 carries the correction; the earlier "28 Sep – 2 Nov"
was wrong). Registered 11 Sep. Solana track only: the accelerator and the fund invest on Solana and
nowhere else.

Two rules from the organisers shape everything below. **Judged only on work committed between 14 Sep
and 12 Oct** — so the branch opens on the 14th and nothing before it counts. **Disclose all prior
work** or be disqualified — Embla, the repo from 22 Aug, and the Eternal sprint (29 Aug – 26 Sep) go
in the form on day one, worded in `docs/internal/CWF_PAST_WORK_DISCLOSURE.md` (written 15 Sep;
gitignored, because the wording is ours before it is the form's).

Rule for the whole sprint, unchanged from [SPRINT_PLAN.md](SPRINT_PLAN.md): **anything not demoable
on the last day does not get built.**

> **Ruling, 15 Sep 2026 ~16:00 — the founder replaced deliverables A/B/C with a single centre.**
> His words: *"เอาอันนี้ เราจะให้คนทั่วโลกมาช่วยกันรักษา โดยเราจะปล่อยคนไข้ออกมาเรื่อยๆ หายแล้วกลับบ้านได้"* —
> we let the whole world treat patients together, we keep releasing patients, and the ones who
> recover go home. The old A (star gate on chain) survives inside it as the shift gate; the old B
> (verify page) survives as the shift receipt; the old C (mainnet + multisig) is week-4 slack only.
> This document was rewritten around that ruling on 15 Sep; the plan it replaces is in git history
> at `cwf-start`.

The one sentence this sprint exists to make true: **the chart is the chain.** A patient's whole
history is a chain of anchored shifts that any stranger's browser can re-derive for itself, so no
operator — us included — can quietly change what happened to her. The three gaps a judge will press
on are unchanged and all three are now answered by the ward: the episode gate still runs client-side
([UNLOCK.md](UNLOCK.md): `required_badge` is designed, not built) — the shift gate moves it on chain;
the Case Registry is unbuilt because "who pays" was unanswered — the ward answers "who plays" first
and honestly defers the rest; and 60 of 100 rubric points are an LLM's word ([RISKS.md](RISKS.md) §3)
— the shift receipt shows the deterministic 40 and the judged 60 as two numbers, never one.

## The ward

**One patient, many strangers, and the chart is the chain.**
**And the ward never runs out of them.**

A patient is released into a public ward. Anyone in the world takes a shift on her — no signup, no
wallet, the relay pays — treats her for a few minutes, and hands over. The next stranger, any time
later, opens her: the client replays every previous shift's tape through the deterministic engine
and arrives at exactly the state the last person left her in. Mistakes carry forward. The log says
who and when, and nobody can rewrite it. When the engine reaches a discharge state she goes home; if
she dies the record stands and the next patient is released. Patients keep being released through
the sprint.

What a judge sees in ninety seconds *(the target for the week-4 cut, not a measurement)*: bed after
bed, not one showcase patient — a woman who has been alive for six days and been treated by eleven
strangers in four countries, her chart re-derived in the browser from eleven anchored tapes with one
shift on it where somebody made her worse and the key that did it still attached; and behind her the
bed that emptied yesterday, already filled.

## What is already built, and what is actually new

Being exact about this is the difference between a four-week plan and a wish.

| The mechanic needs | State on 15 Sep |
|---|---|
| Replay a tape and arrive at a live machine state | **exists** — `vitals-replay::resume(sce_json, tape) -> (SceState, Replay)`, one step loop shared by resume and verify |
| A bounded attempt with a tape, scored deterministically + judged | exists — the encounter engine and `vitals-osce` |
| Commit–reveal per attempt, anchored as a leaf | exists — `(player, sce_hash, run_hash, rubric_hash, outcome, harm, det and judged scores)` |
| Browser re-derivation of a score | exists — the same rubric arithmetic compiled to wasm (Verifier row, README) |
| A patient that is a **hash-linked chain** of shifts, with a head | **new** — program work |
| "Your shift must extend the current head" | **new** — program work |
| Starting an attempt **from** a resumed state rather than from the scenario's start | **new** — engine work, small, on top of `resume` |
| A ward board and a shift receipt page | **new** — web work |

New medicine: none. A stay is a chain of cases we already have (anaphylaxis → observation →
discharge; STEMI → CCU → ward → home), and the bridges between them are **mechanical state
handoff, not new clinical writing**. Say so on the page; a clinician reading it should never think
we authored a new disease course this month.

## The rulings this plan is built on (15 Sep)

1. **Content is what we have.** No new physiology, no new clinical writing. A stay chains existing
   cases; the joins are mechanical.
2. **Between shifts the patient is frozen.** Simulated time advances only while somebody is on
   shift. Honest, simple, and "frozen for three days" is visible on the chart — which is a feature,
   not an apology.
3. **A patient is a chain, the program holds the head.** A `Patient` account with a head; a shift
   commits against the current head (commit–reveal as today); the reveal appends and moves the head;
   discharge or death closes it. The client verifies the whole chain before letting anyone take a
   shift. The old deliverable A survives as the gate: *you may take a shift on a sicker patient only
   if the program's star count says so.* **A stay ends where the engine says it ends: death and
   discharge close the chart; `WinIcu` does not.** Survival into intensive care is a transfer, not
   an ending — she is still on the ward and the next shift continues her, so no video and no page
   ever says "ICU" as a closing.
4. **Griefing is allowed and named.** Harming her is a scored shift with the harm on the record and
   the player's key on it. Limits: one shift per key per patient per day, plus a cooldown. Death is
   permanent for that patient.
5. **Beds, and the queue behind them** (founder, 15 Sep ~18:20: *"เราจะเติมคนไข้มาเรื่อยๆเลยนะ"* — we
   keep adding patients). The ward has a fixed number of open beds — **three** to start — and a
   release is **automatic**: the moment a patient leaves, by discharge or by death, the next one is
   released from the queue with nobody on the team touching anything. The queue is built from the
   content we already have — the five episodes, the twelve stations, and Embla's catalogue as it is
   converted — each stay a mechanical chain of existing cases, as ruled above. **A longer queue is
   more existing cases, never new clinical writing.** The point of automatic release is that the
   ward keeps running while we are asleep, and after 12 Oct.
6. **Public worldwide from day one of the mechanic.** Any invitation to Embla's students is the
   founder's to make, and no institution name appears anywhere in the product or the video.
7. **The ward is its own host and its own service** (founder, 15 Sep ~19:00: *"web ผมจะใช้
   https://world.vitals.academy นะ แยกออกมาให้ชัด"*). The ward is **world.vitals.academy** on Cloud
   Run service **`vitals-world`**. Vitals stays at **vitals.academy** on service **`vitals`**, and
   **no sprint deploy touches it** — the Eternal entry must still be there, unchanged, on the day a
   judge opens it. The deploy script refuses `SERVICE=vitals` from a `cwf/*` branch for that reason.
8. **Everything else stands.** Devnet (mainnet + multisig only if week 4 has slack), no token, no
   money, the relay never signs as author, the payout allowlist stays empty, lamports are never
   called dollars, the review store is untouched, no physician claim anywhere, every figure carries
   its as-of.

## Three rulings that shape the build (producer, 15 Sep)

**The sentence.** *No server can change her past without every browser noticing.* That is the claim,
in the plan, in the product and in every video. The chain holds the leaf and the leaf holds
`run_hash`, not the tape; the tape bytes are served off-chain, so the stronger-sounding "no server
holds patient state" is false and is not to be written or spoken anywhere. What makes the real
sentence true for a stranger who trusts nobody: the shift receipt carries **download every tape of
this patient** — content-addressed, each hash on chain — so anyone can mirror her and check us. A
neutral mirror of our own is listed under *if there is slack*, never promised.

**The lease.** A shift takes the head for a bounded time, on chain, in week 1. One instruction; an
expired lease is free for anyone to take; the ward board shows *on shift since*. The number comes
from the engine rather than from taste: the longest authored working window is **18 minutes**
(EP5's `runtime_min`; the others are 12, 12, 14, 12), so a lease is **that shift's own
`runtime_min` plus 5 minutes** for the reveal to land and for clock skew — a ceiling of 23 minutes
as the content stands today. The reject-on-stale-head path stays underneath as the safety net, and
the week-4 cut shows that refusal once, because the refusal is the honest part.

**The first shift is not gated.** Nobody has stars on the morning the ward opens, so a gate on a new
patient's first shift is a ward that never starts. The star gate belongs to the second track —
sicker patients — when that exists. **If patient one dies at 11:00 on release morning, her chart is
the demo.**

## Before the clock — state as of 15 Sep 16:30, all of it late

- [ ] Eternal form: the eight corrections applied (numbers that match `/api/usage`, the truncated
      fields, the revenue answer aligned with DECISIONS.md #1); the student reviewer's profile complete
- [ ] **File Eternal — planned 15–16 Sep, not yet filed.** Review is first-come-first-served and the
      team is needed here. Hard gate: **it must be filed before week 2 starts.**
- [x] The prior-work disclosure is **written** — `docs/internal/CWF_PAST_WORK_DISCLOSURE.md`,
      15 Sep: every figure with its as-of, and it names the `cwf-start` tag and commit `05a0ad7`
      as where the sprint's history begins.
- [ ] **Paste it into the hackathon form — was due 14 Sep, still open.** Not disclosing prior work
      is disqualification, so this stays the most expensive open item until it is in the form. One
      thing has to happen first: **`cwf-start` is not on any remote branch**, so a judge following
      the tag today finds nothing. Push before the disclosure points at it.
- [x] `cwf-start` tag exists; **`cwf/ward` opened from main 15 Sep** for the mechanic spike.
- [ ] ~~Find the signing physician for deliverable B~~ — **dropped with B.** The judged 60 carries no
      signature this sprint and the pages say so in words.

Exit: Eternal is out of our hands, the hackathon project exists with the disclosure on it, and the
sprint's work sits on `cwf/ward` with dates inside the window.

## Week 1 — the mechanic exists (to 20 Sep)

- [ ] Engine: an attempt can start from a state produced by `resume`, not only from the scenario's
      start. One tape, one machine, no second code path.
- [ ] Program on devnet: `Patient` account with a head; commit binds to the head; reveal must extend
      it or is rejected.
- [ ] **The head lease**: one instruction takes the head for `runtime_min + 5` minutes (23 max as
      the content stands); an expired lease is free for anyone; the ward board shows *on shift
      since*. The stale-head rejection stays as the net underneath it.
- [ ] One patient chained from two existing cases, played end to end **by two different keys**, the
      second starting where the first stopped.
- [ ] Video 1 (20 Sep) — the handover, nothing else.

Exit: a stranger's shift changes what the next stranger finds, the program refuses a shift that does
not extend the head, a second key cannot take the head while the lease stands, a second patient is
released automatically when the first leaves, and **world.vitals.academy answers**.

## Week 2 — patient one is public (to 27 Sep)

- [ ] Patient one released to the world. No signup, no wallet, relay pays.
- [ ] The ward board: who is on the ward, how long each has been there, who is on shift — and the
      moment an empty bed fills itself, which is the thing to watch.
- [ ] **`/api/ward` — the census, public and recomputable.** Cumulative and this-week, in this
      order: **admitted** (released) · **on the ward now** · **went home** (discharged) · **died** ·
      **shifts taken** · **distinct keys** that took a shift. Every number carries its as-of and the
      thing it was derived from, and nothing on it is hand-counted:
      admitted / discharged / died come from the patient chain opening and closing **on chain**;
      shifts from **anchored leaves**; *on the ward now* is `admitted − discharged − died`, never a
      separate tally; distinct keys from the **leaf signers**. If the endpoint and the chain ever
      disagree, the weekly video says they disagree — it does not pick one and it does not wait for
      the next deploy to mention it.
- [ ] Shift receipt at a QR: the browser re-derives that shift from the tape and the chain, shows
      the deterministic 40 and the judged 60 as two numbers, and offers **download every tape of
      this patient** so a stranger can mirror her and check us without asking.
- [ ] Eternal filed before this week starts — otherwise this week is that instead.
- [ ] Video 2 (27 Sep).

Exit: somebody we have never met has taken a shift, and a second stranger can check what they did
without asking us anything.

## Week 3 — it keeps running (to 4 Oct)

- [ ] **The automatic release has run unattended for a week** — beds emptied and refilled with
      nobody on the team touching them — and at least one discharge, and one death if it happens,
      both with the chart intact.
- [ ] **Scope freeze** at the end of this week.
- [ ] Evidence closed: psychometrics on Embla's 671 scored runs published (SEM, and the
      `investigation_choice` r = 0.150 finding stated as a finding); the funnel on `/api/usage` kept
      public.
- [ ] Video 3 (4 Oct).

Exit: the ward has run for a week without us touching it, and the numbers about it are public.

## Week 4 — the cut, and the submission (to 12 Oct)

- [ ] Demo: bed after bed — a real patient's life on the ward and the bed behind her refilling, with
      the refusal and the handover **before** the happy path.
- [ ] If there is slack: mainnet + multisig, and a neutral mirror of the tapes. Neither is promised;
      without them the plan says plainly that devnet was the choice and why.
- [ ] **Submit 9–10 Oct**, not the 12th.
- [ ] Video 4 (11 Oct).

Exit: a judge can watch a stranger's mistake survive in a patient's chart, and re-derive it
themselves from the chain.

## Evidence, in parallel (not code, still scored)

- [ ] The 17-case clinical audit — **reviewer not identified since 14 Sep**; publish it as "reviewed
      by a practising physician" only if one has actually read it, otherwise publish it as our own
      work and say so. Week 2's video says "under physician review" burned into the picture; that
      was true when it was burned and it is not repeated anywhere new. **A candidate for the
      signing-physician seat was named by the founder on 15 Sep; role and consent pending** — so
      nothing changes on any public surface and "signer not yet identified" stays until the producer
      says otherwise. If the seat is confirmed the judged-60 attestation can come back as a week-3
      **option**, never a promise.
- [ ] Psychometrics on Embla's 671 scored runs (as of 25 Aug 2026): SEM, and the
      `investigation_choice` r = 0.150 finding stated as a finding, not hidden.
- [ ] Funnel on `/api/usage` (arrival → play → finish) — already public, keep it so.
- [ ] One-minute video each week: 20 Sep · 27 Sep · 4 Oct · 11 Oct, through the existing
      `docs/internal/video` system.

## Calendar

| Week | Dates | Ships |
|---|---|---|
| 1 | 14 – 20 Sep | resume-from-state · patient head on devnet · two keys, one patient · video 1 |
| 2 | 21 – 27 Sep | patient one public · ward board · shift receipt (40/60 apart) · Eternal filed before this week · video 2 |
| 3 | 28 Sep – 4 Oct | a week of unattended automatic release · a discharge · **scope freeze** · psychometrics + funnel · video 3 |
| 4 | 5 – 12 Oct | demo cut of a patient's life · mainnet only if slack · **submit 9–10 Oct** · video 4 |

## Not this sprint

New story episodes, new clinical content of any kind, patients in Japanese/Korean/Chinese, any
token, compressed anchoring, Bahasa localisation, the Case Registry, author payouts in real money,
the named-clinician attestation. None fits four weeks and none shows on a screen.

## The four questions to rehearse

Same four as [COLOSSEUM_FIT.md](COLOSSEUM_FIT.md), plus the three this sprint invites: *"what is new
since Eternal?"* (the ward, and the git history that starts at `cwf-start`), *"why does this need a
chain?"* (because the next stranger must be able to disbelieve us and still arrive at the same
patient), and *"what stops someone ruining her?"* (nothing — and the record says who did it, which
is the answer a hospital would recognise).
