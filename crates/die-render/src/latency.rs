//! The latency and startup log (R6). Set `DIE_LATENCY_LOG` to a file, and DIE appends a line
//! for each startup stage and each key that drew a frame, written as it happens:
//!
//! ```text
//! startup stage=first_frame ms=98.40
//! key to_frame_ms=0.52 to_shown_ms=11.90 cpu_ms=0.31 gpu_ms=0.20
//! ```
//!
//! `ms` is since `main`, so it leaves out what the OS does before it. `to_frame_ms` is from the
//! key event's own timestamp to the frame committed, DIE's share. `to_shown_ms` is to the frame
//! on the display, or `none` if it never got there. That comes in after the key, so a key's
//! line is written when the next key comes, or when DIE exits, and so is `first_shown`,
//! whose `ms` is still when the frame got there.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{LineWriter, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The environment variable that names the log file.
pub const ENV: &str = "DIE_LATENCY_LOG";
/// How many keys may wait to hear from the display. Past it, the oldest gets `none`.
const WAITING: usize = 256;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Start the startup clock. `main` calls this first. Otherwise it starts at the first stage.
pub fn start_clock() {
    STARTED.get_or_init(Instant::now);
}

/// What one key took, in ms.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KeySample {
    /// From its timestamp to the frame committed.
    pub to_frame_ms: f64,
    /// From its timestamp to the frame on the display, if it got there.
    pub to_shown_ms: Option<f64>,
    /// The main thread's time on the frame.
    pub cpu_ms: f64,
    /// The GPU's time on the frame.
    pub gpu_ms: f64,
}

/// A key whose frame hasn't reached the display: the frame's id, the sample, and when the
/// key happened.
struct Waiting {
    id: u64,
    sample: KeySample,
    at: Instant,
}

/// Where the lines go, and what a check reads.
#[derive(Default)]
pub struct Latency {
    log: Option<LineWriter<File>>,
    waiting: VecDeque<Waiting>,
    /// Keep the samples, for a check.
    pub keep: bool,
    /// The kept samples.
    pub samples: Vec<KeySample>,
    /// When the first frame reached the display.
    pub first_shown: Option<Instant>,
    /// Whether the display dropped a frame before showing one.
    pub first_dropped: bool,
}

impl Latency {
    /// A log appending to `path`, or why it can't.
    pub fn open(path: &Path) -> Result<Latency, String> {
        let file = OpenOptions::new().create(true).append(true).open(path);
        let file = file.map_err(|e| format!("{ENV}: {}: {e}", path.display()))?;
        Ok(Latency {
            log: Some(LineWriter::new(file)),
            ..Latency::default()
        })
    }

    /// The log the environment asks for, and what went wrong with it, for `*messages*`.
    pub fn from_env() -> (Latency, Option<String>) {
        let Some(path) = std::env::var_os(ENV).filter(|p| !p.is_empty()) else {
            return (Latency::default(), None);
        };
        match Latency::open(Path::new(&path)) {
            Ok(latency) => (latency, None),
            Err(e) => (Latency::default(), Some(e)),
        }
    }

    /// Whether there's a file to write to.
    pub fn logging(&self) -> bool {
        self.log.is_some()
    }

    fn line(&mut self, line: &str) {
        if let Some(log) = &mut self.log
            && writeln!(log, "{line}").is_err()
        {
            self.log = None;
        }
    }

    /// Startup has reached this stage.
    pub fn stage(&mut self, name: &str) {
        self.stage_at(name, Instant::now());
    }

    fn stage_at(&mut self, name: &str, at: Instant) {
        let since = at.saturating_duration_since(*STARTED.get_or_init(Instant::now));
        let ms = since.as_secs_f64() * 1e3;
        self.line(&format!("startup stage={name} ms={ms:.2}"));
    }

    /// A key drew frame `id`, `took` after it happened.
    pub fn key(&mut self, id: u64, took: Duration, cpu_ms: f64, gpu_ms: f64) {
        if self.log.is_none() && !self.keep {
            return;
        }
        let now = Instant::now();
        let at = now.checked_sub(took).unwrap_or(now);
        let sample = KeySample {
            to_frame_ms: took.as_secs_f64() * 1e3,
            to_shown_ms: None,
            cpu_ms,
            gpu_ms,
        };
        self.waiting.push_back(Waiting { id, sample, at });
        if self.waiting.len() > WAITING
            && let Some(oldest) = self.waiting.pop_front()
        {
            self.finish(oldest.sample);
        }
    }

    /// These frames reached the display, or didn't. Any key waiting on one is done.
    pub fn shown(&mut self, frames: Vec<(u64, Option<Instant>)>) {
        for (id, at) in frames {
            if self.first_shown.is_none() {
                self.first_shown = at;
                self.first_dropped |= at.is_none();
                if let Some(at) = at {
                    self.stage_at("first_shown", at);
                }
            }
            let Some(i) = self.waiting.iter().position(|w| w.id == id) else {
                continue;
            };
            if let Some(key) = self.waiting.remove(i) {
                let shown = at.map(|at| at.saturating_duration_since(key.at));
                let to_shown_ms = shown.map(|d| d.as_secs_f64() * 1e3);
                self.finish(KeySample {
                    to_shown_ms,
                    ..key.sample
                });
            }
        }
    }

    /// Whatever is still waiting isn't going to hear.
    pub fn flush(&mut self) {
        while let Some(key) = self.waiting.pop_front() {
            self.finish(key.sample);
        }
    }

    fn finish(&mut self, s: KeySample) {
        if self.keep {
            self.samples.push(s);
        }
        let shown = s
            .to_shown_ms
            .map_or_else(|| "none".to_owned(), |ms| format!("{ms:.2}"));
        self.line(&format!(
            "key to_frame_ms={:.2} to_shown_ms={shown} cpu_ms={:.2} gpu_ms={:.2}",
            s.to_frame_ms, s.cpu_ms, s.gpu_ms
        ));
    }
}
