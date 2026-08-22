# embla-proof — Architecture

## 1. Trust model (start here)

The question every judge will ask: *what stops a student from writing themselves a
passing grade?*

```
student device                verifier (school box / protocol node)      Solana
──────────────                ────────────────────────────────────      ──────
                                                                    
 1. request attempt  ─────────▶  issue nonce, sign commit         ──▶  commit PDA
                                 hash(case_id ‖ student ‖ nonce)        (t0 anchored)
                                                                    
 2. run encounter                (inference local via Heimdall)          
    transcript ─────────────────▶                                        
                                                                    
 3.                              deterministic rubric score              
                                 pin engine_ver + model_id               
                                 append to hash-chained audit log        
                                                                   ──▶  reveal:
                                                                        cNFT leaf =
                                                                        hash(transcript,
                                                                             result,
                                                                             engine_ver,
                                                                             model_id,
                                                                             chain_head)
 4. after-action report ◀────────                                        
    + attestation visible                                               
```

The student's device never holds a signing key that can produce a valid attestation.
The verifier does. Three properties follow:

- **Ordering is fixed.** The commit lands before the encounter, so the number of attempts
  started is public even when only some are revealed. Practising until you get a good run
  is visible as an attempt count, not hidden as a single lucky record.
- **Scores are re-derivable.** `engine_ver` and `model_id` are pinned in the leaf. Given the
  revealed transcript, an independent verifier re-runs `embla-engine` at that version and
  must reproduce the same rubric result. A mismatch is a provable dispute, not a he-said-she-said.
- **Escalation is a parameter.** Low-stakes practice: one verifier. Institutional exam:
  n-of-m quorum of verifier nodes, all signatures required on the leaf.

**Honest limit:** this proves the *scoring* was faithful to the transcript. It does not prove
the human at the keyboard was the enrolled student. Identity binding is out of scope for the
protocol — it is delegated to the issuer (school SSO / proctoring), and the credential carries
the issuer's identity so a relying party can judge how much that issuer's proctoring is worth.
Say this out loud; do not let a judge discover it.

## 2. Onchain accounts

### Case Registry (Anchor program)

```
CaseAccount (PDA: ["case", author, case_slug])
  author           Pubkey
  content_hash     [u8; 32]   sha256 of the canonical case JSON (content off-chain)
  rubric_hash      [u8; 32]   sha256 of the rubric — separately hashed so a rubric fix
                              is visible without re-publishing the case
  schema_version   u16
  price_per_attempt u64       in USDC base units; 0 = free/open
  royalty_bps      [(Pubkey, u16)]  split: author / school / protocol
  attempts         u64        counter, incremented on reveal
  status           enum { Draft, Published, Deprecated }
```

Content stays off-chain: encrypted on the author's Embla box, or on Arweave/IPFS for
open cases. The chain holds the commitment, not the payload — a case body is clinical
teaching material with real commercial value and it is not going onchain in the clear.

### Attempt Anchor

```
CommitAccount (PDA: ["commit", case, student, nonce])   -- small, closed on reveal
  case             Pubkey
  student          Pubkey
  commit_hash      [u8; 32]
  verifier         Pubkey
  t0               i64

RevealTree (concurrent merkle tree, Bubblegum)
  leaf = hash(commit_hash ‖ transcript_hash ‖ result_hash ‖ engine_ver ‖ model_id ‖ audit_chain_head)
```

Reveals go into a compressed tree because this is the high-volume write. Commit accounts
are rent-reclaimed on reveal so the steady-state cost per attempt is the leaf, not an account.

### Competency Credential (SAS)

Issued by an accredited issuer, not by us:

```
credential : "embla-proof competency"
schema     : { domain: string, level: u8, blueprint_ver: string,
               attempts_counted: u16, window_end: i64 }
attestation: issued to student pubkey, signed by issuer authority
```

Level thresholds come from Embla's existing competency model (`engine/src/competency.rs`,
Dreyfus levels per specialty), not invented here.

## 3. Off-chain components

| Component | Role | Stack | Source |
|---|---|---|---|
| `embla-engine` | encounter + deterministic rubric + audit chain | Rust | reused from Embla |
| `proof-verifier` | wraps the engine, holds issuer key, signs commit/reveal | Rust | **new** |
| `proof-program` | case registry + commit/reveal | Rust / Anchor | **new** |
| `proof-sdk` | TS client: wallet, gasless relay, read registry | TypeScript | **new** |
| `proof-check` | public web page: paste transcript → verify against chain | TS + Rust wasm | **new** |
| Heimdall | local LLM gateway (inference never leaves the box) | — | reused |

Rust across the engine, the verifier and the Anchor program is not an aesthetic choice —
it means the *same* scoring code path can be compiled into the verifier and, for the
hash-recipe subset, cross-checked in wasm on the public verify page.

## 4. Payment flow

Attempt on a paid case:

```
student (or school's prepaid pool)
   │  USDC, Token-2022
   ▼
reveal instruction ──▶ split by royalty_bps ──▶ author / school / protocol
```

Institutions do **not** transact onchain. A school buys seats in baht on a normal invoice;
the platform funds a prepaid pool and settles author royalties onchain on their behalf.
The crypto rail exists so an independent case author in any country gets paid per attempt
without an acquiring relationship. That is the part card rails genuinely cannot do.

## 5. Open questions

- **Tree sizing / rollover.** One tree per cohort-year, or one global tree with a canopy?
  Affects proof size on the verify page.
- **Revocation.** A case found to be clinically wrong invalidates downstream attestations.
  SAS supports revocation by the issuer — is per-attempt reveal revocation also needed, or is
  deprecating the case enough?
- **Determinism boundary.** The rubric scorer is deterministic; any LLM-assisted grading
  component is not. Either exclude LLM-graded dimensions from the anchored score, or anchor
  them separately as "advisory, not re-derivable". **Decide before Week 2** — this is the
  single most likely place for the pitch to fall apart under questioning.
- **Selective disclosure.** v0.1 is reveal-the-whole-transcript. A ZK range proof
  ("score ≥ 70 without showing the transcript") is the obvious v2 and probably the thing
  that makes this interesting outside medicine. Out of scope for four weeks; name it as roadmap.
