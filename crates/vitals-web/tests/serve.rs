//! Which routes may leave the loop that writes — named one at a time, on purpose.

use vitals_web::serve::is_read_only;

/// **The reads, and nothing else.**
///
/// The list exists because the method cannot answer the question. `GET /api/new` starts a run;
/// `GET /api/step` advances one. Answering either off the writing loop would let two of them race
/// a pass and each other. So the rule is a list of paths that change nothing, and this test is
/// what stops the list growing by assumption.
#[test]
fn only_the_routes_that_change_nothing_leave_the_writing_loop() {
    // The board and the counters a dashboard polls once a minute, which is where the refusals
    // were measured.
    for read in ["/api/ward", "/api/usage", "/review", "/privacy", "/stats", "/start", "/start/"] {
        assert!(is_read_only("GET", read), "{read} changes nothing and may be served off the loop");
    }
    // The files those pages pull.
    for asset in ["/bay.css", "/bay.js", "/favicon.ico", "/world/favicon.svg",
                  "/world/apple-touch-icon.png", "/start/img/01-globe.jpg",
                  "/start/img/08-receipt.jpg"] {
        assert!(is_read_only("GET", asset), "{asset} is a file, and a file changes nothing");
    }

    // **Everything that writes.** A GET among them is the point: the method does not decide this.
    for writes in ["/api/new", "/api/step", "/api/say", "/api/handover", "/api/ward/take",
                   "/api/ward/tick", "/api/ward/submit", "/api/ward/left", "/api/review"] {
        assert!(!is_read_only("GET", writes),
                "{writes} changes something and stays on the loop that serves in order");
    }

    // A method that is not GET never leaves the loop, whatever the path says.
    for m in ["POST", "PUT", "DELETE", "PATCH"] {
        assert!(!is_read_only(m, "/api/ward"), "{m} /api/ward is not a read");
    }
    assert!(is_read_only("get", "/api/ward"), "and the method is matched as the server spells it");

    // A path nobody has classified keeps its old behaviour, which is to stay on the loop. A new
    // route is not a read until somebody has answered the question for it.
    for unknown in ["/", "/ward/1789663069", "/api/chain", "/some/new/thing"] {
        assert!(!is_read_only("GET", unknown),
                "{unknown} has not been classified, so it stays where it was");
    }

    // The prefix is not a licence: it reaches only what the server's own closed set answers.
    assert!(is_read_only("GET", "/start/img/anything.jpg"),
            "the prefix routes to the picture set, which refuses a name it does not hold");
    assert!(!is_read_only("GET", "/startle"), "and a path that merely begins the same way is not one");
}
