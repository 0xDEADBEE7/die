# 11. When does Linux ship, and how equal is it to macOS until then?

**Question:** You've made macOS and Linux required. Should Linux arrive with the terminal milestone (M2), be on par with macOS from M0, or come in its own milestone after M2? And do you accept leaving Windows out?

The full record is [docs/records/platforms.md](../docs/records/platforms.md).

## Background

- **The Mac plan doesn't change.** macOS keeps S1's measured Metal and Core Text path. Linux gets one portable backend: wgpu plus a pure-Rust text stack. D13's wgpu stall came from a review probe, not S1, and S1's own staging ring avoids it (measured: main-thread p99 0.15–0.26 ms).
- **Keeping Linux honest is cheap now.** After an 8-line change, the whole workspace cross-checks for Linux on your Mac in about 11 s (verified). A lint can keep OS-specific code in a few named modules.
- **Building it isn't free.** The Linux window, renderer, clipboard and packaging come to about 1.7–2.6k lines.
- **Testing is the hard part.** You have no Linux machine with a display. colima is headless, aarch64 and has no GPU, and Asahi doesn't run on an M5. So until you have hardware, Linux can be "preview" (builds, and passes every headless and software-rendered test) but not "supported" (S1's latency checks on a real machine).
- **Terminals on Linux need a Linux engine archive.** Ghostty signs only the macOS one today. We can ask upstream to publish Linux ones. If they haven't by M2, DIE's own CI could build one from Ghostty's signed source, which needs a D16 wording change.
- **Windows would be a considerable deviation.** There's no signed Windows engine. ConPTY replaces PTYs, and a pseudoconsole dies with the process that created it, so D20's holder would need a different design. Parity needs a bundled ConPTY binary that D17 doesn't allow. And you have no Windows machine to use it on. Rendering alone would be cheap.

## Options

Every option includes the phase-0 seams and guards, about 2–3 days of AI work and 1 hour of yours.

- **A. Mac first, Linux at M2's exit.** M0 runs on the Mac as planned. The renderer track builds the Linux backend after checkpoint 4, and M2 writes the Linux process layer alongside the Mac one. Linux is "preview" at M2's exit, about March to May 2027.
  - Cost: about 1.5–3 weeks end to end, none of it before M0's exit. About 6–10 hours of your time.
- **B. Parity from M0.** Every checkpoint and exit test runs on both platforms, and none passes until Linux does.
  - Cost: M0 grows from 11–16 to 13–20 weeks, and about 4.5–9 weeks in all. About 25–32 more hours of your time, including a Linux exit day per milestone. It needs a Linux machine with a GPU by about week 7.
- **C. Linux in its own milestone after M2,** alongside M1.
  - Cost: about the same work as A, about 2–4 weeks end to end. Linux arrives last, about mid-April to end of June 2027, and the renderer's seams have one implementation for longer.

## Recommendation

**A.** It keeps M0 about getting you off Emacs, while Linux ships with the first release that has terminals. B slows the daily driver for hardware you don't have. C gets you nothing that A doesn't, and costs time later.

**Windows is out.** That's my call; you can veto it. I'd revisit when Ghostty signs a Windows engine, pseudoconsoles can be handed between processes, and you have a Windows machine. FreeBSD gets compile checks only.

Under any option, three things need your yes:
1. Linux std downloads for the pinned toolchain, and starting colima with a pinned Rust image.
2. Asking Ghostty, from your account, to publish signed Linux engine archives.
3. If they haven't by M2: may DIE's own CI build the Linux archive from Ghostty's signed source? Zig would still run only in CI, never on your machine.

## Your answer

Accepted 2026-10-19, through a multiple-choice prompt:

- **When Linux ships: A.** Linux ships as a preview at M2's exit.
- **Windows: out,** as recommended.
- **The three yeses:**
  1. Approved: the Linux std downloads for the pinned toolchain (x86_64 and aarch64 gnu), and starting colima with a pinned `rust:1.96.0` image.
  2. Declined: asking Ghostty, from your account, to publish signed Linux engine archives.
  3. Declined: DIE's own CI building the Linux archive from Ghostty's signed source.

So D16 keeps its rule, with only the platform-toolchain wording, and no clause for a DIE-built archive. That leaves open where a signed Linux archive comes from: Ghostty doesn't publish one today. It's revisited before M2, which is when Linux terminals need it. Without one, Linux ships editor-only at M2's exit and gets terminals at the first `ghostty bump` that has an archive.
