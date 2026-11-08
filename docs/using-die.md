# Using DIE

How to install and run DIE, and every key and command it has on `m0/integration`, read from the binding tables in `crates/die-vim/src`, `crates/die-core/src` and `apps/die-dev/src/lib.rs`. Keys follow your Evil in fundamental-mode unless [m0-log.md](m0-log.md) says otherwise. `\hk` tells you what any key does, and `\hc` what any command does.

## Install and run

```sh
cargo xtask app          # builds ~/Applications/DIE.app from a release build
open ~/Applications/DIE.app
```

- `cargo xtask app` links `~/.local/bin/die` if that directory is on your PATH. Otherwise it prints the one line to add to your shell config. It replaces a link there but never a file. `--home DIR` installs under DIR instead, to try it without touching yours.
- There is one instance. Launching it again from the Dock or Finder does nothing.
- **After changing DIE or your config:** `:rebuild` in DIE. It runs `cargo xtask app` in the checkout this DIE.app was installed from, whatever file you're in, with cargo's output in `*rebuild*` below. To build another checkout, run `cargo xtask app` there once, and `:rebuild` builds that one from then on. If it builds, DIE restarts into it. If not, nothing is installed, the cursor in `*rebuild*` is on the first error, and `gf` on its `-->` line opens the file there. It replaces the bundle that's running, so it refuses for one outside an `Applications` folder. A change to your config takes about 2 s. By hand it's `cargo xtask app` in a terminal, then `:restart`.
- **What a restart keeps:** your buffers, unsaved text, splits, cursors, marks, registers, macros, `.` and last search. `*grep*` and `*Buffer List*` don't come back. A new build that fails to start leaves the old instance running.
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
die --wait msg.txt       # returns once you're done with it (-w for short)
```

`die file:line` leaves a jump, so `C-o` comes back. `open -a DIE file`, a Finder double-click on a file DIE is the opener of, and dropping files on DIE's Dock icon open them in the running DIE too, or start it with them. Finder offers DIE in Open With for any file, and opens nothing with it until you choose it: Get Info, then Open with, then Change All for every file of that kind.

**DIE as git's editor.** `die --wait` opens files as `die` does, then waits until you're done with them, as `emacsclient` does. git runs `$EDITOR` through a shell, so the flag works there. In your shell's startup file:

```sh
export EDITOR="die --wait"
```

Or for git alone, `git config --global core.editor "die --wait"`.

- **Done** is `:wq`, `:x` or `ZZ`, which save first, or `:q`, `ZQ` or `C-w q`, which refuse while it has unsaved changes, as `:q` does. `:q!` drops them. Killing the buffer is done too: `:bd`, `:kill-buffer`, or the Buffer List's `x`. In a split these finish the file and leave the window open. `C-w c` only closes the window.
- **Then** the buffer goes, and its window shows what it showed before, as your Emacs's `server-edit` has it. A file that was open before `die --wait` stays open and on screen, and goes last in the buffer list. `die --wait a b` shows `b`. Done with it, `a` comes up, and it returns once both are done. Several can wait at once, each for its own files.
- **It returns 0** once you're done, so a commit message left as git wrote it aborts the commit, as git decides. If DIE quits or restarts while it waits, it says so and returns 0 too, as `emacsclient` does, and git goes on with what was saved. With no DIE running it starts one and waits.

## DIE's own keys

The leader is `\`, in normal state.

| Key | Command | What it does |
|---|---|---|
| `\\` | `find-file-home` | Fuzzy find any file under your home |
| `\p` | `find-file-project` | Fuzzy find under the nearest parent with `.git` |
| `\f` | `find-file` | Find a file, going down from the buffer's directory |
| `\/`, `\]` | `switch-buffer` | Switch buffer |
| `\}`, `\?` | `buffer-menu` | The Buffer List, to show, save and kill buffers |
| `\r` | `grep` | Search the project |
| `\x`, `M-x` | `command-palette` | Run any command by name, recent first. `M-x` works in insert and visual too |
| `\z` | `toggle-max-width` | Turn the 80-column centered text on or off. The prompts, their lists and the messages keep to its margins, from where the line numbers start, under a status line that runs edge to edge |
| `\hk` | `describe-key` | What a key does |
| `\hc` | `describe-command` | What a command does |
| Cmd-Q | `quit` | Quit, in every state |

`:name` runs any command by name, so also `:restart`, `:kill-buffer`, `:quit`. While you type the name, `:` lists commands as `M-x` does: see [Prompts](#prompts).

**The finders** list files while you type. They skip `.git`, `target`, `Library`, `.cargo/registry`, `.claude/worktrees` and a fixed set of others, and they don't read `.gitignore`. Only the best 1,000 matches can be reached.

**`:grep PATTERN [DIR]`**, or `\r`. Searches with `/`'s smartcase regex, on a background thread, into `*grep*`, with focus staying where it was. One row per line. It stops at 10,000 rows, and skips binary files and files over 10 MB. If the last word names a directory, that's the root, so `:grep mod tests` searches for `mod` in `tests/`.
- `<CR>` on a row opens it in another window.
- `:cn`, `:cp` go to the next and previous result, from any window.
- `gf` and `gF` open `path:line:col`, `path:line`, `path(line)` or `path(line,col)` under the cursor, trying the buffer's directory and then the project root. `]f [f ]F [F` do the same. `gf` in Visual takes the selection as the name.

**The Buffer List**, `\}`, `\?`, `:ls` or `:buffers`, shows `*Buffer List*` in this window, as your Emacs's `buffer-menu` does. `:files`, or a count first, lists only buffers with files. A row per buffer, the one used last first, leaving out the menu and names starting with a space. Its columns are Emacs's: `C` is `.` for the buffer you came from, `>` for one marked to show, `D` to kill; `R` is `%` for read-only; `M` is `*` for unsaved changes, `S` to save; then the name, the size in characters, the mode (Fundamental, as DIE has no major modes yet) and the file. A line break in a name or a path shows as `^J`. The titles are its first line. Evil's keys work there, except these, which are evil-collection's:

- `d` and `C-k` mark the line's buffer to kill and move down, `C-d` moving up, a count marking that many. `s` marks it to save and `m` to show, each moving down.
- `u` takes the line's marks off and moves down, or up with a count. `<Del>` moves up and takes that line's off. `U` takes every mark off.
- `x` saves what's marked `S` and kills what's marked `D`, asking as `:kill-buffer` does when that loses changes to a file. `no` keeps that one and goes on, `Esc` stops there.
- `~` marks the buffer as having no unsaved changes, or with a count as having some. `%` turns its read-only on or off.
- `go` and `<CR>` show the buffer in this window, `gO` in another (a split if this one is alone), `1` alone, `2` with the buffer before it above, and `gv` alone with each one marked `>` in a window below, sized as Emacs sizes them. When they'd leave a window no row of text, `gv` does nothing.
- `o` and `S` sort by the column under the cursor, or by a count's column from 0 (`3o` by name), and the other way when pressed again. `T` lists only buffers with files, or all of them again. `gr` lists the buffers again. `X` puts the buffer last in the buffer list, and its row at the end.
- `q` and `ZZ` show what the window showed before and put the menu last. `ZQ` closes the window, as elsewhere.
- `1` and `2` aren't counts there, but after a count's first digit they're digits of it, so `31d` marks 31 rows. `gg` goes to the titles.
- Evil's insert and edit keys do nothing there, as your evil-collection makes them in a read-only list: `i I a A c C D r O p P J = < >`.

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

On `:`, `/` and `?` only: `C-v` inserts the next key literally, `<Up>` `<Down>` recall history matching what's typed, `C-b` goes to the start, and `C-u` deletes the whole line. `C-p` `C-n` also go through history. On `:` they do beside its list too, as below.

**`:`'s list.** While the line is a command's name, after any range (`%`, `'<,'>`, `2,5`), `:` shows a list like `M-x`'s, flex matched against the name: the ex commands under their whole names (`write`, `quit`, `substitute`, `vsplit`, `nohlsearch`) and every other command. With nothing typed, the ones you ran from `:` come first. Once you type, the top row is the command the name runs, as Vim reads a short name (`quit` for `q`, `substitute` for `s`, `display` for `di`), then the names it starts, then the others that match.

| Keys | What they do on `:` |
|---|---|
| `<CR>` | Run the line as typed, so `:w` writes. Once you've moved to a row, run that row, with the range kept, which a command that takes none ignores |
| `<Down>` `C-n`, `<PageDown>`, `M-<` `M->` | Move in the list. No row is chosen until you do |
| `<Up>` `C-p`, `<PageUp>` | From the line, history as in Vim, so `:<Up>` brings back the last line and `:s<Up>` the last `:s`. In the list, move up, and from the first row back to the line |
| `<Tab>` | Put the chosen row, or else the top one, in place of the name, with nothing after it |
| `M-p` `M-n` | History, list or not |

Once you're in the history, all of these that walk it stay there until you change the line, so `C-p` `C-n` go back and forth as in Vim, though a line there is a name with a list. Once anything follows the name (a space, a `!`, or `/` after `s`), the list goes, and its rows: the line moves to the bottom row, as Vim's is, and the keys are as above.

In `\f`, `<S-Tab>` goes up a directory: a name you've half typed goes first, then the directory on the line, and from `~/` it goes to `/Users/`.

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
- **Registers:** `"x` then a command. `a-z`, `A-Z` to append, `0`, `1-9`, `-`, `_`, `+` the clipboard, and read-only `.`, `:`, `/`, `%`. Every kill also goes to the clipboard, as in your Emacs. `:reg` (`:registers`, `:di`, `:display`) lists them all in `*registers*` below, a macro as its keys, and `:reg ab` only `a` and `b`. `:q` closes it.
- **Macros:** `q{x}` records into `x`, `q{X}` adds to it, and `q` stops. While it records, the bottom row's right end says `recording @x`, and stopping says `Recorded @x` (or `Added to @x`). `@x` plays it, `@@` plays the last again, `@:` runs the last ex line, each with a count. A macro can be edited as text: `"xp`, change it, `0"xy$`. A typed `C-g` stops a running macro, or a `:norm`.
- **On each line:** `:norm keys` (`:normal`, `:norm!`) types the keys on each line of a range, from the line's start, the cursor's line if none is given: `:%norm A;`, `:'<,'>norm @q`, `:5,$norm .`. A line whose keys fail is left quietly and the next goes ahead, and a macro or `:norm` that ran the `:norm` goes on after it. One `u` undoes it all. Each character is a key, so put an Esc or Enter in with `C-v` first. A prompt the keys leave open is answered as it stands, as in your Evil, so `:%norm :s/a/b/` runs the `:s` on each line. `:g/pat/cmd` runs an ex command on each line that matches, and `:g!` and `:v` on each that doesn't, the whole file if no range is given: `:g/TODO/norm @q`, `:v/^$/norm A;`. Any delimiter works, an empty pattern is the last search, and the pattern becomes the last search. It needs a command: there's no `:p`. The commands on lines below work there too: `:g/TODO/d`, `:v/./d` to drop blank lines, `:g/^/m0` to reverse the file, `:g/fn /t$` to copy every `fn` line to the end. `:g/pat/d` alone, with nothing after the `d`, deletes them all at once into no register, and leaves the last search as it was, as in your Evil. One `u` undoes any `:g`.
- **Lines:** each takes a range (`%`, `'<,'>`, `N,M`, `.`, `$`, `'x`, `+n`, `-n`, or a count before `:`), the cursor's line if none is given, and is one `u`. None is what `.` repeats, as in your Evil, and `@:` runs it again.
  - `:d [x] [count]` (`:delete`) deletes the lines into register `x` or the kill ring, as `dd` does. A count takes that many lines from the range's last, so `:d 3` and `:d a 3` work, and `:d3` too. `:y [x] [count]` (`:yank`) yanks them the same way.
  - `:pu [x]` (`:put`) puts register `x`, or the newest kill, as whole lines below the range's last line, whatever the register holds. `:pu!` puts them above. The cursor goes to the first non-blank of the last line put.
  - `:m {address}` (`:move`) moves the lines after that line: `:m0` to the top, `:m$` to the end, `:m+1`, `:m-2`, `:m'a`. `:t {address}` and `:co[py]` copy them there: `:t.` duplicates the line, `:t$` copies it to the end. The cursor goes to the last line moved or copied.
  - `:j[oin] [count]` joins the lines as `J` does, `:j!` as `gJ` does, and a count joins that many from the range's last.
  - `:>` and `:<` shift the lines once, as `>>` and `<<` do. Your Evil ignores a count there, and `:>>` and `:<<` are no commands in it, so neither is here.
  - `:sor[t]` sorts the lines, the whole file if no range is given, `:sort!` the other way, ties keeping their order. Flags: `i` ignores case, `u` keeps one of lines that are the same, `n` sorts by the first decimal number, `x` by the first hexadecimal one, `o` octal, `b` binary, a line with no number coming first. `:sort /pat/` sorts by what follows the first match, and with `r` by the match itself; a line it doesn't match sorts first. The pattern is Rust's regex and always ignores case, as Emacs's sort does. `:sort //` is a plain sort: your Evil doesn't see `/`'s last search there.
- **Substitute:** `:s/a/b/`, with flags `g` and `&`, `~` for the last replacement, ranges `%`, `'<,'>`, `N,M`, `.`, `$`, `'x`, `+n`, `-n`. A count before `:` starts the line with that many lines from the cursor's, so `3:` is `:.,.+2`. `&` repeats it on the line, `g&` everywhere, also `:&`, `:&&`, and `:s` alone.

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
| `<Home>`, `<End>` | To the start, the end, of the whole text, as your Emacs's Home and End are on macOS |
| `C-Left`, `C-Right` | Back to a word's start, on to a word's end, by Emacs's words (`_` ends one, `$` and `%` don't) |
| `<PageDown>`, `<PageUp>` | A window down, up, less two rows, or with `C-u` count rows. A cursor left behind goes to the edge row |
| `C-j` | New line, with no indentation, and the spaces before the cursor kept |
| `C-k xy` | The digraph `xy` names, from RFC 1345's 1,665, either way round: `C-k e:` is ë, `C-k a*` is α, `C-k Eu` is €. Two keys that name none put in the second |
| `<Insert>` | From insert state to replace state, or back. The count goes, and each is its own `u` |
| `C-g` | Forget a `C-u` count, and stay |
| `C-h k`, `C-h c`, `C-h x`, `C-h f` | What a key does, what a command does, as `\hk` and `\hc` |
| `C-c` then a key, `<S-Tab>` | Nothing: your Emacs binds nothing to them there, and says so |

## Windows and buffers

- **`C-w` then:** `h j k l` and arrows, `w W t b p` to move; `s S v n` to split; `c q o` to close; `= + - > < _ |` to size; `H J K L` to move the window; `r R x` to rotate and exchange; `:` for ex; digits for a count. A key with Control does the same, as `C-w C-h`.
- **Ex:** `:sp[lit]`, `:vs[plit]`, `:new`, `:vne[w]`, each with a file; `:clo[se]`, `:on[ly]`. Every ex command takes Vim's short forms, as `:wri`, `:bdel`, `:nohls`, `:sub/a/b/`.
- **Buffers:** `:e file`, `:e!`, `:b name`, `:bn`, `:bp`, `C-^`, `:bd`, `:bw` (`!` to drop unsaved text), `:kill-buffer`. `:bn` and `:bp` go through file buffers. `:ls` (or `:buffers`) opens [the Buffer List](#dies-own-keys), and `:files` the same with only buffers with files.
- **Files:** `:w`, `:w!` over a file changed on disk, `:up`, `:wa`, `:q`, `:q!`, `:wq`, `:x`, `:wqa`, `:xa`, `:qa`, `ZZ`, `ZQ`. Files changed on disk reload by themselves if you have no unsaved text there, and `:w` refuses if you have. On a file `die --wait` waits on, `:q` and the rest finish it rather than close the window or quit: see [DIE as git's editor](#install-and-run).
- **Saving elsewhere:** `:w path` from a buffer with no file, such as `:new`'s, saves it there, and the buffer then visits that file. From a buffer with a file it writes a copy, and the buffer keeps its file and its unsaved changes, as in your Evil. `:sav[eas] path` saves and moves the buffer to the new file. A path is taken from the buffer's directory, as `:e` takes it, and `~/` is your home. A file that's already there is written over only with `!` (`:w! path`, `:saveas! path`). `:wq path` and `:x path` write there, then close.
- **Which file:** `:f` (`:file`) and `C-g` say the file's full path, its lines and how far down the cursor is, as your Evil's `:f` does. The status line shows only the directory. `C-g` is Vim's: your Evil's quits.

## What isn't there yet

- **A terminal.** No terminal buffers exist. cargo, git and Claude Code run in a terminal of your own next to DIE, and `die file:line` from there.
- **Normal keys:** `gq gw`, `( )` and `is as`, `it at`, `gn gN`, `g?`, `gm`, `gp gP`, `Q`, `C-]`, `C-w f`, `[] ][`, `z. z<CR> z- z+ z^`, folds, `gD`, `C-l`, `C-x`, `]p [p`, `<Insert>`.
- **Ex:** `:p`, `:r :!`, `:w >> file`, `:w !cmd`, a range before `:w` or `:up`, a `/pattern/` as an address, `:marks :jumps :pwd :cd :make`, `:cc :cfirst`, Tab completion in `:e`, `:s` flags other than `g` and `&`. `:pu =` and `C-r =` aren't built anywhere. A range given backward, as `:3,1d`, is refused, where your Evil asks whether to swap it.
- **Insert keys:** `C-h` has only `k c x f`; its other keys say they aren't built.
- **Syntax highlighting.** Source is one colour.
- **Not honoured:** `.gitignore` in the finders and `:grep`.
- **`die --wait`:** a client that has gone, as one stopped with Ctrl-C, isn't noticed, so its file still waits for you to finish it. Nothing ends an edit so that the client fails, as Emacs's `server-edit-abort` does. A file that won't open returns 0 at once, where `emacsclient` returns 1. Quitting DIE with clients waiting doesn't ask first, as Emacs does.
- **Opening from Finder** into a minimized DIE leaves its window in the Dock, by reading AppKit: nothing restores it.
- **Editing CRLF and non-UTF-8 files.** They open read-only.
- **The Buffer List's** `.` after its keys (your Evil repeats them, as it repeats any key that isn't Evil's), `gV` (View mode), `t` (tags tables, so `t` is Evil's), `M-s a C-s`, `M-s a C-M-s` and `M-s a C-o` (searching the marked buffers), `M-DEL`, a middle click, and the column keys `gl gh { }`.
- **Keeping across a restart:** `*grep*`, `*Buffer List*`, the search highlight, `g;`'s list, a register over 1 MiB, and a `q` still recording.
- **Interrupting** a slow command on a huge file. `C-g` stops only macros and `:norm`, so a `:g` whose command isn't `:norm` runs to its end: `:g/./s/a/b/` on 40,000 lines takes about 2 s.
- **Mouse:** double-click and drag autoscroll.
- **Linux:** this page is macOS. The Linux build is checked, not run as an app.
