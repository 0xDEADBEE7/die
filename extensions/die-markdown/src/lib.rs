//! die-markdown: a rendered view of markdown in the buffer the file is in, as an extension
//! (scope D14). It depends on die-core alone, so it's platform-neutral and out of the product:
//! `cargo xtask lint` holds it to that. Its parts:
//!
//! - this file: [`Markdown`], the extension, with your [`Options`], the `markdown` mode a
//!   markdown file's buffer gets, the `markdown-toggle` and `markdown-follow-link` commands, and
//!   the faces the view draws in;
//! - [`plan`]: from markdown source to the decorations that make the view: what to hide, what to
//!   show in its place, which faces go where, and where the links lead. It knows nothing of the
//!   editor;
//! - `view`: the mode, one buffer's view: it puts a plan on the buffer and takes it off.
//!
//! The rendered view is the same buffer and the same text: markup is hidden under decorations,
//! never removed, so the cursor, the file and undo are what they were, and every key works in
//! either view. A cell grid has one size of letter, so a heading is bold and underlined, not
//! large. Nothing runs for a buffer whose view is raw.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod plan;
mod view;

pub use view::View;

use die_core::session::{Kept, Value};
use die_core::{
    Args, BufferId, Ctx, Error, Extension, FileTarget, Registry, Result, State, Style, Underline,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use view::Rendered;

/// The mode's name, which names its keymap: your config binds the toggle's key there.
pub const MODE: &str = "markdown";
/// The keymap over a link in the rendered view.
pub const LINK_KEYS: &str = "markdown-link";

/// Files a link leads to that DIE hands to [`Options::opener`] rather than open as text:
/// pictures, PDFs and web pages (web content goes to a real browser).
const OUTSIDE: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "pdf", "html", "htm",
];

// ------------------------------------------------------------------ options

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The file name extensions, without the dot and in any case, whose files get the mode.
    pub extensions: Vec<String>,
    /// Show front matter dimmed in the rendered view, where the default hides it, as your
    /// preview's export strips it.
    pub front_matter: bool,
    /// The program a link's address goes to when it isn't a file DIE opens: a web address, a
    /// picture, a PDF. None is `xdg-open` where there is one, and `open` otherwise.
    pub opener: Option<String>,
}

impl Default for Options {
    /// The extensions Emacs's markdown-mode takes, front matter hidden, the system's opener.
    fn default() -> Options {
        let extensions = ["md", "markdown", "mkd", "mdown", "mkdn", "mdwn"];
        Options {
            extensions: extensions.map(String::from).to_vec(),
            front_matter: false,
            opener: None,
        }
    }
}

// ------------------------------------------------------------------ the extension

/// The extension: your options, and which buffers' rendered view is on.
#[derive(Default)]
pub struct Markdown {
    /// Your options.
    pub options: Options,
    rendered: Rendered,
}

impl Markdown {
    /// The extension with these options.
    pub fn new(options: Options) -> Markdown {
        Markdown {
            options,
            rendered: Rendered::default(),
        }
    }

    /// `markdown-toggle`: the other view of the buffer, which gets the mode if it hasn't got it,
    /// so the command works on a buffer with no file too.
    fn toggle(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let buf = a.buffer.or(ctx.focused_buffer());
        let buf = buf.ok_or_else(|| Error::Msg("No buffer here".into()))?;
        ctx.enable_mode::<View>(buf)?;
        ctx.with_mode(buf, |v: &mut View, ctx| v.toggle(buf, ctx))?
    }

    /// `markdown-follow-link`: where the link under the cursor goes. It reads the text as it is,
    /// so it works in either view.
    fn follow(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let none = || Error::Msg("No link here".into());
        let buf = a.buffer.or(ctx.focused_buffer()).ok_or_else(none)?;
        let win = a.window.or(ctx.focused()).ok_or_else(none)?;
        let t = ctx.text(buf)?;
        let at = t.cursor(win).ok_or_else(none)?;
        let planned = plan::plan(&t.rope().to_string(), &LINKS);
        let link = planned.links.iter().find(|l| l.range.contains(&at));
        let link = link.ok_or_else(none)?;
        let rendered = ctx.with_mode(buf, |v: &mut View, _| v.is_rendered());
        match Dest::read(&link.dest) {
            Dest::Anchor(anchor) => {
                let pos = heading(&planned, &anchor)?;
                // A file's buffer goes by its name, which keeps the place left, for `C-o`.
                match ctx.file(buf).map(|f| f.path.clone()) {
                    Some(path) => self.go(ctx, path, buf, pos),
                    None => {
                        ctx.text_mut(buf)?.set_cursor(win, pos);
                        Ok(())
                    }
                }
            }
            Dest::Outside(address) => {
                self.outside(ctx, address);
                Ok(())
            }
            Dest::File(path, anchor) => {
                let path = ctx.directory(buf).unwrap_or(Path::new("/")).join(path);
                if !path.exists() {
                    return Err(Error::Msg(format!("No file {}", path.display())));
                }
                let kind = path.extension().and_then(|e| e.to_str());
                if link.image
                    || kind.is_some_and(|k| OUTSIDE.iter().any(|o| o.eq_ignore_ascii_case(k)))
                {
                    self.outside(ctx, path.display().to_string());
                    return Ok(());
                }
                let to = ctx.open_file(&path)?;
                // From a rendered view the file it leads to is rendered too, if it's markdown.
                if rendered == Ok(true) {
                    ctx.with_mode(to, |v: &mut View, ctx| match v.is_rendered() {
                        true => Ok(()),
                        false => v.render(to, ctx),
                    })
                    .unwrap_or(Ok(()))?;
                }
                let pos = match anchor {
                    Some(anchor) => {
                        let text = ctx.text(to)?.rope().to_string();
                        heading(&plan::plan(&text, &LINKS), &anchor)?
                    }
                    None => 0,
                };
                self.go(ctx, path, to, pos)
            }
        }
    }

    /// Show `path`'s buffer in this window with the cursor on the line `pos` is in, as `:e`
    /// would, keeping the place it leaves.
    fn go(&mut self, ctx: &mut Ctx, path: PathBuf, buf: BufferId, pos: usize) -> Result {
        let line = Some(ctx.text(buf)?.line_of(pos) + 1);
        let col = None;
        ctx.open_target(&FileTarget { path, line, col })?;
        Ok(())
    }

    /// Hand an address to your opener, on a thread of its own, and say so if it fails.
    fn outside(&mut self, ctx: &mut Ctx, address: String) {
        let opener = self.options.opener.clone();
        let path = die_core::process::path().to_owned();
        ctx.messages.push(format!("Opening {address}"));
        let said = address.clone();
        let work = move || {
            let run = |program: &str| {
                let mut cmd = Command::new(program);
                cmd.arg(&address).env("PATH", &path);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                cmd.status()
            };
            let status = match &opener {
                Some(program) => run(program),
                None => run("xdg-open").or_else(|_| run("open")),
            };
            match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(s.to_string()),
                Err(e) => Err(e.to_string()),
            }
        };
        let done = move |ctx: &mut Ctx, opened: std::result::Result<(), String>| {
            if let Err(why) = opened {
                ctx.messages.push(format!("Couldn't open {said}: {why}"));
            }
            Ok(())
        };
        ctx.spawn(work, done).detach();
    }
}

/// The plan `markdown-follow-link` reads links and headings from: nothing in it depends on a
/// width, and front matter stays, so nothing a link could be in is left out.
const LINKS: plan::Options = plan::Options {
    width: 0,
    tab_width: 0,
    front_matter: true,
};

/// Where the heading an anchor names starts, by GitHub's rule for anchors.
fn heading(planned: &plan::Plan, anchor: &str) -> Result<usize> {
    let slug = anchor.to_lowercase();
    let found = planned.headings.iter().find(|h| h.slug == slug);
    let found = found.ok_or_else(|| Error::Msg(format!("No heading #{anchor}")));
    Ok(found?.range.start)
}

/// Where a link leads, read from its address as written.
#[derive(Debug, PartialEq)]
enum Dest {
    /// A heading in this buffer: `#install`.
    Anchor(String),
    /// Anything with a scheme but `file:`: a web address, `mailto:`.
    Outside(String),
    /// A file, absolute or relative to this buffer's directory, and a heading in it.
    File(PathBuf, Option<String>),
}

impl Dest {
    fn read(written: &str) -> Dest {
        if let Some(anchor) = written.strip_prefix('#') {
            return Dest::Anchor(decoded(anchor));
        }
        // A scheme is two characters or more, so `C:` isn't one.
        let scheme = written.split_once(':').map(|(s, _)| s).filter(|s| {
            let rest = |c: char| c.is_ascii_alphanumeric() || "+.-".contains(c);
            s.len() > 1 && s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(rest)
        });
        let path = match scheme {
            Some(s) if s.eq_ignore_ascii_case("file") => {
                written[s.len() + 1..].trim_start_matches("//")
            }
            Some(_) => return Dest::Outside(written.into()),
            None => written,
        };
        let (path, anchor) = match path.split_once('#') {
            Some((path, anchor)) => (path, Some(decoded(anchor))),
            None => (path, None),
        };
        Dest::File(PathBuf::from(decoded(path)), anchor)
    }
}

/// `%20` and the like as the characters they stand for.
fn decoded(written: &str) -> String {
    let bytes = written.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) if bytes[i] == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Plan again, if `buf` has the mode and its view is on and behind.
fn refresh(ctx: &mut Ctx, buf: BufferId) -> Result {
    // No mode on it, or one that's busy: nothing of this extension's to do.
    ctx.with_mode(buf, |v: &mut View, ctx| v.refresh(buf, ctx))
        .unwrap_or(Ok(()))
}

/// Every face the view draws in: its name, the base face it's on, and what it adds. A theme
/// that styles only the base faces still draws a rendered file sensibly, and yours can style
/// each by name.
pub fn faces() -> [(&'static str, Option<&'static str>, Style); 18] {
    use Underline::{Double, Single};
    let s = Style::new;
    [
        (plan::H[0], Some("heading"), s().underline(Double)),
        (plan::H[1], Some("heading"), s().underline(Single)),
        (plan::H[2], Some("heading"), s()),
        (plan::H[3], Some("bold"), s()),
        (plan::H[4], Some("bold"), s()),
        (plan::H[5], Some("bold"), s()),
        (plan::BOLD, Some("bold"), s()),
        (plan::ITALIC, None, s().italic()),
        (plan::STRIKE, Some("shadow"), s().strike()),
        (plan::CODE, Some("highlight"), s()),
        (plan::CODE_BLOCK, Some("highlight"), s()),
        (plan::LINK, Some("link"), s()),
        (plan::QUOTE, None, s().italic()),
        (plan::MARKER, Some("shadow"), s()),
        (plan::RULE, Some("shadow"), s()),
        (plan::TABLE_HEADER, Some("bold"), s()),
        (plan::META, Some("shadow"), s()),
        (plan::ALERT, Some("bold"), s()),
    ]
}

impl Extension for Markdown {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn register(&mut self, reg: &mut Registry) {
        for (name, parent, style) in faces() {
            reg.face(name, parent, style);
        }
        let (rendered, front_matter) = (self.rendered.clone(), self.options.front_matter);
        let extensions = self.options.extensions.clone();
        reg.mode(MODE, move || View::new(rendered.clone(), front_matter))
            .files(move |path| {
                let kind = path.extension().and_then(|e| e.to_str());
                kind.is_some_and(|k| extensions.iter().any(|e| e.eq_ignore_ascii_case(k)))
            });
        reg.ext_command(
            "markdown-toggle",
            "Show this buffer's markdown rendered, or raw again: the markup hidden and the text styled, over the same text.",
            Markdown::toggle,
        );
        reg.ext_command(
            "markdown-follow-link",
            "Go where the link under the cursor leads: a heading here, a file in this window, or your browser.",
            Markdown::follow,
        );
        // On a link in the rendered view, where the cursor can only be on its text. No leader
        // key is taken: your config binds the toggle in the mode's keymap.
        reg.keymap(LINK_KEYS)
            .bind(State::Normal, "<CR>", "markdown-follow-link");
    }

    /// The two things a rendered view follows: its text, whoever changes it (your keys, undo, a
    /// file reloaded from disk), and its width, which a command can change with no window
    /// changing size (`\z`). Neither costs more than a comparison while nothing changed.
    fn init(&mut self, ctx: &mut Ctx) -> Result {
        ctx.on_edited(refresh).detach();
        ctx.on_command_ended(|ctx| match ctx.focused_buffer() {
            Some(buf) => refresh(ctx, buf),
            None => Ok(()),
        })
        .detach();
        Ok(())
    }

    /// Which buffers are rendered, so a restart shows them as they were.
    fn save_session(&self, ctx: &Ctx) -> Kept {
        let rendered = self.rendered.borrow();
        let live = rendered.iter().filter(|b| ctx.is_live(**b));
        Kept {
            state: Value::Null,
            buffers: live.map(|b| (*b, Value::Bool(true))).collect(),
        }
    }

    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        for (buf, _) in kept.buffers {
            ctx.enable_mode::<View>(buf)?;
            ctx.with_mode(buf, |v: &mut View, ctx| v.render(buf, ctx))??;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_says_where_it_leads() {
        let file = |p: &str, a: Option<&str>| Dest::File(p.into(), a.map(String::from));
        assert_eq!(Dest::read("#set-up"), Dest::Anchor("set-up".into()));
        assert_eq!(Dest::read("notes.md"), file("notes.md", None));
        assert_eq!(
            Dest::read("../a%20b/c.md#x-y"),
            file("../a b/c.md", Some("x-y"))
        );
        assert_eq!(Dest::read("file:///tmp/a.md"), file("/tmp/a.md", None));
        assert_eq!(Dest::read("C:notes.md"), file("C:notes.md", None));
        for outside in ["https://example.com/a#b", "mailto:you@example.com"] {
            assert_eq!(Dest::read(outside), Dest::Outside(outside.into()));
        }
    }
}
