# DIE: Diminutive Interactive Editor

**Design scope · agreed 2026-10-09 · revised 2026-10-19 after review · no code yet**

Every decision in this doc has been accepted. The originals date from 2026-10-09, and the review's changes from 2026-10-19; the questions and your answers are in [.questions/](../.questions/README.md). What's left is the S0 spikes (§12), which confirm specific choices or fall back to named alternatives, and the open questions in §13.

> **This project builds DIE itself: the core, plus `die-vim` and `die-term`.** It doesn't build extensions. Your packages (seek, scrib, manifold, orchid, termin, delta), and everything else this doc lists as an extension, are yours to build later, separately. They appear here only as requirements the core has to support.

Every row in the decision log is **Decided**: you've made the call. Where a spike can still change a choice, the row says so. Rows changed in the review carry the date.

## 1. Pitch

DIE is a small modal editor and terminal host, written in Rust as a native app for macOS and Linux and built around Emacs-style buffers. The core is a platform. It provides buffers, windows, modal keymaps, named commands, one text surface, and a two-layer display: a monospace cell grid, plus pixels composited with it.

Terminal buffers give you ghostel's experience on the same engine, Ghostty's libghostty-vt: kitty's keyboard and graphics protocols, full mouse support and shell integration. SSH happens inside them. Extensions can own **pixel buffers**, modeled on the canvas in Emacs 32, for plots, viewers, video and games.

Anything you'd call an application (notes, an AI client, a file browser) is a Rust crate you compile in, the same way those things are packages on top of Emacs today. You change DIE by editing source and rebuilding.

## 2. Why: what to keep from Emacs, and what to fix

**Keep:**

- Buffers as named, switchable resources that don't have to be files: terminals, previews, pickers, chat sessions.
- Extensibility deep enough that your own packages (seek, scrib, manifold, orchid, termin, delta) are as capable as anything built in.
- Modal editing everywhere, not only in text.
- Self-documentation: every key leads to a named command with a docstring.

**Fix:**

- **Graphical content is a workaround in Emacs.** About 215 lines of `init.el` (lines 422–636) exist only to make the markdown-xwidget preview behave, handling the history stack, placeholder URIs, scroll restore and cleanup on kill.
- **Terminal graphics fight Emacs's display model.** ghostel, which you use alongside vterm, has to cut each kitty image into per-row overlays and translate between Emacs's logical pixels and the physical pixels that programs expect. In a renderer built around a cell grid, both are trivial.

**Doesn't need building (judging from your config):** LSP, tree-sitter, Org, backups and auto-save (you turn the last two off), and UI chrome. You've disabled the menu, tool and scroll bars, use an undecorated frame, and your mode line shows only `default-directory`.

## 3. Scope guards

These rules keep "diminutive" true as the project grows.

1. **Platform, not applications.** The core ships no notes app, AI client, file manager or git UI. It ships the primitives those need, and this project builds only the platform. *(Decided)*
2. **The source is the config.** Extensions and your personal configuration are Rust crates. Changing behavior means rebuild and restart. There is no embedded scripting language. *(Decided)*
3. **DIE runs locally.** Remote work happens through SSH inside a terminal buffer. DIE has no remote mode. *(Decided)*
4. **Two layers: cells and pixels.** Text renders onto a monospace cell grid. Images and pixel buffers are composited with it. Core has no widget toolkit and no drawing API. *(Decided)*
5. **Rule of two.** A feature moves into core only when at least two extensions need it and it can't be built from existing primitives. *(Decided)*
6. **Restart must be cheap and safe.** Rebuild-and-restart is how you extend DIE, so restoring the session is a core feature. A crash, including a freshly built binary that panics, must not cost you terminals or unsaved text (§5.9). *(Decided)*
7. **Readable code.** The budgets below are measured as code lines, excluding tests:
   - `die-core` ≤ about 14k
   - `die-render`: its neutral modules plus any one platform's modules ≤ about 5k per build, and die-render's total ≤ about 7k. So the weekend read is the shared code plus your platform, and a new platform has to fit inside the total. Estimates: macOS 4.1–5.25k, Linux 3.85–5.05k, 5.35–7.15k in total ([records/platforms.md](records/platforms.md) §5.7).
   - `die-term` ≤ about 4k of glue, plus about 1k for the helper process (S2's prototype: 1.05k), plus its tests and the engine updater
   - `die-vim` ≤ about 5k (M0 estimate: 3.6–4.6k). It has no cut list, since §10 is its scope; an overrun means simplifying at the next review.

   Code lines are counted after rustfmt, without blank lines, comments, `tests/` or `#[cfg(test)]` items, by `cargo xtask budget` under the pinned toolchain. Each module has a ratchet in `budget.toml`, and an overrun goes through a named cut list. If you can't read core in a weekend, it's too big. *(Decided 2026-10-19. The previous 10k goal for core plus renderer was a goal, not a measurement; two independent estimates put them at about 16k.)*
8. **Support faithful ports.** The core must let you rebuild your existing packages (seek, scrib, manifold, orchid, termin, delta) later as DIE extensions that work the same way they do in Emacs, using text buffers, modes and keymaps. The graphical features are for new extensions you couldn't build in Emacs. Building either kind isn't part of this project. *(Decided)*

## 4. Decision log

| ID | Decision | Status | Rationale |
|---|---|---|---|
| D1 | Rust only. Extensions and config are crates; changes need a rebuild and restart. | Decided (amended 2026-10-19) | Smallest core, fully typed. Cargo is the package manager. FFI is confined to `die-render`'s platform modules (objc2 on macOS; the Wayland and X11 glue on Linux), `die-term`'s engine boundary, and die-core's `sys` module. |
| D2 | DIE always runs locally. Remote work is SSH inside a terminal buffer. No TRAMP. | Decided | Your call. Accepted consequence: you edit remote files with whatever editor runs on the remote host, inside the terminal. |
| D3 | Applications are extensions, never core. | Decided | Matches how your packages relate to Emacs today. |
| D4 | Vim depth: the core grammar (§10), matching your Evil setup. Where Evil and textbook Vim differ, Evil wins. The one deliberate change is Rust regex syntax instead of Emacs's. Exceptions: Evil bugs, gaps and artifacts of Emacs buffers, where die-vim follows Vim. Each is listed in [records/s5-evil-oracle.md](records/s5-evil-oracle.md) §4 and gets a hand-written fixture. | Decided (2026-10-19) | The aim was always to cover a vanilla Evil setup like yours. The previous §10 described textbook Vim in places (visual-line `j`/`k`, a separate clipboard register) and would have caused friction on day one. |
| D5 | DIE is a native app on macOS and Linux. There is no TUI frontend. | Decided (amended 2026-10-19) | With SSH inside DIE, the main reason to run inside a terminal is gone. Running DIE inside another terminal would also force kitty graphics from hosted programs through two layers of terminal, the same problem tmux has. |
| D6 | Display: a monospace cell grid, plus pixels (image placements and pixel buffers) composited with it (§5.1). | Decided | Same model as kitty. One image path serves both terminal graphics and pixel buffers. |
| D7 | Editor state is single-threaded. Background work runs on std threads that post messages to the main loop. Pixel buffers can be written from producer threads (§5.2). | Decided | Emacs-like mental model with no locks, while video and plots don't block editing. |
| D8 | Surfaces paint into a DIE-owned cell type. No ratatui in core. | Decided (2026-10-19) | ratatui's cell has a single underline bit, no link id, no placements and no wide-character continuation. An extension can still use ratatui internally and convert its output. |
| D9 | Core has modal states, keymap layers and a prefix-argument slot (count and register). The Vim grammar is built into DIE, implemented in a `die-vim` crate inside the DIE workspace, and die-vim parses all Vim syntax. | Decided (amended 2026-10-19) | Keeps Vim policy out of the core crate while Vim stays part of DIE itself. |
| D10 | Kitty-level terminal buffers are built into DIE, in a `die-term` crate on top of Ghostty's libghostty-vt engine. The engine is linked as a prebuilt static library that Ghostty's CI builds and signs: one signed engine archive per supported target. `cargo xtask ghostty bump` verifies each one, cuts it down, and commits it with a lock file. Where a signed Linux archive comes from is open: Ghostty doesn't publish one today, and you declined both asking upstream from your account and DIE's CI building one (2026-10-19). It's revisited before M2, which is when Linux terminals need it. On a target with no signed archive, die-term-sys links nothing, and the `die` facade and that target's tests leave die-term out. DIE still builds and cross-checks there, without terminals. Building DIE needs only cargo and the platform's standard toolchain (D16). DIE owns no engine code: `die-term` is glue (about 3.2k lines) that shows engine state through core's renderer and forwards keys and mouse. The Rust API is libghostty-rs at a pinned rev, never forked. The feature target is ghostel's (§6). | Decided (2026-10-19) | It's the same engine and the same delivery model as ghostel on macOS, which uses a prebuilt binary with Zig only in upstream CI. The C API covers kitty keyboard, kitty graphics, mouse encoding and OSC callbacks, so DIE doesn't write a terminal. Cost: DIE can't patch the engine itself; fixes wait for a signed upstream build. The full record, with the parity checklist and fallbacks, is [records/die-term-engine.md](records/die-term-engine.md). |
| D11 | Pixel buffers are modeled on the Emacs 32 canvas. Core has no drawing API; extensions bring their own rasterizer, decoder or renderer. | Decided | Raw pixels are the smallest primitive that supports plots, PDFs, video and games. A shared drawing crate can come later under the rule of two. |
| D12 | macOS (aarch64) is supported. Linux (x86_64 and aarch64; Wayland and X11; glibc 2.34+) ships as a preview at M2's exit, and becomes supported when SL-desktop and SL-hw pass. Its terminals need a signed engine archive (D10). Other Unixes are compile-checked only, with no terminals. Windows is out. | Decided (2026-10-19); replaces macOS only | Your call: "I want to at least maintain support across unix platforms such as linux etc." and "if windows would be a considerable deviation then we can drop it." Windows would be one: Ghostty signs no Windows engine, ConPTY replaces PTYs, a pseudoconsole dies with the process that created it, so D20's holder would need another design, and you have no Windows machine ([records/platforms.md](records/platforms.md) §3). The timing is your answer to question 11. A preview builds under D16 and passes every headless and software-rendered test, and makes no latency claim ([records/platforms.md](records/platforms.md) §5.1). |
| D13 | Renderer stack, in a `die-render` crate: winit 0.30.13 (pinned) on every platform. macOS: Metal through objc2-metal and Core Text, as S1 confirmed. Linux: wgpu 30 (Vulkan, GL fallback) and fontations (skrifa, swash, fontique), pending S6. Producers never call wgpu's `write_texture`. | Decided (amended 2026-10-19); macOS confirmed by S1 (2026-10-19); Linux pending S6 | The 5.9 ms main-thread stall (p99) once blamed on wgpu came from the 2026-10-18 review probe, not S1. It's `write_texture` holding a queue lock while it copies a producer's frame. S1's staging ring on buffer uploads avoids it: main-thread p99 0.15–0.26 ms on wgpu's Metal backend, without presenting to a surface (S6a measures that; [records/platforms.md](records/platforms.md) §2.1). Fallbacks: wgpu with buffer uploads only, or AppKit directly instead of winit. S1 confirmed it on your machine: a full redraw of 159×49 cells takes 0.65 ms of main-thread CPU and 0.70 ms of GPU time (p99), a 60 Hz full-window pixel buffer showed all 3,600 frames with no main-thread pixel copies, and idle draws nothing. Typing latency is set by the macOS compositor, not by this stack. The full record is [records/s1-renderer.md](records/s1-renderer.md). |
| D14 | This project builds DIE and nothing on top of it. Extensions, including ports of your Emacs packages, are built later and separately. | Decided | Your call. The contract is proven with test-only fixtures instead (extension-contract §16). |
| D15 | The Vim grammar (`die-vim`) and kitty-level terminals (`die-term`) are part of DIE itself and are built in this project. They always ship with DIE, except that on a target with no signed engine archive (Linux today, D10), DIE ships without die-term until one exists. | Decided (amended 2026-10-19) | Your call. They're separate crates only to keep Vim and terminal logic out of the core crate. Internally they use the same public API as any extension, which also exercises the contract. The exception is for Linux's preview: macOS is the daily driver, so Linux ships without terminals rather than waiting, and gets them at the first `ghostty bump` that has an archive. An editor-only Windows build wasn't offered: you wanted Windows only if it wasn't a considerable deviation, and even without terminals it needed a D16 exception ([records/platforms.md](records/platforms.md) §3). |
| D16 | Toolchain: building DIE needs only cargo and the platform's standard C toolchain, used only as the linker driver: the Xcode Command Line Tools on macOS; gcc or clang with the libc development files on Linux. No `-dev` packages: X11, Wayland, xkbcommon, fontconfig and Vulkan load at run time. Zig and other toolchains run only in CI. A prebuilt binary is allowed when the upstream project's CI signed it, it's tied to a commit whose tests passed, and it's checked by checksum. C compiled through `cc` is avoided and flagged when it appears. A C-building crate may sit in a build graph only on a named allowlist with a reason, and the stand-in build proves it compiles nothing. Dev-only tools that `cargo build` and `cargo test` don't need are fine. A clean-environment build check enforces this. | Decided (2026-10-19) | Your call: "I definitely don't want die-term to bring a 2nd toolchain." ghostel already works this way. |
| D17 | No owned forks. Third-party code is a pinned dependency, never vendored or forked into DIE. Two exceptions, both small and verified on every update: the signed engine archives, one per supported target (D16), and `die-term-sys`, which replaces libghostty-rs's sys crate with about 90 adapted lines plus generated bindings. Ghostty signs no Linux archive today, and where one comes from is open until before M2, when Linux terminals need it (D10). | Decided (2026-10-19) | Your call. The value of DIE is a codebase you can read. |
| D18 | Terminal input model. ESC goes to the program. The DIE layer is Cmd on macOS and Ctrl+Shift by default on Linux, set in config. The leave chord is the layer's Esc: Cmd-Esc on macOS, Ctrl+Shift+Esc on Linux. It always leaves the terminal and is never sent to programs. The whole DIE layer belongs to DIE: its V pastes, its C copies, and its Q quits (Cmd-V, Cmd-C and Cmd-Q on macOS), confirming if processes are running. Normal state in a terminal is read-only copy state that follows new output while you're at the bottom, and `i`/`a` return to the terminal. A per-buffer toggle gives vterm-style ESC instead. | Decided (2026-10-19) | ESC can't mean both "leave" and "send to the program": Claude Code uses ESC to interrupt and runs on the main screen, so ghostel's alternate-screen trick fails for it. |
| D19 | Copy state is die-vim's grammar running over a read-only snapshot of the surface (§5.5). Core has no second motion engine. | Decided (2026-10-19) | Keeps Vim in die-vim (D9). The alternative was about 600–900 lines duplicating die-vim inside core. |
| D20 | Restart: everything survives, crashes included. A holder process keeps PTYs open. Each terminal's engine runs in a helper process, DIE's own binary in a second mode (`die --term-helper`), which takes its PTY from the holder and outlives DIE. The session saves unsaved text, scratch buffers, registers, macros and histories (§5.9). Exit status is watched with kqueue on macOS. On Linux, the holder keeps a pidfd for each shell, and the status after a restart is known on kernel 6.15+ only. The holder gets its own systemd user scope if SL-desktop shows it's needed. | Decided (2026-10-19); holder confirmed by S3, helper by S2 | Rebuild-and-restart is your extension loop. A bad build mustn't cost you live Claude Code or SSH sessions. S2 measured the helper within 0.5 ms of a thread inside DIE, with images crossing as shared memory ([records/s2-term-engine.md](records/s2-term-engine.md)). |
| D21 | v1 is local first. M2 is proven on your real day: Claude Code and codex sessions, and TUIs like btop and ncdu, with one SSH check. | Decided (2026-10-19) | Your recent shell history shows local work, not SSH. |
| D22 | License: MIT OR Apache-2.0. | Decided (2026-10-19) | Compatible with Ghostty (MIT), matches libghostty-rs (MIT OR Apache-2.0), and doesn't bind anyone building on DIE. ghostel (GPL-3.0) is a behavior reference only; none of its code is copied. |
| D23 | The milestones are restructured (§12): spikes first, the API before die-vim, restart proven in M0 and M2, and the terminal (M2) before contract completeness (M1). | Decided (2026-10-19) | The original order couldn't be executed: die-vim came before its API, M1's exit test needed M2 and M3, and restart was proven last. |

## 5. Core model

### 5.1 Display: cells and pixels

Every window's content is a **monospace cell grid**. Text surfaces and terminals render into cells: characters with foreground, background, attributes, underline styles (single, double, curly, dotted and dashed, with a color), link ids and wide-character continuation.

**Pixels** sit on the same grid as **placements**, each anchored to a rectangle of cells. A placement has a source rectangle, a sub-cell pixel offset and a z-order, which can be negative to sit below text, as kitty's placements do. A placement shows either:

- an **image**, which is immutable pixels (a decoded PNG, or an image a program sent through kitty graphics), or
- a **pixel buffer**, which is mutable pixels an extension owns (§5.2).

A pixel buffer can also fill an entire window, which is the "pixel buffer as a buffer" case. A video player, PDF reader or game is a buffer whose surface is one full-window pixel buffer.

This is kitty's display model. It means three things that look different share one implementation:

- `die-term` showing kitty graphics from a program in the terminal
- a markdown view showing an inline image
- an extension animating a plot

Later, maybe: **scaled text runs** that span several cells at a larger size, in the style of kitty's text-sizing protocol. They would give a markdown view large headings without proportional fonts (§13).

### 5.2 Pixel buffers (modeled on the Emacs canvas)

| Emacs 32 canvas | DIE pixel buffer |
|---|---|
| An image type, `canvas`, holding an ARGB32 buffer and identified by `:id` | A handle to an RGBA buffer, owned by an extension and sized in physical pixels |
| Shown through a `display` property on text or an overlay | Shown as a whole surface, or placed inline through a decoration on a text surface (§8) |
| Lisp writes pixels with `aset`; C modules get the raw buffer through `canvas_data` | The extension writes the buffer directly, from the main thread or a producer thread |
| `canvas-refresh` redraws only the glyphs that cover the canvas | `present(damage)` uploads only the changed rectangle, with no re-layout or re-rasterization of anything else |
| Mouse input through `track-mouse` and `posn-object-x-y` | Pointer events arrive with coordinates in the buffer's own pixel space |

Additional rules:

- **The editor never waits.** The producer owns one persistent CPU buffer. `present(damage)` copies the damaged rectangle into a small ring of shared GPU buffers on the producer's own thread. The main thread only records a GPU copy, so it never copies pixels and never waits on the producer. It never takes a lock the producer can hold, either: it only tries the hand-off, and if the producer has it, the frame shows on the next wakeup.
- **Physical pixels, known scale.** On resize, core tells the extension the buffer's physical size and the display scale factor. Emacs only exposes logical pixels, which is why ghostel needs `ghostel-cell-pixel-scale`. DIE avoids that problem from the start.
- **No drawing API in core.** Extensions bring tiny-skia, vello_cpu, a PDF library or a video decoder. If two extensions end up wrapping the same rasterizer, that wrapper becomes a shared `die-draw` crate.
- **Keyboard** goes through normal modal keymaps like any other surface. Pixel buffers have no special key handling.

### 5.3 Workspace, buffers, windows

- **Workspace**: the single global state. It holds the buffer registry, window tree, keymaps, command registry, extension registry, theme and event loop.
- **Buffer**: an id, a unique name, a surface, a list of active modes (text buffers only, contract C17), a buffer-local keymap, a `directory` (like `default-directory`), lifecycle hooks (created, shown, hidden, killed), and an optional backing file or process. A buffer doesn't have to contain text.
- **Window**: a rectangle in a split tree that shows one buffer. Supports split, resize, close and focus movement. No frames or tabs in v1.
- **Status line and command line**: one status line per window, and one shared command line for `:` commands and prompts. Prompts show a vertical list of about 20 candidates, like your vertico, with flex matching (as your `completion-styles '(flex)` does). `find-file` descends into a directory inside the prompt. Fancier pickers are extensions.

### 5.4 Surfaces

A surface is whatever a buffer displays. Core provides one built-in surface and one trait.

**Text surface (core).** Rope storage, cursors and selections, undo, search, and decorations (§8). Files open into this.

**Surface trait (everything else).** The canonical definition is [extension-contract.md](extension-contract.md) §10. In outline:

```
Surface
  kind()               "term": identity for keymaps, restore and describe
  render(area, frame, info)  cells + placements (images, pixel buffers)
  event(event, ctx)    input that keymaps didn't take; in passthrough, every key outside the DIE layer
  initial_state()      normal by default; passthrough for terminals
  text()               optional: full text content, for copy state and search beyond the viewport
  scroll_to / reveal   optional: let copy state and search move the view
  save()               optional: survive restarts (§5.9)
```

The six surface kinds from the first conversation (text, terminal, list, canvas, web, custom) reduce to this. Custom surfaces are for terminals, plot and PDF viewers, and other graphical extensions. Pickers, browsers and orchid's conversation buffer stay text buffers, as they are in Emacs (scope guard 8). Web views are out of scope.

### 5.5 Copy state on every surface

What makes Emacs powerful is that everything is text, so search, motions and yank work the same in dired, magit and compilation buffers. A model with many kinds of surface usually loses that. DIE gets it back through the cell grid: whatever a surface renders into cells is a grid of characters.

A **copy state** (like tmux's copy-mode) puts a cursor on the content of any window. Entering it, core takes a read-only text snapshot of the surface: its `text()` if it has one (for a terminal, the screen plus scrollback, which arrives asynchronously from the terminal's engine), otherwise its rendered cells. die-vim's normal and visual grammar then runs on that snapshot, so you get the same motions, `/` search, visual selection and yank as in a text buffer (D19). Core has no motion code of its own. Pixel content is invisible to copy state unless the surface includes it in `text()`.

In a terminal, leaving passthrough (the DIE layer's Esc) enters copy state. While you're at the bottom, core refreshes the snapshot as output arrives, so it follows new output; once you move away, it stays frozen until you return to the bottom (D18).

### 5.6 Modal input and keymaps

- **States:** normal, insert, replace, visual (char, line, block), operator-pending, command line, copy, and **passthrough**. In passthrough, keys go straight to the surface (used by terminals).
- **Leaving passthrough.** The DIE layer's Esc (Cmd-Esc on macOS, Ctrl+Shift+Esc on Linux) always leaves and is never sent to programs. The DIE layer belongs to DIE in every buffer (D18).
- **Lookup order** for a key is defined in [extension-contract.md](extension-contract.md) §7: the range keymap at the cursor, buffer-local, modes, surface kind, global, then the surface's `event()`.
- **Prefix keys and a leader key.** Yours is `\`.
- **Full key events.** Core receives key press, repeat and release, physical and logical keys, the unshifted codepoint, and modifiers from the OS. Keymaps match press and repeat. Release events reach only surfaces in passthrough, where `die-term` needs them for the kitty keyboard protocol.
- **Alt (Option on macOS) acts as Alt.** Core's own bindings stay off Alt chords, which AeroSpace takes.
- **No IME work in v1.** Your input is the Australian layout plus the Character Viewer. Dead keys arrive as committed text; on Linux they come through xkb compose.
- **Pointer input:** clicks (with a click count), drags and scrolling (with precise trackpad deltas) go to the surface under the pointer. Over a pixel buffer, they carry coordinates in the buffer's pixel space.

### 5.7 Commands and introspection

Every action is a named command with a docstring, registered in a single registry. Keymaps bind to command names. A command receives a context: the workspace, the current buffer, and the prefix argument (count and register). Core only stores the prefix argument; die-vim parses Vim's syntax for it (D9). Equivalents of `describe-key` and `describe-command` are in core, because self-documentation is a big part of why Emacs is pleasant to extend.

### 5.8 Events, jobs and processes

- **Hooks:** buffer lifecycle, after an edit, before and after a save, mode on and off, focus, idle. Hooks are queued (contract C3), so there's no before-edit hook; modes catch up with `edits_since` (contract C15). A save runs its write as a queued step after the before-save hooks.
- **Timers.**
- **Process API:** spawn with piped stdio (output can stream into a buffer or surface), or spawn on a PTY. Every child, shells included, is started with `posix_spawn` and close-on-exec by default, on macOS and glibc, so no child can hold another terminal open.
- Background threads post messages to the main loop (D7).
- **Terminal output is parsed in each terminal's helper process** (D20). DIE's main thread receives finished frames and never waits on a helper.
- **Environment.** PATH additions and an env file are declared in your config, mirroring `init.el`, so DIE.app started from the Dock finds cargo. The same file covers DIE started from a `.desktop` file on Linux. Secrets are never logged.

### 5.9 Persistence and restart

Rebuild-and-restart is how you extend DIE, so a restart should feel like nothing happened, and a crash shouldn't cost you anything that matters (D20).

- **Session file.** JSON with a version field. It records:
  - each buffer, with its file and its cursor and scroll position in each window
  - the window layout
  - unsaved text and scratch buffers
  - registers and recorded macros
  - search and command histories
  - any data surfaces and modes return from `save()`

  Each buffer's data is stored separately, so one that fails to load drops only that buffer; a terminal with a live PTY is reattached fresh instead. Restore is on by default. Undo history comes later.
- **Written continuously.** Core writes the session about 1 s after the last change, not only on restart, so a crash loses at most the last few seconds. It lives where `sys::paths` says: `~/Library/Application Support/DIE/` on macOS, and `$XDG_STATE_HOME/die` (by default `~/.local/state/die`) on Linux. It's written through a temporary file and a rename, and the previous copy is kept.
- **Restart.** The `restart` command saves the session, starts the newly built binary, and exits. Quitting (the DIE layer's Q, Cmd-Q on macOS) is different: after its confirmation, core tells the holder to close the terminals and exit. `restart` copies the fresh build into `~/Applications/DIE.app` on macOS, or into `~/.local/lib/die/die` with `die.desktop` on Linux, and renames it into place, so a build that's still being written is never launched (S3's advice). The old instance saves the session, releases its lock and waits for the new window's first frame, reported over a Unix socket. If none comes within 2 s, it takes the lock back and keeps running, so a build that panics at startup costs nothing. Your startup hook is told whether this is a cold start or a restore, so a restart doesn't open another terminal. On Linux, the clipboard goes away with the app that set it, so the restored DIE takes back clipboard ownership if it owned the clipboard at exit.
- **Terminals survive crashes.** A holder process, which is DIE's own binary in a second mode (`die --fd-store`), keeps every PTY open. PTYs are handed to it over a Unix socket when they're created. Spike S3 measured it on your machine: shells survived 20 restarts and 5 `kill -9`s of DIE, including DIE launched as a `.app`, with the same PIDs. Nothing was lost but a line or two a killed DIE had already read ([records/s3-fd-store.md](records/s3-fd-store.md)).
  - A new DIE reattaches in under a millisecond per terminal. die-term then makes full-screen programs redraw: it shrinks the terminal by one row, waits for the program to start repainting (at most 50 ms), and restores it. The kernel drops a same-size resize, and some programs ignore a resize that's immediately undone.
  - While no DIE is running, each terminal's helper keeps reading, so programs don't pause and their output lands in scrollback.
  - Exit codes survive a restart. On macOS, DIE watches each shell with kqueue, which works even though the shell's parent is gone. On Linux, the holder keeps a pidfd for each shell, opened while DIE is still its parent. After a restart the status is known on kernel 6.15+, and shows as unknown on older kernels.
  - The holder never closes a live terminal by itself, so shells left by a crash you never relaunch after live until logout, as with tmux. It exits a few minutes after it holds nothing.
  - If the holder dies while DIE is down, the helpers' copies of the PTYs keep the shells alive. The holder stays tiny anyway, and a running DIE moves its terminals into a fresh holder if the old one dies.
- **Each terminal's engine runs in a helper process**, DIE's own binary in another mode (`die --term-helper`), one per terminal. The helper takes the terminal's PTY from the holder, parses its output, and sends DIE finished frames over a Unix socket. Images travel as shared memory, never through the socket.
  - **DIE crashes or restarts:** helpers keep running. The new DIE reaches each helper through the holder and gets the screen, scrollback and images back as they were, with no snapshot.
  - **A helper crashes:** that terminal loses its screen and scrollback, nothing else. The holder's copy keeps the shell alive, a new helper takes the PTY, and die-term makes full-screen programs redraw as above.
  - **Old helpers, new builds:** a helper started by an older build serves every newer DIE with the same protocol major version. An incompatible pair is refused cleanly, and the terminal stays alive and detached until you replace its helper, which you do by hand because it loses scrollback.
  - Spike S2 measured all of this ([records/s2-term-engine.md](records/s2-term-engine.md)).
- **Later, maybe:** Rust hot-patching (for example Dioxus's subsecond) would get closer to live eval. Not in scope.

### 5.10 Extension contract

- An extension is a crate implementing an `Extension` trait. At startup it registers commands, keymaps, modes, hooks, surface constructors and faces.
- Your personal config is a binary crate, as in xmonad. Its `main` lists the extensions, sets options and binds keys, then hands control to core.
- Extensions use only core's public API. If an extension has to reach into core internals, core is missing a primitive (scope guard 5).
- The full contract (lifecycle, `Ctx`, commands, keymaps, modes, the text API, die-vim's primitives, async, persistence, errors and testing) is in [extension-contract.md](extension-contract.md).

### 5.11 Rendering

- **One native window.** Borderless with no bars, like your Emacs frame. AeroSpace leaves a borderless window floating by default, so your `aerospace.toml` gets an `on-window-detected` rule that tiles DIE. On Linux, a tiling rule matches DIE by its app_id.
- **Font: Ioskeley Mono Term.** DIE loads it from the font files, so it doesn't need installing. Your installed build draws 1,282 single-width characters two cells wide, `—`, `…` and the arrows among them; Term draws 23, and DIE squeezes those to fit. Ligatures stay off in v1, matching what your Emacs shows.
- **One monospace font with a configurable fallback chain per Unicode range.** Cell metrics come from the primary font.
- **Cell-joining glyphs.** The renderer draws box-drawing, block, braille and powerline glyphs itself so they join across cells, as kitty and Ghostty do. A per-range override hands a range back to the font. Braille stays in Ioskeley Mono, as your `init.el` pins it.
- **Physical pixels internally.** Display scaling is handled once, in the renderer.
- **GPU path (D13).** Metal and a Core Text glyph atlas on macOS; wgpu and a fontations atlas on Linux. Images and pixel buffers are textures. Rebuild only the rows that changed; the GPU recomposites the whole frame, which costs under 1 ms on your Mac. Your panel runs at 120 Hz, so the frame budget is 8.3 ms. When nothing changes, no frames are drawn.
- **Latency.** These are macOS's figures. DIE handles a key and commits its frame within about 2.5 ms (p99). The macOS compositor adds the rest: about 11 ms (p50) when DIE's window covers the whole display, about 23 ms when it's one tile among others. That tile figure is the platform's floor, and it's accepted; covering the full display is the option when you want the faster path. While a 60 Hz pixel buffer animates, a keystroke can wait one more refresh (8.3 ms), because the panel refreshes at most every 8.3 ms. Text and pixels stay on one layer in v1, and M3 revisits that. On Linux, only DIE's share (key event to present call) can be measured, so Linux states that alone.
- **Frame scheduling.** Render right after input, in the same run-loop turn. Draw continuously only while something animates. Don't draw while the window is occluded. Before drawing, match the surface to the window's current size.
  - macOS: run a display link only while something animates. During live resize, present inside the Core Animation transaction, one drawable per transaction, and flush it.
  - Linux: present with Mailbox where it's offered, Fifo otherwise. On Wayland, present only after a frame callback, and treat no frame callbacks as occlusion. On X11, use winit's occlusion event. A timeout getting the next texture skips the frame.
- **Scene boundary.** Core builds a scene (cells, placements, cursor), and only `die-render` touches the window and GPU. A scene can then be snapshot-tested without opening a window.
- **Clipboard.** The system clipboard: NSPasteboard on macOS, the Wayland or X11 clipboard on Linux. die-vim's unnamed register syncs with it (§10). `die-term` bridges OSC 52 writes to it; reads from remote programs are denied.

## 6. Terminal buffers (`die-term`)

`die-term` is built into DIE and ships with it on every target that has a signed engine archive (D10, D15). Internally it's a separate crate that uses only core's public API: the PTY, the cell grid, placements, passthrough and full key events. The terminal is Ghostty's engine, libghostty-vt, linked as a prebuilt library (D10). `die-term` shows the engine's state through DIE's renderer and forwards keys and mouse, so a terminal buffer feels like using the terminal directly. ghostel's feature list is the checklist, and kitty is the target wherever the engine allows:

- True color and all underline styles. Box-drawing, block and powerline glyphs are drawn by DIE's renderer so they join across cells.
- **Kitty keyboard protocol**, including key release and repeat
- **Kitty graphics protocol:** direct placements (timg, `kitten icat`, and yazi under DIE's default identity) and Unicode-placeholder placements (`icat --unicode-placeholder`, and yazi when it detects Ghostty or kitty), shown through core's placements with transparency and z-order
- **Mouse:** every tracking mode and encoding, including pixel coordinates. Shift-click selects even when an app tracks the mouse.
- Synchronized output
- Hyperlinks (OSC 8), plus plain URLs and `file:line`
- Clipboard through OSC 52. Writes are on and reads are off by default. A yank in vim on a remote host over SSH lands on the system clipboard.
- Desktop notifications (OSC 9 and 777), through `notify-send` on Linux, and progress reports
- Shell integration that works with macOS's `/bin/bash` 3.2: prompt navigation and directory tracking. bash starts as `bash --rcfile die.bash -i`; `die.bash` follows bash's login order: `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile`, then `~/.bashrc` behind a guard variable. Then it adds DIE's integration. (ghostel's method doesn't work on bash 3.2, which is why your ghostel has no shell integration today.)
- Window and cell size queries (XTWINOPS) answered in physical pixels
- Copy state over the screen and scrollback (§5.5). Editing the shell's input line from normal state, as evil-ghostel does, can come later with the vterm-style toggle if you miss it; it isn't part of M2.
- Terminals survive DIE restarts and crashes (§5.9)
- **SSH-friendly `TERM`.** Remote hosts often lack terminfo for newer terminals. So `die-term` defaults to `TERM=xterm-256color` and `COLORTERM=truecolor`, and identifies itself as DIE, not Ghostty, in `TERM_PROGRAM` and XTVERSION. Apps detect the extra features through terminal queries. A generic identity also keeps yazi on kitty's direct image path. Presenting as `xterm-ghostty` is an option for local use.

**Defaults:** 10,000 lines of scrollback per terminal, as in vterm, capped at 64 MB. Images capped at 64 MB per screen. Alt (Option on macOS) acts as Meta. Programs can't read local files through the image protocol.

**die-term's API** lets your config open a named terminal with a command, directory and environment, and get a callback with the exit status and final text. By default the buffer closes when the program exits. termin becomes a short command in your config, and fzf can run in a split.

**Updating the engine.** `cargo xtask ghostty bump` takes Ghostty's latest signed build, ties it to a commit whose tests passed, regenerates the bindings, runs die-term's tests, and commits the result. Expect about once a month, sooner for crash and security fixes, and never more than three months behind. Details are in [records/die-term-engine.md](records/die-term-engine.md) §5.

**Known gaps, until Ghostty adds them:** looping image animation (an animated image shows its first frame), kitty notifications (OSC 99), text sizing (OSC 66) and the title stack. Prompt exit codes and a shell-to-editor command channel wait on libghostty-rs exposing more engine callbacks. DIE can't patch the engine itself.

## 7. What lives where

**Core (`die-core`):**

- Event loop, workspace, buffers, window splits
- Text surface: rope, undo, search, decorations, layout (soft wrap, centered width, line numbers)
- Modal states, keymap layers, full key events, the prefix-argument slot, command registry, describe commands
- Extension machinery (`Extension`, registry, `Ctx`) and the headless test harness
- Status line, and a command line with completion
- Faces and themes
- Opening and saving files, auto-revert
- Processes, PTYs, the terminal holder, timers, hooks
- Copy-state snapshots
- Session save and restore, and restart
- The scene (cells, placements, cursor) that `die-render` draws

**Built into DIE (separate crates in the DIE workspace, always included):**

- `die-render`: the window, the GPU and text backends (Metal and Core Text on macOS; wgpu and fontations on Linux), the system clipboard and the rest of the platform layer (D13)
- `die-vim`: the Vim grammar (§10), which also drives copy state
- `die-term`: terminal buffers (§6), on every target with a signed engine archive (D15)

**Extensions (yours to build later; not part of this project):**

- Picker (seek-style), fuzzy finder (fzf-style)
- Syntax highlighting
- Folding (manifold)
- Markdown view
- Pixel-buffer applications: plotting, image or PDF viewer, video
- Notes (scrib), AI client (orchid)
- Diff view (delta), file tree (dired-sidebar)
- Key-hint popup (which-key-style)
- Your themes (mono-dark, dark-coffee), stored as config data

## 8. Text surface requirements (from your config)

| Your Emacs setup | DIE requirement |
|---|---|
| `global-visual-line-mode`; `line-move-visual t`, which Evil overrides for `j`/`k` | Soft wrap on by default. `j`/`k` move by logical line and `gj`/`gk` by visual line, as in your Evil today (§10) |
| `visual-fill-column`, centered at 80 columns | Optional maximum text width, centered in the window. It counts text cells only, so you get 80 columns of code with or without line numbers. `visual-fill-column` counts the gutter too, and this deliberately differs. Your `.rs` files show no line numbers in Emacs, so rows still break where Emacs breaks them |
| Relative/visual line numbers in programming modes | Line-number gutter (absolute, relative or visual), set per mode. Relative counts display lines, as `j` does, so a fold counts once. Emacs's counts hidden lines, and this deliberately differs. Visual matches Emacs. The rules are in [extension-contract.md](extension-contract.md) §9 |
| manifold: `{{{ }}}` folds, a `▼` marker, `j`/`k` skip folded text | Decorations can hide a range and show replacement text; motions skip hidden ranges |
| `search-invisible t` | Search matches inside hidden ranges and reveals them |
| Two custom themes | Named faces; a theme maps faces to styles. Status lines have no box: mono-dark's 1 px `:box` on its mode lines is the one attribute DIE's styles lack, it would take renderer work for one face, and dividers separate the windows |
| Evil's cursors; `blink-cursor-mode` is on | Each state's cursor shape, with replace state's underline. A cursor that isn't taking input is hollow, and from K9 that includes every cursor while DIE's window doesn't have the keyboard focus. A cursor the color of its cell's background takes the cell's foreground instead, so it doesn't vanish on mono-dark's current search match. Cursors don't blink in M0, a deliberate deviation: Emacs stops blinking after 10 blinks, so the difference is small. With die-term (M2), a cursor can carry a terminal's blinking style, and die-render owns the timer |
| `global-auto-revert-mode`; `cargo fmt` and agents rewrite open files | Reload unmodified buffers when the file changes on disk. `:w` checks for on-disk changes first |
| Evil copies every yank and delete to the macOS clipboard | die-vim's unnamed register syncs with the clipboard both ways (§10) |
| Files reached through symlinks, such as `claude-session` | Saves write through symlinks and keep file permissions |
| Backups and auto-save disabled | Don't build them |
| `indent-tabs-mode nil` | Indent with spaces by default |
| orchid's collapsible sections and tool-result stubs | The same hide-and-replace decorations, available to any extension |

Decorations in general are:

- faces over ranges
- hidden ranges, with replacement text **or a placement** (an image or pixel buffer occupying a rectangle of cells)
- virtual text before a position
- read-only ranges, which block your edits but not edits from extension code
- range keymaps, for buttons and toggles
- tags, which attach an extension value to a range

Decorations are the equivalent of Emacs text properties, overlays and the `display` property. A placement replacement is how a canvas is shown inline in Emacs, and it lets a notes or markdown buffer show rendered math, diagrams or images while staying editable.

In v1, decoration providers (computed at render time, like font-lock) return faces only. Hidden ranges, virtual text and placements are stored decorations, so rendering and motions share one layout.

Alongside decorations, text buffers provide markers (positions that move with edits) and silent edits that aren't recorded in undo. orchid's chat needs both. Details are in [extension-contract.md](extension-contract.md) §9.

## 9. Pixel buffers: what they're for

None of your existing packages need pixel buffers (scope guard 8). Pixel buffers are for things you don't have today because Emacs can't support them. These are future extensions. They aren't core features and aren't built here; they're listed to check that the primitive is big enough:

- Plots and charts from data in a buffer
- Image and PDF viewers
- Rendered math and diagrams inline in notes
- Graph views (for example, links between notes)
- A video player, as in the Emacs canvas demos
- Games and toys

## 10. Vim scope (core grammar)

**The spec is your Evil setup (D4).** Where it differs from textbook Vim, Evil wins. The one deliberate change is Rust regex syntax instead of Emacs's. Fidelity is measured against Evil itself, running in batch Emacs (the Evil oracle, §12).

**In:**

- Counts; operators `d c y > < g~ gu gU` (shifts of 4 columns, rounded to a multiple of 4, as in Evil); dot-repeat. `J` puts one space after a period. Counts on an operator and on its motion multiply, so `2d3w` takes six words. A key after an operator that isn't a motion, a text object or the operator's own key cancels it and does nothing else. Evil runs most such keys as motions, so `dA` deletes to the end of the line there; die-vim deliberately doesn't. `<` moves the cursor to the first line it shifts and `>` leaves it on its line, as in Evil. `J` on the last line has nothing to join, and the final newline stays; Evil's takes it. A count on `J` or `p` that would make more than 1 MB fails with "Count too large", where your Evil would freeze. Joining two lines or pasting once goes ahead at any size. An operator that fails, on read-only text or a read-only register, leaves the cursor at the start of its range, as in Evil, except `y` and `>`, which leave it where it was.
- Motions `h j k l w b e W B E 0 ^ $ gg G f F t T ; , % { } H M L gj gk`. `j`/`k` move by logical line and `gj`/`gk` by visual line. `gg`, `G`, `:N`, `N%`, `>>`, `<<` and linewise `d` keep the cursor's column, as in Evil: a linewise `d` puts the cursor in the column it started in, or at the end of the line after `$`. Vim's `startofline` would move it to the first non-blank. `{` and `}` also stop at lines that hold only spaces. A motion that fails forgets the column `j` and `k` are keeping, as in your Evil, except `h`, `l`, `j` and `k`, and `f` or `t` going forward from a line's last character. With no line to go to, `j` and `k` try that column on their own line and fail only if that wouldn't move the cursor, so `$j` on the last line doesn't fail. Motions never move the cursor onto the empty line after a file's final newline. Evil's do; die-vim deliberately doesn't, and does what your Evil does when the text has no final newline. An insert session that ends there leaves the cursor there, as in Evil and Vim, so the next `i` types on that line, and motions along that line keep it there. The cursor rests on a grapheme's start: `e` on "café" with a combining mark lands on the é, as in Vim. `x`, `X`, `s`, `r`, `~` and an operator over `h` or `l` take a whole grapheme, also as in Vim; Evil's split a letter from its mark. `h` and `l` on their own step over a grapheme in both. `%` skips strings and takes any closing bracket as the partner, as in your Evil.
- Scrolling: `C-d C-f C-b C-e C-y` and `zz zt zb`. `C-u` doesn't scroll, as in your Evil; it's the prefix argument, so `C-u j` moves 4 lines. Window positions follow Evil: `M` takes the lower of two middle lines, a count given to `C-d` doesn't stick, and `G` recenters the window. `3H` goes to the third line; Evil's goes to the fourth, and die-vim deliberately fixes that.
- Text objects: words, WORDs, paragraphs, quotes and brackets, in both `i` and `a` forms. They follow your Evil in fundamental-mode. `_` and `::` split words. Quotes pair up from the start of the text and a string can cross lines, so a stray quote earlier on changes what counts as inside one. Inside a `"` string, `'` and `` ` `` pair up along the cursor's line, as in your Evil, so `i'` in `"don't work"` takes `t work`. `"` strings hide brackets from `i(` and the like. Brackets of any kind count toward depth, so `(a [b) c]` nests by position. Outside any pair, `i(` goes to the next one in the text. A string that never closes isn't one: Evil's range there is no use, so die-vim deliberately fails.
- Visual mode: char, line and block
- Insert: autoindent, and one undo step per insert session (arrow keys don't split it). Up and Down in insert state move by screen row and keep their column through a run of them, as your Evil's do. Down on the last row goes to the end of the text before it fails. A count replays a session, up to 1 MB of typed text or 10,000 other edits. Past that it fails with "Count too large", where your Evil would freeze. `o`, `O`, `cc`, `S` and Enter indent like the line above, which is what Evil does in text buffers. Indentation that depends on the language (Lisp, Python) belongs to a mode, not to die-vim.
- Registers: unnamed, `a`–`z`, `0`, `_` and `+`. The unnamed register syncs with the system clipboard both ways, including deletes, so `p` pastes whatever you last copied anywhere. `"_` is the escape. `1`–`9` hold the last nine yanks and deletes, newest first (Evil's kill ring), and `-` the last small delete. After `"Ay`, the unnamed register holds only the appended text. A count typed before `"x` goes, as in Evil. `"x` lasts for one command, even one that fails; Evil's stays after a failed `h`, `l` or `f`, and die-vim deliberately doesn't.
- Search with `/ ? n N * #`: incremental, highlights every match, smartcase, including for `*` and `#`. `*` and `#` follow your Evil in fundamental-mode: `_` isn't a word character, so `*` on `foo` matches inside `foo_bar` but not `foobar`, and `*` on punctuation finds nothing. A pattern that can match empty also follows your Evil: `/^` at a line's start stays there, and a search can land on the empty line after a final newline. A count before `/` or `?`, and an empty `/` or `?` that repeats the last search, work as in Vim. Evil ignores both; die-vim deliberately differs. A search that finds nothing is an error, as in Vim, so a macro stops there; Evil's only says so.
- Macros (`q`, `@`). Inside a macro, `.` does what Vim does: a change made there becomes the last change, so a later `.` repeats it. After `@a`, `.` replays the macro, as Evil's does, without Evil's "No previous macro" error when the macro used an operator.
- Undo and redo: `u` and `C-r`, a linear history as in Vim. Your Evil has no redo (`evil-undo-system` is unset, so its `u` walks Emacs's undo chain); this is a deliberate change. Setting `(setq evil-undo-system 'undo-redo)` would make your Emacs match while you use both. `C-r` with nothing to redo fails, as `u` with nothing to undo does. Each command is its own step. Emacs joins a run of `~`, `J`, `r` or `g~` into one, and die-vim deliberately doesn't. `u` puts the cursor back where the step's first change was made from. Three exceptions follow your Evil: `gU` or `gu` as the first change since the buffer was last unmodified goes to the change's start, `<<` from the start of an indented line goes to the end of the indentation, and `:s` goes to the end of its first match.
- The command line. `:` and `/` take your Evil's `<Up>` and `<Down>` (entries that start with the text before the cursor, and the cursor stays), `<C-u>` (the whole line), `<C-b>` (the line's start), and `<C-p>`, `<C-n>` and the page keys (every entry). Going past the newest brings back what you were typing, where Emacs stops with an error. Deliberate deviations: `<C-a>`, `<C-d>`, `<C-f>` and `<C-k>` keep their Emacs meanings on every prompt, where Evil has completion, the command window and digraphs. `<BS>` on an empty `/` leaves it, as in Vim and Evil's `evil-search` module; your Evil's `/` is isearch, which stays open. `@:` repeats the last ex line, with a count. On every prompt `<D-v>` pastes the clipboard, without the newlines it ends in, `<C-y>` the newest kill, and `<C-r>` a register, or with `<C-w>`, `<C-a>`, `<C-o>`, `<C-f>` and `<C-l>` the word, WORD, symbol, file name and line under the cursor, as Evil's command line has it. That goes past Evil on two counts: Evil's `/` is isearch, where `<C-r>` searches backward, and its other prompts don't have `<C-r>` at all. `<C-r>=` isn't built. `<C-v>` on `:` and `/` inserts the next key as it is. Keys with super go to the keymaps first, so `<D-q>` quits from a prompt.
- Replace mode (`R`)
- Windows: the `C-w` map, and `:sp :vs :only :close`. `:q` closes a split, and quits only from the last window, as in Evil.
- Ex commands `:w :q :wq :e :b :s` (current line, visual range and `%` only), plus `:N` to jump to a line, and `:w! :e! :q!`: `:w` refuses when the file changed on disk since DIE read it, `:w!` writes anyway, `:e!` reloads and `:q!` quits without saving. `:s` keeps the case of each match, as Emacs does: `:s/foo/bar/` turns `Foo` into `Bar`. Its replacements are Rust's: `$1`, `${1}`, `$name`, and `$$` for a literal `$`. So `$1x` names a group called `1x`, and a group followed by a letter needs `${1}x`. `\n`, `\t` and `\\` are a newline, a tab and a backslash, as in Evil. `n` repeats the pattern of `:s`, and an empty pattern is the last search's.
- Marks and the jumplist, as your Evil has them: `m{a-z}`, `m{A-Z}` across files, `` ` `` and `'` as motions, with operators and in visual state, and the marks ``` `` ```, `''`, `.`, `[`, `]`, `<`, `>` and `^`. Marks move with edits. `C-o` and `C-i` or `<Tab>` go through a list per window, which `gg G / ? n N * # % { } H M L`, marks, `gi`, `u` and a change of buffer add to, and which a split copies. `:N` adds nothing, as in your Evil. `gi`. `g;` and `g,` are Vim's, as your Evil has no goto-chg. Marks and the jumplist come back after a restart.

**Later, only if you miss them:** an undo tree.

**Out:** the full ex range grammar, `:g`/`:v`, `:normal`, `:!` (use a terminal instead), digraphs, search offsets, Vimscript, spell checking. Fold commands (`z…` other than `zz zt zb`) come from the folding extension.

## 11. Non-goals for v1

- Embedded scripting or live eval (D1)
- Emacs Lisp or Vimscript compatibility
- TRAMP-style remote editing, or any remote mode for DIE itself (D2)
- A TUI frontend (D5)
- Windows (D12); other Unixes beyond compile checks; the X11 PRIMARY selection
- A drawing API, a widget toolkit, or proportional fonts in core (D6, D11)
- Sixel (kitty doesn't support it either)
- Embedded web views
- LSP
- IME support beyond committed text from dead keys
- Font ligatures
- A package manager (Cargo already is one)
- Patching the terminal engine locally (D10)
- Multiple cursors, collaborative editing, accessibility tooling, bidirectional text
- Any built-in application
- Building any extension other than `die-vim` and `die-term`, including ports of your Emacs packages (D14)

**Stays in Emacs after this project.** Until you build them yourself: orchid, scrib, delta, dired-sidebar, buffer-menu, and the markdown preview (DIE has no web views). During M0, before DIE has a terminal, cargo, git and Claude Code run in Emacs's vterm next to DIE. The exit tests in §12 exclude all of these.

**Cut along the way:**

- From the first conversation: the embedded scripting layer, the three-way split between canvas, widget and embedded surfaces, and web surfaces. The six surface kinds became one text surface plus one trait.
- From the previous draft of this doc: the terminal frontend, the frame protocol, and the client/server split for remote work. They aren't needed now that SSH happens inside DIE.
- From the 2026-10-19 review: ratatui in core (D8), Zig in the build (D16), a terminal engine DIE owns or forks (D10, D17), and restart by in-place `execve`, which doesn't survive a crash (D20).

## 12. Plan

### Spikes (S0)

S0 takes about two weeks. Each spike has pass criteria and a named fallback. Each spike writes a decision record in [records/](records/) with its detailed criteria and measured results. The throwaway code lives in `spikes/`.

| Spike | Question | Fallback |
|---|---|---|
| **S1 Renderer and platform** (D13) | Passed with changes on 2026-10-19 ([records/s1-renderer.md](records/s1-renderer.md)). Can winit, Metal and Core Text meet these on your 120 Hz panel?<br>- DIE's share of typing latency (key event to committed frame) p99 ≤ 3 ms, and key to display p50 ≤ 12 ms in a window that covers the display<br>- a full redraw in ≤ 2 ms of main-thread time<br>- zero frames when idle<br>- a 60 Hz producer with no main-thread pixel copies<br>- a clean keyboard matrix, including key release<br>- correct glyph widths<br>- a rebuild-to-running loop of ≤ 5 s | Not needed. The named fallbacks were wgpu with buffer uploads only, and AppKit directly instead of winit |
| **S2 Terminal engine wiring and helper process** (D10, D20) | Does DIE's exact wiring build and pass the engine battery (DIE's own PNG decoder, no default features)? How fast do frames and images cross from a helper process? | Not needed: S2 passed on 2026-10-19 ([record](records/s2-term-engine.md)). The named fallbacks were DIE's own layer over the engine's C API, and a thread per terminal inside DIE |
| **S3 Terminal survival** (D20) | Passed 2026-10-19 ([record](records/s3-fd-store.md)). `die --fd-store` kept every shell through 20 planned restarts and 5 × `kill -9`, also as a `.app`, with no output lost in the kernel. 100 open and kill cycles leaked nothing. A holder from build N served build N+1 and refused an incompatible major. Shell exit is seen in under 1 ms without being the parent. | In-place `execve` (loses shells on a crash) |
| **S4 Core API and layout** (contract C2–C4, C14–C16) | A headless prototype checks:<br>- the checkout rules<br>- queued calls and subscription handles<br>- edit origins<br>- undo across silent edits<br>- stored versus provided decorations<br>- `edits_since`<br>- the keymap rules<br><br>A property test checks that the cursor is never inside hidden text. | Hidden ranges limited to whole lines, and virtual text to end of line |
| **S5 Evil oracle** (D4) | Done ([records/s5-evil-oracle.md](records/s5-evil-oracle.md)). 306 cases in about 1.2 s. Window geometry and wrapped lines run in a tty Emacs the oracle starts on its own pty. | Hand-written expectations |
| **S6 Portable renderer** (D13) | After checkpoint 4, on the renderer track. S6a: does S1's renderer, ported to wgpu-Metal and presenting to a real surface, meet S1's numbers? S6b: does fontations give the same 19×40 cell and acceptable glyphs, on your Mac and in Debian 13 and Fedora 43 containers? S6c: do the window, keys, clipboard, build rules and frame pacing work on Linux without a GPU, in colima? | A native backend per platform: Vulkan or GL on Linux, with DIE's own fontconfig and FreeType FFI |
| **S3L Linux processes** (D20) | At the start of M2, in colima and on the hosted arm runner. Do S3's survival guarantees hold on Linux, at S3's bars? Plus no inherited fds, a controlling tty through `posix_spawn`, exit status through the holder's pidfd (right on 6.15+, "unknown" before, never wrong), and no lost output when a shell exits. S2's helper runs the engine, so it's tested on Linux with S7. | For exit status, the helper as the shell's parent (about 60 lines) |
| **S7 Linux engine archive** (D10, D20) | Before M2's exit, once a signed Linux archive exists. Does it fit DIE's exact wiring, and does S2's helper hold on Linux? It builds with no Zig, passes 81 of 86 checks with the same 5 gaps, holds the helper's gates, and links next to Rust's `compiler_builtins`. | Linux ships editor-only, and gets terminals at the first `ghostty bump` that has an archive |
| **SL-desktop** (D20) | Before Linux is "supported", on a real Linux desktop or a desktop VM. Does a desktop session kill terminals with DIE? | The holder gets its own systemd user scope |
| **SL-hw** (D13) | Before Linux is "supported", on real hardware with a GPU. Does Linux meet S1's budgets under GNOME Wayland, X11 and sway? | GL instead of Vulkan, then stricter Fifo gating, then a native backend per platform |

All five S0 spikes passed, so S0's spikes are done. The Linux spikes (S6, S3L, S7, SL-desktop and SL-hw) come later, at the times in their rows; they're detailed in [records/platforms.md](records/platforms.md) §8. S1 confirmed the renderer stack; five checks that need your hands are listed in its record (§6). S2 built DIE's exact wiring (81 of 86 checks, the same 5 known gaps, no Zig) and chose the helper process (D20). S4 passed on 2026-10-19 without its fallback ([records/s4-core-api.md](records/s4-core-api.md)), and you accepted its API recommendations. The 2026-10-19 check already covered libghostty-vt from Rust: a clean build with no Zig, and the graphics, mouse, key and protocol battery ([records/die-term-engine.md](records/die-term-engine.md)).

### Milestones

Each milestone ends with a test. None of them involves building an extension (D14). The milestone ids are kept from the original plan, so the order is S0, M0, M2, M1, M3, M4 (D23).

| | Milestone | Scope | Exit test |
|---|---|---|---|
| M0 | Edits itself | Planned in [m0-plan.md](m0-plan.md), with progress in [m0-log.md](m0-log.md): four dogfood checkpoints, headless grammar (weeks 3–5), reader (5–7), editor (8–11) and daily driver (10–14), and about 11–16 weeks in all. Built API-first. First, the public API die-vim needs and the headless harness:<br>- `Extension`, registry and a `Ctx` subset<br>- commands and keymaps<br>- the text API, with undo groups, selections, layout queries, the prefix argument and `feed_keys`<br><br>Then on top of them:<br>- the native window and renderer<br>- the text surface, files, buffers and splits<br>- die-vim (§10)<br>- command line and prompt, status line, faces<br>- auto-revert, the session and `restart` | (a) You edit DIE's own source in DIE for a full day, with Emacs's vterm in an adjacent tile as the terminal.<br>(b) 20 rebuild-and-restarts restore files, splits, per-window cursors and scroll, unsaved text and registers, within 1 s from `restart` to the first frame.<br>(c) `cargo fmt` on an open file reloads an unmodified buffer, and `:w` refuses on a modified one.<br>(d) Every Evil-oracle case passes (523 today), except the deliberate deviations, which are listed by name and have hand-written fixtures.<br>(e) The budgets in guard 7 hold.<br>(f) The workspace cross-checks for x86_64 and aarch64 Linux, and die-core, die-vim and the oracle pass their tests on aarch64 Linux. |
| M2 | Terminal | - the Surface trait<br>- the process and PTY API, the holder, and the terminal helper<br>- `die-term` with ghostel's features except graphics (§6), including the mouse<br>- passthrough and the D18 input model, with full key events<br>- copy state<br>- die-term's API<br>- the login environment<br>- the Linux branches: the process layer, holder and helper; the window, wgpu and fontations renderer, clipboard and packaging ([records/platforms.md](records/platforms.md) §5.1) | (a) A cold start opens `term1` the way your `user/startup` hook does; a restored start doesn't add one.<br>(b) A day of your real local work in die-term. Claude Code and codex sessions survive at least 10 rebuild-and-restarts and a `kill -9` of DIE. btop and ncdu work with the mouse. Output floods don't slow typing.<br>(c) One SSH session to your colima VM, with remote vim using the kitty keyboard flags.<br>(d) The `kitten show-key` matrix passes.<br>(e) cargo resolves in a die-term shell when DIE.app is launched from the Dock.<br>(f) Linux preview: on x86_64 and aarch64, the headless tests, the Linux clean-build, the ELF check, S3L, smoke tests and lavapipe fidelity pass. Terminals are part of it only if a signed Linux archive exists by then (D10). (a)–(e) don't wait on it; if it slips, it runs during M1. |
| M1 | Contract completeness | - full modes and face providers<br>- every decoration kind except Hide with a placement (M3)<br>- `spawn`, `process`, `watch` and timers<br>- nested prompts<br>- the hooks API | Every row of the requirements check (extension-contract §18), including die-vim and mode save and restore across a harness restart, has a passing contract test that uses only `die`'s public API. The placement and image parts of the die-term and markdown rows are left to M3's inline-image test. The multi-step prompt test settles contract §19 question 1. |
| M3 | Pixels | - placements, with source rectangle, pixel offset and z<br>- images<br>- pixel buffers through the staging ring<br>- kitty graphics in `die-term`<br>- image placement in text buffers through Hide decorations | `timg` and `kitten icat` (direct placements) and `kitten icat --unicode-placeholder` render, locally and over SSH. yazi previews images. The inline-image contract test passes. A 60 fps pixel-buffer fixture keeps DIE's share of typing latency within 1 ms of S1's. Key to display may be up to one refresh (8.3 ms) slower at p99, which is the panel's limit. |
| M4 | Ready for extensions | - rustdoc for core's whole public API<br>- an example config crate | You can start your first extension in its own repository, against `die` as a git dependency, without changing core. |

### Crates

```
crates/die-core            headless editor; no window or GPU dependencies
crates/die-render          winit; Metal and Core Text on macOS, wgpu and fontations on Linux; clipboard; draws die-core's scene
crates/die-vim             Vim grammar; depends on die-core only (D15)
crates/die-term            terminal surface; die-core + libghostty-rs; also the helper process (`die --term-helper`)
crates/die-term-sys        the signed engine archives (one per target that has one), generated bindings, a checksum-checking build.rs (D17)
crates/die                 facade: die::run(config), registers die-vim, and die-term where an engine archive exists (D10)
crates/die-contract-tests  test-only fixture extensions for extension-contract §18, using only die's public API
apps/die-dev               your config while DIE is being built
tools/evil-oracle          Evil in batch and tty Emacs (dev-only); fixtures are committed, so cargo test needs no Emacs
tools/test-cap             die-test-cap: the memory limit inside every test binary (dev-only, never linked into DIE)
xtask                      cargo xtask ghostty bump, budget report, clean-environment build check
```

`unsafe` is denied everywhere except three small places: `die-render`'s platform modules (`macos`, and `linux` with about 10–30 lines), `die-term`'s engine boundary, and `die-core`'s `sys` module, one file per OS. On macOS, `sys` is about 80 lines of libc calls for processes and PTYs: `posix_spawn`, `getpeereid`, kqueue, `posix_openpt`, which have no safe route there. Linux adds about 30–40 lines, for the spawn flags and the `PIDFD_GET_INFO` ioctl. You review those modules by hand. One more exception sits outside DIE: `die-test-cap`'s allocator, about 65 lines in `tools/test-cap/src/cap.rs`, and the tests beside it. A global allocator can't be written without `unsafe`, and this one is never linked into DIE: `cargo xtask lint` fails the crate under anything but `[dev-dependencies]`. Its blocks follow die-render's rules: each says why it's sound, and does one unsafe thing. Every other crate stays under `forbid`. `cargo xtask lint` rejects the `unsafe` keyword itself outside these places, since rustc's `unsafe_code` lint misses `#[unsafe(naked)]` and, where it's `deny`, can be lowered by a macro. It also rejects `include!` and the `path` attribute, which could compile a file it never reads. A macro can build that attribute from parts the lint can't recognise, and a manifest can point anywhere, so `cargo xtask sources` then asks the compiler, on macOS and for each Linux target: every file a local crate read must be a `.rs` file in that crate's own directory, and every local crate must be a listed workspace member. Unsafe code inside dependencies, including what their macros expand to in DIE's crates (slotmap's `new_key_type!` emits an `unsafe impl Key`), belongs to the dependency and isn't part of your review. Neither check can see a dependency macro that wraps your code in `unsafe`, so adding one is a dependency decision.

### How the work is split

- **You own** what defines DIE: the core API and contract, die-vim's grammar, and the decision records.
- **An AI drafts,** under tests you review: the renderer and platform layer, `die-term`'s glue, the holder, and test fixtures.
- **In M0 (question 09, answered 2026-10-19):** an AI drafts die-core's API and die-vim's grammar too, and each change to either waits for your approval as a short rustdoc diff (`cargo xtask api-diff`). Everything else merges when `cargo xtask ci` passes. You read the subtlest code in full: undo, layout, key dispatch and die-vim's grammar core.

On that basis M0 is roughly 10–15 weeks, at about 17k lines across crates. Parallel AI sessions can't shorten a chain that has one reviewer. There's no deadline. Dogfood checkpoints inside M0 keep it from going weeks without use:

1. die-vim's first tranche passes headless.
2. Read-only file viewing in the real window.
3. Editing and saving in one window.
4. Splits and restart.

**Testing.**
- die-vim is checked against Evil itself (S5). Keys that depend on window geometry or wrapping come from a tty Emacs the oracle drives. Hand-written fixtures are only for the deliberate deviations from Evil, each with a reason. Window and file commands are the exception: the oracle has no window tree and no files, so core's harness tests cover them.
- Terminal tests use your colima VM as the Linux SSH host. kitty's standalone `kitten` binary, kept in a scratch directory rather than installed system-wide, checks key encoding.
- **Memory.** Every test binary carries its own limits, in `die-test-cap`. Its global allocator counts live memory. Past 4 GiB, it writes one line to stderr, and the process aborts. Behind that, the binary runs itself again under `taskpolicy -m` when it starts, and macOS kills it at 8 GiB, whatever is growing. Linux has no `taskpolicy`, so it has the count alone. `cargo xtask lint` fails a test binary that doesn't link the crate. The crate's docs say what the limits don't cover.
- **Linux.** Every `cargo xtask ci` cross-checks and lints the whole workspace for x86_64 and aarch64 Linux on your Mac, in about 11 s. `cargo xtask ci --linux` runs die-core, die-vim, the oracle, the memory limit and xtask's tests in an aarch64 container on colima, weekly and at each checkpoint. From M2, hosted ubuntu-24.04 and ubuntu-24.04-arm jobs add the Linux clean-build, the ELF check, every test, S3L, smoke tests under Xvfb and headless sway, and lavapipe fidelity, plus a FreeBSD `cargo check`. Until a signed Linux archive exists, they run without die-term (D10). colima has no GPU and no display, so latency and real desktop behaviour wait for Linux hardware (SL-hw, SL-desktop).

## 13. Open questions

Settled in the 2026-10-19 review: the renderer stack (D13), libghostty-vt from Rust (D10), ratatui (D8), how terminals survive restarts (D20), and where the terminal engine runs (S2). Pixel-buffer threading is now one of S1's checks.

1. ~~Terminal engine in a helper process (D20).~~ **Settled by S2 (2026-10-19):** each terminal's engine runs in a helper process ([records/s2-term-engine.md](records/s2-term-engine.md)).
2. **Scaled text runs.** Should the cell renderer support scaled text, so a future markdown extension could show large headings? Or are color and weight enough? Not for terminals: the engine ignores kitty's text-sizing protocol (OSC 66). Deferred until a markdown extension asks for it.
3. **Undo history across restarts.** The session doesn't save undo history in v1. Revisit if restarts that lose undo bother you.
