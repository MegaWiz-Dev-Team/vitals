//! What the donations have actually bought, as numbers a stranger can re-derive.
//!
//! The donate page used to say only where the money goes — "inference for free play, and
//! commissioning new scenarios". A sentence is not a number, and an unverifiable promise is
//! exactly what the rest of this codebase spends its whole architecture refusing to ask anyone
//! for. This module is the reading behind the ask: the relay's balance and the runs it still
//! buys, and the treasury's balances — SOL and USDC, because the page invites both. The month's
//! turns and the anchored count already exist ([`crate::meter`] and the leaf list); the page
//! joins all four in one place.
//!
//! Five rules the shape enforces, each of them a way the honest version could have gone wrong:
//!
//!   * **The server reads the chain, never the browser.** A page fetching a public RPC directly
//!     is a CORS failure on a good day and a per-visitor rate-limit ban on a bad one.
//!   * **A reading is a number or a reason, never both.** When the RPC is down the field carries
//!     the failure and no lamports at all — so a figure from ten minutes ago can never be painted
//!     as current, and a fetch failure can never be mistaken for a balance of zero.
//!   * **Lamports, not floats.** Balances travel as integers and are rendered to a fixed nine
//!     decimals by integer arithmetic. Nothing here rounds a number into looking better than it
//!     is, in either direction; the runway divides *down*.
//!   * **The RPC's own words never reach the page.** A private RPC url can carry an API key, and
//!     transport errors quote the url they failed on. This endpoint is public and ungated, so a
//!     failed read reports a fixed sentence naming the cluster and nothing else.
//!   * **Two assets, two readings, never one number.** The page asks for SOL *or* USDC, so it has
//!     to be able to show either. They are reported side by side in their own units, under their
//!     own addresses, each with its own number-or-reason — and they are never added. There is no
//!     price on this page to add them with, and one figure standing for both would be the exact
//!     thing a reader could mistake for a total when it is only a part.

use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use std::time::{Duration, Instant};

/// What one of this program's transactions actually costs: 5,000 lamports a signature, and every
/// transaction here carries two — the relay's and the player's. Measured, not estimated.
pub const FEE_LAMPORTS_PER_TX: u64 = 10_000;

/// Transactions a finished run sends. Anchor and prove are two; a player's first ever run bundles
/// an `OpenAccount` alongside and reaches three. The *larger* number is the divisor deliberately:
/// a runway quoted off the cheapest case is a runway that runs out earlier than the page said.
pub const TX_PER_RUN: u64 = 3;

/// 30,000 lamports. Printed on the page beside the number it produces, because a figure whose
/// divisor is hidden is not checkable, and unchekable is the thing this page is arguing against.
pub const LAMPORTS_PER_RUN: u64 = FEE_LAMPORTS_PER_TX * TX_PER_RUN;

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// The treasury the donate page prints, QRs and links to the explorer.
pub const TREASURY: &str = "9FJRwWnTNQXB9ff5SSmQKytCdVYqTQQPUz1b4zX9mt8y";

/// USDC on Solana mainnet: Circle's own mint, and the only dollar this page will ever read.
///
/// Source, because a mint is the one constant here that cannot be checked by reading the code:
/// Circle's published contract-address list at
/// <https://developers.circle.com/stablecoins/usdc-contract-addresses>, which names this string
/// for Solana mainnet (and a different one, `4zMMC9sr…`, for devnet — deliberately absent, since
/// devnet dollars are a faucet away and this page only ever claims mainnet). Verified against the
/// chain as well: the account is owned by the SPL Token program, parses as a `mint`, and carries
/// six decimals.
///
/// Getting this wrong is not cosmetic. A mint that is nearly right prints somebody else's holding
/// under our label, which is worse than printing nothing — so the amount is only believed after
/// the account it came out of has been checked against this key. Bridged and wrapped dollars
/// (USDCet and its cousins) and every other stablecoin are different mints and are *not* read
/// here; the page says so rather than leaving a donor to guess.
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// USDC's decimals, fixed by the mint above: six. Published beside the figure it produces, for
/// the same reason the lamports divisor is — a number whose scale is hidden is not checkable.
pub const USDC_DECIMALS: u32 = 6;

/// One USDC in the base units that actually travel. Millionths of a dollar, integers throughout.
const USDC_BASE_UNITS: u64 = 1_000_000;

/// The SPL Token program. It owns every classic token account, USDC's included, and an account
/// not owned by it is not a token account no matter what its bytes look like.
const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// The associated-token-account program. Its only job here is to make the treasury's USDC address
/// *derivable* rather than configurable: a second address written down beside the first is a
/// second address that can be wrong, and this one is recomputed from the treasury on every read.
const ATA_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// Where that address is read.
///
/// Mainnet, and not the cluster the app anchors to. The page's QR is a bare `solana:<address>`
/// and its explorer link carries no cluster, so a wallet scanning it sends on mainnet and a
/// reader checking it reads mainnet. Quoting a devnet balance under a mainnet link would be a
/// lie assembled out of two true halves.
const TREASURY_RPC: &str = "https://api.mainnet-beta.solana.com";

/// A read has this long to answer. The request loop is single threaded, so the ceiling on one
/// balance read is also the ceiling on how long every other visitor waits behind it.
const PATIENCE: Duration = Duration::from_secs(5);

/// How long a good reading stays good: long enough that a burst of page views is still one RPC
/// call, short enough that a donation appears within a minute of confirming.
const FRESH: Duration = Duration::from_secs(60);

/// A failed reading is retried sooner — but not on every request, or an RPC that is merely down
/// turns each page view into a five-second stall for everyone queued behind it.
const RETRY: Duration = Duration::from_secs(10);

/// One address's reading. Either a balance or the reason there isn't one — never both, and never
/// a number carried over from an earlier, luckier attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Lamports(u64),
    Unavailable(String),
}

/// The same shape for the treasury's USDC, in the mint's own base units.
///
/// A separate type rather than another `Reading` variant, because the units are not lamports and
/// the two must never end up added, averaged or swapped by a careless `match`. The compiler is
/// cheaper than a page that prints dollars under a SOL label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usdc {
    /// Base units — millionths of a dollar. `0` is a real reading and the commonest one: the
    /// token account is created by the first transfer, so an untouched treasury genuinely holds
    /// nothing rather than being unreadable.
    Micro(u64),
    Unavailable(String),
}

struct Cached {
    at: Instant,
    ttl: Duration,
    relay: Reading,
    treasury: Reading,
    treasury_usdc: Usdc,
}

/// The balances behind the donate page, read at most once a minute.
pub struct Fuel {
    /// The cluster the app anchors to — where the relay pays its fees.
    play: RpcClient,
    play_cluster: &'static str,
    /// Where the treasury is read, which is not necessarily the same place.
    vault: RpcClient,
    vault_cluster: &'static str,
    treasury: Option<Pubkey>,
    cache: Option<Cached>,
}

impl Fuel {
    /// Configured from the same environment the rest of the server reads.
    ///
    /// `VITALS_RPC` is the anchoring cluster (the relay's). `VITALS_TREASURY` and
    /// `VITALS_TREASURY_RPC` override the donation address and where it is read, so a deployment
    /// that moves its treasury does not need a rebuild — and a test can point both at a local
    /// validator.
    pub fn open() -> Fuel {
        let play_url = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
        let vault_url = std::env::var("VITALS_TREASURY_RPC")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| TREASURY_RPC.to_string());
        let treasury = std::env::var("VITALS_TREASURY")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| TREASURY.to_string());
        Fuel {
            play_cluster: cluster_of(&play_url),
            play: client(&play_url),
            vault_cluster: cluster_of(&vault_url),
            vault: client(&vault_url),
            // An address that does not parse is not quietly swapped for the default: the page
            // says the treasury could not be read, which is true, rather than showing somebody
            // else's balance under a typo'd label.
            treasury: Pubkey::from_str(&treasury).ok(),
            cache: None,
        }
    }

    /// The startup line, next to the meter's.
    pub fn describe(&self) -> String {
        format!(
            "relay balance from {} · treasury {} on {} · SOL and USDC ({}) · {} lamports/run \
             · refreshed every {}s",
            self.play_cluster,
            self.treasury.map(|k| k.to_string()).unwrap_or_else(|| "unreadable".into()),
            self.vault_cluster,
            USDC_MINT,
            LAMPORTS_PER_RUN,
            FRESH.as_secs(),
        )
    }

    /// What `/api/fuel` serves for the chain half. `relay` is the fee-payer's address — `None`
    /// when this deployment has no chain at all, in which case there is no runway to quote and
    /// the page says exactly that instead of inventing one.
    pub fn view(&mut self, relay: Option<&str>) -> serde_json::Value {
        self.refresh(relay);
        let c = self.cache.as_ref().expect("refresh always leaves a reading");
        serde_json::json!({
            // What a reader needs to redo the division themselves.
            "cost": {
                "fee_lamports_per_tx": FEE_LAMPORTS_PER_TX,
                "tx_per_run": TX_PER_RUN,
                "lamports_per_run": LAMPORTS_PER_RUN,
                "lamports_per_sol": LAMPORTS_PER_SOL,
            },
            "relay": entry(relay.unwrap_or(""), self.play_cluster, &c.relay, true),
            "treasury": self.treasury_view(&c.treasury, &c.treasury_usdc),
            // How old the numbers above are, in seconds. The page prints it: a cached figure
            // presented as live is the quiet version of the lie this endpoint exists to avoid.
            "age_secs": c.at.elapsed().as_secs(),
            "fresh_secs": FRESH.as_secs(),
        })
    }

    fn refresh(&mut self, relay: Option<&str>) {
        if let Some(c) = &self.cache {
            if c.at.elapsed() < c.ttl {
                return;
            }
        }
        let mut failed = false;
        let relay_reading = match relay.and_then(|a| Pubkey::from_str(a).ok()) {
            None => Reading::Unavailable("this deployment anchors nothing — no relay is configured".into()),
            Some(k) => read(&self.play, &k, self.play_cluster, &mut failed),
        };
        let treasury_reading = match self.treasury {
            None => Reading::Unavailable("the treasury address on this server does not parse".into()),
            Some(k) => read(&self.vault, &k, self.vault_cluster, &mut failed),
        };
        // The dollars are read only when the SOL read on this same client has just come back.
        //
        // Not to couple them — they are reported independently, and a USDC-specific failure shows
        // up on its own — but because of what the second call costs when the first has already
        // timed out. The request loop is single threaded, so every visitor queues behind the
        // slowest read; a second five-second wait on an RPC that has just proved unreachable
        // would double that stall to buy a failure we can already state. When the vault answers,
        // this is one more fast round trip on a connection that is demonstrably up; when it does
        // not, the USDC field carries the same reason the SOL field does, which is the true one.
        let treasury_usdc = match (self.treasury, &treasury_reading) {
            (Some(k), Reading::Lamports(_)) => read_usdc(&self.vault, &k, self.vault_cluster, &mut failed),
            (_, Reading::Unavailable(why)) => Usdc::Unavailable(why.clone()),
            (None, _) => Usdc::Unavailable("the treasury address on this server does not parse".into()),
        };
        self.cache = Some(Cached {
            at: Instant::now(),
            ttl: if failed { RETRY } else { FRESH },
            relay: relay_reading,
            treasury: treasury_reading,
            treasury_usdc,
        });
    }

    /// The treasury's entry: its SOL reading, and beside it — never merged into it — its USDC.
    ///
    /// Both figures are labelled "at the address above" on the page, so both are built from the
    /// one address this server actually read, and that address is published in the payload so the
    /// page can check its own printed copy against it.
    fn treasury_view(&self, sol: &Reading, usdc: &Usdc) -> serde_json::Value {
        let address = self.treasury.map(|k| k.to_string()).unwrap_or_default();
        let mut v = entry(&address, self.vault_cluster, sol, false);
        v["usdc"] = usdc_entry(self.treasury.as_ref(), self.vault_cluster, usdc);
        v
    }
}

fn client(url: &str) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(url.to_string(), PATIENCE, CommitmentConfig::confirmed())
}

/// One balance, and a fixed sentence when it does not arrive.
///
/// The RPC's own error text is deliberately dropped. It quotes the url it failed on, a private
/// RPC url can carry an API key in a query parameter, and this endpoint is public and needs no
/// token — so the failure says which cluster went quiet and nothing more.
fn read(rpc: &RpcClient, key: &Pubkey, cluster: &str, failed: &mut bool) -> Reading {
    match rpc.get_balance(key) {
        Ok(l) => Reading::Lamports(l),
        Err(_) => {
            *failed = true;
            Reading::Unavailable(format!("could not reach {cluster} just now — check it yourself below"))
        }
    }
}

/// The treasury's USDC, or the reason there is no figure — never both, exactly as SOL.
///
/// Two absences meet here and must not be allowed to look alike:
///
///   * **No account.** An SPL balance lives in a token account, and the treasury's is created by
///     the first transfer that arrives. Until then there is nothing on chain — and the honest
///     reading of nothing on chain is `0`, not an error. `get_account_with_commitment` reports it
///     as `Ok(None)`: a successful read of an empty holding, which is what it is.
///   * **No answer.** The RPC did not reply. That is `Err`, it carries no number at all, and the
///     page prints the failure — because a zero shown here to a donor who has just sent money
///     reads as "it did not arrive", which is the one lie this whole panel exists to prevent.
fn read_usdc(rpc: &RpcClient, owner: &Pubkey, cluster: &str, failed: &mut bool) -> Usdc {
    let ata = usdc_account(owner);
    match rpc.get_account_with_commitment(&ata, CommitmentConfig::confirmed()) {
        Ok(r) => match r.value {
            None => Usdc::Micro(0),
            Some(a) => match token_amount(&a.owner, &a.data, owner, &key(USDC_MINT)) {
                Some(n) => Usdc::Micro(n),
                // Something is at the derived address and it is not this treasury's USDC. No
                // figure at all: a number lifted out of the wrong account is precisely the harm
                // the mint constant is checked against, and it must not reach the page.
                None => Usdc::Unavailable(
                    "the treasury's USDC account did not read as one — no figure rather than a wrong one"
                        .into(),
                ),
            },
        },
        Err(_) => {
            *failed = true;
            Usdc::Unavailable(format!("could not reach {cluster} just now — check it yourself below"))
        }
    }
}

/// The amount inside an SPL token account — and `None` unless the account really is the one asked
/// for.
///
/// The layout is fixed and public: mint (32 bytes), owner (32), amount (`u64`, little-endian).
/// Both keys are checked against what was asked for before the amount is believed, so a wrong
/// mint constant, a mis-derived address or an account that simply is not a token account all
/// yield nothing rather than somebody else's balance. Integers end to end — an amount is a count
/// of base units and never passes through a float.
fn token_amount(program: &Pubkey, data: &[u8], owner: &Pubkey, mint: &Pubkey) -> Option<u64> {
    if program != &key(TOKEN_PROGRAM) {
        return None;
    }
    let d = data.get(..72)?;
    if &d[..32] != mint.as_ref() || &d[32..64] != owner.as_ref() {
        return None;
    }
    Some(u64::from_le_bytes(d[64..72].try_into().ok()?))
}

/// Where an owner's USDC actually sits.
///
/// Not the wallet address a donor pastes: that account holds SOL. The dollars land in the
/// associated token account derived from `[owner, token program, mint]`, which is what a wallet
/// computes when it sends and what this recomputes when it reads — so there is nothing to keep in
/// sync. It does not exist until the first transfer creates it, which is why its absence is read
/// as zero.
pub fn usdc_account(owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), key(TOKEN_PROGRAM).as_ref(), key(USDC_MINT).as_ref()],
        &key(ATA_PROGRAM),
    )
    .0
}

/// One of this file's own address literals, parsed. Every one of them is a compiled-in constant
/// and a test parses all of them, so a panic here would have failed the build first.
fn key(literal: &str) -> Pubkey {
    Pubkey::from_str(literal).expect("an address literal in this file")
}

/// The USDC half of the treasury's entry.
///
/// A sibling of the SOL figure, never a component of it: its own units, its own address, its own
/// number-or-reason. The mint and the token account both travel so the page can print what it
/// read and link where a stranger can re-read it.
fn usdc_entry(owner: Option<&Pubkey>, cluster: &str, u: &Usdc) -> serde_json::Value {
    let mut v = serde_json::json!({
        "symbol": "USDC",
        "mint": USDC_MINT,
        "mint_explorer": explorer(USDC_MINT, cluster),
        "decimals": USDC_DECIMALS,
        "base_units_per_token": USDC_BASE_UNITS,
    });
    // No owner, no derived account — and so no link, for the same reason the SOL entry withholds
    // one: a url built around an empty string is a dead end dressed as a way to check.
    if let Some(k) = owner {
        let ata = usdc_account(k).to_string();
        v["explorer"] = serde_json::json!(explorer(&ata, cluster));
        v["account"] = serde_json::json!(ata);
    }
    match u {
        Usdc::Micro(n) => {
            v["micro"] = serde_json::json!(n);
            v["amount"] = serde_json::json!(usdc(*n));
        }
        Usdc::Unavailable(why) => {
            v["error"] = serde_json::json!(why);
        }
    }
    v
}

fn entry(address: &str, cluster: &str, r: &Reading, runway: bool) -> serde_json::Value {
    let mut v = serde_json::json!({
        "cluster": cluster,
        // Devnet SOL is minted from a faucet by anyone who asks. Saying so is the difference
        // between a fuel gauge and a claim of assets, and the page must never blur the two.
        "play_money": cluster != "mainnet",
    });
    // No address, no link. An explorer url built around an empty string is a link that goes
    // nowhere dressed as a way to check.
    if !address.is_empty() {
        v["address"] = serde_json::json!(address);
        v["explorer"] = serde_json::json!(explorer(address, cluster));
    }
    match r {
        Reading::Lamports(l) => {
            v["lamports"] = serde_json::json!(l);
            v["sol"] = serde_json::json!(sol(*l));
            if runway {
                // Floor division, always. A runway rounded up is a promise of a run the balance
                // cannot pay for.
                v["runs_left"] = serde_json::json!(l / LAMPORTS_PER_RUN);
            }
        }
        Reading::Unavailable(why) => {
            v["error"] = serde_json::json!(why);
        }
    }
    v
}

/// Lamports as an exact decimal string, nine places, no rounding anywhere.
///
/// Not `l as f64 / 1e9`: a balance is an integer count of lamports and f64 stops representing
/// them exactly a few SOL in. The page prints this string verbatim.
pub fn sol(lamports: u64) -> String {
    format!("{}.{:09}", lamports / LAMPORTS_PER_SOL, lamports % LAMPORTS_PER_SOL)
}

/// Base units as an exact decimal string, six places, no rounding anywhere.
///
/// The same integer arithmetic [`sol`] uses and for the same reason: a holding is a count of base
/// units, `n as f64 / 1e6` stops representing them exactly, and this page is an argument about
/// figures a stranger can re-derive. The page prints this string verbatim.
pub fn usdc(micro: u64) -> String {
    format!("{}.{:06}", micro / USDC_BASE_UNITS, micro % USDC_BASE_UNITS)
}

/// A link that opens this address on the cluster it was actually read from.
///
/// Mainnet takes no parameter — that is the explorer's default, and it is the link already on the
/// page. Everything else must carry one, or the reader lands on mainnet, sees an empty account
/// and concludes the page invented the number.
pub fn explorer(address: &str, cluster: &str) -> String {
    match cluster {
        "mainnet" => format!("https://explorer.solana.com/address/{address}"),
        // A local validator is not reachable from a stranger's browser at all. The link still
        // resolves to the right shape; the page is what says a localnet reading is unauditable.
        "localnet" | "custom" => format!("https://explorer.solana.com/address/{address}?cluster=custom"),
        c => format!("https://explorer.solana.com/address/{address}?cluster={c}"),
    }
}

/// Which cluster an RPC url points at. The label is derived from the url the transactions
/// actually go to rather than configured beside it, so the two cannot drift — it said "localnet"
/// on the public demo once.
pub fn cluster_of(rpc: &str) -> &'static str {
    if rpc.contains("devnet") {
        "devnet"
    } else if rpc.contains("testnet") {
        "testnet"
    } else if rpc.contains("mainnet") {
        "mainnet"
    } else if rpc.contains("127.0.0.1") || rpc.contains("localhost") {
        "localnet"
    } else {
        "custom"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_divisor_is_the_cautious_one_and_it_is_published() {
        // Two transactions is the common case; three is the first-ever run. The page divides by
        // the expensive one, so the runway it quotes is never longer than the balance can pay.
        assert_eq!(FEE_LAMPORTS_PER_TX, 10_000);
        assert_eq!(TX_PER_RUN, 3);
        assert_eq!(LAMPORTS_PER_RUN, 30_000);
    }

    #[test]
    fn a_runway_is_floored_never_rounded_up() {
        // 29,999 lamports buys nothing. Rounding this to "1 run left" is the exact failure the
        // page exists to avoid.
        assert_eq!(runs(29_999), 0);
        assert_eq!(runs(30_000), 1);
        assert_eq!(runs(59_999), 1);
        assert_eq!(runs(5_713_747_680), 190_458);
    }

    fn runs(lamports: u64) -> u64 {
        let v = entry("x", "devnet", &Reading::Lamports(lamports), true);
        v["runs_left"].as_u64().expect("a balance quotes a runway")
    }

    #[test]
    fn sol_is_exact_to_the_lamport() {
        assert_eq!(sol(0), "0.000000000");
        assert_eq!(sol(1), "0.000000001");
        assert_eq!(sol(1_000_000_000), "1.000000000");
        assert_eq!(sol(5_713_747_680), "5.713747680");
        // The one f64 gets wrong: 0.1 + 0.2 arithmetic in disguise. Integer division cannot.
        assert_eq!(sol(300_000_007), "0.300000007");
        assert_eq!(sol(u64::MAX), "18446744073.709551615");
    }

    #[test]
    fn a_failed_read_carries_no_number_at_all() {
        let v = entry("x", "devnet", &Reading::Unavailable("rpc is down".into()), true);
        assert!(v["lamports"].is_null(), "a failure must not be paintable as a balance");
        assert!(v["sol"].is_null());
        assert!(v["runs_left"].is_null(), "no balance, no runway");
        assert_eq!(v["error"], "rpc is down");
    }

    #[test]
    fn a_zero_balance_is_a_number_and_not_a_failure() {
        // The treasury is empty today. Showing that is the point; showing nothing would read as
        // a broken page, and showing "—" would read as a secret.
        let v = entry(TREASURY, "mainnet", &Reading::Lamports(0), false);
        assert_eq!(v["lamports"], 0);
        assert_eq!(v["sol"], "0.000000000");
        assert!(v["error"].is_null());
    }

    #[test]
    fn the_rpcs_own_words_never_reach_the_page() {
        // A private RPC url can carry an API key, and transport errors quote the url. This
        // endpoint is public and ungated, so the message is fixed and names only the cluster.
        let mut failed = false;
        // 127.0.0.1:1 has nothing listening; the read fails immediately.
        let r = read(&client("http://127.0.0.1:1"), &Pubkey::new_unique(), "localnet", &mut failed);
        assert!(failed, "a transport failure must shorten the cache ttl");
        match r {
            Reading::Unavailable(why) => {
                assert!(why.contains("localnet"), "{why}");
                assert!(!why.contains("127.0.0.1"), "the url leaked into a public field: {why}");
                assert!(!why.contains("http"), "the url leaked into a public field: {why}");
            }
            Reading::Lamports(_) => panic!("nothing is listening on port 1"),
        }
    }

    #[test]
    fn every_cluster_gets_a_link_that_opens_on_that_cluster() {
        assert_eq!(
            explorer("A", "mainnet"),
            "https://explorer.solana.com/address/A",
            "mainnet is the explorer's default and the link already on the page"
        );
        assert!(explorer("A", "devnet").ends_with("?cluster=devnet"));
        assert!(explorer("A", "testnet").ends_with("?cluster=testnet"));
        assert!(explorer("A", "localnet").ends_with("?cluster=custom"));
    }

    #[test]
    fn devnet_sol_is_marked_as_play_money_and_mainnet_is_not() {
        let d = entry("A", "devnet", &Reading::Lamports(1), true);
        assert_eq!(d["play_money"], true, "devnet SOL comes out of a faucet — never call it an asset");
        let m = entry("A", "mainnet", &Reading::Lamports(1), false);
        assert_eq!(m["play_money"], false);
    }

    #[test]
    fn the_label_names_the_cluster_the_rpc_actually_points_at() {
        assert_eq!(cluster_of("https://api.devnet.solana.com"), "devnet");
        assert_eq!(cluster_of("https://api.testnet.solana.com"), "testnet");
        assert_eq!(cluster_of("https://api.mainnet-beta.solana.com"), "mainnet");
        assert_eq!(cluster_of("http://127.0.0.1:8899"), "localnet");
        assert_eq!(cluster_of("http://localhost:8899"), "localnet");
        assert_eq!(cluster_of("https://rpc.example.com"), "custom");
    }

    #[test]
    fn a_view_without_a_chain_says_so_rather_than_quoting_a_runway() {
        let mut f = Fuel {
            play_cluster: "localnet",
            play: client("http://127.0.0.1:1"),
            vault_cluster: "mainnet",
            vault: client("http://127.0.0.1:1"),
            treasury: None,
            cache: None,
        };
        let v = f.view(None);
        assert!(v["relay"]["lamports"].is_null());
        assert!(v["relay"]["runs_left"].is_null());
        assert!(v["relay"]["error"].as_str().unwrap().contains("no relay"));
        assert!(v["treasury"]["error"].as_str().unwrap().contains("does not parse"));
        // The divisor is published whether or not there is a balance to divide.
        assert_eq!(v["cost"]["lamports_per_run"], 30_000);
        assert_eq!(v["age_secs"], 0);
    }

    #[test]
    fn a_reading_is_cached_and_a_failed_one_is_retried_sooner() {
        let mut f = Fuel {
            play_cluster: "localnet",
            play: client("http://127.0.0.1:1"),
            vault_cluster: "localnet",
            vault: client("http://127.0.0.1:1"),
            treasury: Some(Pubkey::new_unique()),
            cache: None,
        };
        let _ = f.view(None);
        assert_eq!(f.cache.as_ref().unwrap().ttl, RETRY, "a failed read comes back in ten seconds");
        // A second call inside the window reuses the reading rather than dialling again.
        let before = f.cache.as_ref().unwrap().at;
        let _ = f.view(None);
        assert_eq!(f.cache.as_ref().unwrap().at, before, "the cache was not consulted");
    }

    // ── the second asset ───────────────────────────────────────────────────────────────────
    //
    // The page invites "SOL or USDC" and for a long time could only read SOL, so a donor who
    // sent dollars opened it and saw a zero — the promise broken for exactly the person who had
    // just kept their side of it, and broken in the direction that looks like theft. These pin
    // the reading that closed that, and the one thing about it that cannot be checked by reading
    // the code: which mint the word "USDC" means.

    /// Every address this file compiles in, parsed. `key` panics on a bad literal by design; this
    /// is what makes that panic unreachable outside a broken edit, and it fails here rather than
    /// in front of a donor.
    #[test]
    fn every_address_literal_in_this_file_parses() {
        for literal in [TREASURY, USDC_MINT, TOKEN_PROGRAM, ATA_PROGRAM] {
            assert_eq!(key(literal).to_string(), literal, "{literal} does not round-trip");
        }
    }

    /// Which dollar this page means, written down so it can be re-checked.
    ///
    /// A mint is the one constant here a reader cannot verify by reasoning about the code — it is
    /// just a string, and a nearly-right one shows a stranger somebody else's balance under our
    /// label. This is Circle's Solana-mainnet USDC, from their published contract-address list,
    /// and six decimals is that mint's own. Devnet's USDC is a different mint and is deliberately
    /// not here: this page only ever claims mainnet.
    #[test]
    fn the_mint_is_circles_mainnet_usdc_and_the_scale_is_its_own() {
        assert_eq!(USDC_MINT, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        assert_eq!(USDC_DECIMALS, 6);
        assert_eq!(USDC_BASE_UNITS, 10u64.pow(USDC_DECIMALS));
        assert_ne!(USDC_MINT, "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU", "that is devnet's");
    }

    /// The treasury's dollars do not sit at the treasury's address, and this is where they do.
    ///
    /// Derived, never configured — but a derivation with the arguments in the wrong order still
    /// produces a plausible-looking address, so it is pinned against the one the `spl-token` CLI
    /// computes for this exact owner and mint. If this changes, the panel is reading a stranger.
    #[test]
    fn the_dollars_are_read_where_a_wallet_would_have_sent_them() {
        assert_eq!(
            usdc_account(&key(TREASURY)).to_string(),
            "6QoXQNaF3fELSMHPMdpG4SAV43mxfjCw3kYgdFG9qW8q",
            "the USDC account derived for the treasury moved"
        );
    }

    #[test]
    fn usdc_is_exact_to_the_base_unit() {
        assert_eq!(usdc(0), "0.000000");
        assert_eq!(usdc(1), "0.000001");
        assert_eq!(usdc(1_000_000), "1.000000");
        assert_eq!(usdc(250_000_000), "250.000000");
        // The one f64 gets wrong at this scale, for the same reason it gets lamports wrong.
        assert_eq!(usdc(1_000_000_000_007), "1000000.000007");
        assert_eq!(usdc(u64::MAX), "18446744073709.551615");
    }

    /// The commonest reading on this page, and the one it would have been easiest to get wrong.
    ///
    /// A treasury nobody has sent dollars to has no token account at all — the first transfer
    /// creates it. "No account" is a successful read of an empty holding, so it prints `0`. An
    /// error here would tell a donor the page is broken; a blank would read as a secret.
    #[test]
    fn no_token_account_yet_is_zero_and_not_a_failure() {
        let v = usdc_entry(Some(&key(TREASURY)), "mainnet", &Usdc::Micro(0));
        assert_eq!(v["micro"], 0);
        assert_eq!(v["amount"], "0.000000");
        assert!(v["error"].is_null(), "an empty holding is not a failure");
        // And it still says where it looked, so the zero is checkable rather than asserted.
        assert_eq!(v["account"], "6QoXQNaF3fELSMHPMdpG4SAV43mxfjCw3kYgdFG9qW8q");
        assert!(v["explorer"].as_str().unwrap().contains("6QoXQNaF"));
    }

    /// The inverse, and the whole point of keeping them apart: a read that did not happen leaves
    /// no digit behind. A zero here would say "your dollars are not in the treasury".
    #[test]
    fn a_failed_usdc_read_carries_no_number_at_all() {
        let v = usdc_entry(Some(&key(TREASURY)), "mainnet", &Usdc::Unavailable("rpc is down".into()));
        assert!(v["micro"].is_null(), "a failure must not be paintable as an empty wallet");
        assert!(v["amount"].is_null());
        assert_eq!(v["error"], "rpc is down");
        // The mint and the account survive the failure: the reader can still go and look.
        assert_eq!(v["mint"], USDC_MINT);
        assert!(v["explorer"].is_string());
    }

    fn token_account(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
        let mut d = vec![0u8; 165];
        d[..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        d[64..72].copy_from_slice(&amount.to_le_bytes());
        d
    }

    /// An amount is believed only out of the account it was asked for.
    ///
    /// Four ways the bytes at a derived address could belong to somebody else, and all four have
    /// to produce no figure rather than a wrong one — that is what makes a wrong mint constant a
    /// blank panel instead of a stranger's balance printed as ours.
    #[test]
    fn an_amount_is_only_believed_out_of_the_account_it_was_asked_for() {
        let mint = key(USDC_MINT);
        let me = key(TREASURY);
        let token = key(TOKEN_PROGRAM);
        let stranger = Pubkey::new_unique();

        let good = token_account(&mint, &me, 12_345_678);
        assert_eq!(token_amount(&token, &good, &me, &mint), Some(12_345_678));

        assert_eq!(token_amount(&stranger, &good, &me, &mint), None, "not owned by the token program");
        let wrong_mint = token_account(&stranger, &me, 999);
        assert_eq!(token_amount(&token, &wrong_mint, &me, &mint), None, "a different token entirely");
        let wrong_owner = token_account(&mint, &stranger, 999);
        assert_eq!(token_amount(&token, &wrong_owner, &me, &mint), None, "somebody else's dollars");
        assert_eq!(token_amount(&token, &good[..71], &me, &mint), None, "truncated, so unreadable");
    }

    /// SOL and USDC arrive as two figures in two units, and nothing anywhere adds them.
    ///
    /// There is no price on this page, so a combined number could not be re-derived by the
    /// reader — it would be the one figure on the panel that has to be taken on trust, and it
    /// would be the one most likely to be read as a total.
    #[test]
    fn the_two_assets_are_reported_side_by_side_and_never_summed() {
        let f = Fuel {
            play_cluster: "localnet",
            play: client("http://127.0.0.1:1"),
            vault_cluster: "mainnet",
            vault: client("http://127.0.0.1:1"),
            treasury: Some(key(TREASURY)),
            cache: None,
        };
        let v = f.treasury_view(&Reading::Lamports(2_000_000_000), &Usdc::Micro(50_000_000));
        assert_eq!(v["sol"], "2.000000000");
        assert_eq!(v["lamports"], 2_000_000_000u64);
        assert_eq!(v["usdc"]["amount"], "50.000000");
        assert_eq!(v["usdc"]["micro"], 50_000_000u64);
        assert_eq!(v["usdc"]["decimals"], 6);
        // Different addresses, and both published — the SOL sits at the treasury, the dollars at
        // the token account it owns, and a reader can check each where it actually is.
        assert_eq!(v["address"], TREASURY);
        assert_ne!(v["usdc"]["account"], serde_json::json!(TREASURY));
        // No total, by any name. The units do not combine and neither do the numbers.
        for made_up in ["total", "value", "usd", "combined", "worth"] {
            assert!(v[made_up].is_null(), "{made_up} is a figure nobody can re-derive");
        }
    }

    /// One RPC that has already failed is not dialled a second time.
    ///
    /// The request loop is single threaded, so the ceiling on a page view is the sum of its
    /// reads. Adding dollars must not double the stall on the day the chain is unreachable —
    /// which is the day this page gets the most visitors. The USDC field still says why.
    #[test]
    fn an_unreachable_vault_is_not_dialled_twice_for_the_second_asset() {
        let mut f = Fuel {
            play_cluster: "localnet",
            play: client("http://127.0.0.1:1"),
            vault_cluster: "mainnet",
            vault: client("http://127.0.0.1:1"),
            treasury: Some(key(TREASURY)),
            cache: None,
        };
        let v = f.view(None);
        let sol = v["treasury"]["error"].as_str().expect("the SOL read failed");
        let dollars = v["treasury"]["usdc"]["error"].as_str().expect("so the USDC read reports it");
        assert_eq!(sol, dollars, "the dollars must not invent a second, different reason");
        assert!(v["treasury"]["usdc"]["micro"].is_null(), "an unreachable chain is not an empty wallet");
        assert!(v["treasury"]["lamports"].is_null());
    }

    fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
        let a = s.find(open)? + open.len();
        let rest = &s[a..];
        Some(&rest[..rest.find(close)?])
    }

    /// The page must print the address this module reads.
    ///
    /// `donate.html` prints the treasury, QRs it as a `solana:` payload and links it to an
    /// explorer. This module reads a balance and the page labels it "at the address above". That
    /// is one fact kept in two languages, and there is an env override — `VITALS_TREASURY` — that
    /// moves the balance without moving the print. Divergence there is silent by construction:
    /// the page would keep showing address A above address B's balance and look completely
    /// normal. Nobody reviewing either file alone would catch it, so the build catches it. The
    /// runtime half of the same guard is in the page: it rebuilds the printed address, the QR and
    /// the explorer link from the address `/api/fuel` reports, and says so when they differ.
    #[test]
    fn the_page_prints_and_qrs_the_address_this_module_reads() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static/donate.html");
        let html = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

        let printed = between(&html, "id=\"addrText\">", "<").expect("the printed address is gone");
        assert_eq!(printed, TREASURY, "the page prints an address this server does not read");

        let payload = between(&html, "'solana:", "'").expect("the QR payload is gone");
        assert_eq!(payload, TREASURY, "the QR would send to an address this server does not read");

        let audit = between(&html, "href=\"https://explorer.solana.com/address/", "\"")
            .expect("the audit link is gone");
        assert_eq!(audit, TREASURY, "the audit link opens an address this server does not read");

        // And the page has to be able to reconcile at runtime too, or the env override is still
        // a silent mismatch on a deployment that sets it.
        assert!(
            html.contains("v.address"),
            "the page never checks its printed address against the one the balance was read at"
        );
    }
}
