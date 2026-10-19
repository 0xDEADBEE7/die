//! Stand-in for die-term (the libghostty-vt engine is S2's concern; not linked here).
pub fn term() -> usize { die_core::version().len() }
