//! The gantt figure: tasks over time, a row each, with what each comes after.
//!
//! The editor drives a gantt as it drives a graph (`graph/`):
//!
//! - [`GanttModel::build`] resolves a [`GanttSpec`] and its table of a task a row: every date
//!   worked out, the rows ordered, the colours slotted, the links made. Pure: run it in a job;
//! - [`base`] draws the gantt at rest for a [`View`] and a size, with its regions, and returns
//!   the [`GanttLayout`] the rest asks: where every row, bar and link landed;
//! - [`overlay`](fn@overlay) draws what follows the pointer and the cursor ([`OverlayState`]):
//!   the lit row, the brightened bar and its links, the cursor's ring, the card, the date under
//!   the pointer, and a brush;
//! - [`hover`](fn@hover) finds the task under the pointer;
//! - [`nav`] moves the cursor between tasks and changes the view: fit, zoom, pan, reveal,
//!   centre and a brush;
//! - [`GanttModel::names`], [`GanttModel::tsv`] and [`GanttModel::summary`] give what a search
//!   can find, the tasks as a table, and the line under the title;
//! - [`detail`] gives a task's tree for the detail panel.
//!
//! A [`View`]'s `x` is the time shown in UTC seconds, None to fit the plan ([`GanttModel::fit`]);
//! `y` is the rows shown, in px from the top of the rows (`Some((top, top + plot height))`),
//! None for the top; `hidden` and `only` hold the colours' names (each a [`SeriesKey`], `nth`
//! 0). The cursor is a [`Cursor::Task`], named by the task's key: its id, else its label, made
//! unique.
//!
//! # What a task's dates mean
//!
//! A task's row gives some of `start`, `end`, `duration` and `after`:
//!
//! 1. `start` and `end`: as given;
//! 2. `start` and `duration`: it ends that long after; `end` and `duration`: it starts that
//!    long before;
//! 3. no `start`, but `after`: it starts when the last of them ends, and runs its `duration`;
//!    without one it's a milestone there;
//! 4. `start` or `end` alone: a milestone at that time, as is any task whose `milestone` is
//!    true (at its start, else its end, else where `after` puts it);
//! 5. a task others name as `parent`, with no dates of its own, spans its children;
//! 6. `start` and `after` both: the start stands. A task that starts before one it comes after
//!    ends is in conflict ([`Task::late`]);
//! 7. nothing to place it by is an error at the task's place.
//!
//! **Whole days.** When every start and end given is at midnight in the zone and every
//! duration is a whole number of days, the plan is in whole days ([`GanttModel::whole_days`]):
//! an `end` takes in its day (the model keeps the instant the bar stops, the next midnight,
//! and writes the last day wherever an end is shown), and a duration counts days on the
//! calendar ([`calendar::Days`]): working days where the spec's `calendar` says, and then a
//! start the model works out is never on a day off. A plan with a time of day in it is exact,
//! and its calendar isn't used (a warning says so).
//!
//! # Rows and colour
//!
//! Groups come in the order first seen, each under a heading; tasks with no group first, under
//! none. A task with a parent follows it, a level in, whatever its own group. Colour is by the
//! `color` channel's field, else by group, else slot 1 for every bar: the names take the series
//! slots in the order first seen (the ninth and on the neutral), and a `fig` block's colour
//! beats them. A legend lists the names when there are two or more and they aren't the group
//! headings already.

pub mod calendar;
pub mod detail;
pub mod draw;
pub mod geom;
pub mod hover;
pub mod nav;
pub mod overlay;

pub use draw::{base, base_beside};
pub use geom::{GanttLayout, LinkGeom, MarkGeom, RowGeom, RowKind, Shape, TaskGeom};
pub use hover::{Target, hover};
pub use overlay::{OverlayState, overlay};

use crate::data::infer::{is_missing, parse_number};
use crate::data::time::{epoch, parse_time};
use crate::data::{Column, ColumnKind, ReadOptions, SeriesKey, Table, Values};
use crate::error::{ErrorKind, FigError, Pos, Result};
use crate::interact::{Cursor, View};
use crate::json::{Kind, Node};
use crate::spec::gantt::{named, names};
use crate::spec::graph::{number_text, row_at, row_node};
use crate::spec::{Channel, GanttSpec, Today};
use crate::style::{ColorSpec, FigStyle};
use crate::theme::{Color, Theme, color_of};
use calendar::{DAY, Days};
use jiff::tz::TimeZone;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A task, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    /// What names it: its id, else its label, made unique. The cursor's and the regions' key.
    pub key: Box<str>,
    /// Its id as the data gives it, if it has one.
    pub id: Option<String>,
    /// Its name as written at its row: its `fig` label, its label, else its id.
    pub label: String,
    /// Its group, an index into [`GanttModel::groups`].
    pub group: Option<u32>,
    /// What colours it, an index into [`GanttModel::names`].
    pub name: Option<u32>,
    /// The task it's a part of.
    pub parent: Option<u32>,
    /// The tasks that are parts of it, in the data's order.
    pub children: Vec<u32>,
    /// The tasks it comes after, in the order it names them.
    pub after: Vec<u32>,
    /// The tasks that come after it, in the data's order.
    pub then: Vec<u32>,
    /// When it starts, in UTC seconds.
    pub start: f64,
    /// When its bar stops: in a plan of whole days, the midnight after its last day.
    pub end: f64,
    /// Whether it's a moment, drawn as a diamond at `start`.
    pub milestone: bool,
    /// The share of it done as its row says, 0 to 1.
    pub progress: Option<f64>,
    /// The share done as drawn: its own, or for a task with children and none of its own,
    /// theirs weighted by their length. None where the plan doesn't track progress.
    pub done: Option<f64>,
    /// A conflict: the task it comes after that ends latest past its start, and by how many
    /// seconds.
    pub late: Option<(u32, f64)>,
    /// How deep it is under its parents: 0 for a task with none.
    pub depth: u16,
    /// Its `fig` block (the default where blocks don't count).
    pub fig: FigStyle,
    /// Its row as an object: the source's where the rows came from JSON.
    pub raw: Node,
    /// Where it is: its object in JSON, its line in a table.
    pub at: Pos,
    /// Its row in the table, after the spec's transforms.
    pub row: u32,
}

impl Task {
    /// How long it runs, in seconds: nothing for a milestone.
    pub fn span(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// An arrow: `to` comes after `from`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    /// The earlier task.
    pub from: u32,
    /// The later one.
    pub to: u32,
    /// Whether the later starts before the earlier ends.
    pub conflict: bool,
}

/// A mark on the time, resolved: a line at `start`, or a span to `end`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Where the line is, or where the span starts.
    pub start: f64,
    /// Where a span stops (its last day taken in, in a plan of whole days); None for a line.
    pub end: Option<f64>,
    /// What it's called.
    pub label: Option<String>,
    /// Its colour, if its `fig` block gives one.
    pub color: Option<ColorSpec>,
}

/// The rows under one heading: a group's, or those with no group.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The group, an index into [`GanttModel::groups`]; None for the tasks with none.
    pub group: Option<u32>,
    /// Its tasks in row order: each with no parent, then its children, depth first.
    pub tasks: Vec<u32>,
}

/// Which columns of the table the model read, by name.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Columns {
    /// The ids'.
    pub id: Option<Box<str>>,
    /// The labels'.
    pub label: Option<Box<str>>,
    /// The starts'.
    pub start: Option<Box<str>>,
    /// The ends'.
    pub end: Option<Box<str>>,
    /// The durations'.
    pub duration: Option<Box<str>>,
    /// What each comes after.
    pub after: Option<Box<str>>,
    /// The groups'.
    pub group: Option<Box<str>>,
    /// The parents'.
    pub parent: Option<Box<str>>,
    /// The shares done.
    pub progress: Option<Box<str>>,
    /// Whether each is a milestone.
    pub milestone: Option<Box<str>>,
    /// What colours each.
    pub color: Option<Box<str>>,
}

/// An error building a gantt, and the row and column of the table it's about when it's a
/// row's: so the build can place it in the data's file ([`GanttModel::build_placed`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    /// What went wrong.
    pub error: FigError,
    /// The table's row at fault, after the spec's transforms.
    pub row: Option<usize>,
    /// The column of the cell at fault.
    pub column: Option<Box<str>>,
}

impl Fault {
    fn plain(error: FigError) -> Fault {
        Fault {
            error,
            row: None,
            column: None,
        }
    }
}

impl From<FigError> for Fault {
    fn from(error: FigError) -> Fault {
        Fault::plain(error)
    }
}

/// A gantt resolved: every task's dates, the rows' order, the colours, the links.
#[derive(Clone, Debug)]
pub struct GanttModel {
    /// The spec it was built from. Its `view`, `links` and `today` are read as it's drawn, so
    /// the settings may change them without a new build.
    pub spec: GanttSpec,
    /// The source's name, which the subtitle names when the spec has none.
    pub source: Option<String>,
    /// The zone its days are in.
    pub zone: TimeZone,
    /// The clock when it was built, in UTC seconds: where [`Today::Now`]'s line is.
    pub now: f64,
    /// The tasks, in the data's order.
    pub tasks: Vec<Task>,
    /// The rows: the tasks under their headings, in order.
    pub sections: Vec<Section>,
    /// The groups, in the order first seen.
    pub groups: Vec<String>,
    /// The names that colour the bars, in the order first seen: the `color` field's values,
    /// else the groups.
    pub names: Vec<String>,
    /// Whether the names are the group headings: no legend then, the headings say it.
    pub by_group: bool,
    /// The arrows, in the later tasks' order.
    pub links: Vec<Link>,
    /// The marks on the time.
    pub marks: Vec<Mark>,
    /// Whether the plan is in whole days.
    pub whole_days: bool,
    /// Whether any task says how much of it is done.
    pub tracks_progress: bool,
    /// The plan's calendar.
    pub days: Days,
    /// From the first start to the last end of the tasks and the marks.
    pub extent: (f64, f64),
    /// What was odd but not wrong: the calendar unused in a plan of hours, a progress above
    /// 100.
    pub warnings: Vec<FigError>,
    /// The columns read.
    pub columns: Columns,
    /// The table, after the spec's transforms.
    pub table: Arc<Table>,
    index: BTreeMap<Box<str>, u32>,
}

/// A cell of a task's row.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    None,
    Num(f64),
    Str(String),
    Bool(bool),
    List(Vec<String>),
}

impl Cell {
    /// The cell as JSON would write it, for a message.
    fn shown(&self) -> String {
        match self {
            Cell::None => "nothing".into(),
            Cell::Num(v) => number_text(*v),
            Cell::Str(s) => format!("{s:?}"),
            Cell::Bool(b) => b.to_string(),
            Cell::List(l) => format!("{l:?}"),
        }
    }

    /// The cell as an id or a name.
    fn text(&self) -> Option<String> {
        match self {
            Cell::Num(v) => Some(number_text(*v)),
            Cell::Str(s) => Some(s.clone()),
            Cell::Bool(b) => Some(b.to_string()),
            Cell::None | Cell::List(_) => None,
        }
    }
}

/// A task as its row gives it, before its dates are worked out.
struct Given {
    id: Option<String>,
    label: Option<String>,
    start: Option<f64>,
    end: Option<f64>,
    duration: Option<calendar::Span>,
    after: Vec<String>,
    /// Whether `after` came as one string from JSON, which names one task unless no task is
    /// called that.
    after_whole: Option<String>,
    group: Option<String>,
    parent: Option<String>,
    progress: Option<f64>,
    milestone: bool,
    color: Option<String>,
}

/// How a task's dates are worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Plan {
    /// A start and an end.
    Given(f64, f64),
    /// A start and a duration.
    StartFor(f64, f64),
    /// An end and a duration.
    EndFor(f64, f64),
    /// After others, for a duration.
    AfterFor(f64),
    /// A moment where the tasks it comes after end.
    AfterMoment,
    /// A moment at a time.
    At(f64),
    /// Its children's span.
    Children,
    /// From its start to its children's end.
    StartToChildren(f64),
    /// From its children's start to its end.
    ChildrenToEnd(f64),
}

impl Plan {
    /// Whether it waits for the tasks it comes after.
    fn needs_after(self) -> bool {
        matches!(self, Plan::AfterFor(_) | Plan::AfterMoment)
    }

    /// Whether it waits for its children.
    fn needs_children(self) -> bool {
        matches!(
            self,
            Plan::Children | Plan::StartToChildren(_) | Plan::ChildrenToEnd(_)
        )
    }
}

/// Reads the table's cells, the source object's first where the rows came from JSON.
struct Rows<'t> {
    table: &'t Table,
    opt: ReadOptions,
}

impl Rows<'_> {
    /// Where a row is: its object in JSON, its line in a table.
    fn at(&self, row: usize) -> Pos {
        match self.table.raw.as_ref().and_then(|r| r.get(row)) {
            Some(n) => n.at,
            None => row_at(row),
        }
    }

    /// A row's cell in a column: the source object's value where it has the key.
    fn cell(&self, col: &Column, row: usize) -> Cell {
        let raw = (self.table.raw.as_ref())
            .and_then(|r| r.get(row))
            .and_then(|n| n.get(&col.name));
        if let Some(n) = raw {
            return match &n.kind {
                Kind::Null => Cell::None,
                Kind::Num(v) => Cell::Num(*v),
                Kind::Bool(b) => Cell::Bool(*b),
                Kind::Str(s) if s.trim().is_empty() => Cell::None,
                Kind::Str(s) => Cell::Str(s.trim().to_string()),
                Kind::Arr(a) => Cell::List(
                    a.iter()
                        .map(|x| match &x.kind {
                            Kind::Str(s) => s.trim().to_string(),
                            _ => x.to_text(),
                        })
                        .filter(|s| !s.is_empty())
                        .collect(),
                ),
                Kind::Obj(_) => Cell::Str(n.to_text()),
            };
        }
        match &col.values {
            Values::Number(_) | Values::Time(_) => {
                let v = col.f64_at(row);
                if v.is_finite() {
                    Cell::Num(v)
                } else {
                    Cell::None
                }
            }
            _ => match col.str_at(row).map(str::trim) {
                Some(s) if !s.is_empty() => Cell::Str(s.to_string()),
                _ => Cell::None,
            },
        }
    }

    /// An error about a row's cell.
    fn fault(&self, msg: String, row: usize, col: &Column) -> Fault {
        Fault {
            error: FigError::at(msg, self.at(row)),
            row: Some(row),
            column: Some(col.name.clone()),
        }
    }

    /// A row's time in a column, as its channel says to read it.
    fn time(
        &self,
        col: &Column,
        ch: Option<&Channel>,
        row: usize,
    ) -> std::result::Result<Option<f64>, Fault> {
        let name = &*col.name;
        let unit = ch.and_then(|c| c.unit.as_deref());
        let bad = |c: &Cell| {
            self.fault(
                format!("\"{name}\" must be a time, not {}", c.shown()),
                row,
                col,
            )
        };
        if col.kind() == ColumnKind::Time {
            let v = col.f64_at(row);
            return Ok(v.is_finite().then_some(v));
        }
        match self.cell(col, row) {
            Cell::None => Ok(None),
            Cell::Num(v) => match unit.and_then(|u| epoch(v, u)) {
                Some(t) => Ok(Some(t)),
                None => Err(bad(&Cell::Num(v))),
            },
            Cell::Str(s) if is_missing(&s) => Ok(None),
            Cell::Str(s) => match parse_time(&s, &self.opt) {
                Some(t) => Ok(Some(t)),
                None => Err(bad(&Cell::Str(s))),
            },
            other => Err(bad(&other)),
        }
    }
}

/// The column a channel names, else the one found by its usual names. A channel naming a
/// column the data hasn't is an error at the channel's place.
fn column<'t>(
    table: &'t Table,
    ch: &Option<Channel>,
    usual: &[&str],
) -> Result<Option<&'t Column>> {
    let Some(ch) = ch else {
        return Ok(named(table, usual));
    };
    let Some(field) = ch.field.as_deref() else {
        return Ok(named(table, usual));
    };
    match table.column(field) {
        Some(c) => Ok(Some(c)),
        None => {
            let msg = format!("no column \"{field}\" in the data");
            let e = match ch.at {
                Some(at) => FigError::at(msg, at),
                None => FigError::new(msg),
            };
            let have: Vec<&str> = table.columns.iter().map(|c| &*c.name).collect();
            Err(e.note(format!("the data has {}", have.join(", "))))
        }
    }
}

/// Whether a cell says a task is a milestone: `true`, `yes`, `y` or `1`, in any case.
fn truthy(c: &Cell) -> bool {
    match c {
        Cell::Bool(b) => *b,
        Cell::Num(v) => *v == 1.0,
        Cell::Str(s) => ["true", "yes", "y", "1"]
            .iter()
            .any(|t| s.eq_ignore_ascii_case(t)),
        _ => false,
    }
}

impl GanttModel {
    /// The gantt of `spec` over `table`, its rows a task each: the spec's transforms applied,
    /// each column found (the spec's channel, else by its usual name), every task's dates
    /// worked out (see the module's docs), the rows ordered and the colours slotted. `zone` is
    /// the zone its days are in, and `now` the clock in UTC seconds, for the today line.
    ///
    /// Errors, each at the task's place (its object in JSON, its line in a table): ids given
    /// twice, an unknown task in `after` or `parent`, a task that comes after itself or is a
    /// part of itself, an end before its start, a time, a duration or a progress that doesn't
    /// read, and a task with nothing to place it by. A table with no rows is the Empty kind.
    pub fn build(
        spec: GanttSpec,
        table: Arc<Table>,
        zone: &TimeZone,
        now: f64,
    ) -> Result<GanttModel> {
        GanttModel::build_placed(spec, table, zone, now).map_err(|f| f.error)
    }

    /// [`GanttModel::build`], an error with the table's row and column it's about.
    pub fn build_placed(
        spec: GanttSpec,
        table: Arc<Table>,
        zone: &TimeZone,
        now: f64,
    ) -> std::result::Result<GanttModel, Fault> {
        let table = match spec.transform.is_empty() {
            true => table,
            false => Arc::new(crate::data::transform::apply(&table, &spec.transform)?),
        };
        if table.rows == 0 {
            return Err(FigError::new("nothing to draw: the plan has no tasks")
                .with_kind(ErrorKind::Empty)
                .into());
        }
        let f = &spec.fields;
        let id_c = column(&table, &f.id, names::ID)?;
        let label_c = column(&table, &f.label, names::LABEL)?;
        let start_c = column(&table, &f.start, names::START)?;
        let end_c = column(&table, &f.end, names::END)?;
        let dur_c = column(&table, &f.duration, names::DURATION)?;
        let after_c = column(&table, &f.after, names::AFTER)?;
        let group_c = column(&table, &f.group, names::GROUP)?;
        let parent_c = column(&table, &f.parent, names::PARENT)?;
        let progress_c = column(&table, &f.progress, names::PROGRESS)?;
        let milestone_c = column(&table, &f.milestone, names::MILESTONE)?;
        let color_c = column(&table, &f.color, &[])?;
        let name_of = |c: Option<&Column>| c.map(|c| c.name.clone());
        let columns = Columns {
            id: name_of(id_c),
            label: name_of(label_c),
            start: name_of(start_c),
            end: name_of(end_c),
            duration: name_of(dur_c),
            after: name_of(after_c),
            group: name_of(group_c),
            parent: name_of(parent_c),
            progress: name_of(progress_c),
            milestone: name_of(milestone_c),
            color: name_of(color_c),
        };
        let mut opt = ReadOptions::new(zone.clone());
        opt.time_format = spec.data.read.time_format.clone();
        let rows = Rows { table: &table, opt };
        let n = table.rows;
        let mut given: Vec<Given> = Vec::with_capacity(n);
        for row in 0..n {
            let text = |c: Option<&Column>| c.and_then(|c| rows.cell(c, row).text());
            let start = match start_c {
                Some(c) => rows.time(c, f.start.as_ref(), row)?,
                None => None,
            };
            let end = match end_c {
                Some(c) => rows.time(c, f.end.as_ref(), row)?,
                None => None,
            };
            let duration = match dur_c.map(|c| (c, rows.cell(c, row))) {
                None | Some((_, Cell::None)) => None,
                Some((_, Cell::Num(v))) if v >= 0.0 => Some(calendar::Span::days(v)),
                Some((c, cell)) => {
                    let read = match &cell {
                        Cell::Str(s) => calendar::parse_duration(s),
                        _ => None,
                    };
                    let Some(d) = read else {
                        let msg = format!(
                            "\"{}\" must be like \"3d\", \"2w\" or \"36h\", not {}",
                            c.name,
                            cell.shown()
                        );
                        return Err(rows.fault(msg, row, c));
                    };
                    Some(d)
                }
            };
            let (after, after_whole) = match after_c.map(|c| (c, rows.cell(c, row))) {
                Some((_, Cell::List(l))) => (l, None),
                Some((c, Cell::Str(s))) => {
                    let parts: Vec<String> = s
                        .split([',', ';'])
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect();
                    // A JSON string names one task, commas and all, where one is called that.
                    let from_json = (table.raw.as_ref())
                        .and_then(|r| r.get(row))
                        .is_some_and(|r| r.get(&c.name).is_some());
                    (parts, from_json.then_some(s))
                }
                Some((_, Cell::Num(v))) => (vec![number_text(v)], None),
                _ => (Vec::new(), None),
            };
            let progress = match progress_c.map(|c| (c, rows.cell(c, row))) {
                None | Some((_, Cell::None)) => None,
                Some((_, Cell::Num(v))) => Some(v),
                Some((_, Cell::Str(s))) if is_missing(&s) => None,
                Some((c, cell)) => {
                    let read = match &cell {
                        Cell::Str(s) => parse_number(s.trim_end_matches('%').trim_end(), None),
                        _ => None,
                    };
                    let Some(p) = read else {
                        let msg = format!("\"{}\" must be a number, not {}", c.name, cell.shown());
                        return Err(rows.fault(msg, row, c));
                    };
                    Some(p)
                }
            };
            given.push(Given {
                id: text(id_c),
                label: text(label_c),
                start,
                end,
                duration,
                after,
                after_whole,
                group: text(group_c),
                parent: text(parent_c),
                progress,
                milestone: milestone_c.is_some_and(|c| truthy(&rows.cell(c, row))),
                color: text(color_c),
            });
        }

        // Keys: an id, else the label, made unique. Ids must be unique as given.
        let mut index: BTreeMap<Box<str>, u32> = BTreeMap::new();
        let mut keys: Vec<Box<str>> = Vec::with_capacity(n);
        for (row, g) in given.iter().enumerate() {
            if let Some(id) = &g.id
                && index.contains_key(id.as_str())
                && given[index[id.as_str()] as usize].id.as_deref() == Some(id)
            {
                let msg = format!("task ids must be unique: {id} is given twice");
                let col = id_c.map(|c| c.name.clone());
                return Err(Fault {
                    error: FigError::at(msg, rows.at(row)),
                    row: Some(row),
                    column: col,
                });
            }
            let base = match (&g.id, &g.label) {
                (Some(id), _) => id.clone(),
                (None, Some(l)) => l.clone(),
                (None, None) => format!("task {}", row + 1),
            };
            let mut key = base.clone();
            let mut k = 2;
            while index.contains_key(key.as_str()) {
                key = format!("{base} ({k})");
                k += 1;
            }
            index.insert(key.as_str().into(), row as u32);
            keys.push(key.into());
        }

        // What each comes after, and what each is a part of.
        let unknown = |what: &str, id: &str, row: usize, col: Option<&Column>| Fault {
            error: FigError::at(format!("unknown task \"{id}\" in \"{what}\""), rows.at(row)),
            row: Some(row),
            column: col.map(|c| c.name.clone()),
        };
        let mut after: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut parent: Vec<Option<u32>> = vec![None; n];
        for (row, g) in given.iter().enumerate() {
            let what = after_c.map_or("after", |c| &*c.name);
            let whole = g.after_whole.as_deref().and_then(|w| index.get(w));
            let ids: Vec<u32> = match whole {
                Some(&i) => vec![i],
                None => {
                    let mut out = Vec::with_capacity(g.after.len());
                    for id in &g.after {
                        match index.get(id.as_str()) {
                            Some(&i) => out.push(i),
                            None => return Err(unknown(what, id, row, after_c)),
                        }
                    }
                    out
                }
            };
            for i in ids {
                if !after[row].contains(&i) {
                    after[row].push(i);
                }
            }
            if let Some(p) = &g.parent {
                let what = parent_c.map_or("parent", |c| &*c.name);
                match index.get(p.as_str()) {
                    Some(&i) => parent[row] = Some(i),
                    None => return Err(unknown(what, p, row, parent_c)),
                }
            }
        }
        let placed = |i: usize, msg: String, col: Option<&Column>| Fault {
            error: FigError::at(msg, rows.at(i)),
            row: Some(i),
            column: col.map(|c| c.name.clone()),
        };
        // A ring of tasks that wait for each other, in the order they'd happen: from its
        // first in the data, each then the one that comes after it, back to the first.
        let waits_for_itself = |ring: &[usize]| {
            let a = ring[0];
            let mut path: Vec<&str> = vec![&keys[a]];
            path.extend(ring[1..].iter().rev().map(|&i| &*keys[i]));
            path.push(&keys[a]);
            format!("\"{}\" comes after itself: {}", keys[a], path.join(" → "))
        };
        if let Some(ring) = cycle(n, |i| after[i].iter().map(|&j| j as usize).collect()) {
            return Err(placed(ring[0], waits_for_itself(&ring), after_c));
        }
        if let Some(ring) = cycle(n, |i| parent[i].iter().map(|&j| j as usize).collect()) {
            let path: Vec<&str> = ring.iter().map(|&i| &*keys[i]).collect();
            let a = ring[0];
            let msg = format!(
                "\"{}\" is part of itself: {} → {}",
                keys[a],
                path.join(" → "),
                path[0]
            );
            return Err(placed(a, msg, parent_c));
        }
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (i, p) in parent.iter().enumerate() {
            if let Some(p) = *p {
                children[p as usize].push(i as u32);
            }
        }

        // Whole days: every start and end at midnight, every duration a whole number of days,
        // a week being five of them where weekends don't count.
        let days = Days::of(&spec.calendar, zone);
        let per_week = if days.weekdays { 5.0 } else { 7.0 };
        let whole_days = given.iter().all(|g| {
            g.start.is_none_or(|t| days.at_midnight(t))
                && g.end.is_none_or(|t| days.at_midnight(t))
                && (g.duration).is_none_or(|d| (d.in_secs(per_week) / DAY).fract() == 0.0)
        });
        // Each duration in seconds: with times of day in the plan, a week is seven days.
        let per_week = if whole_days { per_week } else { 7.0 };
        let lasts: Vec<Option<f64>> = (given.iter())
            .map(|g| g.duration.map(|d| d.in_secs(per_week)))
            .collect();
        let mut warnings = Vec::new();
        if !whole_days && !days.every_day() {
            warnings.push(FigError::new(
                "\"calendar\" counts whole days, and this plan has times of day: every hour \
                 counts",
            ));
        }

        // How each task's dates are worked out.
        let mut plans: Vec<Plan> = Vec::with_capacity(n);
        for (i, g) in given.iter().enumerate() {
            let kids = !children[i].is_empty();
            let waits = !after[i].is_empty();
            let plan = match (g.start, g.end, lasts[i]) {
                (Some(s), _, _) if g.milestone => Some(Plan::At(s)),
                (None, Some(e), _) if g.milestone => Some(Plan::At(e)),
                (None, None, _) if g.milestone && waits => Some(Plan::AfterMoment),
                (Some(s), Some(e), _) => Some(Plan::Given(s, e)),
                (Some(s), None, Some(d)) => Some(Plan::StartFor(s, d)),
                (None, Some(e), Some(d)) => Some(Plan::EndFor(e, d)),
                (Some(s), None, None) if kids => Some(Plan::StartToChildren(s)),
                (Some(s), None, None) => Some(Plan::At(s)),
                (None, Some(e), None) if kids => Some(Plan::ChildrenToEnd(e)),
                (None, Some(e), None) => Some(Plan::At(e)),
                (None, None, Some(d)) if waits => Some(Plan::AfterFor(d)),
                (None, None, _) if kids => Some(Plan::Children),
                (None, None, None) if waits => Some(Plan::AfterMoment),
                (None, None, _) => None,
            };
            let Some(plan) = plan else {
                let msg = format!(
                    "task \"{}\" has no start: give it \"start\", or \"after\" and a \"duration\"",
                    keys[i]
                );
                return Err(placed(i, msg, None));
            };
            plans.push(plan);
        }
        let needs = |i: usize| -> Vec<usize> {
            let mut out = Vec::new();
            if plans[i].needs_after() {
                out.extend(after[i].iter().map(|&j| j as usize));
            }
            if plans[i].needs_children() {
                out.extend(children[i].iter().map(|&j| j as usize));
            }
            out
        };
        if let Some(ring) = cycle(n, needs) {
            return Err(placed(ring[0], waits_for_itself(&ring), after_c));
        }
        let mut span: Vec<Option<(f64, f64)>> = vec![None; n];
        for i in order(n, needs) {
            let last_end = || {
                (after[i].iter())
                    .filter_map(|&j| span[j as usize].map(|s| s.1))
                    .fold(f64::NEG_INFINITY, f64::max)
            };
            let kids = || {
                (children[i].iter())
                    .filter_map(|&j| span[j as usize])
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| {
                        (a.min(s.0), b.max(s.1))
                    })
            };
            // Where a task that waits starts: when the last of them ends, on a day that counts.
            let waited = |t: f64| match (whole_days, days.date(t)) {
                (true, Some(d)) if !days.at_midnight(t) => {
                    days.start(days.next_working(d.tomorrow().unwrap_or(d)))
                }
                (true, Some(d)) => days.start(days.next_working(d)),
                _ => t,
            };
            let ends = |t: f64| match (whole_days, days.date(t)) {
                (true, Some(d)) => days.start(d.tomorrow().unwrap_or(d)),
                _ => t,
            };
            let runs = |s: f64, d: f64| match (whole_days, days.date(s)) {
                (true, Some(day)) => days.start(days.add(day, (d / DAY).round() as i64)),
                _ => s + d,
            };
            let (s, e) = match plans[i] {
                Plan::Given(s, e) => {
                    if e < s {
                        let msg = format!(
                            "\"{}\" is before \"{}\"",
                            end_c.map_or("end", |c| &*c.name),
                            start_c.map_or("start", |c| &*c.name)
                        );
                        return Err(placed(i, msg, end_c));
                    }
                    (s, ends(e))
                }
                Plan::StartFor(s, d) => (s, runs(s, d)),
                Plan::EndFor(e, d) => match (whole_days, days.date(e)) {
                    (true, Some(last)) if d > 0.0 => {
                        let first = days.back(last, (d / DAY).round() as i64);
                        (days.start(first), ends(e))
                    }
                    (true, _) => (e, e),
                    _ => (e - d, e),
                },
                Plan::AfterFor(d) => {
                    let s = waited(last_end());
                    (s, runs(s, d))
                }
                Plan::AfterMoment => {
                    let s = waited(last_end());
                    (s, s)
                }
                Plan::At(t) => (t, t),
                Plan::Children => kids(),
                Plan::StartToChildren(s) => (s, kids().1.max(s)),
                Plan::ChildrenToEnd(e) => {
                    let e = ends(e);
                    (kids().0.min(e), e)
                }
            };
            let s = if s.is_finite() { s } else { e };
            let e = if e.is_finite() { e.max(s) } else { s };
            span[i] = Some(if given[i].milestone { (s, s) } else { (s, e) });
        }

        // Progress: a share, or percent when any is above 1.
        let percent = given.iter().any(|g| g.progress.is_some_and(|p| p > 1.0));
        let tracks_progress = given.iter().any(|g| g.progress.is_some());
        let mut progress: Vec<Option<f64>> = Vec::with_capacity(n);
        for (i, g) in given.iter().enumerate() {
            progress.push(g.progress.map(|p| {
                if p > 100.0 {
                    let msg = format!("progress {} is above 100: drawn as done", number_text(p));
                    warnings.push(FigError::at(msg, rows.at(i)));
                }
                (if percent { p / 100.0 } else { p }).clamp(0.0, 1.0)
            }));
        }

        // Groups and the names that colour.
        let mut groups: Vec<String> = Vec::new();
        let slot = |list: &mut Vec<String>, v: &Option<String>| -> Option<u32> {
            let v = v.as_ref()?;
            Some(match list.iter().position(|x| x == v) {
                Some(i) => i as u32,
                None => {
                    list.push(v.clone());
                    list.len() as u32 - 1
                }
            })
        };
        let group_of: Vec<Option<u32>> =
            given.iter().map(|g| slot(&mut groups, &g.group)).collect();
        let by_group = match (color_c, group_c) {
            (Some(c), Some(g)) => c.name == g.name,
            (Some(_), None) => false,
            (None, _) => true,
        };
        let mut names_list: Vec<String> = Vec::new();
        let name_of: Vec<Option<u32>> = if by_group {
            names_list = groups.clone();
            group_of.clone()
        } else {
            given
                .iter()
                .map(|g| slot(&mut names_list, &g.color))
                .collect()
        };

        let render = spec.view.render_options;
        let depth = depths(&parent);
        let mut then: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut links = Vec::new();
        let mut tasks: Vec<Task> = Vec::with_capacity(n);
        for i in 0..n {
            let (start, end) = span[i].unwrap_or((f64::NAN, f64::NAN));
            let mut late: Option<(u32, f64)> = None;
            for &p in &after[i] {
                then[p as usize].push(i as u32);
                let p_end = span[p as usize].map_or(f64::NAN, |s| s.1);
                let by = p_end - start;
                let conflict = by > 0.0;
                if conflict && late.is_none_or(|(_, b)| by > b) {
                    late = Some((p, by));
                }
                links.push(Link {
                    from: p,
                    to: i as u32,
                    conflict,
                });
            }
            let fig = match (render, table.styles.as_ref().and_then(|s| s.get(i))) {
                (true, Some(s)) => s.clone(),
                _ => FigStyle::default(),
            };
            let g = &given[i];
            let label = (fig.label.clone())
                .or_else(|| g.label.clone())
                .unwrap_or_else(|| keys[i].to_string());
            let at = rows.at(i);
            tasks.push(Task {
                key: keys[i].clone(),
                id: g.id.clone(),
                label,
                group: group_of[i],
                name: name_of[i],
                parent: parent[i],
                children: children[i].clone(),
                after: after[i].clone(),
                then: Vec::new(),
                start,
                end,
                milestone: g.milestone || end <= start,
                progress: progress[i],
                done: None,
                late,
                depth: depth[i],
                fig,
                raw: row_node(&table, i, at),
                at,
                row: i as u32,
            });
        }
        for (t, th) in tasks.iter_mut().zip(then) {
            t.then = th;
        }
        // A task with children is no moment, whatever its span.
        for t in &mut tasks {
            if !t.children.is_empty() && !given[t.row as usize].milestone {
                t.milestone = false;
            }
        }
        if tracks_progress {
            for i in order(n, |i| children[i].iter().map(|&j| j as usize).collect()) {
                let own = tasks[i].progress;
                let done = own.unwrap_or_else(|| {
                    let (mut sum, mut len) = (0.0, 0.0);
                    for &c in &tasks[i].children {
                        let c = &tasks[c as usize];
                        sum += c.done.unwrap_or(0.0) * c.span();
                        len += c.span();
                    }
                    if len > 0.0 { sum / len } else { 0.0 }
                });
                tasks[i].done = Some(done);
            }
        }

        // The rows: tasks with no group first, then each group, a task's children under it.
        let mut sections: Vec<Section> = Vec::new();
        let heads = std::iter::once(None).chain((0..groups.len() as u32).map(Some));
        for group in heads {
            let mut list = Vec::new();
            for i in 0..n {
                if parent[i].is_none() && group_of[i] == group {
                    walk(i as u32, &children, &mut list);
                }
            }
            if !list.is_empty() {
                sections.push(Section { group, tasks: list });
            }
        }

        let marks: Vec<Mark> = (spec.marks.iter())
            .map(|m| Mark {
                start: m.at.secs,
                end: m.end.as_ref().map(|e| match days.date(e.secs) {
                    Some(d) if whole_days && e.is_date() => days.start(d.tomorrow().unwrap_or(d)),
                    _ => e.secs,
                }),
                label: m.label.clone().filter(|l| !l.is_empty()),
                color: if render { m.fig.color } else { None },
            })
            .collect();
        let mut extent = (f64::INFINITY, f64::NEG_INFINITY);
        for t in &tasks {
            extent = (extent.0.min(t.start), extent.1.max(t.end));
        }
        for m in &marks {
            extent = (
                extent.0.min(m.start),
                extent.1.max(m.end.unwrap_or(m.start)),
            );
        }
        Ok(GanttModel {
            spec,
            source: None,
            zone: zone.clone(),
            now,
            tasks,
            sections,
            groups,
            names: names_list,
            by_group,
            links,
            marks,
            whole_days,
            tracks_progress,
            days,
            extent,
            warnings,
            columns,
            table,
            index,
        })
    }

    /// The model with `source` named in its subtitle.
    pub fn named(mut self, source: impl Into<String>) -> GanttModel {
        self.source = Some(source.into());
        self
    }

    /// How many tasks there are.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether there are none (never, once built).
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// A task's index by its key.
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.index.get(key).map(|&i| i as usize)
    }

    /// The task a cursor is on, if it's a task of this gantt.
    pub fn task_of(&self, cursor: &Cursor) -> Option<usize> {
        match cursor {
            Cursor::Task(key) => self.index_of(key),
            _ => None,
        }
    }

    /// The cursor on task `i`.
    pub fn cursor(&self, i: usize) -> Cursor {
        Cursor::Task(self.tasks[i].key.clone())
    }

    /// A task's key.
    pub fn key(&self, i: usize) -> &str {
        &self.tasks[i].key
    }

    /// A task's name as shown.
    pub fn label(&self, i: usize) -> &str {
        &self.tasks[i].label
    }

    /// The legend's and the hidden set's key for a name.
    pub fn name_key(&self, n: usize) -> SeriesKey {
        SeriesKey::new(&self.names[n], 0)
    }

    /// A task's name's key: its name's, or the key named "" for a task with none.
    pub fn task_key(&self, i: usize) -> SeriesKey {
        match self.tasks[i].name {
            Some(n) => self.name_key(n as usize),
            None => SeriesKey::new("", 0),
        }
    }

    /// The names' keys, in the legend's order: what `x`, `o` and the legend's clicks hide and
    /// show.
    pub fn keys(&self) -> Vec<SeriesKey> {
        (0..self.names.len()).map(|n| self.name_key(n)).collect()
    }

    /// Whether a legend is drawn: two names or more that aren't the group headings already,
    /// and a `view.legend` that isn't `none`.
    pub fn has_legend(&self) -> bool {
        self.names.len() >= 2
            && !self.by_group
            && self.spec.view.legend != crate::spec::LegendPos::None
    }

    /// The view a gantt opens with: the whole plan, from its first row.
    pub fn initial_view(&self) -> View {
        View::default()
    }

    /// Whether task `i` has a row under `view`: its name shown, and not hidden by its `fig`.
    pub fn visible(&self, view: &View, i: usize) -> bool {
        !self.tasks[i].fig.hidden && view.shows(&self.task_key(i))
    }

    /// A task's colour: its `fig` colour, else its name's slot (slot 1 for every task when
    /// there are no names; the neutral for a ninth name and on, and for a task without a name
    /// among tasks with one).
    pub fn color(&self, i: usize, theme: &Theme) -> Color {
        color_of(self.color_spec(i), theme)
    }

    /// A task's colour as a spec names it, for the panel's swatch.
    pub fn color_spec(&self, i: usize) -> ColorSpec {
        if let Some(c) = self.tasks[i].fig.color {
            return c;
        }
        match self.tasks[i].name {
            Some(n) if n < 8 => ColorSpec::Slot(n as u8),
            Some(_) => ColorSpec::Dim,
            None if self.names.is_empty() => ColorSpec::Slot(0),
            None => ColorSpec::Dim,
        }
    }

    /// A name's colour.
    pub fn name_color(&self, n: usize, theme: &Theme) -> Color {
        theme.series.get(n).copied().unwrap_or(theme.other)
    }

    /// Where the today line is, in UTC seconds: the clock's now, the spec's time, or nowhere.
    pub fn today(&self) -> Option<f64> {
        match self.spec.today {
            Today::Now => Some(self.now).filter(|t| t.is_finite()),
            Today::At(t) => Some(t),
            Today::Off => None,
        }
    }

    /// The time a view shows when it fits the plan: its tasks' and marks' whole span (a day at
    /// the least; an hour, in a plan with times of day), padded 3 % each side, and in a plan
    /// of whole days out to whole days.
    pub fn fit(&self) -> (f64, f64) {
        let (mut lo, mut hi) = self.extent;
        if !(lo.is_finite() && hi.is_finite()) {
            (lo, hi) = (self.now, self.now);
        }
        let least = if self.whole_days { DAY } else { 3600.0 };
        if hi - lo < least {
            let mid = (lo + hi) / 2.0;
            (lo, hi) = (mid - least / 2.0, mid + least / 2.0);
        }
        let pad = (hi - lo) * 0.03;
        let (lo, hi) = (lo - pad, hi + pad);
        if !self.whole_days {
            return (lo, hi);
        }
        // Out to whole days, so no sliver of a day shows at an edge.
        let first = self.days.date(lo).map(|d| self.days.start(d));
        let last = (self.days.date(hi))
            .map(|d| (d, self.days.start(d)))
            .map(|(d, at)| match at < hi {
                true => self.days.start(d.tomorrow().unwrap_or(d)),
                false => at,
            });
        match (first, last) {
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() && b > a => (a, b),
            _ => (lo, hi),
        }
    }

    /// The last time the plan shows: the latest of its tasks' ends as they're written (a last
    /// day, a milestone's moment) and its marks.
    pub fn last_shown(&self) -> f64 {
        let tasks = (0..self.len()).map(|i| self.end_shown(i));
        let marks = self.marks.iter().map(|m| m.start);
        tasks.chain(marks).fold(self.extent.0, f64::max)
    }

    /// Whether the plan's dates are in more than one year: its dates are written with theirs.
    pub fn years(&self) -> bool {
        let y = |t: f64| calendar::year_of(t, &self.zone);
        y(self.extent.0) != y(self.last_shown())
    }

    /// A time as the card writes it: "Mon 2 Nov" in a plan of whole days, "Mon 2 Nov 14:30"
    /// otherwise, with the year when the plan spans more than one.
    pub fn when_text(&self, t: f64) -> String {
        if self.whole_days {
            calendar::day_text(t, self.years(), &self.zone)
        } else {
            calendar::moment_text(t, self.years(), &self.zone)
        }
    }

    /// The last instant a task's end is written as: in a plan of whole days its last day, the
    /// one before the midnight its bar stops at.
    pub fn end_shown(&self, i: usize) -> f64 {
        let t = &self.tasks[i];
        if !self.whole_days || t.end <= t.start {
            return t.end;
        }
        match self.days.date(t.end).and_then(|d| d.yesterday().ok()) {
            Some(d) => self.days.start(d).max(t.start),
            None => t.end,
        }
    }

    /// A task's start as the card writes it.
    pub fn start_text(&self, i: usize) -> String {
        self.when_text(self.tasks[i].start)
    }

    /// A task's end as the card writes it: its last day, in a plan of whole days.
    pub fn end_text(&self, i: usize) -> String {
        self.when_text(self.end_shown(i))
    }

    /// How many days a task takes on the plan's calendar, in a plan of whole days.
    pub fn day_count(&self, i: usize) -> Option<i64> {
        let t = &self.tasks[i];
        if !self.whole_days {
            return None;
        }
        Some(
            self.days
                .count(self.days.date(t.start)?, self.days.date(t.end)?),
        )
    }

    /// How long a task runs, as the card writes it: "5 days", "2 weeks 3 days" (a week five
    /// days where weekends don't count), "36 h". Nothing for a milestone.
    pub fn duration_text(&self, i: usize) -> String {
        let t = &self.tasks[i];
        if t.end <= t.start {
            return String::new();
        }
        match self.day_count(i) {
            Some(n) => calendar::days_text(n, if self.days.weekdays { 5 } else { 7 }),
            None => calendar::span_text(t.span()),
        }
    }

    /// A conflict as the card says it: "Starts 2 days before API ends".
    pub fn late_text(&self, i: usize) -> Option<String> {
        let (p, by) = self.tasks[i].late?;
        let how = if self.whole_days {
            calendar::days_text((by / DAY).round().max(1.0) as i64, i64::MAX)
        } else {
            calendar::span_text(by)
        };
        Some(format!(
            "Starts {how} before {} ends",
            self.label(p as usize)
        ))
    }

    /// What a search can find: each task by its label, and by its key where that differs, with
    /// where the cursor goes; in the data's order.
    pub fn names(&self) -> Vec<(String, Cursor)> {
        let mut out = Vec::with_capacity(self.len());
        for (i, t) in self.tasks.iter().enumerate() {
            let c = self.cursor(i);
            out.push((t.label.clone(), c.clone()));
            if t.label != *t.key {
                out.push((t.key.to_string(), c));
            }
        }
        out
    }

    /// A task's fields beyond what the model reads: each other column of its row with its
    /// value as text, in the table's order (a JSON row's own keys, in its order).
    pub fn fields(&self, i: usize) -> Vec<(String, String)> {
        let c = &self.columns;
        let read = [
            &c.id,
            &c.label,
            &c.start,
            &c.end,
            &c.duration,
            &c.after,
            &c.group,
            &c.progress,
        ];
        let known = |k: &str| read.iter().any(|n| n.as_deref() == Some(k)) || k == "fig";
        let mut out = Vec::new();
        for (k, v) in self.tasks[i].raw.as_obj().unwrap_or_default() {
            if known(&k.name) || v.is_null() {
                continue;
            }
            out.push((k.name.to_string(), crate::graph::value_text(v)));
        }
        out
    }

    /// One of a task's fields by its name, as text: a column of its row.
    pub fn field(&self, i: usize, name: &str) -> Option<String> {
        let v = self.tasks[i].raw.get(name)?;
        (!v.is_null()).then(|| crate::graph::value_text(v))
    }

    /// The rows shown under `view`, in order: each task that has one.
    pub fn shown(&self, view: &View) -> Vec<usize> {
        (self.sections.iter())
            .flat_map(|s| s.tasks.iter().map(|&i| i as usize))
            .filter(|&i| self.visible(view, i))
            .collect()
    }

    /// A task's start, end and duration as a spec reads them back: dates in a plan of whole
    /// days (the end its last day, the duration its days on the calendar), times otherwise.
    fn spec_dates(&self, i: usize) -> (String, String, String) {
        let t = &self.tasks[i];
        if self.whole_days {
            let n = self.day_count(i).unwrap_or(0);
            return (
                calendar::iso_day(t.start, &self.zone),
                calendar::iso_day(self.end_shown(i), &self.zone),
                if t.end > t.start {
                    format!("{n}d")
                } else {
                    String::new()
                },
            );
        }
        (
            calendar::iso_moment(t.start, &self.zone),
            calendar::iso_moment(t.end, &self.zone),
            if t.end > t.start {
                calendar::span_spec(t.span())
            } else {
                String::new()
            },
        )
    }

    /// The tasks shown under `view` as TSV, in row order: `id`, `label`, `group`, `start`,
    /// `end`, `duration`, `progress` and `after` (keys, `;` between them), then every other
    /// column of the data.
    pub fn tsv(&self, view: &View) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let shown = self.shown(view);
        let fields: Vec<Vec<(String, String)>> = shown.iter().map(|&i| self.fields(i)).collect();
        let mut extra: Vec<String> = Vec::new();
        for fs in &fields {
            for (k, _) in fs {
                if !extra.contains(k) {
                    extra.push(k.clone());
                }
            }
        }
        let mut out = String::from("id\tlabel\tgroup\tstart\tend\tduration\tprogress\tafter");
        for k in &extra {
            out.push('\t');
            out.push_str(&clean(k));
        }
        out.push('\n');
        for (&i, fs) in shown.iter().zip(&fields) {
            let t = &self.tasks[i];
            let (start, end, duration) = self.spec_dates(i);
            let after: Vec<&str> = t.after.iter().map(|&p| self.key(p as usize)).collect();
            let mut cells = vec![
                clean(&t.key),
                clean(&t.label),
                clean(t.group.map_or("", |g| &self.groups[g as usize])),
                start,
                end,
                duration,
                t.progress.map_or(String::new(), |p| {
                    number_text((p * 1000.0).round() / 1000.0)
                }),
                clean(&after.join(";")),
            ];
            for k in &extra {
                let v = fs.iter().find(|(f, _)| f == k);
                cells.push(v.map_or(String::new(), |(_, v)| clean(v)));
            }
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        out
    }

    /// The line of [`GanttModel::tsv`] the cursor's task is on (from 0, the header's), if it's
    /// shown.
    pub fn tsv_line(&self, view: &View, cursor: &Cursor) -> Option<usize> {
        let i = self.task_of(cursor)?;
        self.shown(view).iter().position(|&j| j == i).map(|p| p + 1)
    }

    /// The line under the title when the spec has none: the source's name, the count and the
    /// plan's dates ("plan.csv · 12 tasks · 2 Nov to 18 Dec").
    pub fn summary(&self) -> String {
        let n = self.len();
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = self.source.as_deref().filter(|s| !s.is_empty()) {
            parts.push(s.to_string());
        }
        parts.push(format!("{n} task{}", if n == 1 { "" } else { "s" }));
        let (lo, hi) = self.extent;
        if lo.is_finite() && hi.is_finite() {
            let last = self.last_shown();
            let day = |t: f64| {
                let d = calendar::short_day(t, &self.zone);
                match (self.years(), calendar::year_of(t, &self.zone)) {
                    (true, Some(y)) => format!("{d} {y}"),
                    _ => d,
                }
            };
            let same_day = self.days.date(lo) == self.days.date(last);
            parts.push(match (same_day, self.whole_days) {
                (true, true) => day(lo),
                (true, false) => format!(
                    "{}, {} to {}",
                    day(lo),
                    calendar::clock_text(lo, &self.zone),
                    calendar::clock_text(hi, &self.zone)
                ),
                (false, _) => format!("{} to {}", day(lo), day(last)),
            });
        }
        parts.join(" · ")
    }
}

/// A task and its children under it, depth first, in the data's order.
fn walk(i: u32, children: &[Vec<u32>], out: &mut Vec<u32>) {
    let mut stack = vec![i];
    while let Some(x) = stack.pop() {
        out.push(x);
        stack.extend(children[x as usize].iter().rev());
    }
}

/// Each task's depth under its parents (which have no cycle).
fn depths(parent: &[Option<u32>]) -> Vec<u16> {
    let n = parent.len();
    let mut depth = vec![u16::MAX; n];
    for i in 0..n {
        let mut chain = Vec::new();
        let mut x = i;
        let base = loop {
            if depth[x] != u16::MAX {
                break depth[x].saturating_add(1);
            }
            chain.push(x);
            match parent[x] {
                Some(p) if chain.len() <= n => x = p as usize,
                _ => break 0,
            }
        };
        for (k, &c) in chain.iter().rev().enumerate() {
            depth[c] = base.saturating_add(k as u16);
        }
    }
    depth
}

/// A cycle among `n` things where `needs(i)` lists what `i` waits for: its members from the
/// first in order that's in one, each followed by the one it waits for. None when there's
/// none.
fn cycle(n: usize, needs: impl Fn(usize) -> Vec<usize>) -> Option<Vec<usize>> {
    // 0 unseen, 1 on the path, 2 done.
    let mut state = vec![0u8; n];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut path: Vec<(usize, Vec<usize>, usize)> = vec![(root, needs(root), 0)];
        state[root] = 1;
        while let Some((at, list, k)) = path.last_mut() {
            if *k >= list.len() {
                state[*at] = 2;
                path.pop();
                continue;
            }
            let next = list[*k];
            *k += 1;
            match state[next] {
                0 => {
                    state[next] = 1;
                    path.push((next, needs(next), 0));
                }
                1 => {
                    let from = path.iter().position(|p| p.0 == next).unwrap_or(0);
                    let mut ring: Vec<usize> = path[from..].iter().map(|p| p.0).collect();
                    // From its first member in the data's order.
                    let first = (0..ring.len()).min_by_key(|&j| ring[j]).unwrap_or(0);
                    ring.rotate_left(first);
                    return Some(ring);
                }
                _ => {}
            }
        }
    }
    None
}

/// `0..n` with each after everything it waits for (`needs`, which has no cycle).
fn order(n: usize, needs: impl Fn(usize) -> Vec<usize>) -> Vec<usize> {
    let mut out = Vec::with_capacity(n);
    let mut state = vec![0u8; n];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut path: Vec<(usize, Vec<usize>, usize)> = vec![(root, needs(root), 0)];
        state[root] = 1;
        while let Some((at, list, k)) = path.last_mut() {
            if *k >= list.len() {
                state[*at] = 2;
                out.push(*at);
                path.pop();
                continue;
            }
            let next = list[*k];
            *k += 1;
            if state[next] == 0 {
                state[next] = 1;
                path.push((next, needs(next), 0));
            }
        }
    }
    out
}
