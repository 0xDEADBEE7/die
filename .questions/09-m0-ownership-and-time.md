# 09. M0: how do you own the API and the grammar?

**Question:** 08 says you own die-core's API and die-vim's grammar. For M0, does that mean you write them yourself, or that an AI drafts them and nothing becomes final without your approval? And roughly how many hours a week can you give M0?

## Why it's open again

- **08's answer:** "You own what defines DIE: the core API and contract, die-vim's grammar, and the decision records. An AI drafts the renderer and platform layer, die-term's glue, the holder, and test fixtures, under tests you review."
- **For M0 you said:** "lets talk through it together to plan it out then you can work through it". That fits either reading.
- **"Own" can mean two things:** you wrote it, or nothing in it became final without your approval. 08 didn't say which.
- **What 08 gives you is small in lines and large in weight.** die-core's public signatures and rustdoc, plus die-vim's grammar core (how counts, registers, operators, motions and text objects fit together), come to about 1.5–2k lines. The rest of die-core, about 6.5–9k lines in M0, is implementation. The rest of die-vim, about 3–4k, is motions, text objects and ex commands that the oracle checks.
- **08 left your pace open.** The plan's dates assume reviews come back within a few days. Your hours a week set the real dates.
- This question is about M0. 08's split stays the default for later milestones unless you say otherwise.

## What holds whichever you pick

- **The API's shape is set.** S4 built it, and you accepted A1–A24. M0 starts by porting it with its tests.
- **die-vim's behavior is set by the oracle:** your Evil on 306 cases, less 24 named exceptions. Whoever writes die-vim, the oracle says whether it's right.
- **08's "under tests you review" stays.** Each package an AI drafts starts with a test plan, the test names and what each asserts, and you approve it in about 10 minutes.
- **Always yours:** the decision records, the deviation list, and DIE's `unsafe` code, which you review by hand. Five scheduled reviews of 1–3 hours read each part of the API as a whole ([docs/m0-plan.md](../docs/m0-plan.md) §3).
- All of that comes to about 18 hours across M0.

## Options

- **A. You write them.** You write die-core's public signatures and rustdoc, and die-vim's grammar core. An AI implements behind your signatures, and writes motions, text objects and ex commands against the oracle.
  - Good: your hands are on everything that defines DIE, and you know it as its author.
  - Cost: die-vim and the window wait on your writing, so M0 takes longer by however long 1.5–2k careful lines take you. The line between signature and implementation is blurry in places, such as layout queries and undo grouping.
- **B. An AI drafts them, and every change waits for you.** A change to die-core's public API or to die-vim's grammar core reaches you as a short rustdoc diff, and it doesn't merge until you approve it. Everything else merges when the checks pass. You also read the subtlest code in full: undo, layout, key dispatch and the grammar core, about 2.5k lines. You can take over any piece at any time.
  - Good: nothing becomes API without you, and M0 runs at about 11–16 weeks. About 30–40 hours of your time across M0, or 2–3 hours a week.
  - Cost: you'll know most of the code as its reviewer, not its author, and its style can drift toward an AI's habits. The weekend rule (guard 7) becomes a real check: if a review is hard to follow, the code is simplified before work goes on.
- **C. An AI drafts them, and you approve at the scheduled reviews.** Everything merges when the checks pass, API changes included. The five reviews are where you approve each part or send it back.
  - Good: the least of your time, about 18 hours, and nothing waits on you between reviews.
  - Cost: API mistakes surface later, after code is built on them, and each review is a bigger catch-up.

A variant of B: you write die-vim's grammar core yourself, about 500–800 lines, and an AI drafts the rest. It's the part you'll feel every day.

## Recommendation

**B** is the fastest way that still keeps anything from becoming API without you. If "talk through it together" means you want your hands in the code, **A** fits better, or B with the grammar core as yours.

**And your hours:** roughly how many a week can you give M0? I'll put dates on the plan from that.

## Your answer

Accepted 2026-10-19: **B**. "go with your recommendations on 09 and 10. In general just continue according to your own recommendations to the best of your ability." No hours a week were given, so the plan keeps its 11–16 week range, which assumes reviews come back within a few days.
