//! Faces and themes (contract §13). A face is a named style that can inherit
//! from a parent face; a theme is data that styles faces by name and sets the
//! 16 ANSI colors.
//!
//! Faces layer the way Emacs's do. What a face leaves unset comes from its
//! parent; what its whole chain leaves unset comes from the face below it on the
//! cell, and in the end from `default`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// A 24-bit sRGB color.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parse `#rrggbb`. Panics on anything else, so a theme written in code
    /// fails loudly (in a `const`, at compile time).
    pub const fn hex(s: &str) -> Rgb {
        let b = s.as_bytes();
        assert!(b.len() == 7 && b[0] == b'#', "a color is #rrggbb");
        Rgb(byte(b[1], b[2]), byte(b[3], b[4]), byte(b[5], b[6]))
    }
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("a color is #rrggbb"),
    }
}

const fn byte(hi: u8, lo: u8) -> u8 {
    nibble(hi) * 16 + nibble(lo)
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// The underline styles a cell can have (scope §5.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// What a face sets. `None` leaves an attribute to the parent face, then to
/// the face below on the cell, then to `default`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Style {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub underline: Option<Underline>,
    /// The underline's color; unset draws it in the foreground color.
    pub underline_color: Option<Rgb>,
}

impl Style {
    /// A style that sets nothing.
    pub fn new() -> Style {
        Style::default()
    }
    pub fn fg(mut self, c: Rgb) -> Style {
        self.fg = Some(c);
        self
    }
    pub fn bg(mut self, c: Rgb) -> Style {
        self.bg = Some(c);
        self
    }
    pub fn bold(mut self) -> Style {
        self.bold = Some(true);
        self
    }
    pub fn italic(mut self) -> Style {
        self.italic = Some(true);
        self
    }
    pub fn strike(mut self) -> Style {
        self.strike = Some(true);
        self
    }
    pub fn underline(mut self, u: Underline) -> Style {
        self.underline = Some(u);
        self
    }
    /// `self`, with what it leaves unset taken from `below`.
    pub fn over(self, below: Style) -> Style {
        Style {
            fg: self.fg.or(below.fg),
            bg: self.bg.or(below.bg),
            bold: self.bold.or(below.bold),
            italic: self.italic.or(below.italic),
            strike: self.strike.or(below.strike),
            underline: self.underline.or(below.underline),
            underline_color: self.underline_color.or(below.underline_color),
        }
    }
}

/// The base faces every theme styles (contract §13), each with its parent.
pub const BASE_FACES: [(&str, Option<&str>); 14] = [
    ("default", None),
    ("shadow", None),
    ("bold", None),
    ("highlight", None),
    ("selection", None),
    ("search", None),
    ("link", None),
    ("heading", Some("bold")),
    ("error", None),
    ("warning", None),
    ("success", None),
    ("line-number", Some("shadow")),
    ("status-line", None),
    ("cursor", None),
];

/// Faces for core's own screen parts, declared the way an extension declares
/// its faces. Each inherits from a base face, so a theme that styles only the
/// base faces still draws them sensibly.
pub const UI_FACES: [(&str, Option<&str>); 6] = [
    ("line-number-current", Some("line-number")),
    ("status-line-inactive", Some("status-line")),
    ("divider", Some("shadow")),
    ("search-current", Some("search")),
    ("bracket-match", Some("highlight")),
    ("prompt", Some("bold")),
];

/// A theme: styles for faces by name, and the 16 ANSI colors terminals use.
/// It's data, so a config builds one (contract §13). Faces it doesn't name keep
/// the style they were declared with; names no one declared are ignored.
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub name: String,
    pub faces: BTreeMap<String, Style>,
    /// Black, red, green, yellow, blue, magenta, cyan and white, then their bright forms.
    pub ansi: [Rgb; 16],
}

impl Theme {
    /// A theme that styles no faces yet.
    pub fn new(name: &str, ansi: [Rgb; 16]) -> Theme {
        Theme {
            name: name.into(),
            faces: BTreeMap::new(),
            ansi,
        }
    }
    /// Style one face.
    pub fn face(mut self, name: &str, style: Style) -> Theme {
        self.faces.insert(name.into(), style);
        self
    }
}

impl Default for Theme {
    /// Core's fallback until a config sets a theme: light on dark, with
    /// xterm's ANSI colors, styling only what a frame can't do without.
    fn default() -> Theme {
        let h = Rgb::hex;
        let ansi = [
            "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
            "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
        ]
        .map(h);
        Theme::new("default", ansi)
            .face("default", Style::new().fg(h("#d0d0d0")).bg(h("#101010")))
            .face("shadow", Style::new().fg(h("#707070")))
            .face("bold", Style::new().bold())
            .face("highlight", Style::new().bg(h("#303030")))
            .face("selection", Style::new().bg(h("#404040")))
            .face("search", Style::new().bg(h("#505050")))
            .face(
                "search-current",
                Style::new().fg(h("#101010")).bg(h("#d0d0d0")),
            )
            .face("error", Style::new().fg(h("#e05050")).bold())
            .face("status-line", Style::new().bg(h("#282828")))
            .face(
                "status-line-inactive",
                Style::new().fg(h("#707070")).bg(h("#101010")),
            )
            .face("cursor", Style::new().bg(h("#d0d0d0")))
    }
}

/// The face registry, and each face resolved against the current theme.
pub(crate) struct Faces {
    names: Vec<String>,
    parents: Vec<Option<usize>>,
    declared: Vec<Style>,
    ids: HashMap<String, usize>,
    theme: Theme,
    /// Each face's style with its parent chain applied.
    resolved: Vec<Style>,
}

impl Faces {
    /// Base and UI faces, then the extensions' `(name, parent, style)`, in order.
    pub(crate) fn new(decls: Vec<(String, Option<String>, Style)>) -> Result<Faces, Vec<String>> {
        let core = BASE_FACES
            .iter()
            .chain(&UI_FACES)
            .map(|(n, p)| (n.to_string(), p.map(String::from), Style::new()));
        let all: Vec<_> = core.chain(decls).collect();
        let mut errors = vec![];
        let mut ids = HashMap::new();
        for (i, (name, _, _)) in all.iter().enumerate() {
            if ids.insert(name.clone(), i).is_some() {
                errors.push(format!("face {name} declared twice"));
            }
        }
        let mut parents = vec![];
        for (name, parent, _) in &all {
            parents.push(parent.as_ref().and_then(|p| {
                let id = ids.get(p).copied();
                if id.is_none() {
                    errors.push(format!(
                        "face {name} inherits from {p}, which isn't declared"
                    ));
                }
                id
            }));
        }
        for start in 0..all.len() {
            let (mut at, mut steps) = (Some(start), 0);
            while let Some(i) = at {
                steps += 1;
                if steps > all.len() {
                    errors.push(format!("face {} inherits from itself", all[start].0));
                    break;
                }
                at = parents[i];
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut faces = Faces {
            names: all.iter().map(|d| d.0.clone()).collect(),
            parents,
            declared: all.iter().map(|d| d.2).collect(),
            ids,
            theme: Theme::default(),
            resolved: vec![],
        };
        faces.resolve();
        Ok(faces)
    }

    fn resolve(&mut self) {
        self.resolved = (0..self.names.len())
            .map(|i| {
                let mut style = Style::new();
                let mut at = Some(i);
                while let Some(f) = at {
                    let own = self
                        .theme
                        .faces
                        .get(&self.names[f])
                        .copied()
                        .unwrap_or(self.declared[f]);
                    style = style.over(own);
                    at = self.parents[f];
                }
                style
            })
            .collect();
    }

    pub(crate) fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.resolve();
    }

    pub(crate) fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The face's style with its parents applied, or None for an unknown name.
    pub(crate) fn style(&self, name: &str) -> Option<Style> {
        self.ids.get(name).map(|&i| self.resolved[i])
    }

    /// Like `style`, but an unknown name sets nothing.
    pub(crate) fn get(&self, name: &str) -> Style {
        self.style(name).unwrap_or_default()
    }
}

impl crate::ctx::Ctx {
    /// Use `theme` from the next frame on.
    pub fn set_theme(&mut self, theme: Theme) {
        self.faces.set_theme(theme);
    }

    pub fn theme(&self) -> &Theme {
        self.faces.theme()
    }

    /// A face's style under the current theme, with its parent chain applied
    /// but nothing from `default` or the faces below it on a cell. None if no
    /// face has that name.
    pub fn face_style(&self, name: &str) -> Option<Style> {
        self.faces.style(name)
    }
}
