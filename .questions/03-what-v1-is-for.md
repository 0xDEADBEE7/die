# 03. What day should DIE v1 carry?

**Question:** What should a successful day in DIE look like when this project ends? And are you OK with the rest staying in Emacs until you build it yourself?

## Background

- **M2's exit test is a day of SSH work, but your recent history doesn't show one.** A reviewer found no `ssh` in your last 500 shell commands. The real load was `make`, `docker`, Claude Code and codex sessions, and TUIs like btop and ncdu.
- **D14 keeps several things in Emacs until you port them:** orchid, scrib, delta, dired-sidebar, buffer-menu and the markdown preview.
  - delta (your own, ~1.4k lines) isn't even in the requirements check yet.
  - The markdown preview is the feature the docs cite as motivation, and DIE drops it, since it has no web views.
- **During M0 there's no terminal yet,** so cargo, git and Claude Code run in Emacs's vterm next to DIE.

## Options

- **A. Local first.** M2 is proven on your real day: Claude Code and codex sessions survive rebuild-restarts and a crash, btop and ncdu work with the mouse, and output floods don't hurt typing. One SSH check through your colima VM.
- **B. SSH first,** as the docs say now. Pick this if remote work is coming that your history doesn't show.
- **C. Both, equally.**

For all of them: you accept that orchid, scrib, delta and the rest stay in Emacs after this project. delta gets added to the requirements check.

## Recommendation

**A.** Write the "still in Emacs" list into scope.md so the exit tests exclude it honestly.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". A, local first. The "still in Emacs" list goes into scope.md, and delta joins the requirements check.
