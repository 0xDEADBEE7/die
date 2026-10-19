> **Decision record for spike S2 (scope D10 and D20), 2026-10-19.** Status: **accepted 2026-10-19** (see below). Evidence paths under `/private/tmp` are scratch files and may not last.

# S2: terminal engine wiring and helper process

Spike code: `spikes/s2-term-engine/` (a standalone cargo project; throwaway).
Engine pin: Ghostty 765ea37ad64f, the signed `tip`. Wrapper: libghostty-rs `d2036bae`.
Machine: Apple M5 Pro, 18 cores, rustc 1.96.0, Xcode Command Line Tools only.

> **Status: accepted 2026-10-19.** Your answer to .questions/06 chose the helper if it was fast enough, and it is. The proposed edits in §1.3 are folded into scope.md and extension-contract.md. The calls in §4.3 stand unless you veto them.

## 1. Verdicts

### 1.1 A. DIE's exact wiring: builds and passes

DIE's exact configuration is now built. That was the one unbuilt gap in [die-term-engine.md](die-term-engine.md) §5.

- **The configuration.** libghostty-rs at `d2036bae` with `default-features = false` and `features = ["kitty-graphics"]`. A `[patch]` swaps its sys crate for `die-term-sys`. DIE installs its own PNG decoder once, process-wide, through `die-term-sys`.
- **The archive.** The minisign signature on the tip zip checks out. Re-deriving the archive from the signed zip gives the same bytes as before (SHA-256 `c819c8d6…fa37`). The lock file records it.
- **The battery.** 81 of 86 checks pass. The 5 failures are the same 5 known gaps as before. Six new wiring checks and four tests also pass.
- **The build.** It runs under `env -i` with a fake `zig` first on PATH. Zig is never called, and no C is compiled through `cc`.
- **What the binary links.** The wiring on its own links only libSystem. The spike's full binary also links `/usr/lib/libiconv.2.dylib`. That comes from the `libc` crate, not the engine (§4.1).

### 1.2 B. Helper process: yes

Run each terminal's engine in a helper process. It met every gate (§2.2), and on most numbers it is within noise of a thread inside DIE.

- **Typing.** The helper adds 17.5 µs at p99, from key to echoed frame through a real `/bin/bash`. The gate allowed 1 ms.
- **Full-screen redraws** (250×70, 60 fps). The helper adds 0.43 ms at p99, PTY write to frame available. The gate allowed 1 ms.
- **Output flood.** With 200 MB of build-log text, DIE's main thread spends at most 108 µs per vsync. The gate allowed 1 ms.
- **Images.** A 4K RGBA image reaches DIE in 2.1 ms after the engine has it. The pixels travel as shared memory. The socket carries a 49-byte message.
- **Idle.** Over 60 s there are no wakeups in DIE's loop, none in the helper's loop, and no CPU time in the helper.
- **Memory.** A helper uses 1.9 MB, or 2.6 MB when it links AppKit, Metal, QuartzCore and CoreText as DIE will. A thread inside DIE costs 0.7 MB. So the overhead is about 1.2–1.9 MB per terminal.
- **Crashes.** `kill -9` of a helper costs that terminal only; with the holder, only its screen and scrollback. `kill -9` of DIE loses nothing: a new DIE process reattaches with the screen, scrollback and image intact, and with output that arrived while DIE was dead. A helper from build N served DIE build N+1.
- **Cost.** The helper-only code is about 1.05k lines (socket, shared memory, codec, handshake). The thread-only code is 0.12k. That is 0.94k more, at the top of die-term-engine.md's 0.5–1k estimate.

**Fit with S3's PTY holder.** Keep S3's design, one holder for every PTY, and add one helper per terminal that takes its PTY from the holder. This is S3's `term/<id>/pty` label.

- **A helper crash** loses only that terminal's screen and scrollback. The holder's copy keeps the shell alive, and a new helper adopts the PTY. The spike demonstrated this (§2.3). Without the holder, a helper crash kills that terminal's shell.
- **A holder crash while DIE is down** no longer hangs up every shell, because each helper holds a copy too. S3 noted this.
- **While DIE is down,** helpers keep reading. Programs don't pause after S3's 1 KB of kernel buffer, and their output lands in scrollback.

### 1.3 Proposed edits (not made)

These target scope.md and extension-contract.md as they stand after S3's accepted edits.

**scope.md D20** (replaces the row):

> | D20 | Restart: everything survives, crashes included. A holder process keeps PTYs open. Each terminal's engine runs in a helper process, DIE's own binary in a second mode (`die --term-helper`), which takes its PTY from the holder and outlives DIE. The session saves unsaved text, scratch buffers, registers, macros and histories (§5.9). | Decided (2026-10-19); holder confirmed by S3, helper by S2 | Rebuild-and-restart is your extension loop. A bad build mustn't cost you live Claude Code or SSH sessions. S2 measured the helper within 0.5 ms of a thread inside DIE, with images crossing as shared memory ([records/s2-term-engine.md](records/s2-term-engine.md)). |

**scope.md guard 7.** Replace "plus up to about 1k for the helper process if spike S2 keeps it" with "plus about 1k for the helper process (S2's prototype: 1.05k)".

**scope.md §5.8** (replaces the "Terminal output is parsed off the main thread" bullet):

> - **Terminal output is parsed in each terminal's helper process** (D20). DIE's main thread receives finished frames and never waits on a helper.

**scope.md §5.9.** In "Terminals survive crashes", replace the sub-bullet "While no DIE is running, the kernel keeps about 1 KB…" with:

> - While no DIE is running, each terminal's helper keeps reading, so programs don't pause and their output lands in scrollback.

Then replace the bullet "The terminal engine runs outside DIE" with:

> - **Each terminal's engine runs in a helper process**, DIE's own binary in another mode (`die --term-helper`), one per terminal. The helper takes the terminal's PTY from the holder, parses its output, and sends DIE finished frames over a Unix socket. Images travel as shared memory, never through the socket.
>   - **DIE crashes or restarts:** helpers keep running. The new DIE reaches each helper through the holder and gets the screen, scrollback and images back as they were, with no snapshot.
>   - **A helper crashes:** that terminal loses its screen and scrollback, nothing else. The holder's copy keeps the shell alive, a new helper takes the PTY, and die-term makes full-screen programs redraw as above.
>   - **The holder crashes while DIE is down:** the helpers' copies keep the shells alive.
>   - **Old helpers, new builds:** a helper started by an older build serves every newer DIE with the same protocol major version. An incompatible pair is refused cleanly, and the terminal stays alive and detached until you replace its helper.
>   - Spike S2 measured all of this ([records/s2-term-engine.md](records/s2-term-engine.md)).

**scope.md §12.** S2 row: keep the question, and replace the fallback cell with:

> Not needed: S2 passed on 2026-10-19 ([record](records/s2-term-engine.md)).

Below the table, add: "S2 then built DIE's exact wiring (81 of 86, the same 5 known gaps) and chose the helper process (D20)."

In the crate list, change die-term's line to "terminal surface; die-core + libghostty-rs; also the helper process (`die --term-helper`)".

**scope.md §13.** Replace open question 1 with:

> 1. ~~Terminal engine in a helper process (D20).~~ **Settled by S2 (2026-10-19):** each terminal's engine runs in a helper process ([records/s2-term-engine.md](records/s2-term-engine.md)).

Also add "and where the terminal engine runs (S2)" to the "Settled in the 2026-10-19 review" sentence. Questions 2 and 3 keep their numbers, so outside references stay valid.

**extension-contract.md §12** (three edits):

- The `pty(cmd, size)` row: "Used by `die-term`, which hands the PTY to the terminal's helper process, so its output never passes through DIE."
- The output bullet:

  > - A terminal's output is parsed in its helper process and reaches the main thread as finished frames: the changed rows as DIE cells, the placements, and images as shared memory (scope §5.8). Other PTY users get main-thread callbacks like `process`.

- The last sentence of the paragraph after the list: "The main thread never waits on a terminal's helper either."

die-term-engine.md §5 calls the exact configuration "the one gap"; this record closes it. That record isn't edited here.

## 2. Pass criteria

### 2.1 A: wiring

| Check | Measured | Result |
|---|---|---|
| Signature on the tip zip (minisign-verify 0.2.5, Ghostty's key) | Verifies. A copy with one flipped byte is rejected. | Pass |
| Signed zip | SHA-256 `e526d23f99bcd24e6834ec893b809453b9388681906e5786fdd22f250aee1448`, 14,446,276 bytes | Pass |
| Archive derived from the signed zip with CLT tools only | Byte-identical to the earlier archive: SHA-256 `c819c8d606eb5136b1441bc46df4f623838a377e256d924ece5683e9ba94fa37`, 2,696,424 bytes. 205 global symbols, all `_ghostty_*`. 39 undefined symbols, all libSystem. | Pass |
| Headers in the zip vs `git show 765ea37:include/ghostty` | Identical | Pass |
| Bindings regenerated with bindgen 0.72.1 and the wrapper's settings | 5,577 lines, byte-identical to the earlier bindings | Pass |
| `die-term-sys` build script | 14 lines. Checks the SHA-256, emits link flags. No network, git or Zig. | Pass |
| Resolved features | The wrapper builds with `kitty-graphics` only. The sys crate in the graph is `die-term-sys`. (test) | Pass |
| DIE's PNG hook is process-global | Installed once on the main thread. All 34 PNGs in the battery, on 10 parser threads, decoded through it. A fresh thread with no install call decodes too. (battery + test) | Pass |
| No wrapper path installs a thread-local decoder (issue #104) | Source audit (test): the engine's PNG callback is set in exactly one place, inside `set_png_decoder` (kitty/graphics.rs:959). Nothing in the wrapper's library code calls it. Its only `thread_local!` is the decoder slot (line 893). | Pass |
| Negative control (test) | After the wrapper's `set_png_decoder` on one thread, a PNG on another thread is rejected. `set_png_decoder(None)` removes DIE's hook for the whole process. Reinstalling DIE's hook fixes every thread. | As expected |
| Battery, against this exact configuration | **81 pass, 5 fail.** The earlier run was also 81/86. | Pass |
| The 5 failures | Legacy ctrl+[, ctrl+i and shift+enter (the ~40-line key shim's job), OSC 99, OSC 66. The same 5 known gaps as before. | Same |
| S2 wiring checks (new) | 6 of 6: the global hook (3 checks), grey+alpha expansion, a PNG over the storage limit refused (ENOMEM), and a forged 20000×20000 PNG header refused by the engine's allocator before any large allocation | Pass |
| `env -i` cold build, fake `zig` first on PATH | Built in 12.9 s. Zig was never called. | Pass |
| C compiled through `cc` | None. There is no `cc` crate in Cargo.lock. | Pass |
| `otool -L` | The wiring alone (`tools/wiring-only`, same battery source): libSystem only. The spike binary: libSystem and libiconv (from the `libc` crate, §4.1). | Pass for the wiring; flagged |

### 2.2 B: the proposed gates

The latency and flood numbers pool five interleaved runs per shape (`data2`). The image, idle and memory numbers come from `data1`. All numbers were measured.

| Gate | Proposed | In-process | Helper | Result |
|---|---|---|---|---|
| Added p99 latency, w1 typing | ≤ 1 ms | key → frame: p50 73.8 µs, p99 154.3 µs (2,500 samples, 0 missed) | p50 95.8 µs, p99 171.9 µs (2,500, 0 missed) | **+17.5 µs. Pass** |
| (same, PTY read → frame available on DIE's main thread) | | p50 21.8 µs, p99 48.8 µs | p50 31.2 µs, p99 57.3 µs | +8.5 µs |
| Added p99 latency, w3 TUI redraw, 250×70 at 60 fps | ≤ 1 ms | PTY write → frame available: p50 1.71 ms, p99 2.43 ms (4,996 frames) | p50 1.81 ms, p99 2.86 ms (4,997 frames) | **+0.43 ms. Pass** |
| Main-thread ingest per frame, w2 200 MB flood | ≤ 1 ms | p99 15.7 µs, max 54 µs; 0 of 701 vsyncs over 1 ms | p99 52.2 µs, max 108 µs; 0 of 758 over 1 ms | **Pass** |
| 4K RGBA image handoff, engine has it → DIE has it | ≤ 5 ms, no pixels through the socket | median 1.83 ms, max 1.95 ms (5 runs) | median 2.08 ms, max 2.13 ms (5 runs). Pixels go by shm fd; the message is 49 bytes. | **Pass** |
| Wakeups while idle, 60 s at a bash prompt | 0 | DIE loop 0, parser thread 0 | DIE loop 0, helper loop 0; helper process 0 kernel wakeups, 0.000 ms CPU | **Pass** |
| Helper memory overhead per terminal, without scrollback | ≤ 20 MB | 0.74 MB per terminal (DIE's footprint grows by that) | 1.94 MB footprint (3.1 MB resident); 2.60 MB linking DIE's frameworks (9.8 MB resident, mostly shared framework pages). DIE's side: 0.41 MB. | **+1.2 to +1.9 MB. Pass** |

"Footprint" is `phys_footprint`, what Activity Monitor calls Memory. w3 measured 4,996 and 4,997 of 5,000 frames: a run's last frame lands after its end marker, and nothing was coalesced.

**Other measurements (no gate):**

| | In-process | Helper |
|---|---|---|
| w3 main-thread ingest per vsync | p50 27 µs, p99 56 µs, max 415 µs | p50 90 µs, p99 156 µs, max 337 µs |
| w3 at 1,000 fps (stress) | 120 frames shown for 1,000 written; 873 coalesced; main thread max 39 µs per vsync | 120 shown, 877 coalesced; max 249 µs |
| w2 throughput, PTY to DIE | 146–192 MiB/s per run | 112–183 MiB/s per run (one slow run in each shape) |
| Frames during the flood | about 121 per second: one per vsync, so coalescing works | the same |
| Socket traffic | none | 420 KB per full 250×70 frame (24 bytes per cell); 421 MB for 1,000 frames; 15.9 MB for the 200 MB flood |
| 1080p PNG | DIE's hook decodes in 15.4 ms on the parser thread; handoff 0.21 ms | decode 15.8 ms in the helper; handoff 0.63 ms |
| 4K image: engine-side copy | 1.82 ms into a fresh heap buffer | 2.00 ms into a fresh shm object |
| 4K image: DIE main-thread time for that wake | 2.4 µs | 26 µs (includes `mmap` and a frame) |
| Copies of the image | 1, on the parser thread | 1, in the helper |
| Scrollback fetch, 10k lines as text (copy state's snapshot) | p50 2.1 ms, max 3.1 ms | p50 2.3 ms, max 3.4 ms |
| Memory with 10k lines of scrollback | DIE grows 10.4 MB per terminal | helper 11.9 MB; DIE unchanged |
| Code that only this shape needs | 117 lines | 1,054 lines (helper 478, codec 386, handshake 76, shm and fd passing 114) |

Shared by both shapes: 987 lines (engine glue 431, host loop 172, message types 122, DIE-side surface 111, PNG hook 81, PTY 53, interface 17).

### 2.3 B: the helper demonstrations

| Demo | What happened | Result |
|---|---|---|
| (i) `kill -9` of one helper | DIE noticed in 1.5 ms and reported "terminal t1 lost; helper ended by signal 9". DIE kept running. The other terminal still echoed. t1's shell got SIGHUP and died, because the helper held the only copy of its PTY. | Pass |
| (i') The holder layout | DIE kept a copy of t3's PTY, standing in for the holder. After `kill -9` of t3's helper, the shell stayed alive. A new helper adopted the PTY and the shell answered. The screen restarted empty. | Pass |
| (ii) `kill -9` of DIE, then a new DIE | The helper and shell kept running. During the 4 s with no DIE, the shell printed more output. The new process attached and had the full state in 0.35 ms. Scrollback had all 3,208 lines, including the output made while DIE was dead. The image matched byte for byte and was not copied again. The shell answered. | Pass |
| (ii') Helper from build N, DIE build N+1 | Two separately built binaries. The helper started by build N served build N+1, with the same checks as (ii). | Pass |
| (iii) Version handshake | The helper speaks 1.1. DIE 1.2 attached at 1.1. DIE 1.0 attached at 1.0. DIE 2.0 and 0.9 were refused with a clear message on both sides, and the helper and its shell kept running. A compatible DIE attached afterwards. | Pass |

**The versioning approach:**

- **A frozen handshake.** `Hello` and its reply have a fixed layout that never changes: a magic string, major and minor versions, the build id, and the helper's and shell's pids. Any two builds can always tell each other what they speak.
- **After the handshake,** messages are `length | tag | payload`.
  - The same major version means compatible. Both sides use the lower minor and send only what exists in it.
  - Unknown tags are skipped by length.
  - Adding a message or a trailing field bumps the minor. Anything else bumps the major.
- **On a different major,** DIE shows the terminal as held by an incompatible helper and leaves it running. With the holder, "replace helper" adopts the PTY in a new helper: the shell survives and the scrollback is lost.

## 3. What was built and how to rerun

**Built** (in `spikes/s2-term-engine/`):

- **`crates/die-term-sys`:** the record's layout. The vendored archive and its `.sha256`, regenerated `bindings.rs`, `lib.rs` from the wrapper with attribution, a 14-line `build.rs`, `ghostty-vt.lock`, the `.minisig`, `LICENSE-ghostty` and `THIRD_PARTY_NOTICES`.
- **`src/hook.rs`:** DIE's process-global PNG decoder (G10), 81 lines.
  - It uses the pure-Rust `png` crate.
  - It allocates from the engine's allocator before decoding, so the engine's cap applies.
  - It catches panics.
  - It counts refusals by stage.
- **One binary, `s2`, with several modes:**
  - `--term-helper` is the helper.
  - `bench` and the demos play DIE's main thread: a poll loop with a simulated 120 Hz vsync that returns frame credits.
  - `gen` writes the deterministic workloads onto the PTY.
- **The two shapes,** behind one interface (`client.rs`):
  - `inproc.rs`: a parser thread per terminal with channels and a wake pipe.
  - `helper.rs`, `shm.rs` and the codec in `proto.rs`: the socket, SCM_RIGHTS, shm images, the handshake and reattach.
- **Shared by both shapes:**
  - The engine side (`engine.rs`, `host.rs`). Dirty rows become 24-byte DIE cells (character or grapheme, fg, bg, attributes, underline style and colour, link id, wide flag). It also produces placements and image uploads. At most one frame is in flight; DIE returns the credit at vsync.
  - DIE's row cache (`surface.rs`).
- **The battery** (`src/bin/battery.rs`): the earlier 86 checks unchanged, plus 6 S2 checks. Tests are in `tests/`.
- **Tools:** `tools/verify` (signature and hashes), `tools/gen-bindings` (bindgen), `tools/wiring-only` (the wiring without the `libc` crate).

**Rerun.** Set `W=/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s2`, `SPIKE=/Users/user/workspace/die/spikes/s2-term-engine` and `CH=$W/downloads/cargo-home`.

```sh
# A1: re-derive the archive from the signed zip, verify the signature and hashes
sh $SPIKE/scripts/derive-archive.sh $W/downloads/tip-765ea37/ghostty-vt.xcframework.zip $W/derive
(cd $SPIKE/tools/verify && CARGO_HOME=$CH CARGO_TARGET_DIR=$W/target-tools cargo build --release)
$W/target-tools/release/s2-verify $W/downloads/tip-765ea37/ghostty-vt.xcframework.zip \
  $W/downloads/tip-765ea37/ghostty-vt.xcframework.zip.minisig $W/derive/libghostty-vt.a
# A2: regenerate the bindings (dev-only tool)
(cd $SPIKE/tools/gen-bindings && GEN_BINDINGS_RS=$(ls -d $CH/git/checkouts/*/d2036ba)/crates/libghostty-vt-sys/tools/gen_bindings.rs \
  LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib CARGO_HOME=$CH CARGO_TARGET_DIR=$W/target-tools cargo build --release)
LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib GHOSTTY_INCLUDE_DIR=$W/derive/x/ghostty-vt.xcframework/macos-arm64_x86_64/Headers \
  CARGO_MANIFEST_DIR=$SPIKE/crates/die-term-sys $W/target-tools/release/s2-gen-bindings
# A3-A5: fetch, then the env -i cold build with a fake zig, battery, tests, otool
(cd $SPIKE && CARGO_HOME=$CH cargo fetch)
sh $SPIKE/scripts/clean-build.sh $W/final/partA $CH
# B: build both variants, run every workload, then the extra repetitions
(cd $SPIKE && CARGO_HOME=$CH cargo build --release --offline)
(cd $SPIKE && CARGO_HOME=$CH CARGO_TARGET_DIR=$W/target-fw cargo build --release --offline --features link-frameworks --bin s2)
sh $SPIKE/scripts/run-measurements.sh $SPIKE/target/release/s2 $W/target-fw/release/s2 $W/data1
sh $SPIKE/scripts/run-reps.sh $SPIKE/target/release/s2 $W/data2 5
/usr/bin/python3 -I $SPIKE/scripts/summarize.py $W/data1/results.jsonl
/usr/bin/python3 -I $SPIKE/scripts/pooled.py $W/data2/results.jsonl
# B: the demos (helper kill + holder layout, DIE kill + reattach, build N -> N+1, versions)
sh $SPIKE/scripts/run-demos.sh $SPIKE $W/final/demos $CH
```

**One workload by hand:**

```sh
s2 bench --shape inproc|helper --workload w1|w2|w3|w4|w5|rss|scrollback --run-dir DIR
```

w4 also takes `--kind rgba4k|png1080`.

**Notes on the run:**

- Every download went into `$W/downloads`: the crates and the libghostty-rs git checkout through `CARGO_HOME`. The tip zip was copied there from the earlier check, and nothing else was downloaded.
- Summaries use `/usr/bin/python3` from the Command Line Tools. It is a dev-only tool.

## 4. Surprises, risks, and calls for you

### 4.1 Surprises

- **`libiconv` in `otool -L`.** The `libc` crate marks its old iconv bindings `#[link(name = "iconv")]`, so any binary that uses `libc` links libiconv. The engine and the wrapper don't.
  - libiconv ships with macOS, so it isn't a second toolchain.
  - Still, "links only libSystem" holds only for the wiring alone.
  - Linking with `-Wl,-dead_strip_dylibs` removes it (verified on the spike binary).
- **The wrapper's `set_png_decoder(None)` clears DIE's hook for the whole process.** One stray call anywhere would turn off PNG images in every terminal. A test pins that the wrapper never calls it internally. Rerun it on every wrapper bump.
- **The decoder's memory cap is not the image storage limit.** A PNG over the storage limit is fully decoded, then refused with ENOMEM. A forged header claiming 1.6 GB is refused by the engine's allocator before anything large is allocated.
- **`default-features = false` plus `kitty-graphics` is the wrapper's default feature set.** The real change from the earlier verified build is dropping `png` and the wrapper's per-thread decoder.
- **The helper is not slower for typing,** within noise: run-to-run spread (per-run p99 from 88 to 224 µs) is bigger than the difference between the shapes. It costs the main thread about 3× more per frame, because it decodes bytes off a socket. That is still at most 0.34 ms.
- **39 undefined symbols, not 40.** This is a different counting tool (`nm -u`, unique names), not a different archive: the archive is byte-identical.

### 4.2 Risks

| Risk | Mitigation |
|---|---|
| The timings ran under CPU contention: load average 3.4–7.3 on 18 cores, with other spikes and an unrelated process at 99% CPU | The shapes were interleaved, and w1, w2 and w3 pooled 5 runs each. The run-to-run spread is reported. |
| The spike's "DIE" is a poll loop, not winit and AppKit, and not a `.app` | Waking the main loop through the run loop costs the same in both shapes. M2 should repeat w1 and the reattach demo in DIE.app. |
| Untested: whether macOS throttles helpers (App Nap, timer coalescing) while DIE is hidden | Helpers are driven by fd events, not timers, but this is unmeasured. Check in M2. |
| A helper outlives DIE by design, so if DIE never comes back, its shell runs on | DIE lists detached terminals at start. The holder can report orphans. |
| If DIE hangs, the helper's outgoing queue grows | Frames are capped at one in flight and images go as fds. Effects (titles, notifications) can still pile up. Coalesce titles and cap the queue. |
| Old helpers keep their old engine until their terminal closes | Intended: no snapshot is needed. But an urgent engine fix needs "replace helper" (§4.3). |
| Copy state "follows new output" (scope §5.5). Refetching all scrollback costs 2.3 ms and 480 KB at 10k lines | Fetch only lines added since the last snapshot. |
| A new compatibility surface: every die-term message change needs version discipline | The frozen handshake plus the rules in §2.3. The demos are the regression test. |
| The spike reattached by socket path, not through S3's holder labels, and it spawned with std's `Command`, not `posix_spawn` | The attach itself (handshake, full resync, image fds) is the same either way. But a stream can't simply be handed from a dead DIE to a new one: the dead DIE may have read half a message. So the holder should keep a small "connect" channel to the helper under `term/<id>/engine`. Each new DIE passes the helper a fresh socketpair over it, and frames never cross the shared channel. Neither this nor `posix_spawn` was built here; S3 built both halves. |
| Socket paths are limited to 104 bytes (S3 hit this too) | Going through the holder needs no path. The spike used paths relative to a run directory. |

### 4.3 Calls I'll make unless you veto them

- **Layout.** This follows S3's accepted design.
  - `pty()` starts the shell with `posix_spawn` and registers the PTY with the holder.
  - die-term starts one helper per terminal (`die --term-helper`), which takes `term/<id>/pty` from the holder.
  - The helper deposits a connect channel as `term/<id>/engine`. Each DIE attaches through it with a fresh socketpair.
  - Exit codes come from DIE's kqueue watch (S3), so the helper doesn't need to be the shell's parent.
  - On a helper crash, DIE starts a new helper on the held PTY. The screen and scrollback are lost; the shell is not.
- **Frames.** Changed rows only, as 24-byte cells. At most one frame in flight; DIE returns the credit at vsync. No shared-memory frame ring for now: socket transfer measured fine.
- **Images.** One shm object per image (`shm_open`, then unlink, then send the fd), mapped read-only by DIE. The helper keeps it, so a reattach resends the fd without copying.
- **Clipboard.** OSC 52 reads are denied inside the helper, inside the engine's synchronous callback. Writes reach DIE as effects. DIE never answers anything synchronously.
- **Versioning.** The frozen handshake and the minor/major rules in §2.3. "Replace helper" is a manual command. DIE never does it on its own, because it loses scrollback.
- **Linking.** DIE's app crate links with `-Wl,-dead_strip_dylibs`, so the clean-environment check can assert exactly libSystem plus the system frameworks.

There are no open questions for you. Your answer to [.questions/06](../../.questions/06-restart-guarantees.md) already chose the helper if it was fast enough, and it is.

## 5. Raw data

Paths are relative to `/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s2/`.

- Part A, final run (cold build, fake zig, battery, tests, otool, wiring-only): `final/partA/clean-build.log`, `final/partA/battery.out`, `final/partA/battery-wiring-only.out`
- Archive re-derivation: `derive/` (members in `derive/members.sha256`, symbols in `derive/globals.txt` and `derive/undefined.txt`); lock file `spikes/s2-term-engine/crates/die-term-sys/ghostty-vt.lock`
- Every workload on both shapes, with load averages: `data1/results.jsonl`, `data1/summary.txt`, `data1/log.txt`
- Five interleaved repetitions with raw samples: `data2/results.jsonl`, `data2/pooled.txt`, `data2/summary.txt`, `data2/log.txt`
- Demos, final code: `final/demos.log`; helper logs in `final/demos/*/`
- The signed zip and its `.minisig`: `downloads/tip-765ea37/`
- The cargo home with every downloaded crate and the wrapper checkout: `downloads/cargo-home/`
