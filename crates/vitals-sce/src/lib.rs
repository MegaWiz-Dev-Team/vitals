//! An independent implementation of the Vitals replay semantics.
//!
//! Held to Embla's reference engine by `conformance/ep1-vectors.json` — see `tests/conformance.rs`.
//! Nothing in this crate reaches for a database, a socket, or a clock.

pub mod text;
pub mod runtime;
pub mod schema;

pub use runtime::{render_beat, NarrativeBeat, Outcome, PatientStatus, SceState, Vitals};
pub use schema::{DebriefSpec, Expect, Sce};
