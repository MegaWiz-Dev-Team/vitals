//! Independent on-chain verification of a player's level — the reproducible half of the thesis.
//!
//! `VERIFICATION.md`, at the root of this repository, is the walkthrough this binary belongs to:
//! clone the public repo and run this, and you re-derive a player's level yourself, from devnet,
//! without trusting whoever recorded the video. It is a **keyless read** — no signer, no key
//! material — and it runs the program's own `summarize`/`dreyfus`/`adjudicate`, not a
//! re-implementation, so the number it prints is the number a claim would compute this instant.
//!
//! ## Why nothing here is pinned to one afternoon
//!
//! The previous build hardcoded one tree id and one player. Both went stale inside a week: the
//! demo server rotates its Merkle tree, and `ProvenAttempt` gained `det_score`/`det_max`, so the
//! default tree came to hold records written by an older layout. An independent reviewer
//! following the landing page got a borsh panic — `Invalid bool representation: 118`, which is
//! the exam_mode byte read four bytes off — and pointing at the current tree got them
//! `nothing proven on this tree`, because the hardcoded player had never played on it. There was
//! no argument that made this tool print a result.
//!
//! A verification tool that dies on its own defaults argues against the thing it exists to prove.
//! So the defaults are now *looked up*, not remembered:
//!
//! * the tree comes from the live server's `/api/chain`, which is the same number the server
//!   anchors against — it cannot drift from reality, because it *is* reality;
//! * the player, when none is given, is discovered from the chain: every claim buffer whose PDA
//!   matches this tree, listed, and the fullest one verified;
//! * [`TREE_ID_FALLBACK`] is only for a reviewer behind a firewall, and `tests/verify_tool.rs`
//!   fails the build if it stops matching the number `VERIFICATION.md` documents.
//!
//! ```text
//! verify_player                        # discover the tree and the players on it, verify one
//! verify_player <PLAYER>               # that player, on the current tree
//! verify_player <PLAYER> <TREE_ID>     # that player, on that tree
//! ```
//!
//! Overridable by environment for a local validator: `VITALS_RPC`, `VITALS_PROGRAM_ID`,
//! `VITALS_TREE_ID`, `VITALS_CHAIN_API`.
use borsh::BorshDeserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use vitals_program::{
    commitment_pda, ClaimAccount, Commitment, Progress, CLAIM_LEN, SEED_CLAIM, SEED_PROGRESS,
};
use vitals_progress::{adjudicate, summarize, Attempt, Difficulty, Dreyfus, Verdict};

/// The deployed program on devnet.
const PROGRAM: &str = "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG";
const RPC: &str = "https://api.devnet.solana.com";
/// Where the demo server publishes the tree it is anchoring to right now.
const CHAIN_API: &str = "https://devnet.vitals.academy/api/chain";
/// The tree that was live when this line was last written — used **only** when `/api/chain` is
/// unreachable, and reported as a guess when it is.
///
/// This constant is the one piece of this tool that can rot, so it is pinned twice: it must
/// appear in `VERIFICATION.md`, which `tests/verify_tool.rs` checks offline on every `cargo test`,
/// and an `--ignored` test in the same file compares it against the live server.
const TREE_ID_FALLBACK: u64 = 488_905_120;
/// Where the committed scenario archive sits in a clone.
///
/// Borrowed from the server's own constant rather than written out again, so the command this tool
/// tells a stranger to run cannot drift from the directory the server reads and the Dockerfile
/// ships. See [`print_scenario_check`].
const SCE_ARCHIVE: &str = vitals_web::archive::DIR;

/// Verified, and the numbers printed.
const EXIT_OK: i32 = 0;
/// Nothing to verify, or the caller asked for something that is not there. Not a crash.
const EXIT_UNVERIFIED: i32 = 1;
/// The records exist but were written by a different `ProvenAttempt` layout than this build.
const EXIT_STALE_LAYOUT: i32 = 2;

const USAGE: &str = "\
verify_player — re-derive a player's level from devnet, keyless.

  verify_player                       discover the current tree and the players on it
  verify_player <PLAYER>              verify that player on the current tree
  verify_player <PLAYER> <TREE_ID>    verify that player on that tree
  verify_player --help

The tree id is taken from the command line, then $VITALS_TREE_ID, then the live server at
/api/chain, and only then from a compiled-in fallback. See VERIFICATION.md.

env: VITALS_RPC · VITALS_PROGRAM_ID · VITALS_TREE_ID · VITALS_CHAIN_API
exit: 0 verified · 1 nothing to verify · 2 records predate this build's layout
";

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// The tree the demo server is anchoring to at this second.
///
/// Read from the server rather than remembered, because the server is the only thing that knows.
fn live_tree_id(api: &str) -> Option<u64> {
    let body: serde_json::Value = ureq::get(api)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    body.get("tree_id")?.as_u64()
}

/// Where a tree id came from, so the header can say so. A number whose provenance is unstated is
/// the defect this tool shipped with.
enum TreeSource {
    Argument,
    Env,
    Live,
    Fallback,
}

impl TreeSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Argument => "command line",
            Self::Env => "$VITALS_TREE_ID",
            Self::Live => "live /api/chain",
            Self::Fallback => "compiled-in fallback — the server was unreachable, so this may be stale",
        }
    }
}

fn resolve_tree(arg: Option<&String>, api: &str) -> (u64, TreeSource) {
    if let Some(s) = arg {
        match s.parse() {
            Ok(n) => return (n, TreeSource::Argument),
            Err(_) => {
                eprintln!("'{s}' is not a tree id — it must be a whole number. See --help.");
                std::process::exit(EXIT_UNVERIFIED);
            }
        }
    }
    if let Ok(s) = std::env::var("VITALS_TREE_ID") {
        match s.parse() {
            Ok(n) => return (n, TreeSource::Env),
            Err(_) => {
                eprintln!("$VITALS_TREE_ID is '{s}', which is not a whole number.");
                std::process::exit(EXIT_UNVERIFIED);
            }
        }
    }
    match live_tree_id(api) {
        Some(n) => (n, TreeSource::Live),
        None => (TREE_ID_FALLBACK, TreeSource::Fallback),
    }
}

fn claim_pda(program: &Pubkey, player: &Pubkey, tree_id: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[SEED_CLAIM, player.as_ref(), &tree_id.to_le_bytes()],
        program,
    )
    .0
}

/// Every player holding a claim buffer on this tree, fullest first.
///
/// This is the answer to "where do I get a player key" for someone who has never played here: the
/// chain already lists them. A claim account names its player in its first 32 bytes, and the
/// account address commits to the tree, so re-deriving the PDA and comparing is what separates
/// *this* tree's buffers from every other tree's.
fn players_on_tree(rpc: &RpcClient, program: &Pubkey, tree_id: u64) -> Vec<(Pubkey, usize)> {
    let Ok(accounts) = rpc.get_program_accounts(program) else {
        return Vec::new();
    };
    let mut found: Vec<(Pubkey, usize)> = accounts
        .into_iter()
        .filter(|(_, a)| a.data.len() == CLAIM_LEN)
        .filter_map(|(key, a)| {
            let claim = ClaimAccount::deserialize(&mut &a.data[..]).ok()?;
            let player = Pubkey::new_from_array(claim.player);
            (claim_pda(program, &player, tree_id) == key).then_some((player, claim.attempts.len()))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    found
}

/// What to print when the bytes on chain do not fit this build's structs.
///
/// The old build called `.expect()` here, so the first independent reviewer to run it got a borsh
/// panic and a line number. The bytes are not corrupt and the chain is not lying — they are
/// exactly what was anchored, by a program whose `ProvenAttempt` had two fewer fields. The only
/// thing the reader needs is which tree to ask instead, and how to find it.
fn explain_stale_layout(pda: &Pubkey, tree_id: u64, len: usize, err: &std::io::Error) -> ! {
    let attempt_size = 32 + 32 + 4 + 4 + 2 + 2 + 1 + 1;
    let header = 32 + 1 + 4;
    println!("\nClaimAccount at {pda} exists, but this build cannot read it.");
    println!(
        "  on chain: {len} bytes · this build expects {CLAIM_LEN} ({header} header + capacity × {attempt_size})"
    );
    println!("  borsh   : {err}");
    println!(
        "\nThe records on tree #{tree_id} were written by an earlier ProvenAttempt layout — before\n\
         the deterministic rubric fields (det_score, det_max) were added — so every field after\n\
         them is read at the wrong offset. Nothing is wrong with the chain: those bytes are\n\
         exactly what was anchored. They predate the struct that would read them.\n\
         \n\
         Ask a tree written by the current layout instead. The server publishes which one that is:\n\
         \n\
         \x20   curl -s {CHAIN_API} | tr ',' '\\n' | grep tree_id\n\
         \x20   verify_player <PLAYER> <TREE_ID>\n\
         \n\
         Or run verify_player with no arguments and it will find the current tree, and the players\n\
         on it, by itself."
    );
    std::process::exit(EXIT_STALE_LAYOUT);
}

/// How to get the scenario bytes a leaf names, printed as a command the reader can actually run.
///
/// This block used to print
///
/// ```text
/// curl -s https://devnet.vitals.academy/api/sce/<case> | shasum -a 256   # → <case>
/// ```
///
/// and that command is now wrong for every hash this tool is likely to print. `/api/sce` serves
/// **retired** scenarios only: a file that can still be sat is the mark sheet — every matcher
/// keyword, every `(HARM)`, the thresholds that decide the outcome, `_note` fields naming the
/// diagnosis — and handing it to a candidate mid-run defeats every seal on the rest of the API.
/// So the endpoint answers `404 {"error":"that scenario is in active use"}` while a station is on
/// the shelf, and today that is every case in the season. A stranger who followed the old line saw
/// a 404 body whose sha256 is obviously not the hash we told them to expect, and concluded the
/// proof was theatre. Printing nothing would have been better; printing this is better still.
///
/// The replacement does not need a server at all. [`SCE_ARCHIVE`] is committed to this repository,
/// append-only, and named by digest, so the file a leaf points at is already on the disk of anyone
/// who cloned the repo to build this binary — and hashing your own copy is the stronger check of
/// the two, because a server can serve you bytes it serves nobody else and a public git history is
/// far harder to keep two versions of. `VERIFICATION.md` §5 leads with the same route, and
/// `tests/verify_tool.rs` fails the build if these two stop agreeing.
fn print_scenario_check(cases: &[[u8; 32]]) {
    // Distinct, in the order they were anchored: the check is per scenario file, not per attempt,
    // so sitting one station three times must not print the same command three times.
    let mut distinct: Vec<String> = Vec::new();
    for c in cases {
        let h = hex32(c);
        if !distinct.contains(&h) {
            distinct.push(h);
        }
    }
    let Some(first) = distinct.first() else { return };

    println!(
        "\ncheck the scenario bytes those runs were computed over — from your own clone, with no\n\
         server in the loop. Run this from the root of the repository you built this binary in:\n"
    );
    for h in &distinct {
        // The expected digest goes on the next line, not in a trailing comment, because the
        // comparison is the whole point: what `shasum` prints must be these 64 characters, which
        // are also in the file name above and on the `case` line further up.
        println!("  shasum -a 256 {SCE_ARCHIVE}/{h}.json");
        println!("     must print → {h}");
    }
    println!(
        "\nsha256sum instead of shasum -a 256 on Linux. Every scenario version an anchored run was\n\
         played against is committed under {SCE_ARCHIVE}/, named by its own digest and never\n\
         deleted, so a leaf written before an edit still resolves to the bytes it was computed\n\
         over. INDEX.json maps each hash to the station file it was archived from, if you would\n\
         rather hash that copy:\n\
         \n\
         \x20   grep -A1 {first} {SCE_ARCHIVE}/INDEX.json\n\
         \n\
         GET /api/sce/<hash> serves the same bytes over HTTP, but only once a case is retired:\n\
         while a station can still be sat its scenario file is the mark sheet — every matcher,\n\
         every harm, every threshold — so the server answers 404 'that scenario is in active use'\n\
         rather than hand a candidate the answers mid-exam, and at the time of writing that is\n\
         every case in the season. The clone above is the check that works either way, which is\n\
         why it is the one printed. See VERIFICATION.md §5."
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }

    let api = env_or("VITALS_CHAIN_API", CHAIN_API);
    let rpc_url = env_or("VITALS_RPC", RPC);
    let program = match Pubkey::from_str(&env_or("VITALS_PROGRAM_ID", PROGRAM)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("$VITALS_PROGRAM_ID is not a pubkey: {e}");
            std::process::exit(EXIT_UNVERIFIED);
        }
    };
    let (tree_id, source) = resolve_tree(args.get(1), &api);
    let rpc = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());

    // Whose run to check. Given, or discovered — never a name compiled in months ago that may
    // have no records on the tree that is live today.
    let id = match args.first().map(|p| Pubkey::from_str(p)) {
        Some(Ok(k)) => k,
        Some(Err(e)) => {
            eprintln!("'{}' is not a Solana public key: {e}", args[0]);
            eprintln!("Run with no arguments to list the players on the current tree.");
            std::process::exit(EXIT_UNVERIFIED);
        }
        None => {
            println!("no player given — asking the chain who has proven anything on tree #{tree_id}\n");
            let found = players_on_tree(&rpc, &program, tree_id);
            if found.is_empty() {
                println!(
                    "No claim buffer on tree #{tree_id} yet, or this RPC declines to enumerate a\n\
                     program's accounts. Play a run at https://devnet.vitals.academy, anchor it,\n\
                     and pass your own key:\n\
                     \n\
                     \x20   verify_player <PLAYER> {tree_id}\n\
                     \n\
                     VERIFICATION.md says where the browser keeps that key."
                );
                std::process::exit(EXIT_UNVERIFIED);
            }
            println!("players with a claim buffer on this tree:");
            for (k, n) in &found {
                println!("  {k}  {n} proven attempt{}", if *n == 1 { "" } else { "s" });
            }
            println!("\nverifying the fullest of them — pass a key as the first argument to choose another.\n");
            found[0].0
        }
    };

    println!("verifying {id} · tree #{tree_id} · program {program} · {rpc_url}");
    println!("tree id from: {}\n", source.as_str());

    // started — the honesty counter the commit-reveal design exists to make undeniable.
    let (cpda, _) = commitment_pda(&program, &id.to_bytes());
    let started = rpc
        .get_account_data(&cpda)
        .ok()
        .and_then(|d| Commitment::deserialize(&mut &d[..]).ok())
        .map(|c| c.started);
    match started {
        Some(n) => println!("started (commitments ever made) : {n}"),
        None => println!("started (commitments ever made) : none"),
    }

    // stored Progress — the snapshot the last successful claim wrote.
    let (ppda, _) = Pubkey::find_program_address(&[SEED_PROGRESS, id.as_ref(), &[1u8]], &program);
    match rpc
        .get_account_data(&ppda)
        .ok()
        .and_then(|d| Progress::deserialize(&mut &d[..]).ok())
    {
        Some(p) => println!(
            "stored Progress (last claim)    : level {} · {} attempts · {} distinct · xp {}",
            p.level, p.attempts_counted, p.distinct_cases, p.xp
        ),
        None => println!("stored Progress (last claim)    : none claimed yet"),
    }

    // The live claim buffer — the exact inputs a NEW claim would adjudicate over.
    let clpda = claim_pda(&program, &id, tree_id);
    let Some(data) = rpc.get_account_data(&clpda).ok() else {
        println!("\nno ClaimAccount at {clpda} — nothing proven on this tree");
        println!(
            "This player may have played on an earlier tree. `verify_player` with no arguments\n\
             lists who has records on tree #{tree_id}."
        );
        std::process::exit(EXIT_UNVERIFIED);
    };
    // Not `.expect()`. A tool whose whole claim is "check it yourself" may not answer the person
    // checking with a panic and a line number — see `explain_stale_layout`.
    let claim = match ClaimAccount::deserialize(&mut &data[..]) {
        Ok(c) => c,
        Err(e) => explain_stale_layout(&clpda, tree_id, data.len(), &e),
    };
    let attempts: Vec<Attempt> = claim
        .attempts
        .iter()
        .map(|a| Attempt {
            case: a.case,
            score: a.score,
            max: a.max,
            det_score: a.det_score,
            det_max: a.det_max,
            difficulty: match a.difficulty {
                1 => Difficulty::Intern,
                2 => Difficulty::Resident,
                _ => Difficulty::Student,
            },
            exam_mode: a.exam_mode,
        })
        .collect();

    println!(
        "\nlive ClaimAccount: {} proven attempt{}",
        attempts.len(),
        if attempts.len() == 1 { "" } else { "s" }
    );
    for (i, a) in claim.attempts.iter().enumerate() {
        // The full 32 bytes, not a four-byte tag. `case` *is* the sha256 of the scenario file the
        // run was played against, so printed whole it is a file name: the archived copy of that
        // exact version is `conformance/sce-archive/<this>.json`, and hashing it gives this number
        // back. That round trip is the last link in the chain from "a level on screen" to "a file
        // you can read", and a truncated tag breaks it for no gain.
        println!("  #{i} case {}", hex32(&a.case));
        println!(
            "     outcome {}/{} · det {}/{} · difficulty {} · exam {}",
            a.score, a.max, a.det_score, a.det_max, a.difficulty, a.exam_mode
        );
        println!("     leaf {}", hex32(&a.leaf));
    }
    print_scenario_check(&claim.attempts.iter().map(|a| a.case).collect::<Vec<_>>());
    let stars = vitals_progress::stars(&attempts, vitals_progress::STAR_PASS_BPS);
    println!(
        "\nstars (distinct exam cases cleared at det >= {}%): {stars}",
        vitals_progress::STAR_PASS_BPS / 100
    );

    let s = summarize(&attempts);
    println!(
        "\nsummary: distinct {} · hard {} · avg {}bps · computed = {}",
        s.distinct_cases,
        s.hard_cases,
        s.avg_bps,
        s.dreyfus.as_str()
    );
    for want in [Dreyfus::Competent, Dreyfus::Proficient] {
        let line = match adjudicate(want, &attempts) {
            Verdict::Granted(l) => format!("GRANTED → {}", l.as_str()),
            Verdict::Rejected { claimed, computed } => {
                format!(
                    "REJECTED (claimed {}, computed {})",
                    claimed.as_str(),
                    computed.as_str()
                )
            }
        };
        println!("  claim {:<11}: {line}", want.as_str());
    }
    std::process::exit(EXIT_OK);
}

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
