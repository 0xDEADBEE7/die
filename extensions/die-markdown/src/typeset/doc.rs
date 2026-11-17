//! The document: a markdown text as blocks and the runs of text in them, each with the source
//! bytes it came from. Built straight on the parser's events, with the planner's option set and
//! its heading anchors, so the page and the raw view agree on what the text is.
//!
//! What the parser leaves raw is read here: addresses in plain text become links as on GitHub,
//! footnotes are numbered by first reference and gathered for the foot, and of HTML the tags a
//! README leans on are understood (pictures, key caps, raised and lowered text, the common
//! inline styles, `<details>`, centred blocks, the places an `id` or a `name` names), comments
//! vanish, and any other tag is dropped with its text kept.

use super::html::{self, Token};
use crate::plan::walk::{Slugs, syntax};
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd,
};
use std::collections::HashMap;
use std::ops::Range;

/// A whole document.
#[derive(Debug)]
pub(super) struct Doc {
    /// Its blocks, the footnotes last.
    pub blocks: Vec<Block>,
    /// Its links, which runs and pictures name by their place here.
    pub links: Vec<Link>,
}

/// Where a link leads.
#[derive(Debug)]
pub(super) struct Link {
    /// The address as the source has it.
    pub dest: String,
    /// Its title, or nothing.
    pub title: String,
    /// Whether it's a picture standing as its own link.
    pub image: bool,
    /// The source bytes it was made from.
    pub src: Range<usize>,
    /// A name the place it's drawn at goes by: a footnote's first reference, for the way back.
    pub anchor: Option<String>,
}

/// Which way lines are set.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Align {
    /// Nothing said: left, but for a table's header cell, which a browser centres.
    #[default]
    Start,
    /// Left.
    Left,
    /// Centred.
    Center,
    /// Right.
    Right,
}

/// How a paragraph stands among its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Fit {
    /// A block: nothing above, a gap below.
    Block,
    /// A loose list item's own paragraph: a gap above and below.
    Loose,
    /// A tight item's text, or an HTML block's own text, which a browser sets in a box with
    /// no margins: no gap.
    Tight,
}

/// A run of lines: a paragraph, a heading's text, a table cell.
#[derive(Debug)]
pub(super) struct Para {
    pub inlines: Vec<Inline>,
    pub align: Align,
    pub fit: Fit,
    pub src: Range<usize>,
}

/// The five alerts, in the palette's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Debug)]
pub(super) enum Block {
    Para(Para),
    Heading {
        level: u8,
        slug: String,
        para: Para,
    },
    Code {
        text: String,
        /// Where each line of `text` starts in the source.
        lines: Vec<usize>,
        /// The fence's language, kept for a highlighter: GitHub shows none, and nor does the page.
        #[allow(dead_code)]
        lang: String,
        src: Range<usize>,
    },
    Quote {
        alert: Option<Alert>,
        blocks: Vec<Block>,
    },
    List {
        /// An ordered list's first number.
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table(Table),
    Rule {
        src: Range<usize>,
    },
    /// `<details>`: its summary line, what it holds, whether its tag says `open`, where its tag
    /// is in the source, and the link its summary line is, for toggling it.
    Details {
        summary: Para,
        blocks: Vec<Block>,
        open: bool,
        at: usize,
        link: u32,
    },
    /// The footnotes, in the order they were first referred to.
    Notes(Vec<Note>),
}

#[derive(Debug)]
pub(super) struct Item {
    /// A task's box, checked or not.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
    pub src: Range<usize>,
}

#[derive(Debug)]
pub(super) struct Table {
    pub aligns: Vec<Align>,
    /// Where the table stands in its column: an HTML block round it may centre it, or set it
    /// right, as a browser sets a block narrower than its container, and its cells with it.
    pub align: Align,
    pub head: Vec<Para>,
    pub rows: Vec<Vec<Para>>,
    pub src: Range<usize>,
}

#[derive(Debug)]
pub(super) struct Note {
    pub number: usize,
    pub label: String,
    pub blocks: Vec<Block>,
}

/// A length an HTML attribute gives.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Length {
    Px(f32),
    Percent(f32),
}

#[derive(Debug)]
pub(super) enum Inline {
    /// Text in one style. Its bytes are the source's from `src` on, but for a decoded entity.
    Text {
        text: String,
        style: Style,
        src: usize,
    },
    /// A hard line break.
    Break,
    /// A place an HTML tag names with `id` or `name`, which a link's `#name` leads to. It
    /// takes no room.
    Anchor { name: String, src: usize },
    /// A picture.
    Image {
        dest: String,
        alt: String,
        /// The link it's part of: its own, or the one it's inside.
        link: u32,
        width: Option<Length>,
        height: Option<Length>,
        src: usize,
    },
}

/// The elements that may give text a colour of their own, as [`Style::roles`] packs them: a
/// link's is there for what's drawn in the colour of an element round it, as a line through.
pub(super) const STRONG: u32 = 1;
pub(super) const EM: u32 = 2;
pub(super) const STRIKE: u32 = 3;
pub(super) const CODE: u32 = 4;
pub(super) const LINK: u32 = 5;

/// What a run of text is inside.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(super) struct Style {
    pub strong: bool,
    pub em: bool,
    pub strike: bool,
    pub code: bool,
    pub kbd: bool,
    /// Raised (1) or lowered (-1).
    pub script: i8,
    pub link: Option<u32>,
    /// The colour-giving elements round it, four bits each, the innermost lowest: a colour
    /// comes from the nearest element that has one, as in a stylesheet.
    pub roles: u32,
    /// Which code span or key cap this is, so two side by side are two boxes.
    pub span: u32,
}

/// The document of a text.
pub(super) fn parse(text: &str, front_matter: bool) -> Doc {
    let mut builder = Builder {
        text,
        front_matter,
        frames: vec![Frame::new(Kind::Root, 0..text.len())],
        open: None,
        marks: Vec::new(),
        links: Vec::new(),
        image: None,
        code: None,
        meta: None,
        html: None,
        skip: None,
        aligns: Vec::new(),
        slugs: Slugs::default(),
        spans: 0,
        numbers: HashMap::new(),
        notes: Vec::new(),
        unopened: 0,
        marker: None,
    };
    for (event, range) in Parser::new_ext(text, syntax()).into_offset_iter() {
        builder.event(event, range);
    }
    builder.finish(text.len())
}

/// A container being filled.
struct Frame {
    kind: Kind,
    blocks: Vec<Block>,
    src: Range<usize>,
}

impl Frame {
    fn new(kind: Kind, src: Range<usize>) -> Frame {
        Frame {
            kind,
            blocks: Vec::new(),
            src,
        }
    }
}

enum Kind {
    Root,
    Quote(Option<Alert>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Item {
        task: Option<bool>,
    },
    Note(String),
    Details {
        summary: Option<Para>,
        open: bool,
        at: usize,
        link: u32,
    },
    Table {
        aligns: Vec<Align>,
        align: Align,
        head: Vec<Para>,
        rows: Vec<Vec<Para>>,
        in_head: bool,
    },
}

/// The lines being filled.
struct Open {
    kind: OpenKind,
    inlines: Vec<Inline>,
    src: Range<usize>,
    /// A heading's text as its anchor is made from it.
    title: String,
    align: Align,
}

#[derive(Clone, Copy, PartialEq)]
enum OpenKind {
    /// A paragraph; one no `Paragraph` event opened is a tight item's text, or HTML's. One
    /// that's `bare` is an HTML block's text outside any `<p>`: a browser gives it no margins.
    Para {
        implicit: bool,
        bare: bool,
    },
    Heading(u8),
    Cell,
    Summary,
}

/// An inline element the text is inside.
#[derive(Clone, Copy)]
enum Mark {
    Strong,
    Em,
    Strike,
    Code(u32),
    Kbd(u32),
    Sup,
    Sub,
    Link(u32),
}

struct ImageOpen {
    dest: String,
    title: String,
    alt: String,
    /// How deep in the picture's own elements the events are.
    depth: usize,
}

struct CodeOpen {
    text: String,
    lines: Vec<usize>,
    lang: String,
    src: Range<usize>,
}

struct Builder<'t> {
    /// The source, for what the parser reads and doesn't pass on.
    text: &'t str,
    front_matter: bool,
    frames: Vec<Frame>,
    open: Option<Open>,
    /// The inline elements open, outermost first, each with the HTML tag that opened it if one
    /// did: markdown's close in order, HTML's by name.
    marks: Vec<(Mark, Option<String>)>,
    links: Vec<Link>,
    image: Option<ImageOpen>,
    code: Option<CodeOpen>,
    meta: Option<(String, Range<usize>)>,
    /// An HTML block's text as its lines come.
    html: Option<(String, usize)>,
    /// Inside `<script>` or `<style>`: nothing shows until it closes.
    skip: Option<String>,
    /// The HTML blocks that set an alignment, by tag, innermost last.
    aligns: Vec<(String, Align)>,
    slugs: Slugs,
    spans: u32,
    /// Footnotes by label: each one's number, in order of first reference, and how many times
    /// it has been referred to.
    numbers: HashMap<String, (usize, usize)>,
    notes: Vec<(String, Vec<Block>)>,
    /// Containers too deep to follow, which were never opened: their ends close nothing.
    unopened: usize,
    /// An alert's marker line the parser took from a quote that isn't an alert, with where it
    /// is: text again, at the quote's first paragraph or as one of its own.
    marker: Option<(String, usize, bool)>,
}

/// How deep in quotes and lists a document is followed. What's deeper is set at this depth:
/// nothing a person writes goes there, and a text made to can't run the layout out of stack.
const DEEPEST: usize = 64;

impl Builder<'_> {
    /// Opens a container, unless that's too deep. A list's item always opens, one past the
    /// depth if need be, since what a list holds outside its items is never shown: so what's
    /// deeper goes in the deepest item.
    fn enter(&mut self, kind: Kind, src: Range<usize>) {
        self.close();
        self.marker_para();
        let item = matches!(kind, Kind::Item { .. })
            && matches!(self.frames.last(), Some(f) if matches!(f.kind, Kind::List { .. }));
        if self.frames.len() >= DEEPEST && !item {
            self.unopened += 1;
        } else {
            self.frames.push(Frame::new(kind, src));
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        // Inside a picture everything is its alternative text.
        if let Some(image) = &mut self.image {
            match event {
                Event::Start(_) => image.depth += 1,
                Event::End(_) if image.depth == 0 => self.end_image(range),
                Event::End(_) => image.depth -= 1,
                Event::Text(t) | Event::Code(t) => image.alt.push_str(&t),
                Event::SoftBreak | Event::HardBreak => image.alt.push(' '),
                _ => {}
            }
            return;
        }
        if self.code.is_some() {
            match (event, &mut self.code) {
                (Event::Text(t), Some(code)) => {
                    let mut at = range.start;
                    for line in t.split_inclusive('\n') {
                        if code.text.is_empty() || code.text.ends_with('\n') {
                            code.lines.push(at.min(range.end));
                        }
                        code.text.push_str(line);
                        at += line.len();
                    }
                }
                (Event::End(TagEnd::CodeBlock), code) => {
                    if let Some(code) = code.take() {
                        self.block(Block::Code {
                            text: code.text,
                            lines: code.lines,
                            lang: code.lang,
                            src: code.src,
                        });
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some((text, _)) = &mut self.meta {
            match event {
                Event::Text(t) => text.push_str(&t),
                Event::End(TagEnd::MetadataBlock(_)) => self.end_meta(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag, range),
            Event::Text(t) | Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t, range),
            Event::Code(t) => {
                self.spans += 1;
                self.marks.push((Mark::Code(self.spans), None));
                self.text(&t, range);
                self.marks.pop();
            }
            Event::Html(t) => match &mut self.html {
                Some((block, _)) => block.push_str(&t),
                None => self.html(&t, range, false),
            },
            Event::InlineHtml(t) => self.html(&t, range, false),
            Event::FootnoteReference(label) => self.reference(&label, range),
            Event::SoftBreak => {
                let style = self.style();
                self.push(
                    Inline::Text {
                        text: " ".to_owned(),
                        style,
                        src: range.start,
                    },
                    &range,
                );
                self.title(" ");
            }
            Event::HardBreak => {
                self.push(Inline::Break, &range);
                self.title(" ");
            }
            Event::Rule => {
                self.close();
                self.block(Block::Rule { src: range });
            }
            Event::TaskListMarker(done) => {
                let mut items = self.frames.iter_mut().rev();
                let item = items.find_map(|f| match &mut f.kind {
                    Kind::Item { task } => Some(task),
                    _ => None,
                });
                if let Some(task) = item {
                    *task = Some(done);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Paragraph => {
                self.close();
                // A quote's taken marker line goes on into its first paragraph, as the source
                // had it, unless a blank quote line stood between them.
                match self.marker.take() {
                    Some((literal, at, true)) => {
                        self.open_as(
                            OpenKind::Para {
                                implicit: false,
                                bare: false,
                            },
                            at..range.end,
                        );
                        self.marker_text(literal, at);
                        self.text(" ", at..at);
                    }
                    Some(marker) => {
                        self.marker = Some(marker);
                        self.marker_para();
                        self.open_as(
                            OpenKind::Para {
                                implicit: false,
                                bare: false,
                            },
                            range,
                        );
                    }
                    None => self.open_as(
                        OpenKind::Para {
                            implicit: false,
                            bare: false,
                        },
                        range,
                    ),
                }
            }
            Tag::Heading { level, .. } => {
                self.close();
                self.open_as(OpenKind::Heading(level as u8), range);
            }
            Tag::BlockQuote(kind) => {
                let mut alert = kind.map(|k| match k {
                    BlockQuoteKind::Note => Alert::Note,
                    BlockQuoteKind::Tip => Alert::Tip,
                    BlockQuoteKind::Important => Alert::Important,
                    BlockQuoteKind::Warning => Alert::Warning,
                    BlockQuoteKind::Caution => Alert::Caution,
                });
                // GitHub makes an alert of a quote at the top level only: in a list item or
                // another quote it's a plain quote whose first line reads as written. The
                // parser took that line, so it's put back from the source.
                let nested = self.frames[1..]
                    .iter()
                    .any(|f| !matches!(f.kind, Kind::Details { .. }));
                if nested && alert.take().is_some() {
                    let rest = self.text.get(range.start..).unwrap_or("");
                    let line = rest.split('\n').next().unwrap_or("");
                    let literal = line.find('[').and_then(|a| {
                        let b = line[a..].find(']')?;
                        Some((line[a..=a + b].to_owned(), range.start + a))
                    });
                    let (literal, at) =
                        literal.unwrap_or_else(|| (format!("[!{}]", kind_name(kind)), range.start));
                    // Whether the next quote line has text: then it's the same paragraph.
                    let next = rest.split('\n').nth(1).unwrap_or("");
                    let joined = !next.trim_start_matches([' ', '\t', '>']).trim().is_empty();
                    self.enter(Kind::Quote(None), range);
                    self.marker = Some((literal, at, joined));
                } else {
                    self.enter(Kind::Quote(alert), range);
                }
            }
            Tag::CodeBlock(kind) => {
                self.close();
                let lang = match &kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
                    CodeBlockKind::Indented => "",
                };
                self.code = Some(CodeOpen {
                    text: String::new(),
                    lines: Vec::new(),
                    lang: lang.to_owned(),
                    src: range,
                });
            }
            Tag::HtmlBlock => self.html = Some((String::new(), range.start)),
            Tag::List(start) => {
                let items = Vec::new();
                self.enter(Kind::List { start, items }, range);
            }
            Tag::Item => self.enter(Kind::Item { task: None }, range),
            Tag::FootnoteDefinition(label) => self.enter(Kind::Note(label.to_string()), range),
            Tag::Table(aligns) => {
                let aligns = aligns.iter().map(|a| match a {
                    Alignment::None => Align::Start,
                    Alignment::Left => Align::Left,
                    Alignment::Center => Align::Center,
                    Alignment::Right => Align::Right,
                });
                let kind = Kind::Table {
                    aligns: aligns.collect(),
                    align: self.aligns.last().map_or(Align::Start, |a| a.1),
                    head: Vec::new(),
                    rows: Vec::new(),
                    in_head: false,
                };
                self.enter(kind, range);
            }
            Tag::TableHead | Tag::TableRow => {
                let head = matches!(tag, Tag::TableHead);
                if let Some(Kind::Table { rows, in_head, .. }) = self.table() {
                    *in_head = head;
                    if !head {
                        rows.push(Vec::new());
                    }
                }
            }
            Tag::TableCell => {
                self.close();
                self.open_as(OpenKind::Cell, range);
            }
            Tag::Emphasis => self.marks.push((Mark::Em, None)),
            Tag::Strong => self.marks.push((Mark::Strong, None)),
            Tag::Strikethrough => self.marks.push((Mark::Strike, None)),
            Tag::Superscript => self.marks.push((Mark::Sup, None)),
            Tag::Subscript => self.marks.push((Mark::Sub, None)),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                // An address in angle brackets is a mail address without its scheme.
                let dest = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                };
                let link = self.link(dest, title.to_string(), false, range);
                self.marks.push((Mark::Link(link), None));
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                self.image = Some(ImageOpen {
                    dest: dest_url.to_string(),
                    title: title.to_string(),
                    alt: String::new(),
                    depth: 0,
                });
            }
            Tag::MetadataBlock(_) => {
                self.close();
                self.meta = Some((String::new(), range));
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.close();
            }
        }
    }

    fn end(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell => self.close(),
            // The innermost containers are the ones that were too deep to open.
            TagEnd::BlockQuote(_)
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
                if self.unopened > 0 =>
            {
                self.close();
                self.unopened -= 1;
            }
            TagEnd::BlockQuote(_) => self.pop_to(|k| matches!(k, Kind::Quote(_))),
            TagEnd::List(_) => self.pop_to(|k| matches!(k, Kind::List { .. })),
            TagEnd::Item => self.pop_to(|k| matches!(k, Kind::Item { .. })),
            TagEnd::FootnoteDefinition => self.pop_to(|k| matches!(k, Kind::Note(_))),
            TagEnd::Table => self.pop_to(|k| matches!(k, Kind::Table { .. })),
            TagEnd::HtmlBlock => {
                if let Some((block, start)) = self.html.take() {
                    self.html(&block, start..range.end.max(start), true);
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link => {
                // Markdown's elements close in order; an HTML tag open among them stays.
                if let Some(at) = self.marks.iter().rposition(|(_, tag)| tag.is_none()) {
                    self.marks.remove(at);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ containers

    /// The table being filled, if the innermost container is one.
    fn table(&mut self) -> Option<&mut Kind> {
        let top = self.frames.last_mut()?;
        matches!(top.kind, Kind::Table { .. }).then_some(&mut top.kind)
    }

    /// A finished block goes in the innermost container.
    fn block(&mut self, block: Block) {
        self.marker_para();
        if let Some(top) = self.frames.last_mut() {
            top.blocks.push(block);
        }
    }

    /// A taken marker line as a paragraph of its own, where something else comes first.
    fn marker_para(&mut self) {
        if let Some((literal, at, _)) = self.marker.take() {
            self.open_as(
                OpenKind::Para {
                    implicit: false,
                    bare: false,
                },
                at..at + literal.len(),
            );
            self.marker_text(literal, at);
            self.close();
        }
    }

    fn marker_text(&mut self, literal: String, at: usize) {
        let range = at..at + literal.len();
        self.text(&literal, range);
    }

    /// Ends the innermost container `is` says yes to, and those inside it.
    fn pop_to(&mut self, is: impl Fn(&Kind) -> bool) {
        self.close();
        let Some(at) = self.frames.iter().rposition(|f| is(&f.kind)) else {
            return;
        };
        while self.frames.len() > at.max(1) {
            self.pop();
        }
    }

    fn pop(&mut self) {
        self.marker_para();
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let Frame { kind, blocks, src } = frame;
        match kind {
            Kind::Root => self.frames.push(Frame { kind, blocks, src }),
            Kind::Quote(alert) => self.block(Block::Quote { alert, blocks }),
            Kind::List { start, items } => self.block(Block::List { start, items }),
            Kind::Item { task } => match self.frames.last_mut() {
                Some(Frame {
                    kind: Kind::List { items, .. },
                    ..
                }) => items.push(Item { task, blocks, src }),
                Some(parent) => parent.blocks.extend(blocks),
                None => {}
            },
            Kind::Note(label) => self.notes.push((label, blocks)),
            Kind::Details {
                summary,
                open,
                at,
                link,
            } => {
                // With no summary of its own a browser writes "Details".
                let summary = summary.unwrap_or(Para {
                    inlines: vec![Inline::Text {
                        text: "Details".to_owned(),
                        style: Style::default(),
                        src: src.start,
                    }],
                    align: Align::Start,
                    fit: Fit::Tight,
                    src: src.start..src.start,
                });
                self.block(Block::Details {
                    summary,
                    blocks,
                    open,
                    at,
                    link,
                });
            }
            Kind::Table {
                aligns,
                align,
                head,
                rows,
                ..
            } => self.block(Block::Table(Table {
                aligns,
                align,
                head,
                rows,
                src,
            })),
        }
    }

    // ------------------------------------------------------------ lines

    fn open_as(&mut self, kind: OpenKind, src: Range<usize>) {
        self.open = Some(Open {
            kind,
            inlines: Vec::new(),
            src,
            title: String::new(),
            align: self.aligns.last().map_or(Align::Start, |a| a.1),
        });
    }

    /// Something for the lines being filled: with none open, a paragraph starts, as for a
    /// tight item's text.
    fn push(&mut self, inline: Inline, range: &Range<usize>) {
        if self.open.is_none() {
            self.open_as(
                OpenKind::Para {
                    implicit: true,
                    bare: false,
                },
                range.clone(),
            );
        }
        if let Some(open) = &mut self.open {
            open.src.end = open.src.end.max(range.end);
            open.inlines.push(inline);
        }
    }

    fn title(&mut self, text: &str) {
        if let Some(open) = &mut self.open {
            open.title.push_str(text);
        }
    }

    fn text(&mut self, text: &str, range: Range<usize>) {
        let style = self.style();
        self.push(
            Inline::Text {
                text: text.to_owned(),
                style,
                src: range.start,
            },
            &range,
        );
        self.title(text);
    }

    /// Ends the lines being filled, and puts them where they belong.
    fn close(&mut self) {
        // An HTML tag left open ends with its paragraph.
        self.marks.retain(|(_, tag)| tag.is_none());
        let Some(mut open) = self.open.take() else {
            return;
        };
        autolink(&mut open.inlines, &mut self.links);
        // Nothing but white space is no paragraph. Line breaks alone are one, of that many
        // empty lines, and so is a no-break space: a browser gives each its line, and README
        // authors open space with them.
        let space = |i: &Inline| match i {
            Inline::Text { text, .. } => text.chars().all(|c| c.is_whitespace() && c != '\u{a0}'),
            _ => false,
        };
        let named = |i: &Inline| matches!(i, Inline::Anchor { .. });
        let blank = open.inlines.iter().all(space);
        // White space and empty tags that name places make no line, as in a browser: the
        // names stay, with no room of their own.
        if !blank && open.inlines.iter().all(|i| space(i) || named(i)) {
            open.inlines.retain(named);
        }
        let in_item = matches!(
            self.frames.last().map(|f| &f.kind),
            Some(Kind::Item { .. } | Kind::Note(_))
        );
        let fit = match open.kind {
            OpenKind::Para { bare: true, .. } => Fit::Tight,
            OpenKind::Para { implicit: true, .. } if in_item => Fit::Tight,
            OpenKind::Para { .. } if in_item => Fit::Loose,
            OpenKind::Summary => Fit::Tight,
            _ => Fit::Block,
        };
        let para = Para {
            inlines: open.inlines,
            align: open.align,
            fit,
            src: open.src,
        };
        match open.kind {
            OpenKind::Para { .. } if blank => {}
            OpenKind::Para { .. } => self.block(Block::Para(para)),
            OpenKind::Heading(level) => {
                let slug = self.slugs.take(&open.title);
                self.block(Block::Heading { level, slug, para });
            }
            OpenKind::Cell => match self.table() {
                Some(Kind::Table {
                    head,
                    rows,
                    in_head,
                    ..
                }) => match rows.last_mut() {
                    Some(row) if !*in_head => row.push(para),
                    _ => head.push(para),
                },
                // A table too deep to follow: its cells are paragraphs where it is.
                _ if blank => {}
                _ => self.block(Block::Para(para)),
            },
            OpenKind::Summary => match self.frames.last_mut().map(|f| &mut f.kind) {
                Some(Kind::Details { summary, .. }) if summary.is_none() => *summary = Some(para),
                _ if blank => {}
                _ => self.block(Block::Para(para)),
            },
        }
    }

    /// The style of text here: what the open elements make it.
    fn style(&self) -> Style {
        let mut style = Style::default();
        for (mark, _) in &self.marks {
            let role = match *mark {
                Mark::Strong => {
                    style.strong = true;
                    STRONG
                }
                Mark::Em => {
                    style.em = true;
                    EM
                }
                Mark::Strike => {
                    style.strike = true;
                    STRIKE
                }
                Mark::Code(span) => {
                    (style.code, style.span) = (true, span);
                    CODE
                }
                Mark::Kbd(span) => {
                    (style.kbd, style.span) = (true, span);
                    0
                }
                Mark::Sup => {
                    style.script = 1;
                    0
                }
                Mark::Sub => {
                    style.script = -1;
                    0
                }
                Mark::Link(link) => {
                    style.link = Some(link);
                    LINK
                }
            };
            if role != 0 {
                style.roles = style.roles << 4 | role;
            }
        }
        style
    }

    fn link(&mut self, dest: String, title: String, image: bool, src: Range<usize>) -> u32 {
        self.links.push(Link {
            dest,
            title,
            image,
            src,
            anchor: None,
        });
        (self.links.len() - 1) as u32
    }

    fn end_image(&mut self, range: Range<usize>) {
        let Some(image) = self.image.take() else {
            return;
        };
        self.picture(image.dest, image.alt, image.title, None, None, range);
    }

    /// A picture: a link to itself unless it's inside one.
    fn picture(
        &mut self,
        dest: String,
        alt: String,
        title: String,
        width: Option<Length>,
        height: Option<Length>,
        range: Range<usize>,
    ) {
        let link = match self.style().link {
            Some(link) => link,
            None => self.link(dest.clone(), title, true, range.clone()),
        };
        let image = Inline::Image {
            dest,
            alt,
            link,
            width,
            height,
            src: range.start,
        };
        self.push(image, &range);
    }

    /// A footnote's reference: its number in brackets, raised, leading to the note. Each
    /// reference is a place the note's back links name, the second and later by their count.
    fn reference(&mut self, label: &str, range: Range<usize>) {
        let next = self.numbers.len() + 1;
        let (number, count) = self.numbers.entry(label.to_owned()).or_insert((next, 0));
        let (number, count) = (*number, {
            *count += 1;
            *count
        });
        let link = self.link(format!("#fn-{label}"), String::new(), false, range.clone());
        self.links[link as usize].anchor = Some(back_anchor(label, count));
        let mut style = self.style();
        (style.script, style.link) = (1, Some(link));
        let text = format!("[{number}]");
        let src = range.start;
        self.push(Inline::Text { text, style, src }, &range);
    }

    /// Front matter. At the top of the text it's left out, or shown as GitHub shows it, a
    /// table of its keys over their values; anywhere else it isn't front matter, and shows as
    /// what it is there, text between two rules.
    fn end_meta(&mut self) {
        let Some((text, src)) = self.meta.take() else {
            return;
        };
        let cell = |text: &str, at: usize| Para {
            inlines: vec![Inline::Text {
                text: text.to_owned(),
                style: Style::default(),
                src: at,
            }],
            align: Align::Start,
            fit: Fit::Block,
            src: at..at,
        };
        if src.start != 0 {
            self.block(Block::Rule {
                src: src.start..src.start,
            });
            self.block(Block::Para(cell(&text, src.start)));
            self.block(Block::Rule {
                src: src.end..src.end,
            });
            return;
        }
        if !self.front_matter {
            return;
        }
        let (mut keys, mut values): (Vec<Para>, Vec<Para>) = (Vec::new(), Vec::new());
        for (key, value, start) in front_matter(&text, src.start) {
            keys.push(cell(&key, start));
            values.push(cell(&value, start));
        }
        if keys.is_empty() {
            return;
        }
        self.block(Block::Table(Table {
            aligns: vec![Align::Start; keys.len()],
            align: Align::Start,
            head: keys,
            rows: vec![values],
            src,
        }));
    }

    // ------------------------------------------------------------ HTML

    /// Raw HTML: a block's worth, or one inline tag.
    fn html(&mut self, raw: &str, src: Range<usize>, block: bool) {
        for (token, at) in html::tokens(raw) {
            let range = (src.start + at).min(src.end)..src.end;
            if let Some(until) = &self.skip {
                if matches!(&token, Token::Close(name) if name == until) {
                    self.skip = None;
                }
                continue;
            }
            match token {
                Token::Text(text) => {
                    // Between a block's tags, white space alone starts no paragraph.
                    if self.open.is_some() || !text.trim().is_empty() {
                        self.bare(block, &range);
                        self.text(&html::decode(text), range);
                    }
                }
                Token::Open(name, attrs) => self.html_open(&name, &attrs, range, block),
                Token::Close(name) => self.html_close(&name, block),
            }
        }
    }

    fn html_open(
        &mut self,
        name: &str,
        attrs: &[(String, String)],
        range: Range<usize>,
        block: bool,
    ) {
        let attr = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let mark = |builder: &mut Builder, mark: Mark| {
            builder.marks.push((mark, Some(name.to_owned())));
        };
        let tag = range.clone();
        match name {
            "br" => {
                self.bare(block, &range);
                self.push(Inline::Break, &range);
            }
            "img" => {
                self.bare(block, &range);
                let length = |key: &str| {
                    let value = attr(key)?.trim();
                    match value.strip_suffix('%') {
                        Some(percent) => percent.trim().parse().ok().map(Length::Percent),
                        None => {
                            let px = value.strip_suffix("px").unwrap_or(value);
                            px.trim().parse().ok().map(Length::Px)
                        }
                    }
                };
                let text = |key: &str| attr(key).unwrap_or("").to_owned();
                let (width, height) = (length("width"), length("height"));
                self.picture(
                    text("src"),
                    text("alt"),
                    text("title"),
                    width,
                    height,
                    range,
                );
            }
            "b" | "strong" => mark(self, Mark::Strong),
            "i" | "em" => mark(self, Mark::Em),
            "s" | "del" | "strike" => mark(self, Mark::Strike),
            "sub" => mark(self, Mark::Sub),
            "sup" => mark(self, Mark::Sup),
            "code" | "tt" | "samp" => {
                self.spans += 1;
                mark(self, Mark::Code(self.spans));
            }
            "kbd" => {
                self.spans += 1;
                mark(self, Mark::Kbd(self.spans));
            }
            "a" => {
                if let Some(href) = attr("href") {
                    let title = attr("title").unwrap_or("").to_owned();
                    let link = self.link(href.to_owned(), title, false, range);
                    mark(self, Mark::Link(link));
                }
            }
            "script" | "style" => self.skip = Some(name.to_owned()),
            // A table's cells stay on their row's line, a space apart.
            "td" | "th" if block && self.open.is_some() => self.text(" ", range),
            _ if !block || !is_block(name) => {}
            "hr" => {
                self.close();
                self.block(Block::Rule { src: range });
            }
            "details" => {
                self.close();
                if self.frames.len() < DEEPEST {
                    // Its summary line is a link the reader toggles it by, named for the tag.
                    let at = range.start;
                    let link =
                        self.link(format!("details:{at}"), String::new(), false, range.clone());
                    let kind = Kind::Details {
                        summary: None,
                        open: attr("open").is_some(),
                        at,
                        link,
                    };
                    self.frames.push(Frame::new(kind, range));
                }
            }
            "summary" => {
                self.close();
                self.open_as(OpenKind::Summary, range);
            }
            _ => {
                self.close();
                let align = match (name, attr("align").map(str::to_ascii_lowercase).as_deref()) {
                    ("center", _) | (_, Some("center" | "middle")) => Some(Align::Center),
                    (_, Some("right")) => Some(Align::Right),
                    (_, Some("left")) => Some(Align::Left),
                    _ => None,
                };
                if matches!(name, "p" | "div" | "center") || name.starts_with('h') {
                    let inherited = self.aligns.last().map_or(Align::Start, |a| a.1);
                    self.aligns
                        .push((name.to_owned(), align.unwrap_or(inherited)));
                }
                match name.strip_prefix('h').and_then(|l| l.parse::<u8>().ok()) {
                    Some(level @ 1..=6) => self.open_as(OpenKind::Heading(level), range),
                    _ if name == "p" => self.open_as(
                        OpenKind::Para {
                            implicit: false,
                            bare: false,
                        },
                        range,
                    ),
                    _ => {}
                }
            }
        }
        // A place the tag names, as GitHub keeps an `id` or a `name` (behind its own prefix,
        // which its links' `#name` don't write): where the tag's line is.
        if self.skip.is_none() {
            for key in ["id", "name"] {
                if let Some(name) = attr(key).map(str::trim).filter(|n| !n.is_empty()) {
                    self.bare(block, &tag);
                    let src = tag.start;
                    self.push(
                        Inline::Anchor {
                            name: name.to_owned(),
                            src,
                        },
                        &(src..src),
                    );
                }
            }
        }
    }

    /// Text an HTML block holds outside any paragraph of its own starts one with no margins.
    fn bare(&mut self, block: bool, range: &Range<usize>) {
        if block && self.open.is_none() {
            let kind = OpenKind::Para {
                implicit: true,
                bare: true,
            };
            self.open_as(kind, range.clone());
        }
    }

    fn html_close(&mut self, name: &str, block: bool) {
        if let Some(at) = self
            .marks
            .iter()
            .rposition(|(_, tag)| tag.as_deref() == Some(name))
        {
            self.marks.remove(at);
            return;
        }
        if !block || !is_block(name) {
            return;
        }
        self.close();
        if let Some(at) = self.aligns.iter().rposition(|(tag, _)| tag == name) {
            self.aligns.truncate(at);
        }
        if name == "details" {
            // Only one opened in this container: a stray close can't end a list or a quote.
            let inner = self
                .frames
                .iter()
                .rposition(|f| !matches!(f.kind, Kind::Details { .. }));
            let details = self
                .frames
                .iter()
                .rposition(|f| matches!(f.kind, Kind::Details { .. }));
            if let Some(at) = details.filter(|d| inner.is_none_or(|i| i < *d)) {
                while self.frames.len() > at {
                    self.pop();
                }
            }
        }
    }

    // ------------------------------------------------------------ the end

    fn finish(mut self, end: usize) -> Doc {
        self.close();
        while self.frames.len() > 1 {
            self.pop();
        }
        // The notes that were referred to, by number, each ending in a way back to each of
        // its references: an arrow, the second and later with a raised count, as GitHub.
        let mut notes: Vec<Note> = Vec::new();
        for (label, mut blocks) in std::mem::take(&mut self.notes) {
            let Some(&(number, count)) = self.numbers.get(&label) else {
                continue;
            };
            if notes.iter().any(|n| n.number == number) {
                continue;
            }
            let mut ways = Vec::new();
            for n in 1..=count {
                let dest = format!("#{}", back_anchor(&label, n));
                let back = self.link(dest, String::new(), false, end..end);
                let style = Style {
                    link: Some(back),
                    ..Style::default()
                };
                let plain = Style::default();
                let text = |text: &str, style| Inline::Text {
                    text: text.to_owned(),
                    style,
                    src: end,
                };
                ways.extend([text(" ", plain), text("↩", style)]);
                if n > 1 {
                    let raised = Style { script: 1, ..style };
                    ways.push(text(&n.to_string(), raised));
                }
            }
            match blocks.last_mut() {
                Some(Block::Para(para)) => para.inlines.extend(ways),
                _ => blocks.push(Block::Para(Para {
                    inlines: ways,
                    align: Align::Start,
                    fit: Fit::Loose,
                    src: end..end,
                })),
            }
            notes.push(Note {
                number,
                label,
                blocks,
            });
        }
        notes.sort_by_key(|n| n.number);
        let mut blocks = self.frames.pop().map_or(Vec::new(), |root| root.blocks);
        if !notes.is_empty() {
            blocks.push(Block::Notes(notes));
        }
        Doc {
            blocks,
            links: self.links,
        }
    }
}

/// Front matter's keys and their values as text, each with where its line starts: a value's
/// quotes are read, a folded or literal block is its lines, and a list's items or a map's
/// entries under a key are its value's parts, a comma apart. Only as much YAML as front matter
/// holds: a key that isn't a plain `name:` starts nothing, and anything else is text.
fn front_matter(text: &str, from: usize) -> Vec<(String, String, usize)> {
    let mut lines = Vec::new();
    let mut at = from;
    for line in text.split_inclusive('\n') {
        lines.push((line.trim_end(), at));
        at += line.len();
    }
    let indented = |line: &str| line.starts_with([' ', '\t']) || line.trim().is_empty();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (line, start) = lines[i];
        i += 1;
        if indented(line) || line.starts_with(['#', '-']) {
            continue;
        }
        let (key, value) = match line.split_once(": ") {
            Some((key, value)) => (key, value.trim()),
            None => match line.strip_suffix(':') {
                Some(key) => (key, ""),
                None => continue,
            },
        };
        // What's indented under the key goes with it.
        let mut under = Vec::new();
        while i < lines.len() && indented(lines[i].0) {
            under.push(lines[i].0.trim());
            i += 1;
        }
        let under = under
            .into_iter()
            .filter(|l| !l.is_empty() && !l.starts_with('#'));
        let value = if value.starts_with(['>', '|']) {
            under.collect::<Vec<_>>().join(" ")
        } else if value.is_empty() {
            let part = |line: &str| match line.strip_prefix('-') {
                Some(item) => scalar(item.trim()),
                None => match line.split_once(": ") {
                    Some((k, v)) => format!("{}: {}", k.trim(), scalar(v.trim())),
                    None => scalar(line),
                },
            };
            under.map(part).collect::<Vec<_>>().join(", ")
        } else if let Some(list) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items = list.split(',').map(|item| scalar(item.trim()));
            items
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            scalar(value)
        };
        out.push((scalar(key.trim()), value, start));
    }
    out
}

/// A YAML scalar as the text it stands for: quotes read, a comment after it dropped.
fn scalar(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some('"') => {
            let (mut out, mut escaped) = (String::new(), false);
            for c in chars {
                if escaped {
                    out.push(if matches!(c, 'n' | 't') { ' ' } else { c });
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    break;
                } else {
                    out.push(c);
                }
            }
            out
        }
        Some('\'') => {
            let inner = value[1..].strip_suffix('\'').unwrap_or(&value[1..]);
            inner.replace("''", "'")
        }
        _ => match value.find(" #") {
            Some(at) => value[..at].trim_end().to_owned(),
            None => value.to_owned(),
        },
    }
}

/// The name a footnote's `n`th reference goes by, as GitHub names it: the label alone for the
/// first, the count after it for the rest.
fn back_anchor(label: &str, n: usize) -> String {
    match n {
        1 => format!("fnref-{label}"),
        n => format!("fnref-{label}-{n}"),
    }
}

/// An alert's word, for a marker line the source can't be read back from.
fn kind_name(kind: Option<BlockQuoteKind>) -> &'static str {
    match kind {
        Some(BlockQuoteKind::Tip) => "TIP",
        Some(BlockQuoteKind::Important) => "IMPORTANT",
        Some(BlockQuoteKind::Warning) => "WARNING",
        Some(BlockQuoteKind::Caution) => "CAUTION",
        _ => "NOTE",
    }
}

/// The HTML tags that end the paragraph before them.
fn is_block(name: &str) -> bool {
    const BLOCKS: [&str; 36] = [
        "address",
        "article",
        "aside",
        "blockquote",
        "center",
        "dd",
        "details",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hr",
        "li",
        "main",
        "nav",
        "ol",
        "p",
        "pre",
        "section",
        "summary",
        "table",
        "tbody",
        "thead",
        "tr",
        "ul",
    ];
    BLOCKS.contains(&name)
}

/// Addresses in plain text become links, as on GitHub: `http://`, `https://` and `www.` at the
/// start of a word, through to white space, less the punctuation a sentence ends an address
/// with. The parser splits text at characters that might have been markup, so an address is
/// looked for across neighbouring runs of one style.
fn autolink(inlines: &mut Vec<Inline>, links: &mut Vec<Link>) {
    let plain = |inline: &Inline| match inline {
        Inline::Text { style, .. } if style.link.is_none() && !style.code && !style.kbd => {
            Some(*style)
        }
        _ => None,
    };
    let hint = |inline: &Inline| match inline {
        Inline::Text { text, .. } => text.contains("://") || text.contains("www."),
        _ => false,
    };
    if !inlines.iter().any(hint) {
        return;
    }
    let mut out = Vec::with_capacity(inlines.len());
    let mut all = std::mem::take(inlines).into_iter().peekable();
    while let Some(first) = all.next() {
        let Some(style) = plain(&first) else {
            out.push(first);
            continue;
        };
        let mut run = vec![first];
        while let Some(next) = all.next_if(|i| plain(i) == Some(style)) {
            run.push(next);
        }
        let mut joined = String::new();
        for inline in &run {
            if let Inline::Text { text, .. } = inline {
                joined.push_str(text);
            }
        }
        let found = addresses(&joined);
        if found.is_empty() {
            out.extend(run);
            continue;
        }
        // Where each piece starts in the joined text and in the source.
        let mut at = 0;
        let starts: Vec<(usize, usize)> = run
            .iter()
            .map(|inline| {
                let from = at;
                let Inline::Text { text, src, .. } = inline else {
                    return (from, 0);
                };
                at += text.len();
                (from, *src)
            })
            .collect();
        let first = links.len() as u32;
        for (range, dest) in &found {
            let piece = starts.iter().rev().find(|(from, _)| *from <= range.start);
            let (from, src) = piece.copied().unwrap_or((0, 0));
            let start = src + (range.start - from);
            links.push(Link {
                dest: dest.clone(),
                title: String::new(),
                image: false,
                src: start..start + range.len(),
                anchor: None,
            });
        }
        // Each piece is cut where an address starts or ends inside it.
        for (inline, (from, _)) in run.into_iter().zip(starts) {
            let Inline::Text { text, src, .. } = inline else {
                continue;
            };
            let to = from + text.len();
            let mut cuts = vec![from];
            for (range, _) in &found {
                let inside = [range.start, range.end].into_iter();
                cuts.extend(inside.filter(|c| *c > from && *c < to));
            }
            cuts.push(to);
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let Some(piece) = text.get(a - from..b - from).filter(|p| !p.is_empty()) else {
                    continue;
                };
                let address = found.iter().position(|(r, _)| r.start <= a && b <= r.end);
                let mut style = style;
                style.link = address.map(|k| first + k as u32);
                out.push(Inline::Text {
                    text: piece.to_owned(),
                    style,
                    src: src + (a - from),
                });
            }
        }
    }
    *inlines = out;
}

/// The addresses in a text: where each is, and where it leads.
fn addresses(text: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while from < text.len() {
        let rest = &text[from..];
        let starts = ["https://", "http://", "www."];
        let next = starts
            .iter()
            .filter_map(|s| Some((rest.find(s)?, *s)))
            .min();
        let Some((at, scheme)) = next else {
            break;
        };
        let start = from + at;
        from = start + scheme.len();
        let before = text[..start].chars().next_back();
        if before.is_some_and(|c| !c.is_whitespace() && !matches!(c, '*' | '_' | '~' | '(')) {
            continue;
        }
        let tail = &text[start..];
        let mut end = tail
            .find(|c: char| c.is_whitespace() || c == '<')
            .unwrap_or(tail.len());
        // What ends a sentence isn't the address's, nor a bracket that closes one opened
        // before it.
        loop {
            let address = &tail[..end];
            let Some(last) = address.chars().next_back() else {
                break;
            };
            let opened = address.matches('(').count() < address.matches(')').count();
            let ends = matches!(
                last,
                '?' | '!' | '.' | ',' | ':' | ';' | '*' | '_' | '~' | '\'' | '"'
            );
            if ends || (last == ')' && opened) {
                end -= last.len_utf8();
            } else {
                break;
            }
        }
        let host = tail.get(scheme.len()..end).unwrap_or("");
        if !host.chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let address = &tail[..end];
        let dest = match scheme {
            "www." => format!("http://{address}"),
            _ => address.to_owned(),
        };
        out.push((start..start + end, dest));
        from = start + end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_in_plain_text_are_found_and_trimmed() {
        let found = |text: &str| -> Vec<String> {
            let all = addresses(text);
            all.into_iter().map(|(r, _)| text[r].to_owned()).collect()
        };
        assert_eq!(
            found("see https://example.com/a_b?c=d, then"),
            ["https://example.com/a_b?c=d"]
        );
        assert_eq!(
            found("(http://x.y/z(1)) and www.a.b."),
            ["http://x.y/z(1)", "www.a.b"]
        );
        assert_eq!(
            found("nothttp://x.y https:// www. ahttps://b.c"),
            Vec::<String>::new()
        );
        assert_eq!(addresses("www.a.b")[0].1, "http://www.a.b");
    }

    #[test]
    fn front_matter_values_are_read_as_yaml_has_them() {
        let text = "title: \"Quoted: title\"\ntags:\n  - alpha\n  - 'beta'\n  - gamma\nauthor:\n  name: Someone\n  url: https://example.com\ndescription: >\n  A folded description that runs on\n  over two lines.\nflow: [a, \"b c\"]\nplain: text # a comment\nquote: 'it''s'\n";
        let read: Vec<(String, String)> = front_matter(text, 0)
            .into_iter()
            .map(|(k, v, _)| (k, v))
            .collect();
        let want = [
            ("title", "Quoted: title"),
            ("tags", "alpha, beta, gamma"),
            ("author", "name: Someone, url: https://example.com"),
            (
                "description",
                "A folded description that runs on over two lines.",
            ),
            ("flow", "a, b c"),
            ("plain", "text"),
            ("quote", "it's"),
        ];
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(read, want);
    }

    #[test]
    fn an_address_split_by_the_parser_is_one_link() {
        let doc = parse(
            "go to https://example.com/a_b_c now, or <https://x.y>.\n",
            false,
        );
        // The parser's own link is made as it starts, the found one when the paragraph ends:
        // the page puts links in reading order, not this list.
        let found: Vec<(&str, Range<usize>)> = doc
            .links
            .iter()
            .map(|l| (l.dest.as_str(), l.src.clone()))
            .collect();
        assert_eq!(
            found,
            [
                ("https://x.y", 40..53),
                ("https://example.com/a_b_c", 6..31)
            ]
        );
    }
}
