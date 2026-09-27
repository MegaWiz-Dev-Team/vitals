//! **An RPC's address is never shown to anybody, because a private RPC's address is a key.**
//!
//! Every chain error this server turns into words quoted the url it failed on — on 27 Sep 2026 a
//! public receipt page read "…429 Too Many Requests) for url (https://api.devnet.solana.com/)".
//! Harmless for the public endpoint. A dedicated RPC puts its API key in that url (a query
//! parameter, or a path segment), so the same sentence would publish the key to any stranger who
//! opened a receipt while the chain was busy. The words say which cluster went quiet; the address
//! never leaves.

use vitals_web::rpc_scrub::scrub_with;

const KEYED_QUERY: &str = "https://devnet.helius-rpc.com/?api-key=0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const KEYED_PATH: &str = "https://example.solana-devnet.quiknode.pro/9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d/";

#[test]
fn a_key_in_the_query_never_reaches_the_words() {
    let said = format!("HTTP status client error (429 Too Many Requests) for url ({KEYED_QUERY})");
    let out = scrub_with(&said, KEYED_QUERY);
    assert!(!out.contains("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"), "the key was left in: {out}");
    assert!(out.contains("429 Too Many Requests"), "and the reason itself survives: {out}");
}

#[test]
fn a_key_in_the_path_never_reaches_the_words() {
    // reqwest prints the url it parsed, which may differ from the configured string (a trailing
    // slash added or dropped) — the key must go either way.
    let trimmed = KEYED_PATH.trim_end_matches('/');
    for shown in [KEYED_PATH, trimmed] {
        let out = scrub_with(&format!("error sending request for url ({shown})"), KEYED_PATH);
        assert!(!out.contains("9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d"), "the key was left in: {out}");
    }
}

#[test]
fn a_key_is_scrubbed_even_from_an_address_it_was_not_configured_with() {
    // Belt and braces: a key-shaped parameter on any url is removed, configured or not.
    let out = scrub_with("for url (https://other.example/?api-key=abcdef0123456789)", "https://api.devnet.solana.com");
    assert!(!out.contains("abcdef0123456789"), "{out}");
}

#[test]
fn the_public_endpoint_and_ordinary_words_are_left_alone() {
    let said = "the chain could not be read: connection refused";
    assert_eq!(scrub_with(said, "https://api.devnet.solana.com"), said);
}

/// The whole path, not just the function: a ward read that fails against a keyed url returns words
/// with no key in them.
#[test]
fn a_failed_ward_read_against_a_keyed_url_says_no_key() {
    std::env::set_var("VITALS_RPC", "http://127.0.0.1:1/?api-key=feedfacecafebeef0123");
    std::env::set_var("VITALS_PROGRAM_ID", "4YpyZ2oM8jtxM9GwC61kUsnhMFvWkYatrWVZpiafqypz");
    std::env::set_var("VITALS_OPERATOR", "11111111111111111111111111111111");
    std::env::remove_var("VITALS_KEYPAIR");
    let chain = vitals_web::ward_chain::WardChain::connect().expect("a handle");
    let dir = std::env::temp_dir().join(format!("vitals-rpc-scrub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = vitals_web::store::Store::open(dir).expect("a store");
    let mut seen = vitals_web::ward_chain::Seen::default();
    let err = chain
        .refresh(1, &mut seen, &store, &vitals_web::ward_chain::Budget::pass())
        .err()
        .expect("nothing listens there, so the read fails");
    assert!(!err.contains("feedfacecafebeef0123"), "the key reached the words: {err}");
}
