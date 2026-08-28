# Verify it yourself

Every claim this project makes about a player — the level, the stars, the number of runs they
started — is derived from records on Solana devnet. Nothing below asks you to trust a recording, a
screenshot, or us. You clone this repository, build one binary, and recompute the answer from the
chain on your own machine.

The tool is a **keyless read**. It never asks for a key, never signs anything, and cannot change
what it is reading.

Last run end to end on **2026-08-28**, against program
`535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG` on devnet. Every output on this page is pasted from
that run, unedited.

---

## 1. Build it

You need a Rust toolchain (CI pins **1.93.1**), `curl`, and `shasum`. No key, no wallet, no
account, no API token.

```bash
git clone https://github.com/MegaWiz-Dev-Team/vitals.git
cd vitals
cargo build -p vitals-web --bin verify_player --release
```

## 2. Run it

With no arguments it asks the live server which Merkle tree is current, asks the chain who has
records on it, and verifies the fullest of them.

```bash
./target/release/verify_player
```

Real output:

```
no player given — asking the chain who has proven anything on tree #488905120

players with a claim buffer on this tree:
  3zi13rwSo1HDBYJxLKGzWQN3J2ebidC5QCfMhBwAxKTT  1 proven attempt
  ACYmqyxqS9SDLYMRNfxJwKAAV2nAC9UcmuhbGqndHVjj  1 proven attempt
  FtUggk4Bfoah25VQeyjTFZ5hoqeStqXPTLGStLf5Kihu  1 proven attempt
  H5dQxmKbAABfm7aaffXLjPPc16Q9ha6kUQNb2DXLDNGX  1 proven attempt

verifying the fullest of them — pass a key as the first argument to choose another.

verifying 3zi13rwSo1HDBYJxLKGzWQN3J2ebidC5QCfMhBwAxKTT · tree #488905120 · program 535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG · https://api.devnet.solana.com
tree id from: live /api/chain

started (commitments ever made) : 1
stored Progress (last claim)    : none claimed yet

live ClaimAccount: 1 proven attempt
  #0 case 4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d
     outcome 100/100 · det 36/40 · difficulty 0 · exam true
     leaf 25473e2f26ae579424e888b009e115f6566a6935bdf9bbb0d817202aa329f87e

fetch the scenario any of these was computed over, and check its hash yourself:
  curl -s https://devnet.vitals.academy/api/sce/4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d | shasum -a 256   # → 4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d

stars (distinct exam cases cleared at det >= 70%): 1

summary: distinct 1 · hard 0 · avg 10000bps · computed = Advanced beginner
  claim Competent  : REJECTED (claimed Competent, computed Advanced beginner)
  claim Proficient : REJECTED (claimed Proficient, computed Advanced beginner)
```

To check a specific player, or a specific tree:

```bash
./target/release/verify_player <PLAYER_PUBKEY>
./target/release/verify_player <PLAYER_PUBKEY> <TREE_ID>
./target/release/verify_player --help
```

Exit codes: `0` verified · `1` nothing to verify · `2` the records predate this build's layout
(see §6).

---

## 3. What each line means

**`tree id from: live /api/chain`** — where the tree number came from. The order is: command-line
argument, then `$VITALS_TREE_ID`, then the live server, then a compiled-in fallback
(`488905120`, only used when the server is unreachable, and it says so when it falls back). A
number whose provenance is unstated is not evidence, so the tool states it.

**`started (commitments ever made) : 1`** — read from the player's commitment account. Before a
run is played, the player signs a declaration and the chain stamps the slot. This counter is
monotonic and cannot be decremented, so it counts **every** run this player ever started,
including the ones they abandoned or failed. It is the honesty counter: a player showing you five
good results whose `started` reads 90 has told you something by that gap alone.

**`stored Progress (last claim) : ...`** — the snapshot written by the last claim that the
*program* accepted. `none claimed yet` means the player has proven attempts but has not asked the
chain to grant a level from them.

**`live ClaimAccount: N proven attempts`** — the claim buffer for this player on this tree. Each
attempt was anchored as a Merkle leaf and then proven against the tree root, so the program has
already checked that the leaf is in the tree at the index claimed. The tool then lists:

| field | meaning |
| --- | --- |
| `case` | the **sha256 of the scenario file** the run was played against. §5 fetches those exact bytes and re-hashes them. Edit a scenario and its hash changes, so old leaves stop proving anything about the new version — which is the correct behaviour, not a bug. |
| `outcome 100/100` | the outcome score, out of its maximum. This is what the level is computed from. |
| `det 36/40` | the deterministic rubric score — replayed from the tape by the pinned engine, never accepted from a browser. This is what a star is measured against. Zero for practice runs; only an exam run is marked. |
| `difficulty` | `0` student · `1` intern · `2` resident |
| `exam true` | whether this was sat as an exam. Only exam runs earn stars. |
| `leaf` | the 32-byte Merkle leaf this run was anchored as. |

**`stars (distinct exam cases cleared at det >= 70%)`** — distinct exam cases where the
deterministic rubric cleared the pass bar. Distinct, so replaying one station cannot buy breadth.

**`summary: distinct 1 · hard 0 · avg 10000bps · computed = Advanced beginner`** — the output of
the program's own `summarize`. `avg` is in basis points (10000 = 100%). `computed` is the Dreyfus
level the attempts actually support.

**`claim Competent : REJECTED (claimed Competent, computed Advanced beginner)`** — **this is the
system working, not failing.** The tool asks the program's own `adjudicate` what would happen if
this player claimed a level. One proven attempt does not support Competent, so the claim is
refused. A level here is computed from records, never asserted by whoever wants it. If the
adjudicator only ever said GRANTED, none of this would mean anything.

> The tool runs the program's own `summarize` / `dreyfus` / `adjudicate` — not a re-implementation
> — so the number it prints is the number a real claim transaction would compute at that instant.

---

## 4. Finding a tree id and a player key

**Tree id.** The demo server publishes the tree it is anchoring to right now:

```bash
curl -s https://devnet.vitals.academy/api/chain | tr ',' '\n' | grep tree_id
```
```
"tree_id":488905120
```

The tree rotates. That is why `verify_player` reads this endpoint instead of remembering a number
— an earlier build hardcoded one, it went stale, and the tool stopped working for everybody.

**A player key.** Three ways, easiest first:

1. **Let the tool find one.** Run it with no arguments. It enumerates the program's accounts,
   keeps the claim buffers whose address re-derives to the current tree, and lists their players.
   Those keys are already public — they are account addresses on a public chain, and no identity
   is attached to any of them.
2. **Use your own.** Play a run at <https://devnet.vitals.academy>, then anchor it. Your browser
   generates the keypair and keeps it in `localStorage` under `vitals.key`; the public half is the
   `player=` parameter on every `/api/…` request the page makes, visible in your browser's Network
   tab. The private half never leaves your browser — the server has no copy and cannot sign for
   you, which is what makes the record yours rather than ours.
3. **Read it off the chain directly**, if you would rather not trust our enumeration:

```bash
curl -s https://api.devnet.solana.com -H 'Content-Type: application/json' -d '{
  "jsonrpc":"2.0","id":1,"method":"getProgramAccounts",
  "params":["535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG",{"encoding":"base64"}]}'
```

Claim buffers are the 1285-byte accounts; the first 32 bytes are the player.

---

## 5. Check the scenario bytes yourself

The `case` hash on each attempt is the sha256 of the scenario file the run was computed over. That
hash resolves to the exact bytes, content-addressed, over a public and ungated endpoint:

```bash
curl -s https://devnet.vitals.academy/api/sce/4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d -o case.json
shasum -a 256 case.json
wc -c case.json
```

Real output:

```
4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d  case.json
    6937 case.json
```

The hash of the bytes you were served equals the hash written into the leaf on chain. The server
re-hashes what it reads before sending it and refuses anything that does not match, so it cannot
serve you the wrong file under the right name. It can only serve the right bytes or nothing.

Old versions count too: the archive under `conformance/sce-archive/` is committed to this
repository and ships in the image, so a leaf anchored against a scenario that has since been
edited still resolves. `conformance/sce-archive/INDEX.json` maps each hash to the path it came
from. If you would rather not fetch at all, hash the file in your own clone:

```bash
shasum -a 256 demo/stations/osce-a.sce.json
```

`GET /api/sce/<hash>` answers `404` with the same message for "not a hash", "no such hash" and
"the file under that name is not that file" — the caller's next move is identical in all three,
and a distinguishing error would tell a stranger which files exist.

---

## 6. When it does not print a result

The tool does not panic at you. Two answers are expected and both explain themselves.

**Records written by an older layout.** `ProvenAttempt` gained the deterministic rubric fields
(`det_score`, `det_max`), so a claim buffer written before that change cannot be read by a current
build. Asking an old tree gives you this, and exit code `2`:

```
verifying 9iTTpzAHhVqsWSJU37rZuNU92sRSeBjjJ1LMLHRCPSFv · tree #487877348 · program 535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG · https://api.devnet.solana.com
tree id from: command line

started (commitments ever made) : 8
stored Progress (last claim)    : level 2 · 6 attempts · 3 distinct · xp 89

ClaimAccount at GayZQNQDD3ugHfFjbBtakKde3sEttnxpQwwvhsrgdqsR exists, but this build cannot read it.
  on chain: 1221 bytes · this build expects 1285 (37 header + capacity × 78)
  borsh   : Invalid bool representation: 118

The records on tree #487877348 were written by an earlier ProvenAttempt layout — before
the deterministic rubric fields (det_score, det_max) were added — so every field after
them is read at the wrong offset. Nothing is wrong with the chain: those bytes are
exactly what was anchored. They predate the struct that would read them.
```

Those bytes are exactly what was anchored; the chain is not lying. Ask a current tree instead.

**Nothing proven on this tree.** The player exists but has no claim buffer on the tree you asked
about — usually because they played on an earlier one. Exit code `1`:

```
no ClaimAccount at GNkxkveLHe6KQ8t57VdLZiU3jwVsJhGysws7CsYSopmS — nothing proven on this tree
This player may have played on an earlier tree. `verify_player` with no arguments
lists who has records on tree #488905120.
```

---

## 7. What you are trusting, and what you are not

**Not trusting us for:** the records, the level, the star count, the run counter. All of it is on
devnet, read by a binary you compiled from source you can read, and recomputed by the same
functions the on-chain program runs. Point it at any RPC you like with `VITALS_RPC=…` — including
your own node — and the answer must not change.

**Trusting us for:** that `devnet.vitals.academy` serves the scenario bytes at all. It cannot serve
you *wrong* bytes — you hash them yourself and compare against the leaf — but it could serve
nothing. The archive is committed to this repository as a second copy for exactly that reason.

**Not proven by any of this:** `judged_score`, the rubric's judged half, is a self-asserted number
in the record. No attestation mechanism exists for it yet, nothing on chain consumes it, and it
cannot buy progression. `verify_player` does not print it and no claim in this project rests on it.

**What a level means.** It says these runs happened, against these exact scenario files, declared
before they were played, and that the program recomputed the level rather than accepting one. It
does not say the player is a good doctor. It is evidence about practice, not a licence.

---

## 8. Environment overrides

| variable | default |
| --- | --- |
| `VITALS_RPC` | `https://api.devnet.solana.com` |
| `VITALS_PROGRAM_ID` | `535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG` |
| `VITALS_TREE_ID` | read from `/api/chain` |
| `VITALS_CHAIN_API` | `https://devnet.vitals.academy/api/chain` |

`crates/vitals-web/tests/verify_tool.rs` keeps this page honest: it fails the workspace build if
the tool's offline fallback tree stops being the number written above, if this file goes missing,
or if the live lookup is ever replaced by a hardcoded default again. Its network half —

```bash
cargo test -p vitals-web --test verify_tool -- --ignored
```

— checks that fallback against the tree the server is anchoring to right now.
