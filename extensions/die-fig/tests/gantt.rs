//! The gantt figure: what a task's dates mean (whole days, the calendar, `after`, parents,
//! milestones, conflicts), each error's message and place, the rows' order, colour and the
//! legend, hidden names, the layout's facts, hover, the motions, zoom, pan and the brush, the
//! overlay, the detail tree, the TSV, and the build from a file.

use die_test_cap as _;

use die_fig::FigError;
use die_fig::data::{ReadOptions, SeriesKey};
use die_fig::detail::{Item as Mark, RowKind};
use die_fig::error::ErrorKind;
use die_fig::gantt::detail::task_tree;
use die_fig::gantt::nav;
use die_fig::gantt::{self, GanttLayout, GanttModel, OverlayState, RowKind as Row, Shape, Target};
use die_fig::graph::overlay::card_rect;
use die_fig::interact::{Action, Cursor, Toggles, View};
use die_fig::scene::{FixedMeasure, Hit, Item, Scene};
use die_fig::source::{self, Job, Snapshot, Text};
use die_fig::spec::{self, DataRef, Figure};
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};

/// 2026-11-02T00:00:00Z, a Monday.
const NOV_2: f64 = 1_793_577_600.0;
const DAY: f64 = 86_400.0;
const HOUR: f64 = 3_600.0;
/// The clock the tests' plans are built against: Wed 25 Nov 2026.
const NOW: f64 = NOV_2 + 23.0 * DAY;
const SIZE: (f32, f32) = (880.0, 470.0);

/// Midnight so many days after Mon 2 Nov 2026.
fn day(n: i32) -> f64 {
    NOV_2 + f64::from(n) * DAY
}

fn built(text: &str) -> Result<GanttModel, FigError> {
    let opt = ReadOptions::new(TimeZone::UTC);
    let parsed = spec::parse(text, "plan.fig", Path::new("/plans"), &opt)?;
    let Figure::Gantt(g) = parsed.figure else {
        panic!("not a gantt: {text}")
    };
    let DataRef::Inline(table) = g.data.source.clone() else {
        panic!("its tasks aren't inline: {text}")
    };
    GanttModel::build(g, table, &TimeZone::UTC, NOW)
}

fn model(text: &str) -> GanttModel {
    built(text).unwrap_or_else(|e| panic!("{e} in\n{text}"))
}

/// A gantt of these tasks, a line each, with more of the spec after them.
fn plan_with(tasks: &[&str], rest: &str) -> String {
    format!(
        "{{\"type\": \"gantt\", \"tasks\": [\n{}\n]{rest}}}",
        tasks.join(",\n")
    )
}

/// A gantt of these tasks, with no today line.
fn plan(tasks: &[&str]) -> GanttModel {
    model(&plan_with(tasks, ", \"today\": false"))
}

/// The error these tasks give: its message and its line.
fn fails(tasks: &[&str]) -> (String, u32) {
    let text = plan_with(tasks, "");
    let e = built(&text).expect_err(&text);
    (e.msg.clone(), e.at.map_or(0, |a| a.line))
}

fn at(m: &GanttModel, id: &str) -> usize {
    m.index_of(id).unwrap_or_else(|| panic!("no task {id}"))
}

fn span(m: &GanttModel, id: &str) -> (f64, f64) {
    let t = &m.tasks[at(m, id)];
    (t.start, t.end)
}

fn laid(m: &GanttModel, view: &View) -> (Scene, GanttLayout) {
    gantt::base(m, view, SIZE, 1.0, &Theme::mono_dark(), &mut FixedMeasure)
}

fn texts(scene: &Scene) -> Vec<String> {
    (scene.items.iter())
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn cursor(id: &str) -> Cursor {
    Cursor::Task(id.into())
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-2
}

/// Whether a view's time is `want`, to the millisecond.
fn shows(v: &View, want: (f64, f64)) -> bool {
    v.x.is_some_and(|x| (x.0 - want.0).abs() < 1e-3 && (x.1 - want.1).abs() < 1e-3)
}

#[test]
fn a_date_end_takes_in_its_day() {
    let m = model(&plan_with(
        &[r#"{"id": "spec", "start": "2026-11-02", "end": "2026-11-06"}"#],
        r#", "marks": [{"start": "2026-11-10", "end": "2026-11-11", "label": "Freeze"}]"#,
    ));
    assert!(m.whole_days);
    // Monday to Friday: the bar stops at Saturday's midnight, and the end is written as Friday.
    assert_eq!(span(&m, "spec"), (day(0), day(5)));
    assert_eq!(m.day_count(0), Some(5));
    assert_eq!(
        (m.start_text(0), m.end_text(0), m.duration_text(0)),
        ("Mon 2 Nov".into(), "Fri 6 Nov".into(), "5 days".into())
    );
    // A mark's end written as a date takes in its day too.
    assert_eq!((m.marks[0].start, m.marks[0].end), (day(8), Some(day(10))));

    // With a time of day anywhere, the plan is exact: an end is the instant it says.
    let m = plan(&[
        r#"{"id": "spec", "start": "2026-11-02T09:00", "end": "2026-11-06T17:00"}"#,
        r#"{"id": "next", "after": "spec", "duration": "36h"}"#,
    ]);
    assert!(!m.whole_days);
    assert_eq!(
        span(&m, "spec"),
        (day(0) + 9.0 * HOUR, day(4) + 17.0 * HOUR)
    );
    assert_eq!(m.end_text(0), "Fri 6 Nov 17:00");
    let next = span(&m, "next");
    assert_eq!(next, (day(4) + 17.0 * HOUR, day(6) + 5.0 * HOUR));
    assert_eq!(m.duration_text(1), "36 h");
}

#[test]
fn a_duration_counts_working_days_where_the_calendar_says() {
    let tasks = [
        r#"{"id": "a", "start": "2026-11-02", "duration": "5d"}"#,
        r#"{"id": "b", "after": "a", "duration": "2w"}"#,
        r#"{"id": "c", "start": "2026-11-05", "duration": "6d"}"#,
        r#"{"id": "m", "after": "a", "milestone": true}"#,
        r#"{"id": "sat", "start": "2026-11-07", "duration": "1d"}"#,
        r#"{"id": "back", "end": "2026-11-20", "duration": "3d"}"#,
        r#"{"id": "off", "start": "2026-11-23", "duration": "3d"}"#,
    ];
    let rest = r#", "today": false, "calendar": {"days": "weekdays", "off": ["2026-11-25"]}"#;
    let m = model(&plan_with(&tasks, rest));
    assert!(m.whole_days && m.warnings.is_empty());
    // Five days from a Monday end that Friday.
    assert_eq!(span(&m, "a"), (day(0), day(5)));
    assert_eq!(m.end_text(at(&m, "a")), "Fri 6 Nov");
    // After a task that ends on a Friday, the next starts on the Monday; two weeks are ten
    // working days.
    assert_eq!(span(&m, "b"), (day(7), day(19)));
    assert_eq!(m.duration_text(at(&m, "b")), "2 weeks");
    // Six days from a Thursday end the next Thursday: the bar runs across the weekend.
    assert_eq!(span(&m, "c"), (day(3), day(11)));
    assert_eq!(m.end_text(at(&m, "c")), "Thu 12 Nov");
    // A milestone placed by `after` is at the start of the next working day.
    assert_eq!(span(&m, "m"), (day(7), day(7)));
    // A date the spec gives stands, day off or not; its day is the next that counts.
    assert_eq!(span(&m, "sat"), (day(5), day(8)));
    // A start worked back from an end is never on a day off: Wed, Thu, Fri.
    assert_eq!(span(&m, "back"), (day(16), day(19)));
    // A day the calendar lists as off doesn't count: Mon, Tue, Thu.
    assert_eq!(span(&m, "off"), (day(21), day(25)));

    // Every day counts unless the calendar says: the next starts on the Saturday, and two
    // weeks are fourteen days.
    let m = model(&plan_with(&tasks[..4], r#", "today": false"#));
    assert_eq!(span(&m, "a"), (day(0), day(5)));
    assert_eq!(span(&m, "b"), (day(5), day(19)));
    assert_eq!(span(&m, "c"), (day(3), day(9)));
    assert_eq!(span(&m, "m"), (day(5), day(5)));
    assert_eq!(m.duration_text(at(&m, "b")), "2 weeks");
}

#[test]
fn a_plan_with_times_of_day_leaves_its_calendar_unused_and_says_so() {
    let m = model(&plan_with(
        &[
            r#"{"id": "a", "start": "2026-11-06T12:00", "duration": "1d"}"#,
            r#"{"id": "b", "after": "a", "duration": "1w", "progress": 140}"#,
        ],
        r#", "calendar": "weekdays""#,
    ));
    // Friday noon and a day is Saturday noon; a week is seven days.
    assert_eq!(span(&m, "a"), (day(4) + 12.0 * HOUR, day(5) + 12.0 * HOUR));
    assert_eq!(span(&m, "b"), (day(5) + 12.0 * HOUR, day(12) + 12.0 * HOUR));
    let said: Vec<&str> = m.warnings.iter().map(|w| &*w.msg).collect();
    assert_eq!(
        said,
        [
            "\"calendar\" counts whole days, and this plan has times of day: every hour counts",
            "progress 140 is above 100: drawn as done"
        ]
    );
    // The warning about a task is at its place.
    assert_eq!(m.warnings[1].at.map(|a| a.line), Some(3));
    assert_eq!(m.tasks[1].progress, Some(1.0));
}

#[test]
fn a_parent_with_no_dates_spans_its_children() {
    let m = plan(&[
        r#"{"id": "epic", "label": "Epic"}"#,
        r#"{"id": "p1", "parent": "epic", "start": "2026-11-02", "duration": "2d", "progress": 100}"#,
        r#"{"id": "p2", "parent": "epic", "start": "2026-11-09", "duration": "6d", "progress": 0}"#,
        r#"{"id": "own", "start": "2026-11-01", "end": "2026-11-30"}"#,
        r#"{"id": "kid", "parent": "own", "start": "2026-11-10", "duration": "1d"}"#,
    ]);
    assert_eq!(span(&m, "epic"), (day(0), day(13)));
    // Its progress is its children's, weighted by their length: two days of eight done.
    assert_eq!(m.tasks[0].progress, None);
    assert_eq!(m.tasks[0].done, Some(0.25));
    // Any progress above 1 makes the column percent.
    assert_eq!(m.tasks[1].progress, Some(1.0));
    // Dates of its own stand.
    assert_eq!(span(&m, "own"), (day(-1), day(29)));
    assert_eq!(
        (
            m.tasks[0].children.clone(),
            m.tasks[2].parent,
            m.tasks[2].depth
        ),
        (vec![1, 2], Some(0), 1)
    );
    let (_, l) = laid(&m, &View::default());
    assert_eq!(l.task(0).unwrap().shape, Shape::Summary);
    assert_eq!(l.task(1).unwrap().shape, Shape::Bar);
}

#[test]
fn one_date_alone_or_milestone_true_is_a_moment() {
    let m = plan(&[
        r#"{"id": "s", "start": "2026-11-04"}"#,
        r#"{"id": "e", "end": "2026-11-06"}"#,
        r#"{"id": "f", "start": "2026-11-02", "duration": "3d"}"#,
        r#"{"id": "m", "after": "f"}"#,
        r#"{"id": "t", "start": "2026-11-09", "end": "2026-11-13", "milestone": true}"#,
    ]);
    assert_eq!(span(&m, "s"), (day(2), day(2)));
    // An end alone is a moment at the time it says: it takes in no day.
    assert_eq!(span(&m, "e"), (day(4), day(4)));
    assert_eq!(span(&m, "m"), (day(3), day(3)));
    assert_eq!(span(&m, "t"), (day(7), day(7)));
    let moments: Vec<bool> = m.tasks.iter().map(|t| t.milestone).collect();
    assert_eq!(moments, [true, true, false, true, true]);
    assert_eq!(m.duration_text(0), "");
    let (_, l) = laid(&m, &View::default());
    let g = l.task(0).unwrap();
    assert_eq!(g.shape, Shape::Milestone);
    // The diamond is centred on its moment, 12 px across.
    assert!(close(g.bar.x + 6.0, l.px(day(2))) && g.bar.w == 12.0);
    assert_eq!(g.date.as_ref().map(|d| &*d.text), Some("4 Nov"));

    // In a table: true, yes, y or 1, in any case.
    let m = model(
        r#"{"type": "gantt", "today": false, "data": {"values":
        "task,start,end,milestone\nA,2026-11-02,2026-11-06,Yes\nB,2026-11-02,2026-11-06,no\nC,2026-11-02,2026-11-06,1\n"}}"#,
    );
    let moments: Vec<bool> = m.tasks.iter().map(|t| t.milestone).collect();
    assert_eq!(moments, [true, false, true]);
}

#[test]
fn a_task_without_an_id_is_named_by_its_label() {
    let m = plan(&[
        r#"{"label": "Draw", "start": "2026-11-02", "duration": "2d"}"#,
        r#"{"label": "Draw", "after": "Draw", "duration": "1d"}"#,
        r#"{"label": "Build", "after": "ship", "duration": "1d"}"#,
        r#"{"id": "ship", "label": "Ship", "after": "Draw", "milestone": true}"#,
        r#"{"id": 7, "after": "ship", "duration": "1d"}"#,
    ]);
    // A label two tasks share names the first; the second's key is made unique. An id may be
    // a number, and a task with no label is called by its id.
    let keys: Vec<&str> = m.tasks.iter().map(|t| &*t.key).collect();
    assert_eq!(keys, ["Draw", "Draw (2)", "Build", "ship", "7"]);
    assert_eq!(m.label(4), "7");
    assert_eq!(m.tasks[1].after, [0]);
    assert_eq!(m.tasks[2].after, [3]);
    assert_eq!(m.cursor(1), cursor("Draw (2)"));
    assert_eq!(m.index_of("Draw (2)"), Some(1));
    // A search finds a task by its label, and by its key where that differs.
    let names: Vec<String> = m.names().into_iter().map(|n| n.0).collect();
    assert_eq!(
        names,
        ["Draw", "Draw", "Draw (2)", "Build", "Ship", "ship", "7"]
    );
}

#[test]
fn a_task_that_starts_before_one_it_comes_after_ends_is_in_conflict() {
    let m = plan(&[
        r#"{"id": "api", "label": "API", "start": "2026-11-02", "end": "2026-11-06"}"#,
        r#"{"id": "ui", "label": "UI", "start": "2026-11-05", "after": "api", "duration": "3d"}"#,
        r#"{"id": "qa", "label": "QA", "after": ["api", "ui"], "duration": "1d"}"#,
    ]);
    // The start stands, and the arrow is still drawn.
    assert_eq!(span(&m, "ui"), (day(3), day(6)));
    assert_eq!(m.tasks[1].late, Some((0, 2.0 * DAY)));
    assert_eq!(
        m.late_text(1).as_deref(),
        Some("Starts 2 days before API ends")
    );
    let conflicts: Vec<(u32, u32, bool)> =
        m.links.iter().map(|l| (l.from, l.to, l.conflict)).collect();
    assert_eq!(conflicts, [(0, 1, true), (0, 2, false), (1, 2, false)]);
    // One worked out from `after` starts when the last of them ends.
    assert_eq!(span(&m, "qa"), (day(6), day(7)));
    assert_eq!((m.tasks[2].late, m.late_text(2)), (None, None));
    assert_eq!(
        (m.tasks[0].then.clone(), m.tasks[2].after.clone()),
        (vec![1, 2], vec![0, 1])
    );
}

#[test]
fn each_error_says_what_is_wrong_at_the_task_s_place() {
    let dated = r#"{"id": "a", "start": "2026-11-02"}"#;
    let cases: &[(&[&str], &str, u32)] = &[
        (
            &[dated, r#"{"id": "a", "start": "2026-11-03"}"#],
            "task ids must be unique: a is given twice",
            3,
        ),
        (
            &[dated, r#"{"id": "b", "after": "y", "duration": "1d"}"#],
            "unknown task \"y\" in \"after\"",
            3,
        ),
        (
            &[
                dated,
                r#"{"id": "b", "parent": "z", "start": "2026-11-03"}"#,
            ],
            "unknown task \"z\" in \"parent\"",
            3,
        ),
        (
            &[
                dated,
                r#"{"id": "b", "after": "d", "duration": "1d"}"#,
                r#"{"id": "c", "after": "b", "duration": "1d"}"#,
                r#"{"id": "d", "after": "c", "duration": "1d"}"#,
            ],
            "\"b\" comes after itself: b → c → d → b",
            3,
        ),
        (
            &[
                r#"{"id": "p", "parent": "q", "start": "2026-11-02"}"#,
                r#"{"id": "q", "parent": "p", "start": "2026-11-02"}"#,
            ],
            "\"p\" is part of itself: p → q → p",
            2,
        ),
        (
            &[r#"{"id": "a", "start": "2026-11-06", "end": "2026-11-02"}"#],
            "\"end\" is before \"start\"",
            2,
        ),
        (
            &[dated, r#"{"id": "b", "start": "soon"}"#],
            "\"start\" must be a time, not \"soon\"",
            3,
        ),
        (
            &[r#"{"id": "a", "start": "2026-11-02", "duration": "long"}"#],
            "\"duration\" must be like \"3d\", \"2w\" or \"36h\", not \"long\"",
            2,
        ),
        (
            &[r#"{"id": "a", "start": "2026-11-02", "progress": "half"}"#],
            "\"progress\" must be a number, not \"half\"",
            2,
        ),
        (
            &[dated, r#"{"id": "x", "label": "X"}"#],
            "task \"x\" has no start: give it \"start\", or \"after\" and a \"duration\"",
            3,
        ),
    ];
    for (tasks, msg, line) in cases {
        assert_eq!(fails(tasks), (msg.to_string(), *line), "{tasks:?}");
    }
    // A plan of no tasks is nothing to draw, not a mistake.
    let e = built(r#"{"type": "gantt", "tasks": []}"#).unwrap_err();
    assert_eq!(
        (e.msg.as_str(), e.kind),
        ("nothing to draw: the plan has no tasks", ErrorKind::Empty)
    );
    // A channel naming a column the data hasn't: at the channel.
    let text = "{\"type\": \"gantt\",\n \"data\": {\"values\": \"task,start,end\\nA,2026-11-02,2026-11-06\\n\"},\n \"start\": \"begins\"}";
    let e = built(text).unwrap_err();
    assert_eq!(e.msg, "no column \"begins\" in the data");
    assert_eq!(e.at.map(|a| (a.line, a.col)), Some((3, 11)));
    // A table's row is placed by its line: the header's is 1.
    let text = r#"{"type": "gantt", "data": {"values":
        "task,start,duration\nA,2026-11-02,2d\nB,2026-11-02,long\n"}}"#;
    let e = built(text).unwrap_err();
    assert!(e.msg.ends_with("not \"long\""), "{e}");
    assert_eq!(e.at.map(|a| (a.line, a.col)), Some((3, 1)));
}

#[test]
fn rows_come_under_their_groups_and_children_under_their_parents() {
    let on = |d: u32| format!("\"start\": \"2026-11-{d:02}\", \"duration\": \"2d\"");
    let tasks = [
        format!(r#"{{"id": "u", "label": "Loose", {}}}"#, on(2)),
        format!(
            r#"{{"id": "d1", "label": "Draw", "group": "Design", {}}}"#,
            on(3)
        ),
        format!(
            r#"{{"id": "b1", "label": "Build it", "group": "Build", {}}}"#,
            on(4)
        ),
        format!(
            r#"{{"id": "d2", "label": "Review", "group": "Design", {}}}"#,
            on(5)
        ),
        format!(
            r#"{{"id": "b2", "label": "Part", "group": "Design", "parent": "b1", {}}}"#,
            on(6)
        ),
        format!(
            r#"{{"id": "b3", "label": "Deep", "parent": "b2", {}}}"#,
            on(7)
        ),
    ];
    let tasks: Vec<&str> = tasks.iter().map(String::as_str).collect();
    let m = plan(&tasks);
    assert_eq!(m.groups, ["Design", "Build"]);
    let (_, l) = laid(&m, &View::default());
    // Tasks with no group first, under no heading; then the groups as first seen; a child
    // under its parent, whatever its own group.
    let kinds: Vec<Row> = l.rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            Row::Task(0),
            Row::Heading(0),
            Row::Task(1),
            Row::Task(3),
            Row::Heading(1),
            Row::Task(2),
            Row::Task(4),
            Row::Task(5),
        ]
    );
    // A task's row is 28 px and a heading's 30, down from the plot's top.
    let tops: Vec<f32> = l.rows.iter().map(|r| r.y - l.plot.y).collect();
    assert_eq!(tops, [0.0, 28.0, 58.0, 86.0, 114.0, 144.0, 172.0, 200.0]);
    // Labels are indented 14 px a level.
    let indent: Vec<f32> = l.rows.iter().map(|r| r.label.x - l.gutter.x).collect();
    assert_eq!(indent, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 14.0, 28.0]);
    // A child keeps its own group's colour.
    let theme = Theme::mono_dark();
    assert_eq!(m.color(4, &theme), theme.series[0]);
    assert_eq!(m.color(2, &theme), theme.series[1]);
    // A heading's line runs across its group's whole span.
    let (a, b, _) = l.rows[4].line.unwrap();
    assert!(close(a, l.px(day(2))) && close(b, l.px(day(7))), "{a} {b}");

    // No group anywhere: no headings.
    let m = plan(&[tasks[0]]);
    let (_, l) = laid(&m, &View::default());
    assert_eq!(l.rows.len(), 1);
}

#[test]
fn colour_is_by_group_unless_a_field_is_named_and_then_a_legend_lists_it() {
    let theme = Theme::mono_dark();
    let on = r#""start": "2026-11-02", "duration": "2d""#;
    let tasks = [
        format!(r#"{{"id": "a", "group": "Design", "owner": "Sol", {on}}}"#),
        format!(r#"{{"id": "b", "group": "Build", "owner": "Lena", {on}}}"#),
        format!(
            r#"{{"id": "c", "group": "Build", "owner": "Sol", {on}, "fig": {{"color": "critical"}}}}"#
        ),
        format!(r#"{{"id": "d", "group": "Build", {on}}}"#),
    ];
    let tasks: Vec<&str> = tasks.iter().map(String::as_str).collect();
    // By group: the headings say it, so no legend.
    let m = plan(&tasks);
    assert!(m.by_group && !m.has_legend());
    assert_eq!(m.names, ["Design", "Build"]);
    assert_eq!(m.color(0, &theme), theme.series[0]);
    assert_eq!(m.color(1, &theme), theme.series[1]);
    // A task's own colour beats its name's.
    assert_eq!(m.color(2, &theme), theme.status[3]);
    let (_, l) = laid(&m, &View::default());
    assert!(l.items.is_empty() && l.legend.is_empty());

    // By a field: its names take the slots as first seen, and a legend lists them.
    let m = model(&plan_with(&tasks, r#", "today": false, "color": "owner""#));
    assert!(!m.by_group && m.has_legend());
    assert_eq!(m.names, ["Sol", "Lena"]);
    assert_eq!(
        m.keys(),
        [SeriesKey::new("Sol", 0), SeriesKey::new("Lena", 0)]
    );
    assert_eq!(m.color(1, &theme), theme.series[1]);
    // A task with no name among tasks with one is the neutral.
    assert_eq!(m.color(3, &theme), theme.other);
    let (scene, l) = laid(&m, &View::default());
    let names: Vec<&str> = l.items.iter().map(|i| &*i.name.text).collect();
    assert_eq!(names, ["Sol", "Lena"]);
    let key = l.legend[1].1;
    assert_eq!(
        scene.hit(key.x + 4.0, key.y + 4.0),
        Some(&Hit::LegendKey(SeriesKey::new("Lena", 0)))
    );
    // `"legend": "none"` draws none; one name alone needs none.
    let m = model(&plan_with(
        &tasks,
        r#", "color": "owner", "view": {"legend": "none"}"#,
    ));
    assert!(!m.has_legend());
    let m = model(&plan_with(&tasks[..1], r#", "color": "owner""#));
    assert!(!m.has_legend());

    // No group and no field: every bar in slot 1. A ninth name is the neutral.
    let m = plan(&[r#"{"id": "a", "start": "2026-11-02", "duration": "2d"}"#]);
    assert_eq!(m.color(0, &theme), theme.series[0]);
    let nine: Vec<String> = (0..9)
        .map(|i| format!(r#"{{"id": "t{i}", "group": "g{i}", {on}}}"#))
        .collect();
    let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
    let m = plan(&nine);
    assert_eq!(m.color(7, &theme), theme.series[7]);
    assert_eq!(m.color(8, &theme), theme.other);
}

#[test]
fn a_hidden_name_s_tasks_have_no_rows() {
    let m = plan(&[
        r#"{"id": "a", "group": "Design", "start": "2026-11-02", "duration": "2d"}"#,
        r#"{"id": "b", "group": "Build", "after": "a", "duration": "1d"}"#,
        r#"{"id": "c", "group": "Build", "after": "b", "duration": "1d", "fig": {"hidden": true}}"#,
    ]);
    // A task whose `fig` block hides it has no row.
    let (_, l) = laid(&m, &View::default());
    assert_eq!(l.order(), [0, 1]);
    assert_eq!(l.links.len(), 1);
    // A hidden name's tasks go, their heading with them, and the links to them.
    let mut view = m.initial_view();
    view.hidden.insert(SeriesKey::new("Build", 0));
    let (_, l) = laid(&m, &view);
    let kinds: Vec<Row> = l.rows.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Row::Heading(0), Row::Task(0)]);
    assert!(l.links.is_empty() && l.task(1).is_none());
    // The time shown stays the whole plan's.
    assert_eq!(l.domain, m.fit());
    // Every name hidden: the plot says so.
    view.hidden.insert(SeriesKey::new("Design", 0));
    let (scene, l) = laid(&m, &view);
    assert!(l.rows.is_empty());
    assert!(texts(&scene).iter().any(|t| t.contains("hidden")));
}

/// A plan of three tasks in a row, for the layout's and the overlay's facts.
fn three() -> GanttModel {
    model(&plan_with(
        &[
            r#"{"id": "a", "label": "Write the spec", "start": "2026-11-02", "duration": "5d", "owner": "Sol"}"#,
            r#"{"id": "b", "label": "Build", "after": "a", "duration": "9d"}"#,
            r#"{"id": "m", "label": "Ship", "after": "b", "milestone": true}"#,
        ],
        r#", "title": "Plan", "today": false, "tooltip": "owner""#,
    ))
}

#[test]
fn the_layout_at_880_by_470() {
    let m = three();
    let (scene, l) = laid(&m, &View::default());
    // The gutter is as wide as its widest label (14 characters of 7.2 px), 16 px from the plot.
    assert_eq!((l.gutter.x, l.gutter.w), (28.0, 101.0));
    assert_eq!((l.plot.x, l.plot.right()), (145.0, 852.0));
    // The axis is over the plot; with no today line and no marks, no strip of their labels.
    assert!(l.axis.bottom() <= l.plot.y && l.axis.y >= l.title.bottom());
    assert!(l.strip.is_empty());
    // The plan fits padded out to whole days: Sun 1 Nov to Tue 17 Nov, 16 days across 707 px.
    assert_eq!(l.domain, (day(-1), day(15)));
    assert_eq!(m.summary(), "3 tasks · 2 Nov to 16 Nov");
    let per_day = 707.0 / 16.0;
    // A bar runs from its start to its end, 14 px high, centred in its 28 px row.
    let a = l.task(0).unwrap();
    assert!(close(a.bar.x, 145.0 + per_day) && close(a.bar.w, 5.0 * per_day));
    assert_eq!((a.bar.y, a.bar.h), (l.plot.y + 7.0, 14.0));
    let b = l.task(1).unwrap();
    assert!(close(b.bar.x, a.bar.right()) && close(b.bar.right(), l.px(day(14))));
    assert_eq!(b.strip.y, l.plot.y + 28.0);
    // The rows don't stretch: three of them, and no scrollbar.
    assert_eq!((l.content, l.scroll, l.scrollbar), (84.0, 0.0, None));
    // A link to a task that starts where the other ends goes round between the rows.
    let link = &l.links[0];
    assert_eq!((link.from, link.to, link.pts.len()), (0, 1, 6));
    assert_eq!(link.pts[0], a.out());
    assert_eq!(link.pts[2].1, a.strip.bottom());
    assert_eq!(link.arrow[0], b.into());
    // The ticks are labelled in the axis strip, each centred on its gridline.
    assert!(!l.tick_labels.is_empty());
    assert!(
        l.tick_labels
            .iter()
            .all(|t| t.y > l.axis.y && t.y < l.axis.bottom())
    );
    // What the pointer finds: the bar, the label, the plot, the axis.
    let mid = (a.bar.x + a.bar.w / 2.0, a.mid());
    assert_eq!(scene.hit(mid.0, mid.1), Some(&Hit::Task("a".into())));
    assert_eq!(scene.hit(40.0, b.mid()), Some(&Hit::Task("b".into())));
    assert_eq!(scene.hit(700.0, a.mid()), Some(&Hit::Plot(0)));
    assert_eq!(scene.hit(400.0, l.axis.y + 4.0), Some(&Hit::AxisX(0)));
    assert!(texts(&scene).contains(&"Write the spec".to_string()));
}

#[test]
fn a_long_plan_scrolls_under_the_axis() {
    let tasks: Vec<String> = (0..30)
        .map(|i| {
            format!(
                r#"{{"id": "t{i}", "start": "2026-11-02", "duration": "{}d"}}"#,
                i + 1
            )
        })
        .collect();
    let tasks: Vec<&str> = tasks.iter().map(String::as_str).collect();
    let m = plan(&tasks);
    let (_, l) = laid(&m, &View::default());
    assert_eq!(l.content, 840.0);
    assert!(l.max_scroll() > 0.0 && l.scrollbar.is_some());
    let mut view = View {
        y: Some((100.0, 100.0 + f64::from(l.plot.h))),
        ..View::default()
    };
    let (_, s) = laid(&m, &view);
    assert_eq!((s.scroll, s.rows[0].y), (100.0, s.plot.y - 100.0));
    // The axis stays put, and the thumb moves down its track.
    assert_eq!((s.axis, s.plot), (l.axis, l.plot));
    assert!(s.scrollbar.unwrap().1.y > l.scrollbar.unwrap().1.y);
    // No further than the last row at the plot's foot.
    view.y = Some((1e6, 1e6));
    let (_, s) = laid(&m, &view);
    assert_eq!(s.scroll, 840.0 - s.plot.h);
    assert!(close(s.rows[29].y + 28.0, s.plot.bottom()));
    // Moving the cursor to a row out of sight scrolls it in; the wheel scrolls the rows.
    let v = nav::reveal(&m, &l, &View::default(), &cursor("t29"));
    assert_eq!(v.y.map(|y| y.0 as f32), Some(840.0 - l.plot.h));
    let v = nav::wheel(&m, &l, &View::default(), 0.0, -56.0);
    assert_eq!((v.x, v.y.map(|y| y.0)), (None, Some(56.0)));
    // H, M and L go to the top, middle and bottom task in view.
    let edge = |e: i8| nav::motion(&m, &s, None, &Action::ViewEdge(e), 1);
    assert_eq!(edge(1), Some(cursor("t29")));
    let top = nav::tasks_in_view(&s)[0];
    assert_eq!(edge(-1), Some(m.cursor(top)));
    assert!(top > 0 && edge(0).is_some_and(|c| c != m.cursor(top) && c != cursor("t29")));
}

#[test]
fn hover_finds_the_task_under_a_point() {
    let m = three();
    let (_, l) = laid(&m, &View::default());
    let (a, b, s) = (l.task(0).unwrap(), l.task(1).unwrap(), l.task(2).unwrap());
    let hover = |x: f32, y: f32| gantt::hover(&m, &l, x, y);
    // On its bar, and a little round it; on its label; on a milestone's diamond.
    assert_eq!(hover(a.bar.x + 10.0, a.mid()), Some(Target::Task(0)));
    assert_eq!(
        hover(b.bar.right() + 3.0, b.mid() + 9.0),
        Some(Target::Task(1))
    );
    assert_eq!(hover(l.gutter.x + 5.0, b.mid()), Some(Target::Task(1)));
    assert_eq!(hover(s.bar.x + 6.0, s.mid()), Some(Target::Task(2)));
    assert_eq!(Target::Task(2).cursor(&m), cursor("m"));
    // Off any bar, off the rows, on the axis: nothing.
    assert_eq!(hover(a.bar.right() + 60.0, a.mid()), None);
    assert_eq!(hover(a.bar.x + 10.0, l.plot.bottom() + 2.0), None);
    assert_eq!(hover(a.bar.x + 10.0, l.axis.y + 2.0), None);
}

#[test]
fn the_motions_go_by_rows_by_time_along_the_links_and_to_milestones() {
    let m = plan(&[
        r#"{"id": "a", "group": "One", "start": "2026-11-02", "duration": "3d"}"#,
        r#"{"id": "b", "group": "One", "after": "a", "duration": "2d"}"#,
        r#"{"id": "m1", "group": "One", "after": "b", "milestone": true}"#,
        r#"{"id": "c", "group": "Two", "start": "2026-11-03", "duration": "4d"}"#,
        r#"{"id": "d", "group": "Two", "after": ["a", "c"], "duration": "2d"}"#,
        r#"{"id": "m2", "group": "Two", "start": "2026-11-12", "milestone": true}"#,
    ]);
    let (_, l) = laid(&m, &View::default());
    let go = |from: Option<&str>, action: Action, count: u32| {
        let from = from.map(cursor);
        nav::motion(&m, &l, from.as_ref(), &action, count)
    };
    let to = |id: &str| Some(cursor(id));
    // j and k: the task below or above, past a heading; a count repeats; they stop at the ends.
    assert_eq!(go(Some("a"), Action::Series(1), 1), to("b"));
    assert_eq!(go(Some("m1"), Action::Series(1), 1), to("c"));
    assert_eq!(go(Some("c"), Action::Series(-1), 2), to("b"));
    assert_eq!(go(Some("a"), Action::Series(-1), 1), None);
    // With no cursor, the first task in view.
    assert_eq!(go(None, Action::Series(1), 1), to("a"));
    // h and l: the task that starts before or after: a, c, b, then m1 and d together.
    assert_eq!(go(Some("a"), Action::Step(1), 1), to("c"));
    assert_eq!(go(Some("b"), Action::Step(-1), 1), to("c"));
    assert_eq!(go(Some("b"), Action::Step(1), 2), to("d"));
    // ] to a task that comes after this one, [ to one it comes after; again goes round them.
    assert_eq!(go(Some("a"), Action::Neighbour(1), 1), to("b"));
    assert_eq!(go(Some("d"), Action::Neighbour(-1), 1), to("a"));
    assert_eq!(go(Some("m2"), Action::Neighbour(1), 1), None);
    let (a, b, d) = (at(&m, "a"), at(&m, "b"), at(&m, "d"));
    assert_eq!(nav::neighbour(&m, &l, a, Some(b), 1), Some(d));
    assert_eq!(nav::neighbour(&m, &l, a, Some(d), 1), Some(b));
    assert_eq!(nav::neighbour(&m, &l, d, Some(a), -1), Some(at(&m, "c")));
    // w and b: the next or previous milestone, in the order they come.
    assert_eq!(go(Some("a"), Action::Peak(1), 1), to("m1"));
    assert_eq!(go(Some("m1"), Action::Peak(1), 1), to("m2"));
    assert_eq!(go(Some("m2"), Action::Peak(-1), 1), to("m1"));
    assert_eq!(go(Some("m2"), Action::Peak(1), 1), None);
    // gg and G: the first and last task.
    assert_eq!(go(Some("c"), Action::First, 1), to("a"));
    assert_eq!(go(None, Action::Last, 1), to("m2"));
    // What a search finds: each task by its label and its key, here one and the same.
    assert_eq!(m.names()[0], ("a".to_string(), cursor("a")));
    assert_eq!(m.task_of(&cursor("d")), Some(d));
    assert_eq!(m.task_of(&Cursor::Node("d".into())), None);
}

#[test]
fn zoom_and_pan_keep_to_their_limits() {
    let m = three();
    let view = View::default();
    let (_, l) = laid(&m, &view);
    let fit = l.fit.1 - l.fit.0;
    let span = |v: &View| v.x.map(|x| x.1 - x.0).unwrap();
    // About the pointer: the time under it stays under it.
    let x = l.px(day(4));
    let v = nav::zoom_about(&m, &l, &view, 2.0, Some((x, 200.0)), None);
    assert!((span(&v) - fit / 2.0).abs() < 1e-6);
    let (_, z) = laid(&m, &v);
    assert!(close(z.px(day(4)), x));
    // About the cursor's task with no pointer.
    let v = nav::zoom_about(&m, &l, &view, 4.0, None, Some(&cursor("a")));
    let (_, z) = laid(&m, &v);
    assert!(close(z.px(day(0) + 2.5 * DAY), l.px(day(0) + 2.5 * DAY)));
    // No closer than a minute across the plot, no further out than 20 times the plan.
    let v = nav::zoom_about(&m, &l, &view, 1e12, None, None);
    assert!((span(&v) - nav::MIN_SPAN).abs() < 1e-3);
    let v = nav::zoom_about(&m, &l, &view, 1e-12, None, None);
    assert!((span(&v) - fit * nav::MAX_OUT).abs() < 1e-3);
    // 0 fits the plan again.
    assert_eq!(nav::fit(&v).x, None);
    // A pan moves the content with the hand, and keeps the middle of the time over the plan.
    let v = nav::pan(&m, &l, &view, 44.1875, 0.0);
    assert!(shows(&v, (day(-2), day(14))), "{:?}", v.x);
    let v = nav::pan(&m, &l, &view, -1e7, 0.0);
    let (lo, hi) = v.x.unwrap();
    assert!(((lo + hi) / 2.0 - l.fit.1).abs() < 1e-3);
    // Rows that don't overflow don't scroll: the wheel pans the time instead.
    assert_eq!(nav::pan(&m, &l, &view, 0.0, -50.0).y, None);
    assert!(shows(
        &nav::wheel(&m, &l, &view, 0.0, 44.1875),
        (day(-2), day(14))
    ));
    // A cursor's bar wholly out of the time shown is brought in; zz centres it.
    let far = View {
        x: Some((day(30), day(46))),
        ..View::default()
    };
    let (_, out) = laid(&m, &far);
    let v = nav::reveal(&m, &out, &far, &cursor("a"));
    assert_eq!(
        v.x,
        Some((
            day(0) + 2.5 * DAY - 8.0 * DAY,
            day(0) + 2.5 * DAY + 8.0 * DAY
        ))
    );
    assert_eq!(nav::reveal(&m, &l, &view, &cursor("a")), view);
    let v = nav::centre(&m, &l, &view, &cursor("m"));
    assert_eq!(v.x, Some((day(14) - 8.0 * DAY, day(14) + 8.0 * DAY)));
    // An ease goes from one view to the other, a fitted one taken as the plan's.
    let half = nav::tween(&m, &view, &far, 0.5);
    assert_eq!(
        half.x,
        Some(((day(-1) + day(30)) / 2.0, (day(15) + day(46)) / 2.0))
    );
    assert_eq!(nav::tween(&m, &far, &view, 1.0), view);
}

#[test]
fn a_brush_zooms_the_time_to_it() {
    let m = three();
    let view = View::default();
    let (_, l) = laid(&m, &view);
    let (from, to) = (day(3) + 12.0 * HOUR, day(9) + 12.0 * HOUR);
    let (x0, x1) = (l.px(from), l.px(to));
    // Either way round; the rows stay.
    let v = nav::brush(&m, &l, &view, x1, x0).unwrap();
    let (lo, hi) = v.x.unwrap();
    assert!(
        (lo - from).abs() < 1.0 && (hi - to).abs() < 1.0,
        "{lo} {hi}"
    );
    assert_eq!(v.y, None);
    // A drag of a few px is a click.
    assert_eq!(nav::brush(&m, &l, &view, 300.0, 305.0), None);
    // While it's dragged the overlay shows it, the plot's whole height, with what it spans.
    let st = OverlayState {
        brush: Some((x0, x1)),
        ..OverlayState::default()
    };
    let over = gantt::overlay(&m, &l, &view, &st, &Theme::mono_dark(), &mut FixedMeasure);
    assert_eq!(texts(&over), ["5 Nov – 11 Nov · 6 days"]);
    let b = over.bounds().unwrap();
    assert!(b.y <= l.plot.y && b.bottom() >= l.plot.bottom());
}

#[test]
fn the_overlay_shows_the_card_the_cursor_and_the_date_under_the_pointer() {
    let m = three();
    let view = View::default();
    let (_, l) = laid(&m, &view);
    let theme = Theme::mono_dark();
    let draw = |st: &OverlayState| gantt::overlay(&m, &l, &view, st, &theme, &mut FixedMeasure);
    // Nothing to show, nothing drawn.
    assert!(draw(&OverlayState::default()).items.is_empty());
    let off = OverlayState {
        pointer: Some((10.0, 10.0)),
        ..OverlayState::default()
    };
    assert!(draw(&off).items.is_empty());
    // A hovered task: its card, clear of its bar and inside the area, with its dates, how
    // long it is and the spec's tooltip field.
    let a = l.task(0).unwrap();
    let st = OverlayState {
        hover: Some(Target::Task(0)),
        pointer: Some((a.bar.x + 30.0, a.mid())),
        ..OverlayState::default()
    };
    let over = draw(&st);
    let card = card_rect(&over).expect("a card");
    assert!(
        card.intersect(&a.bar).is_none(),
        "{card:?} over {:?}",
        a.bar
    );
    assert!(card.x >= 0.0 && card.right() <= SIZE.0 && card.bottom() <= SIZE.1);
    let said = texts(&over);
    for want in [
        "Mon 2 Nov",
        "Fri 6 Nov",
        "5 days",
        "Start",
        "End",
        "Duration",
        "Sol",
        "owner",
    ] {
        assert!(said.contains(&want.to_string()), "{want} in {said:?}");
    }
    // A milestone's says when; what it comes after is named by its label.
    let st = OverlayState {
        hover: Some(Target::Task(2)),
        ..OverlayState::default()
    };
    let said = texts(&draw(&st));
    assert!(said.contains(&"Mon 16 Nov".to_string()) && said.contains(&"When".to_string()));
    assert!(said.contains(&"Build".to_string()) && said.contains(&"After".to_string()));
    // The cursor's task: its card too, unless the detail panel says the same.
    let mut st = OverlayState {
        cursor: Some(cursor("b")),
        cursor_card: true,
        ..OverlayState::default()
    };
    assert!(card_rect(&draw(&st)).is_some());
    st.cursor_card = false;
    let ringed = draw(&st);
    assert!(card_rect(&ringed).is_none() && !ringed.items.is_empty());
    // A search's or the panel's lit task: as a hover, without the card.
    let st = OverlayState {
        emphasis: Some(cursor("b")),
        ..OverlayState::default()
    };
    let lit = draw(&st);
    assert!(card_rect(&lit).is_none() && texts(&lit).contains(&"Build".to_string()));
    // Off any bar: the day under the pointer in a chip on the axis, and no card.
    let st = OverlayState {
        pointer: Some((l.px(day(10) + 3.0 * HOUR), a.mid())),
        ..OverlayState::default()
    };
    let over = draw(&st);
    assert_eq!(texts(&over), ["Thu 12 Nov"]);
    assert!(card_rect(&over).is_none());
}

#[test]
fn the_detail_tree_has_its_dates_its_fields_and_the_tasks_either_side() {
    let m = plan(&[
        r#"{"id": "a", "label": "Spec", "group": "Design", "start": "2026-11-02", "duration": "5d", "owner": "Sol", "notes": {"risk": "low"}}"#,
        r#"{"id": "b", "label": "Build", "after": "a", "duration": "3d", "progress": 0.5}"#,
        r#"{"id": "c", "label": "Ship", "after": "b", "milestone": true}"#,
    ]);
    let tree = task_tree(&m, 1);
    assert_eq!(
        (tree.title.as_str(), tree.kind.as_str()),
        ("Build", "7 Nov to 9 Nov")
    );
    assert_eq!(
        tree.item,
        Some(Mark::Task {
            index: 1,
            id: "b".into()
        })
    );
    let rows: Vec<(&str, &str)> = tree.rows.iter().map(|r| (&*r.key, &*r.value)).collect();
    assert_eq!(
        rows,
        [
            ("start", "Sat 7 Nov"),
            ("end", "Mon 9 Nov"),
            ("duration", "3 days"),
            ("progress", "50 %"),
            ("id", "b"),
            ("Comes after", ""),
            ("Spec", "ends 6 Nov"),
            ("Then", ""),
            ("Ship", "starts 10 Nov"),
        ]
    );
    // The rows under the headings name their tasks, so ⏎ on one goes there.
    assert_eq!(tree.rows[5].kind, RowKind::Section);
    let named = tree.rows[6].item.clone().unwrap();
    assert_eq!(
        named,
        Mark::Task {
            index: 0,
            id: "a".into()
        }
    );
    assert_eq!(named.cursor(), Some(cursor("a")));
    assert!(named.holds(&cursor("a")) && named.same(&m.item_of(&cursor("a")).unwrap()));
    assert_eq!(
        m.detail_of(&named).map(|t| t.title),
        Some("Spec".to_string())
    );
    // Every other key of its row is a field, a nested value a branch.
    let tree = task_tree(&m, 0);
    assert_eq!(tree.kind, "Design · 2 Nov to 6 Nov");
    let keys: Vec<&str> = tree.rows.iter().map(|r| &*r.key).collect();
    assert_eq!(
        keys,
        [
            "start", "end", "duration", "id", "group", "owner", "notes", "risk", "Then", "Build"
        ]
    );
    // `detail` names the fields shown in their place; a milestone says when.
    let text = plan_with(
        &[r#"{"id": "a", "start": "2026-11-02", "owner": "Sol", "team": "Core"}"#],
        r#", "detail": ["team"]"#,
    );
    let tree = task_tree(&model(&text), 0);
    let rows: Vec<(&str, &str)> = tree.rows.iter().map(|r| (&*r.key, &*r.value)).collect();
    assert_eq!(rows, [("when", "Mon 2 Nov"), ("team", "Core")]);
}

#[test]
fn the_tsv_lists_the_tasks_shown_with_their_dates_worked_out() {
    let text = r#"{"type": "gantt", "today": false, "calendar": "weekdays", "color": "owner",
        "data": {"values": "id,task,group,start,duration,after,progress,owner\n1,Spec,Design,2026-11-02,5d,,100,Sol\n2,Build,Make,,2w,1,50,Lena\n3,Ship,Make,,,1;2,,Sol\n"}}"#;
    let m = model(text);
    assert_eq!(
        m.tsv(&m.initial_view()),
        "id\tlabel\tgroup\tstart\tend\tduration\tprogress\tafter\towner\n\
         1\tSpec\tDesign\t2026-11-02\t2026-11-06\t5d\t1\t\tSol\n\
         2\tBuild\tMake\t2026-11-09\t2026-11-20\t10d\t0.5\t1\tLena\n\
         3\tShip\tMake\t2026-11-23\t2026-11-23\t\t\t1;2\tSol\n"
    );
    assert_eq!(m.tsv_line(&m.initial_view(), &cursor("2")), Some(2));
    // A hidden name's tasks are left out.
    let mut view = m.initial_view();
    view.hidden.insert(SeriesKey::new("Sol", 0));
    assert_eq!(m.tsv(&view).lines().count(), 2);
    assert_eq!(m.tsv_line(&view, &cursor("1")), None);
    // A plan with times of day writes them.
    let m = plan(&[r#"{"id": "a", "start": "2026-11-02T08:00", "duration": "90m"}"#]);
    let tsv = m.tsv(&m.initial_view());
    assert_eq!(
        tsv.lines().nth(1),
        Some("a\ta\t\t2026-11-02T08:00\t2026-11-02T09:30\t1h30m\t\t")
    );
}

fn job(text: &str, name: &str, open: Vec<(PathBuf, Snapshot)>) -> Job {
    Job {
        text: Text::Given(text.into()),
        name: name.into(),
        dir: PathBuf::from("/plans"),
        zone: TimeZone::UTC,
        open,
        toggles: Toggles::default(),
        auto: None,
        now: NOW,
    }
}

#[test]
fn a_gantt_is_built_from_its_file_as_a_chart_is() {
    let spec = r#"{"type": "gantt", "title": "Q4", "data": "plan.csv", "id": "key",
        "label": "summary", "start": "begins", "end": "due", "after": "blocked_by"}"#;
    let csv = "key,summary,begins,due,blocked_by\n\
               a,Reset,2026-11-03,2026-11-10,\n\
               b,Passkeys,2026-11-11,2026-11-27,a\n";
    let data = |text: &str| -> Vec<(PathBuf, Snapshot)> {
        vec![(PathBuf::from("/plans/plan.csv"), Box::new(text.to_string()))]
    };
    let b = source::build(job(spec, "plan.fig", data(csv)))
        .ok()
        .unwrap();
    let m = b.gantt.expect("a gantt's model");
    assert!(b.model.is_none() && b.graph.is_none());
    assert_eq!((b.tables.len(), b.tables[0].rows), (1, 2));
    assert_eq!(b.followed, [PathBuf::from("/plans/plan.csv")]);
    // Named by its data's file; the clock is the job's.
    assert_eq!(m.summary(), "plan.csv · 2 tasks · 3 Nov to 27 Nov");
    assert_eq!(m.today(), Some(NOW));
    assert_eq!(m.label(1), "Passkeys");
    // An error about a row is in the data's file, at its cell.
    let bad = csv.replace("2026-11-27", "someday");
    let f = source::build(job(spec, "plan.fig", data(&bad)))
        .err()
        .unwrap();
    let e = f.first();
    assert_eq!(e.msg, "\"due\" must be a time, not \"someday\"");
    assert_eq!(e.file.as_deref(), Some(Path::new("/plans/plan.csv")));
    assert_eq!(e.at.map(|a| (a.line, a.col)), Some((3, 23)));
    assert_eq!(e.line.as_deref(), Some("b,Passkeys,2026-11-11,someday,a"));
    assert!(
        e.context
            .iter()
            .any(|c| c.starts_with("read plan.csv (its buffer) as CSV")),
        "{e:?}"
    );
    // A bare table with a start and an end is a gantt of its own rows.
    let b = source::build(job(
        "task,start,end\nA,2026-11-02,2026-11-06\n",
        "roadmap.csv",
        vec![],
    ));
    let b = b.ok().unwrap();
    assert!(b.inferred);
    assert_eq!(
        b.gantt.unwrap().summary(),
        "roadmap.csv · 1 task · 2 Nov to 6 Nov"
    );
    // The examples build: a plan from a file, with its parents, and one that waits for itself.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data");
    let file = |name: &str| Job {
        text: Text::File(dir.join(name)),
        dir: dir.join(name).parent().unwrap().to_path_buf(),
        ..job("", name.rsplit('/').next().unwrap(), vec![])
    };
    let m = source::build(file("gantt/plan.fig"))
        .ok()
        .unwrap()
        .gantt
        .unwrap();
    assert_eq!((m.len(), m.names.len(), m.by_group), (12, 5, false));
    assert_eq!(m.tasks[1].late, None);
    assert_eq!(
        m.late_text(2).as_deref(),
        Some("Starts 2 days before Password reset ends")
    );
    let f = source::build(file("errors/gantt-cycle.fig")).err().unwrap();
    assert_eq!(f.first().msg, "\"b\" comes after itself: b → c → b");
    assert_eq!(f.first().at.map(|a| a.line), Some(6));
}
