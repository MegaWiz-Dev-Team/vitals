# Unlocking episodes — and the one thing money must never buy

The question this answers: *can you spend a token to open a new scene?*

Short version: **skill opens episodes. Money can open a door, and it can never put anything on
your record.** Those are two different verbs and keeping them apart is the whole design.

## Three ways an episode opens

### 1 · Progression — the default, and always free

Clear EP1 and EP2 opens. This is the path the game is built around and it costs nothing, ever.
Today the gate is the app's and runs client-side. The design moves it on chain as `required_badge`
on the scenario registry — `designed, not built`, and no such field exists in
`crates/vitals-program` — so that the prerequisite would sit where a competing front-end honours
the same gate without asking us.

**No token is involved.** A learner with no money, no wallet and no sponsor plays the entire
season by being good at it.

### 2 · Sponsor — `designed, not built`: the token would flow, the player would still pay nothing

An institution, an alumni fund or a specialty college would open an episode **for a whole cohort**.
They would pay; the author would be paid per replay; every player in that bay would see it unlocked.

This is the flow the token was designed for, and note who is *not* in it: the player. They would
not hold $VIGIL, would not see a wallet, and would not be aware anything had been transacted.

There is nothing to hold. **$VIGIL does not exist** — no mint, no bond account, no staking
instruction and no slashing instruction anywhere in this repository — and
[SPRINT_PLAN.md](SPRINT_PLAN.md) lists **any fungible token** among the explicit scope cuts, as a
v2 line. [TOKENOMICS.md](TOKENOMICS.md) is that design written out under the same label, and it is
worth reading before anyone reinvents it.

### 3 · Impulse — consumer tier only, and strictly cosmetic to standing

Someone playing for entertainment who does not want to grind could open an episode directly.
That is a normal consumer game transaction and there is nothing wrong with it — **provided it
buys the story and never the standing.** No payment path is built; this one is a drawing too.

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
share incentives, why the arena would be named **3R** and the token **$VIGIL** so that not even the
names would touch — neither name exists in `crates/` or `demo/` — and why the distinct-case gate
exists: grinding a favourite episode buys a better *time* and never a better *standing*. The last
of those is built; the first two are the design keeping its distance in advance.

## Current state

| | |
|---|---|
| Progression unlock (clear one, open the next) | **built** — in the app, client-side for now |
| `required_badge` on the scenario registry | designed, not built |
| Sponsor unlock + author royalty | designed, not built |
| Impulse unlock | designed, not built, and deliberately last |
| $VIGIL, the 3R arena, and any fungible token | designed, not built — see [TOKENOMICS.md](TOKENOMICS.md) |

The order matters. The free path shipped first because it is the one the design depends on.
