//! Shell integration (scope §6), so far zsh's directory: a login zsh reports where it is (OSC
//! 7) at each prompt, as ghostel's and Ghostty's integrations make it, so the status line, `gf`
//! on a relative path and a file prompt follow `cd`. macOS's `/etc/zshrc` reports only to
//! Apple's Terminal.
//!
//! The shell starts with `ZDOTDIR` at DIE's own `.zshenv`, which puts yours back, sources your
//! `.zshenv`, and adds the hook; zsh then reads your `.zprofile`, `.zshrc` and `.zlogin` from
//! your `ZDOTDIR` or home as it always does, and programs it runs see your `ZDOTDIR`, not DIE's.
//! bash 3.2's integration waits (scope §6).

use crate::commands::Spec;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// DIE's `.zshenv`, in the quoting Ghostty's uses, so your aliases can't change it.
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
            'builtin' 'printf' '\e]7;file://%s%s\a' "$HOST" "${PWD//\%/%25}"
        }
        'builtin' 'typeset' -ga precmd_functions
        precmd_functions+=(__die_osc7)
    fi
}
"#;

/// Give a login zsh DIE's `.zshenv`, kept where only you can write it: under `$TMPDIR`, which
/// macOS gives each user. Without one, or if it can't be written, the shell starts as it would.
pub(crate) fn integrate(spec: &mut Spec) {
    let zsh = Path::new(&spec.program).file_name() == Some("zsh".as_ref());
    let Some(dir) = (spec.login && zsh).then(zdotdir).flatten() else {
        return;
    };
    let given = spec.env.iter().rev().find(|(k, _)| k == "ZDOTDIR");
    let yours = given.map(|(_, v)| v.clone());
    if let Some(z) = yours.or_else(|| std::env::var_os("ZDOTDIR")) {
        spec.env.push(("DIE_ZSH_ZDOTDIR".into(), z));
    }
    spec.env.push(("ZDOTDIR".into(), dir.into()));
}

/// The directory holding DIE's `.zshenv`, written if it isn't there as it should be.
fn zdotdir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TMPDIR")?).join("die-term-zsh");
    std::fs::create_dir_all(&dir).ok()?;
    // Yours alone, or someone else's file could run in your shell.
    let tmp = dir.join(format!(".zshenv.{}", std::process::id()));
    std::fs::write(&tmp, ZSHENV).ok()?;
    let (meta, mine) = (dir.metadata().ok()?, tmp.metadata().ok()?.uid());
    let private = meta.uid() == mine && meta.permissions().mode() & 0o022 == 0;
    let file = dir.join(".zshenv");
    let kept = private && std::fs::rename(&tmp, &file).is_ok();
    let _ = std::fs::remove_file(&tmp);
    kept.then_some(dir)
}
