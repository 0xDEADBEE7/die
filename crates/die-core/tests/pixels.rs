//! M3a: pixel buffers and placements (contract §11). What core gives an extension that draws
//! pixels: a producer over the harness's fake sink, and a surface's placement of it in the scene.
//!
//! Test plan:
//! - `pixels_are_supported_with_a_sink_and_not_without`: `pixels_supported` and `pixel_buffer`
//!   on a bare `Ctx` and on a harness; a second sink is refused.
//! - `a_present_updates_the_fakes_pixels`: the whole buffer, then a rectangle, which leaves the
//!   rest; the calls the sink heard.
//! - `damage_is_clipped_to_the_buffer_and_none_left_is_no_present`
//! - `a_resize_reallocates_tells_the_sink_and_clears`: the size, the stride, transparent again,
//!   and a bad size refused with the buffer kept.
//! - `bad_sizes_are_errors`: a side of 0 or over `PIXEL_MAX_SIDE`.
//! - `a_present_from_another_thread_wakes_the_loop`: the producer moved to a thread; the waker
//!   fires once until the loop's next turn, as a job's wake does.
//! - `a_surface_places_its_buffer_over_its_area`: the placement's geometry is the area's,
//!   offset by the window and its margins, with its z; placed every render, or not at all.
//! - `placements_are_in_drawing_order_lower_z_first`
//! - `a_dropped_producer_destroys_its_buffer_and_its_placement_goes`
//! - `a_placement_stops_above_the_prompts_list`: as the hollow cursors do.
//! - `the_snapshot_lists_placements`: only when there are any.
//! - `pixel_rects`: union, which saturates rather than overflow, and clipping.

use die_test_cap as _;

use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A surface that places what it's told to at each render, and counts its renders.
struct Board {
    place: Rc<RefCell<Vec<(PixelBufferId, i8)>>>,
    renders: Rc<RefCell<usize>>,
}

impl Surface for Board {
    fn kind(&self) -> &'static str {
        "board"
    }
    fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
        *self.renders.borrow_mut() += 1;
        if frame.is_new() {
            frame.text(0, 0, "board", &FrameCell::blank());
        }
        for &(id, z) in self.place.borrow().iter() {
            frame.place(id, z);
        }
    }
}

struct Toy;

impl Extension for Toy {
    fn name(&self) -> &'static str {
        "toy"
    }
    fn register(&mut self, _: &mut Registry) {}
}

struct Setup {
    h: Harness,
    place: Rc<RefCell<Vec<(PixelBufferId, i8)>>>,
    win: WindowId,
}

/// A text window on the left and a board on the right, focused, with the owner's 80-column
/// centered default so the area has margins.
fn setup(text_defaults: Settings) -> Setup {
    let config = Config {
        text_defaults,
        ..Config::default()
    };
    let mut h = Harness::with_config(vec![Box::new(Toy)], config).unwrap();
    h.open_text("t", "text\n").unwrap();
    let place = Rc::new(RefCell::new(vec![]));
    let board = Board {
        place: place.clone(),
        renders: Rc::default(),
    };
    let buf = h.create_surface("board", board);
    let win = h.show(buf, ShowIn::Split(Side::Right)).unwrap().unwrap();
    h.send(Input::Resize {
        cols: 200,
        rows: 26,
    });
    Setup { h, place, win }
}

fn centered() -> Settings {
    Settings {
        max_width: Some(80),
        center: true,
        ..Settings::default()
    }
}

fn fill(p: &mut PixelProducer, rgba: [u8; 4]) {
    for px in p.pixels_mut().chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

#[test]
fn pixels_are_supported_with_a_sink_and_not_without() {
    let mut bare = Ctx::new(vec![]).unwrap();
    assert!(!bare.pixels_supported(), "no window, no sink");
    let err = bare.pixel_buffer(8, 8).err().map(|e| e.to_string());
    assert_eq!(err.as_deref(), Some("this host shows no pixels"));
    let mut h = Harness::new(vec![]).unwrap();
    assert!(h.pixels_supported());
    assert!(h.pixel_buffer(8, 8).is_ok());
    let again = h.set_pixel_sink(FakePixelSink::default()).err();
    assert_eq!(
        again.map(|e| e.to_string()).as_deref(),
        Some("the pixel sink is already connected")
    );
    assert!(bare.set_pixel_sink(FakePixelSink::default()).is_ok());
    assert!(bare.pixels_supported());
}

#[test]
fn a_present_updates_the_fakes_pixels() {
    let mut h = Harness::new(vec![]).unwrap();
    let (id, mut p) = h.pixel_buffer(4, 3).unwrap();
    assert_eq!((p.id(), p.size(), p.stride()), (id, (4, 3), 16));
    assert_eq!(
        h.pixels(id),
        Some((4, 3, vec![0; 48])),
        "transparent at first"
    );
    fill(&mut p, [10, 20, 30, 255]);
    assert_eq!(
        h.pixels(id).unwrap().2,
        vec![0; 48],
        "nothing until a present"
    );
    p.present(PixelRect::whole(4, 3));
    let (w, hgt, px) = h.pixels(id).unwrap();
    assert_eq!((w, hgt), (4, 3));
    assert!(px.chunks_exact(4).all(|c| c == [10, 20, 30, 255]));
    // A rectangle: pixel (1, 1) and (2, 1) change in the producer; only (1, 1) is presented.
    let row = p.stride();
    p.pixels_mut()[row + 4..row + 8].copy_from_slice(&[1, 1, 1, 255]);
    p.pixels_mut()[row + 8..row + 12].copy_from_slice(&[2, 2, 2, 255]);
    p.present(PixelRect::new(1, 1, 1, 1));
    let px = h.pixels(id).unwrap().2;
    assert_eq!(&px[row + 4..row + 8], &[1, 1, 1, 255]);
    assert_eq!(
        &px[row + 8..row + 12],
        &[10, 20, 30, 255],
        "outside the damage"
    );
    assert_eq!(
        &p.pixels()[row + 8..row + 12],
        &[2, 2, 2, 255],
        "the producer's own are what it drew"
    );
    assert_eq!(
        h.fake_pixels().take_calls(),
        [
            PixelSinkCall::Create(id, 4, 3),
            PixelSinkCall::Present(id, PixelRect::whole(4, 3)),
            PixelSinkCall::Present(id, PixelRect::new(1, 1, 1, 1)),
        ]
    );
}

#[test]
fn damage_is_clipped_to_the_buffer_and_none_left_is_no_present() {
    let mut h = Harness::new(vec![]).unwrap();
    let (id, mut p) = h.pixel_buffer(4, 3).unwrap();
    h.fake_pixels().take_calls();
    fill(&mut p, [9, 9, 9, 255]);
    p.present(PixelRect::new(10, 10, 5, 5));
    p.present(PixelRect::new(0, 0, 0, 3));
    assert_eq!(
        h.fake_pixels().take_calls(),
        [],
        "nothing inside the buffer"
    );
    assert_eq!(h.pixels(id).unwrap().2, vec![0; 48]);
    p.present(PixelRect::new(2, 1, 100, 100));
    assert_eq!(
        h.fake_pixels().take_calls(),
        [PixelSinkCall::Present(id, PixelRect::new(2, 1, 2, 2))]
    );
    let px = h.pixels(id).unwrap().2;
    assert_eq!(&px[0..16], &[0; 16], "the first row wasn't in it");
    assert_eq!(&px[16 + 8..32], &[9, 9, 9, 255, 9, 9, 9, 255]);
}

#[test]
fn a_resize_reallocates_tells_the_sink_and_clears() {
    let mut h = Harness::new(vec![]).unwrap();
    let (id, mut p) = h.pixel_buffer(4, 3).unwrap();
    fill(&mut p, [5, 5, 5, 255]);
    p.present(PixelRect::whole(4, 3));
    h.fake_pixels().take_calls();
    p.resize(2, 5).unwrap();
    assert_eq!((p.size(), p.stride(), p.pixels().len()), ((2, 5), 8, 40));
    assert!(p.pixels().iter().all(|b| *b == 0), "transparent again");
    assert_eq!(h.pixels(id), Some((2, 5, vec![0; 40])));
    assert_eq!(
        h.fake_pixels().take_calls(),
        [PixelSinkCall::Resize(id, 2, 5)],
        "no present of its own: the producer presents when it has drawn"
    );
    assert!(p.resize(0, 5).is_err());
    assert_eq!(p.size(), (2, 5), "kept as it was");
    assert_eq!(h.fake_pixels().take_calls(), []);
}

#[test]
fn bad_sizes_are_errors() {
    let mut h = Harness::new(vec![]).unwrap();
    for (w, hgt) in [
        (0, 1),
        (1, 0),
        (PIXEL_MAX_SIDE + 1, 1),
        (1, PIXEL_MAX_SIDE + 1),
    ] {
        let err = h.pixel_buffer(w, hgt).err().map(|e| e.to_string());
        let want = format!("a pixel buffer is 1 to 16384 pixels a side, not {w}x{hgt}");
        assert_eq!(err.as_deref(), Some(want.as_str()));
    }
    assert_eq!(h.fake_pixels().take_calls(), [], "the sink heard of none");
    assert!(h.pixel_buffer(PIXEL_MAX_SIDE, 1).is_ok());
}

#[test]
fn a_present_from_another_thread_wakes_the_loop() {
    let mut h = Harness::new(vec![]).unwrap();
    let woke = Arc::new(AtomicUsize::new(0));
    let w = woke.clone();
    h.set_waker(move || {
        w.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let (id, mut p) = h.pixel_buffer(3, 3).unwrap();
    let worker = std::thread::spawn(move || {
        fill(&mut p, [1, 2, 3, 255]);
        p.present(PixelRect::whole(3, 3));
        p.present(PixelRect::whole(3, 3));
        p
    });
    let p = worker.join().unwrap();
    assert_eq!(
        woke.load(Ordering::SeqCst),
        1,
        "once until the loop's next turn"
    );
    assert_eq!(h.pixels(id).unwrap().2[..4], [1, 2, 3, 255]);
    h.run_until_idle();
    p.present(PixelRect::new(0, 0, 1, 1));
    assert_eq!(woke.load(Ordering::SeqCst), 2);
    h.run_until_idle();
    drop(p);
    assert_eq!(
        woke.load(Ordering::SeqCst),
        3,
        "a drop wakes it too, for the frame"
    );
}

#[test]
fn a_surface_places_its_buffer_over_its_area() {
    let mut s = setup(centered());
    let (id, _p) = s.h.pixel_buffer(16, 16).unwrap();
    assert_eq!(s.h.scene().placements, [], "nothing placed yet");
    s.place.borrow_mut().push((id, -1));
    let at = s.h.placements().into_iter().find(|p| p.window == s.win);
    let (at, g) = (at.unwrap(), s.h.surface_area(s.win).unwrap());
    assert!(g.text_x > 0, "the area has a margin: {g:?}");
    let want = Placement {
        buffer: id,
        col: at.col + g.text_x,
        row: at.row,
        cols: g.text_w,
        rows: g.rows,
        z: -1,
    };
    assert_eq!(s.h.scene().placements, [want]);
    assert_eq!((want.cols, want.rows), (80, 24));
    // The frame keeps no placement: a render that places nothing shows nothing.
    s.place.borrow_mut().clear();
    assert_eq!(s.h.scene().placements, []);
    // Full width: the area is the window.
    s.place.borrow_mut().push((id, 0));
    s.h.run("toggle-max-width", &Args::default()).unwrap();
    s.h.run_until_idle();
    let g = s.h.surface_area(s.win).unwrap();
    let got = s.h.scene().placements;
    assert_eq!((got[0].col, got[0].cols, got[0].z), (at.col, g.text_w, 0));
    assert_eq!(g.text_x, 0);
}

#[test]
fn placements_are_in_drawing_order_lower_z_first() {
    let mut s = setup(Settings::default());
    let (a, _pa) = s.h.pixel_buffer(2, 2).unwrap();
    let (b, _pb) = s.h.pixel_buffer(2, 2).unwrap();
    let (c, _pc) = s.h.pixel_buffer(2, 2).unwrap();
    *s.place.borrow_mut() = vec![(a, 1), (b, -1), (c, 1)];
    let order: Vec<_> =
        s.h.scene()
            .placements
            .iter()
            .map(|p| (p.buffer, p.z))
            .collect();
    assert_eq!(order, [(b, -1), (a, 1), (c, 1)], "stable within a z");
}

#[test]
fn a_dropped_producer_destroys_its_buffer_and_its_placement_goes() {
    let mut s = setup(Settings::default());
    let (id, p) = s.h.pixel_buffer(8, 8).unwrap();
    s.place.borrow_mut().push((id, -1));
    assert_eq!(s.h.scene().placements.len(), 1);
    s.h.fake_pixels().take_calls();
    drop(p);
    assert_eq!(s.h.fake_pixels().take_calls(), [PixelSinkCall::Destroy(id)]);
    assert_eq!(s.h.pixels(id), None);
    assert_eq!(s.h.scene().placements, [], "the surface still places it");
    assert_eq!(s.place.borrow().len(), 1);
    // A new buffer gets a new id, never the old one.
    let (next, _p) = s.h.pixel_buffer(8, 8).unwrap();
    assert_ne!(next, id);
}

#[test]
fn a_placement_stops_above_the_prompts_list() {
    let mut s = setup(Settings::default());
    let (id, _p) = s.h.pixel_buffer(8, 8).unwrap();
    s.place.borrow_mut().push((id, 1));
    let whole = s.h.scene().placements[0];
    assert_eq!((whole.row, whole.rows), (0, 24));
    let many: Vec<String> = (0..30).map(|i| format!("item {i}")).collect();
    s.h.prompt(PromptSpec::new("Pick: ").candidates(many), |_, _| Ok(()));
    s.h.run_until_idle();
    let scene = s.h.scene();
    let cut = scene.placements[0];
    // The status lines sit on the list's top row, the prompt's line under them (with vertico's
    // count before the label), the candidates below that.
    let prompt_row = (0..scene.rows)
        .find(|&r| scene.row_text(r).contains("Pick:"))
        .expect("the prompt shows over its list");
    assert!(scene.row_text(prompt_row + 1).starts_with("item"));
    assert!(cut.rows < 24, "{cut:?}");
    assert_eq!(
        cut.row + cut.rows,
        prompt_row - 1,
        "above the status line over the list"
    );
    s.h.type_keys("<Esc>").unwrap();
    assert_eq!(s.h.scene().placements[0], whole);
}

#[test]
fn the_snapshot_lists_placements() {
    let mut s = setup(Settings::default());
    assert!(!s.h.style_dump().contains("placements"));
    let (id, _p) = s.h.pixel_buffer(8, 8).unwrap();
    s.place.borrow_mut().push((id, -1));
    let dump = s.h.style_dump();
    let at = s.h.scene().placements[0];
    let want = format!(
        "placements\n{},{} {}x24 z -1 {id:?}\n",
        at.row, at.col, at.cols
    );
    assert!(dump.ends_with(&want), "{dump}");
}

#[test]
fn pixel_rects() {
    let r = PixelRect::new(2, 3, 4, 5);
    assert_eq!(
        r.union(PixelRect::new(0, 10, 1, 1)),
        PixelRect::new(0, 3, 6, 8)
    );
    assert_eq!(r.union(PixelRect::default()), r);
    assert_eq!(PixelRect::default().union(r), r);
    assert_eq!(r.clipped_to(4, 4), PixelRect::new(2, 3, 2, 1));
    assert_eq!(r.clipped_to(1, 1), PixelRect::new(1, 1, 0, 0));
    assert!(r.clipped_to(1, 1).is_empty());
    assert_eq!(PixelRect::whole(7, 2), PixelRect::new(0, 0, 7, 2));
    // A rectangle past the end of u32 unions as clipping does, saturating, rather than overflow.
    let far = PixelRect::new(u32::MAX - 1, 0, 4, 1);
    assert_eq!(
        far.union(PixelRect::new(0, 0, 1, 1)),
        PixelRect::new(0, 0, u32::MAX, 1)
    );
}
