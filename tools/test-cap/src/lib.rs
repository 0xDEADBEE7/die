//! A memory limit inside every test binary.
//!
//! Tests that grew without end have reached 128 to 301 GB before the kernel killed them,
//! and once swap ran out and you had to force-quit eight apps. So each test binary carries
//! its own limits. That also holds when a binary is run directly or from a copy of the
//! tree, where a cargo runner or an outside watcher wouldn't be.
//!
//! Every test binary in the workspace links this crate. A `tests/*.rs` file says
//! `use die_test_cap as _;` and a crate root says `#[cfg(test)] use die_test_cap as _;`.
//! `cargo xtask lint` fails a test binary that doesn't, and a crate that lists this one
//! under anything but `[dev-dependencies]`. It's never linked into DIE, which is why it
//! may hold the workspace's one `unsafe` outside DIE's own code (scope §12).
//!
//! # The two limits
//!
//! The count comes first. This crate is the binary's global allocator, and counts what's
//! live. Past [`DEFAULT_LIMIT`], an allocation writes one line to stderr, and fails. Rust's
//! usual handling of a failed allocation then aborts the process. The line starts with
//! `die-test-cap:`, and `cargo xtask mutate` looks for it. A test that truly needs more
//! calls [`set_limit`]. No environment variable can raise this limit: a limit anyone can
//! switch off isn't one.
//!
//! Behind it is the kernel's. At its first allocation, a test binary runs itself again
//! under `taskpolicy -m`, keeping its pid and arguments, and macOS then kills it with
//! SIGKILL at [`SYSTEM_LIMIT_MIB`], with no line. That holds for all of the process's
//! memory, including what the count can't see. The big blocks malloc keeps after they're
//! freed are the main case, since it keeps as many as 64 of them. A loop that grew a block
//! to 32 MiB and freed it held 2 GiB without this limit, so one that reached 512 MiB could
//! hold 32 GiB. Under this limit it held 8 GiB, and lived. Linux has no `taskpolicy`, so
//! there the binary just carries on, with the count alone. macOS drops `DYLD_*` variables
//! when it runs a system program, so a test binary doesn't see those.
//!
//! # What they don't cover
//!
//! - Doctests. Each is its own small binary, and none of them names this crate.
//! - Other processes: whatever a test spawns, such as cargo or Emacs, has its own memory.
//! - Binaries that aren't tests, such as DIE and `xtask`, and `spikes/`, which is outside
//!   the workspace.
//! - Time. A test that spins without allocating runs on forever.
//! - On Linux, memory that doesn't come through Rust's allocator: C libraries, thread
//!   stacks and direct `mmap` calls.
//! - A little over the count. Threads count apart, so it can be passed by up to 4 MiB. And
//!   a `realloc` that moves a block holds both copies until it's done, and only the new one
//!   counts.
//! - A test binary that runs itself with a plain `exec`. That drops the kernel's limit, and
//!   the pid and the mark stay, so it doesn't go under it again. Setting the mark by hand
//!   does the same. The count still holds in both.

use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Write, stderr};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{self, Command};
use std::sync::atomic::{
    AtomicBool, AtomicU8, AtomicUsize, Ordering::Relaxed, Ordering::SeqCst, fence,
};
use std::time::{Duration, Instant};

#[allow(unsafe_code)]
mod cap;

/// The limit a test binary starts with, in bytes. It must stay at least 3 times the
/// largest peak any real test needs, which is 160 MiB (`die-vim`'s `sessions`, measured
/// with `/usr/bin/time -l`).
pub const DEFAULT_LIMIT: usize = 4 << 30;

/// The kernel's limit on a test binary, in MiB. [`set_limit`] doesn't change it. It's twice
/// [`DEFAULT_LIMIT`], so the line comes first even while a `realloc` that moves a block
/// holds both copies.
pub const SYSTEM_LIMIT_MIB: usize = 8192;
const _: () = assert!(SYSTEM_LIMIT_MIB << 20 >= 2 * DEFAULT_LIMIT);

/// The program that sets the kernel's limit, and then runs what it's given in its own
/// process, so the pid stays the same. Children don't inherit the limit, so each test
/// binary has to set its own.
const TASKPOLICY: &str = "/usr/sbin/taskpolicy";

/// Set to the pid of a process that's under the kernel's limit. A child inherits this and
/// has another pid, so it goes under the limit itself.
const UNDER_LIMIT: &str = "DIE_TEST_CAP_UNDER_LIMIT";

/// Lowers the kernel's limit, in MiB, and can't raise it. This crate's tests need it, since
/// testing the real limit would take 8 GiB of memory each time.
const LOWER_LIMIT: &str = "DIE_TEST_CAP_SYSTEM_LIMIT_MIB";

/// How many counters there are, and how far one moves before the total is added up. The
/// limit can be passed by at most these two multiplied, which is 4 MiB.
const COUNTERS: usize = 64;
const STEP: usize = 1 << 16;

/// A cache line to itself, at its largest, which is Apple's. Sharing one would bring back
/// the cost of a single counter.
#[repr(align(128))]
struct Counter(AtomicUsize);

static LIVE: [Counter; COUNTERS] = [const { Counter(AtomicUsize::new(0)) }; COUNTERS];
static LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_LIMIT);
static LINE: AtomicU8 = AtomicU8::new(UNWRITTEN);
static STARTED: AtomicBool = AtomicBool::new(false);

const UNWRITTEN: u8 = 0;
const WRITING: u8 = 1;
const WRITTEN: u8 = 2;

/// Set this test binary's limit, in bytes, for a test that truly needs more than
/// [`DEFAULT_LIMIT`]. What's already allocated still counts. The kernel's limit stays where
/// it is, so a test that needs more than [`SYSTEM_LIMIT_MIB`] has to raise that too. Say why
/// where you call it.
pub fn set_limit(bytes: usize) {
    LIMIT.store(bytes, Relaxed);
}

/// What a block of `size` bytes, aligned to `align`, counts for. Small blocks cost the
/// system more than they hold: `malloc(1)` spends 16 bytes on macOS and 32 on Linux. Counted
/// bare, a leak of one-byte boxes would use 16 or 32 times the limit, so each block also
/// counts for a header and its rounding, in 16-byte steps. A block aligned to more than its
/// size costs about its alignment.
fn charge(size: usize, align: usize) -> usize {
    size.max(align).saturating_add(31) & !15
}

/// Count `bytes` more if they fit under the limit. If not, say so, and count nothing.
///
/// Threads that allocate at once fight over a single counter, which added about half to the
/// time of die-core's 1 ms speed test. So each thread mostly adds to a counter of its own,
/// and adds up the total only now and then, which adds about 4%.
///
/// A single add is far cheaper than a compare-and-swap loop, for the same reason. The price
/// is that a block about to be refused counts for a moment, so another thread's block can
/// be refused with it.
fn reserve(bytes: usize) -> bool {
    if !STARTED.load(Relaxed) {
        // Set first, since all that comes next allocates, which brings it back here.
        STARTED.store(true, Relaxed);
        go_under_the_kernels_limit();
    }
    let limit = LIMIT.load(Relaxed);
    if bytes <= limit {
        let before = my_counter().fetch_add(bytes, Relaxed);
        if stays_in_its_step(limit, before, bytes) || fits(limit, live) {
            return true;
        }
        release(bytes);
    }
    write_line_once(limit);
    false
}

fn release(bytes: usize) {
    my_counter().fetch_sub(bytes, Relaxed);
}

/// A block needs no look at the total if it leaves its counter in the same [`STEP`]. A big
/// block never does. Under a limit too small to allow for every counter's step, every block
/// gets a look.
fn stays_in_its_step(limit: usize, before: usize, bytes: usize) -> bool {
    limit >= COUNTERS * STEP && (before ^ before.wrapping_add(bytes)) < STEP
}

/// A look at the total, which is what lets a block through.
///
/// The fence is for two threads that each count a big block at the same moment, when only
/// one fits. Without it each can miss the other's add, and both get through. It's on this
/// path alone, which only a block that passes a step takes.
///
/// The second read is for a healthy process: a block that moves between threads can be read
/// in its new counter before the free shows in its old one. It costs nothing unless the
/// first read says no, and a process really over the limit stays over it.
#[cold]
#[inline(never)]
fn fits(limit: usize, mut total: impl FnMut() -> isize) -> bool {
    fence(SeqCst);
    within(limit, total()) || within(limit, total())
}

/// This thread's counter. Threads' stacks are at least 2 MiB apart, unless a thread asks
/// for less, so the address of a local tells them apart. Nothing is kept per thread, so a
/// thread that ends loses nothing from the count, and this can't allocate.
fn my_counter() -> &'static AtomicUsize {
    &LIVE[counter_here()].0
}

fn counter_here() -> usize {
    let here = 0u8;
    ((&raw const here).addr() >> 21) % COUNTERS
}

/// What's counted now. A thread may free a block another thread allocated, so one counter
/// can fall below zero, in wrapping arithmetic, and only the total means anything. The
/// counters are read one at a time, so for a moment it can read low, if it sees a free and
/// not its allocation, or high, if it sees an allocation and not its free.
fn live() -> isize {
    LIVE.iter()
        .map(|counter| counter.0.load(Relaxed))
        .fold(0, usize::wrapping_add)
        .cast_signed()
}

fn within(limit: usize, live: isize) -> bool {
    live.max(0).cast_unsigned() <= limit
}

/// Run this binary again from its start under the kernel's limit, unless it's already
/// there. This is its first allocation, so nothing has happened that a restart would lose.
///
/// If the `exec` itself fails, the binary carries on without that limit, and with SIGPIPE
/// at its default, which `Command::exec` has reset. Only a sandbox that denies this program
/// has done that. Once the `exec` has worked, `taskpolicy`'s own failure ends the binary,
/// with its message and exit code. A refusal that says why beats a limit that's quietly off.
#[cold]
#[inline(never)]
fn go_under_the_kernels_limit() {
    // Not just for speed: `Command::exec` resets SIGPIPE before it tries, and a try that
    // failed would leave a test binary to die of a closed pipe. This covers Linux.
    if !Path::new(TASKPOLICY).exists() {
        return;
    }
    let pid = process::id().to_string();
    if env::var_os(UNDER_LIMIT).is_some_and(|under| under == *pid) {
        return;
    }
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let mib = system_limit_mib(env::var_os(LOWER_LIMIT).as_deref());
    let _ = Command::new(TASKPOLICY)
        .arg("-m")
        .arg(mib.to_string())
        .arg(exe)
        .args(env::args_os().skip(1))
        .env(UNDER_LIMIT, pid)
        .exec();
}

/// The kernel's limit to ask for, in MiB, given what [`LOWER_LIMIT`] says. `taskpolicy`
/// refuses a limit of 0, so that doesn't lower it either.
fn system_limit_mib(lower: Option<&OsStr>) -> usize {
    lower
        .and_then(OsStr::to_str)
        .and_then(|mib| mib.parse().ok())
        .filter(|&mib: &usize| mib > 0)
        .map_or(SYSTEM_LIMIT_MIB, |mib| mib.min(SYSTEM_LIMIT_MIB))
}

/// Write the line, once. This runs inside the allocator, when there may be no room for
/// anything, so it may not allocate: the text is built on the stack, and `to_stderr` writes
/// it straight to the file descriptor. That's also past the test harness's output capture,
/// which would hold the line back from a process about to die.
///
/// A thread that loses the race waits for the winner to finish, or its abort could end
/// the process before the line is out. The wait is bounded, since a full pipe can hold
/// the winner up for good.
#[cold]
#[inline(never)]
fn write_line_once(limit: usize) {
    if LINE
        .compare_exchange(UNWRITTEN, WRITING, Relaxed, Relaxed)
        .is_err()
    {
        let start = Instant::now();
        while LINE.load(Relaxed) != WRITTEN && start.elapsed() < Duration::from_secs(1) {
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
    to_stderr(&buf[..buf.len() - unused]);
    LINE.store(WRITTEN, Relaxed);
}

/// Write to a copy of stderr's file descriptor. Writing through `io::stderr()` won't do
/// here: that takes a lock, and on macOS the first use of the lock allocates 64 bytes,
/// which a process at its limit doesn't have. A process with no descriptors left to copy
/// to writes nothing, and still aborts.
fn to_stderr(line: &[u8]) {
    if let Ok(fd) = stderr().as_fd().try_clone_to_owned() {
        let _ = File::from(fd).write_all(line);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COUNTERS, LINE, STEP, SYSTEM_LIMIT_MIB, WRITING, WRITTEN, charge, counter_here, fits,
        stays_in_its_step, system_limit_mib, within, write_line_once,
    };
    use std::ffi::OsStr;
    use std::sync::atomic::Ordering::Relaxed;
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    #[test]
    fn threads_running_together_mostly_have_their_own_counters() {
        const THREADS: usize = 8;
        let barrier = Arc::new(Barrier::new(THREADS));
        let threads: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = barrier.clone();
                // The stack's size is what tells them apart, whatever RUST_MIN_STACK says.
                std::thread::Builder::new()
                    .stack_size(2 << 20)
                    .spawn(move || {
                        // No thread ends and gives its stack to the next until all have looked.
                        let counter = counter_here();
                        barrier.wait();
                        counter
                    })
                    .unwrap()
            })
            .collect();
        let mut counters: Vec<usize> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        counters.sort_unstable();
        counters.dedup();
        assert!(counters.len() >= THREADS / 2, "{counters:?}");
    }

    #[test]
    fn only_a_block_that_passes_a_step_looks_at_the_total() {
        const ENOUGH: usize = COUNTERS * STEP;
        assert!(stays_in_its_step(ENOUGH, 0, 32));
        assert!(stays_in_its_step(ENOUGH, 0, STEP - 1));
        assert!(stays_in_its_step(ENOUGH, STEP, 32));
        assert!(stays_in_its_step(usize::MAX, 0, 32));
        assert!(!stays_in_its_step(ENOUGH, 0, STEP));
        assert!(!stays_in_its_step(ENOUGH, STEP - 32, 32));
        assert!(!stays_in_its_step(ENOUGH, 0, 1 << 40));
        // A counter below zero, climbing back.
        assert!(stays_in_its_step(ENOUGH, 0usize.wrapping_sub(64), 32));
        assert!(!stays_in_its_step(ENOUGH, 0usize.wrapping_sub(32), 32));
        // Under a limit too small to allow for the steps, every block looks.
        assert!(!stays_in_its_step(ENOUGH - 1, 0, 32));
        assert!(!stays_in_its_step(0, 0, 32));
    }

    /// A block in the middle of moving between threads can read high, once.
    #[test]
    fn a_total_over_the_limit_gets_a_second_read() {
        let reads = |values: [isize; 2]| {
            let mut values = values.into_iter();
            fits(1024, || values.next().unwrap())
        };
        assert!(reads([1024, 5000]));
        assert!(reads([5000, 1024]));
        assert!(!reads([5000, 1025]));
    }

    #[test]
    fn a_total_below_zero_is_within_any_limit() {
        assert!(within(1024, 1024));
        assert!(!within(1024, 1025));
        assert!(within(1024, -50));
        assert!(within(0, 0));
        assert!(!within(0, 1));
        assert!(within(0, -1));
        assert!(within(usize::MAX, isize::MAX));
    }

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
        for (size, align, counted) in [
            (1, 1, 32),
            (16, 8, 32),
            (17, 8, 48),
            (4096, 8, 4112),
            (1, 4096, 4112),
            (4096, 4096, 4112),
            (8192, 4096, 8208),
            (usize::MAX, 1, usize::MAX & !15),
        ] {
            assert_eq!(charge(size, align), counted, "{size} aligned to {align}");
        }
    }

    #[test]
    fn the_kernels_limit_can_be_lowered_but_not_raised() {
        let limit = |lower: Option<&str>| system_limit_mib(lower.map(OsStr::new));
        assert_eq!(SYSTEM_LIMIT_MIB, 8192);
        assert_eq!(limit(None), 8192);
        assert_eq!(limit(Some("128")), 128);
        assert_eq!(limit(Some("8192")), 8192);
        assert_eq!(limit(Some("8193")), 8192);
        assert_eq!(limit(Some("not a number")), 8192);
        assert_eq!(limit(Some("0")), 8192);
        assert_eq!(limit(Some("")), 8192);
    }
}
