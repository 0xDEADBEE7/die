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

#[cfg(test)]
mod tests {
    use super::*;

    /// A command's program and arguments.
    fn words(c: &Command) -> Vec<String> {
        let all = std::iter::once(c.get_program()).chain(c.get_args());
        all.map(|w| w.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn the_commands_are_the_first_programs_the_path_has() {
        let png = Path::new("/tmp/a \"b\".png");
        let mac = pasteboard_command(png, &|p| p == "osascript").unwrap();
        let script =
            "set the clipboard to (read (POSIX file \"/tmp/a \\\"b\\\".png\") as «class PNGf»)";
        assert_eq!(words(&mac), ["osascript", "-e", script]);
        let x = pasteboard_command(Path::new("/tmp/x.png"), &|p| p == "xclip").unwrap();
        let xclip = [
            "xclip",
            "-selection",
            "clipboard",
            "-t",
            "image/png",
            "-i",
            "/tmp/x.png",
        ];
        assert_eq!(words(&x), xclip);
        // wl-copy reads the file as its input, so a file that isn't there is no command.
        let gone = Path::new("/nonexistent/die-draw.png");
        assert!(pasteboard_command(gone, &|p| p == "wl-copy").is_none());
        assert!(pasteboard_command(png, &|_| false).is_none());
        let svg = Path::new("/tmp/x.svg");
        let linux = open_command(svg, &|p| p == "xdg-open").unwrap();
        assert_eq!(words(&linux), ["xdg-open", "/tmp/x.svg"]);
        assert_eq!(
            words(&open_command(svg, &|_| true).unwrap()),
            ["open", "/tmp/x.svg"]
        );
        assert!(open_command(svg, &|_| false).is_none());
        // `which` looks in each of the PATH's directories.
        let dir = std::env::temp_dir().join(format!("die-draw-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prog"), "").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), dir.as_path()]).unwrap();
        assert!(which("prog", &path) && !which("other", &path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
