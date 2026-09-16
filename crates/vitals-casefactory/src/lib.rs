//! The case factory: `embla-cases` in, ward packs out.
//!
//! The ward — Vitals World — does not run the season's content. Its patients come from the
//! embla-cases library, compiled into the engine's scenario format by this crate: a deterministic
//! tool, never a model, so the same case at the same hash compiles to the same pack on any
//! machine. Every pack it writes has been parsed by `vitals-sce`, replayed by `vitals-replay`
//! untreated to a death and along its own management path to a win, and marked by `vitals-osce`
//! — and is still `provisional: true` until a clinician has read it.
//!
//! A case the archetype library cannot honestly model is **refused with a reason**, never forced.
#![forbid(unsafe_code)]

pub mod archetype;
pub mod embla;
