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

    for step in ["mark(\"store\"", "mark(\"meter\"", "mark(\"chain\"", "mark(\"listening\""] {
        assert!(src.contains(step), "{step} is not marked, so its cost cannot be seen");
    }

    // The expensive one is named and counted. 137 s of a 137.8 s boot went here and the log had no
    // word for it: a reader could see the total climb and not what had climbed.
    assert!(src.contains(r#"mark("sessions""#),
            "the session restore is the expensive step and it is not named in the log");
    // The counts ride in the note, which the printer puts *after* the timings. Asserted on the
    // shape rather than on the words: what is counted changed when the boot stopped replaying
    // sessions — restored/dropped became a census of what is in the store — and the rule that
    // survives both is where the numbers sit, because a reader who meets them first ties the
    // seconds to the wrong one.
    let sessions_mark = src
        .split(r#"mark("sessions""#)
        .nth(1)
        .and_then(|s| s.split(");").next())
        .expect("the sessions mark");
    assert!(sessions_mark.contains(r#"" · "#) || sessions_mark.contains("\" · {}\""),
            "the counts have to follow the timings, not lead them: {sessions_mark}");
    assert!(src.contains("dropped {"),
            "and what it threw away is counted, because a boot that silently deletes runs is how \
             a tape for a leaf already on chain disappears");

    // The two that shared the sessions span and were read as it. `boot sessions +30.2s` was taken —
    // reasonably — as the restore, and the restore had already been removed: what was in that span
    // was the sweep and the tape repair. The same lesson as the meter, one level down, so each gets
    // its own mark and its own counts before anybody reasons about where 30 s went.
    // Measured on staging 00062, once the sweep and the repair each had their own mark: repair
    // 38.3 s of a 41.6 s boot (26 patients, one signature listing each) and the sweep 2.8 s. Both
    // are work for nobody who is knocking, and a request arriving during a start waits for all of
    // it — 22.2 s on that deploy. So both moved behind the listener, to the ticker's first pass.
    //
    // Read against the straight-line boot, which is everything before the refill thread is
    // spawned. Textual order is not execution order once a thread exists — the thread's body is
    // written above `mark("listening")` and runs after it — and the first version of this test
    // compared raw indices and failed on its own premise.
    let refill = src.find("The refill, on its own thread").expect("the refill thread");
    let straight_line = &src[..refill];
    for slow in [".sweep(", "repair_tapes(", "ward_chain::tick("] {
        assert!(!straight_line.contains(slow),
                "boot does `{slow}` on the path a first request waits for");
    }

    // **Repair before sweep, and it is not a preference.** The repair recovers a lost tape *from*
    // the stored runs; the sweep deletes stored runs older than a day. Sweeping first can delete
    // the only copy of a tape for a leaf already on chain — the thing the repair exists to put
    // back. Boot did them in that order until 20 ก.ย., and moving them fixed it.
    let tick = src.find("ward_chain::tick(").expect("the ticker's pass");
    let sweep = src.find(".sweep(").expect("the sweep");
    assert!(sweep > tick,
            "the sweep must run after the repair has had its go, or it deletes the evidence");

    // How many patients the pass walked, and how the time was spread across them, used to be
    // asserted here by grepping this file for "patients checked". That sentence now belongs to
    // `ward_chain::slow_pass_note`, where `tests/ward_chain.rs` drives the real function over both
    // shapes it exists to tell apart. A behavioural test replaced a source-text one, so the
    // assertion is gone from here rather than rewritten to grep a different file.

    // The rule this whole item exists to serve, where the next person to add boot work will read it.
    assert!(src.to_lowercase().contains("a request never waits on boot work it does not need"),
            "the invariant is not written in the boot function's own doc comment");
}
