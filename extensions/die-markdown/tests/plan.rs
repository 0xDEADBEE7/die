//! The planner alone, with no DIE round it: what each kind of markdown shows as, and that any
//! text at all has a plan the editor can take as it is.

use die_test_cap as _;

use die_markdown::plan::{
    ALERT, BOLD, CODE, CODE_BLOCK, H, ITALIC, LINK, MARKER, META, Options, PAPER, PLAIN, Plan,
    QUOTE, RULE, STRIKE, TABLE_HEADER, plan,
};
use std::path::{Path, PathBuf};

/// A text area forty cells wide, with a tab stop every eight.
const FORTY: Options = Options {
    width: 40,
    tab_width: 8,
    front_matter: false,
};

/// What `text` shows as.
fn view(text: &str) -> String {
    plan(text, &FORTY).preview(text)
}

/// The pieces of `text` under a face, in order.
fn under<'a>(text: &'a str, plan: &Plan, face: &str) -> Vec<&'a str> {
    let faces = plan.faces.iter().filter(|(_, f)| *f == face);
    faces.map(|(range, _)| &text[range.clone()]).collect()
}

/// What stands in for hidden text, with the text it hides and its face.
fn stand_ins<'a>(text: &'a str, plan: &'a Plan) -> Vec<(&'a str, &'a str, &'a str)> {
    let shown = plan
        .hides
        .iter()
        .filter_map(|h| Some((h, h.shown.as_ref()?)));
    shown
        .map(|(h, s)| (&text[h.range.clone()], s.text.as_str(), s.face))
        .collect()
}

#[test]
fn headings() {
    let text = "\
# Title

## Two words ##

Setext *one*
line two
===

sub
---

### Title

# C# and F#

#
";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
Title

Two words

Setext one
line two
═══

sub
───

Title

C# and F#


"
    );
    // The content of each, its level and its anchor: a second `Title` isn't the first's.
    let headings: Vec<_> = p
        .headings
        .iter()
        .map(|h| (&text[h.range.clone()], h.level, h.slug.as_str()))
        .collect();
    assert_eq!(
        headings,
        [
            ("Title", 1, "title"),
            ("Two words", 2, "two-words"),
            ("Setext *one*\nline two", 1, "setext-one-line-two"),
            ("sub", 2, "sub"),
            ("Title", 3, "title-1"),
            ("C# and F#", 1, "c-and-f"),
            ("", 1, ""),
        ]
    );
    assert_eq!(
        under(text, &p, H[0]),
        ["Title", "Setext *one*", "line two", "C# and F#"]
    );
    assert_eq!(under(text, &p, H[1]), ["Two words", "sub"]);
    assert_eq!(under(text, &p, H[2]), ["Title"]);
    // An underline is a line, as long as it was.
    assert_eq!(
        stand_ins(text, &p),
        [("===", "═══", RULE), ("---", "───", RULE)]
    );

    // An anchor keeps letters of any script, digits, `-` and `_`.
    let text = "## Ünï-cödé_x 1.2!\n\n# a `c` *e* [l](u) &amp;\n";
    let slugs: Vec<_> = slugs_of(text);
    assert_eq!(slugs, ["ünï-cödé_x-12", "a-c-e-l-"]);
}

/// The anchors of a text's headings.
fn slugs_of(text: &str) -> Vec<String> {
    let p = plan(text, &FORTY);
    p.headings.into_iter().map(|h| h.slug).collect()
}

#[test]
fn emphasis() {
    let text = "*a* _b_ **c** __d__ ~~e~~ ~f~ ***g*** **h *i* j**\n";
    let p = plan(text, &FORTY);
    assert_eq!(p.preview(text), "a b c d e f g h i j\n");
    assert_eq!(under(text, &p, ITALIC), ["a", "b", "**g**", "i"]);
    assert_eq!(under(text, &p, BOLD), ["c", "d", "g", "h *i* j"]);
    assert_eq!(under(text, &p, STRIKE), ["e", "f"]);
    // Delimiters that open or close nothing stay.
    assert_eq!(
        view("2 * 3 * 4, snake_case_name, **open\n"),
        "2 * 3 * 4, snake_case_name, **open\n"
    );
}

#[test]
fn inline_code() {
    // One space at each end goes with the backticks, unless there's nothing but spaces.
    let text = "`code` `` a`b `` ` x ` `  ` `*not*`\n";
    let p = plan(text, &FORTY);
    assert_eq!(p.preview(text), "code a`b x    *not*\n");
    assert_eq!(under(text, &p, CODE), ["code", "a`b", "x", "  ", "*not*"]);
    // Over a line end the two rows stay two.
    assert_eq!(view("`a\nb`\n"), "a\nb\n");
    assert_eq!(view("a ` unclosed\n"), "a ` unclosed\n");
}

#[test]
fn links() {
    let text = "\
[text](dest \"title\") [t *e* `]` \\] x][ref] [col][] [short] <https://x.y> <a@b.c> [a [b] c](u) [un]

[ref]: http://r \"T\"
[col]: <http://c>
[short]: /s
";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
text t e ] ] x col short https://x.y a@b.c a [b] c [un]

[ref]: http://r \"T\"
[col]: <http://c>
[short]: /s
"
    );
    let links: Vec<_> = p
        .links
        .iter()
        .map(|l| {
            assert!(!l.image);
            (
                &text[l.range.clone()],
                &text[l.text.clone()],
                l.dest.as_str(),
            )
        })
        .collect();
    assert_eq!(
        links,
        [
            ("[text](dest \"title\")", "text", "dest"),
            ("[t *e* `]` \\] x][ref]", "t *e* `]` \\] x", "http://r"),
            ("[col][]", "col", "http://c"),
            ("[short]", "short", "/s"),
            ("<https://x.y>", "https://x.y", "https://x.y"),
            // Mail to it, though the parser gives the address alone.
            ("<a@b.c>", "a@b.c", "mailto:a@b.c"),
            ("[a [b] c](u)", "a [b] c", "u"),
        ]
    );
    assert_eq!(
        under(text, &p, LINK),
        [
            "text",
            "t *e* `]` \\] x",
            "col",
            "short",
            "https://x.y",
            "a@b.c",
            "a [b] c"
        ]
    );
    // The definitions stay as they're written, dimmed.
    assert_eq!(
        under(text, &p, META),
        ["[ref]: http://r \"T\"", "[col]: <http://c>", "[short]: /s"]
    );

    // A link's face goes on before those of what's inside it, whatever their ranges.
    let text = "[<b>](u) [**a**](v)\n";
    let p = plan(text, &FORTY);
    let faces: Vec<_> = p
        .faces
        .iter()
        .map(|(r, f)| (&text[r.clone()], *f))
        .collect();
    assert_eq!(
        faces,
        [("<b>", LINK), ("<b>", META), ("**a**", LINK), ("a", BOLD)]
    );
    // And a definition inside a quote is dimmed over the quote's face.
    let text = "> [a]: /u\n";
    let p = plan(text, &FORTY);
    let faces: Vec<_> = p
        .faces
        .iter()
        .map(|(r, f)| (&text[r.clone()], *f))
        .collect();
    assert_eq!(faces, [("[a]: /u", QUOTE), ("[a]: /u", META)]);
}

#[test]
fn images() {
    let text = "\
![alt *x*](src/p.png) ![](a/b.png) ![a][] [![in](s)](d)

[a]: /u
";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
‹image: alt x› ‹image: b.png› ‹image: a› ‹image: in›

[a]: /u
"
    );
    let links: Vec<_> = p
        .links
        .iter()
        .map(|l| {
            (
                &text[l.range.clone()],
                &text[l.text.clone()],
                l.dest.as_str(),
                l.image,
            )
        })
        .collect();
    assert_eq!(
        links,
        [
            (
                "![alt *x*](src/p.png)",
                "![alt *x*](src/p.png)",
                "src/p.png",
                true
            ),
            ("![](a/b.png)", "![](a/b.png)", "a/b.png", true),
            ("![a][]", "![a][]", "/u", true),
            ("[![in](s)](d)", "![in](s)", "d", false),
            ("![in](s)", "![in](s)", "s", true),
        ]
    );
    // An image is replaced whole: nothing in its alt text is decorated apart.
    assert_eq!(under(text, &p, ITALIC), [""; 0]);
    let stand_ins = stand_ins(text, &p);
    assert_eq!(stand_ins.len(), 4);
    assert_eq!(
        stand_ins[0],
        ("![alt *x*](src/p.png)", "‹image: alt x›", META)
    );
}

#[test]
fn lists_and_tasks() {
    let text = "\
- one
  - two
    * three
      + four
- [x] done
- [ ] todo
-   [X] wide

1. first
2. [ ] task
10) paren
    - under
";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
• one
  ◦ two
    ▪ three
      • four
☑ done
☐ todo
☑ wide

1. first
2. ☐ task
10) paren
    • under
"
    );
    // An ordered item's number stays, as a marker; bullets and boxes are markers too.
    assert_eq!(under(text, &p, MARKER), ["1.", "2.", "10)"]);
    assert!(stand_ins(text, &p).iter().all(|s| s.2 == MARKER));
    // A box stands where its bullet was: the bullet and the blanks after it show nothing.
    let bullet = p.hides.iter().find(|h| &text[h.range.clone()] == "-   ");
    assert_eq!(bullet.map(|h| h.shown.is_none()), Some(true));
}

#[test]
fn quotes_and_alerts() {
    let text = "\
> quoted **text**
> more
lazy

> > nested
> - item
>   > inner

> [!NOTE]
> body

- > in list
  > second
";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
▎ quoted text
▎ more
lazy

▎ ▎ nested
▎ • item
▎   ▎ inner

▎ Note
▎ body

• ▎ in list
  ▎ second
"
    );
    // Each quote's text on each of its lines, an outer quote's before the one inside it.
    assert_eq!(
        under(text, &p, QUOTE),
        [
            "quoted **text**",
            "more",
            "lazy",
            "> nested",
            "nested",
            "- item",
            "  > inner",
            "inner",
            "[!NOTE]",
            "body",
            "in list",
            "second"
        ]
    );
    let stand_ins = stand_ins(text, &p);
    assert!(stand_ins.contains(&(">", "▎", RULE)));
    assert!(stand_ins.contains(&("[!NOTE]", "Note", ALERT)));
    assert_eq!(view(">[!WARNING]\n>tight\n"), "▎Warning\n▎tight\n");
    assert_eq!(view("> [!caution]\n> x\n"), "▎ Caution\n▎ x\n");
}

#[test]
fn rules() {
    let text = "---\n\n***\n ___\n- - -\n> ---\n";
    let p = plan(text, &FORTY);
    let line = |cells: usize| "─".repeat(cells);
    assert_eq!(
        p.preview(text),
        format!(
            "{}\n\n{}\n {}\n{}\n▎ {}\n",
            line(40),
            line(40),
            line(39),
            line(40),
            line(38)
        )
    );
    assert!(
        p.hides
            .iter()
            .all(|h| h.shown.as_ref().unwrap().face == RULE)
    );
    // Never shorter than three cells, and eighty cells wide when no width is given.
    let narrow = Options { width: 2, ..FORTY };
    assert_eq!(view_with("---\n", &narrow), "───\n");
    let unset = Options { width: 0, ..FORTY };
    assert_eq!(view_with("---\n", &unset), format!("{}\n", line(80)));
    // A `\r` alone ends the rule's line for the parser, though not the editor's row: what
    // follows it there is text, and stays.
    assert_eq!(view("---\rtext\n"), format!("{}\rtext\n", line(40)));
}

/// What `text` shows as under these options.
fn view_with(text: &str, options: &Options) -> String {
    plan(text, options).preview(text)
}

#[test]
fn fenced_code() {
    // A row for each line, as wide as the longest and a cell more; a tab runs to its stop.
    let text = "```rust ignore\nfn main() {\n\tlet s = \"é\";\n}\n```\n";
    let p = plan(text, &FORTY);
    let rows = [
        ("rust", 17),
        ("fn main() {", 10),
        ("\tlet s = \"é\";", 1),
        ("}", 20),
        ("", 21),
    ];
    let expected: String = rows
        .iter()
        .map(|(row, pad)| format!("{row}{}\n", " ".repeat(*pad)))
        .collect();
    assert_eq!(p.preview(text), expected);
    assert_eq!(
        under(text, &p, CODE_BLOCK),
        [
            "```rust ignore\n",
            "fn main() {\n",
            "\tlet s = \"é\";\n",
            "}\n",
            "```\n"
        ]
    );
    assert_eq!(stand_ins(text, &p), [("```rust ignore", "rust", META)]);
    assert!(p.virtuals.iter().all(|v| v.shown.face == CODE_BLOCK));

    // In a container the box starts where the fence does.
    assert_eq!(
        view("- item\n  ```sh\n  ls\n  ```\n"),
        "• item\n  sh \n  ls \n     \n"
    );
    assert_eq!(
        view("> ~~~\n> code\n> ~~~\n> after\n"),
        "▎      \n▎ code \n▎      \n▎ after\n"
    );
    // A fence left open has no closing row, and a longer fence holds a shorter one.
    assert_eq!(view("```\nabc"), "    \nabc ");
    assert_eq!(view("````md\n```\n````\n"), "md  \n``` \n    \n");
    // The box stops at the text area's edge: a line that reaches it has no padding.
    let ten = Options { width: 10, ..FORTY };
    assert_eq!(
        view_with("```\n0123456789abc\nxy\n```\n", &ten),
        "          \n0123456789abc\nxy        \n          \n"
    );
}

#[test]
fn indented_code() {
    let text = "para\n\n    indented\n    code\n\n    more\n\nafter\n";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "para\n\n    indented \n    code     \n             \n    more     \n\nafter\n"
    );
    // The box starts at the code, not at its indentation.
    assert_eq!(
        under(text, &p, CODE_BLOCK),
        ["indented\n", "code\n", "\n", "more\n"]
    );
    assert!(p.hides.is_empty());
    // The blank line hasn't the indentation the box starts after: the paper stands in for it,
    // then the box, so its left edge is straight.
    let blank: Vec<(&str, &str)> = p
        .virtuals
        .iter()
        .filter(|v| v.at == text.find("\n\n    more").unwrap() + 1)
        .map(|v| (v.shown.text.as_str(), v.shown.face))
        .collect();
    assert_eq!(blank, [("    ", PAPER), ("         ", CODE_BLOCK)]);
}

#[test]
fn tables() {
    // Columns as wide as their widest cell, aligned as the delimiter row says.
    let text = "| Name | Qty |\n|:--|--:|\n| apple | 3 |\n| kiwi | 12 |\n";
    let p = plan(text, &FORTY);
    assert_eq!(
        p.preview(text),
        "\
│ Name  │ Qty │
├───────┼─────┤
│ apple │   3 │
│ kiwi  │  12 │
"
    );
    assert_eq!(under(text, &p, TABLE_HEADER), ["Name", "Qty"]);
    assert!(p.virtuals.is_empty());
    assert!(stand_ins(text, &p).iter().all(|s| s.2 == RULE));

    // A source nobody lined up: a cell's width is what it shows as, an escaped pipe is no
    // border, a centred column splits its spare cells, and a short row is short.
    assert_eq!(
        view(
            "\
|a `x\\|y`|  b \\| c  |**d**|
| :-: | --- | ---: |
|  | [l](u) |   |
| only |
|| x |
| &amp; | ![i](s) | \\* |
"
        ),
        "\
│ a x|y │ b | c      │   d │
├───────┼────────────┼─────┤
│       │ l          │     │
│ only  │
│       │ x          │
│   &   │ ‹image: i› │   * │
"
    );
    // A wide character is two cells of its column.
    assert_eq!(
        view("| 日本 | b |\n|---|---|\n| a | c |\n"),
        "│ 日本 │ b   │\n├──────┼─────┤\n│ a    │ c   │\n"
    );
    // Rows written without pipes at their ends have no border there.
    assert_eq!(view("a | b\n-|-\n1 | 2\n"), "a   │ b\n────┼────\n1   │ 2\n");
    // Inside a quote inside a list, after their markers.
    assert_eq!(
        view("- > | q | r |\n  > |---|---|\n  > | 1 | 2 |\n"),
        "• ▎ │ q   │ r   │\n  ▎ ├─────┼─────┤\n  ▎ │ 1   │ 2   │\n"
    );
    // A row with a cell too many, which the parser drops: the table stays as it's written,
    // but for what's inside its cells.
    assert_eq!(
        view("| *a* |\n|---|\n| x | y |\n"),
        "| a |\n|---|\n| x | y |\n"
    );
    // A table too wide for the text area: its widest column gives way, so that padding
    // pushes no row past the edge, and only the row with the long cell runs over.
    let twenty = Options { width: 20, ..FORTY };
    assert_eq!(
        view_with(
            "| a | a long long long cell |\n|---|---|\n| b | c |\n",
            &twenty
        ),
        "\
│ a   │ a long long long cell │
├─────┼────────────┤
│ b   │ c          │
"
    );
}

#[test]
fn front_matter() {
    let text = "---\ntitle: x\n---\n\n# Body\n";
    let hidden = plan(text, &FORTY);
    assert_eq!(hidden.preview(text), "\nBody\n");
    assert_eq!(hidden.hides[0].range, 0..17);
    // Shown, it's dimmed and nothing of it is hidden.
    let options = Options {
        front_matter: true,
        ..FORTY
    };
    let shown = plan(text, &options);
    assert_eq!(shown.preview(text), "---\ntitle: x\n---\n\nBody\n");
    assert_eq!(under(text, &shown, META), ["---", "title: x", "---"]);
    // Only at the very top is such a block front matter.
    let text = "para\n\n---\na: b\n---\n";
    let p = plan(text, &FORTY);
    assert_eq!(p.preview(text), text);
    assert_eq!(under(text, &p, META), ["---", "a: b", "---"]);
}

#[test]
fn escapes_and_entities() {
    let text = "a \\* b \\\\ c\\\nnext &amp; &#123; &copy; &nope; \\a `\\*`\n";
    let p = plan(text, &FORTY);
    assert_eq!(p.preview(text), "a * b \\ c\nnext & { © &nope; \\a \\*\n");
    // An entity shows as what it decodes to, in no face of its own.
    assert_eq!(
        stand_ins(text, &p),
        [
            ("&amp;", "&", PLAIN),
            ("&#123;", "{", PLAIN),
            ("&copy;", "©", PLAIN)
        ]
    );
    assert_eq!(
        view("\\# no heading\n\n\\- no item\n"),
        "# no heading\n\n- no item\n"
    );
    // One that decodes to a line end or a tab can't put one in a replacement.
    assert_eq!(view("a&#10;b&Tab;c\n"), "a b c\n");
}

#[test]
fn footnotes_and_html() {
    let text = "\
a[^1] <b>in</b> <!-- c -->

[^1]: note

<div>
block
</div>
";
    let p = plan(text, &FORTY);
    assert_eq!(p.preview(text), text);
    assert!(p.hides.is_empty());
    assert_eq!(
        under(text, &p, META),
        [
            "[^1]",
            "<b>",
            "</b>",
            "<!-- c -->",
            "[^1]:",
            "<div>",
            "block",
            "</div>"
        ]
    );
}

// ---------------------------------------------------------------- any text has a plan

/// Checks what the editor relies on in the plan of `text`. `what` names the text in a failure.
fn check(what: &str, text: &str, options: &Options) {
    let p = plan(text, options);
    let on_chars = |at: usize| at <= text.len() && text.is_char_boundary(at);
    let one_line = |s: &str| !s.contains(['\n', '\t']);

    let mut end = 0;
    for hide in &p.hides {
        let range = &hide.range;
        assert!(range.start < range.end, "{what}: an empty hide {range:?}");
        assert!(
            range.start >= end,
            "{what}: hides out of order or overlapping at {range:?}"
        );
        assert!(
            on_chars(range.start) && on_chars(range.end),
            "{what}: hide {range:?}"
        );
        let shown = hide.shown.as_ref();
        assert!(
            shown.is_none_or(|s| one_line(&s.text)),
            "{what}: shown at {range:?}"
        );
        end = range.end;
    }

    let mut last = 0;
    for v in &p.virtuals {
        assert!(
            v.at >= last,
            "{what}: virtual text out of order at {}",
            v.at
        );
        assert!(on_chars(v.at), "{what}: virtual text at {}", v.at);
        assert!(one_line(&v.shown.text), "{what}: virtual text at {}", v.at);
        // Not where it wouldn't be drawn.
        let next = p.hides.partition_point(|h| h.range.end <= v.at);
        let drawn = p
            .hides
            .get(next)
            .is_none_or(|h| h.range.start > v.at || (h.range.start == v.at && h.shown.is_some()));
        assert!(drawn, "{what}: virtual text at {} is in a hide", v.at);
        last = v.at;
    }

    let mut before = (0, usize::MAX);
    for (range, _) in &p.faces {
        assert!(range.start < range.end, "{what}: an empty face {range:?}");
        assert!(
            on_chars(range.start) && on_chars(range.end),
            "{what}: face {range:?}"
        );
        let ordered = range.start > before.0 || (range.start == before.0 && range.end <= before.1);
        assert!(ordered, "{what}: faces out of order at {range:?}");
        before = (range.start, range.end);
    }

    for link in &p.links {
        assert!(
            text.get(link.range.clone()).is_some(),
            "{what}: link {link:?}"
        );
        assert!(
            text.get(link.text.clone()).is_some(),
            "{what}: link {link:?}"
        );
    }
    for heading in &p.headings {
        assert!(
            text.get(heading.range.clone()).is_some(),
            "{what}: {heading:?}"
        );
        assert!((1..=6).contains(&heading.level), "{what}: {heading:?}");
    }
    let _ = p.preview(text);
}

/// Every markdown file under a directory.
fn markdown_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown_under(&path, found);
        } else if path.extension().is_some_and(|e| e == "md") {
            found.push(path);
        }
    }
}

/// `text` cut short at `cuts` places spread evenly over it, each on a char boundary.
fn cut_short(text: &str, cuts: usize) -> impl Iterator<Item = &str> {
    (1..cuts).map(move |i| {
        let mut at = text.len() * i / cuts;
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        &text[..at]
    })
}

/// Texts made to trip a rule up: constructs left open, line ends of every kind, characters
/// that aren't one cell or one byte, and containers inside containers.
const NASTY: &[&str] = &[
    "",
    "\n",
    "\r",
    "\0",
    "a\0b\n# \0\n\n```\n\0\u{85}\n```\n",
    "```",
    "```\nunclosed fence",
    "~~~~ info with spaces\n~~~\n",
    "> ```\n> in a quote, unclosed",
    "- ```\n  code\n\n      more\n",
    "|",
    "|\n|",
    "|\n|-",
    "| a |\n|-",
    "| a | b |\n|---|---|\n| 1 | 2 | 3 | 4 |\n|\n||\n| | |\n",
    "a|b\n-|-\n\\|\n`|`|`\\|`\n",
    "| \ttab | b |\n|---|---|\n| a\tb | c |\n",
    "a\r\n\r\n# h\r\n\r\nsetext\r\n===\r\n\r\n```rs\r\ncode\r\n```\r\n\r\n| a | b |\r\n|---|---|\r\n| 1 | 2 |\r\n\r\n---\r\n> q\r\n- [ ] x\r\n",
    "one\rtwo\r# classic\r\r- mac\r",
    "e\u{301}\u{301}\u{301} *a\u{301}* `\u{301}` | \u{301}\n\n# \u{301}\n\n\u{301}\n===\n",
    "👨‍👩‍👧 **👨‍👩‍👧** `👨‍👩‍👧` [👨‍👩‍👧](👨‍👩‍👧) ![👨‍👩‍👧](👨‍👩‍👧)\n\n| 👨‍👩‍👧 | 日本語 |\n|:-:|--:|\n| 1 | 2 |\n\n```👨‍👩‍👧\n👨‍👩‍👧\t👨‍👩‍👧\n```\n",
    "> - | a | b |\n>   |---|---|\n>   | 1 | **2** |\n>\n>   ```\n>   code\n>   ```\n> > - [x] deep\n",
    "> > > > > a\n> > > > b\n> > > c\n> > d\n> e\nf\n",
    "- a\n  - b\n    - c\n      - d\n        - e\n          1. f\n             - [ ] g\n",
    "[",
    "![",
    "[a](",
    "[a][",
    "[a][]",
    "![a][]\n\n[a]: /x",
    "[![a][]](u)\n\n[a]: /x\n",
    "[a `]` \\] [b] c](u) [`x](y)` [x][`]`]\n",
    "<",
    "<a@b.c",
    "<https://x.y> <b <!-- \n-->",
    "*",
    "**",
    "***a**b*",
    "*a **b* c**",
    "~",
    "~~a~",
    "`",
    "`` ` ``",
    "` `` `",
    "`\n`",
    "#",
    "####### seven\n===\n",
    "# #\n## ##\n### a ###   \n# a#\n# \\#\n#\t\ttabbed\t\t#\t\n",
    "=\n=\n",
    "a\n-\n",
    ">",
    "> [!NOTE",
    "> [!NOTE]",
    ">\t[!TIP]\ttab\n",
    "-",
    "- [ ]",
    "- [x]\n-\t[ ] tab\n- \t[x] tabs\n1.\t[ ] x\n1. [ ]\n",
    "---\rrule\r# h #\rtext #\r```\rcode\r```\r| a | b |\r|---|---|\r| 1 | 2 |\r> q\r> r\rsetext\r===\r",
    "1.\n2)\n",
    "---",
    "---\n",
    "---\na: b",
    "---\na: b\n---",
    "---\na: b\n---\n---\nc: d\n---\n",
    " ---\n_ _ _\n*\t*\t*\n",
    "&",
    "&amp",
    "&amp;&#0;&#10;&#9;&#x110000;&Tab;&NewLine;",
    "\\",
    "\\\n",
    "a\\\n\\\nb  \n",
    "\\\\\\*\\`\\|\\\u{301}",
    "[^",
    "[^1]",
    "[^1]:",
    "a[^1]\n\n[^1]: one\n    two\n\n    > three\n",
    "<div>\n*not markdown*\n\n</div>\n<!--\n",
    "    code\n\ttab\n  \n \tmixed\n",
    "\t\t\tdeep\n",
    "text\u{2028}separator\u{2029}paragraph\u{b}vt\u{c}ff\n",
];

#[test]
fn any_text_has_a_plan_the_editor_can_take() {
    let variants = [
        FORTY,
        Options {
            width: 0,
            tab_width: 0,
            front_matter: true,
        },
        Options {
            width: 1,
            tab_width: 3,
            front_matter: false,
        },
        Options {
            width: usize::MAX,
            tab_width: usize::MAX,
            front_matter: false,
        },
    ];
    // The nasty ones, whole and cut short at every character, under every kind of options.
    for (i, text) in NASTY.iter().enumerate() {
        for (at, _) in text.char_indices().chain([(text.len(), ' ')]) {
            for (o, options) in variants.iter().enumerate() {
                check(
                    &format!("nasty text {i} cut at {at}, options {o}"),
                    &text[..at],
                    options,
                );
            }
        }
    }

    // DIE's own documents, whole and cut short at two hundred places.
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut files = Vec::new();
    markdown_under(&docs, &mut files);
    assert!(!files.is_empty(), "no markdown under {}", docs.display());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let name = file.display();
        check(&format!("{name}"), &text, &FORTY);
        check(&format!("{name}, shown front matter"), &text, &variants[1]);
        for cut in cut_short(&text, 200) {
            check(&format!("{name} cut at {}", cut.len()), cut, &FORTY);
        }
    }
}
