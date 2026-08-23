//! The parts of the server worth testing from outside it.
//!
//! `vitals-web` is a binary, so anything only reachable from `main.rs` can be tested only from
//! inside the same file. Clinical scoring is a table with published boundaries — the kind of
//! thing whose tests should read as the specification and live next to it, not buried in a
//! `#[cfg(test)]` block at the bottom of a web server.

pub mod news2;
