//! Pair kerning and tracking, read the way CoreText applies them to the system font: the GPOS
//! `kern` feature's pair adjustments with their variation deltas, else the legacy `kern` table,
//! and the `trak` table's normal track.
//!
//! The pair adjustments are read from the GPOS table's bytes. ttf-parser 0.25.1 resolves a
//! format 1 pair's device offsets from the wrong base, so through its own reader those pairs
//! lose their variation deltas: the system font's digits and `. ` come out wrong at every
//! optical size but its default.

use std::cmp::Ordering;
use ttf_parser::{Face, GlyphId, Tag};

/// The GPOS lookups under `kern` for the default language of `latn` (or `DFLT`).
pub(super) fn lookups(face: &Face<'_>) -> Vec<u16> {
    let Some(gpos) = face.tables().gpos else {
        return vec![];
    };
    let script = [b"latn", b"DFLT"]
        .iter()
        .find_map(|t| gpos.scripts.find(Tag::from_bytes(t)))
        .or_else(|| gpos.scripts.get(0));
    let Some(lang) = script.and_then(|s| s.default_language.or_else(|| s.languages.get(0))) else {
        return vec![];
    };
    // A variable font may swap a feature's lookups at some coordinates.
    let variation = gpos
        .variations
        .and_then(|v| v.find_index(face.variation_coordinates()).map(|i| (v, i)));
    let kern = Tag::from_bytes(b"kern");
    let mut out = vec![];
    for index in lang.feature_indices {
        let Some(feature) = gpos.features.get(index) else {
            continue;
        };
        if feature.tag != kern {
            continue;
        }
        let feature = variation
            .and_then(|(v, i)| v.find_substitute(index, i))
            .unwrap_or(feature);
        out.extend(feature.lookup_indices);
    }
    out
}

/// `trak`'s normal track: (point size, tracking in font units), sizes ascending.
pub(super) fn track(face: &Face<'_>) -> Vec<(f32, f32)> {
    let Some(trak) = face.tables().trak else {
        return vec![];
    };
    let data = trak.horizontal;
    let Some(track) = data.tracks.into_iter().find(|t| t.value == 0.0) else {
        return vec![];
    };
    data.sizes
        .into_iter()
        .zip(track.values)
        .map(|(size, v)| (size.0, f32::from(v)))
        .collect()
}

/// The tracking a track adds to every advance at `points`, in font units: between the sizes
/// the table lists, as CoreText does.
pub(super) fn tracking(track: &[(f32, f32)], points: f32) -> f32 {
    let (Some(first), Some(last)) = (track.first(), track.last()) else {
        return 0.0;
    };
    if points <= first.0 {
        return first.1;
    }
    if points >= last.0 {
        return last.1;
    }
    let i = track.partition_point(|(size, _)| *size <= points);
    let (a, b) = (track[i - 1], track[i]);
    a.1 + (b.1 - a.1) * (points - a.0) / (b.0 - a.0)
}

/// A pair's kerning in font units: the GPOS lookups' if the face has the feature, else the
/// `kern` table's.
pub(super) fn pair(face: &Face<'_>, lookups: &[u16], left: u16, right: u16) -> f32 {
    if lookups.is_empty() {
        let Some(kern) = face.tables().kern else {
            return 0.0;
        };
        return kern
            .subtables
            .into_iter()
            .filter(|s| s.horizontal && !s.variable && !s.has_state_machine)
            .find_map(|s| s.glyphs_kerning(GlyphId(left), GlyphId(right)))
            .map_or(0.0, f32::from);
    }
    let Some(gpos) = face.raw_face().table(Tag::from_bytes(b"GPOS")) else {
        return 0.0;
    };
    let mut sum = 0.0;
    for &index in lookups {
        if let Some((advance, device)) = lookup(gpos, index, left, right) {
            let delta = device.and_then(|(outer, inner)| {
                let gdef = face.tables().gdef?;
                gdef.glyph_variation_delta(outer, inner, face.variation_coordinates())
            });
            sum += f32::from(advance) + delta.unwrap_or(0.0);
        }
    }
    sum
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at.checked_add(2)?)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at.checked_add(4)?)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The record among `count` sorted ones of `len` bytes from `at` that `key` calls Equal. `key`
/// says how the record it's given sorts against what's sought.
fn search(
    d: &[u8],
    at: usize,
    count: usize,
    len: usize,
    key: impl Fn(usize) -> Option<Ordering>,
) -> Option<usize> {
    let (mut lo, mut hi) = (0, count);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = at + mid * len;
        d.get(rec..rec + len)?;
        match key(rec)? {
            Ordering::Equal => return Some(rec),
            Ordering::Less => lo = mid + 1,
            Ordering::Greater => hi = mid,
        }
    }
    None
}

/// How the range record at `r` (start, end, ...) sorts against glyph `g`.
fn range(d: &[u8], r: usize, g: u16) -> Option<Ordering> {
    let (start, end) = (u16_at(d, r)?, u16_at(d, r + 2)?);
    Some(if g < start {
        Ordering::Greater
    } else if g > end {
        Ordering::Less
    } else {
        Ordering::Equal
    })
}

/// A glyph's index in the coverage table at `at`.
fn coverage(d: &[u8], at: usize, g: u16) -> Option<u16> {
    let count = usize::from(u16_at(d, at + 2)?);
    match u16_at(d, at)? {
        1 => {
            let rec = search(d, at + 4, count, 2, |r| Some(u16_at(d, r)?.cmp(&g)))?;
            Some(((rec - at - 4) / 2) as u16)
        }
        2 => {
            let rec = search(d, at + 4, count, 6, |r| range(d, r, g))?;
            Some(u16_at(d, rec + 4)?.wrapping_add(g - u16_at(d, rec)?))
        }
        _ => None,
    }
}

/// A glyph's class in the class definition at `at`: 0 if it's in none.
fn class(d: &[u8], at: usize, g: u16) -> u16 {
    let read = || match u16_at(d, at)? {
        1 => {
            let (start, count) = (u16_at(d, at + 2)?, u16_at(d, at + 4)?);
            let i = g.checked_sub(start).filter(|i| *i < count)?;
            u16_at(d, at + 6 + usize::from(i) * 2)
        }
        2 => {
            let count = usize::from(u16_at(d, at + 2)?);
            let rec = search(d, at + 4, count, 6, |r| range(d, r, g))?;
            u16_at(d, rec + 4)
        }
        _ => None,
    };
    read().unwrap_or(0)
}

/// The bytes a value record of `format` takes.
fn value_len(format: u16) -> usize {
    (format & 0xff).count_ones() as usize * 2
}

/// The x advance of the value record at `at`, and its variation index if it has one. Device
/// offsets are from `base`: the pair set for format 1, the subtable for format 2.
fn x_advance(d: &[u8], base: usize, at: usize, format: u16) -> Option<(i16, Option<(u16, u16)>)> {
    // A field's place: two bytes for each lower bit set.
    let field = |bit: u16| at + (format & (bit - 1)).count_ones() as usize * 2;
    let advance = if format & 0x04 != 0 {
        u16_at(d, field(0x04))? as i16
    } else {
        0
    };
    let mut device = None;
    if format & 0x40 != 0 {
        let offset = usize::from(u16_at(d, field(0x40))?);
        // 0x8000 marks a variation index; the other formats are hinting deltas by ppem.
        if offset != 0 && u16_at(d, base + offset + 4)? == 0x8000 {
            device = Some((u16_at(d, base + offset)?, u16_at(d, base + offset + 2)?));
        }
    }
    Some((advance, device))
}

/// What lookup `index` of the GPOS table `d` does to the first glyph of the pair: its first
/// subtable that takes the pair, as shaping applies it. The x advance adjustment and, if it has
/// one, its variation index (outer, inner).
fn lookup(d: &[u8], index: u16, left: u16, right: u16) -> Option<(i16, Option<(u16, u16)>)> {
    let list = usize::from(u16_at(d, 8)?);
    let lookup = list + usize::from(u16_at(d, list + 2 + usize::from(index) * 2)?);
    let lookup_kind = u16_at(d, lookup)?;
    for i in 0..usize::from(u16_at(d, lookup + 4)?) {
        let mut sub = lookup + usize::from(u16_at(d, lookup + 6 + i * 2)?);
        let mut kind = lookup_kind;
        if kind == 9 {
            // An extension: the real kind, and a 32-bit offset to the real subtable.
            kind = u16_at(d, sub + 2)?;
            sub += u32_at(d, sub + 4)? as usize;
        }
        if kind != 2 {
            continue;
        }
        let Some(covered) = coverage(d, sub + usize::from(u16_at(d, sub + 2)?), left) else {
            continue;
        };
        let (f1, f2) = (u16_at(d, sub + 4)?, u16_at(d, sub + 6)?);
        match u16_at(d, sub)? {
            1 => {
                let set = sub + usize::from(u16_at(d, sub + 10 + usize::from(covered) * 2)?);
                let count = usize::from(u16_at(d, set)?);
                let len = 2 + value_len(f1) + value_len(f2);
                let hit = search(d, set + 2, count, len, |r| Some(u16_at(d, r)?.cmp(&right)));
                if let Some(rec) = hit {
                    return x_advance(d, set, rec + 2, f1);
                }
            }
            2 => {
                let c1 = class(d, sub + usize::from(u16_at(d, sub + 8)?), left);
                let c2 = class(d, sub + usize::from(u16_at(d, sub + 10)?), right);
                let (n1, n2) = (u16_at(d, sub + 12)?, u16_at(d, sub + 14)?);
                if c1 >= n1 || c2 >= n2 {
                    return None;
                }
                let len = value_len(f1) + value_len(f2);
                let rec = sub + 16 + (usize::from(c1) * usize::from(n2) + usize::from(c2)) * len;
                return x_advance(d, sub, rec, f1);
            }
            _ => {}
        }
    }
    None
}
