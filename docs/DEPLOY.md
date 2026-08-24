# Running Vitals

Three processes, and only one of them is in the cluster.

| | where | why |
|---|---|---|
| `vitals-web` | k8s (`vitals` namespace) | stateless enough to containerise once runs persist |
| validator | host, or devnet | `solana-test-validator` is a development tool whose value is being disposable |
| Heimdall | host, launchd | needs the GPU; the cluster reaches it at `host.orb.internal` |

## Local

```
cargo run -p vitals-web
```

Binds `127.0.0.1:8474`. Not 8090 — that port is already three other things on this machine.
Without `VITALS_TOKEN` it refuses to bind anywhere but loopback, because anyone who can reach the
port can make the relay sign.

| variable | default | |
|---|---|---|
| `VITALS_WEB_BIND` | `127.0.0.1:8474` | |
| `VITALS_STATE_DIR` | `./state` | runs in progress, and the anchoring tree |
| `VITALS_SCENARIOS` | the source tree | `CARGO_MANIFEST_DIR` is baked in at build time and does not exist in a container |
| `VITALS_RPC` | `http://127.0.0.1:8899` | |
| `VITALS_PROGRAM_ID` | — | no chain without it; the app still plays |
| `VITALS_KEYPAIR` | `~/.config/solana/id.json` | the **relay**. Pays fees, holds no player key |
| `VITALS_TOKEN` | — | required to bind off loopback |
| `VITALS_CLIPS` | Embla's `cutscenes/ep1` | absent just means no video |

## Cluster

```
export VITALS_PROGRAM_ID=<deployed program>
scripts/deploy.sh
kubectl -n vitals port-forward svc/vitals-web 8474:8474
```

`scripts/deploy.sh` builds the image, creates the config, generates a bearer token once, and
mounts the relay keypair as a secret.

### One replica, and `Recreate`

Two replicas would each hold their own copy of the anchoring tree in memory and both write the
same file. The second to save erases the first one's leaves — which are the leaves its own proofs
are built from. Scaling past one means moving the tree out of the process first.

### TLS

`k8s/ingress-internal.yaml` serves `https://vitals.internal`, signed by the cluster's own CA, with
HTTP redirecting. It is applied separately from `vitals.yaml` so deploying never quietly exposes
anything.

There is **no public Ingress, and that is deliberate** — but not for the reason first written
here. Colosseum accepts a private repository with judges granted access, so submitting never
required publishing anything. What is still true is that a publicly reachable demo is a public
disclosure in its own right, and the patent question is open. Keep it internal until that is
answered. See `RISKS.md`.

## Devnet

```
scripts/deploy-devnet.sh
```

Builds for SBPF v3, deploys, and prints the environment to run against. `CLUSTER=testnet` works
too but there is little reason: testnet exists to test the validator software, devnet is where
applications live and where a judge will look.

| | |
|---|---|
| program rent | ~0.78 SOL for a 108KB program, held not spent |
| per player | ~0.011 SOL of rent for their account, claim buffer and progress record |
| paid by | the relay keypair — players never hold SOL |

Budget for headroom rather than the minimum: the relay funds every player's accounts, so 2 SOL
supports roughly a hundred of them.

**The public faucet refuses more often than it works.** `https://faucet.solana.com` with a GitHub
sign-in is far more reliable, and a free Helius or QuickNode endpoint usually brings its own.

### The program keypair is the program id

`keys/vitals_program-keypair.json` is not a build artifact. It *is* the address the program lives
at, and every anchored record points there. It used to sit in `target/`, which `cargo clean`
deletes and git ignores — one careless clean and the id changes, stranding everything anchored
under the old one. It now lives outside `target/`, is still ignored by git, and is backed up with
the relay key.

## Cloud Run

```
VITALS_GCP_PROJECT=vitals-academy VITALS_PROGRAM_ID=<from deploy-devnet.sh> scripts/deploy-cloudrun.sh
```

Two projects exist, on the same billing account as `cloud-super-hero` and `mega-care`, both with
Firestore in `asia-southeast1` — the region Cloud Run runs in, and a choice that cannot be changed
afterwards:

| project | number | for |
|---|---|---|
| `vitals-academy` | 995399340966 | the public demo — real learners' records, whatever chain they anchor to |
| `vitals-academy-dev` | 367117259093 | testing and load. Deletable data |

The split follows who the data belongs to, not which Solana cluster it points at. The cluster is
`VITALS_RPC`, an environment variable.

The unqualified name is production, as it is for every sibling project. Writing a document with
Thai text and reading it back is the check that the store code and the database agree; both pass.

The script still refuses to run without `VITALS_GCP_PROJECT` even though there is now an obvious
answer. The guard is not about the answer being unknown; it is about the wrong answer being
one keystroke away, and it refuses `cloud-super-hero*` for the same reason.

The whole product is one binary — page, API and relay together — so there is no separate frontend
to host. That follows from the relay paying fees: a static host cannot hold a signing key, and
holding one is what lets a medical student play without ever touching crypto. The usual Solana
advice of "put the frontend on Vercel" assumes the user's own wallet signs, which is exactly the
assumption this product removes.

| | where | why |
|---|---|---|
| server | Cloud Run, one instance | Same place `embla-cloud` runs. Pinned to one instance because the anchoring tree lives in memory — two copies would each keep their own and overwrite the other's leaves. |
| state | Firestore, in this product's own project | A Cloud Run container has no disk that survives a request. Setting `GOOGLE_CLOUD_PROJECT` is what selects this backend; without it the store writes files. The project is not shared with `embla-cloud` — see below. |
| relay key | Secret Manager, mounted at `/relay/id.json` | Read as a file, not an environment variable. |
| the model | wherever the GPU is | Heimdall cannot run on Cloud Run. Point `HEIMDALL_API_URL` at the machine that has one — and if that ever becomes a hosted model instead, the deck's claim that a *local* model plays the patient stops being true and has to change with it. |

### Why a separate project

`embla-cloud` keeps identified people in its Firestore — a LINE user id, a display name and a
profile each — gathered under a consent version it records alongside them. A Cloud Run service
takes its Firestore credential from the metadata server, and that credential is scoped to the
project. A service deployed next to those documents can read them, by default, without ever
intending to. Those people agreed to Embla; they did not agree to this. Per-database IAM
conditions could carve that back, but then isolation is something maintained rather than
something true, and it stays correct only as long as everyone remembers it. A separate project
makes it the default. `deploy-cloudrun.sh` therefore has no default project and refuses
`cloud-super-hero*` outright.

The same boundary answers a few other things at once: what this product costs is legible on its
own bill, a Colosseum reviewer can be granted the project rather than a slice of a shared one,
and if the experiment ends, deleting the project ends it.

### Two kinds of record

The store holds both, and they look identical — JSON behind the same six methods:

| kind | losing it costs | expires |
|---|---|---|
| `sess` | one learner, one run in progress | yes, by age |
| `tree` | **every proof this server can issue** | never |

The Merkle root is anchored on chain and survives anything. The path from a leaf to that root is
not on chain — it is rebuilt from the leaf list here. Expire the list and the anchor remains,
provably meaningless. `sweep` refuses durable kinds rather than trusting its caller to pass the
right string, and a kind nobody has classified is treated as durable: forgetting to classify
something should cost disk, not data.

Rankings will want a third kind — derived, rebuildable, queryable — and a store that answers
"what percentile is this" cheaply. That is not built. There is no one to rank yet, and a
leaderboard schema chosen before the first cohort exists is a guess about what a cohort is.

## Live on devnet

```
535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG
https://explorer.solana.com/address/535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG?cluster=devnet
```

Deployed 24 August, verified byte-for-byte against this tree, upgrade authority left on the
deployer's key — stated explicitly rather than defaulted, which is what saying "this cluster is
disposable" out loud looks like. The four chain tests run against it, not only against a local
validator, which is the first time the whole path has been exercised on a public cluster.

They fund their relays by transfer rather than airdrop. A local validator will airdrop all day;
devnet's faucet refuses, and the tests then failed inside the anchoring assertions — reporting a
broken chain path when what was empty was a wallet. One funding path that works everywhere beats a
faster one that lies on three clusters out of four.

Note the running cost of doing this: each test server takes 0.1 SOL and the accounts it creates
keep their rent. Cheap on a local validator, real on devnet, which is why CI uses the former.

## Who may replace the program, and how anyone checks it

Two things decide whether a deployed program can be trusted, and neither is the code itself.

**The upgrade authority.** A deploy leaves it on whichever key ran the command — on a laptop, a hot
key that also pays for everything else. That is one stolen laptop away from the program being
replaced, and it devalues any audit of it: a report certifies bytecode the key holder can swap out
the next day, which auditors say in as many words. `deploy-devnet.sh` therefore has no default and
refuses to run without `UPGRADE_AUTHORITY` — a Squads multisig address, the deployer's own key to
say out loud that this cluster is disposable, or `none` to burn it and make the program permanently
immutable. That last one is right for a final mainnet deploy and unrecoverable anywhere else.

The authority key needs a balance of its own. Changing authority and upgrading are transactions it
pays for, so a freshly created multisig holding no SOL can do neither — confirmed the direct way,
by transferring authority to an unfunded key on a local validator and being unable to move it back.

**Reproducing the deployed bytecode.** The repository asks to be taken as "a protocol with one
reference client", and publishing source is only half of that: without a way to check what is
actually running against it, open source is a claim about a repository rather than about the thing
answering transactions.

```
VITALS_PROGRAM_ID=<id> scripts/verify-deploy.sh
```

It runs automatically as part of `deploy-devnet.sh`. What it proves is exact: the bytecode on chain
is byte-for-byte the artefact this machine builds, allowing for the zero padding the account keeps
so the program can grow. What it does not prove is that *your* machine would build the same
artefact — that needs a pinned toolchain in a container, which is what `solana-verify` is for and
what should replace this script as soon as it can be installed. Until then it still catches the
failure that actually happens: a deploy that silently lagged behind the source.

The script is tested the only way a verifier can meaningfully be tested — by being shown something
that should fail. A single flipped byte and a truncated build are both rejected, with the offset
reported. A verifier that always says "match" is worse than no verifier.

## What is on chain, and what is not

The player's key is generated by WebCrypto in their browser and stays there. The server prepares a
transaction, the browser signs it, the server pays and submits. No wallet to install, no SOL to
buy, and no key held by a server.

Anchored: a leaf hash, an outcome, a score. Never a name, never a transcript, never anything the
patient said — that comes from a language model and is nowhere near the hash.
