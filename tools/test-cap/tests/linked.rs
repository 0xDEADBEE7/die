//! The one line every test binary carries is all it takes. This file names the cap with
//! that line alone, and calls nothing in it, as a real test binary does, and it holds
//! the default limit.
//!
//! Both cases ask for zeroed pages they never touch, so they cost nothing if the cap is
//! missing.

use die_test_cap as _;

mod support;

use std::hint::black_box;
use support::{NOT_STOPPED, in_child, sample_memory};

const FOUR_GIB: usize = 4 << 30;

#[test]
fn the_line_alone_installs_the_cap_at_the_default_limit() {
    in_child(
        "the_line_alone_installs_the_cap_at_the_default_limit",
        || {
            black_box(vec![0u8; FOUR_GIB + 1]);
            std::process::exit(NOT_STOPPED);
        },
    )
    .died_with("die-test-cap: over the memory limit of 4294967296 bytes (4096 MiB)");
}

#[test]
fn the_default_limit_is_not_lower_than_it_says() {
    let run = in_child("the_default_limit_is_not_lower_than_it_says", || {
        let block = black_box(vec![0u8; FOUR_GIB - (64 << 20)]);
        sample_memory();
        black_box(block);
    });
    run.lived();
    // Zeroed pages stay untouched, as the system gives them, however big the block.
    let peak = run.peak_kib();
    assert!(peak < 64 << 10, "{peak} KiB: {run:#?}");
}
