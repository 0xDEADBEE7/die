//! The dial, the whiteboard's ported: the sectors clockwise from north about a dead zone a
//! cell's larger side, past the ring an arc of words for view, export and settings whose first
//! band picks nothing and whose angle picks past it, the flick that lands past the ring at once
//! staying stage one's, a release and a press picking what the dial says and the dead zone or a
//! sector that doesn't apply nothing, the current choice centred, and the readout. Laid out with
//! `FixedMeasure`, the names clear the ring and each other, and stage two's words clear the line
//! art, the chip and each other, wherever the anchor falls, the corners included, and the
//! readout meets nothing. Drawn, it's line art on a scrim of the surface with no disc, the
//! hovered stretch and chip in the accent, a needle dotted until stage two picks, dimmed sectors
//! dimmed, regions for each name and word and the dead centre, the theme's dial faces, text
//! never in a series colour, and what follows the pointer within the dial's bounds.

use die_test_cap as _;

use die_fig::dial::{
    DialLayout, DialPick, DialSpec, DialState, EXPORT, NAMES, SETTINGS, Stage, VIEW, alpha, art,
    draw, draw_live, draw_still, layout, sector, toward,
};
use die_fig::scene::{
    Anchor, Cap, FixedMeasure, Hit, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec,
    Verb, Weight,
};
use die_fig::theme::{Color, Theme};

/// The whiteboard tests' cells, so the ring is 25 out, the dead zone 10, stage two opens at 30,
/// its guide is at 42.5 and its arc at 75.
const CELL: (f32, f32) = (5.0, 10.0);
/// Your 16 pt cells, in logical px.
const BIG: (f32, f32) = (9.5, 20.0);
const C: (f32, f32) = (100.0, 100.0);

fn out(c: (f32, f32), deg: f32, r: f32) -> (f32, f32) {
    let u = toward(deg);
    (c.0 + u.0 * r, c.1 + u.1 * r)
}

/// The point `r` out from (100, 100), `deg` clockwise from north.
fn at(deg: f32, r: f32) -> (f32, f32) {
    out(C, deg, r)
}

fn dial() -> DialState {
    DialState::open(C, CELL)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn near(c: (f32, f32), r: &Rect) -> f32 {
    let dx = (r.x - c.0).max(0.0).max(c.0 - r.right());
    let dy = (r.y - c.1).max(0.0).max(c.1 - r.bottom());
    dx.hypot(dy)
}

/// Whether two boxes overlap, or come within `gx` across and `gy` up and down.
fn within(a: &Rect, b: &Rect, gx: f32, gy: f32) -> bool {
    a.x < b.right() + gx && b.x < a.right() + gx && a.y < b.bottom() + gy && b.y < a.bottom() + gy
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    within(a, b, 0.0, 0.0)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// Whether a path's bounds are `r`, but for rounding.
fn bounded(p: &PathData, r: Rect) -> bool {
    p.bounds().is_some_and(|b| {
        close(b.x, r.x)
            && close(b.y, r.y)
            && close(b.right(), r.right())
            && close(b.bottom(), r.bottom())
    })
}

#[test]
fn the_sectors_go_clockwise_from_north_and_the_dead_zone_is_a_cells_larger_side() {
    let spec = DialSpec::chart();
    let names: Vec<&str> = spec.sectors.iter().map(|s| s.name).collect();
    assert_eq!(names, NAMES);
    assert_eq!(
        NAMES,
        [
            "detail", "compare", "only", "table", "view", "export", "hide", "settings"
        ]
    );
    let mut d = dial();
    assert_eq!((d.radius(), d.dead()), (25.0, 10.0));
    let s = |dx: f32, dy: f32| d.sector_at((100.0 + dx, 100.0 + dy));
    assert_eq!(s(0.0, -20.0), Some(sector::DETAIL), "north: detail");
    assert_eq!(s(20.0, -20.0), Some(sector::COMPARE), "north-east: compare");
    assert_eq!(s(20.0, 0.0), Some(sector::ONLY), "east: only");
    assert_eq!(s(20.0, 20.0), Some(sector::TABLE), "south-east: table");
    assert_eq!(s(0.0, 20.0), Some(sector::VIEW), "south: view");
    assert_eq!(s(-20.0, 20.0), Some(sector::EXPORT), "south-west: export");
    assert_eq!(s(-20.0, 0.0), Some(sector::HIDE), "west: hide");
    assert_eq!(
        s(-20.0, -20.0),
        Some(sector::SETTINGS),
        "north-west: settings"
    );
    assert_eq!(s(-3.0, -20.0), Some(0), "a little off north is still north");
    assert_eq!(s(0.0, -9.0), None, "the dead zone");
    assert_eq!(s(0.0, 0.0), None);
    // The needle follows every move; the hovered sector changes only across a spoke.
    d.moved((120.0, 100.0), &spec);
    assert_eq!(d.hovered(), Some(sector::ONLY));
    d.moved((122.0, 101.0), &spec);
    assert_eq!((d.hovered(), d.pointer()), (Some(2), (122.0, 101.0)));
    d.moved(C, &spec);
    assert_eq!((d.hovered(), d.stage()), (None, Stage::Sectors));
    // A larger cell: the ring 2.5 cell heights out, the dead zone its larger side.
    let big = DialState::open(C, BIG);
    assert_eq!((big.radius(), big.dead()), (50.0, 20.0));
    let tall = DialState::open(C, (30.0, 20.0));
    assert_eq!(tall.dead(), 30.0);
}

#[test]
fn past_the_ring_a_sector_with_choices_opens_its_arc_whose_first_band_picks_nothing() {
    let spec = DialSpec::chart();
    let mut d = dial();
    // The ring at 25, stage two from 30, its first band to 42.5, the arc at 75.
    assert_eq!((d.opens(), d.band(), d.arc_radius()), (30.0, 42.5, 75.0));
    d.moved(at(180.0, 20.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::VIEW), Stage::Sectors)
    );
    d.moved(at(180.0, 35.0), &spec);
    assert_eq!(d.stage(), Stage::Arc, "past 30: stage two, its first band");
    assert_eq!(d.readout(&spec).line(), "view", "as stage one says");
    d.moved(at(180.0, 50.0), &spec);
    assert_eq!(
        d.stage(),
        Stage::Choice(2),
        "past the guide, straight out: the middle, 24h"
    );
    assert_eq!(d.readout(&spec).line(), "view: 24h");
    // 20° a choice clockwise, 24h centred south: 1h from 130°.
    assert_eq!(d.span(&spec), Some((130.0, 230.0)));
    d.moved(at(140.0, 60.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::VIEW), Stage::Choice(0)),
        "held: the angle picks now, in table's direction"
    );
    d.moved(at(215.0, 60.0), &spec);
    assert_eq!(d.stage(), Stage::Choice(4));
    d.moved(at(300.0, 60.0), &spec);
    assert_eq!(
        d.stage(),
        Stage::Choice(4),
        "past the clockwise end: the last"
    );
    d.moved(at(60.0, 60.0), &spec);
    assert_eq!(d.stage(), Stage::Choice(0), "past the other: the first");
    // Back in the first band nothing is picked again; between the ring and where stage two
    // opens it stays open; inside the ring it closes.
    d.moved(at(170.0, 35.0), &spec);
    assert_eq!((d.hovered(), d.stage()), (Some(sector::VIEW), Stage::Arc));
    d.moved(at(170.0, 27.0), &spec);
    assert_eq!(d.stage(), Stage::Arc);
    d.moved(at(90.0, 20.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::ONLY), Stage::Sectors)
    );
    // A sector without choices has no stage two, however far out.
    d.moved(at(90.0, 90.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::ONLY), Stage::Sectors)
    );
    assert_eq!(d.span(&spec), None);
    // Export's three formats about south-west, svg in the middle; settings' five about
    // north-west, log in the middle.
    let mut d = dial();
    d.moved(at(225.0, 20.0), &spec);
    d.moved(at(225.0, 60.0), &spec);
    assert_eq!(
        (d.span(&spec), d.stage()),
        (Some((195.0, 255.0)), Stage::Choice(1))
    );
    assert_eq!(d.readout(&spec).line(), "export: svg");
    let mut d = dial();
    d.moved(at(315.0, 20.0), &spec);
    d.moved(at(315.0, 60.0), &spec);
    assert_eq!(
        (d.span(&spec), d.stage()),
        (Some((265.0, 365.0)), Stage::Choice(2))
    );
    assert_eq!(d.readout(&spec).line(), "settings: log");
    // Past north, the angle wraps: 10° is markers', the last.
    d.moved(at(10.0, 60.0), &spec);
    assert_eq!(d.stage(), Stage::Choice(4));
    assert_eq!(
        (VIEW.len(), EXPORT.len(), SETTINGS.len()),
        (5, 3, 5),
        "1h 6h 24h 7d all; png svg copy; grid labels log stack markers"
    );
    // A sector that doesn't apply opens nothing either.
    let mut dim = DialSpec::chart();
    dim.sectors[usize::from(sector::EXPORT)].enabled = false;
    let mut d = dial();
    d.moved(at(225.0, 20.0), &dim);
    d.moved(at(225.0, 60.0), &dim);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::EXPORT), Stage::Sectors)
    );
}

#[test]
fn the_arc_centres_the_current_choice_so_straight_out_keeps_it() {
    let spec = DialSpec::chart();
    for (k, word) in VIEW.iter().enumerate() {
        let mut d = dial();
        d.centre_on(sector::VIEW, k as u8);
        d.moved(at(180.0, 20.0), &spec);
        d.moved(at(180.0, 50.0), &spec);
        assert_eq!(d.stage(), Stage::Choice(k as u8), "{word}");
        assert_eq!(d.readout(&spec).line(), format!("view: {word}"));
        let start = 180.0 - (k as f32 + 0.5) * 20.0;
        assert_eq!(d.span(&spec), Some((start, start + 100.0)));
    }
    // Export the first time: png, straight out, the arc running on clockwise from it.
    let mut d = dial();
    d.centre_on(sector::EXPORT, 0);
    d.moved(at(225.0, 20.0), &spec);
    d.moved(at(225.0, 50.0), &spec);
    assert_eq!(
        (d.stage(), d.span(&spec)),
        (Stage::Choice(0), Some((215.0, 275.0)))
    );
    // A choice past the last is the last.
    d.centre_on(sector::EXPORT, 9);
    assert_eq!(d.span(&spec), Some((175.0, 235.0)));
}

#[test]
fn a_flick_that_hovers_a_sector_and_lands_past_the_ring_at_once_is_stage_ones() {
    let spec = DialSpec::chart();
    // One move 35 out at 240°: export's sector, past where stage two opens, and in copy's
    // segment. It hovers export, and letting go there doesn't open it either.
    let mut d = dial();
    d.moved(at(240.0, 35.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::EXPORT), Stage::Sectors)
    );
    d.settle(at(240.0, 35.0), &spec);
    assert_eq!(d.stage(), Stage::Sectors);
    assert_eq!(d.readout(&spec).line(), "export");
    assert_eq!(d.release(&spec), DialPick::Sector(sector::EXPORT));
    // However far it lands.
    let mut d = dial();
    d.moved(at(240.0, 90.0), &spec);
    d.settle(at(240.0, 90.0), &spec);
    assert_eq!(d.release(&spec), DialPick::Sector(sector::EXPORT));
    // A sample repeated where it landed is no move.
    let mut d = dial();
    d.moved(at(240.0, 50.0), &spec);
    d.moved(at(240.0, 50.0), &spec);
    assert_eq!(d.stage(), Stage::Sectors);
    assert_eq!(d.release(&spec), DialPick::Sector(sector::EXPORT));
    // A move into another sector past the ring hovers it; only the next opens it.
    let mut d = dial();
    d.moved(at(180.0, 20.0), &spec);
    d.moved(at(225.0, 50.0), &spec);
    assert_eq!(
        (d.hovered(), d.stage()),
        (Some(sector::EXPORT), Stage::Sectors)
    );
    d.moved(at(225.0, 51.0), &spec);
    assert_eq!(d.stage(), Stage::Choice(1));
    // A flick of a few moves that overshoots into the first band picks nothing of stage two,
    // 15° off the axis where copy's segment is.
    let mut d = dial();
    for r in [15.0, 28.0, 38.0] {
        d.moved(at(240.0, r), &spec);
    }
    assert_eq!((d.hovered(), d.stage()), (Some(sector::EXPORT), Stage::Arc));
    assert_eq!(d.readout(&spec).line(), "export");
    assert_eq!(d.release(&spec), DialPick::Sector(sector::EXPORT));
    // Once it's open, letting go elsewhere moves in it as any move does.
    d.settle(at(240.0, 50.0), &spec);
    assert_eq!(d.stage(), Stage::Choice(2));
    assert_eq!(d.release(&spec), DialPick::Stage2(sector::EXPORT, 2));
}

#[test]
fn a_release_picks_what_the_dial_says_and_the_dead_zone_or_a_dimmed_sector_nothing() {
    let mut spec = DialSpec::chart();
    spec.sectors[usize::from(sector::COMPARE)].enabled = false;
    let mut d = dial();
    assert_eq!(
        d.release(&spec),
        DialPick::None,
        "a right click is harmless"
    );
    d.moved((104.0, 104.0), &spec);
    assert_eq!((d.hovered(), d.release(&spec)), (None, DialPick::None));
    d.moved(at(90.0, 20.0), &spec);
    assert_eq!(d.release(&spec), DialPick::Sector(sector::ONLY));
    d.moved(at(45.0, 20.0), &spec);
    assert_eq!(d.hovered(), Some(sector::COMPARE));
    assert_eq!(
        d.release(&spec),
        DialPick::None,
        "compare doesn't apply here"
    );
    let r = d.readout(&spec);
    assert_eq!((r.line(), r.dim), ("compare".to_string(), true));
    // The first band is stage one's; past it, the choice.
    d.moved(at(180.0, 20.0), &spec);
    d.moved(at(180.0, 35.0), &spec);
    assert_eq!(d.release(&spec), DialPick::Sector(sector::VIEW));
    d.moved(at(180.0, 50.0), &spec);
    assert_eq!(d.release(&spec), DialPick::Stage2(sector::VIEW, 2));
}

#[test]
fn a_press_while_open_picks_at_whichever_stage_and_never_opens_stage_two() {
    let spec = DialSpec::chart();
    let mut d = dial();
    d.moved(at(0.0, 20.0), &spec);
    assert_eq!(
        d.press(at(0.0, 20.0), &spec),
        DialPick::Sector(sector::DETAIL)
    );
    // A press past the ring after a move that hovered view inside it: stage two doesn't open
    // under a press.
    let mut d = dial();
    d.moved(at(180.0, 20.0), &spec);
    assert_eq!(
        d.press(at(180.0, 60.0), &spec),
        DialPick::Sector(sector::VIEW)
    );
    // In stage two it picks the choice there: 7d at 195°.
    let mut d = dial();
    d.moved(at(180.0, 20.0), &spec);
    d.moved(at(180.0, 50.0), &spec);
    assert_eq!(
        d.press(at(195.0, 60.0), &spec),
        DialPick::Stage2(sector::VIEW, 3)
    );
    // In the dead zone, nothing.
    let mut d = dial();
    d.moved(at(90.0, 20.0), &spec);
    assert_eq!(d.press(C, &spec), DialPick::None);
}

#[test]
fn the_readout_says_what_letting_go_does_and_with_nothing_hovered_the_target_dimmed() {
    let spec = DialSpec::chart();
    let area = (400.0, 300.0);
    let mut d = dial();
    assert_eq!(d.readout(&spec).line(), "");
    assert_eq!(
        layout(&d, &spec, area, &mut FixedMeasure).readout,
        None,
        "nothing to say"
    );
    d.set_target("api · 06:05 · 642");
    let r = d.readout(&spec);
    assert_eq!((r.line(), r.dim), ("api · 06:05 · 642".to_string(), true));
    let l = layout(&d, &spec, area, &mut FixedMeasure);
    let b = l.readout.expect("the target");
    let w = FixedMeasure.width("api · 06:05 · 642", TextSpec::new(6.0));
    assert!(close(b.w, w) && b.h == CELL.1, "{b:?}");
    d.moved(at(90.0, 20.0), &spec);
    let r = d.readout(&spec);
    assert_eq!(
        (r.key.as_str(), r.value.as_str(), r.dim),
        ("only", "", false)
    );
    d.moved(at(180.0, 20.0), &spec);
    d.moved(at(180.0, 50.0), &spec);
    let r = d.readout(&spec);
    assert_eq!(
        (r.key.as_str(), r.value.as_str(), r.dim),
        ("view:", "24h", false)
    );
}

/// Anchors over an area: about its middle, wherever in a cell the anchor falls, and at and by
/// each corner.
fn anchors(area: (f32, f32), cell: (f32, f32)) -> Vec<(f32, f32)> {
    let mut out = vec![];
    for fx in [0.0, 0.3, 0.6, 0.99] {
        for fy in [0.0, 0.25, 0.5, 0.75, 0.99] {
            out.push((area.0 / 2.0 + fx * cell.0, area.1 / 2.0 + fy * cell.1));
        }
    }
    let (w, h) = area;
    out.extend([(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]);
    out.extend([
        (1.0, 1.0),
        (w - 1.0, 1.0),
        (1.0, h - 1.0),
        (w - 1.0, h - 1.0),
    ]);
    out.extend([
        (cell.0 * 3.0, h - cell.1 * 2.0),
        (w - cell.0 * 3.0, cell.1 * 2.0),
    ]);
    out
}

/// What the names are measured in: the chip's weight.
fn bold(cell: (f32, f32)) -> TextSpec {
    TextSpec::new(0.6 * cell.1).weight(Weight::Semibold)
}

#[test]
fn the_names_clear_the_ring_its_ticks_and_each_other_wherever_the_anchor_falls() {
    let spec = DialSpec::chart();
    for (cell, area) in [(CELL, (400.0, 300.0)), (BIG, (800.0, 600.0))] {
        for a in anchors(area, cell) {
            for hovered in [None, Some(sector::COMPARE), Some(sector::VIEW)] {
                let mut d = DialState::open(a, cell);
                d.set_target("api · 06:05 · 642");
                if let Some(i) = hovered {
                    d.moved(out(a, f32::from(i) * 45.0, d.radius() * 0.8), &spec);
                }
                let l = layout(&d, &spec, area, &mut FixedMeasure);
                let at = format!("{a:?} {cell:?} {hovered:?}");
                let clear = d.radius() + art::TICK + art::CLEAR;
                assert_eq!(l.shown, [true; 8], "{at}");
                assert!(l.labels.is_empty(), "{at}");
                for (i, b) in l.names.iter().enumerate() {
                    assert!(near(a, b) >= clear - 1e-3, "{i} at {at}: {b:?}");
                    let w = FixedMeasure.width(NAMES[i], bold(cell)) + 2.0 * cell.0;
                    assert!(
                        close(b.w, w) && b.h == cell.1,
                        "a cell either side: {i} {at}"
                    );
                    for (j, o) in l.names[..i].iter().enumerate() {
                        assert!(!overlap(o, b), "{i} meets {j} at {at}");
                    }
                }
                // North and south are centred on the anchor's x, east and west on its y.
                let n = &l.names;
                assert!(close(n[0].x + n[0].w / 2.0, a.0) && close(n[4].x + n[4].w / 2.0, a.0));
                assert!(close(n[2].y + n[2].h / 2.0, a.1) && close(n[6].y + n[6].h / 2.0, a.1));
                assert!(n[0].bottom() < a.1 && n[4].y > a.1 && n[2].x > a.0 && n[6].right() < a.0);
                assert_eq!(l.chip, hovered.map(|i| n[usize::from(i)]), "{at}");
                // The readout is under it all, or over it where the area ends, inside its sides.
                let r = l.readout.unwrap();
                for b in n {
                    assert!(
                        r.y >= b.bottom() || r.bottom() <= b.y,
                        "{at}: {r:?} and {b:?}"
                    );
                }
                assert!(
                    r.y > a.1 + clear || r.bottom() < a.1 - clear,
                    "clear of the ring: {at}"
                );
                assert!(r.x >= 0.0 && r.right() <= area.0 + 1e-3, "{at}: {r:?}");
                let below = n.iter().map(Rect::bottom).fold(f32::MIN, f32::max);
                if below + 2.0 * cell.1 <= area.1 {
                    assert!(
                        close(r.y, below + cell.1),
                        "a blank line under the names: {at}"
                    );
                }
            }
        }
    }
}

/// Points on everything the dial draws but the needle and the text: the ring and its ticks,
/// and in stage two the arc, its ticks and caps, and its guide.
fn line_art(d: &DialState, spec: &DialSpec) -> Vec<(f32, f32)> {
    let c = d.anchor();
    let mut pts: Vec<(f32, f32)> = (0..1440)
        .map(|k| out(c, k as f32 / 4.0, d.radius()))
        .collect();
    let tick = |deg: f32, r: f32, reach: f32| {
        (-8..=8).map(move |t| out(c, deg, r + reach * t as f32 / 8.0))
    };
    for k in 0..8 {
        pts.extend(tick((k as f32 - 0.5) * 45.0, d.radius(), art::TICK));
    }
    if let (Some((start, end)), true) = (d.span(spec), d.stage() != Stage::Sectors) {
        for r in [d.arc_radius(), d.band()] {
            let n = ((end - start) * 8.0) as usize;
            pts.extend((0..=n).map(|k| out(c, start + k as f32 / 8.0, r)));
        }
        let n = ((end - start) / 20.0).round() as usize;
        for k in 0..=n {
            let reach = if k == 0 || k == n {
                art::CAP
            } else {
                art::TICK
            };
            pts.extend(tick(start + k as f32 * 20.0, d.arc_radius(), reach));
        }
    }
    pts
}

/// A dial on `sector`'s arc with `current` centred, pushed straight out past the guide.
fn on_arc(a: (f32, f32), cell: (f32, f32), i: u8, current: u8, spec: &DialSpec) -> DialState {
    let mut d = DialState::open(a, cell);
    d.centre_on(i, current);
    d.set_target("api · 06:05 · 642");
    let dir = f32::from(i) * 45.0;
    d.moved(out(a, dir, d.radius() * 0.8), spec);
    d.moved(out(a, dir, d.band() + 1.0), spec);
    d
}

#[test]
fn stage_twos_words_clear_the_line_art_the_chip_and_each_other_at_any_anchor_including_corners() {
    let spec = DialSpec::chart();
    for cell in [CELL, BIG] {
        let area = (800.0, 600.0);
        for i in [sector::VIEW, sector::EXPORT, sector::SETTINGS] {
            let words = &spec.sectors[usize::from(i)].stage2;
            for current in 0..words.len() as u8 {
                for a in anchors(area, cell) {
                    let d = on_arc(a, cell, i, current, &spec);
                    let at = format!(
                        "{} centred on {current} at {a:?}, {cell:?}",
                        NAMES[usize::from(i)]
                    );
                    assert_eq!(d.stage(), Stage::Choice(current), "{at}");
                    check_stage_two(&d, &spec, area, &at);
                }
            }
        }
    }
}

fn check_stage_two(d: &DialState, spec: &DialSpec, area: (f32, f32), at: &str) {
    let (c, cell) = (d.anchor(), d.cell());
    let i = usize::from(d.hovered().unwrap());
    let l = layout(d, spec, area, &mut FixedMeasure);
    let art = line_art(d, spec);
    let on_art = |b: &Rect| {
        art.iter()
            .find(|p| b.inflate(art::CLEAR - 0.01).contains(p.0, p.1))
            .copied()
    };
    let beyond = d.arc_radius() + art::CAP + art::CLEAR;
    // The chip, past the arc's caps, on the sector's own direction.
    let chip = l.chip.expect("a chip");
    assert_eq!(chip, l.names[i], "{at}");
    assert!(l.shown[i], "{at}");
    assert!(
        near(c, &chip) >= beyond - 1e-3,
        "the chip past the arc: {at}"
    );
    assert_eq!(on_art(&chip), None, "the chip on the line art: {at}");
    // Each word, outside the arc at its segment, clear of the line art, the chip and the
    // words before it, a cell across on its line.
    let words = &spec.sectors[i].stage2;
    let ks: Vec<u8> = l.labels.iter().map(|(k, _)| *k).collect();
    assert_eq!(ks, (0..words.len() as u8).collect::<Vec<_>>(), "{at}");
    let (start, _) = d.span(spec).unwrap();
    for (n, (k, b)) in l.labels.iter().enumerate() {
        let word = words[usize::from(*k)];
        assert!(
            close(b.w, FixedMeasure.width(word, bold(cell))),
            "{word}: {at}"
        );
        assert!(
            near(c, b) >= beyond - 1e-3,
            "{word} inside the arc's caps: {at}"
        );
        assert_eq!(on_art(b), None, "{word} on the line art: {at}");
        assert!(
            !within(b, &chip, cell.0 - 0.01, cell.1 / 2.0 - 0.01),
            "{word} on the chip: {at}"
        );
        for (_, o) in &l.labels[..n] {
            assert!(
                !within(o, b, cell.0 - 0.01, art::CLEAR - 0.01),
                "{word} beside {o:?}: {at}"
            );
        }
        // Its middle is in its own segment, so the word is where its angle picks it.
        let mid = (b.x + b.w / 2.0, b.y + b.h / 2.0);
        let mut probe = d.clone();
        probe.settle(mid, spec);
        assert_eq!(
            probe.stage(),
            Stage::Choice(*k),
            "{word} at {mid:?}, the arc from {start}: {at}"
        );
    }
    // The names that show: clear of the line art, the chip, the words and each other.
    let shown: Vec<(usize, &Rect)> = l
        .names
        .iter()
        .enumerate()
        .filter(|(j, _)| l.shown[*j])
        .collect();
    for &(j, b) in &shown {
        if j == i {
            continue;
        }
        assert_eq!(on_art(b), None, "{} on the line art: {at}", NAMES[j]);
        assert!(!overlap(b, &chip), "{} under the chip: {at}", NAMES[j]);
        for (_, w) in &l.labels {
            assert!(!overlap(b, w), "{} under a word: {at}", NAMES[j]);
        }
        for &(k, o) in &shown {
            assert!(
                k == j || !overlap(b, o),
                "{} meets {}: {at}",
                NAMES[j],
                NAMES[k]
            );
        }
    }
    // The readout meets nothing, and stays inside the area's sides.
    let r = l.readout.expect("the readout");
    assert_eq!(on_art(&r), None, "the readout on the line art: {at}");
    for &(j, b) in &shown {
        assert!(!overlap(&r, b), "the readout on {}: {at}", NAMES[j]);
    }
    for (_, w) in &l.labels {
        assert!(!overlap(&r, w), "the readout on a word: {at}");
    }
    assert!(r.x >= 0.0 && r.right() <= area.0 + 1e-3, "{at}: {r:?}");
    // The scrim reaches past everything but the chip, which is solid.
    for (_, b) in &l.labels {
        assert!(far(c, b) <= l.reach + 1e-3, "{at}");
    }
}

fn far(c: (f32, f32), r: &Rect) -> f32 {
    let dx = (r.x - c.0).abs().max((r.right() - c.0).abs());
    let dy = (r.y - c.1).abs().max((r.bottom() - c.1).abs());
    dx.hypot(dy)
}

#[test]
fn in_stage_two_a_name_gives_way_only_to_what_crosses_or_meets_it() {
    let spec = DialSpec::chart();
    let area = (400.0, 300.0);
    let a = (200.0, 150.0);
    // View's arc, 130° to 230°, and its guide cross table and export, which give way; the rest
    // show, faint.
    let d = on_arc(a, CELL, sector::VIEW, 2, &spec);
    let l = layout(&d, &spec, area, &mut FixedMeasure);
    assert_eq!(l.shown, [true, true, true, false, true, false, true, true]);
    // Stage one before it showed them all, at the same places but view's.
    let mut one = DialState::open(a, CELL);
    one.moved(out(a, 180.0, 20.0), &spec);
    let first = layout(&one, &spec, area, &mut FixedMeasure);
    assert_eq!(first.shown, [true; 8]);
    for j in (0..8).filter(|j| *j != usize::from(sector::VIEW)) {
        assert_eq!(first.names[j], l.names[j], "{}", NAMES[j]);
    }
    assert!(
        l.names[4].y > first.names[4].y + 40.0,
        "view's chip past the arc"
    );
    // Everywhere: a name that gives way has the line art, a word or the chip on it, and one
    // that shows has none of them, give or take half a px.
    for cell in [CELL, BIG] {
        let area = (800.0, 600.0);
        for i in [sector::VIEW, sector::EXPORT, sector::SETTINGS] {
            let n = spec.sectors[usize::from(i)].stage2.len() as u8;
            for current in 0..n {
                for a in anchors(area, cell) {
                    let d = on_arc(a, cell, i, current, &spec);
                    let l = layout(&d, &spec, area, &mut FixedMeasure);
                    let art = line_art(&d, &spec);
                    let chip = l.chip.unwrap();
                    for j in (0..8).filter(|j| *j != usize::from(i)) {
                        let b = l.names[j];
                        let on = |slack: f32| {
                            let r = b.inflate(art::CLEAR + slack);
                            art.iter().any(|p| r.contains(p.0, p.1))
                                || l.labels
                                    .iter()
                                    .any(|(_, w)| within(w, &b, cell.0 + slack, art::CLEAR + slack))
                                || within(&chip, &b, slack, art::CLEAR + slack)
                        };
                        let at = format!("{} for {i}/{current} at {a:?} {cell:?}", NAMES[j]);
                        if l.shown[j] {
                            assert!(!on(-0.5), "{at} shows under something");
                        } else {
                            assert!(on(0.5), "{at} gave way to nothing");
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- drawn

struct Path<'a> {
    path: &'a PathData,
    fill: Option<(Color, f32)>,
    stroke: Option<&'a Stroke>,
}

fn solid(p: &Paint) -> (Color, f32) {
    match p {
        Paint::Solid { color, alpha } => (*color, *alpha),
        other => panic!("a gradient: {other:?}"),
    }
}

fn paths(s: &Scene) -> Vec<Path<'_>> {
    s.items
        .iter()
        .filter_map(|it| match it {
            Item::Path { path, fill, stroke } => Some(Path {
                path,
                fill: fill.as_ref().map(solid),
                stroke: stroke.as_ref(),
            }),
            _ => None,
        })
        .collect()
}

#[derive(Debug)]
struct Text {
    text: String,
    x: f32,
    y: f32,
    spec: TextSpec,
    anchor: Anchor,
    color: Color,
    alpha: f32,
}

fn texts(s: &Scene) -> Vec<Text> {
    s.items
        .iter()
        .filter_map(|it| match it {
            Item::Text {
                text,
                x,
                y,
                spec,
                anchor,
                color,
                alpha,
                ..
            } => Some(Text {
                text: text.to_string(),
                x: *x,
                y: *y,
                spec: *spec,
                anchor: *anchor,
                color: *color,
                alpha: *alpha,
            }),
            _ => None,
        })
        .collect()
}

fn text<'a>(ts: &'a [Text], s: &str) -> &'a Text {
    let found: Vec<&Text> = ts.iter().filter(|t| t.text == s).collect();
    assert_eq!(found.len(), 1, "{s} once: {ts:#?}");
    found[0]
}

/// Each subpath's bounds: a disc's one, a ring's outer and its hole.
fn subpaths(p: &PathData) -> Vec<Rect> {
    let mut parts: Vec<PathData> = vec![];
    for v in &p.verbs {
        if matches!(v, Verb::Move(..)) {
            parts.push(PathData::default());
        }
        if let Some(last) = parts.last_mut() {
            last.verbs.push(*v);
        }
    }
    parts.iter().filter_map(PathData::bounds).collect()
}

/// The end points of a path's steps.
fn ends(p: &PathData) -> Vec<(f32, f32)> {
    p.verbs
        .iter()
        .filter_map(|v| match *v {
            Verb::Move(x, y) | Verb::Line(x, y) | Verb::Quad(_, _, x, y) => Some((x, y)),
            Verb::Cubic(_, _, _, _, x, y) => Some((x, y)),
            Verb::Close => None,
        })
        .collect()
}

fn drawn(d: &DialState, spec: &DialSpec, theme: &Theme) -> (Scene, DialLayout) {
    let mut scene = Scene::new(400.0, 300.0, 2.0);
    draw(d, spec, theme, &mut scene, &mut FixedMeasure);
    let l = layout(d, spec, (400.0, 300.0), &mut FixedMeasure);
    (scene, l)
}

/// The scrim's layers: each one in the surface's colour about the anchor, its outer radius,
/// its hole's (0 for a disc) and its opacity.
fn scrim_layers(s: &Scene, theme: &Theme, c: (f32, f32)) -> Vec<(f32, f32, f32)> {
    paths(s)
        .iter()
        .filter(|p| p.fill.is_some_and(|f| f.0 == theme.surface))
        .map(|p| {
            let parts = subpaths(p.path);
            let radius = |b: &Rect| {
                assert!(
                    close(b.x + b.w / 2.0, c.0) && close(b.y + b.h / 2.0, c.1),
                    "about the anchor"
                );
                b.w / 2.0
            };
            let hole = parts.get(1).map_or(0.0, radius);
            (radius(&parts[0]), hole, p.fill.unwrap().1)
        })
        .collect()
}

/// The scrim's opacity `r` out from the anchor: every layer over it.
fn scrim_at(layers: &[(f32, f32, f32)], r: f32) -> f32 {
    let clear: f32 = layers
        .iter()
        .filter(|(outer, hole, _)| r >= *hole && r < *outer)
        .map(|(_, _, a)| 1.0 - a)
        .product();
    1.0 - clear
}

#[test]
fn the_dial_draws_line_art_on_a_scrim_of_the_surface_and_no_disc() {
    let theme = Theme::mono_dark();
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let d = DialState::open(a, CELL);
    let (scene, l) = drawn(&d, &spec, &theme);
    let s = d.scrim(&l);
    assert_eq!(s, l.reach + CELL.1, "a cell past the farthest thing drawn");
    // The scrim: the surface's colour, held at 0.88 through most of the names, 0.7 at their
    // edge a cell inside it, nothing past it, and straight between.
    let names = 1.0 - CELL.1 / s;
    let want = |r: f32| {
        let t = r / s;
        if t <= names * 0.7 {
            0.88
        } else if t <= names {
            0.88 - 0.18 * (t - names * 0.7) / (names * 0.3)
        } else {
            (0.7 * (1.0 - t) / (1.0 - names)).max(0.0)
        }
    };
    let layers = scrim_layers(&scene, &theme, a);
    assert!((scrim_at(&layers, 0.0) - 0.88).abs() < 0.005);
    // Each place is under one layer, but within a quarter of a physical px of an edge, where
    // two meet so their antialiasing leaves no seam.
    let edges: Vec<f32> = layers.iter().flat_map(|(o, h, _)| [*o, *h]).collect();
    let mut r = 0.05;
    while r < s + 2.0 {
        let seam = edges.iter().any(|e| (r - e).abs() < 0.2);
        let under = layers.iter().filter(|(o, h, _)| r >= *h && r < *o).count();
        assert!(seam || under == usize::from(r < s), "{under} layers at {r}");
        let got = scrim_at(&layers, r);
        assert!(
            seam || (got - want(r)).abs() < 0.08,
            "{got} at {r}, not {}",
            want(r)
        );
        r += 0.1;
    }
    assert_eq!(scrim_at(&layers, s + 0.01), 0.0, "nothing past it");
    let overlaps = edges
        .iter()
        .filter(|e| **e > 0.0)
        .filter(|e| {
            layers
                .iter()
                .filter(|(o, h, _)| **e >= *h && **e < *o)
                .count()
                > 1
        })
        .count();
    assert!(overlaps > 0, "the rings overlap a little at their edges");
    // No disc and no wedge: what else is filled is the pointer's dot and the reticle's.
    for p in paths(&scene) {
        if let Some((colour, alpha)) = p.fill
            && colour != theme.surface
        {
            let b = p.path.bounds().unwrap();
            assert!(b.w <= 2.0 * art::DOT + 1e-3, "a fill of {b:?}");
            assert!(alpha == 1.0 && (colour == theme.strong || colour == theme.dial_accent));
        }
        if p.fill.is_some_and(|f| f.0 == theme.surface) {
            assert!(p.fill.unwrap().1 <= alpha::SCRIM && p.stroke.is_none());
        }
    }
    // The ring and the dead zone dotted in the dial's grey: round dots 1.3 px across, 3 apart.
    let rings: Vec<(f32, f32)> = paths(&scene)
        .iter()
        .filter_map(|p| {
            let s = p.stroke?;
            (s.dash == Some((0.1, 3.0))).then(|| {
                assert_eq!(
                    (s.width, s.cap, solid(&s.paint).0),
                    (1.3, Cap::Round, theme.dial)
                );
                (p.path.bounds().unwrap().w / 2.0, solid(&s.paint).1)
            })
        })
        .collect();
    assert!(
        rings
            .iter()
            .any(|(r, a)| close(*r, 25.0) && *a == alpha::RING),
        "{rings:?}"
    );
    assert!(
        rings
            .iter()
            .any(|(r, a)| close(*r, 10.0) && *a == alpha::DEAD),
        "{rings:?}"
    );
    // A tick at each sector's edge: eight, across the ring, 4 px either side.
    let ticks = paths(&scene)
        .into_iter()
        .find(|p| p.path.verbs.len() == 16 && p.stroke.is_some())
        .expect("the ticks");
    for (k, pair) in ends(ticks.path).chunks(2).enumerate() {
        let deg = (k as f32 - 0.5) * 45.0;
        assert!(
            close(dist(pair[0], out(a, deg, 21.0)), 0.0)
                && close(dist(pair[1], out(a, deg, 29.0)), 0.0)
        );
    }
    // Nothing hovered: no stretch, no chip, and the names all in the dial's grey.
    let accent_strokes = paths(&scene)
        .iter()
        .filter(|p| {
            p.stroke
                .is_some_and(|s| solid(&s.paint).0 == theme.dial_accent && !s.hairline)
        })
        .count();
    assert_eq!(accent_strokes, 0);
    let ts = texts(&scene);
    for name in NAMES {
        let t = text(&ts, name);
        assert_eq!(
            (t.color, t.alpha, t.spec.weight, t.anchor),
            (theme.dial, 1.0, Weight::Regular, Anchor::Middle)
        );
        assert_eq!(t.spec.size, 6.0, "0.6 of a cell height");
    }
    assert_eq!(ts.len(), 8, "no readout with nothing to say");
}

#[test]
fn hovering_lights_the_stretch_and_the_chip_in_the_accent_and_the_needle_ends_in_its_dot() {
    let theme = Theme::mono_dark();
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let mut d = DialState::open(a, CELL);
    let p = out(a, 40.0, 20.0);
    d.moved(p, &spec);
    assert_eq!(d.hovered(), Some(sector::COMPARE));
    let (scene, l) = drawn(&d, &spec, &theme);
    // Compare's stretch of the ring, tick to tick, 3 px in the accent, cut square.
    let stretch = paths(&scene)
        .into_iter()
        .find(|p| p.stroke.is_some_and(|s| s.width == art::THICK))
        .expect("the stretch");
    let st = stretch.stroke.unwrap();
    assert_eq!(
        (solid(&st.paint), st.cap),
        ((theme.dial_accent, 1.0), Cap::Butt)
    );
    let pts = ends(stretch.path);
    assert!(pts.iter().all(|q| close(dist(*q, a), 25.0)), "on the ring");
    assert!(close(dist(pts[0], out(a, 22.5, 25.0)), 0.0));
    assert!(close(dist(*pts.last().unwrap(), out(a, 67.5, 25.0)), 0.0));
    // Its name a chip: the accent, rounded, with the surface's colour on it, semibold.
    let chip = l.chip.unwrap();
    assert_eq!(chip, l.names[1]);
    let ground = paths(&scene)
        .into_iter()
        .find(|p| p.fill == Some((theme.dial_accent, 1.0)) && bounded(p.path, chip))
        .expect("the chip's ground");
    assert!(
        ground
            .path
            .verbs
            .iter()
            .any(|v| matches!(v, Verb::Cubic(..))),
        "rounded"
    );
    let ts = texts(&scene);
    let name = ts
        .iter()
        .find(|t| t.text == "compare" && t.color == theme.surface)
        .expect("on the chip");
    assert_eq!((name.spec.weight, name.alpha), (Weight::Semibold, 1.0));
    assert!(close(name.x, chip.x + chip.w / 2.0) && name.y > chip.y && name.y < chip.bottom());
    assert!(
        ts.iter().filter(|t| t.text == "compare").count() == 2,
        "and the readout's"
    );
    assert_eq!(text(&ts, "detail").color, theme.dial, "detail isn't a chip");
    // The needle, dotted in the grey, from the reticle's edge to the dot, the accent's.
    let needle = paths(&scene)
        .into_iter()
        .find(|p| {
            p.stroke
                .is_some_and(|s| s.dash.is_some() && solid(&s.paint).1 == alpha::NEEDLE)
        })
        .expect("the needle");
    let e = ends(needle.path);
    assert!(
        close(dist(e[0], a), art::RETICLE) && close(dist(e[1], p), art::DOT),
        "{e:?}"
    );
    let dot = paths(&scene).into_iter().last().unwrap();
    assert_eq!(dot.fill, Some((theme.dial_accent, 1.0)), "the dot, last");
    let b = dot.path.bounds().unwrap();
    assert!(
        close(b.x + b.w / 2.0, p.0) && close(b.y + b.h / 2.0, p.1) && close(b.w, 2.0 * art::DOT)
    );
    // A faint crosshair through the pointer, in the accent, across the dial's square.
    let cross = paths(&scene)
        .into_iter()
        .find(|p| p.stroke.is_some_and(|s| s.hairline))
        .expect("the crosshair");
    assert_eq!(
        solid(&cross.stroke.unwrap().paint),
        (theme.dial_accent, alpha::CROSS)
    );
    let bounds = d.bounds(&l);
    let e = ends(cross.path);
    assert_eq!(
        e,
        [
            (bounds.x, p.1),
            (bounds.right(), p.1),
            (p.0, bounds.y),
            (p.0, bounds.bottom())
        ]
    );
    // The readout under it all: the sector in the grey, not dimmed.
    let readout = ts
        .iter()
        .find(|t| t.text == "compare" && t.color == theme.dial)
        .unwrap();
    assert_eq!((readout.alpha, readout.anchor), (1.0, Anchor::Start));
    let r = l.readout.unwrap();
    assert!(close(readout.x, r.x) && readout.y > r.y && readout.y < r.bottom());
}

#[test]
fn stage_two_draws_the_arc_its_words_and_a_solid_needle_the_picked_one_marked() {
    let theme = Theme::mono_dark();
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let mut d = on_arc(a, CELL, sector::VIEW, 2, &spec);
    let p = out(a, 175.0, 55.0);
    d.moved(p, &spec);
    assert_eq!(d.stage(), Stage::Choice(2));
    let (scene, l) = drawn(&d, &spec, &theme);
    // Five segments on the arc, 24h's thick in the accent, the rest in the grey.
    let on_arc: Vec<Path> = paths(&scene)
        .into_iter()
        .filter(|p| p.stroke.is_some() && ends(p.path).iter().all(|q| close(dist(*q, a), 75.0)))
        .collect();
    assert_eq!(on_arc.len(), 5);
    for (k, seg) in on_arc.iter().enumerate() {
        let s = seg.stroke.unwrap();
        let from = 130.0 + 20.0 * k as f32;
        assert!(
            close(dist(ends(seg.path)[0], out(a, from, 75.0)), 0.0),
            "{k}"
        );
        let want = if k == 2 {
            (art::THICK, theme.dial_accent)
        } else {
            (1.5, theme.dial)
        };
        assert_eq!(
            (s.width, solid(&s.paint).0, s.cap),
            (want.0, want.1, Cap::Butt),
            "{k}"
        );
    }
    // The guide, faint, and the ring dimmed.
    assert!(paths(&scene).iter().any(|p| {
        p.stroke
            .is_some_and(|s| solid(&s.paint) == (theme.dial, alpha::GUIDE))
            && ends(p.path).iter().all(|q| close(dist(*q, a), 42.5))
    }));
    assert!(paths(&scene).iter().any(|p| {
        p.stroke
            .is_some_and(|s| s.dash.is_some() && solid(&s.paint).1 == alpha::RING_DIM)
    }));
    // The words, the picked one strong and semibold; each in its box.
    let ts = texts(&scene);
    for (k, b) in &l.labels {
        let word = VIEW[usize::from(*k)];
        let t = ts
            .iter()
            .find(|t| t.text == word && t.anchor == Anchor::Middle)
            .unwrap();
        let want = if *k == 2 {
            (theme.strong, Weight::Semibold)
        } else {
            (theme.dial, Weight::Regular)
        };
        assert_eq!((t.color, t.spec.weight), want, "{word}");
        let w = FixedMeasure.width(word, t.spec);
        assert!(
            t.x - w / 2.0 >= b.x - 1e-3 && t.x + w / 2.0 <= b.right() + 1e-3,
            "{word} in {b:?}"
        );
        assert!(
            t.y - 0.8 * t.spec.size >= b.y - 1e-3 && t.y + 0.2 * t.spec.size <= b.bottom() + 1e-3
        );
    }
    // The other names faint, the chip past the arc.
    for (j, name) in NAMES
        .iter()
        .enumerate()
        .filter(|(j, _)| l.shown[*j] && *j != 4)
    {
        assert_eq!(text(&ts, name).alpha, alpha::FAINT, "{j}");
    }
    assert!(
        ts.iter().all(|t| t.text != "table" && t.text != "export"),
        "given way"
    );
    let chip = ts
        .iter()
        .find(|t| t.text == "view" && t.color == theme.surface)
        .unwrap();
    assert!(dist((chip.x, chip.y), a) > 75.0 + art::CAP);
    // The needle solid now, in the strong grey, with a tick where it crosses the guide.
    let needle = paths(&scene)
        .into_iter()
        .find(|p| {
            p.stroke.is_some_and(|s| {
                solid(&s.paint) == (theme.strong, alpha::NEEDLE) && s.dash.is_none()
            }) && ends(p.path).len() == 4
        })
        .expect("the needle with its tick");
    let e = ends(needle.path);
    let g = out(a, angle(a, p), 42.5);
    assert!(close(
        dist(((e[2].0 + e[3].0) / 2.0, (e[2].1 + e[3].1) / 2.0), g),
        0.0
    ));
    assert!(close(dist(e[2], e[3]), 2.0 * art::TICK * 0.75));
    // The readout: `view:` in the grey, 24h in the accent.
    let r: Vec<&Text> = ts.iter().filter(|t| t.anchor == Anchor::Start).collect();
    let said: Vec<(&str, Color)> = r.iter().map(|t| (t.text.as_str(), t.color)).collect();
    assert_eq!(said, [("view:", theme.dial), ("24h", theme.dial_accent)]);
    assert!(r[1].x > r[0].x + FixedMeasure.width("view:", r[0].spec));
}

fn angle(a: (f32, f32), p: (f32, f32)) -> f32 {
    die_fig::dial::angle_of(p.0 - a.0, p.1 - a.1)
}

#[test]
fn a_sector_that_doesnt_apply_is_dimmed_and_lights_nothing() {
    let theme = Theme::mono_dark();
    let mut spec = DialSpec::chart();
    spec.sectors[usize::from(sector::COMPARE)].enabled = false;
    spec.sectors[usize::from(sector::EXPORT)].enabled = false;
    let a = (200.0, 150.0);
    let mut d = DialState::open(a, CELL);
    let (scene, _) = drawn(&d, &spec, &theme);
    let ts = texts(&scene);
    assert_eq!(text(&ts, "compare").alpha, alpha::DIM);
    assert_eq!(text(&ts, "export").alpha, alpha::DIM);
    assert_eq!(text(&ts, "only").alpha, 1.0);
    // Hovered: its stretch in the grey, no chip, the readout its name dimmed.
    d.moved(out(a, 45.0, 20.0), &spec);
    let (scene, l) = drawn(&d, &spec, &theme);
    assert_eq!(l.chip, None);
    let stretch = paths(&scene)
        .into_iter()
        .find(|p| p.stroke.is_some_and(|s| s.width == art::THICK))
        .unwrap();
    assert_eq!(
        solid(&stretch.stroke.unwrap().paint),
        (theme.dial, alpha::RING)
    );
    let accent_fills = paths(&scene)
        .iter()
        .filter(|p| p.fill.is_some_and(|f| f.0 == theme.dial_accent))
        .count();
    assert_eq!(accent_fills, 1, "the pointer's dot alone");
    let ts = texts(&scene);
    assert!(ts.iter().all(|t| t.color != theme.surface), "no chip");
    let readout = ts.iter().find(|t| t.anchor == Anchor::Start).unwrap();
    assert_eq!(
        (readout.text.as_str(), readout.color, readout.alpha),
        ("compare", theme.dial, alpha::FAINT)
    );
    assert_eq!(d.release(&spec), DialPick::None);
}

#[test]
fn the_regions_say_what_each_place_is() {
    let theme = Theme::mono_dark();
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let d = DialState::open(a, CELL);
    let (scene, l) = drawn(&d, &spec, &theme);
    assert_eq!(scene.hit(a.0, a.1), Some(&Hit::DialCentre));
    for (i, b) in l.names.iter().enumerate() {
        assert_eq!(
            scene.hit(b.x + b.w / 2.0, b.y + b.h / 2.0),
            Some(&Hit::DialSector(i as u8))
        );
    }
    // The dead centre's region is inside the dead zone, so no sector is picked there.
    let centre = scene
        .regions
        .iter()
        .find(|r| r.hit == Hit::DialCentre)
        .unwrap()
        .rect;
    for q in [(centre.x, centre.y), (centre.right(), centre.bottom())] {
        assert!(dist(q, a) <= d.dead() + 1e-3);
    }
    // Stage two: each word, the chip, and no region for a name that gave way.
    let d = on_arc(a, CELL, sector::VIEW, 2, &spec);
    let (scene, l) = drawn(&d, &spec, &theme);
    for (k, b) in &l.labels {
        assert_eq!(
            scene.hit(b.x + b.w / 2.0, b.y + b.h / 2.0),
            Some(&Hit::DialStage2(*k))
        );
    }
    let chip = l.chip.unwrap();
    let hit = scene.hit(chip.x + chip.w / 2.0, chip.y + chip.h / 2.0);
    assert_eq!(hit, Some(&Hit::DialSector(sector::VIEW)));
    let sectors: Vec<u8> = scene
        .regions
        .iter()
        .filter_map(|r| match r.hit {
            Hit::DialSector(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(sectors, [0, 1, 2, 4, 6, 7], "table and export gave way");
}

#[test]
fn the_dial_takes_its_colours_from_the_theme_and_its_text_never_a_series_colour() {
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let mut theme = Theme::light();
    theme.dial = Color(0x55, 0x55, 0x55);
    theme.dial_accent = Color(0x00, 0x88, 0xff);
    let mut d = on_arc(a, CELL, sector::EXPORT, 1, &spec);
    d.set_target("api · 06:05 · 642");
    let (scene, l) = drawn(&d, &spec, &theme);
    let chip = l.chip.unwrap();
    assert!(
        paths(&scene)
            .iter()
            .any(|p| p.fill == Some((theme.dial_accent, 1.0)) && bounded(p.path, chip))
    );
    assert!(
        paths(&scene)
            .iter()
            .any(|p| p.fill.is_some_and(|f| f.0 == theme.surface)),
        "the light scrim"
    );
    let ts = texts(&scene);
    assert_eq!(text(&ts, "detail").color, theme.dial);
    // Every text is in the dial's colours, the strong grey or the surface's on the chip.
    for (st, theme) in [
        (d.clone(), theme.clone()),
        (DialState::open(a, CELL), Theme::mono_dark()),
    ] {
        let (scene, _) = drawn(&st, &spec, &theme);
        for t in texts(&scene) {
            assert!(!theme.series.contains(&t.color), "{t:?}");
            assert!(
                [theme.dial, theme.dial_accent, theme.strong, theme.surface].contains(&t.color),
                "{t:?}"
            );
        }
    }
}

#[test]
fn what_follows_the_pointer_stays_within_the_dials_bounds_and_the_rest_stays_still() {
    let theme = Theme::mono_dark();
    let spec = DialSpec::chart();
    let a = (200.0, 150.0);
    let area = (400.0, 300.0);
    let mut d = on_arc(a, CELL, sector::VIEW, 2, &spec);
    // Far past the dial's square, in the area and out of it: held, the angle picking.
    for p in [(900.0, -50.0), (-300.0, 400.0), (20.0, 290.0)] {
        d.moved(p, &spec);
        assert!(matches!(d.stage(), Stage::Choice(_)), "{p:?}");
        let l = layout(&d, &spec, area, &mut FixedMeasure);
        let mut live = Scene::new(area.0, area.1, 2.0);
        draw_live(&d, &theme, &l, &mut live);
        let bounds = d.bounds(&l).inflate(1.0);
        let parts = d.live_rects(&l);
        for p in paths(&live) {
            let half = p.stroke.map_or(0.0, |s| s.width / 2.0);
            let b = p.path.bounds().unwrap().inflate(half);
            assert!(
                b.x >= bounds.x
                    && b.right() <= bounds.right()
                    && b.y >= bounds.y
                    && b.bottom() <= bounds.bottom()
            );
            for q in ends(p.path) {
                assert!(
                    parts.iter().any(|r| r.inflate(1.0).contains(q.0, q.1)),
                    "{q:?} in {parts:?}"
                );
            }
        }
        assert!(
            live.items.iter().all(|i| matches!(i, Item::Path { .. })),
            "no text follows it"
        );
    }
    // A move within a choice changes what's still not at all; `draw` is the still part then the
    // live one.
    let mut d = on_arc(a, CELL, sector::VIEW, 2, &spec);
    let l = layout(&d, &spec, area, &mut FixedMeasure);
    let mut before = Scene::new(area.0, area.1, 2.0);
    draw_still(&d, &spec, &theme, &l, &mut before, &mut FixedMeasure);
    let was = d.still();
    d.moved(out(a, 183.0, 50.0), &spec);
    assert_eq!(d.still(), was);
    let l = layout(&d, &spec, area, &mut FixedMeasure);
    let mut after = Scene::new(area.0, area.1, 2.0);
    draw_still(&d, &spec, &theme, &l, &mut after, &mut FixedMeasure);
    assert_eq!(before, after);
    let mut live = Scene::new(area.0, area.1, 2.0);
    draw_live(&d, &theme, &l, &mut live);
    let mut whole = Scene::new(area.0, area.1, 2.0);
    draw(&d, &spec, &theme, &mut whole, &mut FixedMeasure);
    let mut both = after.items.clone();
    both.extend(live.items);
    assert_eq!(whole.items, both);
    // A move to another choice changes it.
    d.moved(out(a, 205.0, 50.0), &spec);
    assert_ne!(d.still(), was);
    // The still part keeps to the base's bounds.
    let base = d.base_bounds(&l).inflate(art::CAP);
    for p in paths(&after) {
        let b = p.path.bounds().unwrap();
        assert!(
            b.x >= base.x
                && b.right() <= base.right()
                && b.y >= base.y
                && b.bottom() <= base.bottom(),
            "{b:?}"
        );
    }
}
