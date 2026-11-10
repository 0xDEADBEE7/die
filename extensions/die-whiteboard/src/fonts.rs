//! The font the PNG exporter draws labels with: DIE's own IoskeleyMonoTerm (OFL), read with
//! fontdue. On screen labels are cells, in the editor's font for free; only an export rasterizes
//! text itself. die-render's font-dir rule is out of reach under D14, so it's repeated here:
//! `$DIE_FONT_DIR`, else the app bundle's `Contents/Resources/fonts`, else `fonts/` at the
//! workspace root, which `cargo xtask fonts` fills.

use std::path::{Path, PathBuf};

/// The face labels use.
pub const REGULAR: &str = "IoskeleyMonoTerm-Regular.ttf";

/// Where the fonts are: `option` if set, else die-render's rule.
pub fn font_dir(option: Option<&Path>) -> PathBuf {
    if let Some(d) = option {
        return d.to_path_buf();
    }
    if let Some(d) = std::env::var_os("DIE_FONT_DIR") {
        return PathBuf::from(d);
    }
    let exe = std::env::current_exe().ok();
    // Through ~/.local/bin/die, the executable's path is the link's.
    let exe = exe.map(|e| e.canonicalize().unwrap_or(e));
    match exe.as_deref().and_then(bundle_of) {
        Some(bundle) => bundle.join("Contents/Resources/fonts"),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts")),
    }
}

/// The app bundle holding this executable, if it's at `<name>.app/Contents/MacOS/<exe>`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| bundle.to_path_buf())
}

/// The loaded face.
pub struct Fonts {
    regular: fontdue::Font,
}

impl Fonts {
    /// Read [`REGULAR`] from `dir`.
    pub fn load(dir: &Path) -> Result<Fonts, String> {
        let path = dir.join(REGULAR);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let regular = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Fonts { regular })
    }

    /// A character's coverage bitmap at `px` pixels, and where it sits.
    pub fn glyph(&self, ch: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.regular.rasterize(ch, px)
    }

    /// How far the baseline is below a line's top at `px` pixels.
    pub fn ascent(&self, px: f32) -> f32 {
        self.regular
            .horizontal_line_metrics(px)
            .map_or(px * 0.8, |m| m.ascent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_rule_reads_the_app_from_the_executables_path() {
        let exe = Path::new("/Applications/DIE.app/Contents/MacOS/die");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/DIE.app")));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/die")), None);
        assert_eq!(
            font_dir(Some(Path::new("/x"))),
            PathBuf::from("/x"),
            "the option wins"
        );
    }
}
