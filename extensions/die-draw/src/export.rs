//! The command lines that put a PNG on the system pasteboard or open a file, chosen by what's
//! on the PATH rather than by the platform, so this module stays neutral. A job runs the
//! command itself.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// Whether a program named `name` is in one of `path`'s directories.
pub fn which(name: &str, path: &OsStr) -> bool {
    std::env::split_paths(path).any(|dir| dir.join(name).is_file())
}

/// `path` quoted for an AppleScript string.
fn applescript_string(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The command that puts the PNG at `png` on the pasteboard: `osascript` on macOS, `wl-copy`
/// then `xclip` on Linux, whichever `has` says is there. None if none is.
pub fn pasteboard_command(png: &Path, has: &dyn Fn(&str) -> bool) -> Option<Command> {
    if has("osascript") {
        let script = format!(
            "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
            applescript_string(png)
        );
        let mut c = Command::new("osascript");
        c.arg("-e").arg(script);
        return Some(c);
    }
    if has("wl-copy") {
        let mut c = Command::new("wl-copy");
        c.arg("--type").arg("image/png");
        c.stdin(std::fs::File::open(png).ok()?);
        return Some(c);
    }
    if has("xclip") {
        let mut c = Command::new("xclip");
        c.args(["-selection", "clipboard", "-t", "image/png", "-i"])
            .arg(png);
        return Some(c);
    }
    None
}

/// The command that opens `file` in the program the system gives it: `open` on macOS,
/// `xdg-open` on Linux, whichever `has` says is there.
pub fn open_command(file: &Path, has: &dyn Fn(&str) -> bool) -> Option<Command> {
    let program = ["open", "xdg-open"].into_iter().find(|p| has(p))?;
    let mut c = Command::new(program);
    c.arg(file);
    Some(c)
}
