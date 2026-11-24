//! The gantt's spec: tasks inline or as data, the channels that say which column is what,
//! `today`, `marks` and `view`, each error at its place, a misspelt key's warning, a bare
//! table's guess, and `write`'s text read back as the same figure.

use die_test_cap as _;

use die_fig::data::{ColumnKind, ReadOptions};
use die_fig::spec::{Calendar, DataRef, Figure, GanttSpec, LegendPos, Parsed, Today, parse, write};
use std::path::Path;

const DIR: &str = "/specs";

fn opt() -> ReadOptions {
    ReadOptions::new(jiff::tz::TimeZone::UTC)
}

fn read_as(text: &str, name: &str) -> Parsed {
    match parse(text, name, Path::new(DIR), &opt()) {
        Ok(p) => p,
        Err(e) => panic!("{e} in\n{text}"),
    }
}

fn gantt(text: &str) -> GanttSpec {
    match read_as(text, "plan.fig").figure {
        Figure::Gantt(g) => g,
        f => panic!("not a gantt: {f:?}"),
    }
}

/// The error a spec gives: its message and its line and column.
fn fails(text: &str) -> (String, (u32, u32)) {
    let e = parse(text, "plan.fig", Path::new(DIR), &opt()).expect_err(text);
    let at = e.at.unwrap_or_else(|| panic!("no place for {:?}", e.msg));
    (e.msg, (at.line, at.col))
}

/// 2026-11-02T00:00:00Z.
const NOV_2: f64 = 1_793_577_600.0;
const DAY: f64 = 86_400.0;

const RELEASE: &str = r#"{
  "type": "gantt",
  "title": "Release 2.0",
  "tasks": [
    {"id": "spec", "label": "Write the spec", "group": "Design", "start": "2026-11-02", "end": "2026-11-06", "progress": 1},
    {"id": "api", "label": "API", "group": "Build", "after": "spec", "duration": "8d", "owner": "Ravi", "fig": {"color": "red"}},
    {"id": "beta", "label": "Beta", "group": "Ship", "after": ["api", "spec"], "milestone": true}
  ],
  "today": "2026-11-10",
  "calendar": {"days": "weekdays", "off": ["2026-12-25", {"start": "2026-12-28", "end": "2027-01-01"}]},
  "marks": [
    {"at": "2026-11-20", "label": "Freeze", "fig": {"color": "critical"}},
    {"start": "2026-12-24", "end": "2027-01-02", "label": "Holidays"}
  ],
  "tooltip": "owner",
  "view": {"links": false, "legend": "right"}
}"#;

#[test]
fn inline_tasks_are_a_table_of_a_task_a_row_with_each_row_s_object() {
    let g = gantt(RELEASE);
    assert_eq!(g.title.as_deref(), Some("Release 2.0"));
    assert!(g.tasks);
    let DataRef::Inline(t) = &g.data.source else {
        panic!("{:?}", g.data)
    };
    assert_eq!(t.rows, 3);
    // The columns are every key in the order first met; `fig` is never one.
    let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
    assert_eq!(
        names,
        [
            "id",
            "label",
            "group",
            "start",
            "end",
            "progress",
            "after",
            "duration",
            "owner",
            "milestone"
        ]
    );
    let start = t.column("start").unwrap();
    assert_eq!(start.kind(), ColumnKind::Time);
    assert_eq!(start.f64_at(0), NOV_2);
    assert!(start.f64_at(1).is_nan());
    // A list in a cell is its JSON text in the column, and itself in the row's object.
    assert_eq!(
        t.column("after").unwrap().str_at(2),
        Some(r#"["api","spec"]"#)
    );
    let raw = t.raw.as_ref().unwrap();
    assert_eq!(
        raw[2].get("after").and_then(|a| a.as_arr()).unwrap().len(),
        2
    );
    let styles = t.styles.as_ref().unwrap();
    assert!(styles[1].color.is_some() && styles[0].color.is_none());
    assert_eq!(g.today, Today::At(NOV_2 + 8.0 * DAY));
    // Working days, with a day off and a span of them.
    assert!(g.calendar.weekdays);
    let off: Vec<(&str, Option<&str>)> = (g.calendar.off.iter())
        .map(|(a, b)| (&*a.text, b.as_ref().map(|b| &*b.text)))
        .collect();
    assert_eq!(
        off,
        [("2026-12-25", None), ("2026-12-28", Some("2027-01-01"))]
    );
    assert_eq!(g.tooltip, ["owner"]);
    assert!(!g.links);
    assert_eq!(g.view.legend, LegendPos::Right);
    // A line, and a span whose end was written as a date.
    assert_eq!(g.marks.len(), 2);
    assert_eq!(
        (g.marks[0].at.secs, g.marks[0].end.is_none()),
        (NOV_2 + 18.0 * DAY, true)
    );
    assert_eq!(g.marks[0].label.as_deref(), Some("Freeze"));
    let end = g.marks[1].end.as_ref().unwrap();
    assert!(end.is_date() && &*end.text == "2027-01-02");
}

#[test]
fn data_and_channels_say_which_column_is_what() {
    let g = gantt(
        r#"{"type": "gantt", "data": "plan.csv", "transform": [{"sort": "begins"}],
            "label": "summary", "start": {"field": "begins", "type": "time", "unit": "s"},
            "end": "due", "color": "owner", "detail": ["owner", "notes"], "today": false}"#,
    );
    assert!(!g.tasks);
    assert_eq!(
        g.data.source,
        DataRef::File(Path::new(DIR).join("plan.csv"))
    );
    assert_eq!(g.transform.len(), 1);
    let f = &g.fields;
    assert_eq!(f.label.as_ref().unwrap().field.as_deref(), Some("summary"));
    let start = f.start.as_ref().unwrap();
    assert_eq!(
        (start.field.as_deref(), start.unit.as_deref()),
        (Some("begins"), Some("s"))
    );
    assert_eq!(f.color.as_ref().unwrap().field.as_deref(), Some("owner"));
    assert!(f.id.is_none() && f.after.is_none() && f.group.is_none());
    assert_eq!(g.today, Today::Off);
    assert_eq!(g.detail, ["owner", "notes"]);
    // The defaults: a line at now, arrows drawn, every day counted.
    let plain = gantt(r#"{"type": "gantt", "data": "plan.csv"}"#);
    assert_eq!((plain.today, plain.links), (Today::Now, true));
    assert_eq!(plain.calendar, Calendar::default());
    let weekdays = gantt(r#"{"type": "gantt", "data": "plan.csv", "calendar": "weekdays"}"#);
    assert!(weekdays.calendar.weekdays && weekdays.calendar.off.is_empty());
    // `tasks` alone says it's a gantt.
    let untyped = gantt(r#"{"tasks": [{"label": "a", "start": "2026-11-02", "duration": 2}]}"#);
    assert!(untyped.tasks);
}

#[test]
fn each_mistake_is_an_error_at_its_place() {
    let both = r#"{"type": "gantt", "tasks": [], "data": "plan.csv"}"#;
    assert_eq!(
        fails(both),
        (
            "a gantt has \"tasks\" and \"data\"; give one".into(),
            (1, 40)
        )
    );
    assert_eq!(
        fails(r#"{"type": "gantt"}"#).0,
        "a gantt needs \"tasks\" or \"data\""
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "tasks": [{"label": "a"}, 3]}"#),
        ("a task must be an object, not a number".into(), (1, 45))
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "data": "p.csv", "start": 3}"#).0,
        "\"start\" says which column it is: give a field's name"
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "data": "p.csv", "today": "soon"}"#).0,
        "\"today\" must be true, false or a time, not \"soon\""
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "data": "p.csv", "marks": [{"label": "x"}]}"#).0,
        "a mark needs \"at\", or \"start\" and \"end\""
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "data": "p.csv", "marks": [{"at": 12}]}"#).0,
        "\"at\" must be a time, not 12"
    );
    assert_eq!(
        fails(
            r#"{"type": "gantt", "data": "p.csv", "marks": [{"start": "2026-11-05", "end": "2026-11-02"}]}"#
        )
        .0,
        "a mark's \"end\" must be after its \"start\""
    );
    assert_eq!(
        fails(r#"{"type": "gantt", "data": "p.csv", "calendar": "workdays"}"#).0,
        "\"calendar\" must be \"all\" or \"weekdays\", not \"workdays\""
    );
    assert_eq!(
        fails(
            r#"{"type": "gantt", "data": "p.csv", "calendar": {"off": [{"start": "2026-12-24"}]}}"#
        )
        .0,
        "days off are a date, or a \"start\" and an \"end\""
    );
    // A span of one day: its end the date it starts on.
    let one = gantt(
        r#"{"type": "gantt", "data": "p.csv", "marks": [{"start": "2026-11-05", "end": "2026-11-05"}]}"#,
    );
    assert!(one.marks[0].end.is_some());
}

#[test]
fn a_misspelt_key_warns_but_a_task_s_own_keys_are_its_data() {
    let p = read_as(
        r#"{"type": "gantt", "titel": "Plan", "view": {"link": false},
            "tasks": [{"labell": "a", "start": "2026-11-02", "duration": "2d"}]}"#,
        "plan.fig",
    );
    let said: Vec<&str> = p.warnings.iter().map(|w| &*w.msg).collect();
    assert_eq!(
        said,
        [
            "unknown field \"titel\"; did you mean \"title\"?",
            "unknown field \"link\"; did you mean \"links\"?"
        ]
    );
}

#[test]
fn a_bare_table_with_a_start_and_an_end_or_a_duration_is_a_gantt() {
    let csv = "task,start,finish,group\nSpec,2026-11-02,2026-11-06,Design\nAPI,2026-11-09,2026-11-18,Build\n";
    let p = read_as(csv, "plan.csv");
    assert!(p.inferred);
    let Figure::Gantt(g) = &p.figure else {
        panic!("{:?}", p.figure)
    };
    assert_eq!(g.data.source, DataRef::Source);
    assert!(!g.tasks && g.fields == Default::default());
    let durations = "name,start,duration\nSpec,2026-11-02,5\n";
    assert!(matches!(
        read_as(durations, "plan.csv").figure,
        Figure::Gantt(_)
    ));
    // A start that isn't a time, or no end and no duration: a chart as before.
    let numbers = "start,end\n1,2\n3,4\n";
    assert!(matches!(read_as(numbers, "n.csv").figure, Figure::Chart(_)));
    let lone = "start,value\n2026-11-02,5\n2026-11-03,6\n";
    assert!(matches!(read_as(lone, "t.csv").figure, Figure::Chart(_)));
}

#[test]
fn a_gantt_written_reads_back_as_itself() {
    for text in [
        RELEASE,
        r#"{"type": "gantt", "subtitle": "s", "data": {"file": "plan.tsv", "format": "tsv"},
            "transform": [{"sort": "start"}], "id": "key", "label": "summary",
            "start": {"field": "ts", "type": "time", "unit": "ms"}, "duration": "days",
            "after": "blocked_by", "group": "team", "parent": "epic", "progress": "done",
            "milestone": "gate", "color": "owner", "detail": "notes", "today": false,
            "calendar": "weekdays",
            "view": {"grid": false}}"#,
        r#"{"type": "gantt", "data": {"values": "task,start,end\nA,2026-11-02,2026-11-04"}}"#,
    ] {
        let f = read_as(text, "plan.fig").figure;
        let written = write(&f);
        let back = read_as(&written, "plan.fig");
        assert_eq!(back.warnings, [], "{written}");
        assert_eq!(
            format!("{:?}", back.figure.without_places()),
            format!("{:?}", f.without_places()),
            "{written}"
        );
        assert_eq!(write(&back.figure), written);
    }
    // A guessed gantt is written as one over the buffer.
    let guessed = read_as("task,start,end\nA,2026-11-02,2026-11-04\n", "plan.csv").figure;
    assert_eq!(
        write(&guessed),
        "{ \"type\": \"gantt\", \"data\": { \"file\": \"-\", \"format\": \"csv\" } }\n"
    );
}
