//! Undo and redo over an operation log: one entry per gesture, command or label session, each
//! the elements as they were and as they are, with where they sit. Session state, never the
//! file's.

use crate::model::{Board, Element, Id};

/// How many entries the log keeps; the oldest go first.
pub const KEEP: usize = 1000;

/// One undoable change: `before` are the elements it replaced or removed, `after` those it put
/// in their place or added, each with its index on the board.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Entry {
    /// What there was.
    pub before: Vec<(usize, Element)>,
    /// What there is.
    pub after: Vec<(usize, Element)>,
}

impl Entry {
    /// An element added at `index`.
    pub fn added(index: usize, el: Element) -> Entry {
        Entry {
            before: vec![],
            after: vec![(index, el)],
        }
    }
    /// Elements removed, each from its index.
    pub fn removed(gone: Vec<(usize, Element)>) -> Entry {
        Entry {
            before: gone,
            after: vec![],
        }
    }
    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty()
    }
    /// The ids `after` holds.
    pub fn after_ids(&self) -> Vec<Id> {
        self.after.iter().map(|(_, e)| e.id).collect()
    }
}

/// The index an element at `current` had before `gone`, each at the index it had on the board
/// as it was, were taken out one after another: what an entry records, so undo puts everything
/// back in its place whatever order it went in.
pub fn original_index(current: usize, gone: &[(usize, Element)]) -> usize {
    let mut earlier: Vec<usize> = gone.iter().map(|(i, _)| *i).collect();
    earlier.sort_unstable();
    let mut index = current;
    for g in earlier {
        if g <= index {
            index += 1;
        }
    }
    index
}

/// The log, and where in it the board stands.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    at: usize,
}

/// Take `remove` off the board and put `insert` on it, at their indices, lowest first, so each
/// lands where it was when the entry was made.
fn apply(board: &mut Board, remove: &[(usize, Element)], insert: &[(usize, Element)]) {
    for (_, el) in remove {
        board.remove(el.id);
    }
    let mut back: Vec<&(usize, Element)> = insert.iter().collect();
    back.sort_by_key(|(i, _)| *i);
    for (i, el) in back {
        board.insert(*i, el.clone());
    }
}

impl History {
    /// Record a change that's been made. Anything undone is forgotten.
    pub fn push(&mut self, e: Entry) {
        if e.is_empty() {
            return;
        }
        self.entries.truncate(self.at);
        self.entries.push(e);
        if self.entries.len() > KEEP {
            self.entries.remove(0);
        }
        self.at = self.entries.len();
    }
    /// Take back the last change, if there is one, and say what it was.
    pub fn undo(&mut self, board: &mut Board) -> Option<&Entry> {
        self.at = self.at.checked_sub(1)?;
        let e = &self.entries[self.at];
        apply(board, &e.after, &e.before);
        Some(e)
    }
    /// Do the last undone change again, if there is one, and say what it was.
    pub fn redo(&mut self, board: &mut Board) -> Option<&Entry> {
        let e = self.entries.get(self.at)?;
        apply(board, &e.before, &e.after);
        self.at += 1;
        Some(e)
    }
    /// How many changes can be taken back.
    pub fn undoable(&self) -> usize {
        self.at
    }
    /// How many undone changes can be done again.
    pub fn redoable(&self) -> usize {
        self.entries.len() - self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Shape};

    fn label(board: &mut Board, text: &str) -> (usize, Id) {
        board.add(
            Shape::Label {
                at: Point::default(),
                text: text.into(),
            },
            1,
            1,
        )
    }

    fn texts(board: &Board) -> Vec<String> {
        board
            .elements
            .iter()
            .map(|e| match &e.shape {
                Shape::Label { text, .. } => text.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn undo_and_redo_put_elements_back_where_they_were() {
        let mut board = Board::default();
        let mut h = History::default();
        for t in ["a", "b", "c"] {
            let (i, id) = label(&mut board, t);
            h.push(Entry::added(i, board.get(id).unwrap().clone()));
        }
        let mut gone = vec![];
        for id in [1, 3] {
            let (i, el) = board.remove(id).unwrap();
            let o = original_index(i, &gone);
            gone.push((o, el));
        }
        h.push(Entry::removed(gone));
        assert_eq!(texts(&board), ["b"]);
        h.undo(&mut board);
        assert_eq!(texts(&board), ["a", "b", "c"]);
        h.undo(&mut board);
        assert_eq!(texts(&board), ["a", "b"]);
        h.redo(&mut board);
        h.redo(&mut board);
        assert_eq!(texts(&board), ["b"]);
        assert_eq!((h.undoable(), h.redoable()), (4, 0));
        assert!(h.redo(&mut board).is_none());
        // A new change after an undo forgets what was undone.
        h.undo(&mut board);
        let (i, id) = label(&mut board, "d");
        h.push(Entry::added(i, board.get(id).unwrap().clone()));
        assert_eq!(h.redoable(), 0);
        assert_eq!(texts(&board), ["a", "b", "c", "d"]);
        assert_eq!(board.fresh_id(), 5, "ids are never reused");
    }

    #[test]
    fn removals_one_after_another_record_the_indices_they_had() {
        // [a b c d]: take c (2), then a (0), then d (now 1, was 3).
        let mut board = Board::default();
        for t in ["a", "b", "c", "d"] {
            label(&mut board, t);
        }
        let mut gone = vec![];
        for id in [3, 1, 4] {
            let (i, el) = board.remove(id).unwrap();
            let o = original_index(i, &gone);
            gone.push((o, el));
        }
        let indices: Vec<usize> = gone.iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, [2, 0, 3]);
        let mut h = History::default();
        h.push(Entry::removed(gone));
        h.undo(&mut board);
        assert_eq!(texts(&board), ["a", "b", "c", "d"]);
        h.redo(&mut board);
        assert_eq!(texts(&board), ["b"]);
    }
}
