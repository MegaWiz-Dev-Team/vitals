//! **A third door: the ward is built, the patients are here, and nobody plays yet.**
//!
//! Founder's ruling, 18 ก.ย. Production has been sitting behind a closed door with an empty board
//! and a holding sentence, which means the week before the fair is spent proving nothing: the
//! factory cannot fill it, so nobody can see the patients it would fill it with, and the first
//! thing a judge would meet is a page about a ward rather than a ward.
//!
//! `preview` is the state in between. The factory's doors take packs, so production's queue fills
//! and the globe can show who is waiting and where they are from. The ticker admits nobody, and
//! nothing a stranger can press puts a hand on a patient: take, declare, anchor, release and the
//! leaving beacon all answer in one sentence, and it is the sentence a person standing at a bed
//! needs — "the ward opens soon — nobody plays yet".
//!
//! Everything about this file is about the door as a word and as a set of permissions. What the
//! *page* does with a waiting patient is in `globe_logic.mjs` and `waiting_page.rs`.

use vitals_web::ward_chain::{door_from, Door};

#[test]
fn the_door_has_three_words_and_everything_else_is_closed() {
    assert_eq!(door_from(Some("open")), Door::Open);
    assert_eq!(door_from(Some("preview")), Door::Preview);
    assert_eq!(door_from(Some("closed")), Door::Closed);

    // The spellings a deploy script or a person actually produces.
    assert_eq!(door_from(Some(" Preview ")), Door::Preview, "trimmed and case-blind");
    assert_eq!(door_from(Some("PREVIEW")), Door::Preview);
    assert_eq!(door_from(Some("Open")), Door::Open);

    // Every ambiguity resolves closed, and the asymmetry is the whole argument: a ward that stays
    // shut an hour too long costs an hour, and a ward that opens by accident is strangers treating
    // patients nobody chose to release.
    for wrong in ["", " ", "previewish", "preview mode", "yes", "true", "1", "opening", "open-ish"] {
        assert_eq!(door_from(Some(wrong)), Door::Closed, "{wrong:?} is not a door");
    }
    assert_eq!(door_from(None), Door::Closed, "unset is shut");
}

#[test]
fn what_each_door_allows() {
    // The factory: packs may arrive whenever the ward is being filled, which is both of the states
    // that are not shut. A closed ward refuses them at the door, as it always has.
    assert!(Door::Open.takes_packs());
    assert!(Door::Preview.takes_packs(), "the whole point: production's queue can be filled");
    assert!(!Door::Closed.takes_packs());

    // The ticker: only an open ward admits. In preview the queue grows and the beds stay empty.
    assert!(Door::Open.admits());
    assert!(!Door::Preview.admits(), "a bed filled in preview is a patient nobody may treat");
    assert!(!Door::Closed.admits());

    // A stranger's hands: only an open ward.
    assert!(Door::Open.plays());
    assert!(!Door::Preview.plays());
    assert!(!Door::Closed.plays());

    // The word on the payload, which is what every page branches on.
    assert_eq!(Door::Open.word(), "open");
    assert_eq!(Door::Preview.word(), "preview");
    assert_eq!(Door::Closed.word(), "closed");

    // And the sentence a stranger gets for pressing something in preview. One sentence, twelve
    // words or fewer, and it says what will change rather than what is forbidden.
    let said = Door::Preview.refusal();
    assert_eq!(said, "the ward opens soon — nobody plays yet");
    assert!(said.split_whitespace().count() <= 12);
}
