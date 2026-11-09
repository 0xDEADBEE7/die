//! Shell integration (scope §6): the directory, so the terminal's shell reports where it is (OSC
//! 7) at each prompt, as ghostel's and Ghostty's integrations make it, and the status line, `gf`
//! on a relative path and a file prompt follow `cd`. macOS's `/etc/zshrc` reports only to Apple's
//! Terminal.
//!
//! zsh starts with `ZDOTDIR` at DIE's own `.zshenv`, which puts yours back, sources your
//! `.zshenv`, and adds the hook; zsh then reads your `.zprofile`, `.zshrc` and `.zlogin` from
//! your `ZDOTDIR` or home as it always does, and programs it runs see your `ZDOTDIR`, not DIE's.
//!
//! bash, interactive and not login, as vterm starts it, starts with `--rcfile` at DIE's own rc,
//! which sources your `~/.bashrc` as bash would have, then adds the hook to your
//! `PROMPT_COMMAND`. A login bash ignores `--rcfile`, so it doesn't report yet.
//!
//! Both are kept in a directory of DIE's under `$TMPDIR` that only you can write.

use crate::commands::Spec;
use std::fs::{DirBuilder, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// DIE's `.zshenv`, in the quoting Ghostty's uses, so your aliases can't change it. The hook
/// escapes `%` and control characters as `%XX`, so a BEL in a name can't end the OSC.
const ZSHENV: &str = r#"# DIE's terminal (die-term): your ZDOTDIR back, your .zshenv, and the directory at each prompt.
if [[ -n "${DIE_ZSH_ZDOTDIR+X}" ]]; then
    'builtin' 'export' ZDOTDIR="$DIE_ZSH_ZDOTDIR"
    'builtin' 'unset' 'DIE_ZSH_ZDOTDIR'
else
    'builtin' 'unset' 'ZDOTDIR'
fi
{
    'builtin' 'typeset' _die_zshenv="${ZDOTDIR-$HOME}/.zshenv"
    [[ ! -r "$_die_zshenv" ]] || 'builtin' 'source' '--' "$_die_zshenv"
} always {
    'builtin' 'unset' '_die_zshenv'
    if [[ -o 'interactive' ]]; then
        __die_osc7() {
            'builtin' 'emulate' -L zsh
            'builtin' 'setopt' 'extended_glob'
            'builtin' 'local' d
            d=${PWD//(#m)[%[:cntrl:]]/%${(l:2::0:)$(( [##16] #MATCH ))}}
            'builtin' 'printf' '\e]7;file://%s%s\a' "$HOST" "$d"
        }
        'builtin' 'typeset' -ga precmd_functions
        precmd_functions+=(__die_osc7)
    fi
}
"#;

/// DIE's rc for bash 3.2 and later. The hook keeps `$?`, escapes `%` and control characters as
/// `%XX`, and goes after your `PROMPT_COMMAND`, a string in bash 3.2, or the first of an array's
/// in bash 5.1 and later. A bash started inside inherits an exported `PROMPT_COMMAND` but not the
/// function, so it's called quietly.
const BASHRC: &str = r#"# DIE's terminal (die-term): your .bashrc, as bash reads it, and the directory at each prompt.
if [ -r ~/.bashrc ]; then 'builtin' '.' ~/.bashrc; fi
__die_osc7() {
    'builtin' 'local' s=$? p q= c
    p=$PWD
    case $p in
    *[%[:cntrl:]]*)
        while [[ -n $p ]]; do
            c=${p:0:1} p=${p:1}
            case $c in [%[:cntrl:]]) 'builtin' 'printf' -v c '%%%02X' "'$c" ;; esac
            q=$q$c
        done ;;
    *) q=$p ;;
    esac
    'builtin' 'printf' '\033]7;file://%s%s\a' "$HOSTNAME" "$q"
    'builtin' 'return' "$s"
}
if [ -n "${PROMPT_COMMAND-}" ]; then
    PROMPT_COMMAND="$PROMPT_COMMAND"$'\n''__die_osc7 2>/dev/null'
else
    PROMPT_COMMAND='__die_osc7 2>/dev/null'
fi
"#;

/// Give the terminal's zsh DIE's `.zshenv`, and its bash, not login and with no arguments, DIE's
/// rc. Without them, as without `$TMPDIR`, the shell starts as it would.
pub(crate) fn integrate(spec: &mut Spec) {
    let name = Path::new(&spec.program).file_name();
    let bash = name == Some("bash".as_ref()) && !spec.login && spec.args.is_empty();
    if spec.shell && name == Some("zsh".as_ref()) {
        zsh(spec);
    } else if spec.shell
        && bash
        && let Some(rc) = kept("die-term-bash", "bashrc", BASHRC)
    {
        spec.args = vec!["--rcfile".into(), rc.into()];
    }
}

fn zsh(spec: &mut Spec) {
    let Some(file) = kept("die-term-zsh", ".zshenv", ZSHENV) else {
        return;
    };
    let given = spec.env.iter().rev().find(|(k, _)| k == "ZDOTDIR");
    let yours = given.map(|(_, v)| v.clone());
    if let Some(z) = yours.or_else(|| std::env::var_os("ZDOTDIR")) {
        spec.env.push(("DIE_ZSH_ZDOTDIR".into(), z));
    }
    let dir = file.parent().unwrap_or(&file).to_path_buf();
    spec.env.push(("ZDOTDIR".into(), dir.into()));
}

/// `text` as `name` in DIE's directory `dir` under `$TMPDIR`, which macOS gives each user.
fn kept(dir: &str, name: &str, text: &str) -> Option<PathBuf> {
    kept_in(&PathBuf::from(std::env::var_os("TMPDIR")?), dir, name, text)
}

/// `text` as `name` in `base/dir`, kept where only you can write it: written at each start, to a
/// new file renamed into place, and None unless the directory is yours and no one else can write
/// it, or someone else's file could run in your shell.
fn kept_in(base: &Path, dir: &str, name: &str, text: &str) -> Option<PathBuf> {
    let dir = base.join(dir);
    let _ = DirBuilder::new().recursive(true).mode(0o700).create(&dir);
    let tmp = dir.join(format!("{name}.{}", std::process::id()));
    let file = dir.join(name);
    let kept = private(&dir, &tmp, text) && std::fs::rename(&tmp, &file).is_ok();
    let _ = std::fs::remove_file(&tmp);
    kept.then_some(file)
}

/// Write `text` to `tmp`, a new file in `dir`, and say whether `dir` is yours and no one else
/// can write it. One of yours that others can, as a umask of 002 makes it, is made private, and
/// the file written again in it.
fn private(dir: &Path, tmp: &Path, text: &str) -> bool {
    let (Some(mine), Ok(meta)) = (fresh(tmp, text), std::fs::symlink_metadata(dir)) else {
        return false;
    };
    let only_you = || std::fs::set_permissions(dir, Permissions::from_mode(0o700)).is_ok();
    meta.is_dir()
        && meta.uid() == mine
        && (meta.mode() & 0o022 == 0 || only_you() && fresh(tmp, text).is_some())
}

/// `text` in a new file at `path`, with what was there gone first (a link, not what it names),
/// and its owner.
fn fresh(path: &Path, text: &str) -> Option<u32> {
    let _ = std::fs::remove_file(path);
    let mut new = OpenOptions::new();
    let mut f = new
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .ok()?;
    f.write_all(text.as_bytes()).ok()?;
    Some(f.metadata().ok()?.uid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn base() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("die-term-kept-{}-{n}", std::process::id()));
        std::fs::create_dir_all(base.join("d")).unwrap();
        base
    }

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
    }

    /// Under a umask of 002 the directory came out group-writable, was refused, and stayed so.
    #[test]
    fn a_directory_of_yours_others_could_write_is_made_private_and_used() {
        let base = base();
        let dir = base.join("d");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        let file = kept_in(&base, "d", "rc", "text");
        assert_eq!(file, Some(dir.join("rc")));
        assert_eq!(std::fs::read_to_string(dir.join("rc")).unwrap(), "text");
        assert_eq!(mode(&dir), 0o700);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// The file is new where it's written, so a link planted there isn't written through.
    #[test]
    fn a_link_planted_in_the_directory_is_never_written_through() {
        let base = base();
        let (dir, victim) = (base.join("d"), base.join("victim"));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o770)).unwrap();
        std::fs::write(&victim, "VICTIM").unwrap();
        let planted = dir.join(format!("rc.{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        let file = kept_in(&base, "d", "rc", "text");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "VICTIM");
        assert_eq!(file, Some(dir.join("rc")));
        assert_eq!(std::fs::read_to_string(dir.join("rc")).unwrap(), "text");
        assert!(
            std::fs::symlink_metadata(&planted).is_err(),
            "the link is gone"
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}
