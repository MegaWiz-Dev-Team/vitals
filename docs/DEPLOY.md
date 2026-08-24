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
VITALS_PROGRAM_ID=<from deploy-devnet.sh> scripts/deploy-cloudrun.sh
```

The whole product is one binary — page, API and relay together — so there is no separate frontend
to host. That follows from the relay paying fees: a static host cannot hold a signing key, and
holding one is what lets a medical student play without ever touching crypto. The usual Solana
advice of "put the frontend on Vercel" assumes the user's own wallet signs, which is exactly the
assumption this product removes.

| | where | why |
|---|---|---|
| server | Cloud Run, one instance | Same place `embla-cloud` runs. Pinned to one instance because the anchoring tree lives in memory — two copies would each keep their own and overwrite the other's leaves. |
| state | Firestore | A Cloud Run container has no disk that survives a request. Setting `GOOGLE_CLOUD_PROJECT` is what selects this backend; without it the store writes files. |
| relay key | Secret Manager, mounted at `/relay/id.json` | Read as a file, not an environment variable. |
| the model | wherever the GPU is | Heimdall cannot run on Cloud Run. Point `HEIMDALL_API_URL` at the machine that has one — and if that ever becomes a hosted model instead, the deck's claim that a *local* model plays the patient stops being true and has to change with it. |

## What is on chain, and what is not

The player's key is generated by WebCrypto in their browser and stays there. The server prepares a
transaction, the browser signs it, the server pays and submits. No wallet to install, no SOL to
buy, and no key held by a server.

Anchored: a leaf hash, an outcome, a score. Never a name, never a transcript, never anything the
patient said — that comes from a language model and is nowhere near the hash.
