//! Glyph substitutions read from a face's GSUB table: enough for tabular figures, a feature's
//! single substitutions. Only `fonts` calls it.
//!
//! ttf-parser's own GSUB reader is used, unlike kern.rs's GPOS bytes: a single substitution has
//! no device tables, which is where its GPOS reader goes wrong.

use ttf_parser::gsub::{SingleSubstitution, SubstitutionSubtable};
use ttf_parser::{Face, GlyphId, Tag};

/// The GSUB lookups under `feature` for the default language of `latn` (or `DFLT`), with the
/// feature variations at the face's coordinates applied.
pub(crate) fn lookups(face: &Face<'_>, feature: &[u8; 4]) -> Vec<u16> {
    let Some(gsub) = face.tables().gsub else {
        return vec![];
    };
    let script = [b"latn", b"DFLT"]
        .iter()
        .find_map(|t| gsub.scripts.find(Tag::from_bytes(t)))
        .or_else(|| gsub.scripts.get(0));
    let Some(lang) = script.and_then(|s| s.default_language.or_else(|| s.languages.get(0))) else {
        return vec![];
    };
    // A variable font may swap a feature's lookups at some coordinates.
    let variation = gsub
        .variations
        .and_then(|v| v.find_index(face.variation_coordinates()).map(|i| (v, i)));
    let tag = Tag::from_bytes(feature);
    let mut out = vec![];
    for index in lang.feature_indices {
        let Some(found) = gsub.features.get(index) else {
            continue;
        };
        if found.tag != tag {
            continue;
        }
        let found = variation
            .and_then(|(v, i)| v.find_substitute(index, i))
            .unwrap_or(found);
        out.extend(found.lookup_indices);
    }
    out
}

/// What the first single substitution among `lookups` that covers `glyph` makes of it: None if
/// none does.
pub(crate) fn single(face: &Face<'_>, lookups: &[u16], glyph: u16) -> Option<u16> {
    let gsub = face.tables().gsub?;
    for &index in lookups {
        let Some(lookup) = gsub.lookups.get(index) else {
            continue;
        };
        // An extension lookup (type 7) comes unwrapped from ttf-parser's parse.
        for sub in lookup.subtables.into_iter::<SubstitutionSubtable>() {
            let SubstitutionSubtable::Single(s) = sub else {
                continue;
            };
            match s {
                SingleSubstitution::Format1 { coverage, delta } => {
                    if coverage.get(GlyphId(glyph)).is_some() {
                        // Modulo 65536, as the spec has it.
                        return Some(glyph.wrapping_add(delta as u16));
                    }
                }
                SingleSubstitution::Format2 {
                    coverage,
                    substitutes,
                } => {
                    if let Some(i) = coverage.get(GlyphId(glyph)) {
                        return substitutes.get(i).map(|g| g.0);
                    }
                }
            }
        }
    }
    None
}
