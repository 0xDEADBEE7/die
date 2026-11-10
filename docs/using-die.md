# Using DIE

How to install and run DIE, and every key and command it has on `m0/integration`, read from the binding tables in `crates/die-vim/src`, `crates/die-core/src` and `apps/die-dev/src/lib.rs`. Keys follow your Evil in fundamental-mode unless [m0-log.md](m0-log.md) says otherwise. `\hk` tells you what any key does, and `\hc` what any command does.

## Install and run

```sh
cargo xtask app          # builds ~/Applications/DIE.app from a release build
open ~/Applications/DIE.app
```

- `cargo xtask app` links `~/.local/bin/die` if that directory is on your PATH. Otherwise it prints the one line to add to your shell config. It replaces a link there but never a file. `--home DIR` installs under DIR instead, to try it without touching yours.
- There is one instance. Launching it again from the Dock or Finder does nothing.
- **After changing DIE or your config:** `:rebuild` in DIE. It runs `cargo xtask app` in the checkout this DIE.app was installed from, whatever file you're in, with cargo's output in `*rebuild*` below. To build another checkout, run `cargo xtask app` there once, and `:rebuild` builds that one from then on. If it builds, DIE restarts into it. If not, nothing is installed, the cursor in `*rebuild*` is on the first error, and `<CR>` or `gf` on its `-->` line opens the file there, as `:cn` does from anywhere. It replaces the bundle that's running, so it refuses for one outside an `Applications` folder. A change to your config takes about 2 s. By hand it's `cargo xtask app` in a terminal, then `:restart`.
- **What a restart keeps:** your buffers, unsaved text, splits, cursors, marks, registers, macros, `.` and last search, and a `die --wait` still waits. `*grep*`, `*compilation*`, `*Shell Command Output*`, `*rebuild*` and `*Buffer List*` don't come back, though `:make`'s command does, and where it ran. Commands still running are stopped, at a restart as at quit. A new build that fails to start leaves the old instance running.
- **Quit:** `:q` on the last window, `:qa`, or Cmd-Q. Each refuses if a file is unsaved. `:qa!` doesn't. Each asks first while a program runs in a terminal, `:qa!` too.

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

`die file:line` leaves a jump, so `C-o` comes back. `open -a DIE file`, a Finder double-click on a file DIE is the opener of, and dropping files on DIE's Dock icon open them in the running DIE too, or start it with them. A name with an accent comes as you'd type it, so `:b café` finds it. DIE's Info.plist says it can edit any file, at the rank that makes it the opener of none, so Finder should list it in Open With, and Get Info, then Open with, then Change All should make it the opener of every file of that kind. Only `open -a` has been tried: neither Finder's list nor a double-click has.

**DIE as git's editor.** `die --wait` opens files as `die` does, then waits until you're done with them, as `emacsclient` does. git runs `$EDITOR` through a shell, so the flag works there. In your shell's startup file:

```sh
export EDITOR="die --wait"
```

Or for git alone, `git config --global core.editor "die --wait"`.

- **Done** is `:wq`, `:x` or `ZZ`, which save first, or `:q`, `ZQ` or `C-w q`, which refuse while it has unsaved changes, as `:q` does. `:q!` drops them. Killing the buffer is done too: `:bd`, `:kill-buffer`, or the Buffer List's `x`. In a split these finish the file and leave the window open. `C-w c` only closes the window.
- **`:cq` gives up** on the file: `die --wait` returns 1, so git stops the commit or the rebase, and the file goes as with `:q!`, with what you typed. On a file nothing waits on, `:cq` says "This buffer has no clients" and does nothing.
- **Then** a buffer made for `die --wait` goes, unsaved text and all, and its window shows what any window shows after a kill: the buffer you used last that no window shows. Your Emacs's `server-edit` goes back to what that window showed, which is the same unless another window shows it. A file that was open before `die --wait` stays open and on screen, and goes last in the buffer list. `die --wait a b` shows `b`. Done with it, `a` comes up, and it returns once both are done. `:q!` and `:cq` bring nothing up and say nothing, as your Evil's `:q!` runs only `server-buffer-done`. Several can wait at once, each for its own files.
- **It returns 0** once you're done, so a commit message left as git wrote it aborts the commit, as git decides. It returns 1 after `:cq`, if none of its files would open, or if DIE quits or crashes before you're done, so that git stops rather than go on with a message or a rebase's list saved half done. That's where it differs from `emacsclient`, which returns 0 when Emacs goes. A restart (`:rebuild`, `:restart`) hands it to the new DIE, which waits on its files as they were. With no DIE running it starts one and waits, and while DIE is starting or restarting it waits up to 15 s for it.

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
| `M-!` | `shell-command` | Run a shell command, as `:!` does. It works in insert and visual too |
| `\z` | `toggle-max-width` | Turn the 80-column centered text on or off, in a file or a terminal. The prompts, their lists and the messages keep to its margins, from where the line numbers start, under a status line that runs edge to edge |
| `\hk` | `describe-key` | What a key does |
| `\hc` | `describe-command` | What a command does |
| `\e`, `:term`, `:terminal` | `term` | Open a new [terminal](#terminal) in this window, as your `termin-open` does |
| `\g` | `term-switch` | Go to `*terminal*`, or open it, as your `ghostel` does |
| `\o` | `orchid` | orchid's session browser, as your emacs-orchid-client's `orchid`: see [orchid](#orchid) |
| Cmd-Q | `quit` | Quit, in every state. It asks first while a program runs in a terminal |
| Cmd-V in a terminal | `surface-paste` | Paste the clipboard into the program, as one piece it sees bracketed, or `"` if the clipboard has no text |

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

## Running commands

Commands run without a terminal, with your `$SHELL`, in the buffer's directory, as Emacs runs them. They get the PATH your login shell sets, asked once each time DIE starts, with `~/.local/bin` and `~/.cargo/bin` first if it hasn't got them, so DIE.app from the Dock finds cargo and git as your terminal does.

**`:make cargo test`**, or `M-x compile`. The command's output and errors stream into `*compilation*` below as they come, a line before its newline too, with the focus staying where it was, under Emacs's header: the `-*- mode: compilation; default-directory: ... -*-` line, `Compilation started at ...` and the command. Its last line is Emacs's, `Compilation finished at ..., duration 4.56 s` or `Compilation exited abnormally with code 101 ...`, and the command row says it too.
- `M-x compile` asks with the last command on the line (`make -k ` the first time), and `<Up>` for older ones. `:make` alone, `M-x recompile`, and `gr` in `*compilation*` run the last one again, where it ran, after a restart too.
- Files with unsaved changes are offered for saving first, one at a time: `y` saves it, `n` doesn't, `!` saves the rest, `q` saves no more, and `Esc` runs nothing.
- One runs at a time. Another asks whether to stop the one running. `C-c C-k` in `*compilation*`, or `M-x kill-compilation`, stops it as Ctrl-C would. Quitting DIE, or a restart, stops it without asking: a hangup, then a kill if it's still there.
- `<CR>` on an error's line opens the file there, in another window, and `gf` on its path opens it in this one. `:cn` and `:cp` go through the errors from any window, as they go through `*grep*`'s rows: whichever you made or went to last. In `*compilation*`, `gj` `gk`, `C-j` `C-k` and `<Tab>` `<S-Tab>` move to the next and previous error's line.
- An error is rustc's and cargo's `--> path:line:col`, a test's `panicked at path:line:col:`, or `path:line:col: message` and `path:line: message`. `:cn` passes over notes: a `-->` under a `note:` or `help:` line, any `-->` after an error's first, and `:::` lines, which point at what explains the error, as std's source or a struct's definition. `<CR>` on one opens it.
- rustc's paths, and a test's, are read from the cargo workspace's root first, since cargo names files from there wherever it runs, then from the directory the command ran in, then from its project (`.git`). `path:line:` lines are read from the directory it ran in first. A path that's in none of them says "Can't find PATH from DIR".
- A carriage return with more of its line after it wipes the line, as in Emacs, so a progress line shows its last state.
- Output past 8 MiB is cut, with a note saying so, and the command runs on.

**`:!cmd`**, or `M-!`. Runs a shell command, and shows one line of what it prints on the command row, or more in `*Shell Command Output*` below. With nothing printed, it says whether it succeeded. In the command, `%` is the buffer's file and `#` the other buffer's, `\%` and `\#` themselves. `:!!` runs the last one again.
- **`:{range}!cmd`** sends the lines to the command and puts what it prints, errors too, in their place, as one change: `:%!sort`, `:'<,'>!jq .`, `:.!tr a-z A-Z`. Output with no newline at its end runs into the next line, as in your Evil.
- **The `!` operator** opens `:` with its lines and a `!`: `!!` for this line, `!ip`, `!G`, or `!` over a selection.
- **`:r !cmd`** puts what the command prints below the cursor's line, or the range's last, and `:0r !cmd` above the first. `:r file` does the same with a file's text.
- A key after a command waits up to half a second for it, so a macro with `:%!sort` works on the sorted lines. A slower one goes on in the background and its output comes when it's done, or it's stopped when DIE quits. If you changed the text meanwhile, a filter leaves it alone and says so. A command that prints more than 8 MiB is stopped. A filter or `:r !` whose output isn't UTF-8 leaves the text alone too, and says so.

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
- **Lines:** each takes a range (`%`, `'<,'>`, `N,M`, `.`, `$`, `'x`, `+n`, `-n`, or a count before `:`), the cursor's line if none is given, and is one `u`. A range's line 0 is line 1, as in Vim, except that `:0r` reads above it. None is what `.` repeats, as in your Evil, and `@:` runs it again.
  - `:d [x] [count]` (`:delete`) deletes the lines into register `x` or the kill ring, as `dd` does. A count takes that many lines from the range's last, so `:d 3` and `:d a 3` work, and `:d3` too. `:y [x] [count]` (`:yank`) yanks them the same way. After a range, `:y` leaves the cursor where it was, or at the selection's start if you typed `:` in visual state.
  - `:pu [x]` (`:put`) puts register `x`, or the newest kill, as whole lines below the range's last line, whatever the register holds. `:pu!` puts them above. The cursor goes to the first non-blank of the last line put.
  - `:m {address}` (`:move`) moves the lines after that line: `:m0` to the top, `:m$` to the end, `:m+1`, `:m-2`, `:m'a`. `:t {address}` and `:co[py]` copy them there: `:t.` duplicates the line, `:t$` copies it to the end. The cursor goes to the last line moved or copied. Moved to where they are, nothing moves, and a range given leaves the cursor at its start.
  - `:j[oin] [count]` joins the lines as `J` does, `:j!` as `gJ` does, and a count joins that many from the range's last.
  - `:>` and `:<` shift the lines once, as `>>` and `<<` do. Your Evil ignores a count there, and `:>>` and `:<<` are no commands in it, so neither is here.
  - `:sor[t]` sorts the lines, the whole file if no range is given, `:sort!` the other way, ties keeping their order. Flags: `i` ignores case, `u` keeps one of lines that are the same, `n` sorts by the first decimal number, `x` by the first hexadecimal one, `o` octal, `b` binary, a line with no number coming first (before any number down to your Emacs's `most-negative-fixnum`, as there). `:sort /pat/` sorts by what follows the first match, and with `r` by the match itself; a line it doesn't match sorts first. The pattern is Rust's regex and always ignores case, as Emacs's sort does. `:sort //` is a plain sort: your Evil doesn't see `/`'s last search there.
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
| `<Home>`, `<End>` | To the start, the end, of the whole text, as your Emacs's Home and End are on macOS. After `C-u n`, n tenths of the way in, from the start or the end, then to the next line's start, as there |
| `C-Left`, `C-Right` | Back to a word's start, on to a word's end, by your Emacs's own words, not your Evil's: `_` ends one and `$` and `%` don't, a change of script ends one unless it's kanji then kana, and hiragana then katakana ends one too |
| `<PageDown>`, `<PageUp>` | A window down, up, less two rows, or with `C-u` count rows. A cursor left behind goes to the edge row |
| `C-j` | New line, with no indentation, and the spaces before the cursor kept |
| `C-k xy` | The digraph `xy` names, from RFC 1345's 1,665, either way round: `C-k e:` is ë, `C-k a*` is α, `C-k Eu` is €. Two keys that name none put in the second. `<Tab>`, `<CR>`, `<BS>` and a control key are their characters |
| `<Insert>` | From insert state to replace state, or back. The count goes, and each is its own `u` |
| `C-g` | Forget a `C-u` count, and stay. As in your Evil, what's typed after it isn't part of what `.`, the session's count or a selection's other lines put in again |
| `C-h k`, `C-h c`, `C-h x`, `C-h f` | What a key does, what a command does, as `\hk` and `\hc` |
| `C-c` then a key, `<S-Tab>` | Nothing: your Emacs binds nothing to them there, and says so |

## Windows and buffers

- **`C-w` then:** `h j k l` and arrows, `w W t b p` to move; `s S v n` to split; `c q o` to close; `= + - > < _ |` to size; `H J K L` to move the window; `r R x` to rotate and exchange; `:` for ex; digits for a count. A key with Control does the same, as `C-w C-h`.
- **Ex:** `:sp[lit]`, `:vs[plit]`, `:new`, `:vne[w]`, each with a file; `:clo[se]`, `:on[ly]`. Every ex command takes Vim's short forms, as `:wri`, `:bdel`, `:nohls`, `:sub/a/b/`.
- **Buffers:** `:e file`, `:e!`, `:b name`, `:bn`, `:bp`, `C-^`, `:bd`, `:bw` (`!` to drop unsaved text), `:kill-buffer`. `:bn` and `:bp` go through file buffers. `:ls` (or `:buffers`) opens [the Buffer List](#dies-own-keys), and `:files` the same with only buffers with files.
- **Files:** `:w`, `:w!` over a file changed on disk, `:up`, `:wa`, `:q`, `:q!`, `:wq`, `:x`, `:wqa`, `:xa`, `:qa`, `ZZ`, `ZQ`, and `:cq` on a file `die --wait` waits on. Files changed on disk reload by themselves if you have no unsaved text there, and `:w` refuses if you have. On a file `die --wait` waits on, `:q` and the rest finish it rather than close the window or quit: see [DIE as git's editor](#install-and-run).
- **Saving elsewhere:** `:w path` from a buffer with no file, such as `:new`'s, saves it there, and the buffer then visits that file. From a buffer with a file it writes a copy, and the buffer keeps its file and its unsaved changes, as in your Evil. `:sav[eas] path` saves and moves the buffer to the new file. A path is taken from the buffer's directory, as `:e` takes it, and `~/` is your home. A file that's already there is written over only with `!` (`:w! path`, `:saveas! path`). `:wq path` and `:x path` write there, then close.
- **Which file:** `:f` (`:file`) and `C-g` say the file's full path, its lines and how far down the cursor is, as your Evil's `:f` does. The status line shows only the directory. `C-g` is Vim's: your Evil's quits.

## Terminal

`\e`, `:term`, `:terminal` or `M-x term` opens a terminal in this window, in the buffer's directory: `/bin/bash`, as your Emacs's `ghostel-shell` and `vterm-shell` have it, interactive and not a login shell, as vterm (and so your `termin-open`) starts it, so it reads your `~/.bashrc`, whose prompt and PATH additions go on the login PATH DIE works out. DIE's default, for a config that sets no shell, is `$SHELL` as a login shell. `:term cmd`, as `:term top`, runs `cmd` through that shell instead (`/bin/bash -c cmd`), as Vim's `:terminal cmd` does. The first is `*terminal*`, the next `*terminal*<2>`, and so on. `\g` (`term-switch`) goes to `*terminal*`, or opens it if it isn't there, as your `ghostel` does. The status line shows the terminal's directory, then the program's title or the buffer's name, and `<I>` or `<N>` at the right. bash and zsh report each `cd`; sh and a login bash don't yet, so theirs is the directory they started in. When the program exits, the buffer closes, and an exit that isn't 0 is said in the messages.

- **Insert state** (`<I>`) is where it starts, with a bar cursor on the main screen, as evil-ghostel draws it, and the program's own on the alternate screen. Every key goes to the program but Cmd's: `\`, `C-c`, `C-z`, `C-d`, `C-r`, `C-k`, `C-h`, `C-g` and Tab too, with Option as Meta and the kitty keyboard flags for programs that ask. A Cmd key DIE doesn't bind goes nowhere. A key brings the live screen back if you'd scrolled up.
- **Esc in insert** always goes to normal state, on the alternate screen too, so Esc Esc interrupts Claude Code, its fullscreen mode included, and leaves vim's insert mode: the first Esc leaves insert, and normal state's sends the program one. That holds whatever the program asked of the terminal: the kitty keyboard flags, mouse tracking, bracketed paste. Ctrl-[ is Esc, as in your Emacs, here and in normal state. Cmd-Esc always goes to normal state. Shift-, Ctrl- and Alt-Esc always go to the program. After one Esc, what you type is DIE's: vim's `:wq` from habit says the terminal has no file, and `:q` asks before quitting DIE while a program runs, so give vim Esc Esc, or `C-c C-r` the terminal first. A key you let go after Esc, as a letter rolled over into it or the Ctrl of Ctrl-[, still has its release sent to a program that asked for releases.
- **The toggle**, `C-c C-r` in normal state or `M-x term-toggle-send-escape`, changes where insert's Esc goes in this terminal: from normal state, the default, to the program, for vim run in the terminal, so its Esc goes straight to vim and you stay in insert, and pressed again back to normal state. A count picks one, as evil-ghostel's `evil-ghostel-toggle-send-escape` does: `1 C-c C-r` is evil-ghostel's `auto` (to the program while it's on the alternate screen, as vim, less and htop are, and to normal state otherwise), `2` the program, `3` normal state. From auto, `C-c C-r` goes back to normal state. The messages say which. Cmd-Esc leaves insert whichever it is.
- **Pasting** is one piece, bracketed when the program asked (zsh, Claude Code, vim). A paste with a line break into a program that didn't ask, which could run it as a command, asks first, and the program's output goes on showing while it does.
- **The pointer** in insert state: a program that tracks the mouse (htop, `vim` with `mouse=a`) gets clicks, drags and the wheel over the terminal's columns, and a drag that goes into a margin stops at the edge. A click in a margin only focuses the window. Otherwise the wheel scrolls back through the last 10,000 lines, from a margin too, and less and man scroll on the alternate screen.
- **Size and margins:** a terminal is a file's width, 80 columns centered in your config, with blank margins either side and the status line edge to edge, and `\z` in normal state turns that on and off as in a file; the program hears the new size, and the cursor stays on its text, scrolled back too, while the terminal rewraps long lines to the new width. The terminal fits that area, in cells and pixels, as splits and resizes change it. Two windows on one terminal share it at the smaller area's size, as Emacs sizes a process, and the larger shows it at its area's top left.
- **Copying out:** a program's OSC 52 copy, as vim's `"+y` over SSH, goes on the clipboard and is your newest kill, so `p` pastes it anywhere. A working directory the shell reports (OSC 7) becomes the buffer's, so `\f` and `gf` on a relative path start there. zsh reports it at each prompt: DIE starts a login zsh with a `.zshenv` of its own (under `$TMPDIR`), which puts your `ZDOTDIR` back, reads your `.zshenv`, and adds the hook, and zsh then reads your `.zprofile`, `.zshrc` and `.zlogin` as ever. bash reports it too: DIE starts it with `--rcfile` at an rc of its own (also under `$TMPDIR`), which reads your `~/.bashrc` as bash would have, then adds a hook that keeps `$?` on a line after your `PROMPT_COMMAND` (in bash 5.1 and later, whose `PROMPT_COMMAND` can be an array, after its first element and before the rest). A bash you start inside the terminal inherits an exported `PROMPT_COMMAND` but not the hook, and finds no hook quietly. A login bash ignores `--rcfile`, so it doesn't report. Both rcs live in directories of DIE's that only you can write, and a name with `%` or a control character is reported escaped, so any directory's name comes through.
- **Restarts:** terminals don't survive `:rebuild` or `:restart` yet. A restart hangs their programs up, as quitting does, and kills what's left of the shell's own process group 300 ms later, and the restored session has no terminal. The shell hangs its jobs up too, so a job that ignores the hangup (`nohup`, or `trap '' HUP`) goes on running, as it would after closing a Terminal window. `:restart`, `:rebuild`, Cmd-Q, `:q` in the last window, `:qa`, `ZQ` and the rest ask first while a program other than the shell runs in one, with `!` too, since `!` speaks for unsaved text.

**Normal state** in a terminal is your Evil over what it shows: the scrollback and the screen, as text you can read, search and yank but not change. While a program is on the alternate screen (vim, less, btop, Claude Code's fullscreen mode), it's that screen alone, which has no scrollback. Esc from insert state goes there (Cmd-Esc if the toggle sends Esc to the program), with the cursor on the terminal's own, or after the wheel scrolled back, at the rows the wheel left showing, with the cursor on the last of them.

- [Moving](#moving) and [Selecting](#selecting) work as in a file: counts, objects, `/ ? n N * #`, marks, `v V C-v`, and `y` into registers and the clipboard, `"x` too. So do `:`, the `C-w` keys and your leader's.
- **Scrolling:** `C-d C-f C-b C-e C-y`, `zz zt zb`, `gg` and `G` go over the scrollback as over a file's lines, never lower than the live screen. `C-u` is the prefix argument there too, as in your Evil.
- **What shows** is the terminal's own cells and colors, scrolled to where you are, with the cursor, the selection and the search's matches drawn over them.
- **New output:** while the cursor is on the terminal's cursor line or below it, normal state follows the output, and a cursor on the terminal's stays on it. Move up and it holds still while the program goes on printing, though what it shows is still what `y` takes, after a `clear` too. `G` (`term-goto-cursor`) goes to the terminal's cursor, as evil-ghostel's does, so it follows again; `5G` is line 5. A program switching screens, as one leaving the alternate screen, takes you there too, since the other screen's lines aren't the ones you left. A selection, a waiting operator or a prompt holds it too, until it ends.
- **Lines are rows:** a line the program wrapped is a line per row, and the screen's empty rows below the prompt aren't lines, so `j` stops at the prompt, and so does `G` with a count past it. A resize rewraps them, `\z` or a split too, and a cursor that isn't following the output stays on its text at its column there, as do a selection and the top row.
- **Edits** (`x d c o r J` and the rest) say "Buffer is read-only", and `u` that there's nothing to undo.
- **The program's keys:** `<Esc>` sends the program one Esc and stays in normal state, as your `vterm-send-escape` binding does, so Esc Esc from insert interrupts Claude Code, on either screen. It's encoded as the program's kitty flags ask, with its release for a program that asked for releases. Unlike vterm's, more Escs with no other key between them send nothing, so Esc pressed a few times to be sure of normal state sends one; any other key, a motion too, re-arms it, and so does going back through insert. At the shell's own prompt Esc sends nothing at all, as Evil's Esc does nothing, so bash doesn't offer to "Display all 1773 possibilities". Nor does it while the terminal is in line mode, as a job that reads no keys leaves it (`sleep`, a `cargo build`, `cat`): there an Esc would only wait for the prompt, which would read it as Meta and lose your first letter. A count sends that many, at most two, and only where a lone Esc would go: `2<Esc>` is Claude Code's rewind. After a register (`"a<Esc>`) Esc only ends the prefix. A shell you start inside the terminal, as `zsh` typed at bash's prompt, is a program to DIE, so Esc reaches it, and its line editor then reads the first key you type after `i` as Meta, as bash did. The other way round, a program in the shell's own process group, as `fzf` in `cd $(fzf --height 40%)`, looks like the shell at its prompt, so normal state's Esc doesn't reach it; `C-c C-r` sends insert's Esc to it. `i a I A` go back to insert, at the live screen. `p P` and Cmd-V paste into the program, the clipboard or `"x`, and with a count, as `"a3p`, that many times. `C-c C-c` sends the program C-c, as your ghostel's does in every input mode, so it interrupts a job or Claude Code without going back to insert; `C-c C-z`, `C-c C-\` and `C-c C-d` send C-z, C-\ and C-d. `C-c C-r` is the toggle above.
- **The mouse** is a file's: a click moves the cursor (in a margin, to the row's start or end), a drag selects, the wheel scrolls the scrollback. The program hears no click or drag in normal state. A program that tracks the mouse (Claude Code's fullscreen mode, htop) still gets the wheel, as in insert and as your ghostel sends it in every Evil state, since its screen has no scrollback to scroll.

## orchid

`\o`, or `M-x orchid`, is your emacs-orchid-client, ported as die-orchid-client: an extension DIE builds beside itself, outside the product (scope D14), which your config registers. It drives orchid 0.2.2, your agent CLI, as Emacs does: `orchid` on the PATH your login shell sets (so `~/.local/bin/orchid` from DIE.app too), on orchid's own root, `$ORCHID_CONFIG` or else `~/.config/orchid`, passed to every command as `--config`. Its commands never inherit `ORCHID_SESSION_ID`, `ORCHID_CONFIG` or `ORCHID_HOOK_DEPTH`, so a DIE started from inside an orchid tool doesn't make its sessions children of that one. Emacs and DIE can be open on the same sessions at once: control goes through orchid, and its files are read without a lock, as orchid's own readers read them.

**The session browser**, `\o` or `M-x orchid`, shows `*orchid-sessions*` in this window, never a split, laid out as seek laid it out in Emacs: the title centred in `─`, the search line, the columns `LABEL`, `STATUS` and `UPDATED`, and a row per session, the one changed last first. It's 78 columns wide, or the window's width if that's less, and a row wider than the window wraps at a word, as your Emacs's `global-visual-line-mode` wrapped it there. LABEL is the session's label, or else its agent and workspace (`default-die`), or else its id. STATUS is `ACTIVE` while a chat is open on it and `IDLE` otherwise, with `•RUN` while it runs, wherever the turn was started, and `DEL` or `KILL` when it's marked. UPDATED says how long ago it last changed. The selected row is highlighted, and no cursor is drawn. A window shows as many rows as fit under the six lines above them, 20 at most, and `No results` when there are none.

It reads each session's `metadata.json` rather than running `orchid list`, so it opens at once, with times and order Emacs's never had. A status that changes redraws it, and while a window shows it, it reads the sessions again every 2 s, so a turn you started from Emacs shows too. When the list changes under it, the selection stays on its session. It's read-only, and `:rebuild` brings it back with its query, selection and marks. In normal state, as your Evil has its keys:

- `j` and `k` move down and up, `p` up too (in Emacs, Evil's `p` pasted there), and `J` and `K` a page.
- `RET` opens the selected session's chat in this window, or goes to the one open on it, and kills the browser. `n` asks `Agent: ` among the valid agents and starts a chat on a new session in the browser's directory, the one of the buffer you opened it from; the browser stays. `q` kills it.
- `r` and `g` read the sessions again.
- `D` marks the session to delete (orchid archives it) and `S` to stop its turn, which only a running one has; each moves down. `u` takes the mark off. `x` asks `Execute N marked operation(s)? (yes or no)`, which wants `yes` or `no` typed out, in any case but with nothing around it, as `yes-or-no-p` does, so a bare `RET` asks again. Then it deletes and stops each in turn, in the order you marked them, keeps the marks that failed (`*Orchid Log*` says why), reads the sessions again and says what it did: `Deleted 1 session, Killed 1 process, 1 failed`.
- `/` and `a` search: what you type filters the rows, its characters in that order, in any case, in the label, the status (`run` finds the running ones) or the time it changed. Every printable key goes into the query, `j` and `q` too. `DEL` takes one back, `RET` and `Esc` stop with the query kept, and `C-g` clears it. `d` clears it from normal state too, and goes back to the first row.

**A chat**, `*orchid-chat-<agent>-<wsdir>-<id5>*`, opens in this window from the browser's `RET` or `M-x orchid-open-session`, laid out as your Emacs's: `Workspace:` and `Agent:`, a separator labelled with the workspace, the session's last 250 events, oldest first, each a collapsed 80-column stub with its tokens and local time (`[user]`, `[agnt]`, `[bash]`, `[read]`, `[edit]`, `[rslt]`, and `[fail]` and `[term]`, which Emacs showed as raw JSON), a separator before each of your messages, then `[session restored]` with the session's cumulative input, and your input under the last separator. It starts in normal state at the end, in the directory you opened it from (the browser's, for its `RET`), as Emacs's did, so `find-file` and `orchid-new-session` start there; `/new` goes by the session's. From there it follows the session, every 0.5 s while it runs and every 2 s otherwise, putting each new event in above the line that says how the turn is going. Its keys:

| Key | State | What it does |
|---|---|---|
| `RET` | insert | In your input, send it, trimmed, as `orchid send --id ID -- MSG`; an empty input sends nothing. Above the input, a newline. In normal state `RET` is Evil's, as in your Emacs. |
| `S-RET` | normal, insert | A newline. |
| `TAB` | normal, insert | On a stub or its detail, expand or collapse it; a reply's or a tool call's detail is made the first time, and a result's files are sections of their own inside it. In your input, a tab, which in insert state is part of what `.` puts in again. Anywhere else it says what it's for. |
| `/` | insert | Alone at the start of an input line, the slash menu, `/ `, which wants one of `config` (the find-file prompt at orchid's root), `exit  (quit)` (kill the chat), `kill` (`orchid stop`), `logs` and `metadata` (the session's files), or `new  (clear)` (a new session with this one's agent and directory). Anywhere else, a `/`. |
| `C-c C-l`, `S-Tab` | normal, insert, visual | The session browser, in this window. |
| `C-c C-q` | normal, insert, visual | Kill the chat. The session goes on. |

- **Sending** puts your message's stub and a separator above the input, and under the stub `[Processing: Ns]`, with the cumulative input at column 80, drawn again each second. The turn's events go in above it, collapsed, and it reads `[Finished: Ns]` once the session stops, however it stops (idle, failed, cancelled or terminated). A send that fails says `[SYSTEM] Send failed:` and orchid's reason, framed, above your input.
- **What you type stays put.** Events go in above the input as edits undo never sees, so neither your text nor your cursor moves, in insert state too, and `u` takes back only your typing. A visual selection stays on what it selected, both ends and its shape, and so does one on the ticking indicator. A window reading the transcript is taken to the end when a turn finishes, as Emacs took every window there, a selection there stretching to the end; one in your input stays.
- **Your input** starts at the blank line under the last separator, so `G`, `j` or a click there type into it too (die-vim's motions never go to the empty line after the last newline, where Emacs's input line was). What you type there comes back as it was after `:rebuild`.
- A session already running when you open it shows its indicator, counting from your last message, with its events going in above it. A turn started elsewhere, from Emacs say, shows its message and indicator as one of yours does.
- **Read-only**, as in Emacs: the `[session restored]` line and the separator under it, and the separator under a running session's indicator. The rest of the transcript is yours to edit.
- **Search** (`/ ? n N`) opens a collapsed section it lands in, and `j` and `k` step over hidden text.
- **A new session** (the browser's `n`, `M-x orchid-new-session`, or `/new`) opens `*orchid-chat-<agent>-<wsdir>-new*` at once. Type in it while orchid makes the session, and the session's chat takes its place in the same window with what you typed, still in insert state, labelled `<agent>-<wsdir>` as Emacs labelled it. A create that fails says why there.
- An archived or deleted session's chat says so once, stops following it, and sends nothing more: orchid would refuse it, and its lock would make the session's directory again. A directory left holding only orchid's lock is a session gone too.
- **After `:rebuild`** each chat comes back as a chat, in the windows it was in, rebuilt from `events.jsonl` with what came while DIE was down: the sections you'd expanded expanded again, each window's cursor and scroll on the same text, what you'd typed and your cursor's place in it. It follows again, a chat no window shows too, and if its turn still runs, its indicator ticks again, counting from your last message. One still being made makes its session, or takes the one orchid made while DIE restarted, so there's never a second; one whose session has gone says so and keeps your input. History you'd loaded with `orchid-chat-load-more-history` comes back as the last 250 events.
- `M-x orchid-chat-load-more-history` puts the 250 events before the oldest shown above them, in order; it's unbound, as in Emacs. `M-x orchid-collapsible-report-stats` says how many lazy details were made, and how long that took, in `*orchid-collapsible-stats*`.

What else there is:

- `M-x orchid-check-cli` says whether orchid is on the PATH and its version, and warns below 0.2, which this client is written for.
- `M-x orchid-list-sessions` lists each session as `  label (id)`, oldest first, in `*orchid-list-sessions*`, below, with the focus staying, as Emacs's echo area showed them.
- `M-x orchid-open-session` asks `Session ID or label: ` (or takes it after the name, `:orchid-open-session ID`), finds it by its label too, which Emacs's didn't, and says `Opened session: …`, or `Failed to open session: Session '…' not found`, as Emacs's did.
- `M-x orchid-session-browser-show` is `orchid`'s other name, as in Emacs, and `M-x orchid-collapsible-toggle-at-point` is `TAB` on a section, doing nothing elsewhere.
- `M-x orchid-new-session` asks `Agent: ` among the agents `orchid agent` says are valid, and starts in the buffer's directory. A prompt you open while orchid answers stays yours, and `Agent: ` comes when it closes, as Emacs's never took one from you.
- `M-x orchid-log-show` asks `Session ID: ` (or takes it after the name) and shows ` *orchid-log:<id>*`, beside, as `display-buffer` does: the session's `events.jsonl` as its chat has read it, the last 5 MB at first, then each line as the chat reads it, as Emacs's monitor kept it. It goes with the chat. With no chat following the session, it's Emacs's `Not monitoring session <id>`.
- `*Orchid Log*`, the client's own log, keeps its last 2,000 lines and isn't kept over a restart. You go to it as to any buffer, as in Emacs.
- An orchid command that hasn't answered in 5 minutes is stopped, with whatever it started that's still in its process group; orchid's own turn worker is in a group of its own, so it goes on.
- Nothing of orchid's runs or is read until you ask. While no orchid buffer is open, the client sets no timer. While one is, a single timer looks at the sessions for all of them: every 0.5 s while a turn runs or a send is out, every 2 s otherwise, to see a turn started elsewhere. A turn's indicator ticks each second only while the turn runs. That 2 s look is all an idle DIE does for orchid: it doesn't keep DIE's own looks at your files and its session awake, which rest at one every 5 s after 30 s of nothing from you.

## whiteboard

`\w`, or `M-x whiteboard`, is a whiteboard in a DIE buffer: die-whiteboard, an extension DIE builds beside itself, outside the product (scope D14), which your config registers. A pen first, one-key shape and arrow tools worked with the pointer, labels and a selection; the file is SVG that any browser opens; PNG export, and the PNG on the pasteboard. It draws in pixels (M3a): ink anti-aliased at the display's resolution, with round caps and joins, under the editor's own text for labels; a host that shows no pixels (Linux, until its ring) gets cells instead.

**Opening.** From a buffer that isn't a board, `\w` goes to the board used last, or with none open asks `Board: `; from a board it asks, so `\w` twice opens another. `M-x whiteboard-new` always asks. The prompt descends directories as `\f` does, starting in the buffer's directory, lists `.svg` files (the one you answered last first) and directories, and `<M-CR>` takes a new name; a name without an extension gets `.svg`. `:whiteboard FILE` and `M-x whiteboard FILE` open directly, relative to the buffer's directory. A missing or empty file is a new board; an SVG that isn't a whiteboard's is refused, `not a whiteboard file; open it in a browser`. The buffer is `*whiteboard: NAME.svg*`, in this window, a file's width (80 columns centred in your config) with `\z` widening it as it widens a terminal; its directory is the file's. Two windows on one board show the same view.

**The status line** says `arch.svg  pen·2  medium  12 elements`: the tool and ink, the width, the count, ` *` while unsaved, and why a save or export failed, until the next succeeds. `<N>` and `<I>` at the right as a terminal has them.

**Normal state** is the tool state, and the pen is its default tool. The keys, all in the board's own keymap, so `\hk` describes them:

| Key | What it does |
|---|---|
| `p` | The pen, which draws freehand with the current ink and width. |
| `r` `c` `a` `t` | One shot: the next drag draws a rectangle, an ellipse or an arrow, the next click places a label; then the pen again. |
| `l` | With nothing selected, one shot: the next drag draws a line. With a selection, nudge it a cell right. |
| `h` | With nothing selected, the highlighter on or off (the pen at 16 px and 35 %). With a selection, nudge it a cell left. |
| `e` | The eraser on or off: a drag erases every element it crosses. |
| `v` | Select: a click selects the element under the pointer, a drag a marquee (what it holds whole), a drag on the selection moves it. `j` `k` nudge a cell down and up (`h` `l` too), `x` and `Del` delete, `y` duplicates a cell down and right and selects the copies. |
| `Esc` | Drop the selection and whatever the pointer was doing, back to the pen. |
| `1`..`6` | The ink, for what's drawn next and for the selection: the default foreground, then the theme's red, yellow, green, blue and magenta (faces `whiteboard-ink-1`..`6`, which a theme restyles). |
| `[` `]` | The width, thin, medium or thick (2, 4, 8 px), for what's drawn next and for the selection. |
| `u` `C-r` | Undo and redo, one gesture, command or label at a time; `Cmd-Z` and `Cmd-Shift-Z` too, while a label is typed as well. |
| `+` `-` `0` | Zoom in and out about the area's centre, a quarter at a time between 0.25 and 4, and back to 1:1 with no pan; Cmd with the wheel zooms about the pointer. A board drawn in cells is at 1:1, and `+` and `-` say so. |
| `E` `C` `S` | Export (asks `png` or `svg`), copy the board to the pasteboard as a PNG, save now. |
| `:`, `C-w …`, `\z` | die-vim's command line and window keys, and the width toggle, bound here since a board has no text for die-vim's keys to come through. Your leader's other keys work as anywhere. |

**The pointer.** A left drag draws with the tool, and the stroke shows as it goes; a right drag erases whatever the tool. Shift while dragging a rectangle or ellipse makes a square or circle, and holds a line or arrow to an axis. A line or arrow drawn against a rectangle or ellipse, within 12 px of it, binds to it and lands on its outline, and follows when the shape moves. The press that gives the window the focus only focuses it and draws nothing, so clicking into a board leaves no dot. The wheel pans. A stroke leaving the window stops at its edge. A double click on a label, with `t` or in select, edits it.

**Labels** are typed in insert state: `t` then a click places one, `S-RET` starts a new line, `BS` takes a character back, `Esc` or `RET` ends it, every time, keeping it if it has text and dropping it if it hasn't, with the pen back. On screen they're cells in the editor's font, over the ink, so they don't grow with the zoom (their corner follows it); a PNG export draws them with DIE's own IoskeleyMonoTerm.

**Files.** The board is written a second after the last change (one timer, only while something's unsaved), when its buffer is killed, and when DIE restarts or quits; `S` writes now. There's no `:w` to remember and `:q` never refuses. The write goes to a temporary file renamed over the old one. The file is plain SVG 1.1 in the whiteboard's own dialect: `<path>`s with round caps and joins, `<rect>`, `<ellipse>`, `<line>` with a `<marker>` arrowhead, `<text>` in a monospace family, a `<style>` naming the inks, and `data-wb-*` attributes (the version, each element's id, ink and width, a shape's corners as given, a label's text as typed, a line's bindings) so a board reads back exactly. `M-x whiteboard-open` saves it and opens it in your browser (`open`, or `xdg-open` on Linux).

**Export.** `E` (or `M-x whiteboard-export png`) writes `NAME.png` beside the file, rasterized from the model rather than the screen, at the option `png_scale` (2, so a 400 px board is 800 px) on `export_background` (`theme`, the default, `white` or `transparent`); `svg` writes `NAME.export.svg`, the picture without the dialect's attributes. `C` puts the PNG on the pasteboard, through `osascript` on macOS and `wl-copy` or `xclip` on Linux, whichever is on the PATH. Each runs in a job and says what it wrote.

**On screen.** The board is one pixel buffer the size of the window's area, drawn by tiny-skia on the main thread and placed under the glyphs, so the theme's background and the labels' cells show through and over it: strokes with round caps and joins at 2, 4 or 8 logical pixels, the highlighter at 16 and 35 %, shapes and arrowheads filled and anti-aliased, the selection a dashed outline 3 px outside each element, the marquee a dashed box. A pointer sample draws only the pen's new segment and presents that rectangle; a shape or the highlighter is drawn again whole as it grows; a change to the board draws its region again; undo, zoom, a resize, a theme change or the selection draw it all; the wheel moves the picture by whole pixels and draws the strips it uncovers. On a board of 1,000 strokes a sample costs about 0.1 ms and a full redraw, drawn in bands across the cores, about 25 ms in a release build (the numbers are in docs/m0-log.md, call 1025). **In cells** (a host without pixels, or the option `pixels: false`) rectangles, lines and arrows snap to cells and draw in the box glyphs DIE joins across cells, with `▶ ◀ ▲ ▼` for arrowheads; ellipses and strokes are braille, two by four dots a cell in the ink's colour, the newest element winning a cell; the highlighter is faint; the selection is reverse video; labels are text.

**After `:rebuild`** each board comes back from its file, which was written at the hand-over, with its view, tool, ink, width and selection; the one used last in this window, the rest open in the background. Unsaved changes at a crash are at most a second old.

**Options**, as your config sets them (it sets none): `font_dir` (where the fonts are, for the exporter; DIE's own rule by default), `png_scale` (2), `export_background` (`theme`), `full_width` (a new board the window's whole width rather than a file's), `autosave` (a second), `pixels` (true; false draws every board in cells). Nothing of the whiteboard's runs until you open a board, and nothing while every board is saved.

## What isn't there yet

- **whiteboard:** pixels on Linux (cells until the wgpu ring). Labels don't zoom (they're cells; their outline does). Two windows on one board share one buffer, sized by the window resized last, so the other shows its top-left part. Pressure and tilt (winit has no tablet events on macOS), pinch, a crosshair cursor, images. A click into DIE from another app draws, since macOS activates the app before the click arrives; `u` takes the dot back. The vim-like grammar over shapes (counts, operators, cursor-placed shapes) is later and experimental. A board's own window position across a restart (M1's surface restorer). Opening a `.svg` through `\f` gives its text, as the web-content rule has it.
- **orchid:** in the session browser, insert and visual state have die-vim's keys, where your Emacs's browser keymap worked in insert state too. In a chat, Emacs's hover hints ("Press TAB to toggle"), and `M-p` and `M-n`, which did nothing there. DIE's session file keeps a chat's whole text, as it keeps any buffer's that has no file, though a restart rebuilds the chat from `events.jsonl`: about 250 KB for 250 events, written about every 5 s while a turn runs.
- **Terminal:** surviving `:rebuild` and `:restart` (D20's holder and helper come next); editing the shell's line from normal state, as evil-ghostel's `d`, `c`, `x` and `u` do; Cmd-C, and selecting with the pointer in insert state; images (kitty graphics); shell integration's prompt navigation, and bash's directory tracking (zsh's is there); links (OSC 8); the bell; and terminals on Linux.
- **Running commands:** `M-&` and a command ending in `&` (Emacs's async shell commands), `C-u M-!` (output at the cursor), completion in the command prompts, stopping a `:!` that went on in the background before DIE quits, asking before quitting stops a command, and in `*compilation*` `go` (show an error without going there), `[[` `]]` (by file) and `q`. `:w !cmd`. Backspaces in output aren't moved over, as comint does.
- **Normal keys:** `gq gw`, `( )` and `is as`, `it at`, `gn gN`, `g?`, `gm`, `gp gP`, `Q`, `C-]`, `C-w f`, `[] ][`, `z. z<CR> z- z+ z^`, folds, `gD`, `C-l`, `C-x`, `]p [p`, `<Insert>`.
- **Ex:** `:p`, `:w >> file`, `:w !cmd`, a range before `:w` or `:up`, a `/pattern/` as an address, `:marks :jumps :pwd :cd`, `:cc :cfirst`, Tab completion in `:e`, `:s` flags other than `g` and `&`. `:pu =` and `C-r =` aren't built anywhere. A range given backward, as `:3,1d`, is refused, where your Evil asks whether to swap it.
- **Insert keys:** `C-h` has only `k c x f`; its other keys say they aren't built.
- **Syntax highlighting.** Source is one colour.
- **Not honoured:** `.gitignore` in the finders and `:grep`.
- **`die --wait`:** a client that has gone, as one stopped with Ctrl-C, isn't noticed, so its file still waits for you to finish it. Quitting DIE with clients waiting doesn't ask first, as Emacs does.
- **Opening from Finder** into a minimized DIE leaves its window in the Dock, by reading AppKit: nothing restores it. A file from Finder, the Dock or `open -a` that reaches a DIE about to go is lost, and you open it again: the old DIE in the second of a restart's handover, or one launched while another holds the lock, as `open -n` does, a second copy of DIE.app, or a double-click just after Cmd-Q.
- **Editing CRLF and non-UTF-8 files.** They open read-only.
- **The Buffer List's** `.` after its keys (your Evil repeats them, as it repeats any key that isn't Evil's), `gV` (View mode), `t` (tags tables, so `t` is Evil's), `M-s a C-s`, `M-s a C-M-s` and `M-s a C-o` (searching the marked buffers), `M-DEL`, a middle click, and the column keys `gl gh { }`.
- **Keeping across a restart:** `*grep*`, `*compilation*`, `*Shell Command Output*`, `*rebuild*`, `*Buffer List*`, commands still running, the search highlight, `g;`'s list, a register over 1 MiB, and a `q` still recording.
- **Interrupting** a slow command on a huge file. `C-g` stops only macros and `:norm`, so a `:g` whose command isn't `:norm` runs to its end: `:g/./s/a/b/` on 40,000 lines takes about 2 s.
- **Mouse:** double-click and drag autoscroll.
- **Linux:** this page is macOS. The Linux build is checked, not run as an app.
