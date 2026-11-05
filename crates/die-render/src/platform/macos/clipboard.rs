//! The system clipboard: NSPasteboard behind [`Clipboard`] (scope §5, R5).
//!
//! Text goes on the pasteboard as a plain string, so any app can paste it. A line or block kind
//! goes beside it under a type of DIE's own. Another app's copy clears the pasteboard, and that
//! type with it, so what it copied reads as characters.

use die_core::{Clipboard, RegKind, Register};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSString, ns_string};

/// The pasteboard type that holds a register's kind.
fn kind_type() -> &'static NSString {
    ns_string!("dev.die.register-kind")
}

/// The pasteboard type every app reads and writes text as.
fn text_type() -> &'static NSString {
    // SAFETY: AppKit sets this constant when it loads, and never changes it.
    unsafe { NSPasteboardTypeString }
}

/// A pasteboard, as DIE's clipboard.
pub struct Pasteboard {
    board: Retained<NSPasteboard>,
    /// Whether this one made it, and so gives it back to the pasteboard server when dropped.
    unique: bool,
}

impl Pasteboard {
    /// The one every app shares.
    pub fn general() -> Pasteboard {
        Pasteboard {
            board: NSPasteboard::generalPasteboard(),
            unique: false,
        }
    }

    /// A new one that nothing else knows about unless it's given the [`Pasteboard::name`].
    pub fn unique() -> Pasteboard {
        Pasteboard {
            board: NSPasteboard::pasteboardWithUniqueName(),
            unique: true,
        }
    }

    /// The name another process can open it by.
    pub fn name(&self) -> String {
        self.board.name().to_string()
    }
}

impl Drop for Pasteboard {
    fn drop(&mut self) {
        if self.unique {
            // SAFETY: `releaseGlobally` takes nothing and returns nothing, and this handle is the
            // last use of a pasteboard it made.
            let () = unsafe { msg_send![&*self.board, releaseGlobally] };
        }
    }
}

impl Clipboard for Pasteboard {
    fn read(&mut self) -> Option<Register> {
        let text = self.board.stringForType(text_type())?.to_string();
        let kind = self.board.stringForType(kind_type()).map(|k| k.to_string());
        let kind = match kind.as_deref() {
            Some("line") => RegKind::Line,
            Some("block") => RegKind::Block,
            Some("macro") => RegKind::Macro,
            _ => RegKind::Char,
        };
        Some(Register { kind, text })
    }

    fn write(&mut self, r: &Register) {
        let kind = match r.kind {
            RegKind::Char => ns_string!("char"),
            RegKind::Line => ns_string!("line"),
            RegKind::Block => ns_string!("block"),
            RegKind::Macro => ns_string!("macro"),
        };
        self.board.clearContents();
        self.board
            .setString_forType(&NSString::from_str(&r.text), text_type());
        self.board.setString_forType(kind, kind_type());
    }

    fn change_count(&mut self) -> u64 {
        self.board.changeCount() as u64
    }
}

/// The clipboard a running DIE uses.
pub fn system_clipboard() -> Option<Box<dyn Clipboard>> {
    Some(Box::new(Pasteboard::general()))
}
