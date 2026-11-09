//! Collapsible sections, as the Emacs client's `orchid-collapsible` made them: a stub line, then a
//! detail that starts with the stub's newline and is hidden until you expand it. Here the detail
//! is under a `Deco::Hide`, and a `Deco::Tag` over stub and detail says which section a position
//! is in, so `TAB` anywhere in one toggles it, the innermost where they nest (a result's files).
//! A lazy section's detail is a lone newline until its first expand, which makes the rest once,
//! as Emacs's lazy sections did. Every section starts collapsed.
//!
//! Text goes in through a [`Build`]: the text, with the faces, read-only ranges and sections on
//! parts of it, put in with one silent edit and then decorated, since core has no styled
//! insertion yet. A [`Mark`] is a marker that can be moved, shared with what a restart keeps.
//!
//! A section made from an event has a [`Key`], its event's id and where it is among the event's
//! sections, which names it again when a restart builds the chat anew: the sections you had
//! expanded are expanded again ([`Sections::expanded`], [`Sections::expand`]), and a window's
//! place goes back to the same text ([`Sections::anchor`], [`Sections::resolve`]).

use crate::parse::{self, Piece, Section, Shown};
use crate::store::Event;
use die_core::{Deco, DecorationId, Gravity, MarkerId, Origin, Result, Reveal, TextBuf};
use jiff::tz::TimeZone;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The command a search runs when it lands in a collapsed section, which expands it.
pub const REVEAL: &str = "orchid-chat-reveal";

// ------------------------------------------------------------------ marks

/// A position in a chat that moves with edits and that the chat moves itself, as Emacs's markers
/// were set: text put in at it goes after it. Its clones share it, so what a restart keeps can
/// read where the input starts.
#[derive(Clone, Default, Debug)]
pub struct Mark(Rc<Cell<Option<MarkerId>>>);

impl Mark {
    /// Where it is in `t`, or None if it was never set.
    pub fn pos(&self, t: &TextBuf) -> Option<usize> {
        t.marker(self.0.get()?)
    }

    /// Where it is, or the end of `t` if it was never set.
    pub fn at(&self, t: &TextBuf) -> usize {
        self.pos(t).unwrap_or(t.len())
    }

    /// Put it at `pos`.
    pub fn set(&self, t: &mut TextBuf, pos: usize) -> Result {
        if let Some(old) = self.0.get() {
            t.remove_marker(old);
        }
        self.0.set(Some(t.add_marker(pos, Gravity::Left)?));
        Ok(())
    }
}

// ------------------------------------------------------------------ building text

/// What names a section over a restart: its event's id, then where it is among the event's
/// sections, as `ID#0`, and inside it, as a result's second file, `ID#0.1`. Sections from no
/// event, as your message's stub before orchid has it, have none.
pub type Key = String;

/// The keys of the sections expanded now, shared with what a restart keeps.
pub type Open = Rc<RefCell<BTreeSet<Key>>>;

/// Where a position is, in terms a chat built anew still has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// So far into the section with this key.
    In(Key, usize),
    /// So far past the end of the section with this key, the last before it.
    After(Key, usize),
    /// So far from the start, before any section: the header.
    Top(usize),
}

impl Anchor {
    /// As JSON, for what a restart keeps.
    pub fn json(&self) -> serde_json::Value {
        match self {
            Anchor::In(k, d) => serde_json::json!(["in", k, d]),
            Anchor::After(k, d) => serde_json::json!(["after", k, d]),
            Anchor::Top(d) => serde_json::json!(["top", d]),
        }
    }

    /// From [`Anchor::json`]'s.
    pub fn from_json(v: &serde_json::Value) -> Option<Anchor> {
        let kind = v.get(0)?.as_str()?;
        let last = |i| v.get(i)?.as_u64().map(|d| d as usize);
        let key = || Some(v.get(1)?.as_str()?.to_string());
        match kind {
            "in" => Some(Anchor::In(key()?, last(2)?)),
            "after" => Some(Anchor::After(key()?, last(2)?)),
            "top" => Some(Anchor::Top(last(1)?)),
            _ => None,
        }
    }
}

/// What a lazy section's detail is made from on its first expand.
#[derive(Clone, Debug)]
struct Lazy {
    event: Rc<Event>,
    index: usize,
}

/// A section in a [`Build`], at offsets in its text.
#[derive(Debug)]
struct New {
    start: usize,
    stub_end: usize,
    end: usize,
    lazy: Option<Lazy>,
    key: Option<Key>,
}

/// Text to put in a chat in one edit, with what decorates parts of it.
#[derive(Default, Debug)]
pub struct Build {
    text: String,
    faces: Vec<(Range<usize>, &'static str)>,
    read_only: Vec<Range<usize>>,
    sections: Vec<New>,
}

impl Build {
    /// Nothing yet.
    pub fn new() -> Build {
        Build::default()
    }

    /// Its length in bytes, which is where the next text goes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether it has no text.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Its text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Plain text.
    pub fn push(&mut self, s: &str) -> &mut Build {
        self.text.push_str(s);
        self
    }

    /// Text in `face`.
    pub fn styled(&mut self, s: &str, face: &'static str) -> &mut Build {
        let start = self.len();
        self.text.push_str(s);
        if !s.is_empty() {
            self.faces.push((start..self.len(), face));
        }
        self
    }

    /// Text you can't edit, as Emacs's `read-only` text with `rear-nonsticky`: typing at either
    /// edge is fine. In `face`, if any.
    pub fn read_only(&mut self, s: &str, face: Option<&'static str>) -> &mut Build {
        let start = self.len();
        match face {
            Some(f) => self.styled(s, f),
            None => self.push(s),
        };
        self.read_only.push(start..self.len());
        self
    }

    /// A detail's pieces: text in its faces, and sections inside it, collapsed.
    pub fn pieces(&mut self, pieces: &[Piece]) -> &mut Build {
        self.pieces_in(pieces, None)
    }

    /// [`Build::pieces`], inside the section keyed `parent`, whose sections theirs extend.
    fn pieces_in(&mut self, pieces: &[Piece], parent: Option<&str>) -> &mut Build {
        let mut n = 0;
        for p in pieces {
            match p {
                Piece::Text(s, Some(f)) => self.styled(s, f),
                Piece::Text(s, None) => self.push(s),
                Piece::Section(s, inner) => {
                    let key = parent.map(|k| format!("{k}.{n}"));
                    n += 1;
                    self.section(s, Some(inner), None, key)
                }
            };
        }
        self
    }

    /// A collapsed section: its stub, then the detail under it, `pieces` now, or else made from
    /// `lazy` on the first expand.
    fn section(
        &mut self,
        s: &Section,
        pieces: Option<&[Piece]>,
        lazy: Option<Lazy>,
        key: Option<Key>,
    ) -> &mut Build {
        let start = self.len();
        // A stub is one line, so its end is the first newline after its start.
        self.styled(&s.stub.replace('\n', " "), s.face);
        let stub_end = self.len();
        self.push("\n");
        if let Some(p) = pieces {
            self.pieces_in(p, key.as_deref());
        }
        let end = self.len();
        self.sections.push(New {
            start,
            stub_end,
            end,
            lazy,
            key,
        });
        self
    }

    /// One collapsed section with this stub and the detail `pieces`, made now: your own message.
    pub fn eager(&mut self, s: &Section, pieces: &[Piece]) -> &mut Build {
        self.section(s, Some(pieces), None, None)
    }

    /// An event as the chat shows it, and the newline after it, as the Emacs client's
    /// `orchid-chat-insert-log-line` put it in: one event's sections joined by newlines, each
    /// detail eager or lazy as [`parse::shown`] says. `restoring` shows your own messages. False,
    /// and nothing added, for an event that shows nothing.
    pub fn event(&mut self, event: &Rc<Event>, restoring: bool, zone: &TimeZone) -> bool {
        match parse::shown(event, restoring, zone) {
            Shown::Nothing => return false,
            Shown::Raw(s) => {
                self.push(&s);
            }
            Shown::Sections(list) => {
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        self.push("\n");
                    }
                    let key = event.event_id.as_ref().map(|id| format!("{id}#{i}"));
                    if s.lazy {
                        let lazy = Lazy {
                            event: event.clone(),
                            index: i,
                        };
                        self.section(s, None, Some(lazy), key);
                    } else {
                        let pieces = parse::detail(event, i);
                        self.section(s, Some(&pieces), None, key);
                    }
                }
            }
        }
        self.push("\n");
        true
    }
}

// ------------------------------------------------------------------ the sections

/// What a section's `Deco::Tag` holds: its number in its chat's [`Sections`], and its key, so
/// what a restart keeps can be read from the text alone.
#[derive(Debug)]
struct Tag(u64, Option<Key>);

#[derive(Debug)]
struct Entry {
    /// Over the stub and the detail: where the section is.
    tag: DecorationId,
    /// Over the detail, while it's collapsed.
    hide: Option<DecorationId>,
    /// What makes the detail, until its first expand.
    lazy: Option<Lazy>,
    key: Option<Key>,
}

/// A chat's sections, by number.
#[derive(Default, Debug)]
pub struct Sections {
    by_id: HashMap<u64, Entry>,
    next: u64,
    by_key: HashMap<Key, u64>,
    open: Open,
}

/// What a toggle did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggled {
    /// The detail shows now.
    Expanded,
    /// It's hidden now.
    Collapsed,
}

fn hide() -> Deco {
    Deco::Hide {
        replacement: None,
        reveal: Reveal::Command(REVEAL.into()),
    }
}

impl Sections {
    /// Replace `range` of `t` with `b`'s text, as an edit of the extension's own that undo
    /// doesn't record, then decorate it.
    pub fn place(&mut self, t: &mut TextBuf, range: Range<usize>, b: Build) -> Result {
        let at = range.start;
        let moved = |r: &Range<usize>| at + r.start..at + r.end;
        t.tx(Origin::Extension)
            .silent()
            .replace(range, b.text)
            .commit()?;
        for (r, face) in &b.faces {
            t.decorate(moved(r), Deco::Face(face))?;
        }
        for r in &b.read_only {
            t.decorate(moved(r), Deco::ReadOnly)?;
        }
        for s in b.sections {
            let id = self.next;
            self.next += 1;
            let tag = Deco::Tag(Rc::new(Tag(id, s.key.clone())));
            let tag = t.decorate(at + s.start..at + s.end, tag)?;
            let hidden = t.decorate(at + s.stub_end..at + s.end, hide())?;
            if s.lazy.is_some() {
                count(|st| st.created += 1);
            }
            if let Some(k) = &s.key {
                self.by_key.insert(k.clone(), id);
            }
            let entry = Entry {
                tag,
                hide: Some(hidden),
                lazy: s.lazy,
                key: s.key,
            };
            self.by_id.insert(id, entry);
        }
        Ok(())
    }

    /// How many sections there are.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The innermost section at `pos`: its stub or its detail. At a line's end, the section the
    /// line ends, which is where insert state's cursor sits after a stub.
    pub fn at(&self, t: &TextBuf, pos: usize) -> Option<u64> {
        let find = |p: usize| {
            let tags = t.decorations_at(p).filter_map(|(_, d)| match &d.deco {
                Deco::Tag(v) => {
                    let Tag(id, _) = v.downcast_ref::<Tag>()?;
                    self.by_id.contains_key(id).then_some((d.range.len(), *id))
                }
                _ => None,
            });
            tags.min().map(|(_, id)| id)
        };
        let ends_line = pos >= t.len() || t.rope().byte(pos) == b'\n';
        find(pos).or_else(|| (pos > 0 && ends_line).then(|| find(pos - 1)).flatten())
    }

    /// Where a section is: its stub's start, its stub's end, and the end of its detail.
    pub fn range(&self, t: &TextBuf, id: u64) -> Option<(usize, usize, usize)> {
        let r = t.decoration(self.by_id.get(&id)?.tag)?.range.clone();
        let rope = t.rope();
        let mut stub_end = r.start;
        while stub_end < r.end && rope.byte(stub_end) != b'\n' {
            stub_end += 1;
        }
        Some((r.start, stub_end, r.end))
    }

    /// Whether a section is collapsed.
    pub fn collapsed(&self, id: u64) -> bool {
        self.by_id.get(&id).is_some_and(|e| e.hide.is_some())
    }

    /// Expand a collapsed section, making a lazy one's detail the first time, or collapse an
    /// expanded one. None if there's no such section.
    pub fn toggle(&mut self, t: &mut TextBuf, id: u64) -> Result<Option<Toggled>> {
        let Some((_, stub_end, end)) = self.range(t, id) else {
            return Ok(None);
        };
        let entry = self.by_id.get_mut(&id).expect("a section with a range");
        let key = entry.key.clone();
        if entry.hide.is_none() {
            entry.hide = Some(t.decorate(stub_end..end, hide())?);
            if let Some(k) = &key {
                self.open.borrow_mut().remove(k);
            }
            return Ok(Some(Toggled::Collapsed));
        }
        if let Some(h) = entry.hide.take() {
            t.undecorate(h);
        }
        if let Some(k) = &key {
            self.open.borrow_mut().insert(k.clone());
        }
        if let Some(lazy) = entry.lazy.take() {
            let began = Instant::now();
            let mut b = Build::new();
            let detail = parse::detail(&lazy.event, lazy.index);
            b.push("\n").pieces_in(&detail, key.as_deref());
            // The lone newline that stood in for it goes, and the tag's end follows.
            self.place(t, stub_end..end, b)?;
            let took = began.elapsed();
            count(|st| {
                st.materialized += 1;
                st.time += took;
            });
        }
        Ok(Some(Toggled::Expanded))
    }

    /// The keys of the sections expanded now, which follow each toggle.
    pub fn expanded(&self) -> Open {
        self.open.clone()
    }

    /// Expand the sections with these keys, a section before those inside it, as a restart
    /// brings back what was expanded. Keys no section has any more are passed over.
    pub fn expand(&mut self, t: &mut TextBuf, keys: &[Key]) -> Result {
        let mut keys: Vec<&Key> = keys.iter().collect();
        let depth = |k: &&Key| k.rsplit('#').next().map_or(0, |p| p.matches('.').count());
        keys.sort_by_key(depth);
        for k in keys {
            if let Some(id) = self.by_key.get(k).copied()
                && self.collapsed(id)
            {
                self.toggle(t, id)?;
            }
        }
        Ok(())
    }

    /// Where `pos` is in a chat's text, by the keyed section it's in, the innermost, or else the
    /// last one before it, or else from the start. It reads the text alone, as what a restart
    /// keeps is worked out with nothing else.
    pub fn anchor(t: &TextBuf, pos: usize) -> Anchor {
        let mut inside: Option<(usize, &Key, usize)> = None;
        let mut before: Option<(usize, usize, &Key)> = None;
        for (_, d) in t.decorations() {
            let Deco::Tag(v) = &d.deco else { continue };
            let Some(Tag(_, Some(key))) = v.downcast_ref::<Tag>() else {
                continue;
            };
            let r = &d.range;
            if r.contains(&pos) {
                if inside.is_none_or(|(len, _, _)| r.len() < len) {
                    inside = Some((r.len(), key, pos - r.start));
                }
            } else if r.end <= pos
                && before.is_none_or(|(end, len, _)| (r.end, r.len()) > (end, len))
            {
                before = Some((r.end, r.len(), key));
            }
        }
        match (inside, before) {
            (Some((_, k, d)), _) => Anchor::In(k.clone(), d),
            (None, Some((end, _, k))) => Anchor::After(k.clone(), pos - end),
            (None, None) => Anchor::Top(pos),
        }
    }

    /// Where `anchor` is in `t`, if its section is there, at a character's start.
    pub fn resolve(&self, t: &TextBuf, anchor: &Anchor) -> Option<usize> {
        let range = |k: &Key| self.range(t, *self.by_key.get(k)?);
        let pos = match anchor {
            Anchor::In(k, d) => {
                let (start, _, end) = range(k)?;
                (start + d).min(end)
            }
            Anchor::After(k, d) => range(k)?.2 + d,
            Anchor::Top(d) => *d,
        };
        let mut pos = pos.min(t.len());
        while !t.is_boundary(pos) {
            pos -= 1;
        }
        Some(pos)
    }

    /// A search landed in the hidden text `hide` hides: expand its section. False if it isn't
    /// one of these.
    pub fn reveal(&mut self, t: &mut TextBuf, hide: DecorationId) -> Result<bool> {
        let found = self.by_id.iter().find(|(_, e)| e.hide == Some(hide));
        let Some(id) = found.map(|(id, _)| *id) else {
            return Ok(false);
        };
        Ok(self.toggle(t, id)?.is_some())
    }
}

// ------------------------------------------------------------------ the stats

/// How lazy details have fared, as `orchid-collapsible-report-stats` reports them: for every chat
/// since DIE started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Lazy sections made.
    pub created: u64,
    /// Lazy details made, on a first expand.
    pub materialized: u64,
    /// The time making them took.
    pub time: Duration,
}

thread_local! {
    // Extension code runs on the main thread alone, so its stats are that thread's.
    static STATS: Cell<Stats> = Cell::new(Stats::default());
}

fn count(f: impl FnOnce(&mut Stats)) {
    STATS.with(|s| {
        let mut st = s.get();
        f(&mut st);
        s.set(st);
    });
}

/// The stats so far.
pub fn stats() -> Stats {
    STATS.with(Cell::get)
}

/// The stats as the Emacs client said them.
pub fn report(st: Stats) -> String {
    let share = match st.created {
        0 => 0.0,
        n => 100.0 * st.materialized as f64 / n as f64,
    };
    let total = st.time.as_secs_f64();
    let average = match st.materialized {
        0 => 0.0,
        n => total / n as f64,
    };
    format!(
        "Lazy Formatting Stats:\n  Created: {}\n  Materialized: {} ({share:.1}%)\n  Total time: {total:.3}s\n  Avg time: {average:.3}s\n",
        st.created, st.materialized
    )
}

#[cfg(test)]
mod tests {
    //! The Emacs client's `orchid-collapsible-test.el`, on a text buffer alone.
    use super::*;
    use crate::parse::face;

    fn stub(s: &str) -> Section {
        Section {
            stub: s.into(),
            face: face::STUB,
            lazy: false,
        }
    }

    /// The hidden ranges, as (start, end).
    fn hidden(t: &TextBuf) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = t
            .decorations()
            .filter(|(_, d)| matches!(d.deco, Deco::Hide { .. }))
            .map(|(_, d)| (d.range.start, d.range.end))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_section_starts_collapsed_and_toggles() {
        let mut t = TextBuf::new("");
        let mut s = Sections::default();
        let mut b = Build::new();
        b.eager(&stub("stub"), &[Piece::Text("detail text".into(), None)])
            .push("\n");
        s.place(&mut t, 0..0, b).unwrap();
        assert_eq!(t.rope().to_string(), "stub\ndetail text\n");
        assert_eq!(hidden(&t), [(4, 16)], "the stub's newline and the detail");
        let id = s.at(&t, 0).unwrap();
        assert!(s.collapsed(id));
        assert_eq!(s.at(&t, 10), Some(id), "the detail is the section's too");
        assert_eq!(s.at(&t, 16), Some(id), "the end of the line it ends");
        assert_eq!(s.toggle(&mut t, id).unwrap(), Some(Toggled::Expanded));
        assert!(hidden(&t).is_empty());
        assert_eq!(s.toggle(&mut t, id).unwrap(), Some(Toggled::Collapsed));
        assert_eq!(hidden(&t), [(4, 16)]);
        // Faces: the stub's.
        let faces: Vec<_> = t
            .decorations()
            .filter_map(|(_, d)| match d.deco {
                Deco::Face(f) => Some((d.range.clone(), f)),
                _ => None,
            })
            .collect();
        assert_eq!(faces, [(0..4, face::STUB)]);
    }

    #[test]
    fn outside_a_section_there_is_none() {
        let mut t = TextBuf::new("plain text\n");
        let s = Sections::default();
        assert_eq!(s.at(&t, 0), None);
        let mut s = Sections::default();
        let mut b = Build::new();
        b.push("before\n").eager(&stub("s"), &[]).push("\nafter\n");
        s.place(&mut t, 0..0, b).unwrap();
        assert_eq!(s.at(&t, 0), None);
        assert!(s.at(&t, 7).is_some());
        assert_eq!(s.at(&t, 10), None, "after");
    }

    #[test]
    fn a_lazy_detail_is_made_once_on_the_first_expand() {
        let event = Rc::new(
            Event::parse(r#"{"type":"message","role":"assistant","content":"lazy detail"}"#)
                .unwrap(),
        );
        let mut t = TextBuf::new("");
        let mut s = Sections::default();
        let mut b = Build::new();
        assert!(b.event(&event, false, &TimeZone::UTC));
        let before = stats();
        s.place(&mut t, 0..0, b).unwrap();
        assert_eq!(stats().created, before.created + 1);
        let stub_len = t.rope().to_string().find('\n').unwrap();
        assert_eq!(
            t.len(),
            stub_len + 2,
            "a placeholder newline, and the event's"
        );
        let id = s.at(&t, 0).unwrap();
        s.toggle(&mut t, id).unwrap();
        let text = t.rope().to_string();
        assert!(text.ends_with("]\nAssistant: lazy detail\n"), "{text:?}");
        assert_eq!(stats().materialized, before.materialized + 1);
        // Collapsed again, it hides all of it; expanded again, nothing is made.
        s.toggle(&mut t, id).unwrap();
        assert_eq!(hidden(&t), [(stub_len, t.len() - 1)]);
        s.toggle(&mut t, id).unwrap();
        assert_eq!(t.rope().to_string(), text);
        assert_eq!(stats().materialized, before.materialized + 1);
    }

    #[test]
    fn sections_nest_and_the_innermost_toggles() {
        let mut t = TextBuf::new("");
        let mut s = Sections::default();
        let inner = [Piece::Section(
            stub("[a.rs]"),
            vec![Piece::Text("fn a() {}\n".into(), None)],
        )];
        let mut b = Build::new();
        b.eager(&stub("[rslt]"), &inner).push("\n");
        s.place(&mut t, 0..0, b).unwrap();
        assert_eq!(t.rope().to_string(), "[rslt]\n[a.rs]\nfn a() {}\n\n");
        let outer = s.at(&t, 0).unwrap();
        let file = s.at(&t, 8).unwrap();
        assert_ne!(outer, file);
        assert_eq!(s.len(), 2);
        s.toggle(&mut t, outer).unwrap();
        assert_eq!(hidden(&t), [(13, 24)], "the file is still collapsed inside");
        s.toggle(&mut t, file).unwrap();
        assert!(hidden(&t).is_empty());
    }

    #[test]
    fn each_section_has_its_own_number() {
        let mut t = TextBuf::new("");
        let mut s = Sections::default();
        let mut b = Build::new();
        b.eager(&stub("one"), &[])
            .push("\n")
            .eager(&stub("two"), &[]);
        s.place(&mut t, 0..0, b).unwrap();
        assert_ne!(s.at(&t, 0), s.at(&t, 5));
    }

    #[test]
    fn a_mark_moves_only_when_its_set() {
        let mut t = TextBuf::new("ab");
        let m = Mark::default();
        assert_eq!((m.pos(&t), m.at(&t)), (None, 2));
        m.set(&mut t, 1).unwrap();
        let shared = m.clone();
        t.tx(Origin::Extension).insert(1, "x").commit().unwrap();
        assert_eq!(shared.pos(&t), Some(1), "text put in at it goes after it");
        m.set(&mut t, 3).unwrap();
        assert_eq!(shared.pos(&t), Some(3));
    }

    #[test]
    fn the_stats_read_as_emacs_said_them() {
        let st = Stats {
            created: 4,
            materialized: 1,
            time: Duration::from_millis(2),
        };
        assert_eq!(
            report(st),
            "Lazy Formatting Stats:\n  Created: 4\n  Materialized: 1 (25.0%)\n  Total time: 0.002s\n  Avg time: 0.002s\n"
        );
        assert!(report(Stats::default()).contains("Materialized: 0 (0.0%)"));
    }
}
