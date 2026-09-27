//! **An RPC's address never leaves this server in words.**
//!
//! A dedicated RPC carries its API key in its url — a query parameter (`?api-key=…`) or a path
//! segment — and the Solana client quotes that url in every transport error: "…for url (…)". Those
//! errors reach the log and, through the receipt and the board, public pages. So every chain error
//! this crate turns into text goes through [`scrub`] first. The public endpoint has no secret in it
//! and comes through unchanged; a keyed one comes through with the key replaced by `…`.

/// Shortest path segment treated as a token. Real ones are 32+ characters; ordinary path words
/// (`rpc`, `v1`, `solana`) are short and are left as they are.
const TOKEN_MIN: usize = 16;

/// Parameter names whose value is a secret on any url, configured or not.
const KEYED: [&str; 5] = ["api-key=", "api_key=", "apikey=", "access_token=", "token="];

/// `text` with every secret part of the configured RPC url (`VITALS_RPC`) removed.
pub fn scrub(text: &str) -> String {
    scrub_with(text, &std::env::var("VITALS_RPC").unwrap_or_default())
}

/// [`scrub`] against a given url — pure, so it can be pinned without the environment.
pub fn scrub_with(text: &str, url: &str) -> String {
    let mut out = text.to_string();
    for part in secret_parts(url) {
        out = out.replace(&part, "…");
    }
    for name in KEYED {
        out = blank_values(&out, name);
    }
    out
}

/// The query and every token-length path segment of `url`: the parts that can be a key. The host
/// is kept, because "which provider went quiet" is worth saying and is not a secret.
fn secret_parts(url: &str) -> Vec<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    // Everything after the host, split at the first `?`.
    let Some(at) = rest.find(['/', '?']) else { return Vec::new() };
    let (path, query) = match rest[at..].strip_prefix('?') {
        Some(q) => ("", q),
        None => rest[at..].split_once('?').unwrap_or((&rest[at..], "")),
    };
    let mut parts: Vec<String> = path
        .split('/')
        .filter(|s| s.len() >= TOKEN_MIN)
        .map(str::to_string)
        .collect();
    if !query.is_empty() {
        parts.push(query.to_string());
        parts.extend(query.split('&').filter_map(|kv| kv.split_once('=')).map(|(_, v)| v.to_string()))
    }
    parts.retain(|p| p.len() >= 8);
    // Longest first, so a part is never left behind half-replaced by a shorter one inside it.
    parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
    parts
}

/// Every value after `name` replaced with `…`, up to the first character a key is not made of.
fn blank_values(text: &str, name: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(name) {
        let (before, from) = rest.split_at(at + name.len());
        out.push_str(before);
        let end = from
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
            .unwrap_or(from.len());
        if end > 0 {
            out.push('…');
        }
        rest = &from[end..];
    }
    out.push_str(rest);
    out
}
