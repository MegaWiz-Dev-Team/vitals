# Play — what makes Vitals fun

> Honest starting point: **what has been built so far is an accounting system, not a game.**
> XP, levels and a Dreyfus ladder are the credential layer, and they are the least fun part of
> any game ever shipped. Nobody has ever told a friend about a progress bar.
>
> The fun has to come from somewhere else, and in Vitals it comes from one place: the replay is
> a real object. Everything below falls out of that.

## 1. The replay is the trophy, not the score

A run does not produce a number. It produces a **watchable ninety-second story that anyone can
verify**: adrenaline at 0:38, she settles, you stand her up, she collapses, she lives anyway.

So a player's profile is not a rank. It is a shelf of runs. That is speedrun culture rather than
leaderboard culture, and it is the only trophy case in existence where every trophy can be
re-derived by a stranger.

## 2. Ghost racing — the mechanic determinism hands us for free

The tape is deterministic, so **another player's run can be replayed alongside yours.** Trackmania's
ghost car, in an emergency room.

Load EP1 with the record holder's ghost. Their adrenaline goes in at 0:38 and yours at 1:12 — and
you do not read that in a table afterwards, you *watch the two patients diverge in real time* while
you are still deciding. One monitor stabilising, one still falling.

No other medical simulator can do this, and it costs nothing to build, because determinism was
already required for the credential.

## 3. The board is called 3R

The arena has its own name — **3R**, which is "ER" read back. Not $VIGIL, deliberately: players
never hold the token and never see it, so the competitive surface and the bonding asset should not
even share a word. The 3R board is where you rank. $VIGIL is what a verifier stakes to be allowed
to say your run happened.

## 4. Door-to-drug is a speedrun category the profession already keeps

Medicine invented these timings and audits hospitals against them:

| category | scenario | the real-world standard |
|---|---|---|
| door-to-adrenaline | EP1 | minutes decide it |
| door-to-balloon | EP2 | ≤ 90 minutes |
| time-to-airway-team | EP3 | before the child goes quiet |
| door-to-anticoagulation | EP4 | before the collapse |
| time-to-haemostasis | EP5 | the first thing that matters |

They are already leaderboard categories. And unlike every speedrun board that has ever existed,
**every entry here is a tape that anyone can re-run** — the thing those communities have wanted
since the first submitted VHS tape.

## 5. Harm is the stat that makes a player interesting

Not deaths — harm events. Fifty saves with twelve harms reads completely differently from thirty
saves with none, and both are respectable.

It also keeps the game honest: **the fastest run and the cleanest run are different runs.** Rush
and you stand her up. Slow down and the airway closes. Without that tension a timed medical game
degenerates into button-mashing the known-good sequence.

## 6. Failure has to be content

"She died because you reached for antihistamines" is a better clip than any win. A death should
produce a **death card** — the beat list, the moment it turned, and the diff against what an expert
would have done — that a player wants to show someone.

The design rule: *losing should be legible, teach immediately, and be shareable.* A loss screen
that only says you lost has wasted the most memorable thing that happened all session.

## 7. Every episode needs one counter-intuitive moment

EP3 is the template. Nearly every reflex — look in the throat, get a line in, take the child from
the mother — is harm. The first time a player learns that **doing nothing is the treatment**, they
tell somebody.

That is a content rule, not a mechanic, and it is the one that decides whether the season is
memorable or merely correct.

## 8. The daily seed

One scenario, one variant, everybody plays the same one today. Same `sce_hash`, so the board is
verifiable by construction and the comparison is exactly fair.

Wordle's shape, with a patient in it.

## 9. Co-op: the consult is another player

The lifeline already exists in the design as "phone a specialist". In multiplayer the specialist is
**another player who can only see what you tell them.** They are working from your description,
which is the actual failure mode of real clinical escalation, and it is a genuinely good asymmetric
game mechanic on its own.

## 10. The refusal, reframed as a quest log

Today, claiming a level you have not earned is an error. It should be the most useful screen in the
game:

> **Proficient** needs five distinct cases. You have three.
> Unplayed: *The Masquerader* · *The Night the Stars Fell*.

The progression system already knows exactly what is missing — that is a quest log, and it is
honest, which most quest logs are not.

## What to be careful about

- **Do not lead with XP.** It is the credential layer wearing a game's clothes. The shelf of runs
  is the reward; the level is paperwork that happens to be true.
- **Death must teach in the same breath.** Punishment without immediate explanation is how a
  clinical game becomes something students avoid rather than replay.
- **Keep speed and safety in tension**, or the harm penalty is decoration and the meta collapses
  into one memorised sequence per episode.
- **Never let the fun layer inflate the credential.** They share data and must not share
  incentives. A leaderboard that raises a Dreyfus level is how the whole thing rots — the
  distinct-case gate exists precisely so that grinding a favourite episode buys a better *time*
  and never a better *standing*.

## Built · designed

| | state |
|---|---|
| XP, level, Dreyfus, distinct-case gate | **built**, recomputed on chain |
| Anchored replay per run | **built** |
| Harm on the record | **built** — it already changes the leaf |
| Ghost racing · speedrun categories · daily seed · co-op consult · death card · quest log | designed here, none built |
