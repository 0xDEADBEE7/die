//! Where a figure's text and data come from, and how they're followed: the source buffer's
//! edits, a file's changes by poll; and in phase 4 commands run and streamed, each asked for
//! once by name (ruling 13).
//!
//! A figure's own text is a buffer's (followed as it's edited), a file's (`:fig FILE`, polled
//! while a window shows it), a command's output (`:fig !cmd`, run once), or a selection's lines
//! as they were. Reading it, its data files and building the chart's model all happen in one
//! job, [`build`], off the main thread: the job is given the text of every buffer that visits a
//! file, so a data file open in a buffer is read from the buffer and follows its edits, and the
//! rest from disk, polled. A chart's model is built there, and so is a graph's, laid out once
//! (fig's stress layout is `O(n²)`); a spec whose data is a command's (`run`, `stream`) says
//! those arrive in a later build, as do tiles and dashboards.

use crate::data::{ReadOptions, Table};
use crate::error::{ErrorKind, FigError, Pos, Result};
use crate::graph::GraphModel;
use crate::interact::Toggles;
use crate::plot::ChartModel;
use crate::spec::chart::with_hints;
use crate::spec::{
    Chart, DataFormat, DataRef, DataSpec, Figure, GraphSpec, Parsed, format_of_name,
};
use die_core::BufferId;
use jiff::tz::TimeZone;
use std::fmt::Display;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// What a phase-2 build says of a source it can't read yet.
pub const LATER_RUN: &str = "run and stream sources arrive in a later build";

/// Where a figure's own text comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A text buffer, whose text is a spec or data, followed as it's edited.
    Buffer(BufferId),
    /// A file no buffer visits, read from disk and polled while a window shows it.
    File(PathBuf),
    /// A command's output, run once in `dir` (`:fig !cmd`), and what it printed.
    Command {
        /// The command line, without the `!`.
        cmd: String,
        /// Where it ran.
        dir: PathBuf,
        /// What it printed, once it has.
        out: Option<Arc<str>>,
    },
    /// A selection's lines, as they were when charted.
    Text(Arc<str>),
}

impl Source {
    /// Whether two are the same source, whatever they hold: one buffer, one file however its
    /// path is spelled, one command line in one directory. A selection is the same as nothing,
    /// its text being a moment's.
    pub fn same(&self, other: &Source) -> bool {
        match (self, other) {
            (Source::Buffer(a), Source::Buffer(b)) => a == b,
            (Source::File(a), Source::File(b)) => same_path(a, b),
            (Source::Command { cmd: a, dir: d, .. }, Source::Command { cmd: b, dir: e, .. }) => {
                a == b && d == e
            }
            _ => false,
        }
    }

    /// The source without what it held, to find it by again: a command's output dropped. None
    /// for a selection's lines, which nothing finds again.
    pub fn identity(&self) -> Option<Source> {
        match self {
            Source::Command { cmd, dir, .. } => Some(Source::Command {
                cmd: cmd.clone(),
                dir: dir.clone(),
                out: None,
            }),
            Source::Text(_) => None,
            other => Some(other.clone()),
        }
    }

    /// What a restart keeps of it to find it again, if anything: a file's path, a command line.
    pub fn key(&self) -> Option<String> {
        match self {
            Source::File(p) => Some(p.display().to_string()),
            Source::Command { cmd, .. } => Some(format!("!{cmd}")),
            Source::Buffer(_) | Source::Text(_) => None,
        }
    }
}

/// A buffer's text as a job gets it: a snapshot that writes itself out on the job's thread, so
/// the main thread copies nothing (a rope's clone shares its chunks).
pub type Snapshot = Box<dyn Display + Send>;

/// A figure's text, as the job gets it: a buffer's, a file to read, or text already in hand.
pub enum Text {
    /// A buffer's text.
    Of(Snapshot),
    /// A file to read.
    File(PathBuf),
    /// Text in hand.
    Given(Arc<str>),
}

/// What a build job is given.
pub struct Job {
    /// The figure's own text.
    pub text: Text,
    /// Its file's name, which says its format by its extension.
    pub name: String,
    /// The directory relative paths are in.
    pub dir: PathBuf,
    /// The zone times without an offset are in.
    pub zone: TimeZone,
    /// Each buffer that visits a file: its path and its text.
    pub open: Vec<(PathBuf, Snapshot)>,
    /// What the figure changed of its spec, applied before the model is built.
    pub toggles: Toggles,
}

/// What a build job made.
pub struct Built {
    /// The figure as parsed.
    pub figure: Figure,
    /// Whether it was guessed from bare data.
    pub inferred: bool,
    /// The chart as parsed, before any toggles.
    pub chart: Option<Chart>,
    /// The chart's tables: its data's, then each layer's own.
    pub tables: Vec<Arc<Table>>,
    /// The chart's model: by value, since a model need only be `Send`.
    pub model: Option<ChartModel>,
    /// The graph's model, laid out, when the figure is a graph.
    pub graph: Option<GraphModel>,
    /// What the spec had that was odd but not wrong.
    pub warnings: Vec<FigError>,
    /// The files read from disk, and when each last changed: polled while shown. A figure
    /// whose own text is a file's has it first.
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The data files read from buffers: followed as they're edited.
    pub followed: Vec<PathBuf>,
    /// The figure's own text as the build read it, for the log's caret lines.
    pub text: Arc<str>,
}

/// Why a build failed: every error the build found (the first is the one the stale badge and
/// the symbolic figure name), the warnings of the same parse, the figure's own text as it was
/// read (for the log's caret lines), and the files it read on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Failed {
    /// What went wrong, and where: never empty.
    pub errors: Vec<FigError>,
    /// What the spec had that was odd but not wrong, in the same parse.
    pub warnings: Vec<FigError>,
    /// The figure's own text as the build read it.
    pub text: Arc<str>,
    /// The files read from disk before it failed: polled, so fixing one brings the figure back.
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Failed {
    /// A failure of one error, with nothing read.
    pub fn one(error: FigError) -> Failed {
        Failed {
            errors: vec![error],
            warnings: vec![],
            text: Arc::from(""),
            files: vec![],
        }
    }

    /// The error the badge and the symbolic figure name.
    pub fn first(&self) -> &FigError {
        &self.errors[0]
    }

    /// Every error, then every warning, as the log lists them.
    pub fn all(&self) -> Vec<FigError> {
        let warnings = self.warnings.iter().map(|w| w.clone().warn());
        self.errors.iter().cloned().chain(warnings).collect()
    }
}

/// Parse a figure's text, read its data and build its chart's model: off the main thread.
/// A failure has every error the build could find: the parse's (with its place), a data file's
/// (naming it), each field the data hasn't and each cell of text where the spec says numbers,
/// the model's; each with what was tried on the way.
pub fn build(job: Job) -> std::result::Result<Built, Failed> {
    let mut files = vec![];
    let text = match &job.text {
        Text::Of(r) => r.to_string(),
        Text::File(p) => {
            files.push((p.clone(), modified(p)));
            match read_file(p) {
                Ok(t) => t,
                Err(e) => {
                    let e = e.note(format!("looked for {}", p.display()));
                    return Err(Failed {
                        files,
                        ..Failed::one(e)
                    });
                }
            }
        }
        Text::Given(s) => s.to_string(),
    };
    let text: Arc<str> = Arc::from(text);
    let failed = |errors: Vec<FigError>, warnings: Vec<FigError>, files| Failed {
        errors,
        warnings,
        text: text.clone(),
        files,
    };
    let opt = ReadOptions::new(job.zone.clone());
    let Parsed {
        figure,
        inferred,
        mut warnings,
    } = match crate::spec::parse(&text, &job.name, &job.dir, &opt) {
        Ok(p) => p,
        Err(e) => {
            let e = e.note(read_as(&job.name, &text));
            return Err(failed(vec![e], vec![], files));
        }
    };
    // In the order they're in the text, as a reader goes down it.
    warnings.sort_by_key(|w| w.at.map(|a| (a.line, a.col)));
    let chart = match figure.clone() {
        Figure::Chart(c) => c,
        Figure::Graph(spec) => {
            // Laid out here, off the main thread: the last frame stays up until it's done.
            return match graph_model(&spec, job.toggles, &job.name) {
                Ok(g) => Ok(Built {
                    figure,
                    inferred,
                    chart: None,
                    tables: vec![],
                    model: None,
                    graph: Some(g),
                    warnings,
                    files,
                    followed: vec![],
                    text,
                }),
                Err(e) => Err(failed(vec![e], warnings, files)),
            };
        }
        other => {
            let why = format!("{} arrive in a later build", kind_plural(&other));
            let e = FigError::new(why).with_kind(ErrorKind::Empty);
            return Err(failed(vec![e], warnings, files));
        }
    };
    let mut reader = Reader {
        open: &job.open,
        opt: &opt,
        dir: &job.dir,
        spec: if inferred { None } else { Some(&text) },
        files: std::mem::take(&mut files),
        followed: vec![],
        read: vec![],
    };
    let tables = match reader.tables(&chart) {
        Ok(Ok(t)) => t,
        Ok(Err(error)) => return Err(failed(vec![error], warnings, reader.files)),
        Err(Later) => {
            let e = FigError::new(LATER_RUN).with_kind(ErrorKind::Empty);
            return Err(failed(vec![e], warnings, reader.files));
        }
    };
    // Data that's all there but has no rows: nothing to draw, whatever the spec says.
    if !tables.is_empty() && tables.iter().all(|t| t.rows == 0) {
        let named = reader.read.first().map(|r| shown(&r.path));
        let what = named.unwrap_or_else(|| "the data".into());
        let mut e = FigError::new(format!("nothing to draw: {what} has no rows"))
            .with_kind(ErrorKind::Empty);
        for r in &reader.read {
            e = e.note(r.said());
        }
        return Err(failed(vec![e], warnings, reader.files));
    }
    let model = match ChartModel::build(job.toggles.apply(&chart), tables.clone(), &job.zone) {
        Ok(m) => m,
        Err(e) => {
            let errors = problems(&chart, &tables, &reader, e);
            return Err(failed(errors, warnings, reader.files));
        }
    };
    // Rows read, and none left to chart (a filter that keeps nothing): nothing to draw too.
    if model.series.iter().all(|s| s.xs.is_empty())
        && model.rules.is_empty()
        && model.texts.is_empty()
    {
        let names: Vec<&str> = (chart.transform.iter())
            .chain(chart.layers.iter().flat_map(|l| &l.transform))
            .map(transform_name)
            .collect();
        let why = match names.is_empty() {
            true => "nothing to draw: no rows to chart",
            false => "nothing to draw: no rows after the transforms",
        };
        let mut e = FigError::new(why).with_kind(ErrorKind::Empty);
        if !names.is_empty() {
            e = e.note(format!("the transforms: {}", names.join(" → ")));
        }
        for r in &reader.read {
            e = e.note(r.said());
        }
        return Err(failed(vec![e], warnings, reader.files));
    }
    Ok(Built {
        figure,
        inferred,
        chart: Some(chart),
        tables,
        model: Some(model),
        graph: None,
        warnings,
        files: reader.files,
        followed: reader.followed,
        text,
    })
}

/// How a figure's own text was read, for the log: `read NAME as JSON`.
fn read_as(name: &str, text: &str) -> String {
    let format = format_of_name(name).unwrap_or_else(|| sniff(text));
    let how = match format {
        DataFormat::Csv => "CSV",
        DataFormat::Tsv => "TSV",
        DataFormat::Json => "JSON",
        DataFormat::Jsonl => "JSON lines",
    };
    let name = if name.is_empty() { "the text" } else { name };
    format!("read {name} as {how}")
}

/// Every error a model's build would meet, not just its first: each layer's transforms, each
/// field the data hasn't, each column of text a spec says is numbers (at the cell, in the data
/// file, where the rows are the file's), with what was read. The build's own error first if
/// the search doesn't find it.
fn problems(
    chart: &Chart,
    tables: &[Arc<Table>],
    reader: &Reader<'_>,
    first: FigError,
) -> Vec<FigError> {
    let mut out: Vec<FigError> = Vec::new();
    let push = |e: FigError, out: &mut Vec<FigError>| {
        if !out.iter().any(|o| o.msg == e.msg && o.at == e.at) {
            out.push(e);
        }
    };
    let mut own = 0;
    for layer in &chart.layers {
        let ti = match layer.data {
            Some(_) => {
                own += 1;
                own
            }
            None => 0,
        };
        let Some(table) = tables.get(ti) else {
            continue;
        };
        let read = reader.read.iter().find(|r| r.table == ti);
        let name = read.map_or_else(|| "the data".to_string(), |r| shown(&r.path));
        let ts: Vec<crate::data::transform::Transform> = chart
            .transform
            .iter()
            .chain(&layer.transform)
            .cloned()
            .collect();
        let t = match crate::data::transform::apply(table, &ts) {
            Ok(t) => t,
            Err(e) => {
                let names: Vec<&str> = ts.iter().map(transform_name).collect();
                let mut e = e.note(format!("the transforms: {}", names.join(" → ")));
                if let Some(r) = read {
                    e = e.note(r.said());
                }
                push(e, &mut out);
                continue;
            }
        };
        let enc = &layer.enc;
        let channels = [&enc.x, &enc.y, &enc.y2, &enc.color, &enc.size, &enc.label];
        for ch in channels.into_iter().flatten() {
            let Some(f) = ch.field.as_deref() else {
                continue;
            };
            let Some(col) = t.column(f) else {
                let msg = format!("no column \"{f}\" in the data");
                let mut e = match ch.at {
                    Some(at) => FigError::at(msg, at),
                    None => FigError::new(msg),
                };
                let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
                e = e.note(format!("{name} has {}", names.join(", ")));
                if let Some(r) = read {
                    e = e.note(r.said());
                }
                push(e, &mut out);
                continue;
            };
            if ch.kind != Some(crate::data::ColumnKind::Number) {
                continue;
            }
            let Some((row, v)) = crate::plot::not_a_number(col, reader.opt.thousands) else {
                continue;
            };
            let msg = format!("\"{v}\" in \"{f}\" is not a number");
            let said = match ch.at {
                Some(at) => format!(
                    "the spec says \"{f}\" is a number: line {}, column {}",
                    at.line, at.col
                ),
                None => format!("the spec says \"{f}\" is a number"),
            };
            // At the cell, where the table's rows are the file's.
            let cell = read
                .filter(|_| ts.is_empty())
                .and_then(|r| r.cell(table, row, f).map(|at| (r, at)));
            let mut e = match cell {
                Some((r, at)) => FigError::at(msg, at).in_file(&r.path, Some(&r.text)),
                None => match ch.at {
                    Some(at) => FigError::at(msg, at),
                    None => FigError::new(msg),
                },
            };
            e = e.note(said).note(format!("row {} of {name}", row + 1));
            if let Some(r) = read {
                e = e.note(r.said());
            }
            push(e, &mut out);
        }
    }
    if !out.iter().any(|o| o.msg == first.msg) {
        let mut first = first;
        for r in &reader.read {
            first = first.note(r.said());
        }
        out.insert(0, first);
    }
    out
}

/// A transform's name, as the log lists them.
fn transform_name(t: &crate::data::transform::Transform) -> &'static str {
    use crate::data::transform::Transform as T;
    match t {
        T::Fold { .. } => "fold",
        T::Filter { .. } => "filter",
        T::Bin { .. } => "bin",
        T::Aggregate { .. } => "aggregate",
        T::Cumulative { .. } => "cumulative",
        T::Window { .. } => "window",
        T::Sort { .. } => "sort",
        T::Top { .. } => "top",
    }
}

/// Build a chart's model again from what's already read: a toggle changed the spec.
pub fn rebuild(
    chart: &Chart,
    tables: Vec<Arc<Table>>,
    toggles: Toggles,
    zone: &TimeZone,
) -> Result<ChartModel> {
    ChartModel::build(toggles.apply(chart), tables, zone)
}

/// A graph's model from its spec with the settings turned (its labels, its layout), named for
/// its subtitle by the file it came from, laid out: run it in a job.
pub fn graph_model(spec: &GraphSpec, toggles: Toggles, name: &str) -> Result<GraphModel> {
    let model = GraphModel::build_as(toggles.apply_graph(spec), toggles.graph_layout())?;
    Ok(match name.is_empty() {
        true => model,
        false => model.named(name),
    })
}

/// A figure's kind in the plural, for what arrives later.
fn kind_plural(f: &Figure) -> &'static str {
    match f {
        Figure::Chart(_) => "charts",
        Figure::Graph(_) => "graphs",
        Figure::Stat(_) | Figure::Meter(_) => "stat and meter tiles",
        Figure::Dashboard(_) => "dashboards",
    }
}

/// A source the build can't read yet: a command's.
struct Later;

/// A data file read: which table it made, its path, text and format, and how many rows.
struct ReadFile {
    table: usize,
    path: PathBuf,
    /// The path as the log writes it: from the figure's directory.
    shown: String,
    text: String,
    format: DataFormat,
    from_buffer: bool,
    rows: usize,
    columns: usize,
}

impl ReadFile {
    /// What was tried, as the log says it.
    fn said(&self) -> String {
        let how = match self.format {
            DataFormat::Csv => "CSV",
            DataFormat::Tsv => "TSV",
            DataFormat::Json => "JSON",
            DataFormat::Jsonl => "JSON lines",
        };
        let whence = if self.from_buffer {
            " (its buffer)"
        } else {
            ""
        };
        let rows = if self.rows == 1 { "row" } else { "rows" };
        let cols = if self.columns == 1 {
            "column"
        } else {
            "columns"
        };
        format!(
            "read {}{whence} as {how}: {} {rows}, {} {cols}",
            self.shown, self.rows, self.columns
        )
    }

    /// Where row `row` of column `name` is in the file's text, if the format says.
    fn cell(&self, table: &Table, row: usize, name: &str) -> Option<Pos> {
        match self.format {
            DataFormat::Csv | DataFormat::Tsv => {
                let mut opt = ReadOptions::new(TimeZone::UTC);
                if self.format == DataFormat::Tsv {
                    opt.delimiter = Some(b'\t');
                }
                crate::data::csv::cell_at(&self.text, &opt, row, name).map(|(at, _)| at)
            }
            DataFormat::Json | DataFormat::Jsonl => {
                let raw = table.raw.as_ref()?.get(row)?;
                Some(raw.get(name)?.at)
            }
        }
    }
}

/// Reads a chart's tables, from the buffers given where one visits the file and otherwise
/// from disk, noting which was which and what each held.
struct Reader<'a> {
    open: &'a [(PathBuf, Snapshot)],
    opt: &'a ReadOptions,
    /// The spec's directory, which its data paths are relative to.
    dir: &'a Path,
    /// The spec's own text, where the figure's text is one: a data path's place is in it.
    spec: Option<&'a str>,
    files: Vec<(PathBuf, Option<SystemTime>)>,
    followed: Vec<PathBuf>,
    read: Vec<ReadFile>,
}

impl Reader<'_> {
    /// The chart's data's table, then each layer's own, in order.
    fn tables(
        &mut self,
        chart: &Chart,
    ) -> std::result::Result<std::result::Result<Vec<Arc<Table>>, FigError>, Later> {
        let mut out = vec![];
        let own = chart.layers.iter().filter_map(|l| l.data.as_ref());
        for (i, spec) in std::iter::once(&chart.data).chain(own).enumerate() {
            match self.table(i, spec)? {
                Ok(t) => out.push(t),
                Err(e) => return Ok(Err(e)),
            }
        }
        Ok(Ok(out))
    }

    fn table(
        &mut self,
        i: usize,
        spec: &DataSpec,
    ) -> std::result::Result<Result<Arc<Table>>, Later> {
        Ok(match &spec.source {
            DataRef::Inline(t) => Ok(t.clone()),
            DataRef::File(path) => self.file(i, path, spec),
            DataRef::Run { .. } | DataRef::Stream { .. } => return Err(Later),
            DataRef::Named(name) => Err(FigError::new(format!(
                "\"{name}\" names a dashboard's data, outside a dashboard"
            ))),
            DataRef::Source => Err(FigError::new(
                "\"data\": \"-\" is the buffer itself, which is the spec here",
            )),
        })
    }

    fn file(&mut self, i: usize, path: &Path, spec: &DataSpec) -> Result<Arc<Table>> {
        let (text, from_buffer) = match self.open.iter().find(|(p, _)| same_path(p, path)) {
            Some((_, text)) => {
                self.followed.push(path.to_path_buf());
                (text.to_string(), true)
            }
            None => {
                self.files.push((path.to_path_buf(), modified(path)));
                match read_file(path) {
                    Ok(t) => (t, false),
                    Err(e) => {
                        // At the path in the spec, where it's written there.
                        let at = self.spec.and_then(|s| quoted_path(s, path, self.dir));
                        let e = FigError {
                            at,
                            ..e.note(format!("looked for {}", relative(path, self.dir)))
                        };
                        return Err(e);
                    }
                }
            }
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let name = name.as_deref().unwrap_or("");
        let format = spec
            .format
            .or_else(|| format_of_name(name))
            .unwrap_or_else(|| sniff(&text));
        let table = read_data(&text, spec, name, self.opt);
        let mut read = ReadFile {
            table: i,
            path: path.to_path_buf(),
            shown: relative(path, self.dir),
            text,
            format,
            from_buffer,
            rows: 0,
            columns: 0,
        };
        match table {
            Ok(t) => {
                (read.rows, read.columns) = (t.rows, t.columns.len());
                self.read.push(read);
                Ok(Arc::new(t))
            }
            Err(e) => {
                let said = format!("read {} as {}", read.shown, format_label(format));
                let e = e.in_file(path, Some(&read.text)).note(said);
                Err(e)
            }
        }
    }
}

/// A path as the log writes it: from `dir` (with up to two `..`), else from your home as `~`,
/// else whole.
pub fn relative(path: &Path, dir: &Path) -> String {
    let (p, d) = (lexical(path), lexical(dir));
    let mut base = d.clone();
    for up in 0..=2 {
        if let Ok(rest) = p.strip_prefix(&base) {
            let mut out = PathBuf::new();
            for _ in 0..up {
                out.push("..");
            }
            out.push(rest);
            return out.display().to_string();
        }
        if !base.pop() || base.parent().is_none() {
            break;
        }
    }
    match std::env::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// A data format as the log names it.
fn format_label(f: DataFormat) -> &'static str {
    match f {
        DataFormat::Csv => "CSV",
        DataFormat::Tsv => "TSV",
        DataFormat::Json => "JSON",
        DataFormat::Jsonl => "JSON lines",
    }
}

/// Where a data path is written in a spec's text, quotes and all: as given relative to the
/// spec's directory, else by its file's name. None if it isn't there as either.
fn quoted_path(text: &str, path: &Path, dir: &Path) -> Option<Pos> {
    let rel = path.strip_prefix(dir).ok().map(|p| p.display().to_string());
    let whole = path.display().to_string();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    for c in [rel, Some(whole), name].into_iter().flatten() {
        let quoted = format!("\"{}\"", c.replace('\\', "\\\\"));
        if let Some(i) = text.find(&quoted) {
            return Some(Pos::of(text, i));
        }
        // A path written with more before its name: `"data/x.csv"`, at its opening quote.
        if let Some(i) = text.find(&format!("{}\"", c.replace('\\', "\\\\")))
            && let Some(open) = text[..i].rfind('"')
            && !text[open + 1..i].contains(['"', '\n'])
        {
            return Some(Pos::of(text, open));
        }
    }
    None
}

/// A file's text, or why not, naming it.
pub fn read_file(path: &Path) -> Result<String> {
    let name = shown(path);
    let unreadable = |why: String| FigError::new(why).with_kind(ErrorKind::Unreadable);
    if let Ok(m) = std::fs::metadata(path)
        && m.len() > TOO_LARGE
    {
        let mb = m.len() / (1024 * 1024);
        return Err(unreadable(format!(
            "{name} is too large to chart ({mb} MB)"
        )));
    }
    std::fs::read_to_string(path).map_err(|e| {
        unreadable(match e.kind() {
            std::io::ErrorKind::NotFound => format!("{name}: no such file"),
            std::io::ErrorKind::PermissionDenied => format!("{name}: permission denied"),
            std::io::ErrorKind::InvalidData => format!("{name} isn't text (UTF-8)"),
            std::io::ErrorKind::IsADirectory => format!("{name} is a directory"),
            _ => format!("{name}: {e}"),
        })
    })
}

/// The largest file a figure reads: past this it says so rather than taking the memory.
pub const TOO_LARGE: u64 = 1024 * 1024 * 1024;

/// A path as an error shows it: its file's name, or the whole path if it has none.
fn shown(path: &Path) -> String {
    match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// When a file last changed, if it's there.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A data text read as its spec says: the format given, else the file name's, else CSV or TSV
/// by its first line; the spec's reading hints over `opt`.
pub fn read_data(text: &str, spec: &DataSpec, name: &str, opt: &ReadOptions) -> Result<Table> {
    let mut opt = with_hints(opt, &spec.read);
    let format = spec
        .format
        .or_else(|| format_of_name(name))
        .unwrap_or_else(|| sniff(text));
    match format {
        DataFormat::Csv => crate::data::csv::read_csv(text, &opt),
        DataFormat::Tsv => {
            opt.delimiter.get_or_insert(b'\t');
            crate::data::csv::read_csv(text, &opt)
        }
        DataFormat::Json => crate::data::rows::read_json(&crate::json::parse(text)?, &opt),
        DataFormat::Jsonl => crate::data::rows::read_jsonl(text, &opt),
    }
}

/// JSON by its first character, else TSV when the first line has more tabs than commas, else
/// CSV.
fn sniff(text: &str) -> DataFormat {
    let body = text.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
    if body.starts_with(['{', '[']) {
        return if body.starts_with('{')
            && body
                .lines()
                .nth(1)
                .is_some_and(|l| l.trim_start().starts_with('{'))
        {
            DataFormat::Jsonl
        } else {
            DataFormat::Json
        };
    }
    let line = body.lines().next().unwrap_or("");
    let count = |c: char| line.chars().filter(|&x| x == c).count();
    if count('\t') > count(',') {
        DataFormat::Tsv
    } else {
        DataFormat::Csv
    }
}

/// `p` with `.` and `..` taken out by name, as core gives a file's path.
pub fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Whether two paths name one file: the same by name once `.` and `..` are out, or the same
/// file on disk.
pub fn same_path(a: &Path, b: &Path) -> bool {
    if lexical(a) == lexical(b) {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The lines a selection covers, whole: from the start of the anchor's line to the end of the
/// head's (or the other way round). A selection's partial first and last lines are its rows
/// all the same: a block of numbers out of a log is lines.
pub fn selected_lines(text: &str, a: usize, b: usize) -> String {
    let (lo, hi) = (a.min(b).min(text.len()), a.max(b).min(text.len()));
    let start = text[..lo].rfind('\n').map_or(0, |i| i + 1);
    let end = text[hi..].find('\n').map_or(text.len(), |i| hi + i);
    let mut s = text[start..end].to_string();
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_charts_the_whole_lines_it_covers() {
        let t = "log\nx,y\n1,2\n3,4\nend\n";
        let a = t.find("x,y").unwrap() + 1;
        let b = t.find("3,4").unwrap() + 1;
        assert_eq!(selected_lines(t, b, a), "x,y\n1,2\n3,4\n");
        assert_eq!(selected_lines(t, 0, 0), "log\n");
        assert_eq!(selected_lines(t, 999, 999), "\n");
    }

    #[test]
    fn paths_compare_by_name_or_on_disk() {
        assert!(same_path(Path::new("/a/b/../c"), Path::new("/a/./c")));
        assert!(!same_path(Path::new("/a/b"), Path::new("/a/c")));
        assert_eq!(lexical(Path::new("/x/y/../z")), PathBuf::from("/x/z"));
    }

    #[test]
    fn data_is_read_as_its_name_or_its_text_says() {
        let opt = ReadOptions::new(TimeZone::UTC);
        let spec = DataSpec {
            source: DataRef::Source,
            format: None,
            read: Default::default(),
        };
        let t = read_data("a\tb\n1\t2\n", &spec, "x.tsv", &opt).unwrap();
        assert_eq!(t.columns.len(), 2);
        let t = read_data("[{\"a\":1},{\"a\":2}]", &spec, "x", &opt).unwrap();
        assert_eq!(t.rows, 2);
        let t = read_data("{\"a\":1}\n{\"a\":2}\n", &spec, "x", &opt).unwrap();
        assert_eq!(t.rows, 2);
        assert_eq!(sniff("a,b\n"), DataFormat::Csv);
        assert!(read_file(Path::new("/nonexistent/fig.csv")).is_err());
    }
}
