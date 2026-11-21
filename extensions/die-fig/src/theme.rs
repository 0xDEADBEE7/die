//! A figure's colours: the faces die-fig declares, and the [`Theme`] they resolve to.
//!
//! Chrome comes from the theme and stays quiet; the data is the only loud thing. Each face's
//! colour is its foreground, whatever it paints (the grid's lines, the card's ground, the
//! cursor's chip), as the whiteboard's dial faces are. A face the theme leaves unstyled takes the
//! defaults below: the dark set when `default`'s background is dark (its Rec. 601 luma under
//! 0.5, see [`is_dark`]), else the light. The series and status faces are declared unstyled so the validated sets apply, and
//! a theme that styles `fig-series-N` wins.
//!
//! | Face | Field | Dark default | Light default |
//! |---|---|---|---|
//! | `fig-series-1` … `fig-series-8` | `series` | `#3987e5` … | `#2a78d6` … |
//! | `fig-good` … `fig-critical` | `status` | `#0ca30c #fab219 #ec835a #d03b3b` | the same |
//! | `fig-text` (on `default`) | `text` | `#cccccc` | `#52514e` |
//! | `fig-strong` | `strong` | `#eeeeee` | `#0b0b0b` |
//! | `fig-tick` (on `shadow`) | `muted` | `#888888` | `#898781` |
//! | `fig-grid` | `grid` | `#2a2a2a` | `#e1e0d9` |
//! | `fig-baseline` | `baseline` | `#3a3a3a` | `#c3c2b7` |
//! | `fig-tooltip` | `tooltip` | `#252525` | `#ffffff` |
//! | `fig-ring` | `ring` | `#3a3a3a` | `#e1e0d9` |
//! | `fig-cursor` | `chip` | `#eeeeee` | `#0b0b0b` |
//! | `fig-dial` (on `shadow`) | `dial` | `#888888` | `#898781` |
//! | `fig-dial-accent` | `dial_accent` | `#cc4444` | `#cc4444` |
//!
//! `surface` is `default`'s background, `chip_text` the surface, and `accent` and `other` have
//! no face of their own.

use crate::style::ColorSpec;
use die_core::{Registry, Style};

/// A colour as red, green and blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Color(pub u8, pub u8, pub u8);

/// die-core's colour as die-fig's.
pub fn core_rgb(c: die_core::Rgb) -> Color {
    Color(c.0, c.1, c.2)
}

/// die-fig's colour as die-draw's ink.
pub fn draw_rgb(c: Color) -> die_draw::Rgb {
    die_draw::Rgb(c.0, c.1, c.2)
}

/// Every colour a figure is drawn in, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Whether the surface is dark, which picks the dark defaults.
    pub dark: bool,
    /// The ground everything is drawn on: `default`'s background.
    pub surface: Color,
    /// Labels, ticks' text, the legend's names.
    pub text: Color,
    /// Titles and the card's values.
    pub strong: Color,
    /// Tick labels and secondary text.
    pub muted: Color,
    /// Gridlines.
    pub grid: Color,
    /// The zero line and the axes' lines.
    pub baseline: Color,
    /// The card's ground.
    pub tooltip: Color,
    /// The ring around a hovered point, and the card's border.
    pub ring: Color,
    /// The inverted chip that marks the cursor on an axis.
    pub chip: Color,
    /// The chip's text.
    pub chip_text: Color,
    /// The one accent: a focus, a selection's edge.
    pub accent: Color,
    /// The dial's names and line art.
    pub dial: Color,
    /// The dial's hovered sector, chip and needle.
    pub dial_accent: Color,
    /// The eight series slots.
    pub series: [Color; 8],
    /// Good, warning, serious and critical.
    pub status: [Color; 4],
    /// Series 9 and up on unstacked marks: a neutral.
    pub other: Color,
}

/// Every face die-fig declares: the eight series, the four statuses, then the chrome.
pub const FACES: &[&str] = &[
    "fig-series-1",
    "fig-series-2",
    "fig-series-3",
    "fig-series-4",
    "fig-series-5",
    "fig-series-6",
    "fig-series-7",
    "fig-series-8",
    "fig-good",
    "fig-warning",
    "fig-serious",
    "fig-critical",
    "fig-grid",
    "fig-baseline",
    "fig-tick",
    "fig-text",
    "fig-strong",
    "fig-tooltip",
    "fig-ring",
    "fig-cursor",
    "fig-dial",
    "fig-dial-accent",
];

/// The face a face inherits from: `fig-text` from `default`, `fig-tick` and `fig-dial` from
/// `shadow`, the rest from none.
fn parent(face: &str) -> Option<&'static str> {
    match face {
        "fig-text" => Some("default"),
        "fig-tick" | "fig-dial" => Some("shadow"),
        _ => None,
    }
}

/// Declares every face in [`FACES`], unstyled, on its parent.
pub fn register(reg: &mut Registry) {
    for name in FACES {
        reg.face(name, parent(name), Style::new());
    }
}

/// Whether a background is dark: its luma (Rec. 601, on the colour as it's written, 0 to 1)
/// under a half, as Ghostty decides a terminal's background is light or dark. Not relative
/// luminance (linear Rec. 709), which is under a half up to grey `#bbbbbb`: by it the greys
/// `#808080` to `#bbbbbb` would be dark and take the dark set's light text, which reads poorly
/// on them; by luma they're light, and `#7f7f7f` is the lightest dark grey.
pub fn is_dark(c: Color) -> bool {
    let luma = 0.299 * f32::from(c.0) + 0.587 * f32::from(c.1) + 0.114 * f32::from(c.2);
    luma / 255.0 < 0.5
}

/// The faces as the theme has them now, each unstyled one at its default. `default`'s
/// background is the surface and picks the set: dark when it's dark or unset (DIE's themes are
/// dark first), light otherwise.
pub fn resolve(ctx: &die_core::Ctx) -> Theme {
    let fg = |name: &str| ctx.face_style(name).and_then(|s| s.fg).map(core_rgb);
    let bg = ctx.face_style("default").and_then(|s| s.bg).map(core_rgb);
    let dark = bg.is_none_or(is_dark);
    let base = if dark {
        Theme::mono_dark()
    } else {
        Theme::light()
    };
    let surface = bg.unwrap_or(base.surface);
    let or = |name: &str, default: Color| fg(name).unwrap_or(default);
    let mut series = base.series;
    for (i, slot) in series.iter_mut().enumerate() {
        *slot = or(FACES[i], *slot);
    }
    let mut status = base.status;
    for (i, slot) in status.iter_mut().enumerate() {
        *slot = or(FACES[8 + i], *slot);
    }
    Theme {
        dark,
        surface,
        text: or("fig-text", base.text),
        strong: or("fig-strong", base.strong),
        muted: or("fig-tick", base.muted),
        grid: or("fig-grid", base.grid),
        baseline: or("fig-baseline", base.baseline),
        tooltip: or("fig-tooltip", base.tooltip),
        ring: or("fig-ring", base.ring),
        chip: or("fig-cursor", base.chip),
        chip_text: surface,
        accent: base.accent,
        dial: or("fig-dial", base.dial),
        dial_accent: or("fig-dial-accent", base.dial_accent),
        series,
        status,
        other: base.other,
    }
}

/// The colour a spec's colour is in this theme: a slot's series colour, a status's colour,
/// `dim` the neutral that series past the eighth draw in, `bright` the strong text's colour,
/// `accent` the accent, and `#rrggbb` itself.
pub fn color_of(spec: ColorSpec, theme: &Theme) -> Color {
    match spec {
        ColorSpec::Slot(i) => theme.series[usize::from(i) % theme.series.len()],
        ColorSpec::Status(s) => theme.status[s as usize],
        ColorSpec::Dim => theme.other,
        ColorSpec::Bright => theme.strong,
        ColorSpec::Accent => theme.accent,
        ColorSpec::Rgb(r, g, b) => Color(r, g, b),
    }
}

/// A colour from `#rrggbb`, for the tables below.
const fn hex(s: &str) -> Color {
    let c = die_core::Rgb::hex(s);
    Color(c.0, c.1, c.2)
}

/// The status colours, on dark and light alike.
const STATUS: [Color; 4] = [
    hex("#0ca30c"),
    hex("#fab219"),
    hex("#ec835a"),
    hex("#d03b3b"),
];

impl Theme {
    /// The defaults on a dark surface: mono-dark's.
    pub fn mono_dark() -> Theme {
        let surface = hex("#1a1a1a");
        Theme {
            dark: true,
            surface,
            text: hex("#cccccc"),
            strong: hex("#eeeeee"),
            muted: hex("#888888"),
            grid: hex("#2a2a2a"),
            baseline: hex("#3a3a3a"),
            tooltip: hex("#252525"),
            ring: hex("#3a3a3a"),
            chip: hex("#eeeeee"),
            chip_text: surface,
            accent: hex("#cc4444"),
            dial: hex("#888888"),
            dial_accent: hex("#cc4444"),
            series: [
                hex("#3987e5"),
                hex("#d95926"),
                hex("#199e70"),
                hex("#c98500"),
                hex("#d55181"),
                hex("#0ca30c"),
                hex("#9085e9"),
                hex("#e66767"),
            ],
            status: STATUS,
            other: hex("#5a5a5a"),
        }
    }

    /// The defaults on a light surface.
    pub fn light() -> Theme {
        let surface = hex("#fcfcfb");
        let muted = hex("#898781");
        let accent = hex("#cc4444");
        Theme {
            dark: false,
            surface,
            text: hex("#52514e"),
            strong: hex("#0b0b0b"),
            muted,
            grid: hex("#e1e0d9"),
            baseline: hex("#c3c2b7"),
            tooltip: hex("#ffffff"),
            ring: hex("#e1e0d9"),
            chip: hex("#0b0b0b"),
            chip_text: surface,
            accent,
            dial: muted,
            dial_accent: accent,
            series: [
                hex("#2a78d6"),
                hex("#eb6834"),
                hex("#1baf7a"),
                hex("#eda100"),
                hex("#e87ba4"),
                hex("#008300"),
                hex("#6250d6"),
                hex("#e34948"),
            ],
            status: STATUS,
            other: hex("#b5b4ad"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_faces_are_declared_on_their_parents() {
        let mut ctx = die_core::Ctx::new(vec![Box::new(crate::Fig::default())]).unwrap();
        for name in FACES {
            assert!(ctx.face_style(name).is_some(), "{name} is declared");
        }
        let mut t = ctx.theme().clone();
        t.faces.insert(
            "default".into(),
            Style::new().fg(die_core::Rgb::hex("#123456")),
        );
        t.faces.insert(
            "shadow".into(),
            Style::new().fg(die_core::Rgb::hex("#654321")),
        );
        ctx.set_theme(t);
        let fg = |ctx: &die_core::Ctx, name: &str| ctx.face_style(name).unwrap().fg;
        assert_eq!(fg(&ctx, "fig-text"), Some(die_core::Rgb::hex("#123456")));
        assert_eq!(fg(&ctx, "fig-tick"), Some(die_core::Rgb::hex("#654321")));
        assert_eq!(fg(&ctx, "fig-dial"), Some(die_core::Rgb::hex("#654321")));
        assert_eq!(fg(&ctx, "fig-grid"), None);
        assert_eq!(fg(&ctx, "fig-series-1"), None);
        assert_eq!(FACES.len(), 22);
        assert_eq!(crate::faces(), FACES);
    }

    #[test]
    fn the_defaults_are_the_contracts() {
        let (d, l) = (Theme::mono_dark(), Theme::light());
        assert!(d.dark && !l.dark);
        assert_eq!(d.chip_text, d.surface);
        assert_eq!(l.chip_text, l.surface);
        assert_eq!(d.status, l.status);
        assert_eq!(d.series[0], Color(0x39, 0x87, 0xe5));
        assert_eq!(l.series[7], Color(0xe3, 0x49, 0x48));
        assert_eq!(core_rgb(die_core::Rgb(1, 2, 3)), Color(1, 2, 3));
        assert_eq!(draw_rgb(Color(1, 2, 3)), die_draw::Rgb(1, 2, 3));
    }
}
