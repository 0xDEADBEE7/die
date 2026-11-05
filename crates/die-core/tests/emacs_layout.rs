//! K4's third check: layout against real Emacs. `emacs_layout/` holds what tty Emacs 30.2 did with
//! about 110 texts, and the scripts that asked it. Every case passes here, or differs in a way
//! contract §9 names as deliberate, and `the_differences_are_the_contracts` checks §9 still names
//! each one.
//!
//! The tty's width convention: tty Emacs keeps its last column for the continuation glyph, so a
//! window W wide has W - 1 text columns, as §9 says of the oracle's wrapped cases.

use die_core::*;

// ---------------------------------------------------------------- a little JSON, for the data files

#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(usize),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn at(&self, key: &str) -> &J {
        self.get(key)
            .unwrap_or_else(|| panic!("no {key} in {self:?}"))
    }
    fn num(&self) -> usize {
        match self {
            J::Num(n) => *n,
            _ => panic!("not a number: {self:?}"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => panic!("not an array: {self:?}"),
        }
    }
}

fn parse(text: &str) -> J {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let v = value(&b, &mut i);
    skip(&b, &mut i);
    assert_eq!(i, b.len(), "trailing text");
    v
}

fn skip(b: &[char], i: &mut usize) {
    while b.get(*i).is_some_and(|c| c.is_whitespace()) {
        *i += 1;
    }
}

fn value(b: &[char], i: &mut usize) -> J {
    skip(b, i);
    let list = |i: &mut usize, close: char, f: &mut dyn FnMut(&mut usize)| {
        *i += 1;
        skip(b, i);
        while b[*i] != close {
            f(i);
            skip(b, i);
            if b[*i] == ',' {
                *i += 1;
            }
            skip(b, i);
        }
        *i += 1;
    };
    match b[*i] {
        '[' => {
            let mut a = vec![];
            list(i, ']', &mut |i| a.push(value(b, i)));
            J::Arr(a)
        }
        '{' => {
            let mut o = vec![];
            list(i, '}', &mut |i| {
                let k = string(b, i);
                skip(b, i);
                assert_eq!(b[*i], ':');
                *i += 1;
                o.push((k, value(b, i)));
            });
            J::Obj(o)
        }
        '"' => J::Str(string(b, i)),
        't' => {
            *i += 4;
            J::Bool(true)
        }
        'n' => {
            *i += 4;
            J::Null
        }
        _ => {
            let start = *i;
            while b[*i].is_ascii_digit() {
                *i += 1;
            }
            J::Num(b[start..*i].iter().collect::<String>().parse().unwrap())
        }
    }
}

fn string(b: &[char], i: &mut usize) -> String {
    assert_eq!(b[*i], '"');
    *i += 1;
    let mut s = String::new();
    let hex = |i: &mut usize| {
        let h: String = b[*i + 1..*i + 5].iter().collect();
        *i += 5;
        u32::from_str_radix(&h, 16).unwrap()
    };
    loop {
        match b[*i] {
            '"' => {
                *i += 1;
                return s;
            }
            '\\' => {
                *i += 1;
                match b[*i] {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    'f' => s.push('\u{c}'),
                    'u' => {
                        let mut c = hex(i);
                        if (0xd800..0xdc00).contains(&c) {
                            *i += 1;
                            c = 0x10000 + ((c - 0xd800) << 10) + (hex(i) - 0xdc00);
                        }
                        s.push(char::from_u32(c).unwrap());
                        continue;
                    }
                    c => s.push(c),
                }
                *i += 1;
            }
            c => {
                s.push(c);
                *i += 1;
            }
        }
    }
}

fn data(name: &str) -> J {
    let path = format!("{}/tests/emacs_layout/{name}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read_to_string(path).unwrap())
}

// ---------------------------------------------------------------- what §9 names as deliberate

/// §9: tty Emacs counts filler toward tab stops. DIE's columns don't change with the width.
const FILLER: &str = "tty Emacs counts filler toward tab stops";
/// §9: `posn-at-point` isn't where the cursor is drawn for a wide character that moved. `cursors`
/// below checks that against the terminal's own cursor.
const MOVED: &str =
    "`posn-at-point` reports a wide character that moved whole at the column it left";

/// What a position's cell differs by, in a case with `why`.
const DIFFERENCES: &[(&str, usize, [usize; 2], &str)] = &[
    // aaaaaaaaa中→b: 中 at (1,0), the tab from column 11 to 16 here and from 12 to 16 in Emacs.
    ("tab_after_filler", 11, [7, 1], FILLER),
    ("tab_after_filler", 12, [8, 1], FILLER),
    ("tab_after_filler2", 11, [7, 1], FILLER),
    ("tab_after_filler2", 12, [8, 1], FILLER),
];

#[test]
fn the_differences_are_the_contracts() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/extension-contract.md"
    );
    let contract = std::fs::read_to_string(path).unwrap();
    for why in [FILLER, MOVED, RELATIVE] {
        assert!(contract.contains(why), "§9 doesn't say: {why}");
    }
}

// ---------------------------------------------------------------- the wrap cases

/// A case's text in a window of its text columns, with its hidden text and virtual text.
fn lay_out(c: &J, cols: usize, rows: usize) -> (Ctx, BufferId, WindowId, Vec<usize>) {
    let text = c.at("text").str();
    // Emacs counts characters; DIE counts bytes.
    let bytes: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    let t = ctx.text_mut(b).unwrap();
    if let Some(tab) = c.get("tab") {
        t.settings_mut().tab_width = tab.num();
    }
    for p in c.get("props").map_or(&[][..], J::arr) {
        let p = p.arr();
        assert_eq!(p[2].str(), "invisible");
        let deco = Deco::Hide {
            replacement: None,
            reveal: Reveal::Never,
        };
        t.decorate(bytes[p[0].num()]..bytes[p[1].num()], deco)
            .unwrap();
    }
    for o in c.get("overlays").map_or(&[][..], J::arr) {
        let o = o.arr();
        assert_eq!(o[1].str(), "before-string");
        let at = bytes[o[0].num()];
        t.decorate(at..at, Deco::Virtual(o[2].str().into()))
            .unwrap();
    }
    (ctx, b, w, bytes)
}

/// Each row's first position: its display line's start on a first row, otherwise its first cell's,
/// or else its first cursor position's. That's where `vertical-motion` lands.
fn row_starts(ctx: &mut Ctx, b: BufferId, w: WindowId, cols: usize, rows: usize) -> Vec<usize> {
    let frame = ctx.compose(
        cols,
        rows + 2,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    );
    let vp = ctx.text(b).unwrap().viewport(w).unwrap();
    vp.rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let first = (r.row == 0).then_some(r.dline_start);
            first
                .or(frame.position(i, 0).map(|(_, p)| p))
                .or(r.stops().first().map(|s| s.pos))
        })
        .collect()
}

#[test]
fn every_wrap_case_matches_tty_emacs_or_differs_as_contract_9_says() {
    let cases = data("wrap.json");
    let cases = cases.at("cases").arr();
    assert!(cases.len() >= 90, "{}", cases.len());
    let (mut compared, mut wrong, mut named_seen, mut moved_seen) = (0, vec![], 0, 0);
    for c in cases {
        let id = c.at("id").str();
        // The tty's last column holds the continuation glyph.
        let text_w = c.at("body_width").num() - 1;
        let (mut ctx, b, w, bytes) = lay_out(c, text_w, 30);
        let t = ctx.text(b).unwrap();
        for (i, want) in c.at("posns").arr().iter().enumerate() {
            let q = bytes[i];
            if !t.is_stop(q) {
                continue; // Hidden, or inside a cluster: §9's cursor positions don't include it.
            }
            compared += 1;
            let want = [want.arr()[0].num(), want.arr()[1].num()];
            let got = t.screen_pos(w, q).expect("a tall enough window");
            let got = [got.1, got.0];
            if got == want {
                continue;
            }
            let named = DIFFERENCES
                .iter()
                .any(|d| d.0 == id && d.1 == i && d.2 == got);
            // It moved whole, leaving filler: Emacs reports the column it left.
            let next = bytes
                .get(i + 1)
                .map_or(q, |n| t.snap(*n, Some(Dir::Forward)));
            let cluster_w = t.display_col(next) - t.display_col(q);
            let moved = got == [0, want[1] + 1] && want[0] + cluster_w > text_w;
            named_seen += usize::from(named);
            moved_seen += usize::from(moved);
            if !named && !moved {
                wrong.push(format!(
                    "{id}: position {i} is at {got:?}, tty Emacs says {want:?}"
                ));
            }
        }
        // Rows: every row starts where `vertical-motion` lands, except the last if it starts at
        // the end of the buffer, and a tab's rest, where it can't land on the tab it stands on.
        let text_len = c.at("text").str().len();
        let mut starts = row_starts(&mut ctx, b, w, text_w, 30);
        starts.dedup();
        starts.retain(|&s| s != text_len || text_len == 0);
        let want: Vec<usize> = c
            .at("starts")
            .arr()
            .iter()
            .map(|s| bytes[s.num()])
            .collect();
        if starts != want {
            wrong.push(format!(
                "{id}: rows start at {starts:?}, tty Emacs says {want:?}"
            ));
        }
    }
    assert!(compared > 1000, "{compared} positions");
    assert_eq!(
        named_seen,
        DIFFERENCES.len(),
        "a named difference no longer differs"
    );
    assert!(moved_seen >= 5, "{moved_seen} wide characters moved whole");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---------------------------------------------------------------- the real cursor

/// `posn-at-point` isn't the terminal's cursor for a wide character that moved. The terminal's
/// cursor is where layout puts it.
#[test]
fn the_terminals_own_cursor_is_where_layout_puts_it() {
    let cases = data("cursors.json");
    for c in cases.at("cursors").arr() {
        let (mut ctx, b, w, _) = lay_out(c, c.at("tty_cols").num() - 1, 10);
        let pos = c.at("cursor").num();
        let t = ctx.text_mut(b).unwrap();
        t.set_cursor(w, pos);
        assert_eq!(
            t.screen_pos(w, pos),
            Some((c.at("row").num(), c.at("col").num())),
            "{c:?}"
        );
    }
}

// ---------------------------------------------------------------- line numbers

/// §9: Relative counts display lines, so a fold counts once, and Emacs's count hidden lines.
const RELATIVE: &str = "Emacs's count hidden lines, so this deviates from it";

#[test]
fn every_line_number_screen_matches_tty_emacs_or_differs_as_contract_9_says() {
    let screens = data("numbers.json");
    for s in screens.at("screens").arr() {
        // A gutter of 4, then text in all but the tty's last column.
        let cols = s.at("tty_cols").num() - 1;
        let text = s.at("text").str();
        let mut ctx = Ctx::new(vec![]).unwrap();
        let b = ctx.create_text("t", text);
        let w = ctx.open_window(b, cols, 12).unwrap();
        ctx.focus(w);
        let t = ctx.text_mut(b).unwrap();
        let lnum = s.at("lnum").str();
        t.settings_mut().line_numbers = match lnum {
            "t" => LineNumbers::Absolute,
            "relative" => LineNumbers::Relative,
            _ => LineNumbers::Visual,
        };
        if let Some(f) = s.get("fold") {
            let f = f.arr();
            let deco = Deco::Hide {
                replacement: None,
                reveal: Reveal::Never,
            };
            t.decorate(f[0].num()..f[1].num(), deco).unwrap();
        }
        t.set_cursor(w, s.at("cursor").num());
        let frame = ctx.compose(
            cols,
            14,
            &[WindowPlacement {
                window: w,
                col: 0,
                row: 0,
            }],
        );
        let mut got: Vec<String> = (0..12).map(|r| frame.row_text(r)).collect();
        got.retain(|r| !r.is_empty());
        let want: Vec<&str> = s.at("rows").arr().iter().map(J::str).collect();
        if lnum == "relative" && s.get("fold").is_some() {
            // Emacs shows 1, 1, 4, 5. A fold counts once here, as `j` does.
            assert_eq!(got, ["  1 a", "  1", "  2 e", "  3 f"], "{s:?}");
            assert_eq!(want, ["  1 a", "  1 ", "  4 e", "  5 f"], "{s:?}");
            continue;
        }
        let want: Vec<&str> = want.iter().map(|r| r.trim_end()).collect();
        assert_eq!(
            got,
            want,
            "{lnum} with the cursor at {} in {text:?}",
            s.at("cursor").num()
        );
    }
}

// ---------------------------------------------------------------- `gj` and `gk`

/// The oracle's `wrapped/` cases that are all about rows, in tty Evil, 40 columns wide. die-vim
/// can't run them until V6 binds `gj` and K7 sets a window's top line, but the rows are K4's.
#[test]
fn move_visual_lands_where_tty_evils_gj_and_gk_do() {
    let words = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm\nnext\n";
    let tail = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii\n";
    for (id, text, from, dy, want) in [
        ("gj", words, 0, 1, 35),
        ("gj-keeps-column", words, 3, 1, 38),
        ("gk, after gj", words, 40, -1, 5),
        ("2gj-into-next-line", words, 0, 2, 65),
        ("gk-in-tail", tail, 41, -1, 6),
        ("gj-at-last-visual-line", "short\n", 0, 1, 6),
    ] {
        let mut ctx = Ctx::new(vec![]).unwrap();
        let b = ctx.create_text("t", text);
        let w = ctx.open_window(b, 39, 10).unwrap();
        let got = ctx.text(b).unwrap().move_visual(w, from, dy, None);
        assert_eq!(got.map(|(p, _)| p), Some(want), "{id}");
    }
}
