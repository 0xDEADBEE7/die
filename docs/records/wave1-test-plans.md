# Wave 1: test plans, as built

K5, K4, R2 and V3 each started from a written test plan, as [m0-plan.md](../m0-plan.md) §3 asks, and an independent review then added tests. This is what the builders and finishers reported, kept as they wrote it. The [log](../m0-log.md) has the results and the calls.

The tests have changed a little since. Each file's header lists its own, and those headers are kept current.

## K5

### As built

Unchanged, byte for byte: S4's c2_checkout (5), c3_queue (4), c4_subs (4), proto (8), and also edits, keys, layout, prop, support, files and keymaps. `git diff main` on them is empty.

New: 94 tests in 6 files under crates/die-core/tests/. Each file's header lists them too.

fed_keys.rs (11)
- an_error_drops_the_rest_of_the_fed_keys: Err from a fed key's command; the next key never runs.
- a_panic_drops_the_rest_of_the_fed_keys: the same for a panic.
- an_undefined_fed_key_drops_the_rest: a key nothing handles, and one that ends a prefix.
- an_error_in_a_surfaces_event_drops_the_rest: a panic in event, in and out of passthrough; event is called once.
- a_key_a_surface_handles_drops_nothing: event returning true isn't an error.
- typed_keys_survive_the_drop: only fed keys go.
- a_typed_key_that_fails_drops_what_it_fed: a command feeds keys, then fails.
- what_a_later_command_feeds_runs_again: the drop isn't sticky.
- a_callback_error_drops_nothing: a timer failing mid-macro drops nothing.
- an_error_the_command_handles_drops_nothing: a nested run fails, the command returns Ok.
- the_keys_a_drop_removes_are_never_observed: a key observer sees only keys that dispatched.

panics.rs (29). Most end in logged_once_and_usable: exactly one line in messages, nesting 0, the extension back, a key runs, a timer fires, text edits.
- a_panic_in_register_is_a_startup_error: the error names the extension and the message.
- a_panic_in_init_is_logged_and_the_next_extension_still_inits.
- a_panic_in_shutdown_is_logged_and_the_rest_still_shut_down.
- a_panic_in_a_command_from_a_key.
- a_panic_in_a_command_from_run_is_returned_and_logged.
- a_panic_passed_on_by_the_calling_command_is_logged_once: nested, then propagated.
- a_panic_in_a_reveal_command.
- a_panic_in_a_deferred_call.
- a_panic_in_a_repeating_timer_is_logged_each_time_and_it_goes_on.
- a_panic_in_a_kill_hook.
- a_panic_in_a_prompts_answer_leaves_the_state_restored.
- a_panic_in_a_jobs_work_reaches_the_loop: idle doesn't wait for a dead thread; done doesn't run.
- a_panic_in_a_jobs_work_names_the_extension_that_spawned_it.
- a_panic_in_a_cancelled_jobs_work_is_still_logged.
- a_panic_in_a_jobs_done.
- a_panic_in_a_surfaces_event_for_an_unbound_key: attributed to the extension that made it; not also "undefined"; the surface is back.
- a_panic_in_a_surfaces_event_in_passthrough: the next key still reaches it.
- a_panic_in_a_resize_event: attributed to the owner; the surface is back.
- a_panic_in_render_draws_a_placeholder_and_is_logged_once: over 3 frames; a good render comes back.
- a_panic_in_a_mode_factory_is_returned_and_logged: no mode is left behind.
- panics_in_on_enable_on_disable_and_on_resize.
- a_panic_in_decorate_is_logged_once: the text still draws.
- a_panic_in_a_key_observer_leaves_the_key_dispatching.
- a_panicked_observer_can_run_again.
- a_panic_in_a_drop_during_kill: the buffer is dead and hooks still run.
- a_message_names_the_extension_the_place_and_restarting.
- a_panic_only_the_host_caught_is_logged_at_the_next_turn: the hook.
- a_new_ctx_doesnt_log_a_panic_from_before_it_existed.
- a_loop_that_never_goes_idle_is_reported_not_a_panic.

panic_hook.rs (1, its own binary because a hook is process-wide)
- the_hook_keeps_the_one_before_it: an earlier hook still runs, and the line has the place. A second Ctx doesn't chain again. A hook set later replaces core's, and the line is still logged without the place.

config.rs (17)
- the_steps_run_in_the_contracts_order: register, config, init, startup.
- config_bindings_win_in_the_keymap_they_name: global and kind.
- config_binds_in_a_modes_keymap: mode_keymap, and None for no such mode.
- config_frees_a_key_an_extension_uses_as_a_prefix.
- a_prefix_conflict_with_an_extension_is_still_a_startup_error.
- config_commands_belong_to_config: command_doc and a panic's attribution.
- a_command_name_config_takes_from_an_extension_is_a_startup_error.
- config_can_declare_faces: a theme styles them with no message.
- every_missing_name_is_reported_at_once_and_nothing_inits.
- the_leader_config_sets_applies_to_every_keymap.
- a_panic_in_the_config_step_is_a_startup_error.
- the_theme_applies_before_init.
- a_theme_that_styles_an_undeclared_face_is_reported.
- buffer_defaults_apply_to_every_new_text_buffer: from init on, and no config means no change.
- file_settings_start_from_the_buffer_defaults: and a panic falls back to those defaults, logged once.
- the_startup_hook_can_use_the_editor: it runs at depth 1, and its queued calls wait for the loop.
- an_error_or_panic_in_the_startup_hook_is_logged.

timers.rs (27)
Order:
- timers_fire_in_deadline_order.
- ties_fire_in_the_order_they_were_set.
- repeats_interleave_by_deadline.
- advance_runs_each_timer_at_its_own_time.
- a_timer_set_from_a_callback_counts_from_then: a retry chain runs inside one advance.
Cancelling:
- a_timer_cancelled_when_already_due_doesnt_fire.
- a_repeating_timer_can_cancel_itself.
- cancelled_timers_dont_pile_up: closures are freed before their deadlines.
- a_cancelled_repeat_is_freed_when_it_comes_due.
Limits:
- every_with_no_period_lets_the_loop_go_idle: a zero period runs as 1 ms.
- a_huge_delay_doesnt_overflow: Duration::MAX as delay, period, idle wait and advance, set after the clock has moved.
Next deadline:
- next_timer_is_none_with_nothing_set.
- next_timer_is_the_earliest_live_deadline.
- next_timer_follows_a_repeat.
- next_timer_forgets_what_shutdown_cancelled.
- a_new_earlier_timer_moves_next_timer.
Clocks:
- the_fake_clock_moves_only_when_advanced.
- the_real_clock_runs_timers_with_no_advance: and next_timer shows one overdue.
- advance_doesnt_move_a_real_clock.
- a_repeat_that_fell_behind_skips_what_it_missed.
- a_repeat_keeps_its_phase.
Idle:
- an_idle_timer_fires_once_per_idle_period.
- a_key_restarts_the_idle_wait: typed or fed.
- an_idle_timer_waits_its_whole_period_when_set.
- dropping_an_idle_handle_cancels_it.
- next_timer_counts_idle_timers.
- a_panic_in_an_idle_timer_is_logged.

wake.rs (10)
- a_finished_job_wakes_the_loop.
- wakes_coalesce_until_the_loop_takes_a_turn: many wakes, one call, then it arms again.
- jobs_that_finish_between_turns_wake_once: three jobs, one wake, every done runs.
- a_wake_from_a_callback_wakes_again.
- a_job_that_ends_as_the_loop_looks_wakes_again: the loop arms the waker, then reads the channel. A cancelled job's value drops during the read, which is how the test acts in that gap.
- a_job_finished_before_the_waker_connects_is_still_delivered.
- connecting_a_second_waker_is_an_error: and the first stays connected.
- a_waker_works_from_any_thread_and_outlives_the_ctx.
- no_wake_is_lost_under_load: a host that sleeps until woken sees 4,000 jobs finish.
- step_never_waits_for_a_job: run_until_idle still does.

How the tests were checked: I reverted 53 behaviours one at a time in a throwaway copy, and each now makes a test fail. The first pass let five through. One of those was only a reversal that didn't apply, and it was caught once corrected. The other four showed weak tests, now fixed in 589e36d. I also added three more reversals. The timing-sensitive tests ran 33 rounds under load with no flake.

### Added after its review

fed_keys.rs (14):
- a_key_a_surface_ignores_in_passthrough_drops_nothing: `event` returning false lets all 3 fed keys arrive, with no message.
- a_failing_prompt_answer_drops_the_rest: `:x<CR>b` never runs `b`, logs one line, and the state is Normal.
- a_panicking_prompt_answer_drops_the_rest: the same, for a panic.
- a_prompt_answer_that_succeeds_drops_nothing: the answer runs, then `b`.
- a_cancelled_prompt_whose_answer_fails_drops_the_rest: `:x<Esc>b` never runs `b`.
- a_prompt_a_new_prompt_cancels_drops_nothing: the first answer fails, and fed `xy` still land in the second prompt.
- a_queued_calls_error_drops_nothing: a deferred call, a job's `done` and a hook each fail, and the fed key still runs.
- fed_keys_go_ahead_of_typed_ones: typed `c`, fed `a` gives a, c.
- a_macro_inside_a_macro_runs_before_the_rest_of_it: `agc`, where `g` feeds `b`, gives a, b, c.
- keys_fed_twice_in_one_turn_keep_their_order: `ta` gives b, c, a.
- macros_nested_three_deep_finish_innermost_first: `ia` gives b, c, a.
- an_error_in_a_macro_inside_a_macro_ends_both: `nb`, where `n` feeds a failing key, runs nothing.
- keys_the_host_feeds_go_behind_what_is_left: a, b, then the host's c.
- keys_fed_after_a_nested_turn_emptied_the_queue_still_run: `c` runs, with no panic.

timers.rs (15):
- cancelled_timers_dont_pile_up_when_jobs_are_spawned_too: under 1,100 of 2,100 closures still held.
- an_idle_timer_doesnt_fire_with_a_key_waiting: typed or fed, after a stall, the log is only the key.
- an_idle_wait_counts_from_when_the_command_ended: `next_timer` is at least the command's end plus the wait.
- an_idle_wait_counts_from_when_a_prompts_answer_ended: the same, for an answer.
- a_key_release_restarts_the_idle_wait: no firing 8 ms after a release, even 16 ms after it was set.
- a_cancelled_idle_timer_is_freed_at_the_next_turn: the closure is held until a `step`, then freed.
- deadline_order_holds_across_idle_and_heap_timers: after a stall, idle at 1 ms fires before `after` at 5 ms.
- an_idle_timer_due_with_another_goes_after_it: at the same deadline, `after` then idle.
- a_slow_repeat_gives_keys_a_turn: the repeat and two keys alternate, turn by turn.
- step_hands_back_between_firings_of_a_slow_repeat: true, then false, with `next_timer` at or before now, three times.
- a_chain_of_timers_due_now_runs_inside_one_advance: three firings at 10 ms.
- advance_waits_once_for_a_job_thats_out: 2 firings in under 10 s.
- advance_waits_for_a_job_that_finishes: `done` has run when `advance` returns.
- work_deferred_before_advance_counts_from_before_it: a timer set by a deferred call fires within the move.
- a_nested_advance_doesnt_move_the_clock_back: the clock ends at 60 ms.

panics.rs (13):
- a_panic_dropping_a_cancelled_jobs_value, a_panic_dropping_the_done_of_a_job_whose_work_panicked, a_panic_dropping_a_cancelled_timers_closure_at_its_deadline, a_panic_dropping_a_closure_cancelled_while_it_was_queued, a_panic_dropping_a_closure_in_the_sweep, a_panic_dropping_a_closure_when_next_timer_looks, a_panic_dropping_a_cancelled_idle_timers_closure, a_panic_dropping_a_cancelled_hook: each logs one line, returns normally, and leaves the editor usable.
- a_key_observers_panic_names_its_extension: the line starts `ext panicked: observer bug`.
- a_prompts_answer_panic_names_the_extension_that_asked: the line starts `ext panicked: answer bug`.
- panics_taking_turns_dont_flood_the_log: 2 lines over 100 frames, 5 after another message.
- a_long_run_of_panic_lines_stops_being_compared: a repeat is skipped when close, and logged again past the limit.
- a_render_panic_keeps_its_line_when_a_command_panics_next: both lines are there, for a hooked panic and a `resume_unwind`.

config.rs (3):
- extensions_shut_down_in_reverse_order: b, then a.
- shutdown_cancels_what_config_detached: `next_timer` is None, and the timer, idle timer and hook don't fire.
- shutdown_cancels_what_config_sets_during_it: a timer an extension's `shutdown` sets through a config command is gone too.

Oracle (2, pending for V4): macros/nested-macro-runs-first, macros/error-in-nested-macro-ends-both.

## K4

### As built

crates/die-core/tests/emacs_layout.rs (check 3, tty Emacs 30.2; data in tests/emacs_layout/, scripts in tests/emacs_layout/probe/):
- every_wrap_case_matches_tty_emacs_or_differs_as_contract_9_says: 91 cases at W-1 text columns. Every cursor position's (row, col) and every row's start match, or the difference is one §9 names. More than 1000 positions are compared, and a named difference that stops differing fails.
- every_line_number_screen_matches_tty_emacs_or_differs_as_contract_9_says: 21 screens (t, relative, visual; empty text, final newline, wraps, folds) match row for row, gutter included. The one exception is Relative across a fold, which §9 names.
- the_terminals_own_cursor_is_where_layout_puts_it: 4 captures of the terminal's real cursor match screen_pos. A wide character that moved is drawn on its new row, and a full row's newline at col 10.
- the_differences_are_the_contracts: §9 still contains each sentence the test relies on.
- move_visual_lands_where_tty_evils_gj_and_gk_do: 6 of the oracle's wrapped cases, at 39 text columns.

crates/die-core/tests/prop.rs (check 2; `cargo xtask mutate` runs this file):
- layout_agrees_with_the_naive_layout: across runs of edits and decoration changes on text with clusters and control characters, K4 matches the naive layout on geometry, every row's cells, gutter text and numbers, cursor positions, is_stop, display_col, display_line, snap in all three directions, screen_pos, move_visual and pos_at_vcol. Positions inside a character also stay safe.
- the_gutter_agrees_with_the_naive_layout_where_it_grows: 0 to 10,000 lines, with and without a final newline.
- cursor_never_hidden_and_render_agrees_with_motions: S4's property. Its oracle's line-end branch and cursor filter change for WRAP-2, Op::Set now picks 3 wraps and 4 number styles, and Wrap::Word reads its rows from the naive layout.

crates/die-core/tests/layout_speed.rs (three tries each; test profile, opt-level 1):
- a_keystroke_on_a_10k_line_file_costs_at_most_1ms: insert plus compose, median, measured 0.17 ms.
- a_keystroke_at_either_end_of_a_1_mb_line_costs_at_most_1ms: Word with spaces, Word with none, Char, Off; measured 0.02-0.25 ms.
- moving_to_the_far_end_of_a_1_mb_line_costs_at_most_100ms: a first full layout, measured 21 ms.
- a_cursor_position_costs_the_same_whatever_the_lines_length: DR-09; snap, is_stop and set_cursor on 1 MB against 1 KB stay within 4x, measured 2.1x.
- edits_on_a_growing_line_cost_the_same: the second 20,000 edits cost at most 4x the first, measured about 1x.

crates/die-core/tests/layout.rs (K4 section):
- an_exactly_full_line_gets_no_blank_row: WRAP-2; screen_pos (0,10), and gj goes to b.
- the_empty_line_after_a_final_newline_has_a_row_and_no_number: dev-4; it still counts for distances, and an empty text's row has no number either.
- the_gutter_counts_lines_not_the_empty_last_one: LAYOUT-3; 999 and 1000 lines, and hidden lines count.
- line_numbers_match_the_contracts_table: §9's table, 3 styles by 3 cursors.
- a_fold_counts_once_in_relative_and_visual_numbers: with and without a replacement.
- numbers_for_a_cursor_off_screen_are_still_its_distance: above and below the window.
- pos_at_vcol_lands_after_what_spans_the_goal: LAYOUT-4; tab, wide character, past the line, a joined line.
- wrap_off_word_and_char_each_break_as_they_say: LAYOUT-5; the default is Word.
- scroll_reads_and_sets_a_windows_first_row: LAYOUT-6; top snaps, row clamps, a dead window gives None.
- core_scrolls_back_to_the_cursor_before_a_render: LAYOUT-6.
- a_display_line_and_a_view_row_are_read_through_methods: LAYOUT-7.
- a_tall_file_holds_more_lines_than_the_cache_does: past the 512-line cache.
- toggle_max_width_turns_the_width_off_and_back_on: 80 by default, the width it had, the acting window not the focused one, an error with no buffer.
- styled_text_comes_from_a_plain_string: call 6; a plain string is one call, and newlines and tabs become spaces.

crates/die-core/tests/scene.rs (K4 section):
- a_hide_replacement_is_drawn_in_one_style, a_replacement_split_by_the_wrap_keeps_its_style_and_position, a_tab_split_by_the_wrap_keeps_its_style_and_position: scene-4; one style and one position across rows.
- virtual_and_replacement_text_take_their_faces, a_replacement_with_two_runs_draws_both: scene-5.
- virtual_text_takes_the_faces_of_a_position_off_screen: a position whose own cell isn't drawn still gives its faces.
- an_undeclared_face_on_styled_text_is_reported_and_filler_takes_none.
- a_cursor_past_a_full_row_goes_in_the_padding_or_on_the_last_cell: padding, centered, no padding (past_end), unwrapped (not drawn).
- a_full_rows_newline_has_no_cell_in_a_centered_window: the K4 call on the newline's cell; a click there still lands on the newline.
- control_characters_draw_as_emacs_draws_them: DR-12.
- a_visual_number_wider_than_its_gutter_shows_its_last_digits: ambiguity 3.
- only_the_cursors_row_takes_the_current_lines_face.

crates/die-vim/tests/core_review.rs:
- a_control_character_is_two_columns_wide: DR-12's failing test; j keeps Evil's column.

crates/die-core/tests/naive_layout.rs: 4 new worked examples in its contract table (one space first on its row, a space first on its row after a full word, a tab with no cell on the first row, a control character's rest). tables_match_the_contract still reads §9.

tests/mutants.toml: M5 inverted. New: M27-M32 stale caches, M33-M36 wrap, M37-M40 cursor positions, rows and numbers. M1, M2, M3 and M8 re-pointed at the new code. `cargo xtask mutate`: 40 of 40 caught.

### Added after its review

prop.rs:
- layout_agrees_with_the_naive_layout: now also replaces text. After each step, every row, cell, position, number and query matches the naive layout.
- layout_agrees_with_the_naive_layout_after_lazy_edits: small queries only between edits, then the same check on two windows.
- a_cluster_across_a_ropes_chunks_agrees_with_the_naive_layout: fixed texts with a chunk's edge inside a cluster, which it asserts.
- long_text_across_a_ropes_chunks_agrees_with_the_naive_layout: about 3 KB of mixed text, 5 cases.
layout_speed.rs:
- a_keystroke_at_either_end_of_a_1_mb_line_costs_at_most_1ms and moving_to_the_far_end_of_a_1_mb_line_costs_at_most_100ms: now across scripts.
- a_position_that_isnt_a_cursor_position_costs_the_same_whatever_the_lines_length: 1 MB against 1 KB, ratio at most 4.
- edits_cost_the_same_on_a_1_kb_line_and_a_1_mb_line: ratio at most 4.
- a_keystroke_after_visiting_every_line_costs_at_most_1ms.
- a_face_decoration_costs_no_layout: at most 1 ms.
scene.rs:
- a_block_selection_takes_a_replacement_whole: the selection masks.
- a_provider_isnt_asked_for_a_wide_character_on_the_row_below: visible is 0..9.
- a_folds_later_line_takes_the_current_number_face.
layout.rs:
- a_fold_joining_lines_numbers_the_cursors_own_line_in_visual.
- set_scroll_keeps_the_row_it_stopped_at.
- toggle_max_width_turns_the_width_off_and_back_on: extended with Args::buffer.
naive_layout.rs:
- numbers_on_a_fold_joining_lines.
emacs_layout.rs:
- every_wrap_case_matches_tty_emacs_or_differs_as_contract_9_says: exactly 10 excused positions.
- the_terminals_own_cursor_is_where_layout_puts_it: 4 more captures.
- every_line_number_screen_matches_tty_emacs_or_differs_as_contract_9_says: 30 more screens, and each named difference must still differ.
- the_differences_are_the_contracts: names the new difference.

Ported S4 tests: none edited in this round. One naive-layout rule changed, by the K4-8 call: Visual's number on the cursor's row.

## R2

### As built

145 tests. All are in crates/die-render/tests/. The neutral files (sprites, atlas, font, rows, backend: 90 tests) run under ci --linux. fidelity.rs (39) has no platform cfg, compiles against the stub and skips on Linux. platform.rs (15) is macOS-only, the lint's one named exception.

## sprites.rs (21): coverage bitmaps, no font, no GPU
- every_box_glyph_meets_its_neighbour_at_each_edge_as_its_name_says: at 5 cell sizes, the pixels lit on all four edges of each solid, double and arc glyph (dashes and diagonals have their own tests) match the arm weights parsed from Unicode's names (a fixture, independent of the renderer's table). That is the join.
- light_lines_cross_and_turn_square: pixel-exact ┼ and ┌ at 9×9.
- heavy_arms_reach_across_a_light_line_and_a_half_line_stops_at_the_center: pixel-exact ┝ and ╴.
- opposite_half_lines_meet_at_the_center_and_make_the_whole_line: ╴╶, ╵╷, ╸╺ and ╹╻ don't overlap, and together equal the whole line, at 5 sizes.
- double_lines_leave_their_joins_open: pixel-exact ╬ ╦ ╔ ╠.
- a_light_line_joins_a_double_one_at_the_rail_it_meets: pixel-exact ╒ ╓ ╤ ╧ ╪ ╫ ╢.
- arcs_round_the_corner: ╭ has no square corner, has a curve between its arms, and ╮ mirrors it.
- dashed_lines_have_their_count_of_dashes_and_even_gaps: 2, 3 or 4 dashes, a gap at each edge, heavy twice as thick.
- diagonals_join_at_the_corners_and_the_cross_is_both: ╱ and ╲ reach their corners, and ╳ is the two together.
- blocks_split_a_cell_at_one_place_even_at_an_odd_size: ▀▄, ▌▐ and all 5 quadrant pairs fill the cell with no gap or overlap, at 4 sizes.
- eighths_grow_from_their_edge: ▁..█ and ▉..▏ have the right size and edge; ▔ and ▕ too.
- shades_are_even_all_over_so_they_tile: ░▒▓ are one flat value each.
- braille_dots_sit_where_unicode_numbers_them: dots 1-8 are each in their place, and U+2800 is blank.
- braille_dots_keep_inside_the_cell_at_an_even_pitch: the cell's edges are clear, 4 dot rows, and the pitch across the edge matches the pitch inside.
- powerline_shapes_span_the_cell_and_mirror: E0B0 reaches the far side, E0B2 is its mirror, and the rest have ink.
- a_character_the_renderer_does_not_draw_is_none: a, 中, U+E0B5 and the ranges' neighbours give None.
- a_single_underline_and_the_strike_are_one_band_across_the_cell: exact rows.
- a_double_underline_is_two_bands_with_a_gap_inside_the_cell: exact rows, kept inside the cell.
- a_curly_underline_waves_and_joins_itself_at_the_cell_edge: it waves, column 0 meets column w-1, and near the bottom every column keeps its ink.
- dotted_and_dashed_underlines_break_evenly_and_tile: dotted has at least 3 short runs; dashed is dash, gap, dash.
- a_line_past_the_bottom_stays_in_the_cell: clamped to the cell, and Underline::None draws nothing.

## atlas.rs (9)
- a_glyph_nobody_supplied_is_unknown_and_a_blank_one_is_known_and_takes_no_room: None, Some(None), no upload, and the room is still there.
- a_slot_says_where_its_upload_went_and_where_the_glyph_goes_in_its_cell: slot and upload agree, and each upload comes once.
- glyphs_keep_a_pixel_apart_and_never_overlap_or_leave_the_plane: 200 sizes, checked pairwise.
- a_row_wraps_when_the_next_glyph_does_not_fit_and_a_plane_can_fill_exactly: the exact placements, up to both edges; a glyph that didn't fit isn't noted.
- the_two_planes_fill_apart: each plane has its own room and its own origin.
- clearing_forgets_every_glyph_and_every_upload_and_makes_the_room_again.
- a_glyph_bigger_than_its_plane_draws_nothing_since_clearing_would_not_help.
- full_gives_the_bitmap_back_for_a_second_try.
- keys_tell_styles_spans_and_lines_apart.

## font.rs (21)
- term_at_16_pt_on_a_2x_display_has_the_19_by_40_cell: from S1's measured metrics.
- an_advance_a_hair_under_a_whole_pixel_keeps_that_pixel.
- a_cell_has_some_size_however_small_the_font.
- the_underline_sits_below_the_baseline_and_the_strike_at_half_the_x_height: exact values, with two lines' room above the cell's bottom.
- line_bitmaps_are_the_cell_and_take_their_line_from_the_metrics.
- logical_pixels_scale_to_whole_physical_ones: 2 logical px are 4 at 2x and 2 at 1x; rounds, at least 1.
- ink_with_nothing_in_it_has_no_place.
- a_primary_glyph_stays_where_its_font_puts_it: scale 1, pen on the baseline.
- a_primary_glyph_too_wide_for_its_cell_is_squeezed_and_centered.
- a_squeezed_primary_glyph_whose_ink_overruns_its_advance_is_centered_by_its_advance.
- a_fallback_glyph_is_centered_across_its_span_and_not_squeezed_if_it_fits.
- a_fallback_glyph_wider_than_its_span_is_squeezed_by_its_ink_or_advance.
- a_fallback_glyph_taller_than_the_cell_is_squeezed_and_stays_in_it.
- a_fallback_glyph_that_would_leave_the_cell_moves_to_its_middle.
- a_primary_glyph_may_leave_the_cell_upward_or_downward: a stack of marks isn't squeezed.
- the_bitmap_holds_the_ink_with_a_pixel_to_spare: 2,000 random inks.
- a_union_of_ink_covers_both_and_ignores_empty_ink.
- by_default_the_renderer_draws_boxes_blocks_and_powerline_and_the_font_draws_braille.
- the_first_rule_that_covers_a_character_decides_it.
- faces_are_tried_pinned_then_styled_then_regular.
- a_cluster_stays_in_the_first_face_that_has_all_of_it.

## rows.rs (38): scenes in, instances out, with a stand-in for the faces
- an_instance_is_48_bytes_with_its_fields_where_the_shader_reads_them: size and every offset.
- colors_are_rgba_in_a_u32.
- the_first_frame_builds_every_row.
- an_unchanged_frame_rebuilds_nothing_and_wants_no_glyph: same instances, no uploads.
- a_changed_cell_rebuilds_only_its_row.
- a_change_to_any_field_of_a_cell_rebuilds_its_row: all 8 fields of Cell.
- moving_a_cursor_rebuilds_no_row_and_the_cursor_moves.
- a_new_size_rebuilds_every_row.
- a_new_size_with_the_same_cell_count_is_a_new_grid.
- a_new_background_rebuilds_every_row_since_a_cell_in_it_has_no_instance.
- a_run_of_one_background_is_one_quad_and_the_scene_background_has_none.
- a_run_reaching_the_row_end_is_drawn_to_it.
- every_background_comes_before_every_glyph_and_the_cursors_come_last.
- a_glyph_goes_where_its_slot_says_in_its_cell.
- a_color_glyph_comes_from_the_color_plane.
- a_wide_glyph_is_fitted_to_its_cells_and_its_right_half_draws_nothing.
- a_padding_cell_after_a_narrow_glyph_does_not_widen_it: layout's filler is a Continuation with no wide character before it.
- a_wide_glyph_with_no_room_is_fitted_to_what_it_has.
- a_cluster_keeps_its_marks_and_its_width.
- a_space_wants_no_glyph_but_its_lines_are_drawn.
- wanted_lists_each_glyph_once_and_bold_and_italic_apart.
- a_cell_lines_go_over_its_glyph_in_their_own_colors: the underline in underline_color, the strike in fg.
- a_wide_glyph_lines_cover_both_of_its_cells.
- a_block_cursor_fills_the_cell_and_redraws_the_glyph_in_the_cell_background.
- a_block_cursor_draws_its_cell_lines_in_the_cell_background_too.
- a_bar_cursor_is_2_logical_px_wide_at_the_cell_left_edge_over_the_glyph: at 2x and 1x.
- an_underline_cursor_is_2_logical_px_high_at_the_cell_bottom_over_the_glyph: at 2x and 1x.
- a_half_block_cursor_fills_the_lower_half_over_the_glyph: half the height rounded down at an odd height.
- a_hollow_cursor_is_a_1_logical_px_outline_and_leaves_the_glyph: exact rects at 2x and 1x.
- a_cursor_on_a_wide_character_covers_its_cells_and_on_its_right_half_takes_it_whole.
- a_cursor_off_the_grid_draws_nothing.
- every_cursor_is_drawn_in_scene_order.
- a_full_atlas_starts_over_and_the_frame_still_has_every_glyph: paints the same as a fresh renderer.
- starting_over_rebuilds_rows_that_did_not_change_since_their_glyphs_moved.
- a_frame_that_needs_more_than_a_whole_atlas_drops_the_rest_and_the_next_frame_tries_again.
- a_glyph_too_big_for_any_atlas_draws_nothing_and_costs_no_restart.
- frames_built_from_the_cache_paint_what_a_fresh_renderer_paints: 120 frames of random edits, each picture equal to one from a renderer with no history.
- so_do_frames_built_through_atlas_restarts: the same with an atlas that overflows, asserting a restart happened.

## fidelity.rs (39): Ioskeley Mono Term through Core Text and Metal to a texture, read back
- term_gives_a_19_by_40_cell_at_2x_and_a_9_by_20_one_at_1x.
- a_font_that_will_not_load_is_an_error_that_names_it: also a size of 0.
- dashes_dots_arrows_and_circled_digits_keep_to_one_cell_in_term: advances, measured as ink.
- a_long_arrow_that_term_draws_two_cells_wide_is_squeezed_into_one.
- box_lines_join_across_cells_with_no_seam: full coverage across 12 cells and down 3, and ╬'s center is open.
- a_heavy_line_is_twice_a_light_one_and_joins_too.
- blocks_fill_their_cells_and_shades_stay_even_across_a_join: every pixel.
- a_range_handed_back_to_the_font_tiles_with_the_font_own_glyphs: the per-range override, with the font's uneven ▒ showing who drew it.
- wide_characters_from_fallback_faces_take_two_cells_and_stay_in_them: 中, 한 and 😀 use both cells, stay in them, and are centered.
- emoji_keep_their_own_colors_and_text_takes_the_cell_color.
- a_combining_mark_sits_above_its_base_inside_the_cell: e + U+0301 rises above e, on the same baseline, and the e under it is pixel-identical.
- a_decomposed_letter_looks_like_the_precomposed_one.
- stacked_marks_go_both_above_and_below.
- clusters_the_primary_font_lacks_come_from_the_system_whole: क + U+093F, a ZWJ emoji, a flag, ❤ + VS16.
- a_face_the_rules_pin_goes_first_and_one_without_the_glyph_falls_through.
- a_face_the_rules_pin_is_centered_in_its_cell_as_a_fallback_is.
- a_box_glyph_with_a_mark_comes_from_the_font_since_the_renderer_has_no_marks.
- braille_stays_in_the_font_by_default_and_each_dot_stays_inside_its_cell: 12 dot rows, pitch 9 inside and 13 across, S1's measure of the font.
- a_rule_can_give_braille_to_the_renderer_at_an_even_pitch: an even 10.
- a_block_cursor_fills_the_cell_and_draws_the_glyph_in_the_cell_background: the H's solid pixels turn to the cell's bg, and the bg to the cursor color.
- a_bar_cursor_covers_4_physical_px_at_the_cell_left_edge_and_leaves_the_rest: every pixel of the frame, against the same frame without the cursor.
- an_underline_cursor_covers_4_physical_px_at_the_cell_bottom_and_leaves_the_rest: the same check.
- a_half_block_cursor_hides_the_lower_half_of_the_glyph_and_leaves_the_upper: the same check.
- a_hollow_cursor_outlines_the_cell_and_draws_the_glyph_as_it_is: the same check.
- at_1x_the_bar_and_outline_are_their_logical_pixels_wide.
- a_cursor_on_a_wide_character_covers_it_and_on_an_emoji_leaves_its_colors.
- bold_is_heavier_and_italic_leans_and_bold_italic_is_both: ink totals, and the stem's lean.
- a_glyph_takes_the_cell_colors_and_a_blank_cell_shows_its_background.
- each_underline_style_is_drawn_in_its_own_color_below_the_baseline: all 5 styles.
- a_struck_cell_has_a_band_of_its_foreground_across_its_middle.
- underlining_a_wide_character_covers_both_of_its_cells.
- the_pixels_around_the_grid_are_the_scene_background.
- a_target_with_no_pixels_is_an_empty_image.
- a_frame_with_no_instances_is_cleared_to_the_background.
- frames_on_the_gpu_match_a_fresh_renderer_as_rows_change_and_the_buffer_grows.
- a_frame_that_fills_the_atlas_on_the_gpu_starts_over_and_draws_every_glyph: 2,000 CJK glyphs, then 2,000 more; the driver has to ask again.
- a_character_no_font_has_draws_the_system_placeholder_inside_its_cell: U+10FFFD draws LastResort's box, squeezed into its cell.
- the_shader_blends_as_the_written_rules_say: GPU pixels equal a plain-Rust compositor within ±1 on every channel, with solid, gray and half-transparent color instances.
- the_page_s1_drew_still_measures_as_s1_measured_it: writes page-sprites.png and page-font_only.png.

## platform.rs (15): R1's 6 unchanged, plus 9
- the_compiled_shader_lays_out_its_instance_as_quad_does: Metal's reflection of the compiled shader gives Quad's size and all 5 members' offsets. This is the struct-layout assertion.
- an_upload_that_does_not_fit_its_atlas_is_refused: past the edge, too few or too many bytes, a color pixel's 4 bytes.
- a_texture_metal_cannot_make_is_an_error_since_metal_would_stop_the_process: 0 and 16385 are errors, 16384 works.
- shaping_counts_each_glyph_where_the_font_puts_it: x and y positions count toward the ink.
- a_squeezed_glyph_is_drawn_smaller_and_keeps_its_margin.
- the_system_finds_a_face_for_what_the_font_lacks.
- shaping_gives_a_cluster_one_advance_with_its_mark_on_the_base.
- shaping_finds_a_face_for_a_character_on_its_own_and_says_when_it_is_in_color.
- a_drawn_glyph_is_coverage_where_its_fit_says_and_an_emoji_is_premultiplied_color: margins, solid ink, BGRA order.
- R1's, unchanged: metal_device_and_queue, font_loads_from_its_file, monospace_advances, missing_glyphs_and_files, astral_characters_map, a_font_size_must_be_positive.

## backend.rs (1), R1's and unchanged
- the_backend_says_whether_it_can_draw.

## Planted bugs (not committed)
I planted 84 bugs by script, one at a time, and ran die-render's tests on each. One never applied, 76 were caught, and 7 survived. Each survivor got a test, and I planted them again with bugs in the new checks. The only one left is the check of the command buffer's status after the GPU finishes, because I can't make a GPU fault in a test.

### Added after its review

rows.rs (runs on Linux):
- a_cursor_the_color_of_its_cell_stays_visible: all 5 shapes draw in the cell's foreground; a Block's glyph stays in the cell's background.
- a_cursor_of_another_color_keeps_it_on_a_cell_of_its_foreground: the usual case doesn't change.
- a_cursor_shows_on_a_cell_with_no_other_color_in_it: falls to the scene's background, then black, then white.
- a_block_cursor_cuts_its_redrawn_glyph_to_its_cell: rect cut, uv moved by the same amount, and ink wholly outside has no size.
- a_block_cursor_on_a_wide_character_redraws_the_lines_of_both_of_its_cells: lines on both cells, from either half.
- a_cursor_stays_in_a_cell_too_small_for_its_lines: every rect of every shape stays inside cells of 1×1, 3×3, 1×40 and 19×1.
- a_page_with_more_glyphs_than_the_atlas_costs_nothing_until_it_changes: an unchanged frame or a moved cursor rebuilds no row and uploads nothing; a change restarts once.
- a_page_that_fits_again_goes_back_to_rebuilding_only_what_changed: the retry flag clears.
- a_wide_glyph_lines_cover_both_of_its_cells (extended): strike too, not only underline.

font.rs (runs on Linux):
- a_primary_glyph_is_not_squeezed_for_the_pixel_rounding_took_from_its_cell: 9.6/9, 16.8/16, 15.6/15, 21.6/21 and 19.2/19 stay at scale 1 with the pen at the left; the boundary is 10.0 against 10.1 at w 9; 14.4 at w 9 still squeezes; 38.4 in 2×19 stays.
- a_primary_glyph_narrower_than_its_two_cells_is_centered_across_them: pen at 9.4; full width stays left; span 1 is untouched.
- a_fallback_glyph_is_squeezed_past_half_a_pixel_over_as_ever: 19.4 stays, 19.6 squeezes.
- a_fallback_glyph_stays_in_its_span_whichever_side_its_ink_leaves_by: left, right, within the half-pixel slack, and already inside.
- by_default_the_renderer_draws_boxes_blocks_and_powerline_and_the_font_draws_braille (moved onto `source`): sprite for boxes, blocks and powerline; braille goes to the font; no rule for other characters.
- the_first_rule_that_covers_a_character_decides_it (moved onto `source`): first matching rule wins.
- a_cluster_goes_by_its_first_character_and_only_a_lone_one_goes_to_the_renderer: no sprite for a marked or longer cluster, and the same rule decides it.
- each_rule_pins_its_own_files_in_order_after_the_faces_before_them: `pins` and `files`.
- faces_are_tried_pinned_then_styled_then_regular (extended): a chain of several pinned files keeps its order.
- a_grid_is_its_cells_and_nothing_around_them: `CellMetrics::grid` exactly.
- term_names_its_four_files_in_the_order_the_styles_use: `Config::term`.

sprites.rs (runs on Linux):
- each_quadrant_lights_the_corners_its_name_says: 10 quadrants, 4 corners, 3 sizes.
- the_round_and_thin_powerline_glyphs_point_the_way_they_should: E0B4, E0B6, E0B1 and E0B3 shapes and mirrors.

platform.rs (macOS):
- an_upload_that_does_not_fit_its_atlas_is_refused (extended): wrapping sums, a wrapping length, and empty regions.
- a_drawn_glyph_is_antialiased_so_a_curve_has_levels_between_none_and_full: over a tenth of S's pixels are partial.
- the_baseline_stays_fractional_so_a_stem_covers_88_percent_of_its_last_row: row 30 is 224±3, row 31 is empty.
- a_cluster_of_gray_and_color_runs_goes_in_the_color_plane_with_its_gray_run_white: the color plane, with white pixels in the a's part.

fidelity.rs (macOS; skips on Linux):
- every_cursor_on_a_search_match_of_its_own_color_stays_visible_and_keeps_the_letter: mono-dark's colors, all 5 shapes, every pixel.
- a_block_cursor_changes_no_pixel_outside_its_cell_whatever_its_glyph_overruns: 5 overrunning glyphs.
- a_block_cursor_on_a_wide_character_keeps_its_lines_across_both_cells: underline and strike rows, 38 px each.
- each_rule_draws_with_its_own_pinned_files: two rules, two files, and a third letter unpinned.
- a_rule_tries_its_files_in_order_and_goes_on_past_one_without_the_glyph: order, and fall-through.
- a_wide_character_term_draws_one_cell_wide_is_centered_across_its_two: ☰'s margins differ by at most 2 px.
- a_fallback_glyph_whose_ink_starts_left_of_its_pen_stays_in_its_cell: U+0E33.
- a_cluster_no_face_has_whole_comes_from_the_one_face_the_system_picks: a + U+0616, pixel for pixel against the system's face.
- a_style_reaches_the_face_the_system_picks: bold 中 has over 1.1× the ink.
- a_frame_after_a_target_with_no_pixels_still_has_the_glyphs_it_asked_for: equals a fresh renderer's frame.

## V3

### As built

ORACLE (crates/die-vim/tests/oracle.rs). Every case's text, cursor, state, registers and error positions must match your Evil. 457 new cases, with expected values from run-oracle.sh: operators 222, registers 42, textobjects 135, undo 58. They cover snake_case, ::, ->, tabs, wide characters, operators over several lines, counts on both sides, which signals stop an operator, the goal column after operators, J, p and P, the empty text, and u after every operator. Each one that differs from real Vim has a vim note (328 differ, none missing).

New runner tests in oracle.rs:
- a_kill_must_be_what_the_clipboard_should_have_been_sent: tampering with the expected clipboard text or writes fails the case.
- a_register_the_keys_didnt_change_must_stay_as_it_was: a register your Evil doesn't list must keep its preset value.
- a_case_about_the_clipboard_waits_for_k6: the five clipboard cases fail with "no clipboard yet (K6)".

crates/die-vim/tests/operators.rs, for what the oracle can't run:
- keys_that_cancel_an_operator_do_so_quietly_and_take_the_counts: for all 8 operators, Esc, F5, i<Esc>, a<Esc>, iq, ax, i<F5>, g<Esc>, gq, f/F/t/T<Esc>, f<F5> and " leave the text and cursor alone, return to normal state with no message, and drop the counts.
- another_command_cancels_an_operator_and_doesnt_run: dc, dy, dx, dp, dA, do, dq and others change nothing.
- a_cancelled_operator_forgets_the_goal_column: j after a cancel uses the current column.
- a_key_after_a_register_name_can_cancel_it: "<Esc> names no register.
- r_with_a_key_that_isnt_text_does_nothing: r<Esc> and r<F5> change nothing and drop the count.
- a_motion_that_stops_an_operator_leaves_normal_state: dh, d5h, dk, db, d{, dfz, dFz, dtz, d%, d; and d, give one error, leave the text alone and return to normal state.
- end_of_line_and_end_of_buffer_dont_stop_an_operator: d9l, dw, de and d} take what the motion reached, with no error.
- each_buffer_has_its_own_waiting_operator: d in A leaves B in normal state, and A still waits when focus returns.
- an_operator_acts_for_the_window_it_was_run_for: commands run through Args act on that window, not the focused one.
- operators_on_read_only_text_fail_and_change_nothing: one error, no insert session, and no undo group left open.
- huge_counts_stop_at_the_edges: 20-digit counts on every operator and object don't hang, and p fails with Count too large.
- a_large_register_pastes_once_without_a_count: a 2 MiB register pastes once, and 2p is refused.
- yank_makes_no_undo_step: u after yy has nothing to undo.
- a_change_and_its_insert_session_are_one_undo_step: u restores text and cursor after cw, cc, S, C, s, ciw, cip and c2j.
- a_count_on_c_doesnt_replay_the_insert: 2cw, 2s, 2S and 2C insert once.
- the_cursor_is_never_left_on_a_newline: normal state's position after dw, x, D, d$, p, daw and dG.
- rule_1_after_an_operator_at_the_end_of_the_text: dd and cc on the last line, and operators on the empty line an insert session left.
- case_changes_that_change_the_length: gUw on ß, and 2~ past ß.

Unit tests in src/registers.rs:
- the_ring_keeps_nine_kills_newest_first: " and 1 hold the newest kill, and 9 the ninth.
- a_preset_unnamed_register_is_the_rings_head: "1 reads a preset ", and the next kill moves it to 2.
- only_a_yank_that_names_no_register_fills_zero: deletes and named yanks leave 0.
- the_black_hole_keeps_nothing: "_ leaves " and 0 empty, and reads as empty text.
- appending_mixed_kinds_makes_whole_lines: char+char stays char, and any line makes whole lines.
- read_only_registers_refuse_and_nothing_is_kept: ": refuses and pushes nothing.
- an_empty_register_is_an_error: no register, q and Q all fail when empty.
- naming_a_digit_overwrites_that_place_counting_round: "3 on a two-kill ring, as your Evil.

Unit tests in src/objects.rs:
- a_word_is_cut_at_snake_case_and_a_paths_colons: iw on foo_bar::baz gives foo, _, bar, :: and baz, and iW the whole.
- quotes_pair_up_from_the_start_of_the_text: pairs and multi-line strings.
- brackets_skip_strings_and_escapes: i( past ")" and \).
- angle_brackets_count_only_for_their_own_object: < means nothing to i(.

Unit tests in src/operators.rs:
- counts_multiply_and_saturate: counts multiply and can't overflow.
- exclusive_ranges_ending_at_a_lines_start_step_back: char range or whole lines, by where it starts.
- inclusive_ranges_take_the_whole_grapheme: the mark goes with its letter.
- inclusive_ranges_stop_short_of_a_newline: unless that's where it started.
- inverting_a_character_with_no_single_other_case_leaves_it: ß stays, é becomes É.

Unit tests in src/motions.rs, all plain functions over Start:
- with_an_operator_w_stops_at_the_end_of_the_line_before_the_next_word: where w lands with d waiting.
- with_c_w_stops_at_the_end_of_the_word: cw on a word and on spaces.
- with_an_operator_e_stays_on_a_one_letter_word: only on a word character.
- with_an_operator_dollar_is_the_last_character_and_exclusive_on_an_empty_line: its target and kind.
- with_an_operator_l_reaches_the_newline: normal state's doesn't.
- with_an_operator_close_brace_runs_to_the_end_of_the_text: normal state stops on the last character.
- percent_finds_any_closer_and_skips_strings: partners, this-line rule and multi-line partners.

src/lib.rs:
- killing_a_buffer_with_an_operator_waiting_drops_it: the pending map empties when the buffer dies.

CHECKING THE TESTS: 109 planted bugs across operators, registers, objects, text, insert, motions, lib and grammar. The first run of the 99 then planted let 14 survive, and 2 didn't apply. Each survivor became a new case or a new runner check, or (one, a redundant condition) a code simplification. On the final code all 109 are caught.

DIFFERENTIAL: see notes.

### Added after its review

- j_with_a_count_is_one_transaction: 5J gives "a b c d e\nf\n", and edits_since lists [7,5,3,1].
- j_leaves_another_windows_cursor_on_its_text: another window's cursor on the second c stays on it after 3J.
- j_with_a_huge_count_on_a_long_text_is_quick: 99999J over 20,000 lines takes under 20 s, leaves one line, and u restores it.
- a_count_on_j_that_would_make_too_long_a_line_fails: over 1 MiB, a message, unchanged text and cursor; J and 2J still join two 2 MiB lines.
- a_failed_command_on_read_only_text_leaves_the_cursor_where_evils_does: 27 rows of cursor and " from Evil.
- r_with_escape_forgets_the_goal_column: 4ljr<Esc>j lands at byte 11.
- keys_that_cancel_an_operator_do_so_quietly_and_take_the_counts: gains F<F5>, t<F5>, T<F5>.
Oracle cases, 93 new (980 to 1,073), lines already there unchanged, except that a case added this round was renamed before merge:
- J's corners, in operators.
- failed u, in undo.
- The review's gaps, across operators, registers, textobjects and motions.
- Quotes and bracket counts, in textobjects.
- I on blank lines, in insert.
- $ after a vertical motion, in operators.
- The gU rule, in undo.
- Row 22, in operators, with 5 hand-written fixtures; deviations go from 53 to 58.
- l and h over a mark, in motions.
- r<CR> indentation, in operators.
- Escape at a wrapped row's start, in wrapped.
No ported S4 test or existing expected value changed.

## Added at the merge

- `die-core/tests/config.rs`, `config_commands_belong_to_config`: `toggle-max-width` belongs to core with an extension in the first slot.
- `die-render/tests/rows.rs`, `a_bar_compose_puts_past_a_full_row_draws_at_its_windows_right_edge`: a real `past_end` from `compose` to the bar's place, for a full row and two rows ending in a wide character, plus a row with room and a bar inside a full row.
- `die-render/tests/rows.rs` and `fidelity.rs`: a bar past a row's end sits at the cell's right edge, and only a bar moves.
- `die-vim/tests/operators.rs`, `rebuilding_indentation_on_a_joined_line_counts_from_the_lines_own_start` and `inside_quotes_with_the_closing_quote_hidden_is_empty`.
- `die-vim/tests/oracle.rs`, `wrapped_rows_without_gj_match_the_tty_run`: the 8 wrapped cases K4 passes, at their own window sizes.
- `die-core/tests/scene.rs`, `a_limit_in_ms_stretches_only_at_background_priority`.
