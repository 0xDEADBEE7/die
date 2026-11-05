//! The cap at work: each case runs as a child of this binary, under a low limit, and must
//! die there, quickly and saying why, with its memory near the limit. Others stay under
//! the limit by freeing as they go, and must not die, which shows the count goes down.
//!
//! No case can hurt the machine if the cap is broken: each stops itself at `MOST`, or at
//! worst holds 2 GiB of freed blocks.

use die_test_cap as _;

mod support;

use die_test_cap::set_limit;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio, exit};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use support::{NOT_STOPPED, Run, in_child, in_child_under, note, sample_memory};

const LIMIT: usize = 64 << 20;
const LINE: &str = "die-test-cap: over the memory limit of 67108864 bytes (64 MiB)";

/// The program behind the kernel's limit. Where it's missing, as on Linux, there's no such
/// limit to test.
const TASKPOLICY: &str = "/usr/sbin/taskpolicy";

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
fn what_a_finished_thread_leaked_still_counts() {
    let run = capped("what_a_finished_thread_leaked_still_counts", || {
        for _ in 0..MOST / STEP {
            let _ = thread::spawn(|| {
                vec![1u8; STEP].leak();
            })
            .join();
            sample_memory();
        }
        exit(NOT_STOPPED);
    });
    died_near_the_limit(&run, LIMIT / 1024 / 4);
}

#[test]
fn what_one_thread_frees_of_anothers_goes_off_the_count() {
    let run = capped(
        "what_one_thread_frees_of_anothers_goes_off_the_count",
        || {
            let (send, receive) = mpsc::sync_channel::<Vec<u8>>(0);
            let freer = thread::spawn(move || receive.iter().for_each(drop));
            // Far more than the limit in all, in big blocks and in small ones, and little of it
            // live at any moment. This must live.
            for _ in 0..8 * LIMIT / STEP {
                send.send(vec![1u8; STEP]).unwrap();
            }
            for _ in 0..4 * LIMIT / 1024 {
                send.send(vec![1u8; 1024]).unwrap();
            }
            drop(send);
            freer.join().unwrap();
            note("all freed");
            // The count is still whole, so this must not.
            let mut kept = Vec::new();
            for _ in 0..MOST / STEP {
                kept.push(vec![1u8; STEP]);
            }
            exit(NOT_STOPPED);
        },
    );
    run.died_with(LINE);
    assert!(run.stdout().contains("all freed\n"), "{run:#?}");
}

#[test]
fn a_block_too_big_to_count_still_says_so() {
    capped("a_block_too_big_to_count_still_says_so", || {
        let mut v: Vec<u8> = Vec::new();
        assert!(v.try_reserve_exact(isize::MAX as usize).is_err());
    })
    .lived_with(LINE);
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
fn big_blocks_that_grow_and_go_stay_within_the_kernels_limit() {
    // On macOS, malloc keeps each of these when it's freed, for a request of that size,
    // which never comes: 2 GiB by the end, with half of `LIMIT` counted. Under the kernel's
    // limit it keeps fewer, and the process lives.
    const MIB: usize = 512;
    let run = in_child_under(
        "big_blocks_that_grow_and_go_stay_within_the_kernels_limit",
        MIB,
        || {
            set_limit(LIMIT);
            for _ in 0..80 {
                let mut v = vec![1u8; LIMIT / 4];
                v.reserve_exact(LIMIT / 4);
                v.resize(LIMIT / 2, 1);
                black_box(v);
                sample_memory();
            }
        },
    );
    run.lived();
    let peak = run.peak_kib();
    assert!(peak <= MIB * 1024, "{peak} KiB: {run:#?}");
}

#[test]
fn growth_the_count_cant_see_dies_at_the_kernels_limit() {
    if !Path::new(TASKPOLICY).exists() {
        return;
    }
    // A thread's stack doesn't come through Rust's allocator.
    fn touch_stack(frames: usize) {
        let frame = black_box([1u8; 1 << 16]);
        if frames > 0 {
            touch_stack(frames - 1);
        }
        black_box(frame);
    }
    const FRAMES: usize = (8 * LIMIT) >> 16;
    in_child_under(
        "growth_the_count_cant_see_dies_at_the_kernels_limit",
        (2 * LIMIT) >> 20,
        || {
            let _ = thread::Builder::new()
                .stack_size(10 * LIMIT)
                .spawn(|| touch_stack(FRAMES))
                .unwrap()
                .join();
            exit(NOT_STOPPED);
        },
    )
    .killed_by_the_kernel();
}

#[test]
fn going_under_the_kernels_limit_keeps_the_pid() {
    let run = in_child("going_under_the_kernels_limit_keeps_the_pid", || {
        note(&format!("pid {}", std::process::id()));
    });
    run.lived();
    let line = format!("pid {}\n", run.pid());
    assert!(run.stdout().contains(&line), "{run:#?}");
}

#[test]
fn a_closed_pipe_is_an_error_and_no_signal() {
    // If going under the kernel's limit is tried and fails, SIGPIPE is left to kill this. It's
    // in a child because a process another test is starting holds the pipe open for a moment.
    in_child("a_closed_pipe_is_an_error_and_no_signal", || {
        let mut child = Command::new("true")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        child.wait().unwrap();
        assert!(stdin.write_all(b"x").is_err());
    })
    .lived();
}

#[test]
fn a_refused_allocation_can_be_handled_and_says_so_once() {
    capped(
        "a_refused_allocation_can_be_handled_and_says_so_once",
        || {
            let held = vec![1u8; LIMIT / 2];
            let mut v: Vec<u8> = Vec::new();
            let start = Instant::now();
            assert!(v.try_reserve_exact(LIMIT / 4 * 3).is_err());
            assert!(v.try_reserve_exact(LIMIT / 4 * 3).is_err());
            // A second refusal would wait a full second if the first had left the line
            // looking unfinished.
            assert!(start.elapsed() < Duration::from_millis(900));
            // The refused blocks aren't counted any more, so a block that fits is still fine.
            v.resize(LIMIT / 4, 1);
            black_box((held, v));
        },
    )
    .lived_with(LINE);
}

/// Each thread counts its block, then looks at the total. If each could miss the other's
/// count, both would get a block only one has room for. Even with no fence in `fits` that
/// happens only on some runs, so this often misses a broken cap, but it never fails a sound
/// one. None of these blocks is touched, so they cost almost nothing.
#[test]
fn of_two_threads_asking_for_the_last_room_at_once_only_one_gets_it() {
    const ROUNDS: usize = 100_000;
    in_child(
        "of_two_threads_asking_for_the_last_room_at_once_only_one_gets_it",
        || {
            set_limit(LIMIT);
            let arrived = AtomicUsize::new(0);
            let got = AtomicUsize::new(0);
            let meet = |met: &mut usize| {
                *met += 2;
                arrived.fetch_add(1, SeqCst);
                while arrived.load(SeqCst) < *met {
                    std::hint::spin_loop();
                }
            };
            thread::scope(|threads| {
                for _ in 0..2 {
                    threads.spawn(|| {
                        let mut met = 0;
                        for _ in 0..ROUNDS {
                            meet(&mut met);
                            let mut block: Vec<u8> = Vec::new();
                            if block.try_reserve_exact(LIMIT / 4 * 3).is_ok() {
                                got.fetch_add(1, SeqCst);
                            }
                            meet(&mut met);
                            assert!(got.load(SeqCst) <= 1, "both got the block");
                            drop(block);
                            meet(&mut met);
                            got.store(0, SeqCst);
                            meet(&mut met);
                        }
                    });
                }
            });
        },
    )
    .lived_with(LINE);
}

#[test]
fn a_block_the_system_refuses_counts_for_nothing() {
    in_child("a_block_the_system_refuses_counts_for_nothing", || {
        const HUGE: usize = 1 << 62;
        set_limit(usize::MAX);
        let mut empty: Vec<u8> = Vec::new();
        assert!(empty.try_reserve_exact(HUGE).is_err());
        let mut live = vec![1u8; 16];
        assert!(live.try_reserve_exact(HUGE).is_err());
        // Neither refusal is still counted, so a block that fits is still fine.
        set_limit(LIMIT);
        black_box(vec![1u8; LIMIT / 2]);
    })
    .lived();
}

/// A child whose test has changed its name runs no test, and exits 0. Without this, the
/// cases that must live could stop testing anything, and no one would know.
#[test]
#[should_panic(expected = "left: Some(0)")]
fn a_case_whose_body_never_ran_did_not_live() {
    in_child("no test has this name", || {}).lived();
}

#[test]
fn set_limit_raises_the_limit() {
    capped("set_limit_raises_the_limit", || {
        set_limit(2 * LIMIT);
        black_box(vec![1u8; LIMIT / 2 * 3]);
    })
    .lived();
}
