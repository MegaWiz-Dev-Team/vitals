//! The physiology archetypes — the small library of deterministic state machines a case is
//! compiled under — and the rule that picks one, or refuses.

use crate::embla::{Case, Vitals0};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archetype {
    SepticShock,
    HaemorrhagicShock,
    CardiogenicShock,
    NeuromuscularRespiratoryFailure,
    CnsDepressionHypoglycaemia,
    PaediatricCompensatedShock,
    HypoxicRespiratoryFailure,
}

impl Archetype {
    pub fn id(self) -> &'static str {
        todo!()
    }
    pub fn detect(_case: &Case, _v0: &Vitals0) -> Result<Archetype, String> {
        todo!()
    }
}
