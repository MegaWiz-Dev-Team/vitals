# Decisions — the authoritative record

> **If any other document in this folder contradicts this file, this file wins.**
> Older documents keep the reasoning that led here, which is worth reading; they were written
> before some of these decisions and have not all been rewritten.

All taken 2026-08-24 unless noted.

---

| # | Decision | Consequence |
|---|---|---|
| **1** | **The learner is free forever. Revenue comes from relying parties** — verification fees from employers and residency programmes, marketplace take-rate — with **donations via Solana** as the base. Never charge a learner | Decision #9 needs no reversal: with a commercial component the accelerator's equity fits, and Solana Foundation **Convertible Grants** exist for "public goods with a commercial component". Both doors open |
| **2** | ~~Start the Eternal sprint now~~ **REVISED same day → Plan B: skip Eternal, target the fall hackathon only** | **The 2026 dates were published all along and I had recorded them as unknown — spring 6 Apr–11 May, fall 28 Sep–2 Nov.** That removed the main argument for Eternal (that it was the only deadline). Plan B gives **five weeks of preparation then a five-week hackathon** — the same ten weeks, but one submission twice as good instead of two half as good, and Colosseum say the hackathons carry better accelerator odds. **Bonus the other plan could not offer: the repo opens and the public demo goes live during preparation, so we can arrive at 28 Sep with real users rather than only a product** |
| **3** | **The patent is not the moat. The repository opens** | The moat is 433 cases, 18 faculties, the conformance suite, accumulating outcome data — accumulation, not law. **A public demo URL is now wanted.** Unblocks Public Goods Award, Foundation grants, Superteam. **Irreversible** |
| **4** | **Hybrid inference** — cloud for the public demo, Asgard/Heimdall nodes for base load as they come online | A **recorded exception** to *Heimdall-only, no cloud LLM for Eir*: synthetic-patient dialogue carries no PHI and is not clinical care. Same shape as the CloudyFarm exception to Rust-first |
| **5** | **Mission: newly *licensed* doctors.** On stage: *"doctors who actually reach patients"* | Precise in documents and measurement, plain in the room. Makes both levers count — attrition *and* the 19-point pass gap |
| **6** | **No more story-mode scenarios. The first Commission is the demo** | Five scenarios is enough to show the unlock in a 27-day sprint; a visible funding goal for scenario six demonstrates the donation loop. The content shortage is what the mechanism exists to solve |
| **7** | **Rate limits plus a monthly ceiling, and the ceiling is visible** | When reached, the page shows what this month's compute funded and the donation counter. The constraint is part of the product |

### Earlier, still standing

| Decision | |
|---|---|
| Optimise for the accelerator, not the prize money | 2026-08-23 |
| We are the new thing this competition is looking for | 2026-08-24 |
| Vitals is global B2C; the B2B institutional line is struck — **including for Embla-Thailand**, whose faculties were tested and none bought | 2026-08-24 |
| Cases are not migrated; `embla-cases` stays the source of truth | 2026-08-24 |
| No licence NFTs, no ECFMG-scale verification — three inward roles only | 2026-08-24 |
| Thailand produces the evidence (closed cohort); Indonesia is still the first market | 2026-08-24 |

### Retired — do not bring back

| | Why |
|---|---|
| **CNEU / CME staging** | A revenue line from the Embla Box deck that became a checklist item while revenue was the focus. Irrelevant once the model became donation-funded and learner-free. Also: the doctors' scheme is **CME** (แพทยสภา, 100 credits per 5 years), not CNEU (สภาการพยาบาล, nursing) — and it is a **different population from the mission entirely**: doctors already licensed, not people trying to become doctors. An Embla-Thailand matter |
| **Repo stays private / no public demo URL** | Superseded by decision 3 |
| **Public Goods Award is a trade against novelty** | Superseded by decision 3 — it is now a free shot |

---

## What did **not** change, and is easy to get wrong

- **The `hidden` exam layer must still never be published.** That is Askr's exam security, not
  patent strategy, and it survives the repository going public. `deploy-strip.py` and its
  `--check` CI gate stay mandatory.
- **`embla-cases` stays private.** Opening the Vitals repo does not open the case library.
- **No learner PII on chain**, and `cand_hash` must be an HMAC, never a bare hash.
- **We still do not grant standing.** Only a board can make competence authoritative.
- **Nothing tradeable.** Progression and credentials remain non-transferable by construction.

---

## Open, and nobody is blocked on a decision any more

| | Unblocked by |
|---|---|
| Are the Eternal weekly video updates public? · Next hackathon dates | **One email to Colosseum** — 14 days on the clock |
| Which documents go public when the repo flips | A publishing review. History is clean (63 commits, no keys, no `.env`, no case content) — the question is business confidentiality, not a scrub |
| Global medical graduates per year (WHO GHO) · the 42–63% attrition figure · whether JP/KR practical exams are mandatory | Desk research, blocked by nothing |

---

## GCP projects — `vitals-academy-dev` now, `vitals-academy` held for production

**Decided 2026-08-24.** Two projects, matching the convention every sibling already follows
(`cloud-super-hero` / `-dev`, `mega-care` / `-dev`): the unqualified name is production.

| | | |
|---|---|---|
| `vitals-academy-dev` | 367117259093 | testnet and devnet — everything until the money line |
| `vitals-academy` | 995399340966 | production. Configured, empty, and deliberately unused |

An earlier draft of this entry proposed using `vitals-academy` for development and adding
`vitals-academy-prod` later. That was backwards, and worth recording because the mistake is one-way:
**a deleted GCP project id can never be reused**, so renaming after the fact is impossible and
deleting `vitals-academy` to free the name would have destroyed it. Both projects therefore exist
from the start, and only one is used.

That accident has one benefit: the production project's Firestore is already in `asia-southeast1`,
and a database's location cannot be changed after creation. The thing most likely to be got wrong
under time pressure is already right.

The second project stays empty until **a relay key holds SOL that could be sold, or the treasury
takes a real donation.**

The reasoning, so it is not re-litigated:

- The Solana cluster is chosen by `VITALS_RPC` and `VITALS_PROGRAM_ID`, not by a GCP project. One
  project can serve devnet today and mainnet later. Separating on the cluster axis is not what a
  project boundary is for.
- What a project boundary *is* for here is **Secret Manager**. `deploy-cloudrun.sh` mounts one
  secret, `vitals-relay-key`. Two deployments in one project either share that key — so the mainnet
  relay is the testnet relay — or need the name parameterised, which is isolation somebody has to
  remember. That is exactly the argument used to split from `cloud-super-hero`: isolation that must
  be maintained is not isolation. Below the money line the argument does not bite, because nothing
  in the project can lose anything; above it, it is decisive.
- Everything else usually cited — Firestore data, billing attribution, quota, blast radius — is
  recoverable or a nuisance. A compromised mainnet relay key is not.
- Today nothing in `vitals-academy` can lose money: devnet SOL is worthless. A second project would
  cost two Firestores, two secret stores and two configurations to keep in step, in exchange for
  nothing.

**The trigger is nearer than "someday mainnet."** The plan puts a Commission account and treasury in
the hackathon phase; the moment those take real money, the second project is due.

Both Firestores are `asia-southeast1`, matching each other and the Cloud Run region.

A leaf list no longer collides between deployments regardless of project — that was fixed at the
storage key rather than papered over with a project boundary. See `store::tree_key`.
