//! The cap at work: each case runs as a child of this binary, under a low limit, and must
//! die there, quickly and saying why, with its memory near the limit. Others stay under
//! the limit by freeing as they go, and must not die, which shows the count goes down.
//!
//! No case can hurt the machine if the cap is broken: each stops itself at `MOST`.

use die_test_cap as _;

mod support;

use die_test_cap::set_limit;
use std::hint::black_box;
use std::process::exit;
use std::sync::{Arc, Barrier};
use std::thread;
use support::{NOT_STOPPED, Run, in_child, sample_memory};

const LIMIT: usize = 64 << 20;
const LINE: &str = "die-test-cap: over the memory limit of 67108864 bytes (64 MiB)";

/// How much a growing case adds between samples of its memory.
const STEP: usize = LIMIT / 32;

/// As far as a growing case will go without the cap stopping it, in bytes it asks for.
const MOST: usize = 8 * LIMIT;

/// Run `body` under `LIMIT`, in a child.
fn capped(name: &str, body: impl FnOnce()) -> Run {
    in_child(name, || {
        set_limit(LIMIT);
        body();
    })
}

/// The child died at the limit, with at least `least` KiB in use and not much more than the
/// limit. It's the upper bound that keeps the machine safe; the lower one shows the
/// samples saw the case grow.
#[track_caller]
fn died_near_the_limit(run: &Run, least: usize) {
    run.died_with(LINE);
    let (peak, most) = (run.peak_kib(), LIMIT / 1024 * 3 / 2);
    assert!(
        peak >= least && peak <= most,
        "{peak} KiB isn't within {least}..={most}: {run:#?}"
    );
}

#[test]
fn endless_vec_growth() {
    let run = capped("endless_vec_growth", || {
        let mut v: Vec<u8> = Vec::new();
        while v.len() < MOST {
            v.resize(v.len() + STEP, 1);
            sample_memory();
        }
        exit(NOT_STOPPED);
    });
    // A Vec doubles, so its last growth is refused when it's half full.
    died_near_the_limit(&run, LIMIT / 1024 / 4);
}

#[test]
fn repeated_realloc() {
    let run = capped("repeated_realloc", || {
        let mut v: Vec<u8> = Vec::new();
        while v.len() < MOST {
            v.reserve_exact(STEP);
            v.resize(v.len() + STEP, 1);
            sample_memory();
        }
        exit(NOT_STOPPED);
    });
    died_near_the_limit(&run, LIMIT / 1024 / 2);
}

#[test]
fn one_huge_alloc_zeroed() {
    // Its pages cost nothing until they're touched, and it touches none, so this costs
    // nothing if the cap is broken.
    let run = capped("one_huge_alloc_zeroed", || {
        sample_memory();
        black_box(vec![0u8; 1 << 40]);
        exit(NOT_STOPPED);
    });
    died_near_the_limit(&run, 0);
}

#[test]
fn many_small_boxes_leaked() {
    // One byte each: if the count were of bytes asked for, the system would spend 16 or 32
    // times the limit before the cap saw it.
    let run = capped("many_small_boxes_leaked", || {
        for _ in 0..MOST / 16 / 65536 {
            for _ in 0..65536 {
                black_box(Box::leak(Box::new(1u8)));
            }
            sample_memory();
        }
        exit(NOT_STOPPED);
    });
    died_near_the_limit(&run, LIMIT / 1024 / 4);
}

#[test]
fn the_line_needs_no_memory() {
    // Not even the first use of stderr's lock, which allocates on macOS, finds room here.
    in_child("the_line_needs_no_memory", || {
        set_limit(0);
        black_box(Box::new(1u8));
        exit(NOT_STOPPED);
    })
    .died_with("die-test-cap: over the memory limit of 0 bytes (0 MiB)");
}

#[test]
fn growth_from_several_threads() {
    const THREADS: usize = 4;
    let run = capped("growth_from_several_threads", || {
        // Between its two waits, each round, the main thread samples what the others hold.
        let barrier = Arc::new(Barrier::new(THREADS + 1));
        for _ in 0..THREADS {
            let barrier = barrier.clone();
            thread::spawn(move || {
                loop {
                    vec![1u8; STEP].leak();
                    barrier.wait();
                    barrier.wait();
                }
            });
        }
        for _ in 0..MOST / STEP / THREADS {
            barrier.wait();
            sample_memory();
            barrier.wait();
        }
        exit(NOT_STOPPED);
    });
    died_near_the_limit(&run, LIMIT / 1024 / 4);
}

#[test]
fn threads_that_reach_the_limit_together_write_one_line() {
    for _ in 0..8 {
        let run = capped(
            "threads_that_reach_the_limit_together_write_one_line",
            || {
                let threads: Vec<_> = (0..8)
                    .map(|_| {
                        thread::spawn(|| {
                            for _ in 0..MOST / 8 / 4096 {
                                vec![1u8; 4096].leak();
                            }
                        })
                    })
                    .collect();
                for t in threads {
                    let _ = t.join();
                }
                exit(NOT_STOPPED);
            },
        );
        run.died_with(LINE);
    }
}

#[test]
fn a_case_that_frees_as_it_goes_lives() {
    in_child("a_case_that_frees_as_it_goes_lives", || {
        // Far more than the limit in all, through each way to get and give back memory.
        set_limit(LIMIT);
        for _ in 0..32 {
            black_box(vec![1u8; LIMIT / 2]);
            black_box(vec![0u8; LIMIT / 2]);
            let mut v = vec![1u8; LIMIT / 4];
            v.reserve_exact(LIMIT / 4);
            v.resize(LIMIT / 2, 1);
            black_box(v);
        }
        // A count that drifts by a byte a block would get there too, given blocks enough.
        set_limit(1 << 20);
        for _ in 0..4 << 20 {
            black_box(Box::new(1u8));
        }
    })
    .lived();
}

#[test]
fn a_realloc_that_shrinks_gives_bytes_back() {
    capped("a_realloc_that_shrinks_gives_bytes_back", || {
        let mut kept = Vec::new();
        for _ in 0..8 {
            let mut v = vec![1u8; LIMIT / 4 * 3];
            v.truncate(16);
            v.shrink_to_fit();
            kept.push(v);
        }
        black_box(kept);
    })
    .lived();
}

#[test]
fn a_realloc_that_grows_counts_only_the_growth() {
    // Counting the block's whole new size on top of its old one would die halfway here.
    capped("a_realloc_that_grows_counts_only_the_growth", || {
        let mut v: Vec<u8> = Vec::new();
        while v.len() < LIMIT / 8 * 7 {
            v.reserve_exact(STEP);
            v.resize(v.len() + STEP, 1);
        }
        black_box(v);
    })
    .lived();
}

#[test]
fn big_blocks_that_grow_and_go_dont_pile_up() {
    // Left alone, malloc on macOS keeps each of these when it's freed, for a request of that
    // size, which never comes: about 1 GiB by the end, with half the limit counted.
    // `.cargo/config.toml` turns that off for what cargo runs.
    let run = capped("big_blocks_that_grow_and_go_dont_pile_up", || {
        for _ in 0..32 {
            let mut v = vec![1u8; LIMIT / 4];
            v.reserve_exact(LIMIT / 4);
            v.resize(LIMIT / 2, 1);
            black_box(v);
            sample_memory();
        }
    });
    run.lived();
    let (peak, most) = (run.peak_kib(), LIMIT / 1024 * 3 / 2);
    assert!(
        peak <= most,
        "{peak} KiB is over {most}, so malloc kept what was freed. Did this run through \
         cargo, with MallocLargeCache from .cargo/config.toml? {run:#?}"
    );
}

#[test]
fn a_refused_allocation_can_be_handled_and_says_so_once() {
    capped(
        "a_refused_allocation_can_be_handled_and_says_so_once",
        || {
            let mut v: Vec<u8> = Vec::new();
            assert!(v.try_reserve_exact(LIMIT).is_err());
            assert!(v.try_reserve_exact(LIMIT).is_err());
            // The refused blocks were never counted, so a block that fits is still fine.
            v.resize(LIMIT / 2, 1);
            black_box(v);
        },
    )
    .lived_with(LINE);
}

#[test]
fn set_limit_raises_the_limit() {
    capped("set_limit_raises_the_limit", || {
        set_limit(2 * LIMIT);
        black_box(vec![1u8; LIMIT / 2 * 3]);
    })
    .lived();
}
