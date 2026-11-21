//! Where a figure's text and data come from, and how they're followed: the source buffer's
//! edits, a file's changes by poll; and in phase 4 commands run and streamed, each asked for
//! once by name (ruling 13).
//!
//! A figure's own text is a buffer's (followed as it's edited), a file's (`:fig FILE`, polled
//! while a window shows it), a command's output (`:fig !cmd`, run once), or a selection's lines
//! as they were. Reading it, its data files and building the chart's model all happen in one
//! job, [`build`], off the main thread: the job is given the text of every buffer that visits a
//! file, so a data file open in a buffer is read from the buffer and follows its edits, and the
//! rest from disk, polled. Phase 2 charts; a spec whose data is a command's (`run`, `stream`)
//! says those arrive in a later build, as do graphs, tiles and dashboards.

use crate::data::{ReadOptions, Table};
use crate::error::{FigError, Result};
use crate::interact::Toggles;
use crate::plot::ChartModel;
use crate::spec::chart::with_hints;
use crate::spec::{Chart, DataFormat, DataRef, DataSpec, Figure, Parsed, format_of_name};
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
    /// The chart as parsed, before any toggles; None for a figure that isn't one.
    pub chart: Option<Chart>,
    /// The chart's tables: its data's, then each layer's own.
    pub tables: Vec<Arc<Table>>,
    /// The chart's model, if it has one: by value, since a model need only be `Send`.
    pub model: Option<ChartModel>,
    /// What the spec had that was odd but not wrong.
    pub warnings: Vec<FigError>,
    /// The files read from disk, and when each last changed: polled while shown. A figure
    /// whose own text is a file's has it first.
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The data files read from buffers: followed as they're edited.
    pub followed: Vec<PathBuf>,
    /// Why there's no model though the figure parsed: a later build's figure or source.
    pub later: Option<String>,
}

/// Why a build failed: the error, and the file it's in if it isn't the figure's own text (a
/// data file the spec names), so `<CR>` on the banner goes to the right place.
#[derive(Clone, Debug, PartialEq)]
pub struct Failed {
    /// What went wrong, and where.
    pub error: FigError,
    /// The data file it's in; None for the figure's own text.
    pub file: Option<PathBuf>,
    /// The files read from disk before it failed: polled, so fixing one brings the figure back.
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Failed {
    fn own(error: FigError, files: Vec<(PathBuf, Option<SystemTime>)>) -> Failed {
        Failed {
            error,
            file: None,
            files,
        }
    }
}

/// Parse a figure's text, read its data and build its chart's model: off the main thread.
/// An error is the parse's (with its place), a data file's (naming it), or the model's.
pub fn build(job: Job) -> std::result::Result<Built, Failed> {
    let mut files = vec![];
    let text = match &job.text {
        Text::Of(r) => r.to_string(),
        Text::File(p) => {
            files.push((p.clone(), modified(p)));
            read_file(p).map_err(|e| Failed::own(e, files.clone()))?
        }
        Text::Given(s) => s.to_string(),
    };
    let opt = ReadOptions::new(job.zone.clone());
    let parsed = crate::spec::parse(&text, &job.name, &job.dir, &opt);
    let Parsed {
        figure,
        inferred,
        warnings,
    } = parsed.map_err(|e| Failed::own(e, files.clone()))?;
    let mut built = Built {
        figure: figure.clone(),
        inferred,
        chart: None,
        tables: vec![],
        model: None,
        warnings,
        files,
        followed: vec![],
        later: None,
    };
    let chart = match figure {
        Figure::Chart(c) => c,
        other => {
            built.later = Some(format!("{} arrive in a later build", kind_plural(&other)));
            return Ok(built);
        }
    };
    let mut reader = Reader {
        open: &job.open,
        opt: &opt,
        files: std::mem::take(&mut built.files),
        followed: vec![],
    };
    let tables = match reader.tables(&chart) {
        Ok(Ok(t)) => t,
        Ok(Err((error, file))) => {
            return Err(Failed {
                error,
                file,
                files: reader.files,
            });
        }
        Err(Later) => {
            built.files = reader.files;
            built.chart = Some(chart);
            built.later = Some(LATER_RUN.into());
            return Ok(built);
        }
    };
    built.files = reader.files;
    built.followed = reader.followed;
    let model = ChartModel::build(job.toggles.apply(&chart), tables.clone(), &job.zone)
        .map_err(|e| Failed::own(e, built.files.clone()))?;
    built.chart = Some(chart);
    built.tables = tables;
    built.model = Some(model);
    Ok(built)
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

/// Reads a chart's tables, from the buffers given where one visits the file and otherwise
/// from disk, noting which was which.
struct Reader<'a> {
    open: &'a [(PathBuf, Snapshot)],
    opt: &'a ReadOptions,
    files: Vec<(PathBuf, Option<SystemTime>)>,
    followed: Vec<PathBuf>,
}

/// Why a table couldn't be read, and in which data file.
type Unread = (FigError, Option<PathBuf>);

/// A table, or why not and in which data file.
type Read = std::result::Result<Arc<Table>, Unread>;

impl Reader<'_> {
    /// The chart's data's table, then each layer's own, in order.
    fn tables(
        &mut self,
        chart: &Chart,
    ) -> std::result::Result<std::result::Result<Vec<Arc<Table>>, Unread>, Later> {
        let mut out = vec![];
        let own = chart.layers.iter().filter_map(|l| l.data.as_ref());
        for spec in std::iter::once(&chart.data).chain(own) {
            match self.table(spec)? {
                Ok(t) => out.push(t),
                Err(e) => return Ok(Err(e)),
            }
        }
        Ok(Ok(out))
    }

    fn table(&mut self, spec: &DataSpec) -> std::result::Result<Read, Later> {
        Ok(match &spec.source {
            DataRef::Inline(t) => Ok(t.clone()),
            DataRef::File(path) => self
                .file(path, spec)
                .map_err(|e| (e, Some(path.to_path_buf()))),
            DataRef::Run { .. } | DataRef::Stream { .. } => return Err(Later),
            DataRef::Named(name) => Err((
                FigError::new(format!(
                    "\"{name}\" names a dashboard's data, outside a dashboard"
                )),
                None,
            )),
            DataRef::Source => Err((
                FigError::new("\"data\": \"-\" is the buffer itself, which is the spec here"),
                None,
            )),
        })
    }

    fn file(&mut self, path: &Path, spec: &DataSpec) -> Result<Arc<Table>> {
        let text = match self.open.iter().find(|(p, _)| same_path(p, path)) {
            Some((_, text)) => {
                self.followed.push(path.to_path_buf());
                text.to_string()
            }
            None => {
                self.files.push((path.to_path_buf(), modified(path)));
                read_file(path)?
            }
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        read_data(&text, spec, name.as_deref().unwrap_or(""), self.opt).map(Arc::new)
    }
}

/// A file's text, or why not, naming it.
pub fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| FigError::new(format!("{}: {e}", shown(path))))
}

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
