//! K8: faces, themes and the scene.
//!
//! Test plan:
//! - `child_faces_inherit_what_they_leave_unset`: a face takes unset attributes
//!   from its parent chain, and nothing from `default`.
//! - `a_theme_replaces_a_declared_style_and_keeps_the_parent`
//! - `base_and_ui_faces_inherit_from_their_parents`: `heading` from `bold`,
//!   `line-number` from `shadow`, the UI faces from their base faces.
//! - `bad_face_declarations_are_registration_errors`: a duplicate name, an
//!   unknown parent and a cycle each fail `Ctx::new`.
//! - `a_theme_that_styles_an_undeclared_face_is_reported`: to `*messages*`.
//! - `faces_layer_over_default`: a face that sets only a background keeps
//!   `default`'s foreground.
//! - `overlapping_faces_layer_inner_over_outer_with_providers_on_top`: every
//!   face on a cell applies; a nested decoration is above the one it's in,
//!   whatever order they were added in, and a provider's face is above both.
//! - `highlights_change_only_what_their_faces_set`: a decoration's foreground
//!   and italic survive search, bracket-match and selection, and search's
//!   bold survives selection.
//! - `one_window_snapshot`: the full snapshot of a window with line numbers,
//!   its status line and the command row.
//! - `two_windows_divider_status_lines_and_hollow_cursor`
//! - `nothing_a_window_draws_spills_into_its_neighbour`: a cursor past an
//!   unwrapped window's edge isn't drawn, and a gutter wider than its window
//!   is cut at the window's edge.
//! - `the_prompt_takes_the_command_row_and_the_cursor`
//! - `the_input_cursor_comes_first_and_is_the_only_one_not_hollow`: with and
//!   without a prompt, and with the focused window's cursor hidden.
//! - `cursor_shape_follows_the_state`: block, bar, half block.
//! - `search_matches_and_the_current_match`, `bracket_match`,
//!   `visual_selection_char_line_and_block`: render-time highlights.
//! - `search_matches_dont_overlap_and_the_current_one_starts_at_the_cursor`:
//!   "aa" in "aaaa" with the cursor at 1.
//! - `the_search_highlight_is_a_regex_with_the_callers_case_rule`: `a+b`,
//!   ignoring case and not.
//! - `bracket_match_counts_nested_pairs`: `((a))`.
//! - `only_the_focused_window_draws_the_selection`
//! - `providers_are_asked_for_the_drawn_text_only`: an unwrapped line's first
//!   columns, to a character's end, and nothing when no text fits.
//! - `wide_characters_and_clusters`: continuation cells and graphemes.
//! - `status_line_prompt_and_surface_text_are_laid_out_like_buffer_text`:
//!   clusters, a wide character that doesn't fit, control characters, and a
//!   long prompt showing its end.
//! - `placements_past_the_frame_are_clipped`
//! - `positions_map_cells_to_buffer_positions`
//! - `every_cell_of_a_tab_or_wide_character_maps_to_its_position`
//! - `rgb_hex_and_display`

use die_core::*;
use std::ops::Range;

mod support;

const fn h(s: &str) -> Rgb {
    Rgb::hex(s)
}

fn test_theme() -> Theme {
    Theme::new("test", [h("#000000"); 16])
        .face("default", Style::new().fg(h("#aaaaaa")).bg(h("#000000")))
        .face("shadow", Style::new().fg(h("#555555")))
        .face("bold", Style::new().fg(h("#ffffff")).bold())
        .face("highlight", Style::new().bg(h("#00aa00")))
        .face("line-number-current", Style::new().fg(h("#999999")))
        .face(
            "status-line",
            Style::new().fg(h("#ffffff")).bg(h("#333333")),
        )
        .face(
            "status-line-inactive",
            Style::new().fg(h("#777777")).bg(h("#111111")),
        )
        .face("divider", Style::new().fg(h("#444444")))
        .face("search", Style::new().bg(h("#0000aa")))
        .face("search-current", Style::new().bg(h("#00aaaa")))
        .face("selection", Style::new().bg(h("#aa0000")))
        .face("cursor", Style::new().bg(h("#ffff00")))
}

/// Declares test faces.
struct Faces(Vec<(&'static str, Option<&'static str>, Style)>);

impl Extension for Faces {
    fn name(&self) -> &'static str {
        "faces"
    }
    fn register(&mut self, reg: &mut Registry) {
        for (n, p, s) in &self.0 {
            reg.face(n, *p, *s);
        }
    }
}

fn ctx_with(faces: Vec<(&'static str, Option<&'static str>, Style)>) -> Ctx {
    let mut ctx = Ctx::new(vec![Box::new(Faces(faces))]).unwrap();
    ctx.set_theme(test_theme());
    ctx
}

/// A Ctx with one buffer shown in one focused window.
fn one(text: &str, cols: usize, rows: usize) -> (Ctx, BufferId, WindowId) {
    let mut ctx = ctx_with(vec![]);
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    ctx.focus(w);
    (ctx, b, w)
}

fn compose_one(ctx: &mut Ctx, w: WindowId) -> Scene {
    let (cols, rows) = ctx.window_size(w).unwrap();
    ctx.compose(
        cols,
        rows + 2,
        &[Placement {
            window: w,
            col: 0,
            row: 0,
        }],
    )
}

fn bg(s: &Scene, row: usize, col: usize) -> Rgb {
    s.row(row)[col].bg
}

/// A case-sensitive search highlight.
fn search(src: &str) -> Option<Pattern> {
    Some(Pattern::new(src, false).unwrap())
}

#[test]
fn child_faces_inherit_what_they_leave_unset() {
    let ctx = ctx_with(vec![
        ("t.parent", None, Style::new().fg(h("#010101")).italic()),
        ("t.child", Some("t.parent"), Style::new().bold()),
    ]);
    let s = ctx.face_style("t.child").unwrap();
    assert_eq!(s, Style::new().fg(h("#010101")).italic().bold());
    assert_eq!(s.bg, None, "nothing comes from default");
    assert_eq!(ctx.face_style("nope"), None);
}

#[test]
fn a_theme_replaces_a_declared_style_and_keeps_the_parent() {
    let mut ctx = ctx_with(vec![
        ("t.parent", None, Style::new().fg(h("#010101")).italic()),
        ("t.child", Some("t.parent"), Style::new().bold()),
    ]);
    ctx.set_theme(test_theme().face("t.child", Style::new().fg(h("#020202"))));
    let s = ctx.face_style("t.child").unwrap();
    assert_eq!(
        s,
        Style::new().fg(h("#020202")).italic(),
        "bold was the declared style the theme replaced"
    );
}

#[test]
fn base_and_ui_faces_inherit_from_their_parents() {
    let ctx = ctx_with(vec![]);
    let names: Vec<&str> = BASE_FACES.iter().chain(UI_FACES).map(|f| f.0).collect();
    assert_eq!(names.len(), 20);
    assert!(names.iter().all(|n| ctx.face_style(n).is_some()));
    assert_eq!(
        ctx.face_style("heading").unwrap(),
        Style::new().fg(h("#ffffff")).bold()
    );
    assert_eq!(
        ctx.face_style("line-number").unwrap().fg,
        Some(h("#555555")),
        "from shadow"
    );
    assert_eq!(
        ctx.face_style("line-number-current").unwrap().fg,
        Some(h("#999999"))
    );
    assert_eq!(
        ctx.face_style("bracket-match").unwrap().bg,
        Some(h("#00aa00")),
        "from highlight"
    );
    assert_eq!(
        ctx.face_style("prompt").unwrap(),
        Style::new().fg(h("#ffffff")).bold(),
        "from bold"
    );
}

#[test]
fn bad_face_declarations_are_registration_errors() {
    let err = |faces| {
        Ctx::new(vec![Box::new(Faces(faces))])
            .err()
            .unwrap()
            .join("\n")
    };
    let e = err(vec![("default", None, Style::new())]);
    assert!(e.contains("face default declared twice"), "{e}");
    let e = err(vec![("t.a", Some("t.missing"), Style::new())]);
    assert!(
        e.contains("face t.a inherits from t.missing, which isn't declared"),
        "{e}"
    );
    let e = err(vec![
        ("t.a", Some("t.b"), Style::new()),
        ("t.b", Some("t.a"), Style::new()),
    ]);
    assert!(e.contains("face t.a inherits from itself"), "{e}");
}

#[test]
fn faces_layer_over_default() {
    let mut ctx = ctx_with(vec![("t.bg", None, Style::new().bg(h("#123456")))]);
    let b = ctx.create_text("t", "abcd\n");
    let w = ctx.open_window(b, 10, 1).unwrap();
    ctx.focus(w);
    ctx.text_mut(b)
        .unwrap()
        .decorate(0..4, Deco::Face("t.bg"))
        .unwrap();
    ctx.text_mut(b).unwrap().set_cursor(w, 4);
    ctx.set_search_highlight(search("c"));
    let s = compose_one(&mut ctx, w);
    let c = &s.row(0)[0];
    assert_eq!(
        (c.fg, c.bg),
        (h("#aaaaaa"), h("#123456")),
        "default's foreground, the face's background"
    );
    let c = &s.row(0)[2];
    assert_eq!(
        (c.fg, c.bg),
        (h("#aaaaaa"), h("#0000aa")),
        "search sets only the background"
    );
}

#[test]
fn one_window_snapshot() {
    let (mut ctx, b, w) = one("fn f() {\n    x\n}\n", 16, 4);
    ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    let s = compose_one(&mut ctx, w);
    let want = "\
scene 16x6
text
|  1 fn f() {    |
|  2     x       |
|  3 }           |
|  4             |
| t              |
|                |
style
|aaaabbbbbbbbbbbb|
|ccccbbbbbbbbbbbb|
|ccccbbbbbbbbbbbb|
|ccccbbbbbbbbbbbb|
|dddddddddddddddd|
|bbbbbbbbbbbbbbbb|
legend
a #999999 on #000000
b #aaaaaa on #000000
c #555555 on #000000
d #ffffff on #333333
cursors
0,4 Block #ffff00
";
    assert_eq!(s.snapshot(), want, "\n{}", s.snapshot());
    assert_eq!(s.background, h("#000000"));
}

#[test]
fn two_windows_divider_status_lines_and_hollow_cursor() {
    let mut ctx = ctx_with(vec![]);
    let (a, b) = (ctx.create_text("a", "aaa\n"), ctx.create_text("b", "bbb\n"));
    let (wa, wb) = (
        ctx.open_window(a, 10, 2).unwrap(),
        ctx.open_window(b, 10, 2).unwrap(),
    );
    ctx.focus(wa);
    let s = ctx.compose(
        21,
        4,
        &[
            Placement {
                window: wa,
                col: 0,
                row: 0,
            },
            Placement {
                window: wb,
                col: 11,
                row: 0,
            },
        ],
    );
    let want = "\
scene 21x4
text
|aaa       │bbb       |
|          │          |
| a        │ b        |
|                     |
style
|aaaaaaaaaabaaaaaaaaaa|
|aaaaaaaaaabaaaaaaaaaa|
|ccccccccccbdddddddddd|
|aaaaaaaaaaaaaaaaaaaaa|
legend
a #aaaaaa on #000000
b #444444 on #000000
c #ffffff on #333333
d #777777 on #111111
cursors
0,0 Block #ffff00
0,11 Hollow #ffff00
";
    assert_eq!(s.snapshot(), want, "\n{}", s.snapshot());
}

#[test]
fn the_prompt_takes_the_command_row_and_the_cursor() {
    let (mut ctx, _, w) = one("x\n", 10, 1);
    ctx.prompt(":", |_, _| Ok(()));
    ctx.type_keys("wq");
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(2), ":wq");
    assert!(s.row(2)[0].bold, "the label is in the prompt face");
    assert!(!s.row(2)[1].bold, "the text is in default");
    let shapes: Vec<_> = s.cursors.iter().map(|c| (c.row, c.col, c.shape)).collect();
    assert_eq!(
        shapes,
        [(2, 3, CursorShape::Bar), (0, 0, CursorShape::Hollow)]
    );
}

#[test]
fn cursor_shape_follows_the_state() {
    let (mut ctx, _, w) = one("x\n", 10, 1);
    for (state, shape) in [
        (State::Normal, CursorShape::Block),
        (State::Insert, CursorShape::Bar),
        (State::OperatorPending, CursorShape::HalfBlock),
        (State::Visual, CursorShape::Block),
    ] {
        ctx.set_state(state);
        assert_eq!(
            compose_one(&mut ctx, w).cursors[0].shape,
            shape,
            "{state:?}"
        );
    }
}

#[test]
fn search_matches_and_the_current_match() {
    let (mut ctx, _, w) = one("ab (ab) ab\n", 20, 1);
    ctx.set_search_highlight(search("ab"));
    let s = compose_one(&mut ctx, w);
    let bgs: Vec<Rgb> = (0..10).map(|x| bg(&s, 0, x)).collect();
    let (cur, m, d) = (h("#00aaaa"), h("#0000aa"), h("#000000"));
    assert_eq!(bgs, [cur, cur, d, d, m, m, d, d, m, m]);
    ctx.set_search_highlight(None);
    assert_eq!(bg(&compose_one(&mut ctx, w), 0, 4), d);
}

#[test]
fn bracket_match() {
    let (mut ctx, b, w) = one("a (b [c] d) e\n", 20, 1);
    let green = h("#00aa00");
    ctx.text_mut(b).unwrap().set_cursor(w, 2);
    let s = compose_one(&mut ctx, w);
    let on: Vec<usize> = (0..14).filter(|&x| bg(&s, 0, x) == green).collect();
    assert_eq!(on, [2, 10]);
    ctx.text_mut(b).unwrap().set_cursor(w, 7);
    let s = compose_one(&mut ctx, w);
    let on: Vec<usize> = (0..14).filter(|&x| bg(&s, 0, x) == green).collect();
    assert_eq!(
        on,
        [5, 7],
        "backward from a closing bracket, skipping nothing outside"
    );
    ctx.text_mut(b).unwrap().set_cursor(w, 0);
    let s = compose_one(&mut ctx, w);
    assert!((0..14).all(|x| bg(&s, 0, x) != green));
}

#[test]
fn visual_selection_char_line_and_block() {
    let (mut ctx, b, w) = one("abc\nabc\n", 10, 2);
    let red = h("#aa0000");
    let selected = |ctx: &mut Ctx, anchor, head, shape| {
        ctx.text_mut(b).unwrap().set_selection(
            w,
            Selection {
                anchor,
                head,
                shape,
                goal: None,
            },
        );
        let s = compose_one(ctx, w);
        let mut out = vec![];
        for r in 0..2 {
            out.push(
                (0..3)
                    .map(|x| if bg(&s, r, x) == red { '#' } else { '.' })
                    .collect::<String>(),
            );
        }
        out
    };
    assert_eq!(
        selected(&mut ctx, 1, 5, Shape::Char),
        ["...", "..."],
        "not drawn outside visual state"
    );
    ctx.set_state(State::Visual);
    assert_eq!(
        selected(&mut ctx, 1, 5, Shape::Char),
        [".##", "##."],
        "both ends included"
    );
    assert_eq!(selected(&mut ctx, 1, 1, Shape::Line), ["###", "..."]);
    assert_eq!(selected(&mut ctx, 5, 0, Shape::Block), ["##.", "##."]);
}

#[test]
fn wide_characters_and_clusters() {
    let (mut ctx, _, w) = one("中e\u{301}x\n", 10, 1);
    let s = compose_one(&mut ctx, w);
    let glyphs: Vec<Glyph> = s.row(0)[..4].iter().map(|c| c.glyph.clone()).collect();
    assert_eq!(
        glyphs,
        [
            Glyph::Char('中'),
            Glyph::Continuation,
            Glyph::Cluster("e\u{301}".into()),
            Glyph::Char('x')
        ]
    );
    assert_eq!(s.row_text(0), "中e\u{301}x");
}

#[test]
fn placements_past_the_frame_are_clipped() {
    let (mut ctx, _, w) = one("abcdefghij\n", 10, 2);
    let s = ctx.compose(
        14,
        2,
        &[Placement {
            window: w,
            col: 9,
            row: 0,
        }],
    );
    assert_eq!(s.row_text(0), "         abcde");
    assert_eq!(s.row_text(1), "", "the command row");
    assert_eq!(s.cells.len(), 14 * 2);
}

#[test]
fn positions_map_cells_to_buffer_positions() {
    let (mut ctx, _, w) = one("ab\ncd\n", 10, 3);
    let s = ctx.compose(
        12,
        5,
        &[Placement {
            window: w,
            col: 2,
            row: 0,
        }],
    );
    assert_eq!(s.position(0, 2), Some((w, 0)));
    assert_eq!(s.position(1, 3), Some((w, 4)));
    assert_eq!(s.position(0, 0), None, "left of the window");
    assert_eq!(s.position(3, 2), None, "the status line");
    assert_eq!(s.position(99, 99), None);
}

#[test]
fn rgb_hex_and_display() {
    assert_eq!(Rgb::hex("#1a2B3c"), Rgb(0x1a, 0x2b, 0x3c));
    assert_eq!(Rgb(0x1a, 0x2b, 0x3c).to_string(), "#1a2b3c");
    assert!(std::panic::catch_unwind(|| Rgb::hex("1a2b3c")).is_err());
}

// ---------------------------------------------------------------- providers, surfaces, several windows

/// Paints faces at render time, as a syntax highlighter does, and records the
/// range it was asked for.
#[derive(Default)]
struct Paint {
    faces: Vec<(Range<usize>, &'static str)>,
    asked: Vec<Range<usize>>,
}

impl Mode for Paint {
    fn decorate(
        &mut self,
        _: &TextBuf,
        visible: Range<usize>,
        out: &mut Vec<(Range<usize>, &'static str)>,
    ) {
        self.asked.push(visible);
        out.extend(self.faces.iter().cloned());
    }
}

struct PaintExt;

impl Extension for PaintExt {
    fn name(&self) -> &'static str {
        "paint"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Paint>("paint", Paint::default);
    }
}

/// A surface that renders the lines it holds.
struct Lines(Vec<&'static str>);

impl Surface for Lines {
    fn kind(&self) -> &'static str {
        "lines"
    }
    fn render(&mut self, _: usize, _: usize, out: &mut Vec<String>) {
        out.extend(self.0.iter().map(|l| l.to_string()));
    }
}

fn at(window: WindowId, col: usize) -> Placement {
    Placement {
        window,
        col,
        row: 0,
    }
}

fn cursors(s: &Scene) -> Vec<(usize, usize, CursorShape)> {
    s.cursors.iter().map(|c| (c.row, c.col, c.shape)).collect()
}

#[test]
fn nothing_a_window_draws_spills_into_its_neighbour() {
    let mut ctx = ctx_with(vec![]);
    let (a, b) = (
        ctx.create_text("a", "0123456789abcdefghij\n"),
        ctx.create_text("b", "BBBBBBBBBB\n"),
    );
    let (wa, wb) = (
        ctx.open_window(a, 10, 1).unwrap(),
        ctx.open_window(b, 10, 1).unwrap(),
    );
    ctx.focus(wa);
    ctx.text_mut(a).unwrap().settings_mut().wrap = false;
    for pos in [10, 15] {
        ctx.text_mut(a).unwrap().set_cursor(wa, pos);
        let s = ctx.compose(21, 3, &[at(wb, 11), at(wa, 0)]);
        assert_eq!(s.row_text(0), "0123456789│BBBBBBBBBB");
        assert_eq!(
            cursors(&s),
            [(0, 11, CursorShape::Hollow)],
            "a cursor at {pos} is past its window's edge"
        );
    }
    // A window narrower than its gutter keeps the gutter and the cursor inside it.
    let narrow = ctx.open_window(a, 2, 1).unwrap();
    ctx.text_mut(a).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    let s = ctx.compose(13, 3, &[at(wb, 3), at(narrow, 0)]);
    assert_eq!(s.row_text(0), "  │BBBBBBBBBB");
    assert_eq!(cursors(&s), [(0, 3, CursorShape::Hollow)]);
}

#[test]
fn overlapping_faces_layer_inner_over_outer_with_providers_on_top() {
    let mut ctx = Ctx::new(vec![
        Box::new(Faces(vec![
            ("t.bg", None, Style::new().bg(h("#123456"))),
            ("t.fg", None, Style::new().fg(h("#654321"))),
            ("t.red", None, Style::new().bg(h("#ff0000"))),
            ("t.ul", None, Style::new().underline(Underline::Single)),
        ])),
        Box::new(PaintExt),
    ])
    .unwrap();
    ctx.set_theme(test_theme());
    let b = ctx.create_text("t", "abcdef\n");
    let w = ctx.open_window(b, 10, 1).unwrap();
    let t = ctx.text_mut(b).unwrap();
    // Added first, but nested inside t.bg, so it's drawn above it.
    t.decorate(1..2, Deco::Face("t.red")).unwrap();
    t.decorate(0..4, Deco::Face("t.bg")).unwrap();
    t.decorate(0..2, Deco::Face("t.fg")).unwrap();
    ctx.enable_mode::<Paint>(b).unwrap();
    ctx.with_mode(b, |p: &mut Paint, _| {
        p.faces = vec![(0..6, "t.ul"), (3..4, "t.red")]
    })
    .unwrap();
    let s = compose_one(&mut ctx, w);
    let cell = |x: usize| {
        let c = &s.row(0)[x];
        (c.fg, c.bg, c.underline)
    };
    let (fg, bg, red, ul) = (h("#654321"), h("#123456"), h("#ff0000"), Underline::Single);
    let d = h("#aaaaaa");
    assert_eq!(
        cell(0),
        (fg, bg, ul),
        "t.fg over t.bg, under the provider's face"
    );
    assert_eq!(cell(1), (fg, red, ul), "t.red is inside t.bg");
    assert_eq!(cell(2), (d, bg, ul));
    assert_eq!(
        cell(3),
        (d, red, ul),
        "a provider's face is above stored ones"
    );
    assert_eq!(cell(4), (d, h("#000000"), ul));
}

#[test]
fn highlights_change_only_what_their_faces_set() {
    let mut ctx = ctx_with(vec![(
        "t.deco",
        None,
        Style::new().fg(h("#123456")).italic(),
    )]);
    ctx.set_theme(test_theme().face("search", Style::new().bg(h("#0000aa")).bold()));
    let b = ctx.create_text("t", "(ab) ab\n");
    let w = ctx.open_window(b, 10, 1).unwrap();
    ctx.text_mut(b)
        .unwrap()
        .decorate(0..7, Deco::Face("t.deco"))
        .unwrap();
    ctx.set_search_highlight(search("ab"));
    let cell = |s: &Scene, x: usize| {
        let c = &s.row(0)[x];
        (c.fg, c.bg, c.italic, c.bold)
    };
    let (fg, green, blue, red) = (h("#123456"), h("#00aa00"), h("#0000aa"), h("#aa0000"));
    // The cursor is on '(', so the brackets match.
    let s = compose_one(&mut ctx, w);
    assert_eq!(cell(&s, 0), (fg, green, true, false), "bracket-match");
    assert_eq!(cell(&s, 1), (fg, blue, true, true), "search");
    ctx.set_state(State::Visual);
    ctx.text_mut(b).unwrap().set_selection(
        w,
        Selection {
            anchor: 6,
            head: 0,
            shape: Shape::Char,
            goal: None,
        },
    );
    let s = compose_one(&mut ctx, w);
    assert_eq!(
        cell(&s, 0),
        (fg, red, true, false),
        "selection over bracket-match"
    );
    assert_eq!(cell(&s, 5), (fg, red, true, true), "selection over search");
}

#[test]
fn bracket_match_counts_nested_pairs() {
    let (mut ctx, b, w) = one("((a))\n", 10, 1);
    let green = h("#00aa00");
    for (pos, want) in [(0, [0, 4]), (1, [1, 3]), (4, [0, 4])] {
        ctx.text_mut(b).unwrap().set_cursor(w, pos);
        let s = compose_one(&mut ctx, w);
        let on: Vec<usize> = (0..5).filter(|&x| bg(&s, 0, x) == green).collect();
        assert_eq!(on, want, "cursor at {pos}");
    }
}

#[test]
fn providers_are_asked_for_the_drawn_text_only() {
    let mut ctx = Ctx::new(vec![Box::new(PaintExt)]).unwrap();
    let b = ctx.create_text("t", &format!("{}\n", "x".repeat(1000)));
    let w = ctx.open_window(b, 10, 1).unwrap();
    ctx.text_mut(b).unwrap().settings_mut().wrap = false;
    ctx.enable_mode::<Paint>(b).unwrap();
    let asked = |ctx: &mut Ctx| {
        compose_one(ctx, w);
        let asked = ctx.with_mode(b, |p: &mut Paint, _| p.asked.pop());
        asked.unwrap().unwrap()
    };
    assert_eq!(
        asked(&mut ctx),
        0..10,
        "an unwrapped line shows its first 10 columns"
    );
    let t = ctx.text_mut(b).unwrap();
    t.tx(Origin::User)
        .replace(0..1001, "ab中")
        .commit()
        .unwrap();
    assert_eq!(asked(&mut ctx), 0..5, "to the end of the last character");
    ctx.resize(w, 3, 1).unwrap();
    ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    assert_eq!(asked(&mut ctx), 0..0, "no text fits beside the gutter");
}

#[test]
fn search_matches_dont_overlap_and_the_current_one_starts_at_the_cursor() {
    let (mut ctx, b, w) = one("aaaa\n", 10, 1);
    ctx.text_mut(b).unwrap().set_cursor(w, 1);
    ctx.set_search_highlight(search("aa"));
    let s = compose_one(&mut ctx, w);
    let (cur, m) = (h("#00aaaa"), h("#0000aa"));
    let bgs: Vec<Rgb> = (0..4).map(|x| bg(&s, 0, x)).collect();
    assert_eq!(bgs, [m, cur, cur, m]);
}

#[test]
fn the_search_highlight_is_a_regex_with_the_callers_case_rule() {
    let (mut ctx, _, w) = one("Ab (ab) aab\n", 20, 1);
    let (cur, m, d) = (h("#00aaaa"), h("#0000aa"), h("#000000"));
    let mut bgs = |ignore_case| {
        ctx.set_search_highlight(Some(Pattern::new("a+b", ignore_case).unwrap()));
        let s = compose_one(&mut ctx, w);
        (0..11).map(|x| bg(&s, 0, x)).collect::<Vec<Rgb>>()
    };
    assert_eq!(bgs(true), [cur, cur, d, d, m, m, d, d, m, m, m]);
    assert_eq!(bgs(false), [d, d, d, d, m, m, d, d, m, m, m]);
}

#[test]
fn only_the_focused_window_draws_the_selection() {
    let mut ctx = ctx_with(vec![]);
    let b = ctx.create_text("t", "abcdef\n");
    let (wa, wb) = (
        ctx.open_window(b, 10, 1).unwrap(),
        ctx.open_window(b, 10, 1).unwrap(),
    );
    ctx.focus(wa);
    let t = ctx.text_mut(b).unwrap();
    t.set_cursor(wb, 4);
    t.set_selection(
        wa,
        Selection {
            anchor: 0,
            head: 2,
            shape: Shape::Char,
            goal: None,
        },
    );
    ctx.set_state(State::Visual);
    let s = ctx.compose(21, 3, &[at(wa, 0), at(wb, 11)]);
    let red = h("#aa0000");
    let on: Vec<usize> = (0..21).filter(|&x| bg(&s, 0, x) == red).collect();
    assert_eq!(on, [0, 1, 2]);
}

#[test]
fn every_cell_of_a_tab_or_wide_character_maps_to_its_position() {
    let (mut ctx, _, w) = one("\tx中y\n", 20, 1);
    let s = compose_one(&mut ctx, w);
    let pos: Vec<Option<usize>> = (0..13).map(|x| s.position(0, x).map(|(_, p)| p)).collect();
    let tab = [Some(0); 8];
    assert_eq!(pos[..8], tab);
    assert_eq!(pos[8..], [Some(1), Some(2), Some(2), Some(5), None]);
}

#[test]
fn the_input_cursor_comes_first_and_is_the_only_one_not_hollow() {
    let mut ctx = ctx_with(vec![]);
    let (a, b) = (ctx.create_text("a", "aaa\n"), ctx.create_text("b", "bbb\n"));
    let (wa, wb) = (
        ctx.open_window(a, 10, 1).unwrap(),
        ctx.open_window(b, 10, 1).unwrap(),
    );
    ctx.focus(wa);
    let side_by_side = |ctx: &mut Ctx| ctx.compose(21, 3, &[at(wb, 11), at(wa, 0)]);
    assert_eq!(
        cursors(&side_by_side(&mut ctx)),
        [(0, 0, CursorShape::Block), (0, 11, CursorShape::Hollow)]
    );
    ctx.prompt(":", |_, _| Ok(()));
    assert_eq!(
        cursors(&side_by_side(&mut ctx)),
        [
            (2, 1, CursorShape::Bar),
            (0, 11, CursorShape::Hollow),
            (0, 0, CursorShape::Hollow)
        ]
    );
    assert_eq!(
        support::screen(&mut ctx, wa).cursor,
        Some((0, 0)),
        "the test helper reports the window's cursor, not the prompt's"
    );
    ctx.type_keys("<Esc>");
    ctx.text_mut(a).unwrap().settings_mut().cursor_visible = false;
    assert_eq!(
        cursors(&side_by_side(&mut ctx)),
        [(0, 11, CursorShape::Hollow)],
        "no cursor takes input"
    );
}

#[test]
fn status_line_prompt_and_surface_text_are_laid_out_like_buffer_text() {
    let mut ctx = ctx_with(vec![]);
    let (a, b) = (
        ctx.create_text("abcdefgh中", "\n"),
        ctx.create_text("e\u{301}", "\n"),
    );
    let c = ctx.create_surface("s", Lines(vec!["abcdefghi中", "a\tb"]));
    let (wa, wb, wc) = (
        ctx.open_window(a, 10, 1).unwrap(),
        ctx.open_window(b, 10, 1).unwrap(),
        ctx.open_window(c, 10, 2).unwrap(),
    );
    ctx.focus(wa);
    let s = ctx.compose(21, 4, &[at(wa, 0), at(wb, 11)]);
    let glyphs = |s: &Scene, r: usize, cols: Range<usize>| -> Vec<Glyph> {
        s.row(r)[cols].iter().map(|c| c.glyph.clone()).collect()
    };
    assert_eq!(
        s.row_text(1),
        " abcdefgh │ e\u{301}",
        "中 doesn't fit before the divider"
    );
    assert_eq!(
        s.row(1)[9].bg,
        h("#333333"),
        "a filler in the status line's face"
    );
    assert_eq!(
        glyphs(&s, 1, 11..14),
        [
            Glyph::Char(' '),
            Glyph::Cluster("e\u{301}".into()),
            Glyph::Char(' ')
        ],
        "a combining mark joins its letter"
    );
    let s = ctx.compose(12, 4, &[at(wc, 0)]);
    assert_eq!(s.row_text(0), "abcdefghi │");
    assert_eq!(s.row_text(1), "a?b       │", "a tab is a control character");
    // A prompt too long for the row shows its end, where the cursor is.
    ctx.prompt(":", |_, _| Ok(()));
    ctx.type_keys("ab");
    let s = ctx.compose(5, 3, &[at(wa, 0)]);
    assert_eq!(s.row_text(2), ":ab");
    ctx.type_keys("cdefgh");
    let s = ctx.compose(5, 3, &[at(wa, 0)]);
    assert_eq!(s.row_text(2), "efgh");
    assert_eq!(cursors(&s)[0], (2, 4, CursorShape::Bar));
}

#[test]
fn a_theme_that_styles_an_undeclared_face_is_reported() {
    let mut ctx = ctx_with(vec![]);
    assert!(ctx.messages.is_empty(), "{:?}", ctx.messages);
    ctx.set_theme(test_theme().face("line-numbers", Style::new()));
    assert_eq!(
        ctx.messages,
        ["theme test styles faces no one declared: line-numbers"]
    );
}
