//! `:sor[t][!] [/pattern/] [flags]`, as your Evil's `evil-ex-sort`: the range's lines, the whole
//! text if none is given, in the order of their characters, and with `!` the other way. Lines that
//! tie keep their order either way, as Emacs's `sort-subr` keeps them.
//!
//! The flags are your Evil's: `i` ignores case, `u` keeps the first of lines that are the same
//! once sorted, `n` sorts by the first decimal number, `x` hexadecimal (`0x` and `-` allowed), `o`
//! octal and `b` binary, a line with no number coming first. A pattern skips what it matches, so
//! the rest of the line is sorted on, or with `r` that match is, and a line it doesn't match sorts
//! as empty. The pattern is Rust's (D4), and ignores case as Emacs's `case-fold-search` does. Your
//! Evil's empty pattern means none: the last search is `/`'s isearch, which it doesn't see.

use crate::each;
use crate::lines;
use crate::substitute::Lines;
use crate::text::rest_pos;
use die_core::{Ctx, Error, Origin, Pattern, Result, TextBuf};
use std::cmp::Ordering;
use std::ops::Range;

#[derive(Default)]
struct Flags {
    fold: bool,
    unique: bool,
    radix: Option<u32>,
    of_match: bool,
    pattern: Option<Pattern>,
}

/// What a line sorts on.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Number(i128),
    Text(String),
}

/// `:sort`, `!` being `reverse`, with what follows the name.
pub fn sort(ctx: &mut Ctx, lines: &Lines, reverse: bool, rest: &str) -> Result {
    let flags = flags(rest)?;
    let lines = match lines {
        Lines::Cursor => &Lines::All,
        lines => lines,
    };
    let r = lines::taken(ctx, lines)?;
    let t = ctx.text_mut(r.b)?;
    let (beg, end) = (t.line_start(r.first), line_after(t, r.last));
    let text = t.rope().byte_slice(beg..end).to_string();
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let mut rows: Vec<(Range<usize>, &str)> = vec![];
    let mut at = beg;
    for row in body.split('\n') {
        rows.push((at..at + row.len(), row));
        at += row.len() + 1;
    }
    let keys: Vec<Key> = rows
        .iter()
        .map(|(r, _)| key(t, r.clone(), &flags))
        .collect();
    let by = |a: &usize, b: &usize| match (&keys[*a], &keys[*b]) {
        (Key::Text(x), Key::Text(y)) if flags.fold => folded(x, y),
        (x, y) => x.cmp(y),
    };
    let mut order: Vec<usize> = (0..rows.len()).collect();
    // `sort-subr` reverses the lines, sorts them and reverses them again.
    if reverse {
        order.reverse();
    }
    order.sort_by(by);
    if reverse {
        order.reverse();
    }
    let mut kept: Vec<&str> = vec![];
    for i in order {
        let row = rows[i].1;
        let same = |p: &&str| match flags.fold {
            true => folded(p, row) == Ordering::Equal,
            false => *p == row,
        };
        if !(flags.unique && kept.last().is_some_and(same)) {
            kept.push(row);
        }
    }
    let mut new = kept.join("\n");
    if text.ends_with('\n') {
        new.push('\n');
    }
    // Where your Evil leaves the cursor, and where its `u` goes back to.
    t.set_cursor(r.w, beg);
    t.tx(Origin::User).replace(beg..end, new).commit()?;
    t.set_cursor(r.w, rest_pos(t, beg));
    Ok(())
}

/// Where the line after `line` starts, or the text's end.
fn line_after(t: &TextBuf, line: usize) -> usize {
    match line + 1 < t.line_count() {
        true => t.line_start(line + 1),
        false => t.len(),
    }
}

/// The two in the order of their characters, each in lower case.
fn folded(a: &str, b: &str) -> Ordering {
    let low = |c: char| c.to_lowercase().next().unwrap_or(c);
    a.chars().map(low).cmp(b.chars().map(low))
}

/// What follows `:sort`: flags alone, or else a pattern between delimiters and then flags.
fn flags(rest: &str) -> Result<Flags> {
    let rest = rest.trim();
    let (pattern, options) = match rest.chars().next() {
        Some(c) if !"inxobur".contains(c) => {
            let (pattern, after) = each::parts(rest);
            (Some(pattern), after.unwrap_or(""))
        }
        _ => (None, rest),
    };
    let mut f = Flags::default();
    for c in options.chars() {
        match c {
            'i' => f.fold = true,
            'u' => f.unique = true,
            'r' => f.of_match = true,
            'b' => f.radix = Some(2),
            'o' => f.radix = Some(8),
            'n' => f.radix = Some(10),
            'x' => f.radix = Some(16),
            ' ' => {}
            c => return Err(Error::Msg(format!("Invalid sort option `{c}'"))),
        }
    }
    f.pattern = match pattern.filter(|p| !p.is_empty()) {
        Some(p) => Some(Pattern::new(&p, true)?),
        None => None,
    };
    Ok(f)
}

/// The key of the line at `row`.
fn key(t: &TextBuf, row: Range<usize>, f: &Flags) -> Key {
    let mut part = row.clone();
    if let Some(p) = &f.pattern {
        match t.first_match_in(p, row.clone()) {
            Some(m) if f.of_match => part = m,
            Some(m) => part.start = m.end,
            None => part.start = row.end,
        }
    }
    let text = t.rope().byte_slice(part).to_string();
    match f.radix {
        Some(radix) => Key::Number(number(&text, radix).unwrap_or(i128::MIN)),
        None => Key::Text(text),
    }
}

/// The first number in `s` in this radix, as your Evil's patterns find it: `-?[0-9]+` in decimal,
/// `-?(0x)?[0-9a-f]+` in hexadecimal, any case, and digits alone in octal and binary.
fn number(s: &str, radix: u32) -> Option<i128> {
    let digit = |c: char| c.is_digit(radix);
    let signed = matches!(radix, 10 | 16);
    for (i, c) in s.char_indices() {
        let (minus, after) = match c {
            '-' if signed => (true, &s[i + 1..]),
            c if digit(c) => (false, &s[i..]),
            _ => continue,
        };
        let hex = (radix == 16).then(|| after.strip_prefix("0x").or(after.strip_prefix("0X")));
        let digits = match hex.flatten() {
            Some(rest) if rest.starts_with(digit) => rest,
            _ => after,
        };
        let len = digits.find(|c: char| !digit(c)).unwrap_or(digits.len());
        if len == 0 {
            continue;
        }
        let value = digits[..len].chars().fold(0i128, |n, c| {
            let d = i128::from(c.to_digit(radix).unwrap_or(0));
            n.saturating_mul(i128::from(radix)).saturating_add(d)
        });
        return Some(if minus { -value } else { value });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_found_as_your_evils_patterns_find_them() {
        assert_eq!(number("x10", 10), Some(10));
        assert_eq!(number("a-5", 10), Some(-5));
        assert_eq!(number("a-b", 10), None);
        assert_eq!(number("0x1F", 16), Some(31));
        assert_eq!(number("-0x2", 16), Some(-2));
        assert_eq!(number("0xg", 16), Some(0));
        assert_eq!(number("a0X1F", 16), Some(160));
        assert_eq!(number("x-5", 16), Some(-5));
        assert_eq!(number("8", 8), None);
        assert_eq!(number("17", 8), Some(15));
        assert_eq!(number("2101", 2), Some(5));
        assert_eq!(number("9".repeat(60).as_str(), 10), Some(i128::MAX));
    }
}
