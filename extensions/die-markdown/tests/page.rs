//! The page through the editor: the toggle shows a file's page where the cursor was and the
//! file again, keys and the wheel scroll it between its ends, a link goes where it leads and
//! `C-o` comes back, the page follows the file's text and the window's width, goes with the
//! file, and comes back after a restart; and a host that shows no pixels gets the view in place.
//!
//! Every position comes from the page itself (its links, its headings, where a byte of the
//! source is), never from what a page is known to look like: the typesetter is free to draw as
//! it likes. What the tests do assume of it is a GitHub-like page in a window of 600 by 400
//! pixels at 2x: the first link in view at the top, the first heading in the window's upper
//! half, and a link a paragraph after another on a line of its own under it. Nothing is written
//! outside a temporary directory, and nothing is opened outside DIE: the opener is a script that
//! writes down what it was given.

use die_test_cap as _;

use die_core::{
    Args, BufferId, Button, CellPixels, Config, Ctx, Extension, Harness, Input, Key, KeyEvent,
    Mods, Origin, PixelBufferId, PixelRect, PixelSinkCall, PointerEvent, PointerKind, Registry,
    ShowIn, State,
};
use die_markdown::typeset::{Page, Rect};
use die_markdown::{MODE, Markdown, Options, PAGE, View};
use die_vim::Vim;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The cells the tests' host has: 10 by 20 physical pixels on a 2x display.
const CELL: CellPixels = CellPixels {
    width: 10,
    height: 20,
    scale: 2.0,
};
/// The window: 60 by 20 cells, so a page's area is 600 by 400 pixels.
const AREA: (u32, u32) = (600, 400);
/// The room the page leaves over a heading or a link it scrolled to, in CSS pixels.
const GAP: f32 = 16.0;

/// A fresh directory for one test.
fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("die-markdown-page-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// A file long enough to scroll, with a link of each kind near its top, each a paragraph of
/// its own, and a heading for each of thirty parts.
fn document() -> String {
    let mut s = String::from("# Title\n\n");
    for link in [
        "[part 15](#part-15)",
        "[the notes](b.md#second-part)",
        "[a line](b.md:5)",
        "[plain text](notes.txt)",
        "[the site](https://example.com/x)",
        "[line zero](b.md:0)",
        "[nowhere](#nowhere)",
        "[gone](gone.md)",
    ] {
        s += &format!("See {link}.\n\n");
    }
    for i in 1..=30 {
        s += &format!("## Part {i}\n\nParagraph {i}, with words enough to be a line of text.\n\n");
    }
    s + "## The end\n\nBack to [the top](#title).\n"
}

/// The file the links lead to: long too, so a heading in it can be scrolled to.
fn second() -> String {
    let mut s = String::from("# B\n\n");
    for i in 1..=30 {
        s += &format!("Line {i} of the notes.\n\n");
    }
    s += "## Second part\n\n";
    for i in 31..=60 {
        s += &format!("Line {i} of the notes.\n\n");
    }
    s
}

/// The toggle on `\m` in the mode's keymap and in the page's, as your config has it.
fn bindings(reg: &mut Registry) {
    let keys = reg.global_keymap();
    keys.bind(State::Normal, "<leader>z", "toggle-max-width");
    let keys = reg.mode_keymap(MODE).unwrap();
    keys.bind(State::Normal, "<leader>m", "markdown-toggle");
    let keys = reg.kind_keymap(PAGE);
    keys.bind(State::Normal, "<leader>m", "markdown-toggle");
}

fn extensions(options: Options) -> Vec<Box<dyn Extension>> {
    vec![Box::new(Vim::default()), Box::new(Markdown::new(options))]
}

/// die-vim and the extension in a window of 60 by 20 whose cells have a size in pixels, on an
/// empty buffer. The harness has a pixel sink, so the rendered view is a page.
fn harness(options: Options) -> Harness {
    let config = Config {
        register: Some(Box::new(bindings)),
        ..Config::default()
    };
    let mut h = Harness::with_config(extensions(options), config).unwrap();
    h.open_text("*scratch*", "").unwrap();
    h.set_window_size(60, 20);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

/// Open a file in the focused window.
fn open(h: &mut Harness, path: &Path) -> BufferId {
    let buf = h.open_file(path).unwrap();
    h.show(buf, ShowIn::Current).unwrap();
    h.run_until_idle();
    buf
}

/// Type keys that must all work.
fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
}

/// Type a key that must fail, and how.
fn fails(h: &mut Harness, k: &str) -> String {
    let typed = h.type_keys(k).unwrap();
    let errors: Vec<&String> = typed.iter().flat_map(|k| &k.errors).collect();
    assert_eq!(errors.len(), 1, "{k}: {typed:?}");
    errors[0].clone()
}

/// A page as the probe has it, without the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    buffer: BufferId,
    top: i32,
    area: (u32, u32),
    height: f32,
    ring: Option<usize>,
    pixels: Option<PixelBufferId>,
}

/// The file's page, if it has one.
fn seen(h: &mut Harness, file: BufferId) -> Option<Seen> {
    let read = |m: &mut Markdown, _: &mut Ctx| {
        m.page(file, |p| Seen {
            buffer: p.buffer,
            top: p.top,
            area: p.area,
            height: p.height,
            ring: p.ring,
            pixels: p.pixels,
        })
    };
    h.with_ext(read).unwrap()
}

/// The page's row at the window's top.
fn top(h: &mut Harness, file: BufferId) -> i32 {
    seen(h, file).expect("a page").top
}

/// Ask the file's layout something.
fn ask<R>(h: &mut Harness, file: BufferId, f: impl FnOnce(&Page) -> R) -> R {
    let read = |m: &mut Markdown, _: &mut Ctx| m.page(file, |p| f(p.page.expect("a layout")));
    h.with_ext(read).unwrap().expect("a page")
}

/// The byte of the source the line at the window's top shows.
fn line_at_top(h: &mut Harness, file: BufferId) -> usize {
    let top = top(h, file);
    ask(h, file, |p| p.source_at(top as f32))
}

/// The furthest the page scrolls.
fn limit(h: &mut Harness, file: BufferId) -> i32 {
    let page = seen(h, file).unwrap();
    (page.height.ceil() as i32 - page.area.1 as i32).max(0)
}

/// The link that leads to `dest`, and where its first line is.
fn link(h: &mut Harness, file: BufferId, dest: &str) -> (usize, Rect) {
    ask(h, file, |p| {
        let i = p.links().iter().position(|l| l.dest == dest);
        let i = i.unwrap_or_else(|| panic!("a link to {dest}"));
        (i, p.links()[i].rects[0])
    })
}

/// Where the page scrolls to for a link above the window: its top, less the gap.
fn over(r: Rect) -> i32 {
    (r.y - GAP * CELL.scale as f32).round().max(0.0) as i32
}

/// The top of the heading with this anchor.
fn heading(h: &mut Harness, file: BufferId, slug: &str) -> f32 {
    ask(h, file, |p| {
        let found = p.headings().iter().find(|h| h.slug == slug);
        found.unwrap_or_else(|| panic!("a heading #{slug}")).y
    })
}

/// Whether the heading is the gap under the window's top, or the page is at an end: at its top
/// a heading nearer than the gap can't come lower, and at its bottom one near the end can't
/// go higher.
fn at_heading(h: &mut Harness, file: BufferId, slug: &str) -> bool {
    let (y, top) = (heading(h, file, slug), top(h, file));
    let under = y - top as f32;
    (under - GAP * CELL.scale as f32).abs() <= 0.5 || top == 0 || top == limit(h, file)
}

/// The pixels the focused window's page shows, as last presented.
fn pixels(h: &mut Harness) -> (PixelBufferId, u32, u32, Vec<u8>) {
    let scene = h.scene();
    let placed = scene.placements.first();
    let placed = placed.unwrap_or_else(|| panic!("a placement: {}", scene.snapshot()));
    let (w, h, px) = h.pixels(placed.buffer).expect("the sink has the buffer");
    (placed.buffer, w, h, px)
}

/// A pointer event at a pixel of the focused window's area.
fn pointer(h: &mut Harness, kind: PointerKind, (x, y): (f32, f32)) {
    let w = h.focused().unwrap();
    let at = h.placements().into_iter().find(|p| p.window == w).unwrap();
    let (r0, c0) = (at.row, at.col + h.surface_area(w).unwrap().text_x);
    let (cw, ch) = (CELL.width as f32, CELL.height as f32);
    let errors = h.send(Input::Pointer(PointerEvent {
        kind,
        row: r0 + (y / ch) as usize,
        col: c0 + (x / cw) as usize,
        clicks: u8::from(!matches!(kind, PointerKind::Scroll { .. })),
        mods: Mods::default(),
        px: Some((x + c0 as f32 * cw, y + r0 as f32 * ch)),
    }));
    assert!(errors.is_empty(), "{kind:?} at ({x}, {y}): {errors:?}");
}

/// The middle of a rectangle of the page, in the window.
fn middle(h: &mut Harness, file: BufferId, r: Rect) -> (f32, f32) {
    let top = top(h, file) as f32;
    let at = (r.x + r.w / 2.0, r.y + r.h / 2.0 - top);
    assert!((0.0..AREA.1 as f32).contains(&at.1), "in view: {r:?}");
    at
}

/// A left click on the middle of a rectangle of the page.
fn click(h: &mut Harness, file: BufferId, r: Rect) {
    let at = middle(h, file, r);
    pointer(h, PointerKind::Press(Button::Left), at);
    pointer(h, PointerKind::Release(Button::Left), at);
}

fn wheel(h: &mut Harness, dy: f32) {
    pointer(h, PointerKind::Scroll { dx: 0.0, dy }, (50.0, 50.0));
}

/// `<Tab>` until the link to `dest` is ringed.
fn ring(h: &mut Harness, file: BufferId, dest: &str) {
    let (want, _) = link(h, file, dest);
    for _ in 0..ask(h, file, |p| p.links().len()) {
        if seen(h, file).unwrap().ring == Some(want) {
            return;
        }
        keys(h, "<Tab>");
    }
    assert_eq!(seen(h, file).unwrap().ring, Some(want), "{dest}");
}

fn rendered_in_place(ctx: &mut Ctx, buf: BufferId) -> bool {
    ctx.with_mode(buf, |v: &mut View, _| v.is_rendered())
        .unwrap_or(false)
}

/// The focused window's status line.
fn status(h: &mut Harness) -> String {
    let w = h.focused().unwrap();
    let at = h.placements().into_iter().find(|p| p.window == w).unwrap();
    let rows = h.surface_area(w).unwrap().rows;
    let row = h.scene().row_text(at.row + rows);
    // The state's indicator is at the row's right end, after the gap.
    let row = row.trim();
    row.rsplit_once("  ")
        .map_or(row, |(s, _)| s)
        .trim_end()
        .to_string()
}

#[test]
fn the_toggle_shows_the_page_where_the_cursor_was_and_the_file_again() {
    let dir = tree("toggle");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let file = open(&mut h, &dir.join("a.md"));
    let w = h.focused().unwrap();
    keys(&mut h, "41G");
    let cursor = h.text(file).unwrap().cursor(w).unwrap();
    let (row, _) = h.text(file).unwrap().screen_pos(w, cursor).unwrap();
    let raw_top = h.top_line(w);
    assert!(row > 0, "the cursor is down the window");

    keys(&mut h, "\\m");
    let page = seen(&mut h, file).expect("a page");
    assert_eq!(h.focused_buffer(), Some(page.buffer));
    assert_eq!(h.buffer_name(page.buffer), Some("a.md *page*"));
    assert_eq!(page.area, AREA);
    assert!(status(&mut h).starts_with("a.md *page*  "));
    // It's on screen: placed over the window's area, presented, and a picture of something.
    let (id, wide, tall, px) = pixels(&mut h);
    assert_eq!((Some(id), wide, tall), (page.pixels, AREA.0, AREA.1));
    let placed = h.scene().placements[0];
    assert_eq!((placed.cols, placed.rows, placed.z), (60, 20, -1));
    let canvas = die_markdown::theme_palette(&h).canvas;
    let paper = [canvas.0, canvas.1, canvas.2, 255];
    assert!(px.chunks_exact(4).all(|p| p[3] == 255), "opaque all over");
    assert!(px.chunks_exact(4).any(|p| p != paper), "and not all paper");
    // The cursor's line is at the height the cursor's row had.
    let line = h.text(file).unwrap().line_start(40);
    let want = ask(&mut h, file, |p| p.y_of(line)) - (row as u32 * CELL.height) as f32;
    assert!((page.top as f32 - want).abs() < 1.0, "{} {want}", page.top);
    assert!(page.top > 0 && page.top < limit(&mut h, file));
    assert!(h.text(file).unwrap().decorations().next().is_none());
    assert!(!h.text(file).unwrap().settings().read_only);

    // Back, with nothing scrolled: the cursor and the scroll are where they were, and the page
    // is gone, its pixel buffer with it.
    keys(&mut h, "\\m");
    assert_eq!(h.focused_buffer(), Some(file));
    assert_eq!(h.text(file).unwrap().cursor(w), Some(cursor));
    assert_eq!(h.top_line(w), raw_top);
    assert_eq!(seen(&mut h, file), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);
    assert!(h.pixels(id).is_none());

    // Scrolled, the way back puts the cursor on the line at the page's top, at the window's.
    keys(&mut h, "\\m<C-f>");
    let top = top(&mut h, file);
    assert_ne!(top, page.top);
    let source = ask(&mut h, file, |p| p.source_at(top as f32));
    keys(&mut h, "\\m");
    let t = h.text(file).unwrap();
    let line = t.line_of(source);
    assert_eq!(t.cursor(w).map(|c| t.line_of(c)), Some(line));
    assert_eq!(h.top_line(w), Some(line));

    // A second window on the file shows the same page, and the page outlives the first to go.
    keys(&mut h, "<C-w>v\\m<C-w>l\\m");
    let page = seen(&mut h, file).unwrap();
    let windows = h.window_list();
    assert_eq!(windows.len(), 2);
    assert!(
        windows
            .iter()
            .all(|w| h.window_buffer(*w) == Some(page.buffer))
    );
    keys(&mut h, "\\m");
    assert_eq!(seen(&mut h, file).map(|p| p.buffer), Some(page.buffer));
    keys(&mut h, "<C-w>h\\m");
    assert_eq!(seen(&mut h, file), None);
}

#[test]
fn keys_and_the_wheel_scroll_the_page_between_its_ends() {
    let dir = tree("scroll");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    let end = limit(&mut h, file);
    assert_eq!(
        top(&mut h, file),
        0,
        "the file's first line is the page's top"
    );
    assert!(end > 3000, "the page is long: {end}");
    assert!(status(&mut h).ends_with("Top"));

    // A step is 60 CSS pixels, painted and presented once, the area and no more.
    let (id, _, _, before) = pixels(&mut h);
    h.fake_pixels().take_calls();
    keys(&mut h, "j");
    assert_eq!(top(&mut h, file), 120);
    let whole = PixelRect::new(0, 0, AREA.0, AREA.1);
    assert_eq!(
        h.fake_pixels().take_calls(),
        [PixelSinkCall::Present(id, whole)]
    );
    assert_ne!(pixels(&mut h).3, before);
    assert_eq!(status(&mut h), format!("a.md *page*  {}%", 120 * 100 / end));
    let moves = [
        ("3j", 480),
        ("k", 360),
        ("<Down>", 480),
        ("<Up>", 360),
        ("<C-e>", 480),
        ("<C-y>", 360),
        ("<C-d>", 560),
        ("<C-u>", 360),
        ("2<C-d>", 760),
        ("2<C-u>", 360),
        ("<C-f>", 710),
        ("<C-b>", 360),
        ("<Space>", 710),
        ("<PageUp>", 360),
        ("<PageDown>", 710),
        ("gg", 0),
        ("10j", 1200),
        ("12k", 0),
        ("G", end),
        ("<Home>", 0),
        ("<End>", end),
    ];
    for (key, to) in moves {
        keys(&mut h, key);
        assert_eq!(top(&mut h, file), to, "{key}");
    }
    assert!(status(&mut h).ends_with("Bot"));
    // Shift and Space is Space to a keymap, and goes back, as a browser's does.
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    keys(&mut h, "gg<Space>");
    let errors = h.send(Input::Key(KeyEvent::press(Key::Char(' ')).mods(shift)));
    assert_eq!((top(&mut h, file), errors), (0, vec![]));
    // Held at both ends, where a key paints nothing.
    h.fake_pixels().take_calls();
    keys(&mut h, "k");
    assert_eq!(top(&mut h, file), 0);
    keys(&mut h, "G");
    assert_eq!(h.fake_pixels().take_calls().len(), 1, "the way down, only");
    keys(&mut h, "j");
    assert_eq!(top(&mut h, file), end);
    assert_eq!(h.fake_pixels().take_calls(), []);
    keys(&mut h, "gg");

    // The wheel is in cells: a cell and a half is 30 pixels, and a fraction of a pixel waits.
    let before = pixels(&mut h).3;
    wheel(&mut h, 1.5);
    assert_eq!(top(&mut h, file), 30);
    assert_ne!(pixels(&mut h).3, before);
    wheel(&mut h, 0.02);
    wheel(&mut h, 0.02);
    assert_eq!(top(&mut h, file), 30);
    wheel(&mut h, 0.02);
    assert_eq!(top(&mut h, file), 31);
    wheel(&mut h, -9.0);
    assert_eq!(top(&mut h, file), 0);
    wheel(&mut h, 1e6);
    assert_eq!(top(&mut h, file), end);
    // A delta past any number goes to the end and carries nothing: the next one still counts.
    wheel(&mut h, 3e37);
    assert_eq!(top(&mut h, file), end);
    wheel(&mut h, -1.0);
    assert_eq!(top(&mut h, file), end - 20);

    // Headings: the next one down, a count's worth, and back.
    keys(&mut h, "gg]]");
    assert!(at_heading(&mut h, file, "part-1"), "{}", top(&mut h, file));
    keys(&mut h, "3]]");
    assert!(at_heading(&mut h, file, "part-4"));
    keys(&mut h, "[[");
    assert!(at_heading(&mut h, file, "part-3"));
    assert!(top(&mut h, file) > 0 && top(&mut h, file) < end);

    // A key the page doesn't bind is undefined, and the ones every buffer has still work.
    assert_eq!(fails(&mut h, "x"), "x is undefined");
    keys(&mut h, "\\z");
    assert_eq!(seen(&mut h, file).unwrap().area.0, 60 * CELL.width);
    keys(&mut h, ":markdown-toggle<CR>");
    assert_eq!(h.focused_buffer(), Some(file));
}

#[test]
fn a_short_page_a_taller_window_a_new_scale_and_a_shrinking_text() {
    let dir = tree("short");
    fs::write(dir.join("s.md"), "# Small\n\nOne line.\n").unwrap();
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());

    // A page shorter than the window is all there is: it doesn't scroll, and a key paints
    // nothing.
    let small = open(&mut h, &dir.join("s.md"));
    keys(&mut h, "\\m");
    assert_eq!(limit(&mut h, small), 0);
    assert!(status(&mut h).ends_with("All"), "{}", status(&mut h));
    h.fake_pixels().take_calls();
    keys(&mut h, "j<C-f>G");
    assert_eq!(top(&mut h, small), 0);
    assert_eq!(h.fake_pixels().take_calls(), []);
    keys(&mut h, "\\m");

    // A taller window at the page's end: the end stays at the window's bottom.
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\mG");
    let end = limit(&mut h, file);
    h.set_window_size(60, 30);
    h.run_until_idle();
    assert_eq!(seen(&mut h, file).unwrap().area, (600, 600));
    assert_eq!(top(&mut h, file), limit(&mut h, file));
    assert!(top(&mut h, file) < end);

    // A new scale lays the page out again, the line at the window's top kept.
    keys(&mut h, "gg5j");
    let (was, line) = (seen(&mut h, file).unwrap(), line_at_top(&mut h, file));
    h.set_cell_pixels(CellPixels { scale: 1.0, ..CELL });
    h.run_until_idle();
    let now = seen(&mut h, file).unwrap();
    assert_ne!(now.height, was.height, "laid out for the new scale");
    assert_eq!(line_at_top(&mut h, file), line);
    assert_eq!(now.area, was.area);

    // The text shrinks under a page at its end: the page comes up with it.
    keys(&mut h, "G");
    let len = h.text(file).unwrap().len();
    let t = h.text_mut(file).unwrap();
    t.tx(Origin::User).delete(200..len).commit().unwrap();
    h.run_until_idle();
    let page = seen(&mut h, file).unwrap();
    assert!(page.height < now.height);
    assert_eq!(page.top, limit(&mut h, file));
}

#[test]
fn two_windows_of_unequal_size_show_one_picture_as_large_as_the_largest() {
    let dir = tree("two");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m<C-w>s");
    let page = seen(&mut h, file).unwrap();
    let rows: Vec<usize> = h.scene().placements.iter().map(|p| p.rows).collect();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0], rows[1], "the rows split unevenly");
    let tallest = *rows.iter().max().unwrap();
    let tall = tallest as u32 * CELL.height;
    assert_eq!(page.area, (AREA.0, tall));

    // A step paints and presents the largest area, so neither window shows stale rows.
    let id = pixels(&mut h).0;
    h.fake_pixels().take_calls();
    keys(&mut h, "5j");
    assert_eq!(top(&mut h, file), 600);
    assert_eq!(
        h.fake_pixels().take_calls(),
        [PixelSinkCall::Present(
            id,
            PixelRect::new(0, 0, AREA.0, tall)
        )]
    );
    // What the taller window shows is what a window of its size alone shows at that place.
    let (_, wide, _, px) = pixels(&mut h);
    let shown = &px[..(wide * tall * 4) as usize];
    let mut alone = harness(Options::default());
    alone.set_window_size(60, tallest);
    alone.run_until_idle();
    let same = open(&mut alone, &dir.join("a.md"));
    keys(&mut alone, "\\m5j");
    assert_eq!(seen(&mut alone, same).unwrap().area, (AREA.0, tall));
    let (_, w, t, reference) = pixels(&mut alone);
    assert_eq!((w, t), (wide, tall));
    assert!(shown == reference, "the picture is the one window's");

    // The taller window leaves the page: at the command's end the picture is the other's size.
    let w = h.focused().unwrap();
    let focused_rows = h.surface_area(w).unwrap().rows;
    assert_eq!(focused_rows, tallest, "the split focused the taller window");
    keys(&mut h, "\\m");
    let shorter = (tallest.min(rows[0]).min(rows[1])) as u32 * CELL.height;
    assert_eq!(seen(&mut h, file).unwrap().area, (AREA.0, shorter));
    assert_eq!(h.window_list().len(), 2);
}

#[test]
fn links_ring_from_what_the_window_shows_and_go_round() {
    let dir = tree("ring");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let a = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    let n = ask(&mut h, a, |p| p.links().len());
    assert!(n >= 9, "{n} links");

    // Scrolled so that a link's line is at the window's top: the links before are above it.
    let (third, r3) = link(&mut h, a, "notes.txt");
    wheel(&mut h, r3.y / CELL.height as f32);
    let top = top(&mut h, a);
    assert!(top > 0 && top as f32 <= r3.y);
    let above = ask(&mut h, a, |p| {
        let gone = |l: &die_markdown::typeset::LinkBox| l.rects[0].y + l.rects[0].h <= top as f32;
        p.links()[..third].iter().all(gone)
    });
    assert!(above, "the links before are out of view");

    // `<Tab>` from none rings the first link in view, and `<S-Tab>` the one before it, up
    // where it is, the gap over it.
    keys(&mut h, "<Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, Some(third));
    assert!(status(&mut h).ends_with("notes.txt"));
    let (_, r2) = link(&mut h, a, "b.md:5");
    keys(&mut h, "<S-Tab>");
    let page = seen(&mut h, a).unwrap();
    assert_eq!((page.ring, page.top), (Some(third - 1), over(r2)));
    // From none, `<S-Tab>` starts from the last link in view.
    keys(&mut h, "<Esc>");
    assert_eq!(seen(&mut h, a).unwrap().ring, None);
    let bottom = page.top as f32 + AREA.1 as f32;
    let last_seen = ask(&mut h, a, |p| {
        p.links().iter().rposition(|l| l.rects[0].y < bottom)
    });
    assert!(last_seen.is_some_and(|i| i < n - 1), "{last_seen:?}");
    keys(&mut h, "<S-Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, last_seen);

    // A ringed link out of view is no start: the first in view is. Past the last link is the
    // first, and before the first is the last.
    keys(&mut h, "G<Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, Some(n - 1));
    let (_, r0) = link(&mut h, a, "#part-15");
    keys(&mut h, "<Tab>");
    let page = seen(&mut h, a).unwrap();
    assert_eq!((page.ring, page.top), (Some(0), over(r0)));
    let (_, last) = link(&mut h, a, "#title");
    keys(&mut h, "<S-Tab>");
    let page = seen(&mut h, a).unwrap();
    let under = (last.y + last.h + GAP * CELL.scale as f32 - AREA.1 as f32).round() as i32;
    let end = limit(&mut h, a);
    assert_eq!((page.ring, page.top), (Some(n - 1), under.clamp(0, end)));

    // A count goes round the links, however large.
    keys(&mut h, "3<Tab>");
    let page = seen(&mut h, a).unwrap();
    assert_eq!((page.ring, page.top), (Some(2), over(r2)));
    let started = Instant::now();
    keys(&mut h, "2000000000<Tab>");
    assert_eq!(
        seen(&mut h, a).unwrap().ring,
        Some((2 + 2000000000 % n) % n)
    );
    keys(&mut h, "2000000000<S-Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, Some(2));
    assert!(started.elapsed().as_secs() < 2, "{:?}", started.elapsed());
}

#[test]
fn a_click_lands_where_the_page_is_and_a_follow_keeps_what_it_should() {
    let dir = tree("click");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let a = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    let page = seen(&mut h, a).unwrap();

    // A click on a link, on a page scrolled down a little, is a click on that link: it
    // scrolls to the heading, and `C-o` comes back to the scrolled place.
    let (first, r0) = link(&mut h, a, "#part-15");
    wheel(&mut h, r0.y / 2.0 / CELL.height as f32);
    let left = top(&mut h, a);
    assert!(left > 0);
    click(&mut h, a, r0);
    let there = seen(&mut h, a).unwrap();
    assert!(there.top > left && at_heading(&mut h, a, "part-15"));
    assert_eq!(
        there.ring,
        Some(first),
        "the link clicked is the one ringed"
    );
    keys(&mut h, "<C-o>");
    assert_eq!(top(&mut h, a), left);
    keys(&mut h, "L");
    assert_eq!(top(&mut h, a), there.top);
    keys(&mut h, "H");
    assert_eq!(top(&mut h, a), left);
    assert_eq!(fails(&mut h, "<C-o>"), "No older place");

    // A press on one link and a release on another follows nothing.
    keys(&mut h, "gg");
    let (_, r1) = link(&mut h, a, "b.md#second-part");
    let (down, up) = (middle(&mut h, a, r0), middle(&mut h, a, r1));
    pointer(&mut h, PointerKind::Press(Button::Left), down);
    pointer(&mut h, PointerKind::Release(Button::Left), up);
    h.run_until_idle();
    assert_eq!(h.focused_buffer(), Some(page.buffer));
    assert_eq!(top(&mut h, a), 0);
    // A click off a link takes the ring off, as `<Esc>` does.
    click(
        &mut h,
        a,
        Rect {
            x: 1.0,
            y: 1.0,
            w: 2.0,
            h: 2.0,
        },
    );
    assert_eq!(seen(&mut h, a).unwrap().ring, None);

    // A heading the page hasn't got is said, and leaves no place behind; a file that isn't
    // there is said, and nothing opens.
    ring(&mut h, a, "#nowhere");
    assert_eq!(fails(&mut h, "<CR>"), "No heading #nowhere");
    assert_eq!(fails(&mut h, "<C-o>"), "No older place");
    ring(&mut h, a, "gone.md");
    let said = fails(&mut h, "<CR>");
    assert!(said.starts_with("No file "), "{said}");
    assert_eq!(h.focused_buffer(), Some(page.buffer));
    assert_eq!(h.find_buffer("gone.md"), None);

    // A new follow forgets the way forward, as a browser does: until then `L` still goes.
    keys(&mut h, "L");
    assert!(at_heading(&mut h, a, "part-15"));
    keys(&mut h, "<C-o>");
    ring(&mut h, a, "#title");
    keys(&mut h, "<CR>");
    assert!(at_heading(&mut h, a, "title"));
    assert_eq!(fails(&mut h, "L"), "No newer place");
}

#[test]
fn a_link_goes_where_it_leads_and_the_way_back_is_kept() {
    let dir = tree("links");
    let said = dir.join("opened");
    let opener = dir.join("opener");
    let script = format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", said.display());
    fs::write(&opener, script).unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("a.md"), document()).unwrap();
    fs::write(dir.join("b.md"), second()).unwrap();
    fs::write(dir.join("notes.txt"), "plain\ntext\n").unwrap();
    let options = Options {
        opener: Some(opener.display().to_string()),
        ..Options::default()
    };
    let mut h = harness(options);
    let a = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    let page = seen(&mut h, a).unwrap();

    // `<Tab>` rings the first link in view, then the next, and `<S-Tab>` the one before.
    let (first, _) = link(&mut h, a, "#part-15");
    keys(&mut h, "<Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, Some(first));
    let ringed = pixels(&mut h).3;
    keys(&mut h, "<Tab><Tab><S-Tab>");
    assert_eq!(seen(&mut h, a).unwrap().ring, Some(first + 1));
    assert_ne!(pixels(&mut h).3, ringed, "the ring is drawn");

    // A markdown file opens as its page, in this window, at the heading the link names.
    ring(&mut h, a, "b.md#second-part");
    let left = top(&mut h, a);
    keys(&mut h, "<CR>");
    let b = h.find_buffer("b.md").unwrap();
    let other = seen(&mut h, b).expect("the other file's page");
    assert_eq!(h.focused_buffer(), Some(other.buffer));
    assert_eq!(h.buffer_name(other.buffer), Some("b.md *page*"));
    assert!(other.top > 0 && at_heading(&mut h, b, "second-part"));
    // The page left is put away: no buffer and no picture, once the command is over.
    assert_eq!(seen(&mut h, a), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);
    assert!(h.pixels(page.pixels.unwrap()).is_none());
    // `C-o` there comes back here, to the place left, the link still ringed, and b's page is
    // put away in its turn.
    keys(&mut h, "<C-o>");
    let back = seen(&mut h, a).expect("a page again");
    assert_eq!(h.focused_buffer(), Some(back.buffer));
    assert_eq!((back.top, back.area), (left, AREA));
    assert_eq!(Some(pixels(&mut h).0), back.pixels);
    assert_eq!(back.ring, Some(first + 1));
    assert_eq!(h.find_buffer("b.md *page*"), None);
    // And `L` goes there again, where it was.
    keys(&mut h, "L");
    assert_eq!(top(&mut h, b), other.top);
    keys(&mut h, "H");

    // A line of a markdown file: its page, with that line at the top, whole. Line 0 is line 1.
    ring(&mut h, a, "b.md:5");
    keys(&mut h, "<CR>");
    assert_eq!(h.focused_buffer(), seen(&mut h, b).map(|p| p.buffer));
    let line = h.text(b).unwrap().line_start(4);
    let (want, top) = (ask(&mut h, b, |p| p.y_of(line)), top(&mut h, b));
    assert!((top as f32 - want).abs() < 1.0, "{top} {want}");
    assert_eq!(line_at_top(&mut h, b), line);
    keys(&mut h, "<C-o>");
    ring(&mut h, a, "b.md:0");
    keys(&mut h, "<CR>");
    assert_eq!(h.focused_buffer(), seen(&mut h, b).map(|p| p.buffer));
    assert_eq!(seen(&mut h, b).unwrap().top, 0);
    keys(&mut h, "<C-o>");

    // Any other file opens as text, and this page is put away: the toggle on the file brings
    // it back, at the line it showed.
    ring(&mut h, a, "notes.txt");
    let line = line_at_top(&mut h, a);
    keys(&mut h, "<CR>");
    let plain = h.focused_buffer().unwrap();
    assert_eq!(h.buffer_name(plain), Some("notes.txt"));
    assert!(h.text(plain).is_ok());
    assert_eq!(seen(&mut h, a), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);
    h.show(a, ShowIn::Current).unwrap();
    h.run_until_idle();
    keys(&mut h, "\\m");
    assert_eq!(line_at_top(&mut h, a), line);
    assert_eq!(Some(pixels(&mut h).0), seen(&mut h, a).unwrap().pixels);

    // A web address goes to the opener, and DIE shows nothing new.
    let here = seen(&mut h, a).unwrap().buffer;
    ring(&mut h, a, "https://example.com/x");
    keys(&mut h, "<CR>");
    h.run_until_idle();
    assert_eq!(fs::read_to_string(&said).unwrap(), "https://example.com/x");
    assert_eq!(h.focused_buffer(), Some(here));

    // With no link ringed, `<CR>` says how to ring one.
    keys(&mut h, "<Esc>");
    assert_eq!(seen(&mut h, a).unwrap().ring, None);
    assert!(fails(&mut h, "<CR>").contains("<Tab>"));
}

#[test]
fn the_history_outlives_the_page_and_steps_over_a_dead_file() {
    let dir = tree("history");
    fs::write(dir.join("a.md"), document()).unwrap();
    fs::write(dir.join("b.md"), second()).unwrap();
    let mut h = harness(Options::default());
    let a = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");

    // A look at the file and back forgets nothing: `C-o` still comes back.
    ring(&mut h, a, "#part-15");
    keys(&mut h, "<CR>");
    assert!(at_heading(&mut h, a, "part-15"));
    let line = line_at_top(&mut h, a);
    keys(&mut h, "\\m\\m");
    assert_eq!(line_at_top(&mut h, a), line, "the toggle keeps the line");
    keys(&mut h, "<C-o>");
    assert_eq!(top(&mut h, a), 0);

    // Across pages too: the way back to the page a link led from survives a look at the file
    // it led to.
    keys(&mut h, "3j");
    ring(&mut h, a, "b.md#second-part");
    let left = top(&mut h, a);
    keys(&mut h, "<CR>");
    let b = h.find_buffer("b.md").unwrap();
    let line = line_at_top(&mut h, b);
    keys(&mut h, "\\m\\m");
    assert_eq!(line_at_top(&mut h, b), line);
    keys(&mut h, "<C-o>");
    let back = seen(&mut h, a).expect("a's page again");
    assert_eq!((h.focused_buffer(), back.top), (Some(back.buffer), left));

    // A place in a file whose buffer is gone is stepped over, quietly.
    ring(&mut h, a, "b.md#second-part");
    keys(&mut h, "<CR>");
    assert_eq!(h.focused_buffer(), seen(&mut h, b).map(|p| p.buffer));
    h.kill(a);
    h.run_until_idle();
    assert_eq!(fails(&mut h, "<C-o>"), "No older place");
    assert!(h.take_errors().is_empty());
}

#[test]
fn the_top_stays_the_top_across_a_new_layout_and_a_place_comes_back_to_the_pixel() {
    let dir = tree("top");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    h.set_window_size(30, 20);
    h.run_until_idle();
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    assert_eq!(top(&mut h, file), 0);

    // The window widens, and the page's padding may grow with it: the top is still the top.
    h.set_window_size(100, 20);
    h.run_until_idle();
    assert_eq!(seen(&mut h, file).unwrap().area, (1000, 400));
    assert_eq!(top(&mut h, file), 0);
    assert!(status(&mut h).ends_with("Top"), "{}", status(&mut h));
    // A line typed over the first one: the top shows it.
    let t = h.text_mut(file).unwrap();
    let tx = t.tx(Origin::User).insert(0, "Another first line.\n\n");
    tx.commit().unwrap();
    h.run_until_idle();
    assert_eq!(top(&mut h, file), 0);
    assert_eq!(line_at_top(&mut h, file), 0);

    // Down the page, a place goes to another width and comes back to the pixel, time and
    // again, though the two layouts put the line at different fractions of a pixel.
    keys(&mut h, "10j");
    assert_eq!(top(&mut h, file), 1200);
    let line = line_at_top(&mut h, file);
    for _ in 0..4 {
        h.set_window_size(59, 20);
        h.run_until_idle();
        assert_eq!(line_at_top(&mut h, file), line);
        h.set_window_size(100, 20);
        h.run_until_idle();
        assert_eq!(top(&mut h, file), 1200);
    }
}

#[test]
fn the_page_follows_the_text_and_the_width_and_goes_with_the_file() {
    let dir = tree("follows");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = harness(Options::default());
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m10j");
    let was = seen(&mut h, file).unwrap();
    let source = ask(&mut h, file, |p| p.source_at(was.top as f32));
    let under = was.top as f32 - ask(&mut h, file, |p| p.y_of(source));
    let (_, at) = link(&mut h, file, "#part-15");
    let id = pixels(&mut h).0;

    // The text changes under the page, above what the window shows: the page is laid out again,
    // and the line at the window's top is the line that was there.
    let added = "A line before the rest.\n\n";
    let t = h.text_mut(file).unwrap();
    t.tx(Origin::User).insert(0, added).commit().unwrap();
    h.run_until_idle();
    let now = seen(&mut h, file).unwrap();
    assert!(now.height > was.height, "the page grew by a paragraph");
    assert!(
        now.top > was.top,
        "and the reader's place went down with it"
    );
    let source = source + added.len();
    let (shown, y) = ask(&mut h, file, |p| {
        (p.source_at(now.top as f32), p.y_of(source))
    });
    assert_eq!(shown, source);
    assert!((now.top as f32 - y - under).abs() < 1.0, "to the pixel");
    let wide = link(&mut h, file, "#part-15").1;
    assert_ne!(wide, at);

    // A narrower window: laid out again for its width, in the same buffer, with the place kept.
    h.fake_pixels().take_calls();
    h.set_window_size(40, 20);
    h.run_until_idle();
    let narrow = seen(&mut h, file).unwrap();
    assert_eq!(narrow.area, (400, 400));
    let laid = link(&mut h, file, "#part-15").1;
    assert_ne!((narrow.height, laid), (now.height, wide));
    // A narrower page breaks its lines elsewhere, so the line is asked for by a byte it shows.
    let y = ask(&mut h, file, |p| p.y_of(source));
    assert!((narrow.top as f32 - y - under).abs() < 1.0, "the same line");
    let (same, wide, ..) = pixels(&mut h);
    assert_eq!(
        (same, wide),
        (id, AREA.0),
        "the buffer grows and never shrinks"
    );
    let calls = h.fake_pixels().take_calls();
    let area = PixelSinkCall::Present(id, PixelRect::new(0, 0, 400, 400));
    assert!(calls.contains(&area), "{calls:?}");
    assert!(!calls.iter().any(|c| matches!(c, PixelSinkCall::Resize(..))));

    // The theme changes: the colours are read again when a command ends.
    let paper = die_core::Rgb::hex("#204060");
    let theme = die_core::Theme::default();
    let look = die_core::Style::new().fg(die_core::Rgb::hex("#ffffff"));
    h.set_theme(theme.face("default", look.bg(paper)));
    keys(&mut h, "gg");
    let px = pixels(&mut h).3;
    assert_eq!(px[..4], [0x20, 0x40, 0x60, 255]);

    // The file's buffer goes, and its page with it.
    h.kill(file);
    h.run_until_idle();
    assert_eq!(seen(&mut h, file), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);
    assert!(h.pixels(id).is_none());
    assert!(h.take_errors().is_empty());
}

#[test]
fn a_theme_that_styles_only_the_base_faces_gives_the_quiet_roles_its_shadow() {
    let mut h = harness(Options::default());
    h.set_theme(die_core::Theme::default());
    let palette = die_markdown::theme_palette(&h);
    let shadow = h.face_style("shadow").unwrap().fg.unwrap();
    let shadow = die_markdown::typeset::Rgb(shadow.0, shadow.1, shadow.2);
    assert_ne!(palette.text, shadow);
    assert_eq!(palette.muted, shadow, "muted");
    assert_eq!(palette.quote, shadow, "quote");
    assert_eq!(palette.subtle, shadow, "subtle");
    assert_eq!(palette.border, shadow, "border");
}

/// A harness on a state directory, as a restart finds one.
fn up(state: &Path) -> Harness {
    let config = Config {
        register: Some(Box::new(bindings)),
        session: Some(state.to_path_buf()),
        ..Config::default()
    };
    let mut h = Harness::with_config(extensions(Options::default()), config).unwrap();
    h.resume_session();
    h.set_window_size(60, 20);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

/// What the extension keeps, without the buffers' ids, which a restart changes.
fn kept(h: &mut Harness) -> Vec<String> {
    let kept = h.with_ext(|m: &mut Markdown, ctx| m.save_session(ctx));
    let values = kept.unwrap().buffers.into_iter();
    values.map(|(_, value)| value.to_string()).collect()
}

#[test]
fn a_restart_brings_the_page_back_where_it_was() {
    let dir = tree("restart");
    let state = tree("restart-state");
    fs::write(dir.join("a.md"), document()).unwrap();
    let mut h = up(&state);
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m12j");
    let was = seen(&mut h, file).unwrap();
    let source = ask(&mut h, file, |p| p.source_at(was.top as f32));
    let before = kept(&mut h);
    assert_eq!(before.len(), 1, "{before:?}");

    // Twice in a row, as `:rebuild` after `:rebuild`.
    for _ in 0..2 {
        h.hand_over_session().unwrap();
        drop(h);
        h = up(&state);
        let file = h.find_buffer("a.md").expect("the file is back");
        let page = seen(&mut h, file).expect("and its page");
        assert_eq!(h.focused_buffer(), Some(page.buffer));
        assert_eq!(Some(pixels(&mut h).0), page.pixels);
        assert_eq!(page.area, AREA);
        // The same line is at the window's top, and it keeps as the same session.
        assert_eq!(ask(&mut h, file, |p| p.source_at(page.top as f32)), source);
        assert_eq!(kept(&mut h), before);
        let lost = |m: &&String| m.contains("dropped") || m.starts_with("markdown:");
        assert_eq!(h.messages.iter().find(lost), None);
    }
    // It's a working page: the toggle goes back to the file, at the line it showed.
    let file = h.find_buffer("a.md").unwrap();
    keys(&mut h, "\\m");
    assert_eq!(h.focused_buffer(), Some(file));
    let (w, t) = (h.focused().unwrap(), h.text(file).unwrap());
    assert_eq!(t.cursor(w).map(|c| t.line_of(c)), Some(t.line_of(source)));
}

#[test]
fn a_page_put_away_is_a_place_the_session_keeps_and_the_next_page_starts_at() {
    let dir = tree("away");
    let state = tree("away-state");
    fs::write(dir.join("a.md"), document()).unwrap();
    fs::write(dir.join("notes.txt"), "plain\ntext\n").unwrap();
    let mut h = up(&state);
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m7j");
    ring(&mut h, file, "notes.txt");
    let line = line_at_top(&mut h, file);
    assert!(line > 0);
    keys(&mut h, "<CR>");
    assert_eq!(seen(&mut h, file), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);

    // What the session keeps of it is the place, a plain byte, and a restart keeps it so,
    // making no page.
    assert_eq!(kept(&mut h), [line.to_string()]);
    h.hand_over_session().unwrap();
    drop(h);
    h = up(&state);
    let file = h.find_buffer("a.md").unwrap();
    assert_eq!(seen(&mut h, file), None);
    assert_eq!(h.find_buffer("a.md *page*"), None);
    assert_eq!(kept(&mut h), [line.to_string()]);

    // The toggle on the file brings the page back at that line, the cursor notwithstanding,
    // and from then on the toggle is the cursor's as ever.
    h.show(file, ShowIn::Current).unwrap();
    keys(&mut h, "G\\m");
    assert_eq!(line_at_top(&mut h, file), line);
    assert_eq!(kept(&mut h).len(), 1);
    assert!(kept(&mut h)[0].starts_with('{'), "{:?}", kept(&mut h));
    keys(&mut h, "\\m");
    let (w, t) = (h.focused().unwrap(), h.text(file).unwrap());
    assert_eq!(t.cursor(w).map(|c| t.line_of(c)), Some(t.line_of(line)));
    keys(&mut h, "gg\\m");
    assert_eq!(top(&mut h, file), 0);
}

#[test]
fn a_file_has_one_rendered_view_at_a_time() {
    let dir = tree("one-view");
    fs::write(dir.join("a.md"), document()).unwrap();
    fs::write(dir.join("b.md"), second()).unwrap();
    // A session a build before the page wrote, with both files rendered in place.
    let in_place = Options {
        page: false,
        ..Options::default()
    };
    let mut h = harness(in_place);
    open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    open(&mut h, &dir.join("b.md"));
    keys(&mut h, "\\m");
    let older = h.session_json();

    // Restored with the page on, both are in place. A link from a's page to b's text gives
    // b's page, and b's view in place goes: its text is raw and its own again.
    let mut h = harness(Options::default());
    h.restore_session(&older).unwrap();
    h.run_until_idle();
    let (a, b) = (
        h.find_buffer("a.md").unwrap(),
        h.find_buffer("b.md").unwrap(),
    );
    assert!(rendered_in_place(&mut h, a) && rendered_in_place(&mut h, b));
    h.show(a, ShowIn::Current).unwrap();
    keys(&mut h, "\\m\\m");
    assert!(seen(&mut h, a).is_some());
    ring(&mut h, a, "b.md#second-part");
    keys(&mut h, "<CR>");
    assert!(seen(&mut h, b).is_some());
    assert!(!rendered_in_place(&mut h, b));
    assert!(h.text(b).unwrap().decorations().next().is_none());
    assert!(!h.text(b).unwrap().settings().read_only);
    // The session has one entry a file: a's place and b's page.
    let kept = kept(&mut h);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept.iter().all(|k| k != "true" && k != "false"), "{kept:?}");
    // And the toggle on b's page lands on raw text.
    keys(&mut h, "\\m");
    assert_eq!(h.focused_buffer(), Some(b));
    assert!(!rendered_in_place(&mut h, b));
}

#[test]
fn a_session_with_the_view_in_place_restores_in_place_and_no_pixels_means_in_place() {
    let dir = tree("in-place");
    fs::write(dir.join("a.md"), document()).unwrap();
    let in_place = Options {
        page: false,
        ..Options::default()
    };

    // `page` off: the toggle is the view in place, and no page is made.
    let mut h = harness(in_place);
    let file = open(&mut h, &dir.join("a.md"));
    keys(&mut h, "\\m");
    assert!(rendered_in_place(&mut h, file));
    assert_eq!(h.focused_buffer(), Some(file));
    assert_eq!(seen(&mut h, file), None);
    assert!(h.scene().placements.is_empty());
    let older = h.session_json();

    // What that keeps is what a build before the page kept. With `page` on it comes back as
    // it was, in place; the toggle then goes raw, and from raw to the page.
    let mut h = harness(Options::default());
    h.restore_session(&older).unwrap();
    h.run_until_idle();
    let file = h.find_buffer("a.md").unwrap();
    assert!(rendered_in_place(&mut h, file));
    assert_eq!((h.focused_buffer(), seen(&mut h, file)), (Some(file), None));
    assert_eq!(h.session_json(), older, "and keeps as the same session");
    keys(&mut h, "\\m");
    assert!(!rendered_in_place(&mut h, file));
    keys(&mut h, "\\m");
    let page = seen(&mut h, file).expect("a page");
    assert_eq!(h.focused_buffer(), Some(page.buffer));
    let with_page = h.session_json();

    // A host that shows no pixels, as Linux for now: a workspace with no sink. A session with
    // a page comes back with the view in place, once the first `Resize` finds no pixels.
    let config = Config {
        register: Some(Box::new(bindings)),
        ..Config::default()
    };
    let mut ctx = Ctx::with_config(extensions(Options::default()), config).unwrap();
    ctx.set_frame_size(60, 22);
    ctx.restore_session(&with_page).unwrap();
    ctx.set_cell_pixels(CELL);
    ctx.run_until_idle();
    assert!(!ctx.pixels_supported());
    let file = ctx.find_buffer("a.md").unwrap();
    assert_eq!(ctx.focused_buffer(), Some(file));
    assert!(rendered_in_place(&mut ctx, file));
    assert_eq!(ctx.find_buffer("a.md *page*"), None);
    // And there the toggle is the one in place.
    ctx.run("markdown-toggle", &Args::default()).unwrap();
    assert!(!rendered_in_place(&mut ctx, file));
    ctx.run("markdown-toggle", &Args::default()).unwrap();
    assert!(rendered_in_place(&mut ctx, file));
    assert_eq!(ctx.focused_buffer(), Some(file));
}

/// What a page costs, printed (`--nocapture`): a window of 160 by 47 cells of 19 by 40 pixels,
/// a Retina display's, on the manual and on the log, the repository's longest document. The
/// manual's are held to loose bounds, since the test profile is lightly optimized and the
/// machine may be busy.
#[test]
fn what_a_page_costs() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut h = harness(Options::default());
    h.set_window_size(160, 47);
    h.set_cell_pixels(CellPixels {
        width: 19,
        height: 40,
        scale: 2.0,
    });
    h.run_until_idle();
    for (name, bound) in [("using-die.md", Some(3000)), ("m0-log.md", None)] {
        let file = open(&mut h, &docs.join(name).canonicalize().unwrap());
        let bytes = h.text(file).unwrap().len();
        let started = Instant::now();
        keys(&mut h, "\\m");
        let shown = started.elapsed();
        let page = seen(&mut h, file).unwrap();
        assert_eq!(page.area, (160 * 19, 47 * 40));
        let started = Instant::now();
        for _ in 0..20 {
            keys(&mut h, "j");
        }
        let step = started.elapsed() / 20;
        let started = Instant::now();
        let t = h.text_mut(file).unwrap();
        t.tx(Origin::User).insert(0, "x").commit().unwrap();
        h.run_until_idle();
        let edit = started.elapsed();
        println!(
            "{name}, {bytes} bytes at {}x{}: shown in {shown:?}, a step {step:?}, an edit {edit:?}",
            page.area.0, page.area.1
        );
        if let Some(bound) = bound {
            assert!(shown.as_millis() < bound, "{shown:?}");
            assert!(step.as_millis() < 500, "{step:?}");
            assert!(edit.as_millis() < bound, "{edit:?}");
        }
        keys(&mut h, "\\m");
        h.text_mut(file).unwrap().set_modified(false);
    }
    // What every command costs while a page is on screen: the theme's colours, read and
    // compared.
    let started = Instant::now();
    for _ in 0..1000 {
        std::hint::black_box(die_markdown::theme_palette(&h));
    }
    println!("the colours read in {:?}", started.elapsed() / 1000);
}
