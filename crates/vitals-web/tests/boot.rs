//! What the boot log says, and whether it says what it means.
//!
//! The first boot timings read `boot meter +137.6s` and were taken — by the director *and* by me —
//! as "the meter took 137.6 s". They meant "137.6 s since boot began", and the meter reads one
//! document. Two people were an hour from fixing the wrong component on the strength of a line that
//! was true and unreadable.
//!
//! So a mark carries both numbers, every mark goes through one printer, and the expensive step is
//! named and counted.

use std::path::PathBuf;

fn main_rs() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
        .expect("src/main.rs")
}

/// **A boot marker names the span it times.**
#[test]
fn a_boot_marker_names_the_span_it_times() {
    let src = main_rs();

    assert!(src.contains("let mut mark ="),
            "every boot mark goes through one printer, or two of them will drift apart and one \
             will be read as something it is not");

    // Both numbers on every line: how long since boot began, and how long this step itself took.
    // The second is the one a reader wants and the one that was missing.
    let printer = src
        .split("let mut mark =")
        .nth(1)
        .and_then(|s| s.split("};").next())
        .expect("the mark printer");
    assert!(printer.contains("booted.elapsed()"), "the running total: {printer}");
    assert!(printer.contains("since"), "and the step itself, named as such: {printer}");

    for step in ["mark(\"store\")", "mark(\"meter\")", "mark(\"chain\")", "mark(\"listening\")"] {
        assert!(src.contains(step), "{step} is not marked, so its cost cannot be seen");
    }

    // The expensive one is named and counted. 137 s of a 137.8 s boot went here and the log had no
    // word for it: a reader could see the total climb and not what had climbed.
    assert!(src.contains("sessions · restored"),
            "the session restore is the expensive step and it is not named in the log");
    assert!(src.contains("dropped {"),
            "and what it threw away is counted, because a boot that silently deletes runs is how \
             a tape for a leaf already on chain disappears");

    // The rule this whole item exists to serve, where the next person to add boot work will read it.
    assert!(src.contains("a request never waits on boot work it does not need"),
            "the invariant is not written in the boot function's own doc comment");
}
