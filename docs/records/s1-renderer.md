> **Decision record for scope D13 and spike S1.** Status: **accepted 2026-10-19.** You chose the recommendations on both decisions in §4: accept about 23 ms in a tile as the platform's floor, and keep the borderless window with an AeroSpace rule. The proposed edits are folded into scope.md. The "needs you" runs in §6 are still open. Date: 2026-10-19. Prototype: `spikes/s1-renderer/` (throwaway, standalone cargo project). Machine: Apple M5 Pro, built-in 120 Hz ProMotion panel at 3024×1964, macOS 26.5. Spikes S2–S5 ran at the same time (load average 3–6), so every timing below carries that caveat.

# S1 decision record: renderer and platform (winit + Metal + Core Text)

## 1. Verdict

**Pass with changes.** D13 holds: winit 0.30.13 (pinned), Metal through objc2-metal, and Core Text. No fallback is triggered.

- **What the stack costs is small.** A full redraw of 159×49 cells, every cell changed, takes 0.65 ms of main-thread CPU and 0.70 ms of GPU time (p99). A 60 Hz full-window pixel buffer showed all 3,600 frames in 60 s with no main-thread pixel copies. Idle draws nothing. DIE's own share of a keystroke, from the key event to the committed frame, is 0.4 ms (p50) and 2.4 ms (p99).
- **Typing latency is set by the macOS compositor, not by the stack.** In a window that covers the whole display, key to display is 11 ms (p50) and 17.5–17.7 ms (p99). In a window that is one tile among others, it's about 23 ms (p50). No setting I tried changes that, and wgpu or AppKit directly would sit behind the same compositor.
- **A 60 Hz pixel buffer adds up to one refresh to typing (p99 +8 ms).** The panel refreshes at most every 8.3 ms, so a keystroke that lands just after a pixel-buffer frame waits for the next refresh. The "≤ 1 ms" target can't be met on this panel.
- **The keyboard is clean with one small fix.** All 50 chords pass app-side once an NSEvent local monitor turns Caps Lock into events (winit drops them). Real-hardware behaviour (beep, accent popup) still needs your run.

So the stack stays, and the latency criteria change to what DIE controls. The spike also found rules for frame scheduling and resizing that §5.11 should state (the edits below, and §4 items 5 and 6).

### Fallbacks

| Fallback | Trigger | Result |
|---|---|---|
| AppKit directly instead of winit | Keyboard fails beyond what an NSEvent monitor fixes | **Not triggered.** The monitor fixes the only gap (Caps Lock). If it's ever needed, it costs about 1.2–1.8k more lines. |
| Render thread behind the scene boundary | Check 3 or 5 fails | **Not triggered.** 100% of producer frames shown; worst main-thread stall 9.2 ms. Not prototyped. |
| wgpu 30.0.1 with buffer uploads only | Metal bindings too painful | **Not triggered.** The Metal part is about 750 formatted lines and was straightforward. |

The wgpu stall that scope D13 cited (about 5.9 ms p99) didn't come from this spike, which never built wgpu. It came from the 2026-10-18 review probe: `write_texture` holds a queue lock while it copies, and a ring on buffer uploads avoids it. See [platforms.md](platforms.md) §2.1.

### Edits I propose to docs/scope.md (not made)

**D13 row.** Status becomes "Decided (2026-10-19); confirmed by S1 (2026-10-19)". Append to the rationale:

> S1 confirmed it on your machine: a full redraw of 159×49 cells takes 0.65 ms of main-thread CPU and 0.70 ms of GPU time (p99), a 60 Hz full-window pixel buffer showed all 3,600 frames with no main-thread pixel copies, and idle draws nothing. Typing latency is set by the macOS compositor, not by this stack. The full record is [records/s1-renderer.md](records/s1-renderer.md).

**§5.11.** Change the "GPU path" bullet's "Redraw only what changed." to "Rebuild only the rows that changed; the GPU recomposites the whole frame, which costs under 1 ms." Then add two bullets after it:

> - **Latency.** DIE handles a key and commits its frame within about 2.5 ms (p99). The macOS compositor adds the rest: about 11 ms (p50) when DIE's window covers the whole display, about 23 ms when it's one tile among others. While a 60 Hz pixel buffer animates, a keystroke can wait one more refresh (8.3 ms), because the panel refreshes at most every 8.3 ms.
> - **Frame scheduling.** Render right after input, in the same run-loop turn. Run a display link only while something animates. Don't draw while the window is occluded. Before drawing, match the drawable to the window's current size. During live resize, present inside the Core Animation transaction, one drawable per transaction, and flush it.

In the "Font" bullet, "You'll need to install it" can become "DIE loads it from the font files, so it doesn't need installing". The "One native window" bullet depends on your answer to decision 2 in §4.

**Guard 7.** Keep "`die-render` ≤ about 5k" and add "(S1's estimate: 4.1–5.0k)".

**§12, S1 row.** Mark it done with a link to this record, and replace the first bullet, "typing latency p50 ≤ 12 ms and p99 ≤ 17 ms", with:

> DIE's share of typing latency (key event to committed frame) p99 ≤ 3 ms, and key to display p50 ≤ 12 ms in a window that covers the display

**§12, M3 exit test.** Replace "A 60 fps pixel-buffer fixture keeps typing latency within 1 ms of S1's." with:

> A 60 fps pixel-buffer fixture keeps DIE's share of typing latency within 1 ms of S1's. Key to display may be up to one refresh (8.3 ms) slower at p99, which is the panel's limit.

## 2. Pass criteria

"App-side" means input injected inside the app with `NSApplication postEvent` (no CGEventPost, no permission prompts). An injected key's timestamp is the moment it was posted; latency runs to the frame's `presentedTime`, on the same clock (checked: `systemUptime` and `CACurrentMediaTime` agree to the microsecond).

| # | Check | Target | Measured | Status | How |
|---|---|---|---|---|---|
| 1 | Typing latency, 500 keys | p50 ≤ 12 ms, p99 ≤ 17 ms; p99 rise ≤ 1 ms with producer | **Window covering the display**, two clean runs of 500: p50 11.3 / 11.0 ms, p99 17.7 / 17.5 ms. Two earlier runs: p50 10.0 / 10.7 ms, p99 15.6 / 22.8 ms (the second predates the validity check). Key to committed frame: p50 0.4, p99 2.4 ms. **With the 60 Hz producer**: p50 12.6, p99 25.7 / 25.5 ms (rise +8.0 ms). **1200×800 window**: p50 22.7, p99 33.5 ms. **AeroSpace-style tile** (8 pt gaps, 60 keys): p50 23.7, p99 29.7 ms | **Fail.** p50 passes only when the window covers the display; p99 misses by 0.5–0.7 ms; the producer rise fails. Platform-bound (§4, items 1 and 3) | App-side; `interactive typing` for real keys: needs you |
| 2 | Full redraw, 159×49, all changed | main-thread CPU p99 ≤ 2 ms, GPU p99 ≤ 1.5 ms | 600 frames, 13.5–14k quads each (every cell its own background, glyph and decorations): CPU p99 0.65 ms (wall 0.65), GPU p99 0.70 ms. Building the scene (core's side) adds 0.32 ms p99 | **Pass** | Automated |
| 3 | 60 s of producer frames | ≥ 99% presented; 0 bytes main-thread memcpy | Full-window buffer, 3024×1880 at 60 Hz (22.7 MB a frame): 3,600 of 3,600 presented, 0 bytes copied on the main thread (82.6 GB on the producer thread), 0 ring waits. Main-thread frame CPU p99 0.32 ms, GPU p99 1.26 ms. Half-window buffer: also 3,600 of 3,600 | **Pass** | Automated |
| 4 | Idle 60 s | 0 frames, < 0.3% CPU | 0 frames, 0.004% CPU, 18 wakeups in 60.0 s | **Pass** | Automated |
| 5 | 20 occlusion cycles | no main-thread stall > 50 ms | With the producer at 60 Hz and a 5 ms main-thread ping: 20 cycles of an opaque window covering DIE, then 10 of ordering DIE out and in. Worst stall 9.2 ms, worst `nextDrawable` wait 9.1 ms. Without the occlusion guard (5 + 5 cycles): worst stall 6.6 ms. The macOS 26 one-second `nextDrawable` block didn't reproduce | **Pass app-side**; real Cmd-Tab: **needs you** | App-side (cover window, orderOut/orderFront) |
| 6 | Backing scale change | correct drawable within one frame; atlas and metrics rebuilt | Simulated, 10 changes (2x→1x→2x five times): every first frame had the right drawable (1512×982 at 1x, 3024×1964 at 2x) and was presented. Atlas rebuilt every time, cell 19×40 → 9×20 px, pixel buffer told its new width (1523 → 756 px). Change to presented: p50 21 ms, max 32 ms | **Pass simulated**; real display change: **needs you** (needs a second display) | Our scale path called directly; AppKit's notification not exercised |
| 7 | Live resize, 10 s | no stretched frames; frame p99 ≤ 8 ms | Programmatic, 1,160 steps (`setFrame` every 1/120 s, then a synchronous render): 0 steps without a matching frame, 0 drawable-size mismatches, frame p99 4.9 ms (max 5.8). Through winit's own Resized path at 60 Hz: 717 frames, 0 mismatches, p99 6.7 ms (max 10.2) | **Pass, approximated**; real drag: **needs you** | Programmatic resize, not a user drag |
| 8 | Exec to first frame (warm) | ≤ 150 ms | Three runs of 11 warm launches to the first presented frame: medians 142, 107, 102 ms; p90 162, 118, 110 ms; worst 190 ms (first run, under load) | **Pass** on the median; one run's p90 and max are over | Automated, app not activated |
| 9 | Fidelity | box, braille, block tile; CJK and emoji in 2 cells; `—` `…` in 1 cell with Term | Rendered offscreen at 16 pt@2x and checked pixel by pixel. Box and block glyphs tile (coverage 1.00 at every cell join, renderer-drawn and font-drawn). Braille tiles (4 dot rows per cell, none crossing a cell edge). `中`, `한` and `😀` fit in 2 cells. With Term, `— … → ①` have an advance of 19.2 px = 1.00 cell and stay inside their cell | **Pass** | Automated: pixels and advances |
| 10 | Keyboard matrix | order, no beep, no popup, encoder fields, Option as Alt | 50 chords × 4 variants (IME off/on × monitor fix off/on). With the monitor: 50/50 in both IME states. Without it: 49/50 (Caps Lock gives no events). Press, repeat and release in order; physical key, logical key, unshifted codepoint, modifiers with left/right, repeat flag and text all present. Option acts as Alt on both sides (M-x gives `x`). Cmd-Esc press and release arrive. No would-be beeps | **Pass app-side**; beep, popup and real chords: **needs you** | App-side injection |
| 11 | Rebuild loop | linked binary ≤ 5 s | Real code edit, then `cargo build` to a linked `die-dev`. Dev: 0.36 s (edit die-vim, 3.4k lines), 0.50 s (edit die-render). Release: 1.42 s, 1.83 s (max 2.81 s) | **Pass** | Automated, `skeleton/` workspace |

## 3. What was built, and how to rerun it

`spikes/s1-renderer/` is about 2.9k lines (2.56k code lines; 5.8k once rustfmt'ed). Its README lists every command and option.

- **Window and layer.** A winit window, borderless or with a hidden title bar, with a CAMetalLayer that the app owns on winit's NSView. Option as Alt, no default menu, press-and-hold off (registered for this process only, nothing written to disk).
- **Renderer.** One MSL pipeline, compiled at startup from source (1.7–2.2 ms), draws instanced quads: cell backgrounds, glyphs from a gray atlas and a color atlas, five underline styles, strikethrough, the cursor and the pixel-buffer placement. Rows are rebuilt only when dirty.
- **Cells.** DIE's own cell type: fg, bg, attributes, underline style and color, link id, wide-character continuation.
- **Text.** Core Text loads Ioskeley Mono Term (v2.1.0, four faces) from the font files, nothing installed. Fallback goes through `CTFontCreateForString` (PingFang, Apple Color Emoji). Glyphs wider than their cells are squeezed to fit. Box, block, braille and powerline glyphs can be drawn by the renderer, with a per-range override back to the font; braille stays in the font by default.
- **Views.** A full-window file view (winit's 1,910-line `window_delegate.rs`) with a status line, an insert line you type into, and a split showing the pixel buffer.
- **Pixel buffer.** A 60 Hz producer thread owns one CPU buffer. `present(damage)` copies the damaged rectangle into a ring of three shared MTLBuffers on the producer's thread. The main thread only encodes a blit, and only try-locks the hand-off.
- **Instrumentation.** Every frame is logged with its main-thread CPU and wall time, `nextDrawable` wait, GPU time, `presentedTime`, drawable size and expected size, and every key carries its event timestamp to the frame that shows it.
- **Skeleton workspace** (`skeleton/`). DIE's planned crates with this dependency set: a stub die-core; die-render compiling this prototype's modules in place; a generated 3.4k-line die-vim; a stub die-term (no engine; that's S2); die; and die-dev.

To rerun, from `spikes/s1-renderer/`:

```sh
./fetch-font.sh                                       # Term font into ./fonts (sha256-checked); the scratch copy may be gone
cargo run --release -- fidelity                       # check 9, no window, 2 s
cargo run --release -- bench keys clicks typing       # checks 10 and 1 (~2 min)
cargo run --release -- bench redraw producer --pix-full   # checks 2 and 3 (~70 s)
cargo run --release -- bench scale resize             # checks 6 and 7 (~20 s)
cargo run --release -- bench idle                     # check 4 (~62 s)
cargo run --release -- bench occlusion                # check 5 (~90 s)
cargo run --release -- firstframe --no-activate       # check 8 (~15 s)
cargo run --release -- rebuild                        # check 11 (~1 min)
cargo run --release -- bench typing --windowed        # latency in a 1200×800 window
cargo run --release -- bench typing --frame 8,8,1496,966   # latency in an AeroSpace-style tile
cargo run --release -- bench typing --pix-layer       # experiment: pixel buffer on its own layer
```

Each bench opens one window for its run, takes focus once at launch, and closes when done.

**Toolchain (D16).** cargo and the Command Line Tools only. No C is compiled through `cc` on macOS: `cc` appears only as a build dependency of android-activity, which isn't built here, and objc-sys compiles its one `.m` file only with a feature that's off. The binary is 2.1 MB (1.5 MB stripped).

**Line counts** (code lines, without blanks and comment lines; "fmt" is after rustfmt):

| Module | Code | fmt | What it is |
|---|---|---|---|
| gpu.rs | 390 | 824 | Metal device, MSL, pipeline, atlas, instances, frame log, pixel-layer experiment |
| app.rs | 398 | 821 | window and layer, events, render scheduling, resize and scale paths, script runner |
| sprite.rs | 133 | 295 | renderer-drawn box, block, braille, powerline, underline sprites |
| font.rs | 138 | 289 | Core Text: load from file, metrics, fallback, rasterization |
| platform.rs | 118 | 217 | AppKit glue: layer, NSEvent monitor, event injection, would-beep counter |
| pixbuf.rs | 113 | 198 | producer thread and staging ring |
| scene.rs | 109 | 287 | cell type and grid (core's side), file view |
| keys.rs | 106 | 353 | DIE key event (about 100) and the chord matrix (test data) |
| bench.rs, interactive.rs, fidelity.rs, main.rs, stats.rs, lib.rs | 1,052 | 2,530 | test harness and CLI |
| **Total** | **2,557** | **5,814** | |

**Production `die-render` estimate: 4.1–5.0k lines, inside the ~5k budget but with little room.** The prototype covers about 2.3k formatted lines of what die-render needs. Still to come:

- images and general placements (source rectangle, offset, z, clipping, images through the staging ring): 0.4–0.6k
- the fallback chain per range, clusters and combining marks, atlas eviction: 0.3–0.5k
- mouse and precise scrolling: 0.2–0.3k
- scene API and text snapshots: 0.25–0.35k
- display link: 0.15–0.25k
- finishing the sprites (arcs, double-line joins): 0.2–0.3k
- clipboard, IME toggling, a pre-commit render hook, latency log and error paths: 0.3–0.4k

## 4. Surprises, risks and your decisions

1. **The compositor sets typing latency.** In every run, the key reached the committed frame in 0.4 ms (p50) and 2.4 ms (p99). The rest happened after commit.
   - Only a window that covers the entire display got about 11 ms (p50). A window filling the screen below the menu bar, one with 8 pt gaps, or a 1200×800 window all got about 22–24 ms.
   - Turning off display sync, presenting with the Core Animation transaction, and using 2 drawables didn't help. Transaction presents made it worse (31 ms).
   - That's how macOS composites windows, so wgpu or AppKit directly would measure the same. Your menu bar auto-hides and AeroSpace uses 8 pt gaps, so a DIE tile would get the ~23 ms path.

   **Decision 1:** accept about 23 ms (p50) in a tile as the platform's floor, or make a full-display mode part of how you run DIE when you want the ~11 ms path. I'd accept the floor and keep the full-display path as a known option.

2. **AeroSpace leaves a borderless winit window floating.** With your current config, the borderless window kept whatever frame it asked for. The hidden-title-bar window was tiled at once (744×933 pt). Injected clicks 6 pt below the top edge reached the view in both styles. Scope §5.11 says "Borderless … AeroSpace handles placement", and that isn't true by default.

   **Decision 2:** borderless plus an AeroSpace `on-window-detected` rule that tiles DIE (one line in your aerospace.toml), or a hidden title bar (rounded corners, tiled by default). I'd keep borderless with the rule, since it matches your Emacs frame.

3. **A 60 Hz pixel buffer costs typing up to one refresh.** On this ProMotion panel, frames appear on a 4.17 ms grid but never closer than 8.33 ms apart.
   - A keystroke frame that lands while a pixel-buffer frame is still in flight, or soon after one was shown, waits for the next refresh. Key p99 goes from 17.7 to 25.7 ms; p50 moves 1.3 ms.
   - Putting the pixel buffer on its own CAMetalLayer cut the p99 rise to +4.3 ms. The cost is that text and pixels are no longer one atomic frame.
   - I'd keep one layer in v1 and revisit at M3. The M3 exit test needs the rewording in §1 either way.

4. **winit 0.30.13 brings a second objc2 generation.** It depends on objc2 0.5 and objc2-app-kit 0.2, while objc2-metal, objc2-quartz-core and objc2-core-text are 0.6 and 0.3. Both live in the binary and meet only through raw pointers (the NSView from raw-window-handle). That's workable, but die-render's platform module has to keep the two apart. It also adds compile time.

5. **winit runs its about-to-wait callback after Core Animation commits.** Anything rendered there is committed one run-loop turn later. The spike hit two real bugs from this, both fixed:
   - A render between a window resize and winit's queued Resized event drew at the old size. That's a stretched frame, fixed by matching the drawable to the view before every render.
   - Two transaction-bound presents in one Core Animation transaction made `nextDrawable` block for its full 1 s timeout, five times in one 12 s run. Fixed by flushing the transaction after each one.

   Both rules belong in §5.11 (the "Frame scheduling" bullet above).

6. **The main thread must never take a lock the producer can hold.** The spike deadlocked once this way. The main thread now only try-locks the hand-off, and a busy lock means the frame shows on the next wakeup. That's worth one sentence in §5.2's "The editor never waits".

7. **Startup.** About 50 ms of the ~100–140 ms is AppKit launching to the first window. The first one or two frames are dropped while the window appears. objc2-app-kit's default features also link CloudKit, CoreData and CoreImage, which DIE doesn't need. Trimming those features is the first thing to try if startup matters.

8. **Font notes.** Term still has 23 single-width characters that are 1.5 cells or wider (the long arrows U+27F5–U+27FF, ⟝, ⟞), against 1,282 in your installed build. So the squeeze-to-fit rule stays. The font's braille dots are 9 px apart within a cell and 13 px across a cell edge; renderer-drawn braille is an even 10 px. Your `init.el` pins braille to the font, so that's what you see in Emacs today. At 16 pt@2x the font's own box glyphs tile perfectly too, because the cell height includes the leading.

9. **Smaller traps.**
   - MSL pads a struct of float4s to 16 bytes, so the instance struct needed explicit padding. A size assertion belongs in die-render's tests.
   - Above about 30 Hz, the window server merges programmatic resizes and reports almost no `presentedTime`. That's why check 7 is an approximation.
   - An injected Ctrl-Space whose characters are a NUL is dropped by AppKit. That's a test artefact; with a space it arrives.
   - A ScaleFactorChanged with an unchanged factor can arrive at startup.

10. **Noise.** S2–S5 ran at the same time (load average 3–6). The producer phase of one typing run (A2) and a whole separate-layer run (E) were spoiled because another app took focus and covered the window. Those are excluded. Later runs mark every typing phase `valid` or not; the first full run (A) predates that check.

## 5. Raw data

Everything is under `spikes/s1-renderer/results/`:

- Check 1: `bench-A3.json`, `bench-A5.json` (window covering the display, 500 + 500 keys); `bench-A2.json` (its no-producer phase is valid); `bench-A4win.json` (1200×800); `bench-frame-aerospace.json`, `bench-frame-0,38,1512,944.json`, `bench-frame-10,48,1492,924.json`, `bench-frame-0,0,1512,982.json` (frame sweep); `bench-E2.json` (separate pixel layer); `bench-win-*.json`, `bench-smoke-displaysyncoff.json`, `bench-smoke-drawables2.json` (settings tried); `bench-A.json` (first full run, before the validity check), `bench-E.json` (spoiled by focus loss, for reference)
- Checks 2 and 3: `bench-B2.json` (worst-case redraw, full-window producer), `bench-B.json` (half-window producer)
- Checks 4 and 5: `bench-C.json`, `bench-D.json`
- Checks 6 and 7: `bench-B.json` (scale), `bench-B-resize.json` (resize after the fixes), `interactive-resize-selftest.*` (winit's Resized path), `bench-smoke-resize{8,15,30,60}.json` (presentation against resize rate, measured before the flush fix)
- Check 8: `firstframe-run2.json`, `firstframe-run3.json`. The first run's file was overwritten; its console summary was median 141.7, p90 161.8, max 190.0 ms.
- Check 9: `fidelity.json` and `fidelity-{term,installed}-{sprites,font_only,braille_sprite}.png`
- Check 10: `bench-K.json` (final matrix), `bench-K-titlebar.json` (clicks), `bench-keys-cspace.json` (the Ctrl-Space artefact)
- Check 11: `rebuild.json`
- Every run's frame log: `frames-<label>.csv`. Interactive self-tests: `interactive-*-selftest.{json,md}`.

## 6. Needs you

First run `./fetch-font.sh` in `spikes/s1-renderer/`, since the scratch copy of the font may be gone. Each command below opens one window, guides you on screen, and writes `results/interactive-<mode>.json` and `.md`. Cmd-Q stops any of them early and still saves.

| Command | What you do | Time |
|---|---|---|
| `cargo run --release -- interactive typing --fullscreen` | Type normally until the counter reaches 500. The pixel buffer starts animating halfway. Optionally run it again with `--style titlebar` instead of `--fullscreen`: AeroSpace tiles that window, so you get the tile case | 3–4 min (each) |
| `cargo run --release -- interactive keys` | Press each of the 50 chords shown (hold the "hold" ones about 1 s). F-keys may need fn; Cmd-. skips one. At the end, answer y/n: did you hear a beep, did an accent popup appear | 5 min |
| `cargo run --release -- interactive occlusion` | 20 times: Cmd-Tab to an app whose window covers this one completely (or Cmd-H), wait a second, Cmd-Tab back | 1–2 min |
| `cargo run --release -- interactive resize` | Float the window in AeroSpace, then drag an edge or corner around for 10 s. Note whether any text looks stretched | 30 s |
| `cargo run --release -- interactive scale` | Only if you have a second display at another scale: move the window across and back twice, then Cmd-Q | 1 min |
| Open `results/fidelity-term-sprites.png` and `results/fidelity-term-font_only.png` | Compare the glyph shapes with your Emacs at 16 pt | 1 min |

And the two decisions in §4: the latency floor in a tile (decision 1) and the window style with AeroSpace (decision 2).
