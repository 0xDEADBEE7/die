# M0 log

What merged, what waits on you, what changed in die-core's API, and where the budget stands ([m0-plan.md](m0-plan.md) §3). Newest first.

## 2026-11-05: the reviews you deferred, and the calls I made on them

**Your instruction, 3 November:** "for all the decisions waiting on me I am happy to defer to your own judgement. If you need me to test certain things, find ways you can test independently."

So until you say otherwise:
- 09-B's wait for your approval is off. API and grammar changes merge when `cargo xtask ci` passes and an independent review's findings are fixed, and this log lists each one for you to veto.
- Checks that need your hands become tests I run. They post events to DIE's own window and never touch your desktop's focus or input.

**Waiting on you: nothing.** The deviations review and S1's key check, listed below as waiting on you, are both closed. You can overturn any call in this entry at any time.

**Merged:** PRs #6–#9: this entry and its record, then the three fix branches below. `cargo xtask ci` passes on a fresh clone at each merge. `mutate` catches 26 of 26, `ci --linux` passes on aarch64, and the oracle's fixtures regenerate byte for byte from your Evil.

**The reviews.** Four reviews stood in for your read, each checked by a second agent trying to refute it. The findings are in [records/deferred-reviews.md](records/deferred-reviews.md).

| Review | Findings | Major |
|---|---|---|
| Deviations | 14 | 4 |
| Text and keys | 27 | 3 |
| Scene and `unsafe` | 21 | 3 |
| Deep read of undo, dispatch and the grammar core | 20 | 3 |
| **Total** | **82** | **13** |

None was refuted. The second agent revised the proposed fix on 32 of them.

**The fix round.** 66 of the 69 findings due now are fixed. The other three (WRAP-2, LAYOUT-3 and dev-4) are done in the contract, and K4 builds their code by call. The other 13 findings wait for the packages that rebuild their code (below). Each branch had a fixer, an independent reviewer who reverted the fixes one at a time to see tests fail, and a finisher. The first attempt ran into the weekly usage limit on 3 November, about 10 minutes in, so I restarted it today from its partial scene edits.

| PR | Branch | Findings | The review then found | Biggest change |
|---|---|---|---|---|
| #7 | core | 26 | 15 more (no majors), all but one fixed; the last was a merge step, done in #9 | `u` restores the cursor from before the step's first change. Contract §9's wrap, row and gutter rules, whole, checked against about 90 tty Emacs cases |
| #8 | scene | 19 | 5 more (1 major: another route round the `unsafe` lint), all fixed | Composing a dense 300×100 frame with every search match and 11,039 faces fell from 99.5 ms to 0.89 ms. A plain frame costs about 12–15 % more |
| #9 | die-vim | 24 | 6 more (1 major: a regression in word classes), all fixed | Every deviation has a hand-written fixture, written before its code: 37 cases under 10 rules |

**Oracle:** 248 of 523 cases pass, and V1's pending list is empty. The count grew from 346 for two reasons. The reviews added 159 cases, 116 of them motions (the goal column and words across scripts). And all 37 deviations now run against hand-written fixtures, where 18 used to sit out.

**Budget:**

| Crate | Lines |
|---|---|
| die-core | 4,665 of 14k |
| text and undo | 1,120 of 1,250 |
| keys, keymaps, describe | 598 of 650 |
| faces and scene | 900 of 950 |
| die-render | 123 (macOS build 120, Linux build 19) |
| `platform::macos` | 104 of 900, down from 1,600 (call 16) |
| die-vim | 1,332 of 5k |
| product code | 6,167 |
| xtask (dev) | 3,480, up from 3,035: the `unsafe` lint and `sources` step |

**API changes, for your later review.**
- **Added:** `Ctx::state_of`, `Ctx::set_state_of`, `Pattern::word`, `Cell::new`, `WindowPlacement` (replaces `Placement`), `Style::{bold_is, italic_is, strike_is, underline_color}`, and in die-render `platform::backend::ready()`.
- **Changed:** `State` gains `Replace`, `CursorShape` gains `Underline`, `Key` gains `Unidentified`, `KeyEvent` gains `num_lock`, `Decoration` gains `seq`, and `Cell` is `#[non_exhaustive]`. `add_marker` returns `Result<MarkerId>`, `remove_marker` returns `Option<usize>`, and `set_sticky` returns `Option<()>`.
- **Behaviour with the same signature:** an undefined key clears the prefix. Empty changes make no revision or undo step. Edits between units are their own undo step. Undo and redo leave an open group open. `undo` can return `UndoConflict`. `C-[`, `C-i` and `C-m` fold. A multi-character cluster can't be bound. `printable()` is `None` with Ctrl, Alt or Super held.
- **die-vim's grammar:** adds `Target::short(pos, err)` and `forget_goal(t, w)`. A `Fail` from a motion now clears the goal column.

**S1's key check is partly done.** You pressed 13 of the 50 chords on real hardware, and every field mapped correctly. `results/interactive-keys.md` reports 7 failures, but all 7 fail only the modifier-event check, because Ctrl stayed held from `C-a` through `C-S-a`. The event log shows it. R4 covers the rest with an automated test.

### Calls (veto any)

The IDs are the findings in the record. Where a finding's verifier gave a better fix, that one is followed unless a call says otherwise.

**Layout, wrapping and rows.** Contract §9's text changes now, and K4 builds the code.

1. **WRAP-1: the wrap rule is Emacs's.** It follows `visual-line-mode` with `word-wrap`, including whitespace at a row's end and the cases the old text left open. With `Wrap::Char` the same rule applies with no break points.
2. **WRAP-2: an exactly full line gets no blank row after it,** as in Emacs. Rows, `H M L`, `gj gk` and scrolling then match Evil. The only visible cost is that insert state's bar sits at a full row's right edge.
3. **dev-4: the empty line after a final newline keeps its own screen row,** with no line number, as in Emacs. Evil's window positions stay oracle cases.
4. **LAYOUT-3: the gutter rules get written down,** including `LineNumbers::Visual`, the style you use.
   - `max_width` counts text cells only, so you get 80 columns of code. `visual-fill-column` includes the gutter, so this deviates from it. Your `.rs` files show no line numbers in Emacs, so rows still break where Emacs breaks them.
   - Relative numbers count display lines, as `j` does. Emacs's count hidden lines. It's listed in scope as a deliberate deviation.
5. **LAYOUT-5: `enum Wrap { Off, Word, Char }` replaces `wrap: bool`,** in K4. M2's copy state needs character wrap. If K4 runs over budget, it ships `Off` and `Word`, and `Char` comes with M2.
6. **scene-5: virtual text and `Hide` replacements become a styled string,** in K4: `Styled { text: String, faces: Vec<(Range<usize>, &'static str)> }`, with byte ranges. manifold's fold strings need faces, and contract §9's styled insertion shares the shape.

**Text, undo and keys.**

7. **DR-01: `u` puts the cursor back where it was before the step's first change,** as your Evil does. Core takes the selection just before that first edit, not at `begin_undo_group`, and die-vim's `u` uses it. This amends contract §9, the rustdoc and the A14 note. From now on, a die-vim command that moves and edits in one go (`p`, `J`, `dd` from mid-line) makes its edit first.
8. **DR-04: `Ctx::state_of(buf)` and `Ctx::set_state_of(buf, s)` are new.** `state()` and `set_state()` stay as sugar for the focused buffer, and die-vim uses the `_of` forms for `Args`' buffer. Without them, running `i` for an unfocused window leaves an undo group open for good.
9. **TEXT-5: text inserted at a cursor pushes only the acting window's cursor forward.** Other windows' cursors stay before it, as in Emacs. An extension that wants a window to follow its output sets that window's cursor.
10. **KEYS-5: `C-[` is Escape, `C-i` is Tab and `C-m` is Return,** folded in `chord()`, as in your GUI Emacs and in Vim. So `C-i` can't be bound apart from Tab. The raw event keeps the difference for M2's kitty passthrough.
11. **SEARCH-1: `*` and `#` follow Evil.** `Pattern::word` treats `_` as a non-word character, so `*` on `foo` matches inside `foo_bar`, as V1's `w` already stops at `_`.
12. **KEYS-4 and scene-6: `State::Replace` and `CursorShape::Underline` arrive now,** with rustdoc saying what each shape does to the glyph under it. Hollowing the cursor when the app loses focus waits for K9's `Input::Focus(bool)`. There's no blink in M0, which is listed as a deviation.
13. **scene-7: `Cell` gets `#[non_exhaustive]` and a public `Cell::new(glyph, fg, bg)`,** and the rule that producers resolve reverse, faint and invisible themselves. Overline, the link id and the cell frame for surfaces come with die-term in M2.
14. **faces-2: status lines have no box.** mono-dark's 1 px `:box` is the one attribute DIE's `Style` lacks, and it would take renderer work for one face. Dividers separate the windows. It's recorded in scope §8.
15. **unsafe-4: neutral code names a `platform::backend` alias,** with a stub on other platforms holding only what neutral code names, and no traits. R2's neutral code takes data in and gives data out, and only the driver calls the backend.
16. **unsafe-5: `platform::macos`'s ratchet drops from 1,600 lines to 900,** the size your `unsafe` review was planned for. Going over it forces the thin-wrapper question, with a count in hand.

**die-vim and the deviations.**

17. **dev-3: rule 1 means "Evil without the final newline",** for the cursor and for failures. It's what die-vim's `}` already does. Each rule-1 fixture comes from running the oracle on the text without its final newline, then putting the newline back.
18. **dev-8: inside a macro, `.` does what Vim does. After `@a`, `.` replays the macro,** as Evil does, without Evil's "No previous macro" error. An oracle case that tells the two models apart pins it.
19. **dev-10: three behaviours stay Evil's, though Vim differs:** `N%` goes N percent through the characters, `e` stops on empty lines, and Esc or a failed motion resets the goal column. They come from Evil's and Emacs's design, V1 already passes them, and moving them would add deviations for things rarely noticed.
20. **dev-12: all the remaining hand-written fixtures get written now,** once the rule texts settle, and go on the pending list. Expectations written before the code can't be fitted to it.
21. **dev-13: `:s` replacement templates are Rust's** (`$N`, `${N}`, `$name`, `$$`). So `$1x` names a group called `1x`, and a group followed by a letter needs `${1}x`. The rule says so.
22. **DR-11: `e` on "café" with a combining mark lands on the é, as in Vim,** mid-line and at a line's end. Evil's position comes from Emacs's command loop pushing point out of a composition, not from Evil's grammar. **This changes one of your fixtures:** `motions/combining-mark-e` becomes a deviation with a hand-written fixture.
23. **DR-18: the oracle runner's error checks wait for K7's per-key log.** Until then, DR-03's fix is pinned by a plain die-vim test and a die-core keymaps test.

**Assigned to later packages, with the calls that go with them.**

| Package | Findings | The call |
|---|---|---|
| K4 | LAYOUT-4, LAYOUT-5, LAYOUT-6, LAYOUT-7, DR-12, scene-4, scene-5 | `pos_at_vcol`, the scroll API, opaque `DisplayLine` and `LayoutCell`, control characters drawn as `^M` in two columns, and calls 5 and 6 |
| K7 | DR-18 | Call 23 |
| K9 | scene-2 | `Scene` becomes drawing-only, and the pointer map moves into `Ctx` as `position_at(row, col)`. `Input::Focus` arrives too |
| V3/V4 | KEYS-2, TEXT-3 | `@` types a register's text literally unless `q` recorded it, and core marks recorded registers. A block selection can reach each line's end |
| V5 | SEARCH-2 | `Pattern` gets captures for `:s` |
| R4 | KEYS-8 | The rest of S1's key check, as a test that posts layout-derived events to DIE's own window with `CGEventPostToPid` |

### More calls, from the fix round (veto any)

**Layout rules in contract §9.** K4 builds these.

24. **No number on any display line that starts at the end of the text,** so an empty buffer's only row has none either, as in tty Emacs. It still counts for other rows' distances.
25. **Filler cells don't count toward tab stops,** so display columns don't depend on the width. In `aaaaaaaaa中<Tab>b` at 10 cells, `b` lands at column 7 here and 6 in tty Emacs. It's named as a deliberate difference.
26. **Control characters split across rows the way a tab does,** unless they're a break point, when they move whole. U+0080 to U+009F draw as `\ooo` in four cells.
27. **A cluster wider than a whole row stays at the row's start and runs past its end.** This is DIE's own rule, because Emacs gave no evidence. Any cluster wider than the row reaches it.
28. **Replacement and virtual text break like text.** Where virtual text shows at a `Hide` comes from S4's code: before a replacement, not at all at the start of a `Hide` with no replacement, and shown at a `Hide`'s end.
29. **With a fold joining lines,** the cursor's row in Relative and Visual shows the display line's first line's number. Emacs may show point's own line, but no probe confirmed that.

**Core.**

30. **`state_of` on a dead buffer reads `Normal`, and `set_state_of` ignores it.** That's softer than C4. Veto if you'd rather have `Option` here too.
31. **A running command that moves the focus or opens a window goes on acting for that window,** and its nested callers keep their own. Outside a command, the focused window acts.
32. **A surface's `event()` no longer sees the count,** because DR-03 clears the prefix before calling it. Nothing needs it yet.
33. **`Decoration::seq` is public now,** so the naive layout can read the order. It's the one layout change ahead of K4.
34. **`LOG_CAP` stays a constant.** A8 recommended a setting, but nothing needs one.
35. **`Pattern::word` accepts empty text.** One known limit: with `ignore_case` and a literal whose folded forms differ in length, backward search can skip a match enclosing a rejected shorter one.

**Scene and the `unsafe` confinement.**

36. **`cargo xtask sources` is a new ci step.** Every file the compiler read must be one the lint reads, and a crate may compile only `.rs` files in its own directory. So the first `include_str!` of a non-`.rs` file, likely R2's shaders, has to name an exception there.
37. **The lint is stricter than the review asked.** It rejects `global_asm!` outside `platform::macos`, and any attribute that starts with a macro fragment, so the `$(#[$m:meta])*` idiom isn't available. The workspace has no `macro_rules!` today. It also rejects `NonNull::from(&mut x[0])` and `from_ref(&a[0])` in `platform::macos`. Known false positives: a format argument named `path`, and a variable named `restriction`. Rename to avoid them.
38. **Two limits stay.** A macro-built `allow(warnings)` goes unseen, though it can only hide warnings. And a proc-macro crate added as a workspace member could emit `unsafe` from a string. There's none today, and one would be a new member you'd see.
39. **`platform::backend::ready()` is the stub's one item,** so the cross-check can prove the alias.
40. **A cell past a row's end maps across the whole of `room`,** including a centered window's right margin. Rows below the buffer's last line stay `None`, for K14 to decide.
41. **A linewise selection draws the newline's cell on every selected line,** following mono-dark's region. Nobody checked what Vim draws there. A provider's `visible` range now includes a drawn newline, one byte more than before.
42. **xtask gains `serde_json`** to read cargo's messages. The oracle already pins it, so the lockfile adds no package.
43. **The speed tests get three tries each,** so a busy machine doesn't fail them. A return to the per-cell scan misses by far more than that.

**die-vim.**

44. **`$` and `%` are word characters,** so `foo$bar` is one word, as in your fundamental-mode Evil. V2 should pass die-vim's word test to `Pattern::word`, so `*` agrees.
45. **Word classes have known gaps.** `class()` still uses Rust's `is_alphanumeric`, not Emacs's word syntax. So marks like virama and Thai tone marks split words, and emoji count as punctuation, as on `main` before. One small regression is new: a mark outside the main blocks next to a letter of a different script now splits (`a` + U+0902 + `b c` with `w`: Evil 6, here 4). Fixing all of it means about 343 more table ranges, and I left it for text you rarely meet.
46. **A count past the caps fails with "Count too large",** keeping the session's first copy, where your Evil would freeze.
47. **A deviation with no hand-written fixture now fails the oracle test,** so none can sit silently out of the comparison.
48. **Insert-state Up and Down onto a tab or wide character land before it,** as your tty Evil does. Normal `j` and `k` land after it, as Evil's do. The two differ on purpose.
49. **`$j` on the last line no longer fails.** `j` and `k` with no line to go to signal only if the goal column wouldn't move the cursor, as in Evil. `gj`, `+` and `C-d` are unchanged, and V6 checks them.
50. **`f<Esc>` and a failed Left, Right, Backspace or `C-w` in insert state forget the goal column,** as Evil's do. `f<Esc>` comes from reading Evil's source, because a quit ends the oracle's macro. A plain die-vim test pins it.
51. **`f` or `t` forward from a line's last character says "End of line",** Evil's message. Messages aren't compared (call 23).
52. **Rule 1's macro fixtures take their registers from the run without the final newline.** With it, Evil's macro runs one more `x` on the empty line. This refines call 17's recipe.
53. **DR-11's rule is a new row in S5's table, row 22,** because D4 says each exception is listed there.

### Notes for the next packages

- **K4:** the code half of WRAP-2, LAYOUT-3 and dev-4 is listed in its plan row. Contract §9 disagrees with `layout.rs` on purpose until then. `snap` must cost the same whatever the line's length: today `normalize_views`, `snap` and `dl(pos)` lay out the whole logical line on every commit, and `set_selection` does it twice more. `MAX_REPEAT` and `MAX_REPLAY` can rise after that. K4 also decides whether the newline's cell still draws at `col == text_w` in a centered window, and keeps or replaces `TextBuf::line_end_on`.
- **K7:** state whether `run_until_idle` waits for outstanding jobs before the next key, as the oracle's runner now does. Separate errors from informational messages (call 23).
- **K9:** the positions rule moves with the pointer map, and so do `tests/scene.rs`'s position tests.
- **R2:** build on `platform::backend`. Shaders in their own file need an exception in `sources` (call 36).
- **V2:** pass die-vim's word test to `Pattern::word` (call 44).
- **V3:** bind `i<any>` and `a<any>` (contract §7), and `<Esc>` in `OperatorPending` to `vim-cancel`. Extend inclusive ranges to a grapheme's end. Operators go on after End of line or End of buffer and stop after any other signal, which isn't the same split as which failed motions keep the goal. Commands that move and edit make their edit first (call 7).
- **V6:** the `wrapped/*` cases need K4's word wrap as well as K7. Check `gj`, `+` and `C-d` against `j`'s goal-column rule (call 49).

**Next:** K4, K5 and R2, with V3's operators alongside them. V2 waits, because its search prompt needs K11a, which needs K6 and K9, which need K5. So V2 can't come before V3, as I said earlier. K4's naive layout gets written first, by a different agent, from contract §9's text alone.

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
