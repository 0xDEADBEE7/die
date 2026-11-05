//! A memory limit inside every test binary.
//!
//! macOS doesn't enforce process limits: `ulimit -v` and `ulimit -d` fail there. Tests
//! that grew without end have reached 128 to 301 GB before the kernel killed them, and once
//! swap ran out and you had to force-quit eight apps. So each test binary carries its own
//! limit, as its global allocator. That also holds when a binary is run directly or from a
//! copy of the tree, where a cargo runner or an outside watcher wouldn't be.
//!
//! Every test binary in the workspace links this crate. A `tests/*.rs` file says
//! `use die_test_cap as _;` and a crate root says `#[cfg(test)] use die_test_cap as _;`.
//! `cargo xtask lint` fails a test binary that doesn't, and a crate that lists this one
//! under anything but `[dev-dependencies]`. It's never linked into DIE, which is why it
//! may hold the workspace's one `unsafe` outside DIE's own code (scope §12).
//!
//! Past the limit, an allocation writes one line to stderr, and fails. Rust's usual
//! handling of a failed allocation then aborts the process. The line starts with
//! `die-test-cap:`, and `cargo xtask mutate` looks for it.
//!
//! The limit is [`DEFAULT_LIMIT`]. A test that truly needs more calls [`set_limit`].
//! There's no environment variable: reading one can allocate, and a limit anyone can
//! switch off isn't one.
//!
//! # What it doesn't cover
//!
//! - Doctests. Each is its own small binary, and none of them names this crate.
//! - Memory that doesn't come through Rust's allocator: C libraries and system
//!   frameworks (Metal, Core Text), and direct `mmap` calls.
//! - Other processes: whatever a test spawns, such as cargo or Emacs, has its own memory.
//! - Binaries that aren't tests, such as DIE and `xtask`, and `spikes/`, which is outside
//!   the workspace.
//! - Time. A test that spins without allocating runs on forever.
//! - Blocks malloc keeps after they're freed, in a binary run directly on macOS. The count
//!   goes down at once, but malloc there holds on to as many as 64 big freed blocks, for a
//!   request that fits. If none does, they just stay: a loop that grew a block to 256 MiB
//!   and freed it held 16 GiB, with 256 MiB counted. That's bounded by the 64 blocks, not
//!   by the limit. `.cargo/config.toml` turns this cache off for all that cargo runs.
//! - A moment's extra. A `realloc` that moves a block holds both copies until it's done,
//!   and only the new one counts, so real use can pass the limit briefly.

use std::io::Write;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

#[allow(unsafe_code)]
mod cap;

/// The limit a test binary starts with, in bytes. It must stay at least 3 times the
/// largest peak any real test needs, which is 160 MiB (`die-vim`'s `sessions`, measured
/// with `/usr/bin/time -l`).
pub const DEFAULT_LIMIT: usize = 4 << 30;

static LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_LIMIT);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static LINE: AtomicU8 = AtomicU8::new(UNWRITTEN);

const UNWRITTEN: u8 = 0;
const WRITING: u8 = 1;
const WRITTEN: u8 = 2;

/// Set this test binary's limit, in bytes, for a test that truly needs more than
/// [`DEFAULT_LIMIT`]. What's already allocated still counts. Say why where you call it.
pub fn set_limit(bytes: usize) {
    LIMIT.store(bytes, Relaxed);
}

/// What a block of `size` bytes counts for. Small blocks cost the system more than they
/// hold: `malloc(1)` spends 16 bytes on macOS and 32 on Linux. Counted bare, a leak of
/// one-byte boxes would use 16 or 32 times the limit, so each block also counts for a
/// header and its rounding, in 16-byte steps.
fn charge(size: usize) -> usize {
    size.saturating_add(31) & !15
}

/// Count `bytes` more if they fit under the limit. If not, say so, and count nothing.
///
/// A single add is far cheaper than a compare-and-swap loop when threads allocate at
/// once, and the tests that run at full tilt feel it. The price is that a block about to
/// be refused counts for a moment, so another thread's block can be refused with it.
fn reserve(bytes: usize) -> bool {
    let limit = LIMIT.load(Relaxed);
    let live = LIVE.fetch_add(bytes, Relaxed);
    if live.saturating_add(bytes) <= limit {
        return true;
    }
    release(bytes);
    write_line_once(limit);
    false
}

fn release(bytes: usize) {
    LIVE.fetch_sub(bytes, Relaxed);
}

/// Write the line, once. This runs inside the allocator, when there may be no room for
/// anything, so it may not allocate: the text is built on the stack, and `cap::to_stderr`
/// writes it straight to the file descriptor. That's also past the test harness's output
/// capture, which would hold the line back from a process about to die.
///
/// A thread that loses the race waits for the winner to finish, or its abort could end
/// the process before the line is out. The wait is bounded, since a full pipe can hold
/// the winner up for good.
fn write_line_once(limit: usize) {
    if LINE
        .compare_exchange(UNWRITTEN, WRITING, Relaxed, Relaxed)
        .is_err()
    {
        for _ in 0..1000 {
            if LINE.load(Relaxed) == WRITTEN {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        return;
    }
    let mut buf = [0u8; 128];
    let mut rest = &mut buf[..];
    let _ = writeln!(
        rest,
        "die-test-cap: over the memory limit of {limit} bytes ({} MiB)",
        limit >> 20
    );
    let unused = rest.len();
    cap::to_stderr(&buf[..buf.len() - unused]);
    LINE.store(WRITTEN, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::{LINE, WRITING, WRITTEN, charge, write_line_once};
    use std::sync::atomic::Ordering::Relaxed;
    use std::time::{Duration, Instant};

    /// Two threads reaching the limit together rarely show this, so it's staged here. No
    /// other test in this binary goes near the line.
    #[test]
    fn a_thread_that_loses_the_race_waits_for_the_line_but_not_for_ever() {
        LINE.store(WRITING, Relaxed);
        let loser = std::thread::spawn(|| {
            write_line_once(0);
            Instant::now()
        });
        std::thread::sleep(Duration::from_millis(50));
        let written = Instant::now();
        LINE.store(WRITTEN, Relaxed);
        assert!(loser.join().unwrap() >= written);

        // Here the winner never finishes, so this returning is the test.
        LINE.store(WRITING, Relaxed);
        write_line_once(0);
    }

    #[test]
    fn a_block_counts_for_what_it_costs() {
        for (size, counted) in [
            (0, 16),
            (1, 32),
            (16, 32),
            (17, 48),
            (4096, 4112),
            (usize::MAX, usize::MAX & !15),
        ] {
            assert_eq!(charge(size), counted, "{size}");
        }
    }
}
