> **Decision record for spike S4 (scope §12), 2026-10-19. Status: accepted 2026-10-19.** You answered "go with your recommendations on the S4 decisions", so every recommendation in §3 and §5 stands, and the proposed edits in §1 are folded into extension-contract.md and scope.md. The spike's code is throwaway. Its tests are meant to carry over into `die-core`.

# S4 decision record: core API and layout

Date: 2026-10-19. Spike: `spikes/s4-core-api/`, a standalone cargo project. Toolchain: rustc and cargo 1.96 only. Machine: Apple M5 Pro.

## 1. Verdict

**Pass.** All 13 criteria pass. Everything below was run on 2026-10-19:

- 48 tests in the spike pass, and the review prototype's 8 tests pass unchanged in `proto-orig/`.
- Two property tests run 10,000 cases each, in 8.8 s and 0.6 s.
- I planted 8 bugs in the core, one at a time. The property tests caught all 8.

What it shows:

- **One layout function serves render and motions, with full Hide.** A Hide can start and end mid-line and span lines, which joins them into one display line. It can show replacement text or nothing, and it can sit next to virtual text, tabs and wide characters. Render, the motion queries and an independent naive layout agree on every row and every cursor position in the property test. **The fallback isn't needed:** Hide doesn't have to be limited to whole lines, or virtual text to the end of the line.
- C2, C3, C4, C14, C15, C16 and C17 hold as amended on 2026-10-19. The spike adds clarifications, not reversals.
- Undo across silent edits works. History is rebased over silent edits lazily, so your text comes back exactly. That holds even when an extension writes above your text in the middle of an insert session.
- No core change was needed to pass the criteria. Every first failure was in my tests (§5).

What it doesn't show:

- Speed on big files. The layout cache is cleared on every edit, and decorations move with a linear scan. That's fine at spike sizes. Production needs per-line invalidation (§5).
- Anything outside the criteria: processes, files, the session, faces, the command-line UI, the window split tree, and C9's per-object disabling.

### Proposed edits to extension-contract.md

These are proposals. I haven't made them.

1. **Header (line 7).** Replace "Spike S4 (scope §12) checks the details before M0's code is written. Anything it changes comes back to this doc." with:
   > Spike S4 checked the details on 2026-10-19 ([records/s4-core-api.md](records/s4-core-api.md)). Its clarifications are folded in below.
2. **C2.**
   - Replace "`Gone` if it was killed" with "`Gone` if its buffer was killed (never for `with_ext`)".
   - Replace "`Missing` if it never existed" with "`Missing` if there's no such object: the extension isn't registered, the buffer has no such mode, or its surface is another type".
   - Replace "Killing a buffer while its surface is checked out marks it dead at once, and the object is dropped at check-in." with "Killing a buffer while its surface or a mode is checked out marks it dead at once, and the object is dropped at check-in. A killed buffer's modes are dropped, not disabled: `on_disable` doesn't run."
   - In the rationale, replace "The prototype passes re-entry, panic and kill-during-checkout tests." with "Spike S4 passes re-entry and panic tests for all three, and kill-during-checkout tests for surfaces and modes."
3. **C3.** Replace "Calls that receive `Ctx` (hooks, callbacks, `on_resize`) are queued and run from the top of the event loop. They are never nested inside another extension call, except through an explicit `ctx.run`." with:
   > Calls that receive `Ctx` (hooks, callbacks, prompt answers, `on_enable`, `on_disable`, `on_resize`) are queued and run from the top of the event loop. They are never nested inside another extension call, except through an explicit `ctx.run` or `ctx.reveal`. Calls that get no `Ctx` (`render`, `decorate`, key observers) run synchronously.
4. **C4.** After "A handle stored in a mode is cancelled when its buffer is killed." add: "If the mode is checked out at the time, the handle is cancelled when the mode drops at check-in."
5. **C14.** After "Read-only ranges block only `User` edits." add: "Undo and redo are `User` edits too. A `User` insertion is blocked only strictly inside a read-only range. At an edge it's allowed, and the range's stickiness decides whether the new text becomes read-only (§9)."
6. **C16 rationale.** Add: "Spike S4 confirmed this with mid-line and multi-line Hides, so Hide isn't limited to whole lines."
7. **§5, Text row.** Replace "`text(buffer)` returns a handle for reading, editing, markers, decorations, cursors, selections and layout queries (§9)" with "`text(buffer)` and `text_mut(buffer)` borrow a buffer's text for reading, editing, markers, decorations, per-window cursors and selections, and layout queries (§9)".
8. **§5, Commands row.** Replace "`feed_keys(keys)`, which queues keys as if typed; a key observer, so die-vim can record macros" with "`feed_keys(keys)`, which queues keys as if typed, ahead of further typed input; a key observer that sees each key before dispatch, marked typed or fed, so die-vim can record macros (`qb@aq` records `@a`, as Vim does)".
9. **§6, args.** Add a line: `deco         for a Hide's reveal command: the decoration being revealed (§9)`.
10. **§7.**
    - Replace "Cmd-Esc swallows its own repeat and release." with "Any key DIE takes in passthrough, Cmd-Esc or a Cmd-layer binding, swallows its own repeat and release, so a program never sees a release without its press."
    - After the specificity bullet, add: "**A binding can't also be a prefix** of a longer binding in the same keymap and state, such as `g` and `gg`. Registration reports it, along with the C8 check." This depends on decision A18 below.
11. **§8, Mode.** Mark `on_enable` and `on_disable` "queued (C3)", as `on_resize` already is.
12. **§9, Editing.**
    - Replace "Edits happen inside a **transaction**, which carries an **origin**, `User` or `Extension` (C14)." with "Edits happen inside a **transaction**: a set of changes in the coordinates of the text before it, applied all or none. It carries an **origin**, `User` or `Extension` (C14)."
    - Replace "An insert session or `3dd` is one undo step spanning several commands, and undoing it restores the cursor." with "Each command or callback is one undo step by default, so `3dd` is one step. An explicit group spans commands, as an insert session does. Undoing a step restores the cursor from before it."
    - After "your text comes back exactly." add: "If a silent edit changed the text an undo step needs, undo refuses with an error and drops the older history."
13. **§9, Anchored decorations.**
    - After "their ranges move with edits, the way markers do." add: "Each end has a gravity. By default, text inserted at either edge stays outside the range."
    - In the table, replace the Virtual text effect with "Shows text just before a position, without adding it to the buffer. An end-of-line hint anchors at the newline."
14. **§9, Layout.** Add four queries to the list:
    - `display_line`: the display line holding a position, with its lines, rows and cursor positions
    - `pos_at_vcol`: the position at a display column, for `j`/`k` goals
    - `snap`: the nearest cursor position in a direction, for `h`/`l`
    - `screen_pos`: where a position is drawn in a window

    Then add a paragraph:
    > **Cursor positions.** Layout decides where the cursor can be. A Hide with replacement text keeps its start as a cursor position, drawn on the replacement: that's the fold line. A Hide without replacement text has no cursor position inside it. Overlapping Hides merge. After every edit or decoration change, core moves each cursor out of hidden text: to the fold line if there is one, otherwise to the next visible position. A whole-line fold hides from the start of its first line to the newline that ends its last line, so the next line keeps its own row.
15. **§9, die-vim's primitives, item 6.** Replace "`viewport`, `move_visual`, `display_col`, `next_visible`" with "`viewport`, `move_visual`, `display_col`, `next_visible`, `display_line`, `pos_at_vcol`, `snap`, `screen_pos`".

### Proposed edits to scope.md

1. **§5.8, Hooks.** Replace "buffer lifecycle, before and after an edit, before and after a save, mode on and off, idle." with:
   > buffer lifecycle, after an edit, before and after a save, mode on and off, idle. Hooks are queued (contract C3), so there's no before-edit hook; modes catch up with `edits_since`. A save runs its write as a queued step after the before-save hooks.
2. **§8, decorations list.** Replace "virtual text before or after a position" with "virtual text before a position".
3. **§12, after the spikes table.** Add: "S4 passed on 2026-10-19 without its fallback ([records/s4-core-api.md](records/s4-core-api.md))."

## 2. Pass criteria

Test files are under `spikes/s4-core-api/tests/`. "Pass" means I ran it on 2026-10-19 and it passed.

| # | Criterion | Tests | Result |
|---|---|---|---|
| 1 | The 8 prototype tests still pass | `proto-orig/` runs the original 8 unchanged. `proto.rs` ports them to the new API: `typed_ext_command_and_downcast`, `reentrant_checkout_is_busy_not_gone`, `panic_inside_checkout_restores_object_and_attributes`, `checked_out_mode_sees_its_own_edits` (push `on_edit` became `edits_since`), `kill_during_checkout_and_generations`, `job_handle_in_mode_cancelled_on_kill` (a buffer owner became a handle in a mode), `render_panics_contained`, `keymap_char_wildcard` (now through key dispatch) | Pass, 8 + 8 |
| 2 | C2 checkout: Busy, Gone, Missing; a panic puts the object back and is attributed; a kill during checkout drops at check-in | `c2_checkout.rs`: `busy_on_reentry`, `gone_after_kill`, `missing_for_unknown`, `panic_restores_object_and_is_attributed`, `kill_during_checkout_drops_at_checkin` | Pass |
| 3 | C3: Ctx calls are queued and never nested; `prompt` gives None on cancel | `c3_queue.rs`: `hooks_are_queued_not_nested`, `on_resize_and_on_enable_are_queued`, `timer_and_job_callbacks_run_from_the_top`, `prompt_delivers_none_on_cancel` | Pass |
| 4 | C4: drop cancels; `.detach()` ties to the extension; a handle in a mode is cancelled on kill | `c4_subs.rs`: `drop_cancels`, `detach_ties_to_the_extension`, `handle_in_mode_cancelled_when_buffer_killed`, `handle_in_checked_out_mode_cancelled_at_checkin`; also `proto.rs`: `job_handle_in_mode_cancelled_on_kill` | Pass |
| 5 | C14: `dd` as User fails in read-only; a silent Extension append succeeds | `edits.rs`: `c14_user_dd_fails_in_read_only_but_silent_extension_append_succeeds`, `c14_transaction_is_atomic` | Pass |
| 6 | Undo across silent edits; groups spanning commands undo as one step and restore the cursor | `edits.rs`: `undo_after_silent_edit_above_restores_user_text_exactly`, `silent_edit_mid_session_keeps_one_undo_step`, `redo_after_silent_edit`, `undo_refuses_when_an_extension_rewrote_the_users_text`, `insert_session_is_one_step_and_restores_cursor`, `count_dd_is_one_step_and_restores_cursor`; `prop.rs`: `undo_is_exact_around_silent_edits` (10,000 cases) | Pass |
| 7 | C15: a checked-out mode sees its own edits; a too-old revision gives None | `edits.rs`: `c15_checked_out_mode_sees_own_edits_and_old_rev_is_none`; `proto.rs`: `checked_out_mode_sees_its_own_edits` | Pass |
| 8 | C16 and layout: `j`/`k` skip a fold; `gj`/`gk` over soft wrap, centered at 80 in a 120-column window, with a gutter; tabs and CJK; the four queries exist | `layout.rs`: `j_k_skip_a_fold_made_with_a_stored_hide`, `gj_gk_over_soft_wrap_with_centered_max_width_and_gutter`, `tabs_and_wide_characters_have_correct_display_columns`, `sub_line_hides_and_virtual_text`, `edits_and_new_hides_move_the_cursor_out` | Pass |
| 9 | Property: the cursor is never inside hidden text, and render and motion layout never disagree, over 10,000 random sequences | `prop.rs`: `cursor_never_hidden_and_render_agrees_with_motions` (10,000 cases, 1–39 operations each) | Pass |
| 10 | Keymaps: `<char>` precedence within and across keymaps; `<any>`; exact > `<char>` > `<any>`; no release events; passthrough rules | `keys.rs`: `exact_beats_char_beats_any_within_one_keymap`, `a_modes_char_beats_the_kind_keymaps_j`, `release_events_never_reach_keymaps`, `passthrough_cmd_esc_first_and_other_keys_to_event`, `range_keymap_comes_first` | Pass |
| 11 | Prefix argument set by die-vim-style commands and read from args | `keys.rs`: `prefix_argument_set_by_die_vim_commands_and_read_from_args` | Pass |
| 12 | `feed_keys` and a key observer: `qa…q` then `@a` | `keys.rs`: `macro_round_trip_with_feed_keys_and_a_key_observer` | Pass |
| 13 | Search into a Hide runs the owner's reveal command with the decoration id | `layout.rs`: `search_into_a_fold_runs_the_owners_reveal_command`, `reveal_never_and_remove` | Pass |
| – | Extra: C8's startup check reports every missing command at once | `keys.rs`: `missing_command_names_reported_at_once` | Pass |

**What the property test checks after every operation.** The operations are random inserts and deletes, as User or Extension and silent or recorded, spanning hidden text. There are also Hides with and without replacement text, virtual text, read-only ranges, resizes, settings changes (gutter, maximum width, centering, wrap, tab width), `j`/`k`, `gj`/`gk`, `h`/`l`, cursor jumps, undo, redo and undo groups. After each one, the test checks that:

- the cursor and the selection anchor aren't inside hidden text, by a naive definition written separately from the core
- the core's set of cursor positions equals the naive set, at every character boundary
- every screen row render draws equals a separate char-by-char layout of the whole buffer
- `screen_pos` agrees with that layout for every visible cursor position, and the cell render drew there carries that buffer position
- a failed transaction changed nothing, and undo followed by redo is the identity

**Do the property tests have teeth?** `mutate.sh` plants 8 bugs, one at a time. All 8 are caught (`results/mutations.txt`):

1. a layout cache that ignores decoration changes
2. a Hide merge that ignores the replacement preference
3. no cursor fix-up after a decoration is added
4. an undo tie-break off by one
5. an end-of-line position on a full row that doesn't wrap
6. an open undo group that isn't rebased over silent edits
7. undo dropping its pending silent shifts
8. a wide character that ignores the end of the row

Bug 4 got past the first property test, so I added the model-based undo test (§5). In 2,000 cases, 51% of checks had a live Hide, 35% had a Hide joining lines, and 45% had wrapped rows on screen (`results/prop-coverage.txt`).

## 3. API decisions for your review

**For your review.** I had to choose each of these to build the spike. Each one has what I built, the alternatives and a recommendation. None is settled.

### Context and checkout

- **A1. `Ctx` is the workspace.** One concrete, lifetime-free struct.
  - Alternatives: `Ctx<'a>` borrowing a separate `Workspace`, or a trait so tests can fake it.
  - Recommendation: keep it concrete. The C11 harness uses a real `Ctx`.
- **A2. Checkout shape.** `with_ext`, `with_mode` and `with_surface` take a closure `|obj, ctx|` and return `Result<R, Checkout>`.
  - Wart: `ctx.with_mode::<Fold>(b, …)` doesn't compile, because Rust won't take a partial turbofish with `impl Trait` arguments. Callers write `|f: &mut Fold, ctx|` instead.
  - Alternatives: an RAII lease, which can't work because the guard needs the workspace while the closure holds `&mut Ctx`; or GPUI-style entity handles.
  - Recommendation: keep it, and document the annotation idiom.
- **A3. What the errors mean, and what kill does.** `Missing` means no such object. `Gone` means the buffer was killed, so it never comes from `with_ext`. A kill removes the buffer at once. A checked-out object drops at check-in. Modes are dropped without `on_disable`.
  - Alternative: queue `on_disable` at kill, handing it the object.
  - Recommendation: drop only. Cleanup that needs `Ctx` subscribes to `on_buffer_killed`.
- **A4. Text access.** `ctx.text(buf)` and `ctx.text_mut(buf)` borrow the buffer's `TextBuf` directly. Layout queries are methods on it that take a `WindowId`. Each window's selection, scroll position and size live inside the buffer, keyed by window (Helix's model), so an edit can move them without touching `Ctx`.
  - Alternatives: a `TextMut<'a>` wrapper borrowing the buffer and the windows; or selections stored on windows, which means every edit has to scan the windows.
  - Recommendation: keep the views in the buffer.
- **A5. Mode and surface traits.** `Mode` has `on_enable`, `on_disable` and `on_resize` (all queued), `decorate` (faces only, no `Ctx`) and `save`. `ModeSpec` is registered with its keymap, initial state and a factory, and `enable_mode::<M>(buf)` builds from the factory. `Surface::render` gets no `Ctx`. `Surface::event` does, and runs checked out.
  - Recommendation: keep. The spike renders strings; production renders the cell frame.

### Text and edits

- **A6. Transactions.** A builder: `t.tx(Origin::User).replace(r, s).insert(p, s).commit()`, with `.silent()` for silent edits. Changes are in the coordinates of the text before the transaction and are applied all or none. Read-only is checked before anything changes.
  - Alternative: a closure applying changes one at a time, which needs rollback.
  - Recommendation: the builder. Helix and CodeMirror use this model.
- **A7. Origin is required** on every transaction.
  - Alternative: default to `Extension`, with a `.user()` method.
  - Recommendation: required. Being explicit is the point of C14.
- **A8. Revisions and the change log.** The revision goes up by one per change, not per transaction. `edits_since` iterates a 1,024-change log. Each change carries byte offsets, line and column points, the new text, the origin and the silent flag.
  - Alternatives: one revision per transaction; a log capped by bytes.
  - Recommendation: per change, with the cap configurable. *(Amended 2026-11-05: `LOG_CAP` stays a constant in M0, since nothing has asked to change it.)*
- **A9. Decoration stickiness.** Each decoration has a gravity at each end. The default keeps text inserted at an edge outside the range. Read-only blocks a User insertion only strictly inside a range. To keep streamed text read-only, orchid would make its transcript range end-sticky.
  - Alternative: Emacs's default, which is rear-sticky.
  - Recommendation: keep non-sticky as the default.
- **A10. Replacement and virtual text** are one line: newlines and tabs become spaces, and an empty replacement means none. Virtual text shows just before its position. End-of-line hints anchor at the newline. After-side virtual text wasn't built.
  - Recommendation: before-side only in v1.

### Undo

- **A11. Grouping.** Each top-level unit (a key-dispatched command or a queued callback) is one undo step by default, like Emacs's undo boundaries. Explicit `begin_undo_group` and `end_undo_group` calls span commands. An explicit group also takes in an edit made earlier in the same command, as `o` needs; that part is built but not tested. An open group takes in every recorded edit to its buffer until it closes, including recorded Extension edits from callbacks. *(Amended by K2, 2026-10-19: `undo_boundary` ends the current step early, even inside one command or an open group, which stays open as a new step.)* *(Amended 2026-11-05, DR-13 and DR-14: an edit made between units, by the host or a harness, is a step of its own, not part of the last command's. An undo or redo inside an open group leaves it open, and the next edit starts a new step inside it.)*
  - Alternatives: no implicit grouping, so die-vim groups everything; or open groups that take only User edits.
  - Recommendation: keep it. Revisit if an extension makes recorded edits during an insert session. None of your packages does: orchid's streaming is silent.
- **A12. Overlapping silent edits.** If a silent edit overlapped text an undo step needs, undo refuses with `UndoConflict` and drops the undo history. *(Amended 2026-11-05, DR-15: if the step is still being collected, the history goes at the next recorded edit, and the undo that reaches the gap returns `UndoConflict`, once.)*
  - Alternatives: undo only the parts that don't overlap (CodeMirror 6 does this); or adjust nothing, as Emacs does, where positions go stale.
  - Recommendation: refuse and drop, for v1. It's honest and small.
- **A13. Undo is a User edit,** so it's blocked by read-only text, as in Emacs. After orchid moves your submitted input into its read-only transcript, `u` can't take it back.
  - Alternative: undo bypasses read-only.
  - Recommendation: blocked.
- **A14. Cursor and decorations on undo.** Undo restores the cursor from before the step, moved through any silent edits. Redo puts it at the start of the first change. *(Amended by K2, 2026-10-19: undo restores the acting window's whole selection from before the step (anchor, head, shape and goal), moved through any silent edits, and returns the range it changed, in the text after it. Redo returns its range too and puts a cursor at that range's start. die-vim collapses a restored visual selection after `u`.)* *(Amended 2026-11-05, DR-01: undo restores the acting window's whole selection from just before the step's first change, not from `begin_undo_group`, and die-vim's `u` leaves the cursor there, as your Evil does. So a die-vim command that moves and edits in one go makes its edit first.)* Undo doesn't restore decorations: after `dd` on a fold and then `u`, the lines come back unfolded (tested), as with Emacs overlays. manifold would re-derive its folds from `edits_since`.
  - Recommendation: keep.

### Layout and cursor positions

- **A15. Cursor positions and Hide.** Layout decides where the cursor can be: these are the "stops". A Hide with replacement text keeps its start as a stop, the fold line. A Hide without replacement text has no stop inside it or at its start. Overlapping Hides merge. The first-starting one wins, preferring one with replacement text, then the longest, then the lowest id. *(Amended 2026-11-05: the one added first, since ids are reused and don't say. Virtual text at one position shows in the order added too. Contract §9 has the whole rule.)*
  - Recommendation: keep, and write it into §9 (edit 14).
- **A16. Eager cursor fix-up.** After every edit, decoration change or `set_selection`, every window's selection moves to a stop. The default goes to the fold line if there is one, otherwise forward. Motions pass a direction with `snap(pos, dir)`.
  - Alternative: adjust after each command only, as Emacs does.
  - Recommendation: eager. "Never inside" then holds at every point, not just between commands.
- **A17. Query set.** The contract lists 4 queries. The spike needed 9: `viewport`, `move_visual`, `display_col`, `next_visible`, `display_line`, `pos_at_vcol`, `snap`, `is_stop` and `screen_pos`, plus `geometry`.
  - Recommendation: make all of them public. They're thin wrappers over one function. One goal column on the selection means whatever die-vim decides: a display column for `j`/`k`, a screen column for `gj`/`gk`.
  - Geometry, part of the same choice: the gutter sits immediately left of the text, and gutter and text are centered together. The alternative is the gutter at the window edge, with only the text centered. Recommendation: keep it next to the text.

### Keys, commands and async

- **A18. Keymaps.** There's one trie per keymap and state. At each step, an exact key beats `<char>`, which beats `<any>`, with no backtracking. The first layer that returns a command or a prefix wins. `<char>` matches committed text. Range keymaps are named keymaps, referenced by name. A key bound both alone and as a prefix (`g` and `gg`) waits for the next key forever.
  - Alternatives: a timeout, as Vim has; or a registration error.
  - Recommendation: a registration error, reported with C8. Evil doesn't bind `g` alone.
- **A19. Passthrough.** Cmd-Esc is checked first. The Cmd layer is looked up in every layer, in passthrough state. Any key DIE takes swallows its own repeat and release, not only Cmd-Esc.
  - Recommendation: keep. Otherwise a program gets a release for a press it never saw.
- **A20. Prefix argument.** The slot is taken, and cleared, when a key-dispatched command starts, and it's handed over in `Args`. Prefix commands, such as digits and `"x`, write it back. This is Emacs's model. Core never parses Vim syntax. *(Amended 2026-11-05, DR-03: an undefined key clears it too.)*
  - Recommendation: keep.
- **A21. `feed_keys` and the key observer.** `feed_keys` parses notation, and fed keys run before further typed keys. `observe_keys(FnMut(&KeyEvent, KeySource))` is synchronous and gets no `Ctx`. It sees presses and repeats before dispatch, each marked typed or fed. *(Amended by K5, 2026-11-05: a key's command failing, by an error or a panic, or a key nothing handles, drops the rest of the fed keys, so a macro stops where it fails. The observer never sees what's dropped, and a panic in it no longer stops the key. A prompt's answer a key submitted is part of the key stream, so one that fails drops the rest too. Keys fed from inside a call run ahead of what's left of the fed keys, so a macro inside a macro runs first. S4 added them at the end.)*
  - Recommendation: keep.
- **A22. Reveal.** `ctx.reveal(buf, pos)` applies each covering Hide's policy. A reveal command runs nested, as an explicit run, so the caller can place the cursor straight after. `Args` gains `deco`.
  - Alternative: queue the reveal command, and continue in a callback.
  - Recommendation: run it nested. C3 allows explicit runs.
- **A23. Subscriptions.** `Sub` is `#[must_use]`. Dropping it cancels the subscription, and `.detach()` ties it to the extension until shutdown. It's `Rc`-based, so main thread only. Hooks are typed methods, such as `on_buffer_killed`, not one generic hook enum. *(Amended by K5, 2026-11-05: timers keep their handles in a timer heap and `idle` joins `after` and `every`, with no change to what a handle does. A cancelled timer's closure is freed by a sweep, at most about a thousand timers later, or when it comes due. Shutdown also cancels what your config detached, last.)*
  - Recommendation: keep.
- **A24. C9 in the spike.** Panics are caught at every call into extension code and attributed. `with_*` puts the object back, then lets the panic continue. Panics are counted, but repeated panics don't disable anything yet. *(Amended by K5, 2026-11-05: every panic is also logged to `*messages*` once, with the place. A surface's `event`, a key observer, a job's `work`, a `Drop` during `kill` and the `Drop` of anything cancelled used to escape the guard or hang the loop, and are now caught. A panic hook sees what no guard catches. Contract §15 lists what it can't reach.)*
  - Recommendation: build per-object disabling in M1, as the contract says.

## 4. What was built

**Core modules.** Code lines are non-blank, non-comment lines, after rustfmt 1.9.0 with default settings. The source as I first wrote it was 2,465 code lines; rustfmt adds about 24%.

| Module | Raw lines | Code lines | Contents |
|---|---|---|---|
| `ctx.rs` | 1,281 | 1,151 | `Ctx`, traits, registry, checkout, buffers and windows, modes, commands, queue, subscriptions, prompt, key dispatch |
| `layout.rs` | 643 | 585 | the layout function, Hide merging, geometry, cache, every query |
| `text.rs` | 615 | 549 | rope, transactions, change log, markers, decorations, views |
| `undo.rs` | 307 | 271 | groups, lazy rebase over silent edits |
| `keys.rs` | 221 | 198 | key events and notation |
| `render.rs` | 166 | 155 | headless frames, gutter, faces |
| `keymap.rs` | 97 | 85 | tries, `<char>`, `<any>` |
| `error.rs`, `ids.rs`, `lib.rs` | 86 | 66 | |
| **Total** | **3,416** | **3,060** | |

The tests are 2,779 raw lines, or 2,585 code lines. `tests/support/mod.rs` (409 code lines) holds a toy die-vim, a fold mode in manifold's shape and a toy terminal surface, all using only the public API. The toy die-vim is a test fixture, not a start on die-vim. The property tests are 720 code lines.

Dependencies:

- `ropey =1.6.1`, with default features off and `simd` on, so `unicode_lines` is dropped
- `slotmap =1.1.1`
- `unicode-width =0.2.2`
- `unicode-segmentation =1.13.3`
- `proptest =1.11.0`, for tests only, with default features off and `std` on; 1.12.0 exists, and I pinned the cached 1.11.0

Nothing is compiled through `cc`: `Cargo.lock` has no `cc`, and the only build scripts are pure Rust. I didn't need `regex-cursor`; search is a literal search over the rope.

**Timings.** The test profile is opt-level 1 with debug assertions.

| | Test profile | Release |
|---|---|---|
| `cursor_never_hidden_and_render_agrees_with_motions`, 10,000 cases | 8.75 s | 7.97 s |
| `undo_is_exact_around_silent_edits`, 10,000 cases | 0.63 s | 0.54 s |
| Whole suite (`cargo test`, wall clock) | 12.9 s | – |

There are about 21 checks per case, each laying out the whole buffer twice (the core and the oracle), so 10,000 cases run about 210,000 full checks.

**Budget estimate for the production `die-core`,** against scope guard 7's limit of about 14k. The first block grows measured spike code by 1.5–2× for what production adds. The second block wasn't prototyped, so it's judgment only.

| Production module | Spike basis (code lines) | What production adds | Estimate |
|---|---|---|---|
| Text: transactions, log, markers, decorations, views | `text.rs` 549 | styled insertion, tag lookup, regex search, an interval index for decorations | 900–1,100 |
| Undo | `undo.rs` 271 | selection-shape restore, a size cap | 300–400 |
| Layout and queries | `layout.rs` 585 | per-line cache invalidation, horizontal scroll, relative and visual numbers, image placements in Hide | 800–1,000 |
| Keys, keymaps, describe | `keys.rs` and `keymap.rs` 283 | OS key events, leader from config, describe-key and describe-command, shadow reports | 550–700 |
| Workspace, buffers, window split tree | part of `ctx.rs`, about 250 | the split tree, `show(buffer, ShowIn)`, focus by direction | 600–800 |
| Extension machinery, checkout, modes, panics | part of `ctx.rs`, about 450 | panic hook, per-object disabling, mode save and restore, file-type modes | 600–750 |
| Dispatch, commands, prefix, queue, subscriptions, timers | part of `ctx.rs`, about 450 | idle timers, more hook kinds | 500–650 |
| Scene | `render.rs` 155 | styled cells, cursor shapes, status lines, command line, placements | 500–700 |
| *Subtotal, prototyped* | *3,060* | | *4,750–6,100* |
| Command line and prompt with completion | – | | 600–800 |
| Processes, PTYs, file watching | – | | 600–900 |
| Holder client | – (S3) | | 300–500 |
| Files: open, save through symlinks, auto-revert | – | | 400–600 |
| Session save and restore, restart | – | | 700–1,000 |
| Faces and themes | – | | 250–350 |
| Copy-state snapshot | – | | 150–300 |
| Public headless harness | – | | 300–400 |
| Messages and notifications | – | | 150–250 |
| *Subtotal, not prototyped* | | | *3,450–5,100* |
| **die-core total** | | | **about 8.2k–11.2k** |

That leaves 3k–6k of headroom under 14k. The weakest numbers are the session and the command line, which nothing has measured yet.

**What the fallback would have bought.** I didn't build the fallback. Limiting Hide to whole lines and virtual text to the end of the line would remove, as I read `layout.rs`, the run splitting at Hide starts, the no-replacement rule and inline virtual text. That's roughly 60–100 of its 585 code lines, and the joined-line walk would get simpler. It would also lose markdown markup hiding and inline hints, which your future markdown view needs. Since the general version passes, I recommend not taking it.

**How to rerun:**

```
cd /Users/user/workspace/die/spikes/s4-core-api
cargo test                                         # 48 tests; both properties at 10,000 cases
cargo test --release --test prop                   # property timings
S4_CASES=100000 cargo test --release --test prop   # more cases
cargo test --release --test prop coverage_stats -- --nocapture
./mutate.sh                                        # the 8 planted bugs, about 2 minutes
(cd proto-orig && cargo test)                      # the review's prototype, unchanged
```

`proto-orig/` is a byte-for-byte copy of the review prototype's `src/` and `tests/`. Its `Cargo.toml` gains an empty `[workspace]` table, and that's the only change.

## 5. Surprises, risks and broad decisions

### Surprises

- **No core change was needed to pass the criteria.** Every first failure was in my tests:
  - three wrong expectations in my tests: a wide character at the end of a row, the toy die-vim's end-of-buffer clamp, and a macro's final cursor
  - an oracle comparison that assumed horizontal scrolling in no-wrap mode

  No core code changed after tests ran, apart from rustfmt.
- **Refusing to undo is legal, so the first property couldn't see an over-cautious undo** (planted bug 4). A second, model-based property was needed. It checks that undo succeeds and restores the exact text and cursor whenever the silent edits don't overlap your text. It's the undo test `die-core` should keep.
- **A whole-line fold must stop before its last newline.** Otherwise the next line joins the fold's row. manifold already does this: it makes text invisible up to `end - 1`.
- **Undo doesn't bring back decorations.** See A14.
- **Before-edit hooks don't fit C3.** A queued "before" hook runs after the edit. Before-save hooks work only if core runs the write as a queued step after them. This is scope edit 1.
- **The `with_mode::<M>` turbofish doesn't compile.** See A2.

### Risks

- **Speed at scale isn't measured.**
  - The layout cache is cleared on every edit and decoration change.
  - Decorations move by a linear scan on every edit.
  - Search converts the rope to a `String`.
  - Graphemes are segmented through a temporary `String` per run.

  Production needs per-line invalidation and an interval index. Face providers keep stored decorations few, which helps.
- **The oracle is my second implementation of the same written rules.** It catches code bugs (8 of 8 planted) but not mistakes in the rules. The rules are in edit 14 for your review.
- **No horizontal scroll without wrap.** Your Emacs wraps everywhere (`global-visual-line-mode`), but code buffers may not.
- **CRLF.** Without ropey's `cr_lines`, a `\r` shows as a `?` cell. Your files are LF. If CRLF matters, enable `cr_lines` on its own; it doesn't pull in `unicode_lines`.
- **A Hide boundary inside a grapheme cluster splits the cluster.** It's rare, and only cosmetic.
- **Undo memory.** Undo stores full strings per change, with a 2,000-entry cap and no byte cap.

### Broad decisions for you

1. Should undo refuse and drop history when an extension's silent edit overlapped your text (A12)?
2. Should undo be blocked by read-only text (A13)?
3. Should the before-edit hook go away, and should the save be queued behind before-save hooks (scope edit 1)?
4. Should a key bound both alone and as a prefix be a registration error (A18)?
5. Should an open insert-session group take in recorded Extension edits too (A11)?

## 6. Raw data and test output

All under `/Users/user/workspace/die/spikes/s4-core-api/`:

- `results/test-all.txt`: the full `cargo test` run (48 tests), with wall time
- `results/prop-timings.txt`: each property, both profiles
- `results/prop-release.txt`: the release property run
- `results/prop-coverage.txt`: what 2,000 random cases exercised
- `results/mutations.txt`: the 8 planted bugs and which test caught each
- `results/line-counts.txt`: module sizes
- `results/proto-orig.txt`: the unchanged prototype's 8 tests
- Source: `src/`. Tests: `tests/`, with fixtures in `tests/support/mod.rs`. Mutation script: `mutate.sh`.
