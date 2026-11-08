# M0 log

What merged, what waits on you, what changed in die-core's API, and where the budget stands ([m0-plan.md](m0-plan.md) §3). Newest first.

## 2026-11-08: the MVP run, 17 packages on `m0/integration`

**Waiting on you:**
1. **A day in it.** Exit test (a) needs you. Keep any terminal in the next tile for cargo and git, and use `die file:line` from it to reach a file. DIE's own terminal stays in M2, as planned. `EDITOR=die` doesn't work for `git commit`, as there's no `--wait`.
2. **Whether this reaches `main`.** Nothing in this run has.
3. **The hand checks** below.
4. **Any call below.** The ones I'd read first: 444 (project search and the finder are in core), 445 (die-vim is at 8,801, against 5,000 at the guard's start), 449 (`platform::macos`'s limit and the test that holds it), 467 (`<CR>` in a finder waits on the main thread for up to 2 s), 472 (`gf` isn't your Evil's `gf`), 478 (`*grep*` doesn't survive a restart) and 530 (a whole big file yanked doesn't survive one either).

**Checkpoint, after this entry was first written:** on b85389e, `ci --linux` passed in 169 s, and exit test (f) holds. `ci --full` ran every test with the ignored ones and the old property counts, and only one failed: `the_restart_loop_finds_equal_digests_and_logs_each_time`, whose first restart took 1,021.6 ms against 1 s while the whole suite ran. Its failure poisoned `two_launches_leave_one_instance` too. Alone, it passed. Exit test (b) therefore holds with no margin under load. `mutate` caught 57 of 58 on b85389e, missing M41, a property-test catch that took 6 to 15 s when caught before; a rerun caught all 58 in 155 s, so M41 is a rare catch rather than a regression (layout.rs is unchanged). Earlier rounds of `ci --full` found four problems, all in checks or the build rather than in what you'd use: the clipboard's exception catch kept 86 lines of assembly in a `.s` file that lint and the budget don't read, now in `catch.rs` (call 560); an app test read `session.json` between a write's two renames; `a_day.rs`'s read-only test failed as root on Linux; and die-vim was one line over after the merges.

### What you can do now, on `m0/integration`

[using-die.md](using-die.md) has every key. New in this run:

- **Open files from a terminal.** `die foo.rs:12:3` opens it in the running DIE, or starts one, and `C-o` comes back.
- **Find things.** `\\` fuzzy finds from home, `\p` from the project root, with results while you type at 500,000 paths. `\/` switches buffer.
- **Search the project.** `:grep` and `\r` stream into `*grep*`, `<CR>` opens a row, `:cn` and `:cp` step. `gf`, `gF`, `]f` and Visual `gf` open `path:line:col`.
- **More Vim.** Marks, the jump list and `g;`. `ge gE g_ | [[ ]] gd gJ & g& = ga`, `+ - _`, `g0 g^ g$`, `g* g#`, `[( ]) [{ ]}`, `io ao`, `ZZ ZQ`, `C-^`. `:2,5s` with addresses, `:bn :bp :bd :up :u :$ :wa :qa :wqa :xa :x`.
- **Insert state.** `C-o`, `C-p`, `C-n`, `C-x C-n`, `C-x C-p`, `C-t`, `C-d`, `C-a`, `C-@`, `C-e`, `C-y`, `C-r`, `C-v`, `C-q`, `C-u`, `<Del>`.
- **Prompts.** Cmd-V, `C-y`, `C-r` and `C-r C-w` paste into any of them.
- **Splits in the app.** Before this run the windowed app drew only the focused window of a split.
- **Restart keeps more.** `.`, `@@`, `;`, `,`, `n`, `N`, `gv`, `&`, marks and jumps come back.
- **Huge and odd files.** A 100 MB file opens in 42 ms and most keys take under a millisecond. `*` on a giant word no longer crashes it. Directories, FIFOs and `/dev/zero` are refused.
- **Idle.** About 0.55-0.6 OS wakes a second, down from 2.4.

**Oracle, from a fresh `DIE_ORACLE_BLESS=1` run on 30ed2f0:** 2,136 of 2,136 cases pass, with 107 deviations excluded and 0 pending. That's 2,135 from your Evil (2,045 batch, 90 tty), less the 107, plus 108 hand-written fixtures. It was 1,411 in the entry below. `DIE_ORACLE_BLESS` only rewrites `pending.txt`, so I also regenerated every fixture from batch Emacs into a scratch directory: all are byte for byte what's in the repo.

**Budget, `cargo xtask budget` on 30ed2f0, the lines that moved:**

| Crate or module | Entry below | Now | Limit |
|---|---|---|---|
| die-core | 9,970 | 11,754 | 14k |
| text and undo | 1,260 | **1,416** | 1,420, was 1,265 |
| layout | 914 | 947 | 950 |
| keys, keymaps, describe | 646 | 673 | 675, was 650 |
| ext, dispatch, rt | 1,411 | 1,521 | 1,525, was 1,500 |
| workspace, windows, pointer | 1,317 | 1,361 | 1,365, was 1,320 |
| files | 634 | **833** | 833, was 650 |
| command line, prompt, core commands | 1,135 | 1,288 | 1,300 |
| project (new) | none | 747 | 750, was 600 |
| faces and scene | 936 | 939 | 950 |
| session | 1,086 | **1,384** | 1,385, was 1,086 |
| harness, app interface, messages, ids, errors | 631 | 698 | 800 |
| die-render | 4,128 | 4,312 | 7k |
| macOS build | 3,971 | 4,147 | 5k |
| Linux build | 3,088 | 3,135 | 5k |
| neutral | 2,232 | 2,255 | 2,300 |
| checks | 699 | 715 | 720, was 700 |
| `platform::macos` | 1,040 | 1,266 | 1,270, was 1,060 |
| platform stub | 157 | **165** | 165, was 160 |
| die-vim | 5,789 | **8,876** | 8,876, was 5,790 |
| die | 358 | 493 | none |
| die-dev | 181 | 189 | none |
| product code | 20,426 | 25,549 | none |
| xtask (dev) | 4,517 | 4,554 | none |
| evil-oracle (dev) | 588 | 589 | none |

Most core modules have 4 lines or fewer left, and files none, so most next changes need a cut or a call. Calls 441 to 450 list each raise.

### What only you can check

These stand with the entry below's list, which no one has worked through.

- `die foo.rs` from another app, also with DIE minimized or hidden: does it come forward? `come_to_front` has never run, since every test stays in the background. If it doesn't, the U1 review's guess is that the client should also run `open` on the bundle.
- `cargo xtask app` for your real `~/Applications` and `~/.local/bin` link. V1 ran it only under `--home`.
- Launch from the Dock and type, real keys and input methods (V1 posted events to a background instance), restart while in front, and AeroSpace tiling and a live resize with splits.
- Press `p` after copying in another app: the pasteboard alert should appear only then. Copy a file in Finder and an image in a browser, then `p`: the register's text should paste. Yank and paste both ways on the real pasteboard (V6).
- The leader keys, `:grep`'s streaming and a real mouse in a drawn window. None of it has been tried in one.
- Colours on a P3 display: the Metal layer sets no colorspace, so reds and greens may look stronger than in Emacs (V4, from reading the code).
- `gd` in your GUI Emacs, where xref might ask for a TAGS file rather than fall through to the search the oracle saw (U4). `ga` on non-ASCII in graphical Emacs (U5).

### What wasn't verified

- **Nobody has typed into this.** V1 posted real key events to a release bundle before later packages landed. Everything since ran through the harness or through offscreen frames. U2, U3, U4, U5, U6, U7, U8, V2, V3 and W2 ran no window test.
- **No gate covers the tip.** A package that had a push refused re-merged and pushed on `cargo check`, as the rule allows, so no test has run on the merged whole. U1, U6, V2 and W3 say so for their own, and V6 re-ran only the budget and `tests/platform.rs`. W1's 6 window tests passed before two later merges. V5's `idle-60` and wake figures weren't re-run on its tip. W3 changed `crates/die/src/app.rs` and didn't run `xtask --test app`. V4 changed die-render and ran no window test.
- **Not run:** `ci --full`, `ci --linux`, `mutate`, `vim/compare.sh`, anything on Linux, and `restart-loop` with marks present.
- **The window rule, broken once and possibly twice.** U1's window tests ran up to three times. V6 ran `crates/die-render/tests/keys.rs` repeatedly and didn't check whether it put windows on the screen.
- **A flake, cause unknown.** `the_restart_loop_finds_equal_digests_and_logs_each_time` failed once in V1's full run, with `two_launches_leave_one_instance` failing on the poisoned mutex. It passed alone and on rerun.
- **Compared with Evil by reading, not running:** `n` after `gd`, because batch Emacs loses keys after `gd` and why isn't traced. Anything a click touches, and opening a file with `gf`, because the oracle can't click or set up files. `` `F `` for a killed file, and a jump into a killed buffer.
- **Measured once, under load:** the finder at 500,000 synthetic paths. A real `~` was never walked. No real-tree timing of `:grep` since the review's fixes. V3's before and after figures aren't from the same load.
- **The original SIGABRT in `tests/platform.rs` never reproduced** in 60 runs, so its cause isn't known. Any raise is now caught, which is all V6 can claim.

### The packages

**U1, `die file:line`.** Review: 4 minor, all fixed. `die a.txt:4` set no jump unless it crossed a buffer. A file could replace a lone terminal, a client could hang writing a long request to a stuck instance, and `restart` hung for good if `open.sock` had been removed. Probes of prompts, goal column and paths found nothing. API: die-core adds `FileTarget`, `Ctx::open_target`, `Ctx::set_jump_rule`, and `OpenRequest`, `OpenListener`, `send_open`, `Sent` and `OPEN_SOCKET` in `instance`. die-render adds `come_to_front`. die adds `app::is_client`, `app::client`, `Launch.background`. xtask adds `app::link_command`. Open: coming to the front, `cargo xtask app` for real, exit 1 from the real binary. `open -a DIE file` and Finder aren't built, by estimate over 30 lines. The restart test may not exercise the retry, which only die-core's socket test proves.

**U2, finders.** No review. A worktree's `.git` file was listed: fixed. At 500,000 synthetic paths and load about 18, a key and its frame took p50 125 us, p99 343 us on the main thread, and the new list arrived in p50 70 ms, p99 451 ms. API: die-core adds `walk`, `project_root`, `PathSource`, `Ctx::find_under`, `Ctx::prompt_settle`, and `Ctx::step` takes the finder's news. Open: at load 33 the ranker reached about 2 s, so `<CR>` can do nothing and need a second press. The frame was imitated, not a real GPU frame.

**U3, `:grep` and `gf`.** Review: 1 major, 5 minor, all fixed. 4,360 of 4,398 rows on DIE's own tree came from `.claude/worktrees` (major). Rows waited through long stretches with no match, `<CR>` ignored a window already on the file, a deleted file's row opened an empty buffer that `:w` could write back, an empty match made a row past the last line, and `]f [f ]F [F` and Visual `gf` were missing. API: die-core adds `grep`, `compile-goto-error`, `next-error`, `previous-error`, `Pattern::ignores_case`. Open: `.gitignore` isn't read, `:cc` and the rest aren't bound, and opening a file with `gf` has never been compared with Evil. `rows_show_while_the_search_goes_on_through_files_with_none` has about 700 ms of margin under load.

**U4, motions.** No review. `ge gE g_ | [[ ]] gd` with 150 oracle cases. `gd` hit a batch Emacs error, fixed in the oracle with `(require 'etags)`, and a full regeneration then matched. API: 7 commands, and `[<any>` and `]<any>` cancel an operator. Open: keys after `gd` are lost in batch Emacs. `[[ ]] gd` now set jumps through U7's list, which U4 left open.

**U5, edits.** No review. `gJ & g& = == ga`, with `:&`, `:&&`, the `&` flag and `~` in a replacement, in 31 oracle cases. `=` read a stale column and `u` after `&` went to the wrong place, both fixed before landing. `gJ` on the last line is a new deviation row. API: 5 commands and `Operator::Indent`, and `:s/x/~/` now means the last replacement. Open: `:sort :d :y :m :t` weren't built, at about 160 lines against a limit of about 60. V2 later built the numeric ranges. `&` and `=` on blocks, folds and Hide aren't checked.

**U6, insert state.** Review: 3 major, 8 minor, all fixed, and it built the builder's open keys. A macro with `C-o` ran its later text as normal commands (major). `C-p` after a click panicked and lost the session (major). `C-o` then a command that wasn't die-vim's stayed in normal state (major). Also `C-o` with `:b`, with `R`, with `gi`, and with `dd` on the last line, `C-a` with a count, and two multibyte failures in completion. API: die-core adds `Ctx::on_command_ended`. 16 commands in all, 246 oracle cases, 243 from Evil. `registers::read` now answers `:`, `/`, `%`. Open: `<Insert>`, a count on `C-x C-p`, no message when line completion finds nothing, non-ASCII symbols in a word for `C-p`. Four `C-o` probes that killed the batch run stay unprobed.

**U7, marks and the jump list.** Review: 1 major, 5 minor fixed, 1 made a deviation. A mark or jump into another buffer left the cursor off screen (major). `:s`, `&` and `g&` set no jump and `:s` no `[ ]`, a key naming no mark gave no error, a killed file's `A`-`Z` were lost, and `.` after `:%s` went to the first line changed. API: die-core adds `Place`, `PlaceList`, `Ctx::places`, `Ctx::set_places`, and `TextBuf`'s `mark`, `set_mark`, `remove_mark`, `marks`. session.json gains additive fields. `<Tab>` in motion states is now a jump. Evil-oracle adds kind `not-in-scope`. Open: a killed file's marks don't survive a restart, and past 1,024 edits `.` can be imprecise, with no test of that.

**U8, what a restart keeps.** No review. `.`, `@@`, `;`, `,`, `n`, `N`, `gv` and `&`'s replacement survive, and an old or nonsense session.json still loads. A kept pattern with no replacement would have made `&` delete: fixed, from reading. API: `Extension::save_session` and `restore_session`, `session::{Kept, Value, json}`. Open: no real restart was run, only 8 harness tests. The restart loop covers these only through the digest.

**V1, the real bundle.** No review. The release bundle was built, signed and launched with `open -g`, and took posted keys for `:w`, `/`, `C-w v`, find-file, `:q` and Cmd-Q. A rebuild then `:restart` ran the new code with everything back. Fixed: `:wa :qa :wqa :xa :x` were missing, each restart left a socket file, bundle replacement left a gap (narrowed, not closed). W1 landed the same split fix first. API: `Handover::wait` removes its socket's file, 5 families of ex commands, `cargo xtask app --home DIR`. Open: a state directory path over about 100 characters fails to bind, `die` on an unreadable file exits 0, keys typed during the handover are lost.

**V2, a day's keys.** No review. 15 realistic tasks on a copy of die-vim's source pass in `tests/a_day.rs`, with no panic. One product bug: `cargo fmt` under a clean buffer moved the cursor, and now it stays on its line when the line count holds. Missing small keys are bound, as listed above. API: `:e!` and the poll apply same-size runs line by line, `Words` takes a `WordKind`. Open: `gq gw`, sentences, `:d :m :t :g :normal` and the rest in using-die.md, which V2 thinks is the next package. rustfmt wasn't really run. `ZQ`, `:bw` and the page keys have no test.

**V3, hostile input.** No review. Fixed: out of memory on `*` on a 100 MB word. `ggVGrX` on 100 MB never finished, now 0.4 s. `gg<C-v>G$d` 300 s to 5 s, `:%s` 6.2 s to 0.66 s, `gg>G` 71 s to 1.5 s, a macro fan-out 74 s to 0.3 s, `w` in a 5 MB word 2-10 s to 0.2 s, lines of combining marks 15 s to 190 ms, and `:noh` on a highlighted 5 MB line 3.7 s to under 30 ms. Odd files, resizes and regex blow-ups held with no change. API: `Ctx::move_register`. A `Tx` of 4,096 changes or more runs in one pass and may leave `edits_since` returning None. `Pattern::new` refuses over 64 KiB. Open: on 100 MB, `di"` takes 4-6 s, `#` up to 7 s, `dap` 1.4 s, all uninterruptible. A dirty 100 MB buffer rewrites 100 MB to session.json each quiet second, and RSS reached 2.9 GB. The 100 MB one-line sweep died on the probe's own memory and wasn't rerun.

**V4, looks.** No review. 36 scenes rendered offscreen and looked at, and `apps/die-dev/tests/looks.rs` makes them again. Theme faces match mono-dark by value. One defect fixed: a status line or a prompt's row stopped up to a cell short of the right edge. API: `Frames::build` extends a row's last run. Open: no syntax highlighting. That's no difference for Rust, since your Emacs opens `.rs` in fundamental-mode with no colours, but other languages lose theirs until M1. `:b *messages*` fails until there's a message, and the colorspace is unset. A block cursor on an emoji and the status bar at the edge weren't seen in a picture.

**V5, idle.** No review. Steady wakes fell from 2.4 to 0.55-0.6 a second: the session look and file poll are one timer, resting at 5 s after 30 s of nothing. The "20 a second" was a launch transient of about 10 OS wakes for 15 s, inside AppKit, not fixed. `--check idle-60` now reads the OS's count, fails at 1 a second, and takes about 95 s. API: `driver::process_wakeups`. Open: `idle-60` was never seen failing on a real regression. An occluded window wasn't measured for wakes.

**V6, clipboard.** No review. Nothing landed in this run's pass: its work was already there as e9aa36c, merged at 7233e9c, and this pass reviewed it and ran the gate. Fixed there: an NSException on a yank aborted the app, write failures were silent, a Finder or image copy went in as text, `generalPasteboard` opened at launch. API: `Clipboard`'s `read`, `write`, `change_count` return `Result`, `FakeClipboard` gains `reads` and `set_failing`, die-render adds `catch::guarded`. Open: the hand checks above, and a real pasteboard failure wasn't tried. Local commit 1d528c5 on `mvp/v6-clipboard` is moot.

**W1, pointer review (K14).** Fixed: the windowed app drew only the focused window (major), a scroll over a prompt's list moved the window behind it (major), `o` then a click on a blank line then Esc emptied that line, a click on virtual text at a row's edge landed a line up, a narrow frame could index out of bounds. API: `Ctx::cell_at` is None over a prompt, `Host::set_grid` sets the frame, and a loose `open_window` no longer shows or takes focus. Open: no real pointer session. Evil's click behaviour comes from reading its source.

**W2, prompts (K11, K16).** Fixed: Cmd-V, `C-r` and `C-y` on prompts, Cmd-Q with a prompt open, `*messages*` opening at its start, a scroll over the list, `describe-command` reordering the palette, find-file starting at `/` from Finder. `\x \f \] \hk \hc` have end-to-end tests. API: `Ctx::set_prompt_register`, `registers::for_prompt`, and super keys go to keymaps first. Open: `C-r =`, Cmd-V in the real app, and no real surface overrides `Surface::handles`, for the terminal to fix.

**W3, hardening review (K15, R6).** Fixed: `3 C-u j` moved 12 (major), and the rest of `C-u`'s arithmetic against Evil in 12 cases. A cursor scrolled off a wrapped line lost its screen column. A frame that failed to draw waited for the next event (major by reading, not reproduced), and a launch in front could miss the keyboard (plausible, not reproduced). R6's latency code and the handover found nothing. API: `grammar::CountWas`, `window::sleep_or_retry`, `RETRY_DRAW`, `Shell::take_focus_when_active`. Open: `C-u -` isn't done and has no row. A restart in front can steal focus back if you Cmd-Tab away in that half second. Call 335 is out of date: a drag over the gutter moves the head.

**X1, saving elsewhere, after the run.** No review. `:w PATH` and `:w! PATH` follow Evil: a buffer with no file becomes that file, one with a file writes a copy and keeps its own. `:sav[eas]`, `:wq PATH`, `:x PATH`, `:up PATH`. `:f`, `:file` and `C-g` show Evil's file info. `:ls`, `:buffers` and `:files` list buffers in `*messages*`. 4 oracle cases and 5 harness tests. API: `Ctx::save_to`, `Ctx::messages_buffer`. Open: `:d :y :m :t :co` with ranges (80 to 100 lines). `:w >> file` and `:w !cmd` refuse. `:up PATH` and `:x PATH` write with nothing unsaved, where Evil doesn't. `~/` in `:w` untested. Nothing seen in a real window.

### Calls (veto any)

**Budget and scope**

441. die-vim's limit went from 5,790 to 7,000 ahead of the run (85b9410), for the Evil keys left out of M0, marks and the jump list.
442. session's limit went from 1,086 to 1,250 ahead of the run (85b9410), for opening files in the running instance and what a restart keeps.
443. A new module, project, got 600 lines ahead of the run (85b9410), for the walk, the finder and grep.
444. Project search and fuzzy file finding are in core, not an extension (scope §11).
445. die-vim's limit during the run: 7,800 (U6), 7,850 (W1), 8,050 (U8), 8,100 (V1), 8,150 (U3), 8,200 (V3), 8,500 (U6's review), 8,800 (V2).
446. session: 1,305 (U7), 1,385 (U8). project: 750 (U3). files: 760 (U1), 780 (V2).
447. text and undo: 1,285 (U7), 1,300 (U3), 1,420 (V3). The last is a real increase in core, for the one-pass path.
448. workspace, windows, pointer: 1,340 (V6), 1,365 (U7). ext, dispatch, rt: 1,510 (V1, for another package's overrun), 1,525 (V3). keys, keymaps, describe: 660 (V3), 675 (U6's review).
449. `platform::macos`: 1,160 (V6), 1,170 (U7), 1,180 (V5). Stub: 165 (U7). U7's two only cover another package's overrun. V1 moved the test that holds the macOS limit from 1,060 to 1,180 to match, and that test exists to hold the unsafe-review limit, so look at why the limit moved without it.
450. checks: 720 (V5).
451. Paused WIP history was squashed into one commit each (U2, U6, V5, W1), and commits carry no attribution lines, by your rule over the session's reminder.

**U1**

452. A run whose parent is launchd is an instance, any other from a bundle binary a client, unless it has `--handover`. A process orphaned to pid 1 before that check would become an instance, judged harmless.
453. `die` with no files from a terminal brings the instance forward or starts one.
454. If the focused window is a terminal, the file opens in the most recently used window that isn't, and that window takes focus. Tested only with a buffer set to Passthrough.
455. Several files open as `:e` would, so the last is on screen. `die a b` outside a bundle still shows the first and parses no positions.
456. Columns count characters. rg's `--column` counts bytes, so it can land off on non-ASCII lines.
457. `+LINE PATH` takes PATH literally, and takes `+LINE:COL` too. `foo:12:3` where `foo:12` exists means line 3 of that file.
458. A new instance gives a client's lock probe 20 ms, adding up to 20 ms to a second launch about to fail.
459. If `~/.local/bin` isn't on PATH, the printed line adds the bundle's `Contents/MacOS` to PATH. A regular file at `~/.local/bin/die` is left alone and reported.
460. Activation uses the deprecated `activateIgnoringOtherApps`, because the newer `activate` only takes what the active app yields.
461. A lone terminal is split with the new window on its left, not replaced. If it's too small to split, it's replaced.
462. `die file:line` sets a jump only with a line. A plain `die file` into another buffer gets it at the next key, as `:b` does.
463. The jump goes through a keymap rule on `Ctx`, so core knows nothing of Evil's jump list.
464. A file arriving during a prompt opens underneath, and the prompt stays up.
465. `open -a DIE file` and Finder aren't built, by both agents' estimate of over 30 lines.

**U2**

466. Two plain `std::thread`s rather than `Ctx::spawn`, as the ranker needs a long-lived loop and the walker a stop flag. Closing the prompt stops both.
467. `<CR>` and `<Tab>` wait for the ranker's answer for up to 2 s on the main thread, and do nothing if it doesn't come. Typing never waits.
468. Only the best 1,000 matches reach the main thread. The count shows the true total.
469. Non-UTF-8 paths, sockets and pipes aren't listed. A symlink to a file is. A symlink to a directory is neither followed nor listed.
470. `.cargo/registry` is skipped only under `.cargo`. The other skips match by name at any depth, so a project directory named `target` or `Library` is skipped.
471. If the app starts in `/` and no buffer has a directory, `find-file-project` uses home.

**U3**

472. `gf` does what your `gF` does, line numbers included, because the package asked. Your Evil's `gf` is plain `find-file-at-point`, which prompts on a name that isn't a file, and here it's an error. No fixture can show this, so no row. A directory counts as not a file.
473. `:grep` keeps focus in the source window. `*grep*` goes in a window showing it, else the most recently used other window that isn't a terminal, else a split below.
474. One row per line at its first match, so `:cn` steps by line.
475. `[DIR]` is the last word if it names a directory, so a pattern can't end in one, and `:grep mod tests` searches `tests/`.
476. A row shows at most 500 characters of its line.
477. Non-UTF-8 files with no early NUL are searched, with replacement characters.
478. `*grep*` isn't kept by the session, because its `<CR>` binding and run state can't be restored. V2 notes this may matter for restart-heavy use.
479. `*grep*` fills its window, without max width or centering.
480. Emacs's command names and end messages: `next-error`, `previous-error`, `compile-goto-error`, `Moved past last match`.
481. `.claude/worktrees` is skipped by name rather than by reading `.gitignore`.
482. Visual `gf` tries the buffer's directory and then the project root, where Evil tries only the first. No row, as the oracle can't set up files. After one that succeeds, the old buffer's selection ends, not compared.
483. `:line:col` keeps character columns, where Evil's `gF` uses `move-to-column`, so tabs land differently.

**U4**

484. New deviation rule, "sections start at a bracket, whatever is open" (evil-bug, 2 cases): `]]` and `[[` go by lines starting with `(`, `[` or `{` outside a `"` string. Evil's results differ only for nested or unbalanced column-0 brackets.
485. 8 new rows under "the last line is the last line", including `99gg`, `9g_` and `]]` with no section left.
486. `tools/evil-oracle/oracle/user-evil.el` has `(require 'etags)`, though it otherwise mirrors init.el. I assume your running Emacs has it loaded, unchecked.
487. `gd` also sets the search highlight, as `*` does here. Evil only flashes it.
488. `][` and `[]` are unbound.
489. `is_symbol` is DIE's word test plus `_-+*/&|<>=`, so non-ASCII symbols won't match Emacs's table exactly.
490. New oracle topics go in their own files, so packages don't conflict (U4, W3).

**U5**

491. `:sort :d :y :m :t` are left out, at about 160 lines. A draft compiled up to a few borrow errors and never ran.
492. `ga` on non-ASCII says `file ...`, as batch Emacs does. A graphical Emacs may print bytes.
493. `=` rewrites leading whitespace as spaces, as the oracle's Evil has `indent-tabs-mode nil`, so it converts tab-indented lines.
494. `&` on a block acts on the bytes from the selection's first end to its last. Not asked of the oracle.
495. U5 regenerated `operators.json` and `ex.json` only, and W2 `ex.json` only. This entry's full regeneration matches.

**U6**

496. A core hook, `Ctx::on_command_ended`, rather than die-vim guessing. It fires after a key's command, a prompt's answer and a pointer press, like `post-command-hook`, and queues one call per command.
497. C-o's return lives in that hook, and swallows any error, since no command is left to answer for it.
498. A click counts as a command for die-vim: it ends a C-o wait and breaks a C-p or C-y run.
499. `C-k` stays out, as scope lists digraphs as out. `C-z`, `C-h` and the global `C-b C-f C-j C-l` aren't built.
500. `C-x C-n`, `C-x C-p`, `S-Left` and `S-Right` are built rather than given rows, as all four are in your Evil's map. `<Insert>` is unbound: Mac keyboards generally lack it, and Evil's behaviour there is quirky.
501. All 246 cases are kept, well over about 3 per key, as they encode real behaviour and the oracle takes under a second.

**U7**

502. `:N` sets no jump, against the package text, as Evil's `ex-line-jumps` fixture shows.
503. `g;`, `g,` and the `.` mark are Vim's, because your Evil has no goto-chg and only errors there. An evil-gap row with hand-written fixtures.
504. `g;`'s list isn't saved across a restart. The `.` mark is.
505. A mark or jump into another buffer under an operator cancels it with an error.
506. scope §10's line also lists `^`, `{ }`, `gi` and `u` as jump sources, taken from Evil.
507. A killed file's mark reopens the file without Evil's "Visit file again?".
508. A jump into a killed buffer is dropped. Evil's would stick on that entry, from reading evil-jumps.el.
509. `.` for a command with several edits is the furthest place in the text, as Vim has it for `:%s`.
510. A new deviation kind, `not-in-scope`, for `` `( `` and `` `) ``, as DIE has no sentence motions.

**U8**

511. A generic extension hook with opaque JSON, so core doesn't learn about `.`. die-vim builds JSON through a re-export, so it has no serde dependency.
512. `&`'s replacement is kept, though not on the list, since a pattern alone would make `&` delete.
513. The search highlight doesn't come back until `n` or `N`, since it would also have to respect `:noh`.
514. Not kept: `g;`, a `q` still recording, and the keys of an insert session open at the restart. Its text stays, and `.` stays the change before.
515. An extension whose `save_session` panics keeps nothing for that snapshot, unlogged, as a snapshot has no `&mut Ctx`.

**V1**

516. `cargo xtask app --home DIR`, as the harness refuses commands that set HOME.
517. `:qa` without `!` refuses with Cmd-Q's message. Evil may prompt. Not asked of the oracle, no row.
518. `:wa` tries every file, past one that's refused, and returns the first error. Not asked of the oracle.
519. `:wqa!` and `:xa!` force the writes but refuse to quit if something's left unsaved, as `:wq!` does.

**V2**

520. `+`, `_` and Return stop on the last line, and `+` fails there, under "the last line is the last line". 4 rows.
521. `:bn` and `:bp` go through file buffers in opening order and wrap. Evil follows each window's history, which core doesn't keep.
522. `:bd` refuses unsaved text instead of asking. It closes the windows on the buffer and keeps the last.
523. PageUp and PageDown use the `C-b` and `C-f` commands. Not compared with the oracle.
524. `C-^` shows the most recently used buffer no window shows, or else the most recent other, approximating `other-buffer`.
525. `g$` lands two characters before the next row's start, a quirk of your Evil.
526. `:3,2s` does nothing, with no error, as the oracle has it.
527. `a_day.rs` repeats four leader bindings, because a die-dev dev-dependency would link a second die-vim.
528. `:'q,.s` with an unset mark was removed rather than given a row: Evil errors during typing, DIE at Return.
529. `:redo` isn't bound, as it errors in your Evil's batch run.

**V3**

530. Registers over 1 MiB stay out of the session, so a whole big file yanked is gone after a restart.
531. A pattern over 64 KiB is an error, so `*` on a longer word says "too long".
532. A grapheme cluster over about 1 KiB is cut. A cursor can land mid-flag deep in a run of flags, and `l` on a line of only marks says "End of line".
533. Search highlight on lines over 1 MiB looks only within 128 KiB of the view, so `^` may highlight at that cut.
534. A typed `C-g` ends a running macro and is consumed.
535. MAX_RUNS stays at 100,000 macro starts between typed keys.

**V4**

536. The status line keeps showing only the directory, clipped in narrow splits, as `init.el` line 379 has it. V1 notes you may want the file name added.
537. The inactive status line stays #555 on #1a1a1a with no box. Emacs draws a #3a3a3a box and DIE doesn't.
538. The prompt list stays tall after candidates narrow, and draws over the bottom rows. I read this as vertico's grow-only default, unchecked.
539. Only the right edge is extended. Below the grid stays the scene's background.
540. `looks.rs` has its own copy of the small PNG writer, to avoid a dependency.
541. The ignored test asserts four mono-dark colours. The search faces were checked by eye.

**V5**

542. At rest the file poll is every 5 s, Emacs's auto-revert interval, or within 1 s of any input. Your target said at most once a second.
543. `idle-60` takes the median of 6 ten-second slices, so a burst shorter than half the run passes unnoticed.
544. `idle-60` holds its 1.0 limit in the check too, so a plain `die --check idle-60` fails.

**V6**

545. U7's limits were kept over the 1,175 in the branch's unpushed commit, and no empty merge was pushed.
546. `catch.s` is clang's `@try` output as hand-kept aarch64 assembly via `global_asm`, instead of objc2's `exception` feature, which compiles through cc and D16 keeps out. aarch64 only, enforced with a compile_error (D12).
547. Only pasteboard, defaults, layer and key-monitor calls are guarded. Getters, `come_to_front`, `resize`, `post_key` and the like aren't, from reading, not a test.

**W1**

548. A scroll over a prompt's list or line moves the chosen row, replacing W2's choice to ignore it.
549. Double-click and drag autoscroll stay unbuilt.
550. `3ix`, a click, `y`, Esc replays "xy" at the click. Not checked against Evil, as the oracle can't click.

**W2**

551. `C-r` works on every prompt, past Evil, whose `/` is isearch. Rows and hand-written fixtures for `/`.
552. `C-y` pastes the newest kill on every prompt. `C-v` is literal on `:` and `/` only, and candidates prompts keep it as page down.
553. Cmd-V strips only trailing newlines. Inner ones draw as `^J`.
554. A super key bound in a layer beats the prompt, so binding `<D-v>` would beat prompt paste.
555. A window that starts showing `*messages*` goes to its end, even if a cursor was parked. An open one follows only from the last line.

**W3**

556. Evil's `C-u 0` is a count of zero, so `j` doesn't move. DIE treats it as none, as changing that touches every motion. Row "a count of zero".
557. `d C-u` doesn't copy Evil writing empty text over the registers and clipboard. Row "an operator C-u cuts short".
558. The drawing retry is 50 ms and stops while occluded or zero-sized. A GPU that keeps failing wakes the loop 20 times a second.
559. `take_focus_when_active` stays armed after an `open -g` launch and fires on the first activation, however late. It takes nothing from another app.

**After the run**

560. The clipboard's Objective-C `@try` is hand-written aarch64 assembly inside `catch.rs`, as clang writes it, since objc2's own needs a C compiler (D16). platform::macos's limit went from 1,180 to 1,270 to count it.
561. die-vim's limit went from 8,800 to 8,810 for the merges after V2, then to 8,876 for X1. files went from 780 to 833 for X1.
562. `C-g` shows file info as Vim's does. In your Evil it's `keyboard-quit`, which the oracle can't run.
563. `:w own.txt` naming the buffer's own file by a relative path is just `:w`. Evil refuses, since it compares the raw path.
564. An existing file is refused without `!` ("PATH exists; ! overwrites it"), where Evil asks y/n. That includes `:saveas` with no name on a buffer with a file, which DIE treats as `:w`.
565. A missing directory in `:w PATH` is an error and changes nothing, where Evil offers to make it.
566. `:saveas` or `:w PATH` onto a file another buffer visits refuses, and so does any `:w PATH` from a file that opened read-only.
567. `*grep*` and `*messages*` write a copy with `:w PATH` and keep their names.
568. `:saveas` with no name on a buffer with no file is an error, where Evil prompts for one.
569. `:ls` shows `*messages*` in the window, as Evil's buffer menu takes it. The format is DIE's: marks, the name padded to 20, then the path.
570. The restart loop's 1 s limit stays, though one restart took 1,021.6 ms under the full suite's load.
571. `M-x` opens the command palette in normal, insert, replace, visual and copy, bound by `die` beside Cmd-Q, as Emacs's global map has it under Evil. Not in a prompt, where Emacs refuses it, nor in operator-pending, where it ends the operator. Your first `M-x` in the app said `<A-x> is undefined`.

## 2026-11-07: 28 packages on `m0/integration`, and none of them on `main`

**Waiting on you:**
1. **Whether this reaches `main`.** A lander's `gh pr merge` was denied by the auto mode classifier as "Merge Without Review", so nothing in this run has reached `main`. That decision is yours. PR #17 (K10b) is still open and the branch supersedes it.
2. **The hand checks** in "What only you can check" below.
3. **Any call below.** You can overturn any of them at any time. The ones I'd read first: 299 (`die-vim`'s guard 7 limit is now 5,790, against 5,000), 300 (`platform::macos` is no longer a thin wrapper), 360 (keys typed during a restart are dropped), 417 (`/` follows `evil-command-line-map` though your `/` is isearch), and 383 (Cmd-Q refuses unsaved work even with a session to keep it).

### How this run worked, and how that differs

You said: "we need to be faster still - fewer tests and more agents". So:
- Everything lands on the branch `m0/integration`, not `main`.
- A builder lands its own work on a gate of fmt, lint, budget, check, clippy and the touched crates' tests. After a rejected push it re-merges and runs only `cargo check`.
- Reviews run behind the landing, and fixes go forward. Before, a review and `ci` came ahead of each merge.
- `ci --full`, `ci --linux` and `mutate` run only at checkpoints.
- Speed, window and slow tests are `#[ignore]`d by default, so plain `cargo test` doesn't run them.

**Final checks on `main` at 7c530fb, before this run:** `mutate` caught 48 of 48 in 3,117 s, and `ci --linux` passed in 495 s. This closes the run the entry below left waiting.

**A correction to the five log commits from before this run.** T0, K6, K9, K10b, R3, F3 and V4 have their reports on the local branches `m0/t0-faster-checks`, `m0/k6-k9`, `m0/k10b-save`, `m0/r3-f3-window` and `m0/v4-visual`, and those log commits never landed. Each says `ci` passed or that it merged. That didn't happen. On `m0/integration` they had a whole-workspace test run with no failures, at 100 property cases, and no `ci`. Their calls all started at 191, so here they're renumbered, 191 to 295. Their references to one another are corrected below.

### What you can do now, on `m0/integration`

`main` still has none of this. Nobody has yet typed into any of it with real keys.

- **Open a file and edit it.** `die file.txt` opens a borderless window without taking focus. `cargo xtask app` builds `~/Applications/DIE.app`, which keeps one instance and restores your session. No report shows that command run for your real bundle.
- **Vim, held to your Evil by the oracle, except that windows have only harness tests.** Normal, insert, replace and visual state, operators, text objects, registers, `.`, macros, `/ ? n N * #`, `:s`, `H M L`, `gj gk`, the scroll keys, `C-u`, and the `C-w` map with `:sp :vs :new :vne :clo :on`.
- **Files.** `:w :w! :q :q! :wq :e :e! :b :N :noh`. `:w` refuses a file changed on disk, and files that change on disk reload.
- **Find things.** `\f` find-file, descending inside the prompt. `\x` command palette, recent first. `\]` switch-buffer. `:kill-buffer`. `\hk` and `\hc` describe a key and a command.
- **Mouse and trackpad.** Click, drag to select, scroll the window under the pointer.
- **Clipboard.** Yanks reach other apps and their copies reach `"+`, with line and block kinds kept between DIE instances.
- **Quit and restart.** `:q` and Cmd-Q quit, and refuse unsaved file buffers. `restart` in DIE.app hands over to a new instance. Splits, cursors, scroll, unsaved text, registers, macros and histories come back.

**Exit tests:** (b) `cargo xtask restart-loop 20` passed: 21 equal digests, slowest restart 580.9 ms against 1 s, in one run on a loaded machine. (d) holds by count (below), except that V7's row also asks for cases from dogfooding, and there are none.

**Oracle, from a fresh `DIE_ORACLE_BLESS=1` run on this entry's base:** 1,411 of 1,411 cases pass, with 68 deviations excluded and 0 pending. That's 1,410 from your Evil, less the 68, plus 69 hand-written fixtures. It was 905 of 1,075 in the entry below. Any entry in `pending.txt` now fails the test.

**Budget, `cargo xtask budget`, the lines that moved:**

| Crate or module | Entry below | Now | Limit |
|---|---|---|---|
| die-core | 5,450 | 9,970 | 14k |
| text and undo | 1,171 | 1,260 | 1,265, was 1,250 |
| layout | 888 | 914 | 950 |
| keys, keymaps, describe | 598 | 646 | 650 |
| ext, dispatch, rt | 1,498 | 1,411 | 1,500 |
| workspace, windows, pointer | not listed | **1,317** | 1,320, was 1,100 |
| files | not listed | 634 | 650 |
| command line, prompt, core commands | not listed | 1,135 | 1,300 |
| faces and scene | 902 | 936 | 950 |
| session | not listed | **1,086** | 1,086, was 1,000 |
| harness, app interface, messages, ids, errors | not listed | 631 | 800 |
| die-render | 1,781 | 4,128 | 7k |
| macOS build | 1,717 | 3,971 | 5k |
| Linux build | 1,258 | 3,088 | 5k |
| neutral | not listed | 2,232 | 2,300 |
| checks (new) | none | **699** | 700 |
| `platform::macos` | 523 | 1,040 | 1,060, was 900 |
| platform stub | 64 | 157 | 160, was 70 |
| die-vim | 3,095 | **5,789** | 5,790, was 5,000 |
| die | not listed | 358 | none |
| die-dev | not listed | 181 | none |
| product code | 10,373 | 20,426 | none |
| xtask (dev) | 3,790 | 4,517 | none |
| evil-oracle (dev) | not listed | 588 | none |

die-vim, session and checks have 1 line or none left, so the next change to any of them needs a cut or a call. Call 298 lists each raise.

### What only you can check

- Type into a key window, including `C-/`, `C-Return` and whether AppKit beeps (R4). Yank in DIE and paste in another app, and the other way, and watch for a pasteboard privacy alert (R5).
- Insert a character from a live Character Viewer (R4). Cmd-Q on a real key (R4). Hold `j`: it should repeat, with no accent popup, since only NSUserDefaults was read (R4).
- From the Dock, launch and type. Restart while DIE is in front, then type (K12, K15). One Dock icon, a restart may briefly show two, and AeroSpace tiles `dev.die.DIE` (F4a, F4b).
- S1's `interactive typing` on a window covering the display, a real Cmd-Tab, a real display change and a real resize drag (R6).
- Compare a rendered page with your Emacs at 16 pt (R2). `:w` refusing a modified file, once (V5, criterion (c)).

### The packages

**T0, faster checks.** No review (dev tooling, call 186). `ci` skips `guards` and `clean-build` unless the build setup changed, `ci --full` runs everything, `mutate --changed` runs a subset, and each mutant is cheaper. Measured: prop tests 702.6 s to 419 s, and full `mutate` 2,052 s at 1,000 cases with debug info off. T1 has since changed the case count to 100. API: none, and `prop.rs` reads a new `DIE_PROP_SHRINK`. Open: `mutate --changed` has only run with 0 mutants selected. M48 is a rare catch, at about 600 cases from two samples, so I estimate a 4% chance of a false NOT CAUGHT at 2,000.

**T1, checks for this run.** No review. 21 tests ignored, none deleted. CPU for a warm run fell from 60.5 s to 28-29 s. API: none. `ci` now tests at 100 cases, and `mutate` and `ci --linux` pass `--include-ignored`. Open: the under-60 s wall-clock target wasn't measured (81 s and 119 s at load 29-42, about 54 s is derived). `docs/scope.md` lines 440 and 453 still describe merging on `ci`, and m0-plan's "each package starts with a test plan" conflicts with this run. Some ignored speed tests also check behaviour, which now runs only at checkpoints.

**K6, the workspace, and K9, the host's seam.** Review: 8 findings, 1 major. Fixed: closing a window could shrink an unrelated one to 1 column (major), closing overwrote a buffer's rest position, focus could land outside the tree, a surface heard `Focus(true)` under an unfocused window, a long paste logged a false message. 3 nits left: no tests for `write_clipboard`'s `seen_text`, `normalize_views` or `widen_path`. API: adds `ShowIn`, `Side`, `Clipboard`, `FakeClipboard`, `Input`, `PointerEvent`, `PointerKind`, `Button`, `TURN_LIMIT`, `CellHit`, `WindowPart`, `Config.clipboard`, `TextBuf::{modified, set_modified}`, and on `Ctx` `show, buffer_list, directory, set_directory, split_window, close_window, only_window, balance_windows, window_in_direction, focus_direction, window_list, set_frame_size, frame_size, placements, set_clipboard, clipboard, write_clipboard, clipboard_news, handle, run_ready, next_deadline, scene, cell_at`. Behaviour: `kill` moves windows on, `open_window`, `resize` and `focus` follow the tree, `compose` keeps a position map. Open: window sizes and focus rest partly on memory of Emacs, and V6b's harness tests check some of it. A second `open_window` stays outside the tree.

**K10b, saving.** Review: 9 findings, 1 major. Fixed: undoing to the saved text still counted as modified (major), a reload's cost grew with the file, a poll could load a half-written file, undo past a reload could write U+FFFD, `:e!` onto non-UTF-8 broke `u`, a reload cleared a read-only flag, a save dropped ACLs on macOS. Accepted: BOM loss on undo past a reload (call 250), and ACLs still lost on Linux (call 252). 2 nits left: the `:w!` hint on a read-only file, and `Ctx::save`'s doc on an in-place write that fails part way. API: adds `Ctx::{save, save_force, revert_file, check_files}`. `FileInfo` gains private fields, and `open_file` starts a 1 s timer. Open: the owner-can't-be-kept fallback and a crash during a save are untested. Save hooks don't exist, so the contract's queued write isn't honoured.

**R3 and F3, the window.** Review: 10 findings, 1 major. Fixed: `read_next_frame` read a framebuffer-only texture, which was unsound (major), `*scratch*` covered a startup hook's window, a window opened hidden presented nothing, line numbers by extension, the status line's directory, `--check show` failing a healthy window, `die -h`, startup errors in Debug format. 2 nits left: the `idle` test doesn't catch a 100 Hz poll, and nothing guards the flush per transaction. API: die-core adds `Ctx::set_status_text`, `StatusTextFn` and `Display` for `Error`. die-render adds `window`, `check` and `Surface`. `die` adds `Config`, `start`, `run` and `check`. Open: about 20 idle wakes a second where S1 measured 0.3, source not found. Plain mode's first frame isn't run for real.

**V4, visual, `.`, macros, `R`.** Review: 10 findings, 6 major. All acted on, one only partly: `R` still has no `C-a`, `C-e` or `C-u`, and neither does insert state. `a=xu` was the finding's error: Evil gives `abc`. API: adds `RegKind::Macro`, `Ctx::on_fed_dropped`, `Ctx::clear_register` and `Start.visual`. `Selection.goal` of `usize::MAX` on a block means to each line's end. Open: unchecked against Evil are a macro ending on `f`, `g` or `"a`, `3@a` with open operators, `q1`-`q9`, and `vi"i"`. No fixture for `gv` after a visual paste, or `O` on a block with a short line.

**K7, harness.** Review: 1 major, 4 minor, all fixed. Tty cases now run at W - 1 columns and are held to Evil's rows (major). Panics that bypassed the error log, repeated job waits, `with_config`'s clipboard and errors outside an input are all fixed. API: adds `Harness` and `KeyLog`, including `set_clipboard` and `take_errors`. Open: S4 tests stay on `Ctx`, so that part of the row isn't met (call 305). Error text isn't compared, only which key it came after. One new test takes about 5 s against a 9 s limit.

**R4, keys.** Review: 2 major, 2 minor, all fixed. `:q` and Cmd-Q now quit (major), Character Viewer text now reaches core (major, winit's drop confirmed by test), a second button no longer ends a drag, press-and-hold is off in the argument domain. API: die-core none, die-vim adds `quit_app`. die-render adds `input`, `KeyMonitor`, `insert_text` on `Shell` and `Surface`, and `Options.hidden`. Open: everything under "What only you can check". Pointer and IME have no live test, and Linux was checked, never run.

**R5, clipboard.** No review. Yanks and outside copies go through the general pasteboard, and kinds survive under `dev.die.register-kind`. API: die-render adds `Pasteboard` and `system_clipboard`, and `die::run` fills the clipboard. Open: the `launch` wiring has no test, the privacy setting wasn't tested, `Clipboard::write` drops failures silently, and non-text copies are untested.

**K14, pointer.** No review. API: adds `Ctx::set_cursor_rest`, and `Input::Pointer` now acts. Open: drag autoscroll and double-click aren't built. A click in insert state replays through `.` as if there was no move, not compared with Evil. No oracle cases cover the pointer, and a key observer doesn't see its `<Esc>`.

**K11a, command line and messages.** Review: 1 major, 5 minor. Fixed: `<Up>` now matches the prefix (major), Evil's line keys on `:` and `/`, `:` no longer clears `.`, `@:` built, a blank after a colon. K11a-6 is a comment and a scope entry, with behaviour unchanged. API: `Ctx::prompt` takes `PromptSpec` (`initial`, `history`, `backspace_cancels`, `on_change`, `evil_keys`), plus `Ctx::prompt_history`. Reported errors are now Display text, `messages` keeps 1,000 lines, `compose` draws the newest message. Open: `C-r`, `C-v`, `C-y` in prompts aren't built, Cmd-V there isn't checked, `*messages*` doesn't follow its end. The cursor rule after `<Up>` comes from memory (call 324).

**K11b, candidates, find-file, buffers.** K11b1 review: 1 major, 5 minor. Fixed: palette order, a bare directory line on `<CR>`, exact over case-fold ties, `require_match`, `<M-<>` and `<M->>`. K11b1-3 was already fixed by K11a's fix, with a probe passing. K11b2 review: 1 major, 3 minor. Fixed: `~/` and `//` restart the path (major), symlink `..`, a missing directory now offered at open. K11b2-3 was already fixed, and a test now holds it. K11b3: no review. API: adds `PromptSpec::{candidates, candidates_from, descends, require_match}`, `Ctx::{prompt_candidates, prompt_chosen, command_names}`, `flex_rank`, `recent_first`, faces `prompt-current` and `prompt-match`, commands `command-palette`, `find-file`, `switch-buffer`, `kill-buffer`. Open: the flex table is only what Emacs 30.2 printed for the first agent, and the reviewer couldn't reproduce it. `\x`, `\f` and `\]` have no test. `~` on the starting line has no test. A trackpad scroll over the list seems to scroll the hidden window (read, not probed).

**K16, describe.** No review. API: adds `Ctx::{describe_key, describe_command, layers}`, `KeyDescription`, `KeyAction`, `CommandDescription`, `Surface::handles`, and two commands. While describe-key reads, keys go to it ahead of the prompt and passthrough. Open: no surface overrides `handles` outside the test, `\hk` and `\hc` have no end-to-end test, and the builder that landed it didn't read the tests it inherited.

**V2, search and basic ex.** Review: 4 minor, all fixed. `*` and `#` on punctuation, empty matches, a huge count freezing, `:+5`. API: die-core adds `TextBuf::empty_match_at`, and `Pattern::word` needs a word character at both ends. die-vim adds `search.rs`, `/ ? n N * #`, `:N :e :b`, `Target.to_last_line`. evil-oracle adds `OracleDies`. Open: scroll restore on cancel has no test, and highlights have only been seen through scene cells.

**V5, the rest of ex.** Review: 2 major, 4 minor, all fixed. `:q!` now really drops (major), `\n` and `\t` in a replacement (major), `:s` shares its pattern with search, two line edges of `substitutions`, `:wq!` forces only the write, `'<,'>` is stripped for other commands. API: adds `Ctx::{quit, quit_wanted}` and `TextBuf::substitutions`, and `about_to_wait` exits on `quit_wanted`. `C-w q` and `:q` call `quit`. Open: one SIGABRT in `crates/die-render/tests/platform.rs` ("Rust cannot catch foreign exceptions"), cause not found. It passed alone, and K15's run didn't show it.

**V6, scroll and windows.** V6a review: 2 major, 4 minor, all fixed: `C-e` and `C-y` landing, `row_pos` on wrapped lines (majors), a `C-e` count past the end, `gj` and `j` resetting each other's goal, a part-way `gj` count, `C-d` at the end. V6b review: 3 major, 3 minor. Fixed: `:q` on a split balances, `C-w x` is a port of `window-swap-states`, `~/` in `:sp` (majors), `C-w` count digits, `N C-w |`. V6b-6 was overtaken by V5. API: adds `TextBuf::{scroll_rows, recenter}`, `Ctx::{windows_by_use, close_window_balancing, swap_windows, move_window, balance_frame, exchange_windows}`, `Start.window`, and the `vim-window-*` and `vim-scroll-*` commands. `command()` reveals the cursor after each command. Open: window behaviour has only harness tests, no oracle cases. Most scroll commands on wrapped text have none. `$gj` onto a short row wasn't compared with Evil. `C-w x` with 4 in a split wasn't either.

**K12 and K13, the session.** K12 review: 1 major, 5 minor. Fixed: sustained change put off every write, with a 5 s ceiling now (major), a cursor saved in insert state, focus on `*messages*`, `read_only` on clean files, an extension's buffer doubling, and focus at restart (a hand check). A startup hook's `create_text` still gets `<name><2>`: such a hook should check `find_buffer`. K13 review: 4 minor. Fixed: a window on `*messages*`, sizes after a gone window, Insert's cursor, wrong-shaped fields. Partly left: a tree that is all `*messages*` windows doesn't come back. API: adds `session`, `Config.session`, `Ctx::{session_json, restore_session, resume_session, save_session, hand_over_session, flush_session, set_insert_end}`, `Harness::restore_session`, `Instance::take_over`, and serde to die-core. Open: `.`, `@@`, `;`, `n` and `gv` don't survive a restart. Real SIGKILL during a write isn't run. Third-launch race between exit and lock. Corrupt `session.json`'s narrow crash window. 80x26 frame default until the host sets one, not looked at in the app. A panic partway through a restore can leave the tree half replaced.

**F4, the app.** F4a and F4b: no review. API: die-core adds `sys::state_dir` and `instance`. die adds `app`, with `Launch.restart_loop`. die-render adds `Options.first_frame` and `Remote::exit`, and `font_dir` prefers the bundle. xtask adds `app` and `restart-loop`. Open: `cargo xtask app` hasn't run for your real bundle. The loop's rebuilds don't change code, so it doesn't prove a restart between different builds. No release restart test in the gate, and the 500 ms check is on the dev build.

**R6, latency.** No review. `DIE_LATENCY_LOG` and `--check typing, redraw, firstframe, idle-60, occlusion-20`. API: die-core none, die-render adds `latency`, `post_key`, `take_key_at`, `watch_presents`, and `Presented` gains fields and loses `Eq`. Open, one run at load 9-13, over S1's raw limits: redraw CPU p99 7.69 ms against 2, GPU p99 1.74 against 1.5, resize p99 11.98 against 8, key to display p50 21.49 and p99 27.54 in a small window. A quiet machine wasn't tried. The redraw limit may flake. Check 1's 60 Hz rise waits for M3.

**V7, the oracle closes.** No review. API: `u` after `:s` follows Evil. Evil's `history-length` is t in the mirror, and each oracle case starts on one window. Open: the new pending check hasn't been seen failing. The 10 new cases are my guesses, not from use. Your Evil keys not yet bound, waiting until you miss them: `ge gE g_ gJ ga g; | [[ ]] & =`, and insert `C-o C-p C-n C-t C-k`.

**K15, hardening.** No review. Core's share of a keystroke: p50 0.157, p99 0.713, max 1.055 ms over 4,000 keys on 10k lines, at opt-level 1 and load about 5, against 10.4 ms p99 before. `gg` and `G` had asked for a whole viewport. Rustdoc on every public item of the product crates. Fixes from use: wrapped-line trackpad scroll, clicks in the gutter or below the text, input held during a handover, focus when launched in front, `C-u 3`, `:noh`. API: signatures unchanged. `hand_over_session` holds typed input until `resume_session`, and `C-u` then a digit restarts the count. Open: only core's share is measured, not scroll commands or wrapped text. The 1 ms limit may flake. xtask and evil-oracle have about 155 undocumented items.

### Checkpoint: the full checks on `m0/integration`

Added after the run. `ci --full` passed on 0ec2045 in 273 s: 82 test binaries at the old property counts, with the ignored speed and window tests included, plus guards and clean-build. On 36d60af, `ci --linux` passed in 125 s and `mutate` caught 58 of 58 in 158 s. The only change between those commits is a retry in xtask's app test. `mutate` took 3,117 s on 48 mutants at 7c530fb, at background priority and higher property counts, so 158 s isn't a like-for-like figure.

It took four rounds of `ci --full`. They found five problems, all in the checks and none in product code:
- `die-render`'s key test took `--include-ignored` for its own re-run, which ended the test binary early and hid what came after it.
- `other_app`, a helper test another test launches, failed when `--include-ignored` ran it alone.
- Guards planted code with no docs, which K15's `missing_docs` rejects.
- M10 had gone stale, because `undo.rs` gained a line between the two it targets. The other 57 still matched.
- `two_launches_leave_one_instance` failed once, when `open` got LaunchServices error -600 from an instance that had only just started.

The light gate let all five through, since none of them run under plain `cargo test`.

### What wasn't verified

- **Checks ran after this entry was first written**, under "Checkpoint" below, and they cover the whole tree. They can't show a problem the tests don't look for, and Linux can still break between checkpoints, since the landing gate has no cross-check.
- **The tip's tests.** Many packages landed with only `cargo check` on their final merge. K15's gate ran on the tip it pushed, 12834dd: fmt, lint, budget, check, clippy, tests for die-core, die-vim, die, die-render and die-dev, plus `die-dev --test window` (8 of 8) and `xtask --test app` (7 of 7). But its filter hid the per-binary totals, so that rests on no FAILED line. evil-oracle's tests didn't run in it, and I ran only the oracle on this entry's base.
- **Speed tests.** Ignored by default, so no gate holds them, and `layout_speed` failed under load in several packages' runs before T1. Load is the reading, not a finding. No limit in ms was raised.
- **Tests that a fix fails first.** Most did, as listed in the reports. These didn't: K11b2-4's rewritten test, K11b2-3's main parts, `wrapped/j-then-gj`, `visual-s-then-undo`. K15's new tests weren't checked by reverting, and K7's rest on an earlier agent's commit.
- **Calls from memory of Emacs or Evil, not run:** marked in each call.
- **Reviews.** T0, T1, R5, K14, K11b3, K16, F4a, F4b, R6, V7 and K15 had none. Those that ran were time-boxed, and several ran over or left part unprobed. V6b's review ran no DIE code.
- **Window test rule slips.** K12 ran die-render's suite 3 times, K15 ran its line twice, and F4b ran two extra short loops. All put windows on your screen. Their reports show no focus taken. Focus as such wasn't checked.

### Calls (veto any)

Calls later fixes reversed appear once, as they now stand. Procedural notes from reports are folded into calls 296-298. The numbers carried over are renumbered as said above.

**T0, carried over**

191. `debug = "line-tables-only"` stays in `[profile.test]`, and this is a close call against the 15% rule. CPU for a cold `cargo test --no-run --workspace`: 835 s to 693 s (17.0% less), then 785 s to 678 s (13.7% less), mean 15.4%. Wall clock did not clear 15%: 12.7% faster in one run and 6% slower in the other. Other agents' load swamped wall clock, so I judged on CPU. A die-core and prop rebuild takes about 21% less CPU, and the cold cache went from 1.4 GB to 1.1 GB. To veto, delete the one line in `Cargo.toml`.
192. A `Cargo.toml`, `Cargo.lock` or `.cargo/` at any depth triggers the slow steps, not just the root ones, because a crate's manifest can change the build graph. Call 188 says "a manifest".
193. A `build.rs` at any depth is a trigger too, which the package didn't list. Cargo picks up a new build script with no manifest change, and `clean-build` is what catches one calling a C compiler.
194. If git can't say what changed (no `main`, say), `ci` runs `guards` and `clean-build`, and `mutate --changed` runs every mutant. Both say why.
195. `ci --full` clears `DIE_PROP_CASES` instead of inheriting it, so a leftover value can't thin a checkpoint. For higher counts, such as 40,000 after a layout change, run `cargo test` directly.
196. A plain `ci` honours an already-set `DIE_PROP_CASES`, as the package asks, and the summary states the value.
197. A rename counts as both its names in the diff, so a mutant naming a moved file fails loudly instead of being skipped.
198. `ci` on main itself has an empty diff, so it skips the slow steps and says so. Use `--full` to check main. `ci --linux` accepts `--full` and ignores it, because its container already runs everything.
199. In `mutate`, the first test named FAILED ends the run, so the report names that test, or those already finished, not every test that would fail.
200. `mutate` sets a new `DIE_PROP_SHRINK=0`, and three `max_shrink_iters` config lines in `crates/die-core/tests/prop.rs` now read it. This touches ported S4 test config only: no test body and no default changed. Shrinking was the biggest cost: M1 spent 515 s after its build with shrinking and without the early stop, and about 1 s with neither.
201. `mutate` sets `CARGO_PROFILE_TEST_DEBUG=false` for its builds. Rebuild CPU went from 22.8 s to 7.9 s, and mean wall from 58.8 s to 39.2 s under load.
202. `mutate`'s default stays at 2,000 cases. At 1,000 all 48 were caught, but M48 took about 600 cases in both runs, so I estimate 1,000 would miss it roughly one run in five. That estimate rests on two samples.
203. A mutant's time limit now multiplies only the tests' part of the first run, plus the build once. The old formula gave each mutant 76 minutes after a slow first build. No mutant in the last two full runs needed the limit.
204. Beyond the package: `ci`, `mutate`, `guards`, `sources` and `cross-check` keep their caches under `$CARGO_TARGET_DIR/xtask/` when it's set, so a worktree's warm clone is actually used. Before this, `ci --worktree` in a new worktree built in an empty `<worktree>/target/xtask/ci`.
205. No mutants added, because this package isn't in die-core.

**K6 and K9, carried over**

206. Scene-2: `positions` stay in `Scene`, and `compose` also copies them into `Ctx` for `cell_at`. The finding recommended moving them out, which would have changed many ported tests and R2's public type.
207. `open_window` keeps S4's meaning. The first window roots the tree, and later ones stand outside it at their own size, because ported tests need explicit sizes. Windows join the tree through `split_window` and `show`.
208. The plan's `run_until_idle(now)` is `run_ready()`, with no `now`, because the clock is in core. It stops at `TURN_LIMIT`, 10,000 turns, and `next_deadline` then returns zero. Call 226 settles when it logs.
209. All host input goes through one ordered queue, so keys, pointer, focus and resize run in the order they came. `Ctx.typed` now holds `Input`. `handle` runs nothing until a turn.
210. Split sizes as the builder recalls Emacs's: the new window gets half, rounded down. A closed window's cells go to the sibling before it, or else the one after. Focus goes to the window used last, and a window never focused doesn't count as used. From Emacs's source by memory, not checked against a running Emacs.
211. The minimum is 2 cells on each axis, one of text and one for the status line or divider, not Emacs's `window-min-height` of 4 or `window-min-width` of 10. In a frame too small for its windows, some get no cells and `placements` leaves them out. They never overlap, and the sizes come back when the frame grows.
212. `balance_windows` is added beyond the plan's row. `evil-auto-balance-windows` defaults to on and your init.el doesn't set it, so V6 needs it. It balances the window's parent, as Evil does. Not tested against Evil.
213. `ShowIn::Current` and `Split` take the focus. `Existing` prefers the current window, then the one used last, and with none falls back to the current window rather than failing.
214. A window's cursor and scroll in a buffer it left are kept and moved through edits. A window new to a buffer starts where the window used last on it is, or else where the last window to leave it left off, Emacs's buffer point, or else at the start.
215. Silent edits set the modified flag, and undo can't clear it, unlike Emacs's `with-silent-modifications`. Otherwise an extension's silent edit to a file buffer would look unmodified, and auto-revert or `:q` could lose it.
216. A kill uses one last-used list across the workspace, not Emacs's per-window buffer history. The per-window history is about 20 more lines if you want it.
217. `clipboard_news` also compares text, as Emacs does, so another app copying the text DIE already knows isn't news. What was on the clipboard at startup counts as news on the first look, as in Emacs. `*` stays a plain register, matching the oracle's separate PRIMARY.
218. Every workspace starts with an in-memory clipboard, as with the fake clock. `Clipboard` methods run unguarded, since it's the host's code and not an extension's.
219. `Event::Focus` is delivered, but not from `open_window`. Five ported tests list a surface's events exactly, so they stay unchanged. A surface's first window therefore doesn't hear `Focus(true)`.
220. Pointer input only counts as activity for idle timers, except a bare `Move`. What a click does is K14's. *K14 has since built what a click does.*
221. The budget wasn't raised. `rename_buffer` was cut because the contract doesn't need it. `FakeClipboard` went to `harness.rs` and the pointer input types to `app.rs`, both in the module with room.
222. The two new properties went into `prop.rs`, because `mutate` only runs that file. 5 mutants added (M49-M53), each checked once with a throwaway script, not `cargo xtask mutate`. All 5 were caught.
223. K6K9-3: whether a second `open_window` takes focus depends on whether the host set a frame. A host that did draws with `scene()`, so focus stays in the tree. A host with no frame size (S4's compose hosts) still gets focus on the new window, so no ported test changed.
224. K6K9-3: an explicit `Ctx::focus(outside)` is still allowed in a fixed frame. It's the caller's choice and the doc says so, so that path can still leave focus on a window `scene()` doesn't draw.
225. K6K9-3: `drop_window` has no fallback to windows outside the tree. The tree is never empty when a window is dropped, so closing a focused outside window also sends focus to the tree.
226. K6K9-5: `run_ready` still yields at `TURN_LIMIT`, and logs only if no turn in the batch took input, not by counting queued-call turns, because a key's command can itself run as a queued call and a paste mixes both. A runaway started by a key in a batch is logged on the next batch, one frame later.
227. K6K9-5: the message now reads 'Queued calls ran for 10000 turns with no key between them, so the frame went first', since 10,000 plain defers can also reach it. `a_call_that_keeps_queueing_another_stops_for_the_frame` changed to match, and it's K9's own test.
228. K6K9-5: a key that feeds itself forever still yields each frame but logs nothing, because input is consumed every batch.

**K10b, carried over**

229. A save writes a temp file (`.die-save-<pid>-<n>`, mode 0600 until the end) and renames it over the file. It writes in place for a hard link, an owner it can't keep, or a directory it can't write. Your Emacs, with backups off, always writes in place.
230. A crash during a replacement can leave a `.die-save-*` file beside the original. Nothing cleans those up.
231. `:w!` still refuses a file that opened read-only (not UTF-8, NUL bytes, CRLF), as K10a's docs require, because for non-UTF-8 the text doesn't hold the bytes.
232. `:w` on an unmodified buffer whose file exists writes nothing and refuses nothing, even if the file changed on disk, because there's nothing to lose. `:w!` always writes.
233. A file that's gone is written again by `:w` without refusing, as I recall Emacs's `basic-save-buffer` doing. This wasn't run against Emacs or Evil.
234. `:w` refuses a file you can't write. `:w!` replaces it if the directory allows, keeping mode 0444.
235. Any fingerprint change makes `:w` refuse on a modified buffer, including `touch` or a same-bytes rewrite, because there's no content hash. As far as I recall, Emacs compares only the modification time too.
236. Refusals are `Error::Msg` with the reason in the text, with no new `Error` variant.
237. The poll runs every 1 s. Your init.el leaves Emacs's interval at its default of 5 s, but I recall Emacs also using file notifications, so 5 s here would feel slower. A host wakes once a second while a file is open.
238. The poll is an ordinary `every` timer started by the first `open_file`, so an extension opening the first file owns it and its shutdown cancels it. That's harmless today, and M1's disabling should recheck it.
239. A reload is one recorded undo step with `Origin::Extension`, so it works in read-only buffers, and undoing it leaves the buffer modified.
240. Beyond the plan row, reloads use a line diff of about 70 lines, patience-style on lines unique to both files and trimmed to character boundaries, so `cargo fmt` keeps cursors and markers between its changes. As I recall, Emacs's revert only trims the two ends. At worst it's one change.
241. A reload waits while any undo group is open, as during an insert with nothing typed. It happens at the next poll after the group closes.
242. A modified buffer gets one message per divergence, not one per outside rewrite. A save, a reload or the file being deleted resets it.
243. A deleted file keeps its text, with one message. If the file comes back, an unmodified buffer reloads and a modified one refuses `:w`.
244. When a reload finds a read-only file now editable, `Settings::read_only` goes back to what it was before the file made it read-only, so your per-path settings hold. A flag you set by hand on a normally editable file stays.
245. Reloads read and diff on the main thread, and the poll won't read files over 16 MiB. The number is my choice, not measured. A hitch while reading a file below it stays possible, which falls short of contract §13's "There are no blocking calls".
246. The model property is in `tests/prop.rs`, as a `saves` module, because `cargo xtask mutate` only runs that binary. It's capped at 300 cases because saves flush to disk.
247. Past 1 MiB of differing text a reload is one change, so cursors and markers inside that middle region aren't kept.
248. F3's wait is a wall-clock check on mtime, and a future mtime doesn't count as recent. `:e!` doesn't wait. A tool that takes longer than 200 ms between truncating and writing still shows the empty file for one poll. The test's assertion is skipped if the poll itself took 150 ms or more on a very slow host.
249. A reload into or out of a read-only state commits as `.silent()`, so `u` and `C-r` can't step into lossy text. This is wider than the finding, because redo could otherwise bring it back. After `:e!` out of such a state, `u` can't bring back edits made with `settings.read_only` off. Text you deleted in that state that held U+FFFD could still return through an older undo step. I accepted that rather than drop the history, since a tool caught mid-write would otherwise wipe your undo.
250. The BOM belongs to the file as last read, so undoing past a reload and then `:w` writes no BOM. A test pins this.
251. `:e!` with unsaved changes refuses when the file can't be edited, including one that was read-only already and stays so. Use `u` first, or `:w!` if the file has become editable. With no unsaved changes, it reloads.
252. On macOS a save now copies the old file's data once before overwriting it, because that's what keeps the ACL, so saving a large file costs more. Copying to a new name clones on APFS and drops the ACL, so it copies onto the existing empty temp. On Linux `fs::copy` carries neither ACLs nor extended attributes, so they're still lost there, and the module doc says so.
253. Some of K10b's own tests changed after the review; no ported S4 test did. The `rustfmt` test's `cfg!(target_os)` branch failed `cargo xtask lint`, so it now probes `rustfmt --version` and skips if that fails, on any platform. It also waits 250 ms after rustfmt, and `rename_over` and two tests age the mtime, so the poll doesn't treat those writes as in progress.

**R3 and F3, carried over**

254. The checks ship in the binary and run on a thread acting through `window::Remote`, because winit needs the main thread. `apps/die-dev/tests/window.rs` runs `die --check` as a child and holds its numbers to S1's rules.
255. A fifth check, `show`, isn't in the plan row: pixels on the drawable against the offscreen path, `Ctx` input drawing in its own turn, a timer and a job's wake each making one frame, borderless, Option as Alt, not active and not key. It opens its own `*show*` buffer, so it doesn't show the named file reaching the window. The facade tests cover that through the screen. The buffer and its window stay behind, because core can't close a window yet, but only in a check process.
256. `Pipeline::present` waits for the GPU every frame, so the SAFETY comments on `upload` and `fill_instances` still hold. It costs main-thread time. S1's ring of buffers waits until something needs it.
257. A check's window floats, has no Dock icon, is 420x280 and is placed by pid. A real run gets a Dock icon and a normal window level.
258. Checks draw even when occluded, except `occlusion`, because a check can't choose what's on your screen. `occlusion` says `skip`, not FAIL, if the window server shows the window to no one, such as on a locked display.
259. A frame is skipped when the composed `Scene` equals the last one, so a timer that changes nothing presents nothing.
260. One pump takes at most 1,024 turns (`TURN_LIMIT`), then the loop draws, reads its events and comes back without sleeping.
261. The resize speed test holds the median frame to S1's 8 ms x 4, not the p99. The median was 5.7-6.8 ms and the p99 97-159 ms at PRI 4 under load. The check prints p99, and R6 holds it to S1's limit on the real app.
262. The idle test's limit is 1,000 wakes in 3 s, not S1's 0.3 a second. See open.
263. Pixels are read back only on request (`Surface::read_next_frame`), because reading a drawable took 37-200 ms. Check windows use a readable layer throughout, timed checks included. Reading an unreadable surface is an `Err`, not a no-op, so a caller can't wait for an image that never comes.
264. The status function lives on `Ctx` and is stored in `Files`, because `ext.rs` has 2 lines left. It gets the name and `FileInfo`, not `Ctx`, so it can't re-enter core during compose.
265. `die::start` always sets a real clock, whatever `Config.core.clock` says.
266. `Renderer::set_scale` reloads the fonts and starts a new `Frames`, but keeps the pipeline and atlas textures. Old atlas pixels are never sampled, because only what's placed again gets used.
267. With no files, one window shows an empty `*scratch*`. A file that won't open is logged and the next goes on screen. Later files open as buffers you can't reach until buffer switching lands. Startup messages also go to stderr until K11a's `*messages*`.
268. Your config has wrap at 80, centered, for every file, because `init.el` has `global-visual-line-mode` and nothing turns it off per file. Leader is `\`. `\z` is the only binding, because `\f \] \/ \x` have no commands yet and C8 would refuse them.
269. Line numbers go by extension and ignore ASCII case, against the finding's "stays case-sensitive", because `emacs -Q` folds case when nothing matches exactly. Only about 45 candidate extensions were compared, not all of `auto-mode-alist`, and exact names like `SConstruct` aren't covered. Beyond the finding's list this adds `i ii lex y shtm`, from the same output. *Both since done, by K11b3 and K11a.*
270. `check.rs` gets its own `checks` budget module, limit 700, so it doesn't eat R4's room in `neutral`. The stub's limit goes from 70 to 110, as `budget.toml` says it must grow with what the driver names. *`\x`, `\f`, `\]`, `\hk` and `\hc` have since been added.*
271. No mutants added: the only die-core changes are small and have their tests.
272. Press-and-hold off is left to R4, with the key monitor. S1 lists it under the window, but only typing needs it.
273. `settings()` in `apps/die-dev/tests/config.rs` gave two tests one directory, which failed one run. It now uses a counter. The race was already there.
274. The facade now imports `std::os::unix`, fine since Windows is out. *R4 did it.*

**V4, carried over**

275. KEYS-2 is a `RegKind::Macro` variant, not the review's `Register.keys: bool`, so no `Register { .. }` literal breaks while K6 changes registers.
276. `macros/macro-with-search` and `search/v-slash-d` stay pending, still tagged V4 because `tranche()` takes the highest feature. V2 takes them, as with V3's `search/d-slash`. I didn't build a stand-in `/`. *V2 took them.*
277. `.` replays keys. Each change command records its own, plus a count and register kept apart. An operator's two counts multiply into one, so a count on `.` replaces both (`d2w3.` acts as `d3w`). A visual change also keeps its extent.
278. Only a change that succeeded becomes what `.` repeats. Two cases agree with Evil.
279. A macro's end is detected at the next typed key, because fed keys run ahead of typed ones. A key observer notes that key, and every die-vim command goes through one `command()` wrapper. `.` running the macro again, the macro's one undo step and restoring `;` all hang on this, so inside a macro a `;` after a `.` sees the replay's search.
280. A macro is one undo step (Evil's `evil-with-single-undo`), kept open with an undo group. What's typed next is its own step, even in an insert session the macro began.
281. New limit, `MAX_RUNS`: 100,000 macros started without a typed key is an error, so a macro that calls itself forever ends. Emacs ends one when its Lisp nesting runs out, at a different count, and no fixture pins the number.
282. `q` reads its register through `State::CommandLine` with `<any>` bound there, with no core change.
283. `cases/visual-corners.json` adds 160 cases beyond the plan's 67, generated from your Evil. 158 are held to Evil as they are, after 12 fixes they prompted. The other 2 are the next two calls.
284. New S5 row 26, an Evil bug: `Rx<CR><BS><BS>` on `abc` gives `abbc` in Evil and `abc` in die-vim, as in Vim. Veto it and die-vim copies Evil, with about 1 line less code.
285. `visual-corners/v-G-d` goes under row 1: `G` in visual state stops on the last line, so `vGd` leaves `ai\n` where Evil gives `a`.
286. A macro whose last key is an operator fails on that key, as your Evil's does. I followed Evil rather than Vim, which would wait for a typed motion.
287. `R` overwrites by display columns, as Emacs's overwrite mode does: `x` over a wide character leaves a space, and a tab it wouldn't fill stays. The typed width comes from a throwaway `TextBuf`. `<BS>` puts back everything the key took; Evil puts back one character, which differs for a wide character typed over two narrow ones. No fixture covers that.
288. In visual state, a text object that adds nothing tries the next positions the way the selection goes. It approximates Evil's logic rather than copying it. `vi"i"` is unchecked.
289. No mutants added, since V4 is in die-vim.
290. The new core API lives in `fed.rs`, counted under `keys, keymaps, describe` in `budget.toml`. That moves `ext, dispatch, rt` down and leaves 8 lines in keys.
291. `insert_state_arrows_match_the_tty_run` goes from 17 to 23 cases, the one ported-test change, because `R`'s arrows and a failed `.` differ from batch mode and only a tty run shows it.
292. New S5 row 27: `u` typed after a macro's `u`. New row 28: a block insert in a macro. In both, die-vim's result is the better one.
293. New S5 row 29: `gv` after undoing what took its text (`vjdugv`). Emacs's undo moves markers back and die-core's doesn't.
294. New S5 row 30: a second `i(` on a one-character selection. Evil is inconsistent: on `((a))` it fails with `(` selected, and in `((a) b)` it takes the outer pair. die-vim finds what the first `i(` found.
295. `v.x`, a macro ending on an undefined key in visual state: die-vim's cursor stays on the head and Evil's steps right. The oracle rejects any case with an undefined key, so there's no row or fixture, and `tests/visual.rs` holds only the state and text.


**This run: procedure and budget**

296. Commit messages carry no attribution lines, following your standing rule, over a session reminder that asked for them. Builders squashed or reworded "WIP: paused" commits, and histories were rewritten only where not yet pushed.
297. Tests stayed within limits: K14 used all 8 of S's and K12 all 15 of M's. Ported S4 tests changed only as listed under each package's calls.
298. Budget raises. die-vim 5,000 to 5,600 (V2), 5,700 (V5, V6), 5,720 (V2's fixes), 5,721 (R4's quit), 5,780 (V5's fixes), 5,790 (K15). workspace 1,100 to 1,190 and 1,240 (V6b), 1,320 (K13). session 1,000 to 1,060 (K13), 1,085 (K13's fixes), 1,086 (K15). text and undo 1,250 to 1,265 (V2's fixes). `platform::macos` 900 to 950 (R6), 1,060 (R4's fix). platform stub 70 to 110 (R3), 115 (R5), 150 (R4), 155 (R6), 160 (R4's fix). `checks` is new at 700 (R3). Each report gives its reason. K13 says it didn't work through the core cut list.
299. die-vim's guard 7 limit went from 5,000 to 5,790 in this one run. The plan's text says an overrun means simplifying at the next review. Veto if you want the cut now.
300. `platform::macos` at 1,060: R4's hook on winit's view "isn't a thin wrapper of anything", so the `unsafe` review has more to read. F4b changed xtask's guard test from 900 to 1,060 as its comment asks, so that test no longer raises the question. This is that question, for you.

**T1**

301. Every test asserting a limit in ms is marked speed (15), though some also check behaviour, which now runs only at checkpoints. Hang guards (a 2 s FIFO, two 10 s waits) stay in, and ratio tests stay in.
302. Window tests marked: 4. Slow marked: 5, by the over-5 s rule on the lower-load run. Tests over 5 s only at load 29-42 stay in.
303. `mutate` passes `--include-ignored` always, and `ci --linux` on its 2 general test steps too, since it's a checkpoint.
304. Plan row (f) and the Linux-drift risk now say the cross-check runs at each checkpoint, since the gate has none. A Linux break can wait until a checkpoint.

**K7**

305. S4 tests stay on `Ctx`, against the plan row, because the standing rule says ported tests don't change. `Harness` derefs to `Ctx`, so they could move later.
306. A key's log holds errors and panics only. News stays in `Ctx::messages`. The runner now counts these errors, not every new message.
307. The harness draws after every input, since top line depends on it. A macro's inner keys get no frame between them, as in the oracle.
308. `with_config` replaces a real clock with a fake and always replaces the clipboard with its own fake. `set_clipboard` through `DerefMut` can still bypass `fake_clipboard`, and its doc says so.
309. `type_keys` resolves `<leader>`, and a failing key doesn't drop later ones, unlike `feed_keys`.
310. `set_window_size(cols, rows)` is the text area, so the frame is `rows + 2`. Default 80x24.
311. Removed `insert_state_arrows_match_the_tty_run` and `wrapped_rows_without_gj_match_the_tty_run` from `oracle.rs`, since the main oracle now holds those cases more strictly.
312. K7-2 goes a step further: the cursor rest rule's panic also goes through `log_error`. New API not in contract §16: `Harness::set_clipboard` and `take_errors`.
313. The oracle runner sets `max_width` to width - 1 on tty cases, m0-plan's Geometry mitigation.

**K11a**

314. `PromptSpec` is a builder with `From<&str>`, so ported `ctx.prompt(":", ...)` calls stay unchanged.
315. Changed 6 assertions in ported or earlier tests (3 in `die-core/tests/edits.rs`, 3 in `die-vim/tests/core_review.rs`) to Display text, because the command row now shows these strings.
316. Prompt keys are hard-coded in `prompt.rs`, so they can't be rebound in M0. Cancel is Esc, C-g and C-c.
317. `<BS>` on an empty line cancels only with `.backspace_cancels()`, which `:`, `/` and `?` use.
318. History: 100 per name, no repeat of the newest or empty answers, and moving off a recalled entry discards edits to it.
319. Prompt cursor moves and deletes by grapheme cluster. A word is alphanumerics and `_`, or a run of other non-blanks, so C-w on `a/b` takes `b`.
320. `:` has history `ex`, and `/` and `?` share `search`.
321. `*messages*` syncs lazily, at each compose and key press. Killed, it comes back with only later messages. `other_buffer` never falls back on it. It and `ctx.messages` keep 1,000 lines.
322. The echo clears on any key but a lone modifier, including fed keys, and has the default face. Startup's `eprintln!` is gone.
323. `PromptSpec::evil_keys()` is for `:`, `/` and `?` only. Other prompts keep Emacs's `<Up>`, `<C-u>`, `<C-b>`.
324. With `evil_keys`, the cursor stays where it was after `<Up>` and `<Down>`. From memory of `previous-complete-history-element`, not verified: simple.el isn't here, and one batch probe hung.
325. `<Down>` past the newest brings back the draft, where Emacs errors. In scope §10, not `deviations.json`, because a rule there must name oracle cases.
326. `@:` above MAX_FEED errors `Count too large`, and a count of 0 runs once, where Evil's loops forever. It shares `last_macro`, so `@@` re-runs the line, as in Evil. A change run from `:` or the palette leaves `.` alone, including insert sessions and operators it starts.
327. A recalled entry whose grapheme continues past the prefix doesn't match, keeping the cursor on a boundary.
328. With candidates, arrows, page keys, `<C-p>` and `<C-n>` choose rows, and history is on `<M-p>` and `<M-n>`.

**K14**

329. Under die-vim in normal state, a click past a line's end rests per `rest_pos`, on the last character. Core alone, and insert and visual state, can land on the newline.
330. `Ctx::set_cursor_rest` is public rather than a core rule on newlines. A panic in it logs and falls back to the clicked position.
331. A wrapped row's padding click lands on that row's last position.
332. A press ends visual state or a waiting operator by running the keymap's `<Esc>`, in the old window and the clicked one, so `gv` still works. It bypasses key observers, so a macro doesn't record it.
333. A click in insert or replace state stays there and moves the cursor. A drag from either ends the session as one undo step, then selects.
334. A drag starts on leaving the pressed cell, and visual state stays after release.
335. A press on a status line or divider focuses and places nothing. K15 moved gutter clicks and clicks below the text onto the nearest text. A drag over those leaves the head.
336. A scroll goes to the window under the pointer without focus. Positive dy moves the view down, it stops with the last row on top, and dx is ignored.
337. A cursor left behind moves to the top or bottom visible row at its goal, so a scroll in visual state grows the selection.
338. The scroll remainder is one value across all windows and isn't reset on reversal, so it can be off by under a row.
339. With a prompt open, press and drag are ignored. Double clicks and modifiers are ignored throughout.

**K12, K13**

340. The old instance keeps the lock through the handover, against scope 5.9's release and retake, so there's no moment with no holder.
341. The wait for a first frame stays at 10 s, where scope 5.9 says 2 s. A hung build holds the old instance 10 s, but a slow launch isn't killed. A late new instance exits after 2 s on a held lock. Scope isn't updated.
342. A new instance restores under the old one's lock, and its writes stay paused until `resume_session`.
343. An unreadable file or dropped record is copied to `session.rejected-<secs>-<n>.json`, mode 0600, never cleaned up.
344. A restart is refused if the handover write fails.
345. A clean buffer whose file is gone is dropped with a message. A file record whose path is already open keeps its text in a buffer with no file.
346. `read_only` applies to every record, now `Option<bool>`, and a file uneditable now stays read-only. Turning it off on an already uneditable file stays off, as text-bearing records did.
347. The idle write's ceiling is 5 s from the first unwritten change, and a stale value there only causes an early write.
348. A restored cursor goes through `Ctx::rested`, so core gains no Vim rule.
349. With no tree and no focus, the last used restored buffer goes on screen, for older files and a gone focus too. A K12 test's setup changed for it.
350. A no-file record whose name matches a text buffer present before the restore replaces its text silently, and the extension keeps its id. Session text beats what `init` wrote.
351. A restart from a frontmost instance runs `open -n` without `-g`, passes `--frontmost`, and calls `focus_window` after settling. A failed restart takes focus back. Otherwise `-g`. Only the option choice is tested, and why both are needed is from reading.
352. Macros are saved as register text. A recording in progress isn't.
353. `.`, `@@`, `;`, `n` and `gv` aren't saved, since die-vim holds them and that would need a save hook. `/<Up>` and `@:` do work after a restore.
354. Visual state comes back. Insert and half-typed operators don't, but a cursor from Insert or Replace takes the leave-Insert step for the window used last only, using the raw saved cursor. It moves the cursor and edits nothing, so Escape's indent drop isn't applied. The `insert` flag clears, so such a session doesn't round-trip byte for byte.
355. `Ctx::set_insert_end` is public, beside `set_cursor_rest`, because Escape's position is the keymap's business.
356. Windows outside the split tree aren't saved.
357. An unreadable tree counts as a dropped record, and the focused buffer shows. An unreadable or non-list `views` skips silently, so a newer build's view can't cost an older one a buffer. A wrong `cursor` type still drops that buffer. Wrong-shaped `registers` give a message and keep the file.
358. Restored registers don't write to the clipboard, and restored `"` counts as known.
359. `other_states_do_not_come_back` now compares with live Escape, so there's no deviation row.

**K15**

360. Keys typed during a restart's handover are dropped, with input held and the frame unchanged, rather than handed across. No beep or message. This settles K12's lost-text window.
361. `if frontmost || shell.app_active() { take_focus }` at the first frame: only an already active app, so it can't take focus from another. Not exercised.
362. The speed proof is one ignored test with a hard 1 ms p99 and no retries, since you asked for one measurement.
363. "Every public item" means die-core, die-vim, die-render, die, die-dev. `missing_docs` is `warn` at crate roots and clippy's `-D warnings` fails it, so a concurrent `cargo check` doesn't break.
364. `:noh`, `:nohl` and `:nohlsearch` added though scope doesn't list them, because otherwise a highlight stays until the next search.

**F4**

365. Bundle id `dev.die.DIE`, and tests use `dev.die.DIE.test` with their own bundle and state directory.
366. Single instance and handover live in die-core, counted under `session`, per `docs/records/platforms.md` §5.2. `sys::state_dir` gains `$DIE_STATE_DIR`.
367. Socket at `<state dir>/handover-<pid>.sock` in a 0700 directory. A connection closing with nothing said means the new instance died, so the old stays up.
368. The old instance keeps lock and socket until exit, and the new one reads to EOF and then takes the lock on its own thread.
369. The lock applies only inside a .app. `restart` is registered only in DIE.app, plus `--restart-after-frame`.
370. App tests bundle dev, not release (about 40 s against 4 minutes). No test holds release to 500 ms. One release measurement in F4a was 458.6 ms.
371. A launch keeps R3's no-activation, except as K12 and K15 changed it.
372. The restart loop's digest is std's `DefaultHasher` over `session_json()`, no dependency, and the JSON is kept beside it for a diff.
373. DIE waits for a `restart-loop.go` file between digest and restart, a slight change to the plan row, so the driver can swap the build.
374. A rebuild touches `main.rs`, runs `cargo build`, re-signs and replaces the bundle, about 0.3 s a turn. The binary is probably the same, not compared.
375. The driver also checks the session still holds its seed, not in the row, since 20 equal digests of a lost session would pass. It caught a wrong seed on the first try.
376. The seed is hand-written JSON in xtask, with a real fingerprint. A schema change fails the loop rather than passing it.
377. Count is N+1 digests. On failure the state directory stays and is named. The loop isn't in `ci`.

**R4, R5, R6**

378. The key matrix posts CGEvents to its own pid, and `KeyMonitor::deliver_to`, compiled into shipping code, hands them to the hidden window. A key window receiving keys is left to the hand run.
379. Repeats are NSEvents via `postEvent:atStart:`, since macOS drops a posted repeat key-down. Cmd chords expect no key-up in the matrix.
380. `tests/keys.rs` is `harness = false`, re-runs itself with arguments, and joins the lint's allow-list. A `FAILED` static stops its exit 0 from hiding a failure.
381. On Linux, `KeyMonitor::install` returns Err and the window has no monitor. winit is assumed to deliver Caps Lock, not run.
382. die-render gets macOS-only dev-dependencies on `objc2-app-kit` and `objc2-core-graphics` at =0.3.2, and the NSRange feature of objc2-foundation. The Viewer hook swaps two methods on winit's view class, with `=0.30.13` keeping the method types right. A `keyDown:` counter separates typing from input-method text.
383. Cmd-Q follows `:q`: it refuses unsaved file buffers, even with a session that would keep them. Scope D18 only asks for a confirmation with running processes.
384. New test-facing `Shell::insert_text`, `Surface::insert_text`, `KeyMonitor::take_inserted`, because tests may not use `unsafe`.
385. The system clipboard is wired in `die::launch` only with no check script, so no test can write your clipboard. Linux stays in memory until its backend.
386. Kind goes under `dev.die.register-kind`, including macro. An unknown value reads Char. A clipboard manager that restores plain text gives Char.
387. The test's outside copy is the binary re-run as a child, a real second process, with no test-only write method.
388. `typing` posts NSEvents to the app's own queue, not CGEventPostToPid, so nothing leaves the process and it skips the window server. Its to-display figures aren't a real key's path.
389. `typing` times on a non-readable layer and restores a readable one after.
390. `first_shown` logs the display time but is written late, and the sorted-stages assertion leaves it out.
391. New check names beyond the row's: `redraw`, `idle-60`, `occlusion-20`, `firstframe-once`.
392. Window tests hold S1's limits x 4 (`ALLOWANCE`). Typing's p99 is on key to commit, not display, which is printed but not limited.

**K11b, K16**

393. The list draws over the windows and grows without shrinking while the prompt is open, as your vertico does.
394. Flex ignores case when the query has no capitals, a smartcase difference from Emacs's exact default, in the contract. Ties go to the candidate matching the query's case, then given order.
395. `<CR>` on an empty line answers the first candidate, as vertico does, except in `descends` prompts, where no row is chosen until `<Down>` or typing and `<CR>` answers with the line. `<M-CR>` always takes the typed text.
396. `\x` for the palette, unchecked against your Emacs for a clash. `:`, `/` and `?` get no candidates.
397. mono-dark's `prompt-current` copies vertico-current, and `prompt-match` copies completions-common-part. List drawing is in `prompt/draw.rs`. `base_and_ui_faces_inherit_from_their_parents` now expects 22 faces, not checked whether it's ported.
398. Only `descends` prompts move to the line above the first row. Vertico allows it wherever no match is required, but the contract and a test stop rows at the ends.
399. `<M-<>` follows vertico-first, so on a `descends` prompt at the first row it moves to the line.
400. `recent_first` isn't applied to find-file, whose `listing` keeps hidden entries last. `kill-buffer` doesn't `require_match`, since K11b3's test expects an error for an unknown name.
401. ` [Match required]` is drawn by the scene after the prompt's text, not written to `*messages*`.
402. `switch-buffer` keeps the current buffer last rather than removing it. I recall Emacs excluding it, not checked. It lists last-used first after history, and a new name makes a buffer as `switch-to-buffer` does.
403. `kill-buffer` puts the current buffer first. It asks only for a modified buffer with a file, and only a typed `yes` kills.
404. `*messages*` stays on offer. `buffer` is one shared history. `kill-buffer` gets no key.
405. Find-file puts the whole path on the line, with candidates for the part after the last `/`. A listing stays until the line names another directory, so a new file won't show. Non-UTF-8 names aren't offered.
406. Answering with a directory reopens the prompt there, since DIE has no dired, and adds to file history. `<M-CR>` on a directory name does the same and adds `/`.
407. A restart covers `//`, `/~/` and a final `/~`, as Emacs does. `~name` stays a plain name, not another user's home.
408. A new file under a missing directory asks `create? (y or n)`, with `y` or `yes` plus `<CR>`, no single-key prompt. Save itself doesn't make directories, against Emacs, because a ported test holds that and a save can't prompt. `y` makes it even if never saved. `:e` only gets a message.
409. `tests/keymaps.rs` (ported) binds its own `my-finder`, since core registers `find-file`. What it asserts is unchanged. `:find-file foo` ignores the text and opens the prompt.
410. describe-key's code is in `commands/describe.rs`. It answers in a read-only `*help*`, split below or an existing window, with focus restored. A prefix keeps reading, and only `Ctx::describe_key` can return `KeyAction::Prefix`.
411. `Surface::handles` is optional, default false, and is how a surface declares passthrough keys (§7).

**V2, V5, V7**

412. New S5 row 31, `search/not-found`, evil-gap: a failed `/` is an error, as the task required, while your isearch only says so.
413. New deviation kind `oracle-dies`, for cases with no fixture. `macros/failed-search-stops` holds the contract's rule, not an Evil result.
414. `*` and `#` on punctuation fail as your Evil does, with the rule in `Pattern::word`, and search tests in `die-core/tests/search.rs` changed for it. `*`'s next-word rule is from memory of `evil-find-thing`.
415. Empty matches follow Evil rather than a deviation, adding `TextBuf::empty_match_at` and `Target.to_last_line`.
416. `/` remembers a pattern that finds nothing. One that doesn't compile isn't remembered. `/<Esc>` cancels a waiting operator and forgets the goal.
417. `/` and `?` take `evil-command-line-map`'s keys, though your `/` is isearch per the review's reading of evil-search.el. `/<BS>` leaves the line, as in Vim, in scope §10. Isearch against `evil-search` for `/` remains your call, since K11a's review left it to V2 and V2 didn't decide it.
418. `:e` with no name fails `No file name`, where I recall yours reloading. It resolves from the buffer's directory, then from where DIE started, and expands `~/`.
419. `:b name` tries exact, then unique substring, otherwise an error, where yours asks. `:b` alone is the last other buffer, skipping `*messages*`. `:b N` isn't built.
420. `/$` and `n` pass over a match on the newline the cursor rests before. A huge count stops once a landing repeats and takes the rest modulo the cycle. Failure reads `Search failed: <pattern>`, with no "wrapped" message.
421. `:q` takes no argument, and `:w` with a path isn't built. `g` is the only `:s` flag.
422. `:` from a selection starts `'<,'>`, the last selection's whole lines. Other commands strip it or `%`, but `:'<,'>w` and `:'<,'>wq` refuse. `:%w` acts as `:w`. Bare `:%` and `:'<,'>` are silent no-ops, and `:'<,'>5` goes to line 5, neither checked against Evil.
423. `:q` from the last window refuses on any modified file buffer. On a split it closes without checking, as Evil does.
424. `:q!` clears modified flags rather than reloading, because a reload can fail. A file missing from disk restores empty.
425. `:s` stores its pattern even on no match, and clears the highlight, both from memory of Evil. With no earlier search `n` goes backward, which the oracle confirms.
426. 58 fixtures and 1,075 cases in the plan were stale, and it now says 69 and 1,410.
427. `u` after `:s` follows Evil, an Emacs undo artifact, as the third exception in scope §10. Multi-line matches were inferred from 9 probes.
428. The oracle mirror's `history-length` is t, so `@:` is held to real Evil. `pending.txt` stays generated, but any entry fails. A failing case must be fixed or become a deviation.

**V6**

429. Recentring is in die-vim, not core. `scroll::reveal` runs after every command, so `j` past the bottom recentres, as Emacs does with `scroll-conservatively` unset.
430. H M L and the scrolls go to the logical line's goal column, and when that's out of view the cursor jumps to the row above the window's middle, keeping the goal. This copies Evil's positions from fixtures, and that redisplay causes it is inferred.
431. C-e and C-y land a pushed cursor at the row's start. C-f, C-e and C-d never take the top or cursor to the empty row after a final newline. A C-e count past the end fails and doesn't scroll. C-f past the end goes to the last line.
432. `evil-scroll-up` isn't bound, because `C-u` is the prefix. Scrolls aren't bound with an operator waiting, so `d<C-d>` cancels, unchecked.
433. `Goal::Row` sits in the top bit of `Selection`'s goal, so core's goal stays `usize`. `app.rs` masks it, which means core knows one die-vim detail. A typed goal later would remove that. H M L and scrolls read it as a logical column, and only j and k recompute it.
434. `:new`, `:vne` and `C-w n` kept beyond the row, because Evil binds them together. `C-w f` and `C-w T` are left out.
435. Window commands are tried first in `ex::run`, and an all-digit line falls through to `:N`.
436. `C-w x`, two children of one split: `Tree::trade` moves cells only between them, where Emacs can shift others. Read from window.el, not run, and no test with 4 in a split.
437. `C-w x` across splits follows `window-swap-states`, whole or not at all per axis, without the preserve-size step, so a third column can shift in rare layouts. `Kid` and `Node` now derive Clone.
438. WindowIds move with the buffer in DIE, so focus after `C-w x` is the `to` window, at the original place.
439. `2<C-w>5s` gives 25, where Evil multiplies to 10, because the prefix can't tell a first digit from a later one. `<C-w>0` is a digit even with no count, as Evil's map binds it.
440. xtask's app test retries `open` on LaunchServices' -600, up to 5 tries. It was seen once, and I didn't find out what a real second launch does at that moment. It may show you an error if you launch DIE twice within a moment. Veto if you'd rather it stayed a failure until that's understood.

## 2026-11-05 (later): K4, K5, R2 and V3 merged, and every test binary has a memory limit

**Waiting on you: nothing.** You can overturn any call below at any time. The one check still open is R2's: comparing a rendered page with your Emacs at 16 pt needs Emacs's window on screen, so it waits for R3 or for your eye.

**Merged:**

| PR | What | Review found | Major | Fixed |
|---|---|---|---|---|
| #11 | The memory limit, and time limits for `mutate` | 19, over two reviews | 3 | 19 |
| #12 | K5, the runtime | 14 | 2 | 14 |
| #13 | K4, layout | 12 | 2 | 12 |
| #14 | R2, the offscreen renderer | 15 | 2 | 15 |
| #15 | V3, operators | 10 | 1 | 9, and 1 waits for K6's modified flag |

**A correction:** I told you the four packages' reviews found 49 problems, 48 fixed. It's 51, with 50 fixed and 1 deferred.

Two of the memory limit's findings were settled by saying so in the crate's docs rather than by code (calls 169 and 173). A separate review read only what merging the four packages together changed. It found 7 more, no majors, all done at the merge. The biggest was a panic in V3 under a Hide: `cc` on a line a fold joined to the one above panicked with the line already deleted. Its fuzz failed 326 of 6,000 runs, on V3's own tip too, and five seeds now give 0 of 6,000 each.

**Checks on the final `main`:** `cargo xtask ci` passed on a fresh clone at each of the five merges, and the fixtures regenerate byte for byte from your Evil. `ci --linux` passed twice on the memory limit's final commit. `mutate` on the final `main` was still running after 50 minutes, so I stopped waiting for it. K4's branch caught all 48 mutants, and a review confirmed each still applies on the merged tree. The whole run's result goes in the next entry.

**You can't use it yet.** `die` still prints `die: no window yet (M0 phase 0)` and exits 2. R2 draws a scene into an offscreen image, and the window is R3.

### What you have now

- **K5, the runtime.** Timers, idle timers, jobs on threads, waking the loop, config at startup, and panics that log a line and leave the editor running. `ctx.rs` is split into `ext.rs`, `dispatch.rs` and `rt.rs`, and the prompt has its own file.
- **K4, layout.** Word and character wrap, folds, tabs, wide clusters, the gutter and virtual text, all matching tty Emacs or differing as contract §9 says. A cursor position now costs the same on a line of any length. At normal priority, a keystroke on a 1 MB line takes at most 1 ms in every script tested.
- **R2, the renderer, offscreen.** Glyph atlas, row cache, instance building, the drawn box and block glyphs, and the Metal backend. Everything before the backend is data in and data out, so it also runs on Linux.
- **V3, operators.** Operators, text objects, registers, counts and undo. 86,000 random key sequences ran through your Evil and die-vim, with and without a final newline, and the differences are either named rows in S5 or listed below.

**Oracle:** 905 of 1,075 cases pass, up from 248 of 523. 58 deviations are excluded, up from 37. The 170 still pending belong to later packages: V2 16, V3 8, V4 67, V5 13, V6 66. V3's 8 are 5 clipboard cases for K6, 2 search cases for V2 and the `gu` rule for K6. Regenerating every fixture from your Evil changed nothing, and `~/.emacs.d` wasn't touched.

**Budget:**

| Crate | Lines |
|---|---|
| die-core | 5,450 of 14k |
| text and undo | 1,171 of 1,250 |
| layout | 888 of 950 |
| keys, keymaps, describe | 598 of 650 |
| ext, dispatch, rt | **1,498 of 1,500** |
| faces and scene | 902 of 950 |
| die-render | 1,781 of 7k (macOS build 1,717 of 5k, Linux build 1,258 of 5k) |
| `platform::macos` | 523 of 900 |
| platform stub | 64 of 70 (limit raised from 50, call 129) |
| die-vim | 3,095 of 5k |
| product code | 10,373 |
| xtask (dev) | 3,790 |
| die-test-cap (dev) | 187 |

`ext, dispatch, rt` has 2 lines left. K6 moves buffers and windows out of `ext.rs`, which gives it room back.

### Your machine running out of memory

You asked whether there was a leak. There wasn't one in DIE: nothing in a running editor grew. Agents were planting bugs in copies of the code to check that tests catch them, and some of those bugs made a test binary grow without end. The kernel log shows seven kills that day:

| Time | Binary | Size | Cause |
|---|---|---|---|
| 12:06 | `naive_layout` | 185 GB | A planted bug in K4's own check |
| 12:07, 12:10 | `timers` | 128 and 154 GB | Probably K5's planted timer bugs, inferred and not proven |
| 12:18–12:45 | `prop`, four times | 230–301 GB | A real bug in K5's test path: a list of panics carried over between test cases, so shrinking never ended. Fixed in 57e29f8 |

At 13:13 swap ran out, and the kernel paused Steam, Firefox and Emacs. The likely cause is a planted bug in K5's review, also not proven. You force-quit eight apps at 13:24.

You asked why the test binaries didn't have limits of their own, and now they do:
- **The count.** Every test binary's allocator counts live memory. Past 4 GiB it writes a line starting `die-test-cap:` and the process aborts. The largest real test needs 160 MiB.
- **The kernel.** On macOS, each binary also runs itself under `taskpolicy -m 8192`, and the kernel kills it at 8 GiB, whatever is growing: Metal buffers, thread stacks, `mmap`, or the freed blocks malloc keeps. I'd said macOS couldn't enforce a process limit, because `ulimit` can't, and the first design followed that. The first review showed I was wrong.
- **`cargo xtask lint`** fails a test binary without the limit, and fails the crate if it's anything but a dev-dependency. `die` has none of its symbols.
- **`cargo xtask mutate`** gives each mutant a time limit, and reports which limit it hit.

**What this doesn't cover:** the limit is per binary, not per machine. Eight agents at once could still reach 64 GiB, which is your RAM. Linux has only the count. Processes a test spawns, such as cargo and Emacs, aren't limited. The crate's docs give the full list.

**One side effect.** Each aborted test binary leaves a crash report in `~/Library/Logs/DiagnosticReports`: 111 of the 282 there come from the limit's own tests. They take no focus, and I've left them alone.

### Speed tests at background priority

macOS moves this session between normal priority (PRI 31) and background (PRI 4) by itself, I think according to whether its terminal is in use, though I haven't confirmed that. The same frame takes 0.27 ms at normal priority and 0.94 ms at background. So scene's 1 ms test failed 40–44 of 100 runs on `main`, and `cargo xtask ci` often failed for no real reason. Limits in ms now allow 4 times as much at background priority, and in `ci --linux`'s VM (call 179). I didn't raise any priority, because that would compete with what you're doing.

### Rare differences from your Evil

S5 says this log lists these. V3's differential found them, each measured against real Evil, and none is a named deviation. They're corners of how Emacs parses strings or restores the cursor.

1. `I<Esc>u`: Evil's `I` puts point on the undo list, so `u` only moves the cursor. `xI<Esc>u` on `a|bcd` keeps `acd`, and die-vim undoes the `x`. Only `I` with no typed text does this. It belongs with row 6.
2. `<Esc>` at a wrapped row's start: Evil doesn't step back under visual-line-mode. Pending for V6.
3. A count on `$` landing on an empty line, with a shift from mid-line, is an Evil bug. `>2$` on `ab|c\n\nde\n` gives `ab  c` in Evil, and die-vim shifts the line.
4. `daW` from an empty first line with a whitespace-only line next: Evil takes both lines, die-vim only the newline.
5. Read-only text: a failed `O` goes to the line's start and a failed `o` to its end in Evil. die-vim stays.
6. Strings that never close (row 24) still differ where the odd quote comes from an apostrophe in a comment.

### Web content goes to a real browser

You said it's "probably better just to defer to an actual web browser for rendering web content", and asked about `.html` and `.md` files. So DIE won't embed web pages or render them into pixel buffers. It edits `.md` and `.html` as text, and a browser renders them. That matches the preview you have now, which is export to HTML plus WebKit, and a browser brings back and forward with it. The preview becomes an extension under D14, after M0.

### API changes, for your later review

**K5, die-core.** 14 added, nothing removed, no signature changed, and every public path through `die_core::` stays the same.
- **Added:** `Config`, `Clock` (with `Clock::real`, `Clock::now`, and a default that's fake, at zero), `Waker` (with `Waker::wake`), `Ctx::{with_config, now, next_timer, idle, waker, set_waker}`, `Registry::mode_keymap`.
- **Behaviour with the same signature:**
  - `Ctx::new` is `with_config` with an empty config. The first `Ctx` installs a process-wide panic hook, chained to the one already there.
  - A panic is logged to `messages` as well as returned, and its line reads `ext panicked: msg at file:line. Its state may be half-updated: restart if it misbehaves.` A line already among the panic lines at the log's end, up to 32 back, isn't repeated.
  - Panics in a surface's `event`, key observers, a job's `work` and any `Drop` of what core holds no longer escape. A surface's `event` runs guarded, with `nesting()` at 1 and `cur_ext()` its owner.
  - A failing key's command, a key nothing handles, or a prompt answer submitted by a key that fails drops the rest of the fed keys. `feed_keys` from inside a call goes ahead of what's left.
  - `advance` runs what's queued at the old time first, stops at each deadline, waits for a blocked job at most once per move, and its clock can't go back. A repeat that fell a period behind skips what it missed, and `every(0)` runs as 1 ms.
  - `step` returns false once what was ready has run, even with a repeat overdue, and gives keys a turn between batches of timers.
  - `run_until_idle` logs a message after 1,000,000 turns instead of panicking. `shutdown` also cancels what config owns.

**K4, die-core.**
- **Changed:** `DisplayLine`'s fields are private. `TextBuf::display_line` returns a value, not an `Rc`. `ViewRow` loses `cells`, gains `number`, and most of its fields are private. `Cursor` gains `past_end`. `Deco::Hide` and `Deco::Virtual` take `Styled`. `LineNumbers` gains `Visual`. `Settings::wrap` is a `Wrap`, not a bool.
- **Added:** `DisplayLine::{start, end, lines, eof, stops}`, `TextBuf::{scroll, set_scroll}`, `ViewRow::stops`, `Styled`, `Wrap`.
- **Removed:** `CellKind` and `LayoutCell` (now `pub(crate)`), and `DisplayLine::{is_stop, stop_le}`.
- **Behaviour with the same signature:** the default wrap is `Wrap::Word`. A full row's line end stays on its row, at column `text_w`. The empty last row has no number. Control characters take 2 or 4 cells. `pos_at_vcol` can land on a later line of a fold. There's a new core command, `toggle-max-width`.

**R2.** die-core: `CursorShape`'s docs only. die-render's new public items: modules `atlas`, `driver`, `font`, `frame`, `rows` and `sprite`, plus `metal::Pipeline`, `Gpu::{pipeline, instance_layout}`, `text::Shaped` and `Face::{fallback_for, shape}`. `FaceMetrics` moved to `font` and gained `x_height`.

**V3, die-vim's grammar.** Adds `Operator`, `Start.op`, `Target.kind`, `Target.no_column` and `char_key`. `run_motion` now uses `kind`, and hands off to `operators::on_motion` when an operator is waiting. Nothing removed.

**Ported tests changed**, all by K4, and none by K5, R2 or V3:
- `tests/layout.rs`, `tabs_and_wide_characters_have_correct_display_columns`: `pos_at_vcol(0,10)` goes from 3 to 6, and `pos_at_vcol(0,5)` from 1 to 2 (LAYOUT-4).
- `tests/scene.rs`, `one_window_snapshot`: the empty last row loses its `4` (calls 3 and 24, LAYOUT-3).
- `tests/scene.rs`, `status_line_prompt_and_surface_text_are_laid_out_like_buffer_text`: `a?b` becomes `a^Ib` (call 102).
- `tests/prop.rs`, S4's oracle: its line-end branch is gone and its cursor-column filter changed (WRAP-2). `Op::Set` picks from 3 wraps and 4 number styles, and `Wrap::Word` reads its rows from the naive layout.
- Others needed only to follow the API, with no asserted value changed.

**One thing weaker than it looks.** K4's builder read the naive layout before writing K4, because its first steps said to. So no disagreement in 40,000 sequences is weaker evidence than two blind implementations would be. tty Emacs's fixtures and §9's worked examples are the independent checks.

**Test plans:** [records/wave1-test-plans.md](records/wave1-test-plans.md).

### Calls (veto any)

Each package's own, as its builder and finisher made them. A number below 54 is an earlier entry's call.

**K5, the runtime**

54. The prompt moved to prompt.rs (K11a's budget module) as a pure move. The three planned files hit 1,501 of 1,500, and K11a rebuilds that code there anyway. Buffers, windows and registers stay in ext.rs until K6, as budget.toml now says.
55. The timer heap is a BTreeMap keyed by (deadline, sequence), not a BinaryHeap. Same order and same log n, with no Ord impl to write. Veto if you want the literal heap, at about 12 more lines.
56. Clock is an enum (Real, Fake), not a trait, and the default is fake, so S4's tests stay byte for byte. F3 must pass Clock::real(), or timers never fire. advance on a real clock only runs until idle.
57. advance now stops at each deadline. S4's jumped to the end. A callback reads its own time, and a retry chain runs inside one advance. This is what lets a repeat skip missed firings without breaking S4's '3 ticks in 35 ms' test.
58. A repeat that fell a whole period behind fires once and skips the rest, so a laptop's sleep can't queue hours of firings. Emacs instead caps catch-up at timer-max-repeats, 10. The shortest period is 1 ms.
59. idle(d, f) arrived now, though the plan's K5 row doesn't name it: contract §12 lists it and K12's idle write needs it. It fires once per idle period, counts from when it's set, and any key event restarts it, typed or fed, press or release. K9 must also restart it for pointer input.
60. Besides errors and panics, a key nothing handles also drops the fed keys, as Emacs's undefined does in a keyboard macro. Callback errors drop nothing, and neither does an error a command handles itself. A panic in a key observer doesn't stop the key. The contract was silent on all of this, and §6 now says it.
61. Panics in render and decorate are logged at the next turn, through the hook, and name only the place, not the extension. Naming it means editing scene.rs, which this package was told to leave alone.
62. A line identical to the one before it isn't logged again, so a panic on every frame can't flood the log. So a repeating panic with the same message shows as one line until something else is logged. *Replaced by call 78.*
63. The panic hook's list is per thread, and a new Ctx clears it. A panic on a thread an extension started itself reaches only the standard error stream, not messages.
64. spawn keeps one thread per job. The contract said a thread pool, and I changed the contract instead of building one, since nothing has measured a need.
65. Config has public fields and closures, not a builder. Its commands are owned by 'config' for describe-command and panics. A config command reusing an extension's name is a startup error, like any duplicate. Declared overrides of core commands wait for K11b.
66. run_until_idle gives up after 1,000,000 turns with a message and leaves the work queued. It used to panic.
67. set_waker can connect only once; a second call returns an error. There's no public post from other threads yet, since spawn is the channel's only user until M1's process.
68. A surface's panics are attributed to whichever extension called create_surface, or 'core' if the host did. A Drop panic during kill is attributed to 'core', with the place in the source.
69. One edit to text.rs, against the brief's caution: map_pos's two identical branches joined, because dropping text's clippy allow needs it. It doesn't change behaviour.
70. K5R-1 goes further than the review's fix. `step` also returns false after a batch of timers, even with a repeat overdue, so a host draws and reads its events. The review's version would still spin with no key waiting. So false now means 'what was ready has run', not 'nothing is due', and a host must ask `next_timer` before sleeping.
71. Timers and keys alternate under a slow repeat, in a macro too. One cost: a timer a callback sets for right now runs after a key that's already waiting. S4 ran it first. `advance` and `run_until_idle` still run such chains to the end.
72. K5R-2's hand-written fixture for a failed `/` in a macro is deferred to V2. The oracle's batch run dies on that case, die-vim has no `/` until V2, and tests/oracle.rs accepts a hand-written fixture only for a deviation, which this isn't. So V2 has to make room for this kind of case. Core's half is tested in die-core now. Veto if you want the harness changed now, though V3 also edits that file.
73. `<Esc>` in a prompt counts as a key, so a cancel answer that returns Err also ends a macro. A prompt cancelled by a new prompt doesn't.
74. Keys fed from any call, including a timer's, go ahead of what's left. So a timer that feeds keys mid-macro lands in the middle of it. Nothing does that today, and I didn't check it against Emacs.
75. I added a second oracle case beyond the review's ask, macros/error-in-nested-macro-ends-both. Your Evil gives `one`, with the error after key 2.
76. An idle timer due at the same moment as another timer goes after it, rather than tracking set order across the two. The contract says so.
77. An idle wait counts from when a key's command or keyed prompt answer ended. Other work queued after a key, such as a hook, doesn't restart it. The restart moved to the end of the key's turn, so a command that runs a nested loop could see an idle timer fire inside it.
78. The panic log's rule is 'already among the panic lines at the log's end, up to 32 back', not the review's 'last 8 lines of messages'. Any other message makes a panic news again. Past 32 objects panicking in turn, the log still grows until M1's disabling.
79. K5R-7 guards each drop site rather than wrapping `collect_due`, because the wrap misses a queued call's closure and could lose timers already collected. These panics name core, not the extension, as a `Drop` during `kill` already did. This also covers `next_timer` and the sweep, which the review left as best effort.
80. Config's handles are cancelled last, not in reverse startup order, because an extension's `shutdown` can run a config command that sets a timer. The review's reason, that config applied last, would have pointed the other way. Handles the host itself detached still aren't cancelled by `shutdown`.
81. K5R-9 is fixed rather than documented: a callback may call `advance`, and the clock ends at the later of the two.
82. One new test takes 5 s of real time, advance_waits_once_for_a_job_thats_out, because the job wait is a fixed 5 s. I didn't make the wait configurable just for a test.
83. I shortened three lines of `collect_timers`, with no change in behaviour, to leave room for K4's 4 lines in `Ctx::with_config`. K6 moving buffers and windows out of ext.rs gives the module its real room back.

**K4, layout**

84. Newline's cell at col == text_w: it isn't drawn, in a centered window or any other. A selection doesn't highlight past a full row, so a highlight doesn't depend on whether padding happens to exist. The cursor still draws in the padding, and a click there still lands on the newline. §9 says so.
85. TextBuf::line_end_on is replaced by a pub(crate) ViewRow::line_end, which the viewport already knows.
86. Cursor::past_end: col is the row's last cell. A bar goes at that cell's right edge; other shapes draw on the cell as usual. The snapshot prints `past-end`.
87. Ambiguity 1 (break point, 'that item'): reading A. tty Emacs confirms it: ` bbbbbbbbbbbb` gives `·|bbbbbbbbbb|bb`, and `\tbbbbbbbbbbbb` and `aaaaaaaaaa\tbbbbb` agree. §9's sentence is rewritten, with worked examples added.
88. Ambiguity 2 (a control character's rest): reading A. tty Emacs confirms it: `aaaaaaaaa^A` + 11 b gives `aaaaaaaaa^|Abbbbbbbbb|bb`. §9 was at fault: 'or control character' is removed, and the opposite is stated with an example.
89. Ambiguity 3 (a number wider than its gutter): keep the last digits, so the text doesn't move. Added to §9. This is DIE's own rule: no Emacs evidence.
90. Ambiguity 4 (undrawn cells of an oversize cluster): they count toward tab stops. §9 adds 'drawn or not'. tty Emacs composes no cluster that wide, so there's no evidence, as with call 27.
91. Ambiguity 5 (the empty last row with virtual text on several rows): no row of that display line has a number. §9 reworded.
92. Ambiguity 6 (a hidden last line): it keeps its number, because the rule goes by where the display line starts, not what shows. §9 says so.
93. Ambiguity 7 ('the fold line if there is one'): only the hidden range the cursor is in counts. §9 reworded.
94. Ambiguity 8 (gutter size): hidden lines count, so folding the end of a file doesn't move the text. §9 adds 'hidden or not', and a test now pins it.
95. Ambiguity 9: 'never start a row' read literally contradicted the empty-row rules. §9 now says 'a line end never moves to a new row'.
96. Ambiguity 10: §9's number table now states its text and 12 cells, and the fold example says 'with a replacement or without one'.
97. Ambiguity 11: a Hide showing a placement is M3's. §9 now says so; Deco::Hide has no placement field yet.
98. §9 gains a sentence: `posn-at-point` reports a wide character that moved whole at the column it left, but the terminal's own cursor is drawn on its new row. I checked this by capturing the real cursor through term.el. It explains probe output; layout doesn't differ from Emacs here.
99. Styled: a cluster takes the last run holding its first byte. Runs don't stack, unlike stored faces on text.
100. Virtual text is styled as the position it stands before, with its own face over that, and still maps to no position. A click on it is K14's question.
101. toggle-max-width turns the width off, then back on at the width it had, or 80 if it never had one. It leaves `center` alone. The remembered width is a private TextBuf field, not in Settings, so K12's session doesn't save it unless K12 adds that.
102. Status lines, prompts and surface lines draw control characters as `^A` too (a surface's tab is `^I`), so 'laid out like buffer text' stays true. This goes past DR-12's wording.
103. Line-number face: with Visual, only the cursor's row is 'current'. With Absolute and Relative, the cursor's display line is, including a later logical line of a fold.
104. A Face decoration no longer empties the layout cache or runs normalize_views; only Hide and Virtual do.
105. The 1 MB line's budget, which the plan doesn't state: 1 ms per keystroke at either end, in every wrap mode, and at most 100 ms for a first full layout.
106. Code placement for the budget: per-window views in windows.rs, hidden ranges and display-line edges in text/hidden.rs. Cursor-position queries stay in layout.rs.
107. LAYOUT-4: die-vim's pos_at_col now delegates to core and loses its filter to the logical line, as the verifier asked. On a fold it can land on a later line of the display line.
108. K4-8, reverses call 29 for Visual: on a fold that joins lines, the cursor's row shows the line the cursor is on. With lines a to d and a joined to c, the cursor on c shows 3, 1, as tty Emacs does. It costs one line of code and keeps scope §8's 'Visual matches Emacs' true. Veto it and §9 names one more difference.
109. K4-8, keeps call 29 for Relative: the cursor's display line still shows its first line's number, so that case shows 1, 1. Emacs shows 2, 1, but its 2 is a count of hidden lines, and call 4 already deviates from those. §9 gives the example.
110. K4-8, whole text hidden: Emacs draws no number, and DIE's only row keeps one. In Visual that's the cursor's line, so `a\nb` all hidden shows 2. §9 names it as a difference. I didn't add code to follow Emacs in a case this rare.
111. K4-6: the 1 MB budget now holds for every script tested, with the limits unchanged at 1 ms a key and 100 ms for a first layout. I took neither of the review's options, looser limits or one kept GraphemeCursor. A cluster inside a rope chunk now takes no rope walk, in about 10 more lines.
112. K4-5: a block selects a replacement by its position's column, or not at all, as a tab's cells work. So a block whose columns start past that column takes none of it, even over cells it visually crosses. No Emacs or Vim evidence checked; it follows scene-4's 'one style' and the tab rule.
113. K4-3: I added a test with fixed texts beside the review's 5-case property, because 5 random cases could let a mutant through by chance. It asserts that a chunk's edge really falls inside a cluster, so it can't pass with nothing to test.
114. K4-7: the excuse count is exactly 10, not the 11 the review proposed. The excuse now never applies to a tab.

**R2, the renderer**

115. Cursor pixels are logical. Bar and Underline's '2 px' and Hollow's '1 px' are points, so 4 and 2 physical px at 2x, and 2 and 1 at 1x. Emacs on macOS draws `(bar . 2)` in points, and the rustdoc names Evil's `(bar . 2)`. CursorShape's rustdoc just says px, and I left that die-core text alone.
116. On a wide character, Block, Underline, HalfBlock and Hollow cover its cells, and Bar sits at its left edge. A cursor on the right half takes the character's first cell. The rustdoc says only 'the cell'.
117. A block cursor also redraws the cell's underline and strike in the cell's background, so they stay visible. A color emoji under it keeps its colors, since it can't be tinted.
118. HalfBlock is half the cell's height rounded down, at the bottom, as Evil's half cursor is.
119. The baseline stays S1's fractional ascent, not a rounded one. A font's own box glyphs tile only that way (a per-range override), and text comes out byte-identical to S1's. The cost is a baseline row that is 88 % covered, not 100 %.
120. Renderer-drawn double lines leave their joins open, as the font's do: ╬ is four corners and ╦ has an unbroken top rail. Arcs are a quarter circle with a radius of about half a cell, plus straight runs.
121. A double underline sits at the underline's position and two line-widths below it, where S1 straddled that position. Both keep it inside the cell. The strike sits at half the x-height, where S1 used 11/20 of the cell, so FaceMetrics gains x_height.
122. I built the plan §4 cut-list items anyway, since the budget has room: renderer-drawn braille (off by default), dashed and diagonal box glyphs. Powerline too, as scope §5.11 lists it.
123. Rules look at a cluster's first character, and the first matching rule wins. The renderer draws single characters only, so a box glyph with a mark comes from the font. A rule giving a range to the renderer falls through to the font for anything it doesn't draw.
124. A face pinned with Draw::File counts as a fallback: squeezed to fit and centered. Your braille pin is Draw::Primary, which isn't affected. *Now `Draw::Files`, call 137.*
125. A cluster stays in one face where it can: the first in the chain with all its characters. Otherwise the system picks a face for the whole cluster, not base and mark from different fonts.
126. Marks from the primary font aren't squeezed. x + U+20DD (an enclosing circle, which Term has) runs about 9 px past each side of its cell, onto its neighbours. Stacks of marks can reach into the row above. Only fallback glyphs are squeezed to the cell. A fallback glyph too tall for the cell is now squeezed too; S1 only centered it.
127. The atlas is fixed at 2048 px gray and 1024 px color. When full, it starts over at most once a frame and every row rebuilds. A frame needing more than a whole atlas (over about 2,500 distinct glyphs on screen) draws what fits and tries again next frame, so such a page could flicker. A bitmap bigger than its plane draws nothing. *Its "could flicker" is replaced by call 141.*
128. A cluster with both color and gray runs goes in the color plane, and its gray run draws white, since that plane can't be tinted. This should be rare.
129. The stub's budget limit goes from 50 to 70 (65 used). Each item the driver names needs one in the stub, so R3 will raise it again.
130. Metal aborts the process on a texture size it can't make, so Pipeline::render and pipeline return an error for 0 or above 16384 per side. This M5 allows 32768, but I used the lower side I recalled for older Apple GPUs, without checking Apple's table. The driver returns an empty Image for a target with no pixels.
131. A frame has three explicit steps (diff, supply, build) rather than a rasterizer closure or trait, to hold call 15's 'data in, data out' literally.
132. `#![warn(missing_docs)]` goes on each neutral die-render module, not on the crate. `cargo xtask guards` plants an undocumented `pub fn` in platform/macos/text.rs and needs clippy to pass on it. Public items in platform have rustdoc by hand.
133. tests/platform.rs's R1 test metal_device_and_queue still fails on a Mac with no Metal device, on purpose, so the fidelity skips can't hide a broken backend on your Mac. A hosted Mac runner without Metal would fail that one test.
134. The fidelity tests write PNGs only when DIE_RENDER_PNG_DIR is set, with a small uncompressed PNG writer in the test file, so png isn't added as a dev-dependency.
135. A cursor the color of its cell's background (mono-dark's current search match) changes color. Bar, Underline, HalfBlock and Hollow draw in the cell's foreground. Block fills with the foreground and keeps its glyph in the cell's background. If the foreground is that color too, it uses the scene's background, then black, then white. This follows my memory and the reviewer's of Emacs's `x_set_cursor_gc` in xterm.c, whose source isn't on this machine. From memory only, the macOS port keeps the cursor's fill and draws the glyph in the frame's background, so your Emacs may show the letter readable but no distinct cursor on a match. I couldn't check that without opening its window. I chose a visible cursor. The cost: under a block cursor, a one-letter match looks like plain text. Veto if you'd rather match the macOS port.
136. A block cursor's redrawn glyph is cut to the cursor's cells, including a wide character's two. Overrunning ink outside those cells keeps its normal color, so x + U+20DD's ring stays visible past the block.
137. A range rule pins any number of font files, tried in order, then the styled face, then regular: `Draw::Files(Vec<PathBuf>)`. R2's call about pinned faces counting as fallbacks now applies to all of them.
138. A primary glyph is squeezed only past `span * (cell.w + 1)`. So 16 pt at 1x draws at full size on a 9 px pitch, where it used to be 6 % smaller. Its 9.6 px advance runs up to 0.6 px over the cell, so letters sit slightly closer. 16 pt at 2x is unchanged on S1's page.
139. Term's 288 wide characters with a one-cell advance (☰ ⚡ ⭐ ⌚ and others) are centered across their two cells, with a fractional pen, as fallback glyphs have.
140. A fallback glyph squeezes by its ink's width, not its right edge, then moves back into its span if its ink leaves by more than half a pixel. So a glyph that fits but sits off-center now moves rather than shrinks, and a lone mark like U+0E33 stays in its cell. The half pixel keeps an italic's small overhang where the font put it.
141. A page with more glyphs than the atlas holds keeps its holes, stable, and tries again only when a row changes. So it doesn't flicker and costs nothing while idle or when only the cursor moves. But each change on such a page re-rasterizes the whole page in that frame: 183 ms on the reviewer's 3,896-glyph page in the test profile. This replaces R2's 'could flicker' wording. M3's eviction ends it. It is rare, needing over about 2,500 distinct glyphs on screen.
142. `Face::fallback_for` stays in the driver and the stub, against the reviewer's first option, because it keeps clusters in one face (159 of 716 tested clusters differ without it).

**V3, operators**

143. Runner: until K6, a kill's clipboard expectations are checked against the kill ring. Without this nearly every yank and delete case stays pending, but a case can now pass with no clipboard behind it. Cases about the clipboard itself (id contains clipboard, keys use "+, or the case presets the clipboard) stay pending. Veto if you'd rather keep them all pending until K6.
144. Runner: a register your Evil doesn't list must keep its preset value. This is stricter than before.
145. Rule 1 extended: J on the last line keeps the final newline, where your Evil's takes it. A count past the last line doesn't reach the empty line after it, so 2D on the last line from column 0 takes the line's text as characters, where your Evil's takes the whole line. Five new fixtures: J-last-line, 2D-last-line, d5j-goes-as-far-as-it-can, dap-before, dap-last-paragraph.
146. New deviation, S5 row 23: only a motion, a text object or the operator's own key can follow an operator. dA, dI and similar cancel quietly, as contract §7 says; your Evil runs them as motions. Vim also does nothing there, but beeps.
147. New deviation, S5 row 24: a string that never closes isn't a string, so di" fails. Your Evil deletes from the quote or the character before it, or fails, depending on earlier text. Inside a " string that never closes, i' and i` still look in that string.
148. New deviation, S5 row 25: "x lasts one command even if it fails. Your Evil keeps it after a failed h, l or f.
149. Undo model extended, S5 row 6: each command is its own step. Emacs joins a run of ~, J, r or g~ into one, and die-vim doesn't, as in Vim. After J from an empty line onto an indented one, u goes back where it started; your Evil's lands a character on.
150. S5 row 22 extended: h, l, x, X, s, r and ~ take a whole grapheme, as in Vim. Your Evil's split a letter from its mark.
151. % is rewritten to Emacs's scan, which changes V1's normal-state %. Strings are skipped, any closer can be the partner (so { ] pairs), it looks only on this line for the bracket, and in a string with nothing left it retries from the string's end.
152. f, F, t and T now take Tab as a tab character in normal state too, as your Evil does. r<Tab> replaces with a tab.
153. p with a count over 1 MiB of text fails with Count too large, following call 46. Your Evil would just do it. A large register with no count pastes as normal.
154. A count typed before "x is dropped, as your Evil does: 2"a3yw yanks 3 words. Vim multiplies.
155. gU and gu as a buffer's first change follow your Evil, and u goes to the change's start. Known limit: the test is rev() == 0, so after an edit that was undone (ifoo<Esc>uguuu) die-vim returns to where the command started and your Evil goes to the start. *The rule is "the first change since the buffer was last unmodified". Deferred to K6's modified flag.*
156. Not bound, so they cancel an operator: _ + - (not in scope §10's motion list), the v, V and C-v type modifiers (dvj), and is, as, it, at, io, ao (sentences, tags and symbols aren't in scope §10). Your Evil has all of them. Veto if you use d_ or dit.
157. Brackets and J's no-space rule know only ( [ { ) ] } and, for i< and %, < >. Emacs's table also gives some Unicode brackets paren syntax. *Call 166 corrects this: `%` doesn't know `<` and `>`.*
158. A count on J that would make a line over 1 MiB fails with Count too large and changes nothing, following call 46 and p. Joining two lines, with J or 2J, goes ahead at any size.
159. J is one transaction of several small changes, not the single replace the review proposed. So rev() rises by one per join, and whatever sits between joins stays on its text.
160. A failed operator on read-only text or a read-only register leaves the cursor at the range's start, except y and >, and J goes to the next line's start. This follows Evil as measured, and replaces the review's proposal to restore the cursor. Vim stays put. Veto if you'd rather a failed command never moved the cursor. The oracle has no read-only cases, so these numbers live in a plain test.
161. Quote objects now port Evil's parsers, and they keep Evil's odd results because D4 says Evil wins. da' in "don't work" takes through the closing quote and leaves "don. i' there takes t work. da' with no whitespace anywhere in the string, or the text, runs to its end. Vim differs on all of these. Veto if you'd rather die-vim fail there, as row 24 does for strings that never close.
162. Row 24 stays, with one edge moved: di' on an unclosed "a'b' now takes b. Your Evil takes a'b. It failed before, and neither result is much use.
163. I on a line of nothing but spaces follows your Evil, inserting before the last space, so V1's behaviour changes. Vim inserts at the end of the line.
164. $ as an operator's motion, right after a vertical motion, sets the goal column the operator goes by, as in your Evil. gg>$ and <Down>d5$ end at the line's end. l>$ keeps its column.
165. A count on i( that gets no further than the string it started from fails, as in your Evil, and then looks for the count-th next opener. 2di( on f("ab") fails, while 2di( on f("ab", c) empties the pair.
166. Correction to the V3 report's call on brackets: % doesn't know < and >. Only i< and a< do.

**The memory limit**

167. Test binaries carry two limits, a count at 4 GiB and the kernel's at 8 GiB, which is twice the count, so the line comes first even while a `realloc` holds both copies. `set_limit` doesn't change the kernel's. A test that needs more than 8 GiB must raise the constant.
168. A second place for `unsafe` outside DIE's own code: `tools/test-cap/src/cap.rs`, about 65 lines (scope §12). A global allocator can't be written without it. It's never linked into DIE.
169. Two environment variables, where the design said none. `DIE_TEST_CAP_UNDER_LIMIT` is the mark that stops the re-run looping. `DIE_TEST_CAP_SYSTEM_LIMIT_MIB` can only lower the kernel's limit, so the tests can prove it at 128 MiB. Neither can raise the count. Setting the mark by hand skips the kernel's limit, and a binary that runs itself with a plain `exec` loses it. No test does either.
170. 64 counters on their own cache lines, where the design said one. One cost +49 to +55% on the scene test, and this costs about +4%. The count can pass the limit by up to 4 MiB. A fence and a second read on the rare path keep two threads from both getting the last room, and keep a healthy process from being refused.
171. A block counts for its size plus a 16-byte header, rounded up to 16, or for its alignment if that's larger, so a leak of one-byte boxes can't reach 16 times the limit.
172. A thread that loses the race to write the line waits up to 1 s for the winner, so its abort can't end the process before the line is out.
173. If `exec` of `taskpolicy` itself fails, the binary carries on with the count alone and SIGPIPE at its default. Only a sandbox that denies that program has done this. If `taskpolicy` runs and then fails, the binary ends with its message. I chose a refusal that says why over a limit that's quietly off.
174. `mutate` treats a kernel kill, an abort with no line, and a failure that names no test as hard errors. The old `<- failed` catch-all is gone. Each mutant gets 5 times the unmutated run, at least 120 s. The first run gets 30 minutes. A small `sh` watchdog in each run's group kills the group if xtask goes away, since `process_group(0)` means ^C no longer reaches cargo.
175. Every target but a build script must carry the line, including `test = false` targets and examples. Cargo can run all of those by name.
176. `set_limit` can raise the limit to anything, and no lint watches who calls it. Only the limit's own tests do today. Veto if you want raises reviewed.
177. The race test for call 170's fence never fails sound code, but it often misses a missing fence: 0 of 6 runs at background priority. The evidence for the fence is the reviewer's probe, up to 13,542 of 200,000 rounds before and 0 of 300,000 after.
178. `ci --linux` runs the limit's tests too, since the count is all Linux has.

**Made at the merge**

179. Limits in ms allow 4 times as much at background priority, or when `ci --linux` sets `DIE_SLOW_HOST`. The test asks `ps` only after a miss, so a healthy run spawns nothing. A limit on a ratio doesn't change. At background priority 4 ms is about 1.1 ms at normal speed, so the check keeps its strength. The factor is measured on macOS, and is a guess on Linux at nice 10 or more. A real regression under 4 times can pass at background priority, but it fails at normal priority.
180. K4's two naive-layout properties default to 2,000 cases, not 10,000. They took about 8 minutes of each `ci` at background priority, and `DIE_PROP_CASES=40000` still runs the deep check, which K4 passed. So after changing layout, run that by hand. Veto if you'd rather have the longer `ci`.
181. `MAX_REPEAT` and `MAX_REPLAY` stay at 1 MiB and 10,000. K4 says they can now rise, to about 16 MiB and 100,000 edits, but no one has measured memory there and nothing needs it yet. Their comments and scope §10 say so.
182. Indentation in die-vim now comes from the text alone, not from `display_col`, so a Hide can't change it. Indentation a Hide covers still counts. From memory, Emacs's `current-indentation` skips invisible text, so it could differ there. I haven't checked, and the oracle has no Hides.
183. The 8 wrapped cases K4 passes now run at their own window sizes, ahead of K7, so something holds K4's wrap to your Evil.
184. Test plans go in a record of their own, because together they'd double this entry.

### Notes for the next packages

- **Every package:** a new `tests/*.rs` file starts with `use die_test_cap as _;`, and the lint fails one that doesn't.
- **K6:** `windows.rs` exists, with the views in it. Move the buffers and windows blocks out of `ext.rs`, which is at 1,498 of 1,500. Change `registers.rs` alone for the clipboard, then replace the stand-in in `tests/oracle.rs`'s `run_in_window` and the "no clipboard yet" check in `run_case`, which should pass 5 cases. Replace `t.rev() == 0` in `operators::recase` with the modified flag, which should clear `undo/gU-after-an-undone-edit-then-u`.
- **K7:** `advance` and `run_until_idle` wait for jobs, up to 5 s each. Use typed keys for the per-key error log, because `feed_keys` drops at the first error.
- **K9:** the loop is `while ctx.step() {}`, and `next_timer` may already be past when it ends, so don't sleep then. A call that keeps queueing another needs a turn limit. Call `restart_idle` for pointer input. `run_until_idle` blocks on jobs, so the host's needs another name. With a clock in core, the plan's `now` parameter may not be needed.
- **K11a:** all of `prompt.rs`. Keep `Queued.keyed`: it's set only from a key's Enter or Esc.
- **K12:** `max_width_off` isn't in `Settings`, so a session doesn't save it unless K12 adds that.
- **R3:** `Pipeline::render` makes a new target and waits for the GPU each frame, and the SAFETY comments on `fill_instances` and `upload` rely on that wait. A drawable path without it needs S1's ring of buffers. `Renderer` has no `set_scale`. The target is BGRA8Unorm with gamma-space blending, so the layer must match. The stub's limit rises with each new item the driver names.
- **F3:** pass `Clock::real()`, or timers never fire.
- **R6:** a throwaway probe at 159×49 cells gave 8.79 to 9.42 ms a frame, including a new 24 MB texture, the GPU wait and the readback, which a window won't do. These aren't R6's check.
- **V2:** bind `/ ? n N * #` in `OperatorPending` too. Take `search/d-slash` and `search/not-found`, which are still tagged V3. A failed `/` must return `Err` from its answer. `tests/oracle.rs` accepts a hand-written fixture only for a deviation, so make room for the failed-`/`-in-a-macro case, whose oracle run dies.
- **V4:** `@` should call `feed_keys` from the command. `OBJECTS` and `Operator` are ready for visual state. `paste` treats `Block` as characters. `.` isn't recorded yet.
- **V5:** Evil's `:s` with no match doesn't stop a macro, so log it and return `Ok`.
- **V6:** `H M L gj gk` added to `MOTIONS` take the operator path automatically, so they must check `Start.op` where they land differently. `wrapped/esc-at-a-wrapped-rows-start` is on its list.

### How the work changes from here

You said this was far too slow. I measured the last 27 agents' transcripts: 23.6 agent-hours and 5.1M output tokens.

- **Few agents were running.** One agent writes about 150 tokens a second when it's generating, which is 43% of its time. The rest it waits on tools. Wave 1 ended by 14:30, and I started nothing new until after 21:00, because I'd made the merges wait for the memory limit, then ran them one at a time. For hours there were only 1 to 3 agents running.
- **Checking took more than building.** Review and finishing took 56% of agent time. Hand-planted bugs alone took 19%, more than `cargo xtask ci` and `cargo test` together (15%).
- **A small dev tool got a product's treatment.** The memory limit is 187 lines, and it took 5 hours and two reviews.
- **Everything ran at background priority.** 466 of 658 processes are at PRI 4 while Moonlight runs at 97, which looks like macOS Game Mode, though I haven't confirmed that. A frame takes 3.6 times as long there.

So these calls change how packages run, and you can veto any of them:

185. **Packages run from their dependencies, with no waves.** A package starts as soon as what it needs has landed, and a queue in the workflow merges each one when it's ready. I don't gate merges, and each package's merge writes its own entry in this log.
186. **One time-boxed review per package, and no second review.** Fixes before merging are for majors and for minors that change behaviour. Nits go on a list. Dev tooling gets no review, only its tests and `ci --full`.
187. **No hand-planted bug sweeps.** A reviewer may plant at most 5 bugs, in the code that matters most. `mutants.toml` stays as the record.
188. **Agents test the crate they're changing, with low property counts,** and run `ci` once at the end. `ci` skips `guards` and `clean-build` unless a manifest or xtask changed, and runs properties at 2,000 cases. `ci --full` runs everything, at each checkpoint. `mutate --changed` runs only the mutants on changed files.
189. **K6 and K9 are one package, and R3 and F3 are another.** R3 starts now against K5's seam, not after K9, so the first window comes sooner.
190. **The tests themselves all stay:** the oracle, the properties, the Emacs fixtures, tests first, and `ci` before every merge. What drops is how often they rerun and how much is re-verified by hand. The risk is that a rare bug reaches `main` and gets caught later, by a checkpoint or by you.

**Next:** T0 (the faster checks), K6 with K9, R3 with F3, K10b and V4 all start now. Then K7, K11a, K14, R4 and F4a, then V2. That makes the Reader. Then the Editor.

## 2026-11-05: the reviews you deferred, and the calls I made on them

**Your instruction, 3 November:** "for all the decisions waiting on me I am happy to defer to your own judgement. If you need me to test certain things, find ways you can test independently."

So until you say otherwise:
- 09-B's wait for your approval is off. API and grammar changes merge when `cargo xtask ci` passes and an independent review's findings are fixed, and this log lists each one for you to veto.
- Checks that need your hands become tests I run. They post events to DIE's own window and never touch your desktop's focus or input.

**Waiting on you: nothing.** The deviations review and S1's key check, listed below as waiting on you, are both closed. You can overturn any call in this entry at any time.

**Merged:** PRs #6–#9: this entry and its record, then the three fix branches below. `cargo xtask ci` passes on a fresh clone at each merge. `mutate` catches 26 of 26, `ci --linux` passes on aarch64, and the oracle's fixtures regenerate byte for byte from your Evil.

**The reviews.** Four reviews stood in for your read, each checked by a second agent trying to refute it. The findings are in [records/deferred-reviews.md](records/deferred-reviews.md).

| Review | Findings | Major |
|---|---|---|
| Deviations | 14 | 4 |
| Text and keys | 27 | 3 |
| Scene and `unsafe` | 21 | 3 |
| Deep read of undo, dispatch and the grammar core | 20 | 3 |
| **Total** | **82** | **13** |

None was refuted. The second agent revised the proposed fix on 32 of them.

**The fix round.** 66 of the 69 findings due now are fixed. The other three (WRAP-2, LAYOUT-3 and dev-4) are done in the contract, and K4 builds their code by call. The other 13 findings wait for the packages that rebuild their code (below). Each branch had a fixer, an independent reviewer who reverted the fixes one at a time to see tests fail, and a finisher. The first attempt ran into the weekly usage limit on 3 November, about 10 minutes in, so I restarted it today from its partial scene edits.

| PR | Branch | Findings | The review then found | Biggest change |
|---|---|---|---|---|
| #7 | core | 26 | 15 more (no majors), all but one fixed; the last was a merge step, done in #9 | `u` restores the cursor from before the step's first change. Contract §9's wrap, row and gutter rules, whole, checked against about 90 tty Emacs cases |
| #8 | scene | 19 | 5 more (1 major: another route round the `unsafe` lint), all fixed | Composing a dense 300×100 frame with every search match and 11,039 faces fell from 99.5 ms to 0.89 ms. A plain frame costs about 12–15 % more |
| #9 | die-vim | 24 | 6 more (1 major: a regression in word classes), all fixed | Every deviation has a hand-written fixture, written before its code: 37 cases under 10 rules |

**Oracle:** 248 of 523 cases pass, and V1's pending list is empty. The count grew from 346 for two reasons. The reviews added 159 cases, 116 of them motions (the goal column and words across scripts). And all 37 deviations now run against hand-written fixtures, where 18 used to sit out.

**Budget:**

| Crate | Lines |
|---|---|
| die-core | 4,665 of 14k |
| text and undo | 1,120 of 1,250 |
| keys, keymaps, describe | 598 of 650 |
| faces and scene | 900 of 950 |
| die-render | 123 (macOS build 120, Linux build 19) |
| `platform::macos` | 104 of 900, down from 1,600 (call 16) |
| die-vim | 1,332 of 5k |
| product code | 6,167 |
| xtask (dev) | 3,480, up from 3,035: the `unsafe` lint and `sources` step |

**API changes, for your later review.**
- **Added:** `Ctx::state_of`, `Ctx::set_state_of`, `Pattern::word`, `Cell::new`, `WindowPlacement` (replaces `Placement`), `Style::{bold_is, italic_is, strike_is, underline_color}`, and in die-render `platform::backend::ready()`.
- **Changed:** `State` gains `Replace`, `CursorShape` gains `Underline`, `Key` gains `Unidentified`, `KeyEvent` gains `num_lock`, `Decoration` gains `seq`, and `Cell` is `#[non_exhaustive]`. `add_marker` returns `Result<MarkerId>`, `remove_marker` returns `Option<usize>`, and `set_sticky` returns `Option<()>`.
- **Behaviour with the same signature:** an undefined key clears the prefix. Empty changes make no revision or undo step. Edits between units are their own undo step. Undo and redo leave an open group open. `undo` can return `UndoConflict`. `C-[`, `C-i` and `C-m` fold. A multi-character cluster can't be bound. `printable()` is `None` with Ctrl, Alt or Super held.
- **die-vim's grammar:** adds `Target::short(pos, err)` and `forget_goal(t, w)`. A `Fail` from a motion now clears the goal column.

**S1's key check is partly done.** You pressed 13 of the 50 chords on real hardware, and every field mapped correctly. `results/interactive-keys.md` reports 7 failures, but all 7 fail only the modifier-event check, because Ctrl stayed held from `C-a` through `C-S-a`. The event log shows it. R4 covers the rest with an automated test.

### Calls (veto any)

The IDs are the findings in the record. Where a finding's verifier gave a better fix, that one is followed unless a call says otherwise.

**Layout, wrapping and rows.** Contract §9's text changes now, and K4 builds the code.

1. **WRAP-1: the wrap rule is Emacs's.** It follows `visual-line-mode` with `word-wrap`, including whitespace at a row's end and the cases the old text left open. With `Wrap::Char` the same rule applies with no break points.
2. **WRAP-2: an exactly full line gets no blank row after it,** as in Emacs. Rows, `H M L`, `gj gk` and scrolling then match Evil. The only visible cost is that insert state's bar sits at a full row's right edge.
3. **dev-4: the empty line after a final newline keeps its own screen row,** with no line number, as in Emacs. Evil's window positions stay oracle cases.
4. **LAYOUT-3: the gutter rules get written down,** including `LineNumbers::Visual`, the style you use.
   - `max_width` counts text cells only, so you get 80 columns of code. `visual-fill-column` includes the gutter, so this deviates from it. Your `.rs` files show no line numbers in Emacs, so rows still break where Emacs breaks them.
   - Relative numbers count display lines, as `j` does. Emacs's count hidden lines. It's listed in scope as a deliberate deviation.
5. **LAYOUT-5: `enum Wrap { Off, Word, Char }` replaces `wrap: bool`,** in K4. M2's copy state needs character wrap. If K4 runs over budget, it ships `Off` and `Word`, and `Char` comes with M2.
6. **scene-5: virtual text and `Hide` replacements become a styled string,** in K4: `Styled { text: String, faces: Vec<(Range<usize>, &'static str)> }`, with byte ranges. manifold's fold strings need faces, and contract §9's styled insertion shares the shape.

**Text, undo and keys.**

7. **DR-01: `u` puts the cursor back where it was before the step's first change,** as your Evil does. Core takes the selection just before that first edit, not at `begin_undo_group`, and die-vim's `u` uses it. This amends contract §9, the rustdoc and the A14 note. From now on, a die-vim command that moves and edits in one go (`p`, `J`, `dd` from mid-line) makes its edit first.
8. **DR-04: `Ctx::state_of(buf)` and `Ctx::set_state_of(buf, s)` are new.** `state()` and `set_state()` stay as sugar for the focused buffer, and die-vim uses the `_of` forms for `Args`' buffer. Without them, running `i` for an unfocused window leaves an undo group open for good.
9. **TEXT-5: text inserted at a cursor pushes only the acting window's cursor forward.** Other windows' cursors stay before it, as in Emacs. An extension that wants a window to follow its output sets that window's cursor.
10. **KEYS-5: `C-[` is Escape, `C-i` is Tab and `C-m` is Return,** folded in `chord()`, as in your GUI Emacs and in Vim. So `C-i` can't be bound apart from Tab. The raw event keeps the difference for M2's kitty passthrough.
11. **SEARCH-1: `*` and `#` follow Evil.** `Pattern::word` treats `_` as a non-word character, so `*` on `foo` matches inside `foo_bar`, as V1's `w` already stops at `_`.
12. **KEYS-4 and scene-6: `State::Replace` and `CursorShape::Underline` arrive now,** with rustdoc saying what each shape does to the glyph under it. Hollowing the cursor when the app loses focus waits for K9's `Input::Focus(bool)`. There's no blink in M0, which is listed as a deviation.
13. **scene-7: `Cell` gets `#[non_exhaustive]` and a public `Cell::new(glyph, fg, bg)`,** and the rule that producers resolve reverse, faint and invisible themselves. Overline, the link id and the cell frame for surfaces come with die-term in M2.
14. **faces-2: status lines have no box.** mono-dark's 1 px `:box` is the one attribute DIE's `Style` lacks, and it would take renderer work for one face. Dividers separate the windows. It's recorded in scope §8.
15. **unsafe-4: neutral code names a `platform::backend` alias,** with a stub on other platforms holding only what neutral code names, and no traits. R2's neutral code takes data in and gives data out, and only the driver calls the backend.
16. **unsafe-5: `platform::macos`'s ratchet drops from 1,600 lines to 900,** the size your `unsafe` review was planned for. Going over it forces the thin-wrapper question, with a count in hand.

**die-vim and the deviations.**

17. **dev-3: rule 1 means "Evil without the final newline",** for the cursor and for failures. It's what die-vim's `}` already does. Each rule-1 fixture comes from running the oracle on the text without its final newline, then putting the newline back.
18. **dev-8: inside a macro, `.` does what Vim does. After `@a`, `.` replays the macro,** as Evil does, without Evil's "No previous macro" error. An oracle case that tells the two models apart pins it.
19. **dev-10: three behaviours stay Evil's, though Vim differs:** `N%` goes N percent through the characters, `e` stops on empty lines, and Esc or a failed motion resets the goal column. They come from Evil's and Emacs's design, V1 already passes them, and moving them would add deviations for things rarely noticed.
20. **dev-12: all the remaining hand-written fixtures get written now,** once the rule texts settle, and go on the pending list. Expectations written before the code can't be fitted to it.
21. **dev-13: `:s` replacement templates are Rust's** (`$N`, `${N}`, `$name`, `$$`). So `$1x` names a group called `1x`, and a group followed by a letter needs `${1}x`. The rule says so.
22. **DR-11: `e` on "café" with a combining mark lands on the é, as in Vim,** mid-line and at a line's end. Evil's position comes from Emacs's command loop pushing point out of a composition, not from Evil's grammar. **This changes one of your fixtures:** `motions/combining-mark-e` becomes a deviation with a hand-written fixture.
23. **DR-18: the oracle runner's error checks wait for K7's per-key log.** Until then, DR-03's fix is pinned by a plain die-vim test and a die-core keymaps test.

**Assigned to later packages, with the calls that go with them.**

| Package | Findings | The call |
|---|---|---|
| K4 | LAYOUT-4, LAYOUT-5, LAYOUT-6, LAYOUT-7, DR-12, scene-4, scene-5 | `pos_at_vcol`, the scroll API, opaque `DisplayLine` and `LayoutCell`, control characters drawn as `^M` in two columns, and calls 5 and 6 |
| K7 | DR-18 | Call 23 |
| K9 | scene-2 | `Scene` becomes drawing-only, and the pointer map moves into `Ctx` as `position_at(row, col)`. `Input::Focus` arrives too |
| V3/V4 | KEYS-2, TEXT-3 | `@` types a register's text literally unless `q` recorded it, and core marks recorded registers. A block selection can reach each line's end |
| V5 | SEARCH-2 | `Pattern` gets captures for `:s` |
| R4 | KEYS-8 | The rest of S1's key check, as a test that posts layout-derived events to DIE's own window with `CGEventPostToPid` |

### More calls, from the fix round (veto any)

**Layout rules in contract §9.** K4 builds these.

24. **No number on any display line that starts at the end of the text,** so an empty buffer's only row has none either, as in tty Emacs. It still counts for other rows' distances.
25. **Filler cells don't count toward tab stops,** so display columns don't depend on the width. In `aaaaaaaaa中<Tab>b` at 10 cells, `b` lands at column 7 here and 6 in tty Emacs. It's named as a deliberate difference.
26. **Control characters split across rows the way a tab does,** unless they're a break point, when they move whole. U+0080 to U+009F draw as `\ooo` in four cells.
27. **A cluster wider than a whole row stays at the row's start and runs past its end.** This is DIE's own rule, because Emacs gave no evidence. Any cluster wider than the row reaches it.
28. **Replacement and virtual text break like text.** Where virtual text shows at a `Hide` comes from S4's code: before a replacement, not at all at the start of a `Hide` with no replacement, and shown at a `Hide`'s end.
29. **With a fold joining lines,** the cursor's row in Relative and Visual shows the display line's first line's number. Emacs may show point's own line, but no probe confirmed that.

**Core.**

30. **`state_of` on a dead buffer reads `Normal`, and `set_state_of` ignores it.** That's softer than C4. Veto if you'd rather have `Option` here too.
31. **A running command that moves the focus or opens a window goes on acting for that window,** and its nested callers keep their own. Outside a command, the focused window acts.
32. **A surface's `event()` no longer sees the count,** because DR-03 clears the prefix before calling it. Nothing needs it yet.
33. **`Decoration::seq` is public now,** so the naive layout can read the order. It's the one layout change ahead of K4.
34. **`LOG_CAP` stays a constant.** A8 recommended a setting, but nothing needs one.
35. **`Pattern::word` accepts empty text.** One known limit: with `ignore_case` and a literal whose folded forms differ in length, backward search can skip a match enclosing a rejected shorter one.

**Scene and the `unsafe` confinement.**

36. **`cargo xtask sources` is a new ci step.** Every file the compiler read must be one the lint reads, and a crate may compile only `.rs` files in its own directory. So the first `include_str!` of a non-`.rs` file, likely R2's shaders, has to name an exception there.
37. **The lint is stricter than the review asked.** It rejects `global_asm!` outside `platform::macos`, and any attribute that starts with a macro fragment, so the `$(#[$m:meta])*` idiom isn't available. The workspace has no `macro_rules!` today. It also rejects `NonNull::from(&mut x[0])` and `from_ref(&a[0])` in `platform::macos`. Known false positives: a format argument named `path`, and a variable named `restriction`. Rename to avoid them.
38. **Two limits stay.** A macro-built `allow(warnings)` goes unseen, though it can only hide warnings. And a proc-macro crate added as a workspace member could emit `unsafe` from a string. There's none today, and one would be a new member you'd see.
39. **`platform::backend::ready()` is the stub's one item,** so the cross-check can prove the alias.
40. **A cell past a row's end maps across the whole of `room`,** including a centered window's right margin. Rows below the buffer's last line stay `None`, for K14 to decide.
41. **A linewise selection draws the newline's cell on every selected line,** following mono-dark's region. Nobody checked what Vim draws there. A provider's `visible` range now includes a drawn newline, one byte more than before.
42. **xtask gains `serde_json`** to read cargo's messages. The oracle already pins it, so the lockfile adds no package.
43. **The speed tests get three tries each,** so a busy machine doesn't fail them. A return to the per-cell scan misses by far more than that.

**die-vim.**

44. **`$` and `%` are word characters,** so `foo$bar` is one word, as in your fundamental-mode Evil. V2 should pass die-vim's word test to `Pattern::word`, so `*` agrees.
45. **Word classes have known gaps.** `class()` still uses Rust's `is_alphanumeric`, not Emacs's word syntax. So marks like virama and Thai tone marks split words, and emoji count as punctuation, as on `main` before. One small regression is new: a mark outside the main blocks next to a letter of a different script now splits (`a` + U+0902 + `b c` with `w`: Evil 6, here 4). Fixing all of it means about 343 more table ranges, and I left it for text you rarely meet.
46. **A count past the caps fails with "Count too large",** keeping the session's first copy, where your Evil would freeze.
47. **A deviation with no hand-written fixture now fails the oracle test,** so none can sit silently out of the comparison.
48. **Insert-state Up and Down onto a tab or wide character land before it,** as your tty Evil does. Normal `j` and `k` land after it, as Evil's do. The two differ on purpose.
49. **`$j` on the last line no longer fails.** `j` and `k` with no line to go to signal only if the goal column wouldn't move the cursor, as in Evil. `gj`, `+` and `C-d` are unchanged, and V6 checks them.
50. **`f<Esc>` and a failed Left, Right, Backspace or `C-w` in insert state forget the goal column,** as Evil's do. `f<Esc>` comes from reading Evil's source, because a quit ends the oracle's macro. A plain die-vim test pins it.
51. **`f` or `t` forward from a line's last character says "End of line",** Evil's message. Messages aren't compared (call 23).
52. **Rule 1's macro fixtures take their registers from the run without the final newline.** With it, Evil's macro runs one more `x` on the empty line. This refines call 17's recipe.
53. **DR-11's rule is a new row in S5's table, row 22,** because D4 says each exception is listed there.

### Notes for the next packages

- **K4:** the code half of WRAP-2, LAYOUT-3 and dev-4 is listed in its plan row. Contract §9 disagrees with `layout.rs` on purpose until then. `snap` must cost the same whatever the line's length: today `normalize_views`, `snap` and `dl(pos)` lay out the whole logical line on every commit, and `set_selection` does it twice more. `MAX_REPEAT` and `MAX_REPLAY` can rise after that. K4 also decides whether the newline's cell still draws at `col == text_w` in a centered window, and keeps or replaces `TextBuf::line_end_on`.
- **K7:** state whether `run_until_idle` waits for outstanding jobs before the next key, as the oracle's runner now does. Separate errors from informational messages (call 23).
- **K9:** the positions rule moves with the pointer map, and so do `tests/scene.rs`'s position tests.
- **R2:** build on `platform::backend`. Shaders in their own file need an exception in `sources` (call 36).
- **V2:** pass die-vim's word test to `Pattern::word` (call 44).
- **V3:** bind `i<any>` and `a<any>` (contract §7), and `<Esc>` in `OperatorPending` to `vim-cancel`. Extend inclusive ranges to a grapheme's end. Operators go on after End of line or End of buffer and stop after any other signal, which isn't the same split as which failed motions keep the goal. Commands that move and edit make their edit first (call 7).
- **V6:** the `wrapped/*` cases need K4's word wrap as well as K7. Check `gj`, `+` and `C-d` against `j`'s goal-column rule (call 49).

**Next:** K4, K5 and R2, with V3's operators alongside them. V2 waits, because its search prompt needs K11a, which needs K6 and K9, which need K5. So V2 can't come before V3, as I said earlier. K4's naive layout gets written first, by a different agent, from contract §9's text alone.

## 2026-10-20: phase 0's first API merged; Linux joins ci

**Merged:** PRs #1–#5, which you approved. `main`'s tree is identical to the reviewed top of the stack. `cargo xtask ci` and `cargo xtask ci --linux` both pass on it.

How they landed:
- **#1 (K3)** merged into `main`.
- **#3 (K10a) and #5 (V1)** merged into their stacked bases, because I didn't wait for GitHub to retarget them. I then reopened #2 and #4 against `main`, and they carried #3 and #5 in:
  - **#2 (K2)** brought K10a;
  - **#4 (K8)** brought V1.

Nothing was lost, and the PR pages say which merge carried what.

**History rewritten, as you asked.** Every commit message and PR description no longer carries Claude Code attribution.
- `main` was force-pushed. Every tree is unchanged; only the messages differ.
- A bundle of every ref from before the rewrite is at `.git/backup-before-attribution-rewrite.bundle`, and git's `refs/original/` holds them too. Both are local only, and you can delete them once you're happy.
- No commit or PR will carry attribution from now on.

**Platforms (question 11, answered):**
- **Linux:** ships as a preview at M2's exit (A).
- **Windows:** out.
- **Approved:** the Linux std downloads and colima runs.
- **Declined:** asking Ghostty, and a CI-built engine. Where a signed Linux engine comes from stays open until before M2.
- The doc edits are on `main` (scope D1, D5, D10, D12, D13, D16, D17, D18 and D20, guard 7, the contract, the M0 plan and the records).

**New in `ci`:**
- the platform lint and the denylist for neutral crates;
- clean-build checks each target's graph against an allowlist;
- a Linux cross-check and clippy, for x86_64 and aarch64 gnu, with the pinned toolchain;
- guard 7's per-build budget.

`cargo xtask ci --linux` runs exit test (f) in a digest-pinned rust:1.96.0 container on colima:
- the tests, the oracle, and the Linux clean-build with an ELF NEEDED check;
- about 45 s warm.

It already caught one test that assumed a non-root user.

**Oracle:** 124 of 346 cases pass on macOS and on aarch64 Linux, and V1's pending list is empty.

**Budget:**

| Crate | Lines |
|---|---|
| die-core | 4,406 of 14k |
| die-render | 118 (macOS build 118, Linux build 12) |
| die-vim | 1,091 of 5k |
| product code | 5,662 |

**Waiting on you:** the deviations review (about 1 h) and S1's `interactive keys` check (5 min).

**Next, by the plan:** phase 1. K4 (layout), K5 (the extension machinery), K6 (windows), R2 (the offscreen renderer) and V2 (search), then checkpoint 1.

## 2026-10-19 (later): phase 0's first API, up for your approval

**Answered:** 09-B and 10-C, as recommended. The plan's doc edits are on `main`, and the private remote is github.com/0xDEADBEE7/die. You opted into workflows; the review below was the first.

**Waits on you: five stacked PRs.** Merge them in order. Each holds the contract edits that go with its code, so approving one approves those too.

| PR | Package | What it is |
|---|---|---|
| #1 | K3 | `KeyEvent` with S1's fields, one notation parser shared with the oracle, the prefix error, `unbind`, shadow lists, the leader from config. Contract §7 |
| #2 | K2 | Regex search, undo caps, selection restored on undo, `undo_boundary`, undo and redo return their range. Contract §9 and the A11/A14 notes |
| #3 | K10a | `open_file`: UTF-8 check, read-only reasons, BOM, fingerprint, per-path settings |
| #4 | K8 | Faces, themes (mono-dark in die-dev), the composed `Scene`, render-time highlights. Contract §10 and §13 |
| #5 | V1 | die-vim's grammar core, motions, counts, insert sessions, 44 new oracle cases. Scope §10 |

**How they were checked.**
- Four parallel agents built K2, K3, K8 and V1, and I built K10a.
- A review workflow then gave each branch an independent reviewer, and a second agent tried to refute each finding before fixing it with a test. The result: 59 findings, 13 of them major. 52 were confirmed and fixed, and 7 were deferred.
- Where the deferred findings went:
  - three contract wordings: into the PRs;
  - two xtask bugs: fixed on `main`;
  - K3-12, where the leader is set: to F3;
  - K10a-8, where per-path settings attach: to K5;
  - K8-7, pointer mapping: to K9;
  - V1-1: decided below.
- `cargo xtask ci` passes on every branch of the stack, and `mutate` catches 15 of 15 from K2 up.

**Calls I made under your standing instruction (veto any):**
- **V1-1:** Escape on the new line after a final newline leaves the cursor there, as Evil and Vim do. Scope §10's deviation now covers motions only.
- **K2:** search runs over a copied string, freed at the next edit. `regex-cursor`, a new crate, wasn't added.
- **K3:** `unbind` is a separate commit, so it can be dropped on its own.

**Oracle at the top of the stack:** 124 of 346 cases pass, and V1's pending list is empty.

**Budget at the top of the stack:**

| Crate or module | Lines |
|---|---|
| die-core | 4,405 of 14k |
| text and undo | 1,035 of 1,250 |
| keys | 524 of 650 |
| files | 168 of 650 |
| faces and scene | 820 of 950 |
| die-vim | 1,091 of 5k |
| product code | 5,659 |

**Platforms.** You reversed D12:
- macOS and Linux are required;
- other Unixes are welcome where they're cheap;
- Windows is in only if it isn't a considerable deviation.

A research workflow is drafting `docs/records/platforms.md` and question 11, on when Linux ships. Phase 0's code is platform-neutral except K10a's inode-based fingerprint, which is fine on Unix.

**Also waiting on you:** the deviations review (about 1 h; it now includes V1's six hand-written fixtures), and S1's `interactive keys` check (5 min).

## 2026-10-19: phase 0 starts

**Done, uncommitted:** the five packages the plan says can start under any answer to 09 and 10. `cargo xtask ci --worktree` passes every step: fmt, lint, budget, clippy, test, guards and clean-build. It takes about 40 s cold and 16 s warm.

| Package | What landed | Proven by |
|---|---|---|
| F1 Workspace | Edition 2024, resolver 3, rustc pinned to 1.96.0 in `rust-toolchain.toml`, exact version pins, a lockfile, `LICENSE-MIT` and `LICENSE-APACHE`. `unsafe_code` is forbidden workspace-wide; die-render denies it and allows it on `platform` only. die-core's `compile_error!` stops `panic = "abort"` builds | `cargo xtask guards`: the tree builds, an `unsafe` block planted in die-core or in die-render outside `platform` fails, and `-Cpanic=abort` fails with the guard's message |
| F2 xtask | `budget`, `lint`, `guards`, `api-diff`, `fonts`, `clean-build`, `mutate` and `ci` | Counter, lint and api-diff unit tests. `clean-build` passes, and fails when a build script calls `cc`, `clang` or `zig` |
| K1 Port S4 | S4's code and its 48 tests in `die-core`, as they were. Only the crate name changed, plus `DIE_PROP_CASES` and module-scoped clippy allows | All 48 pass, with the properties at 10,000 cases (8.4 s). `cargo xtask mutate` catches 8 of 8, through the same tests as in S4 |
| V0 Oracle | `tools/evil-oracle`, with S5's fixtures, schema, guards and scripts. A tranche classifier. die-vim's `tests/oracle.rs` runner on S4's `Ctx`, with `deviations.json` and `pending.txt` | 306 cases validate. 24 are deviations, and all 282 others are pending, so the test passes. A pending case that passes, a failure that isn't pending, or an unknown name each fails it |
| R1 die-render | S1's pinned stack, with AppKit and the other objc2 crates trimmed to the features named. `platform::{appkit, metal, text}`, the only `unsafe`, with thin Metal and Core Text wrappers | Platform tests: a Metal device, the font loads from its file, equal advances, missing glyphs. Budget 116 lines. `clean-build` passes |

**Waits on you:**

- **Questions 09 and 10.**
- **The deviations review (about 1 h):** `crates/die-vim/tests/oracle/deviations.json` holds 24 cases under 9 rules, all from your S5 answers. The note on `dk` and `dG` is a call: Vim's line, but Evil's column.
- **S1's `interactive keys` check (5 min)** before the Text and keys review.
- **These test plans.** You hadn't approved them before the work started, so read them as built (below).

**API:** die-core's public API is S4's as you accepted it (A1–A24), 196 items in `cargo xtask api-diff`. Nothing new.

**Budget** (`cargo xtask budget`):

| Crate | Lines |
|---|---|
| die-core | 3,060 of 14k. This matches S4's own count, which checks the counter |
| die-render | 116 of 5k |
| die-vim | 9 |
| xtask (dev) | 1.6k, against the plan's 0.7k |
| evil-oracle (dev) | 0.6k, against 0.3k |

The dev tools are uncapped, but they came in at about twice the estimate: api-diff and the tranche classifier are bigger than planned.

### Calls made while building (veto any)

- **`mutate` is an xtask command, not a script.** The planted bugs are data in `crates/die-core/tests/mutants.toml`. It runs in a copy of the tree, so a crash can't leave a mutant in your source, and it drops `mutate.sh`'s use of Python.
- **No C builders in the graph.** `clean-build` fails on a C-building crate (`cc`, `cmake`, `pkg-config`, `bindgen`, `zig`) in this Mac's build graph. One in the lockfile only, such as winit's `cc` for Android, prints a warning.
- **ci runs work in a copy.** `ci` clones HEAD by default. `ci --worktree` copies the working tree as git would commit it, for uncommitted work. Its build cache lives under `target/xtask/`.
- **What api-diff counts.** It treats every `pub` item in die-core's source as API, reachable or not, so it errs toward listing too much. Doc-only changes are listed apart. "die-vim's grammar module" means `crates/die-vim/src/grammar.rs` or `grammar/`, which don't exist yet.
- **What the oracle runner fails.** Until K7's harness lands, a case fails with a reason when the runner can't set it up or check it: visual start, clipboard, window top line, selection. An unbound key never counts as the error Evil signalled.
- **Tranches come from code.** The order is V1 < … < V6, and a case's tranche is the highest among its features. With deviations: V1 68, V2 16, V3 107, V4 59, V5 12, V6 44. The plan's first pass said 66, 16, 109, 59, 12, 44.
- **die-core's clippy allows are scoped to S4's modules** (`ctx`, `text`, `undo`). Fixing those lints would change accepted code, and the exact `if` lines two planted bugs target. K2 and K5 rebuild those modules and drop the allows.
- **The license names "The DIE authors"** as the copyright holder.

### Test plans, as built

- **F1** (`cargo xtask guards`): the unplanted tree checks clean; `unsafe {}` appended to die-core's `lib.rs` fails with "usage of an `unsafe` block"; the same in die-render's `lib.rs` fails; `-Cpanic=abort` on die-core fails with "DIE requires panic = \"unwind\"".
- **F2:**
  - `count`: `blank_lines_and_comments_dont_count`, `nested_block_comments`, `comment_markers_inside_strings_are_code`, `multi_line_strings_count_every_line`, `cfg_test_modules_and_items_dont_count`, `other_cfgs_count`, `lifetimes_and_char_literals`, `raw_identifiers_and_byte_strings`.
  - `lint`: `unsafe_allow_only_on_platform` (allowed on `pub mod platform` only, not in strings or comments, `forbid` is fine) and `pins` (`"1.0"` and `"^2"` flagged; `=`, path and workspace dependencies pass).
  - `api`: `lists_pub_items_only`, `bodies_and_docs_dont_change_signatures`, `derives_are_api`, `module_paths`.
  - `tests/clean_build.rs`: `a_crate_without_c_builds`, and `a_build_script_calling_a_c_compiler_fails` for `cc`, `clang` and `zig`.
- **K1:** S4's 48 tests unchanged (`c2_checkout` 5, `c3_queue` 4, `c4_subs` 4, `edits` 9, `keys` 8, `layout` 7, `prop` 3, `proto` 8) and the 8 mutants.
- **V0:**
  - S5's 8 fixture guards.
  - `keys_pick_the_tranche`: 20 key sequences plus a tty case.
  - `pending_file_format`.
  - die-vim's `oracle`.
- **R1:** `metal_device_and_queue`, `font_loads_from_its_file`, `monospace_advances`, `missing_glyphs_and_files`.
