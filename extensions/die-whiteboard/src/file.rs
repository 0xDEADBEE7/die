//! Board files on disk: reading one (a missing or empty file is a new board, a foreign one is
//! refused), writing one atomically, the names a board buffer and its exports get, and the
//! `Board: ` prompt's directory listing.

use crate::model::Board;
use crate::svg::{self, Look};
use std::io;
use std::path::{Path, PathBuf};

/// `name` with `.svg` if it names no extension.
pub fn with_svg(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_owned();
    name.push(".svg");
    PathBuf::from(name)
}

/// The file's name, for the status line: `arch.svg`.
pub fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The buffer's name: `*whiteboard: arch.svg*`.
pub fn buffer_name(path: &Path) -> String {
    format!("*whiteboard: {}*", name_of(path))
}

/// `arch.svg` beside the file as `arch.png`, or `arch.export.svg`.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "board".into());
    path.with_file_name(format!("{stem}{suffix}"))
}

/// The board in `path`: a new one if the file is missing or empty.
pub fn load(path: &Path) -> Result<Board, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Board::default()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    svg::read(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `bytes` to `path` through a temporary file beside it, renamed over the old one, so a
/// crash leaves one or the other whole.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    let name = name_of(path);
    let tmp = dir
        .unwrap_or(Path::new("."))
        .join(format!(".{name}.die-{}.tmp", std::process::id()));
    let done = (|| {
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// Save `board` to `path` in the dialect. The error says what went wrong, not where: the
/// caller names the file as it wants it named.
pub fn save(path: &Path, board: &Board, look: &Look) -> Result<(), String> {
    let text = svg::write(board, look, true);
    write_atomic(path, text.as_bytes()).map_err(|e| e.to_string())
}

/// A directory's boards and subdirectories by name, hidden ones last, each directory ending in
/// `/`: what the `Board: ` prompt lists. A name that isn't UTF-8 can't go on the line.
pub fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<String> = read
        .flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir())
                || e.file_type().is_ok_and(|t| t.is_symlink()) && e.path().is_dir();
            let name = e.file_name().into_string().ok()?;
            if is_dir {
                Some(name + "/")
            } else {
                // Not a board's own export, which it would refuse as not a whiteboard file.
                (name.ends_with(".svg") && !name.ends_with(".export.svg")).then_some(name)
            }
        })
        .collect();
    out.sort_unstable_by(|a, b| (a.starts_with('.'), a).cmp(&(b.starts_with('.'), b)));
    out
}

/// The prompt's starting line for a directory: ending in `/`, with `~` for the home directory
/// as Emacs abbreviates it.
pub fn line_for(dir: &Path, home: Option<&Path>) -> String {
    let shown = home
        .and_then(|h| dir.strip_prefix(h).ok())
        .map(|rest| Path::new("~").join(rest));
    let line = shown.as_deref().unwrap_or(dir).to_string_lossy();
    format!("{}/", line.trim_end_matches('/'))
}

/// What a line means: from where it last starts again (`//` or `/~`), `~` is the home
/// directory, a relative path starts from `dir`.
pub fn resolve(dir: &Path, home: Option<&Path>, line: &str) -> PathBuf {
    let line = restarted(line);
    let path = match (line.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_path_buf(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => dir.join(line),
    };
    normalize(&path)
}

/// The line from where it last starts again, as Emacs's `substitute-in-file-name` has it: the
/// second slash of a `//`, or the `~` of a `/~/`.
fn restarted(line: &str) -> &str {
    let b = line.as_bytes();
    let starts = |&i: &usize| {
        b[i - 1] == b'/'
            && (b[i] == b'/' || b[i] == b'~' && matches!(b.get(i + 1), None | Some(b'/')))
    };
    (1..b.len()).rev().find(starts).map_or(line, |i| &line[i..])
}

/// `.` and `..` resolved by name.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_lines() {
        assert_eq!(with_svg(Path::new("/a/arch")), PathBuf::from("/a/arch.svg"));
        assert_eq!(
            with_svg(Path::new("/a/arch.svg")),
            PathBuf::from("/a/arch.svg")
        );
        assert_eq!(
            buffer_name(Path::new("/a/arch.svg")),
            "*whiteboard: arch.svg*"
        );
        assert_eq!(
            sibling(Path::new("/a/arch.svg"), ".png"),
            PathBuf::from("/a/arch.png")
        );
        assert_eq!(
            sibling(Path::new("/a/arch.svg"), ".export.svg"),
            PathBuf::from("/a/arch.export.svg")
        );
        let home = Path::new("/Users/me");
        assert_eq!(
            line_for(Path::new("/Users/me/notes"), Some(home)),
            "~/notes/"
        );
        assert_eq!(line_for(Path::new("/tmp"), Some(home)), "/tmp/");
        assert_eq!(
            resolve(Path::new("/tmp"), Some(home), "~/n/a.svg"),
            PathBuf::from("/Users/me/n/a.svg")
        );
        assert_eq!(
            resolve(Path::new("/tmp"), Some(home), "/tmp/x//etc/y"),
            PathBuf::from("/etc/y")
        );
        assert_eq!(
            resolve(Path::new("/tmp/a"), None, "../b.svg"),
            PathBuf::from("/tmp/b.svg")
        );
    }

    #[test]
    fn a_missing_file_is_a_new_board_and_a_write_is_atomic() {
        let dir = std::env::temp_dir().join(format!("die-wb-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("new.svg");
        assert_eq!(load(&path), Ok(Board::default()));
        let mut board = Board::default();
        board.add(
            crate::model::Shape::Rect {
                a: crate::model::Point::new(1.0, 2.0),
                b: crate::model::Point::new(3.5, 4.25),
            },
            3,
            2,
        );
        save(&path, &board, &Look::default()).unwrap();
        assert_eq!(load(&path), Ok(board));
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "no temporary file left");
        std::fs::write(dir.join("other.svg"), "<svg></svg>").unwrap();
        assert!(
            load(&dir.join("other.svg"))
                .unwrap_err()
                .contains("browser")
        );
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        assert_eq!(entries(&dir), ["new.svg", "other.svg", "sub/"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
