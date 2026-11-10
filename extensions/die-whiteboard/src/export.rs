//! Export and the pasteboard: PNG encoding, and the command lines that put a PNG on the system
//! pasteboard or open a file in the browser, chosen by what's on the PATH rather than by the
//! platform, so this module stays neutral. Core's `Clipboard` is text-only, and there's no
//! `ctx.process` yet (M1), so a job runs `std::process::Command` itself.

use crate::canvas::Raster;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// `raster` as a PNG.
pub fn encode_png(raster: &Raster) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, raster.width, raster.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&raster.rgba)
            .map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The size and the straight RGBA pixels of a PNG, for tests and a later clipboard read.
pub fn decode_png(bytes: &[u8]) -> Result<Raster, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => return Err(format!("a {other:?} PNG")),
    };
    Ok(Raster {
        width: info.width,
        height: info.height,
        rgba,
    })
}

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

/// The command that opens `file` in the browser: `open` on macOS, `xdg-open` on Linux.
pub fn open_command(file: &Path, has: &dyn Fn(&str) -> bool) -> Option<Command> {
    let program = ["open", "xdg-open"].into_iter().find(|p| has(p))?;
    let mut c = Command::new(program);
    c.arg(file);
    Some(c)
}

/// A command as a line, for messages and tests.
pub fn shown(cmd: &Command) -> String {
    die_core::process::shown(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips() {
        let r = Raster {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 128],
        };
        let bytes = encode_png(&r).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(decode_png(&bytes).unwrap(), r);
    }

    #[test]
    fn the_pasteboard_command_is_the_first_program_the_path_has() {
        let png = Path::new("/tmp/a \"b\".png");
        let mac = pasteboard_command(png, &|p| p == "osascript").unwrap();
        let line = shown(&mac);
        assert!(line.starts_with("osascript -e "), "{line}");
        assert!(
            line.contains("POSIX file \\\"/tmp/a \\\\\\\"b\\\\\\\".png\\\"")
                || line.contains("PNGf"),
            "{line}"
        );
        let args: Vec<String> = mac
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args[1],
            "set the clipboard to (read (POSIX file \"/tmp/a \\\"b\\\".png\") as «class PNGf»)"
        );
        let x = pasteboard_command(Path::new("/tmp/x.png"), &|p| p == "xclip").unwrap();
        assert_eq!(
            shown(&x),
            "xclip -selection clipboard -t image/png -i /tmp/x.png"
        );
        assert!(pasteboard_command(png, &|_| false).is_none());
        assert_eq!(
            shown(&open_command(Path::new("/tmp/x.svg"), &|p| p == "xdg-open").unwrap()),
            "xdg-open /tmp/x.svg"
        );
        assert_eq!(
            shown(&open_command(Path::new("/tmp/x.svg"), &|_| true).unwrap()),
            "open /tmp/x.svg"
        );
    }

    #[test]
    fn which_looks_in_each_directory() {
        let dir = std::env::temp_dir().join(format!("die-wb-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prog"), "").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), dir.as_path()]).unwrap();
        assert!(which("prog", &path));
        assert!(!which("other", &path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
