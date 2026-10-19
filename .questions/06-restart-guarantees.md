# 06. What must survive a restart or a crash?

**Question:** You'll restart DIE many times a day to pick up rebuilds. What has to come through untouched? And does that include a *crash*, such as a freshly built binary that panics on startup?

## Background

- **Terminals.** Measured on this machine:
  - Without a holder, a crash of DIE kills every shell, SSH and Claude Code session at once.
  - `catch_unwind` can't prevent that: stack overflows and some panics abort regardless.
  - A tiny holder process that keeps the terminals open did survive `kill -9` of DIE, including when DIE was launched as a `.app`. That's DIE's own binary in a second mode, not a second toolchain.
  - The terminal engine ([01](01-terminal-engine.md)) can't recover from its own crashes; one takes down the whole process. So it's safer outside DIE's process.
- **Editor state.** The docs restore files, cursors, scroll positions and layout. They say nothing about:
  - unsaved edits;
  - scratch buffers;
  - registers and recorded macros;
  - search and command history;
  - undo history.

## Options

- **A. Everything survives, crashes included.**
  - A holder keeps terminals alive through crashes.
  - The session saves unsaved text, registers, macros and histories.
  - Undo history comes later.
- **B. Planned restarts only.** Restart in place, with no holder process. A crash loses your terminals.
- **C. Refuse to restart while anything is unsaved,** and list what is.

## Recommendation

**A.** Losing live Claude Code or SSH sessions to a bad build would make rebuild-and-restart, your extension loop, something to dread. The holder is about 250 lines.

Under A, my call is to run each terminal's engine in a small helper process alongside the holder. An engine crash then costs one terminal instead of DIE and your unsaved text. Scrollback and images also survive every rebuild. It costs an estimated 0.5–1k more lines. A short spike before M2 checks that frames and images cross between the processes fast enough.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". A, everything survives, crashes included. Each terminal's engine runs in a helper process if the pre-M2 spike shows that's fast enough.
