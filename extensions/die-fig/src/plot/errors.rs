//! What a figure shows when it can't draw (contract-p2b §1): the error figure, in two states,
//! and the stale badge.
//!
//! - **Symbolic**, the default: centred and quiet, a muted outline glyph for the kind of failure
//!   (a document with a question mark when the text couldn't be had, a document with a squiggled
//!   line when it doesn't parse, two pieces that don't fit when the data doesn't fit the spec,
//!   empty dashed bars when there's nothing to draw), the status as an icon and a label (never
//!   colour alone), the title `Can't draw NAME` (or `Nothing to draw in NAME`), the first error
//!   with its place, and a hint of the keys. A small area drops the hint, then the message's
//!   second line, then the status and the message (the glyph and the title), then the title
//!   (the glyph and its status), then shows the glyph alone, then the title alone, so nothing
//!   ever overlaps.
//! - **Log**: every error and warning of the newest parse in the monospace, as rustc writes them:
//!   the kind and the message, the file and line:column, the source line with a caret under the
//!   column (clipped to the width, the caret kept in view), and what was tried. It scrolls by
//!   lines; the marked entry is lit.
//!
//! The stale badge sits in the top-right corner of a figure that's kept while its source fails
//! to parse: the status icon and `Error · line 14`.
//!
//! Each draws into a [`Scene`] with regions: the badge, the symbolic figure (and the log's
//! header), each log entry (ruling 5).

use crate::error::{ErrorKind, FigError};
use crate::scene::{
    Anchor, Cap, Hit, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Verb,
    Weight,
};
use crate::theme::{Color, Theme};

/// Which state the error figure is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ErrorView {
    /// The glyph, the status, the title and the first error.
    #[default]
    Symbolic,
    /// Every error and warning: `top` is the first line of the log shown, `mark` the entry lit.
    Log {
        /// The first line shown, from 0.
        top: usize,
        /// The entry marked, from 0.
        mark: usize,
    },
}

/// The log's text size.
pub const LOG_TEXT: f32 = 12.0;
/// A line of the log.
pub const LOG_LINE: f32 = 18.0;
/// The log's header, over its lines.
pub const LOG_HEADER: f32 = 40.0;
/// From the area's edge to the log's text.
const LOG_PAD: f32 = 20.0;
/// What the log keeps under its last line.
const LOG_FOOT: f32 = 10.0;

/// A status colour as text on the theme's ground: mixed towards white on a dark ground or
/// black on a light one until it reads at 4.5 to 1, the contrast small text asks for.
pub fn readable(c: Color, theme: &Theme) -> Color {
    let ground = lum(theme.surface);
    let ratio = |c: Color| {
        let (a, b) = (lum(c), ground);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    };
    let to = if theme.dark { 255.0 } else { 0.0 };
    let mix = |t: f32| {
        let m = |v: u8| (f32::from(v) + (to - f32::from(v)) * t).round() as u8;
        Color(m(c.0), m(c.1), m(c.2))
    };
    (0..=20)
        .map(|i| mix(i as f32 * 0.05))
        .find(|&c| ratio(c) >= 4.5)
        .unwrap_or(mix(1.0))
}

/// A colour's relative luminance (WCAG's).
fn lum(c: Color) -> f32 {
    let ch = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
}

/// The contrast between two colours, 1 to 21 (WCAG's ratio).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (lum(a), lum(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// What the symbolic figure's status row says.
fn status_label(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Unreadable => "Unreadable",
        ErrorKind::Malformed => "Malformed",
        ErrorKind::Mismatch => "Mismatch",
        ErrorKind::Empty => "Empty",
    }
}

/// The error the symbolic figure and the badge are about: the first that isn't a warning, else
/// the first.
fn lead(errors: &[FigError]) -> Option<&FigError> {
    errors.iter().find(|e| !e.warning).or(errors.first())
}

/// The title: `Can't draw NAME`, or `Nothing to draw in NAME`.
pub fn title(kind: ErrorKind, name: &str) -> String {
    let name = if name.is_empty() { "this" } else { name };
    match kind {
        ErrorKind::Empty => format!("Nothing to draw in {name}"),
        _ => format!("Can't draw {name}"),
    }
}

/// The first error as the symbolic figure says it: its place, then what went wrong, without a
/// "nothing to draw:" the title has said already.
pub fn summary(e: &FigError) -> String {
    let msg = e.msg.strip_prefix("nothing to draw: ").unwrap_or(&e.msg);
    match e.place() {
        Some(p) => format!("{p}: {msg}"),
        None => msg.to_string(),
    }
}

/// Draws the error figure for `errors` (errors first, then warnings) over the whole area:
/// symbolic or the log. `source` is the figure's own text, for the log's caret lines, and
/// `name` its name, which the title and the log's places say.
#[allow(clippy::too_many_arguments)]
pub fn error_figure(
    errors: &[FigError],
    source: &str,
    name: &str,
    state: ErrorView,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(size.0, size.1, scale);
    match state {
        ErrorView::Symbolic => symbolic(&mut scene, errors, name, theme, m),
        ErrorView::Log { top, mark } => log(&mut scene, errors, source, name, top, mark, theme, m),
    }
    scene
}

// ------------------------------------------------------------------ the symbolic state

/// What the symbolic figure shows at a size, from all of it down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rung {
    /// Glyph, status, title, the message in so many lines, and the hint if `hint`.
    Full { lines: usize, hint: bool },
    /// Glyph and title.
    Titled,
    /// Glyph and the status: its icon and label, without the count.
    Status,
    /// The glyph alone.
    Glyph,
    /// The title alone.
    Title,
    /// Not even that.
    Nothing,
}

/// The glyph's side at a size.
fn glyph_side(w: f32, h: f32) -> f32 {
    (w.min(h * 1.6) * 0.11).clamp(40.0, 56.0)
}

const GLYPH_GAP: f32 = 18.0;
const STATUS_H: f32 = 16.0;
const STATUS_GAP: f32 = 10.0;
const TITLE_GAP: f32 = 6.0;
const MSG_LINE: f32 = 17.0;
const HINT_GAP: f32 = 16.0;
const HINT_H: f32 = 18.0;

fn symbolic(
    scene: &mut Scene,
    errors: &[FigError],
    name: &str,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let (w, h) = (scene.width, scene.height);
    scene.region(Rect::new(0.0, 0.0, w, h), Hit::ErrorFigure);
    let Some(first) = lead(errors) else {
        return;
    };
    let kind = first.kind;
    let margin = if h < 200.0 || w < 300.0 { 8.0 } else { 16.0 };
    let (aw, ah) = ((w - 2.0 * margin).max(0.0), (h - 2.0 * margin).max(0.0));
    let title_spec = TextSpec::new(15.0).weight(Weight::Semibold);
    let msg_spec = TextSpec::new(12.0);
    let title_text = title(kind, name);
    let title_h = m.ascent(title_spec) + m.descent(title_spec);
    let title_fits = m.width(&title_text, title_spec) <= aw;
    let g = glyph_side(w, h);
    let text_w = aw.min(560.0);
    let message = summary(first);
    let msg_lines = wrap(&message, msg_spec, text_w, 2, m);
    let keys = hint_parts();
    let hint_w = hint_width(&keys, m);
    let full = |lines: usize, hint: bool| {
        g + GLYPH_GAP
            + STATUS_H
            + STATUS_GAP
            + title_h
            + TITLE_GAP
            + lines as f32 * MSG_LINE
            + if hint { HINT_GAP + HINT_H } else { 0.0 }
    };
    // Words want room to be read: below 230 px across, the glyph and the title at most (a
    // third of an 880 px dashboard still says what went wrong).
    let wordy = aw >= 230.0;
    let one = msg_lines.len().min(1);
    // The hint goes before the message's second line: what went wrong matters more than how
    // to see more of it.
    let rung = [
        Rung::Full {
            lines: msg_lines.len(),
            hint: hint_w <= aw,
        },
        Rung::Full {
            lines: msg_lines.len(),
            hint: false,
        },
        Rung::Full {
            lines: one,
            hint: false,
        },
    ]
    .into_iter()
    .find(|r| match *r {
        Rung::Full { lines, hint } => wordy && title_fits && full(lines, hint) <= ah,
        _ => false,
    })
    .unwrap_or(if title_fits && g + GLYPH_GAP + title_h <= ah {
        Rung::Titled
    } else if g + GLYPH_GAP + STATUS_H <= ah && status_width(first, errors, false, m) <= aw {
        Rung::Status
    } else if g <= ah && g <= aw {
        Rung::Glyph
    } else if title_h <= ah && aw >= 24.0 {
        Rung::Title
    } else {
        Rung::Nothing
    });
    let block = match rung {
        Rung::Full { lines, hint } => full(lines, hint),
        Rung::Titled => g + GLYPH_GAP + title_h,
        Rung::Status => g + GLYPH_GAP + STATUS_H,
        Rung::Glyph => g,
        Rung::Title => title_h,
        Rung::Nothing => 0.0,
    };
    // A little over the middle, where the eye takes the middle to be.
    let cx = (w / 2.0).round();
    let lift = (h * 0.03).min(12.0);
    let mut y = ((h - block) / 2.0 - lift).max(margin).round();
    if matches!(
        rung,
        Rung::Full { .. } | Rung::Titled | Rung::Status | Rung::Glyph
    ) {
        glyph(scene, kind, Rect::new(cx - g / 2.0, y, g, g), theme);
        y += g + GLYPH_GAP;
    }
    if matches!(rung, Rung::Full { .. } | Rung::Status) {
        let count = matches!(rung, Rung::Full { .. });
        status_row(scene, first, errors, count, cx, y, theme, m);
        y += STATUS_H + STATUS_GAP;
    }
    if matches!(rung, Rung::Full { .. } | Rung::Titled | Rung::Title) {
        let text = if title_fits {
            title_text
        } else {
            fit_words(&title_text, title_spec, aw, m)
        };
        scene.push(text_item(
            &text,
            cx,
            y + m.ascent(title_spec),
            title_spec,
            Anchor::Middle,
            theme.strong,
        ));
        y += title_h;
    }
    if let Rung::Full { lines, hint } = rung {
        y += TITLE_GAP;
        for (i, line) in msg_lines.iter().take(lines).enumerate() {
            // The last line shown carries the rest, cut to the width.
            let text = if i + 1 == lines {
                fit_words(&msg_lines[i..].join(" "), msg_spec, text_w, m)
            } else {
                line.clone()
            };
            let base = y + (MSG_LINE + m.ascent(msg_spec) - m.descent(msg_spec)) / 2.0;
            scene.push(text_item(
                &text,
                cx,
                base,
                msg_spec,
                Anchor::Middle,
                theme.text,
            ));
            y += MSG_LINE;
        }
        if hint {
            y += HINT_GAP;
            draw_hint(scene, &keys, cx - hint_w / 2.0, y, theme, m);
        }
    }
}

/// What the status row says after its label: how many errors there are when there's more
/// than one, if `count`.
fn status_more(errors: &[FigError], count: bool) -> Option<String> {
    let n = errors.iter().filter(|e| !e.warning).count();
    (count && n > 1).then(|| format!(" · {n} errors"))
}

/// The status row's width.
fn status_width(first: &FigError, errors: &[FigError], count: bool, m: &mut dyn Measure) -> f32 {
    let spec = TextSpec::new(12.0).weight(Weight::Medium);
    let lw = m.width(status_label(first.kind), spec);
    let mw = status_more(errors, count).map_or(0.0, |t| m.width(&t, TextSpec::new(12.0)));
    13.0 + 6.0 + lw + mw
}

/// The status row: the icon and the label, and how many errors there are when there's more
/// than one and `count`.
#[allow(clippy::too_many_arguments)]
fn status_row(
    scene: &mut Scene,
    first: &FigError,
    errors: &[FigError],
    count: bool,
    cx: f32,
    y: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let spec = TextSpec::new(12.0).weight(Weight::Medium);
    let label = status_label(first.kind);
    let more = status_more(errors, count);
    let icon = 13.0;
    let lw = m.width(label, spec);
    let total = status_width(first, errors, count, m);
    let x = (cx - total / 2.0).round();
    let mid = y + STATUS_H / 2.0;
    status_icon(
        scene,
        (x + icon / 2.0, mid),
        icon / 2.0,
        first.kind.critical(),
        theme,
    );
    let base = mid + m.cap(spec) / 2.0;
    scene.push(text_item(
        label,
        x + icon + 6.0,
        base,
        spec,
        Anchor::Start,
        theme.text,
    ));
    if let Some(t) = more {
        let at = x + icon + 6.0 + lw;
        scene.push(text_item(
            &t,
            at,
            base,
            TextSpec::new(12.0),
            Anchor::Start,
            theme.muted,
        ));
    }
}

/// The hint's pieces: keys (drawn as caps) and words.
fn hint_parts() -> Vec<(bool, String)> {
    vec![
        (true, "!".into()),
        (false, "details".into()),
        (false, "·".into()),
        (true, "\\v".into()),
        (false, "source".into()),
    ]
}

const CAP_PAD: f32 = 5.0;
const HINT_SPACE: f32 = 6.0;

fn key_spec() -> TextSpec {
    TextSpec::new(11.0).mono()
}

fn word_spec() -> TextSpec {
    TextSpec::new(11.0)
}

fn hint_width(parts: &[(bool, String)], m: &mut dyn Measure) -> f32 {
    let mut w = 0.0;
    for (i, (key, text)) in parts.iter().enumerate() {
        if i > 0 {
            w += HINT_SPACE;
        }
        w += if *key {
            m.width(text, key_spec()).max(7.0) + 2.0 * CAP_PAD
        } else {
            m.width(text, word_spec())
        };
    }
    w
}

/// Keys as small caps on hairline outlines, words muted, from `x` along the row at `y`.
fn draw_hint(
    scene: &mut Scene,
    parts: &[(bool, String)],
    x: f32,
    y: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let mut x = x.round();
    let mid = y + HINT_H / 2.0;
    for (i, (key, text)) in parts.iter().enumerate() {
        if i > 0 {
            x += HINT_SPACE;
        }
        if *key {
            let tw = m.width(text, key_spec()).max(7.0);
            let cap = Rect::new(x, y, tw + 2.0 * CAP_PAD, HINT_H);
            scene.push(Item::Path {
                path: PathData::rounded(cap, [4.0; 4]),
                fill: Some(Paint::solid(
                    theme.text,
                    if theme.dark { 0.05 } else { 0.04 },
                )),
                stroke: Some(Stroke::hairline(theme.ring, 1.0)),
            });
            let base = mid + m.cap(key_spec()) / 2.0;
            scene.push(text_item(
                text,
                x + cap.w / 2.0,
                base,
                key_spec(),
                Anchor::Middle,
                theme.text,
            ));
            x += cap.w;
        } else {
            let base = mid + m.cap(word_spec()) / 2.0;
            scene.push(text_item(
                text,
                x,
                base,
                word_spec(),
                Anchor::Start,
                theme.muted,
            ));
            x += m.width(text, word_spec());
        }
    }
}

/// `text` broken at spaces into at most `most` lines no wider than `max`; what's left over
/// rides on the last, which the caller cuts.
fn wrap(text: &str, spec: TextSpec, max: f32, most: usize, m: &mut dyn Measure) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let tried = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if m.width(&tried, spec) <= max || cur.is_empty() {
            cur = tried;
        } else {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > most {
        let rest = lines.split_off(most);
        let last = lines.last_mut().expect("most is at least one");
        last.push(' ');
        last.push_str(&rest.join(" "));
    }
    // Two lines break where they're most even, so the second isn't a word left over.
    if lines.len() == 2 && most >= 2 {
        let words: Vec<&str> = text.split(' ').collect();
        let mut best: Option<(f32, String, String)> = None;
        for k in 1..words.len() {
            let (a, b) = (words[..k].join(" "), words[k..].join(" "));
            let (wa, wb) = (m.width(&a, spec), m.width(&b, spec));
            if wa <= max && wb <= max && best.as_ref().is_none_or(|x| wa.max(wb) < x.0) {
                best = Some((wa.max(wb), a, b));
            }
        }
        if let Some((_, a, b)) = best {
            return vec![a, b];
        }
    }
    lines
}

/// What a cut leaves dangling before its '…': a space, a full stop, a comma, a colon, a dot
/// between words.
fn dangling(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | ',' | ':' | ';' | '·')
}

/// `text` cut to `max` with an ellipsis as [`Measure::fit`] cuts it, but at the end of a word
/// where one is a few characters back, and with nothing dangling before the '…' (never
/// `expected …` or `latency-ms.…`).
pub fn fit_words(text: &str, spec: TextSpec, max: f32, m: &mut dyn Measure) -> String {
    let cut = m.fit(text, spec, max);
    if cut == text {
        return cut;
    }
    let Some(kept) = cut.strip_suffix('…') else {
        return cut;
    };
    let chars: Vec<char> = kept.chars().collect();
    let mut end = chars.len();
    // Cut in the middle of a word: back to its start where that's near.
    let mid_word = text.chars().nth(end).is_some_and(|c| !c.is_whitespace());
    if mid_word
        && let Some(sp) = (end.saturating_sub(6)..end)
            .rev()
            .find(|&i| chars[i] == ' ')
        && sp * 2 >= end
    {
        end = sp;
    }
    while end > 0 && dangling(chars[end - 1]) {
        end -= 1;
    }
    if end == 0 {
        return cut;
    }
    format!("{}…", chars[..end].iter().collect::<String>())
}

fn text_item(text: &str, x: f32, y: f32, spec: TextSpec, anchor: Anchor, color: Color) -> Item {
    Item::Text {
        text: text.into(),
        x,
        y,
        spec,
        anchor,
        color,
        alpha: 1.0,
        max_width: None,
    }
}

// ------------------------------------------------------------------ every series hidden

/// What a chart shows in its plot when every series is hidden (the empty kind, contract-p2b
/// §1): the empty glyph, the status, `Every series is hidden`, and how to bring them back,
/// centred in `plot`, the chart's legend and axes around it kept so a click on a key brings
/// its series back. A short plot drops the hint, then the status, then the glyph.
pub fn all_hidden(scene: &mut Scene, plot: Rect, theme: &Theme, m: &mut dyn Measure) {
    let title_spec = TextSpec::new(13.0).weight(Weight::Semibold);
    let title = "Every series is hidden";
    let keys: Vec<(bool, String)> = vec![
        (true, "X".into()),
        (false, "shows them all".into()),
        (false, "·".into()),
        (false, "or click a key".into()),
    ];
    let room = (plot.w - 16.0).max(0.0);
    let g = if plot.h >= 200.0 { 40.0 } else { 32.0 };
    let gap = 14.0;
    let title_h = m.ascent(title_spec) + m.descent(title_spec);
    let title_w = m.width(title, title_spec);
    let hint_w = hint_width(&keys, m);
    let height = |glyph: bool, status: bool, hint: bool| {
        let mut h = title_h;
        if glyph {
            h += g + gap;
        }
        if status {
            h += STATUS_H + STATUS_GAP;
        }
        if hint {
            h += HINT_GAP + HINT_H;
        }
        h
    };
    let fits = |glyph: bool, status: bool, hint: bool| {
        height(glyph, status, hint) <= plot.h - 16.0 && title_w <= room && (!hint || hint_w <= room)
    };
    let Some((glyph_on, status, hint)) = [
        (true, true, true),
        (true, true, false),
        (true, false, false),
        (false, false, false),
    ]
    .into_iter()
    .find(|&(a, b, c)| fits(a, b, c)) else {
        return;
    };
    let cx = (plot.x + plot.w / 2.0).round();
    let mut y = (plot.y + (plot.h - height(glyph_on, status, hint)) / 2.0).round();
    if glyph_on {
        glyph(
            scene,
            ErrorKind::Empty,
            Rect::new(cx - g / 2.0, y, g, g),
            theme,
        );
        y += g + gap;
    }
    if status {
        let why = FigError::new("every series is hidden").with_kind(ErrorKind::Empty);
        status_row(
            scene,
            &why,
            std::slice::from_ref(&why),
            false,
            cx,
            y,
            theme,
            m,
        );
        y += STATUS_H + STATUS_GAP;
    }
    scene.push(text_item(
        title,
        cx,
        y + m.ascent(title_spec),
        title_spec,
        Anchor::Middle,
        theme.strong,
    ));
    y += title_h;
    if hint {
        draw_hint(scene, &keys, cx - hint_w / 2.0, y + HINT_GAP, theme, m);
    }
}

// ------------------------------------------------------------------ the glyphs and the icon

/// The status icon, centred at `c`: a disc with an exclamation mark for what's critical, a
/// triangle with one for a warning, the mark white or near black by the fill's luminance.
pub fn status_icon(scene: &mut Scene, c: (f32, f32), r: f32, critical: bool, theme: &Theme) {
    let (cx, cy) = c;
    let fill = if critical {
        theme.status[3]
    } else {
        theme.status[1]
    };
    let ink = super::marks::on_fill(fill);
    if critical {
        scene.push(Item::Path {
            path: PathData::circle(cx, cy, r),
            fill: Some(Paint::solid(fill, 1.0)),
            stroke: None,
        });
    } else {
        let t = PathData {
            verbs: vec![
                Verb::Move(cx, cy - r * 0.95),
                Verb::Line(cx + r * 1.05, cy + r * 0.85),
                Verb::Line(cx - r * 1.05, cy + r * 0.85),
                Verb::Close,
            ],
        };
        scene.push(Item::Path {
            path: t,
            fill: Some(Paint::solid(fill, 1.0)),
            stroke: Some(Stroke {
                width: r * 0.3,
                paint: Paint::solid(fill, 1.0),
                cap: Cap::Round,
                join: Join::Round,
                dash: None,
                hairline: false,
            }),
        });
    }
    // The mark, a little low in the triangle, whose middle is.
    let dy = if critical { 0.0 } else { r * 0.18 };
    let bar = Rect::new(cx - r * 0.11, cy - r * 0.5 + dy, r * 0.22, r * 0.58);
    scene.push(Item::Path {
        path: PathData::rounded(bar, [r * 0.11; 4]),
        fill: Some(Paint::solid(ink, 1.0)),
        stroke: None,
    });
    scene.push(Item::Path {
        path: PathData::circle(cx, cy + r * 0.38 + dy, r * 0.13),
        fill: Some(Paint::solid(ink, 1.0)),
        stroke: None,
    });
}

/// The kind's outline glyph in `r`, drawn on a 48-unit grid: muted, on a faint disc.
pub fn glyph(scene: &mut Scene, kind: ErrorKind, r: Rect, theme: &Theme) {
    let s = r.w.min(r.h) / 48.0;
    let (ox, oy) = (r.x + (r.w - 48.0 * s) / 2.0, r.y + (r.h - 48.0 * s) / 2.0);
    let p = |x: f32, y: f32| (ox + x * s, oy + y * s);
    // The faint disc it sits on.
    let (cx, cy) = p(24.0, 24.0);
    scene.push(Item::Path {
        path: PathData::circle(cx, cy, 24.0 * s),
        fill: Some(Paint::solid(
            theme.text,
            if theme.dark { 0.045 } else { 0.05 },
        )),
        stroke: None,
    });
    let ink = theme.muted;
    let stroke = |w: f32| Stroke {
        width: w * s,
        paint: Paint::solid(ink, 1.0),
        cap: Cap::Round,
        join: Join::Round,
        dash: None,
        hairline: false,
    };
    let line = |pts: &[(f32, f32)]| {
        let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        PathData::polyline(&pts)
    };
    let mut path = |verbs: Vec<Verb>, st: Stroke, scene: &mut Scene| {
        scene.push(Item::Path {
            path: PathData { verbs },
            fill: None,
            stroke: Some(st),
        });
    };
    let mv = |x, y| {
        let (a, b) = p(x, y);
        Verb::Move(a, b)
    };
    let ln = |x, y| {
        let (a, b) = p(x, y);
        Verb::Line(a, b)
    };
    let qd = |x1, y1, x, y| {
        let (a, b) = p(x1, y1);
        let (c, d) = p(x, y);
        Verb::Quad(a, b, c, d)
    };
    let cu = |x1, y1, x2, y2, x, y| {
        let (a, b) = p(x1, y1);
        let (c, d) = p(x2, y2);
        let (e, f) = p(x, y);
        Verb::Cubic(a, b, c, d, e, f)
    };
    let width = 1.9;
    // A document with its corner folded: the text that couldn't be had, or doesn't parse.
    let document = |scene: &mut Scene, path: &mut dyn FnMut(Vec<Verb>, Stroke, &mut Scene)| {
        path(
            vec![
                mv(14.5, 12.0),
                qd(14.5, 9.5, 17.0, 9.5),
                ln(27.5, 9.5),
                ln(34.5, 16.5),
                ln(34.5, 36.0),
                qd(34.5, 38.5, 32.0, 38.5),
                ln(17.0, 38.5),
                qd(14.5, 38.5, 14.5, 36.0),
                Verb::Close,
            ],
            stroke(width),
            scene,
        );
        path(
            vec![
                mv(27.5, 9.5),
                ln(27.5, 14.5),
                qd(27.5, 16.5, 29.5, 16.5),
                ln(34.5, 16.5),
            ],
            stroke(width),
            scene,
        );
    };
    match kind {
        ErrorKind::Unreadable => {
            document(scene, &mut path);
            // A question mark in it.
            path(
                vec![
                    mv(21.2, 22.2),
                    cu(21.2, 19.9, 22.6, 18.7, 24.6, 18.7),
                    cu(26.7, 18.7, 28.0, 20.0, 28.0, 21.8),
                    cu(28.0, 23.9, 24.6, 24.4, 24.6, 27.2),
                ],
                stroke(width),
                scene,
            );
            let (dx, dy) = p(24.6, 31.4);
            scene.push(Item::Path {
                path: PathData::circle(dx, dy, 1.25 * s),
                fill: Some(Paint::solid(ink, 1.0)),
                stroke: None,
            });
        }
        ErrorKind::Malformed => {
            document(scene, &mut path);
            // Two lines of text and a third squiggled: an error underline.
            scene.push(Item::Path {
                path: line(&[(19.0, 21.0), (27.0, 21.0)]),
                fill: None,
                stroke: Some(stroke(width)),
            });
            scene.push(Item::Path {
                path: line(&[(19.0, 26.5), (30.0, 26.5)]),
                fill: None,
                stroke: Some(stroke(width)),
            });
            path(
                vec![
                    mv(19.0, 32.5),
                    qd(20.4, 30.3, 21.8, 32.5),
                    qd(23.2, 34.7, 24.6, 32.5),
                    qd(26.0, 30.3, 27.4, 32.5),
                    qd(28.8, 34.7, 30.2, 32.5),
                ],
                stroke(width),
                scene,
            );
        }
        ErrorKind::Mismatch => {
            // Two jigsaw pieces that don't fit: the left one's round tab, low, against the
            // right one's round socket, high, a little apart. Centred on the disc, and kept as
            // far in from its edge as the other glyphs are.
            path(
                vec![
                    mv(11.7, 21.45),
                    ln(19.9, 21.45),
                    qd(21.54, 21.45, 21.54, 23.09),
                    ln(21.54, 24.73),
                    cu(24.82, 24.73, 24.82, 29.65, 21.54, 29.65),
                    ln(21.54, 31.29),
                    qd(21.54, 32.93, 19.9, 32.93),
                    ln(11.7, 32.93),
                    qd(10.06, 32.93, 10.06, 31.29),
                    ln(10.06, 23.09),
                    qd(10.06, 21.45, 11.7, 21.45),
                    Verb::Close,
                ],
                stroke(width),
                scene,
            );
            path(
                vec![
                    mv(28.1, 14.07),
                    ln(36.3, 14.07),
                    qd(37.94, 14.07, 37.94, 15.71),
                    ln(37.94, 23.91),
                    qd(37.94, 25.55, 36.3, 25.55),
                    ln(28.1, 25.55),
                    qd(26.46, 25.55, 26.46, 23.91),
                    ln(26.46, 22.27),
                    cu(29.74, 22.27, 29.74, 17.35, 26.46, 17.35),
                    ln(26.46, 15.71),
                    qd(26.46, 14.07, 28.1, 14.07),
                    Verb::Close,
                ],
                stroke(width),
                scene,
            );
        }
        ErrorKind::Empty => {
            // Axes, and bars that aren't there: faint outlines where they'd stand.
            scene.push(Item::Path {
                path: line(&[(11.5, 11.0), (11.5, 36.5), (37.5, 36.5)]),
                fill: None,
                stroke: Some(stroke(width)),
            });
            let ghost = Stroke {
                paint: Paint::solid(ink, 0.45),
                ..stroke(1.6)
            };
            for (x, top) in [(15.5, 25.0), (23.0, 17.0), (30.5, 28.0)] {
                let (a, b) = p(x, top);
                let (c, d) = p(x + 5.0, 33.5);
                let r = 1.2 * s;
                scene.push(Item::Path {
                    path: PathData {
                        verbs: vec![
                            Verb::Move(a, d),
                            Verb::Line(a, b + r),
                            Verb::Quad(a, b, a + r, b),
                            Verb::Line(c - r, b),
                            Verb::Quad(c, b, c, b + r),
                            Verb::Line(c, d),
                        ],
                    },
                    fill: None,
                    stroke: Some(ghost.clone()),
                });
            }
        }
    }
}

// ------------------------------------------------------------------ the stale badge

/// What the badge says: `Error · line 14`, `Error · latency.csv, line 6`, the file alone, or
/// the word alone; `Empty` for nothing to draw.
pub fn badge_text(first: &FigError) -> String {
    let word = if first.kind.critical() {
        "Error"
    } else {
        "Empty"
    };
    let line = first.at.map(|at| format!("line {}", at.line));
    match (first.file_name(), line) {
        (Some(f), Some(l)) => format!("{word} · {f}, {l}"),
        (Some(f), None) => format!("{word} · {f}"),
        (None, Some(l)) => format!("{word} · {l}"),
        (None, None) => word.to_string(),
    }
}

/// What the stale badge says, from all of it down to the icon alone (an empty string): the
/// file and line, the line alone, the word alone.
fn badge_texts(first: &FigError) -> Vec<String> {
    let full = badge_text(first);
    let word = full.split(" · ").next().unwrap_or("").to_string();
    let mut out = vec![full.clone()];
    if let (Some(_), Some(at)) = (first.file_name(), first.at) {
        out.push(format!("{word} · line {}", at.line));
    }
    out.push(word);
    out.push(String::new());
    out.dedup();
    out
}

/// What of a chart the badge must leave readable.
#[derive(Clone, Copy, PartialEq)]
enum Obstacle {
    /// A text, the scene's item `usize`.
    Text(usize),
    /// A legend key.
    Key,
    /// The title's region.
    Title,
}

/// Where a chart has drawn what the badge must leave readable: every text's box, the legend's
/// keys and the title's region.
fn badge_obstacles(scene: &Scene, m: &mut dyn Measure) -> Vec<(Rect, Obstacle)> {
    let mut out = Vec::new();
    for (i, item) in scene.items.iter().enumerate() {
        if let Item::Text {
            text,
            x,
            y,
            spec,
            anchor,
            max_width,
            ..
        } = item
        {
            if text.trim().is_empty() {
                continue;
            }
            let w = m.width(text, *spec).min(max_width.unwrap_or(f32::INFINITY));
            let left = match anchor {
                Anchor::Start => *x,
                Anchor::Middle => x - w / 2.0,
                Anchor::End => x - w,
            };
            let top = y - m.ascent(*spec);
            let r = Rect::new(left, top, w, m.ascent(*spec) + m.descent(*spec));
            out.push((r, Obstacle::Text(i)));
        }
    }
    for r in &scene.regions {
        match r.hit {
            Hit::LegendKey(_) => out.push((r.rect, Obstacle::Key)),
            Hit::PanelTitle(_) => out.push((r.rect, Obstacle::Title)),
            _ => {}
        }
    }
    out
}

/// The least of a title the badge leaves when it cuts it to make room.
const TITLE_KEPT: f32 = 64.0;

/// The stale badge in the top-right corner of `scene`: the status icon and what
/// [`badge_text`] says, on the card's ground, with its region. It never covers what the chart
/// says: where its words would meet the title, the legend or a label on its row (8 px kept
/// between), it says less (the line without the file, the word, the icon alone); where even
/// the icon would, the title is cut back with its ellipsis to make room (keeping some of it);
/// else the icon goes into the plot's top-right corner, clear of the chart's words there; and
/// only where that fails too, it sits in the corner over whatever's there.
pub fn stale_badge(scene: &mut Scene, first: &FigError, theme: &Theme, m: &mut dyn Measure) {
    let w = scene.width;
    let spec = TextSpec::new(12.0);
    let h = 24.0;
    // On the chart's own lines: its right edge where the end labels end, its middle on the
    // first row's, the title's or the legend's.
    let (pad, pad_top, _) = super::layout::paddings(w, scene.height);
    let inset = pad.min(w / 10.0);
    let title = TextSpec::new(super::layout::TITLE).weight(Weight::Semibold);
    let mid = pad_top + m.ascent(title) - m.cap(title) / 2.0;
    let top = (mid - h / 2.0).max(4.0).round();
    let icon = 12.0;
    let room = (w - 2.0 * inset).max(0.0);
    let pad = (10.0, 12.0);
    let width = |t: &str, m: &mut dyn Measure| {
        if t.is_empty() {
            h
        } else {
            pad.0 + icon + 6.0 + m.width(t, spec) + pad.1
        }
    };
    let most = room.min((w * 0.6).max(160.0));
    let obstacles = badge_obstacles(scene, m);
    // Clear of everything, but the title's text `but` and its region when it's to be cut.
    let clear = |r: &Rect, but: Option<usize>| {
        let near = Rect::new(r.x - 8.0, r.y, r.w + 16.0, r.h);
        obstacles.iter().all(|(o, what)| {
            let cut = match what {
                Obstacle::Text(i) => but == Some(*i),
                Obstacle::Title => but.is_some(),
                Obstacle::Key => false,
            };
            cut || near.intersect(o).is_none_or(|i| i.w <= 0.01 || i.h <= 0.01)
        })
    };
    let plot = scene
        .regions
        .iter()
        .find(|r| matches!(r.hit, Hit::Plot(_)))
        .map(|r| r.rect);
    let texts = badge_texts(first);
    let fit = |right: f32, y: f32, texts: &[String], but: Option<usize>, m: &mut dyn Measure| {
        texts.iter().find_map(|t| {
            let bw = width(t, m);
            if bw > most && !t.is_empty() {
                return None;
            }
            let r = Rect::new((right - bw).round(), y.round(), bw, h);
            (r.x >= inset.min(4.0) && clear(&r, but)).then(|| (r, t.clone()))
        })
    };
    // The top-right corner, clear of everything.
    let mut placed = fit(w - inset, top, &texts, None, m);
    // The title cut back to make room for a word or the icon.
    if placed.is_none() {
        let row = Rect::new(0.0, top, w, h);
        let title_item = obstacles.iter().find_map(|(o, what)| {
            let Obstacle::Text(i) = *what else {
                return None;
            };
            let start = matches!(
                scene.items[i],
                Item::Text {
                    anchor: Anchor::Start,
                    ..
                }
            );
            let titled = scene.regions.iter().any(|r| {
                matches!(r.hit, Hit::PanelTitle(_)) && r.rect.contains(o.x + 1.0, o.y + o.h / 2.0)
            });
            (start && titled && row.intersect(o).is_some()).then_some((i, *o))
        });
        if let Some((i, o)) = title_item {
            // The word where the title keeps a good part of itself, else the icon.
            let tries = texts.iter().rev().take(2).rev();
            let cut = tries.filter_map(|t| {
                let (r, t) = fit(w - inset, top, std::slice::from_ref(t), Some(i), m)?;
                let keep = if t.is_empty() {
                    TITLE_KEPT
                } else {
                    2.0 * TITLE_KEPT
                };
                (r.x - 8.0 - o.x >= keep.min(o.w)).then_some((r, t))
            });
            if let Some((r, t)) = cut.into_iter().next() {
                if let Item::Text {
                    text,
                    spec,
                    max_width,
                    ..
                } = &mut scene.items[i]
                {
                    let room = r.x - 8.0 - o.x;
                    *text = fit_words(text, *spec, room, m).into();
                    *max_width = Some(room);
                }
                placed = Some((r, t));
            }
        }
    }
    // Into the plot's top-right corner, over the data: as small as it goes, the icon.
    if placed.is_none()
        && let Some(p) = plot
    {
        placed = fit(p.right() - 8.0, p.y + 8.0, &[String::new()], None, m);
    }
    let (r, text) = placed.unwrap_or_else(|| {
        let bw = h.min(room.max(h));
        (
            Rect::new((w - inset - bw).round(), top, bw, h),
            String::new(),
        )
    });
    scene.push(Item::Shadow {
        rect: r,
        radius: h / 2.0,
        blur: 6.0,
        dy: 1.0,
        alpha: if theme.dark { 0.45 } else { 0.12 },
    });
    scene.push(Item::Path {
        path: PathData::rounded(r, [h / 2.0; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    let ic = if text.is_empty() {
        (r.x + r.w / 2.0, r.y + h / 2.0)
    } else {
        (r.x + pad.0 + icon / 2.0, r.y + h / 2.0)
    };
    status_icon(scene, ic, icon / 2.0, first.kind.critical(), theme);
    if !text.is_empty() {
        let base = r.y + h / 2.0 + m.cap(spec) / 2.0;
        scene.push(text_item(
            &text,
            r.x + pad.0 + icon + 6.0,
            base,
            spec,
            Anchor::Start,
            theme.text,
        ));
    }
    scene.region(r, Hit::Badge);
}

// ------------------------------------------------------------------ the log

/// One entry of the log as lines: what each says and how it's inked.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Its lines, each a run of spans.
    pub lines: Vec<Vec<Span>>,
    /// Which of them is the source line, the caret's under it.
    pub source: Option<usize>,
    /// The kind's colour role: critical or a warning's.
    pub critical: bool,
}

/// A run of a log line, in one ink.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// What it says.
    pub text: String,
    /// Its ink.
    pub ink: LogInk,
    /// Set heavier.
    pub bold: bool,
}

/// The log's inks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogInk {
    /// The status colour of the entry: the kind's word and the caret.
    Status,
    /// The message.
    Strong,
    /// The source line, the place, what was tried.
    Text,
    /// The gutter: `-->`, `|`, line numbers, `=`.
    Muted,
    /// What was tried: text, a path in it cut from the left where it's too long, so it keeps its file.
    Note,
}

/// Where the log's lines start, each entry's first and how many it has, and how many there are.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LogLayout {
    /// Each entry's first line.
    pub starts: Vec<usize>,
    /// Each entry's lines, the blank one after it not counted.
    pub lens: Vec<usize>,
    /// Every line, the blanks between entries counted.
    pub total: usize,
}

impl LogLayout {
    /// The entry on a line, if any (a blank between two is neither's).
    pub fn entry_at(&self, line: usize) -> Option<usize> {
        let i = self.starts.partition_point(|&s| s <= line).checked_sub(1)?;
        (line < self.starts[i] + self.lens[i]).then_some(i)
    }

    /// The highest first line a page of `page` lines may show.
    pub fn max_top(&self, page: usize) -> usize {
        self.total.saturating_sub(page.max(1))
    }

    /// `top` moved the least so that entry `mark` is on the page (its first line at least, and
    /// all of it where it fits).
    pub fn reveal(&self, top: usize, mark: usize, page: usize) -> usize {
        let (Some(&s), Some(&n)) = (self.starts.get(mark), self.lens.get(mark)) else {
            return top.min(self.max_top(page));
        };
        let page = page.max(1);
        let top = if s < top {
            s
        } else if s + n > top + page {
            (s + n).saturating_sub(page).min(s)
        } else {
            top
        };
        top.min(self.max_top(page))
    }
}

/// How many lines of the log a page of this size shows.
pub fn log_page(size: (f32, f32)) -> usize {
    ((size.1 - LOG_HEADER - LOG_FOOT) / LOG_LINE)
        .floor()
        .max(1.0) as usize
}

/// The log's entries as lines (rustc's layout), and where each starts.
pub fn log_entries(errors: &[FigError], source: &str, name: &str) -> (Vec<Entry>, LogLayout) {
    // One gutter down the whole log, as wide as its longest line number.
    let digits = errors
        .iter()
        .filter_map(|e| e.at)
        .map(|at| at.line.max(1).to_string().len())
        .max()
        .unwrap_or(1);
    let entries: Vec<Entry> = errors
        .iter()
        .map(|e| entry(e, source, name, digits))
        .collect();
    let mut layout = LogLayout::default();
    let mut at = 0;
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            at += 1;
        }
        layout.starts.push(at);
        layout.lens.push(e.lines.len());
        at += e.lines.len();
    }
    layout.total = at;
    (entries, layout)
}

/// The log's layout alone, for the keys and the wheel that scroll it: the same lines the log
/// draws, so `name` is the figure's, as [`error_figure`] is given it.
pub fn log_layout(errors: &[FigError], source: &str, name: &str) -> LogLayout {
    log_entries(errors, source, name).1
}

fn span(text: impl Into<String>, ink: LogInk, bold: bool) -> Span {
    Span {
        text: text.into(),
        ink,
        bold,
    }
}

/// Tabs as four spaces, and the column `col` (in chars, from 1) where it is after.
fn untab(line: &str, col: usize) -> (String, usize) {
    let mut out = String::with_capacity(line.len());
    let mut at = col;
    for (i, c) in line.chars().enumerate() {
        if c == '\t' {
            out.push_str("    ");
            if i + 1 < col {
                at += 3;
            }
        } else if !c.is_control() {
            out.push(c);
        } else {
            out.push(' ');
        }
    }
    (out, at)
}

/// How many chars the caret underlines from column `col` (from 1) of `line`: a quoted string
/// to its closing quote, a word or a number to its end, else one.
fn caret_len(line: &str, col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let Some(&c) = chars.get(col.saturating_sub(1)) else {
        return 1;
    };
    let rest = &chars[col.saturating_sub(1)..];
    let n = if c == '"' {
        rest.iter()
            .skip(1)
            .position(|&d| d == '"')
            .map_or(1, |i| i + 2)
    } else if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
        rest.iter()
            .position(|d| !(d.is_alphanumeric() || matches!(d, '_' | '-' | '.' | ':')))
            .unwrap_or(rest.len())
    } else {
        1
    };
    n.clamp(1, 60)
}

/// One error as rustc writes one: `kind: message`, `--> file:line:col`, the source line with
/// a caret under the column, and `= what was tried`.
fn entry(e: &FigError, source: &str, name: &str, digits: usize) -> Entry {
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let word = if e.warning { "warning" } else { e.kind.name() };
    // `empty: the data has no rows`, not `empty: nothing to draw: ...`.
    let msg = match e.kind {
        ErrorKind::Empty => e.msg.strip_prefix("nothing to draw: ").unwrap_or(&e.msg),
        _ => &e.msg,
    };
    lines.push(vec![
        span(word, LogInk::Status, true),
        span(format!(": {msg}"), LogInk::Strong, true),
    ]);
    let file = e
        .file_name()
        .or_else(|| (!name.is_empty()).then(|| name.to_string()));
    let text = match (&e.line, e.at) {
        (Some(l), Some(_)) => Some(l.clone()),
        (None, Some(at)) if e.file.is_none() => source
            .split('\n')
            .nth(at.line.saturating_sub(1) as usize)
            .map(|l| l.trim_end_matches('\r').to_string()),
        _ => None,
    };
    let pad = " ".repeat(digits);
    if let Some(f) = &file {
        let place = match e.at {
            Some(at) => format!("{f}:{}:{}", at.line, at.col),
            None => f.clone(),
        };
        lines.push(vec![
            span(format!("{pad}--> "), LogInk::Muted, false),
            span(place, LogInk::Text, false),
        ]);
    }
    let mut src_line = None;
    if let (Some(t), Some(at)) = (text, e.at) {
        let (shown, col) = untab(&t, at.col as usize);
        src_line = Some(lines.len() + 1);
        let n = caret_len(&shown, col);
        lines.push(vec![span(format!("{pad} |"), LogInk::Muted, false)]);
        lines.push(vec![
            span(format!("{:>digits$} | ", at.line), LogInk::Muted, false),
            span(shown, LogInk::Text, false),
        ]);
        lines.push(vec![
            span(format!("{pad} | "), LogInk::Muted, false),
            span(" ".repeat(col.saturating_sub(1)), LogInk::Text, false),
            span("^".repeat(n), LogInk::Status, true),
        ]);
    }
    for c in &e.context {
        lines.push(vec![
            span(format!("{pad} = "), LogInk::Muted, false),
            span(c.clone(), LogInk::Note, false),
        ]);
    }
    Entry {
        lines,
        source: src_line,
        critical: !e.warning && e.kind.critical(),
    }
}

/// The log over the whole area: its header (how many errors and warnings, the keys), then its
/// lines from `top`, the entry `mark` lit, a scrollbar where they don't all fit.
#[allow(clippy::too_many_arguments)]
fn log(
    scene: &mut Scene,
    errors: &[FigError],
    source: &str,
    name: &str,
    top: usize,
    mark: usize,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let (w, h) = (scene.width, scene.height);
    scene.region(Rect::new(0.0, 0.0, w, h), Hit::Chrome);
    let (entries, layout) = log_entries(errors, source, name);
    let page = log_page((w, h));
    let top = top.min(layout.max_top(page));
    let pad = LOG_PAD.min(w / 12.0).round();
    // The header: the counts, and the keys at the right.
    let head = Rect::new(0.0, 0.0, w, LOG_HEADER);
    let n_err = errors.iter().filter(|e| !e.warning).count();
    let n_warn = errors.len() - n_err;
    let empty = n_err > 0
        && (errors.iter())
            .filter(|e| !e.warning)
            .all(|e| e.kind == ErrorKind::Empty);
    let count = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let mut said = Vec::new();
    // Nothing to draw isn't an error, as the figure's own status says.
    if empty {
        said.push("Nothing to draw".to_string());
    } else if n_err > 0 || n_warn == 0 {
        said.push(count(n_err, "error", "errors"));
    }
    if n_warn > 0 {
        said.push(count(n_warn, "warning", "warnings"));
    }
    let head_spec = TextSpec::new(12.0).weight(Weight::Semibold);
    let base = LOG_HEADER / 2.0 + m.cap(head_spec) / 2.0;
    let counts = said.join(" · ");
    let all_keys: [(bool, String); 6] = [
        (true, "!".into()),
        (false, "figure".into()),
        (true, "⏎".into()),
        (false, "go to".into()),
        (true, "j k".into()),
        (false, "move".into()),
    ];
    let cw = m.width(&counts, head_spec);
    let in_name = if name.is_empty() {
        String::new()
    } else {
        format!(" in {name}")
    };
    let ic = 12.0;
    // The keys give way from the right (move, then go to) before the figure's name is cut;
    // a name too long to fit whole keeps every key that fits.
    let name_w = m.width(&in_name, TextSpec::new(12.0));
    let room_with = |n: usize, m: &mut dyn Measure| {
        let keys_w = match n {
            0 => 0.0,
            n => 24.0 + hint_width(&all_keys[..2 * n], m),
        };
        w - 2.0 * pad - cw - keys_w
    };
    let shown = (0..=3)
        .rev()
        .find(|&n| room_with(n, m) - ic - 8.0 >= name_w)
        .or_else(|| (1..=3).rev().find(|&n| room_with(n, m) >= 0.0))
        .unwrap_or(0);
    let keys = &all_keys[..2 * shown];
    let keys_w = hint_width(keys, m);
    let show_keys = shown > 0;
    let name_room = room_with(shown, m);
    let first = lead(errors);
    let mut x = pad;
    if let Some(f) = first
        && pad + ic + 8.0 + cw <= w
    {
        status_icon(
            scene,
            (x + ic / 2.0, LOG_HEADER / 2.0),
            ic / 2.0,
            f.kind.critical() && !f.warning,
            theme,
        );
        x += ic + 8.0;
    }
    scene.push(text_item(
        &counts,
        x,
        base,
        head_spec,
        Anchor::Start,
        theme.strong,
    ));
    let named = fit_words(
        &in_name,
        TextSpec::new(12.0),
        (name_room - ic - 8.0).max(0.0),
        m,
    );
    // A name cut to a few letters and its dots says nothing: none, then.
    let named = if named.chars().count() < " in ".len() + 6 + 1 && named != in_name {
        String::new()
    } else {
        named
    };
    scene.push(text_item(
        &named,
        x + cw,
        base,
        TextSpec::new(12.0),
        Anchor::Start,
        theme.muted,
    ));
    if show_keys {
        draw_hint(
            scene,
            keys,
            w - pad - keys_w,
            (LOG_HEADER - HINT_H) / 2.0,
            theme,
            m,
        );
    }
    scene.push(Item::Path {
        path: PathData::polyline(&[(0.0, LOG_HEADER), (w, LOG_HEADER)]),
        fill: None,
        stroke: Some(Stroke::hairline(theme.grid, 1.0)),
    });
    scene.region(head, Hit::ErrorFigure);
    // The lines.
    let body = Rect::new(0.0, LOG_HEADER + 1.0, w, (h - LOG_HEADER - 1.0).max(0.0));
    let mono = TextSpec::new(LOG_TEXT).mono();
    let bold = mono.weight(Weight::Semibold);
    let cell = m.width("0", mono).max(1.0);
    let text_x = pad + 4.0;
    let right = w - pad - 6.0;
    let chars = ((right - text_x) / cell).floor().max(4.0) as usize;
    let y0 = LOG_HEADER + 8.0;
    let line_y = |line: usize| y0 + (line as f32 - top as f32) * LOG_LINE;
    scene.push(Item::PushClip(body));
    for (i, e) in entries.iter().enumerate() {
        let (s, n) = (layout.starts[i], layout.lens[i]);
        if s + n <= top || s >= top + page {
            continue;
        }
        let r = Rect::new(
            pad / 2.0,
            line_y(s) - 4.0,
            w - pad,
            n as f32 * LOG_LINE + 8.0,
        );
        let status = if e.critical {
            theme.status[3]
        } else {
            theme.status[1]
        };
        let status_ink = readable(status, theme);
        if i == mark {
            scene.push(Item::Path {
                path: PathData::rounded(r, [6.0; 4]),
                fill: Some(Paint::solid(
                    theme.text,
                    if theme.dark { 0.05 } else { 0.045 },
                )),
                stroke: None,
            });
            let bar = Rect::new(r.x, r.y + 4.0, 2.0, r.h - 8.0);
            scene.push(Item::Path {
                path: PathData::rounded(bar, [1.0; 4]),
                fill: Some(Paint::solid(status, 0.9)),
                stroke: None,
            });
        }
        if let Some(clipped) = r.intersect(&body) {
            scene.region(clipped, Hit::LogEntry(i as u32));
        }
        let lines = keep_caret(e, chars);
        for (k, spans) in lines.iter().enumerate() {
            let line = s + k;
            if line < top || line >= top + page {
                continue;
            }
            let base = line_y(line) + (LOG_LINE + m.ascent(mono) - m.descent(mono)) / 2.0;
            let mut col = 0usize;
            let spans = clip_line(spans, chars);
            for sp in &spans {
                if sp.text.is_empty() {
                    continue;
                }
                let color = match sp.ink {
                    LogInk::Status => status_ink,
                    LogInk::Strong => theme.strong,
                    LogInk::Text | LogInk::Note => theme.text,
                    LogInk::Muted => theme.muted,
                };
                let spec = if sp.bold { bold } else { mono };
                if !sp.text.trim().is_empty() {
                    scene.push(text_item(
                        &sp.text,
                        text_x + col as f32 * cell,
                        base,
                        spec,
                        Anchor::Start,
                        color,
                    ));
                }
                col += sp.text.chars().count();
            }
        }
    }
    scene.push(Item::PopClip);
    // A scrollbar where the lines don't all fit.
    if layout.total > page {
        let track = Rect::new(w - 7.0, body.y + 6.0, 3.0, (body.h - 12.0).max(0.0));
        let frac = page as f32 / layout.total as f32;
        let th = (track.h * frac).max(18.0).min(track.h);
        let at = top as f32 / layout.max_top(page).max(1) as f32;
        let thumb = Rect::new(track.x, track.y + (track.h - th) * at, track.w, th);
        scene.push(Item::Path {
            path: PathData::rounded(thumb, [1.5; 4]),
            fill: Some(Paint::solid(theme.muted, 0.55)),
            stroke: None,
        });
    }
}

/// How many cells a caret moved into view keeps from the right edge.
const CARET_MARGIN: usize = 4;

/// An entry's lines with its source line and caret moved left together where the caret would
/// be past `chars` cells: the source starts further in, after a '…', by the least that brings
/// the caret and [`CARET_MARGIN`] cells into view, so as much of the line as can be stays.
fn keep_caret(e: &Entry, chars: usize) -> Vec<Vec<Span>> {
    let mut lines = e.lines.clone();
    let Some(si) = e.source else {
        return lines;
    };
    let (Some(src), Some(car)) = (lines.get(si), lines.get(si + 1)) else {
        return lines;
    };
    let gutter = src.first().map_or(0, |s| s.text.chars().count());
    let c0 = car.get(1).map_or(0, |s| s.text.chars().count());
    let n = car.get(2).map_or(1, |s| s.text.chars().count());
    let room = chars.saturating_sub(gutter + 1);
    if c0 + n <= room || room < 8 {
        return lines;
    }
    // The least shift that shows the caret's run and the margin; a run longer than the room
    // starts two cells in, after the '…', its end cut.
    let shift = (c0 + n + CARET_MARGIN)
        .saturating_sub(room)
        .min(c0.saturating_sub(2));
    if shift == 0 {
        return lines;
    }
    if let Some(t) = lines[si].get_mut(1) {
        let rest: String = t.text.chars().skip(shift + 1).collect();
        t.text = format!("…{rest}");
    }
    if let Some(sp) = lines[si + 1].get_mut(1) {
        sp.text = " ".repeat(c0 - shift);
    }
    lines
}

/// `note` in `room` chars by cutting its path's leading directories to '…/', its file kept:
/// None if it has no path, or not even its file and the rest fit.
fn shorten_path(note: &str, room: usize) -> Option<String> {
    let words: Vec<&str> = note.split(' ').collect();
    let (i, path) = words
        .iter()
        .enumerate()
        .filter(|(_, w)| w.contains('/'))
        .max_by_key(|(_, w)| w.chars().count())?;
    let n = note.chars().count();
    let over = n.checked_sub(room)?;
    let file = path.rsplit('/').next().unwrap_or(path);
    let keep = path.chars().count().checked_sub(over + 2)?;
    if keep < file.chars().count() {
        return None;
    }
    let tail: String = path.chars().skip(path.chars().count() - keep).collect();
    let mut words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    words[i] = format!("…/{}", tail.trim_start_matches('/'));
    Some(words.join(" "))
}

/// A line's spans cut to `chars` cells: the ends of what doesn't fit become '…'. A caret line
/// and the source line above it are cut alike, so the caret stays under its column: where the
/// caret is past the width, both start further in.
fn clip_line(spans: &[Span], chars: usize) -> Vec<Span> {
    let len: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if len <= chars {
        return spans.to_vec();
    }
    // A note loses a path's leading directories first, so its file stays; else its end.
    if let Some((last, before)) = spans.split_last()
        && last.ink == LogInk::Note
    {
        let used: usize = before.iter().map(|s| s.text.chars().count()).sum();
        let room = chars.saturating_sub(used);
        if let Some(text) = shorten_path(&last.text, room) {
            let mut out = before.to_vec();
            out.push(Span {
                text,
                ..last.clone()
            });
            return out;
        }
    }
    let mut out = Vec::new();
    let mut left = chars.saturating_sub(1);
    for s in spans {
        if left == 0 {
            break;
        }
        let n = s.text.chars().count();
        if n <= left {
            out.push(s.clone());
            left -= n;
        } else {
            out.push(Span {
                text: s.text.chars().take(left).collect(),
                ..s.clone()
            });
            left = 0;
        }
    }
    // Nothing dangling before the '…'.
    while let Some(last) = out.last_mut() {
        let kept = last.text.trim_end_matches(dangling).len();
        last.text.truncate(kept);
        if !last.text.is_empty() {
            break;
        }
        out.pop();
    }
    out.push(span("…", LogInk::Muted, false));
    out
}
