> **Decision record for spike S5 (scope D4, §10, §12).** Written 2026-10-19. Status: **accepted 2026-10-19** (see below). Evidence paths under `/private/tmp` are scratch files and may disappear; everything that matters is in `spikes/s5-evil-oracle/`.

# S5 decision record: the Evil oracle

Date: 2026-10-19. Evil 1.15.0 as vendored in `~/.emacs.d/lisp/evil`, named in every fixture by a SHA-256 over its `evil*.el` files (`c0253cf6…`). Emacs 30.2 (Homebrew, dev-only). Vim 9.1 (`/usr/bin/vim`, for comparison only).

> **Status: accepted 2026-10-19.** The proposed edits are folded into scope.md. Both questions in §5 were decided as recommended, and you can veto either: die-vim follows Vim for the six Evil bugs and artifacts, and undo is Vim's `u`/`C-r`.

## 1. Verdict

**Pass.** Your own Evil, run by Emacs, turns 306 cases into JSON fixtures in about 1.2 s. The fixtures are byte-identical from run to run, and a Rust test reads and checks every one.

- **Batch Emacs covers most of §10.** 262 cases run in `emacs -Q --batch`.
- **Batch Emacs can't model window geometry or wrapped lines.** In batch mode `line-move` always moves by logical line, so `gj` acts like `j`. Redisplay never runs, and Evil's `C-d` reads the window end that redisplay keeps, so scrolling is wrong too.
- **A tty Emacs can, so geometry needs no hand-written fixtures.** The batch run starts a child `emacs -Q -nw` on a pty that Emacs creates itself. The child redisplays after every command, as the GUI does between keystrokes. 44 cases (`C-d C-f C-b C-e C-y`, `zz zt zb`, `H M L`, `gj gk`) come from it. Run there too, the 262 batch cases give identical results.
- **The oracle found 21 places where your Evil differs from textbook Vim** (§4), across 61 cases. Most should stay as they are, since D4 says Evil wins. Six are Evil bugs, gaps or artifacts of Emacs buffers, and I propose die-vim fixes those (§5, question 1).
- **Your Evil has no redo.** `evil-undo-system` is unset, so `C-r` signals an error, and `u` after any other command undoes the previous undo. §10 promises redo, so this needs your call (§5, question 2).

### Proposed edits to scope.md

None are made yet. Each replaces the quoted text.

**§10, operators bullet.** "Counts; operators `d c y > < g~ gu gU` (shifts of 4 columns, Evil's default); dot-repeat" becomes:

> Counts; operators `d c y > < g~ gu gU` (shifts of 4 columns, rounded to a multiple of 4, as in Evil); dot-repeat. `J` puts one space after a period.

**§10, motions bullet.** Append:

> `gg`, `G`, `:N`, `>>` and `<<` keep the cursor's column, as in Evil; Vim's `startofline` would move it to the first non-blank. `{` and `}` also stop at lines that hold only spaces. The cursor never rests on the empty line after a file's final newline. Evil's does; die-vim deliberately doesn't.

**§10, scrolling bullet.** Append:

> `C-u` is the prefix argument, so `C-u j` moves 4 lines. Window positions follow Evil: `M` takes the lower of two middle lines, a count given to `C-d` doesn't stick, and `G` recenters the window. `3H` goes to the third line; Evil's goes to the fourth, and die-vim deliberately fixes that.

**§10, insert bullet.** "Insert: autoindent, and one undo step per insert session" becomes:

> Insert: autoindent, and one undo step per insert session (arrow keys don't split it). `o`, `O`, `cc`, `S` and Enter indent like the line above, which is what Evil does in text buffers. Indentation that depends on the language (Lisp, Python) belongs to a mode, not to die-vim.

**§10, registers bullet.** Append:

> `1`–`9` hold the last nine yanks and deletes, newest first (Evil's kill ring), and `-` the last small delete. After `"Ay`, the unnamed register holds only the appended text.

**§10, search bullet.** "Search with `/ ? n N * #`: incremental, highlights every match, smartcase" becomes:

> Search with `/ ? n N * #`: incremental, highlights every match, smartcase, including for `*` and `#`. A count before `/`, and an empty `/` that repeats the last search, work as in Vim. Evil ignores both; die-vim deliberately differs.

**§10, macros bullet.** "Macros (`q`, `@`); undo and redo; replace mode (`R`)" becomes, if you choose Vim-style undo in §5:

> Macros (`q`, `@`). `.` inside a macro repeats the macro's own last change; Evil's signals an error, and die-vim deliberately differs. Undo and redo: `u` and `C-r`, a linear history as in Vim. Your Evil has no redo; this is a deliberate change. Replace mode (`R`).

**§10, ex bullet.** Append:

> `:s` keeps the case of each match, as Emacs does: `:s/foo/bar/` turns `Foo` into `Bar`.

**D4.** Append to the decision:

> Exceptions: Evil bugs and artifacts of Emacs buffers. Each is listed in [records/s5-evil-oracle.md](records/s5-evil-oracle.md) §4 and gets a hand-written fixture.

**§12, S5 row.** Replace the question with:

> Done ([records/s5-evil-oracle.md](records/s5-evil-oracle.md)). 306 cases in about 1.2 s. Window geometry and wrapped lines run in a tty Emacs the oracle starts on its own pty.

**§12, Testing bullet.** "Vim keys that depend on window geometry get hand-written fixtures." becomes:

> Keys that depend on window geometry or wrapping come from a tty Emacs the oracle drives. Hand-written fixtures are only for the deliberate deviations from Evil, each with a reason.

**§12, M0 exit test (d).** "At least 300 Evil-oracle cases pass, with any deviations listed by name." becomes:

> Every Evil-oracle case passes (306 today), except the deliberate deviations, which are listed by name and have hand-written fixtures.

**§12, Crates.** The `tools/evil-oracle` line becomes:

> `tools/evil-oracle          Evil in batch and tty Emacs (dev-only); fixtures are committed, so cargo test needs no Emacs`

## 2. Pass criteria

| Check | Target | Measured |
|---|---|---|
| 50+ cases run | under 10 s total wall time | **306 cases in 1.11–1.16 s** (three runs of `./run-oracle.sh`). Batch: 262 cases in 0.53 s. Tty: 44 cases in 0.25 s. The rest is Emacs startup. |
| JSON consumed by the Rust test | all fixtures parse and validate | **Pass.** `cargo test`: 8 tests, 13 files, 306 cases. A tampering test checks that the validator catches bad cursors, tokens, states, clipboards and missing registers. |
| `~/.emacs.d` untouched | no new or modified files | **Pass.** A listing of all 4,662 entries with mtime and size, taken before any work and again at the end, is identical. |
| Deterministic | two runs give byte-identical JSON | **Pass.** Three runs and the committed fixtures are byte-identical. |

Further checks:

- **Cases don't leak state.** Running every case in reverse order gives byte-identical fixtures.
- **Batch mode doesn't distort results.** With `EVIL_ORACLE_RUN=tty`, all 306 cases run in the tty Emacs with full redisplay, and every case matches the fixtures.
- **Nothing is compiled.** `run-oracle.sh` fails if Emacs writes any `.eln` or `.elc`, even into the scratch HOME. Before that guard existed, early tty runs did native-compile some of Emacs's own libraries into the scratch HOME (never into `~/.emacs.d`). The fix was to switch compilation off on the child's command line.

## 3. How it works

### Your Evil settings, mirrored

`oracle/user-evil.el` mirrors every line of `init.el` that changes how Evil edits text.

| `init.el` | Setting | Why it matters |
|---|---|---|
| 93–94 | `evil-want-integration t`, `evil-want-keybinding nil` | Which Evil files load |
| 99 | `(evil-mode 1)` | |
| 284–285 | leader `\` in normal and visual state | `\` is a prefix key |
| 307–309 | no menu, scroll or tool bar | The tty window's height |
| 330 | `indent-tabs-mode nil` | Tab and shifts insert spaces |
| 335 | `global-visual-line-mode` | Soft wrap; `j`/`k` stay logical, because Evil binds them so |
| 344 | `line-move-visual t` | `gj`/`gk` |
| 358 | `enable-recursive-minibuffers t` | |
| 363–364 | no backups, no auto-save | |
| 369 | `search-invisible t` | |

Not mirrored, because it doesn't change editing: evil-collection (bindings for other major modes only), vertico and marginalia, themes and fonts, vterm and ghostel, auto-revert, and line numbers in programming modes. Two things that affect geometry aren't mirrored: visual-fill-column, which centers text at 80 columns, and the GUI's fringes. Fixtures carry their own window size and line layout instead.

Evil defaults you haven't changed, which the fixtures depend on: `evil-undo-system` nil (no redo), `evil-search-module` isearch, `evil-shift-width` 4, `evil-shift-round` t, `evil-start-of-line` nil, `evil-want-C-u-scroll` nil, `evil-want-fine-undo` nil, `evil-respect-visual-line-mode` nil, and `evil-kill-on-visual-paste` t.

Batch mode differs from your GUI in a few ways the oracle corrects. `transient-mark-mode` is switched on, as in the GUI. The clipboard is simulated (below). In the tty run the echo area is kept at one line, so a long message can't shrink the window mid-case.

### Running the keys

- **Each case runs alone.** It gets a fresh buffer with undo on, and the global Evil, kill-ring, register and search state is reset before it. That's what the reverse-order check proves.
- **Keys run as one keyboard macro.** Three pieces of advice make it behave like typing:
  - An error in a command is recorded, and the keys after it still run, as they would if you typed them. Inside `@a`, an error still ends the macro.
  - `q` can record. Emacs never records keys that come from a macro, so the oracle tracks which keys were typed while recording and hands them to `end-kbd-macro`, as the keyboard would.
  - A bell inside a macro ends it, as in the GUI.
- **The clipboard is simulated.** In your GUI, `kill-new` pushes every yank and delete to the macOS clipboard. The oracle records each of those writes instead of touching the real clipboard, and it can start a case with text that another app copied.
- **Safety.** `run-oracle.sh` runs Emacs with `-Q` and HOME set to a scratch directory, and loads Evil read-only. Afterwards it fails if anything under `~/.emacs.d` is newer than the run, or if anything was compiled.

### Case format

One JSON file per topic in `cases/`. I chose JSON because Emacs 30 parses it natively and serde reads it, so neither side needs a parser of its own. YAML would need an Emacs package. Text is an array of lines, so cases don't need `\n` escapes.

```json
{"topic": "undo",
 "cases": [
  {"id": "u-after-move-redoes", "keys": "xxulu", "text": ["|abcd"]},
  {"id": "p-from-preset", "keys": "\"ap", "text": ["|x"],
   "registers": {"a": {"text": "one\n", "type": "line"}}}
 ]}
```

| Field | Meaning |
|---|---|
| `id`, `keys` | Required. Keys are in Vim notation: `<Esc> <CR> <BS> <Tab> <C-x> <lt>` and the arrow keys. |
| `text` | An array of lines, each ending in a newline, or a string for exact text. `\|` marks the cursor; change the marker with `cursor_marker`. |
| `visual` | `char`, `line` or `block`. `<` and `>` in the text mark the selection, and the cursor must be at one end. |
| `registers` | Initial registers by name (`a`–`z`, `0`, `"`). Types are `char`, `line` or `macro` (keys in Vim notation). |
| `clipboard` | Text another app copied before the keys. |
| `mode` | The Emacs major mode. The default is `fundamental-mode`. |
| `window` | `width`, `height`, `top_line`. Only for tty topics. |
| `description`, `vim` | Free text. `vim` says how textbook Vim differs. |

File-level `"run": "tty"` sends the topic to the tty Emacs, and `"defaults"` fills fields every case shares.

### Fixture schema

One file per topic in `fixtures/`: a header, then one case per line. `src/lib.rs` has the serde types and the validator.

- **Header:** `schema` (1), `topic`, `generator`, `run` (`batch` or `tty`), `evil_version`, `evil_source_sha256`, `emacs_version`.
- **Case:** `id`, `source` (`oracle`, or `hand-written` with a `reason`), `keys`, `key_tokens` (the keys split one per key), `mode`, `input`, `expected`, `vim`.
- **Positions:** `{line, col, byte}`, all 0-based. `col` counts characters, and `byte` is the UTF-8 offset into the text.
- **`input`:** `text`, `cursor`, plus `visual`, `registers`, `clipboard`, `window` and `layout` when the case uses them. `layout` is the byte offset where each screen line starts, so die-vim's tests can use Emacs's wrapping rather than recompute it.
- **`expected`:**
  - `text`, `cursor`
  - `state`: `normal`, `insert`, `visual`, `replace` or `operator`
  - `visual` in visual state: `type`, `start` and an exclusive `end`
  - `registers`: always the unnamed `"`, plus every register the keys changed. Each is `{text, type}`, where type is `char`, `line`, `block` or `macro`, or null when empty.
  - `clipboard`: `text`, and `writes`, every string `kill-new` sent, in order
  - `window` for tty cases
  - `errors`: each has `symbol`, `message` and `after_keys`

The Rust test checks that every position matches its text, that `key_tokens` equals die-vim-style parsing of `keys`, that there's a selection exactly in visual state, that linewise registers end in a newline, that the clipboard's text is its last write, and that ids are unique.

### Adding a case and rerunning

```sh
cd /Users/user/workspace/die/spikes/s5-evil-oracle
# 1. Add a line to cases/<topic>.json, for example:
#      {"id": "dw-last-word", "keys": "dw", "text": ["foo |bar"]},
# 2. Regenerate every fixture (about 1 s):
./run-oracle.sh
# 3. Read the result, then check the schema:
jq '.cases[] | select(.id == "dw-last-word") | .expected' fixtures/<topic>.json
cargo test
# Optional: compare with Vim (writes vim/report.jsonl).
./vim/compare.sh
# Optional: run every case in the tty Emacs, or in reverse order.
EVIL_ORACLE_RUN=tty ./run-oracle.sh --out /path/to/dir
./run-oracle.sh --reverse --out /path/to/dir
```

## 4. Where your Evil differs from textbook Vim

`vim/compare.sh` replays every fixture in Vim 9.1. Vim runs with `-u NONE` plus only the options that match your Evil: shiftwidth 4, expandtab, autoindent, ignorecase, smartcase and `backspace=indent,eol,start`. Of 306 cases, 291 were compared and 60 differ. The other 15 aren't comparable: 12 involve wrapping, where Vim wraps one column later than a tty Emacs, and 3 involve the clipboard. One wrapped case is judged by hand. Each differing case carries a `vim` note in its fixture.

The last column is my proposal for die-vim. "Evil" means keep Evil's behaviour, as D4 says. "Vim" means a deliberate deviation.

| # | Area | Your Evil | Textbook Vim | Cases | die-vim |
|---|---|---|---|---|---|
| 1 | End of file | The cursor can rest on the empty line after the final newline: `G`, `5j` past the end, `w` on the last word, `}`, `dd`/`dk`/`dG`/`dj` on the last lines, `L` and `M` in a short buffer, `gj` on the last line, and `C-d` and `H` after `G` follow from it. `J` on the last line takes the final newline, and a count past the last line reaches that empty line | The last line is the last line | 24 | **Evil without the final newline** (an Emacs buffer artifact; see below) |
| 2 | Column after jumps | `gg`, `G`, `:N`, `>>`, `<<`, `dk` keep the column | `startofline` moves to the first non-blank | 12 | Evil |
| 3 | Shifts | Round to a multiple of 4: 6 spaces `<<` gives 4, 2 spaces `>>` gives 4 | 6 → 2, 2 → 6 | 2 | Evil |
| 4 | Indentation | `cc`, `S`, `O` indent by the major mode: in text, like the line above; Lisp and Python modes indent by language | Copy the current line's indent | 6 | Evil (line above); languages later, per mode |
| 5 | Redo | None: `C-r` signals an error | `C-r` redoes | 1 | **Your call** (§5) |
| 6 | Undo chain | `u`, another command, `u` redoes the first undo. A run of `~`, `J`, `r` or `g~` is one step, because Emacs's `delete-char` joins them. After `J` from an empty line, `u` leaves the cursor on the next line | Undoes further; each command is a step; `u` goes back where the step started | 4 | **Your call** (§5) |
| 7 | Undo in insert | Arrow keys don't split the insert's undo step | Arrow keys split it | 1 | Evil |
| 8 | Numbered registers | `"1`–`"9` are the kill ring: `"1` is the last yank or delete of any size | `"1` is the last delete of a line or more | 2 | Evil |
| 9 | Appending registers | After `"Ay`, `"` holds only the new text | `"` holds all of `"a` | 2 | Evil |
| 10 | `*` and `#` | Use smartcase | Ignore smartcase | 1 | Evil |
| 11 | Search count | `2/x` and `2?x` ignore the count | Go to the 2nd match | 1 | **Vim** (missing in Evil) |
| 12 | Empty search | `/<CR>` does nothing | Repeats the last search, and with none is an error | 1 | **Vim** (missing in Evil) |
| 13 | `:s` case | Keeps the case of the match: `Foo` → `X` for `:s/foo/x/` | Inserts `x` | 1 | Evil |
| 14 | Paragraphs | `}` stops at lines holding only spaces | Only at empty lines | 1 | Evil |
| 15 | `J` | One space after a period | Two (`joinspaces` is on by default) | 1 | Evil |
| 16 | Cursor details | `yip` leaves the cursor where it was; block `A` leaves it after the first append | Moves to the start | 2 | Evil |
| 17 | `.` in a macro | Repeats the last change made before the macro ran, and errors when there is none (Evil runs macros without command hooks). After `@a`, `.` replays the macro, but errors after an operator | Repeats the macro's own last change, in the macro and after it | 3 | **Vim** in the macro (Evil bug); after `@a`, **Evil's** replay without its error |
| 18 | `Vp`, `VP` and `Vjp` over the last lines | Leave an empty line and drop the final newline | Replace the lines | 3 | **Vim** (Evil bug, from #1) |
| 19 | Scrolling | `C-u` is the prefix argument; a count on `C-d` doesn't stick; after `C-b`, and near the end after `C-d`, the cursor lands a line or two off Vim's | `C-u` scrolls; a `C-d` count sticks | 6 | Evil |
| 20 | Window lines | `M` takes the lower middle line, and uses the window's middle even in a short buffer; `G` recenters, so `H` lands lower; `nH` goes to line n+1 | Upper middle, text's middle, last line at the bottom, line n | 5 | Evil, except **`nH` → Vim** (bug) |
| 21 | `C-a` (outside §10) | Not bound | Increments a number | 1 | Out of scope |
| 22 | Combining marks | `e` and `$` onto a letter with a combining mark leave the cursor after the mark, on the space or the newline (Emacs's command loop, not Evil's grammar). `x`, `X`, `s`, `r`, `~` and an operator over `h` or `l` split a letter from its mark | Stay on the letter, and take the letter and its mark together | 12 | **Vim** (an Emacs artifact) |
| 23 | A key after an operator | Any other command runs as if it were a motion: `dA` deletes to the end of the line. Some (`x`, `y`, `p`, Esc) quit | Fails, and does nothing | 1 | **Vim** (Evil bug) |
| 24 | A string that never closes | `di"` deletes from the quote, or the character before it, to the end of the text less one character, or fails, depending on what's earlier in the text | Fails | 2 | **Vim** (Evil bug) |
| 25 | A register before a failed motion | `"a` before an `h`, `l` or `f` that fails stays named for the next command | The failed command uses it up | 1 | **Vim** (Evil bug) |
| 26 | `R`'s `<BS>` over a newline it typed | Deletes it and inserts the next character a second time: `Rx<CR><BS><BS>` on `abc` gives `abbc` | Puts back what the key took: `abc` | 1 | **Vim** (Evil bug) |
| 27 | `u` typed after a macro's `u` | Emacs's undo counts the first as an edit, so `xx`, `@a` holding `u`, then `u` on `abcd` gives `cd` | Each `u` goes back one step: `abcd` | 1 | **DIE** (Emacs artifact) |
| 28 | A block's `I` replayed by a macro | Reaches only the first line, since macros run without hooks: `XXab`, `Xcd` | Every line: `XXab`, `XXcd` | 1 | **Vim** (Evil bug) |
| 29 | `gv` after undoing what took its text | Emacs's undo moves markers back, so `vjdugv` ends on the old head | Markers stay where the delete put them, so it ends at the start | 1 | **DIE** (Emacs artifact) |
| 30 | A second `i(` on a one-character selection | On `((a))` it fails with `(` selected; from an opening bracket it takes the outer pair | Finds what the first did | 2 | **DIE** (Evil bug) |

Rows 1–4 and 8 affect everyday editing most. Row 1 is also a question for core, not only die-vim: should a file that ends in a newline show an empty last line?

**Updated 2026-11-05, after the deferred reviews.** Rows 1, 11, 12, 17, 18 and 20 are reworded, and row 22 is new. The deviations review compared errors too, which this section's first run didn't: `compare.vim` now counts Vim's beeps against Evil's errors. Of the 523 cases at this update, 504 were compared and 166 differ, and each of those carries a `vim` note. Row 1's choice now reads "Evil without the final newline": each of its fixtures takes its cursor and errors from your Evil on the same text without its final newline, and its text and registers from the run with the newline. The exact rules are in `crates/die-vim/tests/oracle/deviations.json`. Three things that differ from Vim stay Evil's, and aren't rows here: `N%` goes N percent through the characters, `e` stops on empty lines, and Esc or a failed motion resets the goal column. Only `h`, `l`, `j`, `k`, and `f` or `t` from a line's last character keep it when they fail.

**Updated again with V3.** Row 6 takes in the undo differences operators showed. Rows 23–25 are new. Rows 1 and 22 take in more cases. V3 added 550 cases, for what you'll meet in Rust: `snake_case`, `::`, `->`, tabs, wide characters, operators over several lines, counts on both sides. Of the 1,075 cases now, 1,055 were compared and 380 differ, and each of those carries a `vim` note. A random differential test against your Evil found the rest of these rows. It also found a few rare differences that aren't rows, because they're corners of Emacs and Evil, not choices. The M0 log lists them.

## 5. Surprises, risks and decisions for you

### Two questions for you

1. **Evil bugs and Emacs artifacts.** D4 says Evil wins. Six behaviours are Evil bugs, gaps, or artifacts of how Emacs buffers end, not choices anyone made: rows 1, 11, 12, 17, 18 and the `3H` part of row 20. I recommend die-vim follows Vim for exactly these six, each as a named, hand-written deviation, and Evil everywhere else. Is that the right rule?
2. **Undo and redo.** Your Evil has no redo. Its `u` redoes when another command comes between two `u`s, which is Emacs's undo chain. Options:
   - **A. Vim's model:** `u` steps back, `C-r` steps forward. This is what §10 says today.
   - **B. Your Evil's model,** exactly.

   I recommend A. It's what §10 promises, and it's easier to predict. If you pick A, setting `(setq evil-undo-system 'undo-redo)` in Emacs would make your Emacs match DIE while you use both.

### Specifics I decided (veto any)

- **Indentation.** die-vim indents new lines like the line above (Evil in text buffers). Language-aware indentation is a mode hook, later.
- **Kept Evil's way:** columns after jumps (no `startofline`), shift rounding, numbered registers as the kill ring, smartcase on `*`/`#`, case-keeping `:s`, whitespace-only lines as paragraph breaks, one space on `J`, and Evil's window and scroll positions.
- **Regex syntax.** One case uses Emacs regex groups, `:s/\(foo\) \(bar\)/\2 \1/`. With D4's Rust regex, die-vim's test must use `:s/(foo) (bar)/$2 $1/` instead. The case says so in its description, and future regex cases need the same note. Replacements are Rust's too (`$N`, `${N}`, `$name`, `$$`), so `$1x` names a group called `1x`, and a group followed by a letter needs `${1}x`.
- **Fixtures are committed.** `cargo test` reads them with no Emacs installed. Emacs and Evil are dev-only tools for regenerating them, as D16 allows.

### Surprises

- **Batch Emacs can't do geometry.** This took the most work. Three separate obstacles: `line-move` is always logical when Emacs is non-interactive, Evil's scrolling reads `window-end`, and `redisplay` does nothing while a keyboard macro runs. The tty child solves all three. The oracle hides the running macro from `redisplay` for a moment after each command.
- **`script` can't host the tty Emacs.** It turns the end of its input into a `C-d` keystroke, which scrolled the first case. Batch Emacs now creates the pty itself and never writes to it.
- **Terminal type.** `TERM=xterm` makes Emacs query the terminal and wait 2 s for an answer, so the child uses `vt100`.
- **Narrow windows.** A 40-column window wraps long messages, which grew the echo area and shrank the case's window mid-case. The echo area is now fixed at one line.
- **Evil runs macros without command hooks,** which is why `.` inside a macro fails (row 17). That's real Evil behaviour: a register preset with `dw.` fails the same way as a recorded one.

### Risks

- **Emulated recording.** Emacs can't record `q` from inside a keyboard macro, so recording is the oracle's own code (about 20 lines). Recorded and preset macros play back the same way, but it's still an emulation.
- **Tty versus GUI.** A tty frame keeps its last column for the wrap glyph, so it wraps one column earlier than your GUI. Fixtures carry their layout, so die-vim's tests should take wrapping from `layout` rather than recompute it.
- **Evil updates.** If you update Evil in `~/.emacs.d`, `evil_source_sha256` changes. Rerun the oracle and read the fixture diff.
- **Major modes.** Almost every case runs in `fundamental-mode`. Your real buffers use other modes, whose paragraph and indentation rules can differ.
- **The baseline.** The Vim comparison uses Vim 9.1 defaults, such as `startofline` and `joinspaces`. Neovim's defaults would remove rows 2 and 15.

## 6. Raw data

- **Spike:** `/Users/user/workspace/die/spikes/s5-evil-oracle/`
  - `cases/`: 13 topic files you edit
  - `fixtures/`: generated, 306 cases
  - `oracle/evil-oracle.el`, `oracle/user-evil.el`: the oracle and the mirrored settings
  - `run-oracle.sh`: the runner, with the `~/.emacs.d` and no-compilation guards
  - `src/lib.rs`, `tests/fixtures.rs`: the serde types, the validator and `cargo test`
  - `vim/compare.vim`, `vim/compare.sh`: the Vim comparison; `vim/report.jsonl` holds its result for every case
- **Scratch evidence:** `/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s5/`
  - `final/checks.log`: timings, the determinism, reverse-order and tty comparisons, case counts, the Vim summary and `cargo test`
  - `emacsd-before.txt`, `emacsd-after.txt`: the `~/.emacs.d` listings, with mtimes and sizes
