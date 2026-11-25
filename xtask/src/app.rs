//! `cargo xtask app`: `~/Applications/DIE.app`, from the release build of a config crate and the
//! fonts. The config is a crate whose binary is `die`: DIE's own `apps/die-dev`, which has no
//! extension, or one outside DIE that builds on it, as the owner's die-config does (scope D14).
//! `open -g ~/Applications/DIE.app` launches it without taking focus, and it gives a terminal
//! `die file:line` through `~/.local/bin/die`.

use crate::fonts;
use crate::util::{Result, cargo, output, run as run_cmd};
use std::ffi::OsStr;
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
            Profile::Dev => "--profile=dev",
        }
    }

    fn dir(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Dev => "debug",
        }
    }
}

/// Where DIE's own config is, in its checkout: the base, with no extension.
pub const DIE_DEV: &str = "apps/die-dev";

/// The config a build takes when `--config` names none, from the environment's `DIE_CONFIG`.
pub const CONFIG_VAR: &str = "DIE_CONFIG";

/// Build `~/Applications/DIE.app`, or the same under `home` if it's given, which is how to try
/// the whole of this without touching your own, from the config `find_config` finds.
pub fn run(root: &Path, home: Option<&Path>, config: Option<&Path>) -> Result {
    let home = match home {
        Some(home) => home.to_path_buf(),
        None => std::env::home_dir().ok_or("HOME isn't set")?,
    };
    let env = std::env::var_os(CONFIG_VAR);
    let config = find_config(root, config, env.as_deref())?;
    println!("app: the config at {}", config.display());
    let app = home.join("Applications/DIE.app");
    build(root, &config, &app, BUNDLE_ID, Profile::Release)?;
    println!("app: {}", app.display());
    let path = std::env::var_os("PATH").unwrap_or_default();
    println!("{}", link_command(&home, &app, &path)?);
    Ok(())
}

/// The config to build: `given` (`--config`), else `env` (`$DIE_CONFIG`) if it's set, else
/// `die-config` beside the checkout at `root` if it's a crate, else DIE's own `apps/die-dev`.
/// One that's named has to be a crate. The path comes back absolute, with no `..` or link in it,
/// as the bundle records it for `rebuild`.
pub fn find_config(root: &Path, given: Option<&Path>, env: Option<&OsStr>) -> Result<PathBuf> {
    let named = |dir: &Path, by: String| -> Result<PathBuf> {
        match dir.join("Cargo.toml").is_file() {
            true => dir.canonicalize().map_err(|e| format!("{by}: {e}")),
            false => Err(format!(
                "{by}: there's no Cargo.toml there, so no config to build"
            )),
        }
    };
    if let Some(dir) = given {
        return named(dir, format!("--config {}", dir.display()));
    }
    if let Some(dir) = env.filter(|e| !e.is_empty()).map(Path::new) {
        return named(dir, format!("${CONFIG_VAR} {}", dir.display()));
    }
    let beside = root.parent().map(|p| p.join("die-config"));
    if let Some(dir) = beside.filter(|d| d.join("Cargo.toml").is_file()) {
        return named(&dir, dir.display().to_string());
    }
    named(&root.join(DIE_DEV), DIE_DEV.into())
}

/// If `~/.local/bin` is on `path`, link `~/.local/bin/die` to the bundle's binary, replacing a
/// link but never a file. What to tell you comes back. It edits no dotfile: otherwise, it's the
/// one line for yours, which needs no link.
pub fn link_command(home: &Path, app: &Path, path: &OsStr) -> Result<String> {
    let bin = home.join(".local/bin");
    let binary = app.join("Contents/MacOS/die");
    if !std::env::split_paths(path).any(|dir| dir == bin) {
        return Ok(format!(
            "add this to your shell's startup file for `die file`: export PATH=\"{}:$PATH\"",
            binary.parent().unwrap_or(app).display()
        ));
    }
    let link = bin.join("die");
    let shown = |e: std::io::Error| format!("{}: {e}", link.display());
    fs::create_dir_all(&bin).map_err(shown)?;
    match fs::symlink_metadata(&link) {
        Ok(found) if !found.is_symlink() => {
            return Ok(format!("{} is a file, so it's left alone", link.display()));
        }
        Ok(_) => fs::remove_file(&link).map_err(shown)?,
        Err(_) => {}
    }
    std::os::unix::fs::symlink(&binary, &link).map_err(shown)?;
    Ok(format!(
        "linked: {} -> {}",
        link.display(),
        binary.display()
    ))
}

/// The config at `config` built, and the bundle at `app` with the bundle id `id`, with the fonts
/// of the checkout at `root`.
pub fn build(root: &Path, config: &Path, app: &Path, id: &str, profile: Profile) -> Result {
    if !fonts::present(&fonts::dir(root))? {
        fonts::run_fonts(root)?;
    }
    let binary = build_config(root, config, profile)?;
    bundle(&binary, &fonts::dir(root), app, id, root, config)
}

/// Build the config at `config` and give its `die` binary. Cargo runs in the config's own
/// directory, so its `.cargo/config.toml` applies, as a config outside DIE patches DIE's crates to
/// a checkout there, and `--locked` when `locked` says. Where the binary lands is cargo's to say:
/// its target directory can be set there too.
pub fn build_config(root: &Path, config: &Path, profile: Profile) -> Result<PathBuf> {
    let meta = output(Command::new(cargo()).current_dir(config).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta).map_err(|e| format!("cargo metadata: {e}"))?;
    let dir = |key: &str| {
        let dir = meta[key].as_str().map(PathBuf::from);
        dir.ok_or_else(|| format!("cargo metadata in {} gave no {key}", config.display()))
    };
    let (target, workspace) = (dir("target_directory")?, dir("workspace_root")?);
    let mut cmd = Command::new(cargo());
    cmd.current_dir(config).args(["build", profile.flag()]);
    if locked(root, &workspace) {
        cmd.arg("--locked");
    }
    cmd.arg("--manifest-path")
        .arg(config.join("Cargo.toml"))
        .args(["--bin", "die"]);
    run_cmd(&mut cmd)?;
    Ok(target.join(profile.dir()).join("die"))
}

/// Whether a config whose workspace is at `workspace` builds `--locked`: only when that's the
/// workspace of DIE's checkout at `root`, whose lock DIE's commits keep current along with each
/// change. A config outside it builds without, though it has a `Cargo.lock`: nothing keeps that
/// lock current, as DIE's crates and the extensions reach it by path patches to checkouts whose
/// pins move with their own commits, and `--locked` would stop at the first dependency they add
/// or bump. Cargo then brings that lock up to date, changing only what it must.
fn locked(root: &Path, workspace: &Path) -> bool {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    same(root, workspace) && workspace.join("Cargo.lock").is_file()
}

/// Make the bundle beside `app`, sign it, and put it in place, so that a running DIE and a
/// failed build don't see half of one. A running DIE keeps the files it has open. The old one is
/// moved aside rather than deleted first: a `restart` that comes just then finds a whole bundle,
/// or none and says so. `Contents/Resources/checkout` names `checkout`, the workspace it came
/// from, and `Contents/Resources/config` the config, for DIE's `rebuild`.
pub fn bundle(
    binary: &Path,
    font_dir: &Path,
    app: &Path,
    id: &str,
    checkout: &Path,
    config: &Path,
) -> Result {
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
    let named = format!("{}\n", checkout.display());
    fs::write(new.join("Contents/Resources/checkout"), named)
        .map_err(|e| format!("checkout: {e}"))?;
    let named = format!("{}\n", config.display());
    fs::write(new.join("Contents/Resources/config"), named).map_err(|e| format!("config: {e}"))?;
    run_cmd(
        Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&new),
    )?;
    let old = parent.join(format!(".DIE-{}.old.app", std::process::id()));
    let _ = fs::remove_dir_all(&old);
    let _ = fs::rename(app, &old);
    fs::rename(&new, app).map_err(|e| format!("{}: {e}", app.display()))?;
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// The bundle's Info.plist. DIE says it edits any file, so that Finder offers it in Open With and
/// the Dock takes a file dropped on it, but at the `Alternate` rank, so it's the opener of nothing
/// until you choose it. Folders aren't files: DIE refuses them.
fn info_plist(id: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>File</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key><array><string>public.data</string></array>
    </dict>
  </array>
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{TempDir, write};

    #[test]
    fn the_config_is_the_one_named_then_die_config_beside_the_checkout_then_die_dev() {
        let tmp = TempDir::new("app-config").unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("die");
        let die_dev = root.join(DIE_DEV);
        write(&die_dev.join("Cargo.toml"), "").unwrap();
        let mine = top.join("elsewhere/mine");
        write(&mine.join("Cargo.toml"), "").unwrap();
        let find = |given: Option<&Path>, env: Option<&str>| {
            find_config(&root, given, env.map(OsStr::new))
        };
        assert_eq!(find(None, None), Ok(die_dev.clone()));
        assert_eq!(
            find(None, Some("")),
            Ok(die_dev.clone()),
            "set empty is unset"
        );
        let beside = top.join("die-config");
        std::fs::create_dir_all(&beside).unwrap();
        assert_eq!(
            find(None, None),
            Ok(die_dev.clone()),
            "a directory isn't a crate"
        );
        write(&beside.join("Cargo.toml"), "").unwrap();
        assert_eq!(find(None, None), Ok(beside.clone()));
        let env = mine.to_str().unwrap();
        assert_eq!(
            find(None, Some(env)),
            Ok(mine.clone()),
            "$DIE_CONFIG before it"
        );
        let dotted = top.join("elsewhere/../die-config");
        assert_eq!(
            find(Some(&dotted), Some(env)),
            Ok(beside.clone()),
            "--config before both, with its `..` gone"
        );
        let no_crate = top.join("elsewhere");
        let e = find(Some(&no_crate), None).unwrap_err();
        assert!(e.contains("--config") && e.contains("no Cargo.toml"), "{e}");
        let e = find(None, Some(no_crate.to_str().unwrap())).unwrap_err();
        assert!(
            e.contains("$DIE_CONFIG") && e.contains("no Cargo.toml"),
            "{e}"
        );
    }

    #[test]
    fn only_the_checkouts_own_workspace_builds_locked() {
        let tmp = TempDir::new("app-locked").unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("die");
        let beside = top.join("die-config");
        let inside = root.join("target/throwaway-config");
        for dir in [&root, &beside, &inside] {
            write(&dir.join("Cargo.lock"), "").unwrap();
        }
        assert!(locked(&root, &root), "DIE's own, by its lock");
        assert!(
            locked(&root, &top.join("die-config/../die")),
            "the same workspace by another path"
        );
        assert!(!locked(&root, &beside), "a config outside, lock or not");
        assert!(
            !locked(&root, &inside),
            "a workspace of its own inside the checkout"
        );
        std::fs::remove_file(root.join("Cargo.lock")).unwrap();
        assert!(!locked(&root, &root), "no lock to hold it to");
    }
}
