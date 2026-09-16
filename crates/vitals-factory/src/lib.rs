//! The patient factory: the job that keeps the ward's queue full.
//!
//! Every ten minutes it reads the ward, builds as many packs as the queue is short, and pushes
//! them through the ward's door. A pack is three things the ward already defines
//! (`vitals_web::ward::Pack`): a case we have, a person we invented, and her picture. Nothing here
//! writes medicine, decides who is admitted, or touches a key.
//!
//! The parts are split by what can be tested without a network: planning a pack is a pure function
//! over the catalogue, the pool, the portrait manifest and the ward's last answer; the door and
//! the tools (mflux, cwebp, gcloud, Vertex) sit behind traits with fakes.

#![forbid(unsafe_code)]

pub mod catalogue;
pub mod door;
pub mod ledger;
pub mod manifest;
pub mod need;
pub mod plan;
pub mod pool;
pub mod prompts;
pub mod tick;
pub mod tools;
