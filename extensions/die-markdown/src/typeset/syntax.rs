//! syntax: a fenced code block's text in colours, as GitHub colours a README's. One tokenizer,
//! told by a table what a language's comments, strings, numbers and words are; the languages
//! that are lines more than tokens (a diff, markdown, YAML, a console session, a Makefile, a
//! Dockerfile) and the two that are markup (HTML and CSS) have a pass of their own over the same
//! pieces. A block names its language by its fence's first word; one it doesn't name, or names
//! as nothing known here, is drawn plain, as GitHub draws it.
//!
//! It reads a block a line at a time, carrying what's open (a block comment, a string, a tag, a
//! heredoc, JSX) from one line to the next. It never goes back, and never looks further ahead
//! than the end of the line it's on, and no more than a fixed few look aheads cover the same
//! bytes: its time is the text's length. Nothing it's given makes it panic.

use super::{Rgb, Syntax};

/// What a piece of code is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Constant,
    Meta,
    Variable,
    Tag,
    Attr,
    Inserted,
    Deleted,
    Heading,
}

impl Role {
    /// Its colour in `syntax`; plain code's is `plain`.
    pub(super) fn ink(self, syntax: &Syntax, plain: Rgb) -> Rgb {
        match self {
            Role::Plain => plain,
            Role::Keyword => syntax.keyword,
            Role::String => syntax.string,
            Role::Comment => syntax.comment,
            Role::Number => syntax.number,
            Role::Function => syntax.function,
            Role::Type => syntax.type_,
            Role::Constant => syntax.constant,
            Role::Meta => syntax.meta,
            Role::Variable => syntax.variable,
            Role::Tag => syntax.tag,
            Role::Attr => syntax.attr,
            Role::Inserted => syntax.inserted,
            Role::Deleted => syntax.deleted,
            Role::Heading => syntax.heading,
        }
    }
}

/// A block's highlighter: its language, and what one line left open for the next.
#[derive(Debug)]
pub(super) struct Highlighter {
    lang: &'static Lang,
    glance: Glance,
    open: Open,
    /// The next line is the block's first: a shebang can be on it.
    first: bool,
    /// The word that ends a heredoc whose body is next or open.
    heredoc: Vec<u8>,
    /// CSS: how many braces are open, whether the next token starts a statement, and whether
    /// the statement at hand is a selector.
    depth: u32,
    stmt: bool,
    sel: bool,
    /// JSX: what's open where a tag is, innermost last.
    jsx: Vec<Jsx>,
}

/// What a language's table can start with, worked out once a block, so that most bytes and
/// words are passed over at a glance: a mark that opens nothing, a name in no list.
#[derive(Clone, Copy, Debug)]
struct Glance {
    /// The bytes besides a word's that can start something other than a plain mark.
    special: u128,
    /// The first bytes of the words the language lists, lowercase where its words are any
    /// case, and the longest of them.
    first: u128,
    longest: usize,
}

impl Glance {
    fn of(lang: &Lang) -> Glance {
        let mut special = 0u128;
        let mut add = |c: u8| {
            if c < 128 {
                special |= 1 << c;
            }
        };
        let opens = lang.line.iter().chain(lang.quotes.iter().map(|q| &q.open));
        let pairs = lang.block.iter().chain(&lang.pragma).map(|p| &p.0);
        for s in opens.chain(pairs) {
            s.bytes().take(1).for_each(&mut add);
        }
        b"0123456789.".iter().copied().for_each(&mut add);
        lang.sigils.iter().copied().for_each(&mut add);
        let flags = [
            (lang.hash_line || lang.attrs, b'#'),
            (lang.at, b'@'),
            (lang.long, b'['),
            (lang.chars, b'\''),
            (lang.regex, b'/'),
            (lang.symbols, b':'),
            (lang.heredocs || lang.jsx, b'<'),
        ];
        for (on, c) in flags {
            if on {
                add(c);
            }
        }
        if lang.kind == Kind::Conf {
            b"{},[".iter().copied().for_each(&mut add);
        }
        if lang.shell() {
            special = u128::MAX;
        }
        let lists = [lang.keywords, lang.types, lang.builtins, lang.constants];
        let words = lists.iter().flat_map(|l| l.iter());
        let first = words
            .clone()
            .fold(0u128, |m, w| match w.as_bytes().first() {
                Some(&c) if c < 128 => m | 1 << c,
                _ => m,
            });
        let longest = words.map(|w| w.len()).max().unwrap_or(0);
        Glance {
            special,
            first,
            longest,
        }
    }

    /// Whether the byte can start something besides a plain mark.
    fn special(&self, c: u8) -> bool {
        c >= 128 || self.special >> c & 1 == 1
    }

    /// Whether a word could be in one of the language's lists.
    fn listed(&self, w: &[u8], fold: bool) -> bool {
        let c = match fold {
            true => w.first().map(u8::to_ascii_lowercase),
            false => w.first().copied(),
        };
        w.len() <= self.longest && c.is_some_and(|c| c < 128 && self.first >> c & 1 == 1)
    }
}

/// What a line left open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Nothing,
    /// A block comment, this deep.
    Comment(u32),
    /// A string the table's quote `which` opened; `raw` ones take no escapes.
    Str {
        which: u8,
        raw: bool,
    },
    /// A Rust raw string, closed by `"` and this many `#`.
    Raw(u32),
    /// A Lua long bracket of this level: a comment, or a string.
    Long(u32, bool),
    /// A heredoc's body.
    Heredoc,
    /// An HTML tag's attributes.
    Tag,
    /// A YAML block scalar: lines indented past this column are its text.
    Scalar(u32),
    /// A fenced block inside a markdown block: its fence's byte and length.
    Fence(u8, u32),
}

/// What's open in JSX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Jsx {
    /// An element's children: text, tags and `{…}`.
    Text,
    /// A tag's attributes, to its `>`; a closing tag's if `close`.
    Tag { close: bool },
    /// Code in `{…}`, with this many braces of its own open.
    Code(u32),
}

impl Highlighter {
    /// The highlighter for a fence's info string, or none for a language not known here.
    pub(super) fn new(info: &str) -> Option<Highlighter> {
        let lang = language(info)?;
        Some(Highlighter {
            lang,
            glance: Glance::of(lang),
            open: Open::Nothing,
            first: true,
            heredoc: Vec::new(),
            depth: 0,
            stmt: true,
            sel: true,
            jsx: Vec::new(),
        })
    }

    /// The roles of the next line's bytes, as runs: each the byte it starts at and its role, the
    /// first at 0, starts rising. A run goes on to the next one's start; the last to the end.
    pub(super) fn line(&mut self, text: &str, runs: &mut Vec<(usize, Role)>) {
        runs.clear();
        runs.push((0, Role::Plain));
        let b = text.as_bytes();
        let mut out = Out { runs };
        let first = std::mem::replace(&mut self.first, false);
        let script = matches!(self.lang.kind, Kind::Code | Kind::Shell);
        if first && script && b.starts_with(b"#!") && matches!(b.get(2), Some(b'/' | b' ')) {
            out.span(0, b.len(), Role::Meta);
            return;
        }
        match self.lang.kind {
            Kind::Code | Kind::Shell | Kind::Json | Kind::Conf => self.code(b, 0, &mut out),
            Kind::Console => self.console(b, &mut out),
            Kind::Make => {
                self.make(b, &mut out);
                self.line_ends(b);
            }
            Kind::Docker => {
                self.docker(b, &mut out);
                self.line_ends(b);
            }
            Kind::Yaml => self.yaml(b, &mut out),
            Kind::Lisp => self.lisp(b, &mut out),
            Kind::Markup => self.markup(b, &mut out),
            Kind::Css => self.css(b, &mut out),
            Kind::Diff => diff(b, &mut out),
            Kind::Markdown => self.markdown(b, &mut out),
        }
    }

    // ------------------------------------------------------------ code, by the table

    fn code(&mut self, b: &[u8], from: usize, out: &mut Out) {
        let lang = self.lang;
        let shell = lang.shell();
        let mut i = self.resume(b, from, out);
        let mut pending = None;
        // Whether what came last ends an operand, so a `/` after it divides.
        let mut operand = false;
        let mut regex = lang.regex;
        let mut key = lang.kind == Kind::Conf && i == from && self.open == Open::Nothing;
        let mut inline = 0u32;
        let lead = skip_space(b, 0);
        // A shell word here would be a command's name.
        let mut command = true;
        // A shell's parentheses open, and how many were when `((` opened arithmetic, plus one.
        let (mut parens, mut arith) = (0u32, 0u32);
        while i < b.len() {
            match self.jsx.last_mut() {
                Some(Jsx::Text) => {
                    i = self.jsx_text(b, i, out);
                    operand = self.jsx.is_empty();
                    continue;
                }
                Some(Jsx::Tag { .. }) => {
                    i = self.jsx_tag(b, i, out);
                    operand = self.jsx.is_empty();
                    continue;
                }
                Some(Jsx::Code(depth)) if matches!(b[i], b'{' | b'}') => {
                    if b[i] == b'{' {
                        *depth += 1;
                    } else if *depth > 0 {
                        *depth -= 1;
                    } else {
                        self.jsx.pop();
                        i += 1;
                        continue;
                    }
                }
                _ => {}
            }
            let c = b[i];
            if is_space(c) {
                i += 1;
                continue;
            }
            if shell {
                match c {
                    b'(' => {
                        if arith == 0 && b.get(i + 1) == Some(&b'(') {
                            arith = parens + 1;
                        }
                        parens = parens.saturating_add(1);
                    }
                    b')' => {
                        parens = parens.saturating_sub(1);
                        if parens < arith {
                            arith = 0;
                        }
                    }
                    _ => {}
                }
            }
            if key {
                key = false;
                if let Some(end) = self.conf_key(b, i, i == lead, out) {
                    i = end;
                    continue;
                }
            }
            let wordy = match shell {
                true => shell_byte(c) && c != b'#',
                false => word_start(c, lang),
            };
            if !wordy {
                pending = None;
            }
            if shell
                && let Some(end) =
                    self.shell_token(b, i, arith > 0, out, &mut pending, &mut command)
            {
                i = end;
                continue;
            }
            // No language's comment or string opens with a letter (a prefix is the word's to
            // see), so a word is a word; and a mark that opens nothing is a mark.
            if wordy && !shell {
                let (end, role) = self.word(b, i, &mut pending, out);
                operand = role != Role::Keyword;
                i = end;
                continue;
            }
            if !self.glance.special(c) {
                operand = matches!(c, b')' | b']' | b'}');
                command = matches!(c, b';' | b'|' | b'&' | b'(' | b'{' | b'`' | b'!');
                i += 1;
                continue;
            }
            if let Some(s) = lang.line.iter().find(|s| at(b, i, s)) {
                let after = i + s.len();
                if lang.long
                    && b.get(after) == Some(&b'[')
                    && let Some((level, n)) = long_open(b, after)
                {
                    i = self.long(b, i, after + n, level, true, out);
                    continue;
                }
                out.span(i, b.len(), Role::Comment);
                break;
            }
            if let Some((open, close)) = lang.pragma
                && at(b, i, open)
            {
                let end = find(b, i + open.len(), close).map_or(b.len(), |k| k + close.len());
                out.span(i, end, Role::Meta);
                i = end;
                continue;
            }
            if let Some((open, _)) = lang.block
                && at(b, i, open)
            {
                i = self.comment(b, i, i + open.len(), 1, out);
                continue;
            }
            if c == b'#' && lang.hash_line && i == lead {
                i = directive(b, i, out);
                continue;
            }
            if c == b'#' && lang.attrs && (b.get(i + 1) == Some(&b'[') || at(b, i + 1, "![")) {
                let open = if b[i + 1] == b'[' { i + 1 } else { i + 2 };
                let end = close_bracket(b, open, b'[', b']');
                out.span(i, end, Role::Meta);
                i = end;
                continue;
            }
            if c == b'@' && lang.at && b.get(i + 1).is_some_and(|&d| word_start(d, lang)) {
                let mut j = i + 1;
                loop {
                    while j < b.len() && word_byte(b[j], lang) {
                        j += 1;
                    }
                    match b.get(j) == Some(&b'.')
                        && b.get(j + 1).is_some_and(|&d| word_start(d, lang))
                    {
                        true => j += 1,
                        false => break,
                    }
                }
                out.span(i, j, Role::Meta);
                i = j;
                continue;
            }
            if c == b'['
                && lang.long
                && let Some((level, n)) = long_open(b, i)
            {
                i = self.long(b, i, i + n, level, false, out);
                operand = true;
                continue;
            }
            if c == b'\'' && lang.chars {
                match char_end(b, i) {
                    Some(end) => {
                        out.span(i, end, Role::String);
                        i = end;
                        operand = true;
                    }
                    // A lifetime or a label, or a prime: plain, with its word.
                    None => {
                        i += 1;
                        while i < b.len() && word_byte(b[i], lang) {
                            i += 1;
                        }
                    }
                }
                continue;
            }
            if c == b'<'
                && lang.heredocs
                && let Some(end) = self.ruby_heredoc(b, i)
            {
                i = end;
                operand = true;
                continue;
            }
            if c == b'<'
                && lang.jsx
                && !operand
                && let Some(end) = self.jsx_open(b, i, out)
            {
                i = end;
                continue;
            }
            if let Some(which) = quote_at(lang, b, i) {
                let q = &lang.quotes[which];
                // A JSON string with a colon after it is a key: a constant's colour, as GitHub
                // draws one, and highlight.js's attribute grey is its constant grey.
                if lang.kind == Kind::Json
                    && let Some(end) = closed(b, i + q.open.len(), q)
                {
                    let role = match b.get(skip_space(b, end)) {
                        Some(b':') => Role::Constant,
                        _ => Role::String,
                    };
                    out.span(i, end, role);
                    i = end;
                } else {
                    i = self.string(b, i, i + q.open.len(), which as u8, false, out);
                }
                (operand, command) = (true, false);
                continue;
            }
            if c == b'/' && regex && !operand {
                match regex_end(b, i) {
                    Some(end) => {
                        out.span(i, end, Role::String);
                        i = end;
                        operand = true;
                    }
                    // Nothing on the line closes it, nor will anything after.
                    None => {
                        regex = false;
                        i += 1;
                    }
                }
                continue;
            }
            let digit = |k: usize| b.get(k).is_some_and(u8::is_ascii_digit);
            if c.is_ascii_digit() || (c == b'.' && !operand && digit(i + 1)) {
                let end = number_end(b, if c == b'.' { i + 1 } else { i });
                out.span(i, end, Role::Number);
                i = end;
                operand = true;
                continue;
            }
            if lang.sigils.contains(&c) {
                let mut j = i + 1;
                while b.get(j) == Some(&c) {
                    j += 1;
                }
                if b.get(j).is_some_and(|&d| word_start(d, lang)) {
                    while j < b.len() && word_byte(b[j], lang) {
                        j += 1;
                    }
                    out.span(i, j, Role::Variable);
                    operand = true;
                }
                i = j;
                continue;
            }
            if c == b':'
                && lang.symbols
                && b.get(i + 1).is_some_and(|&d| word_start(d, lang))
                && (i == 0 || b[i - 1] != b':')
            {
                let mut j = i + 1;
                while j < b.len() && word_byte(b[j], lang) {
                    j += 1;
                }
                out.span(i, j, Role::Constant);
                i = j;
                operand = true;
                continue;
            }
            if lang.kind == Kind::Conf {
                match c {
                    b'{' => {
                        inline += 1;
                        key = true;
                    }
                    b'}' => inline = inline.saturating_sub(1),
                    b',' if inline > 0 => key = true,
                    _ => {}
                }
            }
            operand = matches!(c, b')' | b']' | b'}');
            command = matches!(c, b';' | b'|' | b'&' | b'(' | b'{' | b'`' | b'!');
            i += 1;
        }
        if !self.heredoc.is_empty() && self.open != Open::Heredoc {
            match self.open {
                Open::Nothing => self.open = Open::Heredoc,
                _ => self.heredoc.clear(),
            }
        }
    }

    /// Goes on with what the last line left open, from `i`: where what's open ends, or the
    /// line's end if it doesn't.
    fn resume(&mut self, b: &[u8], i: usize, out: &mut Out) -> usize {
        match self.open {
            Open::Comment(depth) => self.comment(b, i, i, depth, out),
            Open::Str { which, raw } => self.string(b, i, i, which, raw, out),
            Open::Raw(hashes) => self.raw(b, i, i, hashes, out),
            Open::Long(level, comment) => self.long(b, i, i, level, comment, out),
            Open::Heredoc => self.heredoc(b, out),
            _ => i,
        }
    }

    /// A block comment from `from`, `depth` deep at `i`.
    fn comment(
        &mut self,
        b: &[u8],
        from: usize,
        mut i: usize,
        mut depth: u32,
        out: &mut Out,
    ) -> usize {
        let Some((open, close)) = self.lang.block else {
            self.open = Open::Nothing;
            return i;
        };
        while i < b.len() {
            if at(b, i, close) {
                i += close.len();
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.span(from, i, Role::Comment);
                    self.open = Open::Nothing;
                    return i;
                }
            } else if self.lang.nest && at(b, i, open) {
                i += open.len();
                depth = depth.saturating_add(1);
            } else {
                i += 1;
            }
        }
        out.span(from, b.len(), Role::Comment);
        self.open = Open::Comment(depth.max(1));
        b.len()
    }

    /// A string from `from` whose text starts at `i`.
    fn string(
        &mut self,
        b: &[u8],
        from: usize,
        mut i: usize,
        which: u8,
        raw: bool,
        out: &mut Out,
    ) -> usize {
        let Some(q) = self.lang.quotes.get(which as usize) else {
            self.open = Open::Nothing;
            return i;
        };
        let escape = q.escape && !raw;
        let make = self.lang.kind == Kind::Make;
        let mut seg = from;
        while i < b.len() {
            let c = b[i];
            if escape && c == b'\\' {
                i += 2;
                continue;
            }
            if !q.close.is_empty() && at(b, i, q.close) {
                i += q.close.len();
                out.span(seg, i, Role::String);
                self.open = Open::Nothing;
                return i;
            }
            if q.vars
                && c == b'$'
                && let Some(end) = shell_var(b, i, make)
            {
                out.span(seg, i, Role::String);
                out.span(i, end, Role::Variable);
                (seg, i) = (end, end);
                continue;
            }
            i += 1;
        }
        out.span(seg, b.len(), Role::String);
        self.open = match q.lines && !q.close.is_empty() {
            true => Open::Str { which, raw },
            false => Open::Nothing,
        };
        b.len()
    }

    /// A Rust raw string from `from`, its text from `i`.
    fn raw(&mut self, b: &[u8], from: usize, mut i: usize, hashes: u32, out: &mut Out) -> usize {
        let h = hashes as usize;
        while i < b.len() {
            if b[i] == b'"' && b.len() - i > h && b[i + 1..i + 1 + h].iter().all(|&c| c == b'#') {
                i += 1 + h;
                out.span(from, i, Role::String);
                self.open = Open::Nothing;
                return i;
            }
            i += 1;
        }
        out.span(from, b.len(), Role::String);
        self.open = Open::Raw(hashes);
        b.len()
    }

    /// A Lua long bracket from `from`, its text from `i`.
    fn long(
        &mut self,
        b: &[u8],
        from: usize,
        mut i: usize,
        level: u32,
        comment: bool,
        out: &mut Out,
    ) -> usize {
        let role = if comment { Role::Comment } else { Role::String };
        let l = level as usize;
        while i < b.len() {
            if b[i] == b']'
                && b.len() - i - 1 > l
                && b[i + 1..i + 1 + l].iter().all(|&c| c == b'=')
                && b[i + 1 + l] == b']'
            {
                i += 2 + l;
                out.span(from, i, role);
                self.open = Open::Nothing;
                return i;
            }
            i += 1;
        }
        out.span(from, b.len(), role);
        self.open = Open::Long(level, comment);
        b.len()
    }

    /// A line of a heredoc's body, or the word that ends it.
    fn heredoc(&mut self, b: &[u8], out: &mut Out) -> usize {
        if b.trim_ascii() == self.heredoc.as_slice() {
            self.open = Open::Nothing;
            self.heredoc.clear();
        } else {
            out.span(0, b.len(), Role::String);
        }
        b.len()
    }

    /// A word of code from `i`: where it ends and what it is.
    fn word(
        &mut self,
        b: &[u8],
        i: usize,
        pending: &mut Option<Role>,
        out: &mut Out,
    ) -> (usize, Role) {
        let lang = self.lang;
        let mut j = i + 1;
        while j < b.len() && word_byte(b[j], lang) {
            j += 1;
        }
        if lang.symbols && matches!(b.get(j), Some(b'?' | b'!')) && b.get(j + 1) != Some(&b'=') {
            j += 1;
        }
        if let Some(end) = self.prefixed(b, i, j, out) {
            *pending = None;
            return (end, Role::String);
        }
        let (w, fold) = (&b[i..j], lang.fold);
        let listed = self.glance.listed(w, fold);
        if listed && has(lang.keywords, w, fold) {
            let defs = [
                (lang.defs, Role::Function),
                (lang.type_defs, Role::Type),
                (lang.var_defs, Role::Variable),
            ];
            *pending = defs
                .into_iter()
                .find(|(l, _)| has(l, w, fold))
                .map(|(_, r)| r);
            out.span(i, j, Role::Keyword);
            return (j, Role::Keyword);
        }
        let next = b.get(j).copied();
        let caps = |c: &u8| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_';
        let assigned = lang.eq_keys && {
            let k = skip_space(b, j);
            b.get(k) == Some(&b'=') && b.get(k + 1) != Some(&b'=')
        };
        let role = if listed && has(lang.constants, w, fold) {
            Role::Constant
        } else if let Some(role) = pending.take() {
            role
        } else if listed && has(lang.types, w, fold) {
            lang.prims
        } else if listed && has(lang.builtins, w, fold) {
            Role::Constant
        } else if lang.macros
            && next == Some(b'!')
            && matches!(b.get(j + 1), Some(b'(' | b'[' | b'{'))
        {
            j += 1;
            Role::Function
        } else if next == Some(b'(') {
            Role::Function
        } else if lang.symbols && next == Some(b':') && b.get(j + 1) != Some(&b':') {
            j += 1;
            Role::Constant
        } else if assigned {
            Role::Attr
        } else if lang.upper
            && !fold
            && w.len() > 1
            && w[0].is_ascii_uppercase()
            && w.iter().all(caps)
        {
            Role::Constant
        } else if lang.caps && w[0].is_ascii_uppercase() {
            Role::Type
        } else {
            Role::Plain
        };
        *pending = None;
        out.span(i, j, role);
        (j, role)
    }

    /// A string whose prefix is the word `b[i..j]`: `r"…"`, `b'…'`, `f"…"`, `L"…"` and their kin.
    fn prefixed(&mut self, b: &[u8], i: usize, j: usize, out: &mut Out) -> Option<usize> {
        let w = &b[i..j];
        match self.lang.prefixes {
            Prefixes::None => None,
            Prefixes::Rust => match w {
                b"r" | b"br" | b"cr" => {
                    let mut k = j;
                    while b.get(k) == Some(&b'#') {
                        k += 1;
                    }
                    let hashes = (k - j <= 255).then_some((k - j) as u32)?;
                    (b.get(k) == Some(&b'"')).then(|| self.raw(b, i, k + 1, hashes, out))
                }
                b"b" | b"c" => match b.get(j) {
                    Some(b'"') => Some(self.string(b, i, j + 1, 0, false, out)),
                    Some(b'\'') => char_end(b, j).inspect(|&end| out.span(i, end, Role::String)),
                    _ => None,
                },
                _ => None,
            },
            // PostgreSQL's E'…' takes a backslash's escapes.
            Prefixes::Sql => {
                if !matches!(w, b"E" | b"e") || b.get(j) != Some(&b'\'') {
                    return None;
                }
                let which = self.lang.quotes.iter().position(|q| q.open == "E'")?;
                Some(self.string(b, i, j + 1, which as u8, false, out))
            }
            Prefixes::Python | Prefixes::C => {
                if !matches!(b.get(j), Some(b'"' | b'\'')) || w.len() > 3 {
                    return None;
                }
                let low = w.to_ascii_lowercase();
                let raw = match self.lang.prefixes {
                    // A raw string keeps its backslashes, but one still keeps a quote from
                    // closing it: it's read as any string is.
                    Prefixes::Python => match low.as_slice() {
                        b"r" | b"br" | b"rb" | b"fr" | b"rf" | b"u" | b"b" | b"f" => false,
                        _ => return None,
                    },
                    _ => match w {
                        b"R" | b"LR" | b"uR" | b"UR" | b"u8R" => true,
                        b"L" | b"u" | b"U" | b"u8" => false,
                        _ => return None,
                    },
                };
                let which = quote_at(self.lang, b, j)?;
                let open = self.lang.quotes[which].open.len();
                Some(self.string(b, i, j + open, which as u8, raw, out))
            }
        }
    }

    /// A TOML or INI key at `i`, and a section's header if `i` is the line's first byte past
    /// its indent: where it ends, or none if what's at `i` is no key.
    fn conf_key(&self, b: &[u8], i: usize, lead: bool, out: &mut Out) -> Option<usize> {
        if b[i] == b'[' && lead {
            let mut end = find(b, i, "]").map_or(b.len(), |k| k + 1);
            if b.get(end) == Some(&b']') {
                end += 1;
            }
            out.span(i, end, Role::Heading);
            return Some(end);
        }
        let mut j = i;
        while j < b.len() {
            let c = b[j];
            if self.lang.key_seps.contains(&c) {
                if c == b'=' && b.get(j + 1) == Some(&b'=') {
                    return None;
                }
                let mut end = j;
                while end > i && is_space(b[end - 1]) {
                    end -= 1;
                }
                if end == i {
                    return None;
                }
                out.span(i, end, Role::Attr);
                return Some(end);
            }
            match c {
                b'"' | b'\'' => j += b[j + 1..].iter().position(|&d| d == c)? + 2,
                b'#' | b';' | b'[' | b']' | b'{' | b'}' | b',' => return None,
                _ => j += 1,
            }
        }
        None
    }

    // ------------------------------------------------------------ shells and the files like them

    /// A shell's own token at `i`: where it ends, or none for what any code has (a quote, a
    /// mark). In `arith`metic, `<<` shifts.
    fn shell_token(
        &mut self,
        b: &[u8],
        i: usize,
        arith: bool,
        out: &mut Out,
        pending: &mut Option<Role>,
        command: &mut bool,
    ) -> Option<usize> {
        let lang = self.lang;
        let make = lang.kind == Kind::Make;
        let c = b[i];
        match c {
            b'#' => {
                out.span(i, b.len(), Role::Comment);
                Some(b.len())
            }
            b'\\' => Some((i + 2).min(b.len())),
            b'$' if b.get(i + 1) != Some(&b'\'') => {
                *command = false;
                let end = shell_var(b, i, make).unwrap_or(i + 1);
                if end > i + 1 {
                    out.span(i, end, Role::Variable);
                }
                Some(end)
            }
            b'<' if at(b, i, "<<") && !at(b, i, "<<<") && !arith => {
                *command = false;
                Some(self.heredoc_start(b, i))
            }
            _ if shell_byte(c) => {
                let mut j = i;
                while j < b.len() && shell_byte(b[j]) {
                    j += 1;
                }
                let w = &b[i..j];
                let role = if let Some(role) = pending.take() {
                    role
                } else if has(lang.keywords, w, false) {
                    if has(lang.defs, w, false) {
                        *pending = Some(Role::Function);
                    }
                    Role::Keyword
                } else if has(lang.constants, w, false)
                    || (*command && has(lang.builtins, w, false))
                {
                    Role::Constant
                } else if w.iter().all(u8::is_ascii_digit) {
                    Role::Number
                } else if b.get(j) == Some(&b'=') && is_ident(w) {
                    Role::Variable
                } else if at(b, j, "()") {
                    Role::Function
                } else {
                    Role::Plain
                };
                // After `if`, `then`, `do` and the like a command comes, and after a command's
                // name, its arguments.
                *command = role == Role::Keyword;
                out.span(i, j, role);
                Some(j)
            }
            _ => None,
        }
    }

    /// `<<WORD`, `<<-'WORD'`, `<<\WORD` and the like at `i`: the lines after this one are a
    /// heredoc's body, up to WORD's own line. Where the token ends.
    fn heredoc_start(&mut self, b: &[u8], i: usize) -> usize {
        let mut j = i + 2;
        if matches!(b.get(j), Some(b'-' | b'~')) {
            j += 1;
        }
        j = skip_space(b, j);
        let quote = b.get(j).copied().filter(|&c| c == b'\'' || c == b'"');
        if quote.is_some() || b.get(j) == Some(&b'\\') {
            j += 1;
        }
        let w0 = j;
        while b
            .get(j)
            .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_')
        {
            j += 1;
        }
        if j == w0 {
            return i + 2;
        }
        self.heredoc = b[w0..j].to_vec();
        if quote.is_some() && b.get(j).copied() == quote {
            j += 1;
        }
        j
    }

    /// Ruby's `<<~WORD`, `<<-WORD`, `<<WORD` and their quoted words at `i`: where the token
    /// ends, with the word kept for the line that ends the body; none for a `<<` that appends
    /// or shifts, with a space or a number after it, or a lowercase word with no `~` or `-`.
    fn ruby_heredoc(&mut self, b: &[u8], i: usize) -> Option<usize> {
        if !at(b, i, "<<") {
            return None;
        }
        let mut j = i + 2;
        let marked = matches!(b.get(j), Some(b'~' | b'-'));
        j += usize::from(marked);
        let quote = b
            .get(j)
            .copied()
            .filter(|&c| matches!(c, b'\'' | b'"' | b'`'));
        j += usize::from(quote.is_some());
        let w0 = j;
        while b
            .get(j)
            .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_')
        {
            j += 1;
        }
        let first = *b.get(w0).filter(|_| j > w0)?;
        if first.is_ascii_digit() || !(marked || quote.is_some() || first.is_ascii_uppercase()) {
            return None;
        }
        let word = w0..j;
        if let Some(q) = quote {
            j += usize::from(b.get(j) == Some(&q));
        }
        self.heredoc = b[word].to_vec();
        Some(j)
    }

    /// A JSX element's start at the `<` at `i`, where an operand goes: its tag's name, or the
    /// `<>` of a fragment. None for what's no tag, as a generic function's `<T,>`.
    fn jsx_open(&mut self, b: &[u8], i: usize, out: &mut Out) -> Option<usize> {
        match *b.get(i + 1)? {
            b'>' => {
                self.jsx.push(Jsx::Text);
                Some(i + 2)
            }
            c if c.is_ascii_alphabetic() => {
                let end = jsx_name(b, i + 1);
                let k = skip_space(b, end);
                if b.get(k) == Some(&b',') || at(b, k, "extends ") {
                    return None;
                }
                out.span(i + 1, end, Role::Tag);
                self.jsx.push(Jsx::Tag { close: false });
                Some(end)
            }
            _ => None,
        }
    }

    /// An element's text from `i`, plain, to a tag, a `{` or the line's end: where it stops.
    fn jsx_text(&mut self, b: &[u8], i: usize, out: &mut Out) -> usize {
        let mut j = i;
        while j < b.len() {
            match (b[j], b.get(j + 1).copied()) {
                (b'{', _) => {
                    self.jsx.push(Jsx::Code(0));
                    return j + 1;
                }
                // A closing tag ends the element this is the text of.
                (b'<', Some(b'/')) => {
                    self.jsx.pop();
                    if b.get(j + 2) == Some(&b'>') {
                        return j + 3;
                    }
                    let end = jsx_name(b, j + 2);
                    out.span(j + 2, end, Role::Tag);
                    self.jsx.push(Jsx::Tag { close: true });
                    return end;
                }
                (b'<', Some(b'>')) => {
                    self.jsx.push(Jsx::Text);
                    return j + 2;
                }
                (b'<', Some(c)) if c.is_ascii_alphabetic() => {
                    let end = jsx_name(b, j + 1);
                    out.span(j + 1, end, Role::Tag);
                    self.jsx.push(Jsx::Tag { close: false });
                    return end;
                }
                _ => j += 1,
            }
        }
        j
    }

    /// A tag's attributes from `i`, to its `>`, a `{` or the line's end: where it stops.
    fn jsx_tag(&mut self, b: &[u8], i: usize, out: &mut Out) -> usize {
        let close = self.jsx.last() == Some(&Jsx::Tag { close: true });
        let mut j = i;
        while j < b.len() {
            let c = b[j];
            match c {
                b'>' => {
                    self.jsx.pop();
                    if !close {
                        self.jsx.push(Jsx::Text);
                    }
                    return j + 1;
                }
                b'/' if b.get(j + 1) == Some(&b'>') => {
                    self.jsx.pop();
                    return j + 2;
                }
                b'{' => {
                    self.jsx.push(Jsx::Code(0));
                    return j + 1;
                }
                b'"' | b'\'' => {
                    let end = b[j + 1..]
                        .iter()
                        .position(|&d| d == c)
                        .map_or(b.len(), |p| j + p + 2);
                    out.span(j, end, Role::String);
                    j = end;
                }
                _ if c.is_ascii_alphabetic() || c == b'_' => {
                    let end = jsx_name(b, j);
                    out.span(j, end, Role::Attr);
                    j = end;
                }
                _ => j += 1,
            }
        }
        j
    }

    /// A console session: what follows a `$ ` prompt is a command, the rest what it printed.
    fn console(&mut self, b: &[u8], out: &mut Out) {
        if self.open != Open::Nothing {
            return self.code(b, 0, out);
        }
        let i = skip_space(b, 0);
        if b.get(i) == Some(&b'$') && matches!(b.get(i + 1), None | Some(b' ' | b'\t')) {
            self.code(b, i + 1, out);
        }
    }

    /// A Makefile: rules, assignments and directives, with a recipe's lines in the shell's
    /// colours.
    fn make(&mut self, b: &[u8], out: &mut Out) {
        if self.open != Open::Nothing || b.first() == Some(&b'\t') {
            return self.code(b, 0, out);
        }
        let i = skip_space(b, 0);
        if i == b.len() {
            return;
        }
        if b[i] == b'#' {
            return out.span(i, b.len(), Role::Comment);
        }
        let mut j = i;
        while b
            .get(j)
            .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            j += 1;
        }
        if j > i && has(MAKE_DIRECTIVES, &b[i..j], false) && b.get(j).is_none_or(|&c| is_space(c)) {
            out.span(i, j, Role::Keyword);
            return self.code(b, j, out);
        }
        // An assignment or a rule, by which of `=` and `:` comes first outside `$(…)`.
        let (mut k, mut depth) = (i, 0u32);
        while k < b.len() {
            match b[k] {
                b'$' if matches!(b.get(k + 1), Some(b'(' | b'{')) => {
                    depth += 1;
                    k += 1;
                }
                b')' | b'}' if depth > 0 => depth -= 1,
                b'=' | b':' if depth == 0 => break,
                b'#' if depth == 0 => k = b.len() - 1,
                _ => {}
            }
            k += 1;
        }
        if k >= b.len() {
            return self.code(b, i, out);
        }
        let trim = |mut end: usize| {
            while end > i && (is_space(b[end - 1]) || matches!(b[end - 1], b'?' | b'+' | b'!')) {
                end -= 1;
            }
            end
        };
        // What's after is text, not the shell's, but for a rule's recipe after a `;`.
        if b[k] == b'=' || at(b, k, ":=") || at(b, k, "::=") {
            out.span(i, trim(k), Role::Variable);
            make_text(b, k + 1, false, out);
        } else {
            let role = if b[i] == b'.' {
                Role::Keyword
            } else {
                Role::Function
            };
            out.span(i, trim(k), role);
            if let Some(semi) = make_text(b, k + 1, true, out) {
                self.code(b, semi + 1, out);
            }
        }
    }

    /// A Makefile's line, or a Dockerfile's, is a command of its own unless a `\` goes on with
    /// it: a quote it leaves open closes at its end.
    fn line_ends(&mut self, b: &[u8]) {
        if matches!(self.open, Open::Str { .. }) && !b.trim_ascii_end().ends_with(b"\\") {
            self.open = Open::Nothing;
        }
    }

    /// A Dockerfile: an instruction, then its arguments in the shell's colours.
    fn docker(&mut self, b: &[u8], out: &mut Out) {
        if self.open != Open::Nothing {
            return self.code(b, 0, out);
        }
        let i = skip_space(b, 0);
        let mut j = i;
        while b.get(j).is_some_and(u8::is_ascii_alphabetic) {
            j += 1;
        }
        if j > i && has(DOCKER_INSTRUCTIONS, &b[i..j], true) {
            out.span(i, j, Role::Keyword);
            // A maintainer is a name, in no shell's quotes.
            if b[i..j].eq_ignore_ascii_case(b"maintainer") {
                return;
            }
            return self.code(b, j, out);
        }
        self.code(b, i, out);
    }

    // ------------------------------------------------------------ YAML

    fn yaml(&mut self, b: &[u8], out: &mut Out) {
        let ind = b.iter().take_while(|&&c| c == b' ').count();
        match self.open {
            Open::Scalar(col) => {
                if b[ind..].iter().all(|&c| is_space(c)) {
                    return;
                }
                if ind as u64 > u64::from(col) {
                    return out.span(ind, b.len(), Role::String);
                }
                self.open = Open::Nothing;
            }
            Open::Str { .. } => {
                let i = self.resume(b, 0, out);
                if self.open == Open::Nothing {
                    self.yaml_value(b, i, ind, out);
                }
                return;
            }
            _ => {}
        }
        let mut i = ind;
        if ind == 0
            && (b.starts_with(b"---") || b.starts_with(b"..."))
            && b.get(3).is_none_or(|&c| is_space(c))
        {
            out.span(0, 3, Role::Meta);
            i = 3;
        }
        loop {
            i = skip_space(b, i);
            match b.get(i) == Some(&b'-') && b.get(i + 1).is_none_or(|&c| is_space(c)) {
                true => i += 1,
                false => break,
            }
        }
        if b.get(i) == Some(&b'#') {
            return out.span(i, b.len(), Role::Comment);
        }
        let col = i;
        if let Some((end, colon)) = yaml_key(b, i) {
            out.span(i, end, Role::Attr);
            i = colon + 1;
        }
        self.yaml_value(b, i, col, out);
    }

    /// A YAML value from `i`, whose key (or entry) is at column `col`.
    fn yaml_value(&mut self, b: &[u8], mut i: usize, col: usize, out: &mut Out) {
        let (mut first, mut flow) = (true, 0u32);
        while i < b.len() {
            let c = b[i];
            if is_space(c) {
                i += 1;
                continue;
            }
            if c == b'#' && (i == 0 || is_space(b[i - 1])) {
                return out.span(i, b.len(), Role::Comment);
            }
            match c {
                b'|' | b'>' if first => {
                    let mut j = i + 1;
                    while b
                        .get(j)
                        .is_some_and(|&d| d == b'+' || d == b'-' || d.is_ascii_digit())
                    {
                        j += 1;
                    }
                    let k = skip_space(b, j);
                    if k == b.len() || b[k] == b'#' {
                        out.span(k, b.len(), Role::Comment);
                        self.open = Open::Scalar(col.min(u32::MAX as usize) as u32);
                        return;
                    }
                    i = j;
                }
                b'"' | b'\'' => {
                    let which = if c == b'"' { 0 } else { 1 };
                    i = self.string(b, i, i + 1, which, false, out);
                    if self.open != Open::Nothing {
                        return;
                    }
                }
                b'&' | b'*' | b'!' => {
                    let mut j = i + 1;
                    while j < b.len() && !is_space(b[j]) && !b",[]{}".contains(&b[j]) {
                        j += 1;
                    }
                    let role = if c == b'!' {
                        Role::Type
                    } else {
                        Role::Variable
                    };
                    out.span(i, j, role);
                    i = j;
                }
                b'{' | b'[' => {
                    flow += 1;
                    i += 1;
                }
                b'}' | b']' => {
                    flow = flow.saturating_sub(1);
                    i += 1;
                }
                b',' => i += 1,
                _ => {
                    let mut j = i;
                    while j < b.len() && !is_space(b[j]) && !b",[]{}".contains(&b[j]) {
                        j += 1;
                    }
                    let w = &b[i..j];
                    if flow > 0 && w.len() > 1 && w.ends_with(b":") {
                        out.span(i, j - 1, Role::Attr);
                    } else if is_number(w) {
                        out.span(i, j, Role::Number);
                    } else if has(YAML_CONSTANTS, w, false) {
                        out.span(i, j, Role::Constant);
                    }
                    i = j;
                }
            }
            first = false;
        }
    }

    // ------------------------------------------------------------ the lisps

    fn lisp(&mut self, b: &[u8], out: &mut Out) {
        let lang = self.lang;
        let mut i = self.resume(b, 0, out);
        let mut pending = None;
        while i < b.len() {
            let c = b[i];
            let next = b.get(i + 1).copied();
            match c {
                _ if is_space(c) => i += 1,
                b';' => return out.span(i, b.len(), Role::Comment),
                b'"' => {
                    i = self.string(b, i, i + 1, 0, false, out);
                    pending = None;
                }
                b'#' if next == Some(b'|') => i = self.comment(b, i, i + 2, 1, out),
                // `#'name`: a function, by name.
                b'#' if next == Some(b'\'') => {
                    let end = symbol_end(b, i + 2);
                    out.span(i, end, Role::Function);
                    i = end;
                }
                // `#\a`, `#\space`: a character.
                b'#' if next == Some(b'\\') => {
                    let end = symbol_end(b, (i + 2 + char_len(b, i + 2)).min(b.len()));
                    out.span(i, end, Role::String);
                    i = end;
                }
                b'#' if next.is_some_and(|d| d.is_ascii_alphabetic()) => {
                    let end = symbol_end(b, i + 1);
                    let w = &b[i + 1..end];
                    let radix = matches!(w[0], b'x' | b'X' | b'b' | b'B' | b'o' | b'O');
                    let role = match w {
                        b"t" | b"f" | b"true" | b"false" | b"nil" => Role::Constant,
                        _ if radix && w.len() > 1 && w[1..].iter().all(u8::is_ascii_hexdigit) => {
                            Role::Number
                        }
                        _ => Role::Plain,
                    };
                    out.span(i, end, role);
                    i = end;
                }
                // `?a`, `?\n`, `?\C-x`: an Emacs character.
                b'?' => {
                    let mut j = i + 1;
                    if b.get(j) == Some(&b'\\') {
                        j += 1;
                    }
                    let end = symbol_end(b, (j + char_len(b, j)).min(b.len()));
                    out.span(i, end, Role::String);
                    i = end;
                }
                b'\'' if next.is_some_and(symbol_byte) => {
                    let end = symbol_end(b, i + 1);
                    out.span(i, end, Role::Constant);
                    i = end;
                    pending = None;
                }
                // A form opens: `(define (name …` names what comes first in it.
                b'(' | b'[' | b'{' => i += 1,
                _ if !symbol_byte(c) => {
                    i += 1;
                    pending = None;
                }
                _ => {
                    let end = symbol_end(b, i).max(i + 1);
                    let w = &b[i..end];
                    let role = if let Some(role) = pending.take() {
                        role
                    } else if w.len() > 1 && w[0] == b':' {
                        Role::Constant
                    } else if is_number(w) {
                        Role::Number
                    } else if has(lang.constants, w, false) {
                        Role::Constant
                    } else if has(lang.keywords, w, false) {
                        let defs = [
                            (lang.defs, Role::Function),
                            (lang.type_defs, Role::Type),
                            (lang.var_defs, Role::Variable),
                        ];
                        pending = defs
                            .into_iter()
                            .find(|(l, _)| has(l, w, false))
                            .map(|(_, r)| r);
                        Role::Keyword
                    } else if has(lang.builtins, w, false) {
                        Role::Constant
                    } else {
                        Role::Plain
                    };
                    out.span(i, end, role);
                    i = end;
                }
            }
        }
    }

    // ------------------------------------------------------------ markup

    fn markup(&mut self, b: &[u8], out: &mut Out) {
        let mut i = 0;
        while i < b.len() {
            match self.open {
                Open::Comment(depth) => {
                    i = self.comment(b, i, i, depth, out);
                    continue;
                }
                Open::Tag => {
                    i = self.attrs(b, i, out);
                    continue;
                }
                _ => {}
            }
            match b[i] {
                b'<' if at(b, i, "<!--") => i = self.comment(b, i, i + 4, 1, out),
                b'<' if matches!(b.get(i + 1), Some(b'!' | b'?')) => {
                    let end = find(b, i, ">").map_or(b.len(), |k| k + 1);
                    out.span(i, end, Role::Meta);
                    i = end;
                }
                b'<' => {
                    let name = if b.get(i + 1) == Some(&b'/') {
                        i + 2
                    } else {
                        i + 1
                    };
                    if !b.get(name).is_some_and(u8::is_ascii_alphabetic) {
                        i += 1;
                        continue;
                    }
                    let mut j = name;
                    while b
                        .get(j)
                        .is_some_and(|&c| c.is_ascii_alphanumeric() || b"-_:.".contains(&c))
                    {
                        j += 1;
                    }
                    out.span(name, j, Role::Tag);
                    self.open = Open::Tag;
                    i = j;
                }
                b'&' => {
                    let mut j = i + 1;
                    while j < b.len()
                        && j - i <= 32
                        && (b[j].is_ascii_alphanumeric() || b[j] == b'#')
                    {
                        j += 1;
                    }
                    if j > i + 1 && b.get(j) == Some(&b';') {
                        out.span(i, j + 1, Role::Constant);
                        i = j + 1;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
    }

    /// A tag's attributes from `i`, to its `>`.
    fn attrs(&mut self, b: &[u8], mut i: usize, out: &mut Out) -> usize {
        while i < b.len() {
            let c = b[i];
            match c {
                b'>' => {
                    self.open = Open::Nothing;
                    return i + 1;
                }
                b'/' | b'?' if b.get(i + 1) == Some(&b'>') => {
                    self.open = Open::Nothing;
                    return i + 2;
                }
                b'"' | b'\'' => {
                    let end = b[i + 1..]
                        .iter()
                        .position(|&d| d == c)
                        .map_or(b.len(), |p| i + p + 2);
                    out.span(i, end, Role::String);
                    i = end;
                }
                b'=' => {
                    i += 1;
                    if b.get(i)
                        .is_some_and(|&d| !is_space(d) && !b"\"'>".contains(&d))
                    {
                        let mut j = i;
                        while j < b.len() && !is_space(b[j]) && b[j] != b'>' {
                            j += 1;
                        }
                        out.span(i, j, Role::String);
                        i = j;
                    }
                }
                _ if is_space(c) || c == b'/' || c == b'?' => i += 1,
                _ => {
                    let mut j = i + 1;
                    while j < b.len() && !is_space(b[j]) && !b"=>/\"'".contains(&b[j]) {
                        j += 1;
                    }
                    out.span(i, j, Role::Attr);
                    i = j;
                }
            }
        }
        i
    }

    // ------------------------------------------------------------ CSS

    fn css(&mut self, b: &[u8], out: &mut Out) {
        let lang = self.lang;
        let mut i = self.resume(b, 0, out);
        let mut prop = false;
        while i < b.len() {
            let c = b[i];
            if is_space(c) {
                i += 1;
                continue;
            }
            if at(b, i, "/*") {
                i = self.comment(b, i, i + 2, 1, out);
                continue;
            }
            if lang.line.iter().any(|s| at(b, i, s)) {
                return out.span(i, b.len(), Role::Comment);
            }
            if self.stmt && !matches!(c, b'{' | b'}' | b';') {
                self.stmt = false;
                self.sel = self.depth == 0 || nested(b, i);
                prop = !self.sel;
            }
            let name = |k: usize| {
                let mut j = k;
                while b.get(j).is_some_and(|&d| {
                    d.is_ascii_alphanumeric() || d == b'-' || d == b'_' || d >= 0x80
                }) {
                    j += 1;
                }
                j
            };
            let digit = |k: usize| b.get(k).is_some_and(u8::is_ascii_digit);
            match c {
                b'{' | b'}' | b';' => {
                    self.depth = match c {
                        b'{' => self.depth.saturating_add(1),
                        b'}' => self.depth.saturating_sub(1),
                        _ => self.depth,
                    };
                    self.stmt = true;
                    i += 1;
                }
                b'"' | b'\'' => {
                    let which = if c == b'"' { 0 } else { 1 };
                    i = self.string(b, i, i + 1, which, false, out);
                }
                b'@' | b'!' => {
                    let end = name(i + 1);
                    if end > i + 1 {
                        out.span(i, end, Role::Keyword);
                    }
                    i = end.max(i + 1);
                }
                b'$' => {
                    let end = name(i + 1);
                    out.span(i, end, Role::Variable);
                    (i, prop) = (end, false);
                }
                // A selector: `.class`, `#id`, `:hover`, `%placeholder`, a tag, `[attr]`.
                _ if self.sel => match c {
                    b'.' | b'#' | b'%' | b':' => {
                        let start = if at(b, i, "::") { i + 2 } else { i + 1 };
                        let end = name(start);
                        if end > start {
                            out.span(i, end, Role::Function);
                        }
                        i = end.max(i + 1);
                    }
                    b'[' => {
                        let end = name(i + 1);
                        out.span(i + 1, end, Role::Attr);
                        i = end.max(i + 1);
                    }
                    _ if c.is_ascii_digit() => {
                        let end = number_end(b, i);
                        let end = if b.get(end) == Some(&b'%') {
                            end + 1
                        } else {
                            end
                        };
                        out.span(i, end, Role::Number);
                        i = end;
                    }
                    _ if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                        let end = name(i);
                        out.span(i, end, Role::Tag);
                        i = end;
                    }
                    _ => i += 1,
                },
                // A declaration: a property, its value.
                b'#' if b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) => {
                    let end = name(i + 1);
                    out.span(i, end, Role::Number);
                    i = end;
                }
                _ if c.is_ascii_digit()
                    || (c == b'.' && digit(i + 1))
                    || (c == b'-'
                        && (digit(i + 1) || (b.get(i + 1) == Some(&b'.') && digit(i + 2)))) =>
                {
                    let start = if c == b'-' { i + 1 } else { i };
                    let start = if b.get(start) == Some(&b'.') {
                        start + 1
                    } else {
                        start
                    };
                    let end = number_end(b, start);
                    let end = if b.get(end) == Some(&b'%') {
                        end + 1
                    } else {
                        end
                    };
                    out.span(i, end, Role::Number);
                    i = end;
                }
                _ if c.is_ascii_alphabetic() || c == b'-' || c == b'_' || c >= 0x80 => {
                    let end = name(i);
                    let w = &b[i..end];
                    // A property is a constant's colour, as GitHub draws one.
                    let role = if w.starts_with(b"--") {
                        Role::Variable
                    } else if prop {
                        Role::Constant
                    } else if b.get(end) == Some(&b'(') {
                        Role::Function
                    } else {
                        Role::Plain
                    };
                    out.span(i, end, role);
                    prop = false;
                    i = end;
                    if role == Role::Function && w.eq_ignore_ascii_case(b"url") {
                        let k = skip_space(b, end + 1);
                        if !matches!(b.get(k), Some(b'"' | b'\'')) {
                            let close = find(b, end, ")").unwrap_or(b.len());
                            out.span(end + 1, close, Role::String);
                            i = close;
                        }
                    }
                }
                _ => i += 1,
            }
        }
    }

    // ------------------------------------------------------------ markdown

    fn markdown(&mut self, b: &[u8], out: &mut Out) {
        let ind = b.iter().take_while(|&&c| c == b' ').count();
        let rest = &b[ind..];
        if let Open::Fence(ch, n) = self.open {
            let run = rest.iter().take_while(|&&c| c == ch).count();
            if ind < 4 && run as u64 >= u64::from(n) && rest[run..].iter().all(|&c| is_space(c)) {
                self.open = Open::Nothing;
            }
            return out.span(0, b.len(), Role::String);
        }
        if ind < 4 {
            if let Some(&ch) = rest.first().filter(|&&c| c == b'`' || c == b'~') {
                let run = rest.iter().take_while(|&&c| c == ch).count();
                if run >= 3 {
                    self.open = Open::Fence(ch, run.min(u32::MAX as usize) as u32);
                    return out.span(0, b.len(), Role::String);
                }
            }
            let hashes = rest.iter().take_while(|&&c| c == b'#').count();
            if (1..=6).contains(&hashes) && rest.get(hashes).is_none_or(|&c| is_space(c)) {
                return out.span(ind, b.len(), Role::Heading);
            }
            if rest.first() == Some(&b'>') {
                return out.span(ind, b.len(), Role::Tag);
            }
            if let Some(&ch) = rest.first().filter(|&&c| b"-*_".contains(&c))
                && rest.iter().all(|&c| c == ch || is_space(c))
                && rest.iter().filter(|&&c| c == ch).count() >= 3
            {
                return out.span(ind, b.len(), Role::Meta);
            }
        }
        let mut i = ind;
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        let marker = match rest.first() {
            Some(b'-' | b'*' | b'+') => 1,
            _ if (1..=9).contains(&digits) && matches!(rest.get(digits), Some(b'.' | b')')) => {
                digits + 1
            }
            _ => 0,
        };
        if marker > 0 && rest.get(marker).is_none_or(|&c| is_space(c)) {
            out.span(ind, ind + marker, Role::Variable);
            i = ind + marker;
        }
        // A code span, a link; each look ahead that finds nothing ends the looking for that
        // kind on the line, as nothing after would be found either: for a code span, for that
        // many backticks, those of 63 or more counted as one length.
        let (mut unmatched, mut brackets) = (0u64, true);
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'`' => {
                    let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                    let bit = 1u64 << n.min(63);
                    match ((unmatched & bit) == 0).then(|| tick_run(b, i + n, n)) {
                        Some(Some(end)) => {
                            out.span(i, end, Role::String);
                            i = end;
                        }
                        _ => {
                            unmatched |= bit;
                            i += n;
                        }
                    }
                }
                b'[' | b'!' if brackets && (b[i] == b'[' || b.get(i + 1) == Some(&b'[')) => {
                    let open = if b[i] == b'!' { i + 1 } else { i };
                    match find(b, open + 1, "]") {
                        None => {
                            brackets = false;
                            i = open + 1;
                        }
                        Some(close) if b.get(close + 1) == Some(&b'(') => {
                            let end = find(b, close + 1, ")").map_or(b.len(), |k| k + 1);
                            out.span(i, close + 1, Role::String);
                            out.span(close + 1, end, Role::Constant);
                            i = end;
                        }
                        Some(close) => i = close + 1,
                    }
                }
                b'<' if at(b, i, "<http") => {
                    let end = find(b, i, ">").map_or(b.len(), |k| k + 1);
                    out.span(i, end, Role::Constant);
                    i = end;
                }
                _ => i += 1,
            }
        }
    }
}

/// A diff, by each line's first byte.
fn diff(b: &[u8], out: &mut Out) {
    const HEADERS: [&[u8]; 14] = [
        b"+++ ",
        b"--- ",
        b"diff ",
        b"index ",
        b"Index: ",
        b"===",
        b"new file mode",
        b"deleted file mode",
        b"similarity index",
        b"rename from",
        b"rename to",
        b"old mode",
        b"new mode",
        b"Binary files",
    ];
    let role = if HEADERS.iter().any(|h| b.starts_with(h)) || b == b"+++" || b == b"---" {
        Role::Comment
    } else if b.starts_with(b"@@") {
        Role::Meta
    } else {
        match b.first() {
            Some(b'+' | b'>') => Role::Inserted,
            Some(b'-' | b'<') => Role::Deleted,
            _ => Role::Plain,
        }
    };
    out.span(0, b.len(), role);
}

// ------------------------------------------------------------------ the runs a line makes

struct Out<'r> {
    runs: &'r mut Vec<(usize, Role)>,
}

impl Out<'_> {
    /// From `at` on, `role`: a run of its own, or the last one's if it has the same.
    fn put(&mut self, at: usize, role: Role) {
        let runs = &mut *self.runs;
        match runs.last_mut() {
            Some(last) if last.1 == role => {}
            Some(last) if last.0 >= at => {
                last.1 = role;
                let n = runs.len();
                if n >= 2 && runs[n - 2].1 == role {
                    runs.pop();
                }
            }
            _ => runs.push((at, role)),
        }
    }

    /// `from..to` is `role`, and what follows plain until said otherwise.
    fn span(&mut self, from: usize, to: usize, role: Role) {
        if to > from {
            self.put(from, role);
            self.put(to, Role::Plain);
        }
    }
}

// ------------------------------------------------------------------ pieces

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\x0c' | b'\n')
}

fn skip_space(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(|&c| is_space(c)) {
        i += 1;
    }
    i
}

/// Whether `s` is at `b[i..]`.
fn at(b: &[u8], i: usize, s: &str) -> bool {
    b.get(i..).is_some_and(|r| r.starts_with(s.as_bytes()))
}

/// Where the next `s` from `i` is.
fn find(b: &[u8], i: usize, s: &str) -> Option<usize> {
    let s = s.as_bytes();
    let r = b.get(i..)?;
    match s {
        [] => Some(i),
        [one] => r.iter().position(|c| c == one).map(|p| i + p),
        _ => r.windows(s.len()).position(|w| w == s).map(|p| i + p),
    }
}

/// The byte after the bracket that closes the one at `open`, or the line's end.
fn close_bracket(b: &[u8], open: usize, o: u8, c: u8) -> usize {
    let mut depth = 0u32;
    for (k, &d) in b.iter().enumerate().skip(open) {
        if d == o {
            depth = depth.saturating_add(1);
        } else if d == c {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return k + 1;
            }
        }
    }
    b.len()
}

fn word_start(c: u8, lang: &Lang) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 || (c == b'$' && lang.word.contains(&b'$'))
}

fn word_byte(c: u8, lang: &Lang) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 || lang.word.contains(&c)
}

fn is_ident(w: &[u8]) -> bool {
    w.first()
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && w.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// What a shell word is made of: anything but space and the shell's own marks.
fn shell_byte(c: u8) -> bool {
    !is_space(c) && !b";&|()<>\"'`$\\{}=".contains(&c)
}

/// Where a shell variable at the `$` at `i` ends: `$NAME`, `$1`, `$@`, `${…}`, and in a
/// Makefile `$(…)`, `$<` and `$$NAME`. None for a `$` that names none, as `$(` in a shell.
fn shell_var(b: &[u8], i: usize, make: bool) -> Option<usize> {
    match *b.get(i + 1)? {
        b'{' => Some(close_bracket(b, i + 1, b'{', b'}')),
        b'(' if make => Some(close_bracket(b, i + 1, b'(', b')')),
        b'$' if make => Some(shell_var(b, i + 1, false).unwrap_or(i + 2)),
        c if c.is_ascii_alphabetic() || c == b'_' => {
            let mut j = i + 2;
            while b
                .get(j)
                .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_')
            {
                j += 1;
            }
            Some(j)
        }
        c if c.is_ascii_digit() || b"@*#?$!-".contains(&c) || (make && b"<^+%|".contains(&c)) => {
            Some(i + 2)
        }
        _ => None,
    }
}

/// What a lisp symbol is made of.
fn symbol_byte(c: u8) -> bool {
    !is_space(c) && !b"()[]{}\"';`,".contains(&c)
}

fn symbol_end(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(|&c| symbol_byte(c)) {
        i += 1;
    }
    i
}

/// The bytes of the character whose first byte is at `i`, none past the line's end.
fn char_len(b: &[u8], i: usize) -> usize {
    let n = match b.get(i) {
        None => 0,
        Some(c) if *c >= 0xf0 => 4,
        Some(c) if *c >= 0xe0 => 3,
        Some(c) if *c >= 0xc0 => 2,
        Some(_) => 1,
    };
    n.min(b.len().saturating_sub(i))
}

/// Where a character literal at the `'` at `i` ends: `'a'`, `'\n'`, `'\u{1F600}'`; none if
/// the quote opens none.
fn char_end(b: &[u8], i: usize) -> Option<usize> {
    let j = i + 1;
    match *b.get(j)? {
        b'\\' => (j + 2..b.len().min(i + 13))
            .find(|&k| b[k] == b'\'')
            .map(|k| k + 1),
        b'\'' => None,
        _ => {
            let n = char_len(b, j);
            (b.get(j + n) == Some(&b'\'')).then_some(j + n + 1)
        }
    }
}

/// Where a string whose text starts at `i` closes on this line.
fn closed(b: &[u8], mut i: usize, q: &Quote) -> Option<usize> {
    while i < b.len() {
        if q.escape && b[i] == b'\\' {
            i += 2;
        } else if !q.close.is_empty() && at(b, i, q.close) {
            return Some(i + q.close.len());
        } else {
            i += 1;
        }
    }
    None
}

/// The table's quote that opens at `i`, if one does.
fn quote_at(lang: &Lang, b: &[u8], i: usize) -> Option<usize> {
    lang.quotes.iter().position(|q| at(b, i, q.open))
}

/// A Lua long bracket's opening at the `[` at `i`: its level and its length.
fn long_open(b: &[u8], i: usize) -> Option<(u32, usize)> {
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') && j - i <= 256 {
        j += 1;
    }
    let level = j - i - 1;
    (b.get(j) == Some(&b'[') && level <= 255).then_some((level as u32, j + 1 - i))
}

/// Where a regular expression from the `/` at `i` ends, flags and all; none if nothing on the
/// line closes it.
fn regex_end(b: &[u8], i: usize) -> Option<usize> {
    let (mut k, mut class) = (i + 1, false);
    while k < b.len() {
        match b[k] {
            b'\\' => k += 1,
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class && k > i + 1 => {
                k += 1;
                while b.get(k).is_some_and(u8::is_ascii_alphabetic) {
                    k += 1;
                }
                return Some(k);
            }
            b'/' if !class => return None,
            _ => {}
        }
        k += 1;
    }
    None
}

/// Where a number from `i` ends: `0x1F`, `1_000`, `1.5e-3`, `10u32`, `2.0f64`, `0b1010`.
fn number_end(b: &[u8], mut i: usize) -> usize {
    let digits = |b: &[u8], mut k: usize| {
        while b.get(k).is_some_and(|&c| c.is_ascii_digit() || c == b'_') {
            k += 1;
        }
        k
    };
    let digit = |k: usize| b.get(k).is_some_and(u8::is_ascii_digit);
    if b.get(i) == Some(&b'0') && matches!(b.get(i + 1), Some(b'x' | b'X')) {
        i += 2;
        while b
            .get(i)
            .is_some_and(|&c| c.is_ascii_hexdigit() || c == b'_')
        {
            i += 1;
        }
    } else {
        i = digits(b, i);
        if b.get(i) == Some(&b'.') && digit(i + 1) {
            i = digits(b, i + 1);
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            let k = if matches!(b.get(i + 1), Some(b'+' | b'-')) {
                i + 2
            } else {
                i + 1
            };
            if digit(k) {
                i = digits(b, k);
            }
        }
    }
    while b
        .get(i)
        .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_')
    {
        i += 1;
    }
    i
}

/// Whether a whole word is a number: YAML's scalars, lisp's atoms.
fn is_number(w: &[u8]) -> bool {
    let w = w
        .strip_prefix(b"-")
        .or_else(|| w.strip_prefix(b"+"))
        .unwrap_or(w);
    match w {
        [d, ..] if d.is_ascii_digit() => number_end(w, 0) == w.len(),
        [b'.', d, ..] if d.is_ascii_digit() => number_end(w, 1) == w.len(),
        _ => false,
    }
}

/// A C preprocessor line's directive at the `#` at `i`, and an `#include`'s `<file>`.
fn directive(b: &[u8], i: usize, out: &mut Out) -> usize {
    let w0 = skip_space(b, i + 1);
    let mut j = w0;
    while b.get(j).is_some_and(u8::is_ascii_alphabetic) {
        j += 1;
    }
    out.span(i, j, Role::Meta);
    if matches!(&b[w0..j], b"include" | b"import" | b"include_next") {
        let k = skip_space(b, j);
        if b.get(k) == Some(&b'<') {
            let end = find(b, k, ">").map_or(b.len(), |e| e + 1);
            out.span(k, end, Role::String);
            return end;
        }
    }
    j
}

/// Whether the CSS statement from `i` is a nested rule's selector: a `{` comes before its end;
/// or, where nothing on the line ends it, it doesn't start as a declaration does, with a
/// property's name and a colon, as a selector list over lines doesn't.
fn nested(b: &[u8], i: usize) -> bool {
    match b[i..].iter().find(|&&c| matches!(c, b'{' | b';' | b'}')) {
        Some(&c) => c == b'{',
        None => {
            let mut j = i;
            while b
                .get(j)
                .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                j += 1;
            }
            let k = skip_space(b, j);
            let colon = b.get(k) == Some(&b':') && b.get(k + 1).is_none_or(|&c| is_space(c));
            !(j > i && colon)
        }
    }
}

/// A Makefile's value or a rule's prerequisites from `i`: text, with its variables and its
/// comment; in a `rule`, to a `;`, where the recipe starts, if one does.
fn make_text(b: &[u8], mut i: usize, rule: bool, out: &mut Out) -> Option<usize> {
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'#' => {
                out.span(i, b.len(), Role::Comment);
                return None;
            }
            b';' if rule => return Some(i),
            b'$' => {
                let end = shell_var(b, i, true).unwrap_or(i + 1);
                out.span(i, end, Role::Variable);
                i = end;
            }
            _ => i += 1,
        }
    }
    None
}

/// Where a JSX name from `i` ends: a tag's, `Foo.Bar`, `svg:rect`, an attribute's.
fn jsx_name(b: &[u8], mut i: usize) -> usize {
    while b
        .get(i)
        .is_some_and(|&c| c.is_ascii_alphanumeric() || b"-_:.$".contains(&c))
    {
        i += 1;
    }
    i
}

/// Where the next run of exactly `n` backticks from `i` ends.
fn tick_run(b: &[u8], mut i: usize, n: usize) -> Option<usize> {
    while i < b.len() {
        if b[i] == b'`' {
            let run = b[i..].iter().take_while(|&&c| c == b'`').count();
            if run == n {
                return Some(i + n);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// A YAML key at `i`: where it ends and where its colon is.
fn yaml_key(b: &[u8], i: usize) -> Option<(usize, usize)> {
    match *b.get(i)? {
        q @ (b'"' | b'\'') => {
            let end = closed(
                b,
                i + 1,
                &quote(if q == b'"' { "\"" } else { "'" }, q == b'"', false),
            )?;
            let k = skip_space(b, end);
            let colon = b.get(k) == Some(&b':') && b.get(k + 1).is_none_or(|&c| is_space(c));
            colon.then_some((end, k))
        }
        b'|' | b'>' | b'&' | b'*' | b'!' | b'#' | b'[' | b'{' | b'%' | b'@' | b'`' | b',' => None,
        _ => {
            for j in i..b.len() {
                if b[j] == b':' && b.get(j + 1).is_none_or(|&c| is_space(c)) {
                    let mut end = j;
                    while end > i && is_space(b[end - 1]) {
                        end -= 1;
                    }
                    return Some((end, j));
                }
                if b[j] == b'#' && j > i && is_space(b[j - 1]) {
                    return None;
                }
            }
            None
        }
    }
}

/// Whether a sorted list has a word: as it is, or lowercased where the language's words are
/// any case.
fn has(list: &[&str], w: &[u8], fold: bool) -> bool {
    if list.is_empty() || w.len() > 32 {
        return false;
    }
    let mut low = [0u8; 32];
    let w = match fold {
        true => {
            low[..w.len()].copy_from_slice(w);
            low[..w.len()].make_ascii_lowercase();
            &low[..w.len()]
        }
        false => w,
    };
    list.binary_search_by(|k| k.as_bytes().cmp(w)).is_ok()
}

// ------------------------------------------------------------------ the languages

/// What drives the reading of a language's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Tokens, by the table.
    Code,
    /// A shell's words, variables and heredocs.
    Shell,
    /// A shell's, on `$ ` lines only.
    Console,
    /// Code whose strings before a colon are keys.
    Json,
    /// Code whose lines start with a key or a section: TOML and INI.
    Conf,
    Yaml,
    Lisp,
    Markup,
    Css,
    Diff,
    Markdown,
    /// A Makefile's rules, with the shell's words in its recipes.
    Make,
    /// A Dockerfile's instructions, with the shell's words after them.
    Docker,
}

/// How a word before a quote makes the quote a string of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefixes {
    None,
    /// `r"…"`, `r#"…"#`, `b"…"`, `b'…'`.
    Rust,
    /// `r'…'`, `b"…"`, `f"…"`, `rb'…'`.
    Python,
    /// `L"…"`, `u8"…"`, `R"(…)"`.
    C,
    /// PostgreSQL's `E'…'`.
    Sql,
}

/// A string's quotes.
#[derive(Debug)]
struct Quote {
    open: &'static str,
    /// What closes it; nothing for a string to the line's end.
    close: &'static str,
    /// A backslash escapes the byte after it.
    escape: bool,
    /// It may go on over lines.
    lines: bool,
    /// `$NAME` in it is a variable's.
    vars: bool,
}

const fn quote(open: &'static str, escape: bool, lines: bool) -> Quote {
    Quote {
        open,
        close: open,
        escape,
        lines,
        vars: false,
    }
}

/// A language: what its comments, strings and words are. Every word list is sorted, as bytes,
/// for a binary search, and lowercase where the language's words are any case.
#[derive(Debug)]
struct Lang {
    kind: Kind,
    /// What starts a comment to the line's end.
    line: &'static [&'static str],
    /// What opens and closes a block comment, and whether they nest.
    block: Option<(&'static str, &'static str)>,
    nest: bool,
    /// What opens and closes a pragma, on one line: Haskell's `{-# … #-}`.
    pragma: Option<(&'static str, &'static str)>,
    /// Its strings, the longer of two openings that start alike first.
    quotes: &'static [Quote],
    /// `'` opens a character literal, or is plain.
    chars: bool,
    prefixes: Prefixes,
    keywords: &'static [&'static str],
    /// Its built-in types, in `prims`: a keyword's colour, as GitHub draws C's `int` and Go's
    /// `string`; a constant's for TypeScript's `number`. A type's own name is a type's.
    types: &'static [&'static str],
    prims: Role,
    /// Functions and objects the language comes with: drawn as GitHub draws them, as constants.
    builtins: &'static [&'static str],
    constants: &'static [&'static str],
    /// Keywords the next word after is a function's name, a type's or a variable's.
    defs: &'static [&'static str],
    type_defs: &'static [&'static str],
    var_defs: &'static [&'static str],
    /// Its words are any case.
    fold: bool,
    /// Bytes a word has besides letters, digits and `_`.
    word: &'static [u8],
    /// A capitalised word is a type's name.
    caps: bool,
    /// A word in capitals is a constant's name, as `MAX_LEN` is; not in Haskell, whose `IO` is
    /// a type's.
    upper: bool,
    /// Bytes that start a variable's name: Ruby's `@` and `$`.
    sigils: &'static [u8],
    /// Ruby's `:symbol`, `key:`, `empty?`.
    symbols: bool,
    /// `#` first on a line starts a preprocessor directive.
    hash_line: bool,
    /// `#[…]` is an attribute.
    attrs: bool,
    /// `@name` is a decorator or an annotation.
    at: bool,
    /// `name!(…)` is a macro's call.
    macros: bool,
    /// A `/` where an operand goes starts a regular expression.
    regex: bool,
    /// A `<` where an operand goes starts a JSX element.
    jsx: bool,
    /// Ruby's `<<~WORD` starts a heredoc.
    heredocs: bool,
    /// Lua's long brackets: `[[…]]`, `--[==[…]==]`.
    long: bool,
    /// A word before `=` is an attribute's name: Nix's.
    eq_keys: bool,
    /// What ends a key at a line's start: TOML's and INI's.
    key_seps: &'static [u8],
}

impl Lang {
    fn shell(&self) -> bool {
        matches!(
            self.kind,
            Kind::Shell | Kind::Console | Kind::Make | Kind::Docker
        )
    }
}

const BASE: Lang = Lang {
    kind: Kind::Code,
    line: &[],
    block: None,
    nest: false,
    pragma: None,
    quotes: &[],
    chars: false,
    prefixes: Prefixes::None,
    keywords: &[],
    types: &[],
    prims: Role::Keyword,
    builtins: &[],
    constants: &[],
    defs: &[],
    type_defs: &[],
    var_defs: &[],
    fold: false,
    word: &[],
    caps: false,
    upper: true,
    sigils: &[],
    symbols: false,
    hash_line: false,
    attrs: false,
    at: false,
    macros: false,
    regex: false,
    jsx: false,
    heredocs: false,
    long: false,
    eq_keys: false,
    key_seps: &[],
};

const SLASHES: &[&str] = &["//"];
const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));
const HASH: &[&str] = &["#"];
const DQ: Quote = quote("\"", true, false);
const SQ: Quote = quote("'", true, false);

/// The language a fence's info string names: its first word, any case, by its usual names.
fn language(info: &str) -> Option<&'static Lang> {
    let name = info.trim_start_matches(['{', '.']);
    let end = name
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}'))
        .unwrap_or(name.len());
    let name = name[..end].to_ascii_lowercase();
    Some(match name.as_str() {
        "rust" | "rs" => &RUST,
        "sh" | "bash" | "shell" | "zsh" | "ksh" => &SH,
        "console" | "shell-session" | "shellsession" | "sh-session" => &CONSOLE,
        "toml" => &TOML,
        "ini" | "cfg" | "conf" | "dosini" | "editorconfig" | "gitconfig" | "properties" => &INI,
        "json" | "jsonc" | "json5" | "geojson" => &JSON,
        "yaml" | "yml" => &YAML,
        "python" | "py" | "python3" | "py3" => &PYTHON,
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => &JS,
        "typescript" | "ts" | "mts" | "cts" => &TS,
        "tsx" => &TSX,
        "c" | "h" => &C,
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => &CPP,
        "go" | "golang" => &GO,
        "java" => &JAVA,
        "kotlin" | "kt" | "kts" => &KOTLIN,
        "swift" => &SWIFT,
        "ruby" | "rb" => &RUBY,
        "lua" => &LUA,
        "elisp" | "emacs-lisp" | "el" | "lisp" | "common-lisp" | "scheme" | "scm" | "racket"
        | "rkt" | "clojure" | "clj" | "cljs" | "edn" => &LISP,
        "sql" | "postgresql" | "postgres" | "psql" | "plsql" | "sqlite" => &SQL,
        "mysql" | "mariadb" => &MYSQL,
        "html" | "htm" | "xhtml" | "xml" | "svg" | "plist" | "xsl" | "rss" | "atom" => &HTML,
        "css" => &CSS,
        "scss" | "less" => &SCSS,
        "diff" | "patch" | "udiff" => &DIFF,
        "markdown" | "md" | "mdown" | "mkd" => &MARKDOWN,
        "dockerfile" | "docker" | "containerfile" => &DOCKER,
        "makefile" | "make" | "mk" | "mak" => &MAKE,
        "nix" => &NIX,
        "zig" => &ZIG,
        "haskell" | "hs" => &HASKELL,
        _ => return None,
    })
}

const RUST: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    nest: true,
    quotes: &[quote("\"", true, true)],
    chars: true,
    prefixes: Prefixes::Rust,
    keywords: &[
        "Self", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
        "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "type",
        "union", "unsafe", "use", "where", "while", "yield",
    ],
    types: &[
        "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize", "str", "u128",
        "u16", "u32", "u64", "u8", "usize",
    ],
    constants: &["Err", "None", "Ok", "Some", "false", "true"],
    defs: &["fn"],
    type_defs: &["enum", "struct", "trait", "type", "union"],
    caps: true,
    attrs: true,
    macros: true,
    ..BASE
};

const SH_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in", "select",
    "then", "time", "until", "while",
];
const SH_BUILTINS: &[&str] = &[
    "alias", "bg", "break", "builtin", "cd", "command", "continue", "declare", "echo", "eval",
    "exec", "exit", "export", "fg", "getopts", "hash", "jobs", "kill", "let", "local", "popd",
    "printf", "pushd", "pwd", "read", "readonly", "return", "set", "shift", "source", "test",
    "trap", "type", "typeset", "ulimit", "umask", "unalias", "unset", "wait",
];
const SH_QUOTES: &[Quote] = &[
    Quote {
        open: "$'",
        close: "'",
        ..quote("'", true, true)
    },
    Quote {
        vars: true,
        ..quote("\"", true, true)
    },
    quote("'", false, true),
];

const SH: Lang = Lang {
    kind: Kind::Shell,
    quotes: SH_QUOTES,
    keywords: SH_KEYWORDS,
    builtins: SH_BUILTINS,
    constants: &["false", "true"],
    defs: &["function"],
    ..BASE
};

const CONSOLE: Lang = Lang {
    kind: Kind::Console,
    ..SH
};

const MAKE: Lang = Lang {
    kind: Kind::Make,
    ..SH
};

const DOCKER: Lang = Lang {
    kind: Kind::Docker,
    ..SH
};

const MAKE_DIRECTIVES: &[&str] = &[
    "-include", "define", "else", "endef", "endif", "export", "ifdef", "ifeq", "ifndef", "ifneq",
    "include", "override", "sinclude", "undefine", "unexport", "vpath",
];

const DOCKER_INSTRUCTIONS: &[&str] = &[
    "add",
    "arg",
    "cmd",
    "copy",
    "entrypoint",
    "env",
    "expose",
    "from",
    "healthcheck",
    "label",
    "maintainer",
    "onbuild",
    "run",
    "shell",
    "stopsignal",
    "user",
    "volume",
    "workdir",
];

const TOML: Lang = Lang {
    kind: Kind::Conf,
    line: HASH,
    quotes: &[
        quote("\"\"\"", true, true),
        quote("'''", false, true),
        DQ,
        quote("'", false, false),
    ],
    constants: &["false", "inf", "nan", "true"],
    key_seps: b"=",
    ..BASE
};

const INI: Lang = Lang {
    kind: Kind::Conf,
    line: &["#", ";"],
    quotes: &[DQ, SQ],
    constants: &["false", "no", "off", "on", "true", "yes"],
    key_seps: b"=:",
    ..BASE
};

const JSON: Lang = Lang {
    kind: Kind::Json,
    line: SLASHES,
    block: C_BLOCK,
    quotes: &[DQ, SQ],
    constants: &["false", "null", "true"],
    ..BASE
};

const YAML: Lang = Lang {
    kind: Kind::Yaml,
    quotes: &[quote("\"", true, true), quote("'", false, true)],
    ..BASE
};

const YAML_CONSTANTS: &[&str] = &[
    ".inf", ".nan", "FALSE", "False", "NULL", "Null", "TRUE", "True", "false", "null", "true", "~",
];

const PYTHON: Lang = Lang {
    line: HASH,
    quotes: &[
        quote("\"\"\"", true, true),
        quote("'''", true, true),
        DQ,
        SQ,
    ],
    prefixes: Prefixes::Python,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
        "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
        "yield",
    ],
    builtins: &[
        "abs",
        "all",
        "any",
        "bool",
        "bytes",
        "dict",
        "dir",
        "enumerate",
        "filter",
        "float",
        "format",
        "getattr",
        "hasattr",
        "input",
        "int",
        "isinstance",
        "iter",
        "len",
        "list",
        "map",
        "max",
        "min",
        "next",
        "object",
        "open",
        "print",
        "range",
        "repr",
        "reversed",
        "set",
        "setattr",
        "sorted",
        "str",
        "sum",
        "super",
        "tuple",
        "type",
        "zip",
    ],
    constants: &["Ellipsis", "False", "None", "NotImplemented", "True"],
    defs: &["def"],
    type_defs: &["class"],
    caps: true,
    at: true,
    ..BASE
};

const JS_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const JS_BUILTINS: &[&str] = &[
    "Array",
    "Boolean",
    "Date",
    "Error",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Reflect",
    "RegExp",
    "Set",
    "String",
    "Symbol",
    "WeakMap",
    "console",
    "document",
    "globalThis",
    "module",
    "process",
    "require",
    "window",
];
const JS_QUOTES: &[Quote] = &[DQ, SQ, quote("`", true, true)];
const JS_CONSTANTS: &[&str] = &["Infinity", "NaN", "false", "null", "true", "undefined"];

const JS: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    quotes: JS_QUOTES,
    keywords: JS_KEYWORDS,
    builtins: JS_BUILTINS,
    constants: JS_CONSTANTS,
    defs: &["function"],
    type_defs: &["class"],
    word: b"$",
    caps: true,
    at: true,
    regex: true,
    // Where an operand goes a `<` is no JavaScript, so it's JSX in any block.
    jsx: true,
    ..BASE
};

const TS: Lang = Lang {
    keywords: &[
        "abstract",
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "declare",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "implements",
        "import",
        "in",
        "infer",
        "instanceof",
        "interface",
        "is",
        "keyof",
        "let",
        "namespace",
        "new",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "satisfies",
        "set",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "try",
        "type",
        "typeof",
        "var",
        "while",
        "with",
        "yield",
    ],
    types: &[
        "any", "bigint", "boolean", "never", "number", "object", "string", "symbol", "unknown",
        "void",
    ],
    prims: Role::Constant,
    type_defs: &["class", "enum", "interface", "namespace", "type"],
    // `<T>(x: T) => x` and `<T>value` are TypeScript's, with no JSX.
    jsx: false,
    ..JS
};

const TSX: Lang = Lang { jsx: true, ..TS };

const C_TYPES: &[&str] = &[
    "FILE",
    "bool",
    "char",
    "double",
    "float",
    "int",
    "int16_t",
    "int32_t",
    "int64_t",
    "int8_t",
    "long",
    "ptrdiff_t",
    "short",
    "signed",
    "size_t",
    "ssize_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "uint8_t",
    "uintptr_t",
    "unsigned",
    "void",
];
const C_BUILTINS: &[&str] = &[
    "calloc", "exit", "fprintf", "free", "malloc", "memcpy", "memset", "printf", "puts", "realloc",
    "snprintf", "sprintf", "strcmp", "strcpy", "strlen",
];

const C: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    quotes: &[DQ],
    chars: true,
    prefixes: Prefixes::C,
    keywords: &[
        "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern",
        "for", "goto", "if", "inline", "register", "restrict", "return", "sizeof", "static",
        "struct", "switch", "typedef", "union", "volatile", "while",
    ],
    types: C_TYPES,
    builtins: C_BUILTINS,
    constants: &["EOF", "NULL", "false", "stderr", "stdin", "stdout", "true"],
    type_defs: &["enum", "struct", "union"],
    caps: true,
    hash_line: true,
    ..BASE
};

const CPP: Lang = Lang {
    keywords: &[
        "alignas",
        "alignof",
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "co_await",
        "co_return",
        "co_yield",
        "const",
        "const_cast",
        "consteval",
        "constexpr",
        "continue",
        "decltype",
        "default",
        "delete",
        "do",
        "dynamic_cast",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "reinterpret_cast",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "try",
        "typedef",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    types: &[
        "FILE",
        "bool",
        "char",
        "char16_t",
        "char32_t",
        "char8_t",
        "double",
        "float",
        "int",
        "int16_t",
        "int32_t",
        "int64_t",
        "int8_t",
        "long",
        "ptrdiff_t",
        "short",
        "signed",
        "size_t",
        "ssize_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "uint8_t",
        "uintptr_t",
        "unsigned",
        "void",
        "wchar_t",
    ],
    builtins: &["cerr", "cin", "cout", "endl", "printf"],
    constants: &["NULL", "false", "nullptr", "true"],
    type_defs: &["class", "enum", "struct", "union"],
    ..C
};

const GO: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    quotes: &[DQ, quote("`", false, true)],
    chars: true,
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "type",
        "var",
    ],
    types: &[
        "any",
        "bool",
        "byte",
        "complex128",
        "complex64",
        "error",
        "float32",
        "float64",
        "int",
        "int16",
        "int32",
        "int64",
        "int8",
        "rune",
        "string",
        "uint",
        "uint16",
        "uint32",
        "uint64",
        "uint8",
        "uintptr",
    ],
    builtins: &[
        "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make",
        "max", "min", "new", "panic", "print", "println", "real", "recover",
    ],
    constants: &["false", "iota", "nil", "true"],
    defs: &["func"],
    type_defs: &["type"],
    ..BASE
};

const JAVA: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    quotes: &[quote("\"\"\"", true, true), DQ],
    chars: true,
    keywords: &[
        "abstract",
        "assert",
        "break",
        "case",
        "catch",
        "class",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extends",
        "final",
        "finally",
        "for",
        "if",
        "implements",
        "import",
        "instanceof",
        "interface",
        "native",
        "new",
        "package",
        "permits",
        "private",
        "protected",
        "public",
        "record",
        "return",
        "sealed",
        "static",
        "strictfp",
        "super",
        "switch",
        "synchronized",
        "this",
        "throw",
        "throws",
        "transient",
        "try",
        "var",
        "volatile",
        "while",
        "yield",
    ],
    types: &[
        "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
    ],
    constants: &["false", "null", "true"],
    type_defs: &["class", "enum", "interface", "record"],
    caps: true,
    at: true,
    ..BASE
};

const KOTLIN: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    nest: true,
    quotes: &[quote("\"\"\"", false, true), DQ],
    chars: true,
    keywords: &[
        "abstract",
        "as",
        "break",
        "by",
        "catch",
        "class",
        "companion",
        "const",
        "continue",
        "data",
        "do",
        "else",
        "enum",
        "finally",
        "for",
        "fun",
        "if",
        "import",
        "in",
        "init",
        "inline",
        "interface",
        "internal",
        "is",
        "lateinit",
        "object",
        "open",
        "operator",
        "out",
        "override",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "sealed",
        "super",
        "suspend",
        "this",
        "throw",
        "try",
        "typealias",
        "val",
        "var",
        "vararg",
        "when",
        "where",
        "while",
    ],
    constants: &["false", "null", "true"],
    defs: &["fun"],
    type_defs: &["class", "interface", "object", "typealias"],
    caps: true,
    at: true,
    ..BASE
};

const SWIFT: Lang = Lang {
    line: SLASHES,
    block: C_BLOCK,
    nest: true,
    quotes: &[quote("\"\"\"", true, true), DQ],
    keywords: &[
        "Self",
        "as",
        "associatedtype",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "continue",
        "default",
        "defer",
        "deinit",
        "do",
        "else",
        "enum",
        "extension",
        "fallthrough",
        "fileprivate",
        "final",
        "for",
        "func",
        "guard",
        "if",
        "import",
        "in",
        "init",
        "inout",
        "internal",
        "is",
        "lazy",
        "let",
        "mutating",
        "open",
        "operator",
        "override",
        "private",
        "protocol",
        "public",
        "repeat",
        "required",
        "rethrows",
        "return",
        "self",
        "some",
        "static",
        "struct",
        "subscript",
        "super",
        "switch",
        "throw",
        "throws",
        "try",
        "typealias",
        "var",
        "weak",
        "where",
        "while",
    ],
    builtins: &["print"],
    constants: &["false", "nil", "true"],
    defs: &["func"],
    type_defs: &[
        "class",
        "enum",
        "extension",
        "protocol",
        "struct",
        "typealias",
    ],
    caps: true,
    at: true,
    hash_line: true,
    ..BASE
};

const RUBY: Lang = Lang {
    line: HASH,
    quotes: &[
        quote("\"", true, true),
        quote("'", true, true),
        quote("`", true, true),
    ],
    keywords: &[
        "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do",
        "else", "elsif", "end", "ensure", "for", "if", "in", "module", "next", "not", "or", "redo",
        "rescue", "retry", "return", "self", "super", "then", "undef", "unless", "until", "when",
        "while", "yield",
    ],
    builtins: &[
        "attr_accessor",
        "attr_reader",
        "attr_writer",
        "extend",
        "include",
        "lambda",
        "loop",
        "p",
        "print",
        "private",
        "proc",
        "protected",
        "public",
        "puts",
        "raise",
        "require",
        "require_relative",
    ],
    constants: &["false", "nil", "true"],
    defs: &["def"],
    type_defs: &["class", "module"],
    caps: true,
    sigils: b"@$",
    symbols: true,
    regex: true,
    heredocs: true,
    ..BASE
};

const LUA: Lang = Lang {
    line: &["--"],
    quotes: &[DQ, SQ],
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in",
        "local", "not", "or", "repeat", "return", "then", "until", "while",
    ],
    builtins: &[
        "assert",
        "error",
        "getmetatable",
        "io",
        "ipairs",
        "math",
        "next",
        "os",
        "pairs",
        "pcall",
        "print",
        "require",
        "select",
        "setmetatable",
        "string",
        "table",
        "tonumber",
        "tostring",
        "type",
        "unpack",
    ],
    constants: &["false", "nil", "true"],
    defs: &["function"],
    long: true,
    ..BASE
};

const LISP: Lang = Lang {
    kind: Kind::Lisp,
    block: Some(("#|", "|#")),
    nest: true,
    quotes: &[quote("\"", true, true)],
    keywords: &[
        "&aux",
        "&body",
        "&key",
        "&optional",
        "&rest",
        "and",
        "case",
        "catch",
        "cl-case",
        "cl-defmacro",
        "cl-defmethod",
        "cl-defstruct",
        "cl-defun",
        "cl-flet",
        "cl-labels",
        "cl-letf",
        "cl-loop",
        "cond",
        "condition-case",
        "def",
        "defclass",
        "defconst",
        "defcustom",
        "defface",
        "defgroup",
        "define",
        "define-derived-mode",
        "define-minor-mode",
        "define-syntax",
        "defmacro",
        "defmethod",
        "defn",
        "defparameter",
        "defstruct",
        "defsubst",
        "defun",
        "defvar",
        "defvar-local",
        "do",
        "dolist",
        "dotimes",
        "fn",
        "if",
        "if-let",
        "ignore-errors",
        "interactive",
        "lambda",
        "let",
        "let*",
        "letrec",
        "loop",
        "ns",
        "or",
        "pcase",
        "pcase-let",
        "prog1",
        "progn",
        "quote",
        "recur",
        "save-excursion",
        "save-restriction",
        "setf",
        "setq",
        "setq-local",
        "throw",
        "unless",
        "unwind-protect",
        "use-package",
        "when",
        "when-let",
        "while",
        "with-current-buffer",
        "with-eval-after-load",
        "with-temp-buffer",
    ],
    builtins: &[
        "add-hook",
        "add-to-list",
        "append",
        "apply",
        "assoc",
        "car",
        "cdr",
        "concat",
        "cons",
        "define-key",
        "eq",
        "equal",
        "error",
        "format",
        "funcall",
        "gethash",
        "global-set-key",
        "insert",
        "keymap-set",
        "list",
        "mapc",
        "mapcar",
        "member",
        "message",
        "nth",
        "null",
        "provide",
        "push",
        "remove-hook",
        "require",
        "string-match",
        "user-error",
    ],
    constants: &["false", "nil", "t", "true"],
    defs: &[
        "cl-defmacro",
        "cl-defmethod",
        "cl-defun",
        "define",
        "defmacro",
        "defmethod",
        "defn",
        "defsubst",
        "defun",
    ],
    type_defs: &["cl-defstruct", "defclass", "defstruct"],
    var_defs: &[
        "def",
        "defconst",
        "defcustom",
        "defface",
        "defgroup",
        "defparameter",
        "defvar",
        "defvar-local",
    ],
    ..BASE
};

const SQL: Lang = Lang {
    line: &["--"],
    block: C_BLOCK,
    quotes: &[
        quote("'", false, true),
        quote("\"", false, false),
        quote("`", false, false),
        // Only after its `E`, which the word sees.
        Quote {
            open: "E'",
            close: "'",
            ..quote("'", true, true)
        },
    ],
    prefixes: Prefixes::Sql,
    keywords: &[
        "add",
        "all",
        "alter",
        "and",
        "as",
        "asc",
        "begin",
        "between",
        "by",
        "case",
        "check",
        "column",
        "commit",
        "constraint",
        "create",
        "cross",
        "database",
        "default",
        "delete",
        "desc",
        "distinct",
        "drop",
        "else",
        "end",
        "exists",
        "foreign",
        "from",
        "full",
        "grant",
        "group",
        "having",
        "if",
        "in",
        "index",
        "inner",
        "insert",
        "interval",
        "into",
        "is",
        "join",
        "key",
        "left",
        "like",
        "limit",
        "not",
        "offset",
        "on",
        "or",
        "order",
        "outer",
        "primary",
        "references",
        "returning",
        "right",
        "rollback",
        "select",
        "set",
        "table",
        "then",
        "transaction",
        "union",
        "unique",
        "update",
        "using",
        "values",
        "view",
        "when",
        "where",
        "with",
    ],
    types: &[
        "bigint",
        "blob",
        "boolean",
        "char",
        "date",
        "decimal",
        "double",
        "float",
        "int",
        "integer",
        "json",
        "jsonb",
        "numeric",
        "real",
        "serial",
        "smallint",
        "text",
        "time",
        "timestamp",
        "uuid",
        "varchar",
    ],
    builtins: &["avg", "coalesce", "count", "max", "min", "now", "sum"],
    constants: &["false", "null", "true"],
    fold: true,
    ..BASE
};

/// MySQL's strings take a backslash's escapes, and a `#` starts a comment.
const MYSQL: Lang = Lang {
    line: &["#", "--"],
    quotes: &[
        quote("'", true, true),
        quote("\"", true, false),
        quote("`", false, false),
    ],
    ..SQL
};

const HTML: Lang = Lang {
    kind: Kind::Markup,
    block: Some(("<!--", "-->")),
    ..BASE
};

const CSS: Lang = Lang {
    kind: Kind::Css,
    block: C_BLOCK,
    quotes: &[DQ, SQ],
    ..BASE
};

const SCSS: Lang = Lang {
    line: SLASHES,
    ..CSS
};

const DIFF: Lang = Lang {
    kind: Kind::Diff,
    ..BASE
};

const MARKDOWN: Lang = Lang {
    kind: Kind::Markdown,
    ..BASE
};

const NIX: Lang = Lang {
    line: HASH,
    block: C_BLOCK,
    quotes: &[quote("''", false, true), quote("\"", true, true)],
    keywords: &[
        "assert", "else", "if", "in", "inherit", "let", "or", "rec", "then", "with",
    ],
    builtins: &[
        "abort",
        "baseNameOf",
        "builtins",
        "derivation",
        "dirOf",
        "fetchGit",
        "fetchTarball",
        "fetchurl",
        "import",
        "isNull",
        "map",
        "removeAttrs",
        "throw",
        "toString",
    ],
    constants: &["false", "null", "true"],
    word: b"-'",
    eq_keys: true,
    ..BASE
};

const ZIG: Lang = Lang {
    line: SLASHES,
    quotes: &[
        Quote {
            close: "",
            ..quote("\\\\", false, false)
        },
        DQ,
    ],
    chars: true,
    keywords: &[
        "align",
        "allowzero",
        "and",
        "anyframe",
        "asm",
        "async",
        "await",
        "break",
        "catch",
        "comptime",
        "const",
        "continue",
        "defer",
        "else",
        "enum",
        "errdefer",
        "error",
        "export",
        "extern",
        "fn",
        "for",
        "if",
        "inline",
        "noalias",
        "nosuspend",
        "opaque",
        "or",
        "orelse",
        "packed",
        "pub",
        "resume",
        "return",
        "struct",
        "suspend",
        "switch",
        "test",
        "threadlocal",
        "try",
        "union",
        "unreachable",
        "usingnamespace",
        "var",
        "volatile",
        "while",
    ],
    types: &[
        "anyerror",
        "anyopaque",
        "anytype",
        "bool",
        "c_int",
        "c_long",
        "comptime_float",
        "comptime_int",
        "f128",
        "f16",
        "f32",
        "f64",
        "i128",
        "i16",
        "i32",
        "i64",
        "i8",
        "isize",
        "noreturn",
        "type",
        "u128",
        "u16",
        "u32",
        "u64",
        "u8",
        "usize",
        "void",
    ],
    constants: &["false", "null", "true", "undefined"],
    defs: &["fn"],
    caps: true,
    at: true,
    ..BASE
};

const HASKELL: Lang = Lang {
    line: &["--"],
    block: Some(("{-", "-}")),
    nest: true,
    pragma: Some(("{-#", "#-}")),
    quotes: &[DQ],
    chars: true,
    keywords: &[
        "as",
        "case",
        "class",
        "data",
        "default",
        "deriving",
        "do",
        "else",
        "forall",
        "foreign",
        "hiding",
        "if",
        "import",
        "in",
        "infix",
        "infixl",
        "infixr",
        "instance",
        "let",
        "module",
        "newtype",
        "of",
        "qualified",
        "then",
        "type",
        "where",
    ],
    builtins: &[
        "filter", "foldl", "foldr", "fst", "head", "map", "mapM_", "print", "putStrLn", "return",
        "show", "snd", "tail", "zip",
    ],
    constants: &["False", "Nothing", "True"],
    type_defs: &["class", "data", "newtype", "type"],
    word: b"'",
    caps: true,
    upper: false,
    ..BASE
};

/// Every language, for the tests.
#[cfg(test)]
const ALL: &[(&str, &Lang)] = &[
    ("rust", &RUST),
    ("sh", &SH),
    ("console", &CONSOLE),
    ("make", &MAKE),
    ("docker", &DOCKER),
    ("toml", &TOML),
    ("ini", &INI),
    ("json", &JSON),
    ("yaml", &YAML),
    ("python", &PYTHON),
    ("js", &JS),
    ("ts", &TS),
    ("tsx", &TSX),
    ("c", &C),
    ("cpp", &CPP),
    ("go", &GO),
    ("java", &JAVA),
    ("kotlin", &KOTLIN),
    ("swift", &SWIFT),
    ("ruby", &RUBY),
    ("lua", &LUA),
    ("elisp", &LISP),
    ("sql", &SQL),
    ("mysql", &MYSQL),
    ("html", &HTML),
    ("css", &CSS),
    ("scss", &SCSS),
    ("diff", &DIFF),
    ("markdown", &MARKDOWN),
    ("nix", &NIX),
    ("zig", &ZIG),
    ("haskell", &HASKELL),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::{Palette, Rgb};

    /// The role of every byte of `text` highlighted as `lang`, each line's runs checked.
    fn roles(lang: &str, text: &str) -> Vec<Role> {
        let mut h = Highlighter::new(lang).expect("a language known here");
        let (mut runs, mut all) = (Vec::new(), Vec::new());
        for line in text.split('\n') {
            h.line(line, &mut runs);
            check_runs(&runs, line, lang);
            let mut k = 0;
            for at in 0..line.len() {
                while k + 1 < runs.len() && runs[k + 1].0 <= at {
                    k += 1;
                }
                all.push(runs[k].1);
            }
            all.push(Role::Plain);
        }
        all
    }

    #[track_caller]
    fn check_runs(runs: &[(usize, Role)], line: &str, lang: &str) {
        assert_eq!(runs.first().map(|r| r.0), Some(0), "{lang}: {line:?}");
        assert!(
            runs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1),
            "{lang}: runs out of order on {line:?}: {runs:?}"
        );
        assert!(runs.iter().all(|r| r.0 <= line.len()), "{lang}: {line:?}");
    }

    /// The role of the first `needle` in `text`, after checking every byte of it has the same.
    /// A needle may name its word in a context, as `"&'a ⟨str⟩"` does.
    #[track_caller]
    fn role(lang: &str, text: &str, needle: &str) -> Role {
        let all = roles(lang, text);
        let (before, word) = match needle.split_once('⟨') {
            Some((before, rest)) => (before, rest.split_once('⟩').map_or(rest, |w| w.0)),
            None => ("", needle),
        };
        let context = needle.replace(['⟨', '⟩'], "");
        let found = text
            .find(&context)
            .unwrap_or_else(|| panic!("{needle:?} in the sample"));
        let at = found + before.len();
        let got = all[at];
        let whole = &all[at..at + word.len()];
        assert!(
            whole.iter().all(|r| *r == got),
            "{lang}: {needle:?} is {whole:?}"
        );
        got
    }

    #[track_caller]
    fn expect(lang: &str, text: &str, cases: &[(&str, Role)]) {
        for (needle, want) in cases {
            assert_eq!(role(lang, text, needle), *want, "{lang}: {needle:?}");
        }
    }

    #[test]
    fn rust_has_each_of_its_roles() {
        let text = r##"#[derive(Debug, Clone)]
/// A point.
pub struct Point<'a> {
    name: &'a str, /* a block
       comment */ x: f64,
}

fn main() {
    let v: Vec<String> = Vec::new();
    let raw = r#"say "hi""#;
    let c = '\n';
    println!("{} {}", MAX_LEN, 0x1F_u32);
    if v.is_empty() { return Some(true); }
}"##;
        use Role::*;
        expect(
            "rust",
            text,
            &[
                ("#[derive(Debug, Clone)]", Meta),
                ("/// A point.", Comment),
                ("pub", Keyword),
                ("Point", Type),
                ("&'a", Plain),
                ("&'a ⟨str⟩", Keyword),
                ("comment */", Comment),
                ("x:", Plain),
                ("f64", Keyword),
                ("fn", Keyword),
                ("main", Function),
                ("String", Type),
                ("new", Function),
                ("r#\"say \"hi\"\"#", String),
                ("'\\n'", String),
                ("println!", Function),
                ("\"{} {}\"", String),
                ("MAX_LEN", Constant),
                ("0x1F_u32", Number),
                ("is_empty", Function),
                ("Some", Constant),
                ("true", Constant),
            ],
        );
        // A comment nests, and strings span lines.
        expect(
            "rust",
            "/* a /* b */ c */ d\nlet s = \"one\ntwo\"; e",
            &[
                ("c */", Comment),
                ("*/ ⟨d⟩", Plain),
                ("two\"", String),
                ("; ⟨e⟩", Plain),
            ],
        );
    }

    #[test]
    fn a_shell_has_its_variables_words_and_heredocs() {
        let text = r#"#!/bin/bash
# Build it.
export PATH="$HOME/bin:${PATH}"
if [ -n "$1" ]; then
  echo 'done' && cargo build --release
fi
cat <<EOF > out.txt
literal $text
EOF
NAME=die; count=42
greet() { local who=$1; }"#;
        use Role::*;
        expect(
            "sh",
            text,
            &[
                ("#!/bin/bash", Meta),
                ("# Build it.", Comment),
                ("export", Constant),
                ("$HOME", Variable),
                ("/bin:", String),
                ("${PATH}", Variable),
                ("if", Keyword),
                ("'done'", String),
                ("cargo", Plain),
                ("--release", Plain),
                ("fi", Keyword),
                ("literal $text", String),
                ("NAME", Variable),
                ("42", Number),
                ("greet", Function),
                ("local", Constant),
            ],
        );
        assert_eq!(role("sh", text, "PATH"), Variable);
        assert_eq!(role("sh", text, "$1"), Variable);
        assert_eq!(role("sh", text, "EOF\n"), Plain, "the heredoc's end");
        // A console's prompt lines are commands; the rest, what they printed.
        expect(
            "console",
            "$ echo $HOME\n/home/me\n$ ls # list\n$ cd x && cargo test; test -f y",
            &[
                ("echo", Constant),
                ("$HOME", Variable),
                ("/home/me", Plain),
                ("# list", Comment),
                ("cd", Constant),
                ("cargo ⟨test⟩", Plain),
                ("; ⟨test⟩", Constant),
            ],
        );
    }

    #[test]
    fn python_has_each_of_its_roles() {
        let text = r#"@dataclass
class Point(Base):
    """A point,
    on two lines."""
    def norm(self, k=0.5e-3) -> float:
        # the length
        return sqrt(self.x ** 2) if self.x is not None else rb'\d'
print(f"{len(xs)}", MAX)"#;
        use Role::*;
        expect(
            "python",
            text,
            &[
                ("@dataclass", Meta),
                ("⟨class⟩ Point", Keyword),
                ("Point", Type),
                ("Base", Type),
                ("on two lines.\"\"\"", String),
                ("def", Keyword),
                ("norm", Function),
                ("self", Plain),
                ("0.5e-3", Number),
                ("float", Constant),
                ("# the length", Comment),
                ("sqrt", Function),
                ("None", Constant),
                ("rb'\\d'", String),
                ("print", Constant),
                ("f\"{len(xs)}\"", String),
                ("MAX", Constant),
            ],
        );
    }

    #[test]
    fn elisp_has_each_of_its_roles() {
        let text = r#";;; init.el --- my config
(defvar my-width 80 "The width.")
(defun my-greet (name &optional loud)
  "Say hello to NAME."
  (interactive "sName: ")
  (let ((msg (format "Hi %s" name)))
    (when loud (setq msg (upcase msg)))
    (message msg)))
(use-package evil :ensure t :config (evil-mode 1))
(add-hook 'prog-mode-hook #'display-line-numbers-mode)
(define-key map (kbd "C-c") ?\C-x nil)"#;
        use Role::*;
        expect(
            "elisp",
            text,
            &[
                (";;; init.el --- my config", Comment),
                ("defvar", Keyword),
                ("my-width", Variable),
                ("80", Number),
                ("\"The width.\"", String),
                ("defun", Keyword),
                ("my-greet", Function),
                ("&optional", Keyword),
                ("\"Say hello to NAME.\"", String),
                ("interactive", Keyword),
                ("let", Keyword),
                ("format", Constant),
                ("when", Keyword),
                ("setq", Keyword),
                ("upcase", Plain),
                ("use-package", Keyword),
                (":ensure", Constant),
                ("'prog-mode-hook", Constant),
                ("#'display-line-numbers-mode", Function),
                ("?\\C-x", String),
                ("nil", Constant),
            ],
        );
        assert_eq!(
            roles("elisp", text)[text.find(" t ").unwrap() + 1],
            Constant
        );
    }

    #[test]
    fn json_keys_values_and_a_diffs_lines() {
        let json = r#"{
  "name": "die", "count": 3, "ok": true,
  // a comment, in jsonc
  "list": [null, -1.5e3, "x"]
}"#;
        use Role::*;
        expect(
            "json",
            json,
            &[
                ("\"name\"", Constant),
                ("\"die\"", String),
                ("3", Number),
                ("true", Constant),
                ("// a comment, in jsonc", Comment),
                ("\"list\"", Constant),
                ("null", Constant),
                ("1.5e3", Number),
                ("\"x\"", String),
            ],
        );
        let diff =
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@ fn main\n same\n-old\n+new";
        expect(
            "diff",
            diff,
            &[
                ("diff --git a/x b/x", Comment),
                ("--- a/x", Comment),
                ("+++ b/x", Comment),
                ("@@ -1,2 +1,2 @@ fn main", Meta),
                (" same", Plain),
                ("-old", Deleted),
                ("+new", Inserted),
            ],
        );
    }

    #[test]
    fn the_rest_find_their_keys_tags_and_headings() {
        use Role::*;
        expect(
            "toml",
            "[package]\nname = \"die\" # it\nversion.workspace = true\n[[bin]]\ndeps = { a = 1, \"b-c\" = '2' }",
            &[
                ("[package]", Heading),
                ("name", Attr),
                ("\"die\"", String),
                ("# it", Comment),
                ("version.workspace", Attr),
                ("true", Constant),
                ("[[bin]]", Heading),
                ("{ ⟨a⟩ =", Attr),
                ("1", Number),
                ("\"b-c\"", Attr),
                ("'2'", String),
            ],
        );
        expect(
            "yaml",
            "on:\n  push:\n    branches: [main]\njobs:\n  build:\n    runs-on: ubuntu-latest # here\n    steps:\n      - name: \"Test\"\n        run: |\n          cargo test\n          echo ok\n      - uses: x@v4\n        with: { depth: 0, lfs: true }",
            &[
                ("on", Attr),
                ("branches", Attr),
                ("runs-on", Attr),
                ("ubuntu-latest", Plain),
                ("# here", Comment),
                ("name", Attr),
                ("\"Test\"", String),
                ("cargo test", String),
                ("echo ok", String),
                ("uses", Attr),
                ("depth", Attr),
                ("0", Number),
                ("true", Constant),
            ],
        );
        expect(
            "html",
            "<!-- a\ncomment -->\n<a href=\"/x\" class=y>Hi &amp; bye</a>\n<img\n  src='p.png' alt=\"\" />",
            &[
                ("comment -->", Comment),
                ("href", Attr),
                ("\"/x\"", String),
                ("class", Attr),
                ("Hi", Plain),
                ("&amp;", Constant),
                ("img", Tag),
                ("src", Attr),
                ("'p.png'", String),
                ("alt", Attr),
            ],
        );
        assert_eq!(role("html", "<a href=1>x</a>", "a"), Tag);
        assert_eq!(role("html", "<a href=y>x</a>", "y"), String);
        expect(
            "css",
            "/* site */\nbody, .card:hover > #main {\n  --gap: 4px;\n  margin: 0 auto !important;\n  color: #fff;\n  background: url(img/a.png);\n  width: calc(100% - var(--gap));\n}\n@media (max-width: 600px) { a { color: red; } }",
            &[
                ("/* site */", Comment),
                ("body", Tag),
                (".card", Function),
                (":hover", Function),
                ("#main", Function),
                ("--gap", Variable),
                ("4px", Number),
                ("margin", Constant),
                ("auto", Plain),
                ("!important", Keyword),
                ("#fff", Number),
                ("url", Function),
                ("img/a.png", String),
                ("calc", Function),
                ("100%", Number),
                ("@media", Keyword),
                ("600px", Number),
                ("red", Plain),
            ],
        );
        expect(
            "markdown",
            "# Title\n\nSome `code` and [a link](http://x).\n- item\n> quoted\n```sh\nls -la\n```\nafter",
            &[
                ("# Title", Heading),
                ("`code`", String),
                ("[a link]", String),
                ("(http://x)", Constant),
                ("-", Variable),
                ("> quoted", Tag),
                ("ls -la", String),
                ("after", Plain),
            ],
        );
        expect(
            "makefile",
            "CC ?= cc\n.PHONY: all\nall: die $(OBJS)\n\t$(CC) -o $@ main.c # link\ninclude rules.mk",
            &[
                ("CC", Variable),
                (".PHONY", Keyword),
                ("$(OBJS)", Variable),
                ("$(CC)", Variable),
                ("$@", Variable),
                ("# link", Comment),
                ("include", Keyword),
            ],
        );
        assert_eq!(role("makefile", "all: die", "all"), Function);
        expect(
            "dockerfile",
            "FROM rust:1.80 AS build\nRUN cargo build --release && \\\n    strip target/die\nENV HOME=/root",
            &[
                ("FROM", Keyword),
                ("RUN", Keyword),
                ("strip", Plain),
                ("ENV", Keyword),
                ("HOME", Variable),
            ],
        );
        expect(
            "c",
            "#include <stdio.h>\n/* a\n b */ int main(void) {\n  printf(\"%d\\n\", 'x', NULL);\n}",
            &[
                ("#include", Meta),
                ("<stdio.h>", String),
                ("b */", Comment),
                ("int", Keyword),
                ("main", Function),
                ("printf", Constant),
                ("\"%d\\n\"", String),
                ("'x'", String),
                ("NULL", Constant),
            ],
        );
        expect(
            "js",
            "const re = /a\\/b[/]/g; let n = a / b / c;\nfunction go() { return `x\n${y}`; }",
            &[
                ("/a\\/b[/]/g", String),
                ("a /", Plain),
                ("function", Keyword),
                ("go", Function),
                ("${y}`", String),
            ],
        );
        expect(
            "lua",
            "--[[ long\ncomment ]] local s = [==[ a ]] b ]==]",
            &[
                ("comment ]]", Comment),
                ("local", Keyword),
                ("[==[ a ]] b ]==]", String),
            ],
        );
        expect(
            "haskell",
            "{-# LANGUAGE GADTs #-}\nmain :: IO ()\nmain = putStrLn \"hi\" -- greet\nfoldl' f = 'a'",
            &[
                ("{-# LANGUAGE GADTs #-}", Meta),
                ("IO", Type),
                ("putStrLn", Constant),
                ("\"hi\"", String),
                ("-- greet", Comment),
                ("foldl'", Plain),
                ("'a'", String),
            ],
        );
        expect(
            "ruby",
            "def empty?(x)\n  @items.each { |i| puts :done, key: 1 }\nend",
            &[
                ("empty?", Function),
                ("@items", Variable),
                ("puts", Constant),
                (":done", Constant),
                ("key:", Constant),
            ],
        );
        expect(
            "sql",
            "SELECT name, COUNT(*) FROM users WHERE id = 'it''s' -- note",
            &[
                ("SELECT", Keyword),
                ("COUNT", Constant),
                ("users", Plain),
                ("'it''s'", String),
                ("-- note", Comment),
            ],
        );
        assert_eq!(Highlighter::new("text").map(|h| h.lang.kind), None);
        assert!(Highlighter::new("").is_none());
        assert!(Highlighter::new("Rust,ignore").is_some());
        assert!(Highlighter::new("{.python}").is_some());
    }

    #[test]
    fn a_quote_opens_no_string_where_the_language_reads_none() {
        use Role::*;
        // Ruby's regular expressions hold quotes and `#{…}`, and a `/` after an operand divides.
        expect(
            "ruby",
            "parts = line.split(/\"/)\ncount = parts.size\nok = name =~ /#{prefix}-\\d+/ && 1\nhalf = total / 2 / n",
            &[
                ("/\"/", String),
                ("count", Plain),
                ("/#{prefix}-\\d+/", String),
                ("&& ⟨1⟩", Number),
                ("total /", Plain),
                ("/ n", Plain),
            ],
        );
        // Its heredocs are strings to their word's line, and `<<` with a space after appends.
        expect(
            "ruby",
            "sql = <<~SQL\n  SELECT * FROM users WHERE name = 'Bob'\n  -- the user's rows\nSQL\nUser.find_by_sql(sql)\ntext = <<-'EOS'.strip\nDon't\n  EOS\nclass << self\n  items << value\nend\nputs text",
            &[
                ("the user's rows", String),
                ("SELECT", String),
                ("User", Type),
                ("find_by_sql", Function),
                ("strip", Plain),
                ("Don't", String),
                ("value", Plain),
                ("end", Keyword),
                ("puts", Constant),
            ],
        );
        // JSX's text is text, its tags are tags, and `{…}` in it is code again.
        let jsx = "const App = () => <p>Don't panic</p>;\n\
                   const List = ({ items }) => <ul>{items.map(i => <li key={i}>{i}</li>)}</ul>;\n\
                   const two = <><p>a</p><p>b</p></>;\n\
                   const n = a < b ? 1 : 2;\n\
                   function Card({ title }) {\n\
                   \x20 return (\n\
                   \x20   <div className=\"card\" onClick={() => go(title)}>\n\
                   \x20     <h2>{title}</h2>\n\
                   \x20     It's {count > 1 ? \"many\" : \"one\"} here.\n\
                   \x20   </div>\n\
                   \x20 );\n\
                   }\n\
                   const after = 'x';";
        for lang in ["jsx", "js", "tsx"] {
            expect(
                lang,
                jsx,
                &[
                    ("<⟨p⟩>Don't", Tag),
                    ("Don't panic", Plain),
                    ("</⟨p⟩>;", Tag),
                    ("<⟨ul⟩>", Tag),
                    ("map", Function),
                    ("key", Attr),
                    ("</⟨li⟩>", Tag),
                    ("</⟨ul⟩>", Tag),
                    ("<p>⟨b⟩", Plain),
                    ("</⟨p⟩></>", Tag),
                    ("a < b", Plain),
                    ("? ⟨1⟩", Number),
                    ("className", Attr),
                    ("\"card\"", String),
                    ("go", Function),
                    ("h2", Tag),
                    ("It's", Plain),
                    ("\"many\"", String),
                    ("here.", Plain),
                    ("</⟨div⟩>", Tag),
                    ("'x'", String),
                ],
            );
        }
        // A generic arrow function is no tag in TSX, and TypeScript has no JSX.
        expect(
            "tsx",
            "const id = <T,>(x: T) => x;\nconst s = 'y';",
            &[("'y'", String), ("=> ⟨x⟩", Plain)],
        );
        expect(
            "ts",
            "let s = <string>value;\nconst t = 'y';",
            &[("string", Constant), ("'y'", String)],
        );
        // A shell's shift is no heredoc, and `<<\WORD` is one.
        expect(
            "sh",
            "mask=$((1 << 4)); (( n <<= bits ))\necho \"mask is $mask\"\ncat <<\\EOF\nit's literal\nEOF\necho finished",
            &[
                ("\"mask is ", String),
                ("$mask", Variable),
                ("it's literal", String),
                ("⟨echo⟩ finished", Constant),
                ("finished", Plain),
            ],
        );
        expect(
            "console",
            "$ echo $((1<<2))\nout\n$ ls",
            &[("out", Plain), ("ls", Plain)],
        );
        // A Makefile's values are text, and a recipe's line or a Dockerfile's instruction ends
        // where its line does, unless a `\` goes on with it.
        expect(
            "makefile",
            "DESCRIPTION = It's a tool # what\nNAME := $(shell date)\nall: build ; @echo 'x'\n\t@echo it's ok\n\tcargo build --release",
            &[
                ("DESCRIPTION", Variable),
                ("It's a tool", Plain),
                ("# what", Comment),
                ("$(shell date)", Variable),
                ("all", Function),
                ("build", Plain),
                ("'x'", String),
                ("cargo", Plain),
                ("--release", Plain),
            ],
        );
        expect(
            "dockerfile",
            "MAINTAINER Jane O'Brien\nRUN echo 'a \\\n    b' && echo it's\nCMD [\"run\"]",
            &[
                ("O'Brien", Plain),
                ("RUN", Keyword),
                ("b'", String),
                ("CMD", Keyword),
                ("\"run\"", String),
            ],
        );
    }

    #[test]
    fn an_escape_is_where_the_dialect_has_one() {
        use Role::*;
        let text = "SELECT 'O\\'Brien' FROM authors; -- MySQL\nSELECT 1;";
        expect(
            "mysql",
            text,
            &[
                ("'O\\'Brien'", String),
                ("FROM", Keyword),
                ("-- MySQL", Comment),
                ("⟨SELECT⟩ 1", Keyword),
            ],
        );
        // Standard SQL takes no backslash, but PostgreSQL's E'…' does.
        expect(
            "sql",
            "SELECT 'C:\\' FROM t;\nSELECT E'O\\'Brien', e'\\n' FROM u;",
            &[
                ("'C:\\'", String),
                ("FROM", Keyword),
                ("E'O\\'Brien'", String),
                ("e'\\n'", String),
                ("⟨FROM⟩ u", Keyword),
            ],
        );
        // A Python raw string's backslash keeps its quote from closing it, and stays.
        expect(
            "python",
            "pat = re.compile(r\"\\\"(.*?)\\\"\")\ns = r\"\\\"\" + x",
            &[
                ("r\"\\\"(.*?)\\\"\"", String),
                ("compile", Function),
                ("r\"\\\"\"", String),
                ("+ ⟨x⟩", Plain),
            ],
        );
    }

    #[test]
    fn a_markdown_code_span_or_a_css_selector_is_found_past_what_isnt_one() {
        use Role::*;
        // A run of backticks with no match is text, and a run of another length can still be
        // a code span.
        expect(
            "markdown",
            "Use `` for an empty span, and `code` here.\nAnd ``a`b`` too.",
            &[("for an", Plain), ("`code`", String), ("``a`b``", String)],
        );
        // A selector list over lines, inside a block, is selectors; a value over lines is a
        // value.
        expect(
            "css",
            "@media (max-width: 600px) {\n  .a,\n  .b {\n    color: red;\n    font-family: Arial,\n      sans-serif;\n  }\n  h1,\n  h2 { margin: 0 }\n}",
            &[
                (".a", Function),
                (".b", Function),
                ("color", Constant),
                ("font-family", Constant),
                ("sans-serif", Plain),
                ("h1", Tag),
                ("h2", Tag),
                ("margin", Constant),
            ],
        );
    }

    #[test]
    fn what_is_open_at_a_blocks_end_stays_in_its_block() {
        use Role::*;
        // An unterminated string goes to the block's end, and a block after starts afresh.
        let all = roles("rust", "let s = \"never\nclosed");
        assert_eq!(all.last().copied(), Some(Plain));
        assert_eq!(all[all.len() - 2], String);
        assert_eq!(role("rust", "let s = 1;", "let"), Keyword);
        assert_eq!(role("python", "x = '''open\nstill", "still"), String);
        assert_eq!(role("c", "/* open\nstill", "still"), Comment);
    }

    #[test]
    fn every_word_list_is_sorted_for_its_search() {
        let words = |l: &Lang| {
            [
                ("keywords", l.keywords),
                ("types", l.types),
                ("builtins", l.builtins),
                ("constants", l.constants),
                ("defs", l.defs),
                ("type_defs", l.type_defs),
                ("var_defs", l.var_defs),
            ]
        };
        let extra = [
            ("make", "directives", MAKE_DIRECTIVES),
            ("docker", "instructions", DOCKER_INSTRUCTIONS),
            ("yaml", "constants", YAML_CONSTANTS),
        ];
        let lists = ALL
            .iter()
            .flat_map(|(name, l)| words(l).map(|(what, list)| (*name, what, list, l.fold)))
            .chain(extra.map(|(name, what, list)| (name, what, list, name == "docker")));
        for (name, what, list, fold) in lists {
            for pair in list.windows(2) {
                assert!(pair[0] < pair[1], "{name}'s {what}: {pair:?} out of order");
            }
            if fold {
                assert!(
                    list.iter().all(|w| *w == w.to_ascii_lowercase()),
                    "{name}'s {what}"
                );
            }
        }
        // A word that names a definition is a keyword too, or nothing would see it.
        for (name, l) in ALL {
            for w in l.defs.iter().chain(l.type_defs).chain(l.var_defs) {
                assert!(has(l.keywords, w.as_bytes(), l.fold), "{name}: {w}");
            }
        }
    }

    /// Bytes no one would write, and the runs that make a careless scanner slow or lost.
    fn hostile() -> Vec<String> {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let alphabet = b"\"'`\\/*#[]{}()<>-=:;$@?!.,|&~%^+_ \t\naZ09\xc3\xa9\xe2\x86\x92";
        let mut texts: Vec<String> = (0..60)
            .map(|n| {
                let len = (next() % (40 * (n + 1))) as usize;
                let bytes: Vec<u8> = (0..len)
                    .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                    .collect();
                String::from_utf8_lossy(&bytes).into_owned()
            })
            .collect();
        let raw: Vec<u8> = (0..4000).map(|_| next() as u8).collect();
        texts.push(String::from_utf8_lossy(&raw).into_owned());
        for unit in [
            "[", "`", "\"", "'", "#", "/*", "{-", "#|", "<", "<!--", "$(", "${", "<<", "r#", "[=",
            "--[[", "/", "\\", ": ", "- ", "@", "?", "'a", "0x", ".5", "#[", "<a ", "&", "url(",
            "<a>", "<a {", "{<a>", "</a>", "<<~E ", "$((", "`` `", "E'", ".a,",
        ] {
            texts.push(unit.repeat(20_000));
            texts.push(
                format!("x {}", unit.repeat(5_000))
                    .replace(' ', "\n")
                    .repeat(2),
            );
        }
        texts
    }

    #[test]
    fn nothing_makes_it_panic_or_take_long() {
        let texts = hostile();
        for (name, _) in ALL {
            for text in &texts {
                roles(name, text);
            }
        }
    }

    #[test]
    fn every_code_block_in_the_docs_reads_in_every_language() {
        use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let Ok(dir) = std::fs::read_dir(&root) else {
            return eprintln!("skipped: no {}", root.display());
        };
        let mut blocks = Vec::new();
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let mut open: Option<(String, String)> = None;
            for event in Parser::new(&text) {
                match event {
                    Event::Start(Tag::CodeBlock(kind)) => {
                        let info = match kind {
                            CodeBlockKind::Fenced(info) => info.to_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        open = Some((info, String::new()));
                    }
                    Event::Text(t) => {
                        if let Some((_, body)) = &mut open {
                            body.push_str(&t);
                        }
                    }
                    Event::End(TagEnd::CodeBlock) => blocks.extend(open.take()),
                    _ => {}
                }
            }
        }
        assert!(!blocks.is_empty(), "the docs have code blocks");
        for (info, body) in &blocks {
            if Highlighter::new(info).is_some() {
                roles(info, body);
            }
            for (name, _) in ALL {
                roles(name, body);
            }
        }
    }

    #[test]
    fn mono_darks_code_colours_read_on_its_panel() {
        fn lum(c: Rgb) -> f64 {
            let lin = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
        }
        let contrast = |a: Rgb, b: Rgb| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        let p = Palette::mono_dark();
        let s = p.syntax;
        for (what, ink) in [
            ("keyword", s.keyword),
            ("string", s.string),
            ("comment", s.comment),
            ("number", s.number),
            ("function", s.function),
            ("type", s.type_),
            ("constant", s.constant),
            ("meta", s.meta),
            ("variable", s.variable),
            ("tag", s.tag),
            ("attr", s.attr),
            ("inserted", s.inserted),
            ("deleted", s.deleted),
            ("heading", s.heading),
        ] {
            assert!(contrast(ink, p.panel) >= 2.0, "{what}: {ink:?}");
        }
        // The comment was lifted from grayscale-dark's #525252, and only as far as it had to be.
        assert_eq!(s.comment, Rgb(0x54, 0x54, 0x54));
        assert!(contrast(Rgb(0x53, 0x53, 0x53), p.panel) < 2.0);
    }
}
