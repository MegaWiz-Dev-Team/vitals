//! The parts of the server worth testing from outside it.
//!
//! `vitals-web` is a binary, so anything only reachable from `main.rs` can be tested only from
//! inside the same file. Clinical scoring is a table with published boundaries — the kind of
//! thing whose tests should read as the specification and live next to it, not buried in a
//! `#[cfg(test)]` block at the bottom of a web server.

// No unsafe, enforced rather than observed. Nothing in the web crate needs it, and in a codebase whose
// product is verifiability, "the compiler checked every memory access" should be a property a
// stranger can confirm from one line. (vitals-program cannot carry this: Solana's entrypoint!
// macro expands to the unsafe input deserialisation every program has.)
#![forbid(unsafe_code)]
pub mod archive;
pub mod fuel;
pub mod lang;
pub mod meter;
pub mod news2;
pub mod patient;
pub mod store;
