> **Decision record for scope D10 (accepted 2026-10-19).** This is the full record from the 2026-10-19 check of the prebuilt-libghostty path. The question and your answer are in [.questions/01](../../.questions/01-terminal-engine.md). The helper-process question in §9 became scope D20 and spike S2. Evidence paths under `/private/tmp` were scratch files and may no longer exist.

# die-term decision record: prebuilt libghostty-vt (the ghostel model)

Date: 2026-10-19. Status: **accepted 2026-10-19** as scope D10. §9's question 2 became D20 and spike S2. S2 then built the exact configuration that §5 calls "the one gap", and it passed ([s2-term-engine.md](s2-term-engine.md)).
Engine pin: Ghostty `main` 765ea37ad64f. That is also today's signed `tip`, re-checked with `git ls-remote` on 2026-10-19. Rust wrapper: libghostty-rs `d2036bae`, which is still the head of master today.

## 1. Recommendation

Build die-term the way ghostel is built, on Ghostty's own terminal engine, libghostty-vt, linked as a prebuilt static library.

- **The binary.** Ghostty's CI builds and signs it. `cargo xtask ghostty bump` then:
  - checks the signature
  - ties the binary to a commit whose tests passed
  - cuts it down to arm64 with only Ghostty's own symbols visible (2.7 MB)
  - commits it with a lock file
- **The build.** Building DIE needs only cargo and the Xcode Command Line Tools. Zig runs only in Ghostty's CI. That is already true of the ghostel module you use today: `ghostel-module-install.el` downloads a prebuilt binary from GitHub releases (line 62).
- **What the engine does.** All the terminal work:
  - parsing, the screen, scrollback and reflow
  - the kitty keyboard protocol
  - kitty graphics storage and placement
  - mouse, paste and focus encoding
  - selection, search, OSC callbacks and query replies
- **What DIE owns.** No engine code. die-term is about 3.2k lines of glue (±30%) that turn engine state into DIE cells and placements and send keys and mouse back. On top of that come about 1.1k lines of tests and about 1k lines of tooling, scripts and data.
- **The Rust API.** The community crate libghostty-rs at a pinned rev. It is an ordinary dependency and is never forked.
- **Evidence, checked today against 765ea37:**
  - Two independent harnesses pass: 81 of 86 checks, where the 5 failures are known gaps, and 76 of 76 checks through raw FFI.
  - The build runs under `env -i` with a fake `zig` on PATH that is never called.
  - The binary links only libSystem.
- **What is not available yet.** Nothing blocks the plan. Three features (exit codes on prompt marks, shell-to-editor commands, and OSC 7501 and reset) wait on engine callbacks the wrapper does not expose yet. Your ghostel doesn't give you any of them today either.

A self-drawing terminal view embedded in DIE was ruled out. Copy state, Vim motions and editing the shell's input line all need the cells (scope §5.5), and scope §5.11 allows one renderer.

**Fallbacks, and what triggers each:**

- **Wrapper to DIE's own engine layer.** DIE replaces libghostty-rs with its own direct engine calls in one module, about 1.2k–4k more lines (the two reviewers' estimates differ). DIE never carries a fork of the wrapper. This happens if any of these is true:
  - the wrapper blocks an engine bump for more than one monthly cycle
  - a wrapper soundness bug affects DIE
  - you want exit codes or shell-to-editor commands before upstream exposes the callbacks (PR #92 has been open since 2026-09-08 and needs a follow-up)
- **Binary source to DIE's own CI build.** DIE keeps building from the committed archive and adds a CI job (never on your machine) that builds the archive with Zig from a pinned commit. This happens if:
  - Ghostty stops publishing the xcframework, or renames it
  - a fix DIE needs has no signed build within about 4 weeks

  A third trigger, a target Ghostty doesn't publish (Linux today), was proposed in [platforms.md](platforms.md) §4.3 and declined on 2026-10-19, as was asking Ghostty to publish Linux archives. Where a signed Linux archive comes from is open until before M2 (scope D10).
- **Engine to alacritty_terminal plus a DIE-written kitty graphics layer.** About 7.5–8.5k lines plus about 2k of tests. This happens if:
  - libghostty-vt's C API is withdrawn or relicensed
  - no published build passes DIE's battery for 3 months

## 2. How it works

```
DIE process
  main thread (editor state, D7)
    TermSurface: row cache, placements, cursor, mode summary, title/pwd/prompt marks
    (plain Send data only; never touches an engine handle)
       |  Job (asynchronous only): Key, Mouse, Paste, Focus, Resize, Scroll,
       |  Select, Search, Freeze, LineEdit, Snapshot
       v                                   ^ FrameDelta, ImageUpload (RGBA Arc),
  parser thread, one per terminal          | Placements, Effect
    owns every !Send engine object (Terminal, RenderState, encoders, iterators);
    runs engine callbacks, the placeholder resolver, the legacy-key shim, the PNG hook
       ^ PTY bytes                 | replies, key/mouse/paste/focus bytes
  PTY reader <---- master fd ----> PTY writer
                    | fd passed over a Unix socket (SCM_RIGHTS)
  die-ptyd (PTY holder, ~300 lines): keeps shells alive across DIE crashes and restarts
```

Question 2 in §9 asks whether the parser thread should instead live in a helper process, one per terminal.

**Threading:**

- **Why one thread per terminal.** Engine types are `!Send`, and callbacks run synchronously inside `vt_write`. A 150 ms callback makes `vt_write` take 154 ms. PNG decode takes 5.4 ms at 1080p and 16.5 ms at 4K. On its own thread, a stall like this only affects that one terminal.
- **Deadlock rule.** The main thread never calls the engine and never waits on a parser thread; every job is asynchronous.
  - The engine's clipboard read and write callbacks must reply before they return (terminal.h ~800–990), and contract §12 has no blocking calls.
  - OSC 52 read is denied by default. If you enable a prompt for it, the parser thread waits on the prompt with a timeout and then denies.
- **Thread safety.** The engine has no thread-locals and its generation counter is atomic. Eight threads gave state identical to a single-thread run over 1,600 runs. A 60 s fuzz on 8 threads (15 GB of input) did not crash.

**Frames:**

- Frames are pulled at vsync, and only when the terminal is dirty.
- Only dirty rows are converted to DIE cells.
- While mode 2026 (synchronized output) holds rendering, no frame is produced, up to a timeout.
- Cost: the proposal measured 0.53 ms for a full 250×70 frame including conversion to DIE cells, and 15 µs for one dirty row. The reviewer measured 0.11–0.32 ms for the engine side alone. The 0.53 ms figure was not reproduced.

**Images:**

- Each image is uploaded once per `(id, generation)`. Placements are sent every frame.
- A placement partly scrolled off has a negative row. Negative z draws below the text.
- For Unicode-placeholder placements (yazi), the engine gives the size but **not the position**. DIE's resolver finds the position from the placeholder cells:
  - the foreground colour gives the image id
  - the underline colour gives the placement id
  - the diacritics give the row and column
  - only rows flagged `KITTY_VIRTUAL_PLACEHOLDER` are scanned

**Hyperlinks.** The C API gives each cell only a `HAS_HYPERLINK` flag. die-term fetches the URI with `ghostty_grid_ref_hyperlink_uri` for flagged cells, only on rows whose `HYPERLINK` flag is set, and interns it. OSC 8's `id=` parameter is not exposed.

**Scrollback compression** is driven by the caller (terminal.h:45–54, 3248–3320). G1 runs incremental compression from an idle timer whenever the activity token changes.

**Engine defaults G1 must override** (all verified):

| Setting | Engine default | Effect of the default |
|---|---|---|
| Scrollback | 10 KB | about 840 rows at 80 columns |
| Image storage | 10 MB | older images are evicted |
| Mode 2027 | off | skin-tone emoji split into two cells |
| Option as Alt | off | alt+x sends "≈" |
| `continuation_max_bytes` | 0 | |
| fg/bg colours | unset | OSC 10/11 queries go unanswered |
| XTVERSION | `libghostty` | yazi treats that as Ghostty (§8) |

**What die-term needs from core (not in the contract yet):**

- **PTY:**
  - PTY output delivered to a non-main thread (contract §12 runs PTY callbacks on the main thread)
  - TIOCSWINSZ with pixel sizes
  - `tcgetattr` (password prompts) and `tcgetpgrp` (confirm before killing a running command)
- **Input events:**
  - key events with the unshifted codepoint and IME composing state
  - pointer events with click count and precise trackpad scroll deltas
- **Placements and images:**
  - placements with negative z, source rectangles and pixel offsets
  - a texture cache keyed by generation
- **Renderer, for kitty-like visuals.** The engine does not do these:
  - box-drawing, block, braille and powerline glyphs drawn as sprites so they join across cells, with a per-range override, because your init.el pins braille to Ioskeley Mono
  - bold-as-bright, because the engine's foreground colour does not apply it
- **macOS services:** NSCursor shapes, macOS notifications, and a way for threads to wake the main loop.

## 3. Ghostel-parity checklist

**How to read this table:**

- **Verified** means the engine behaviour was exercised against 765ea37. The glue itself is unwritten everywhere.
- **Assumed** means the row depends on core or glue that isn't built, or on an engine path nobody exercised.
- **Blocked (wrapper)** means the raw C API does it (verified), but the pinned libghostty-rs cannot register the callback. Its `Terminal.inner` is `pub(crate)` (terminal.rs:228), and the engine allows one userdata pointer per terminal.
- **G#** refers to the glue modules listed after this table. A module's line count appears on the first row that uses it.

| Capability | Provided by | DIE glue LOC | Status |
|---|---|---|---|
| **Display** ||||
| 16, 256 and 24-bit colour; palette from the theme; OSC 4/10/11 replies | Engine + G19 + core theme | 60 | Verified (OSC 10/11 go unanswered until colours are set) |
| Bold, italic, faint, inverse, strike, overline; 5 underline styles with colour | Engine + G2 + core renderer | 300 | Verified |
| Wide characters, graphemes, skin-tone emoji (mode 2027 turned on) | Engine, set by G1 + core fonts | 330 | Verified |
| Box-drawing, block, braille and powerline glyphs that join across cells; bold-as-bright | Core renderer | – | Assumed (core work) |
| Cursor shape, blink and visibility; alternate screen; reflow | Engine + G2 | (G2) | Verified |
| Synchronized output (2026) | Engine render-hold callback + G13 | 90 | Verified |
| Scrollback of 10k lines (capped at 64 MB); compression while idle | Engine via G1 | (G1) | Size Verified (9,611 rows); compression Assumed |
| Redraw only what changed | Engine dirty rows + G2 + core vsync | (G2) | Verified |
| Title (OSC 0/2), bell | Engine + G11 + core | 180 | Verified |
| Colour-scheme reports (996/2031) | Engine + G19 | (G19) | Assumed |
| **Kitty graphics** ||||
| Direct transmission: PNG, RGB(A), grey, zlib, chunked | Engine + G10 (one process-global PNG hook) | 40 | Verified through raw FFI; wrapper + hook combination Assumed (§5) |
| File, temp-file and shm media off | Engine default, which G1 asserts | – | Verified |
| Direct placements: cells or pixels, crop, offsets, z, alpha, partly scrolled, in scrollback | Engine + G8 + core compositor | 200 | Verified |
| Relative placements | Engine + G8 | (G8) | Verified (except when the parent is a placeholder image) |
| Unicode placeholders | Engine gives the size; G9 finds the position | 150 + ~300 data | Engine inputs Verified; the resolver passed yazi fixtures (2026-10-18) |
| Delete variants, replacing an image, `a=q` | Engine | – | Verified |
| Image memory cap (64 MiB per screen) | Engine via G1 | (G1) | Verified (limit and eviction) |
| XTWINOPS 14/16/18/19 in physical pixels | Engine + G14 | 60 | Verified |
| Engine-driven animation loop | – | – | Gap (frames the client switches with `a=a,c=N` work) |
| **Mouse** ||||
| Tracking modes 9/1000/1002/1003; formats 1005/1006/1015/1016 | Engine encoder + G5 + core pointer events | 200 | Verified |
| Wheel goes to the app when it tracks the mouse, otherwise scrolls; mode 1007 sends arrows on the alternate screen; trackpad accumulation | G5 | (G5) | Assumed (the encoder emits nothing for 1007: Verified) |
| Shift-click selects even while an app tracks the mouse | G5 | (G5) | Assumed |
| Drag enters copy state; double click selects a word, triple click a line | Engine `select_word`/`select_line` + G6 + core | 260 | Verified (engine) |
| Clicking a link opens it | G7 + core | 260 | Assumed (URI lookup Verified) |
| Pointer shape (OSC 22) mapped to an NSCursor | Engine + G5 + core | (G5) | Verified (engine) |
| Drag and drop of files and text | Core + G13 | (G13) | Assumed |
| A click that only focuses the window doesn't move point | Core | – | Assumed |
| **Keyboard** ||||
| Passthrough, with one reserved key to leave it | Core | – | Assumed |
| Legacy keys, DECCKM, keypad mode, modifyOtherKeys | Engine encoder + G4 | 330 | DECCKM Verified; keypad and modifyOtherKeys Assumed |
| Kitty keyboard flags 1–31: press, repeat, release | Engine + G4 + core full key events | (G4) | Verified |
| In legacy mode, ctrl+[, ctrl+i, ctrl+m and modified Enter/Tab/Esc send C0 bytes | G4 shim (~40 lines) | (G4) | The need is Verified (key_encode.zig:845–853); the shim is Assumed |
| Option as Meta | Engine option + G4 | (G4) | Verified |
| C-c, C-z and C-d; sending the next key literally; macOS IME | Core + G4 | (G4) | Assumed |
| Focus reporting (1004) | Engine + G13 | (G13) | Verified |
| Typing while scrolled back jumps to the bottom | G6 | (G6) | Assumed |
| **Clipboard and paste** ||||
| Bracketed paste with an unsafe-paste check | Engine + G13 + core registers | (G13) | Verified |
| OSC 52 write lands on the macOS clipboard | Engine + G11 + core | (G11) | Verified |
| OSC 52 read, denied by default | Engine + G11 | (G11) | Verified (the callback is synchronous) |
| Copy scrollback with soft wraps removed | Engine formatter + G6 | (G6) | Verified |
| **Protocols and shell integration** ||||
| OSC 7 working directory | Engine + G11 | (G11) | Verified |
| OSC 133 prompt marks, prompt jumps, input region | Engine row and cell semantic data + G12 | 80 | Verified |
| OSC 133 exit status | Engine semantic-prompt callback | (G12) | Blocked (wrapper) |
| OSC 8 hyperlinks (DIE interns the URIs) | Engine + G7 | (G7) | Verified |
| Plain-text URL and `file:line` detection across soft wraps | G7 | (G7) | Assumed |
| Notifications (OSC 9/777), progress (OSC 9;4) | Engine + G11 + core | (G11) | Verified |
| Program status (OSC 7501), reset (RIS) effect | Engine callbacks | (G11) | Blocked (wrapper) |
| DA, XTVERSION, DSR, DECRQM, DECRQSS, XTGETTCAP | Engine + G17 | 60 + ~90 terminfo | Verified (XTVERSION and the terminal name are settable) |
| Shell integration on `/bin/bash` 3.2 (via `--rcfile`) | G16 + ~160 lines of bash | 110 | Verified (bash behaviour) |
| Shell-to-editor commands (private OSC) | Engine unknown-sequence callback | (G16) | Blocked (wrapper) |
| Title stack (CSI 22/23 t) | – | – | Gap |
| **Modes (the evil-ghostel equivalent)** ||||
| Copy state, frozen, entered by selection | Core + G6 | (G6) | Assumed |
| Search scrollback | Engine search + G6 + core | (G6) | Assumed (not exercised) |
| ESC enters normal state, or goes to the app on the alternate screen | Core + G15 | 250 | Assumed |
| Vim motions over the grid and scrollback | Core copy state + die-vim | – | Assumed |
| Operators edit the shell's input line, kept inside the input region | G15 + die-vim | (G15) | Assumed (input cells marked by OSC 133: Verified) |
| Cursor shape follows modal state; bookmarks | Core | – | Assumed |
| **Lifecycle** ||||
| PTY; the buffer closes when the shell exits; confirm before killing a running command (`tcgetpgrp`) | Core + G3 | 150 | Assumed |
| Password prompt detection, then secure input | G18 using `tcgetattr` (ECHO off) | 40 | Assumed |
| Several terminals; project terminals | Core | – | Assumed |
| Survive a DIE restart | PTY holder + G20 snapshot (or the helper process, question 2) | 80 | Snapshot round trip Verified (keeps text, drops images) |
| Clear; clear scrollback | Engine + G6 | (G6) | Assumed |
| ghostel's Emacs-only parts: line-spacing workarounds, image slicing, writing scrollback into a buffer, the module loader, comint, ConPTY, TRAMP terminfo push, middle-click primary selection | – | – | Dropped |

On password detection: the engine's `CURSOR_PASSWORD_INPUT` flag has no setter in the C API. Only Ghostty's app sets it (src/Surface.zig:1444–1447), so it doesn't help G18.

**Glue modules (die-term Rust):**

| G | Module | LOC |
|---|---|---|
| G1 | Parser-thread host, channels, callbacks, engine defaults, idle compression | 330 |
| G2 | Render state to DIE cells, cursor, dirty rows | 300 |
| G3 | PTY bridge, exit and kill lifecycle | 150 |
| G4 | Keys: NSEvent table, modifiers, legacy shim, option as Alt, IME | 330 |
| G5 | Mouse policy, wheel, mode 1007, trackpad, shift override, pointer shape | 200 |
| G6 | Selection, freeze, `text()`, search, clear | 260 |
| G7 | Links: OSC 8 URI interning, URL and `file:line` detection | 260 |
| G8 | Direct placements to DIE placements | 200 |
| G9 | Placeholder resolver (plus a ~300-line diacritic table) | 150 |
| G10 | Process-global PNG decode hook | 40 |
| G11 | OSC effects | 180 |
| G12 | OSC 133 prompts | 80 |
| G13 | Paste, drag and drop, focus, synchronized-output hold | 90 |
| G14 | Resize, pixel sizes, XTWINOPS | 60 |
| G15 | Editing the shell's input line from normal state | 250 |
| G16 | Shell bootstrap, command channel | 110 |
| G17 | Identity, terminfo | 60 |
| G18 | Password prompt detection | 40 |
| G19 | Palette, theme, colour-scheme reports | 60 |
| G20 | Snapshot and restart hook | 80 |
| | **Total** | **≈3.2k (±30%: 2.3–4.2k), estimated** |

## 4. What DIE owns, compared with the alternatives

Every column is measured against the same target: ghostel parity as in §3. All figures are estimates. Nothing has been written yet.

| | **Prebuilt libghostty-vt + libghostty-rs (recommended)** | Same, with DIE's own engine layer | alacritty_terminal 0.26 + DIE kitty layer | rio-vt 0.5.28 | qwertty-term-vt fork |
|---|---|---|---|---|---|
| Parity glue | ≈3.0k | ≈3.0k | ≈3.0k | ≈3.0k | ≈2.1k |
| Engine-specific DIE code | ≈0.25k (resolver, PNG hook, key shim; counted in the glue) + 0.1k sys crate | + 1.2–4k of direct engine calls | ≈3–4k (kitty graphics, placeholders, OSC pre-filter, mouse encoder, XTGETTCAP/DECRQSS, emoji widths) + ~1.4k lifted code | ≈0.15k of patches, plus fixes that are not small (a remote-triggerable abort, an image-budget leak, a u16 extras table) | ~0.25k of fork edits |
| Tests | ~1.1k (the prototype battery is 1,125 lines) | ~1.3–1.5k | ~2k | not estimated | not estimated |
| Tooling, scripts, data | ~0.45k xtask + ~0.55k | same | ~0.25k + scripts | ~0.25k + scripts | ~0.25k |
| **Hand-written total** | **≈5.3k (4.3–6.6k)** | **≈6.5–9k** | **≈9.5–10.5k** | **≈3.4k + tests + a carried patch set** | **≈70k (you rejected this)** |
| Generated or binary, refreshed each bump | 5.6k lines of bindings + a 2.7 MB archive | same | none | none (but compiles simdutf C++ through cc) | none |
| Third-party code depended on | Engine binary + libghostty-rs (15.4k lines, 7.6k of them non-test source; effectively one active maintainer) | Engine binary only | alacritty_terminal | rio-vt (one maintainer) | – |
| Engine code DIE maintains | 0 | 0 | Coupled to alacritty internals | Patches in its hottest files | All of it |

**Why the figure went up from 1.5k.** You were last told about 1.5k lines of glue. That was for a text-only terminal on alacritty_terminal. Ghostel parity (images, mouse, input-line editing, copy state) costs about 3k lines of glue on any engine.

## 5. Getting and updating the binary

### Layout (verified, with one gap)

The `[patch]` setup builds with no `.cargo/config.toml` and never runs `zig`. The binary links only libSystem.

```toml
[workspace.dependencies]
libghostty-vt = { git = "https://github.com/uzaaft/libghostty-rs", rev = "<pinned>", default-features = false, features = ["kitty-graphics"] }

[patch."https://github.com/uzaaft/libghostty-rs"]
libghostty-vt-sys = { path = "crates/die-term-sys" }   # links = "ghostty-vt"
```

`crates/die-term-sys/` contains:

- `vendor/<target>/libghostty-vt.a`: one archive per supported target. Today that's only macOS's, thinned to arm64, stripped and prelinked
- `vendor/<target>/libghostty-vt.a.sha256`
- `src/bindings.rs`: generated from the archive's own headers
- `src/lib.rs`: about 90 lines from libghostty-rs, MIT/Apache, with attribution
- `build.rs`: about 15 lines. It picks the archive by `TARGET`, checks the SHA-256 and emits the link flags. On a target it has no archive for, it links nothing, and the `die` facade leaves die-term out there (scope D10). No network, git or Zig.
- `ghostty-vt.lock`, with a section per target
- the `.minisig`
- `LICENSE-ghostty` and `THIRD_PARTY_NOTICES`

**The one gap: the exact proposed configuration is unbuilt.** The verified build used `features = ["png"]` and the wrapper's thread-local PNG decoder. The proposed configuration (`default-features = false` plus DIE's global hook installed through die-term-sys) has not been built. Building it is the first spike task, and it must confirm that no wrapper code path installs a thread-local decoder.

### `cargo xtask ghostty bump`

This is about 350–500 lines of Rust. Today only a 17-line signature check exists. It uses only `/usr/bin/curl`, `git`, `ditto`, `lipo`, `strip`, `ld` and `libtool`, plus the bindgen and minisign-verify 0.2.5 crates with the CLT's libclang.

1. **Get the signed asset.** Download the `tip` release's `ghostty-vt.xcframework.zip` and `.minisig` into a fresh temporary directory.
   - Verify the signature against `RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV` (Ghostty PACKAGING.md:30).
   - The signature's trusted comment names no commit.
2. **Find its commit.**
   - Start at `refs/tags/tip^{}`, then walk `main` from newest to oldest. For each commit, fetch `https://tip.files.ghostty.org/<sha>/ghostty-vt.xcframework.zip` until one is byte-identical to the signed asset.
   - Walking is needed because the asset can move ahead of the tag. The lib-vt job needs only `setup` (release-tip.yml:328), while the `tag` job also needs `build-macos` (:117).
   - If several commits match, they built the same binary, and step 4 picks the one whose headers match.
   - At the end, re-check that the signed asset has not changed.
3. **Require passing tests.** The Actions API must show the `Test` push run for `<sha>` concluded `success`. release-tip runs after Test completes whatever the outcome (release-tip.yml:1–5 has no conclusion check).
4. **Check the headers.** Unpack with `ditto -x -k`. `Headers/ghostty` must equal `git show <sha>:include/ghostty`.
   - Print the header diff since the last pin for a person to review.
   - A compile error catches only a renamed or removed name that the wrapper or DIE uses. Renumbered enums and changed callback contracts pass silently. Two examples since the wrapper's pin: `CONTEXT_SIGNAL` was renamed to `HIERARCHICAL_CONTEXT` (079f5d202), and `mode_set`/`get` were removed (cfc19e805).
5. **Derive the archive,** with `ZERO_AR_DATE=1` throughout.
   - `lipo -thin arm64`, then `strip -S`.
   - Hash every member. Fail if the set of members changes, because that needs a licence review.
   - `ld -r -arch arm64 -platform_version macos 13.0 13.0 -exported_symbols_list <_ghostty_*>`, then `libtool -static`.
   - Assert that only `_ghostty_*` symbols are global (205 today, 0 others) and that every undefined symbol is on a libSystem allowlist (40 today).
   - The output is deterministic: two runs were identical.
6. **Regenerate bindings** with bindgen 0.72.1 and `LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib`, using the wrapper's generator settings. The xtask calls bindgen as a library, so the wrapper's Zig-running build script is never involved.
7. **Record provenance.**
   - Write the lock file: commit, date, URLs, the signed and derived SHA-256s, the member hashes, a hash of the header tree, the Zig version and the wrapper rev.
   - Save the `.minisig` and `LICENSE`.
   - Diff `pkg/{highway,wuffs,simdutf}/build.zig.zon` and the uucode pin. Any change flags a review of the notices.
8. **Build and test.**
   - Run `cargo build`, then die-term's battery: the engine checks, the kitty key vectors with the shim, the yazi placeholder fixtures, the replies, and the stress and thread tests.
   - The wrapper's own tests cannot run as `cargo test -p libghostty-vt`, because cargo refuses to test a git dependency that is not a workspace member. The xtask runs them in a temporary checkout of libghostty-rs at the pinned rev, with the sys crate swapped for die-term-sys. Verified: 110 pass and 3 are ignored, or 115 pass and 3 are ignored with `--features png`.
   - If the wrapper does not compile against the new bindings, bump its rev or wait. Waiting longer than one monthly cycle is a fallback trigger (§1).
9. **Commit** everything as one commit. Rollback is `git revert`, which works offline.

**`--commit <sha>` for any commit that is not the signed tip is refused.** Per-commit blobs are unsigned: the `.minisig` at the per-commit path returns 404. Such a commit is allowed only when the optional CI rebuild (below) matches the lock's member hashes.

### Optional CI cross-check

This is about 60 lines of YAML. It runs only on a fallback trigger, and it is the only place Zig would ever appear.

- It builds `zig build -Demit-lib-vt -Doptimize=ReleaseFast` at the locked commit and compares the member hashes with the lock.
- Reproducibility evidence: the proposer's Zig build, run in the scratch directory, matched CI. I re-ran `cmp` on it: all 13 members are identical (12 object files plus the symbol table).

### Cadence and effort

- **Default:** bump to the signed tip once a month.
- **Bump immediately** for crash or security fixes in `src/terminal`, `src/lib_vt.zig` or `include/ghostty`.
- **Never fall more than 3 months behind.**
- **Archive availability.** Archives exist for about half of `main` commits, because release-tip.yml:103–115 skips commits that change nothing that gets built. The proposal measured a median gap of 4.3 h and a maximum of 36 h between archives. Older blobs are still served (a887df42 from Jul 11), but no retention policy is stated.
- **Effort:**
  - Steps 1–8 run in under a minute.
  - Plan on about 30–60 minutes of review per bump when nothing breaks. This is an estimate, not measured.
  - It takes longer when the C API changes meaning, or when the wrapper lags it. Today the wrapper's sys pin is 212 commits behind, including 15 that changed the headers.
- **Repository growth:** about 0.5–1 MiB per bump.
- **Versioned releases.** Switch to them if libghostty starts publishing them. Today there are none: release-tag.yml builds no lib-vt.

### Licence notices to ship

- Ghostty: MIT
- Zig compiler_rt: MIT
- wuffs: MIT and Apache-2.0
- simdutf: Apache-2.0 or MIT
- Highway: Apache-2.0 or BSD-3
- uucode: MIT, plus Unicode-3.0 and Bjoern Hoehrmann's licence

The archive contains precompiled C and C++ (wuffs, simdutf, Highway). Nothing is compiled through cc on your machine, and there is no C++ runtime dependency. ghostel is GPL-3.0: it was read for behaviour only, and nothing from it is copied.

## 6. Known gaps

**Compared with your ghostel 0.49 as configured:**

- **Title stack (CSI 22/23 t).** The engine drops it. Only ghostel 0.51 and later has it.
- **ghostel's private OSC 52;e.** The engine turns it into an ordinary clipboard write, so DIE needs its own channel.
- **Exit codes on prompt marks, OSC 7501, the reset effect and shell-to-editor commands.** These wait on the wrapper (§3). None of them works in your ghostel today either: Apple's bash 3.2 ignores `ENV`, so its bash integration never loads (verified).
- **Dropped:** middle-click primary selection (macOS has none), TRAMP terminfo push (D2), and line-spacing workarounds.

**Compared with kitty:**

| Gap | Effect |
|---|---|
| Engine-driven animation | An animated GIF shown with icat stays on its first frame. The engine's `animationTick` is not exported. Frame switches the client drives itself work. |
| OSC 99 notifications | Dropped before any callback fires. OSC 9 and 777 work. Ghostty issue #5634 is open. |
| OSC 66 text sizing | Dropped. Scope §5.1 has text sizing as "later, maybe". |
| File, temp-file and shm media | Off on purpose: `t=f` would let a remote program read local files. Tools fall back to direct transmission. |
| Legacy key encoding | Differs from kitty on 38 of 486 vectors. DIE's ~40-line shim fixes it. |
| Relative placement whose parent is a placeholder image | No geometry. Rare. |
| Remote control, kittens | Out of scope. |

## 7. Risks

| Risk | Mitigation |
|---|---|
| An engine panic is a process-wide `@trap` (lib_vt.zig:539–570). It kills all of DIE, unsaved buffers included | A PTY holder keeps the shells alive. Save the session often. Run the stress battery on every bump. Question 2 moves the engine out of DIE's process. |
| The CI build is ReleaseFast, so engine bugs can corrupt memory instead of trapping. Fuzzing has run only 60 s | Gate bumps on Ghostty's Test run plus DIE's battery. Ghostty ships the same build mode in its app. Question 2 isolates the engine. |
| Neither you nor DIE can patch the engine locally (no Zig). Every fix waits for upstream and a signed tip | Same as ghostel today. The CI-build fallback covers urgent fixes. |
| The C API is unstable: headers changed in 82 commits in 3 months, including removals | Regenerated bindings turn removals into compile errors, and a person reviews the header diff for changed meaning (§5, step 4). |
| Per-commit archives are unsigned, the signature names no commit, and the tag can lag the asset | Match the signed asset to a commit (§5, step 2). Refuse unsigned commits. |
| Ghostty renames or drops the xcframework, for example at 1.4 or at a libghostty release | The committed archive keeps DIE building. Bumps then use the CI-build fallback, or wait. |
| libghostty-rs: effectively one maintainer (83 of the last 100 commits; the second author's last commit was 2026-09-01); open soundness issues #70 and #101–#104; no raw handle; 4 callbacks missing; lags the C API | Pin a rev and never fork it. DIE installs its own global PNG hook (issue #104). Engine objects stay on the parser thread. Send the raw-handle and callback PR upstream. Switch to DIE's own layer on a §1 trigger. |
| Deadlock between the main thread and a parser thread | Jobs are asynchronous only. A clipboard prompt times out to deny (§2). |
| 854 bundled global symbols could collide with other libraries | Prelinking hides them; 0 remain. |
| Engine defaults would break the experience | G1 sets all of them and the battery asserts them. |
| Image memory: 557 MB RSS was measured at ghostel's 320 MiB cap | A 64 MiB cap per screen, configurable. |
| The snapshot format changes between engine versions and drops images | The snapshot envelope stores the pin. On a mismatch, reattach without scrollback and send SIGWINCH. Question 2 removes the dependency on snapshots. |

## 8. Proposed scope.md text

**D10 (replaces the current row):**

> | D10 | Kitty-level terminal buffers are built into DIE, in a `die-term` crate on top of Ghostty's libghostty-vt engine. The engine is linked as a prebuilt static library that Ghostty's CI builds and signs. `cargo xtask ghostty bump` verifies it, cuts it down, and commits it with a lock file. Building DIE needs only cargo and the Xcode Command Line Tools. DIE owns no engine code: `die-term` is glue (about 3.2k lines) that shows engine state through core's renderer and forwards keys and mouse. The Rust API is libghostty-rs at a pinned rev, never forked. The feature target is ghostel's (§6). | Proposed | It's the same engine and the same delivery model as ghostel, which uses a prebuilt binary with Zig only in upstream CI. The C API covers kitty keyboard, kitty graphics, mouse encoding and OSC callbacks, so DIE doesn't write a terminal. Cost: DIE can't patch the engine itself; fixes wait for a signed upstream build. |

**§6 (replaces the section body):**

> `die-term` is built into DIE and always ships with it. Internally it's a separate crate that uses only core's public API: the PTY, the cell grid, placements, passthrough and full key events. The terminal is Ghostty's engine, libghostty-vt, linked as a prebuilt library (D10). `die-term` shows the engine's state through DIE's renderer and forwards keys and mouse, so a terminal buffer feels like using the terminal directly. ghostel's feature list is the checklist, and kitty is the target wherever the engine allows:
>
> - True color and all underline styles. Box-drawing, block and powerline glyphs are drawn by DIE's renderer so they join across cells.
> - **Kitty keyboard protocol**, including key release and repeat
> - **Kitty graphics protocol:** direct placements (timg, `kitten icat`, and yazi under DIE's default identity) and Unicode-placeholder placements (`icat --unicode-placeholder`, and yazi when it detects Ghostty or kitty), shown through core's placements with alpha and z-order
> - **Mouse:** every tracking mode and encoding, including pixel coordinates. Shift-click selects even when an app tracks the mouse.
> - Synchronized output
> - Hyperlinks (OSC 8), plus plain URLs and `file:line`
> - Clipboard through OSC 52. Writes are on and reads are off by default. A yank in vim on a remote host over SSH lands on your Mac's clipboard.
> - Desktop notifications (OSC 9 and 777) and progress reports
> - Shell integration that works with macOS's `/bin/bash` 3.2: prompt navigation and directory tracking
> - Window and cell size queries (XTWINOPS) answered in physical pixels
> - Copy state over the screen and scrollback, and editing the shell's input line from normal state
> - Terminals survive DIE restarts (§5.9)
> - **SSH-friendly `TERM`.** Remote hosts often lack terminfo for newer terminals. So `die-term` defaults to `TERM=xterm-256color` and `COLORTERM=truecolor`, and identifies itself as DIE, not Ghostty, in `TERM_PROGRAM` and XTVERSION. Apps detect the extra features through terminal queries. Presenting as `xterm-ghostty` is an option for local use.
>
> **Known gaps, until Ghostty adds them:** looping image animation (an animated image shows its first frame), kitty notifications (OSC 99), text sizing (OSC 66) and the title stack. DIE can't patch the engine itself.

(Historical: this referred to the pre-review §13, whose libghostty question it answered. The text above was adopted into scope.md with small changes.)

## 9. Questions for you

1. **Is a committed, prebuilt engine binary acceptable as the price of ghostel-level terminals?**
   - It is signed by Ghostty's CI, checked by DIE's xtask, and refreshed about monthly.
   - You never need Zig, and DIE owns no engine code.
   - Neither you nor DIE can patch the engine. A terminal bug waits for an upstream fix and a signed build. Looping animation, OSC 99 and OSC 66 stay missing until Ghostty adds them.
   - If not, the alternative is alacritty_terminal plus a DIE-written kitty layer: about 10k DIE lines including tests, partly rewriting a terminal.
2. **Should terminals run inside DIE, or in a small helper process, one per terminal?**
   - **Inside DIE** is simplest. An engine crash takes DIE down, unsaved buffers included, while a PTY holder keeps the shells alive. A rebuild restores scrollback only when the engine version is unchanged, and images are always lost.
   - **A helper process** (`die-termd`) owns the PTY and the engine. A crash loses only that one terminal. Scrollback and images survive every DIE rebuild, with no snapshots. It costs an estimated 0.5–1k more lines and a protocol between DIE and the helper that has to stay compatible across rebuilds.
   - I lean towards the helper. A short spike before M2 would measure the frame and image transfer cost and settle it.

**Calls I'll make unless you veto them:**

- **Identity:** keep scope §6 as written:
  - `TERM=xterm-256color`, `COLORTERM=truecolor` and `TERM_PROGRAM=DIE`
  - XTVERSION "DIE \<version\>"
  - the terminal name XTGETTCAP reports matches `TERM`

  XTVERSION must not contain "ghostty". The engine's default "libghostty" does, and yazi matches it as a substring (yazi-emulator/src/brand.rs:34–50). That moves yazi onto the placeholder path, while a generic identity keeps it on direct placements (drivers.rs:24–26). An `xterm-ghostty` option stays available for local use.
- **Binary source:** Ghostty's signed tip builds only. A Zig CI job is added only on a §1 trigger.
- **Wrapper:** libghostty-rs, pinned and never forked. I'll send the raw-handle and callback PR upstream. If shell-to-editor commands are wanted sooner, they go through a local Unix socket, as emacsclient does. That works locally but not over SSH.
- **Defaults:**
  - scrollback of 10k lines, capped at 64 MB
  - an image cap of 64 MiB per screen
  - mode 2027 on and Option as Meta on
  - OSC 52 write on and read denied
  - file and shm media off
  - shift-click override on
- **Shell start:** `bash --rcfile die.bash -i`. `die.bash` sources `/etc/profile`, `~/.profile` and `~/.bashrc`, then DIE's integration.

---

**Corrections made after review:**

- Placeholder placements do get a size from the C API; only the position is missing.
- The wrapper gaps block exit codes, OSC 7501, the reset effect and the command channel, not just exit codes. There is now a no-fork policy with triggers.
- The fallback is +1.2–4k lines, not 1k.
- libghostty-rs has effectively one active maintainer, not two.
- The header diff is not purely additive, and compile errors catch only names in use.
- Signatures are tied to a commit only through the byte match with the signed tip. Unsigned commits are refused, and a tag that lags the asset is handled.
- The identity argument is reversed: a generic identity puts yazi on direct placements.
- The exact wiring (no default features, global PNG hook) is unbuilt and is the first spike.
- `cargo test -p libghostty-vt` runs in a temporary checkout, not in the workspace.
- §4 now counts tests, generated bindings and tooling.
- The 0.53 ms frame cost was not reproduced; the reviewer measured 0.11–0.32 ms on the engine side.
- `CURSOR_PASSWORD_INPUT` has no C setter, so it does not simplify password detection.

**Evidence** (all read-only; nothing under /Users/user/workspace/die or ~/.emacs.d was modified):

- Battery through the `[patch]` wiring (81 pass / 5 known gaps): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/proposal/patchtest/battery.out
- Raw-FFI battery (76/76): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/run/battery-final.out
- Cold build with the fake `zig`: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/run/coldbuild-final.log
- Wrapper wiring without Zig: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/patchws
- Wrapper tests in a temporary checkout: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/lgrs-test
- Wrapper's private terminal handle (line 228): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/dl-lgrs/repo/crates/libghostty-vt/src/terminal.rs
- Engine headers (screen.h, render.h, terminal.h): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/x/ghostty-vt.xcframework/macos-arm64_x86_64/Headers/ghostty/vt/
- Ghostty tip workflow: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/constraints-critic/dl-wf/release-tip.yml
- Where Ghostty's app sets the password flag (lines 1444–1447): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/technical-skeptic/src-ghostty/src/Surface.zig
- Prelinked archive: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/distribution/prelink/libghostty-vt.a
- Reproducibility (`cmp` re-run, 13/13 identical): /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/distribution/arcmp
- yazi driver choice: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/final/dl-yazi/drivers.rs
- yazi brand detection: /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/ghostel-model/final/dl-yazi/brand2.rs
- Checked live on 2026-10-19:
  - `git ls-remote`: Ghostty tip and main are both 765ea37ad64f; libghostty-rs master is d2036bae.
  - GitHub API: libghostty-rs PR #92 is still open.