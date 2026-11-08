# Using DIE

How to install and run DIE, and every key and command it has on `m0/integration`, read from the binding tables in `crates/die-vim/src`, `crates/die-core/src` and `apps/die-dev/src/lib.rs`. Keys follow your Evil in fundamental-mode unless [m0-log.md](m0-log.md) says otherwise. `\hk` tells you what any key does, and `\hc` what any command does.

## Install and run

```sh
cargo xtask app          # builds ~/Applications/DIE.app from a release build
open ~/Applications/DIE.app
```

- `cargo xtask app` links `~/.local/bin/die` if that directory is on your PATH. Otherwise it prints the one line to add to your shell config. It replaces a link there but never a file. `--home DIR` installs under DIR instead, to try it without touching yours.
- There is one instance. Launching it again from the Dock or Finder does nothing.
- **After changing DIE or your config:** `:rebuild` in DIE. It runs `cargo xtask app` in the DIE checkout of the buffer you're in, or else the last one it built, with cargo's output in `*rebuild*` below. If it builds, DIE restarts into it. If not, nothing is installed, the cursor in `*rebuild*` is on the first error, and `gf` on its `-->` line opens the file there. It replaces the bundle that's running, so it refuses for one outside an `Applications` folder. A change to your config takes about 2 s. By hand it's `cargo xtask app` in a terminal, then `:restart`.
- **What a restart keeps:** your buffers, unsaved text, splits, cursors, marks, registers, macros, `.` and last search. `*grep*` doesn't come back. A new build that fails to start leaves the old instance running.
- **Quit:** `:q` on the last window, `:qa`, or Cmd-Q. Each refuses if a file is unsaved. `:qa!` doesn't.

From a terminal, `die` opens files in the running DIE and returns at once. If none is running, it starts one.

```sh
die foo.rs               # as :e would
die foo.rs:12            # line 12
die foo.rs:12:3          # line 12, character 3
die foo.rs:12:3:         # as compilers print it
die +12 'odd:name'       # the name is taken literally
die a.rs b.rs            # each opens in turn, and the last ends up on screen
die                      # brings DIE forward, or starts it
```

`die file:line` leaves a jump, so `C-o` comes back. `EDITOR=die` doesn't work for `git commit`, because there is no `--wait`. `open -a DIE file` and a Finder double-click don't open the file yet.

## DIE's own keys

The leader is `\`, in normal state.

| Key | Command | What it does |
|---|---|---|
| `\\` | `find-file-home` | Fuzzy find any file under your home |
| `\p` | `find-file-project` | Fuzzy find under the nearest parent with `.git` |
| `\f` | `find-file` | Find a file, going down from the buffer's directory |
| `\/`, `\]` | `switch-buffer` | Switch buffer |
| `\r` | `grep` | Search the project |
| `\x`, `M-x` | `command-palette` | Run any command by name, recent first. `M-x` works in insert and visual too |
| `\z` | `toggle-max-width` | Turn the 80-column centered text on or off |
| `\hk` | `describe-key` | What a key does |
| `\hc` | `describe-command` | What a command does |
| Cmd-Q | `quit` | Quit, in every state |

`:name` runs any command by name, so also `:restart`, `:kill-buffer`, `:quit`.

**The finders** list files while you type. They skip `.git`, `target`, `Library`, `.cargo/registry`, `.claude/worktrees` and a fixed set of others, and they don't read `.gitignore`. Only the best 1,000 matches can be reached.

**`:grep PATTERN [DIR]`**, or `\r`. Searches with `/`'s smartcase regex, on a background thread, into `*grep*`, with focus staying where it was. One row per line. It stops at 10,000 rows, and skips binary files and files over 10 MB. If the last word names a directory, that's the root, so `:grep mod tests` searches for `mod` in `tests/`.
- `<CR>` on a row opens it in another window.
- `:cn`, `:cp` go to the next and previous result, from any window.
- `gf` and `gF` open `path:line:col`, `path:line`, `path(line)` or `path(line,col)` under the cursor, trying the buffer's directory and then the project root. `]f [f ]F [F` do the same. `gf` in Visual takes the selection as the name.

## Prompts

These work on `:`, `/`, `?` and the finders.

| Keys | What they do |
|---|---|
| `<CR>` / `<Esc>` `C-g` `C-c` | Answer / cancel |
| Cmd-V | Paste the clipboard, without its final newlines |
| `C-y` | Paste the newest kill |
| `C-r x` | Paste register x |
| `C-r C-w`, `C-a`, `C-o`, `C-f`, `C-l` | Paste the word, WORD, symbol, file name or line under the cursor |
| `<Left>` `C-b`, `<Right>` `C-f`, `<Home>` `C-a`, `<End>` `C-e` | Move |
| `M-b`, `M-f`, also `C-` or `S-` with the arrows | Move by word |
| `<BS>` `C-h`, `<Del>` `C-d`, `C-w` `M-BS`, `M-d` | Delete a character, a word |
| `C-u`, `C-k` | Delete to the line's start, to its end |

On `:`, `/` and `?` only: `C-v` inserts the next key literally, `<Up>` `<Down>` recall history matching what's typed, `C-b` goes to the start, and `C-u` deletes the whole line. `C-p` `C-n` also go through history.

On a list of candidates: `<Down>` `C-n` and `<Up>` `C-p` choose a row, `<PageDown>` `C-v` and `<PageUp>` `M-v` go a screenful, `M-<` and `M->` go to the ends, `<Tab>` puts the row on the line, `M-<CR>` answers with what's typed, and `M-p` `M-n` go through history.

## Moving

| Keys | |
|---|---|
| `h j k l`, arrows, `<BS>`, `<Space>` | Characters and lines |
| `gj gk`, `g0 g^ g$` | Screen rows |
| `0 ^ $ g_ <Home> <End> \|` | In the line (`\|` counts from 0, as your Evil does) |
| `C-a` | Start of the screen row |
| `+ - _ <CR>` | Line by line, to the first non-blank |
| `w W b B e E ge gE` | Words (a word stops at `_`) |
| `f F t T` then `; ,` | Find a character |
| `{ } %` | Paragraphs, matching bracket |
| `[[ ]]` | Lines starting with a bracket. In Rust that's mostly the file's ends |
| `[( ]) [{ ]}` | The enclosing bracket |
| `gg G :N :$` | Lines |
| `H M L` | Window rows |
| `/ ? n N * # g* g#` | Search, then `:noh` to clear the highlight |
| `gd` | First place in the text with this symbol, as your Evil falls back to |
| `ga` | Say what the character under the cursor is |

**Marks and jumps.** `m{a-z}` per buffer, `m{A-Z}` across buffers, `` `x `` and `'x` to go there, also under operators. Special: `[ ]` what the last operator or paste covered, `< >` the last selection, `^` where insert ended, `.` the last change. `C-o` and `<Tab>` (`C-i`) walk the window's jump list, across buffers. `g;` and `g,` walk the change list. `gi` goes back and inserts.

**Scrolling.** `C-d`, `C-f`, `C-b`, `<PageDown>`, `<PageUp>`, `C-e`, `C-y`, `zz zt zb`, and the trackpad. `C-u` doesn't scroll, as in your Evil: it's the prefix argument.

**Mouse.** Click, drag to select, scroll the window under the pointer.

## Changing

- **Insert:** `i a I A o O gi`. Replace: `R`.
- **Operators:** `d c y < > = g~ gu gU`, doubled for lines, with any motion or object.
- **Short forms:** `x X s S D C Y ~ r J gJ`, `p P`, `u C-r` (also `:u`), `.`.
- **Objects:** `iw aw iW aW io ao` (word, WORD, symbol: `cio` for snake_case), `ip ap`, `i" a" i' a'`, ``i` a` ``, `i( a( ib ab`, `i[ a[`, `i{ a{ iB aB`, `i< a<`.
- **Registers:** `"x` then a command. `a-z`, `A-Z` to append, `0`, `1-9`, `-`, `_`, `+` the clipboard, and read-only `.`, `:`, `/`, `%`. Every kill also goes to the clipboard, as in your Emacs.
- **Macros:** `q{x}`, `q`, `@x`, `@@`, `@:`. A typed `C-g` stops a running macro.
- **Substitute:** `:s/a/b/`, with flags `g` and `&`, `~` for the last replacement, ranges `%`, `'<,'>`, `N,M`, `.`, `$`, `'x`, `+n`, `-n`. `&` repeats it on the line, `g&` everywhere, also `:&`, `:&&`.

## Selecting

`v V C-v`, `gv` to reselect. Motions, objects and operators work, plus `o` and `O` for the other end, `I` and `A` to insert on each line, `u` and `U` for case, `R` and `X` for whole lines, `r`, `J`, `p`, `&`, `:`, and `gf`.

## Insert and replace state

| Key | What it does |
|---|---|
| `<Esc>` | Back to normal |
| `<BS>`, `<Del>`, `C-w` | Delete a character back, a character on, a word back |
| `<CR>`, `<Tab>` | New line with this indentation, indent to the line above's next point |
| arrows, `S-Left`, `S-Right` | Move, by word |
| `C-o` | One normal command, then back |
| `C-p`, `C-n` | Complete a word, cycling: this buffer, then others |
| `C-x C-n`, `C-x C-p` | Complete a whole line |
| `C-t`, `C-d`, `0 C-d` | Shift right, shift left, take away the indent |
| `C-a`, `C-@` | The last insertion, and that then Esc |
| `C-e`, `C-y` | Copy from the line below, above |
| `C-r x` | A register's text |
| `C-v x`, `C-q x` | The next key literally, or an octal code |
| `C-u` | The prefix argument |

## Windows and buffers

- **`C-w` then:** `h j k l` and arrows, `w W t b p` to move; `s S v n` to split; `c q o` to close; `= + - > < _ |` to size; `H J K L` to move the window; `r R x` to rotate and exchange; `:` for ex; digits for a count. A key with Control does the same, as `C-w C-h`.
- **Ex:** `:sp[lit]`, `:vs[plit]`, `:new`, `:vne[w]`, each with a file; `:clo[se]`, `:on[ly]`.
- **Buffers:** `:e file`, `:e!`, `:b name`, `:bn`, `:bp`, `C-^`, `:bd`, `:bw` (`!` to drop unsaved text), `:kill-buffer`. `:bn` and `:bp` go through file buffers. `:ls` (or `:buffers`) lists every buffer in `*messages*` and shows it there, the one used last first, `.` for this one, `%` read-only, `*` unsaved. `:files` lists only those with a file.
- **Files:** `:w`, `:w!` over a file changed on disk, `:up`, `:wa`, `:q`, `:q!`, `:wq`, `:x`, `:wqa`, `:xa`, `:qa`, `ZZ`, `ZQ`. Files changed on disk reload by themselves if you have no unsaved text there, and `:w` refuses if you have.
- **Saving elsewhere:** `:w path` from a buffer with no file, such as `:new`'s, saves it there, and the buffer then visits that file. From a buffer with a file it writes a copy, and the buffer keeps its file and its unsaved changes, as in your Evil. `:sav[eas] path` saves and moves the buffer to the new file. A path is taken from the buffer's directory, as `:e` takes it, and `~/` is your home. A file that's already there is written over only with `!` (`:w! path`, `:saveas! path`). `:wq path` and `:x path` write there, then close.
- **Which file:** `:f` (`:file`) and `C-g` say the file's full path, its lines and how far down the cursor is, as your Evil's `:f` does. The status line shows only the directory. `C-g` is Vim's: your Evil's quits.

## What isn't there yet

- **A terminal.** No terminal buffers exist. cargo, git and Claude Code run in a terminal of your own next to DIE, and `die file:line` from there.
- **Normal keys:** `gq gw`, `( )` and `is as`, `it at`, `gn gN`, `g?`, `gm`, `gp gP`, `Q`, `C-]`, `C-w f`, `[] ][`, `z. z<CR> z- z+ z^`, folds, `gD`, `C-l`, `C-x`, `]p [p`.
- **Ex:** `:d :m :t :co :y :pu :j :> :<`, `:normal :g :v :sort :r :!`, `:w >> file`, `:w !cmd`, a range before `:w`, `:reg :marks :jumps :pwd :cd :make`, `:cc :cfirst`, Tab completion in `:e`, `:s` flags other than `g` and `&`.
- **Insert keys:** `<Home> <End> <PageUp> <PageDown> C-Left C-Right S-Tab C-c C-g C-h C-j C-k <Insert>`. `C-r =` isn't built anywhere.
- **Syntax highlighting.** Source is one colour.
- **Not honoured:** `.gitignore` in the finders and `:grep`.
- **Opening:** `open -a DIE file`, a Finder double-click, `EDITOR=die` (no `--wait`).
- **Editing CRLF and non-UTF-8 files.** They open read-only.
- **Keeping across a restart:** `*grep*`, the search highlight, `g;`'s list, a register over 1 MiB, and a `q` still recording.
- **Interrupting** a slow command on a huge file. `C-g` stops only macros.
- **Mouse:** double-click and drag autoscroll.
- **Linux:** this page is macOS. The Linux build is checked, not run as an app.
