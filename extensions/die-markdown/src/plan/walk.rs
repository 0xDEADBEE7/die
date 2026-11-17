//! The parser's events, turned into decorations one element at a time. pulldown-cmark says what
//! each element is and where it starts and ends; where its markup is inside that, the rules here
//! find by looking at the element's own bytes, and check before they hide anything: an element
//! that doesn't look as expected keeps its markup.
//!
//! What the parser's ranges cover, as found by trying it (0.13.6):
//!
//! - an escape's backslash is in no event: the text after it starts a new `Text`;
//! - an entity is a `Text` of its decoded character, over the entity's bytes;
//! - a collapsed reference's `[]` is outside its link's or image's range;
//! - a block quote, a list item, a rule, a fence and a table row start at their own first
//!   character, after any indentation or enclosing container's markers; an indented code block
//!   starts at its first character of code;
//! - a fenced code block ends with its closing fence, without that line's newline, and its code
//!   comes as `Text` pieces that start a new one wherever something was taken off a line's start;
//! - a table cell is what's between two pipes, whitespace included; a cell a short row doesn't
//!   have is an empty range at the row's end, and one past the header's count has no event;
//! - a table's delimiter row and a link reference definition have no events at all: the
//!   definitions' spans come from the parser's own list of them;
//! - a heading runs through its line's newline, a setext one through its underline's;
//! - a task box's range starts at a tab before it, if one is there;
//! - a YAML block is reported wherever a block can start, not only at the top of the text;
//! - a line ends at a `\r` alone too, as CommonMark has it.

use super::lines::{Lines, columns, is_blank, sub};
use super::table::{Cell, Row};
use super::{
    ALERT, BOLD, CODE, H, Heading, Hide, ITALIC, LINK, Link, MARKER, META, Options, PLAIN, Plan,
    QUOTE, RULE, STRIKE, Shown, TABLE_HEADER, Virtual, code, one_line, shown, table,
};
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, LinkType, Options as Syntax, Parser, Tag,
};
use std::collections::HashMap;
use std::ops::Range;

/// A bullet's stand-in, by how deep its list is among bullet lists.
const BULLETS: [&str; 3] = ["•", "◦", "▪"];

/// The widest text area a plan is laid out for, and the widest tab stop: a rule or a row of
/// padding is this long at most, whatever is asked for.
const WIDEST: usize = 4096;

/// The plan of `text` as the rules make it, before the last pass puts it in order.
pub(super) fn walk(text: &str, options: &Options) -> Plan {
    let syntax = Syntax::ENABLE_TABLES
        | Syntax::ENABLE_STRIKETHROUGH
        | Syntax::ENABLE_TASKLISTS
        | Syntax::ENABLE_FOOTNOTES
        | Syntax::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Syntax::ENABLE_GFM;
    let parser = Parser::new_ext(text, syntax);
    let mut walk = Walk::new(text, options);
    // Link reference definitions, which no event shows. Their face goes on after the events'
    // faces, so over a block quote's where a definition is inside one.
    let mut definitions: Vec<Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .collect();
    definitions.sort_by_key(|span| span.start);
    for (event, range) in parser.into_offset_iter() {
        walk.event(event, range);
    }
    for span in definitions {
        walk.face_lines(span, META);
    }
    walk.out
}

/// An element the walk is inside, with what its end needs.
enum Frame {
    /// Nothing to do at its end.
    Other,
    /// A list. A bullet list makes the bullets inside it one deeper.
    List { bullet: bool },
    /// A list item: where its marker is, and the hide over a bullet, which a task box replaces.
    Item { start: usize, bullet: Option<usize> },
    /// A block quote.
    Quote,
    /// A heading: its content's range, and its text as it's read.
    Heading {
        level: u8,
        range: Range<usize>,
        title: String,
    },
    /// A link: where it goes, and where its face goes among the faces, before those inside it.
    Link {
        kind: LinkType,
        dest: String,
        faces: usize,
    },
    /// An image: where it goes, and its alt text as it's read.
    Image {
        kind: LinkType,
        dest: String,
        alt: String,
    },
    /// A code block: a fence's info string, and the pieces of code the parser gives.
    Code {
        info: Option<String>,
        chunks: Vec<Range<usize>>,
    },
    /// A block whose text is no markdown: front matter.
    Verbatim,
    /// A table: its columns' alignments, and its rows as they end.
    Table {
        aligns: Vec<Alignment>,
        rows: Vec<Row>,
    },
    /// A table row: its range, and its cells as they end.
    Row {
        header: bool,
        range: Range<usize>,
        cells: Vec<Cell>,
    },
    /// A table cell: how many hides there were before it.
    Cell { hides: usize },
}

/// The walk over one text's events.
pub(super) struct Walk<'a> {
    /// The text.
    pub(super) text: &'a str,
    /// Its lines.
    pub(super) lines: Lines<'a>,
    /// The text area's width in cells.
    pub(super) width: usize,
    /// Cells per tab stop.
    pub(super) tab: usize,
    /// Whether front matter shows.
    show_front_matter: bool,
    /// The plan so far, in no order yet.
    out: Plan,
    /// The elements the walk is inside, outermost first.
    stack: Vec<Frame>,
    /// Where the latest event that wasn't a start ended: text before a `Text` that no event
    /// covered is markup.
    covered: usize,
    /// How many block quotes, bullet lists, tables, and blocks of text that's no markdown the
    /// walk is inside.
    quotes: usize,
    bullets: usize,
    tables: usize,
    verbatim: usize,
    /// The heading the walk is inside, as a place in the stack.
    heading: Option<usize>,
    /// The outermost image the walk is inside, as a place in the stack. Everything in it is its
    /// alt text, and gets no decoration of its own.
    image: Option<usize>,
    /// For each line asked about, where the `>` of the run that starts it are.
    marks: HashMap<usize, Vec<usize>>,
    /// The slugs taken, and how many times each was asked for again.
    slugs: HashMap<String, usize>,
}

impl<'a> Walk<'a> {
    fn new(text: &'a str, options: &Options) -> Walk<'a> {
        Walk {
            text,
            lines: Lines::new(text),
            width: match options.width {
                0 => 80,
                width => width.min(WIDEST),
            },
            tab: options.tab_width.clamp(1, WIDEST),
            show_front_matter: options.front_matter,
            out: Plan::default(),
            stack: Vec::new(),
            covered: 0,
            quotes: 0,
            bullets: 0,
            tables: 0,
            verbatim: 0,
            heading: None,
            image: None,
            marks: HashMap::new(),
            slugs: HashMap::new(),
        }
    }

    /// The byte at `at`, or a NUL past the end: no markup is a NUL.
    fn byte(&self, at: usize) -> u8 {
        self.text.as_bytes().get(at).copied().unwrap_or(0)
    }

    /// How many of `byte` run from `at`.
    fn run(&self, at: usize, byte: u8) -> usize {
        let bytes = self.text.as_bytes();
        let rest = bytes.get(at..).unwrap_or_default();
        rest.iter().take_while(|b| **b == byte).count()
    }

    /// Hides a range, behind `shown` if there's any. What's shown is one line's worth from the
    /// start, so that a table measures it as it will be drawn.
    pub(super) fn hide(&mut self, range: Range<usize>, mut shown: Option<Shown>) {
        if range.start < range.end {
            if let Some(shown) = &mut shown {
                one_line(&mut shown.text);
            }
            self.out.hides.push(Hide { range, shown });
        }
    }

    /// Puts a face over a range.
    pub(super) fn face(&mut self, range: Range<usize>, face: &'static str) {
        if range.start < range.end {
            self.out.faces.push((range, face));
        }
    }

    /// Shows text that isn't there before the byte at `at`.
    pub(super) fn virtual_text(&mut self, at: usize, shown: Shown) {
        self.out.virtuals.push(Virtual { at, shown });
    }

    /// Puts a face over each line of a range, without the markers and indentation a container
    /// puts before the lines after the first, or the blanks at a line's end.
    fn face_lines(&mut self, range: Range<usize>, face: &'static str) {
        if range.is_empty() || range.end > self.text.len() {
            return;
        }
        let first = self.lines.index(range.start);
        for line in first..=self.lines.index(range.end - 1) {
            let from = match line == first {
                true => range.start,
                false => self.lines.inset(line),
            };
            let to = self.lines.trimmed(line, from).min(range.end);
            self.face(from..to, face);
        }
    }

    /// Adds to the text of the heading the walk is inside.
    fn title(&mut self, text: &str) {
        if let Some(Frame::Heading { title, .. }) = self.heading.and_then(|i| self.stack.get_mut(i))
        {
            title.push_str(text);
        }
    }

    // -------------------------------------------------------------- events

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        if let Some(image) = self.image {
            return self.in_image(image, event, range);
        }
        match event {
            Event::Start(tag) => return self.start(tag, range),
            Event::End(_) => self.end(range.clone()),
            Event::Text(text) => self.text(&text, &range),
            Event::Code(code) => self.code(&code, &range),
            Event::Html(_) | Event::InlineHtml(_) => self.face_lines(range.clone(), META),
            Event::FootnoteReference(_) => self.face(range.clone(), META),
            Event::SoftBreak => self.title(" "),
            Event::HardBreak => {
                // A backslash that breaks the line is markup; two spaces already show nothing.
                if self.byte(range.start) == b'\\' {
                    self.hide(range.start..range.start + 1, None);
                }
                self.title(" ");
            }
            Event::Rule => self.rule(&range),
            Event::TaskListMarker(done) => self.task(done, &range),
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
        self.covered = self.covered.max(range.end);
    }

    /// An event inside an image: only its text matters, as alt text.
    fn in_image(&mut self, image: usize, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(_) => return self.stack.push(Frame::Other),
            Event::End(_) => {
                let frame = self.stack.pop();
                if self.stack.len() <= image {
                    self.image = None;
                    if let Some(Frame::Image { kind, dest, alt }) = frame {
                        self.end_image(range.clone(), kind, &dest, &alt);
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(Frame::Image { alt, .. }) = self.stack.get_mut(image) {
                    alt.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(Frame::Image { alt, .. }) = self.stack.get_mut(image) {
                    alt.push(' ');
                }
            }
            _ => {}
        }
        self.covered = self.covered.max(range.end);
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        let frame = match tag {
            Tag::Heading { level, .. } => self.heading(level as u8, &range),
            Tag::BlockQuote(kind) => {
                self.quote(kind, &range);
                Frame::Quote
            }
            Tag::CodeBlock(kind) => {
                self.verbatim += 1;
                Frame::Code {
                    info: match kind {
                        CodeBlockKind::Fenced(info) => Some(info.to_string()),
                        CodeBlockKind::Indented => None,
                    },
                    chunks: Vec::new(),
                }
            }
            Tag::MetadataBlock(_) => {
                self.verbatim += 1;
                self.front_matter(&range);
                Frame::Verbatim
            }
            Tag::List(first) => {
                let bullet = first.is_none();
                self.bullets += usize::from(bullet);
                Frame::List { bullet }
            }
            Tag::Item => self.item(&range),
            Tag::FootnoteDefinition(_) => {
                // Its label, `[^1]:`.
                let end = self.lines.end(self.lines.index(range.start));
                if let Some(close) = sub(self.text, range.start..end).find("]:") {
                    self.face(range.start..range.start + close + 2, META);
                }
                Frame::Other
            }
            Tag::Table(aligns) => {
                self.tables += 1;
                Frame::Table {
                    aligns,
                    rows: Vec::new(),
                }
            }
            Tag::TableHead => Frame::Row {
                header: true,
                range,
                cells: Vec::new(),
            },
            Tag::TableRow => Frame::Row {
                header: false,
                range,
                cells: Vec::new(),
            },
            Tag::TableCell => {
                if matches!(self.stack.last(), Some(Frame::Row { header: true, .. })) {
                    let (mut from, mut to) = (range.start, range.end.min(self.text.len()));
                    while from < to && is_blank(self.byte(from)) {
                        from += 1;
                    }
                    while to > from && is_blank(self.byte(to - 1)) {
                        to -= 1;
                    }
                    self.face(from..to, TABLE_HEADER);
                }
                Frame::Cell {
                    hides: self.out.hides.len(),
                }
            }
            Tag::Emphasis => {
                self.delimited(&range, 1, b"*_", ITALIC);
                Frame::Other
            }
            Tag::Strong => {
                self.delimited(&range, 2, b"*_", BOLD);
                Frame::Other
            }
            Tag::Strikethrough => {
                let run = self.run(range.start, b'~').clamp(1, 2);
                self.delimited(&range, run, b"~", STRIKE);
                Frame::Other
            }
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => Frame::Link {
                kind: link_type,
                dest: dest_url.to_string(),
                faces: self.out.faces.len(),
            },
            Tag::Image {
                link_type,
                dest_url,
                ..
            } => {
                self.image = Some(self.stack.len());
                Frame::Image {
                    kind: link_type,
                    dest: dest_url.to_string(),
                    alt: String::new(),
                }
            }
            _ => Frame::Other,
        };
        self.stack.push(frame);
    }

    fn end(&mut self, range: Range<usize>) {
        match self.stack.pop() {
            Some(Frame::Heading {
                level,
                range,
                title,
            }) => {
                self.heading = None;
                let slug = self.slug(&title);
                self.out.headings.push(Heading { range, level, slug });
            }
            Some(Frame::Quote) => self.quotes = self.quotes.saturating_sub(1),
            Some(Frame::List { bullet: true }) => self.bullets = self.bullets.saturating_sub(1),
            Some(Frame::Code { info, chunks }) => {
                self.verbatim = self.verbatim.saturating_sub(1);
                code::layout(self, range, info.as_deref(), &chunks);
            }
            Some(Frame::Verbatim) => self.verbatim = self.verbatim.saturating_sub(1),
            Some(Frame::Cell { hides }) => {
                if let Some(Frame::Row { cells, .. }) = self.stack.last_mut() {
                    cells.push(Cell {
                        range,
                        hides: hides..self.out.hides.len(),
                    });
                }
            }
            Some(Frame::Row {
                header,
                range,
                cells,
            }) => {
                if let Some(Frame::Table { rows, .. }) = self.stack.last_mut() {
                    // The header is the first row a table has.
                    if header == rows.is_empty() {
                        rows.push(Row { range, cells });
                    }
                }
            }
            Some(Frame::Table { aligns, rows }) => {
                self.tables = self.tables.saturating_sub(1);
                let hides = &mut self.out.hides;
                let area = (self.width, self.tab);
                table::layout(self.text, &self.lines, area, &aligns, &rows, hides);
            }
            Some(Frame::Link { kind, dest, faces }) => self.end_link(range, kind, dest, faces),
            _ => {}
        }
    }

    // -------------------------------------------------------------- inline

    fn text(&mut self, text: &str, range: &Range<usize>) {
        if self.verbatim > 0 {
            if let Some(Frame::Code { chunks, .. }) = self.stack.last_mut() {
                chunks.push(range.clone());
            }
            return;
        }
        // `\*`: the backslash is in no event, and the text it escapes starts a new one.
        let at = range.start;
        if at > self.covered && self.byte(at - 1) == b'\\' && self.byte(at).is_ascii_punctuation() {
            self.hide(at - 1..at, None);
        }
        // `&amp;`: a text of what the entity decodes to, over the entity.
        let source = sub(self.text, range.clone());
        if source != text && source.starts_with('&') && source.ends_with(';') {
            self.hide(range.clone(), Some(shown(text, PLAIN)));
        }
        self.title(text);
    }

    fn code(&mut self, code: &str, range: &Range<usize>) {
        let ticks = self.run(range.start, b'`');
        let closed = (0..ticks).all(|i| self.byte(range.end.wrapping_sub(i + 1)) == b'`');
        if ticks == 0 || range.len() < 2 * ticks || !closed {
            self.face(range.clone(), CODE);
        } else {
            let (mut from, mut to) = (range.start + ticks, range.end - ticks);
            // A code span that begins and ends with a space, and isn't all spaces, loses one
            // at each end. A line end counts as a space there, but hiding one would join rows.
            let space = |at: usize| matches!(self.byte(at), b' ' | b'\n');
            let inside = sub(self.text, from..to);
            if to - from >= 2
                && space(from)
                && space(to - 1)
                && inside.bytes().any(|b| !matches!(b, b' ' | b'\n' | b'\r'))
            {
                from += usize::from(self.byte(from) == b' ');
                to -= usize::from(self.byte(to - 1) == b' ');
            }
            self.hide(range.start..from, None);
            self.hide(to..range.end, None);
            self.face(from..to, CODE);
            // In a table a pipe is written `\|` even in code, and shows as a pipe.
            if self.tables > 0 {
                let escapes: Vec<usize> = sub(self.text, from..to)
                    .match_indices("\\|")
                    .map(|(i, _)| from + i)
                    .collect();
                for at in escapes {
                    self.hide(at..at + 1, None);
                }
            }
        }
        self.title(code);
    }

    /// `*a*`, `**a**`, `~~a~~`: hides `run` delimiter bytes at each end, with a face on what's
    /// between. If the ends aren't the same delimiter, the face goes over it all.
    fn delimited(&mut self, range: &Range<usize>, run: usize, marks: &[u8], face: &'static str) {
        let (start, end) = (range.start, range.end);
        let mark = self.byte(start);
        let same = (0..run)
            .all(|i| self.byte(start + i) == mark && self.byte(end.wrapping_sub(i + 1)) == mark);
        if marks.contains(&mark) && same && range.len() >= 2 * run {
            self.hide(start..start + run, None);
            self.hide(end - run..end, None);
            self.face(start + run..end - run, face);
        } else {
            self.face(range.clone(), face);
        }
    }

    fn end_link(&mut self, range: Range<usize>, kind: LinkType, dest: String, faces: usize) {
        let mut whole = range.clone();
        let text = match kind {
            // `<https://x>`: the angle brackets go.
            LinkType::Autolink | LinkType::Email => {
                if range.len() >= 2
                    && self.byte(range.start) == b'<'
                    && self.byte(range.end - 1) == b'>'
                {
                    self.hide(range.start..range.start + 1, None);
                    self.hide(range.end - 1..range.end, None);
                    range.start + 1..range.end - 1
                } else {
                    range
                }
            }
            // `[text](dest)`, `[text][ref]`, `[text][]`, `[text]`: the opening bracket goes,
            // and everything from the bracket that closes the text.
            _ => {
                whole.end = self.with_collapsed(kind, range.end);
                match self.closing_bracket(range.start, range.end) {
                    Some(close) if self.byte(range.start) == b'[' => {
                        self.hide(range.start..range.start + 1, None);
                        self.hide(close..whole.end, None);
                        range.start + 1..close
                    }
                    _ => whole.clone(),
                }
            }
        };
        // Its face goes before those of what's inside it, which are already there.
        if !text.is_empty() {
            let at = faces.min(self.out.faces.len());
            self.out.faces.insert(at, (text.clone(), LINK));
        }
        self.out.links.push(Link {
            range: whole,
            text,
            dest,
            image: false,
        });
    }

    fn end_image(&mut self, range: Range<usize>, kind: LinkType, dest: &str, alt: &str) {
        let whole = range.start..self.with_collapsed(kind, range.end);
        // With no alt text, the file's name says what it is.
        let name = match alt.trim() {
            "" => dest
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or(dest),
            alt => alt,
        };
        let stand_in = match name {
            "" => "‹image›".to_string(),
            name => format!("‹image: {name}›"),
        };
        self.hide(whole.clone(), Some(shown(stand_in, META)));
        self.covered = self.covered.max(whole.end);
        self.out.links.push(Link {
            range: whole.clone(),
            text: whole,
            dest: dest.to_string(),
            image: true,
        });
    }

    /// The end of a link or an image with the `[]` of a collapsed reference, which the parser
    /// leaves outside it.
    fn with_collapsed(&self, kind: LinkType, end: usize) -> usize {
        let collapsed = matches!(kind, LinkType::Collapsed | LinkType::CollapsedUnknown);
        match collapsed && sub(self.text, end..end + 2) == "[]" {
            true => end + 2,
            false => end,
        }
    }

    /// The `]` that closes the text of the link from `open` to `end`. It stands where the last
    /// event inside the link ended; failing that, it's the bracket that balances the opening
    /// one, counting neither escaped brackets nor those in code spans.
    fn closing_bracket(&self, open: usize, end: usize) -> Option<usize> {
        let bytes = self.text.as_bytes().get(..end)?;
        let after_children = self.covered.max(open + 1);
        if bytes.get(after_children) == Some(&b']') {
            return Some(after_children);
        }
        let mut depth = 0usize;
        // Lengths of backtick runs that nothing closes: looked for once each.
        let mut unclosed: Vec<usize> = Vec::new();
        let mut at = open + 1;
        while at < end {
            match bytes[at] {
                b'\\' => at += 1,
                b'`' => {
                    let run = self.run(at, b'`').min(end - at);
                    let mut close = None;
                    let mut next = match unclosed.contains(&run) {
                        true => end,
                        false => at + run,
                    };
                    while close.is_none() && next < end {
                        match bytes[next] {
                            b'`' => {
                                let other = self.run(next, b'`').min(end - next);
                                close = (other == run).then_some(next + other);
                                next += other;
                            }
                            _ => next += 1,
                        }
                    }
                    match close {
                        Some(close) => at = close - 1,
                        None => {
                            unclosed.push(run);
                            at += run - 1;
                        }
                    }
                }
                b'[' => depth += 1,
                b']' if depth == 0 => return Some(at),
                b']' => depth -= 1,
                _ => {}
            }
            at += 1;
        }
        None
    }

    // -------------------------------------------------------------- blocks

    fn heading(&mut self, level: u8, range: &Range<usize>) -> Frame {
        let level = level.clamp(1, 6);
        let face = H[usize::from(level) - 1];
        let first = self.lines.index(range.start);
        let hashes = self.run(range.start, b'#');
        let after = range.start + hashes;
        let atx = hashes == usize::from(level)
            && (after >= self.text.len()
                || matches!(self.byte(after), b' ' | b'\t' | b'\n' | b'\r'));
        let content = if atx {
            // `## Title ##`: the `#`s and the blanks after them go, and a closing run of `#`s
            // with the blanks before it, which must be there for it to be one.
            let end = self.lines.trimmed(first, after);
            let mut from = after;
            while from < end && is_blank(self.byte(from)) {
                from += 1;
            }
            let mut closing = end;
            while closing > from && self.byte(closing - 1) == b'#' {
                closing -= 1;
            }
            let mut to = end;
            if closing < end && (closing == from || is_blank(self.byte(closing - 1))) {
                to = closing;
                while to > from && is_blank(self.byte(to - 1)) {
                    to -= 1;
                }
                self.hide(to..end, None);
            }
            self.hide(range.start..from, None);
            self.face(from..to, face);
            from..to
        } else {
            // Setext: the last line is the underline, drawn as a line of its own length.
            let last = self
                .lines
                .index(range.end.saturating_sub(1).max(range.start));
            let under = self.lines.inset(last);
            let mark = self.byte(under);
            if last > first && matches!(mark, b'=' | b'-') {
                let run = self.run(under, mark);
                let line = if mark == b'=' { "═" } else { "─" };
                self.hide(under..under + run, Some(shown(line.repeat(run), RULE)));
            }
            let lines = first..last.max(first + 1);
            let end = self.lines.trimmed(lines.end - 1, range.start);
            self.face_lines(range.start..end, face);
            range.start..end
        };
        self.heading = Some(self.stack.len());
        Frame::Heading {
            level,
            range: content,
            title: String::new(),
        }
    }

    /// A heading's anchor as GitHub makes it, and not one already given.
    fn slug(&mut self, title: &str) -> String {
        let base: String = title
            .chars()
            .flat_map(char::to_lowercase)
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect();
        let mut slug = base.clone();
        while self.slugs.contains_key(&slug) {
            let again = self.slugs.entry(base.clone()).or_insert(0);
            *again += 1;
            slug = format!("{base}-{again}");
        }
        self.slugs.insert(slug.clone(), 0);
        slug
    }

    fn quote(&mut self, kind: Option<BlockQuoteKind>, range: &Range<usize>) {
        self.quotes += 1;
        if range.is_empty() || range.end > self.text.len() {
            return;
        }
        let first = self.lines.index(range.start);
        for line in first..=self.lines.index(range.end - 1) {
            // Its `>` on this line: where the quote starts, on its first line; after that, as
            // many `>` into the run that starts the line as the quote is deep among quotes. A
            // line that continues a paragraph lazily has none.
            let mark = match line == first {
                true => Some(range.start).filter(|at| self.byte(*at) == b'>'),
                false => self.mark(line, self.quotes),
            };
            let from = match mark {
                Some(at) => {
                    self.hide(at..at + 1, Some(shown("▎", RULE)));
                    at + 1 + usize::from(self.byte(at + 1) == b' ')
                }
                None if line == first => range.start,
                None => self.lines.inset(line),
            };
            let end = self.lines.end(line);
            let end = end - usize::from(end > from && self.byte(end - 1) == b'\r');
            self.face(from..end, QUOTE);
            // `> [!NOTE]`: the alert's title.
            if let (true, Some(kind), Some(at)) = (line == first, kind, mark) {
                let mut open = at + 1;
                while open < end && is_blank(self.byte(open)) {
                    open += 1;
                }
                let rest = sub(self.text, open..end);
                if let Some(close) = rest.find(']').filter(|_| rest.starts_with("[!")) {
                    let title = match kind {
                        BlockQuoteKind::Note => "Note",
                        BlockQuoteKind::Tip => "Tip",
                        BlockQuoteKind::Important => "Important",
                        BlockQuoteKind::Warning => "Warning",
                        BlockQuoteKind::Caution => "Caution",
                    };
                    self.hide(open..open + close + 1, Some(shown(title, ALERT)));
                }
            }
        }
    }

    /// Where the `depth`-th `>` is in the run of spaces, tabs and `>` that starts a line. The
    /// run is read once for all the quotes that ask.
    fn mark(&mut self, line: usize, depth: usize) -> Option<usize> {
        let (text, lines) = (self.text, &self.lines);
        let marks = self.marks.entry(line).or_insert_with(|| {
            let (start, inset) = (lines.start(line), lines.inset(line));
            let run = sub(text, start..inset);
            run.match_indices('>').map(|(i, _)| start + i).collect()
        });
        marks.get(depth.checked_sub(1)?).copied()
    }

    fn item(&mut self, range: &Range<usize>) -> Frame {
        let bullet = matches!(self.stack.last(), Some(Frame::List { bullet: true }));
        let end = self.lines.end(self.lines.index(range.start));
        let mut start = range.start;
        while start < end && is_blank(self.byte(start)) {
            start += 1;
        }
        if bullet && matches!(self.byte(start), b'-' | b'*' | b'+') {
            // A bullet, by how deep its list is.
            let glyph = BULLETS[self.bullets.saturating_sub(1) % BULLETS.len()];
            self.hide(start..start + 1, Some(shown(glyph, MARKER)));
            return Frame::Item {
                start,
                bullet: Some(self.out.hides.len() - 1),
            };
        }
        if !bullet {
            // `12.` or `3)` stays, as a marker.
            let mut to = start;
            while to < end && self.byte(to).is_ascii_digit() {
                to += 1;
            }
            if to > start && matches!(self.byte(to), b'.' | b')') {
                self.face(start..to + 1, MARKER);
            }
        }
        Frame::Item {
            start,
            bullet: None,
        }
    }

    /// `- [ ] x`: the box stands where the bullet was, so the bullet and the blanks after it
    /// go. An ordered item keeps its number.
    fn task(&mut self, done: bool, range: &Range<usize>) {
        // After a tab the parser's range starts at the tab: the box is the brackets.
        let mut open = range.start;
        while open < range.end && is_blank(self.byte(open)) {
            open += 1;
        }
        if self.byte(open) != b'[' {
            return;
        }
        let item = self.stack.iter().rev().take(3).find_map(|f| match f {
            Frame::Item { start, bullet } => Some((*start, *bullet)),
            _ => None,
        });
        if let Some((start, Some(bullet))) = item
            && start < open
            && (start + 1..open).all(|at| is_blank(self.byte(at)))
            && let Some(hide) = self.out.hides.get_mut(bullet)
            && hide.range == (start..start + 1)
        {
            *hide = Hide {
                range: start..open,
                shown: None,
            };
        }
        let glyph = if done { "☑" } else { "☐" };
        self.hide(open..range.end, Some(shown(glyph, MARKER)));
    }

    /// `---`: a line from where the rule starts to the text area's edge.
    fn rule(&mut self, range: &Range<usize>) {
        if !matches!(self.byte(range.start), b'-' | b'*' | b'_') {
            return;
        }
        let line = self.lines.index(range.start);
        let before = sub(self.text, self.lines.start(line)..range.start);
        let column = columns(before, 0, self.tab);
        let length = self.width.saturating_sub(column).max(3);
        let end = self.lines.end(line);
        self.hide(range.start..end, Some(shown("─".repeat(length), RULE)));
    }

    /// A YAML block. At the very top it's front matter: hidden whole, through its closing
    /// line's newline, or dimmed if it's to show. Anywhere else it stays, dimmed.
    fn front_matter(&mut self, range: &Range<usize>) {
        if range.start == 0 && !self.show_front_matter {
            let last = self.lines.index(range.end.saturating_sub(1));
            self.hide(0..self.lines.after(last), None);
        } else {
            self.face_lines(range.clone(), META);
        }
    }
}
