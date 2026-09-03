//! Sign a case's hash as its author, and write the side table.
//!
//! Attribution is a claim, and a claim nobody signed is a line of text. This turns the archive's
//! case hashes into signed entries in `conformance/sce-archive/AUTHORS.json`.
//!
//! ```text
//! VITALS_AUTHOR_KEY=~/keys/author.json cargo run --bin sign-attribution -- --all
//! cargo run --bin sign-attribution -- --key ~/keys/author.json <sce_hash> [<sce_hash>...]
//! ```
//!
//! ## Where the key may live, and where it may not
//!
//! The path comes from `--key` or `$VITALS_AUTHOR_KEY` and from nowhere else. There is no default
//! and no search: not the working directory, not a name inside the repository, nothing this could
//! find on its own. **A path that resolves inside the repository is refused**, because the way a
//! signing key gets committed is that somebody puts it somewhere convenient first and a tool
//! reads it happily from there.
//!
//! `tests/authors.rs` is the other half of that: it fails the build if anything shaped like a
//! keypair is inside what git would carry.

use solana_sdk::signature::{read_keypair_file, Signer};
use std::path::{Path, PathBuf};
use vitals_web::authors::{self, Attribution};

const USAGE: &str = "\
sign-attribution — sign case hashes as their author

  sign-attribution --all                 every case in the archive index
  sign-attribution <sce_hash>...         only these

  --key <path>    the author's keypair. Or $VITALS_AUTHOR_KEY.
                  Never a path inside this repository.
  --out <path>    where to write (default: conformance/sce-archive/AUTHORS.json)
  --dry-run       print what would be written, write nothing
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }

    let repo = repo_root();
    let key_path = match flag(&args, "--key").or_else(|| std::env::var("VITALS_AUTHOR_KEY").ok()) {
        Some(p) => PathBuf::from(shellexpand(&p)),
        None => die("no key. Pass --key <path> or set VITALS_AUTHOR_KEY.\n\
                     There is deliberately no default: a signing key has no business living \
                     anywhere this tool could guess."),
    };
    refuse_inside_repo(&key_path, &repo);

    let keypair = read_keypair_file(&key_path)
        .unwrap_or_else(|e| die(&format!("{}: {e}", key_path.display())));
    let author = keypair.pubkey().to_string();

    let index = repo.join(authors::INDEX_PATH);
    let known = authors::archive_hashes(&index).unwrap_or_else(|e| die(&e));

    let wanted: Vec<String> = if args.iter().any(|a| a == "--all") {
        known.clone()
    } else {
        positionals(&args)
    };
    if wanted.is_empty() {
        die("nothing to sign. Give case hashes, or --all.");
    }
    for h in &wanted {
        if !known.contains(h) {
            die(&format!("{h} is not in {}. Only cases the archive holds can be \
                          attributed — a case nobody can fetch cannot be one anybody wrote.",
                         authors::INDEX_PATH));
        }
    }

    // Existing entries are kept unless this run re-signs them: re-assignment replaces a case's
    // author, it does not append a second claim to the same case.
    let out_path = flag(&args, "--out")
        .map(|p| PathBuf::from(shellexpand(&p)))
        .unwrap_or_else(|| repo.join(authors::AUTHORS_PATH));
    let mut table = authors::load(&out_path).unwrap_or_else(|e| die(&e));
    table.retain(|a| !wanted.contains(&a.sce_hash));

    for h in &wanted {
        let msg = authors::message(h).unwrap_or_else(|e| die(&e));
        table.push(Attribution {
            sce_hash: h.clone(),
            author: author.clone(),
            signature: keypair.sign_message(&msg).to_string(),
        });
    }
    table.sort_by(|a, b| a.sce_hash.cmp(&b.sce_hash));

    let problems = authors::audit(&table, &known);
    if !problems.is_empty() {
        die(&format!("refusing to write a table that does not verify:\n  {}", problems.join("\n  ")));
    }

    let json = serde_json::to_string_pretty(&table).unwrap_or_else(|e| die(&e.to_string()));
    if args.iter().any(|a| a == "--dry-run") {
        println!("{json}");
        eprintln!("\n-- dry run, nothing written --");
        return;
    }
    std::fs::write(&out_path, format!("{json}\n"))
        .unwrap_or_else(|e| die(&format!("{}: {e}", out_path.display())));
    println!("signed {} case(s) as {author}", wanted.len());
    println!("wrote {}", out_path.display());
}

/// A key inside the repository is refused, whatever it is called and whichever end is inside.
///
/// **Both ends are checked.** The obvious one is where the file really is, after symlinks. The
/// other is the path as given: a link inside the repo pointing at a key outside it resolves to a
/// perfectly legitimate location, and this used to accept it and sign happily. Nothing secret
/// gets committed that way — git stores the link, not its target, and `tests/authors.rs` reads
/// through it and fails the build anyway — but a tool that reads a key through the repository is
/// teaching the habit the rule exists to prevent. So it refuses at the door as well.
fn refuse_inside_repo(key: &Path, repo: &Path) {
    let asked = key.canonicalize().ok().unwrap_or_else(|| absolutise(key));
    let parent_resolved = key
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(key.file_name().unwrap_or_default()))
        .unwrap_or_else(|| absolutise(key));

    for (candidate, how) in [
        (parent_resolved, "the path given is inside this repository"),
        (asked, "the key itself is inside this repository"),
    ] {
        if candidate.starts_with(repo) {
            die(&format!(
                "{}: {how}.\n\
                 A signing key reached through this directory is one `git add -A` from being \
                 published, and tests/authors.rs fails the build for exactly that. Keep it \
                 somewhere the repository cannot reach, and give that path.",
                candidate.display()
            ));
        }
    }
}

fn absolutise(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf())
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|e| die(&format!("cannot find the repository root: {e}")))
}

/// `~` only. Enough for a path typed by a person, without a dependency for it.
fn shellexpand(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => p.to_string(),
        },
        None => p.to_string(),
    }
}

/// Everything that is neither a flag nor a flag's value.
///
/// `--key ~/k.json <hash>` used to yield the key path as well as the hash, because a filter on
/// "does not start with --" cannot tell a value from an argument. It then reported the key path
/// as a case the archive does not have, which is a confusing way to be told you typed something
/// fine.
fn positionals(args: &[String]) -> Vec<String> {
    const TAKES_A_VALUE: [&str; 2] = ["--key", "--out"];
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if TAKES_A_VALUE.contains(&a.as_str()) {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.clone());
        }
    }
    out
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}
