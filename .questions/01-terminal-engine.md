# 01. Terminal: Ghostty's engine as a prebuilt binary

**Settled 2026-10-19:**
- DIE doesn't own or fork a terminal engine.
- Building DIE needs only cargo and the Xcode Command Line Tools.
- The target is the ghostel experience, including kitty graphics and the mouse.

**Question:** Is it OK for DIE to commit a prebuilt copy of Ghostty's terminal engine? It's what makes ghostel-level terminals possible, but neither you nor DIE can patch it locally.

## How ghostel does it

ghostel's terminal engine is libghostty, from Ghostty, written in Zig. You've never needed Zig because `ghostel-module-install.el` downloads a prebuilt binary from ghostel's GitHub releases. Zig runs only in ghostel's CI.

DIE does the same, with more checking:

- Ghostty's CI publishes a signed build of the engine's C library, libghostty-vt.
- A DIE command, `cargo xtask ghostty bump`, checks the signature and ties the build to a Ghostty commit whose tests passed. It then cuts the library down to 2.7 MB and commits it with a lock file.
- `cargo build` links it like any static library.

This was checked today against Ghostty 765ea37:

- **The build needs no Zig.** It ran in a clean environment with a fake `zig` on PATH, which was never called. The result links only macOS's system library.
- **Two independent test harnesses pass.** One passes 81 of 86 checks; the 5 failures are the known gaps below. The other passes 76 of 76.
- **The engine's C API covers the hard parts:**
  - kitty graphics, including the placeholder kind that yazi uses
  - mouse and key encoding
  - OSC callbacks
  - selection and search
- **One piece isn't built yet:** the exact wiring DIE will use, in which DIE supplies its own PNG decoder. Building it is the first spike task.

## What you get

Every ghostel feature was checked one by one. The full table is in [docs/records/die-term-engine.md](../docs/records/die-term-engine.md#3-ghostel-parity-checklist). In short:

- **Images:** kitty graphics for `kitten icat`, timg and yazi previews, with transparency and layering.
- **Mouse:** every mouse mode, including pixel coordinates. Shift-click selects text even when an app has captured the mouse.
- **Keyboard:** the full kitty keyboard protocol, including key release.
- **Protocols:** links, the OSC 52 clipboard, notifications, prompt marks and directory tracking.
- **Modes:** copy state, and editing the shell's input line from normal state, as evil-ghostel does.

A few things will work better than your ghostel does today:

- **Shell integration.** Your ghostel has none. macOS's bash 3.2 ignores the `ENV` variable ghostel loads it through, so your shell never reads `~/.bashrc`, and prompt jumps and directory tracking don't work. DIE starts bash with `--rcfile`, which bash 3.2 does honour.
- **Mouse hover reporting.**
- **The scroll wheel in full-screen apps** sends arrow keys.
- **Full kitty keyboard support.** ghostel sends key presses only.

## What DIE owns

| | Lines |
|---|---|
| die-term glue: turning the engine's screen and images into DIE's, sending keys and mouse back, shell integration, input-line editing | ~3.2k (±30%) |
| Tests | ~1.1k |
| The updater, scripts, terminfo and a data table | ~1k |
| **Written by hand** | **~5.3k** |
| Regenerated on each update: Rust bindings to the C API | 5.6k |
| Committed binary | 2.7 MB |

I told you about 1.5k lines earlier. That figure covered a text-only terminal. Copy state, links and input-line editing add about 1k on any engine, and images and the mouse add the rest.

The Rust API is the community crate libghostty-rs. DIE pins it and never forks it.

## What it costs

- **You can't patch the engine.** A terminal bug waits for a fix from Ghostty and a signed build. ghostel works the same way for you today. If a fix is urgent, DIE's CI can build the engine with Zig from a pinned commit. Zig still never touches your machine.
- **An update about once a month,** with roughly 30–60 minutes of review. You run the updater, read the changes to the C headers, and run the tests. The C API is still moving: its headers changed in 82 commits over the last three months.
- **Known gaps, until Ghostty fills them:**
  - Animated images show only their first frame.
  - kitty's own notifications (OSC 99) and text sizing (OSC 66) are ignored.
  - There's no title stack.
- **Waiting on the Rust crate:** prompt exit codes and a channel for shell commands that drive the editor. Your ghostel doesn't have these today either.
- **libghostty-rs has effectively one maintainer.** If it blocks an update, DIE writes its own thin layer over the C API instead (1.2–4k lines). It never forks the crate.
- **An engine crash is a hard crash.** My call in [06](06-restart-guarantees.md) is to run each terminal's engine in a small helper process. Then a crash costs one terminal, not DIE.

## Alternatives

- **alacritty_terminal plus a kitty layer DIE writes:** pure Rust, with no binary to commit. It's about 10k DIE lines including tests, and it means partly writing a terminal, which you've ruled out.
- **alacritty_terminal with no images:** the smallest option, but it gives up kitty graphics.

## Recommendation

**Accept.** It's the same engine and the same delivery model you already rely on through ghostel. The detailed calls are under the three "Terminal" headings in the [README](README.md).

## Your answer

**Accepted 2026-10-19:** "yes the prebuilt binary is fine, go with that". die-term uses Ghostty's signed libghostty-vt build with thin glue, as recommended. The calls in the README stand.

