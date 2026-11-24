//! Where the gantt meets the detail panel and search: a task's tree, the items its rows and a
//! search's matches name.

use super::GanttModel;
use super::calendar;
use crate::detail::{self, DetailRow, DetailTree, Item, RowKind, Shape, Swatch};
use crate::graph::{is_name_key, is_parent_key};
use crate::interact::Cursor;
use crate::style::ColorSpec;

impl GanttModel {
    /// A task's colour as the panel's swatch names it: its bar's, or the neutral for a task
    /// with children, whose bar is the chrome's.
    pub fn swatch(&self, i: usize) -> Swatch {
        let t = &self.tasks[i];
        let color = match !t.children.is_empty() && !t.milestone && t.fig.color.is_none() {
            true => ColorSpec::Dim,
            false => self.color_spec(i),
        };
        Swatch {
            color,
            shape: Shape::Square,
        }
    }

    /// Task `i` as an item: its index and its key.
    pub fn item(&self, i: usize) -> Item {
        Item::Task {
            index: i as u32,
            id: self.tasks[i].key.clone(),
        }
    }

    /// The item the cursor is on: its task's.
    pub fn item_of(&self, cursor: &Cursor) -> Option<Item> {
        self.task_of(cursor).map(|i| self.item(i))
    }

    /// The detail of what an item names: a task, by its key (so an item from before an edit
    /// still finds it). None for another figure's item or a task the plan no longer has.
    pub fn detail_of(&self, item: &Item) -> Option<DetailTree> {
        match item {
            Item::Task { id, .. } => self.index_of(id).map(|i| task_tree(self, i)),
            _ => None,
        }
    }

    /// What a search looks through: each task by its label, and by its key where that
    /// differs, naming the task ([`GanttModel::names`] as items).
    pub fn search_names(&self) -> Vec<(String, Item)> {
        let mut out = Vec::with_capacity(self.len());
        for (i, t) in self.tasks.iter().enumerate() {
            out.push((t.label.clone(), self.item(i)));
            if t.label != *t.key {
                out.push((t.key.to_string(), self.item(i)));
            }
        }
        out
    }

    /// A time in a few words, for a row's note: "13 Nov", or "12 Jan 10:15" in a plan with
    /// times of day.
    fn brief(&self, t: f64) -> String {
        let day = calendar::short_day(t, &self.zone);
        match self.whole_days {
            true => day,
            false => format!("{day} {}", calendar::clock_text(t, &self.zone)),
        }
    }

    /// Task `j` as a row of another's tree names it, with a note.
    fn naming(&self, j: usize, note: String) -> DetailRow {
        DetailRow::mark(
            self.label(j).to_string(),
            note,
            Some(self.swatch(j)),
            self.item(j),
        )
        .quiet()
        .copying(self.key(j).to_string())
    }
}

/// Task `i`'s tree for the detail panel: its dates as the model worked them out (start, end
/// and duration, or when for a milestone; its progress; a conflict), then its row's fields
/// (the spec's `detail` fields where it names some, else every key but those the dates
/// already say), then the tasks it comes after and those that come after it, each a row that
/// names its task, and its `fig` block. Titled by its label, its kind line its group and
/// dates.
pub fn task_tree(model: &GanttModel, i: usize) -> DetailTree {
    let t = &model.tasks[i];
    let moment = t.end <= t.start;
    let dates = match moment {
        true => format!("milestone · {}", model.brief(t.start)),
        false => format!(
            "{} to {}",
            model.brief(t.start),
            model.brief(model.end_shown(i))
        ),
    };
    let kind = match t.group {
        Some(g) => format!("{} · {dates}", model.groups[g as usize]),
        None => dates,
    };
    let mut tree = DetailTree::new(t.label.clone(), kind);
    tree.swatch = Some(model.swatch(i));
    tree.item = Some(model.item(i));
    let iso = |at: f64| match model.whole_days {
        true => calendar::iso_day(at, &model.zone),
        false => calendar::iso_moment(at, &model.zone),
    };
    let mut rows = Vec::new();
    if moment {
        rows.push(DetailRow::field("when", model.start_text(i)).copying(iso(t.start)));
    } else {
        rows.push(DetailRow::field("start", model.start_text(i)).copying(iso(t.start)));
        let end = model.end_shown(i);
        rows.push(DetailRow::field("end", model.end_text(i)).copying(iso(end)));
        rows.push(DetailRow::field("duration", model.duration_text(i)));
        let done = match t.children.is_empty() {
            true => t.progress,
            false => t.done,
        };
        if let Some(p) = done {
            rows.push(DetailRow::field(
                "progress",
                format!("{} %", (p * 100.0).round()),
            ));
        }
    }
    if let Some(late) = model.late_text(i) {
        rows.push(DetailRow::field("conflict", late));
    }

    // The row's own fields.
    let c = &model.columns;
    let said = [&c.start, &c.end, &c.duration, &c.progress, &c.after];
    let mut skip: Vec<&str> = said.iter().filter_map(|n| n.as_deref()).collect();
    skip.push("fig");
    let mut own = Vec::new();
    if model.spec.detail.is_empty() {
        detail::object_rows(&t.raw, &skip, 0, &mut own);
    } else {
        for f in &model.spec.detail {
            if let Some(v) = t.raw.get(f) {
                detail::json_rows(f, v, 0, &mut own);
            }
        }
    }
    // A table's row repeats what the tree already says: its name column is the title, and
    // its parent column names the parent by id, which reads better as its label.
    // An empty cell says nothing either.
    if model.spec.detail.is_empty() {
        let names = |k: &str| c.label.as_deref() == Some(k) || is_name_key(k);
        own.retain(|r| {
            !(r.depth == 0
                && r.kind == RowKind::Field
                && (r.quiet || (names(&r.key) && r.value == t.label)))
        });
    }
    for r in &mut own {
        let parent = c.parent.as_deref() == Some(r.key.as_str()) || is_parent_key(&r.key);
        if r.depth == 0
            && r.kind == RowKind::Field
            && parent
            && let Some(p) = t.parent
        {
            let id = std::mem::replace(&mut r.value, model.label(p as usize).to_string());
            (r.item, r.copy) = (Some(model.item(p as usize)), Some(id));
        }
    }
    rows.extend(own);

    if !t.after.is_empty() {
        rows.push(DetailRow::section("Comes after"));
        for &p in &t.after {
            let p = p as usize;
            let ends = model.brief(model.end_shown(p));
            rows.push(model.naming(p, format!("ends {ends}")));
        }
    }
    if !t.then.is_empty() {
        rows.push(DetailRow::section("Then"));
        for &n in &t.then {
            let n = n as usize;
            let starts = model.brief(model.tasks[n].start);
            rows.push(model.naming(n, format!("starts {starts}")));
        }
    }
    if let Some(fig) = (t.raw.get("fig")).filter(|n| n.as_obj().is_some_and(|m| !m.is_empty())) {
        rows.push(DetailRow::section("fig"));
        detail::object_rows(fig, &[], 0, &mut rows);
    }
    tree.rows = rows;
    tree
}
