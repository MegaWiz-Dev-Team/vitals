//! The compiler is part of the repo, not part of the machine.
//!
//! 20 ก.ย. 2026, 23:33: the `stable` toolchain on the build machine moved from 1.93 to 1.98.1 under
//! a running workspace test — a case-factory agent's private CARGO_HOME let the rustup proxy fetch
//! it. The half-built test binaries refused the old artifacts (`E0514`), zero suites ran, and the
//! wrapper still printed "done". Then clippy 1.98 flagged four `result_large_err` the old compiler
//! did not, and every commit on the branch was red on the new one. Meanwhile CI had been pinned at
//! 1.93.1 the whole time, so gates here and CI there had already been judging the same commits with
//! different compilers for as long as the machine's `stable` had drifted.
//!
//! So: one version, named once in `rust-toolchain.toml`, and CI names the same one. A compiler bump
//! is a commit somebody reviews, never something that happens to the machine.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// **The repo pins its compiler, and CI compiles with the same one.**
#[test]
fn the_repo_and_ci_name_the_same_compiler() {
    let pin = std::fs::read_to_string(root().join("rust-toolchain.toml")).expect(
        "rust-toolchain.toml at the repo root — without it every session floats on whatever \
         `stable` is at the moment, and a background `rustup update` can void a gates run",
    );
    let channel = pin
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("channel")
                .and_then(|r| r.trim().strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
        .expect("rust-toolchain.toml names a `channel`");
    assert!(
        channel.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "the channel is an exact version, not `stable`: pinning to a moving name pins nothing — {channel}"
    );

    let ci = std::fs::read_to_string(root().join(".github/workflows/ci.yml")).expect("ci.yml");
    let ci_versions: Vec<String> = ci
        .lines()
        .filter_map(|l| l.trim().strip_prefix("toolchain:").map(|v| v.trim().to_string()))
        .collect();
    assert!(!ci_versions.is_empty(), "ci.yml installs a toolchain by name somewhere");
    for v in &ci_versions {
        assert_eq!(
            v, &channel,
            "CI compiles with {v} while the repo pins {channel} — gates here and CI there would be \
             judging the same commit with different compilers, which is how a branch is green in \
             one place and red in the other"
        );
    }

    // The third place the compiler lives, and the one that ships: the deploy image. Found while
    // writing the green — `FROM rust:1.93-slim-bookworm` — so production was being built with a
    // compiler neither gates nor CI had run, and nothing in the red above would have said so.
    let dockerfile = std::fs::read_to_string(root().join("Dockerfile")).expect("Dockerfile");
    let image = dockerfile
        .lines()
        .find_map(|l| l.trim().strip_prefix("FROM rust:").map(|r| r.split_whitespace().next().unwrap_or("").to_string()))
        .expect("the Dockerfile builds FROM a rust image");
    assert!(
        image.starts_with(&format!("{channel}-")) || image == channel,
        "the deploy image is rust:{image} while the repo pins {channel} — production would be \
         built with a compiler that neither gates nor CI ran"
    );
}
