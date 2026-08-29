# vitals — 4-Week Sprint Plan

Colosseum Eternal: a four-week timed sprint, **29 Aug 13:03 → 26 Sep 13:03 (+07)**, with a
one-minute video update every week and a submission at the end. The dates here are read off the
dashboard rather than assumed — RISKS §1 records what it cost the one time they were not.

Rule for the whole sprint: **anything not demoable on the last day does not get built.**

The head start is real and it is worth being exact about where it is. The encounter engine, the
onchain program and the replay verifier all exist and run on devnet; the architecture table in
the README says which layers are shipped and which are still drawings. What has no head start is
the evidence — whether the clinical content is right according to someone who is not us, and
whether anyone pays for it. Four weeks buys those two and the videos that show the work.

## Week 1 — correctness before capability (29 Aug – 5 Sep)

- [ ] The existing cases go out for external clinical review — doses, timing thresholds,
      diagnosis codes. Their rubrics stay marked provisional until it comes back
- [ ] Every claim in the repository re-read against the program. Anything the code does not do
      comes out of the documents rather than being softened
- [ ] Project created on the sprint dashboard

Exit: the documents describe the program that actually exists, and the clinical content is in the
hands of people qualified to say it is wrong.

## Week 2 — act on the review (5 – 12 Sep)

- [ ] Apply the corrections that come back. A rubric stops being provisional because a physician
      checked it, not because the word was deleted
- [ ] The reviewer path becomes a route in the product instead of a document exchange
- [ ] **Project form complete** — a milestone of this week, not a week-4 task

Exit: a reviewer can file a finding without an email, and a corrected rubric carries a verdict
rather than a caveat.

## Week 3 — viability, and figures anyone can read (12 – 19 Sep)

- [ ] **Answer who pays for a case.** The Case Registry is `designed, not built` because that
      question is open, not because the code is hard — three plausible payers give three different
      protocols, so the answer comes before the instruction
- [ ] Publish the funnel — arrival, play, completion — on the same public endpoint that already
      serves the raw usage counts, uncurated, including the days they read one and one
- [ ] **Team profiles complete**
- [ ] Start writing the two week-4 videos. Three minutes each is about three times a weekly
      script, and that does not fit inside week 4

Exit: the funding question has an answer we are willing to publish, including the answer that we
do not have one yet.

## Week 4 — finish and submit (19 – 26 Sep)

The submission box opens 19 Sep 13:03 and the sprint closes on the 26th. We file on
**22–23 Sep**: review is first-come-first-served and results arrive fourteen days after each
team's own submission date, so filing early is an earlier answer and a margin for the day an
upload misbehaves. A deliberate choice you miss by a day was never a choice.

- [ ] Pitch video, ≤3 minutes
- [ ] Technical demo, ≤3 minutes — it shows the program refusing a level-up claim before it shows
      one being accepted: a program turning down a claim it can check is the one part of this that
      cannot be mocked up, and it is worth more than the happy path
- [ ] Every claim audited against the code once more before the button is pressed
- [ ] Submission write-up + repo hygiene — the repo is part of what ships, not a byproduct of it
- ~~Mainnet-beta deploy~~ — ตัดโดยมติ 26 ส.ค.: ส่งแข่งบน devnet; mainnet trigger = เส้นเงินจริง เหมือนเดิม

## Weekly video updates

One minute, once a week. Do not narrate the roadmap — show the thing that started working that
week. Week 1's is the integrity work: a leak found in the exam and closed, the claims that had
outrun the code taken back out of the documents, the cases sent for review, and the part of the
leak still open stated with the numbers that measure it. Every week after is held to the same
rule against whatever that week actually produced.

Week 1's box opens 3 Sep and is due on the 5th — it appears days before its deadline, so each
week's video is finished by the time the box appears rather than on the day it closes.

## Explicitly out of scope

ZK selective disclosure ("top decile without revealing the score") · verifier-node DePIN with
staking · any fungible token · mobile app · newly authored clinical cases · **tradeable badges,
ever**.

The first five are v2 lines. A cut stated up front beats a half-built feature: the half-built
one costs the same maintenance as a whole one, has to be explained every time it is seen, and
still does not do the job. Correcting a case that already exists is week 2's work and is not on
this list; writing new ones is.
The last is not a scope cut, it is a design decision — see [GAMIFICATION.md](GAMIFICATION.md) §2.

Two more stay exactly where the README's architecture table puts them, `designed, not built`: the
Case Registry and the competency credential. Neither is held up by a coding question — who pays
for a case, and whose name is on the attestation — and week 3 attempts the first of those, not
either build.
