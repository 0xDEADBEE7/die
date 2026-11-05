//! The allocator, and the only `unsafe` in this crate: `System` with the count around it,
//! and a way to write to stderr that can't allocate. The counting itself is safe code, in
//! the crate's root.

use crate::{charge, release, reserve};
use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::File;
use std::io::{Write, stderr};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd};
use std::ptr::null_mut;

struct Cap;

#[global_allocator]
static GLOBAL: Cap = Cap;

// SAFETY: every block comes from `System`, which meets `GlobalAlloc`'s contract, and goes
// back to it with the layout it came with. Returning null is how an allocator refuses.
unsafe impl GlobalAlloc for Cap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !reserve(charge(layout.size())) {
            return null_mut();
        }
        // SAFETY: the caller gives `layout` under `alloc`'s contract, which is `System`'s too.
        let block = unsafe { System.alloc(layout) };
        if block.is_null() {
            release(charge(layout.size()));
        }
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !reserve(charge(layout.size())) {
            return null_mut();
        }
        // SAFETY: as in `alloc`. This stays `System`'s own, whose pages cost nothing until
        // they're touched; the default would write every byte.
        let block = unsafe { System.alloc_zeroed(layout) };
        if block.is_null() {
            release(charge(layout.size()));
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller's `block` came from this allocator, so from `System`, with `layout`.
        unsafe { System.dealloc(block, layout) };
        release(charge(layout.size()));
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let (old, new) = (charge(layout.size()), charge(new_size));
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

/// Write straight to stderr's file descriptor. `io::stderr()` won't do here: it takes a
/// lock, and on macOS the first use of that lock allocates 64 bytes, which a process at
/// its limit doesn't have.
pub fn to_stderr(line: &[u8]) {
    // SAFETY: the `File` only writes to the descriptor, which fails harmlessly if it isn't
    // open, and `ManuallyDrop` keeps the `File` from closing it.
    let mut file = ManuallyDrop::new(unsafe { File::from_raw_fd(stderr().as_raw_fd()) });
    let _ = file.write_all(line);
}
