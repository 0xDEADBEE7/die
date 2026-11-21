//! What a figure shows of its data: its domains, what's hidden, the reference, pinned nodes;
//! the settings `s` turns over the spec's own; and the undo of changes to them, one entry a
//! discrete change or gesture.

use super::cursor::Cursor;
use crate::data::SeriesKey;
use crate::spec::{Chart, GraphSpec, Labels, Layout, MarkKind, ScaleKind, Stack};
use std::collections::{BTreeMap, BTreeSet};

/// How many changes an undo history keeps (briefs/mouse-ux.md §9): the oldest go first.
pub const UNDO_CAP: usize = 200;

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

impl View {
    /// Whether `key` is drawn: not hidden, and the one shown alone if one is.
    pub fn shows(&self, key: &SeriesKey) -> bool {
        !self.hidden.contains(key) && self.only.as_ref().is_none_or(|o| o == key)
    }
}

/// One of the settings `s` offers (the design's settings menu), each a toggle over what the
/// spec says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Setting {
    /// Horizontal gridlines.
    Grid,
    /// The axes' ticks and labels.
    Axes,
    /// The value axis on a log scale.
    LogY,
    /// Bars and areas stacked.
    Stack,
    /// Each line's name at its end.
    Labels,
    /// A point at each sample of a line.
    Markers,
}

impl Setting {
    /// Every setting, in the menu's order.
    pub const ALL: [Setting; 6] = [
        Setting::Grid,
        Setting::Axes,
        Setting::LogY,
        Setting::Stack,
        Setting::Labels,
        Setting::Markers,
    ];

    /// Its name in the menu and the messages.
    pub fn name(self) -> &'static str {
        match self {
            Setting::Grid => "grid",
            Setting::Axes => "axes",
            Setting::LogY => "log y",
            Setting::Stack => "stack",
            Setting::Labels => "labels",
            Setting::Markers => "markers",
        }
    }

    /// The setting with this name.
    pub fn from_name(name: &str) -> Option<Setting> {
        Setting::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// A graph's layout as the settings choose it, over what the spec says (or the graph picks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPick {
    /// fig's stress layout.
    Force,
    /// A tidy tree, top down.
    Tree,
    /// Layers, top down.
    Layered,
}

impl LayoutPick {
    /// Every layout, in the menu's order.
    pub const ALL: [LayoutPick; 3] = [LayoutPick::Force, LayoutPick::Tree, LayoutPick::Layered];

    /// Its name in the menu, the messages and the session.
    pub fn name(self) -> &'static str {
        match self {
            LayoutPick::Force => "force",
            LayoutPick::Tree => "tree",
            LayoutPick::Layered => "layered",
        }
    }

    /// The layout with this name.
    pub fn from_name(name: &str) -> Option<LayoutPick> {
        LayoutPick::ALL.into_iter().find(|l| l.name() == name)
    }

    /// The spec's layout it is.
    pub fn layout(self) -> Layout {
        match self {
            LayoutPick::Force => Layout::Force,
            LayoutPick::Tree => Layout::Tree { root: None },
            LayoutPick::Layered => Layout::Layered,
        }
    }

    /// The pick a laid-out graph's layout is (None for `auto`, which a model never keeps).
    pub fn of(layout: &Layout) -> Option<LayoutPick> {
        match layout {
            Layout::Force => Some(LayoutPick::Force),
            Layout::Tree { .. } => Some(LayoutPick::Tree),
            Layout::Layered => Some(LayoutPick::Layered),
            Layout::Auto => None,
        }
    }
}

/// The settings turned over the spec's own: None is the spec's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Toggles {
    /// Gridlines.
    pub grid: Option<bool>,
    /// Axes.
    pub axes: Option<bool>,
    /// A log value axis.
    pub log_y: Option<bool>,
    /// Stacked bars and areas.
    pub stack: Option<bool>,
    /// End labels.
    pub labels: Option<bool>,
    /// Points on lines.
    pub markers: Option<bool>,
    /// A graph's layout.
    pub layout: Option<LayoutPick>,
}

/// Whether a layer's marks stack, or could: bars and areas.
fn stackable(kind: MarkKind) -> bool {
    matches!(kind, MarkKind::Bar | MarkKind::Area)
}

impl Toggles {
    fn slot(&mut self, s: Setting) -> &mut Option<bool> {
        match s {
            Setting::Grid => &mut self.grid,
            Setting::Axes => &mut self.axes,
            Setting::LogY => &mut self.log_y,
            Setting::Stack => &mut self.stack,
            Setting::Labels => &mut self.labels,
            Setting::Markers => &mut self.markers,
        }
    }

    /// What the spec itself says of `s`.
    pub fn spec_value(chart: &Chart, s: Setting) -> bool {
        let marks = || chart.layers.iter().map(|l| &l.mark);
        match s {
            Setting::Grid => chart.view.grid,
            Setting::Axes => chart.view.axes,
            Setting::LogY => chart
                .layers
                .iter()
                .any(|l| l.enc.y.as_ref().and_then(|c| c.scale) == Some(ScaleKind::Log)),
            Setting::Stack => marks().any(|m| stackable(m.kind) && m.stack != Stack::None),
            Setting::Labels => chart.view.labels != Labels::None,
            Setting::Markers => marks().any(|m| m.kind == MarkKind::Line && m.points),
        }
    }

    /// What `s` is for `chart`: turned, else the spec's.
    pub fn value(&self, chart: &Chart, s: Setting) -> bool {
        let mut t = *self;
        t.slot(s).unwrap_or_else(|| Toggles::spec_value(chart, s))
    }

    /// `s` turned over for `chart`; back to the spec's own when that's what it comes to.
    pub fn flip(&mut self, chart: &Chart, s: Setting) {
        let to = !self.value(chart, s);
        *self.slot(s) = (to != Toggles::spec_value(chart, s)).then_some(to);
    }

    /// A graph's spec with these turned: its labels on or off. The layout goes to the model's
    /// build beside it ([`Toggles::graph_layout`]).
    pub fn apply_graph(&self, spec: &GraphSpec) -> GraphSpec {
        let mut g = spec.clone();
        if let Some(on) = self.labels {
            g.view.labels = if on { Labels::Auto } else { Labels::None };
        }
        g
    }

    /// The layout the settings chose for a graph, over the spec's; None leaves it the spec's.
    pub fn graph_layout(&self) -> Option<Layout> {
        self.layout.map(LayoutPick::layout)
    }

    /// [`Toggles::graph_layout`] for `spec`: a tree picked where the spec's own layout is a tree
    /// keeps the spec's root.
    pub fn graph_layout_for(&self, spec: &GraphSpec) -> Option<Layout> {
        self.layout.map(|p| match (p, &spec.layout) {
            (LayoutPick::Tree, Layout::Tree { root }) => Layout::Tree { root: root.clone() },
            (p, _) => p.layout(),
        })
    }

    /// Whether a graph's labels show: turned, else the spec's.
    pub fn graph_labels(&self, spec: &GraphSpec) -> bool {
        self.labels.unwrap_or(spec.view.labels != Labels::None)
    }

    /// A graph's labels turned over; back to the spec's own when that's what it comes to.
    pub fn flip_graph_labels(&mut self, spec: &GraphSpec) {
        let to = !self.graph_labels(spec);
        self.labels = (to != (spec.view.labels != Labels::None)).then_some(to);
    }

    /// `chart` with these turned: what the model is built from.
    pub fn apply(&self, chart: &Chart) -> Chart {
        let mut c = chart.clone();
        if let Some(on) = self.grid {
            c.view.grid = on;
        }
        if let Some(on) = self.axes {
            c.view.axes = on;
        }
        if let Some(on) = self.labels {
            c.view.labels = if on { Labels::Ends } else { Labels::None };
        }
        for l in &mut c.layers {
            if let Some(on) = self.log_y
                && !matches!(l.mark.kind, MarkKind::Rule | MarkKind::Text)
            {
                let y = l.enc.y.get_or_insert_with(Default::default);
                y.scale = Some(if on {
                    ScaleKind::Log
                } else {
                    ScaleKind::Linear
                });
            }
            if let Some(on) = self.stack
                && stackable(l.mark.kind)
            {
                l.mark.stack = if on { Stack::Zero } else { Stack::None };
            }
            if let Some(on) = self.markers
                && l.mark.kind == MarkKind::Line
            {
                l.mark.points = on;
            }
        }
        c
    }
}

/// An undo and redo history of states: each entry is what a change replaced. A new change
/// forgets what could be redone, and past [`UNDO_CAP`] entries the oldest goes.
#[derive(Clone, Debug)]
pub struct Steps<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T> Default for Steps<T> {
    fn default() -> Steps<T> {
        Steps {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl<T: Clone + PartialEq> Steps<T> {
    /// Keeps `before`, the state a change is about to replace, and forgets what could be redone.
    pub fn push(&mut self, before: T) {
        self.undo.push(before);
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The state before the last change, keeping `now` to redo. None with nothing to undo.
    pub fn undo(&mut self, now: &T) -> Option<T> {
        let before = self.undo.pop()?;
        self.redo.push(now.clone());
        Some(before)
    }

    /// The state undone last, keeping `now` to undo again. None with nothing to redo.
    pub fn redo(&mut self, now: &T) -> Option<T> {
        let after = self.redo.pop()?;
        self.undo.push(now.clone());
        Some(after)
    }

    /// Takes back the last entry pushed, as if its change hadn't been made: a double-click's
    /// second press undoing its first click's toggle, so the two are one step.
    pub fn retract(&mut self) -> Option<T> {
        self.undo.pop()
    }

    /// How many changes there are to undo.
    pub fn undoable(&self) -> usize {
        self.undo.len()
    }

    /// How many to redo.
    pub fn redoable(&self) -> usize {
        self.redo.len()
    }
}

/// The undo and redo of a view's changes.
#[derive(Debug, Default)]
pub struct ViewStack {
    steps: Steps<View>,
}

impl ViewStack {
    /// Keeps the view as it was before a change, and forgets what could be redone.
    pub fn push(&mut self, v: View) {
        self.steps.push(v);
    }

    /// The view before the last change, keeping `now` to redo.
    pub fn undo(&mut self, now: &View) -> Option<View> {
        self.steps.undo(now)
    }

    /// The view undone last, keeping `now` to undo again.
    pub fn redo(&mut self, now: &View) -> Option<View> {
        self.steps.redo(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> View {
        View {
            x: Some((0.0, x)),
            ..View::default()
        }
    }

    #[test]
    fn undo_and_redo_walk_the_changes_and_a_new_one_forgets_the_redo() {
        let mut s = ViewStack::default();
        assert_eq!(s.undo(&at(1.0)), None);
        s.push(at(1.0));
        s.push(at(2.0));
        assert_eq!(s.undo(&at(3.0)), Some(at(2.0)));
        assert_eq!(s.undo(&at(2.0)), Some(at(1.0)));
        assert_eq!(s.undo(&at(1.0)), None);
        assert_eq!(s.redo(&at(1.0)), Some(at(2.0)));
        s.push(at(9.0));
        assert_eq!(s.redo(&at(9.0)), None);
    }

    #[test]
    fn the_history_keeps_two_hundred_and_a_retract_takes_back_the_last() {
        let mut s: Steps<u32> = Steps::default();
        for i in 0..250 {
            s.push(i);
        }
        assert_eq!(s.undoable(), UNDO_CAP);
        assert_eq!(s.retract(), Some(249));
        assert_eq!(s.undo(&0), Some(248));
        assert_eq!(s.redoable(), 1);
    }

    #[test]
    fn a_toggle_turns_a_setting_over_the_specs_and_back() {
        let table = crate::data::Table::new(vec![crate::data::Column::number("a", vec![1.0])]);
        let chart = crate::spec::guess(&table.unwrap()).unwrap();
        let mut t = Toggles::default();
        assert!(t.value(&chart, Setting::Grid));
        t.flip(&chart, Setting::Grid);
        assert_eq!(t.grid, Some(false));
        assert!(!t.apply(&chart).view.grid);
        t.flip(&chart, Setting::Grid);
        assert_eq!(t, Toggles::default(), "back to the spec's own");
        t.flip(&chart, Setting::LogY);
        let c = t.apply(&chart);
        assert_eq!(
            c.layers[0].enc.y.as_ref().unwrap().scale,
            Some(ScaleKind::Log)
        );
        assert!(t.value(&chart, Setting::LogY));
        for s in Setting::ALL {
            assert_eq!(Setting::from_name(s.name()), Some(s));
        }
    }

    #[test]
    fn a_series_shows_unless_hidden_or_another_is_alone() {
        let (a, b) = (SeriesKey::new("a", 0), SeriesKey::new("b", 0));
        let mut v = View::default();
        assert!(v.shows(&a));
        v.hidden.insert(a.clone());
        assert!(!v.shows(&a) && v.shows(&b));
        v.hidden.clear();
        v.only = Some(b.clone());
        assert!(!v.shows(&a) && v.shows(&b));
    }
}
