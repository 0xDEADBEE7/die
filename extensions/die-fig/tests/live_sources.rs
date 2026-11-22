//! Live sources with real processes through `sh`, and no window (contract-p4 §4): a run's rows,
//! a skipped tick, a failure's error and stderr tail, the 64 MB cap, a stream folding at most
//! thirty times a second, the 200,000-row cap, a stream's end and `R`, killing on close with no
//! zombie (and SIGKILL for a command that ignores SIGTERM), the build waiting for a command and
//! built from its table, follow mode sliding and stopping on a pan, and the cursor kept on its
//! sample as rows drop from the front.
//!
//! The clock is the test's: each poll moves it on, and real time passes with it while the
//! commands run. Every test stops what it started and waits for it to be gone.

use die_test_cap as _;

use die_fig::data::{ColumnKind, ReadOptions, Table};
use die_fig::error::ErrorKind;
use die_fig::interact::{Cursor, Toggles};
use die_fig::live::{
    Event, FOLD_EVERY, FollowChange, KILL_GRACE, Live, LiveKey, LiveSpec, MAX_ROWS, PathFrom,
    Rerun, at_end, follow, keep_cursor,
};
use die_fig::plot::ChartModel;
use die_fig::source::{self, Job, Text};
use die_fig::spec::{DataRef, DataSpec, Figure, ReadHints};
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/live")
}

fn live() -> Live {
    Live::new(TimeZone::UTC, PathFrom::Inherited)
}

fn data(source: DataRef) -> DataSpec {
    DataSpec {
        source,
        format: None,
        read: ReadHints::default(),
    }
}

fn run_spec(cmd: &str, every: Option<Duration>, dir: &Path) -> LiveSpec {
    let d = data(DataRef::Run {
        cmd: cmd.into(),
        every,
    });
    LiveSpec::of(&d, dir).unwrap()
}

fn stream_spec(cmd: &str, dir: &Path) -> LiveSpec {
    LiveSpec::of(&data(DataRef::Stream { cmd: cmd.into() }), dir).unwrap()
}

fn key(cmd: &str) -> LiveKey {
    LiveKey::new(Some(Path::new("/specs/live.fig")), cmd)
}

/// What an event is, for a failure's message: no tables in full.
fn said(events: &[Event]) -> Vec<String> {
    let one = |e: &Event| match e {
        Event::Began(k) => format!("began {}", k.cmd),
        Event::Data { key, table } => format!("data {} ({} rows)", key.cmd, table.rows),
        Event::Failed { key, error } => format!("failed {}: {}", key.cmd, error.msg),
        Event::Ended { key, how, .. } => format!("ended {} ({how})", key.cmd),
    };
    events.iter().map(one).collect()
}

/// Poll every 10 ms, the clock and real time both moving on, until an event `done` takes
/// comes; everything that came, that one last. Ten seconds at most.
fn until(live: &mut Live, now: &mut Duration, mut done: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let mut all = vec![];
    for _ in 0..1000 {
        *now += Duration::from_millis(10);
        for e in live.poll(*now) {
            let hit = done(&e);
            all.push(e);
            if hit {
                return all;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("nothing came in 10 s: {:?}", said(&all));
}

fn is_data(e: &Event) -> bool {
    matches!(e, Event::Data { .. })
}

fn table_of(e: &Event) -> Arc<Table> {
    match e {
        Event::Data { table, .. } => table.clone(),
        other => panic!("not data: {:?}", said(std::slice::from_ref(other))),
    }
}

/// Whether a process (or with `group`, anything in its group) is there, a zombie included.
fn exists(pid: u32, group: bool) -> bool {
    let target = if group {
        format!("-{pid}")
    } else {
        pid.to_string()
    };
    Command::new("/bin/kill")
        .args(["-s", "0", "--", &target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Stop everything and wait for it to be gone.
fn finish(mut live: Live) {
    assert!(
        live.stop_all().wait(KILL_GRACE * 3),
        "every command gone within the grace"
    );
}

#[test]
fn a_run_prints_rows_read_as_a_file_would_be_in_its_specs_directory() {
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key("./errors.sh");
    assert!(live.start(k.clone(), run_spec("./errors.sh", None, &fixtures()), now));
    assert!(!live.start(k.clone(), run_spec("./errors.sh", None, &fixtures()), now));
    let events = until(&mut live, &mut now, is_data);
    let t = table_of(events.last().unwrap());
    assert_eq!(t.rows, 20, "ten minutes of two services");
    let kinds: Vec<ColumnKind> = t.columns.iter().map(|c| c.kind()).collect();
    assert_eq!(
        kinds,
        [ColumnKind::Time, ColumnKind::Category, ColumnKind::Number]
    );
    let st = live.status(&k).unwrap();
    assert_eq!((st.runs, st.running, st.updated), (1, false, Some(now)));
    assert_eq!(live.next_poll(now), None, "run once: nothing due, no timer");
    finish(live);
}

#[test]
fn a_tick_while_a_run_is_going_is_skipped_and_r_says_it_still_runs() {
    let dir = std::env::temp_dir();
    let cmd = "sleep 0.5; printf 'x,y\\n1,2\\n'";
    let mut live = live();
    let k = key(cmd);
    let every = Duration::from_millis(100);
    live.start(k.clone(), run_spec(cmd, Some(every), &dir), Duration::ZERO);
    assert_eq!(live.next_poll(Duration::ZERO), Some(FOLD_EVERY));
    // Ticks at 100 and 200 ms come while the first run is still going.
    assert!(live.poll(Duration::from_millis(100)).is_empty());
    assert!(live.poll(Duration::from_millis(250)).is_empty());
    let st = live.status(&k).unwrap();
    assert_eq!((st.runs, st.skipped, st.running), (1, 2, true));
    let r = live.rerun(None, Duration::from_millis(260));
    assert_eq!(r, [Rerun::StillRunning(k.clone())]);
    assert_eq!(
        r[0].message().unwrap(),
        format!("fig: {cmd} is still running")
    );
    // Once it's done, the next tick runs it again.
    let mut now = Duration::from_millis(260);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Began(_)));
    assert!(events.iter().any(is_data), "{:?}", said(&events));
    assert_eq!(live.status(&k).unwrap().runs, 2);
    finish(live);
}

#[test]
fn a_failure_is_unreadable_with_its_status_and_the_last_twenty_lines_of_stderr() {
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key("./fail.sh");
    live.start(k.clone(), run_spec("./fail.sh", None, &fixtures()), now);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Failed { .. }));
    let Some(Event::Failed { error, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(error.kind, ErrorKind::Unreadable);
    assert_eq!(error.msg, "./fail.sh exited with status 3");
    let dir = fixtures().display().to_string();
    assert_eq!(
        error.context[0],
        format!("ran `./fail.sh` in {dir}: exit 3")
    );
    let stderr: Vec<&String> = error.context[1..].iter().collect();
    assert_eq!(stderr.len(), 20);
    assert_eq!(
        stderr[0],
        "stderr: fail.sh: step 6 of 25: the upstream said no"
    );
    assert_eq!(
        stderr[19],
        "stderr: fail.sh: step 25 of 25: the upstream said no"
    );
    let st = live.status(&k).unwrap();
    assert!(st.error.is_some_and(|(at, e)| at == now && e == *error));
    finish(live);
}

#[test]
fn a_run_past_64_mb_is_stopped_unreadable_and_gone() {
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key("yes 0123456789");
    live.start(
        k.clone(),
        run_spec("yes 0123456789", None, &std::env::temp_dir()),
        now,
    );
    let pid = live.status(&k).unwrap().pid;
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Failed { .. }));
    let Some(Event::Failed { error, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(error.kind, ErrorKind::Unreadable);
    assert_eq!(error.msg, "yes 0123456789 printed more than 64 MB");
    if let Some(pid) = pid {
        assert!(!exists(pid, false), "stopped and waited for");
    }
    finish(live);
}

#[test]
fn a_stream_folds_at_most_thirty_times_a_second() {
    let dir = std::env::temp_dir();
    let cmd = "echo n; i=0; while :; do echo $i; i=$((i + 1)); done";
    let mut live = live();
    let k = key(cmd);
    live.start(k.clone(), stream_spec(cmd, &dir), Duration::ZERO);
    let (mut folds, mut rows) = (vec![], 0);
    let step = Duration::from_millis(5);
    let mut now = Duration::ZERO;
    for _ in 0..300 {
        now += step;
        for e in live.poll(now) {
            if let Event::Data { table, .. } = &e {
                assert!(table.rows >= rows, "rows only grow");
                rows = table.rows;
                folds.push(now);
            }
        }
        // While rows keep coming, the next poll is the fold's pace away at most.
        let next = live.next_poll(now).unwrap();
        assert!(next <= now + FOLD_EVERY);
        std::thread::sleep(step);
    }
    assert!(
        folds.len() >= 10,
        "it folds while rows come: {}",
        folds.len()
    );
    for w in folds.windows(2) {
        assert!(w[1] - w[0] >= FOLD_EVERY, "folds {:?} apart", w[1] - w[0]);
    }
    for (i, t) in folds.iter().enumerate() {
        let in_a_second = folds[i..]
            .iter()
            .take_while(|u| **u < *t + Duration::from_secs(1));
        assert!(in_a_second.count() <= 30);
    }
    assert!(live.status(&k).unwrap().running);
    finish(live);
}

#[test]
fn a_stream_keeps_the_newest_200000_rows_and_its_data_when_it_ends() {
    let dir = std::env::temp_dir();
    let cmd = "awk 'BEGIN { print \"n,v\"; for (i = 0; i < 250000; i++) print i \",\" i % 7 }'";
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key(cmd);
    live.start(k.clone(), stream_spec(cmd, &dir), now);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Ended { .. }));
    let last = events
        .iter()
        .rev()
        .find(|e| is_data(e))
        .map(table_of)
        .unwrap();
    assert_eq!(last.rows, MAX_ROWS);
    assert_eq!(last.columns[0].f64_at(0), 50_000.0, "the oldest dropped");
    assert_eq!(last.columns[0].f64_at(MAX_ROWS - 1), 249_999.0);
    let Some(Event::Ended { how, error, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!((how.as_str(), error), ("exit 0", &None));
    let st = live.status(&k).unwrap();
    assert_eq!((st.rows, st.dropped, st.running), (MAX_ROWS, 50_000, false));
    assert_eq!(
        st.notes(),
        ["keeping the newest 200,000 rows", "stream ended (exit 0)"]
    );
    assert_eq!(live.next_poll(now), None, "an ended stream needs no timer");
    finish(live);
}

#[test]
fn a_stream_that_ends_keeps_its_rows_and_r_starts_it_again() {
    let dir = std::env::temp_dir();
    let cmd = "printf 't,v\\n1,10\\n2,20\\n'";
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key(cmd);
    live.start(k.clone(), stream_spec(cmd, &dir), now);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Ended { .. }));
    let i = events.iter().position(is_data).unwrap();
    assert!(i < events.len() - 1, "its rows come before its end");
    assert_eq!(table_of(&events[i]).rows, 2);
    assert_eq!(live.rerun(Some(&k), now), [Rerun::Started(k.clone())]);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Ended { .. }));
    let t = events
        .iter()
        .rev()
        .find(|e| is_data(e))
        .map(table_of)
        .unwrap();
    assert_eq!(t.rows, 4, "the rows stay, the new ones after them");
    assert_eq!(live.status(&k).unwrap().runs, 2);
    finish(live);
}

#[test]
fn a_stream_whose_header_cant_be_one_fails_malformed() {
    let dir = std::env::temp_dir();
    let cmd = "printf 'a,a\\n1,2\\n'; sleep 5";
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key(cmd);
    live.start(k.clone(), stream_spec(cmd, &dir), now);
    let events = until(&mut live, &mut now, |e| matches!(e, Event::Failed { .. }));
    let Some(Event::Failed { error, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(error.kind, ErrorKind::Malformed);
    assert!(
        error.msg.contains("two columns named \"a\""),
        "{}",
        error.msg
    );
    assert!(!events.iter().any(is_data));
    assert!(
        !live.status(&k).unwrap().running,
        "stopped, not left to sleep"
    );
    finish(live);
}

#[test]
fn closing_kills_the_group_and_leaves_no_zombie() {
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key("./ticker.sh");
    live.start(k.clone(), stream_spec("./ticker.sh", &fixtures()), now);
    let events = until(&mut live, &mut now, is_data);
    let t = table_of(events.last().unwrap());
    assert_eq!(
        t.columns.iter().map(|c| c.kind()).collect::<Vec<_>>(),
        [ColumnKind::Time, ColumnKind::Number, ColumnKind::Number]
    );
    let pid = live.status(&k).unwrap().pid.expect("running");
    assert!(exists(pid, true));
    assert!(live.stop(&k).wait(KILL_GRACE * 3), "gone within the grace");
    assert!(!exists(pid, false), "waited for: no zombie");
    assert!(!exists(pid, true), "nothing left in its group");
    assert!(live.is_empty() && live.next_poll(now).is_none());

    // Dropping the figure's sources does the same.
    let mut live = Live::new(TimeZone::UTC, PathFrom::Inherited);
    let k = key("sleep 30");
    live.start(
        k.clone(),
        stream_spec("sleep 30", &std::env::temp_dir()),
        now,
    );
    let mut pid = None;
    for _ in 0..200 {
        pid = live.status(&k).unwrap().pid;
        if pid.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = pid.expect("started");
    drop(live);
    let gone = (0..300).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        !exists(pid, true)
    });
    assert!(gone, "a dropped Live stops its commands");
}

#[test]
fn a_command_that_ignores_sigterm_gets_sigkill_after_the_grace() {
    let cmd = "trap '' TERM; echo n; while :; do echo 1; sleep 0.05; done";
    let mut live = live();
    let mut now = Duration::ZERO;
    let k = key(cmd);
    live.start(k.clone(), stream_spec(cmd, &std::env::temp_dir()), now);
    until(&mut live, &mut now, is_data);
    let pid = live.status(&k).unwrap().pid.expect("running");
    let stopped = live.stop(&k);
    assert!(!stopped.wait(KILL_GRACE / 2), "SIGTERM is ignored");
    assert!(stopped.wait(KILL_GRACE * 3), "SIGKILL after the grace");
    assert!(!exists(pid, true));
}

#[test]
fn set_starts_the_new_stops_the_gone_and_keeps_a_run_whose_tick_changed() {
    let dir = std::env::temp_dir();
    let mut live = live();
    let (a, b) = (key("sleep 30"), key("printf 'x\\n1\\n'"));
    let wants = vec![
        (a.clone(), stream_spec("sleep 30", &dir)),
        (
            b.clone(),
            run_spec("printf 'x\\n1\\n'", Some(Duration::from_secs(5)), &dir),
        ),
    ];
    assert_eq!(live.set(&wants, Duration::ZERO), [a.clone(), b.clone()]);
    assert_eq!(live.keys(), [a.clone(), b.clone()]);
    // The run's every changes: it goes on, ticking from now; the stream is no longer wanted.
    let now = Duration::from_secs(1);
    let wants = vec![(
        b.clone(),
        run_spec("printf 'x\\n1\\n'", Some(Duration::from_secs(2)), &dir),
    )];
    assert!(live.set(&wants, now).is_empty());
    assert_eq!(live.keys(), std::slice::from_ref(&b));
    assert_eq!(live.status(&b).unwrap().runs, 1);
    assert!(live.next_poll(now).unwrap() <= now + Duration::from_secs(2));
    finish(live);
}

/// A chart's model from CSV text, as `\v` builds one.
fn model(csv: &str) -> ChartModel {
    let job = Job {
        text: Text::Given(csv.into()),
        name: "feed.csv".into(),
        dir: std::env::temp_dir(),
        zone: TimeZone::UTC,
        open: vec![],
        toggles: Toggles::default(),
        auto: None,
    };
    source::build(job).ok().unwrap().model.unwrap()
}

#[test]
fn the_build_waits_for_a_command_and_is_built_from_what_it_printed() {
    let spec = r#"{"type": "line", "data": {"run": "./errors.sh", "every": "30s"},
        "x": "time", "y": "errors", "color": "service"}"#;
    let job = || Job {
        text: Text::Given(spec.into()),
        name: "health.fig".into(),
        dir: fixtures(),
        zone: TimeZone::UTC,
        open: vec![],
        toggles: Toggles::default(),
        auto: None,
    };
    let Err(f) = source::build(job()) else {
        panic!("no output yet: it waits")
    };
    assert!(f.is_waiting());
    let want = run_spec("./errors.sh", Some(Duration::from_secs(30)), &fixtures());
    assert_eq!(f.waiting(), [&want]);
    assert_eq!((f.feeds.len(), f.feeds[0].table), (1, 0));
    assert_eq!(f.errors[0].msg, "waiting for ./errors.sh");
    assert_eq!(f.errors[0].kind, ErrorKind::Empty);
    // Run it, and build from what it printed.
    let mut live = live();
    let mut now = Duration::ZERO;
    live.start(key("./errors.sh"), want.clone(), now);
    let events = until(&mut live, &mut now, is_data);
    let table = table_of(events.last().unwrap());
    let Ok(b) = source::build_with(job(), &[(want.clone(), table.clone())]) else {
        panic!("built from the command's table")
    };
    let m = b.model.unwrap();
    assert_eq!(m.keys().len(), 2, "api and worker");
    assert_eq!(b.feeds[0].spec, want);
    // A fold's new table: the model again from the tables in hand.
    let chart = b.chart.unwrap();
    let again = source::rebuild(&chart, vec![table], Toggles::default(), &TimeZone::UTC);
    assert!(again.is_ok());
    // A command that printed a header and no rows: nothing to draw, naming it.
    let empty = Arc::new(Table::default());
    let Err(f) = source::build_with(job(), &[(want, empty)]) else {
        panic!("nothing to draw")
    };
    assert!(!f.is_waiting());
    assert_eq!(f.errors[0].msg, "nothing to draw: ./errors.sh has no rows");
    finish(live);
}

#[test]
fn live_fig_is_a_dashboard_of_a_stream_and_a_run_every_five_seconds() {
    let path = fixtures().join("live.fig");
    let text = std::fs::read_to_string(&path).unwrap();
    let opt = ReadOptions::new(TimeZone::UTC);
    let p = die_fig::spec::parse(&text, "live.fig", &fixtures(), &opt).unwrap();
    let Figure::Dashboard(d) = p.figure else {
        panic!("a dashboard")
    };
    let sources: Vec<&DataRef> = d.data.iter().map(|(_, s)| &s.source).collect();
    assert!(matches!(sources[0], DataRef::Stream { cmd } if cmd == "./ticker.sh"));
    assert!(matches!(
        sources[1],
        DataRef::Run { cmd, every: Some(e) } if cmd == "./errors.sh" && *e == Duration::from_secs(5)
    ));
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

#[test]
fn follow_slides_a_following_window_and_stops_on_a_pan_away() {
    let px = 0.05;
    // Following at the end: new rows slide the window, its width kept.
    let f = follow(Some((0.0, 10.0)), Some(12.0), true, px, FollowChange::Data);
    assert_eq!((f.x, f.on), (Some((2.0, 12.0)), true));
    // Newer data inside the window moves nothing.
    let f = follow(Some((0.0, 12.0)), Some(11.0), true, px, FollowChange::Data);
    assert_eq!(f.x, Some((0.0, 12.0)));
    // A pan into the past stops it, and new data then never moves the view.
    let f = follow(Some((-3.0, 7.0)), Some(12.0), true, px, FollowChange::Moved);
    assert!(!f.on);
    let f = follow(f.x, Some(14.0), f.on, px, FollowChange::Data);
    assert_eq!((f.x, f.on), (Some((-3.0, 7.0)), false));
    // Panned back to the end, it follows again.
    let f = follow(
        Some((4.0, 13.98)),
        Some(14.0),
        false,
        px,
        FollowChange::Moved,
    );
    assert!(f.on, "within a pixel of the newest");
    // A brush away from the end stops it; G resumes, to the end.
    let f = follow(Some((6.0, 9.0)), Some(14.0), true, px, FollowChange::Moved);
    assert!(!f.on);
    let f = follow(f.x, Some(14.0), f.on, px, FollowChange::Resume);
    assert_eq!((f.x, f.on), (Some((11.0, 14.0)), true));
    // A view that fits the data grows with it, at its end.
    let f = follow(None, Some(20.0), false, px, FollowChange::Moved);
    assert_eq!((f.x, f.on), (None, true));
    let f = follow(None, Some(21.0), true, px, FollowChange::Data);
    assert_eq!((f.x, f.on), (None, true));
    assert!(
        at_end((0.0, 12.0), 10.0, px),
        "past the newest is at the end"
    );
    assert!(!at_end((0.0, 9.9), 10.0, px));
    assert!(!at_end((11.0, 12.0), 10.0, px), "the newest out of view");
}

#[test]
fn the_cursor_stays_on_its_sample_as_rows_drop_from_the_front() {
    let old = model("t,v\n1,10\n2,20\n3,30\n4,40\n");
    let new = model("t,v\n3,30\n4,40\n5,50\n6,60\n");
    let series = old.keys()[0].clone();
    let c = Cursor::Point {
        series: series.clone(),
        row: 3,
    };
    let kept = keep_cursor(&old, &new, &c);
    assert_eq!(
        kept,
        Some(Cursor::Point { series, row: 1 }),
        "t = 4 is row 1 now"
    );
    let gone = Cursor::Point {
        series: old.keys()[0].clone(),
        row: 0,
    };
    assert_eq!(keep_cursor(&old, &new, &gone), None, "t = 1 dropped");
}
