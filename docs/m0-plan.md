# M0 plan: Edits itself

**Accepted 2026-10-19:** 09 answered B and 10 answered C, both as recommended. Progress is in [m0-log.md](m0-log.md).

**Since 2026-11-03:** 09-B's wait for your approval is off until you say otherwise, and your reviews and hand checks are done for you, with every call logged for veto ([m0-log.md](m0-log.md), 2026-11-05).

This plan breaks scope §12's M0 row into dogfood checkpoints, reviews and work packages. It builds on the S0 spike records in [records/](records/) and on every answer in [.questions/](../.questions/README.md).

It assumes my recommended answer to each of this round's two questions, and says what changes under the others. The specifics I've decided within those answers are listed for veto in the [questions README](../.questions/README.md), under "M0 planning".

**Work that's the same under every answer can start now:** F1 and F2 (the workspace and xtask), K1 (porting S4 under its tests), V0 (moving the oracle in) and R1 (the renderer crate).

| | Question | The plan assumes | If you answer differently |
|---|---|---|---|
| [09](../.questions/09-m0-ownership-and-time.md) | How you own die-core's API and die-vim's grammar, and how many hours a week you have | B: an AI drafts them, and each change waits for your approval | A: you write the public signatures and die-vim's grammar core (§5's API and Grammar rows), and the dates move by your writing time. C: those changes merge on the checks, and you approve them at the scheduled reviews |
| [10](../.questions/10-remote-and-ci.md) | Remote and CI | C: a private remote, with the checks run on your machine | B: the same checks also run on hosted runners. A: no remote |

## In short

- **Weeks 1–2:** S4's API is ported into die-core under its tests, the oracle moves into the workspace, and the renderer starts. Nothing to look at yet.
- **Checkpoint 1, weeks 3–5:** die-vim passes your Evil's motion and insert cases, headless.
- **Checkpoint 2, weeks 5–7:** you read DIE's source in DIE.app, tiled beside Emacs.
- **Checkpoint 3, weeks 8–11:** you edit and save DIE in DIE, and a crash or a restart keeps your unsaved text.
- **Checkpoint 4, weeks 10–14:** splits, and a restart in under a second. Attempts at the exit day start here. M0 ends at about weeks 11–16.
- **Your time under 09-B:** about 30–40 hours across M0, or 2–3 hours a week (§3), plus using DIE at each checkpoint.

## 1. Goal and exit test

**Goal:** you edit DIE's own source in DIE. Until M2 gives DIE a terminal, cargo, git and Claude Code run in Emacs's vterm in the next tile.

The exit test is scope §12's. (f) was added for Linux on 2026-10-19 (question 11):

| | Exit test | How it's run | Mostly built by |
|---|---|---|---|
| (a) | You edit DIE's own source in DIE for a full day, with Emacs's vterm in an adjacent tile as the terminal. | A working day. Each switch to Emacs to edit becomes a test, and the day is tried again. | everything |
| (b) | 20 rebuild-and-restarts restore files, splits, per-window cursors and scroll, unsaved text and registers, within 1 s from `restart` to the first frame. | `cargo xtask restart-loop 20`. After each restore, DIE writes a digest of its session state. The driver checks that all 20 match and that each restart took under 1 s. | K12, K13, F4a, F4b |
| (c) | `cargo fmt` on an open file reloads an unmodified buffer, and `:w` refuses on a modified one. | Harness tests on a temporary directory, then once by hand. | K10b, V5 |
| (d) | Every Evil-oracle case passes (306 today), except the deliberate deviations, which are listed by name and have hand-written fixtures. | `cargo test -p die-vim --test oracle` with an empty pending list. The 24 excluded cases are named in `deviations.json`, each with a hand-written fixture. | V0–V7 |
| (e) | The budgets in guard 7 hold. | `cargo xtask budget` | F2 |
| (f) | The workspace cross-checks for x86_64 and aarch64 Linux, and die-core, die-vim and the oracle pass their tests on aarch64 Linux. | `cargo xtask ci` cross-checks on every merge; `cargo xtask ci --linux` runs the tests in colima | F1, F2, R1 |

How they're judged:

- (a) is judged as written: a day with no switch to Emacs to edit. More than 3 switches in a dogfood day is an early warning (§6), not a pass mark.
- (b) is timed from the `restart` command to the new window's first presented frame. The build comes before `restart`, as §5.9 describes it.
- (b)'s digest covers everything the session saves, so it also checks macros and histories, which (b) doesn't name.

## 2. Shape

**First, port S4 (weeks 1–2).** S4 built the API, and you accepted it (A1–A24). M0 starts by porting S4 into `die-core` as it is: every module, its 48 tests (both property tests among them) and its planted-bug script. The port merges as already accepted. The oracle moves into the workspace at the same time. Its runner uses the ported `Ctx`, which already types keys, sets registers and runs until idle, until the public harness (K7) takes over. die-vim is written against this API from its first line (D15).

**Then thin slices.** Each dogfood checkpoint in §12 becomes a slice through every crate: reading, then editing, then splits and restart. Each one ends with something you do in the real window. The spike-quality parts are rebuilt under the same tests while you use DIE: the 1,151-line `ctx.rs`, the layout cache and its character wrap, the key type and the scene.

**Four tracks, one reviewer.** Core runs as two AI sessions. The text side does text, undo, layout, files and the prompt's candidate list. The runtime side does keys, the extension machinery, windows, the command line and the session. A third session does the renderer and the scene types it draws, and a fourth does die-vim. What paces them is review, not code.

Why this shape:

- The old plan's top risk was writing about 15k lines before using DIE at all. Slices put a window in front of you at weeks 5–7.
- Keyboard, font, resize and tiling problems only show in the real app. S1 left five checks that need your hands, and the AeroSpace rule hasn't been tried on a borderless window.
- Rebuilding a layer underneath you is safe when the tests are strong. S4's property tests caught all 8 planted bugs, and the oracle checks die-vim against your own Evil.

This is how §12 already describes M0, so it's a call, not a question. **If you veto it,** the same packages run layer by layer. The window waits until the scene, the harness and the extension API are reviewed, so reading in DIE moves to about weeks 7–9 and editing to about weeks 10–12.

## 3. Checkpoints and reviews

### Phases

The weeks are ranges because they depend on how fast reviews come back, and on estimates nothing has measured yet. They assume 09-B.

| Phase | Weeks | Core, text side | Core, runtime side | Renderer and scene | die-vim | You |
|---|---|---|---|---|---|---|
| 0. Spine | 1–2 | F1, F2, K1, K2 | K3, then K5 | R1, K8, then R2 | V0, then V1 | The deviations. S1's key check. Test plans. Text and keys review |
| 1. Reader | 3–7 | K4, K10a, K14 | K5, K6, K9, K11a | R2, R3, R4, F3, F4a | V1, V2, then V3 | Scene review. The `unsafe` review, in pieces. Checkpoints 1 and 2 |
| 2. Editor | 6–11 | K10b, K11b | K7, K12 | R5, R6 | V3, V5, the editing part of V4 | Extension API review. Files and session review. Checkpoint 3 |
| 3. Daily driver | 9–14 | speed work, fixes from use | K13, F4b | fixes from use | the rest of V4, then V6 | Restart review. Checkpoint 4 |
| 4. Close-out | 11–16 | K15 | K16 | fixes from use | V7 | The exit tests |

- **Total:** about 11–16 weeks under 09-B, and 13 is my best guess. Under 09-A, add your writing time to the core and die-vim tracks. Under 09-C it's a little faster, since nothing waits between reviews.
- **What doesn't wait on you:** the renderer runs on its own tests until K9, and die-vim's motions are plain functions over the text and layout. die-vim's operators start on the ported API before the Extension API review, and adapt to whatever it changes.
- **What does:** the `unsafe` pieces and the test plans, and under 09-B each change to die-core's public API or die-vim's grammar. I'll send each as a short diff and aim to have it back within two days. Checkpoint 3 doesn't wait for the Files and session review: the save, refuse and revert rules are calls you can veto now, and the review checks them in code.

### Dogfood checkpoints

1. **Headless grammar (weeks 3–5).** No window yet. `cargo test -p die-vim` runs your Evil's motion and insert cases (about 66) against die-vim, and they pass. `cargo xtask budget` prints every crate's size.
2. **Reader (weeks 5–7).** You open DIE's source in `~/Applications/DIE.app` with `:e`. It's a borderless window that AeroSpace tiles, in Ioskeley Mono Term and mono-dark, wrapped and centered at 80 columns, with visual line numbers.
   - Move with `h j k l w b e 0 ^ $ gg G f t % { }`, `gj gk`, and `C-d C-f C-b zz`, which are approximate until V6.
   - Search with `/ ? n N * #`, with every match highlighted. Scroll with the trackpad, and click to place the cursor.
   - You can't edit yet. Use DIE as your code reader beside Emacs for a day.
   - Add the AeroSpace rule, and run S1's hand checks on the real app: typing, resize and occlusion, about 10 minutes.
3. **Editor (weeks 8–11).** You edit and save in one window.
   - Insert with autoindent, operators with counts, text objects, `v` and `V`, `.`, `u` and `C-r`.
   - Registers sync with the macOS clipboard both ways.
   - `:w :q :wq :e :b`. `\f` (`find-file`) and `\]` (`switch-buffer`) arrive with the candidate list (K11b), at this checkpoint or soon after.
   - `cargo fmt` in vterm reloads your open files, and `:w` refuses when a modified buffer's file changed underneath it.
   - A crash or a `restart` brings back your files, cursors and unsaved text.
   - About 190 oracle cases cover this slice (V1–V3). Make small real commits to DIE from DIE.
4. **Daily driver (weeks 10–14).** You split with `C-w` and `:sp :vs :only :close`.
   - `restart` brings back splits, per-window cursors and scroll, unsaved text, registers and macros, in under a second.
   - Macros, `R`, visual block and `:s` work.
   - Attempts at exit day (a) start here. What's left is the rest of the oracle, `describe-key` and `describe-command`, the budgets, and fixes from use.

### Your reviews

The five scheduled reviews each read a whole surface at once. The other three run alongside the work.

| Review | When | What you look at | Time |
|---|---|---|---|
| Text and keys | weeks 2–3 | The rustdoc for text, undo, keys and keymaps, the layout queries, the word-wrap rule, and the undo property test | about 2 h |
| Scene | weeks 4–6 | `Cell`, `Style`, `Scene`, faces and themes, the snapshot format, the four calls die-render uses | about 1 h |
| Extension API | weeks 6–8 | `Extension`, `Registry`, checkout, `Args`, subscriptions and hooks, the prefix argument, `feed_keys` and the key observer, windows, registers, `Clipboard`, the harness. S4's toy die-vim recompiled against it, and die-vim's grammar core | about 3 h |
| Files and session | weeks 7–10 | The `prompt` spec, core commands and how they're overridden, the save, refuse and revert rules, session schema v1 with a sample file | about 2 h |
| Restart | weeks 10–13 | The full session, `restart`, single instance, the `restart-loop` log | about 1 h |
| `unsafe` | in pieces as R1–R4 land, done before checkpoint 2 | `die_render::platform` by hand, about 0.6–0.9k lines | about 3–4 h |
| Deviations | weeks 1–2 | The 24 entries in `deviations.json` | about 1 h |
| Test plans | as packages start | For each AI-drafted package, the test names and what each asserts | about 10 min each, about 4 h in all |

**Hours.** Under every answer, those come to about 18 hours. Under 09-B you also approve API and grammar changes as they arrive, about 3–6 hours of short diffs. And you read the subtlest code in full: undo, layout, key dispatch and die-vim's grammar core, about 2.5k lines, or 10–17 hours. That's about 30–40 hours across M0, or 2–3 hours a week, plus using DIE at each checkpoint. Under 09-C it's the 18 hours. Under 09-A, add your writing.

**Before work starts:** about 45 minutes to read this plan and answer 09 and 10. Then 6 minutes of S1 checks in `spikes/s1-renderer/`: run `./fetch-font.sh`, then `cargo run --release -- interactive keys` (5 minutes, since `KeyEvent` is fixed at the Text and keys review), and open `results/fidelity-term-sprites.png` and `results/fidelity-term-font_only.png` (1 minute).

### How work lands

- Each work package is a branch, and up to four AI sessions work at once.
- Nothing merges until `cargo xtask ci` passes in a fresh clone: the tests, the oracle's pending list, the budget and the clean build.
- `cargo xtask api-diff` lists every public item a branch adds, removes or changes in die-core, and every change to die-vim's grammar module. Under 09-B, a branch with a non-empty list waits for you. Everything else merges when the checks pass.
- Each package an AI drafts starts with a test plan. That's 08's "under tests you review".
- A weekly note in `docs/m0-log.md` says what merged, what waits on you, what changed in the API, and where the budget stands.
- Every problem you hit while dogfooding becomes a test first: an oracle case for die-vim, a harness test for core.

## 4. Budget

**Counting rule** (a call): code lines after rustfmt, excluding blank lines, comments, `tests/` and `#[cfg(test)]` modules, counted by xtask's own counter. S1's and S4's estimates already use it. The toolchain is pinned to rustc 1.96, as S4 used, so a rustfmt update can't move the counts.

| Crate | Guard 7 | M0 estimate | Added later | Expected total |
|---|---|---|---|---|
| die-core | ≤ about 14k | 7.8–10.2k | +1.6–2.6k: processes, PTYs, the holder client and copy-state snapshots (M2); the decoration index, modes and per-object disabling (M1); placements (M3) | 9.4–12.8k |
| die-render | ≤ about 5k per build, ≤ about 7k in total | 3.2–3.9k | +0.9–1.1k: images, placements, the pixel buffer, the display link and atlas eviction (M3). The Linux backend (M2) | macOS build 4.1–5.25k; all platforms 5.35–7.15k |
| die-vim | none yet; proposed ≤ about 5k | 3.6–4.6k | small: copy state (M2) | 3.7–4.7k |
| die | – | about 0.2k | registering die-term, the holder and helper modes (M2) | |
| apps/die-dev | – (your config) | about 0.15k | | |
| xtask, tools/evil-oracle | dev-only: counted, not capped | about 0.7k and 0.3k | `ghostty bump` (M2) | |
| **Product code** | | **about 15–19k** | | |

die-render's M0 figure is S1's 4.1–5.0k minus what M3 adds. The platforms record puts the macOS build at 4.1–5.25k, up to 0.25k above S1's figure, and all platforms at 5.35–7.15k ([records/platforms.md](records/platforms.md) §5.7). Tests come on top: about 5–6k in core (2.6k of it from S4), 1.5k in die-vim and 0.5k in the renderer.

Each module has a ratchet: a limit in `budget.toml` that `cargo xtask budget` enforces. It starts at the top of the module's estimate, and after M0 it drops to the measured size. die-core's modules at M0, from S4's ranges less what M0 defers:

| Module | M0 | Module | M0 |
|---|---|---|---|
| text and undo | 1.05–1.25k | workspace, windows, pointer | 0.85–1.1k |
| layout | 0.8–0.95k | files | 0.45–0.65k |
| keys, keymaps, describe | 0.5–0.65k | command line, prompt, core commands | 0.9–1.3k |
| ext, dispatch, rt | 1.2–1.5k | faces and scene | 0.7–0.95k |
| session | 0.7–1.0k | harness, app interface, messages, ids, errors | 0.6–0.8k |

An overrun fails the check and goes through the cut list, in this order:

- **die-core:** candidate annotations in prompts, history search, per-pattern settings.
- **die-render:** renderer-drawn braille (off by default), dashed and diagonal box glyphs (the font draws them), the latency log (it moves into test code).
- **die-vim:** no cut list, since §10 is its scope. An overrun means simplifying at the next review. Guard 7 would say so (§8).

## 5. Work packages

Sizes are AI-assisted effort: S is up to 2 days, M 3–7 days, L 1.5–3 weeks. Core packages are K1–K16, so they don't collide with the contract's C1–C17.

**Owner.** Rows marked **API** or **Grammar** follow [09](../.questions/09-m0-ownership-and-time.md). Under A you write the public signatures and rustdoc in the API rows and the grammar core in the Grammar rows, and an AI implements the rest. Under B an AI drafts them, and each change waits for your approval. Under C an AI drafts them, and you approve them at the scheduled review. Rows marked **AI** are drafted by an AI under a test plan you approve.

### Foundation

| ID | Deliverable | Needs | Size | Proven by | Owner |
|---|---|---|---|---|---|
| F1 | The workspace: edition 2024, resolver 3, rustc pinned to 1.96, with the x86_64 and aarch64 Linux gnu targets in `rust-toolchain.toml`. Crates `die-core`, `die-render`, `die-vim`, `die`, `apps/die-dev`, `xtask`, `tools/evil-oracle`. `unsafe` denied outside `die_render::platform`, a `compile_error!` under `panic = "abort"`, exact version pins, a committed lockfile, license files | – | S | `cargo test` passes. An `unsafe` block outside `platform` fails to build, and so does `-Cpanic=abort`, with the guard's message | AI |
| F2 | xtask: `budget` (its own counter, with limits and platform tags in `budget.toml`; it reports each build and the total), `api-diff`, `fonts`, `clean-build` and `ci` (every check in a fresh clone). `clean-build` uses an empty environment and `--locked --offline`, names the linker by its full path, puts failing `cc`, `c++`, `clang`, `clang++`, `gcc`, `g++`, `zig` and `pkg-config` first on PATH, sets failing `CC`, `CXX` and `AR`, and finds no `cc` in the macOS build graph. D15's lint covers every dependency table, target tables included. xtask hashes with sha2 instead of `/usr/bin/shasum`, and finds curl, unzip and the oracle's Emacs on PATH. For Linux: a platform lint (no `target_os` and no OS-specific `std::os` modules in die-core, die-vim or the oracle outside `die-core/src/sys/`), a dependency denylist that keeps window and GPU crates out of die-core and die-vim on every target, `ci` cross-checking and linting the workspace for x86_64 and aarch64 Linux, `ci --linux` in an aarch64 container on colima with the fonts mounted through `DIE_FONT_DIR`, and clean-build's Linux mode (the same stand-ins, and a per-target graph check with an allowlist) | F1 | S | Counter tests on sample sources. `clean-build` passes, and fails on a planted build script that calls a C compiler. The platform lint fails on a planted `target_os` in die-core | AI |
| F3 | The `die` facade and `apps/die-dev`: `die::run(config)`, the config type, files named at startup. Your config: leader `\`, mono-dark, Ioskeley Mono Term with braille pinned, per-pattern wrap, width and line numbers, and bindings that grow with each checkpoint (`\f \] \/ \x \z`) | K9 | S | Contract C8's check (every bound command exists) at every checkpoint. A startup snapshot | AI drafts it from `init.el`; yours after |
| F4a | DIE.app: `cargo xtask app` builds `~/Applications/DIE.app` with a fixed bundle id and the fonts, launched through LaunchServices. A lock file keeps one instance. A stub `restart`, timed end to end. The first-frame handshake runs over a Unix socket, not through LaunchServices | R3, F3 | S | Two launches leave one instance. A stub restart under 500 ms. One Dock icon, and tiled by your AeroSpace rule (by hand, once) | AI |
| F4b | The restart loop: under a flag, DIE writes a digest of its session state after each restore and then runs `restart`. `cargo xtask restart-loop 20` compares the digests and logs each restart's time | K13 | S | 20 equal digests, each restart under 1 s | AI |

### Core (`die-core`)

| ID | Deliverable | Needs | Size | Proven by | Owner |
|---|---|---|---|---|---|
| K1 | Port S4 as it is: every module, the 48 tests, and `mutate.sh` pointed at the new paths. It merges as already accepted (A1–A24) | F1 | M | Every S4 test passes in the workspace, both properties at 10,000 cases, and `mutate.sh` catches 8 of 8 | AI |
| K2 | Text and undo to production: regex search over the rope, an undo cap, the selection shape restored on undo, explicit undo units, and `undo` reporting the range it changed | K1 | M | Edit tests, the model-based undo property, search tests. The undo bugs planted again | API |
| K3 | Keys and keymaps: one `KeyEvent` with S1's fields and no winit types, one notation parser shared with the oracle, a key bound both alone and as a prefix as a registration error (A18), shadow lists, the leader from config | K1 | S | A notation round-trip property, the prefix error, S4's key tests | API |
| K4 | Layout to production: a per-line cache, compact cells, word wrap, absolute, relative and visual line numbers. A `toggle-max-width` command. It owns each buffer's per-window views | K2 | L | S4's layout tests. A naive layout rewritten by a different session from the written wrap rule, and the property against it. A planted stale-cache bug and a planted wrap bug caught. A 10k-line file and a 1 MB line within the speed budget | API (queries) |
| K5 | Extension machinery, dispatch and runtime: `ctx.rs` split into `ext`, `dispatch` and `rt`. The config step, a timer heap, a channel with a `Waker`, a real and a fake `Clock`, panics caught and logged to `*messages*`, and an error that drops the rest of the fed keys | K3 | L | S4's checkout, queue and subscription tests unchanged. Timer order, wake coalescing, next deadline | API |
| K6 | Workspace and windows: buffers, names, `directory`, the modified flag, the split tree, `show(ShowIn)`, focus by direction, close, only, resize. Typed registers, and the `Clipboard` trait with a change count that changes when another app copies | K5 | M | A split-tree property (windows tile the frame with no overlap). Focus by direction. Killing a buffer shown in two windows | API |
| K7 | The public harness (contract §16): typed keys and pointer events with an error log per key, a fake clock and clipboard, window size and top line, the screen as text plus a style dump, save and restore | K6, K9; K12 for restore | S–M | Every S4 test and every oracle case runs through it | API |
| K8 | Faces, themes and the scene: 14 base faces with parents, themes as data with the 16 ANSI colors, `Cell` with resolved colors, one composed grid with gutters, status lines, dividers, command-line rows and the cursor shape. The search, selection and bracket highlights computed at render time, moved here from K4. The search highlight takes K2's `Pattern`, and V2 decides smartcase | K1 | M | Screen and style snapshots. Face inheritance | API (types) |
| K9 | The calls die-render drives core through: `handle(input)`, `run_until_idle(now)`, `next_deadline()`, `scene()`. Pointer cells map to a window and a position | K6, K8 | S | Headless tests of the loop and the deadline | API |
| K10a | Files, read side: open into a buffer, the UTF-8 check (other files open read-only), the fingerprint, and your per-pattern settings applied on open | K1 | S | Temp-directory tests | AI |
| K10b | Files, write side: save through symlinks keeping permissions, a hard-linked file written in place, the `:w` refusal, auto-revert by polling | K10a | M | Temp-directory tests: a symlink and mode 0755 survive a save, a byte-exact round trip, a rename-over reverts, `:w` refuses and `:w!` writes, a rewrite with the same bytes leaves no undo step | AI; rules checked at the Files and session review |
| K11a | The command line: line editing and history, `:` and `/` input, `prompt` without candidates, `*messages*` | K8, K9 | S–M | A harness test per editing key. Cancel gives none | API (prompt) |
| K11b | Candidates and core commands: a 20-row flex-matched list, `find-file` with descent, `switch-buffer`, `kill-buffer`, `command-palette` | K11a, K10a | L | A harness test per command. A table of flex rankings. Descent into `crates/die-core/` | API (prompt) |
| K12 | Crash-safe session and `restart`: schema v1, an idle write off the main thread, unsaved text with file fingerprints, restored files and cursors, the lock handover and the wait for the new window's first frame. The session path comes from a safe `sys::paths` function (the rest of `sys` waits for M2), single instance uses `File::lock`, and the first-frame handshake runs over a Unix socket | K10b, K11a, F4a | M | A round trip of every field. A corrupt buffer is dropped alone. Build N reads build N+1's file, and the reverse. The idle write fires on the fake clock. A new build that panics before its first frame leaves the old one running | AI; schema at the Files and session review |
| K13 | Full session: the window tree, per-window cursor, selection and scroll, registers and macros, histories | K12, V4 | M | Harness save-and-restore gives equal state | AI |
| K14 | Pointer on text windows: a click focuses the window and places the cursor, a drag selects in visual state, the trackpad scrolls the view | K6, K9 | S | Harness pointer tests | AI |
| K15 | Hardening: core's share of a keystroke ≤ 1 ms p99 on a 10k-line file, rustdoc on every public item, fixes from use, the budget | all | M | Speed tests. No public item without docs | AI |
| K16 | `describe-key` and `describe-command` | K11b | S | Harness tests, including a shadowed binding | API |

### Renderer (`die-render`) and DIE.app

| ID | Deliverable | Needs | Size | Proven by | Owner |
|---|---|---|---|---|---|
| R1 | The crate, from S1's skeleton: pinned versions, trimmed AppKit features, objc-sys's `.m` feature off. `platform::macos::{appkit, metal, text}`, cfg-gated to macOS, is the only `unsafe`, and holds thin wrappers only: the atlas, row cache and instance building live outside it. The Apple crates sit in a macOS target table. On other platforms a stub `run()` returns "no window backend on this platform yet" | F1 | S | It builds, the budget counts it, `clean-build` passes, and it cross-checks for Linux | AI; you review `platform` |
| R2 | Offscreen renderer, scene to texture: the glyph atlas, row cache and diff, renderer-drawn box and block glyphs, Core Text with a fallback chain and combining marks, cursors. The test plan states the frame-data boundary between neutral code and the backend, and the instance layout is asserted | R1, K8 | L | CPU row tests, the GPU struct-layout assertion, glyph joins, advances, offscreen fidelity | AI |
| R3 | Window and loop: borderless, Option as Alt, render in the same run-loop turn as the input, occlusion, S1's resize and scale rules, wakes through the event loop. Its neutral code compiles on Linux | R2, K9 | M | `die-dev --check idle,resize,scale,occlusion` | AI |
| R4 | Input: `KeyEvent` mapping, the key monitor (Caps Lock, `C-/`, `C-Return`), pointer with click count and trackpad deltas. Modifier sides come from modifier-key events, and the mapping compiles on Linux | R3, K3 | M | S1's 50-chord matrix with every field checked. Mapping tests | AI |
| R5 | The macOS clipboard behind K6's `Clipboard` trait, with its change count (NSPasteboard's) and line or block kind | R3, K6 | S | A round trip on a private pasteboard. An outside copy is detected | AI |
| R6 | A latency and startup log behind an environment variable. S1's checks 1, 2 and 4–9 rerun on the real app | R3, R4 | S–M | `--check typing,firstframe` | AI; the hand runs are yours |

### die-vim

Each case's tranche comes from the keys it uses, not its topic file: a case lands with the last feature it needs. The counts are a first pass over the fixtures; V0 generates the exact lists. They add up to 306.

| ID | Deliverable | Needs | Size | Proven by | Owner |
|---|---|---|---|---|---|
| V0 | The oracle in the workspace: `tools/evil-oracle` with committed fixtures and its guards. A pending list (a case on it that passes, or a failure not on it, fails the test). `deviations.json` with the 24 excluded cases. A runner on K1's `Ctx`, moving to K7's harness when it lands | F1, K1 | S | All 306 cases validate, and every case starts on the pending list | AI; the deviations are yours |
| V1 | Motions, counts, insert sessions and their undo (about 66). New cases with `_`, `::`, `->`, tabs and wide characters | V0 | M | Those cases leave the pending list | Grammar |
| V2 | Search and moving between files: `/ ? n N * #`, incremental, every match highlighted, smartcase; `:e`, `:b` and `:N` (about 16) | V1, K2, K11a | S–M | Same, plus harness tests for `:e` and `:b` | Grammar |
| V3 | Operators, registers with the kill ring and clipboard sync, text objects (about 109, with most undo cases) | V1, K6 | L | Same | Grammar |
| V4 | Visual mode including block, `.`, macros, `R` (about 59) | V3, K5 | M | Same | Grammar |
| V5 | The rest of ex: `:w :q :wq :s`, and `:w! :e! :q!` (about 12) | V3, K10b, K11a | S–M | Same, plus harness tests for the file commands | Grammar |
| V6 | Geometry and windows (44): scrolling and recentering, `H M L`, wrapped lines, the `C-w` map, `:sp :vs :only :close`, `:q` on a split | V3, K4, K6 | M | Same, plus about 15 harness tests for window commands | Grammar |
| V7 | Close-out: every case outside `deviations.json` passes, the pending list is empty, the 24 hand-written fixtures pass, and dogfooding has added cases | V1–V6 | S–M | Exit test (d) | Grammar |

## 6. Risks and early warnings

| Risk | Early warning | Response |
|---|---|---|
| **One reviewer paces four tracks.** | An API diff waiting more than two days, or a review more than a week late. | The renderer, files and motions run on their own tests. Only API and grammar changes queue. |
| **Rebuilding S4's code breaks a subtle rule:** queued calls, objects dropped at check-in, undo attribution. | A ported S4 test is edited instead of the code. | Tests land first. Any change to one is called out at the next review. |
| **Layout cache and word wrap.** S4's first planted bug was a stale cache. S4 wraps by character, and so does the naive layout its property test checks against. | `mutate.sh` misses a planted bug. | The wrap rule is written into contract §9 first. A different session rewrites the naive layout from it. Cache and wrap bugs are planted. |
| **The command line and the session are the least measured estimates.** | Either passes its module limit (1.3k, 1.0k) before it's complete. | The core cut list. |
| **Restart misses 1 s:** launching through LaunchServices, the lock handover, loading the session. | F4a's stub restart takes over 500 ms. | Time it first. Trim AppKit features. Launch the bundle's binary directly if `open` is the slow part, after checking that the Dock and AeroSpace treat it the same. Load hidden buffers after the first frame. |
| **Session format drift.** Every restart crosses builds. | A restore drops a buffer while dogfooding. | A version field, per-buffer JSON read with defaults, no panic on load, the previous copy kept. |
| **Auto-revert edge cases:** an identical rewrite, a revert during an insert, a deleted file. | A rewrite with the same bytes shows up in undo. | One test for each. |
| **Evil quirks you'll feel:** the kill ring, errors at line edges that end macros, where `u` leaves the cursor. | More than 3 switches to Emacs to edit in a dogfood day. | Every switch becomes an oracle case first. |
| **The fixtures don't look like Rust.** No case has `_`, a tab or non-ASCII text. Your Emacs opens `.rs` files in fundamental-mode, where `w` stops at `_`. | A surprise on `snake_case`. | New cases in V1, run in fundamental-mode. A call you can veto. |
| **`cc` creeping in.** winit's lockfile already lists `cc`, for Android, which macOS never builds. Regex over a rope could bring in more. | `clean-build` fails. | It checks the macOS build graph and any build script that calls a C compiler, so a lockfile entry alone only warns. Don't weaken the check. |
| **Regex habits.** `\(`, `\|` and `\<` become Rust syntax, and replacements use `$1`. | Misses while dogfooding. | V2 and V5 add cases. |
| **Geometry.** The oracle's tty Emacs wraps one column earlier than your GUI. | Wrapped cases still failing in V6. | Take wrap columns from each fixture's layout. |
| **Renderer:** two objc2 generations, an untried AeroSpace rule, a restart that reflows the vterm tile twice. | An old objc2 type in a function signature. `platform` over 0.9k lines. The tile jumping at checkpoint 4. | Old types stay in `platform::macos::appkit`. The thin-wrapper rule. DIE.app's stable bundle id. Check at checkpoints 2 and 4. |
| **die-render near its cap.** | M0's size over 3.9k. | Its cut list. |
| **Code you can't read in a weekend** (09-B, 09-C). | A review takes much longer than its estimate. | Simplify before going on. That's the weekend rule doing its job. |
| **M2's copy state reuses die-vim's grammar.** | A motion that reads editor state instead of the text and layout. | Motions are plain functions from V1 on. |
| **Linux drifts while nobody runs it.** | A Linux-only failure found weeks after the change that caused it. | The cross-check in `cargo xtask ci` on every merge, and `cargo xtask ci --linux` on colima weekly and at each checkpoint. |

## 7. Deferred out of M0

- **To M1:** the decoration index, disabling an object after repeated panics, nested prompts, file-type modes and mode save and restore, `process` and `watch` as public API, `die-contract-tests`.
- **To M2:** processes, PTYs, the holder and the terminal helper, core's `sys` module, `die-term` and `die-term-sys`, the environment from config (PATH and the env file), the startup hook's cold-or-restored flag, key release in keymaps, and the clean-build check over the engine archive.
- **To M3:** placements, images, pixel buffers, Hide with a placement, the display link, atlas eviction.
- **Not scheduled:**
  - a file-system watcher (M0 polls instead)
  - editing CRLF and non-UTF-8 files (they open read-only)
  - horizontal scrolling
  - language-aware indentation
  - dark-coffee
  - opening a file in a running DIE from a vterm
  - an equivalent of `\\` (your fzf from home); `\f` descends from the buffer's directory instead
  - undo history across restarts; marks, the jumplist, an undo tree
  - S1's second-display check, whenever a display is at hand
- **In M0, after checkpoint 4, not gating it:** `describe-key` and `describe-command`, annotations in the command palette.

## 8. Doc edits

Applied on 2026-10-19, after 09 and 10 were answered:

- **Scope §12:** the M0 row links this plan and names the four checkpoints with their week ranges. "How the work is split" follows your answer to 09.
- **Scope §12, Testing:** window and file commands are tested by core harness tests, since the oracle has no window tree and no files. That's an exception to "hand-written fixtures are only for the deliberate deviations".
- **Guard 7:** the counting rule, and a line for die-vim (≤ about 5k, no cut list).
- **§10:** add `:w!`, `:e!` and `:q!`, which exit test (c) needs.
- **§5.9:** DIE.app holds a copy of the build (S3's advice) rather than pointing at the dev build. The old instance waits for the new one's first frame. The session's location and idle delay.
- **Contract §9, Layout:** where soft wrap breaks (at word boundaries, as `visual-line-mode` does), written before K4 changes the code.
- **Records:** done. The S2 and S5 records now say "accepted" on their first lines.
