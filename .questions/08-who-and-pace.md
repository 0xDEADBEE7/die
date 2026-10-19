# 08. Who writes the code, and at what pace?

**Question:** Will you write most of DIE yourself? Or will an AI write it with you reviewing, or will you split it by area? And are you thinking evenings and weekends, a focused block, or a deadline?

## Why it matters

- **The estimates assume AI-assisted work with you as the only reviewer.** On that basis M0 takes roughly 10–15 weeks. Parallel AI sessions can't shorten a chain that has one reviewer.
- **It decides how many spikes run at once** in the first two weeks.
- **It decides what "readable in a weekend" means:** code you wrote, or code you reviewed.

## A split that would make sense

- **You own** what defines DIE: the core API and contract, die-vim's grammar, and the decision records.
- **An AI drafts,** under tests you review: the renderer and platform layer, engine glue, the PTY holder, and test fixtures.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". The split above stands: you own the core API and contract, die-vim's grammar and the decision records, and an AI drafts the rest under tests you review. You didn't state a pace, so the estimates assume that split with you as the only reviewer and no deadline.
