# S1 renderer and platform spike

Throwaway prototype for scope D13: winit 0.30.13 (pinned), Metal through objc2-metal, and Core Text. The decision record is [docs/records/s1-renderer.md](../../docs/records/s1-renderer.md). This crate is standalone (empty `[workspace]`) and is not part of DIE.

## What it is

- A winit window (borderless, or hidden title bar with `--style titlebar`) with a CAMetalLayer we own on winit's NSView.
- A Metal renderer: one MSL pipeline drawing instanced quads (cell backgrounds, glyphs, five underline styles, strikethrough, cursor, the pixel-buffer placement). Per-row instance caches; nothing is drawn when nothing changes.
- A Core Text glyph atlas for Ioskeley Mono Term at 16 pt and the display's backing scale, loaded from the font files (nothing is installed). Box drawing, blocks, braille and powerline can be drawn by the renderer; braille stays in the font unless `--braille-sprite`.
- A file view with a status line, an insert line you type into, and a split showing a 60 Hz producer-thread pixel buffer fed through a staging ring of shared MTLBuffers.
- Instrumentation: each key's event timestamp against the frame's `presentedTime`, main-thread CPU per frame, GPU time per command buffer, nextDrawable wait, producer frames presented, main-thread pixel copies.

## Font

The Term font is looked up in `--font DIR`, `$S1_FONT_DIR`, `./fonts`, then the spike's scratch download directory. To get it:

```sh
./fetch-font.sh          # downloads v2.1.0, checks SHA-256, extracts 4 .ttf files to ./fonts
```

Without it the app falls back to your installed (non-Term) Ioskeley Mono and says so. `--font installed` forces that.

## Commands

All commands: `cargo run --release -- <command> [options]` from this directory (or `./target/release/s1 ...` after a build).

| Command | What it does | Time |
|---|---|---|
| `view` | Opens the window: type into the insert line, PageUp/PageDown scroll, Cmd-Q quits | as long as you like |
| `bench [names...]` | Automated, app-side checks in one full-display window. Names: `keys clicks typing redraw producer scale resize idle occlusion` (default: all) | ~6 min for all |
| `fidelity` | Offscreen render of a test page, pixel checks, advances; writes PNGs. No window | 2 s |
| `firstframe [--runs 12] [--no-activate]` | Launches the app repeatedly and times exec to first presented frame | 15 s |
| `rebuild [--runs 6]` | Edits a leaf crate in `skeleton/` and times `cargo build` to a linked binary (dev and release) | 1 min |
| `interactive typing [--fullscreen] [--keys 500]` | You type; latency per key, without and with the 60 Hz producer | 3–4 min |
| `interactive keys` | Guides you through ~50 chords; checks order, fields, beep, accent popup | 5 min |
| `interactive occlusion` | You Cmd-Tab away and back 20 times; measures main-thread stalls | 1–2 min |
| `interactive resize` | You drag a window edge for 10 s; checks for stretched frames and frame time | 30 s |
| `interactive scale` | Needs a second display with another scale; you move the window across | 1 min |

Every interactive mode also takes `--selftest`, which drives it with app-side input to check its plumbing (results go to `*-selftest.*`).

Options: `--style borderless|titlebar`, `--font term|installed|DIR`, `--no-sprites`, `--braille-sprite`, `--no-split`, `--windowed` (bench in a 1200×800 window), `--frame x,y,w,h` (points from the top-left), `--quick` (short benches), `--label NAME`, `--pix-layer` (experiment: pixel buffer on its own CAMetalLayer), `--pix-full` (full-window pixel buffer), `--no-occlusion-guard`, `--no-key-fixes`, `--no-reconcile`, `--no-flush`, `--display-sync off`, `--drawables N`, `--pwt`.

## Results

`results/` holds one JSON per run (`bench-<label>.json`, `fidelity.json`, `firstframe.json`, `rebuild.json`, `interactive-<mode>.json` plus a `.md` summary), the raw frame logs (`frames-<label>.csv`) and the fidelity PNGs.

## Layout

```
src/app.rs          window, layer, events, render scheduling, resize and scale paths, script runner
src/gpu.rs          Metal: MSL, pipeline, atlas, instance building, frame log, pixel-buffer layer experiment
src/font.rs         Core Text: load from file, metrics, fallback, rasterization
src/sprite.rs       renderer-drawn box/block/braille/powerline and underline sprites
src/pixbuf.rs       producer thread and staging ring
src/platform.rs     AppKit glue: layer attach, NSEvent monitor, postEvent injection, would-beep counter
src/keys.rs         DIE key event and the keyboard matrix
src/scene.rs        DIE cell type, grid, file view
src/bench.rs        automated benches;  src/interactive.rs  interactive modes
src/fidelity.rs     offscreen pixel checks;  src/stats.rs  clocks, percentiles, JSON
skeleton/           DIE's planned workspace shape with this dependency set (check 11)
```
