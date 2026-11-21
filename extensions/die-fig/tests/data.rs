//! The data module: CSV, TSV, JSON and JSONL read into typed tables (quotes, delimiters, a BOM,
//! headers, document order, fig blocks, errors in their places), each column's type inferred
//! from all of its rows, times parsed in a fixed-offset zone and in Europe/London across its
//! changes of clocks, and every transform with its edges: missing values, groups in the order
//! first met, and categories re-coded to the values left.

use die_test_cap as _;

use die_fig::data::csv::read_csv;
use die_fig::data::infer::{MISSING, infer, parse_number};
use die_fig::data::rows::{read_json, read_jsonl};
use die_fig::data::time::{as_time, parse_time};
use die_fig::data::transform::{AggOp, FilterOp, OTHER, Transform, apply};
use die_fig::data::{Column, ColumnKind, ReadOptions, Table, Values};
use die_fig::error::Pos;
use die_fig::json;
use die_fig::style::{ColorSpec, FigStyle, Status};
use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};

// ------------------------------------------------------------------ helpers

/// Ten hours east of UTC, as orchid's tests: no daylight saving, and far enough from UTC that a
/// time read in the wrong zone shows.
fn zone() -> TimeZone {
    TimeZone::fixed(Offset::constant(10))
}

fn opt() -> ReadOptions {
    ReadOptions::new(zone())
}

fn london() -> ReadOptions {
    ReadOptions::new(TimeZone::get("Europe/London").expect("the system's zoneinfo has London"))
}

/// An instant written with its offset, as seconds since the epoch.
fn at(s: &str) -> f64 {
    s.parse::<Timestamp>().unwrap().as_duration().as_secs_f64()
}

fn kinds(t: &Table) -> Vec<(&str, ColumnKind)> {
    t.columns.iter().map(|c| (&*c.name, c.kind())).collect()
}

fn nums(t: &Table, name: &str) -> Vec<f64> {
    let c = t.column(name).unwrap_or_else(|| panic!("no column {name}"));
    (0..c.len()).map(|r| c.f64_at(r)).collect()
}

fn strs<'t>(t: &'t Table, name: &str) -> Vec<Option<&'t str>> {
    let c = t.column(name).unwrap_or_else(|| panic!("no column {name}"));
    (0..c.len()).map(|r| c.str_at(r)).collect()
}

fn dict<'t>(t: &'t Table, name: &str) -> Vec<&'t str> {
    match &t.column(name).unwrap().values {
        Values::Category { dict, .. } => dict.iter().map(|s| &**s).collect(),
        v => panic!("{name} is not a category: {v:?}"),
    }
}

/// Equal, NaN equal to NaN.
#[track_caller]
fn same(got: &[f64], want: &[f64]) {
    let eq = got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(a, b)| (a.is_nan() && b.is_nan()) || a == b);
    assert!(eq, "got {got:?}, want {want:?}");
}

/// Equal tables, NaN equal to NaN (as their debug forms are).
#[track_caller]
fn same_table(got: &Table, want: &Table) {
    assert_eq!(format!("{got:?}"), format!("{want:?}"));
}

fn pos(line: u32, col: u32, byte: u32) -> Option<Pos> {
    Some(Pos { line, col, byte })
}

fn csv(text: &str) -> Table {
    read_csv(text, &opt()).unwrap_or_else(|e| panic!("{e}"))
}

fn json_table(text: &str) -> die_fig::error::Result<Table> {
    read_json(&json::parse(text).unwrap(), &opt())
}

const NAN: f64 = f64::NAN;

// ------------------------------------------------------------------ CSV

#[test]
fn a_csv_with_a_header_is_typed_from_its_cells() {
    let t = csv("time,api,host,note\n\
         2026-11-21T06:00,642,web,first\n\
         2026-11-21T06:05,655.5,db,second\n\
         2026-11-21T06:10,-1e3,web,third\n");
    use ColumnKind::*;
    assert_eq!(
        kinds(&t),
        [
            ("time", Time),
            ("api", Number),
            ("host", Category),
            ("note", Category)
        ]
    );
    assert_eq!(t.rows, 3);
    // Civil times are in the injected zone, ten hours east.
    same(
        &nums(&t, "time"),
        &[
            at("2026-11-20T20:00Z"),
            at("2026-11-20T20:05Z"),
            at("2026-11-20T20:10Z"),
        ],
    );
    same(&nums(&t, "api"), &[642.0, 655.5, -1000.0]);
    assert_eq!(strs(&t, "host"), [Some("web"), Some("db"), Some("web")]);
    assert_eq!(dict(&t, "host"), ["web", "db"]);
    assert_eq!((t.styles.clone(), t.raw.clone()), (None, None));
}

#[test]
fn quoted_fields_take_doubled_quotes_delimiters_and_line_breaks() {
    let t = csv("name,said,n\r\n\
         \"Smith, J\",\"he said \"\"hi\"\"\r\nand left\",1\r\n\
         \"\",plain,\"2\"\r\n");
    assert_eq!(t.rows, 2);
    assert_eq!(strs(&t, "name"), [Some("Smith, J"), None]);
    assert_eq!(
        strs(&t, "said"),
        [Some("he said \"hi\"\r\nand left"), Some("plain")]
    );
    // A quoted number is a number.
    same(&nums(&t, "n"), &[1.0, 2.0]);
}

#[test]
fn the_delimiter_is_sniffed_from_the_first_line_unless_given() {
    let tsv = csv("time\tapi\tworker\n2026-11-21\t1,5\t2\n");
    assert_eq!(kinds(&tsv).len(), 3);
    // With tab the delimiter, "1,5" is one cell, and no number.
    assert_eq!(strs(&tsv, "api"), [Some("1,5")]);
    same(&nums(&tsv, "worker"), &[2.0]);

    let semi = csv("a;b;c\n1;2;3\n");
    same(&nums(&semi, "c"), &[3.0]);

    // Commas inside quotes don't count: two semicolons beat them.
    let quoted = csv("\"x,y,z\";b;c\n1;2;3\n");
    assert_eq!(
        kinds(&quoted).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["x,y,z", "b", "c"]
    );

    // A tie goes to the comma; one column has none.
    let tie = csv("a,b;c\n1,2;3\n");
    assert_eq!(strs(&tie, "b;c"), [Some("2;3")]);
    // Blank lines before the header don't hide its delimiter.
    let late = csv("\n\r\na;b\n1;2\n");
    same(&nums(&late, "b"), &[2.0]);
    let one = csv("value\n1\n2\n");
    same(&nums(&one, "value"), &[1.0, 2.0]);

    // A given delimiter wins over the sniff.
    let mut o = opt();
    o.delimiter = Some(b';');
    let given = read_csv("a,b;c\n1,2;3\n", &o).unwrap();
    same(&nums(&given, "c"), &[3.0]);
    assert_eq!(strs(&given, "a,b"), [Some("1,2")]);
}

#[test]
fn a_byte_order_mark_is_stripped() {
    let t = csv("\u{feff}time,v\n2026-11-21,1\n");
    assert_eq!(t.columns[0].name.as_ref(), "time");
    same(&nums(&t, "time"), &[at("2026-11-20T14:00Z")]);
}

#[test]
fn blank_lines_spaces_and_short_rows() {
    let t = csv("a, b ,c\n\n1 , 2,3\n   \n4,5\n\n");
    assert_eq!(
        kinds(&t).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    same(&nums(&t, "a"), &[1.0, 4.0]);
    same(&nums(&t, "b"), &[2.0, 5.0]);
    // A short row is missing its last cells.
    same(&nums(&t, "c"), &[3.0, NAN]);
    // A lone CR ends a line too; no final line break is needed.
    let cr = csv("a,b\r1,2\r3,4");
    same(&nums(&cr, "b"), &[2.0, 4.0]);
    // A quoted field keeps its spaces.
    let sp = csv("a\n\" x \"\n");
    assert_eq!(strs(&sp, "a"), [Some(" x ")]);
}

#[test]
fn without_a_header_columns_are_numbered_and_empty_names_too() {
    let mut o = opt();
    o.header = false;
    let t = read_csv("1,2\n3,4\n", &o).unwrap();
    assert_eq!(
        kinds(&t).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["column 1", "column 2"]
    );
    same(&nums(&t, "column 1"), &[1.0, 3.0]);
    // Without a header the widest row sets the columns, as JSON's arrays do, and a trailing
    // delimiter's empty column is dropped.
    let wide = read_csv("1\n2,3\n4,5,6\n", &o).unwrap();
    assert_eq!(
        kinds(&wide).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["column 1", "column 2", "column 3"]
    );
    same(&nums(&wide, "column 2"), &[NAN, 3.0, 5.0]);
    same(&nums(&wide, "column 3"), &[NAN, NAN, 6.0]);
    let trailing = read_csv("1,2,\n3,4,\n", &o).unwrap();
    assert_eq!(
        kinds(&trailing).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["column 1", "column 2"]
    );
    let named = csv("a,,c\n1,2,3\n");
    same(&nums(&named, "column 2"), &[2.0]);
    // A trailing delimiter's empty column is dropped; an empty column with a name stays.
    let trailing = csv("a,b,\n1,,\n2,,\n");
    assert_eq!(
        kinds(&trailing).iter().map(|k| k.0).collect::<Vec<_>>(),
        ["a", "b"]
    );
    same(&nums(&trailing, "b"), &[NAN, NAN]);
}

#[test]
fn missing_tokens_are_gaps_in_every_kind() {
    assert_eq!(MISSING, ["", "NA", "N/A", "null", "-", "NaN"]);
    let t = csv("n,host,time\n\
         1,web,2026-11-21\n\
         ,NA,N/A\n\
         NA,db,null\n\
         N/A,-,\n\
         null,web,NaN\n\
         -,,-\n\
         NaN,db,2026-11-22\n");
    use ColumnKind::*;
    assert_eq!(
        kinds(&t),
        [("n", Number), ("host", Category), ("time", Time)]
    );
    same(&nums(&t, "n"), &[1.0, NAN, NAN, NAN, NAN, NAN, NAN]);
    assert_eq!(
        strs(&t, "host"),
        [
            Some("web"),
            None,
            Some("db"),
            None,
            Some("web"),
            None,
            Some("db")
        ]
    );
    assert_eq!(dict(&t, "host"), ["web", "db"]);
    let time = nums(&t, "time");
    assert_eq!(time.iter().filter(|v| v.is_nan()).count(), 5);
    assert_eq!(time[6], at("2026-11-21T14:00Z"));
}

#[test]
fn a_thousands_separator_only_when_asked() {
    let text = "n\n\"1,234\"\n\"12,345,678.5\"\n";
    assert_eq!(csv(text).columns[0].kind(), ColumnKind::Category);
    let mut o = opt();
    o.thousands = Some(',');
    let t = read_csv(text, &o).unwrap();
    same(&nums(&t, "n"), &[1234.0, 12_345_678.5]);

    assert_eq!(parse_number("1,234", Some(',')), Some(1234.0));
    assert_eq!(parse_number("-1,234e2", Some(',')), Some(-123_400.0));
    assert_eq!(parse_number("1234", Some(',')), Some(1234.0));
    assert_eq!(parse_number("12,34", Some(',')), None);
    assert_eq!(parse_number("1,2345", Some(',')), None);
    assert_eq!(parse_number("1234,567", Some(',')), None);
    assert_eq!(parse_number("1,234", None), None);
    // A point separator makes the comma the decimal mark.
    assert_eq!(parse_number("1.234,5", Some('.')), Some(1234.5));
    assert_eq!(parse_number("0,25", Some('.')), Some(0.25));
    assert_eq!(parse_number("1 234 567", Some(' ')), Some(1_234_567.0));
    // Numbers are ASCII and finite: no inf, no percent, no currency.
    for s in [
        "inf",
        "-infinity",
        "NaN",
        "12%",
        "$5",
        "1_000",
        "0x10",
        "",
        "+",
        ".",
    ] {
        assert_eq!(parse_number(s, None), None, "{s}");
    }
    for (s, v) in [
        (" 7 ", 7.0),
        ("+5", 5.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("1E3", 1000.0),
    ] {
        assert_eq!(parse_number(s, None), Some(v), "{s}");
    }
}

#[test]
fn csv_errors_are_in_their_places() {
    // Columns count chars: "é" is two bytes and one column.
    let e = read_csv("é,b\n1,\"open\n2,3\n", &opt()).unwrap_err();
    assert_eq!(e.msg, "unterminated quoted field");
    assert_eq!(e.at, pos(2, 3, 7));
    assert_eq!(e.to_string(), "line 2, column 3: unterminated quoted field");

    let e = read_csv("a,b\n1,2\n3,4,5,6\n", &opt()).unwrap_err();
    assert_eq!(e.msg, "a row of 4 fields where the header has 2");
    assert_eq!(e.at, pos(3, 5, 12));

    let e = read_csv("a,b,a\n1,2,3\n", &opt()).unwrap_err();
    assert_eq!(e.msg, "two columns are named \"a\"");
    assert_eq!(e.at, pos(1, 5, 4));

    let e = read_csv("a,b\n\"x\"y,2\n", &opt()).unwrap_err();
    assert_eq!(
        e.msg,
        "expected a delimiter or the line's end after a closing quote"
    );
    assert_eq!(e.at, pos(2, 4, 7));

    // A BOM is bytes but no column.
    let e = read_csv("\u{feff}a,a\n", &opt()).unwrap_err();
    assert_eq!(e.at, pos(1, 3, 5));

    // A lone CR ends a line for a place as it does for a row; CRLF is one break.
    let e = read_csv("a,b\r1,2\r3,4,5\r", &opt()).unwrap_err();
    assert_eq!(e.at, pos(3, 5, 12));
    let e = read_csv("a,b\r\n1,2\r\n3,4,5\r\n", &opt()).unwrap_err();
    assert_eq!(e.at, pos(3, 5, 14));
    let e = read_csv("a\r\"b\rc", &opt()).unwrap_err();
    assert_eq!(e.at, pos(2, 1, 2));
}

#[test]
fn a_delimiter_is_one_ascii_character_and_never_padding() {
    let mut o = opt();
    // The bytes of '§', and the bytes that can't delimit.
    for d in [0xA7, 0xC2, b'"', b'\n', b'\r'] {
        o.delimiter = Some(d);
        let e = read_csv("a§b\n1§2\n", &o).unwrap_err();
        assert_eq!(
            e.msg, "a delimiter is one ASCII character, not a quote or a line break",
            "{d:#x}"
        );
    }
    // A space delimits, a quoted field keeps its spaces, and a tab still pads.
    o.delimiter = Some(b' ');
    let t = read_csv("name n\n\"x y\" 2\nz\t 3\n", &o).unwrap();
    assert_eq!(strs(&t, "name"), [Some("x y"), Some("z")]);
    same(&nums(&t, "n"), &[2.0, 3.0]);
    // Two spaces are an empty field between them, as two tabs are with tab.
    let t = read_csv("a b c\n1  3\n", &o).unwrap();
    same(&nums(&t, "b"), &[NAN]);
    same(&nums(&t, "c"), &[3.0]);
}

#[test]
fn empty_text_is_an_empty_table_and_a_header_alone_has_no_rows() {
    let t = csv("");
    assert_eq!((t.rows, t.columns.len()), (0, 0));
    let t = csv("\u{feff}\n\n");
    assert_eq!((t.rows, t.columns.len()), (0, 0));
    let t = csv("a,b\n");
    assert_eq!(t.rows, 0);
    assert_eq!(
        kinds(&t),
        [("a", ColumnKind::Number), ("b", ColumnKind::Number)]
    );
}

// ------------------------------------------------------------------ JSON and JSONL

#[test]
fn json_rows_keep_document_order_fig_blocks_and_their_objects() {
    let text = r#"[
      {"time": "2026-11-21T06:00", "zeta": 3, "alpha": "web", "fig": {"color": "warning", "size": 2}},
      {"zeta": "4", "time": "2026-11-21T06:05", "alpha": true, "extra": [1, {"k": null}]},
      {"time": "2026-11-21T06:10", "zeta": null, "alpha": "web", "fig": {"hidden": true, "label": "late"}}
    ]"#;
    let node = json::parse(text).unwrap();
    let t = read_json(&node, &opt()).unwrap();
    use ColumnKind::*;
    // Keys in the order first met, not the alphabet's; fig is no column.
    assert_eq!(
        kinds(&t),
        [
            ("time", Time),
            ("zeta", Number),
            ("alpha", Category),
            ("extra", Category)
        ]
    );
    assert_eq!(t.rows, 3);
    same(
        &nums(&t, "time"),
        &[
            at("2026-11-20T20:00Z"),
            at("2026-11-20T20:05Z"),
            at("2026-11-20T20:10Z"),
        ],
    );
    // A number stays one, a string of one is one, null is missing.
    same(&nums(&t, "zeta"), &[3.0, 4.0, NAN]);
    assert_eq!(strs(&t, "alpha"), [Some("web"), Some("true"), Some("web")]);
    // A nested value is its JSON; a row without the key is missing it.
    assert_eq!(strs(&t, "extra"), [None, Some("[1,{\"k\":null}]"), None]);

    let styles = t.styles.as_deref().expect("rows had fig blocks");
    assert_eq!(
        styles[0],
        FigStyle {
            color: Some(ColorSpec::Status(Status::Warning)),
            size: Some(2),
            ..FigStyle::default()
        }
    );
    assert_eq!(styles[1], FigStyle::default());
    assert!(styles[2].hidden);
    assert_eq!(styles[2].label.as_deref(), Some("late"));

    let raw = t.raw.as_deref().expect("rows from JSON keep their objects");
    assert_eq!(raw, node.as_arr().unwrap());
    assert_eq!(raw[1].at.line, 3);

    // Without a fig block anywhere there are no styles, but the objects stay.
    let plain = json_table(r#"[{"a": 1}, {"a": 2}]"#).unwrap();
    assert_eq!(plain.styles, None);
    assert_eq!(plain.raw.as_ref().map(|r| r.len()), Some(2));
}

#[test]
fn json_an_object_of_arrays_is_a_column_a_key() {
    let t = json_table(
        r#"{"time": ["2026-11-21", "2026-11-22"], "b": [2, null], "a": ["x", "y"],
            "fig": [{"dash": true}, null]}"#,
    )
    .unwrap();
    use ColumnKind::*;
    assert_eq!(kinds(&t), [("time", Time), ("b", Number), ("a", Category)]);
    same(&nums(&t, "b"), &[2.0, NAN]);
    let styles = t.styles.as_deref().unwrap();
    assert!(styles[0].dash);
    assert_eq!(styles[1], FigStyle::default());
    assert_eq!(t.raw, None);

    let empty = json_table("{}").unwrap();
    assert_eq!((empty.rows, empty.columns.len()), (0, 0));
    assert_eq!(json_table("[]").unwrap(), Table::default());
}

#[test]
fn json_arrays_with_a_header_row() {
    let t = json_table(
        r#"[["time", "api", 2026, ""], ["2026-11-21", 1, "a"], ["2026-11-22", 2.5, "b", "z"]]"#,
    )
    .unwrap();
    use ColumnKind::*;
    assert_eq!(
        kinds(&t),
        [
            ("time", Time),
            ("api", Number),
            ("2026", Category),
            ("column 4", Category)
        ]
    );
    same(&nums(&t, "api"), &[1.0, 2.5]);
    assert_eq!(strs(&t, "column 4"), [None, Some("z")]);
    assert_eq!((t.styles.clone(), t.raw.clone()), (None, None));

    let mut o = opt();
    o.header = false;
    let t = read_json(&json::parse("[[1, 2], [3]]").unwrap(), &o).unwrap();
    same(&nums(&t, "column 1"), &[1.0, 3.0]);
    same(&nums(&t, "column 2"), &[2.0, NAN]);
}

#[test]
fn json_errors_are_at_the_value_at_fault() {
    let err = |text: &str| json_table(text).unwrap_err();

    let e = err("\n  42");
    assert_eq!(
        e.msg,
        "expected an array of rows or an object of columns, not a number"
    );
    assert_eq!(e.at, pos(2, 3, 3));

    let e = err("[{\"a\": 1},\n 7]");
    assert_eq!(e.msg, "expected an object like the first row, not a number");
    assert_eq!(e.at, pos(2, 2, 12));

    let e = err("[\"a\"]");
    assert_eq!(e.msg, "expected a row, an object or an array, not a string");
    assert_eq!(e.at, pos(1, 2, 1));

    let e = err("[[\"a\"], {\"a\": 1}]");
    assert_eq!(e.msg, "expected an array like the first row, not an object");
    assert_eq!(e.at, pos(1, 9, 8));

    let e = err("[[\"a\", \"b\"],\n [1, 2, 3]]");
    assert_eq!(e.msg, "a row of 3 values where the header has 2");
    assert_eq!(e.at, pos(2, 9, 21));

    let e = err("[[\"a\", {\"x\": 1}]]");
    assert_eq!(e.msg, "expected a column's name, not an object");
    assert_eq!(e.at, pos(1, 8, 7));

    let e = err("[[\"a\", \"a\"]]");
    assert_eq!(e.msg, "two columns are named \"a\"");
    assert_eq!(e.at, pos(1, 8, 7));

    let e = err("{\"a\": [1, 2],\n \"b\": 3}");
    assert_eq!(
        e.msg,
        "expected an array of the values of \"b\", not a number"
    );
    assert_eq!(e.at, pos(2, 7, 20));

    let e = err("{\"a\": [1, 2], \"b\": [3]}");
    assert_eq!(e.msg, "\"b\" has 1 values where \"a\" has 2");
    assert_eq!(e.at, pos(1, 20, 19));

    // The reader's own errors keep their places: a duplicate key at the second.
    let e = json::parse("[{\"a\": 1, \"a\": 2}]").unwrap_err();
    assert_eq!(e.to_string(), "line 1, column 11: duplicate key \"a\"");
}

#[test]
fn jsonl_is_a_row_a_line() {
    let text = "{\"t\": \"2026-11-21T06:00Z\", \"v\": 1}\n\n{\"v\": 2, \"t\": \"2026-11-21T06:01Z\", \"fig\": {\"color\": \"red\"}}\r\n  \n{\"t\": \"2026-11-21T06:02Z\"}\n";
    let t = read_jsonl(text, &opt()).unwrap();
    assert_eq!(
        kinds(&t),
        [("t", ColumnKind::Time), ("v", ColumnKind::Number)]
    );
    // An instant with Z wins over the zone.
    same(
        &nums(&t, "t"),
        &[
            at("2026-11-21T06:00Z"),
            at("2026-11-21T06:01Z"),
            at("2026-11-21T06:02Z"),
        ],
    );
    same(&nums(&t, "v"), &[1.0, 2.0, NAN]);
    let styles = t.styles.as_deref().unwrap();
    assert_eq!(styles[1].color, Some(ColorSpec::Slot(7)));
    let raw = t.raw.as_deref().unwrap();
    assert_eq!(raw[1].at.line, 3);

    // Arrays with a header line read as JSON's arrays do.
    let arrays = read_jsonl("[\"a\", \"b\"]\n[1, 2]\n[3, 4]\n", &opt()).unwrap();
    same(&nums(&arrays, "b"), &[2.0, 4.0]);

    // Errors are in the whole text's places.
    let e = read_jsonl("{\"a\": 1}\n{\"a\": 2}\n{\"a\": }\n", &opt()).unwrap_err();
    assert_eq!(e.at.map(|p| (p.line, p.col)), Some((3, 7)));
    let e = read_jsonl("{\"a\": 1}\n\n  \"b\"\n", &opt()).unwrap_err();
    assert_eq!(e.msg, "expected an object like the first row, not a string");
    assert_eq!(e.at, pos(3, 3, 12));
    assert_eq!(read_jsonl("\n\n", &opt()).unwrap(), Table::default());
}

// ------------------------------------------------------------------ inference

#[test]
fn inference_reads_every_row() {
    let o = opt();
    let mut cells: Vec<Option<&str>> = vec![Some("1"); 5000];
    assert_eq!(infer("n", &cells, &o).kind(), ColumnKind::Number);
    // One string at the very end makes it no number column.
    cells.push(Some("x"));
    let c = infer("n", &cells, &o);
    assert_eq!(c.kind(), ColumnKind::Category);
    assert_eq!((c.str_at(0), c.str_at(5000)), (Some("1"), Some("x")));
    // Likewise one date among times, one number among dates.
    let t = infer("t", &[Some("2026-11-21T06:00"), Some("2026-11-21")], &o);
    assert_eq!(t.kind(), ColumnKind::Time);
    let mixed = infer("t", &[Some("2026-11-21"), Some("5")], &o);
    assert_eq!(mixed.kind(), ColumnKind::Category);
    // None is missing as the tokens are.
    let n = infer("n", &[None, Some("2"), Some(" NA ")], &o);
    assert_eq!(n.kind(), ColumnKind::Number);
    same(&[n.f64_at(0), n.f64_at(1), n.f64_at(2)], &[NAN, 2.0, NAN]);
    // A column with no values is numbers, all gaps.
    let none = infer("e", &[None, Some(""), Some("-")], &o);
    assert_eq!(none.kind(), ColumnKind::Number);
    assert_eq!(none.len(), 3);
}

#[test]
fn more_distinct_strings_than_the_limit_make_text() {
    let names: Vec<String> = (0..51).map(|i| format!("host-{i}")).collect();
    let fifty: Vec<Option<&str>> = names[..50].iter().map(|s| Some(s.as_str())).collect();
    let mut o = opt();
    let c = infer("host", &fifty, &o);
    assert_eq!(c.kind(), ColumnKind::Category);
    let mut fifty_one = fifty.clone();
    fifty_one.push(Some(&names[50]));
    fifty_one.push(None);
    let c = infer("host", &fifty_one, &o);
    assert_eq!(c.kind(), ColumnKind::Text);
    assert_eq!((c.str_at(50), c.str_at(51)), (Some("host-50"), Some("")));
    // Repeats don't count against the limit.
    o.category_limit = 2;
    let c = infer("h", &[Some("a"), Some("b"), Some("a"), Some("b")], &o);
    assert_eq!(c.kind(), ColumnKind::Category);
    let c = infer("h", &[Some("a"), Some("b"), Some("c")], &o);
    assert_eq!(c.kind(), ColumnKind::Text);
}

#[test]
fn a_time_format_reads_slashed_and_numeric_dates() {
    let mut o = opt();
    let slashed = [Some("21/11/2026"), Some("1/12/2026"), Some("NA")];
    assert_eq!(infer("d", &slashed, &o).kind(), ColumnKind::Category);
    let compact = [Some("20261121"), Some("20261201")];
    assert_eq!(infer("d", &compact, &o).kind(), ColumnKind::Number);

    o.time_format = Some("%d/%m/%Y".into());
    let c = infer("d", &slashed, &o);
    assert_eq!(c.kind(), ColumnKind::Time);
    same(
        &[c.f64_at(0), c.f64_at(1), c.f64_at(2)],
        &[at("2026-11-20T14:00Z"), at("2026-11-30T14:00Z"), NAN],
    );
    // ISO cells still read beside the format; numbers that don't match stay numbers.
    assert_eq!(
        parse_time("2026-11-21T06:00Z", &o),
        Some(at("2026-11-21T06:00Z"))
    );
    assert_eq!(
        infer("n", &[Some("1"), Some("2")], &o).kind(),
        ColumnKind::Number
    );

    // Cells that are numbers stay numbers even where the format takes them; a channel that
    // says "type": "time" reads them through it (as_time).
    o.time_format = Some("%Y%m%d".into());
    let c = infer("d", &compact, &o);
    assert_eq!(c.kind(), ColumnKind::Number);
    let c = as_time(&c, None, &o).unwrap();
    assert_eq!(c.kind(), ColumnKind::Time);
    assert_eq!(c.f64_at(1), at("2026-11-30T14:00Z"));

    // An offset or a zone in the format's text wins over the injected zone.
    o.time_format = Some("%d/%m/%Y %H:%M %z".into());
    assert_eq!(
        parse_time("21/11/2026 06:00 +0100", &o),
        Some(at("2026-11-21T05:00Z"))
    );
    o.time_format = Some("%d/%m/%Y %H:%M %Q".into());
    assert_eq!(
        parse_time("21/07/2026 06:00 Europe/London", &o),
        Some(at("2026-07-21T05:00Z"))
    );
}

#[test]
fn a_time_format_leaves_the_other_number_columns_numbers() {
    let mut o = opt();
    o.time_format = Some("%s".into());
    let t = read_csv("ts,count\n1795240800,5\n1795240860,7\n", &o).unwrap();
    use ColumnKind::*;
    assert_eq!(kinds(&t), [("ts", Number), ("count", Number)]);
    same(&nums(&t, "count"), &[5.0, 7.0]);
    let ts = as_time(t.column("ts").unwrap(), None, &o).unwrap();
    same(
        &[ts.f64_at(0), ts.f64_at(1)],
        &[at("2026-11-21T06:00Z"), at("2026-11-21T06:01Z")],
    );

    // An id beside a date column is an id.
    o.time_format = Some("%Y%m%d".into());
    let t = read_csv("d,id\n20261121,10000101\n20261201,10000102\n", &o).unwrap();
    assert_eq!(kinds(&t), [("d", Number), ("id", Number)]);
    same(&nums(&t, "id"), &[10_000_101.0, 10_000_102.0]);
    let d = as_time(t.column("d").unwrap(), None, &o).unwrap();
    let want = [at("2026-11-20T14:00Z"), at("2026-11-30T14:00Z")];
    same(&[d.f64_at(0), d.f64_at(1)], &want);

    // JSON's numbers, and its strings of digits, are read as CSV's cells are.
    let j = read_json(
        &json::parse(r#"[{"d": 20261121, "s": "20261121"}, {"d": 20261201, "s": null}]"#).unwrap(),
        &o,
    )
    .unwrap();
    assert_eq!(kinds(&j), [("d", Number), ("s", Number)]);
    let jd = as_time(j.column("d").unwrap(), None, &o).unwrap();
    same(&[jd.f64_at(0), jd.f64_at(1)], &want);

    // A number the format doesn't take is an error that names it; a unit beats the format.
    let half = Column::number("d", vec![20_261_121.5, NAN]);
    assert_eq!(
        as_time(&half, None, &o).unwrap_err().msg,
        "\"20261121.5\" in \"d\" is not a time"
    );
    let secs = as_time(&half, Some("s"), &o).unwrap();
    assert_eq!(secs.f64_at(0), 20_261_121.5);
}

// ------------------------------------------------------------------ times

#[test]
fn times_in_a_fixed_zone_in_ruling_twos_order() {
    let o = opt();
    let t = |s: &str| parse_time(s, &o);
    // A civil time is in the zone.
    let six = Some(at("2026-11-20T20:00Z"));
    assert_eq!(t("2026-11-21T06:00"), six);
    assert_eq!(t("2026-11-21 06:00"), six);
    assert_eq!(t("2026-11-21t06:00"), six);
    assert_eq!(t("  2026-11-21T06:00:00  "), six);
    assert_eq!(
        t("2026-11-21T06:00:00.250"),
        Some(at("2026-11-20T20:00:00.25Z"))
    );
    // An offset or Z wins over the zone.
    assert_eq!(t("2026-11-21T06:00:00Z"), Some(at("2026-11-21T06:00Z")));
    assert_eq!(
        t("2026-11-21T06:00:00+01:00"),
        Some(at("2026-11-21T05:00Z"))
    );
    // So does a zone annotation without an offset.
    assert_eq!(
        t("2026-11-21T06:00[America/New_York]"),
        Some(at("2026-11-21T11:00Z"))
    );
    // A date alone is its midnight in the zone.
    assert_eq!(t("2026-11-21"), Some(at("2026-11-20T14:00Z")));
    // A time part that isn't one is no date with the time dropped.
    assert_eq!(t("2026-11-21T25:00"), None);
    // A zone that doesn't exist, or is misspelt, is no time, not one in the injected zone.
    assert_eq!(t("2026-11-21T06:00[Foo/Bar]"), None);
    assert_eq!(t("2026-11-21T06:00[America/New_Yrok]"), None);
    // Slashed and day-first dates need a format; epoch numbers need a unit.
    for s in [
        "2026/11/21 06:00",
        "21/11/2026",
        "11/21/2026",
        "1795240800",
        "20261121",
        "",
        "NA",
        "web",
        "06:00",
    ] {
        assert_eq!(t(s), None, "{s}");
    }
}

#[test]
fn times_in_london_follow_its_clocks() {
    let o = london();
    let t = |s: &str| parse_time(s, &o).unwrap();
    // GMT in winter, BST in summer.
    assert_eq!(t("2026-01-15T12:00"), at("2026-01-15T12:00Z"));
    assert_eq!(t("2026-07-15T12:00"), at("2026-07-15T11:00Z"));
    assert_eq!(t("2026-07-15"), at("2026-07-14T23:00Z"));
    // The clocks go forward at 01:00 on 29 March: 01:30 never happens, and reads as the
    // instant an hour on (02:30 BST).
    assert_eq!(t("2026-03-29T01:30"), at("2026-03-29T01:30Z"));
    assert_eq!(t("2026-03-29T00:59"), at("2026-03-29T00:59Z"));
    assert_eq!(t("2026-03-29T02:00"), at("2026-03-29T01:00Z"));
    // They go back at 02:00 BST on 25 October: 01:30 happens twice, and reads as the first.
    assert_eq!(t("2026-10-25T01:30"), at("2026-10-25T00:30Z"));
    assert_eq!(t("2026-10-25T02:30"), at("2026-10-25T02:30Z"));
    // That day is 25 hours long.
    assert_eq!(t("2026-10-26") - t("2026-10-25"), 25.0 * 3600.0);
    assert_eq!(t("2026-03-30") - t("2026-03-29"), 23.0 * 3600.0);
    // An offset still wins.
    assert_eq!(t("2026-07-15T12:00Z"), at("2026-07-15T12:00Z"));
}

#[test]
fn epoch_numbers_are_times_only_with_a_unit() {
    let o = opt();
    let secs = Column::number("t", vec![1_795_240_800.0, NAN]);
    let c = as_time(&secs, Some("s"), &o).unwrap();
    assert_eq!(c.kind(), ColumnKind::Time);
    same(&[c.f64_at(0), c.f64_at(1)], &[at("2026-11-21T06:00Z"), NAN]);
    let millis = Column::number("t", vec![1_795_240_800_250.0]);
    let c = as_time(&millis, Some("ms"), &o).unwrap();
    assert_eq!(c.f64_at(0), at("2026-11-21T06:00:00.25Z"));

    let e = as_time(&secs, None, &o).unwrap_err();
    assert_eq!(
        e.msg,
        "\"t\" holds numbers: give \"unit\": \"s\" or \"ms\" to read them as epoch times"
    );
    let e = as_time(&secs, Some("us"), &o).unwrap_err();
    assert_eq!(
        e.msg,
        "unit \"us\": an epoch time's unit is \"s\" or \"ms\""
    );

    // Strings are parsed; a time column is as it was.
    let s = Column::category("t", &["2026-11-21T06:00", "NA"]);
    let c = as_time(&s, None, &o).unwrap();
    same(&[c.f64_at(0), c.f64_at(1)], &[at("2026-11-20T20:00Z"), NAN]);
    let again = as_time(&c, None, &o).unwrap();
    assert_eq!(format!("{again:?}"), format!("{c:?}"));
    let bad = Column::text("t", &["2026-11-21", "soon"]);
    assert_eq!(
        as_time(&bad, None, &o).unwrap_err().msg,
        "\"soon\" in \"t\" is not a time"
    );
}

// ------------------------------------------------------------------ transforms

fn wide() -> Table {
    let mut t = Table::new(vec![
        Column::time("time", vec![0.0, 60.0]),
        Column::number("api", vec![1.0, 2.0]),
        Column::number("worker", vec![10.0, NAN]),
        Column::number("batch", vec![100.0, 200.0]),
    ])
    .unwrap();
    t.styles = Some(
        vec![
            FigStyle::default(),
            FigStyle {
                dash: true,
                ..FigStyle::default()
            },
        ]
        .into(),
    );
    t
}

fn fold(fields: &[&str], key: &str, value: &str) -> Transform {
    Transform::Fold {
        fields: fields.iter().map(|s| s.to_string()).collect(),
        key: key.into(),
        value: value.into(),
    }
}

fn by(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn fold_makes_wide_rows_long() {
    let t = apply(
        &wide(),
        &[fold(&["api", "worker", "batch"], "service", "rpm")],
    )
    .unwrap();
    use ColumnKind::*;
    assert_eq!(
        kinds(&t),
        [("time", Time), ("service", Category), ("rpm", Number)]
    );
    assert_eq!(t.rows, 6);
    same(&nums(&t, "time"), &[0.0, 0.0, 0.0, 60.0, 60.0, 60.0]);
    assert_eq!(dict(&t, "service"), ["api", "worker", "batch"]);
    assert_eq!(
        strs(&t, "service"),
        ["api", "worker", "batch", "api", "worker", "batch"].map(Some)
    );
    same(&nums(&t, "rpm"), &[1.0, 10.0, 100.0, 2.0, NAN, 200.0]);
    // Each row's fig block follows it to its folded rows.
    let dashes: Vec<bool> = t
        .styles
        .as_deref()
        .unwrap()
        .iter()
        .map(|s| s.dash)
        .collect();
    assert_eq!(dashes, [false, false, false, true, true, true]);

    // Strings fold into a category.
    let s = Table::new(vec![
        Column::category("a", &["x", "y"]),
        Column::text("b", &["y", "z"]),
    ])
    .unwrap();
    let f = apply(&s, &[fold(&["a", "b"], "k", "v")]).unwrap();
    assert_eq!(dict(&f, "v"), ["x", "y", "z"]);
    // Text's empty string is missing once folded.
    let gap = Table::new(vec![
        Column::text("a", &["x", ""]),
        Column::text("b", &["", "y"]),
    ])
    .unwrap();
    let f = apply(&gap, &[fold(&["a", "b"], "k", "v")]).unwrap();
    assert_eq!(strs(&f, "v"), [Some("x"), None, None, Some("y")]);
}

#[test]
fn fold_errors_name_the_column() {
    let e = apply(&wide(), &[fold(&["api", "apii"], "k", "v")]).unwrap_err();
    assert_eq!(
        e.to_string(),
        "fold: no column \"apii\"; the columns are time, api, worker, batch"
    );
    let e = apply(&wide(), &[fold(&["api"], "time", "v")]).unwrap_err();
    assert_eq!(e.msg, "fold: a column is already named \"time\"");
    let e = apply(&wide(), &[fold(&["api", "time"], "k", "v")]).unwrap_err();
    assert_eq!(
        e.msg,
        "fold: \"api\" holds numbers and \"time\" times; fold fields of one kind"
    );
    let e = apply(&wide(), &[fold(&[], "k", "v")]).unwrap_err();
    assert_eq!(e.msg, "fold: no fields to fold");
    let e = apply(&Table::default(), &[fold(&["a"], "k", "v")]).unwrap_err();
    assert_eq!(e.msg, "fold: no column \"a\"; the table has none");
}

fn hosts() -> Table {
    let mut t = Table::new(vec![
        Column::category("host", &["web", "db", "cache", "web", "db", "cache"]),
        Column::number("ms", vec![5.0, 50.0, 1.0, NAN, 70.0, 3.0]),
        Column::time("time", vec![0.0, 0.0, 0.0, 60.0, 60.0, 60.0]),
        Column::text("note", &["a", "b", "c", "d", "e", "f"]),
    ])
    .unwrap();
    let raw = json::parse("[1, 2, 3, 4, 5, 6]").unwrap();
    t.raw = Some(raw.as_arr().unwrap().to_vec().into());
    t
}

fn filter(field: &str, op: FilterOp) -> Transform {
    Transform::Filter {
        field: field.into(),
        op,
    }
}

#[test]
fn filter_keeps_rows_and_recodes_categories() {
    let only = apply(
        &hosts(),
        &[filter("host", FilterOp::In(by(&["db", "cache"])))],
    )
    .unwrap();
    assert_eq!(only.rows, 4);
    // "web" is gone from the dictionary, and "db" is first now.
    assert_eq!(dict(&only, "host"), ["db", "cache"]);
    same(&nums(&only, "ms"), &[50.0, 1.0, 70.0, 3.0]);
    // Rows keep their objects.
    let raw: Vec<f64> = only
        .raw
        .as_deref()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    assert_eq!(raw, [2.0, 3.0, 5.0, 6.0]);

    let not = apply(
        &hosts(),
        &[filter("note", FilterOp::NotIn(by(&["a", "f"])))],
    )
    .unwrap();
    assert_eq!(strs(&not, "note"), ["b", "c", "d", "e"].map(Some));

    // Numbers by value; a range is inclusive and drops gaps; not-in keeps them.
    let n = apply(&hosts(), &[filter("ms", FilterOp::In(by(&["5", "70.0"])))]).unwrap();
    same(&nums(&n, "ms"), &[5.0, 70.0]);
    let n = apply(&hosts(), &[filter("ms", FilterOp::NotIn(by(&["5"])))]).unwrap();
    same(&nums(&n, "ms"), &[50.0, 1.0, NAN, 70.0, 3.0]);
    let r = apply(
        &hosts(),
        &[filter("ms", FilterOp::Range(Some(3.0), Some(50.0)))],
    )
    .unwrap();
    same(&nums(&r, "ms"), &[5.0, 50.0, 3.0]);
    let r = apply(&hosts(), &[filter("ms", FilterOp::Range(None, Some(3.0)))]).unwrap();
    same(&nums(&r, "ms"), &[1.0, 3.0]);
    let r = apply(
        &hosts(),
        &[filter("time", FilterOp::Range(Some(60.0), None))],
    )
    .unwrap();
    assert_eq!(strs(&r, "host"), ["web", "db", "cache"].map(Some));

    // A missing category is never in a list, and always not in one.
    let m = Table::new(vec![Column {
        name: "c".into(),
        values: Values::Category {
            codes: vec![0, u32::MAX].into(),
            dict: vec!["a".into()].into(),
        },
    }])
    .unwrap();
    assert_eq!(
        apply(&m, &[filter("c", FilterOp::In(by(&["a"])))])
            .unwrap()
            .rows,
        1
    );
    let kept = apply(&m, &[filter("c", FilterOp::NotIn(by(&["a"])))]).unwrap();
    assert_eq!(strs(&kept, "c"), [None]);
    assert_eq!(dict(&kept, "c"), Vec::<&str>::new());
}

#[test]
fn filter_errors() {
    let e = apply(
        &hosts(),
        &[filter("host", FilterOp::Range(Some(1.0), None))],
    )
    .unwrap_err();
    assert_eq!(
        e.msg,
        "filter: a range needs numbers or times, and \"host\" holds categories"
    );
    let e = apply(&hosts(), &[filter("time", FilterOp::In(by(&["0"])))]).unwrap_err();
    assert_eq!(
        e.msg,
        "filter: \"time\" holds times; filter them with a range"
    );
    let e = apply(&hosts(), &[filter("ms", FilterOp::In(by(&["fast"])))]).unwrap_err();
    assert_eq!(
        e.msg,
        "filter: \"fast\" is not a number, and \"ms\" holds numbers"
    );
}

fn bin(step: Option<f64>, max_bins: usize) -> Transform {
    Transform::Bin {
        field: "v".into(),
        step,
        max_bins,
        start: "lo".into(),
        end: "hi".into(),
    }
}

#[test]
fn bin_puts_values_in_nice_aligned_bins() {
    let t = Table::new(vec![Column::number(
        "v",
        vec![3.0, 17.0, 20.0, 0.0, NAN, 9.99],
    )])
    .unwrap();
    let b = apply(&t, &[bin(Some(10.0), 10)]).unwrap();
    same(&nums(&b, "lo"), &[0.0, 10.0, 10.0, 0.0, NAN, 0.0]);
    same(&nums(&b, "hi"), &[10.0, 20.0, 20.0, 10.0, NAN, 10.0]);
    assert_eq!(b.columns.len(), 3);

    // A picked step: 0 to 20 in at most 4 bins is a step of 5, the largest value in the last.
    let b = apply(&t, &[bin(None, 4)]).unwrap();
    same(&nums(&b, "lo"), &[0.0, 15.0, 15.0, 0.0, NAN, 5.0]);
    // In at most 3 it's 10, as 5 would make four.
    let b = apply(&t, &[bin(None, 3)]).unwrap();
    same(&nums(&b, "lo"), &[0.0, 10.0, 10.0, 0.0, NAN, 0.0]);

    // Decimal steps give decimal edges.
    let d = Table::new(vec![Column::number("v", vec![0.31, 0.05, -0.15, 0.7])]).unwrap();
    let b = apply(&d, &[bin(Some(0.1), 10)]).unwrap();
    same(&nums(&b, "lo"), &[0.3, 0.0, -0.2, 0.6]);
    same(&nums(&b, "hi"), &[0.4, 0.1, -0.1, 0.7]);
    let b = apply(&d, &[bin(None, 10)]).unwrap();
    same(&nums(&b, "lo"), &[0.3, 0.0, -0.2, 0.6]);

    // One value, and none.
    let one = Table::new(vec![Column::number("v", vec![7.0, 7.0])]).unwrap();
    let b = apply(&one, &[bin(None, 10)]).unwrap();
    same(&nums(&b, "lo"), &[7.0, 7.0]);
    same(&nums(&b, "hi"), &[8.0, 8.0]);
    let none = Table::new(vec![Column::number("v", vec![NAN])]).unwrap();
    same(
        &nums(&apply(&none, &[bin(None, 10)]).unwrap(), "lo"),
        &[NAN],
    );

    let e = apply(&t, &[bin(Some(0.0), 10)]).unwrap_err();
    assert_eq!(e.msg, "bin: a step is a positive number, not 0");
    let c = Table::new(vec![Column::category("v", &["a"])]).unwrap();
    let e = apply(&c, &[bin(None, 10)]).unwrap_err();
    assert_eq!(e.msg, "bin: \"v\" holds categories, not numbers");
}

/// `v` binned into `lo` and `hi`, every value inside its bin: `lo <= x < hi`, or `x == hi` for
/// the largest.
#[track_caller]
fn binned(v: &[f64], step: Option<f64>, max_bins: usize) -> Table {
    let t = Table::new(vec![Column::number("v", v.to_vec())]).unwrap();
    let b = apply(&t, &[bin(step, max_bins)]).unwrap();
    let (lo, hi) = (nums(&b, "lo"), nums(&b, "hi"));
    let max = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    for (i, &x) in v.iter().enumerate() {
        let inside = lo[i] <= x && (x < hi[i] || (x == max && x == hi[i]));
        assert!(inside, "{x} is not in [{}, {})", lo[i], hi[i]);
    }
    b
}

#[test]
fn bin_edges_hold_their_values_far_from_zero() {
    // Near a billion by ones; the largest on an edge goes below.
    let b = binned(&[1e9 + 0.95, 1e9 + 0.5, 1e9 + 5.0], Some(1.0), 10);
    same(&nums(&b, "lo"), &[1e9, 1e9, 1e9 + 4.0]);

    // Epoch milliseconds by the second, given and picked.
    let ms = [
        1_795_240_800_850.0,
        1_795_240_801_900.0,
        1_795_240_800_000.0,
        1_795_240_803_999.0,
    ];
    let b = binned(&ms, Some(1000.0), 10);
    let lo = [
        1_795_240_800_000.0,
        1_795_240_801_000.0,
        1_795_240_800_000.0,
        1_795_240_803_000.0,
    ];
    same(&nums(&b, "lo"), &lo);
    let b = binned(&ms, None, 4);
    same(&nums(&b, "lo"), &lo);

    // Epoch seconds by the minute.
    let secs = [
        1_795_240_859.9,
        1_795_240_800.0,
        1_795_240_860.0,
        1_795_240_919.0,
    ];
    let b = binned(&secs, Some(60.0), 10);
    let lo = [
        1_795_240_800.0,
        1_795_240_800.0,
        1_795_240_860.0,
        1_795_240_860.0,
    ];
    same(&nums(&b, "lo"), &lo);

    // Every 100 ms over two seconds, in at most ten bins: a step of 200, odd values too.
    let every: Vec<f64> = (0..20).map(|i| 1.7e12 + 100.0 * f64::from(i)).collect();
    let b = binned(&every, None, 10);
    same(&nums(&b, "lo")[..3], &[1.7e12, 1.7e12, 1.7e12 + 200.0]);
    same(&nums(&b, "hi")[..1], &[1.7e12 + 200.0]);

    // Ten billion by twos; cents and tenths of large values.
    let tens: Vec<f64> = (0..=10).map(|i| 1e10 + f64::from(i)).collect();
    let b = binned(&tens, Some(2.0), 10);
    same(&nums(&b, "lo")[..2], &[1e10, 1e10]);
    let b = binned(&[1_234_567.899_9, 1_234_567.0], Some(0.01), 10);
    same(&nums(&b, "lo")[..1], &[1_234_567.89]);
    let b = binned(&[123_456_789.99, 123_456_789.0], Some(0.1), 10);
    same(&nums(&b, "lo")[..1], &[123_456_789.9]);
}

#[test]
fn bin_across_zero_takes_two_bins_at_least() {
    // Bins sit on multiples of the step, so no one bin holds both signs: one bin asked for is
    // two.
    let b = binned(&[-1.0, 0.5, 1.0], None, 1);
    same(&nums(&b, "lo"), &[-1.0, 0.0, 0.0]);
    same(&nums(&b, "hi"), &[0.0, 1.0, 1.0]);
    let b = binned(&[-0.5, 0.5], None, 1);
    same(&nums(&b, "lo"), &[-0.5, 0.0]);
    same(&nums(&b, "hi"), &[0.0, 0.5]);
    // Up to zero, or from it, one bin is one bin.
    let b = binned(&[-1.0, 0.0], None, 1);
    same(&nums(&b, "lo"), &[-1.0, -1.0]);
    let b = binned(&[2.0, 3.0], None, 1);
    same(&nums(&b, "hi"), &[3.0, 3.0]);
    // Zero's edge is +0, from -0.0 too, so no axis shows "-0".
    let b = binned(&[-0.0, 1.5, -1e-20], Some(1.0), 10);
    let (lo, hi) = (nums(&b, "lo"), nums(&b, "hi"));
    assert!(
        lo[0].is_sign_positive() && hi[2].is_sign_positive(),
        "{lo:?} {hi:?}"
    );
}

#[test]
fn bin_steps_that_arent_decimals_keep_their_own_edges() {
    let third = 1.0 / 3.0;
    let b = binned(&[1.0, 0.5, 2.0, 0.999_999_999_5], Some(third), 10);
    same(&nums(&b, "lo")[..2], &[3.0 * third, third]);
    same(&nums(&b, "hi")[..2], &[4.0 * third, 2.0 * third]);
    // A decimal step keeps decimal edges.
    let b = binned(&[0.21, 0.3, 0.0], Some(0.07), 10);
    same(&nums(&b, "lo"), &[0.21, 0.28, 0.0]);
    same(&nums(&b, "hi"), &[0.28, 0.35, 0.07]);
}

fn agg(op: AggOp, field: Option<&str>, group: &[&str], out: &str) -> Transform {
    Transform::Aggregate {
        op,
        field: field.map(String::from),
        by: by(group),
        out: out.into(),
    }
}

#[test]
fn aggregate_gives_a_row_a_group() {
    let t = hosts();
    let sum = apply(&t, &[agg(AggOp::Sum, Some("ms"), &["host"], "total")]).unwrap();
    assert_eq!(
        kinds(&sum),
        [
            ("host", ColumnKind::Category),
            ("total", ColumnKind::Number)
        ]
    );
    assert_eq!(strs(&sum, "host"), ["web", "db", "cache"].map(Some));
    // The gap is skipped.
    same(&nums(&sum, "total"), &[5.0, 120.0, 4.0]);
    assert_eq!((sum.styles.clone(), sum.raw.clone()), (None, None));

    let each = |op| {
        nums(
            &apply(&t, &[agg(op, Some("ms"), &["host"], "v")]).unwrap(),
            "v",
        )
    };
    same(&each(AggOp::Mean), &[5.0, 60.0, 2.0]);
    same(&each(AggOp::Min), &[5.0, 50.0, 1.0]);
    same(&each(AggOp::Max), &[5.0, 70.0, 3.0]);
    same(&each(AggOp::P50), &[5.0, 60.0, 2.0]);
    same(&each(AggOp::Count), &[1.0, 2.0, 2.0]);
    let rows = apply(&t, &[agg(AggOp::Count, None, &["host"], "n")]).unwrap();
    same(&nums(&rows, "n"), &[2.0, 2.0, 2.0]);

    // Two columns group by their pairs, in the order first met.
    let two = apply(&t, &[agg(AggOp::Count, None, &["time", "host"], "n")]).unwrap();
    assert_eq!(two.rows, 6);
    // No `by` is one row, even of no rows.
    let all = apply(&t, &[agg(AggOp::Max, Some("ms"), &[], "max")]).unwrap();
    same(&nums(&all, "max"), &[70.0]);
    let empty = Table::new(vec![Column::number("ms", vec![])]).unwrap();
    let n = apply(&empty, &[agg(AggOp::Count, None, &[], "n")]).unwrap();
    same(&nums(&n, "n"), &[0.0]);
    let s = apply(&empty, &[agg(AggOp::Sum, Some("ms"), &[], "s")]).unwrap();
    same(&nums(&s, "s"), &[NAN]);
}

#[test]
fn percentiles_interpolate_between_ranks() {
    let v: Vec<f64> = (1..=20).map(f64::from).collect();
    let t = Table::new(vec![Column::number("v", v)]).unwrap();
    let p = |op| nums(&apply(&t, &[agg(op, Some("v"), &[], "p")]).unwrap(), "p")[0];
    assert_eq!(p(AggOp::P50), 10.5);
    assert!((p(AggOp::P95) - 19.05).abs() < 1e-12);
    assert_eq!(p(AggOp::Mean), 10.5);
    assert_eq!(p(AggOp::Sum), 210.0);
}

#[test]
fn aggregate_of_times_and_its_errors() {
    let t = hosts();
    let last = apply(&t, &[agg(AggOp::Max, Some("time"), &["host"], "last")]).unwrap();
    assert_eq!(last.column("last").unwrap().kind(), ColumnKind::Time);
    same(&nums(&last, "last"), &[60.0, 60.0, 60.0]);
    let n = apply(&t, &[agg(AggOp::Count, Some("time"), &[], "n")]).unwrap();
    assert_eq!(n.column("n").unwrap().kind(), ColumnKind::Number);

    let e = apply(&t, &[agg(AggOp::Sum, Some("time"), &[], "s")]).unwrap_err();
    assert_eq!(
        e.msg,
        "aggregate: sum needs numbers, and \"time\" holds times"
    );
    let e = apply(&t, &[agg(AggOp::Mean, Some("host"), &[], "s")]).unwrap_err();
    assert_eq!(
        e.msg,
        "aggregate: mean needs numbers or times, and \"host\" holds categories"
    );
    let e = apply(&t, &[agg(AggOp::P95, None, &[], "s")]).unwrap_err();
    assert_eq!(e.msg, "aggregate: p95 needs a field");
    let e = apply(&t, &[agg(AggOp::Count, None, &["hots"], "n")]).unwrap_err();
    assert_eq!(
        e.msg,
        "aggregate: no column \"hots\"; the columns are host, ms, time, note"
    );
    let e = apply(&t, &[agg(AggOp::Count, None, &["host"], "host")]).unwrap_err();
    assert_eq!(e.msg, "aggregate: a column is already named \"host\"");
    let e = apply(&t, &[agg(AggOp::Count, None, &["host", "host"], "n")]).unwrap_err();
    assert_eq!(e.msg, "aggregate: \"host\" is listed twice in by");
}

#[test]
fn cumulative_runs_within_each_group_past_gaps() {
    let t = apply(
        &hosts(),
        &[Transform::Cumulative {
            field: "ms".into(),
            by: by(&["host"]),
            out: "run".into(),
        }],
    )
    .unwrap();
    same(&nums(&t, "run"), &[5.0, 50.0, 1.0, NAN, 120.0, 4.0]);
    // Rows and their objects are as they were.
    assert_eq!(t.rows, 6);
    assert_eq!(t.raw.as_ref().map(|r| r.len()), Some(6));

    let all = Table::new(vec![Column::number("v", vec![1.0, NAN, 2.0, 3.0])]).unwrap();
    let c = apply(
        &all,
        &[Transform::Cumulative {
            field: "v".into(),
            by: vec![],
            out: "c".into(),
        }],
    )
    .unwrap();
    same(&nums(&c, "c"), &[1.0, NAN, 3.0, 6.0]);
    let e = apply(
        &all,
        &[Transform::Cumulative {
            field: "v".into(),
            by: vec![],
            out: "v".into(),
        }],
    )
    .unwrap_err();
    assert_eq!(e.msg, "cumulative: a column is already named \"v\"");
}

#[test]
fn window_is_a_trailing_mean_within_each_group() {
    let t = Table::new(vec![
        Column::category("g", &["a", "a", "b", "a", "a", "b", "a"]),
        Column::number("v", vec![1.0, 2.0, 10.0, 3.0, NAN, 20.0, 7.0]),
    ])
    .unwrap();
    let w = |size, group: &[&str]| {
        let tr = Transform::Window {
            field: "v".into(),
            size,
            by: by(group),
            out: "m".into(),
        };
        apply(&t, &[tr]).map(|t| nums(&t, "m"))
    };
    // a: 1, 2, 3, NaN, 7 → 1, 1.5, 2, gap, mean(3, 7) over the last three rows (3, NaN, 7).
    same(
        &w(3, &["g"]).unwrap(),
        &[1.0, 1.5, 10.0, 2.0, NAN, 15.0, 5.0],
    );
    same(
        &w(1, &["g"]).unwrap(),
        &[1.0, 2.0, 10.0, 3.0, NAN, 20.0, 7.0],
    );
    // Ungrouped, over every row.
    same(&w(2, &[]).unwrap(), &[1.0, 1.5, 6.0, 6.5, NAN, 20.0, 13.5]);
    assert_eq!(
        w(0, &[]).unwrap_err().msg,
        "window: a window is at least 1 row"
    );
}

#[test]
fn window_keeps_a_running_sum_that_loses_nothing() {
    // A huge value leaving the window takes the small ones with it in a plain running sum.
    let t = Table::new(vec![Column::number("v", vec![1e17, 1.0, 1.0, 1.0])]).unwrap();
    let tr = |size| Transform::Window {
        field: "v".into(),
        size,
        by: vec![],
        out: "m".into(),
    };
    same(
        &nums(&apply(&t, &[tr(2)]).unwrap(), "m"),
        &[1e17, 5e16, 1.0, 1.0],
    );

    // The same as each window's mean summed afresh, over gaps and three groups.
    let mut seed = 7u64;
    let mut v = Vec::new();
    for i in 0..600 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let x = (seed >> 11) as f64 / (1u64 << 53) as f64;
        v.push(if i % 7 == 3 { NAN } else { (x - 0.5) * 1e4 });
    }
    let g: Vec<&str> = (0..600).map(|i| ["a", "b", "c"][i % 3]).collect();
    let t = Table::new(vec![
        Column::category("g", &g),
        Column::number("v", v.clone()),
    ])
    .unwrap();
    for size in [1, 5, 40, 1000] {
        let got = nums(
            &apply(
                &t,
                &[Transform::Window {
                    field: "v".into(),
                    size,
                    by: by(&["g"]),
                    out: "m".into(),
                }],
            )
            .unwrap(),
            "m",
        );
        for r in 0..600 {
            let mine: Vec<f64> = (r % 3..=r).step_by(3).map(|q| v[q]).collect();
            let last: Vec<f64> = mine[mine.len().saturating_sub(size)..]
                .iter()
                .copied()
                .filter(|x| !x.is_nan())
                .collect();
            let want = match v[r].is_nan() {
                true => NAN,
                false => last.iter().sum::<f64>() / last.len() as f64,
            };
            let ok = (got[r].is_nan() && want.is_nan()) || (got[r] - want).abs() <= 1e-9;
            assert!(ok, "size {size}, row {r}: got {}, want {want}", got[r]);
        }
    }
}

fn sort(field: &str, descending: bool) -> Transform {
    Transform::Sort {
        field: field.into(),
        descending,
    }
}

#[test]
fn sort_is_stable_with_gaps_last() {
    let t = hosts();
    let up = apply(&t, &[sort("ms", false)]).unwrap();
    same(&nums(&up, "ms"), &[1.0, 3.0, 5.0, 50.0, 70.0, NAN]);
    let down = apply(&t, &[sort("ms", true)]).unwrap();
    same(&nums(&down, "ms"), &[70.0, 50.0, 5.0, 3.0, 1.0, NAN]);
    assert_eq!(
        strs(&down, "note"),
        ["e", "b", "a", "f", "c", "d"].map(Some)
    );
    // The categories are re-coded in their new order.
    assert_eq!(dict(&down, "host"), ["db", "web", "cache"]);
    let raw: Vec<f64> = down
        .raw
        .as_deref()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    assert_eq!(raw, [5.0, 2.0, 1.0, 6.0, 3.0, 4.0]);

    // Strings by code point, stable: equal hosts keep their order.
    let by_host = apply(&t, &[sort("host", false)]).unwrap();
    assert_eq!(
        strs(&by_host, "note"),
        ["c", "f", "b", "e", "a", "d"].map(Some)
    );
    let by_time = apply(&t, &[sort("time", true)]).unwrap();
    assert_eq!(
        strs(&by_time, "note"),
        ["d", "e", "f", "a", "b", "c"].map(Some)
    );
    let e = apply(&t, &[sort("nope", false)]).unwrap_err();
    assert!(e.msg.starts_with("sort: no column \"nope\""), "{e}");
}

fn top(n: usize) -> Transform {
    Transform::Top {
        n,
        by: "service".into(),
        field: "rpm".into(),
    }
}

fn services() -> Table {
    Table::new(vec![
        Column::time("time", vec![0.0, 0.0, 0.0, 0.0, 60.0, 60.0, 60.0, 60.0]),
        Column::category(
            "service",
            &[
                "small", "api", "tiny", "batch", "api", "small", "batch", "tiny",
            ],
        ),
        Column::number("rpm", vec![5.0, 100.0, 1.0, 50.0, 110.0, 6.0, 40.0, NAN]),
    ])
    .unwrap()
}

#[test]
fn top_keeps_the_largest_groups_and_sums_the_rest_into_other() {
    let t = apply(&services(), &[top(2)]).unwrap();
    // api (210) and batch (90) are kept, in source order; small and tiny become Other at each
    // time, after every kept row.
    assert_eq!(
        strs(&t, "service"),
        ["api", "batch", "api", "batch", OTHER, OTHER].map(Some)
    );
    same(&nums(&t, "time"), &[0.0, 0.0, 60.0, 60.0, 0.0, 60.0]);
    same(&nums(&t, "rpm"), &[100.0, 50.0, 110.0, 40.0, 6.0, 6.0]);
    assert_eq!(dict(&t, "service"), ["api", "batch", OTHER]);
    assert_eq!(t.column("service").unwrap().kind(), ColumnKind::Category);

    // n groups or fewer: unchanged.
    same_table(&apply(&services(), &[top(4)]).unwrap(), &services());
    // None kept: everything is Other.
    let none = apply(&services(), &[top(0)]).unwrap();
    assert_eq!(strs(&none, "service"), [Some(OTHER), Some(OTHER)]);
    same(&nums(&none, "rpm"), &[156.0, 156.0]);
}

#[test]
fn top_makes_one_other_a_time_whatever_else_the_rows_hold() {
    let t = Table::new(vec![
        Column::time("time", vec![0.0, 0.0, 0.0, 60.0, 60.0, 60.0]),
        Column::category("service", &["api", "small", "tiny", "api", "small", "tiny"]),
        Column::number("rpm", vec![100.0, 5.0, 6.0, 110.0, 7.0, 8.0]),
        Column::number("errors", vec![1.0, 2.0, 3.0, 4.0, NAN, NAN]),
        Column::text("note", &["a", "b", "c", "d", "e", "f"]),
    ])
    .unwrap();
    let k = apply(&t, &[top(1)]).unwrap();
    // One Other at each time, every measure summed, text left out.
    assert_eq!(strs(&k, "service"), ["api", "api", OTHER, OTHER].map(Some));
    same(&nums(&k, "time"), &[0.0, 60.0, 0.0, 60.0]);
    same(&nums(&k, "rpm"), &[100.0, 110.0, 11.0, 15.0]);
    same(&nums(&k, "errors"), &[1.0, 4.0, 5.0, NAN]);
    assert_eq!(strs(&k, "note"), [Some("a"), Some("d"), Some(""), Some("")]);
    assert_eq!(
        kinds(&k),
        kinds(&t),
        "every column keeps its kind and its place"
    );

    // Another category is a key as a time is: an Other for each region at each time.
    let mut cols = t.columns.clone();
    cols.push(Column::category(
        "region",
        &["eu", "eu", "us", "eu", "eu", "us"],
    ));
    let k = apply(&Table::new(cols).unwrap(), &[top(1)]).unwrap();
    assert_eq!(strs(&k, "region")[2..], ["eu", "us", "eu", "us"].map(Some));
    same(&nums(&k, "time")[2..], &[0.0, 0.0, 60.0, 60.0]);
    same(&nums(&k, "rpm")[2..], &[5.0, 6.0, 7.0, 8.0]);

    // With no time or category beside it, the x is a number column, and Other is one an x.
    let n = Table::new(vec![
        Column::number("x", vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0]),
        Column::category("service", &["api", "small", "tiny", "api", "small", "tiny"]),
        Column::number("rpm", vec![100.0, 5.0, 6.0, 110.0, 7.0, 8.0]),
    ])
    .unwrap();
    let k = apply(&n, &[top(1)]).unwrap();
    same(&nums(&k, "x"), &[1.0, 2.0, 1.0, 2.0]);
    same(&nums(&k, "rpm"), &[100.0, 110.0, 11.0, 15.0]);
}

#[test]
fn top_folds_a_missing_group_and_an_existing_other() {
    let t = Table::new(vec![
        Column::text("service", &["a", "Other", "b", "", "a", "c"]),
        Column::number("rpm", vec![1.0, 1000.0, 2.0, 500.0, 1.0, 2.0]),
    ])
    .unwrap();
    // b and c tie at 2: the first met wins. "Other" and the missing one are never kept.
    let k = apply(
        &t,
        &[Transform::Top {
            n: 2,
            by: "service".into(),
            field: "rpm".into(),
        }],
    )
    .unwrap();
    assert_eq!(k.column("service").unwrap().kind(), ColumnKind::Text);
    assert_eq!(strs(&k, "service"), ["a", "b", "a", OTHER].map(Some));
    same(&nums(&k, "rpm"), &[1.0, 2.0, 1.0, 1502.0]);

    let e = apply(
        &t,
        &[Transform::Top {
            n: 2,
            by: "rpm".into(),
            field: "rpm".into(),
        }],
    )
    .unwrap_err();
    assert_eq!(
        e.msg,
        "top: \"rpm\" holds numbers; top ranks the groups of a category or text column"
    );
}

#[test]
fn transforms_chain_in_order() {
    // Wide to long, the busiest service per minute's total, largest first.
    let t = apply(
        &wide(),
        &[
            fold(&["api", "worker", "batch"], "service", "rpm"),
            agg(AggOp::Sum, Some("rpm"), &["service"], "total"),
            sort("total", true),
            filter("total", FilterOp::Range(Some(5.0), None)),
        ],
    )
    .unwrap();
    assert_eq!(strs(&t, "service"), [Some("batch"), Some("worker")]);
    same(&nums(&t, "total"), &[300.0, 10.0]);
    assert_eq!(dict(&t, "service"), ["batch", "worker"]);
    same_table(&apply(&wide(), &[]).unwrap(), &wide());
}

#[test]
fn tables_from_readers_go_to_jobs() {
    fn send_sync<T: Send + Sync + 'static>(_: &T) {}
    let t = csv("a\n1\n");
    send_sync(&t);
    let handle = std::thread::spawn(move || apply(&t, &[]).map(|t| t.rows));
    assert_eq!(handle.join().unwrap().unwrap(), 1);
}
