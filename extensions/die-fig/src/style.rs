//! The data's own look: fig's `fig` block on a series, a node, an edge or a row, read from a
//! [`Node`] into a [`FigStyle`].
//!
//! The block is tolerant, as fig's is: an unknown key, or a known one with a value of the wrong
//! kind, is ignored without a word. Its keys are kebab-case (`override-focus`, `show-label`),
//! and a snake_case spelling is just an unknown key. A colour is a name or `#rrggbb`
//! ([`ColorSpec::parse`]); the names fig had (`red`, `green`, `blue`, `yellow`, `dim`,
//! `bright`) keep their hues by taking the series slots of the same names.

use crate::json::Node;

/// How things stand, for a value that has a good side: the status colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    /// All is well.
    Good,
    /// Worth a look.
    Warning,
    /// Needs attention.
    Serious,
    /// Needs it now.
    Critical,
}

impl Status {
    /// Every status, best first.
    pub const ALL: [Status; 4] = [
        Status::Good,
        Status::Warning,
        Status::Serious,
        Status::Critical,
    ];

    /// Its name in a spec: `good`, `warning`, `serious` or `critical`.
    pub fn name(self) -> &'static str {
        match self {
            Status::Good => "good",
            Status::Warning => "warning",
            Status::Serious => "serious",
            Status::Critical => "critical",
        }
    }
}

/// The names of the eight series slots, slot 0 first: a series' colour by name.
pub const SLOT_NAMES: [&str; 8] = [
    "blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red",
];

/// A colour as a spec gives it: one the theme resolves, or one exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpec {
    /// A series slot, 0 to 7: blue, orange, aqua, yellow, magenta, green, violet, red.
    Slot(u8),
    /// A status colour.
    Status(Status),
    /// The theme's muted text.
    Dim,
    /// The theme's strongest text.
    Bright,
    /// The theme's accent.
    Accent,
    /// Exactly this colour.
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// A colour from its name or `#rrggbb`, in any case: a slot's name ([`SLOT_NAMES`]), a
    /// status's, `dim`, `bright` or `accent`. Anything else is None: a short `#rgb`, a
    /// `#rrggbbaa`, a name no one gave.
    pub fn parse(s: &str) -> Option<ColorSpec> {
        let s = s.to_ascii_lowercase();
        if let Some(hex) = s.strip_prefix('#') {
            // ASCII first: fig sliced bytes here and panicked on "#1é234".
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(ColorSpec::Rgb(byte(0)?, byte(2)?, byte(4)?));
        }
        if let Some(slot) = SLOT_NAMES.iter().position(|n| *n == s) {
            return Some(ColorSpec::Slot(slot as u8));
        }
        if let Some(st) = Status::ALL.into_iter().find(|st| st.name() == s) {
            return Some(ColorSpec::Status(st));
        }
        match s.as_str() {
            "dim" => Some(ColorSpec::Dim),
            "bright" => Some(ColorSpec::Bright),
            "accent" => Some(ColorSpec::Accent),
            _ => None,
        }
    }

    /// Its name in a spec, as [`ColorSpec::parse`] reads it back: `#rrggbb` in lower case for
    /// an exact colour. A slot past 7 has no name and is written as slot 7's.
    pub fn name(self) -> String {
        match self {
            ColorSpec::Slot(i) => SLOT_NAMES[usize::from(i.min(7))].to_string(),
            ColorSpec::Status(st) => st.name().to_string(),
            ColorSpec::Dim => "dim".into(),
            ColorSpec::Bright => "bright".into(),
            ColorSpec::Accent => "accent".into(),
            ColorSpec::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// A `fig` block: how one series, node, edge or row of the data asks to be drawn.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FigStyle {
    /// `"color"`: its colour, which beats emphasis.
    pub color: Option<ColorSpec>,
    /// `"size"`, 1 to 3: line width 2, 3 or 4 px, point diameter 8, 11 or 14 px, node radius
    /// ×1, ×1.4 or ×1.8.
    pub size: Option<u8>,
    /// `"override-focus"`: drawn at full strength while something else has the emphasis. It
    /// matters only without a colour.
    pub override_focus: bool,
    /// `"show-label"`: its label shown, or hidden, whatever the figure's rule.
    pub show_label: Option<bool>,
    /// `"hidden"`: hidden when the figure opens.
    pub hidden: bool,
    /// `"label"`: its display name in place of the data's; on a rule, its annotation.
    pub label: Option<String>,
    /// `"dash"`: a dashed stroke, 6 on and 4 off.
    pub dash: bool,
}

impl FigStyle {
    /// The block a node holds: an object's known keys, each ignored if its value is of the
    /// wrong kind, and everything else ignored. A node that isn't an object is no block.
    ///
    /// `size` is a whole number, clamped to 1..=3; a fraction or a negative is ignored, as fig
    /// ignored them. (`2.0` is 2: a JSON number is an `f64` here, so it can't be told from `2`.)
    pub fn from_node(n: &Node) -> FigStyle {
        let mut st = FigStyle::default();
        let Some(obj) = n.as_obj() else {
            return st;
        };
        for (key, v) in obj {
            match &*key.name {
                "color" => st.color = v.as_str().and_then(ColorSpec::parse),
                "size" => st.size = v.as_f64().and_then(size),
                "override-focus" => st.override_focus = v.as_bool().unwrap_or(false),
                "show-label" => st.show_label = v.as_bool(),
                "hidden" => st.hidden = v.as_bool().unwrap_or(false),
                "label" => st.label = v.as_str().map(String::from),
                "dash" => st.dash = v.as_bool().unwrap_or(false),
                _ => {}
            }
        }
        st
    }
}

/// A block's size: a whole number, not negative, clamped to 1..=3.
fn size(v: f64) -> Option<u8> {
    (v >= 0.0 && v.fract() == 0.0).then(|| v.clamp(1.0, 3.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn style(text: &str) -> FigStyle {
        FigStyle::from_node(&parse(text).unwrap())
    }

    #[test]
    fn figs_names_keep_their_hues_in_the_slots_of_the_same_names() {
        let p = ColorSpec::parse;
        assert_eq!(p("red"), Some(ColorSpec::Slot(7)));
        assert_eq!(p("green"), Some(ColorSpec::Slot(5)));
        assert_eq!(p("blue"), Some(ColorSpec::Slot(0)));
        assert_eq!(p("yellow"), Some(ColorSpec::Slot(3)));
        assert_eq!(p("dim"), Some(ColorSpec::Dim));
        assert_eq!(p("bright"), Some(ColorSpec::Bright));
        // In any case, as fig lower-cased them.
        assert_eq!(p("RED"), Some(ColorSpec::Slot(7)));
        assert_eq!(p("Dim"), Some(ColorSpec::Dim));
    }

    #[test]
    fn every_slot_and_status_has_a_name() {
        for (i, name) in SLOT_NAMES.iter().enumerate() {
            assert_eq!(ColorSpec::parse(name), Some(ColorSpec::Slot(i as u8)));
        }
        let statuses: Vec<_> = ["good", "warning", "serious", "critical"]
            .iter()
            .map(|n| ColorSpec::parse(n))
            .collect();
        assert_eq!(statuses, Status::ALL.map(|s| Some(ColorSpec::Status(s))));
        assert_eq!(ColorSpec::parse("accent"), Some(ColorSpec::Accent));
        for bad in ["", "default", "grey", "normal", " red", "red ", "slot1"] {
            assert_eq!(ColorSpec::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_exact_colour_is_six_hex_digits_and_ascii_is_checked_first() {
        assert_eq!(
            ColorSpec::parse("#3987e5"),
            Some(ColorSpec::Rgb(0x39, 0x87, 0xe5))
        );
        assert_eq!(
            ColorSpec::parse("#FFaa00"),
            Some(ColorSpec::Rgb(0xff, 0xaa, 0))
        );
        // Seven bytes with a two-byte char in them: fig panicked here.
        for bad in [
            "#1é234",
            "#fff",
            "#ffffff00",
            "#gggggg",
            "#12345",
            "ffffff",
            "#",
            "#+12345",
        ] {
            assert_eq!(ColorSpec::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_name_reads_back() {
        let all = SLOT_NAMES
            .iter()
            .filter_map(|n| ColorSpec::parse(n))
            .chain(Status::ALL.map(ColorSpec::Status))
            .chain([
                ColorSpec::Dim,
                ColorSpec::Bright,
                ColorSpec::Accent,
                ColorSpec::Rgb(1, 0xab, 0xff),
            ]);
        for c in all {
            assert_eq!(ColorSpec::parse(&c.name()), Some(c), "{c:?}");
        }
    }

    #[test]
    fn a_block_reads_its_kebab_case_keys() {
        let st = style(
            r##"{"color": "#ff0000", "size": 2, "override-focus": true, "show-label": false,
                "hidden": true, "label": "p95", "dash": true}"##,
        );
        assert_eq!(
            st,
            FigStyle {
                color: Some(ColorSpec::Rgb(255, 0, 0)),
                size: Some(2),
                override_focus: true,
                show_label: Some(false),
                hidden: true,
                label: Some("p95".into()),
                dash: true,
            }
        );
        // snake_case is just an unknown key.
        let st = style(r#"{"override_focus": true, "show_label": true}"#);
        assert_eq!(st, FigStyle::default());
    }

    #[test]
    fn a_block_ignores_what_it_doesnt_know_and_bad_values() {
        assert_eq!(style("null"), FigStyle::default());
        assert_eq!(style("[1, 2]"), FigStyle::default());
        assert_eq!(style(r#""red""#), FigStyle::default());
        let st = style(
            r#"{"color": 3, "size": "big", "override-focus": "yes", "show-label": 1,
                "hidden": null, "label": 7, "dash": [6, 4], "weight": 2, "metadata": {}}"#,
        );
        assert_eq!(st, FigStyle::default());
        assert_eq!(style(r#"{"color": "chartreuse"}"#).color, None);
        assert_eq!(
            style(r#"{"color": "green", "size": 1}"#).color,
            Some(ColorSpec::Slot(5))
        );
    }

    #[test]
    fn size_is_clamped_to_one_to_three() {
        let size = |v: &str| style(&format!("{{\"size\": {v}}}")).size;
        assert_eq!(size("0"), Some(1));
        assert_eq!(size("1"), Some(1));
        assert_eq!(size("3"), Some(3));
        assert_eq!(size("40"), Some(3));
        assert_eq!(size("1e300"), Some(3));
        assert_eq!(size("2.0"), Some(2));
        assert_eq!(size("2.5"), None);
        assert_eq!(size("-1"), None);
    }
}
