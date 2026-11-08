//! What differs by platform in die-core (docs/records/platforms.md §5.2): the path to DIE's
//! state, and for PTYs the path to DIE's own binary and the locale a program gets. The holder's
//! part of `sys` comes with D20.

use std::path::PathBuf;

/// Where DIE keeps its state: `$DIE_STATE_DIR` if it's set, else the platform's place for
/// it, under `$HOME`. None if there's neither.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DIE_STATE_DIR") {
        return Some(PathBuf::from(dir));
    }
    platform_state_dir()
}

#[cfg(target_os = "macos")]
fn platform_state_dir() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join("Library/Application Support/DIE"))
}

#[cfg(not(target_os = "macos"))]
fn platform_state_dir() -> Option<PathBuf> {
    match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir).join("die")),
        _ => Some(std::env::home_dir()?.join(".local/state/die")),
    }
}

/// A path that runs this process's own binary, for a PTY's trampoline (`crate::pty`). On Linux
/// it's `/proc/self/exe`, which still runs it once a rebuild has replaced the file.
#[cfg(target_os = "linux")]
pub fn own_exe() -> std::io::Result<PathBuf> {
    Ok(PathBuf::from("/proc/self/exe"))
}

/// A path that runs this process's own binary, for a PTY's trampoline (`crate::pty`).
#[cfg(not(target_os = "linux"))]
pub fn own_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// The locale a program on a PTY gets when DIE's environment names none, as DIE.app's from the
/// Dock doesn't: your region's in UTF-8, as Terminal.app and Ghostty give it. macOS keeps the
/// region as `AppleLocale`, such as `en_AU`, and it's read once.
#[cfg(target_os = "macos")]
pub fn lang() -> &'static str {
    static LANG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LANG.get_or_init(|| {
        let read = std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLocale"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let said = read.ok().filter(|o| o.status.success());
        let said = said.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        utf8_locale(
            &said.unwrap_or_default(),
            std::path::Path::new("/usr/share/locale"),
        )
    })
}

/// `apple`'s locale (`en_AU`, `zh-Hans_CN`, `en_AU@rg=auzzzz`) in UTF-8 if `dir` has it, as
/// `en_AU.UTF-8`, and otherwise `en_US.UTF-8`, as Ghostty falls back to.
#[cfg(target_os = "macos")]
fn utf8_locale(apple: &str, dir: &std::path::Path) -> String {
    let base = apple.trim().split('@').next().unwrap_or_default();
    let (lang, region) = base.split_once('_').unwrap_or((base, ""));
    let lang = lang.split('-').next().unwrap_or_default();
    let name = format!("{lang}_{region}.UTF-8");
    match !lang.is_empty() && !region.is_empty() && dir.join(&name).is_dir() {
        true => name,
        false => "en_US.UTF-8".into(),
    }
}

/// The locale a program on a PTY gets when DIE's environment names none. A Linux session names
/// one, so this is seldom used, and C.UTF-8 is there in every libc DIE runs on.
#[cfg(not(target_os = "macos"))]
pub fn lang() -> &'static str {
    "C.UTF-8"
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn a_region_becomes_its_utf_8_locale_or_en_us() {
        use super::utf8_locale;
        let dir = std::path::Path::new("/usr/share/locale");
        let cases = [
            ("en_AU\n", "en_AU.UTF-8"),
            ("en_AU@rg=auzzzz", "en_AU.UTF-8"),
            ("zh-Hans_CN", "zh_CN.UTF-8"),
            ("es_419", "en_US.UTF-8"),
            ("en", "en_US.UTF-8"),
            ("", "en_US.UTF-8"),
        ];
        for (apple, want) in cases {
            assert_eq!(utf8_locale(apple, dir), want, "{apple:?}");
        }
    }
}
