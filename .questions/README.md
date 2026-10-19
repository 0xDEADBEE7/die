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
- The font is Ioskeley Mono **Term**. You'll need to install it, because your current build draws `—`, `…` and arrows two cells wide. Ligatures stay off in v1.
- The window is borderless, like your Emacs frame. AeroSpace handles placement.
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
