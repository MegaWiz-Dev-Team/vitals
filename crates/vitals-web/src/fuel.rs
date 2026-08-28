//! What the donations have actually bought, as numbers a stranger can re-derive.
//!
//! The donate page used to say only where the money goes — "inference for free play, and
//! commissioning new scenarios". A sentence is not a number, and an unverifiable promise is
//! exactly what the rest of this codebase spends its whole architecture refusing to ask anyone
//! for. This module is the reading behind the ask: the relay's balance and the runs it still
//! buys, and the treasury's balance. The month's turns and the anchored count already exist
//! ([`crate::meter`] and the leaf list); the page joins all four in one place.
//!
//! Four rules the shape enforces, each of them a way the honest version could have gone wrong:
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

struct Cached {
    at: Instant,
    ttl: Duration,
    relay: Reading,
    treasury: Reading,
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
            "relay balance from {} · treasury {} on {} · {} lamports/run · refreshed every {}s",
            self.play_cluster,
            self.treasury.map(|k| k.to_string()).unwrap_or_else(|| "unreadable".into()),
            self.vault_cluster,
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
            "treasury": entry(
                &self.treasury.map(|k| k.to_string()).unwrap_or_default(),
                self.vault_cluster,
                &c.treasury,
                false,
            ),
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
        self.cache = Some(Cached {
            at: Instant::now(),
            ttl: if failed { RETRY } else { FRESH },
            relay: relay_reading,
            treasury: treasury_reading,
        });
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
}
