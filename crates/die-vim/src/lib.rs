//! die-vim: DIE's Vim grammar (scope §10). It depends on die-core only (D15), and
//! its behaviour is your Evil's, checked case by case by the Evil oracle
//! (`tests/oracle.rs`), except the deviations named in `tests/oracle/deviations.json`.
//!
//! Empty until V1. Every oracle case starts on the pending list
//! (`tests/oracle/pending.txt`) and leaves it as the grammar lands.

use die_core::{Extension, Registry};

/// The die-vim extension. It registers nothing yet.
#[derive(Default)]
pub struct Vim;

impl Extension for Vim {
    fn name(&self) -> &'static str {
        "die-vim"
    }
    fn register(&mut self, _reg: &mut Registry) {}
}
