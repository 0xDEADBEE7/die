//! The budget's line counter (docs/m0-plan.md §4): code lines after rustfmt,
//! without blank lines, comments (doc comments included) or `#[cfg(test)]` items.
//! `tests/` directories are left out by the caller.

use crate::util::{Result, show};
use std::io::Write;
use std::process::{Command, Stdio};

/// Format `src` with the pinned rustfmt's defaults, then count it.
pub fn count_formatted(src: &str) -> Result<usize> {
    let mut cmd = Command::new(rustfmt());
    cmd.args(["--edition", "2024", "--emit", "stdout", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", show(&cmd)))?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(src.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "rustfmt failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(count(&String::from_utf8_lossy(&out.stdout)))
}

fn rustfmt() -> std::path::PathBuf {
    let rustc = crate::util::rustc();
    match rustc.parent() {
        Some(dir) if dir.join("rustfmt").exists() => dir.join("rustfmt"),
        _ => "rustfmt".into(),
    }
}

#[derive(Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Punct(char),
    Lit,
}

/// Count the lines of `src` that hold code.
pub fn count(src: &str) -> usize {
    let lines = src.lines().count().max(1);
    let toks = lex(src);
    let mut code = vec![false; lines + 1];
    let mut i = 0;
    while i < toks.len() {
        if let Some(end) = cfg_test_item(&toks, i) {
            i = end;
            continue;
        }
        let (_, first, last) = toks[i];
        code[first..=last].fill(true);
        i += 1;
    }
    code.iter().filter(|&&c| c).count()
}

/// If a `#[cfg(test)]` attribute starts at `i`, the index just past the item it's on.
fn cfg_test_item(toks: &[(Tok, usize, usize)], i: usize) -> Option<usize> {
    use Tok::*;
    let want = [
        Punct('#'),
        Punct('['),
        Ident("cfg".into()),
        Punct('('),
        Ident("test".into()),
        Punct(')'),
        Punct(']'),
    ];
    if toks.len() < i + want.len() || toks[i..i + want.len()].iter().map(|t| &t.0).ne(want.iter()) {
        return None;
    }
    let mut j = i + want.len();
    // More attributes on the same item.
    while j + 1 < toks.len() && toks[j].0 == Punct('#') && toks[j + 1].0 == Punct('[') {
        j = skip_group(toks, j + 1)?;
    }
    // The item runs to its first `;` or `{...}` outside brackets and parentheses.
    while j < toks.len() {
        match toks[j].0 {
            Punct(';') => return Some(j + 1),
            Punct('{') => return skip_group(toks, j),
            Punct('(') | Punct('[') => j = skip_group(toks, j)?,
            _ => j += 1,
        }
    }
    Some(j)
}

/// The index just past the group opened at `open`.
pub fn skip_group(toks: &[(Tok, usize, usize)], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (j, (t, _, _)) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(j + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Tokens with the (0-based) lines they start and end on. Comments are dropped.
pub fn lex(src: &str) -> Vec<(Tok, usize, usize)> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 0);
    let ident_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    while i < c.len() {
        let ch = c[i];
        let start = line;
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if ch == '"' {
            i = string_end(&c, i + 1, 0, &mut line);
            out.push((Tok::Lit, start, line));
        } else if ch == '\'' {
            // A char literal ('a', '\n', '{') or a lifetime or label ('a, 'static).
            if c.get(i + 1) == Some(&'\\') {
                // Past the quote, the backslash and the escaped character.
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
                out.push((Tok::Lit, start, line));
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
                out.push((Tok::Lit, start, line));
            } else {
                i += 1;
                while i < c.len() && ident_char(c[i]) {
                    i += 1;
                }
                out.push((Tok::Lit, start, line));
            }
        } else if ident_char(ch) {
            let s = i;
            while i < c.len() && ident_char(c[i]) {
                i += 1;
            }
            let word: String = c[s..i].iter().collect();
            let prefix = matches!(word.as_str(), "r" | "b" | "br" | "c" | "cr");
            let raw = word.ends_with('r');
            if prefix && c.get(i) == Some(&'"') {
                i = if raw {
                    raw_end(&c, i + 1, 0, &mut line)
                } else {
                    string_end(&c, i + 1, 0, &mut line)
                };
                out.push((Tok::Lit, start, line));
            } else if prefix && raw && c.get(i) == Some(&'#') {
                let mut hashes = 0;
                while c.get(i + hashes) == Some(&'#') {
                    hashes += 1;
                }
                if c.get(i + hashes) == Some(&'"') {
                    i = raw_end(&c, i + hashes + 1, hashes, &mut line);
                    out.push((Tok::Lit, start, line));
                } else if word == "r" && hashes == 1 {
                    // A raw identifier, `r#match`, is that identifier: rustc reads
                    // `#[r#path = "x"]` as `#[path = "x"]`.
                    let s = i + 1;
                    i = s;
                    while i < c.len() && ident_char(c[i]) {
                        i += 1;
                    }
                    out.push((Tok::Ident(c[s..i].iter().collect()), start, line));
                } else {
                    out.push((Tok::Ident(word), start, line));
                }
            } else if word == "b" && c.get(i) == Some(&'\'') {
                i += if c.get(i + 1) == Some(&'\\') { 4 } else { 3 };
                while i < c.len() && c[i - 1] != '\'' {
                    i += 1;
                }
                out.push((Tok::Lit, start, line));
            } else {
                out.push((Tok::Ident(word), start, line));
            }
        } else {
            out.push((Tok::Punct(ch), start, line));
            i += 1;
        }
    }
    out
}

/// Past the closing quote of a string whose body starts at `i`.
fn string_end(c: &[char], mut i: usize, _hashes: usize, line: &mut usize) -> usize {
    while i < c.len() {
        match c[i] {
            '\\' => {
                if c.get(i + 1) == Some(&'\n') {
                    *line += 1;
                }
                i += 2;
            }
            '"' => return i + 1,
            '\n' => {
                *line += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// Past the closing `"###` of a raw string with `hashes` hashes.
fn raw_end(c: &[char], mut i: usize, hashes: usize, line: &mut usize) -> usize {
    while i < c.len() {
        if c[i] == '"' && (0..hashes).all(|k| c.get(i + 1 + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        if c[i] == '\n' {
            *line += 1;
        }
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::{Tok, count, lex};

    #[test]
    fn blank_lines_and_comments_dont_count() {
        let src = "// a comment\n\n/// docs\n//! inner docs\nfn f() {}\n/* block\n   comment */\nfn g() {} // trailing\n";
        assert_eq!(count(src), 2);
    }

    #[test]
    fn nested_block_comments() {
        let src = "/* outer /* inner */ still comment\n*/\nfn f() {}\n";
        assert_eq!(count(src), 1);
    }

    #[test]
    fn comment_markers_inside_strings_are_code() {
        let src =
            "let a = \"// not a comment\";\nlet b = \"/* nor this\";\nlet c = 1; /* but this */\n";
        assert_eq!(count(src), 3);
    }

    #[test]
    fn multi_line_strings_count_every_line() {
        let src = "let a = \"one\ntwo\nthree\";\nlet b = r#\"x \" {\n}\"#;\n";
        assert_eq!(count(src), 5);
    }

    #[test]
    fn cfg_test_modules_and_items_dont_count() {
        let src = "fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { let s = \"}\"; let c = '}'; }\n}\n#[cfg(test)]\nuse std::fmt;\n#[cfg(test)]\n#[allow(dead_code)]\nfn helper(x: [u8; 2]) -> u8 { x[0] }\nfn g() {}\n";
        assert_eq!(count(src), 2);
    }

    #[test]
    fn other_cfgs_count() {
        let src =
            "#[cfg(panic = \"abort\")]\ncompile_error!(\"x\");\n#[cfg(not(test))]\nfn f() {}\n";
        assert_eq!(count(src), 4);
    }

    #[test]
    fn lifetimes_and_char_literals() {
        let src = "fn f<'a>(x: &'a str) -> char {\n    let _ = '{';\n    let _ = '\\'';\n    let _ = b'}';\n    'x'\n}\n#[cfg(test)]\nmod t {\n    fn g<'a>(_: &'a ()) {}\n}\n";
        assert_eq!(count(src), 6);
    }

    #[test]
    fn a_raw_identifier_is_that_identifier() {
        let toks: Vec<Tok> = lex("r#path = r#\"s\"# r#a_b;")
            .into_iter()
            .map(|t| t.0)
            .collect();
        assert_eq!(
            toks,
            [
                Tok::Ident("path".into()),
                Tok::Punct('='),
                Tok::Lit,
                Tok::Ident("a_b".into()),
                Tok::Punct(';'),
            ]
        );
    }

    #[test]
    fn raw_identifiers_and_byte_strings() {
        let src = "let r#match = br\"{\";\nlet x = b\"}\";\n#[cfg(test)]\nmod t {}\n";
        assert_eq!(count(src), 2);
    }
}
