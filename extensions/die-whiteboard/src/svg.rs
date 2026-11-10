//! The file: SVG in the whiteboard's own dialect, which any browser opens, and which reads back
//! exactly. Plain SVG 1.1 carries the picture: a `<path>` with round caps and joins for a stroke,
//! `<rect>`, `<ellipse>`, `<line>` with a `<marker>` arrowhead, `<text>` in a monospace family, a
//! `<style>` naming the inks. `data-wb-version="1"` on the root marks the dialect, and
//! `data-wb-*` attributes carry what SVG rounds or lacks: each element's id, ink number and
//! width index, a shape's corners as given, a label's text as typed, a line's bindings. An
//! export (`dialect` false) is the same picture without them.
//!
//! The reader is a tokenizer for this dialect alone: tags, attributes and text between them. A
//! file whose root has no `data-wb-version` is refused as foreign.

use crate::model::{Board, CellSize, Element, Id, Point, Rect, Shape};
use die_core::Rgb;
use std::fmt::{self, Write as _};

/// The dialect this build writes.
pub const VERSION: &str = "1";
/// The margin round the board's bounds in a file and an export, in board units.
pub const PAD: f32 = 16.0;
/// What an empty board's picture spans.
pub const EMPTY: (f32, f32) = (800.0, 600.0);
/// The family a label asks a browser for, then any monospace.
pub const FONT_FAMILY: &str = "'Ioskeley Mono Term', 'Iosevka', monospace";

/// How a board is coloured in a file or an export.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    /// Each ink's colour; index 0 is ink `1`.
    pub inks: [Rgb; 6],
    /// The background, or none for transparent.
    pub bg: Option<Rgb>,
    /// A cell in board units, which sizes labels.
    pub cell: CellSize,
}

impl Default for Look {
    /// White on a dark board, with xterm's red, yellow, green, blue and magenta.
    fn default() -> Look {
        let h = Rgb::hex;
        Look {
            inks: [
                h("#d0d0d0"),
                h("#cd0000"),
                h("#cdcd00"),
                h("#00cd00"),
                h("#0000ee"),
                h("#cd00cd"),
            ],
            bg: Some(h("#101010")),
            cell: CellSize::default(),
        }
    }
}

/// Why a file wasn't read.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    /// Not the whiteboard's dialect.
    Foreign,
    /// The dialect, but broken.
    Malformed(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Refused::Foreign => f.write_str("not a whiteboard file; open it in a browser"),
            Refused::Malformed(why) => write!(f, "not a whiteboard file DIE can read: {why}"),
        }
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// The picture's box: the board's bounds with [`PAD`] around, or [`EMPTY`] at the origin.
pub fn picture(board: &Board, cell: CellSize) -> Rect {
    match board.bounds(cell) {
        Some(b) => b.inflate(PAD),
        None => Rect::from_points(Point::default(), Point::new(EMPTY.0, EMPTY.1)),
    }
}

/// `text` as an attribute value or text content: the five entities, and a newline as `&#10;`
/// so a label's lines survive an attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            c => out.push(c),
        }
    }
    out
}

/// The text an attribute value or text content means.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e => e
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix('x') {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn points_attr(points: &[Point]) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let _ = write!(d, "{}{} {}", if i == 0 { "M " } else { " L " }, p.x, p.y);
    }
    if points.len() == 1 {
        let p = points[0];
        let _ = write!(d, " L {} {}", p.x, p.y);
    }
    d
}

/// The board as SVG: the file's dialect with `dialect`, or an export without the `data-wb-*`
/// attributes.
pub fn write(board: &Board, look: &Look, dialect: bool) -> String {
    let pic = picture(board, look.cell);
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = write!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {w} {h}\"",
        x = pic.min.x,
        y = pic.min.y,
        w = pic.width(),
        h = pic.height()
    );
    if dialect {
        let _ = write!(
            s,
            " data-wb-version=\"{VERSION}\" data-wb-next-id=\"{}\" data-wb-cell=\"{} {}\"",
            board.next_id, look.cell.w, look.cell.h
        );
    }
    s.push_str(">\n<style>\n");
    for (i, ink) in look.inks.iter().enumerate() {
        let _ = writeln!(s, ".i{}{{stroke:{c};fill:{c}}}", i + 1, c = hex(*ink));
    }
    let _ = writeln!(
        s,
        "text{{font-family:{FONT_FAMILY};font-size:{}px;white-space:pre}}",
        look.cell.h * 0.8
    );
    s.push_str("</style>\n<defs>\n");
    for (i, ink) in look.inks.iter().enumerate() {
        let _ = writeln!(
            s,
            "<marker id=\"arrow-{}\" viewBox=\"0 0 10 10\" refX=\"8\" refY=\"5\" markerWidth=\"4\" \
             markerHeight=\"4\" orient=\"auto\" markerUnits=\"strokeWidth\">\
             <path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"{}\" stroke=\"none\"/></marker>",
            i + 1,
            hex(*ink)
        );
    }
    s.push_str("</defs>\n");
    if let Some(bg) = look.bg {
        let _ = writeln!(
            s,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"none\"/>",
            pic.min.x,
            pic.min.y,
            pic.width(),
            pic.height(),
            hex(bg)
        );
    }
    for el in &board.elements {
        write_element(&mut s, el, look, dialect);
    }
    s.push_str("</svg>\n");
    s
}

fn write_element(s: &mut String, el: &Element, look: &Look, dialect: bool) {
    let class = format!("i{}", el.ink.clamp(1, 6));
    let width = el.stroke_width();
    let common = if dialect {
        format!(
            " data-wb-id=\"{}\" data-wb-ink=\"{}\" data-wb-width=\"{}\"",
            el.id, el.ink, el.width
        )
    } else {
        String::new()
    };
    match &el.shape {
        Shape::Stroke {
            points,
            highlighter,
        } => {
            let alpha = if *highlighter {
                format!(" stroke-opacity=\"{}\"", crate::model::HIGHLIGHTER_ALPHA)
            } else {
                String::new()
            };
            let hl = if *highlighter && dialect {
                " data-wb-hl=\"1\""
            } else {
                ""
            };
            let _ = writeln!(
                s,
                "<path class=\"{class}\" fill=\"none\" stroke-width=\"{width}\" stroke-linecap=\"round\" \
                 stroke-linejoin=\"round\"{alpha} d=\"{}\"{common}{hl}/>",
                points_attr(points)
            );
        }
        Shape::Rect { a, b } => {
            let r = Rect::from_points(*a, *b);
            let corners = corners(a, b, dialect);
            let _ = writeln!(
                s,
                "<rect class=\"{class}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" \
                 stroke-width=\"{width}\" stroke-linejoin=\"round\"{common}{corners}/>",
                r.min.x,
                r.min.y,
                r.width(),
                r.height()
            );
        }
        Shape::Ellipse { a, b } => {
            let r = Rect::from_points(*a, *b);
            let c = r.center();
            let corners = corners(a, b, dialect);
            let _ = writeln!(
                s,
                "<ellipse class=\"{class}\" cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"none\" \
                 stroke-width=\"{width}\"{common}{corners}/>",
                c.x,
                c.y,
                r.width() / 2.0,
                r.height() / 2.0
            );
        }
        Shape::Line {
            a,
            b,
            arrow,
            from,
            to,
        } => {
            let marker = if *arrow {
                format!(" marker-end=\"url(#arrow-{})\"", el.ink.clamp(1, 6))
            } else {
                String::new()
            };
            let mut extra = String::new();
            if dialect {
                if *arrow {
                    extra.push_str(" data-wb-arrow=\"1\"");
                }
                if let Some(id) = from {
                    let _ = write!(extra, " data-wb-from=\"{id}\"");
                }
                if let Some(id) = to {
                    let _ = write!(extra, " data-wb-to=\"{id}\"");
                }
            }
            let _ = writeln!(
                s,
                "<line class=\"{class}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-width=\"{width}\" \
                 stroke-linecap=\"round\"{marker}{common}{extra}/>",
                a.x, a.y, b.x, b.y
            );
        }
        Shape::Label { at, text } => {
            let kept = if dialect {
                format!(" data-wb-text=\"{}\"", escape(text))
            } else {
                String::new()
            };
            let _ = write!(
                s,
                "<text class=\"{class}\" x=\"{}\" y=\"{}\" stroke=\"none\"{common}{kept}>",
                at.x, at.y
            );
            let ascent = look.cell.h * 0.8;
            for (i, line) in crate::model::label_lines(text).iter().enumerate() {
                let dy = if i == 0 { ascent } else { look.cell.h };
                let _ = write!(
                    s,
                    "<tspan x=\"{}\" dy=\"{dy}\">{}</tspan>",
                    at.x,
                    escape(line)
                );
            }
            s.push_str("</text>\n");
        }
    }
}

fn corners(a: &Point, b: &Point, dialect: bool) -> String {
    if dialect {
        format!(
            " data-wb-a=\"{} {}\" data-wb-b=\"{} {}\"",
            a.x, a.y, b.x, b.y
        )
    } else {
        String::new()
    }
}

// ------------------------------------------------------------------ reading

/// A tag, as the tokenizer gives it.
#[derive(Debug, PartialEq)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    self_closing: bool,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Tag(Tag),
    Text(String),
}

/// The tokens of `text`: tags and the text between them; comments, the XML declaration and
/// doctypes are skipped.
fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut out = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Token::Text(unescape(rest)));
            break;
        };
        if lt > 0 {
            out.push(Token::Text(unescape(&rest[..lt])));
        }
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").ok_or("an unclosed comment")?;
            rest = &after[end + 3..];
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>').ok_or("an unclosed declaration")?;
            rest = &rest[end + 1..];
            continue;
        }
        let end = rest.find('>').ok_or("an unclosed tag")?;
        let inside = &rest[1..end];
        rest = &rest[end + 1..];
        let (closing, inside) = match inside.strip_prefix('/') {
            Some(i) => (true, i),
            None => (false, inside),
        };
        let (self_closing, inside) = match inside.strip_suffix('/') {
            Some(i) => (true, i),
            None => (false, inside),
        };
        let inside = inside.trim();
        let name_end = inside
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inside.len());
        let (name, mut attrs_text) = inside.split_at(name_end);
        let mut attrs = vec![];
        loop {
            attrs_text = attrs_text.trim_start();
            if attrs_text.is_empty() {
                break;
            }
            let eq = attrs_text.find('=').ok_or("an attribute without a value")?;
            let key = attrs_text[..eq].trim().to_string();
            let after = attrs_text[eq + 1..].trim_start();
            let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'');
            let quote = quote.ok_or("an unquoted attribute")?;
            let close = after[1..].find(quote).ok_or("an unclosed attribute")?;
            attrs.push((key, unescape(&after[1..1 + close])));
            attrs_text = &after[close + 2..];
        }
        out.push(Token::Tag(Tag {
            name: name.to_string(),
            attrs,
            closing,
            self_closing,
        }));
    }
    Ok(out)
}

fn number(tag: &Tag, name: &str) -> Result<f32, String> {
    let v = tag
        .attr(name)
        .ok_or_else(|| format!("<{}> without {name}", tag.name))?;
    v.trim()
        .parse()
        .map_err(|_| format!("<{}> {name}={v:?}", tag.name))
}

fn point(tag: &Tag, name: &str) -> Result<Point, String> {
    let v = tag
        .attr(name)
        .ok_or_else(|| format!("<{}> without {name}", tag.name))?;
    let mut it = v.split_whitespace().map(|n| n.parse::<f32>());
    match (it.next(), it.next()) {
        (Some(Ok(x)), Some(Ok(y))) => Ok(Point::new(x, y)),
        _ => Err(format!("<{}> {name}={v:?}", tag.name)),
    }
}

fn id_attr(tag: &Tag, name: &str) -> Result<Option<Id>, String> {
    tag.attr(name)
        .map(|v| {
            v.trim()
                .parse()
                .map_err(|_| format!("<{}> {name}={v:?}", tag.name))
        })
        .transpose()
}

fn path_points(d: &str) -> Result<Vec<Point>, String> {
    let nums: Result<Vec<f32>, _> = d
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty() && *t != "M" && *t != "L")
        .map(|t| t.parse::<f32>())
        .collect();
    let nums = nums.map_err(|_| format!("a path with d={d:?}"))?;
    if nums.len() < 2 || nums.len() % 2 != 0 {
        return Err(format!("a path with d={d:?}"));
    }
    let mut points: Vec<Point> = nums.chunks(2).map(|c| Point::new(c[0], c[1])).collect();
    if points.len() == 2 && points[0] == points[1] {
        points.pop(); // a dot, written as a zero-length segment
    }
    Ok(points)
}

/// The element a tag with `data-wb-id` describes, its text content given for a `<text>`.
fn element(tag: &Tag, id: Id, content: &str) -> Result<Element, String> {
    let ink = number(tag, "data-wb-ink")? as u8;
    let width = number(tag, "data-wb-width")? as u8;
    let shape = match tag.name.as_str() {
        "path" => Shape::Stroke {
            points: path_points(tag.attr("d").unwrap_or(""))?,
            highlighter: tag.attr("data-wb-hl") == Some("1"),
        },
        "rect" => Shape::Rect {
            a: point(tag, "data-wb-a")?,
            b: point(tag, "data-wb-b")?,
        },
        "ellipse" => Shape::Ellipse {
            a: point(tag, "data-wb-a")?,
            b: point(tag, "data-wb-b")?,
        },
        "line" => Shape::Line {
            a: Point::new(number(tag, "x1")?, number(tag, "y1")?),
            b: Point::new(number(tag, "x2")?, number(tag, "y2")?),
            arrow: tag.attr("data-wb-arrow") == Some("1"),
            from: id_attr(tag, "data-wb-from")?,
            to: id_attr(tag, "data-wb-to")?,
        },
        "text" => Shape::Label {
            at: Point::new(number(tag, "x")?, number(tag, "y")?),
            text: tag
                .attr("data-wb-text")
                .map_or_else(|| content.to_string(), str::to_string),
        },
        other => return Err(format!("<{other}> with a data-wb-id")),
    };
    Ok(Element {
        id,
        ink,
        width,
        shape,
    })
}

/// The board a file holds. Empty text is an empty board.
pub fn read(text: &str) -> Result<Board, Refused> {
    if text.trim().is_empty() {
        return Ok(Board::default());
    }
    let tokens = tokenize(text).map_err(Refused::Malformed)?;
    let mut tags = tokens.iter();
    let root = loop {
        match tags.next() {
            Some(Token::Tag(t)) if !t.closing => break t,
            Some(_) => continue,
            None => return Err(Refused::Foreign),
        }
    };
    if root.name != "svg" || root.attr("data-wb-version").is_none() {
        return Err(Refused::Foreign);
    }
    let mut board = Board::default();
    let mut label: Option<(&Tag, Id, String)> = None;
    for tok in tags {
        match tok {
            Token::Text(t) => {
                if let Some((_, _, content)) = &mut label {
                    content.push_str(t);
                }
            }
            Token::Tag(t) if t.closing && t.name == "text" => {
                if let Some((tag, id, content)) = label.take() {
                    let lines: Vec<&str> = content.split('\n').collect();
                    let el = element(tag, id, &lines.join("\n")).map_err(Refused::Malformed)?;
                    board.insert(board.len(), el);
                }
            }
            Token::Tag(t) if t.closing => {}
            Token::Tag(t) if t.name == "tspan" => {
                if let Some((_, _, content)) = &mut label
                    && !content.is_empty()
                {
                    content.push('\n');
                }
            }
            Token::Tag(t) => {
                let Some(id) = id_attr(t, "data-wb-id").map_err(Refused::Malformed)? else {
                    continue;
                };
                if t.name == "text" && !t.self_closing {
                    label = Some((t, id, String::new()));
                    continue;
                }
                let el = element(t, id, "").map_err(Refused::Malformed)?;
                board.insert(board.len(), el);
            }
        }
    }
    if let Some(next) = root
        .attr("data-wb-next-id")
        .and_then(|n| n.trim().parse().ok())
    {
        board.next_id = board.next_id.max(next);
    }
    Ok(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_round_trip() {
        let s = "a<b>&\"c'\nd";
        assert_eq!(escape(s), "a&lt;b&gt;&amp;&quot;c&apos;&#10;d");
        assert_eq!(unescape(&escape(s)), s);
        assert_eq!(unescape("x&#x41;y&bogus;"), "xAy&bogus;");
    }

    #[test]
    fn the_tokenizer_reads_tags_attributes_and_text() {
        let toks = tokenize("<?xml v?><!-- c --><a x=\"1\" y='two'><b/>t&amp;t</a>").unwrap();
        assert_eq!(toks.len(), 4);
        let Token::Tag(a) = &toks[0] else { panic!() };
        assert_eq!(
            (a.name.as_str(), a.attr("x"), a.attr("y")),
            ("a", Some("1"), Some("two"))
        );
        let Token::Tag(b) = &toks[1] else { panic!() };
        assert!(b.self_closing && b.name == "b");
        assert_eq!(toks[2], Token::Text("t&t".into()));
        assert!(matches!(&toks[3], Token::Tag(t) if t.closing));
        assert!(tokenize("<a x=1>").is_err(), "unquoted");
    }

    #[test]
    fn foreign_and_empty_files() {
        assert_eq!(read("<svg xmlns=\"x\"></svg>"), Err(Refused::Foreign));
        assert_eq!(read("hello"), Err(Refused::Foreign));
        assert_eq!(read("  \n"), Ok(Board::default()));
        assert_eq!(
            Refused::Foreign.to_string(),
            "not a whiteboard file; open it in a browser"
        );
    }
}
