//! Negative control for wrapper issue #104: the wrapper's own `set_png_decoder` keeps the
//! decoder in a thread_local, so a PNG parsed on another thread is rejected, and calling it
//! replaces DIE's global hook process-wide. This is why DIE never calls it.
use libghostty_vt::Terminal;
use libghostty_vt::alloc::{Allocator, Bytes};
use libghostty_vt::kitty::graphics::{self as kg, DecodePng, DecodedImage};
use s2_term_engine::{dterm::kitty_chunked, dterm::png_rgba, hook};

struct OnePixel;
impl DecodePng for OnePixel {
    fn decode_png<'a>(&mut self, alloc: &'a Allocator<'_>, _data: &[u8]) -> Option<DecodedImage<'a>> {
        let mut b = Bytes::new_with_alloc(alloc, 4).ok()?;
        b.copy_from_slice(&[1, 2, 3, 255]);
        Some(DecodedImage { width: 1, height: 1, data: b })
    }
}

fn png_on_new_thread() -> bool {
    std::thread::spawn(|| {
        let mut t = Terminal::new(40, 10).unwrap();
        t.set_kitty_image_storage_limit(1 << 20).unwrap();
        for ch in kitty_chunked("q=2,a=t,f=100,i=3", &png_rgba(4, 4, |_, _| [9, 9, 9, 255])) {
            t.vt_write(&ch);
        }
        t.kitty_graphics().unwrap().image(3).is_some()
    })
    .join()
    .unwrap()
}

#[test]
fn wrapper_thread_local_decoder_fails_across_threads_and_clobbers_the_hook() {
    hook::install();
    assert!(png_on_new_thread(), "DIE's global hook works on any thread");
    // The wrapper's API, called on this thread only:
    kg::set_png_decoder(Some(Box::new(OnePixel))).unwrap();
    assert!(!png_on_new_thread(), "issue #104: another thread finds no decoder");
    kg::set_png_decoder(None).unwrap();
    assert!(!png_on_new_thread(), "clearing it removed DIE's hook process-wide too");
    hook::set(true);
    assert!(png_on_new_thread(), "reinstalling DIE's hook restores every thread");
}
