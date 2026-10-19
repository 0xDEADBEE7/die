//! The one small interface DIE's main thread uses for a terminal, whatever the shape.
//! Nothing here blocks: `send` queues, `drain` reads what is ready.
use crate::proto::{ToDie, ToEngine};
use std::os::fd::RawFd;

pub trait TermClient {
    /// Readable when messages may be waiting (wake pipe or socket).
    fn fd(&self) -> RawFd;
    /// True when queued output is waiting for the fd to become writable.
    fn wants_write(&self) -> bool {
        false
    }
    fn on_writable(&mut self) {}
    /// Queue a job for the engine side. Never blocks.
    fn send(&mut self, m: ToEngine);
    /// Move every message that is ready into `out`. Err means the terminal is lost.
    fn drain(&mut self, out: &mut Vec<ToDie>) -> Result<(), String>;
    /// The process that runs the engine (DIE itself for the in-process shape).
    fn engine_pid(&self) -> i32;
    /// The program on the PTY.
    fn child_pid(&self) -> i32;
    /// Bytes received over the transport so far (0 in-process).
    fn bytes_in(&self) -> u64 {
        0
    }
    fn shape(&self) -> &'static str;
}
