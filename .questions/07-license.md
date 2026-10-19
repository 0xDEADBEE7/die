# 07. License

**Question:** MIT OR Apache-2.0, or GPL-3.0?

## Background

- **Ghostty's engine is MIT, and libghostty-rs is MIT or Apache-2.0.** The libraries bundled inside the engine binary are MIT, Apache-2.0 or BSD: wuffs, simdutf, Highway, uucode and Zig's compiler_rt. DIE ships their notices. The rest of the planned dependencies are MIT or Apache, and either license works with all of them.
- **The only GPL code worth borrowing is from ghostel and evil-ghostel.** That's a shell-integration bootstrap for macOS's bash 3.2, and prompt-editing logic. Both are small enough to write from scratch.
- **GPL would bind anyone building on DIE.**

## Recommendation

**MIT OR Apache-2.0.** ghostel stays a behaviour reference only.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". MIT OR Apache-2.0.
