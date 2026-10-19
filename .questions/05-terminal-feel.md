# 05. Should terminals feel like vterm today, or like kitty and Ghostty?

**Question:** What should using a terminal buffer feel like?

## How you work today

Your daily terminal is **vterm**: `termin` and the startup hook open it. ghostel is only on `<leader>g`. In vterm:

- **ESC leaves to Evil normal state.**
- **Normal state is live and editable.** Operators edit the shell's input line, `[[` and `]]` jump between prompts, and output keeps streaming.
- **Cmd-V pastes into the shell or the Claude Code prompt.**

## Where that gets awkward

- **ESC is also how you interrupt Claude Code,** and how you leave insert mode in vim. ESC can't mean both "leave the terminal" and "send to the program". Ghostel's trick of sending ESC to the program only on the alternate screen fails for Claude Code, which runs on the main screen.
- **The docs describe a read-only copy state and "one reserved key"** that's never named. They don't mention Cmd at all.

## Options

- **A. vterm-like.** ESC leaves to normal state, a toggle sends ESC through, and normal state can edit the shell prompt. Familiar, but interrupting Claude Code needs the toggle. Prompt editing needs shell integration for bash 3.2, which is already in die-term's plan ([01](01-terminal-engine.md)).
- **B. kitty/Ghostty-like.**
  - ESC always goes to the program.
  - A Cmd chord (e.g. Cmd-Esc) always leaves.
  - The whole Cmd layer belongs to DIE: Cmd-V pastes, Cmd-C copies, Cmd-Q quits and confirms if processes are running.
  - Normal state is read-only. It's for moving, searching and yanking, and it follows new output while you're at the bottom.
  - `i`/`a` go back to the terminal.
- **C. B by default, plus a per-buffer toggle** for vterm-style ESC. Prompt editing can come later if you miss it.

## Recommendation

**C.** Every program behaves correctly under it, and you can flip a buffer back to vterm habits.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". C, kitty/Ghostty-like by default, plus a per-buffer toggle for vterm-style ESC.
