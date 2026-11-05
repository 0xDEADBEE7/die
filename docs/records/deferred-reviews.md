# Deferred reviews, 2026-11-03

On 3 November you handed me every decision waiting on you, and asked me to replace checks that needed your hands with tests I run myself. Four reviews from [m0-plan.md](../m0-plan.md) §3 stood in for your read. This record holds what they found. The calls I made on them, which you can veto, are in [m0-log.md](../m0-log.md) under 2026-11-05.

## How they ran

Each review was one agent reading the code and testing its claims. A second agent then tried to refute each finding, and it could also propose a better fix. Both could run your Evil through `tools/evil-oracle/run-oracle.sh` and real Vim 9.1 where a claim needed them.

Every finding survived: none of the 82 was refuted. The second agent revised the proposed fix on 32 of them. Where it did, its version is the one to follow, unless the log's calls say otherwise.

These stand in for your review. They don't replace your exit tests, or using DIE at each checkpoint.

| Review | What it read | Findings | Major | Minor | Nit |
|---|---|---|---|---|---|
| Deviations | `deviations.json`, its 9 rules and V1's 6 hand-written fixtures | 14 | 4 | 9 | 1 |
| Text and keys | Rustdoc for text, undo, keys and keymaps, the layout queries, the wrap rule, the undo property test | 27 | 3 | 22 | 2 |
| Scene and `unsafe` | `Cell`, `Style`, `Scene`, faces, themes, the snapshot format, and `die_render::platform` | 21 | 3 | 11 | 7 |
| Deep read | Undo, key dispatch and die-vim's grammar core, line by line | 20 | 3 | 15 | 2 |
| **Total** | | **82** | **13** | **57** | **12** |

## Where each finding goes

69 are fixed in the round that follows these reviews, on three branches: `m0/review-core`, `m0/review-scene` and `m0/review-vim`. The other 13 wait for the package named, because they change code that package rebuilds.

Each line is a finding's first sentence. The full claim, evidence, proposed fix and verifier's verdict are in the JSON files below.

| ID | Severity | Goes to | Claim |
|---|---|---|---|
| dev-1 | major | die-vim branch | Rule 1 misses two cases whose Evil result comes from the empty line after the final newline: geometry/M-short-buffer and geometry/H-at-end. |
| dev-2 | major | die-vim branch | The ctrl-d-at-end note says 'Vim stays on the last line without an error'. |
| dev-3 | major | die-vim branch | Rule 1's die_vim text ('As in Vim: motions stop on the last line, j and gj on it signal an error...') doesn't decide the shapes where Vim and Evil-without-a-final-newline disagree. |
| dev-4 | major | core branch | Nothing decides whether the empty line after a final newline takes a screen row (S5 §4 leaves it open: 'Row 1 is also a question for core'). |
| dev-5 | minor | die-vim branch | Rule 1's evil text misdescribes the fixtures. |
| dev-6 | minor | die-vim branch | Keep 'Vim's line, Evil's column', but no fixture shows it. |
| dev-7 | minor | die-vim branch | close-brace-last-paragraph's reason says '} stops at the end of the last line', but the last line is 'c', where the end and the start are the same column, so the fixture can't catch a die-vim that goes to col 0. |
| dev-8 | minor | die-vim branch | '. repeats the last change made inside the macro' is too narrow: a . |
| dev-9 | minor | die-vim branch | The Vp bug isn't limited to Vp: VP, and Vjp over the last two lines, break the same way in Evil, so the rule should cover any linewise visual put over the buffer's last line(s). |
| dev-10 | minor | die-vim branch | 58 cases added after S5's Vim comparison were never compared with Vim. |
| dev-11 | minor | die-vim branch | The runner checks that a hand-written case is a deviation naming its file, but not that it keeps the oracle case's keys, mode and input. |
| dev-12 | minor | die-vim branch | The 18 deviations without fixtures are excluded and untested until each tranche writes its own expected values, so those values could end up fitted to the implementation. |
| dev-13 | minor | die-vim branch | The rule fixes the keys as :s/(foo) (bar)/$2 $1/ but doesn't state the replacement-template rules. |
| dev-14 | nit | die-vim branch | Wording gaps that V2 and V6 would otherwise decide on their own: Evil also ignores a count on ?; an empty search with no previous pattern; nH for any n; C-r with nothing to redo. |
| WRAP-1 | major | core branch | The wrap rule as written contradicts Emacs's visual-line-mode on whitespace at the row's end, and says nothing about several cases that two independent implementations would resolve differently. |
| WRAP-2 | major | core branch | An end-of-line or end-of-buffer position on a full row starts an extra blank row. |
| TEXT-1 | major | core branch | A change that changes nothing still bumps the revision, logs a Change, records an undo step that does nothing, and clears redo. |
| TEST-1 | minor | core branch | undo_history_matches_a_model lets six plausible undo bugs pass the whole die-core suite, and three more are caught only by unit tests. |
| LAYOUT-3 | minor | core branch | The gutter rules that K4 and the naive layout need are unwritten. |
| KEYS-1 | minor | core branch | printable() returns the OS text even with Ctrl held, and winit's text for Ctrl chords is a control character. |
| LAYOUT-4 | minor | K4 | pos_at_vcol, which the contract lists "for j/k goals", lands on a tab or wide character that spans the goal column. |
| TEXT-2 | minor | core branch | The change log's order within a transaction, and the presence of undo and redo changes in it, are undocumented. |
| UNDO-2 | minor | core branch | Two group behaviours are undocumented and untested: an explicit undo group that's never ended swallows every later command into one step, and undo or redo silently ends an open group. |
| SEARCH-1 | minor | core branch | In your fundamental-mode, Evil's * and # treat `_` as a non-word character, so `*` on foo matches inside foo_bar. |
| KEYS-2 | minor | V3/V4 | Because register text is parsed as notation, `@` on a register that `q` didn't record fails on yanked Rust code, or presses keys named in it. |
| KEYS-3 | minor | core branch | Committed text of more than one code point comes back from notation as several keys. |
| LAYOUT-5 | minor | K4 | `wrap: bool` can't express character wrap. |
| LAYOUT-6 | minor | K4 | There's no public way to read or set a window's scroll position. |
| KEYS-4 | minor | scene branch | State has no replace state. If `R` runs as Insert, it draws a bar instead of Evil's underline and shares insert state's keymap. |
| KEYS-5 | minor | core branch | In your GUI Emacs and in Vim, C-[ is Escape, C-i is Tab and C-m is Enter. |
| KEYS-6 | minor | core branch | Notation accepts three spellings that can't mean what they say: `<C-S-2>` binds a chord no key produces, `<S-Space>` is plain Space, and an unclosed `<C-a` binds four keys silently. |
| KEYS-7 | minor | core branch | KeyEvent can't carry Num Lock or keys that Key has no name for, which Ghostty's encoder accepts. |
| SEARCH-2 | minor | V5 | Pattern exposes no captures, so V5's `:s` with `$1` would need a second regex engine in die-vim, with its own syntax settings. |
| LAYOUT-7 | minor | K4 | DisplayLine and LayoutCell expose their fields, and die-vim reads them, yet K4's compact cells will change them. |
| TEXT-3 | minor | V3/V4 | Selection can't say "each line to its end" for V4's `<C-v>$`. |
| TEXT-4 | minor | core branch | add_marker accepts a position inside a character, where decorate refuses one. |
| DOCS-1 | minor | core branch | 175 public items in scope have no rustdoc, including the units that matter most. |
| TEXT-5 | minor | core branch | Text inserted at a cursor pushes that cursor forward in every window. |
| KEYS-8 | minor | R4 | S1's hand check, which was to come before this review, recorded 13 of 50 chords. |
| KEYMAP-1 | nit | core branch | `<any>` doesn't catch a key that fails under a prefix, but the contract reads as if it does. |
| TEXT-6 | nit | core branch | Styled insertion has no work package, and its likely shape would change Tx::commit's return type. |
| unsafe-1 | major | scene branch | The unsafe confinement can be bypassed three ways that every ci check accepts. |
| scene-1 | major | scene branch | Each text cell's style is found by scanning every decoration and every search match, with a string-keyed face lookup per decoration per cell. |
| scene-2 | major | K9 | The Scene holds core's pointer map (`positions`) and no row revision. |
| scene-3 | minor | scene branch | Layout gives a newline no cell, so compose never draws one. |
| scene-4 | minor | K4 | A Hide decoration's replacement text is drawn in two styles. |
| scene-5 | minor | K4 | Virtual text, and a Hide replacement beyond the faces of the text it hides, can't carry a face. |
| scene-6 | minor | scene branch | CursorShape and Cursor fall short of M0's V4 and M2's terminal: - No underline (hbar) shape. |
| scene-7 | minor | scene branch | Cell can't yet hold what a ghostel-level terminal needs, and adding it later breaks every cell producer. |
| scene-8 | minor | scene branch | `Placement` means "where a window sits in the frame". |
| scene-9 | minor | scene branch | The snapshot silently merges every style past the 62nd into '?', so two different styles can produce the same snapshot. |
| faces-1 | minor | scene branch | A decoration or provider face name that no one declared draws as nothing, with no message, though a theme's undeclared face is reported. |
| unsafe-2 | minor | scene branch | Face::glyph passes pointers made from references to the first element only (`&mut chars[0]`, `&mut glyphs[0]`). |
| unsafe-3 | minor | scene branch | The font descriptor comes from a raw-pointer dereference whose lifetime the compiler doesn't tie to `descs`. |
| unsafe-4 | minor | scene branch | platform/mod.rs says logic "lives outside this module and calls in". |
| scene-10 | nit | scene branch | The status line's text is hard-coded to " " plus the buffer name. |
| faces-2 | nit | scene branch | mono-dark's mode lines draw a 1 px `:box` in #3a3a3a, which DIE's Style can't express. |
| faces-3 | nit | scene branch | Style's builder can't set an underline colour, and can't set an attribute explicitly off, which Emacs's `:weight normal` and `:underline nil` need to override an inherited one (mono-dark's org-tag and nobreak-space do). |
| unsafe-5 | nit | scene branch | platform::macos's ratchet is 1,600 lines. |
| unsafe-6 | nit | scene branch | The module's rules don't yet cover what R3 and R4 bring: - Rust closures that AppKit calls (the NSEvent monitor block, delegates, a display-link callback), where a panic must not unwind into Objective-C frames; - AppKit's main-thread requirement. |
| unsafe-7 | nit | scene branch | Scope §12 implies DIE's only unsafe is in the named modules. |
| unsafe-8 | nit | scene branch | from_file turns the path into a UTF-8 string, so a font under a non-UTF-8 path fails. |
| DR-01 | major | core branch | After `u`, die-vim puts the cursor at the start of the undone range (`history()` uses `r.start`). |
| DR-02 | major | die-vim branch | V1-1 leaves the cursor on the empty line after a final newline. |
| DR-03 | major | core branch | An undefined key sequence doesn't clear the prefix argument. |
| DR-04 | minor | core branch | State is per buffer, but `Ctx::set_state` and `Ctx::state` always act on the focused buffer, while die-vim's commands act on `Args::window`/`Args::buffer`. |
| DR-05 | minor | die-vim branch | A motion that moves fewer times than its count (`5h` near the line start) signals an error, yet die-vim drops the goal column. |
| DR-06 | minor | die-vim branch | In insert state, `<Up>`/`<Down>` take the current column each time, so a run of them through a short line loses the column. |
| DR-07 | minor | die-vim branch | The owner's Emacs runs global-visual-line-mode with line-move-visual. |
| DR-08 | minor | die-vim branch | `C-a` with a count ignores the count. |
| DR-09 | minor | die-vim branch | Replaying a session for its count takes time quadratic in the count, and nothing can interrupt it. |
| DR-10 | minor | die-vim branch | die-vim's word scripts are only Han, Kana, Hangul and Other, so Latin followed by Cyrillic, Armenian, Hebrew, Arabic, Devanagari, Thai, Georgian or IPA reads as one word. |
| DR-11 | minor | die-vim branch | grammar.rs promises that the cursor never rests on a non-empty line's newline, but `e` onto a word that ends in a combining mark at the end of a line lands on the newline. |
| DR-12 | minor | K4 | Layout draws every control character (\r, \x01, …) as `?`, one column wide. |
| DR-13 | minor | core branch | An edit made by host code between commands (the harness, an embedder, a file revert run outside a queued callback) joins the previous command's undo step, so one `u` takes back both. |
| DR-14 | minor | core branch | An undo inside an open group resets `depth` to 0. |
| DR-15 | minor | core branch | If a silent edit lands inside a group that's still collecting (an extension rewriting what you're typing), `record` drops the whole history at your next keystroke without saying so. |
| DR-16 | minor | die-vim branch | Escape after `f`, `F`, `t` or `T` should cancel quietly, as in Vim and in Evil, where the read quits silently. |
| DR-17 | minor | die-vim branch | Suspicion, a latent gap with no current miss: the runner compares only how many errors there are. |
| DR-19 | minor | core branch | Suspicion, no failing test: silent shifts pile up with no bound. |
| DR-18 | nit | K7 | The runner fails any case in which die-vim reports "… is undefined", and the oracle doesn't record Evil's own `undefined` calls, which are a ding plus a message, not a signal. |
| DR-20 | nit | die-vim branch | Suspicion, no failing test: `Vim::inserts` is never cleaned up when a buffer is killed in insert state, so one Session (with its edit list) leaks per such buffer. |

## Files

- `findings-core.json`, `findings-scene.json` and `findings-vim.json`: the findings for each branch, with their verifier's verdict.
- `findings-later.json`: the 13 for later packages, keyed by package.
- `reviews.json`: each review's summary and how it checked things, with its verdict on each surface it read.
- The reviewers' working material, which the fix round reused, was removed once it merged. It's in git history under `docs/records/deferred-reviews/work/`, at commit 41ec7ef.
