//! What a figure shows of its data: its domains, what's hidden, the reference, pinned nodes;
//! and the undo of changes to it, one entry a discrete change or gesture.

use super::cursor::Cursor;
use crate::data::SeriesKey;
use std::collections::{BTreeMap, BTreeSet};

/// What a figure shows of its data.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct View {
    /// The x domain; None fits the data.
    pub x: Option<(f64, f64)>,
    /// The y domain; None fits the data.
    pub y: Option<(f64, f64)>,
    /// The series hidden.
    pub hidden: BTreeSet<SeriesKey>,
    /// The one series shown alone.
    pub only: Option<SeriesKey>,
    /// The reference the card compares with.
    pub reference: Option<Cursor>,
    /// Following live data's end (`G`).
    pub follow: bool,
    /// Graph nodes moved by hand, by id.
    pub pinned: BTreeMap<Box<str>, (f64, f64)>,
}

/// The undo and redo of a view's changes.
#[derive(Debug, Default)]
pub struct ViewStack {
    _undo: Vec<View>,
    _redo: Vec<View>,
}

impl ViewStack {
    /// Keeps the view as it was before a change, and forgets what could be redone.
    pub fn push(&mut self, _v: View) {
        todo!("P2-surface: ViewStack::push")
    }

    /// The view before the last change, keeping `now` to redo.
    pub fn undo(&mut self, _now: &View) -> Option<View> {
        todo!("P2-surface: ViewStack::undo")
    }

    /// The view undone last, keeping `now` to undo again.
    pub fn redo(&mut self, _now: &View) -> Option<View> {
        todo!("P2-surface: ViewStack::redo")
    }
}
