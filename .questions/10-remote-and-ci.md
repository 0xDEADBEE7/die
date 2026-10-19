# 10. Should DIE get a private remote and CI?

**Question:** The repository exists only on this laptop. Should it get a private remote, and should its checks run somewhere other than your machine?

## Background

- **There's no remote today.** The design docs, the spike records and soon M0's code exist in one copy, plus whatever backs up this laptop.
- **No check needs Emacs.** The oracle's fixtures are committed, so `cargo test` needs only cargo. The font tests need the font, which `cargo xtask fonts` fetches by checksum.
- **die-render's GPU tests need a real Mac GPU.** Hosted macOS runners are virtual machines, so I'd expect those tests to run on your machine only.
- **The clean-build check (D16)** proves DIE builds with only cargo and the Xcode Command Line Tools. It means most on a machine that isn't yours, and most from M2 on, when the build links the signed terminal engine.
- **Hosted macOS minutes are metered** on private repositories, and they count several times faster than Linux minutes. I haven't checked current prices or free allowances.

## Options

- **A. Stay local.** Nothing leaves your machine. `cargo xtask ci` runs every check in a fresh clone before anything merges.
  - Cost: one copy of everything, and no independent check.
- **B. Private GitHub repository with hosted CI.** Every push runs the tests, the oracle, the budget and the clean build on a fresh macOS runner. Work arrives as pull requests you can review in the browser.
  - Good: an off-machine copy, and a check that doesn't depend on an AI session reporting that the tests passed.
  - Cost: a CI setup to maintain, metered minutes, GPU tests still local, and the code lives on GitHub, privately.
- **C. Private remote, checks run locally.** The remote holds a copy and the pull requests. `cargo xtask ci` runs the same checks in a fresh clone on your machine before anything merges.
  - Good: the backup and the review flow, without depending on hosted runners. Hosted CI can be added later in an afternoon.
  - Cost: "the tests passed" is still checked on your machine.

## Recommendation

**C** for M0, then **B** from M2, when the build starts linking the downloaded terminal engine and a separate machine is worth more. A private GitHub repository is the simplest host, but any private git host works.

## Your answer

Accepted 2026-10-19: **C**, a private remote with the checks run on your machine, and hosted CI from M2. The remote is a private GitHub repository.
