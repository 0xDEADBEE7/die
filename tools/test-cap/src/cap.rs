//! The allocator, and the only `unsafe` in this crate: `System` with the count around it.
//! The counting itself is safe code, in the crate's root.

use crate::{charge, release, reserve};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr::null_mut;

struct Cap;

#[global_allocator]
static GLOBAL: Cap = Cap;

// SAFETY: every block comes from `System`, which meets `GlobalAlloc`'s contract, and goes
// back to it with the layout it came with. Returning null is how an allocator refuses.
unsafe impl GlobalAlloc for Cap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let charged = charge(layout.size(), layout.align());
        if !reserve(charged) {
            return null_mut();
        }
        // SAFETY: the caller gives `layout` under `alloc`'s contract, which is `System`'s too.
        let block = unsafe { System.alloc(layout) };
        if block.is_null() {
            release(charged);
        }
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let charged = charge(layout.size(), layout.align());
        if !reserve(charged) {
            return null_mut();
        }
        // SAFETY: as in `alloc`. This stays `System`'s own, whose pages cost nothing until
        // they're touched; the default would write every byte.
        let block = unsafe { System.alloc_zeroed(layout) };
        if block.is_null() {
            release(charged);
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller's `block` came from this allocator, so from `System`, with `layout`.
        unsafe { System.dealloc(block, layout) };
        release(charge(layout.size(), layout.align()));
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = charge(layout.size(), layout.align());
        let new = charge(new_size, layout.align());
        if new > old && !reserve(new - old) {
            return null_mut();
        }
        // SAFETY: the caller's `block` came from this allocator, so from `System`, with
        // `layout`, and `new_size` meets `realloc`'s contract, which is `System`'s too.
        let moved = unsafe { System.realloc(block, layout, new_size) };
        if moved.is_null() {
            // The old block still stands, at its old size.
            release(new.saturating_sub(old));
        } else {
            release(old.saturating_sub(new));
        }
        moved
    }
}

/// Only `unsafe` can ask for these two kinds of block, so only here can a test reach them.
#[cfg(test)]
mod tests {
    use super::{Cap, GlobalAlloc, Layout};
    use crate::{DEFAULT_LIMIT, live, set_limit};
    use std::hint::black_box;
    use std::process::Command;

    /// `alloc_zeroed` has no safe way to fail: `vec![0; n]` aborts. So this one's here, and
    /// `alloc`'s and `realloc`'s are in `tests/cap.rs`. It runs in a child, since a block
    /// this size, counted for a moment, would stop the other tests' allocations.
    #[test]
    fn a_zeroed_block_the_system_refuses_counts_for_nothing() {
        const CHILD: &str = "DIE_TEST_CAP_CHILD";
        const HUGE: usize = 1 << 62;
        if std::env::var_os(CHILD).is_none() {
            let name = "cap::tests::a_zeroed_block_the_system_refuses_counts_for_nothing";
            let out = Command::new(std::env::current_exe().unwrap())
                .args([name, "--exact"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            let ran = String::from_utf8_lossy(&out.stdout).contains("1 passed");
            assert!(out.status.success() && ran, "{out:?}");
            return;
        }
        let layout = Layout::from_size_align(HUGE, 1).unwrap();
        set_limit(usize::MAX);
        // SAFETY: the layout's size isn't zero. `black_box` because the compiler may drop a
        // call to `calloc` if all that's done with the block is to compare it with null.
        let block = black_box(unsafe { Cap.alloc_zeroed(layout) });
        set_limit(DEFAULT_LIMIT);
        assert!(block.is_null());
        assert!(live() < (HUGE / 2).cast_signed(), "{} counted", live());
    }

    /// Each of these costs the system its alignment, for the one byte it holds. None of it
    /// is touched, so it costs nothing here. Other tests here allocate only small blocks,
    /// so the checks leave plenty of room for them.
    #[test]
    fn a_block_with_a_big_alignment_counts_for_that_alignment() {
        const ALIGN: usize = 64 << 20;
        let one = Layout::from_size_align(1, ALIGN).unwrap();
        let two = Layout::from_size_align(2, ALIGN).unwrap();
        let half_a_block_more = live() + (ALIGN / 2).cast_signed();
        // SAFETY: the layout's size isn't zero.
        let block = unsafe { Cap.alloc(one) };
        assert!(!block.is_null());
        assert!(live() > half_a_block_more, "`alloc`");
        // SAFETY: `block` came from this allocator with `one`, and the new size isn't zero.
        let block = unsafe { Cap.realloc(block, one, 2) };
        assert!(!block.is_null());
        assert!(live() > half_a_block_more, "`realloc`");
        // SAFETY: `block` came from this allocator with `two`, which `realloc` left it with.
        unsafe { Cap.dealloc(block, two) };
        assert!(live() < half_a_block_more, "`dealloc`");
        // SAFETY: the layout's size isn't zero.
        let block = unsafe { Cap.alloc_zeroed(one) };
        assert!(!block.is_null());
        assert!(live() > half_a_block_more, "`alloc_zeroed`");
        // SAFETY: `block` came from this allocator with `one`.
        unsafe { Cap.dealloc(block, one) };
    }
}
