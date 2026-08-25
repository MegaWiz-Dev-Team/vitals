//! The voice has two sources and one is a fallback. This pins how the choice is made.
//!
//! A local model plays her dialogue. If it is unreachable, a cloud model stands in — and either
//! way it is only the voice, never the proof: nothing here is anchored, so which model spoke is
//! a resilience detail, not a correctness one. What must NOT happen is the two swapping mid-
//! encounter, which would change her character in front of a candidate. The backend is therefore
//! chosen once, per session, and held.

use vitals_web::patient::Backend;

#[test]
fn the_local_model_is_preferred_when_it_is_reachable() {
    let b = Backend::choose(/* local_ok */ true, /* cloud_ok */ true);
    assert_eq!(b, Some(Backend::Local), "a reachable local model plays her dialogue");
}

#[test]
fn the_cloud_model_stands_in_when_local_is_down() {
    let b = Backend::choose(false, true);
    assert_eq!(b, Some(Backend::Cloud), "cloud takes over when the local model is unreachable");
}

#[test]
fn no_voice_when_neither_is_reachable() {
    // The app already plays without a voice and says so; it must not pretend one exists.
    assert_eq!(Backend::choose(false, false), None);
}

#[test]
fn local_wins_even_if_cloud_is_also_up() {
    // Cloud is the understudy, not a load-balanced peer. Local first, always, when it can serve.
    assert_eq!(Backend::choose(true, false), Some(Backend::Local));
    assert_eq!(Backend::choose(true, true), Some(Backend::Local));
}
