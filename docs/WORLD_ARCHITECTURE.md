# Vitals World: architecture and integrations

> The ward at [world.vitals.academy](https://world.vitals.academy), as built on the `cwf/*` branches
> (Crypto World's Fair 2026). [`ARCHITECTURE.md`](ARCHITECTURE.md) is the earlier protocol design
> for the single-player game. This page covers the ward: what runs, where, and what talks to what.
> Devnet only. No token. No money moves.

## One picture

```
 browser (any visitor, no signup)                     Solana devnet
 ├─ globe + board        GET /api/ward ─────┐         ├─ vitals-program (native solana-program)
 ├─ bedside: ask/examine/order               │         │   patient accounts · heads · leases
 │    POST /api/step · /api/say              │         │   commitments · leaves (incremental Merkle)
 ├─ its own devnet key (made in the page)    ▼         │
 │    signs every transaction  ──►  vitals-web on Cloud Run ──► relay pays the fee ──┘
 └─ receipt /shift/<hash>             │  (tiny_http, Rust)
                                      ├─ ward pass, once a minute (Cloud Scheduler → POST /api/ward/tick)
                                      ├─ store: Firestore (tapes, packs, clocks)
                                      ├─ Gemini on Vertex AI: the patient's voice (/api/say)
                                      └─ the door: token-guarded routes for the case factory
                                                    ▲
 vitals-factory (scheduled job) ──── packs: case + person + faces ──┘
   need weights (World Bank people per doctor) · spread rules · Vertex image + Gemini age/realism gate
```

## The crates

| crate | what it is |
|---|---|
| `vitals-program` | The Solana program, written against `solana-program` with no framework. Patient accounts, the shift lease, commitments, and leaves appended to an incremental Merkle tree. A leaf must extend the patient's current head; a stale head is refused. |
| `vitals-sce` | The physiology engine: a scenario is a sha256-pinned file; vitals move on a clock, and interventions are recognised from what was typed or pressed. |
| `vitals-replay` | Rebuilds a shift from its tape and computes the leaf. Anyone can run it against `/api/tape/<hash>` and must get the same hash. |
| `vitals-osce` | The rubric: deterministic marks from the tape (no model scores anything). |
| `vitals-progress` | Shared record types and the Merkle tree used by the program and the server. |
| `vitals-web` | The server and every page. One writing loop handles anything that changes state, and a reader pool of four threads answers pure reads (receipts, the board), so a receipt is not stuck behind a pass. |
| `vitals-factory` | Builds the patients the ward admits: who, from where, with which case, and their portraits at each stage. |

## A shift, end to end

1. **Take.** The visitor's browser has made its own devnet key. Taking a bed places a short lease on chain, so two strangers cannot hold one patient.
2. **Treat.** Every question, examination and order is a step on the tape:
   - `/api/step` resolves an order in the tab it was typed in;
   - `/api/say` sends a question to Gemini, which answers from the patient's case file;
   - the engine moves the vitals on the clock.
3. **Hand over.** A shift is four transactions in all: the player's account, the head, the declaration and the anchor. **The browser signs every one.** The relay pays for all four and cannot produce any of the signatures, so the record belongs to the player. The leaf commits to the scenario hash, the tape hash, the rubric hash, the outcome, harm and both scores.
4. **Receipt.** `/shift/<hash>` is rebuilt from the bytes that were played, never from a summary. If the bytes are not held, the receipt says so instead of guessing. `/api/ward/bytes` walks every shift on the list.
5. **Nobody comes.** A patient whose clock runs out dies of being left. The ward closes her with its own key, so that death is on chain too.

## The ward pass

Once a minute Cloud Scheduler calls `POST /api/ward/tick`. The route is guarded by the door token, and the pass runs inside that request: Cloud Run throttles CPU outside requests.
- **What a pass does:** rebuilds every patient from the chain; repairs what changed; closes the dead; frees beds; admits from the queue on the arrival clock; publishes each patient's time left.
- **If the scheduler goes quiet:** if no scheduled tick has arrived for 150 s, an in-process ticker runs the pass instead.
- **Logging:** a pass slower than 10 s logs a line with its stage timings.

## Integrations

| what | how it is used | where the secret lives |
|---|---|---|
| Solana devnet | program, leases, leaves; the relay pays fees | relay and ward keys outside the repo; the RPC endpoint is a dedicated provider, its URL and key in Secret Manager |
| Gemini via Vertex AI | the patient's voice only; rate-limited per caller | the Cloud Run service account; nothing in the repo |
| Vertex AI (images) + Gemini judge | the factory's portraits per stage, rejected if they look the wrong age or unreal | the factory host's Google credentials |
| Firestore | tapes, packs, the clocks the board shows | the metadata server's token on Cloud Run |
| Cloud Scheduler | the once-a-minute tick | the door token, read from Secret Manager |
| World Bank WDI SH.MED.PHYS.ZS | the globe's colours and the factory's need weights | a vendored, dated file |

## Read it yourself (public GETs)

| route | what it returns |
|---|---|
| `/api/ward` | census, patients, policy, with how each number is derived |
| `/api/ward/patient/<id>` | one patient's shifts and signers |
| `/api/ward/cases` | the case catalogue, each case marked provisional until clinically reviewed |
| `/api/usage` | the funnel, counted per event, never per person |
| `/api/fuel` | the relay's balance and runway, in lamports |
| `/api/shift/<hash>` · `/api/tape/<hash>` | a receipt, and the bytes behind it |

## Hardening

- Security headers and a full Content-Security-Policy on every response.
- Door routes return 401 without the token. Secrets are compared in constant time, are never named in a reply, and never appear in process arguments.
- Per-caller rate limits on the model route.
- Checked with cargo-audit, gitleaks (full history), semgrep, OWASP ZAP (baseline and a full active scan) and nuclei, last on 3–4 Oct 2026.
- An external watcher alerts on slow passes, refused closures, errors and the relay's runway.
