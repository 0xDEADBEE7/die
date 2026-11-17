//! The planner: from markdown source to the decorations that make its rendered view. It knows
//! nothing of the editor: [`plan`] reads a text and says which ranges to hide, what to show in
//! their place, what to draw that isn't in the text, and which faces go where. All offsets are
//! byte offsets into that text.
//!
//! It never panics, whatever the text, and its work grows with the text's length: the lines are
//! found once, and each rule looks only at the element it's for.
//!
//! - this file: what a plan is made of, the last pass that puts it in order, and [`Plan::preview`];
//! - `walk`: the parser's events, turned into decorations one element at a time;
//! - `code`: a code block's box;
//! - `table`: a table's columns and borders;
//! - `lines`: where lines start and end, and how many cells text takes.

use std::cmp::Reverse;
use std::ops::Range;

mod code;
mod lines;
mod table;
mod walk;

/// The faces of headings, level 1 to level 6: `markdown-h1` to `markdown-h6`.
pub const H: [&str; 6] = [
    "markdown-h1",
    "markdown-h2",
    "markdown-h3",
    "markdown-h4",
    "markdown-h5",
    "markdown-h6",
];
/// The face of `**strong**` text.
pub const BOLD: &str = "markdown-bold";
/// The face of `*emphasized*` text.
pub const ITALIC: &str = "markdown-italic";
/// The face of `~~struck~~` text.
pub const STRIKE: &str = "markdown-strike";
/// The face of inline code.
pub const CODE: &str = "markdown-code";
/// The face of a code block's box: its text, its fence rows and its padding.
pub const CODE_BLOCK: &str = "markdown-code-block";
/// The face of a link's text.
pub const LINK: &str = "markdown-link";
/// The face of a block quote's text.
pub const QUOTE: &str = "markdown-quote";
/// The face of what marks a list item: a bullet, an ordered item's number, a task's box.
pub const MARKER: &str = "markdown-marker";
/// The face of what is a line: a rule, a block quote's bar, a table's borders, and the
/// underline of a setext heading.
pub const RULE: &str = "markdown-rule";
/// The face of a table's header cells.
pub const TABLE_HEADER: &str = "markdown-table-header";
/// The face of what isn't the document's own words: HTML, front matter when it's shown, link
/// reference definitions, a fence's language, footnote labels, and what stands in for an image.
pub const META: &str = "markdown-meta";
/// The face of an alert's title: the `Note` of `> [!NOTE]`.
pub const ALERT: &str = "markdown-alert";
/// No face of its own: what's shown draws as the text under it does. A decoded entity's, which
/// is only text.
pub const PLAIN: &str = "";
/// The editor's base face, drawn over what's under it: the paper, where a code block's box
/// mustn't reach.
pub const PAPER: &str = "default";

/// Text shown that isn't in the buffer, and the face it draws in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The text: one line's worth, with no newline or tab in it.
    pub text: String,
    /// The face it draws in, over the faces of where it stands.
    pub face: &'static str,
}

/// A range of the text to hide, and what shows in its place, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hide {
    /// The bytes hidden.
    pub range: Range<usize>,
    /// What shows instead. With none, the range takes no room at all.
    pub shown: Option<Shown>,
}

/// Text shown just before the byte at `at` (or at the end of the text), not part of the buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Virtual {
    /// The byte it's drawn before, or the text's length.
    pub at: usize,
    /// The text and its face.
    pub shown: Shown,
}

/// A link or an image, for following it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// The whole element, `[text](dest)`.
    pub range: Range<usize>,
    /// The part of it that stays visible when rendered: the link's text. For an image, which is
    /// replaced whole, the same as `range`.
    pub text: Range<usize>,
    /// Where it goes, as written, a reference resolved through its definition.
    pub dest: String,
    /// Whether it's an image rather than a link.
    pub image: bool,
}

/// A heading, for `#anchor` links and for an outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// Its content, without the markers round it.
    pub range: Range<usize>,
    /// Its level, 1 to 6.
    pub level: u8,
    /// Its anchor as GitHub makes one: the text in lower case, with only letters, digits, `-`
    /// and `_` kept and spaces turned to `-`, and `-1`, `-2`… after one already taken.
    pub slug: String,
}

/// What the plan is laid out for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The text area's width in cells: how long a rule is, and where a code block's box and a
    /// table's padding stop. 0 counts as 80.
    pub width: usize,
    /// Cells per tab stop (0 counts as 1), for measuring code lines.
    pub tab_width: usize,
    /// Show front matter dimmed rather than hide it.
    pub front_matter: bool,
}

impl Default for Options {
    /// Eighty cells, a tab stop every eight, and front matter hidden.
    fn default() -> Options {
        Options {
            width: 80,
            tab_width: 8,
            front_matter: false,
        }
    }
}

/// A text's rendered view, as decorations over it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// What to hide. Sorted by start; none empty; no two overlap (touching is fine); every end
    /// on a char boundary and within the text.
    pub hides: Vec<Hide>,
    /// What to draw that isn't there. Sorted by `at`, in the order to show them where equal;
    /// none strictly inside a hide, nor at the start of one that shows nothing.
    pub virtuals: Vec<Virtual>,
    /// Faces over byte ranges, in stacking order: sorted by start, a longer range before a
    /// shorter one at the same start (outer before inner).
    pub faces: Vec<(Range<usize>, &'static str)>,
    /// The links and images, in the text's order.
    pub links: Vec<Link>,
    /// The headings, in the text's order.
    pub headings: Vec<Heading>,
}

/// The rendered view of `text`, which is CommonMark with GitHub's tables, strikethrough, task
/// lists, footnotes and alerts, and YAML front matter. It's total: any text has a plan.
pub fn plan(text: &str, options: &Options) -> Plan {
    let mut plan = walk::walk(text, options);
    plan.settle(text);
    plan
}

impl Plan {
    /// What a screen wide enough not to wrap would show, as plain text: hides applied,
    /// replacements and virtual text put in. For tests, and for looking at a plan. It draws as
    /// the editor does: virtual text strictly inside a hide, or at the start of one that shows
    /// nothing, isn't drawn.
    pub fn preview(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut virtuals = self.virtuals.iter().peekable();
        let mut at = 0;
        for hide in &self.hides {
            let Range { start, end } = hide.range;
            if start < at || end < start || text.get(start..end).is_none() {
                continue;
            }
            // What stands before the hide, each virtual text before the byte it's at.
            while let Some(v) = virtuals.next_if(|v| v.at < start) {
                if v.at >= at {
                    out.push_str(text.get(at..v.at).unwrap_or(""));
                    out.push_str(&v.shown.text);
                    at = v.at;
                }
            }
            out.push_str(text.get(at..start).unwrap_or(""));
            while let Some(v) = virtuals.next_if(|v| v.at == start) {
                if hide.shown.is_some() {
                    out.push_str(&v.shown.text);
                }
            }
            if let Some(shown) = &hide.shown {
                out.push_str(&shown.text);
            }
            // Inside it, nothing.
            while virtuals.next_if(|v| v.at < end).is_some() {}
            at = end;
        }
        for v in virtuals {
            if v.at >= at && v.at <= text.len() {
                out.push_str(text.get(at..v.at).unwrap_or(""));
                out.push_str(&v.shown.text);
                at = v.at.max(at);
            }
        }
        out.push_str(text.get(at..).unwrap_or(""));
        out
    }

    /// The last pass, over whatever the rules collected: everything within the text and on char
    /// boundaries, in order, no hide over another, no virtual text where it wouldn't be drawn,
    /// and no newline or tab in what's shown.
    fn settle(&mut self, text: &str) {
        let whole = |r: &Range<usize>| {
            r.start <= r.end
                && r.end <= text.len()
                && text.is_char_boundary(r.start)
                && text.is_char_boundary(r.end)
        };
        let Plan {
            hides,
            virtuals,
            faces,
            links,
            headings,
        } = self;

        hides.retain(|h| !h.range.is_empty() && whole(&h.range));
        for shown in hides.iter_mut().filter_map(|h| h.shown.as_mut()) {
            one_line(&mut shown.text);
        }
        // Where two overlap, the one that starts first stays, and of two that start together
        // the longer: an image over the emphasis in its alt text, say.
        hides.sort_by_key(|h| (h.range.start, Reverse(h.range.end)));
        let mut end = 0;
        hides.retain(|h| {
            let keep = h.range.start >= end;
            if keep {
                end = h.range.end;
            }
            keep
        });

        virtuals.retain(|v| !v.shown.text.is_empty() && whole(&(v.at..v.at)));
        for v in virtuals.iter_mut() {
            one_line(&mut v.shown.text);
        }
        virtuals.sort_by_key(|v| v.at);
        let mut next = 0;
        virtuals.retain(|v| {
            while hides.get(next).is_some_and(|h| h.range.end <= v.at) {
                next += 1;
            }
            hides.get(next).is_none_or(|h| {
                h.range.start > v.at || (h.range.start == v.at && h.shown.is_some())
            })
        });

        faces.retain(|(r, _)| !r.is_empty() && whole(r));
        faces.sort_by_key(|(r, _)| (r.start, Reverse(r.end)));

        // An image inside a link ended first: the link goes before it, as it starts.
        links.retain(|l| whole(&l.range) && whole(&l.text));
        links.sort_by_key(|l| (l.range.start, Reverse(l.range.end)));
        headings.retain(|h| whole(&h.range));
    }
}

/// Makes shown text one line's worth: a newline or a tab in it becomes a space.
fn one_line(text: &mut String) {
    if text.contains(['\n', '\t']) {
        *text = text.replace(['\n', '\t'], " ");
    }
}

/// Shown text in a face.
fn shown(text: impl Into<String>, face: &'static str) -> Shown {
    Shown {
        text: text.into(),
        face,
    }
}
