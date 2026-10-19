# M0 log

What merged, what waits on you, what changed in die-core's API, and where the budget stands ([m0-plan.md](m0-plan.md) §3). Newest first.

## 2026-10-20: phase 0's first API merged; Linux joins ci

**Merged:** PRs #1–#5, which you approved. `main`'s tree is identical to the reviewed top of the stack. `cargo xtask ci` and `cargo xtask ci --linux` both pass on it.

How they landed:
- **#1 (K3)** merged into `main`.
- **#3 (K10a) and #5 (V1)** merged into their stacked bases, because I didn't wait for GitHub to retarget them. I then reopened #2 and #4 against `main`, and they carried #3 and #5 in:
  - **#2 (K2)** brought K10a;
  - **#4 (K8)** brought V1.

Nothing was lost, and the PR pages say which merge carried what.

**History rewritten, as you asked.** Every commit message and PR description no longer carries Claude Code attribution.
- `main` was force-pushed. Every tree is unchanged; only the messages differ.
- A bundle of every ref from before the rewrite is at `.git/backup-before-attribution-rewrite.bundle`, and git's `refs/original/` holds them too. Both are local only, and you can delete them once you're happy.
- No commit or PR will carry attribution from now on.

**Platforms (question 11, answered):**
- **Linux:** ships as a preview at M2's exit (A).
- **Windows:** out.
- **Approved:** the Linux std downloads and colima runs.
- **Declined:** asking Ghostty, and a CI-built engine. Where a signed Linux engine comes from stays open until before M2.
- The doc edits are on `main` (scope D1, D5, D10, D12, D13, D16, D17, D18 and D20, guard 7, the contract, the M0 plan and the records).

**New in `ci`:**
- the platform lint and the denylist for neutral crates;
- clean-build checks each target's graph against an allowlist;
- a Linux cross-check and clippy, for x86_64 and aarch64 gnu, with the pinned toolchain;
- guard 7's per-build budget.

`cargo xtask ci --linux` runs exit test (f) in a digest-pinned rust:1.96.0 container on colima:
- the tests, the oracle, and the Linux clean-build with an ELF NEEDED check;
- about 45 s warm.

It already caught one test that assumed a non-root user.

**Oracle:** 124 of 346 cases pass on macOS and on aarch64 Linux, and V1's pending list is empty.

**Budget:**

| Crate | Lines |
|---|---|
| die-core | 4,406 of 14k |
| die-render | 118 (macOS build 118, Linux build 12) |
| die-vim | 1,091 of 5k |
| product code | 5,662 |

**Waiting on you:** the deviations review (about 1 h) and S1's `interactive keys` check (5 min).

**Next, by the plan:** phase 1. K4 (layout), K5 (the extension machinery), K6 (windows), R2 (the offscreen renderer) and V2 (search), then checkpoint 1.

## 2026-10-19 (later): phase 0's first API, up for your approval

**Answered:** 09-B and 10-C, as recommended. The plan's doc edits are on `main`, and the private remote is github.com/0xDEADBEE7/die. You opted into workflows; the review below was the first.

**Waits on you: five stacked PRs.** Merge them in order. Each holds the contract edits that go with its code, so approving one approves those too.

| PR | Package | What it is |
|---|---|---|
| #1 | K3 | `KeyEvent` with S1's fields, one notation parser shared with the oracle, the prefix error, `unbind`, shadow lists, the leader from config. Contract §7 |
| #2 | K2 | Regex search, undo caps, selection restored on undo, `undo_boundary`, undo and redo return their range. Contract §9 and the A11/A14 notes |
| #3 | K10a | `open_file`: UTF-8 check, read-only reasons, BOM, fingerprint, per-path settings |
| #4 | K8 | Faces, themes (mono-dark in die-dev), the composed `Scene`, render-time highlights. Contract §10 and §13 |
| #5 | V1 | die-vim's grammar core, motions, counts, insert sessions, 44 new oracle cases. Scope §10 |

**How they were checked.**
- Four parallel agents built K2, K3, K8 and V1, and I built K10a.
- A review workflow then gave each branch an independent reviewer, and a second agent tried to refute each finding before fixing it with a test. The result: 59 findings, 13 of them major. 52 were confirmed and fixed, and 7 were deferred.
- Where the deferred findings went:
  - three contract wordings: into the PRs;
  - two xtask bugs: fixed on `main`;
  - K3-12, where the leader is set: to F3;
  - K10a-8, where per-path settings attach: to K5;
  - K8-7, pointer mapping: to K9;
  - V1-1: decided below.
- `cargo xtask ci` passes on every branch of the stack, and `mutate` catches 15 of 15 from K2 up.

**Calls I made under your standing instruction (veto any):**
- **V1-1:** Escape on the new line after a final newline leaves the cursor there, as Evil and Vim do. Scope §10's deviation now covers motions only.
- **K2:** search runs over a copied string, freed at the next edit. `regex-cursor`, a new crate, wasn't added.
- **K3:** `unbind` is a separate commit, so it can be dropped on its own.

**Oracle at the top of the stack:** 124 of 346 cases pass, and V1's pending list is empty.

**Budget at the top of the stack:**

| Crate or module | Lines |
|---|---|
| die-core | 4,405 of 14k |
| text and undo | 1,035 of 1,250 |
| keys | 524 of 650 |
| files | 168 of 650 |
| faces and scene | 820 of 950 |
| die-vim | 1,091 of 5k |
| product code | 5,659 |

**Platforms.** You reversed D12:
- macOS and Linux are required;
- other Unixes are welcome where they're cheap;
- Windows is in only if it isn't a considerable deviation.

A research workflow is drafting `docs/records/platforms.md` and question 11, on when Linux ships. Phase 0's code is platform-neutral except K10a's inode-based fingerprint, which is fine on Unix.

**Also waiting on you:** the deviations review (about 1 h; it now includes V1's six hand-written fixtures), and S1's `interactive keys` check (5 min).

## 2026-10-19: phase 0 starts

**Done, uncommitted:** the five packages the plan says can start under any answer to 09 and 10. `cargo xtask ci --worktree` passes every step: fmt, lint, budget, clippy, test, guards and clean-build. It takes about 40 s cold and 16 s warm.

| Package | What landed | Proven by |
|---|---|---|
| F1 Workspace | Edition 2024, resolver 3, rustc pinned to 1.96.0 in `rust-toolchain.toml`, exact version pins, a lockfile, `LICENSE-MIT` and `LICENSE-APACHE`. `unsafe_code` is forbidden workspace-wide; die-render denies it and allows it on `platform` only. die-core's `compile_error!` stops `panic = "abort"` builds | `cargo xtask guards`: the tree builds, an `unsafe` block planted in die-core or in die-render outside `platform` fails, and `-Cpanic=abort` fails with the guard's message |
| F2 xtask | `budget`, `lint`, `guards`, `api-diff`, `fonts`, `clean-build`, `mutate` and `ci` | Counter, lint and api-diff unit tests. `clean-build` passes, and fails when a build script calls `cc`, `clang` or `zig` |
| K1 Port S4 | S4's code and its 48 tests in `die-core`, as they were. Only the crate name changed, plus `DIE_PROP_CASES` and module-scoped clippy allows | All 48 pass, with the properties at 10,000 cases (8.4 s). `cargo xtask mutate` catches 8 of 8, through the same tests as in S4 |
| V0 Oracle | `tools/evil-oracle`, with S5's fixtures, schema, guards and scripts. A tranche classifier. die-vim's `tests/oracle.rs` runner on S4's `Ctx`, with `deviations.json` and `pending.txt` | 306 cases validate. 24 are deviations, and all 282 others are pending, so the test passes. A pending case that passes, a failure that isn't pending, or an unknown name each fails it |
| R1 die-render | S1's pinned stack, with AppKit and the other objc2 crates trimmed to the features named. `platform::{appkit, metal, text}`, the only `unsafe`, with thin Metal and Core Text wrappers | Platform tests: a Metal device, the font loads from its file, equal advances, missing glyphs. Budget 116 lines. `clean-build` passes |

**Waits on you:**

- **Questions 09 and 10.**
- **The deviations review (about 1 h):** `crates/die-vim/tests/oracle/deviations.json` holds 24 cases under 9 rules, all from your S5 answers. The note on `dk` and `dG` is a call: Vim's line, but Evil's column.
- **S1's `interactive keys` check (5 min)** before the Text and keys review.
- **These test plans.** You hadn't approved them before the work started, so read them as built (below).

**API:** die-core's public API is S4's as you accepted it (A1–A24), 196 items in `cargo xtask api-diff`. Nothing new.

**Budget** (`cargo xtask budget`):

| Crate | Lines |
|---|---|
| die-core | 3,060 of 14k. This matches S4's own count, which checks the counter |
| die-render | 116 of 5k |
| die-vim | 9 |
| xtask (dev) | 1.6k, against the plan's 0.7k |
| evil-oracle (dev) | 0.6k, against 0.3k |

The dev tools are uncapped, but they came in at about twice the estimate: api-diff and the tranche classifier are bigger than planned.

### Calls made while building (veto any)

- **`mutate` is an xtask command, not a script.** The planted bugs are data in `crates/die-core/tests/mutants.toml`. It runs in a copy of the tree, so a crash can't leave a mutant in your source, and it drops `mutate.sh`'s use of Python.
- **No C builders in the graph.** `clean-build` fails on a C-building crate (`cc`, `cmake`, `pkg-config`, `bindgen`, `zig`) in this Mac's build graph. One in the lockfile only, such as winit's `cc` for Android, prints a warning.
- **ci runs work in a copy.** `ci` clones HEAD by default. `ci --worktree` copies the working tree as git would commit it, for uncommitted work. Its build cache lives under `target/xtask/`.
- **What api-diff counts.** It treats every `pub` item in die-core's source as API, reachable or not, so it errs toward listing too much. Doc-only changes are listed apart. "die-vim's grammar module" means `crates/die-vim/src/grammar.rs` or `grammar/`, which don't exist yet.
- **What the oracle runner fails.** Until K7's harness lands, a case fails with a reason when the runner can't set it up or check it: visual start, clipboard, window top line, selection. An unbound key never counts as the error Evil signalled.
- **Tranches come from code.** The order is V1 < … < V6, and a case's tranche is the highest among its features. With deviations: V1 68, V2 16, V3 107, V4 59, V5 12, V6 44. The plan's first pass said 66, 16, 109, 59, 12, 44.
- **die-core's clippy allows are scoped to S4's modules** (`ctx`, `text`, `undo`). Fixing those lints would change accepted code, and the exact `if` lines two planted bugs target. K2 and K5 rebuild those modules and drop the allows.
- **The license names "The DIE authors"** as the copyright holder.

### Test plans, as built

- **F1** (`cargo xtask guards`): the unplanted tree checks clean; `unsafe {}` appended to die-core's `lib.rs` fails with "usage of an `unsafe` block"; the same in die-render's `lib.rs` fails; `-Cpanic=abort` on die-core fails with "DIE requires panic = \"unwind\"".
- **F2:**
  - `count`: `blank_lines_and_comments_dont_count`, `nested_block_comments`, `comment_markers_inside_strings_are_code`, `multi_line_strings_count_every_line`, `cfg_test_modules_and_items_dont_count`, `other_cfgs_count`, `lifetimes_and_char_literals`, `raw_identifiers_and_byte_strings`.
  - `lint`: `unsafe_allow_only_on_platform` (allowed on `pub mod platform` only, not in strings or comments, `forbid` is fine) and `pins` (`"1.0"` and `"^2"` flagged; `=`, path and workspace dependencies pass).
  - `api`: `lists_pub_items_only`, `bodies_and_docs_dont_change_signatures`, `derives_are_api`, `module_paths`.
  - `tests/clean_build.rs`: `a_crate_without_c_builds`, and `a_build_script_calling_a_c_compiler_fails` for `cc`, `clang` and `zig`.
- **K1:** S4's 48 tests unchanged (`c2_checkout` 5, `c3_queue` 4, `c4_subs` 4, `edits` 9, `keys` 8, `layout` 7, `prop` 3, `proto` 8) and the 8 mutants.
- **V0:**
  - S5's 8 fixture guards.
  - `keys_pick_the_tranche`: 20 key sequences plus a tty case.
  - `pending_file_format`.
  - die-vim's `oracle`.
- **R1:** `metal_device_and_queue`, `font_loads_from_its_file`, `monospace_advances`, `missing_glyphs_and_files`.
