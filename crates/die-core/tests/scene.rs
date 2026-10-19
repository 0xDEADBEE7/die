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
//! - `faces_layer_over_default`: a face that sets only a background keeps
//!   `default`'s foreground, and a layer above changes only what it sets.
//! - `one_window_snapshot`: the full snapshot of a window with line numbers,
//!   its status line and the command row.
//! - `two_windows_divider_status_lines_and_hollow_cursor`
//! - `the_prompt_takes_the_command_row_and_the_cursor`
//! - `cursor_shape_follows_the_state`: block, bar, half block.
//! - `search_matches_and_the_current_match`, `bracket_match`,
//!   `visual_selection_char_line_and_block`: render-time highlights.
//! - `wide_characters_and_clusters`: continuation cells and graphemes.
//! - `placements_past_the_frame_are_clipped`
//! - `positions_map_cells_to_buffer_positions`
//! - `rgb_hex_and_display`

use die_core::*;

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
    let names: Vec<&str> = BASE_FACES.iter().chain(&UI_FACES).map(|f| f.0).collect();
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
    ctx.set_search_highlight(Some("c".into()));
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
    ctx.set_search_highlight(Some("ab".into()));
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
