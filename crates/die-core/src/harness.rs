//! What tests stand in for the outside world with. K7's harness goes here.
#![warn(missing_docs)]
use crate::workspace::{Clipboard, RegKind, Register};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct FakeState {
    held: Option<Register>,
    count: u64,
    writes: Vec<Register>,
}

/// A clipboard in memory, shared by its clones, so a test keeps one and gives core the other.
/// Every workspace starts with its own until the host connects the real one, as it does with
/// the clock ([`Ctx::set_clipboard`]).
#[derive(Clone, Default)]
pub struct FakeClipboard(Rc<RefCell<FakeState>>);

impl FakeClipboard {
    /// Another app copies `text`.
    pub fn copy_from_outside(&self, text: &str) {
        let mut s = self.0.borrow_mut();
        s.held = Some(Register {
            kind: RegKind::Char,
            text: text.into(),
        });
        s.count += 1;
    }
    /// What it holds.
    pub fn held(&self) -> Option<Register> {
        self.0.borrow().held.clone()
    }
    /// Everything written since the last call, oldest first.
    pub fn take_writes(&self) -> Vec<Register> {
        std::mem::take(&mut self.0.borrow_mut().writes)
    }
}

impl Clipboard for FakeClipboard {
    fn read(&mut self) -> Option<Register> {
        self.held()
    }
    fn write(&mut self, r: &Register) {
        let mut s = self.0.borrow_mut();
        s.held = Some(r.clone());
        s.writes.push(r.clone());
        s.count += 1;
    }
    fn change_count(&mut self) -> u64 {
        self.0.borrow().count
    }
}
