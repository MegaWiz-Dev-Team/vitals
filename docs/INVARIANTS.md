# Invariants

What must always be true of the on-chain program, where each is enforced, and what proves it.

This exists because of the finding that produced it. The tree PDA was validated — against a
derivation seeded on a globally-guessable number, so the check passed while anyone could address
anyone's tree. No scanner catches that: the check was *present*. It was found by asking **who is
allowed to write here**, which is an invariant question, and it is the question an auditor cannot
ask on our behalf without being told what we believe.

Hand this to an auditor with the frozen commit. An auditor who has to infer intent spends the
budget on comprehension instead of on finding the next one of these.

## Identity and authority

| # | Invariant | Enforced | Proven by |
|---|---|---|---|
| 1 | Every instruction requires two signatures: the funder, and a device | `is_signer` on both, in each handler | `the_relay_cannot_sign_for_the_player` |
| 2 | A device may only act for an account that lists it | `authorised` → `account.allows` | `naming_an_account_that_is_not_yours_is_refused_everywhere` |
| 3 | An account's stored id must match the address it lives at | `authorised`, PDA vs `account.id` | *(unreachable without a forged account — see below)* |
| 4 | A player's record is seeded on the player, never the funder | `SEED_ACCOUNT` on `account.id` | `a_player_with_no_sol_can_still_prove_and_claim` |
| 5 | Adding a device requires an existing authority to sign | `authorised` before `AddAuthority` | `devices_can_be_dropped_but_never_the_last_one` |
| 6 | At most 8 devices, and the last one cannot be removed | `MAX_AUTHORITIES`, `LastAuthority` | `the_device_list_refuses_a_ninth_machine`, `devices_can_be_dropped_but_never_the_last_one` |

Invariant 4 is why an account keeps working across machines with no migration: the account id is
the first device's key, so the PDA never moves when a second device joins.

## The anchoring tree

| # | Invariant | Enforced | Proven by |
|---|---|---|---|
| 7 | A tree belongs to the funder that created it, and no other funder can address it | `tree_pda` seeds on the operator | `a_stranger_cannot_append_to_my_tree`, `two_operators_in_one_slot_do_not_share_a_tree` |
| 8 | The same operator and id always resolve to the same tree | same, deterministic | `the_same_operator_and_id_is_always_the_same_tree` |
| 9 | A tree never exceeds `MAX_LEAVES` (4,096) | `next_index >= MAX_LEAVES` before append | merkle unit tests — **not at the program boundary** |
| 10 | Appending is the only mutation; leaves are never rewritten | no instruction writes an existing leaf | by construction |

Invariant 7 was false until 2026-08-24. Two servers started in the same slot reported the same
tree, and any funded signer who read that number off a status line could append to a stranger's
tree.

## Records and proofs

| # | Invariant | Enforced | Proven by |
|---|---|---|---|
| 11 | A record's difficulty and outcome must name a real variant | `RecordWire::decode` | `a_record_with_an_impossible_difficulty_or_outcome_is_refused` |
| 12 | A proof only counts if the leaf is in the tree at the claimed index | `root_from_proof` vs stored root | `anchor_prove_claim_and_every_way_it_can_refuse` |
| 13 | One run counts once | `DuplicateAttempt` | `anchor_prove_claim_and_every_way_it_can_refuse` |
| 14 | A claim must be earned by proven attempts, recomputed on chain | `ClaimNotEarned` | `anchor_prove_claim_and_every_way_it_can_refuse` |
| 15 | A player cannot prove someone else's run | `NotYourRun` | `anchor_prove_claim_and_every_way_it_can_refuse` |
| 16 | Every PDA passed in must be the canonical one for what it claims to be | `WrongPda`, six sites | `naming_an_account_that_is_not_yours_is_refused_everywhere` |

## What is deliberately not an invariant

- **Anyone may read anything.** Nothing here is confidential; the record is meant to be verified by
  strangers.
- **The tree is shared by every player on a server.** That is the design — proofs are per-player
  through the claim and progress accounts, not through tree ownership.
- **No PHI or personal data ever reaches the chain.** Hashes and public keys only. This is a
  property of what the server sends, not something the program can enforce, and it belongs in a
  review of `chain.rs` rather than here.

## Known gaps

Named so they are not rediscovered as findings.

- **Invariant 3 is untestable from outside.** It guards an account whose contents disagree with its
  own address, which cannot be produced through any instruction. It is defence against a future
  bug, not against a caller.
- **`TreeFull` and `ClaimFull` are not covered at the program boundary.** They need 4,096 leaves
  and 16 anchor-and-prove cycles respectively; both are better reached by pre-seeding an account
  than by paying for them on every run.
- **`WrongOwner` is not covered.** It needs an account owned by another program, which
  `ProgramTest::add_account` can construct.
- **Overflow.** The arithmetic is defended by bounds rather than by `checked_*` — `norm_bps`
  returns early on a zero divisor and clamps, the tree checks before incrementing — and
  `overflow-checks` is on in release so a future unguarded line aborts rather than wraps.
- **Upgrade authority.** Until it is a multisig, every invariant above describes bytecode that one
  key can replace. See `DEPLOY.md`.
