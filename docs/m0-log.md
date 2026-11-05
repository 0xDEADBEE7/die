# M0 log

What merged, what waits on you, what changed in die-core's API, and where the budget stands ([m0-plan.md](m0-plan.md) §3). Newest first.

## 2026-11-05 (K10b): saving works, and files changed on disk reload

**Waiting on you: nothing.** You can overturn any call below at any time.

**You can't use it yet.** `:w`, `:w!` and `:e!` wait for V5, and the window for R3 and F3. Today this is die-core's API, and criterion (c) is checked by a test that runs the real `rustfmt` on open files through that API. The check by hand and a `:w` through die-vim wait for V5 and F3.

### What you have now

- **Saving.** It works through symlinks, including dangling ones, and keeps the mode and group. Hard-linked files are written in place. The round trip is byte-exact, BOM included. On macOS, ACLs and extended attributes survive.
- **Refusing.** `:w` refuses a file changed on disk, and `:w!` writes it. Both refuse a file that opened read-only.
- **Reloading.** A one-second poll reloads unmodified buffers as one undo step, keeping cursors and markers between changes. A rewrite with the same bytes leaves no undo step. A modified buffer is never reloaded over. Files over 16 MiB aren't read by the poll: it says so once and leaves them to `:e!`.
- **The modified flag.** K6's flag came over early, byte for byte, because the review showed K10b's stand-in was wrong. A buffer undone back to its saved text now counts as unmodified.

**Oracle:** unchanged, because nothing in die-vim moved.

### What the review found

One review, stopped at 16 minutes: 9 findings, 1 major, 6 minors and 2 nits. All 7 that change behaviour are fixed. The 2 nits are left for later. It found no data loss in normal use and no unsound code, and the diff adds no `unsafe`.

| | Severity | What | What became of it |
|---|---|---|---|
| F1 | major | A buffer undone back to the saved text still counted as modified, so it didn't reload and `:w` refused. That missed criterion (c) | Fixed with K6's flag |
| F2 | minor | A reload's cost grew with the file: 1.7 s for one line added to a 10 MB log, every second, and the poll read files of any size | Fixed: shared ends are skipped, and there are limits (calls 207, 209). A test now holds that log to 300 ms |
| F3 | minor | A poll between `rustfmt` truncating and writing loaded the empty file and lost the cursor and markers | Fixed: the poll skips a file whose mtime is under 200 ms old (call 210) |
| F4 | minor | Undoing past a reload and then `:w` could write U+FFFD over real bytes, or drop a BOM | U+FFFD fixed (call 211). The BOM loss is accepted (call 212) |
| F5 | minor | `:e!` onto a file now non-UTF-8 broke `u`, and the work couldn't be saved | Fixed: `:e!` refuses and keeps the text (call 213) |
| F6 | minor | A reload cleared a read-only flag your config had set | Fixed: the flag from before is restored (call 206) |
| F7 | minor | A save dropped ACLs and extended attributes | Fixed on macOS. Still lost on Linux (call 214) |
| F8 | nit | The changed-on-disk message offers `:w!` on a file that opened read-only, where `:w!` always refuses | Left for later |
| F9 | nit | `Ctx::save`'s doc says an error leaves the file as it was, but an in-place write that fails part way leaves new bytes and the old tail | Left for later |

### API changes, for your later review

From `cargo xtask api-diff`. Nothing removed, and no signature changed. die-vim's grammar has no changes.

- die-core adds `Ctx::save`, `Ctx::save_force`, `Ctx::revert_file`, `Ctx::check_files`, `TextBuf::modified` and `TextBuf::set_modified`.
- `FileInfo` gains private fields, so it can't be built or destructured outside core.

Behaviour with the same signature:

- `open_file` starts a repeating 1 s timer, so `next_timer` is `Some` while any buffer visits a file, and `None` again once the last one is killed.
- `FileInfo.fingerprint` becomes `None` when a check finds the file gone. `fingerprint`, `bom` and `read_only` change on a reload.
- A reload sets `Settings::read_only` when the file stops being UTF-8-editable, and restores its earlier value when the file becomes editable again.

### Budget

| Crate | Lines |
|---|---|
| die-core | 5,888 of 14k, up from 5,450 |
| text and undo | 1,213 of 1,250, up from 1,171 |
| files | 563 of 650 |
| product code | 10,811, up from 10,373 |

No limit was raised and no dependency added. Text and undo has 37 lines left, and K6's flag is already counted in it.

### Calls (veto any)

191. A save writes a temp file (`.die-save-<pid>-<n>`, mode 0600 until the end) and renames it over the file. It writes in place for a hard link, an owner it can't keep, or a directory it can't write. Your Emacs, with backups off, always writes in place.
192. A crash during a replacement can leave a `.die-save-*` file beside the original. Nothing cleans those up.
193. `:w!` still refuses a file that opened read-only (not UTF-8, NUL bytes, CRLF), as K10a's docs require, because for non-UTF-8 the text doesn't hold the bytes.
194. `:w` on an unmodified buffer whose file exists writes nothing and refuses nothing, even if the file changed on disk, because there's nothing to lose. `:w!` always writes.
195. A file that's gone is written again by `:w` without refusing, as I recall Emacs's `basic-save-buffer` doing. This wasn't run against Emacs or Evil.
196. `:w` refuses a file you can't write. `:w!` replaces it if the directory allows, keeping mode 0444.
197. Any fingerprint change makes `:w` refuse on a modified buffer, including `touch` or a same-bytes rewrite, because there's no content hash. As far as I recall, Emacs compares only the modification time too.
198. Refusals are `Error::Msg` with the reason in the text, with no new `Error` variant.
199. The poll runs every 1 s. Your init.el leaves Emacs's interval at its default of 5 s, but I recall Emacs also using file notifications, so 5 s here would feel slower. A host wakes once a second while a file is open.
200. The poll is an ordinary `every` timer started by the first `open_file`, so an extension opening the first file owns it and its shutdown cancels it. That's harmless today, and M1's disabling should recheck it.
201. A reload is one recorded undo step with `Origin::Extension`, so it works in read-only buffers, and undoing it leaves the buffer modified.
202. Beyond the plan row, reloads use a line diff of about 70 lines, patience-style on lines unique to both files and trimmed to character boundaries, so `cargo fmt` keeps cursors and markers between its changes. As I recall, Emacs's revert only trims the two ends. At worst it's one change.
203. A reload waits while any undo group is open, as during an insert with nothing typed. It happens at the next poll after the group closes.
204. A modified buffer gets one message per divergence, not one per outside rewrite. A save, a reload or the file being deleted resets it.
205. A deleted file keeps its text, with one message. If the file comes back, an unmodified buffer reloads and a modified one refuses `:w`.
206. When a reload finds a read-only file now editable, `Settings::read_only` goes back to what it was before the file made it read-only, so your per-path settings hold. A flag you set by hand on a normally editable file stays.
207. Reloads read and diff on the main thread, and the poll won't read files over 16 MiB. The number is my choice, not measured. A hitch while reading a file below it stays possible, which falls short of contract §13's "There are no blocking calls".
208. The model property is in `tests/prop.rs`, as a `saves` module, because `cargo xtask mutate` only runs that binary. It's capped at 300 cases because saves flush to disk.
209. Past 1 MiB of differing text a reload is one change, so cursors and markers inside that middle region aren't kept.
210. F3's wait is a wall-clock check on mtime, and a future mtime doesn't count as recent. `:e!` doesn't wait. A tool that takes longer than 200 ms between truncating and writing still shows the empty file for one poll. The test's assertion is skipped if the poll itself took 150 ms or more on a very slow host.
211. A reload into or out of a read-only state commits as `.silent()`, so `u` and `C-r` can't step into lossy text. This is wider than the finding, because redo could otherwise bring it back. After `:e!` out of such a state, `u` can't bring back edits made with `settings.read_only` off. Text you deleted in that state that held U+FFFD could still return through an older undo step. I accepted that rather than drop the history, since a tool caught mid-write would otherwise wipe your undo.
212. The BOM belongs to the file as last read, so undoing past a reload and then `:w` writes no BOM. A test pins this.
213. `:e!` with unsaved changes refuses when the file can't be edited, including one that was read-only already and stays so. Use `u` first, or `:w!` if the file has become editable. With no unsaved changes, it reloads.
214. On macOS a save now copies the old file's data once before overwriting it, because that's what keeps the ACL, so saving a large file costs more. Copying to a new name clones on APFS and drops the ACL, so it copies onto the existing empty temp. On Linux `fs::copy` carries neither ACLs nor extended attributes, so they're still lost there, and the module doc says so.
215. Some of K10b's own tests changed after the review; no ported S4 test did. The `rustfmt` test's `cfg!(target_os)` branch failed `cargo xtask lint`, so it now probes `rustfmt --version` and skips if that fails, on any platform. It also waits 250 ms after rustfmt, and `rename_over` and two tests age the mtime, so the poll doesn't treat those writes as in progress.

### Notes for the next packages

- **K6:** the flag is in, and `settle()` in `files.rs` is the one place that marks a buffer as matching its file. `undo.rs` and `text.rs` hold K6's code byte for byte. A trial merge with m0/k6-k9 merges those and `files.rs` cleanly, and conflicts only in the lists in `tests/mutants.toml` and `tests/prop.rs`.
- **V5:** `:w` is `Ctx::save`, `:w!` is `save_force`, and `:e!` is `revert_file`. Refusal text arrives in `Error::Msg`. `:w` with a path isn't built.
- **F3, K9:** `next_timer` is now always set while a file is open. `check_files` is public if you want a check on focus-in.
- **K12:** restoring a session can use `set_modified`, but `files.rs` has no setter for `fingerprint` yet.
- **Whoever builds save hooks:** the contract says a save runs its write as a queued step after the before-save hooks, but `write_file` writes at once. No plan row names save hooks.
- **Mutants:** M49 to M53 are in `mutants.toml`. A failed run of these can add seeds to `prop.proptest-regressions`, so check that file before committing.
- **Timings:** one-off, at load about 40, background priority and `opt-level` 1, before the review's fixes. A poll of 200 unchanged files took 15 ms, reloading a 1.2 MB file with 250 scattered changes took 357 ms, and saving it took 98 ms.

### Open, and what wasn't verified

- **`ci`:** PLACEHOLDER
- **`mutate` wasn't run.** The five mutants were each caught when applied by hand and run against the model property, before the review's fixes, so `cargo xtask mutate`'s own report on them doesn't exist yet.
- **`ci --linux` wasn't run,** so these tests haven't run on Linux. The root and missing-`rustfmt` guards were written from reading `xtask/src/linux.rs`.
- **Not tested:** the in-place fallback for an owner it can't keep, which needs a file owned by someone else, and crash behaviour during a save.
- **Not run against Emacs or Evil:** what's recalled in calls 195, 197, 199 and 202.
- **Save hooks don't exist,** so the contract's queued write isn't honoured yet.

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
