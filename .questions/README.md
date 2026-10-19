# DIE: review questions (answered 2026-10-19)

These were the broad decisions left over from the 2026-10-18 review. **All of them were answered on 2026-10-19**, and the answers are folded into [docs/scope.md](../docs/scope.md) (decisions D4 and D8–D23, §12's plan) and [docs/extension-contract.md](../docs/extension-contract.md) (C2, C4, C9 amended; C14–C17 added). Each file keeps its explanation and your answer. The calls below were made within those answers, and you can still veto any of them.

| # | Question |
|---|---|
| [01](01-terminal-engine.md) | **Answered:** the terminal uses Ghostty's engine as a prebuilt binary, as ghostel does. |
| [02](02-plan-shape.md) | **Answered:** the restructured plan is approved: spikes first, API first, terminal before contract, restated budget. |
| [03](03-what-v1-is-for.md) | **Answered:** v1 is local first. The rest stays in Emacs until you build it. |
| [04](04-vim-fidelity.md) | **Answered:** die-vim matches your Evil setup. |
| [05](05-terminal-feel.md) | **Answered:** terminals feel like kitty and Ghostty, with a per-buffer toggle for vterm-style ESC. |
| [06](06-restart-guarantees.md) | **Answered:** everything survives a restart or a crash. |
| [07](07-license.md) | **Answered:** MIT OR Apache-2.0. |
| [08](08-who-and-pace.md) | **Answered:** you own the API, die-vim's grammar and the decision records; an AI drafts the rest under your review. |

## Calls made within those answers

**Toolchain rule (recorded as D16)**

- C compiled by Apple clang through `cc` is allowed but avoided, and flagged when it appears.
- A prebuilt binary is allowed when the upstream project's CI signed it, it's tied to a commit whose tests passed, and it's checked by checksum. That's how ghostel gets its engine. See [01](01-terminal-engine.md).
- Zig and other toolchains may run in CI only, never on your machine and never in `cargo build`.
- Dev-only tools that `cargo build` and `cargo test` don't need are fine.
- A clean-environment build check enforces the rule.

**Renderer and platform**

- winit (pinned), Metal through objc2, and Core Text, in their own `die-render` crate.
- No ratatui in core (D8). Its cell type can't hold underline styles, links or wide characters.
- The renderer draws box-drawing, block, braille and powerline glyphs itself so they join across cells, as kitty and Ghostty do. A per-range override keeps braille in Ioskeley Mono, as your init.el does.
- The font is Ioskeley Mono **Term**, which fixes `—`, `…` and arrows that your current build draws two cells wide. DIE loads it from the font files (S1), so it doesn't need installing. Ligatures stay off in v1.
- The window is borderless, like your Emacs frame. AeroSpace leaves borderless windows floating (S1 found), so your `aerospace.toml` gets an `on-window-detected` rule that tiles DIE.
- Option acts as Alt, and core bindings stay off Alt chords, which AeroSpace takes.
- No IME work, since your inputs are the Australian layout plus the Character Viewer.

**Core API**

- Edits carry an origin (`User` or `Extension`); read-only text blocks only `User` edits.
- Decoration providers return faces only in v1. Folds, hidden markup and inline images are stored decorations, and modes apply only to text buffers.
- Core holds generic modal states and a count/register slot; die-vim parses Vim syntax.
- Copy state is die-vim's grammar over a read-only snapshot, not a second motion engine in core.
- Calls that receive `Ctx` are queued, not nested. Modes pull edits with `edits_since`.
- Each terminal's output is parsed on its own thread, not the main thread. The main thread never waits on those threads.
- Prompts report cancellation, and subscriptions cancel when dropped.
- The contract's C-items are confirmed, with the C2, C4 and C9 fixes the Rust prototype found.

**M0 editing**

- A vertical completion list of about 20 rows, like your vertico, in core.
- `find-file` on a directory descends into it inside the prompt.
- No `:!`; use a terminal instead.
- The session file is JSON with a version field.
- Saves write through symlinks and keep file permissions.
- Auto-revert, plus a check for external changes before `:w`.

**Terminal engine and updates**

- The engine is libghostty-vt, using only Ghostty's signed builds. Updates are monthly, or sooner for crash and security fixes, and never more than three months behind.
- The Rust API is libghostty-rs, pinned and never forked. If it blocks an update for more than a month, DIE writes its own thin layer over the C API instead.
- Each terminal's engine runs in a small helper process, so an engine crash costs one terminal and scrollback and images survive every rebuild. S2 confirmed it's fast enough (within 0.5 ms of a thread inside DIE). See [06](06-restart-guarantees.md).

**Terminal identity**

- `TERM` is xterm-256color, which works over SSH without extra setup. `COLORTERM` is `truecolor` and `TERM_PROGRAM` is `DIE`.
- die-term gives its name as "DIE" to programs that ask (XTVERSION), never "ghostty". That keeps yazi on kitty's direct image path.
- An `xterm-ghostty` option is available for local use.

**Terminal defaults and shell**

- Remote programs can write your clipboard but not read it. Programs can't read local files through the image protocol.
- 10,000 lines of scrollback per terminal, as in vterm, capped at 64 MB.
- Images are capped at 64 MB per screen.
- bash starts as `bash --rcfile die.bash -i`. `die.bash` reads your usual profile and `~/.bashrc`, then adds prompt marks and directory tracking. This works on macOS's bash 3.2, unlike ghostel's method.
- After a reattach, die-term redraws by shrinking the terminal one row, waiting for the program to repaint (at most 50 ms), then restoring it. S3 found that an immediate change and change back doesn't repaint pico or top.
- A die-term API lets your config run fzf in a split, and turns termin into a config command.

**Launch and environment**

- PATH prepends and an env file are declared in config, mirroring `init.el`. Secrets are never logged.
- A script builds DIE.app pointing at the dev build, so restart runs the binary you just built.
- The startup hook gets a cold/restored flag, so restarts don't keep opening new terminals.

**Testing**

- Your colima VM serves as the Linux SSH host for terminal tests. The only extra tool is kitty's standalone `kitten` binary, which checks key encoding; it lives in a scratch directory, not installed system-wide.
- Vim keys that depend on window geometry come from a tty Emacs the Evil oracle drives (S5 found this works). Hand-written fixtures are only for deliberate deviations from Evil.
- I hand you DIE's own `unsafe` modules for review, kept small by design.

## M0 planning (2026-10-19)

You asked to plan M0 together before work starts. The plan is [docs/m0-plan.md](../docs/m0-plan.md): four dogfood checkpoints, your reviews, the budget, the work packages and the risks. Two questions set its direction, and **both were answered on 2026-10-19 as recommended.** Progress is in [docs/m0-log.md](../docs/m0-log.md).

| # | Question |
|---|---|
| [09](09-m0-ownership-and-time.md) | **Answered:** an AI drafts die-core's API and die-vim's grammar, and each change to either waits for your approval (B). |
| [10](10-remote-and-ci.md) | **Answered:** a private GitHub repository, with the checks run on your machine; hosted CI from M2 (C). |

### Calls I'll make within those answers

You can veto any of these. Four change something you accepted earlier, and say so.

**Process**

- M0 follows §12's four checkpoints as thin slices. S4's API is ported first, then each checkpoint cuts through every crate, and the spike-quality parts are rebuilt under the same tests while you use DIE. Veto if you'd rather nothing is built on a layer until you've reviewed it. Reading in DIE then moves to about weeks 7–9.
- The port of S4 merges as already accepted (A1–A24). Its tests land before the code under them changes. If one has to change, the next review says why.
- Four AI sessions work at once, each on its own branch: core's text side, core's runtime side, the renderer with the scene types, and die-vim.
- `cargo xtask api-diff` finds every public item a branch adds, removes or changes in die-core, by parsing its source, and every change to die-vim's grammar module. Nothing gets installed. Under 09-B, a branch with a non-empty list waits for you.
- The `unsafe` code is reviewed in pieces as it lands, finishing before checkpoint 2.
- A weekly note in `docs/m0-log.md` says what merged, what waits on you, what changed in the API, and the budget.
- Every problem you hit while dogfooding becomes a test first: an oracle case for die-vim, a harness test for core.

**Exit tests**

- (a) is judged as written: a day with no switch to Emacs to edit. More than 3 switches in a day is only an early warning.
- `restart-loop` checks state as well as time. After each restore, DIE writes a digest of its session state, and the driver checks that all 20 match. That covers macros and histories too.
- (b)'s second runs from `restart` to the new window's first frame. The build comes before it.

**Budget**

- Code lines are counted after rustfmt, excluding blank lines, comments, `tests/` and `#[cfg(test)]` modules, by a small counter in xtask, so nothing gets installed. rustc is pinned to 1.96 so the counts don't drift.
- die-vim gets a line in guard 7: about 5k (estimate 3.6–4.6k). It has no cut list, since §10 is its scope; an overrun means simplifying at the next review. *(Amends guard 7.)*
- Each module's limit starts at the top of its estimate and drops to the measured size after M0.
- On an overrun, the cuts come in this order. In core: candidate annotations in prompts, history search, per-pattern settings. In die-render: renderer-drawn braille, dashed and diagonal box glyphs, the latency log (it moves into test code).

**Core**

- Search matches, the visual selection and bracket highlights are computed at render time, so M0 needs no decoration index. The index moves to M1.
- Undo keeps up to 2,000 steps or 16 MB of text per buffer, whichever comes first.
- After `u`, die-vim puts the cursor where the undone change starts, as your Evil does (`ifoo<Esc>abar<Esc>u` on `x` ends on the `x`). Core's undo still restores the cursor from before the step (A14) and reports the range it changed, so the contract doesn't change.
- No horizontal scrolling in M0.
- Wrap, 80 columns centered, and line numbers are set per file pattern in die-dev until modes arrive in M1.
- Code files get visual line numbers, `.rs` included. Your Emacs shows none in `.rs` files only because it has no Rust mode.
- die-dev binds `\f \] \/ \x` as your init.el does, and `\z` toggles the 80-column width, as `visual-fill-column-mode` does. `\\` (your fzf from home) has no M0 equivalent; `\f` covers DIE's own tree.
- Pointer basics: a click focuses a window and places the cursor, a drag selects in visual state, and the trackpad scrolls.
- The status line shows the buffer's directory, like your mode line, plus a mark when the buffer is modified or its file changed on disk.
- `describe-key` and `describe-command` are in M0, after checkpoint 4.

**Files**

- Only UTF-8 files are editable. Other encodings, files with NUL bytes, and CRLF files open read-only with a message. A BOM and a missing final newline survive a save byte for byte.
- A save resolves symlinks, writes a temporary file beside the target, flushes it to disk, copies the permissions, and renames it into place. A hard-linked file is written in place instead.
- `:w` refuses if the file's modification time, size or inode changed since DIE last read or wrote it. `:w!` writes anyway, `:e!` reloads, and `:q!` quits without saving. *(Adds the three `!` forms to §10's ex commands.)*
- Auto-revert checks open files once a second and whenever DIE gains focus, with no file-watching dependency.
- An unmodified buffer reloads as one undoable edit that replaces only the span between the first and last changed lines, as Emacs's revert does, so cursors elsewhere stay put. A modified buffer is flagged "changed on disk". A deleted file is flagged, never closed.

**Session and restart**

- The session lives in `~/Library/Application Support/DIE/`. It's written about 1 s after the last change, and on restart and quit, through a temporary file and a rename. The previous copy is kept.
- It's never written before a restore finishes, so a new build that panics can't overwrite it.
- Unsaved text is stored only for modified or fileless buffers, with the file's fingerprint, so a restore notices a file that changed while DIE was down.
- Crash safety (files, cursors, unsaved text) arrives with checkpoint 3, and the full session (splits, registers, macros, histories) with checkpoint 4.
- `restart` copies the fresh build into `~/Applications/DIE.app`, renames it into place and launches it. *(Changes an earlier call, "A script builds DIE.app pointing at the dev build". S3 advised copying and renaming, so a build that's still being written is never launched.)*
- The old instance writes the session and releases its lock before launching the new one, then waits for the new window's first frame. If none comes within 2 s, the old one takes the lock back and keeps running, with an error message. A build that panics at startup costs you nothing.
- DIE.app runs a dev build with dependencies optimized, which keeps rebuilds short (S1 measured about 0.5 s after an edit to the renderer).

**die-vim**

- A pending list holds the cases still expected to fail. It must be empty at M0's exit.
- 24 cases are excluded by name, each with a hand-written fixture: 21 from the six Evil bugs and artifacts and the Vim-style undo you accepted in S5, 1 rewritten in Rust regex syntax, and 2 language indents (Python, Emacs Lisp) that wait for modes. That's exit test (d)'s list.
- Cases are grouped into tranches by the keys they use, not by topic file.
- Errors are checked by the key that raised them, not by Emacs's message text.
- Window and file commands get core harness tests, since the oracle has no window tree and no files. *(An exception to §12's "hand-written fixtures are only for the deliberate deviations".)*
- New cases with `_`, `::`, `->`, tabs and wide characters run in fundamental-mode, which is what your Emacs uses for `.rs` files, and die-vim follows what Evil does there. So `w` stops at the `_` in `snake_case`, as it does for you today. Veto if you'd rather `w` treat `_` as part of a word, as Vim does.

**Renderer**

- The fonts aren't committed until their license is checked. `cargo xtask fonts` fetches them by checksum into `fonts/`, which git ignores, DIE.app gets a copy, and the build itself never touches the network.
- `die_render::platform` holds only thin wrappers over AppKit, Metal and Core Text. The atlas, the row cache and instance building live outside it, which keeps the `unsafe` review to about 0.6–0.9k lines.
- The clean-build check looks for `cc` in the macOS build graph, not in the lockfile, where winit lists it for Android.

**Deferred** (the full list is in the plan's §7)

- The environment from config (PATH and the env file) and the startup hook's cold-or-restored flag move to M2. Nothing in M0 runs a shell or a terminal.
- No file-system watcher in M0, dark-coffee later, and no way yet to open a file in a running DIE from a vterm.
