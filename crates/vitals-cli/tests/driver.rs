//! The driver has to actually drive.
//!
//! It is the reference client — the thing that shows what the three instructions look like from
//! outside the browser — and it had no tests, so when the program grew an account layer the
//! driver silently stopped working. Every transaction failed with "insufficient account keys"
//! and nothing said so until somebody ran it by hand.
//!
//! `#[ignore]`, because it needs a validator and a deployed program:
//!
//! ```text
//! VITALS_PROGRAM_ID=<id> cargo test -p vitals-cli -- --ignored
//! ```

use std::process::Command;

#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn the_driver_runs_the_whole_season_without_a_surprise() {
    let program = std::env::var("VITALS_PROGRAM_ID")
        .expect("set VITALS_PROGRAM_ID to a deployed program");
    let out = Command::new(env!("CARGO_BIN_EXE_vitals-cli"))
        .env("VITALS_PROGRAM_ID", program)
        .output()
        .expect("run the driver");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(out.status.success(), "the driver exited {:?}\n{text}", out.status.code());

    // The driver prints this when a transaction fails in a way it did not predict. A predicted
    // refusal — claiming a level the arithmetic does not support — is the demonstration, not a
    // failure, and does not print it.
    assert!(
        !text.contains("unexpected failure"),
        "the driver could not talk to the program:\n{text}"
    );

    // And it has to get all the way to the end, or "no unexpected failure" just means it stopped.
    assert!(text.contains("anchor"), "the driver never anchored anything:\n{text}");
}
