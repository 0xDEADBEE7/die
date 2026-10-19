# 02. Approve the restructured plan?

**Question:** The milestone table in scope §12 can't be carried out in its current order. Do you approve this restructure? It changes things you accepted on 2026-10-09.

## Why the current order doesn't work

- **M0 builds die-vim before its API exists.** D15 says die-vim uses only core's public API, but that API arrives in M1.
- **M1's exit test needs M2 and M3 features.** It requires every row of the requirements check.
- **Restart is proven last.** Rebuild-and-restart is your development loop, yet it's only proven in M4.
- **The spikes have no slot.**

## The restructure

1. **S0, about 2 weeks.** Spikes for the renderer, the core API and layout, and terminal survival. The terminal engine ([01](01-terminal-engine.md)) is already proven, so it needs only two short checks: building DIE's exact wiring, and measuring whether each terminal's engine can run in a helper process ([06](06-restart-guarantees.md)). Then fix the docs.
2. **M0 starts with the public API skeleton and the headless test harness,** and die-vim is built on them from its first line.
3. **Restart is proven in M0 (files and layout) and M2 (live terminals).**
4. **The terminal milestone (M2) comes before contract completeness (M1),** because your day starts in a terminal.
5. **Honest size.** M0 is about 17k lines across crates, and roughly 10–15 weeks. The 10k core guard becomes:
   - core ≤ about 14k lines;
   - renderer ≤ about 5k lines in its own crate;
   - a per-module ratchet, with a named cut list for overruns;
   - die-term ≤ about 4k lines of glue, plus its tests and the engine updater, in its own crate.

   Two independent estimates put core plus renderer at about 16k. The 10k figure was a goal, not a measurement.

## Recommendation

Approve all five.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". All five changes are approved, including the restated budget.
