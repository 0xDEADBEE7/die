//! The little of HTML a markdown page reads: tags with their attributes, text, and entities.
//! Not a parser of documents: it never fails, and what it doesn't understand is text.

use std::borrow::Cow;

/// A piece of raw HTML.
#[derive(Debug, PartialEq)]
pub(super) enum Token<'a> {
    /// Text between tags, entities still in it.
    Text(&'a str),
    /// An opening tag: its name in lower case, and its attributes, names in lower case and
    /// values decoded.
    Open(String, Vec<(String, String)>),
    /// A closing tag.
    Close(String),
}

/// The pieces of `html`, each with where it starts. Comments, doctypes and processing
/// instructions are dropped here.
pub(super) fn tokens(html: &str) -> Vec<(Token<'_>, usize)> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let (mut at, mut text) = (0, 0);
    while at < bytes.len() {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        let rest = &html[at..];
        let next = bytes.get(at + 1).copied().unwrap_or(b' ');
        let after = bytes.get(at + 2).copied().unwrap_or(b' ');
        let (len, token) = if let Some(comment) = rest.strip_prefix("<!--") {
            (comment.find("-->").map_or(rest.len(), |e| e + 7), None)
        } else if next == b'!' || next == b'?' {
            (rest.find('>').map_or(rest.len(), |e| e + 1), None)
        } else if next.is_ascii_alphabetic() || (next == b'/' && after.is_ascii_alphabetic()) {
            let len = tag_end(rest);
            (len, Some(tag(&rest[1..len])))
        } else {
            // A lone `<` is text.
            at += 1;
            continue;
        };
        if text < at {
            out.push((Token::Text(&html[text..at]), text));
        }
        out.extend(token.map(|t| (t, at)));
        at += len;
        text = at;
    }
    if text < html.len() {
        out.push((Token::Text(&html[text..]), text));
    }
    out
}

/// How long the tag at the start of `rest` is: through its `>`, which a quoted value may hold.
fn tag_end(rest: &str) -> usize {
    let mut quote = None;
    for (i, b) in rest.bytes().enumerate() {
        match (quote, b) {
            (None, b'>') => return i + 1,
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), b) if q == b => quote = None,
            _ => {}
        }
    }
    rest.len()
}

/// A tag from what's between its `<` and the end of its `>`.
fn tag(inner: &str) -> Token<'_> {
    let inner = inner.strip_suffix('>').unwrap_or(inner);
    let name_of = |s: &str| {
        let end = s.find(|c: char| !c.is_ascii_alphanumeric());
        s[..end.unwrap_or(s.len())].to_ascii_lowercase()
    };
    if let Some(closing) = inner.strip_prefix('/') {
        return Token::Close(name_of(closing));
    }
    let name = name_of(inner);
    let mut rest = &inner[name.len()..];
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            break;
        }
        let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '=' | '/'));
        let end = end.unwrap_or(rest.len());
        let key = rest[..end].to_ascii_lowercase();
        rest = rest[end..].trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let quoted = after.chars().next().filter(|c| matches!(c, '"' | '\''));
            (value, rest) = match quoted {
                Some(q) => {
                    let close = after[1..].find(q).map_or(after.len(), |e| e + 1);
                    (&after[1..close], after.get(close + 1..).unwrap_or(""))
                }
                None => {
                    let close = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
        }
        // A value with no name before it is dropped: its `=` is past, so the loop goes on.
        if !key.is_empty() {
            attrs.push((key, decode(value).into_owned()));
        }
    }
    Token::Open(name, attrs)
}

/// Text with its entities read: the numeric ones, and the named ones a README uses.
pub(super) fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..].find(|c: char| !c.is_ascii_alphanumeric() && c != '#');
        let found = end.filter(|e| rest.as_bytes().get(e + 1) == Some(&b';'));
        match found.and_then(|e| Some((entity(&rest[1..=e])?, e + 2))) {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "bull" => '•',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "times" => '×',
        "laquo" => '«',
        "raquo" => '»',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "deg" => '°',
        "check" => '✓',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_text_and_comments_come_apart() {
        let all =
            tokens("a <b>x</b><!-- gone --><img src='p.png' width=20 alt=\"a > b\"> &amp; <3");
        let kinds: Vec<&Token<'_>> = all.iter().map(|(t, _)| t).collect();
        let attrs = |pairs: &[(&str, &str)]| {
            let own = |(k, v): &(&str, &str)| (k.to_string(), v.to_string());
            pairs.iter().map(own).collect::<Vec<_>>()
        };
        assert_eq!(
            kinds,
            [
                &Token::Text("a "),
                &Token::Open("b".into(), vec![]),
                &Token::Text("x"),
                &Token::Close("b".into()),
                &Token::Open(
                    "img".into(),
                    attrs(&[("src", "p.png"), ("width", "20"), ("alt", "a > b")])
                ),
                &Token::Text(" &amp; <3"),
            ]
        );
        assert_eq!(
            decode("&lt;a&gt; &amp; &#65;&#x42; &unknown; & b"),
            "<a> & AB &unknown; & b"
        );
        // Whatever it's given, it ends.
        for odd in [
            "<",
            "<a",
            "<a b=",
            "<a b='",
            "</",
            "<!--",
            "<a =>",
            "<a b=\"c\"d>",
        ] {
            let _ = tokens(odd);
        }
    }
}
