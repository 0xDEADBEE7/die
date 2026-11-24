//! Gantt charts: `"type": "gantt"`, tasks over time, a row each.
//!
//! The tasks are rows: `tasks`, a list of objects in the spec, or `data`, as any chart's (a
//! file, inline values, a command, the buffer, a dashboard's named data). Exactly one of them.
//! `tasks` is read as `{"values": […]}` is, so either way the model sees a table of a task a
//! row, with each row's object and `fig` block when the rows came from JSON
//! ([`crate::gantt::GanttModel`] says what a task's fields mean).
//!
//! Its keys, all optional but the tasks:
//!
//! - `title`, `subtitle`: strings;
//! - `tasks` or `data`, and `transform`, a chart's transforms run over the rows;
//! - `id`, `label`, `start`, `end`, `duration`, `after`, `group`, `parent`, `progress`,
//!   `milestone` and `color`: which column is what, each a channel that names a field (`"start":
//!   "begins"`, or an object with `field` and, for epoch numbers, `"type": "time"` and `unit`).
//!   One left out is found by its usual names ([`names`]); `color` left out colours by group;
//! - `tooltip` and `detail`: a field or a list, more for the card and what the detail panel
//!   shows;
//! - `calendar`: the days a duration counts in a plan of whole days: `"all"` (the default),
//!   `"weekdays"` (Monday to Friday), or `{"days": …, "off": […]}` with days off besides, each a
//!   date or `{"start": date, "end": date}`;
//! - `today`: `true` (the default) for a line at now, `false` for none, or a time for a line
//!   there;
//! - `marks`: a list, each `{"at": time}` (a line) or `{"start": time, "end": time}` (a shaded
//!   span), with a `label` and a `fig` block;
//! - `view`: `grid`, `axes`, `legend` (`top`, `right`, `none`), `links` (the arrows between
//!   tasks, on unless it says), `render_options` and `motion`.
//!
//! [`from_table`] makes a gantt of a bare table that has a `start` column of times and an
//! `end` or a `duration`: a plan from a CSV.

use super::chart::{channel, data_of, fig_block, transforms, view_settings_of};
use super::{
    Calendar, Channel, Cx, DataRef, DataSpec, GanttFields, GanttMark, GanttSpec, ReadHints, Today,
    ViewSettings, When, arr, field, obj, opt_bool, opt_string, strings,
};
use crate::data::time::parse_time;
use crate::data::{Column, ColumnKind, Table};
use crate::error::{FigError, Result};
use crate::json::{Kind, Node};
use std::sync::Arc;

/// The names each of a gantt's columns goes by, in any case, the first found winning.
pub mod names {
    /// A task's id.
    pub const ID: &[&str] = &["id"];
    /// Its name.
    pub const LABEL: &[&str] = &["label", "name", "task", "title"];
    /// When it starts.
    pub const START: &[&str] = &["start"];
    /// When it ends.
    pub const END: &[&str] = &["end", "finish"];
    /// How long it takes.
    pub const DURATION: &[&str] = &["duration"];
    /// What it comes after.
    pub const AFTER: &[&str] = &["after", "depends", "depends_on"];
    /// Its group.
    pub const GROUP: &[&str] = &["group", "section"];
    /// What it's a part of.
    pub const PARENT: &[&str] = &["parent", "parent_id"];
    /// The share done.
    pub const PROGRESS: &[&str] = &["progress"];
    /// Whether it's a milestone.
    pub const MILESTONE: &[&str] = &["milestone"];
}

/// The channels that say which column is what, in the order they're written.
pub const FIELDS: [&str; 11] = [
    "id",
    "label",
    "start",
    "end",
    "duration",
    "after",
    "group",
    "parent",
    "progress",
    "milestone",
    "color",
];

/// A gantt's keys.
pub const TOP: &[&str] = &[
    "type",
    "title",
    "subtitle",
    "tasks",
    "data",
    "transform",
    "id",
    "label",
    "start",
    "end",
    "duration",
    "after",
    "group",
    "parent",
    "progress",
    "milestone",
    "color",
    "tooltip",
    "detail",
    "calendar",
    "today",
    "marks",
    "view",
    "fig",
];
/// The keys of a gantt's `calendar`.
pub const CALENDAR: &[&str] = &["days", "off"];
/// The keys of a gantt's `view`.
pub const VIEW: &[&str] = &[
    "grid",
    "axes",
    "legend",
    "links",
    "render_options",
    "motion",
];
/// A mark's keys.
pub const MARK: &[&str] = &["at", "start", "end", "label", "fig"];

impl GanttFields {
    /// Each channel with its key, in [`FIELDS`]' order.
    pub fn each(&self) -> [(&'static str, &Option<Channel>); 11] {
        [
            ("id", &self.id),
            ("label", &self.label),
            ("start", &self.start),
            ("end", &self.end),
            ("duration", &self.duration),
            ("after", &self.after),
            ("group", &self.group),
            ("parent", &self.parent),
            ("progress", &self.progress),
            ("milestone", &self.milestone),
            ("color", &self.color),
        ]
    }

    fn slot(&mut self, key: &str) -> Option<&mut Option<Channel>> {
        Some(match key {
            "id" => &mut self.id,
            "label" => &mut self.label,
            "start" => &mut self.start,
            "end" => &mut self.end,
            "duration" => &mut self.duration,
            "after" => &mut self.after,
            "group" => &mut self.group,
            "parent" => &mut self.parent,
            "progress" => &mut self.progress,
            "milestone" => &mut self.milestone,
            "color" => &mut self.color,
            _ => return None,
        })
    }
}

/// A gantt from its spec.
pub(crate) fn parse(node: &Node, cx: &mut Cx<'_>) -> Result<GanttSpec> {
    let o = obj(node, "a gantt")?;
    cx.check_top(o, TOP);
    let (data, tasks) = rows(node, cx)?;
    let mut fields = GanttFields::default();
    for key in FIELDS {
        let Some(n) = field(node, key) else { continue };
        let c = channel(n, key, cx)?;
        if c.field.is_none() {
            return Err(FigError::at(
                format!("\"{key}\" says which column it is: give a field's name"),
                n.at,
            ));
        }
        if let Some(slot) = fields.slot(key) {
            *slot = Some(c);
        }
    }
    let list = |key: &str| field(node, key).map(|n| strings(n, key)).transpose();
    let view = view_settings_of(node, ViewSettings::default(), VIEW, cx)?;
    let links = match field(node, "view") {
        Some(v) => opt_bool(v, "links")?.unwrap_or(true),
        None => true,
    };
    Ok(GanttSpec {
        title: opt_string(node, "title")?,
        subtitle: opt_string(node, "subtitle")?,
        data,
        tasks,
        transform: transforms(node.get("transform"), cx)?,
        fields: Box::new(fields),
        calendar: calendar(node, cx)?,
        today: today(node, cx)?,
        marks: marks(node, cx)?,
        tooltip: list("tooltip")?.unwrap_or_default(),
        detail: list("detail")?.unwrap_or_default(),
        links,
        view,
    })
}

/// The tasks' rows, and whether they were written as `tasks`.
fn rows(node: &Node, cx: &mut Cx<'_>) -> Result<(DataSpec, bool)> {
    let Some(tasks) = field(node, "tasks") else {
        let named = cx.names.as_ref().is_some_and(|n| !n.is_empty());
        if field(node, "data").is_none() && !named {
            return Err(FigError::at("a gantt needs \"tasks\" or \"data\"", node.at));
        }
        return Ok((data_of(node, cx)?, false));
    };
    if let Some(d) = field(node, "data") {
        return Err(FigError::at(
            "a gantt has \"tasks\" and \"data\"; give one",
            d.at,
        ));
    }
    let list = arr(tasks, "tasks")?;
    if let Some(bad) = list.iter().find(|t| t.as_obj().is_none()) {
        return Err(FigError::at(
            format!("a task must be an object, not {}", super::a(bad)),
            bad.at,
        ));
    }
    let table = crate::data::rows::read_json(tasks, cx.opt)?;
    let data = DataSpec {
        source: DataRef::Inline(Arc::new(table)),
        format: None,
        read: ReadHints::default(),
    };
    Ok((data, true))
}

/// A time a spec gives as a string, kept with its text.
fn when(n: &Node, key: &str, cx: &Cx<'_>) -> Result<When> {
    let bad = || {
        FigError::at(
            format!("\"{key}\" must be a time, not {}", n.to_text()),
            n.at,
        )
    };
    let text = n.as_str().ok_or_else(bad)?;
    let secs = parse_time(text, cx.opt).ok_or_else(bad)?;
    Ok(When {
        secs,
        text: text.into(),
    })
}

/// `calendar`: `"all"`, `"weekdays"`, or an object of `days` and `off`.
fn calendar(node: &Node, cx: &mut Cx<'_>) -> Result<Calendar> {
    let Some(n) = field(node, "calendar") else {
        return Ok(Calendar::default());
    };
    let days = |d: &Node, key: &str| match d.as_str() {
        Some("all") => Ok(false),
        Some("weekdays") => Ok(true),
        _ => Err(FigError::at(
            format!(
                "\"{key}\" must be \"all\" or \"weekdays\", not {}",
                d.to_text()
            ),
            d.at,
        )),
    };
    if n.as_obj().is_none() {
        return Ok(Calendar {
            weekdays: days(n, "calendar")?,
            off: Vec::new(),
        });
    }
    cx.check(obj(n, "\"calendar\"")?, CALENDAR);
    let weekdays = field(n, "days").map(|d| days(d, "days")).transpose()?;
    let mut off = Vec::new();
    if let Some(list) = field(n, "off") {
        for d in arr(list, "off")? {
            if d.as_obj().is_none() {
                off.push((when(d, "off", cx)?, None));
                continue;
            }
            let (Some(s), Some(e)) = (field(d, "start"), field(d, "end")) else {
                return Err(FigError::at(
                    "days off are a date, or a \"start\" and an \"end\"",
                    d.at,
                ));
            };
            let (start, end) = (when(s, "start", cx)?, when(e, "end", cx)?);
            if end.secs < start.secs {
                return Err(FigError::at(
                    "days off must \"end\" on or after their \"start\"",
                    e.at,
                ));
            }
            off.push((start, Some(end)));
        }
    }
    Ok(Calendar {
        weekdays: weekdays.unwrap_or(false),
        off,
    })
}

/// `today`: true, false, or a time.
fn today(node: &Node, cx: &Cx<'_>) -> Result<Today> {
    let Some(n) = field(node, "today") else {
        return Ok(Today::Now);
    };
    let bad = || {
        FigError::at(
            format!(
                "\"today\" must be true, false or a time, not {}",
                n.to_text()
            ),
            n.at,
        )
    };
    match &n.kind {
        Kind::Bool(true) => Ok(Today::Now),
        Kind::Bool(false) => Ok(Today::Off),
        Kind::Str(s) => parse_time(s, cx.opt).map(Today::At).ok_or_else(bad),
        _ => Err(bad()),
    }
}

/// `marks`: lines and spans on the time.
fn marks(node: &Node, cx: &mut Cx<'_>) -> Result<Vec<GanttMark>> {
    let Some(list) = field(node, "marks") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for m in arr(list, "marks")? {
        let o = obj(m, "a mark")?;
        cx.check(o, MARK);
        let (at, end) = match (field(m, "at"), field(m, "start"), field(m, "end")) {
            (Some(at), None, None) => (when(at, "at", cx)?, None),
            (None, Some(s), Some(e)) => (when(s, "start", cx)?, Some((when(e, "end", cx)?, e.at))),
            _ => {
                return Err(FigError::at(
                    "a mark needs \"at\", or \"start\" and \"end\"",
                    m.at,
                ));
            }
        };
        let end = match end {
            None => None,
            // A date alone may be the day the span starts on: it takes in that day.
            Some((e, _)) if e.secs > at.secs || (e.secs == at.secs && e.is_date()) => Some(e),
            Some((_, pos)) => {
                return Err(FigError::at(
                    "a mark's \"end\" must be after its \"start\"",
                    pos,
                ));
            }
        };
        out.push(GanttMark {
            at,
            end,
            label: opt_string(m, "label")?,
            fig: field(m, "fig")
                .map(|f| fig_block(f, cx))
                .unwrap_or_default(),
            pos: m.at,
        });
    }
    Ok(out)
}

/// The column named one of `names` (in any case, trimmed), the first of `names` found.
pub fn named<'t>(table: &'t Table, names: &[&str]) -> Option<&'t Column> {
    names.iter().find_map(|want| {
        table
            .columns
            .iter()
            .find(|c| c.name.trim().eq_ignore_ascii_case(want))
    })
}

/// A gantt from a bare table that has a `start` column of times and an `end` (or `finish`)
/// column of times or a `duration` column: every column found by its usual name, the data the
/// table itself. None for any other table.
pub fn from_table(table: &Table) -> Option<GanttSpec> {
    let start = named(table, names::START)?;
    if start.kind() != ColumnKind::Time {
        return None;
    }
    let end = named(table, names::END).is_some_and(|c| c.kind() == ColumnKind::Time);
    if !end && named(table, names::DURATION).is_none() {
        return None;
    }
    Some(GanttSpec {
        title: None,
        subtitle: None,
        data: DataSpec {
            source: DataRef::Source,
            format: None,
            read: ReadHints::default(),
        },
        tasks: false,
        transform: Vec::new(),
        fields: Box::default(),
        calendar: Calendar::default(),
        today: Today::Now,
        marks: Vec::new(),
        tooltip: Vec::new(),
        detail: Vec::new(),
        links: true,
        view: ViewSettings::default(),
    })
}
