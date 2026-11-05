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
//! - `cursor_shape_follows_the_state`: block, bar, underline, half block.
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
//! - `a_tab_and_a_wide_character_take_one_style_across_their_cells`
//! - `linewise_selection_shows_empty_lines`,
//!   `charwise_selection_across_a_newline_shows_it`: a newline has a cell.
//! - `a_highlighted_newline_shows_only_its_own_cell`
//! - `a_block_selection_skips_a_short_lines_newline`: and the buffer's end, as
//!   Evil's overlays do.
//! - `a_block_selection_takes_a_replacement_whole`: by its position's column,
//!   so a block's edge can't cut it.
//! - `a_provider_isnt_asked_for_a_wide_character_on_the_row_below`: the filler
//!   it leaves isn't its text.
//! - `a_folds_later_line_takes_the_current_number_face`: the current number
//!   goes by display line.
//! - `cells_past_the_end_of_a_line_map_to_its_newline`,
//!   `cells_past_a_wrapped_row_map_to_its_last_position`: where a click lands.
//! - `cells_are_built_with_new_and_field_writes`: from outside die-core. The
//!   doctest on `Cell` shows a struct expression doesn't compile there.
//! - `a_snapshot_shows_every_style_it_has_a_letter_for`,
//!   `a_snapshot_with_more_styles_than_letters_panics`
//! - `an_undeclared_decoration_face_is_reported_once`: to `*messages*`, the
//!   first time it's drawn.
//! - `style_setters_set_the_underline_color_and_turn_attributes_off`
//! - `style_sweep_matches_the_naive_scan`: a property. Every cell of random
//!   stored and provider faces, search matches and selections matches a scan of
//!   every face and match, per cell, as compose once did.
//! - `an_80_column_window_with_4000_faces_and_a_one_letter_search_composes_within_1ms`
//!   and `faces_and_search_cost_at_most_twice_a_plain_frame`: K15's speed.
//! - `rgb_hex_and_display`

use die_test_cap as _;

use die_core::*;
use proptest::prelude::*;
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
        &[WindowPlacement {
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
|                |
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
            WindowPlacement {
                window: wa,
                col: 0,
                row: 0,
            },
            WindowPlacement {
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
        (State::Replace, CursorShape::Underline),
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
        &[WindowPlacement {
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
        &[WindowPlacement {
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

fn at(window: WindowId, col: usize) -> WindowPlacement {
    WindowPlacement {
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
    ctx.text_mut(a).unwrap().settings_mut().wrap = Wrap::Off;
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
    ctx.text_mut(b).unwrap().settings_mut().wrap = Wrap::Off;
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
    let t = ctx.text_mut(b).unwrap();
    t.tx(Origin::User).replace(0..5, "ab\n").commit().unwrap();
    assert_eq!(asked(&mut ctx), 0..3, "a newline's cell is drawn too");
    ctx.resize(w, 3, 1).unwrap();
    ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    assert_eq!(asked(&mut ctx), 0..0, "no text fits beside the gutter");
}

#[test]
fn a_provider_isnt_asked_for_a_wide_character_on_the_row_below() {
    let mut ctx = Ctx::new(vec![Box::new(PaintExt)]).unwrap();
    let b = ctx.create_text("t", "aaaaaaaaa中b");
    let w = ctx.open_window(b, 10, 1).unwrap();
    ctx.enable_mode::<Paint>(b).unwrap();
    compose_one(&mut ctx, w);
    let asked = ctx.with_mode(b, |p: &mut Paint, _| p.asked.pop());
    assert_eq!(
        asked.unwrap().unwrap(),
        0..9,
        "the filler at column 9 stands for 中, which shows only on the row below the window"
    );
}

#[test]
fn a_folds_later_line_takes_the_current_number_face() {
    let (mut ctx, b, w) = one("a\nb\nc\nd\n", 10, 2);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Absolute;
    let deco = Deco::Hide {
        replacement: None,
        reveal: Reveal::Never,
    };
    t.decorate(1..4, deco).unwrap();
    t.set_cursor(w, 4); // On c, which the fold joins to a.
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "  1 ac");
    assert_eq!(s.row(0)[2].fg, h("#999999"), "the cursor's display line");
    assert_eq!(s.row(1)[2].fg, h("#555555"));
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
    assert_eq!(pos[8..], [Some(1), Some(2), Some(2), Some(5), Some(6)]);
}

#[test]
fn a_tab_and_a_wide_character_take_one_style_across_their_cells() {
    let (mut ctx, _, w) = one("\t中\n", 20, 1);
    ctx.set_search_highlight(search("\t|中"));
    let s = compose_one(&mut ctx, w);
    let (cur, m, d) = (h("#00aaaa"), h("#0000aa"), h("#000000"));
    let bgs: Vec<Rgb> = (0..12).map(|x| bg(&s, 0, x)).collect();
    assert_eq!(bgs[..8], [cur; 8], "the tab, where the cursor is");
    assert_eq!(bgs[8..], [m, m, d, d], "the wide character's two cells");
}

/// A `#` for each of the first `cols` cells of `rows` rows in the test theme's
/// selection color.
fn selected_cells(s: &Scene, rows: usize, cols: usize) -> Vec<String> {
    let red = h("#aa0000");
    (0..rows)
        .map(|r| {
            let on = |x| if bg(s, r, x) == red { '#' } else { '.' };
            (0..cols).map(on).collect()
        })
        .collect()
}

/// `text` in a 10 × `rows` window, in visual state with this selection.
fn with_selection(text: &str, rows: usize, anchor: usize, head: usize, shape: Shape) -> Scene {
    let (mut ctx, b, w) = one(text, 10, rows);
    ctx.set_state(State::Visual);
    ctx.text_mut(b).unwrap().set_selection(
        w,
        Selection {
            anchor,
            head,
            shape,
            goal: None,
        },
    );
    compose_one(&mut ctx, w)
}

#[test]
fn linewise_selection_shows_empty_lines() {
    let s = with_selection("ab\n\ncd\n", 4, 0, 4, Shape::Line);
    assert_eq!(
        selected_cells(&s, 4, 4),
        ["###.", "#...", "###.", "...."],
        "each newline's cell, as Vim and mono-dark's region show one"
    );
    let s = with_selection("ab\n\ncd\n", 4, 7, 7, Shape::Line);
    assert_eq!(
        selected_cells(&s, 4, 4),
        ["....", "....", "....", "#..."],
        "the empty line after a final newline has a cell too"
    );
}

#[test]
fn charwise_selection_across_a_newline_shows_it() {
    let s = with_selection("ab\n\ncd\n", 4, 1, 3, Shape::Char);
    assert_eq!(
        selected_cells(&s, 3, 4),
        [".##.", "#...", "...."],
        "b and both newlines, and nothing past them"
    );
    let s = with_selection("ab", 2, 2, 2, Shape::Char);
    assert_eq!(selected_cells(&s, 1, 4), ["..#."], "the buffer's end");
}

#[test]
fn a_block_selection_skips_a_short_lines_newline() {
    let blocks = |text, anchor, head| {
        selected_cells(&with_selection(text, 3, anchor, head, Shape::Block), 3, 6)
    };
    assert_eq!(
        blocks("abcd\nab\nabcd\n", 2, 11),
        ["..##..", "......", "..##.."],
        "the middle line is shorter than the block: nothing on it, as in Evil"
    );
    assert_eq!(
        blocks("ab\n\ncd\n", 0, 5),
        ["##....", "......", "##...."],
        "nor on an empty line"
    );
    assert_eq!(
        blocks("abc\nab", 2, 6),
        ["..#...", "......", "......"],
        "nor at the end of the buffer"
    );
}

/// TEXT-3: on a block, the goal `$` sets reaches each line's end, however far.
#[test]
fn a_block_selection_with_the_end_of_line_goal_reaches_each_lines_end() {
    let blocks = |goal| {
        let (mut ctx, b, w) = one("ab\nabcd\nabc\n", 10, 3);
        ctx.set_state(State::Visual);
        let sel = Selection {
            anchor: 1,
            head: 5,
            shape: Shape::Block,
            goal,
        };
        ctx.text_mut(b).unwrap().set_selection(w, sel);
        selected_cells(&compose_one(&mut ctx, w), 3, 6)
    };
    assert_eq!(blocks(None), [".#....", ".##...", "......"]);
    assert_eq!(
        blocks(Some(usize::MAX)),
        [".#....", ".###..", "......"],
        "and no further: not its newline"
    );
}

/// TEXT-3: a block takes a tab or a wide character it covers any part of.
#[test]
fn a_block_selection_takes_a_tab_or_wide_character_it_only_starts_in() {
    let blocks = |text, anchor, head| {
        selected_cells(&with_selection(text, 2, anchor, head, Shape::Block), 2, 8)
    };
    assert_eq!(
        blocks("a\tb\nabcdefgh\n", 8, 2),
        [".#######", "....####"],
        "the tab covers columns 1 to 7, and the block starts at 4"
    );
    assert_eq!(
        blocks("世x\nabcd\n", 6, 3),
        ["###.....", ".##....."],
        "the wide character covers columns 0 and 1, and the block starts at 1"
    );
}

/// scene-4: a replacement shows one position, so it's selected whole or not at all, as a tab is.
#[test]
fn a_block_selection_takes_a_replacement_whole() {
    let blocks = |anchor, head| {
        let (mut ctx, b, w) = one("abcdefgh\nabcdefgh\n", 10, 2);
        ctx.set_state(State::Visual);
        let t = ctx.text_mut(b).unwrap();
        let deco = Deco::Hide {
            replacement: Some("XYZ".into()),
            reveal: Reveal::Never,
        };
        t.decorate(1..3, deco).unwrap();
        let shape = Shape::Block;
        let sel = Selection {
            anchor,
            head,
            shape,
            goal: None,
        };
        t.set_selection(w, sel);
        selected_cells(&compose_one(&mut ctx, w), 2, 8)
    };
    assert_eq!(
        blocks(0, 10),
        ["####....", "##......"],
        "the block's edge at the replacement's column takes all of it"
    );
    assert_eq!(
        blocks(3, 11),
        ["....#...", "..###..."],
        "a block starting past its first cell doesn't take its later ones"
    );
}

#[test]
fn a_highlighted_newline_shows_only_its_own_cell() {
    let mut ctx = ctx_with(vec![("t.bg", None, Style::new().bg(h("#123456")))]);
    let b = ctx.create_text("t", "ab\ncd\n");
    let w = ctx.open_window(b, 10, 2).unwrap();
    ctx.focus(w);
    ctx.text_mut(b)
        .unwrap()
        .decorate(1..4, Deco::Face("t.bg"))
        .unwrap();
    let s = compose_one(&mut ctx, w);
    let (face, d) = (h("#123456"), h("#000000"));
    let row = |r: usize| (0..4).map(|x| bg(&s, r, x)).collect::<Vec<Rgb>>();
    assert_eq!(row(0), [d, face, face, d], "b and the newline, no further");
    assert_eq!(row(1), [face, d, d, d]);
    assert_eq!(s.row_text(0), "ab", "a newline draws no glyph");
}

#[test]
fn cells_past_the_end_of_a_line_map_to_its_newline() {
    let (mut ctx, b, w) = one("ab\n\ncd\n", 8, 5);
    ctx.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Absolute;
    let s = compose_one(&mut ctx, w);
    let row = |r: usize| {
        (0..8)
            .map(|x| s.position(r, x).map(|(_, p)| p))
            .collect::<Vec<Option<usize>>>()
    };
    let text = |cells: [usize; 4]| [[None; 4], cells.map(Some)].concat();
    assert_eq!(row(0), text([0, 1, 2, 2]), "the gutter maps to nothing");
    assert_eq!(row(1), text([3, 3, 3, 3]), "an empty line");
    assert_eq!(row(2), text([4, 5, 6, 6]));
    assert_eq!(row(3), text([7, 7, 7, 7]), "the buffer's end");
    assert_eq!(row(4), [None; 8], "below the buffer's last line");

    let (mut ctx, _, w) = one("ab\ncd", 10, 2);
    let s = compose_one(&mut ctx, w);
    for x in 2..10 {
        assert_eq!(s.position(0, x), Some((w, 2)), "the newline, column {x}");
        assert_eq!(
            s.position(1, x),
            Some((w, 5)),
            "no final newline, column {x}"
        );
    }
    assert_eq!(s.position(2, 5), None, "the status line");
}

#[test]
fn cells_past_a_wrapped_row_map_to_its_last_position() {
    let (mut ctx, b, w) = one("abcdef\n", 10, 2);
    ctx.text_mut(b).unwrap().settings_mut().max_width = Some(4);
    let s = compose_one(&mut ctx, w);
    let row = |r: usize| {
        (0..10)
            .map(|x| s.position(r, x).map(|(_, p)| p))
            .collect::<Vec<Option<usize>>>()
    };
    let (a, b_, c, d) = (Some(0), Some(1), Some(2), Some(3));
    let (e, f, nl) = (Some(4), Some(5), Some(6));
    assert_eq!(
        row(0),
        [a, b_, c, d, d, d, d, d, d, d],
        "where the row breaks"
    );
    assert_eq!(row(1), [e, f, nl, nl, nl, nl, nl, nl, nl, nl]);

    // The padding where a wide character didn't fit is past its row's end.
    let (mut ctx, _, w) = one("abc中\n", 4, 2);
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "abc");
    assert_eq!(s.position(0, 3), Some((w, 2)));
    assert_eq!(s.position(1, 0), Some((w, 3)));
    assert_eq!(s.row(0)[3].bg, h("#000000"), "padding takes no highlight");
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
    assert_eq!(s.row_text(1), "a^Ib      │", "a tab is a control character");
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

// ---------------------------------------------------------------- cells and snapshots

/// One row of these cells.
fn row_of(cells: Vec<Cell>) -> Scene {
    Scene {
        cols: cells.len(),
        rows: 1,
        positions: vec![None; cells.len()],
        cells,
        cursors: vec![],
        background: h("#000000"),
    }
}

/// `n` cells, each in a foreground of its own.
fn distinct_styles(n: u8) -> Scene {
    let cell = |i| Cell::new(Glyph::Char('x'), Rgb(i, 0, 0), h("#000000"));
    row_of((0..n).map(cell).collect())
}

#[test]
fn cells_are_built_with_new_and_field_writes() {
    let mut c = Cell::new(Glyph::Char('x'), h("#010101"), h("#020202"));
    assert_eq!(
        (c.bold, c.italic, c.strike, c.underline, c.underline_color),
        (false, false, false, Underline::None, h("#010101")),
        "no attributes, and the underline in the foreground color"
    );
    c.bold = true;
    c.underline = Underline::Curly;
    c.underline_color = h("#030303");
    let snap = row_of(vec![c]).snapshot();
    assert!(
        snap.contains("a #010101 on #020202 bold underline Curly #030303\n"),
        "{snap}"
    );
}

#[test]
fn a_snapshot_shows_every_style_it_has_a_letter_for() {
    let snap = distinct_styles(62).snapshot();
    assert!(snap.contains("|abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789|"));
    assert!(snap.contains("9 #3d0000 on #000000"), "{snap}");
}

#[test]
fn a_snapshot_with_more_styles_than_letters_panics() {
    // Past the letters, two different frames could give the same text.
    let s = distinct_styles(63);
    let e = std::panic::catch_unwind(|| s.snapshot()).expect_err("it panics");
    let e = e.downcast_ref::<String>().expect("a formatted message");
    assert_eq!(e, "a snapshot shows at most 62 styles");
}

// ---------------------------------------------------------------- faces

#[test]
fn an_undeclared_decoration_face_is_reported_once() {
    let mut ctx = Ctx::new(vec![
        Box::new(Faces(vec![(
            "t.known",
            None,
            Style::new().fg(h("#123456")),
        )])),
        Box::new(PaintExt),
    ])
    .unwrap();
    ctx.set_theme(test_theme());
    let b = ctx.create_text("t", "abcd\nefgh\nijkl\n");
    let w = ctx.open_window(b, 10, 1).unwrap();
    ctx.focus(w);
    let t = ctx.text_mut(b).unwrap();
    t.decorate(0..1, Deco::Face("t.typo")).unwrap();
    t.decorate(1..2, Deco::Face("t.typo")).unwrap();
    t.decorate(2..3, Deco::Face("t.known")).unwrap();
    t.decorate(10..11, Deco::Face("t.below")).unwrap();
    ctx.enable_mode::<Paint>(b).unwrap();
    ctx.with_mode(b, |p: &mut Paint, _| p.faces = vec![(3..4, "t.painted")])
        .unwrap();
    let reported = [
        "face t.typo isn't declared (a decoration or a mode's provider uses it)",
        "face t.painted isn't declared (a decoration or a mode's provider uses it)",
    ];
    let s = compose_one(&mut ctx, w);
    assert_eq!(ctx.messages, reported, "t.below isn't drawn yet");
    let d = (h("#aaaaaa"), h("#000000"));
    assert_eq!((s.row(0)[0].fg, s.row(0)[0].bg), d, "it draws as nothing");
    assert_eq!(s.row(0)[2].fg, h("#123456"), "and the others still draw");
    compose_one(&mut ctx, w);
    assert_eq!(ctx.messages, reported, "once for each name");
    ctx.text_mut(b).unwrap().set_cursor(w, 10);
    compose_one(&mut ctx, w);
    assert_eq!(
        ctx.messages.last().unwrap(),
        "face t.below isn't declared (a decoration or a mode's provider uses it)",
        "when it's first drawn"
    );
    assert_eq!(ctx.messages.len(), 3);
}

#[test]
fn style_setters_set_the_underline_color_and_turn_attributes_off() {
    let below = Style::new().bold().italic().strike();
    let off = Style::new()
        .underline(Underline::Dashed)
        .underline_color(h("#123456"))
        .bold_is(false)
        .italic_is(false)
        .strike_is(false)
        .over(below);
    assert_eq!(off.bold, Some(false), "it overrides the bold below");
    assert_eq!(off.italic, Some(false));
    assert_eq!(off.strike, Some(false));
    assert_eq!(off.underline, Some(Underline::Dashed));
    assert_eq!(off.underline_color, Some(h("#123456")));
    let on = Style::new().bold_is(true).italic_is(true).strike_is(true);
    assert_eq!(on, below);
    // In a frame: a face that turns an inherited attribute off.
    let mut ctx = ctx_with(vec![
        ("t.bold", None, Style::new().bold()),
        ("t.plain", Some("t.bold"), Style::new().bold_is(false)),
    ]);
    let b = ctx.create_text("t", "ab\n");
    let w = ctx.open_window(b, 10, 1).unwrap();
    let t = ctx.text_mut(b).unwrap();
    t.decorate(0..2, Deco::Face("t.bold")).unwrap();
    t.decorate(1..2, Deco::Face("t.plain")).unwrap();
    let s = compose_one(&mut ctx, w);
    assert!(s.row(0)[0].bold);
    assert!(!s.row(0)[1].bold, "off beats the parent and the face below");
}

// ---------------------------------------------------------------- the sweep and a naive scan

const SWEEP_FACES: [&str; 6] = ["t.f0", "t.f1", "t.f2", "t.f3", "t.f4", "t.f5"];

/// What compose once did for one cell: every face and every match, each time.
struct Scan {
    base: Style,
    faces: Vec<(Range<usize>, Style)>,
    search: Vec<Range<usize>>,
    current: Option<Range<usize>>,
    selection: Option<Selection>,
    search_face: Style,
    current_face: Style,
    selection_face: Style,
}

impl Scan {
    fn style(&self, t: &TextBuf, q: usize) -> Style {
        let mut s = self.base;
        for (_, f) in self.faces.iter().filter(|(r, _)| r.contains(&q)) {
            s = f.over(s);
        }
        let current = self.current.as_ref().is_some_and(|m| m.contains(&q));
        if current {
            s = self.current_face.over(s);
        } else if self.search.iter().any(|m| m.contains(&q)) {
            s = self.search_face.over(s);
        }
        if let Some(sel) = self.selection {
            let (a, h) = (t.line_of(sel.anchor), t.line_of(sel.head));
            let lines = (a.min(h)..=a.max(h)).contains(&t.line_of(q));
            let on = match sel.shape {
                Shape::Char => (sel.anchor.min(sel.head)..=sel.anchor.max(sel.head)).contains(&q),
                Shape::Line => lines,
                Shape::Block => {
                    let (a, h) = (t.display_col(sel.anchor), t.display_col(sel.head));
                    lines
                        && (a.min(h)..=a.max(h)).contains(&t.display_col(q))
                        && q < t.len()
                        && t.rope().byte(q) != b'\n'
                }
            };
            if on {
                s = self.selection_face.over(s);
            }
        }
        s
    }
}

/// `(fg, bg, bold, italic, strike, underline, underline color)` of a style or cell.
type Look = (Rgb, Rgb, bool, bool, bool, Underline, Rgb);

fn look_of_style(s: Style) -> Look {
    let fg = s.fg.unwrap();
    (
        fg,
        s.bg.unwrap(),
        s.bold.unwrap_or(false),
        s.italic.unwrap_or(false),
        s.strike.unwrap_or(false),
        s.underline.unwrap_or_default(),
        s.underline_color.unwrap_or(fg),
    )
}

fn look_of_cell(c: &Cell) -> Look {
    (
        c.fg,
        c.bg,
        c.bold,
        c.italic,
        c.strike,
        c.underline,
        c.underline_color,
    )
}

/// One case of the property: the text, stored and provider faces as (bytes a and
/// b, face), a literal search, the cursor or selection, and the window's settings.
struct SweepCase {
    text: String,
    stored: Vec<(usize, usize, usize)>,
    provided: Vec<(usize, usize, usize)>,
    pattern: Option<&'static str>,
    selection: Option<(usize, usize, Shape)>,
    cursor: usize,
    wrap: bool,
    gutter: bool,
}

fn check_sweep(case: SweepCase) -> std::result::Result<(), TestCaseError> {
    let names = SWEEP_FACES
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let v = 0x10 * (i as u8 + 1);
            let s = match i {
                0 => Style::new().fg(Rgb(v, 0, 0)),
                1 => Style::new().bg(Rgb(0, v, 0)),
                2 => Style::new().fg(Rgb(v, v, 0)).bold(),
                3 => Style::new().italic().underline(Underline::Curly),
                4 => Style::new().bold_is(false).strike(),
                _ => Style::new()
                    .bg(Rgb(0, 0, v))
                    .underline(Underline::Single)
                    .underline_color(Rgb(v, 0, v)),
            };
            (*n, None, s)
        })
        .collect();
    let mut ctx = Ctx::new(vec![Box::new(Faces(names)), Box::new(PaintExt)]).unwrap();
    ctx.set_theme(test_theme());
    let b = ctx.create_text("t", &case.text);
    let (cols, rows) = (14, 6);
    let w = ctx.open_window(b, cols, rows).unwrap();
    ctx.focus(w);
    let len = case.text.len();
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().wrap = if case.wrap { Wrap::Char } else { Wrap::Off };
    if case.gutter {
        t.settings_mut().line_numbers = LineNumbers::Absolute;
    }
    for &(x, y, f) in &case.stored {
        let (x, y) = (x % (len + 1), y % (len + 1));
        t.decorate(x.min(y)..x.max(y), Deco::Face(SWEEP_FACES[f]))
            .unwrap();
    }
    let cursor = case.cursor % (len + 1);
    t.set_cursor(w, cursor);
    let (mut anchor_head, mut cursor) = (None, cursor);
    if let Some((a, h, shape)) = case.selection {
        let (a, h) = (a % (len + 1), h % (len + 1));
        t.set_selection(
            w,
            Selection {
                anchor: a,
                head: h,
                shape,
                goal: None,
            },
        );
        anchor_head = t.selection(w);
        cursor = h;
    }
    ctx.enable_mode::<Paint>(b).unwrap();
    // A provider may return anything: ranges past the text, empty or backward.
    let provided: Vec<(Range<usize>, &'static str)> = case
        .provided
        .iter()
        .map(|&(x, y, f)| (x..y, SWEEP_FACES[f]))
        .collect();
    let given = provided.clone();
    ctx.with_mode(b, move |p: &mut Paint, _| p.faces = given)
        .unwrap();
    if case.selection.is_some() {
        ctx.set_state(State::Visual);
    }
    let pat = case.pattern.map(|p| Pattern::new(p, false).unwrap());
    ctx.set_search_highlight(pat.clone());

    let scene = compose_one(&mut ctx, w);
    let visible = ctx
        .with_mode(b, |p: &mut Paint, _| p.asked.pop())
        .unwrap()
        .expect("compose asks the provider");

    let style = |n: &str| ctx.face_style(n).unwrap();
    let t = ctx.text(b).unwrap();
    let mut stored: Vec<(Range<usize>, &'static str)> = t
        .decorations()
        .filter_map(|(_, d)| match d.deco {
            Deco::Face(f) => Some((d.range.clone(), f)),
            _ => None,
        })
        .collect();
    stored.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let at_cursor = |lit: &str| {
        case.text[cursor..]
            .starts_with(lit)
            .then(|| cursor..cursor + lit.len())
    };
    let scan = Scan {
        base: style("default"),
        faces: stored
            .iter()
            .chain(&provided)
            .map(|(r, f)| (r.clone(), style(f)))
            .collect(),
        search: pat
            .as_ref()
            .map_or(vec![], |p| t.matches(p, visible.clone())),
        current: case.pattern.and_then(at_cursor),
        selection: anchor_head,
        search_face: style("search"),
        current_face: style("search-current"),
        selection_face: style("selection"),
    };
    // Every buffer position's cell, then every cell of the text area: a position's
    // own has its faces, and the rest are `default`.
    let g = t.geometry(w).unwrap();
    let mut own = std::collections::HashMap::new();
    for q in 0..=len {
        if let Some((row, col)) = t.screen_pos(w, q).filter(|&(_, col)| col < cols) {
            prop_assert_eq!(scene.position(row, col), Some((w, q)), "position {}", q);
            own.insert((row, col), q);
        }
    }
    for row in 0..rows {
        for col in g.text_x..cols {
            let want = own
                .get(&(row, col))
                .map_or(scan.base, |&q| scan.style(t, q));
            prop_assert_eq!(
                look_of_cell(&scene.row(row)[col]),
                look_of_style(want),
                "row {}, column {}, position {:?}, in {:?}",
                row,
                col,
                own.get(&(row, col)),
                case.text
            );
        }
    }
    Ok(())
}

fn a_shape() -> impl Strategy<Value = Shape> {
    prop_oneof![Just(Shape::Char), Just(Shape::Line), Just(Shape::Block)]
}

proptest! {
    #[test]
    fn style_sweep_matches_the_naive_scan(
        text in "[ab \n]{0,120}",
        stored in prop::collection::vec((0..130usize, 0..130usize, 0..6usize), 0..16),
        provided in prop::collection::vec((0..140usize, 0..140usize, 0..6usize), 0..8),
        pattern in prop::option::of(prop_oneof![Just("a"), Just("ab"), Just("b b"), Just("  ")]),
        selection in prop::option::of((0..130usize, 0..130usize, a_shape())),
        cursor in 0..130usize,
        wrap in any::<bool>(),
        gutter in any::<bool>(),
    ) {
        check_sweep(SweepCase { text, stored, provided, pattern, selection, cursor, wrap, gutter })?;
    }
}

// ---------------------------------------------------------------- K4: what layout's rows draw

fn with_kw_face() -> Ctx {
    ctx_with(vec![
        ("p.kw", None, Style::new().fg(h("#ff0000"))),
        ("p.other", None, Style::new().bg(h("#00ff00"))),
    ])
}

fn kw_mask(s: &Scene, row: usize, cols: usize) -> String {
    (0..cols)
        .map(|x| {
            if s.row(row)[x].fg == h("#ff0000") {
                '#'
            } else {
                '.'
            }
        })
        .collect()
}

fn in_window(ctx: &mut Ctx, text: &str, cols: usize, rows: usize) -> (BufferId, WindowId) {
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    ctx.focus(w);
    (b, w)
}

/// scene-4: every cell of a replacement is its hidden range's start, in that position's faces.
#[test]
fn a_hide_replacement_is_drawn_in_one_style() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "fn x() { body }", 16, 1);
    let t = ctx.text_mut(b).unwrap();
    let hide = Deco::Hide {
        replacement: Some("...".into()),
        reveal: Reveal::Never,
    };
    t.decorate(8..14, hide).unwrap();
    t.decorate(0..15, Deco::Face("p.kw")).unwrap();
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "fn x() {...}");
    assert_eq!(kw_mask(&s, 0, 12), "############");
    let at: Vec<_> = (8..11).map(|x| s.position(0, x).unwrap().1).collect();
    assert_eq!(at, [8, 8, 8]);
}

#[test]
fn a_replacement_split_by_the_wrap_keeps_its_style_and_position() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "x body y", 6, 3);
    let t = ctx.text_mut(b).unwrap();
    let hide = Deco::Hide {
        replacement: Some("[folded]".into()),
        reveal: Reveal::Never,
    };
    t.decorate(2..6, hide).unwrap();
    t.decorate(0..8, Deco::Face("p.kw")).unwrap();
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "x");
    assert_eq!(s.row_text(1), "[folde");
    assert_eq!(s.row_text(2), "d] y");
    assert_eq!(kw_mask(&s, 1, 6), "######");
    assert_eq!(s.position(1, 5).unwrap().1, 2);
    assert_eq!(
        s.position(2, 0).unwrap().1,
        2,
        "the replacement's rest, on the next row"
    );
}

#[test]
fn a_tab_split_by_the_wrap_keeps_its_style_and_position() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "abcd\tx", 6, 2);
    let t = ctx.text_mut(b).unwrap();
    t.decorate(4..5, Deco::Face("p.other")).unwrap();
    let s = compose_one(&mut ctx, w);
    for (row, col) in [(0, 4), (0, 5), (1, 0), (1, 1)] {
        assert_eq!(s.position(row, col).unwrap().1, 4, "{row}, {col}");
        assert_eq!(s.row(row)[col].bg, h("#00ff00"), "{row}, {col}");
    }
    assert_eq!(s.position(1, 2).unwrap().1, 5, "x");
}

/// scene-5, call 6: runs of faces, over the text the string stands in for or before.
#[test]
fn virtual_and_replacement_text_take_their_faces() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "abcd", 20, 1);
    let t = ctx.text_mut(b).unwrap();
    let styled = |text: &str| Styled {
        text: text.into(),
        faces: vec![(0..text.len(), "p.kw")],
    };
    t.decorate(1..1, Deco::Virtual(styled("VT"))).unwrap();
    let hide = Deco::Hide {
        replacement: Some(styled("R")),
        reveal: Reveal::Never,
    };
    t.decorate(2..3, hide).unwrap();
    t.decorate(1..2, Deco::Face("p.other")).unwrap();
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "aVTbRd");
    assert_eq!(kw_mask(&s, 0, 6), ".##.#.");
    assert_eq!(s.position(0, 1), None, "virtual text has no position");
    assert_eq!(s.position(0, 4).unwrap().1, 2);
    assert_eq!(
        s.row(0)[1].bg,
        h("#00ff00"),
        "over the faces at its position"
    );
}

/// The faces at its position reach it, even when that position's own cell isn't drawn.
#[test]
fn virtual_text_takes_the_faces_of_a_position_off_screen() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "ab", 4, 1);
    let t = ctx.text_mut(b).unwrap();
    t.decorate(1..1, Deco::Virtual("xxxx".into())).unwrap();
    t.decorate(1..2, Deco::Face("p.kw")).unwrap();
    t.set_cursor(w, 0);
    let s = compose_one(&mut ctx, w);
    assert_eq!(
        s.row_text(0),
        "axxx",
        "x and b go on the next row, out of view"
    );
    assert_eq!(kw_mask(&s, 0, 4), ".###");
}

#[test]
fn a_replacement_with_two_runs_draws_both() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "abcd", 20, 1);
    let t = ctx.text_mut(b).unwrap();
    let replacement = Styled {
        text: "xyz".into(),
        faces: vec![(0..1, "p.kw"), (2..3, "p.other"), (0..1, "p.other")],
    };
    let hide = Deco::Hide {
        replacement: Some(replacement),
        reveal: Reveal::Never,
    };
    t.decorate(1..3, hide).unwrap();
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "axyzd");
    assert_eq!(s.row(0)[1].bg, h("#00ff00"), "the last run holding it wins");
    assert_ne!(s.row(0)[1].fg, h("#ff0000"));
    assert_eq!(s.row(0)[2].bg, h("#000000"), "no run: the text's own");
    assert_eq!(s.row(0)[3].bg, h("#00ff00"));
}

#[test]
fn an_undeclared_face_on_styled_text_is_reported_and_filler_takes_none() {
    let mut ctx = with_kw_face();
    let (b, w) = in_window(&mut ctx, "aa", 4, 2);
    let t = ctx.text_mut(b).unwrap();
    let styled = Styled {
        text: "中".into(),
        faces: vec![(0..3, "p.kw")],
    };
    t.decorate(2..2, Deco::Virtual(styled)).unwrap();
    t.decorate(
        1..1,
        Deco::Virtual(Styled {
            text: "x".into(),
            faces: vec![(0..1, "nope")],
        }),
    )
    .unwrap();
    let s = compose_one(&mut ctx, w);
    assert!(
        ctx.messages.iter().any(|m| m.contains("nope")),
        "{:?}",
        ctx.messages
    );
    assert_eq!(s.row_text(1), "中");
    assert_eq!(
        s.row(0)[3].fg,
        h("#aaaaaa"),
        "the filler a wide item leaves isn't in its face"
    );
}

/// WRAP-2's cost. A cursor past a full row goes in the right padding, or else on the row's last cell.
#[test]
fn a_cursor_past_a_full_row_goes_in_the_padding_or_on_the_last_cell() {
    let cursor = |s: &Scene| {
        let c = s.cursors[0];
        (c.row, c.col, c.past_end)
    };
    let mut ctx = ctx_with(vec![]);
    let (b, w) = in_window(&mut ctx, "aaaaaaaaaa\nb", 10, 3);
    ctx.text_mut(b).unwrap().set_cursor(w, 10);
    let s = compose_one(&mut ctx, w);
    assert_eq!(
        cursor(&s),
        (0, 9, true),
        "no padding: the last cell's right edge"
    );
    assert!(
        s.snapshot().contains("0,9 Block past-end "),
        "{}",
        s.snapshot()
    );

    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().max_width = Some(10);
    ctx.resize(w, 14, 3).unwrap();
    let s = compose_one(&mut ctx, w);
    assert_eq!(cursor(&s), (0, 10, false), "the right padding");

    // Centered, the same: the padding's after the text.
    ctx.text_mut(b).unwrap().settings_mut().center = true;
    let s = compose_one(&mut ctx, w);
    assert_eq!(cursor(&s), (0, 12, false));

    // At the end of the text too, and past an unwrapped row it's not drawn at all.
    let (b2, w2) = in_window(&mut ctx, "aaaaaaaaaa", 10, 1);
    let t = ctx.text_mut(b2).unwrap();
    t.set_cursor(w2, 10);
    let s = compose_one(&mut ctx, w2);
    assert_eq!(cursor(&s), (0, 9, true));
    ctx.text_mut(b2).unwrap().settings_mut().wrap = Wrap::Off;
    assert!(compose_one(&mut ctx, w2).cursors.is_empty());

    // A cursor on the last cell itself isn't past anything.
    ctx.text_mut(b2).unwrap().settings_mut().wrap = Wrap::Word;
    ctx.text_mut(b2).unwrap().set_cursor(w2, 9);
    assert_eq!(cursor(&compose_one(&mut ctx, w2)), (0, 9, false));
}

/// K4's call: a full row's newline takes no cell, even with padding there to draw one in.
#[test]
fn a_full_rows_newline_has_no_cell_in_a_centered_window() {
    let (mut ctx, b, w) = one("abcd\nef\n", 8, 3);
    let t = ctx.text_mut(b).unwrap();
    let s = t.settings_mut();
    s.max_width = Some(4);
    s.center = true;
    t.set_selection(
        w,
        Selection {
            anchor: 0,
            head: 5,
            shape: Shape::Line,
            goal: None,
        },
    );
    ctx.set_state(State::Visual);
    let s = compose_one(&mut ctx, w);
    let sel = h("#aa0000");
    let bgs = |r: usize| (0..8).map(|x| bg(&s, r, x) == sel).collect::<Vec<_>>();
    let (t, f) = (true, false);
    assert_eq!(
        bgs(0),
        [f, f, t, t, t, t, f, f],
        "abcd, and nothing past it"
    );
    assert_eq!(
        bgs(1),
        [f, f, t, t, t, f, f, f],
        "ef, and its newline's cell"
    );
    assert_eq!(
        s.position(0, 6),
        Some((w, 4)),
        "a click in the padding lands on the newline"
    );
    assert_eq!(s.position(0, 7), Some((w, 4)));
}

/// DR-12: as Emacs draws them, a cell per character, and each cell is the character's.
#[test]
fn control_characters_draw_as_emacs_draws_them() {
    let (mut ctx, _, w) = one("a\u{1}\r\u{7f}\u{85}b\n", 20, 1);
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "a^A^M^?\\205b");
    let at: Vec<usize> = (0..13).map(|x| s.position(0, x).unwrap().1).collect();
    assert_eq!(at, [0, 1, 1, 2, 2, 3, 3, 4, 4, 4, 4, 6, 7]);
}

/// A gutter sized by lines can be narrower than a Visual distance in rows.
#[test]
fn a_visual_number_wider_than_its_gutter_shows_its_last_digits() {
    let (mut ctx, b, w) = one(&"a".repeat(2004), 6, 1001);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Visual;
    let s = compose_one(&mut ctx, w);
    let vp = ctx.text(b).unwrap().viewport(w).unwrap();
    assert_eq!(vp.geometry.gutter, 4);
    assert_eq!(vp.rows[1000].number, Some(1000), "the whole number");
    assert_eq!(
        s.row_text(1000),
        "000 aa",
        "its last digits, and the text doesn't move"
    );
    assert_eq!(s.row_text(0), "  1 aa");
    assert_eq!(s.row_text(5), "  5 aa");
}

/// Rows above and below a window's edge: only its own rows are numbered, from its cursor.
#[test]
fn only_the_cursors_row_takes_the_current_lines_face() {
    let (mut ctx, b, w) = one("aaaa bbbb cccc\nd\n", 9, 5);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Visual;
    let current = h("#999999");
    let face = |s: &Scene, r: usize| s.row(r)[2].fg == current;
    let s = compose_one(&mut ctx, w);
    assert_eq!(s.row_text(0), "  1 aaaa");
    assert_eq!(s.row_text(1), "  1 bbbb");
    assert_eq!(s.row_text(2), "  2 cccc");
    assert!(face(&s, 0), "the cursor's row");
    assert!(!face(&s, 1), "the next row of its display line");
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Absolute;
    let s = compose_one(&mut ctx, w);
    assert_eq!(
        s.row_text(1),
        "    bbbb",
        "Absolute: a display line's first row only"
    );
    assert!(face(&s, 0));
    assert!(!face(&s, 3), "line 2");
}

// ---------------------------------------------------------------- speed (K15)

/// Dense prose: `lines` lines of about `width` columns, with plenty of e's.
fn prose(lines: usize, width: usize) -> String {
    let words = [
        "the", "editor", "sees", "every", "level", "here", "code", "deep", "tree", "never",
        "remember", "release",
    ];
    let (mut s, mut k) = (String::new(), 0);
    for _ in 0..lines {
        let mut line = String::new();
        while line.len() + 10 < width {
            line.push_str(words[k % words.len()]);
            line.push(' ');
            k += 1;
        }
        s.push_str(line.trim_end());
        s.push('\n');
    }
    s
}

/// A buffer of 400 dense lines shown in a `cols` × 98 window, with 4000 stored
/// faces if `faces`: a token every 14 bytes from the top, so about half are
/// off-screen in a 300-column window and more in a narrow one.
fn dense(ctx: &mut Ctx, cols: usize, faces: bool) -> WindowId {
    let b = ctx.create_text("t", &prose(400, 280));
    let w = ctx.open_window(b, cols, 98).unwrap();
    ctx.focus(w);
    let t = ctx.text_mut(b).unwrap();
    t.settings_mut().line_numbers = LineNumbers::Absolute;
    let names = ["t.k0", "t.k1", "t.k2", "t.k3"];
    for i in 0..4000 * faces as usize {
        t.decorate(i * 14..i * 14 + 5, Deco::Face(names[i % 4]))
            .unwrap();
    }
    w
}

fn speed_ctx() -> Ctx {
    ctx_with(vec![
        ("t.k0", None, Style::new().fg(h("#ff0000"))),
        ("t.k1", None, Style::new().fg(h("#00ff00")).bold()),
        ("t.k2", None, Style::new().fg(h("#0000ff")).italic()),
        ("t.k3", None, Style::new().underline(Underline::Single)),
    ])
}

/// The median of 21 composes of a `cols` × 100 frame showing `w`, in ms.
fn p50_ms(ctx: &mut Ctx, w: WindowId, cols: usize) -> f64 {
    compose_one(ctx, w);
    let mut ms: Vec<f64> = (0..21)
        .map(|_| {
            let t = std::time::Instant::now();
            std::hint::black_box(ctx.compose(cols, 100, &[at(w, 0)]));
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    ms[10]
}

/// Three tries at a limit on a ratio, so a busy machine doesn't fail it: the per-cell
/// scan this guards against misses them by far more than a machine's load.
fn within_three_tries(limit: f64, mut measure: impl FnMut() -> f64) {
    let tries: Vec<f64> = (0..3).map(|_| measure()).collect();
    assert!(
        tries.iter().any(|&m| m <= limit),
        "{tries:.2?} are all over {limit}"
    );
}

#[test]
#[ignore = "speed: runs in ci --full"]
fn an_80_column_window_with_4000_faces_and_a_one_letter_search_composes_within_1ms() {
    let mut ctx = speed_ctx();
    let w = dense(&mut ctx, 84, true);
    ctx.set_search_highlight(search("e"));
    support::within_three_tries_ms(1.0, || p50_ms(&mut ctx, w, 84));
}

#[test]
fn a_limit_in_ms_stretches_only_at_background_priority() {
    assert_eq!(support::stretch_at(Some(4)), 4.0);
    assert_eq!(support::stretch_at(Some(9)), 4.0);
    assert_eq!(support::stretch_at(Some(10)), 1.0);
    assert_eq!(support::stretch_at(Some(31)), 1.0);
    assert_eq!(support::stretch_at(None), 1.0);
}

#[test]
fn faces_and_search_cost_at_most_twice_a_plain_frame() {
    let mut ctx = speed_ctx();
    let plain = dense(&mut ctx, 300, false);
    let decorated = dense(&mut ctx, 300, true);
    within_three_tries(2.0, || {
        ctx.set_search_highlight(None);
        let p = p50_ms(&mut ctx, plain, 300);
        ctx.set_search_highlight(search("e"));
        p50_ms(&mut ctx, decorated, 300) / p
    });
}
