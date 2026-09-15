# vitals — Crypto World's Fair sprint plan

Colosseum's autumn hackathon, **Crypto World's Fair: 14 Sep 04:00 PDT → 12 Oct 2026**, read off
colosseum.com/hackathon on 11 Sep (DECISIONS.md #2 carries the correction; the earlier "28 Sep – 2 Nov"
was wrong). Registered 11 Sep. Solana track only: the accelerator and the fund invest on Solana and
nowhere else.

Two rules from the organisers shape everything below. **Judged only on work committed between 14 Sep
and 12 Oct** — so the branch opens on the 14th and nothing before it counts. **Disclose all prior
work** or be disqualified — Embla, the repo from 22 Aug, and the Eternal sprint (29 Aug – 26 Sep) go
in the form on day one, worded in `pitch/CRYPTO_WORLDS_FAIR_PAST_WORK.md` (MyHero).

Rule for the whole sprint, unchanged from [SPRINT_PLAN.md](SPRINT_PLAN.md): **anything not demoable
on the last day does not get built.**

The one sentence this sprint exists to make true: **the door is opened by the chain, not by the
client.** Today the episode gate runs client-side ([UNLOCK.md](UNLOCK.md): `required_badge` is
designed, not built); the Case Registry is unbuilt because "who pays" was unanswered; and 60 of 100
rubric points are an LLM's word ([RISKS.md](RISKS.md) §3). Those three gaps are what a judge will
press on, and every one of them is reachable from what already runs: twelve OSCE stations, the
program on devnet, the replay verifier.

## Before the clock (11 – 14 Sep)

- [ ] Eternal form: the eight corrections applied (numbers that match `/api/usage`, the truncated
      fields, the revenue answer aligned with DECISIONS.md #1); Pimnipa's profile complete
- [ ] **File Eternal on 15–16 Sep**, not 22–23: review is first-come-first-served, and the team is
      needed here from the 14th
- [ ] 14 Sep: register the project on the hackathon form with the prior-work disclosure; open the
      `cwf/` branch that day so the history shows where the sprint's work begins
- [ ] Find the signing physician for deliverable B (a date to close the 17-case audit, and agreement
      to sign the judged-60 attestation with a key of their own), or drop the attestation from B

Exit: Eternal is out of our hands, the hackathon project exists with the disclosure on it, and the
first commit on the sprint branch is dated 14 Sep or later.

## Deliverable A — the star gate on chain (weeks 1–2, 14 – 27 Sep)

- [ ] Program: a station attempt anchors as a leaf; stars are recomputed by the program from proven
      leaves; the scenario registry carries the prerequisite (`required_stars` / badge predicate)
- [ ] Client: the episode gate reads on-chain state and nothing else — the client-side gate is
      deleted, not bypassed
- [ ] The refusal is the demo: claim stars you did not earn → REJECTED; earn them at a station → the
      next episode opens because the chain says so
- [ ] Tag `v0.10.0`; `check.sh demo`-style two-run byte identity on the replay path still holds
- [ ] osce-a.json carries a stale reviewer attribution in its status string; pinned by 7 leaves;
      changes with the next rubric version through the archive path, not before

Exit: no code path outside the program can open an episode, and the refusal and the unlock are both
recorded on screen from the deployed build.

## Deliverable B — the two other roles get a surface (weeks 2–3, 21 Sep – 4 Oct)

The three inward roles are the learner, the relying party and the author. The learner has the bay;
the other two have nothing to look at. Both pages read the chain and `/api/usage` only — no login, no
database, no number that cannot be recomputed by a stranger.

- [ ] **Relying-party page**: paste a record or a pubkey → the wasm verifier re-derives the level in
      the browser; the 40 deterministic points and the 60 attested points are shown as two numbers,
      never one
- [ ] **Named-clinician attestation**: the judged 60 carries a signing physician's signature (signer
      not yet identified — founder's decision) at a stated rubric version, replacing the anonymous
      model's word — RISKS §3 closed the honest way, by naming who vouches
- [ ] A "request verification" action with a price on it; no payment rail yet, the surface is the
      deliverable (this is the answer to "how do you make money", on screen)
- [ ] **Author ledger** `/authors/<wallet>`: per case — plays, proven replays (counted from anchored
      leaves), accrued at the configured rate, paid with transaction links, platform share by bps.
      Labelled devnet SOL until mainnet; if `VITALS_PAYOUT_LAMPORTS` is unset the page says "rate not
      set", it never invents one

Exit: a stranger with no account can verify a learner and can read what an author earned, and both
pages agree with the chain to the leaf.

## Deliverable C — mainnet-beta, authority in a multisig (week 4, go/no-go 5 Oct)

- [ ] Go only if A is stable on devnet by 4 Oct; otherwise the multisig lands on devnet and mainnet
      moves to the next sprint
- [ ] Program deployed to mainnet-beta; upgrade authority transferred to a Squads multisig
- [ ] Relay funded for the sprint's anchors only; anchoring stays opt-in; devnet kept as the sandbox
- [ ] Author payouts are **not** switched on for real money — Thai withholding and SEC digital-asset
      rules are unresolved; the chain is the ledger, the rail can be baht (see DECISIONS.md, UNLOCK.md)

Exit: "devnet by choice" becomes "mainnet, and no single key can upgrade it" — or the plan says
plainly that it did not, and why.

## Evidence, in parallel (not code, still scored)

- [ ] The 17-case clinical audit closed with the reviewer named (or "reviewed by a practising
      physician" if they decline the name)
- [ ] Psychometrics on Embla's 671 scored runs published: SEM, and the `investigation_choice`
      r = 0.150 finding stated as a finding, not hidden
- [ ] Funnel on `/api/usage` (arrival → play → finish) — already public, keep it so
- [ ] One-minute video each week: 20 Sep · 27 Sep · 4 Oct · 11 Oct, through the existing
      `docs/internal/video` system

## Calendar

| Week | Dates | Ships |
|---|---|---|
| 1 | 14 – 20 Sep | A: program + client reads the gate from chain · video 1 |
| 2 | 21 – 27 Sep | A done, refusal/unlock on screen · B begun (verify page, clinician signing) · Eternal window closes 26 Sep, already filed · video 2 |
| 3 | 28 Sep – 4 Oct | B done incl. author ledger · audit + psychometrics closed · **scope freeze** · video 3 |
| 4 | 5 – 12 Oct | C if go · new technical demo (this sprint's work, refusal before happy path) · pitch reused or re-cut · **submit 9–10 Oct** |

## Not this sprint

New story episodes, patients in Japanese/Korean/Chinese, any token, compressed anchoring, Bahasa
localisation. None fits four weeks and none shows on a screen.

## The four questions to rehearse

Same four as [COLOSSEUM_FIT.md](COLOSSEUM_FIT.md), plus the two this sprint invites: *"what is new
since Eternal?"* (A, B, C — and the git history that starts on 14 Sep) and *"why should anyone pay
for a hash?"* (the relying-party page, with the price on it).
