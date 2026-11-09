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

/// DIE's rc for bash 3.2 and later. The hook keeps `$?`, and goes after your `PROMPT_COMMAND`,
/// a string in bash 3.2, or the first of an array's in bash 5.1 and later.
const BASHRC: &str = r#"# DIE's terminal (die-term): your .bashrc, as bash reads it, and the directory at each prompt.
if [ -r ~/.bashrc ]; then 'builtin' '.' ~/.bashrc; fi
__die_osc7() {
    'builtin' 'local' s=$?
    'builtin' 'printf' '\033]7;file://%s%s\a' "$HOSTNAME" "${PWD//\%/%25}"
    'builtin' 'return' "$s"
}
if [ -n "${PROMPT_COMMAND-}" ]; then
    PROMPT_COMMAND="$PROMPT_COMMAND"$'\n'__die_osc7
else
    PROMPT_COMMAND=__die_osc7
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

/// `text` as `name` in DIE's directory `dir`, kept where only you can write it: under `$TMPDIR`,
/// which macOS gives each user. It's written at each start, and None unless the directory is
/// yours and no one else can write it, or someone else's file could run in your shell.
fn kept(dir: &str, name: &str, text: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TMPDIR")?).join(dir);
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = dir.join(format!("{name}.{}", std::process::id()));
    std::fs::write(&tmp, text).ok()?;
    let (meta, mine) = (dir.metadata().ok()?, tmp.metadata().ok()?.uid());
    let private = meta.uid() == mine && meta.permissions().mode() & 0o022 == 0;
    let file = dir.join(name);
    let kept = private && std::fs::rename(&tmp, &file).is_ok();
    let _ = std::fs::remove_file(&tmp);
    kept.then_some(file)
}
