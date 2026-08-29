# vitals — Architecture

> **What is running here, and what is a drawing.** This file is the protocol design, and the built
> system departs from it deliberately in three places a reader should know before the first
> diagram. Anchoring uses a fixed-depth Merkle tree the program keeps itself, not a Bubblegum
> tree, so a proof is checkable on a laptop with no indexer and no network. Progression is a PDA
> the program writes, not a mint — `crates/vitals-program` contains no mint, no token, no cNFT and
> no Token-2022, and no Anchor either: it is a native `solana-program` entrypoint. The Case
> Registry, the royalty split and the SAS credential are designed and not built. The README's
> architecture table carries the same split, primitive by primitive.

## 1. Trust model (start here)

The question every judge will ask: *what stops a student from writing themselves a passing grade?*

```mermaid
sequenceDiagram
    autonumber
    actor Student as Student Client
    participant Verifier as Verifier Node (Rust)
    participant Solana as Solana Program & Merkle Tree

    Note over Student,Solana: Phase 1: Commit Before Play (Anti-Retry Farming)
    Student->>Verifier: 1. Request attempt (case_id, student_pubkey)
    Verifier->>Solana: Issue nonce & Anchor Commit PDA<br/>hash(case_id ‖ student ‖ nonce)
    Solana-->>Verifier: Commit PDA anchored (t0 recorded)

    Note over Student,Verifier: Phase 2: Live Encounter Simulation
    Student->>Verifier: 2. Interactive action stream (questions & medication orders)
    Note over Verifier: Local inference via Heimdall (Dialogue)<br/>Deterministic Physiology Automaton (Vitals & Outcomes)

    Note over Verifier,Solana: Phase 3: Deterministic Reduction & Anchor
    Verifier->>Verifier: 3. Compute discrete facts (ordered beats, harm, outcome)<br/>Append to hash-chained audit log
    Verifier->>Solana: 4. Reveal & Append Compressed Leaf<br/>cNFT leaf = sha256(sce_hash ‖ tape ‖ beats ‖ harm ‖ outcome)
    Solana-->>Student: 5. After-action report & on-chain state finalized
```

The student's device never holds a signing key that can produce a valid attestation. The verifier does. Three properties follow:

- **Ordering is fixed.** The commit lands before the encounter, so the number of attempts started is public even when only some are revealed. Practising until you get a good run is visible as an attempt count, not hidden as a single lucky record.
- **Scores are re-derivable.** `engine_ver` and `model_id` are pinned in the leaf. Given the revealed transcript, an independent verifier re-runs the deterministic automaton at that version and must reproduce the same rubric result. A mismatch is a provable dispute, not a subjective disagreement.
- **Escalation is a parameter.** Low-stakes practice: one verifier. Institutional exam: n-of-m quorum of verifier nodes, all signatures required on the leaf.

> [!NOTE]
> **Honest limit:** this proves the *scoring* was faithful to the transcript. It does not prove the human at the keyboard was the enrolled student. Identity binding is out of scope for the protocol — it is delegated to the issuer (school SSO / proctoring), and the credential carries the issuer's identity so a relying party can judge how much that issuer's proctoring is worth.

---

## 2. End-to-End Verification Pipeline

```mermaid
flowchart TD
    subgraph OffChain ["OFF-CHAIN (Local / Node Runtime)"]
        Tape["Action Tape<br/><i>(what was ordered & when)</i>"]
        Scenario["Scenario JSON<br/><i>(pinned by sce_hash)</i>"]
        Automaton["Deterministic Automaton<br/><i>(Physiology State Machine)</i>"]
        Facts["Discrete Facts<br/><i>(ordered beats · harm · outcome)</i>"]
        Leaf["Single Leaf Hash<br/><code>sha256(sce_hash ‖ tape ‖ beats ‖ harm ‖ outcome)</code>"]

        Tape & Scenario --> Automaton
        Automaton --> Facts
        Facts --> Leaf
    end

    subgraph OnChain ["ON-CHAIN (Solana Program & Accounts)"]
        Tree["Concurrent Merkle Tree<br/><i>(Bubblegum State Compression)</i>"]
        Program["Native Vitals Program<br/><i>(Recomputes Level Predicates)</i>"]
        Progression["Progression PDA<br/><i>(program-owned record — no token)</i>"]
        Refusal{"Claim vs Arithmetic"}

        Leaf -->|Append Leaf| Tree
        Tree -->|Merkle Proofs + Leaves| Program
        Program --> Refusal
        Refusal -->|Claim <= Computed| Progression
        Refusal -->|Claim > Computed| Reject["❌ INSTRUCTION REFUSED<br/><i>(On-chain Rejection)</i>"]
    end

    style OffChain fill:#F8FAFA,stroke:#C9D6D6,stroke-width:1px
    style OnChain fill:#F2F6F6,stroke:#0A5E4B,stroke-width:2px
    style Refusal fill:#FFF6ED,stroke:#8E3D11,stroke-width:1px
    style Reject fill:#FDE8E8,stroke:#E02424,stroke-width:1px
```

---

## 3. Onchain accounts

### Case Registry (a second Solana program — designed, not built)

```mermaid
classDiagram
    class CaseAccount {
        +Pubkey author
        +[u8; 32] content_hash
        +[u8; 32] rubric_hash
        +u16 schema_version
        +u64 price_per_attempt
        +Vec~(Pubkey, u16)~ royalty_bps
        +u64 attempts
        +Status status
    }

    class CommitAccount {
        +Pubkey case
        +Pubkey student
        +[u8; 32] commit_hash
        +Pubkey verifier
        +i64 t0
    }

    class ProgressionAccount {
        +Pubkey student
        +u64 xp
        +u16 level
        +u64 proven_leaves
        +[u8; 32] last_counted
    }

    class SkillTreeAccount {
        +Pubkey student
        +u8 specialty
        +u16 distinct_cases
        +u16 hard_cases
        +u16 avg_bps
        +u32 variance_bps
        +u8 dreyfus_stage
    }

    CaseAccount --> CommitAccount : Initiates Attempt
    CommitAccount --> ProgressionAccount : Proven Leaves Counted
    ProgressionAccount --> SkillTreeAccount : Specialty Skill Growth
```

### Account Structure Details

#### Case Registry (`CaseAccount`)
- **PDA**: `["case", author_pubkey, case_slug]`
- Content stays off-chain (encrypted on author's storage or Arweave/IPFS). The chain holds the cryptographic commitment, not the clinical text.

#### Attempt Anchor (`CommitAccount` & `RevealTree`)
- **PDA**: `["commit", case_pubkey, student_pubkey, nonce]` (Closed upon reveal to reclaim rent).
- **RevealTree**: designed as a Metaplex Bubblegum concurrent Merkle tree, ~$110 per 1M compressed attempt writes. Built as a fixed-depth tree the program keeps itself, so a proof needs no indexer.

#### Progression Layer (`ProgressionAccount` & `SkillTreeAccount`)
- **PDA**: `["prog", student_pubkey]` & `["skill", student_pubkey, specialty_id]`
- **No mint**: the design named a Token-2022 `NonTransferable` mint. The built version has no token at all — the PDA above is the record, so there is nothing to transfer.
- **Self-Grading Rejection**: `claim_progress` recomputes the level predicate directly from Merkle proofs inside the Solana program runtime.

---

## 4. Payment & Micro-Royalty Flow

```mermaid
flowchart LR
    subgraph Payer ["Funding Source"]
        Student["Medical Student<br/><i>(Free in Season)</i>"]
        Sponsor["Sponsor / University Pool<br/><i>(Prepaid Fiat Invoice / USDC)</i>"]
    end

    subgraph Splitter ["Token-2022 Royalty Engine"]
        RevealIx["Reveal Instruction<br/><code>split by royalty_bps</code>"]
    end

    subgraph Beneficiaries ["Automated Micro-Distribution"]
        Author["Case Author<br/><i>(Earns per replay)</i>"]
        School["Originating Faculty<br/><i>(Institutional Share)</i>"]
        Protocol["Protocol Treasury<br/><i>(Infrastructure & Reserve)</i>"]
    end

    Student -.->|Free Access| RevealIx
    Sponsor -->|USDC / Token-2022| RevealIx
    RevealIx --> Author
    RevealIx --> School
    RevealIx --> Protocol

    style Payer fill:#F8FAFA,stroke:#C9D6D6
    style Splitter fill:#E6F1ED,stroke:#0A5E4B
    style Beneficiaries fill:#F2F6F6,stroke:#6E8084
```

Institutions do **not** need crypto knowledge. A medical school buys seats in fiat on standard invoices; the platform sponsors gasless fee-payer relays and settles author royalties onchain on their behalf.

---

## 5. Component Stack Summary

| Component | Role | Stack | Source |
|---|---|---|---|
| `embla-engine` | Encounter simulation + deterministic automaton + audit chain | Rust | Production engine |
| `vitals-program` | Case registry + commit/reveal + on-chain progression predicate | Rust / Anchor | **New onchain** |
| `vitals-progress` | Integer arithmetic twin of `xp_for`/`level_for`/`dreyfus` (no_std) | Rust | **Shared twin** |
| `vitals-cli` | Local testnet validator & verification runner | Rust | **New tool** |
| `vitals-web` | Playable browser client with fee relayer | HTML5 / Vanilla JS / Rust | **New client** |
| `Heimdall` | Local LLM dialogue gateway (dialogue never touches grade hash) | Local inference | Reused gateway |

---

## 6. Open Questions & Roadmap

- **Tree sizing / rollover:** One concurrent tree per cohort-year with canopy depth 14–17.
- **Zero-Knowledge Selective Disclosure (v2):** ZK range proofs to allow students to prove competency (`score >= 80`) without revealing sensitive encounter transcripts.
- **Verifier DePIN Market:** Open staking with `$VIGIL` bonds to decentralize verifier nodes once verification volume grows.
