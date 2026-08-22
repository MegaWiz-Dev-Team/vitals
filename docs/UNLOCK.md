# Unlocking episodes — and the one thing money must never buy

The question this answers: *can you spend a token to open a new scene?*

Short version: **skill opens episodes. Money can open a door, and it can never put anything on
your record.** Those are two different verbs and keeping them apart is the whole design.

## Three ways an episode opens

### 1 · Progression — the default, and always free

Clear EP1 and EP2 opens. This is the path the game is built around and it costs nothing, ever.
It is `required_badge` on the scenario registry: the prerequisite lives on chain, so a competing
front-end honours the same gate without asking us.

**No token is involved.** A learner with no money, no wallet and no sponsor plays the entire
season by being good at it.

### 2 · Sponsor — the token flows, the player still pays nothing

An institution, an alumni fund or a specialty college opens an episode **for a whole cohort**.
They pay; the author is paid per replay; every player in that bay sees it unlocked.

This is the flow the token was designed for, and note who is *not* in it: the player. They do not
hold $VIGIL, do not see a wallet, and are not aware anything was transacted.

### 3 · Impulse — consumer tier only, and strictly cosmetic to standing

Someone playing for entertainment who does not want to grind can open an episode directly.
That is a normal consumer game transaction and there is nothing wrong with it — **provided it
buys the story and never the standing.**

The safeguard is already built and already enforced on chain:

> Opening an episode gets you into the bay. Your level still comes from `claim_progress`, which
> recomputes `distinct_cases`, `avg_bps` and the difficulty mix from **anchored runs**. A bought
> episode you played badly is an attempt with a bad score, which is worse for you than not having
> played it.

You cannot buy a leaf. You can only buy the chance to make one.

## What is never for sale

- **Standing.** No payment path touches `claim_progress`. The program does not know or care how a
  scenario came to be unlocked.
- **A better score.** The automaton does not negotiate.
- **Learner access.** In any institutional or educational tier, every episode is reachable by
  progression alone. If a learner ever hits a paywall, the deployment is misconfigured.

## Why this is worth being strict about

A credential is worth exactly as much as the least honest way to obtain one. The moment money and
standing touch — even indirectly, even through a leaderboard that feeds a level — the whole record
is worth nothing, and it is worth nothing *retroactively*, including for everyone who earned it
properly.

That is why [PLAY.md](PLAY.md) says the fun layer and the credential layer share data and never
share incentives, why the arena is called **3R** and the token **$VIGIL** so they do not even share
a name, and why the distinct-case gate exists: grinding a favourite episode buys a better *time*
and never a better *standing*.

## Current state

| | |
|---|---|
| Progression unlock (clear one, open the next) | **built** — in the app, client-side for now |
| `required_badge` on the scenario registry | designed, not built |
| Sponsor unlock + author royalty | designed, not built |
| Impulse unlock | designed, not built, and deliberately last |

The order matters. The free path shipped first because it is the one the design depends on.
