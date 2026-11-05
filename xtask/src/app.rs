//! `cargo xtask app`: `~/Applications/DIE.app`, from the release build of die-dev and the
//! fonts. `open -g ~/Applications/DIE.app` launches it without taking focus.

use crate::fonts;
use crate::util::{Result, cargo, run as run_cmd};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What LaunchServices and your window manager know DIE by.
pub const BUNDLE_ID: &str = "dev.die.DIE";

/// Which build goes in the bundle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// What you run.
    Release,
    /// What the tests have already built.
    Dev,
}

impl Profile {
    fn flag(self) -> &'static str {
        match self {
            Profile::Release => "--release",
            Profile::Dev => profile.flag(),
        }
    }

    fn dir(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Dev => "debug",
        }
    }
}

/// Build `~/Applications/DIE.app`.
pub fn run(root: &Path) -> Result {
    let home = std::env::home_dir().ok_or("HOME isn't set")?;
    let app = home.join("Applications/DIE.app");
    build(root, &app, BUNDLE_ID, Profile::Release)?;
    println!("app: {}", app.display());
    Ok(())
}

/// The binary, and the bundle at `app` with the bundle id `id`.
pub fn build(root: &Path, app: &Path, id: &str, profile: Profile) -> Result {
    if !fonts::present(&fonts::dir(root))? {
        fonts::run_fonts(root)?;
    }
    run_cmd(Command::new(cargo()).current_dir(root).args([
        "build",
        "--profile=dev",
        "--locked",
        "-p",
        "die-dev",
        "--bin",
        "die",
    ]))?;
    let binary = target_dir(root).join(profile.dir()).join("die");
    bundle(&binary, &fonts::dir(root), app, id)
}

fn target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => root.join(dir),
        _ => root.join("target"),
    }
}

/// Make the bundle beside `app`, sign it, and put it in place, so that a running DIE and a
/// failed build don't see half of one. A running DIE keeps the files it has open.
fn bundle(binary: &Path, font_dir: &Path, app: &Path, id: &str) -> Result {
    let parent = app.parent().ok_or("the app has no parent directory")?;
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let new = parent.join(format!(".DIE-{}.new.app", std::process::id()));
    let _ = fs::remove_dir_all(&new);
    let macos = new.join("Contents/MacOS");
    let fonts_out = new.join("Contents/Resources/fonts");
    fs::create_dir_all(&macos).map_err(|e| format!("{}: {e}", macos.display()))?;
    fs::create_dir_all(&fonts_out).map_err(|e| format!("{}: {e}", fonts_out.display()))?;
    let copy =
        |from: &Path, to: &Path| fs::copy(from, to).map_err(|e| format!("{}: {e}", from.display()));
    copy(binary, &macos.join("die"))?;
    for (name, _) in fonts::FILES {
        copy(&font_dir.join(name), &fonts_out.join(name))?;
    }
    fs::write(new.join("Contents/Info.plist"), info_plist(id))
        .map_err(|e| format!("Info.plist: {e}"))?;
    run_cmd(
        Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&new),
    )?;
    let _ = fs::remove_dir_all(app);
    fs::rename(&new, app).map_err(|e| format!("{}: {e}", app.display()))?;
    Ok(())
}

fn info_plist(id: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>die</string>
  <key>CFBundleIdentifier</key><string>{id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>DIE</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}
