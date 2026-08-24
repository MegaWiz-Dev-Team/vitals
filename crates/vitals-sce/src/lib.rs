//! An independent implementation of the Vitals replay semantics.
//!
//! Held to Embla's reference engine by `conformance/ep1-vectors.json` — see `tests/conformance.rs`.
//! Nothing in this crate reaches for a database, a socket, or a clock.

// No unsafe, enforced rather than observed. Nothing in the physiology automaton needs it, and in a codebase whose
// product is verifiability, "the compiler checked every memory access" should be a property a
// stranger can confirm from one line. (vitals-program cannot carry this: Solana's entrypoint!
// macro expands to the unsafe input deserialisation every program has.)
#![forbid(unsafe_code)]
pub mod text;
pub mod runtime;
pub mod schema;

pub use runtime::{render_beat, NarrativeBeat, Outcome, PatientStatus, SceState, Vitals};
pub use schema::{DebriefSpec, Expect, Sce};
