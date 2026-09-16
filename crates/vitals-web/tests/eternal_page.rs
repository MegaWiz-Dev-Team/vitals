//! The Eternal entry's own page, pinned before it is taken apart.
//!
//! The ward's shift page is being separated from the bay's, and the risk that separation carries
//! is precise: vitals.academy is the entry a judge opens, it is live, and every change made for
//! the ward is a change to it. `bay_unchanged.rs` holds the server's behaviour. This holds the
//! **page** — the shelf, the hero, the season, the footer — as bytes.
//!
//! A hash would fail on any edit and teach people to update the hash. A fixture fails the same way
//! but shows the diff, which is the difference between a guard somebody reads and a guard
//! somebody silences.
//!
//! When the lobby is deliberately changed, the fixture is regenerated in the same commit — and a
//! reviewer then sees the lobby's diff in the fixture, which is exactly what should be reviewed.

use std::path::PathBuf;

fn page() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html"),
    )
    .expect("the bay's page")
}

/// The lobby is everything from its own div to the start of the play surface.
///
/// The surface used to be written into this file and is now composed in at `<!--BAY-->`, so the
/// boundary is whichever comes first. The fixture is unchanged across that move, which is the
/// point: the extraction moved the bay out and left the season's page exactly as it was.
fn lobby_of(page: &str) -> &str {
    let start = page.find("<div id=\"lobby\">").expect("the page has a lobby");
    let end = page
        .find("<!--BAY-->")
        .or_else(|| page.find("<div class=\"app hide\" id=\"game\">"))
        .expect("the page has a bay, written in or composed in");
    &page[start..end]
}

/// The season's front page does not change while the ward's page is built.
#[test]
fn the_eternal_entry_keeps_its_own_front_page() {
    let want = include_str!("fixtures/eternal-lobby.html");
    let page = page();
    let got = lobby_of(&page);
    assert_eq!(got, want,
               "the Eternal entry's lobby changed. If that was deliberate, regenerate \
                tests/fixtures/eternal-lobby.html in the same commit so the diff is reviewed — \
                and if it was not, this is a judged page changing for a reason nobody intended");
}

/// The two pages share a bay and share none of a season.
///
/// **What this can and cannot see.** Markup is a file and is checked here. The season's *code* is
/// still inside the shared script after this step — the shelf, the hero and the record are drawn
/// by it — and taking that out is step two, scheduled in CWF_PLAN.md rather than wished for. So
/// what is asserted is what a stranger can see and click: the shift page's own markup carries no
/// lobby content, no shelf, no season list and no way into the bay's single-player door, and its
/// `#lobby` is an empty anchor rather than a page.
#[test]
fn the_shift_page_shares_the_bay_and_carries_none_of_the_season() {
    let page = page();
    let shift = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/world/shift.html"),
    )
    .expect("the ward's own page");

    // Both are the same bay: one stylesheet, one script, one surface composed in by the server.
    for both in ["/bay.css", "/bay.js", "<!--BAY-->"] {
        assert!(page.contains(both), "the Eternal entry lost {both}");
        assert!(shift.contains(both), "the shift page lost {both}");
    }

    // The Eternal entry keeps its own front page.
    assert!(page.contains("<div id=\"lobby\">"), "the season's page is the entry's own");

    // The shift page carries none of it — measured on the markup rather than on the prose, since
    // the comments in it necessarily name the things it must not contain.
    let markup: String = shift
        .split("<!--")
        .map(|chunk| chunk.split_once("-->").map(|(_, rest)| rest).unwrap_or(chunk))
        .collect();
    for never in ["Season 1", "enter the bay", "Play S1:E1", "The Resus Bay", "lb-bar", "shelf"] {
        assert!(!markup.contains(never),
                "the shift page carries {never} — a stranger who came from the globe to treat one \
                 patient must find nothing here that belongs to the single-player season");
    }
    // Its lobby is an anchor the shared script can hide, and nothing else.
    assert!(markup.contains("<div id=\"lobby\" class=\"hide\"></div>"),
            "the shift page's lobby must be empty: the script hides and shows it by name, and what \
             it holds on the entry is exactly what must not exist here");
    // And its case select holds no list. The season is not browsable from a bed.
    let sel = markup.find("<select id=\"ep\"").expect("the shift page has the case holder");
    let after = &markup[sel..(sel + 200).min(markup.len())];
    assert!(!after.contains("<option"),
            "the shift page's case holder ships with options — hers is added when the ward says \
             who she is, and a list here is the season by another name");
}
