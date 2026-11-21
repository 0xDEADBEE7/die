//! die-fig: figures from data in a DIE buffer (charts, network graphs and dashboards) drawn in
//! pixels, in the ethos of fig, as an extension (scope D14). It depends on die-core and
//! die-draw alone, so it's platform-neutral and out of the product: `cargo xtask lint` holds it
//! to that. Its parts:
//!
//! - this file: [`Fig`], the extension, with your [`Options`], its commands and its faces;
//! - [`json`], [`error`] and [`style`]: the positional JSON reader every spec and JSON file goes
//!   through, its errors with places, and the data's own `fig` blocks;
//! - [`data`]: tables read from CSV, TSV, JSON and JSONL, and their transforms;
//! - [`spec`]: what to draw, from a fig file, a spec, or a guess at bare data;
//! - [`scale`]: data to pixels, ticks, and how values are written;
//! - [`scene`], [`theme`], [`raster`] and [`svg`]: the display list, its colours, and its pixels,
//!   PNG and SVG;
//! - [`algo`]: layouts, downsampling, envelopes, search and the hit index;
//! - [`plot`] and [`interact`]: a chart drawn, and the keyboard's and pointer's ways with it;
//! - [`detail`], [`graph`], [`figure`], [`dashboard`], [`source`], [`surface`] and [`cells`]:
//!   the detail panel, the network figure, tiles, dashboards, where data comes from, the DIE
//!   surface, and the braille fallback.
//!
//! Nothing of fig's runs until `fig-toggle` asks for a figure.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod algo;
pub mod cells;
pub mod dashboard;
pub mod data;
pub mod detail;
pub mod error;
pub mod figure;
pub mod graph;
pub mod interact;
pub mod json;
pub mod plot;
pub mod raster;
pub mod scale;
pub mod scene;
pub mod source;
pub mod spec;
pub mod style;
pub mod surface;
pub mod svg;
pub mod theme;

pub use error::{FigError, Pos};

use die_core::{Args, Ctx, Extension, Registry};
use jiff::tz::TimeZone;
use std::cell::OnceCell;
use std::path::PathBuf;

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Options {
    /// The directory DIE's own fonts are in, which a figure falls back on where the system has
    /// none it knows. None finds it as DIE does: `$DIE_FONT_DIR`, the app's bundle, the
    /// workspace's `fonts/`.
    pub fonts: Option<PathBuf>,
}

/// The extension: figures from data.
#[derive(Debug, Default)]
pub struct Fig {
    options: Options,
    /// The zone times without an offset are in: the one injected, else the system's, looked up
    /// the first time it's needed.
    zone: OnceCell<TimeZone>,
}

impl Fig {
    /// The extension with your options, times in the system's zone.
    pub fn new(options: Options) -> Fig {
        Fig {
            options,
            zone: OnceCell::new(),
        }
    }

    /// The extension with the default options, times in `zone` rather than the system's: for
    /// tests, which use a fixed zone (ruling 2).
    pub fn with_zone(zone: TimeZone) -> Fig {
        Fig {
            options: Options::default(),
            zone: OnceCell::from(zone),
        }
    }

    /// Your options.
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The zone times without an offset are in.
    pub fn zone(&self) -> &TimeZone {
        self.zone.get_or_init(TimeZone::system)
    }
}

/// Every face die-fig declares, for a theme to style by name.
pub fn faces() -> &'static [&'static str] {
    theme::FACES
}

/// `fig-toggle`, for now: it says so.
fn toggle(ctx: &mut Ctx, _a: &Args) -> die_core::Result {
    ctx.messages.push("fig: not built yet".into());
    Ok(())
}

impl Extension for Fig {
    fn name(&self) -> &'static str {
        "fig"
    }

    fn register(&mut self, reg: &mut Registry) {
        theme::register(reg);
        reg.command(
            "fig-toggle",
            "Show this buffer's data as a figure, or the buffer again from its figure.",
            toggle,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use die_core::Harness;

    #[test]
    fn fig_toggle_says_its_not_built_yet() {
        let mut h = Harness::new(vec![Box::new(Fig::default())]).unwrap();
        h.open_text("data.csv", "time,a\n1,2\n").unwrap();
        h.run("fig-toggle", &Args::default()).unwrap();
        assert!(
            h.messages.iter().any(|m| m == "fig: not built yet"),
            "{:?}",
            h.messages
        );
        assert_eq!(h.next_timer(), None);
    }

    #[test]
    fn a_zone_given_is_the_zone_used() {
        let zone = TimeZone::fixed(jiff::tz::offset(2));
        let fig = Fig::with_zone(zone.clone());
        assert_eq!(fig.zone(), &zone);
        assert_eq!(fig.options(), &Options::default());
        assert_eq!(Fig::new(Options::default()).name(), "fig");
    }
}
