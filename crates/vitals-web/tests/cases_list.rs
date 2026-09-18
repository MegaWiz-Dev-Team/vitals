//! The reviewer's list of cases the ward holds — `/ward/review`.
//!
//! The page is one inline script over one public endpoint, so what it decides is decided at
//! runtime: which rows a reviewer sees, in which group, under which badge. That runs here, in node,
//! against the page that ships — the same arrangement the globe's arithmetic is held to.

use std::path::PathBuf;

/// **138 rows, 60 of them withdrawn, and the ones to read in the middle of them.**
///
/// The ward holds every case it has ever been sent, and withdrawing one is how a case is taken out
/// of placement — not deleted, because a shift already played on it still has to replay. On the
/// demo capture that meant 60 withdrawn cases sitting in the level bands in the same weight as the
/// live ones, most with Thai titles, and the 78 a reviewer has to read scattered among them.
///
/// So: withdrawn cases in one folded group at the bottom, counted in its summary, and the badge on
/// a live case reads `provisional` — the compiler's own word for "compiled, not clinically
/// reviewed" — rather than `NOT REVIEWED`, which a reviewer reads as a fault they are being shown.
#[test]
fn the_reviewers_list_folds_away_what_is_withdrawn() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::process::Command::new("node")
        .arg(here.join("tests/world/cases_logic.mjs"))
        .arg(here.join("static/world/review.html"))
        .output()
        .expect("run node");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
