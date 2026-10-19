//! G10: DIE's process-global PNG decode hook, installed once through die-term-sys.
//!
//! The engine's PNG callback is process-global (`ghostty_sys_set`, sys.h). libghostty-rs's
//! `set_png_decoder` keeps the decoder in a `thread_local!` (wrapper issue #104), so a PNG
//! parsed on any other thread is rejected. DIE never calls that function. It installs this
//! stateless callback once at startup, and every parser thread (or helper process) uses it.
use libghostty_vt_sys as sys;
use std::ffi::c_void;
use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub static DECODES: AtomicU64 = AtomicU64::new(0);
pub static FAILURES: AtomicU64 = AtomicU64::new(0);
pub static DECODE_NS: AtomicU64 = AtomicU64::new(0);
/// Failures by stage: header rejected by the png crate (incl. its own 64 MiB limit), the
/// engine allocator returned NULL (its cap), or the pixel data failed to decode.
pub static REFUSED_HEADER: AtomicU64 = AtomicU64::new(0);
pub static REFUSED_ALLOC: AtomicU64 = AtomicU64::new(0);
pub static REFUSED_DATA: AtomicU64 = AtomicU64::new(0);

static INSTALL: Once = Once::new();

/// Install the hook. Idempotent; call before the first terminal is created.
pub fn install() {
    INSTALL.call_once(|| set(true));
}

/// Install (`true`) or clear (`false`) unconditionally. Only tests use this directly.
pub fn set(on: bool) {
    let f: sys::SysDecodePngFn = if on { Some(decode_png) } else { None };
    let v = f.map_or(std::ptr::null(), |p| p as *const c_void);
    // SAFETY: DECODE_PNG takes the function pointer itself as the value (sys.h).
    let r = unsafe { sys::ghostty_sys_set(sys::SysOption::DECODE_PNG, v) };
    assert_eq!(r, sys::Result::SUCCESS, "ghostty_sys_set(DECODE_PNG) failed");
}

unsafe extern "C" fn decode_png(
    _userdata: *mut c_void,
    alloc: *const sys::Allocator,
    data: *const u8,
    data_len: usize,
    out: *mut sys::SysImage,
) -> bool {
    let t0 = std::time::Instant::now();
    // A panic must not unwind into the engine.
    let ok = std::panic::catch_unwind(|| {
        // SAFETY: the engine passes a valid buffer for the duration of the call.
        let png = unsafe { std::slice::from_raw_parts(data, data_len) };
        decode_into(alloc, png).map(|img| {
            // SAFETY: `out` is valid and zeroed by the engine.
            unsafe { *out = img };
        })
    })
    .ok()
    .flatten()
    .is_some();
    DECODE_NS.fetch_add(t0.elapsed().as_nanos() as u64, Relaxed);
    if ok { DECODES.fetch_add(1, Relaxed) } else { FAILURES.fetch_add(1, Relaxed) };
    ok
}

/// Decode to RGBA8 in a buffer from the engine's allocator, which caps image memory.
/// The buffer is allocated from the header's size before any pixels are decoded.
fn decode_into(alloc: *const sys::Allocator, png: &[u8]) -> Option<sys::SysImage> {
    use png::{BitDepth, ColorType, Transformations};
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);
    let Ok(mut reader) = dec.read_info() else {
        REFUSED_HEADER.fetch_add(1, Relaxed);
        return None;
    };
    let (w, h) = reader.info().size();
    let px = (w as usize).checked_mul(h as usize)?;
    let rgba_len = px.checked_mul(4)?;
    let gray = match reader.output_color_type() {
        (ColorType::Rgba, BitDepth::Eight) => false,
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => true,
        _ => return None,
    };
    let decoded_len = if gray { px * 2 } else { rgba_len };
    if reader.output_buffer_size()? != decoded_len {
        return None;
    }
    // SAFETY: allocator is valid for this call; NULL means over the engine's cap.
    let ptr = unsafe { sys::ghostty_alloc(alloc, rgba_len) };
    if ptr.is_null() {
        REFUSED_ALLOC.fetch_add(1, Relaxed);
        return None;
    }
    // SAFETY: ptr points at rgba_len writable bytes from the engine allocator.
    let buf = unsafe { std::slice::from_raw_parts_mut(ptr, rgba_len) };
    let ok = reader.next_frame(&mut buf[..decoded_len]).is_ok_and(|i| i.buffer_size() == decoded_len);
    if !ok {
        REFUSED_DATA.fetch_add(1, Relaxed);
        // SAFETY: same allocator and length as the allocation above.
        unsafe { sys::ghostty_free(alloc, ptr, rgba_len) };
        return None;
    }
    if gray {
        // Expand GA to RGBA in place, from the end so nothing is overwritten early.
        for i in (0..px).rev() {
            let (g, a) = (buf[2 * i], buf[2 * i + 1]);
            buf[4 * i..4 * i + 4].copy_from_slice(&[g, g, g, a]);
        }
    }
    Some(sys::SysImage { width: w, height: h, data: ptr, data_len: rgba_len })
}
