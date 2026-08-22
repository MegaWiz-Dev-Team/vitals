# $VIGIL — the verification market

> The token secures verification. It never represents competence.
> That sentence is the whole design, and the second half is the load-bearing one.

**Why "vigil".** A vigil is kept *beside* the patient, never of the patient — which is exactly
where this token sits relative to the credential. Operators keep vigil over a replay; lie about
what you saw and your bond is slashed. "Vigilance" has been the motto on the American Society of
Anesthesiologists' seal since 1932, under a lighthouse standing for dependable presence, and that
is the job description.

**And why the arena has a different name.** The place players compete is **3R** — the emergency
room, read back. The leaderboard is the 3R board; the season happens in 3R. Players never hold
$VIGIL and never see it, so the two layers do not merely avoid sharing incentives — *they do not
share a name.* If you ever find yourself wanting to call the board "the $VIGIL leaderboard",
something has gone wrong upstream.

## 1. The problem a token actually solves here

Everything else in Vitals is trustless already. The scenario is public, the replay is
deterministic, the progression predicate is recomputed onchain. There is exactly one place left
where someone has to be trusted:

**Somebody has to replay the tape and attest the outcome.**

Make that one party — us, or a school, or a foundation — and we have rebuilt the database this
project exists to escape. The credential ends up worth exactly as much as that party's reputation,
which is the current state of digital credentials and the reason nobody uses them.

So the verifier set has to be open. And an open verifier set needs a reason not to lie.

## 2. Why fraud proofs work here and usually don't

Optimistic verification is a well-worn idea that mostly fails, because most disputes are about
something fuzzy — was this content acceptable, was this price fair, was this answer good.

Replay is not fuzzy. Given the scenario hash and the tape, the outcome is **exact**. Two honest
verifiers cannot disagree. So:

```
operator stakes $VIGIL          → eligible for replay work, bond sizes their throughput
operator verifies + signs       → fee paid by whoever ordered the verification (USDC)
anyone may challenge            → post a bond, re-run the tape, the chain sees who is right
wrong party slashed             → challenger paid from the slash, a share burned
```

A dishonest verifier is not caught by a committee. They are caught by arithmetic, by anyone,
at any time, forever — because the tape is anchored and the scenario is pinned. **The determinism
that makes the credential meaningful is the same property that makes the token enforceable.**
Neither half works without the other.

## 3. Sinks and sources

| | Flow | Effect on supply |
|---|---|---|
| **sink** | verifier bond, locked while active | out of circulation |
| **sink** | scenario listing bond, dispute bonds | out of circulation |
| **sink** | slashing — a share burned | destroyed |
| **source** | verification fees → operators | earned |
| **source** | replay royalties → scenario authors | earned |

**Who pays:** institutions on ordinary invoices, and sponsors funding outcome-linked
scholarships. Fees are denominated in USDC; the token is the bond and the punishment, not the
unit of account. A school does not have to hold $VIGIL to use Vitals, and will not.

**Who earns:** node operators and scenario authors — the two parties whose work the network
consumes. Weighting distribution toward them rather than toward speculation is the only allocation
decision that follows from the design rather than from convention.

**Who pays nothing:** players. Gasless via fee-payer relay, no wallet to fund, no token to hold,
no seed phrase to lose. A student who cannot afford anything can still practise and still earn a
credential.

## 3b. Who actually gets $VIGIL — and who never does

This section exists because the question "how do I get some?" did not have an obvious answer from
the rest of the document, and if the person building it has to ask, a judge will too.

| you are | do you hold $VIGIL? | how |
|---|---|---|
| **a player** | **no, by design** | you never touch it, never see it, never need a wallet |
| a scenario author | yes | earn per replay of a scenario you wrote |
| a verifier operator | yes | stake it to take replay work, earn fees on top |
| a challenger | yes | catch a verifier lying and take their slashed bond |
| an institution or sponsor | mostly no | they pay in USDC on an invoice; the token is bonded on their behalf |

**Players earn nothing, and that is the design rather than an oversight.** The moment playing pays,
farming becomes economically rational — and the distinct-case gate only protects *standing*, not
*earnings*. Someone grinding EP1 four hundred times would never rise above Advanced beginner and
would still be making money, which is exactly the incentive that has to not exist inside a system
whose output is a medical credential.

### What a player *can* receive

Money, but never this token, and never for playing — only for a provable achievement:

> A sponsor escrows a scholarship. *"First 100 to reach Emergency Medicine · Proficient this year:
> $150 each."* The payout is **stablecoin**, released against a level the program recomputed from
> anchored runs, gated by distinct cases so it cannot be farmed.

The player is paid for **being good**, in a currency that is not the network's, by someone who
chose to fund it. Nobody is paid for **playing**.

### If we ever change our mind

The alternative — players earning $VIGIL — is a normal play-to-earn design and it is not hard to
build. It should be refused for a specific reason rather than on principle: this project's product
is a record that a residency programme is meant to rely on, and the first question anyone asks
about a paid-to-play credential is who was farming it. There is no answer to that question that
survives a regulator, and losing it would invalidate every credential already issued, including the
honest ones.

## 4. What is never tokenized

This list matters more than the one above, and should be read out loud in the pitch:

- **Credentials, badges, skill trees.** Token-2022 `NonTransferable`, no exceptions. A tradeable
  "Expert in Cardiology" is credential fraud with extra steps, and a liquid market for one would
  destroy the system faster than any technical attack.
- **Access to play.** No token gates a learner out of practice, in any tier, ever.
- **The outcome of a run.** Nobody can buy a better replay. The automaton does not negotiate.

We are giving up the secondary-market volume on purpose. Every pitch reaches for tradability
because it makes the tokenomics slide easier; refusing it is the part that shows we understand
what we are building.

## 5. Honest position on timing

**No token ships inside the hackathon window, and the deck says so.**

A verification market only needs to exist once verification is worth attacking. Today the verifier
set is one node and the correct design is to say that plainly. Launching a token before there is
anything to secure would be the exact overclaim the rest of this project is built to avoid — and
judges who have seen a thousand token slides will notice which kind they are looking at.

The allocation table is a **proposal, not a commitment**. Precise percentages presented as
settled facts, for a network that does not yet exist, would be a number invented to fill a slide.

## 6. The regulatory line

A token that gates or represents a professional healthcare credential is a bad idea legally as
well as ethically. The split above keeps $VIGIL entirely on the infrastructure side of that line:
it pays for computation and punishes lying about computation. It confers no standing, no
qualification, and no access to care.

Anyone evaluating this should be able to check that in one read of the sinks table — which is why
the table is short.
