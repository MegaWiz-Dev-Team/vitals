# Postmortem — opening Vitals World, 19–21 September 2026

*Written the night the ward opened. Everything here is measured or quoted from logs; where a number is an estimate it says so. No patient on the ward is a real person.*

## Summary

Vitals World — a public ward where strangers take ten-minute shifts on simulated patients from the countries with the fewest doctors, with every shift anchored on Solana devnet — opened on **21 September 2026 at 21:30 ICT (14:30 UTC)**, two and a half hours after the 19:00 the founder had set that morning. The delay was not the ward: the ward had been ready since 14:45. The door could not be turned because the human credential every deploy depended on had expired at 18:52, and the service account created that morning to replace it lacked one permission.

The two days before the opening were spent measuring rather than building, and they took the ward's cold boot from **137 s to 3–8 s** and its background pass from **100 s to about 4 s** without making any single operation faster. Along the way we found a session sweep that had never run, a fix that had armed a dormant data-loss bug, a gate a panic could leave locked, a partial read trusted for a chain write, and a public sentence composed from a constant rather than from the state it described. Twenty-three incidents are recorded; the rule we drew from them is the one that survived every case: **a rule that is not executable has a shelf life.**

## Timeline

| When (ICT) | What |
|---|---|
| 19 Sep | Boot markers added; first marker mislabelled (blamed the meter for 137 s of unrelated work). Access token found to be fetched on every store call. Firestore session sweep found to have returned 0 since the service existed. |
| 20 Sep 03:00–08:00 | Boot 137 → 80 → 30.8 s (token cached, sessions rebuilt on first use). Repair and sweep moved behind the listener, repair first — which also fixed an ordering that could have deleted the only copy of a tape. Boot 4.0 s. |
| 20 Sep 08:00–15:00 | The ticker's pass measured: 100 s over 26 patients, median 674 ms, max 10.7 s — rate limiting on the free devnet RPC, not CPU throttling as first guessed. The same signature listing was found to be made three times per patient per pass, one answer discarded. Once, shared: ~4 s. |
| 20 Sep 20:21 | Production's min-instances set to 0 by another session applying the founder's cost ruling of that morning; the producer, unaware of the ruling, restored 1; the founder confirmed 0. The collision with "the refill is an event" was resolved by a scheduled tick route. |
| 20 Sep 22:15 | Founder: open tomorrow, 19:00, all cases provisional, min-instances 1 for 48 h from the opening. Clinical advisor's review of the 18 endemic cases arrives (provenance to be confirmed). |
| 20 Sep 23:33 | The machine's Rust toolchain moved under a running check (a build agent's side effect); a second void verdict when macOS's tmp reaper deleted 48 tracked files from a worktree. Toolchain pinned; worktrees moved out of `/private/tmp`. |
| 21 Sep 01:00–06:30 | Board blackout on one 429 fixed (a failed history keeps its cached copy, flagged; the rest of the board fresh). `POST /api/ward/tick` route, single-flight, called by Cloud Scheduler every minute. Ops service account created; deploy script allowlists it; the script's own test harness found dead and revived as a gate. |
| 21 Sep 08:30 | 75 packs recompiled with the advisor's seven rulings and per-case emphasis pushed through the case door to staging and production; three cases cut by his rule 3.7. The door refused all 75 once — the new rubric note said "OSCE", the season word the door is built to refuse. Fixed in the compiler. |
| 21 Sep 11:30–14:45 | Compiler bump (rust 1.98.1 in Dockerfile, CI and `rust-toolchain.toml`), `/stats` dashboard, the opening date as one constant, catalogue counts excluding withdrawn packs — staging then production (revision 00024). Ready. |
| 21 Sep 18:52 | Door flip attempted: the founder's gcloud login had expired (fifth time in six days); the ops service account lacked `artifactregistry.reader` for a Cloud Run update. Neither could be fixed without the founder. |
| 21 Sep 21:05 | Founder re-authenticates. Readiness re-checked: 20 waiting, 18 countries, 75 cases, all 20 queued patients with complete portrait sets, Scheduler 200 every minute. |
| **21 Sep 21:30:12** | **Door opened** (revision 00025: same image, `VITALS_WARD_DOOR=open`, min-instances 1). 21:30:44: first three admitted — Rwanda, Tanzania, Papua New Guinea. |
| 21 Sep 22:00–00:00 | The patient factory found stopped twice: first on the expired login (it read its token through the human credential every ten minutes), then because its binary had been deleted from the machine by disk cleanup in another session. Rebuilt; the job now runs under the service account. |

## What went wrong, and why

1. **A human credential was the single point of failure for production.** Every deploy and the factory's token read went through one person's gcloud login, which expires in roughly twelve hours. It failed five times in six days and once at the exact minute of the opening. *Fix:* an ops service account with the minimum roles, used by the factory job and (once its last permission is in) by deploys. *Rule:* nothing scheduled or unattended authenticates as a person.
2. **The service account was under-provisioned for the one call the opening needed.** It could read secrets and call Cloud Run's API but not pull the image (`artifactregistry.reader`), and Cloud Build refused it for reasons still open. *Fix:* the missing role granted; the Cloud Build gap is documented, not guessed at.
3. **A public sentence composed from a constant.** "open for play since 21 Sep 2026" was built from the opening date and never asked the door, so for two and a half hours the ward told visitors it was open while the door was in preview. *Fix (queued):* the sentence takes the door's state. *Rule:* a claim on a page is derived from the state it describes, never from a date.
4. **An instrument that lied, twice.** A boot marker named "meter" timed 150 lines of unrelated work; after the markers were fixed, a "sessions" span held the sweep and the repair. Each cost about an hour. *Fix:* every span names what is inside it; the unnamed remainder prints as `elsewhere`; median and max per patient, never a mean.
5. **A fix that armed a dormant bug.** The 16 Sep tape-repair fix was placed four lines after a session sweep, under a comment saying "before anything is dropped". Inert only because the Firestore sweep had never deleted anything; making the sweep work on 19 Sep armed it. Found by reading, fixed in a day, no loss known. *Rule:* the rule lived in a comment and a commit message; the test that came with the fix was the first thing that enforced it.
6. **Machine-wide state changed under running work.** Another session switched gcloud's active account; a build agent's workaround moved the Rust toolchain; macOS's tmp reaper deleted files older than three days from two worktrees; a disk cleanup deleted a build directory another job depended on. *Fixes:* per-process gcloud configuration, `rust-toolchain.toml`, long-lived worktrees under the home directory, and a gates wrapper that prints toolchain, tracked-file count and deleted count before its verdict — so a void verdict announces itself.
7. **The free RPC endpoint, met in three places.** A 429 on one patient's history blacked out the whole board; the same listing was made three times per patient; one long history cost 51 s. *Fixes:* a failed history keeps its cached copy and is flagged on the row; one listing per patient per pass, shared; a per-patient budget so a long history costs several bounded passes.
8. **The door's own guard refused the recompiled cases.** The new rubric note said "the case's OSCE mark"; the case door refuses any pack that names the previous season's product. Every one of 75 packs was turned away. *This was the guard working.* Fixed in the compiler with a test that holds the sentence.

## What went right

- **Measuring before building.** Two plausible hypotheses (CPU throttling; a slow RPC listing) were wrong and the instrument said so before code was written for either. The listings item shrank from "parallel fetches with retry" to "stop asking three times".
- **The kept board.** A fresh instance answered from the board its predecessor kept in 0.1–0.9 s on every deploy, including the one where the previous revision was answering unreadable.
- **The case door.** Its season-word guard, its shape checks and its withdrawn rule all fired exactly when they should, including against our own recompile.
- **The clinical advisor's review landed the night before and was applied by morning:** seven compiler rulings (pass mark 70 %, SVT/AF endings, defibrillation scored, paediatric shock pays for antibiotics and glucose, non-deteriorating library cases cut) and per-case emphasis on 14 of 18 endemic cases, each in his words, each with a test.
- **All 20 queued patients had complete portrait sets by opening** — the factory was widened to make faces for waiting patients, not only those in beds, and the daily cap raised for two days.
- **Rollback was one command the whole time** (door back to preview) and was never needed.

## Numbers (measured)

| | before | after |
|---|---|---|
| cold boot to first answer | 137.8 s | 2.7–8.5 s (rust 1.93 → 1.98.1 added ~3 s) |
| ticker pass, 26 patients, quiet ward | 100.1 s | ~4 s |
| signature listings per pass, quiet ward | 26–78 | 0–2 |
| board unreadable after one 429 | yes | no (row flagged, board fresh) |
| sessions swept on Cloud Run, ever | 0 | 4 · 12 · 135 on the first three passes |
| cases placeable at opening | 78 provisional | 75 (3 cut by the advisor), 18 endemic |
| queued patients with all six portrait states | 5 / 20 | 20 / 20 |
| front-page arrivals by 00:00 on opening night | — | 381 (facebook 28 · colosseum 10 · x 8 · discord 5 · linkedin 4 · reddit 3 · no source 323) |

Fees on devnet are paid by the relay: 10,000 lamports a shift (two signatures), measured. Portrait edits are estimated at list price, ≈ USD 1–3 a day at the caps used.

## Rules we wrote down

1. **A rule that is not executable has a shelf life.** Every rule that held this week was a test, a gate, a guard or a first line that prints its own preconditions; every rule that failed was a sentence.
2. **An instrument accounts for its whole.** Named spans, `elsewhere` for the rest, median and max rather than a mean.
3. **A gate is held by a guard, not by a line you expect to reach.**
4. **A figure's precision is finer than the change the claim makes about it.** One per cent a year of 537 is 5.4 people; a number rounded to ten cannot carry that claim.
5. **Measure before building; ask once and share the answer before making anything faster.**
6. **Persistence and trust are decided separately.** Keep the progress of a partial read; never decide on it.
7. **Nothing unattended authenticates as a person; production is changed by the deploy script from a committed revision, never by hand.**
8. **A public sentence is derived from the state it describes.**

## Open at the time of writing

- ~~The status sentence still ignores the door.~~ Fixed 21 Sep (`catalogue_status(door)`, `8512cbc`); deployed 22 Sep.
- ~~The refill can admit a queued patient whose case was withdrawn.~~ Fixed 21 Sep (`Placeable::may_place`, `e57609a`, with the cold-start guard); deployed 22 Sep.
- The ops service account cannot yet submit Cloud Build; deploys still run under the founder.
- 0 of 75 cases carry the *reviewed* label: the advisor's review exists and is applied to the packs, but the label waits on confirming its provenance directly with him.
- Nobody outside the team had taken a shift by midnight. The first stranger's receipt will be the first real test of everything above.

## Addendum, 22 Sep — how each of these does not happen again

The founder's question the morning after was the right one: not *what went wrong* but *what makes it impossible next time*. One row per failure above; the middle column is the mechanism, not a resolution to be careful. "Done" means merged and, where it applies, deployed; a date means queued with an owner.

| # | Failure | What makes it impossible | Status |
|---|---------|--------------------------|--------|
| 1 | A person's login was the only way to open the door | `scripts/ward-door.sh open\|preview\|closed` flips the door as the ops service account, from the secret, with no build — and refuses to run as a person. The scheduled tick and the factory already run as the service account. | script queued, 23 Sep; SA in use for the factory and the ticker since 21 Sep |
| 2 | The service account lacked one role on the one call that mattered | The roles are written down with the service account, and `scripts/opening-check.sh` exercises every call the opening needs (secret read, revision describe, image pull, build submit) as the service account, days before, and prints one line per call. | check script queued, 23 Sep; Cloud Build grant still open |
| 3 | A public sentence composed from a constant | `catalogue_status` takes the door; `tests/doors.rs` holds the preview wording and the open wording apart, and the end-to-end door suite (33 cases) reads the sentence off the running service. | done, deployed 22 Sep |
| 4 | An instrument that lied | Every boot and pass span is named; the remainder prints as `elsewhere`; the slow-pass line carries median and max per patient. A span whose name does not match its body cannot pass the boot test that reads the marks in order. | done, deployed 20 Sep |
| 5 | A fix that armed a dormant bug | The ordering rule (repair before sweep) is a test that runs with a tape the sweep would have dropped; the comment is gone, the test stays. | done, deployed 20 Sep |
| 6 | Machine-wide state changed under running work | `rust-toolchain.toml` pins the compiler for every session and CI; gcloud is pinned per process (`CLOUDSDK_ACTIVE_CONFIG_NAME`) and no session sets the global account; long-lived worktrees live under `~/Developer`; the gates wrapper prints toolchain, tracked-file count and deleted count on its first line so a hollow run announces itself; the factory binary moves to `~/.vitals/bin/` where no build cleanup reaches it. | pin, gates line, worktrees done 20–21 Sep; binary path done 22 Sep |
| 7 | The free RPC met in three places | One listing per patient per pass; a failed history keeps its cached copy and flags the row; a per-patient budget. The staging blackout on a single 429 is a test now. Cloud Monitoring alerts on 5xx rate and on the tick's `took_ms` are the next layer, so the next 51 s pass pages a person instead of waiting for one to look. | fixes done, deployed 20–21 Sep; alerts queued, 24 Sep |
| 8 | The guard refused the recompiled cases | Kept as is — the guard was right. The compiler's own test holds the sentence it writes so a wording change fails at build, not at the door. | done, 21 Sep |
| — | Production changed by hand under pressure | After 26 Sep, production deploys come from CI on a tagged commit; until then the deploy script runs only from a committed revision the producer has read, with `MIN_INSTANCES=0` explicit. The one exception on opening night (an env-only flip of the already-built image) is recorded in the incident log. | rule in force; CI deploys queued, 26 Sep |

*Vitals World is MegaWiz's entry to Colosseum's Crypto World's Fair hackathon (14 Sep – 12 Oct 2026), built during the event on top of Vitals and Embla, which are disclosed as prior work.*
