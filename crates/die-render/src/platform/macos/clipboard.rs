//! The system clipboard: NSPasteboard behind [`Clipboard`] (scope §5, R5).
//!
//! Text goes on the pasteboard as a plain string, so any app can paste it. A line or block kind
//! goes beside it under a type of DIE's own. Another app's copy clears the pasteboard, and that
//! type with it, so what it copied reads as characters.

use super::catch::guarded;
use die_core::{Clipboard, RegKind, Register};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSException, NSString, ns_string};

/// The pasteboard type that holds a register's kind.
fn kind_type() -> &'static NSString {
    ns_string!("dev.die.register-kind")
}

/// The pasteboard type every app reads and writes text as.
fn text_type() -> &'static NSString {
    // SAFETY: AppKit sets this constant when it loads, and never changes it.
    unsafe { NSPasteboardTypeString }
}

/// Whether a copy with these types, the provider's favourite first, is text. Finder's copy of a file
/// has the file's name as text, and a browser's of an image can have its address, so neither is.
/// An app that offers text first with a picture of it, as a spreadsheet does, is.
fn is_text(types: &[String]) -> bool {
    const FILES: [&str; 2] = ["public.file-url", "NSFilenamesPboardType"];
    const IMAGES: [&str; 7] = [
        "public.tiff",
        "public.png",
        "public.jpeg",
        "public.heic",
        "com.compuserve.gif",
        "com.adobe.pdf",
        "NeXT TIFF v4.0 pasteboard type",
    ];
    let text = text_type().to_string();
    let Some(at) = types.iter().position(|t| *t == text) else {
        return false;
    };
    let names = |list: &[&str], t: &String| list.contains(&t.as_str());
    !types.iter().any(|t| names(&FILES, t)) && !types[..at].iter().any(|t| names(&IMAGES, t))
}

/// A pasteboard, as DIE's clipboard.
pub struct Pasteboard {
    /// Opened when it's first used, since opening one talks to the pasteboard server and can raise.
    board: Option<Retained<NSPasteboard>>,
    /// Whether this one made it, and so gives it back to the pasteboard server when dropped.
    unique: bool,
    /// See [`Pasteboard::raising`].
    raises: bool,
}

impl Pasteboard {
    /// The one every app shares. This doesn't touch it.
    pub fn general() -> Pasteboard {
        Pasteboard {
            board: None,
            unique: false,
            raises: false,
        }
    }

    /// A new one that nothing else knows about unless it's given the [`Pasteboard::name`]. Tests
    /// use these, so it panics if it can't.
    pub fn unique() -> Pasteboard {
        let board = guarded(
            "can't open a pasteboard",
            NSPasteboard::pasteboardWithUniqueName,
        );
        Pasteboard {
            board: Some(board.unwrap_or_else(|e| panic!("{e}"))),
            unique: true,
            raises: false,
        }
    }

    /// One that raises a real NSException at every call, as one does with the pasteboard server
    /// gone, which a test can't arrange otherwise.
    pub fn raising() -> Pasteboard {
        let mut board = Pasteboard::general();
        board.raises = true;
        board
    }

    /// The name another process can open it by. Empty if it hasn't been used.
    pub fn name(&self) -> String {
        let board = self.board.as_ref();
        board.map_or_else(String::new, |b| b.name().to_string())
    }

    /// Use the pasteboard, opening it if this is the first time.
    fn with<R>(&mut self, what: &str, f: impl FnOnce(&NSPasteboard) -> R) -> Result<R, String> {
        guarded(what, || {
            if self.raises {
                let name = ns_string!("NSPasteboardCommunicationException");
                let reason = ns_string!("the pasteboard server is gone");
                if let Some(e) = NSException::new(name, Some(reason), None) {
                    e.raise();
                }
            }
            f(self
                .board
                .get_or_insert_with(NSPasteboard::generalPasteboard))
        })
    }
}

impl Drop for Pasteboard {
    fn drop(&mut self) {
        if let Some(board) = self.board.as_ref().filter(|_| self.unique) {
            // SAFETY: `releaseGlobally` takes nothing and returns nothing, and this handle is the
            // last use of a pasteboard it made.
            let released = guarded("", || unsafe {
                let () = msg_send![&**board, releaseGlobally];
            });
            // There's no one to tell: only tests make these.
            drop(released);
        }
    }
}

impl Clipboard for Pasteboard {
    /// This, and not the change count, is what macOS's pasteboard privacy alert is about. It asks
    /// for the types first, which doesn't alert, and leaves what isn't text unread.
    fn read(&mut self) -> Result<Option<Register>, String> {
        self.with("can't read the clipboard", |board| {
            let types = board.types()?;
            let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
            if !is_text(&types) {
                return None;
            }
            let text = board.stringForType(text_type())?.to_string();
            let kind = board.stringForType(kind_type()).map(|k| k.to_string());
            let kind = match kind.as_deref() {
                Some("line") => RegKind::Line,
                Some("block") => RegKind::Block,
                Some("macro") => RegKind::Macro,
                _ => RegKind::Char,
            };
            Some(Register { kind, text })
        })
    }

    fn write(&mut self, r: &Register) -> Result<(), String> {
        let kind = match r.kind {
            RegKind::Char => ns_string!("char"),
            RegKind::Line => ns_string!("line"),
            RegKind::Block => ns_string!("block"),
            RegKind::Macro => ns_string!("macro"),
        };
        let text = NSString::from_str(&r.text);
        let what = "can't write the clipboard";
        let taken = self.with(what, |board| {
            board.clearContents();
            board.setString_forType(&text, text_type())
                && board.setString_forType(kind, kind_type())
        })?;
        taken
            .then_some(())
            .ok_or_else(|| format!("{what}: the pasteboard refused it"))
    }

    fn change_count(&mut self) -> Result<u64, String> {
        self.with("can't ask the clipboard if it changed", |board| {
            board.changeCount() as u64
        })
    }
}

/// The clipboard a running DIE uses.
pub fn system_clipboard() -> Option<Box<dyn Clipboard>> {
    Some(Box::new(Pasteboard::general()))
}
