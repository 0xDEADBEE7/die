//! Reading text the way die-vim's motions need: characters and their classes,
//! lines, and where the cursor may rest in normal state.

use die_core::{Dir, TextBuf};

/// A character's position in the text, stepping by characters.
pub struct Chars<'a> {
    t: &'a TextBuf,
    ci: usize,
}

impl<'a> Chars<'a> {
    pub fn at(t: &'a TextBuf, pos: usize) -> Self {
        Chars {
            t,
            ci: t.rope().byte_to_char(pos.min(t.len())),
        }
    }
    /// The byte position.
    pub fn pos(&self) -> usize {
        self.t.rope().char_to_byte(self.ci)
    }
    /// The character here; None at the end of the text.
    pub fn ch(&self) -> Option<char> {
        self.t.rope().get_char(self.ci)
    }
    /// The character before this one.
    pub fn prev_ch(&self) -> Option<char> {
        self.ci
            .checked_sub(1)
            .and_then(|i| self.t.rope().get_char(i))
    }
    /// Step forward; false at the end of the text.
    pub fn fwd(&mut self) -> bool {
        let ok = self.ci < self.t.rope().len_chars();
        if ok {
            self.ci += 1;
        }
        ok
    }
    /// Step back; false at the start of the text.
    pub fn back(&mut self) -> bool {
        let ok = self.ci > 0;
        if ok {
            self.ci -= 1;
        }
        ok
    }
    /// At the start of a line: the text's start, or just after a newline.
    pub fn at_line_start(&self) -> bool {
        matches!(self.prev_ch(), None | Some('\n'))
    }
}

/// What a word motion sees a character as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Blank,
    Newline,
    /// A run of these is a word; a change of script ends it, as in Emacs.
    Word(Script),
    /// Anything else that isn't blank: `_`, `::`, `->` and `.` are each a word of
    /// their own, as in Evil in fundamental-mode.
    Punct,
}

/// The scripts Emacs separates words between. Everything else is `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Han,
    Kana,
    Hangul,
    Other,
}

/// A character's class. With `big` (WORD motions), every non-blank is one class.
pub fn class(c: char, big: bool) -> Class {
    match c {
        '\n' => Class::Newline,
        ' ' | '\t' | '\r' | '\u{c}' => Class::Blank,
        _ if big => Class::Punct,
        _ if c.is_alphanumeric() || combining(c) => Class::Word(script(c)),
        _ => Class::Punct,
    }
}

/// The main blocks of combining marks, which `is_alphanumeric` leaves out. Emacs
/// gives them word syntax, so "café" spelled with U+0301 is one word.
fn combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}' | '\u{fe20}'..='\u{fe2f}')
}

fn script(c: char) -> Script {
    match c as u32 {
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x3ffff => Script::Han,
        0x3040..=0x30ff | 0x31f0..=0x31ff | 0xff66..=0xff9f => Script::Kana,
        0x1100..=0x11ff | 0x3130..=0x318f | 0xac00..=0xd7af => Script::Hangul,
        _ => Script::Other,
    }
}

/// The last line the cursor can be on. A final newline ends the last line; it
/// doesn't start another (a deliberate deviation from Evil, deviations.json).
pub fn last_line(t: &TextBuf) -> usize {
    let n = t.line_count();
    if !t.is_empty() && t.rope().byte(t.len() - 1) == b'\n' {
        n.saturating_sub(2)
    } else {
        n.saturating_sub(1)
    }
}

/// Where `line` ends: its newline, or the end of the text.
pub fn line_end(t: &TextBuf, line: usize) -> usize {
    if line + 1 < t.line_count() {
        t.line_start(line + 1) - 1
    } else {
        t.len()
    }
}

/// Whether `line` holds nothing but spaces and tabs.
pub fn blank_line(t: &TextBuf, line: usize) -> bool {
    let (s, e) = (t.line_start(line), line_end(t, line));
    t.rope()
        .byte_slice(s..e)
        .chars()
        .all(|c| c == ' ' || c == '\t')
}

/// Where `line`'s leading spaces and tabs end.
pub fn indent_end(t: &TextBuf, line: usize) -> usize {
    let s = t.line_start(line);
    let ws = t
        .rope()
        .byte_slice(s..line_end(t, line))
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count();
    s + ws
}

/// The width of `line`'s leading spaces and tabs, in display columns.
pub fn indentation(t: &TextBuf, line: usize) -> usize {
    t.display_col(indent_end(t, line))
}

/// The first cursor position on `line` at or after display column `col`, else the
/// line's end. A column inside a tab or a wide character lands after it, as
/// Emacs's `move-to-column` does.
pub fn pos_at_col(t: &TextBuf, line: usize, col: usize) -> usize {
    let (s, e) = (t.line_start(line), line_end(t, line));
    t.display_line(s)
        .stops
        .iter()
        .filter(|st| st.pos >= s && st.pos <= e)
        .find(|st| st.vcol >= col)
        .map_or(e, |st| st.pos)
}

/// The nearest place the cursor may rest in normal state: on a character of a
/// line up to the last line, and on a newline only when its line is empty.
pub fn normal_pos(t: &TextBuf, pos: usize) -> usize {
    let line = t.line_of(pos).min(last_line(t));
    let (s, e) = (t.line_start(line), line_end(t, line));
    let p = pos.clamp(s, e);
    if p == e && e > s {
        t.snap(e - 1, Some(Dir::Backward))
    } else {
        t.snap(p, None)
    }
}

/// The next cursor position after `pos`, if it isn't past the end of the text.
pub fn next_stop(t: &TextBuf, pos: usize) -> Option<usize> {
    let mut c = Chars::at(t, pos);
    c.ch()?;
    c.fwd();
    Some(t.snap(c.pos(), Some(Dir::Forward)))
}

/// The cursor position before `pos`, if there is one.
pub fn prev_stop(t: &TextBuf, pos: usize) -> Option<usize> {
    let mut c = Chars::at(t, pos);
    c.back().then(|| t.snap(c.pos(), Some(Dir::Backward)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_classes() {
        // In fundamental-mode `_` isn't a word character, so `foo_bar` is three words.
        assert_eq!(class('_', false), Class::Punct);
        assert_eq!(class(':', false), class('>', false));
        assert_eq!(class('a', false), class('é', false));
        assert_eq!(
            class('e', false),
            class('\u{301}', false),
            "a combining acute"
        );
        // A change of script ends a word.
        assert_ne!(class('日', false), class('a', false));
        assert_eq!(class('日', false), class('本', false));
        // WORD motions see every non-blank alike.
        assert_eq!(class('_', true), class('a', true));
        assert_eq!(class('\t', true), Class::Blank);
    }

    #[test]
    fn normal_state_positions() {
        let t = TextBuf::new("ab\n\ncd\n");
        assert_eq!(last_line(&t), 2);
        assert_eq!(normal_pos(&t, 2), 1, "not on a non-empty line's newline");
        assert_eq!(normal_pos(&t, 3), 3, "an empty line's newline is fine");
        assert_eq!(
            normal_pos(&t, 7),
            5,
            "a final newline doesn't start another line"
        );
        assert_eq!(normal_pos(&TextBuf::new(""), 0), 0);
        assert_eq!(normal_pos(&TextBuf::new("x"), 1), 0);
    }

    #[test]
    fn indentation_counts_columns() {
        let t = TextBuf::new("\tx\n  y\n   \n");
        assert_eq!(indentation(&t, 0), 8);
        assert_eq!(indentation(&t, 1), 2);
        assert!(blank_line(&t, 2) && !blank_line(&t, 1));
    }
}
