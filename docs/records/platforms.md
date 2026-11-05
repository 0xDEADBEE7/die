> **Decision record for scope D5, D12 and D13, with changes to D1, D10, D15, D16, D17, D18 and D20.** Your answer is in [.questions/11](../../.questions/11-platforms.md). §7's edits are folded into the docs, adjusted for the two approvals you declined.

# Platforms: macOS and Linux, and a verdict on Windows

Date: 2026-10-19.
Status: **accepted 2026-10-19.** Question 11: A, Linux ships as a preview at M2's exit. Windows is out. You approved the Linux std downloads and starting colima with a pinned `rust:1.96.0` image (§6 items 3 and 5). You declined asking Ghostty from your account for signed Linux archives (path A, §6 item 7) and DIE's CI building one (path B). So D16 keeps its rule with only the platform-toolchain wording, and where a signed Linux archive comes from is open until before M2. The proposed edits in §7 are folded into scope.md, extension-contract.md, m0-plan.md and the S1 and die-term-engine records, without path B's D16 clause.

Evidence notes:
- `registry:` means `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- `scratch:` means `/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/`. Scratch files may not last.
- Machine: Apple M5 Pro, macOS 26.5, rustc 1.96.0. The Linux cross-checks used `cargo +stable` (also 1.96.0), because the pinned toolchain has only the macOS std. Nothing was installed. Builds ran `--offline`.
- Each claim is marked **verified** (read in source or measured), **likely** (from documentation or project reports) or **unverified**.

## 1. The decision and the questions left

**What you decided.**
- "I want to at least maintain support across unix platforms such as linux etc."
- "If its not difficult to support all then we can include windows but if windows would be a considerable deviation then we can drop it."

So:

| Platform | Status |
|---|---|
| macOS, aarch64 | Required. The daily driver. |
| Linux: Wayland and X11, x86_64 and aarch64 | Required. |
| FreeBSD and other Unixes | Welcome where they come cheaply. |
| Windows | Only if it isn't a considerable deviation. |

**What this record proposes** (each is listed for veto):
1. **Windows is out.** It would be a considerable deviation (§3).
2. **FreeBSD is compile-checked only.** No terminals, no promise.
3. **OS-specific code lives in three places:** die-core's `sys` module, die-render's platform modules and portable backend, and die-term-sys's choice of archive. Unix is the floor, so `std::os::unix` is fine in neutral code.
4. **macOS keeps S1's measured path:** Metal and Core Text. **Linux gets a portable backend:** wgpu plus a pure-Rust text stack (fontations).
5. **D16 becomes** "cargo plus the platform's standard C toolchain, used only as the linker driver".

**Left for you:**
- **Question 11: when Linux ships.** At M2's exit (recommended), on par from M0, or in its own milestone after M2.
- **Three yeses, needed under any answer:**
  1. The Linux std downloads for the pinned toolchain, and starting colima with a pinned Rust image.
  2. Asking Ghostty, from your account, to publish signed Linux lib-vt archives.
  3. Whether DIE's own CI may build the Linux archive from Ghostty's signed source if upstream hasn't published one by M2. That changes D16's wording.

  *Answered 2026-10-19: 1 approved, 2 and 3 declined. Where the Linux archive comes from is open until before M2.*
- **Later:** Linux hardware with a display, for the "supported" label (§5.1).

## 2. What the research found

### 2.1 Rendering and text

| Finding | Source | |
|---|---|---|
| D13's "wgpu stalls the main thread (about 5.9 ms p99, measured)" didn't come from S1. It came from the 2026-10-18 review probe. S1 never built wgpu: its fallback table marks "wgpu with buffer uploads only" as not triggered. | scope.md D13; [s1-renderer.md](s1-renderer.md) §1; scratch: `review/renderer-stack.md` | verified |
| **The cause.** wgpu-core 30.0.1's `Queue::write_texture` takes the pending-writes lock (queue.rs:924) before it allocates staging memory and copies the frame into it. `Queue::submit` takes the same lock (:1529). A producer writing a full 3024×1964 frame (23.7 MB) blocks the main thread's submit for the length of the copy. Rerun today at load 1–2: main-thread p99 **6.04 ms** with a 60 Hz full-frame producer, 0.16 ms without. An earlier rerun gave 4.69 ms. | registry: `wgpu-core-30.0.1/src/device/queue.rs`; scratch: `renderer-stack/gpu_probe` (`wgpu_probe`) | verified |
| **The stall is avoidable inside wgpu.** `Queue::write_buffer_with` hands the producer a `Send` view to fill outside the lock. Dropping the view records a buffer copy. S1's ring on it: 3 buffers, rows padded to 256 bytes, and the main thread only records `copy_buffer_to_texture`. Main-thread p99 was **0.15, 0.26 and 0.24 ms** in three runs, with 600 of 600 frames shown and 0 ring waits. This ran on wgpu's Metal backend **without presenting to a surface**. | registry: `wgpu-30.0.1/src/api/queue.rs:68–230`, `wgpu-core-30.0.1/src/device/queue.rs:631–850`; scratch: `renderer-stack/gpu_probe/src/bin/wgpu_ring_probe.rs` | verified |
| **Device init.** Today at load 1–2: wgpu instance, adapter and device took 14–29 ms (4 runs), and Metal's device and queue took 16–17 ms (2 runs). The earlier figures (wgpu 74–78 ms, Metal 28.6 ms) came from runs at load 5–12. Init time alone is not a reason to keep wgpu off macOS. | scratch: `wgpu_probe`, `metal_probe`, rerun 2026-10-19 23:05 | verified |
| **What wgpu loses on macOS.** The Metal drawable is private, so there is no `presentedTime` and no key-to-display measurement. Every surface configure sets `allowsNextDrawableTimeout(false)`, so `nextDrawable` can block with no limit; S1 saw 1 s blocks with the timeout on. Only the Fifo and Immediate present modes exist. The binary is about 4.3 MB larger. wgpu does read `presentsWithTransaction` from the layer. | registry: `wgpu-hal-30.0.1/src/metal/mod.rs:653–659`, `metal/surface.rs:261–265, 330–338, 399`; binary sizes from the review probe | verified (sizes likely) |
| wgpu compiles no C. Vulkan is loaded at run time through ash. DX12 can use Windows' built-in FXC compiler; the `static-dxc` feature pulls a prebuilt library Microsoft doesn't ship and must stay off. | registry: `wgpu-hal-30.0.1/Cargo.toml`; `wgpu-types-30.0.1/src/backend.rs:777–804` | verified |
| Zed moved Linux to wgpu (PR #46758, February 2026). It kept native Metal on macOS and its own renderer on Windows. Hangs and sleep/wake corruption were reported afterwards (#52403, #63088). | https://github.com/zed-industries/zed/pull/46758 | likely |
| Mesa's Wayland Vulkan driver used to stall FIFO apps whose windows were hidden. Mesa 24.3 moved FIFO onto the fifo-v1 and commit-timing protocols. Whether later fixes merged is unconfirmed. On NVIDIA with Wayland, `vkQueuePresentKHR` has been reported to wait for the GPU. | https://www.phoronix.com/news/Mesa-frog-fifo-v1-MR ; https://phoronix.com/news/Mesa-24.3-Vulkan-FIFO ; https://forums.developer.nvidia.com/t/vulkan-wayland-vkqueuepresentkhr-waits-for-gpu-to-finish/302203 | unverified |
| **winit 0.30.13 on Linux.** It has no `Occluded` event on Wayland or Windows. `pre_present_notify` throttles redraws on Wayland frame callbacks. It doesn't bind `wp_presentation`, and wgpu exposes no presentation timing. On Linux, only DIE's share of latency (key event to present call) is measurable. | registry: `winit-0.30.13/src/event.rs:405–426`, `src/window.rs:610–640` | verified (wgpu: likely) |
| **The cell grid is identical on every text backend.** Ioskeley Mono Term's typo and hhea metrics both sum to 1,250 units. At 16 pt@2x, skrifa gives a 19×40 px cell with the baseline at 30.88, the same as Core Text (S1: 19×40). | `fonts/IoskeleyMonoTerm-*.ttf`; scratch: `xplat/text_port` | verified |
| swash 0.2.10 rasterizes outlines, COLRv0, sbix, CBDT and EBDT, but not COLRv1. I found no maintained COLRv1 rasterizer crate, so DIE would write a painter on skrifa's `ColorPainter`, about 0.25–0.4k lines. Fedora 43's Noto Color Emoji is COLRv1, and FreeType-based apps (foot, Pango) also broke on it. | registry: `swash-0.2.10/src/scale/mod.rs:516–545`; https://discussion.fedoraproject.org/t/f43-change-proposal-use-colr-for-noto-color-emoji-system-wide/148883 | likely |
| FreeType from crates.io conflicts with D16. freetype-sys has `cc` and `pkg-config` build dependencies and compiles a bundled FreeType when it can't find one. | registry: `freetype-sys-0.20.1/build.rs` | verified |
| **fontique 0.9** gives one fallback API over Core Text, DirectWrite and fontconfig. Its `fontconfig-dlopen` feature loads fontconfig at run time. FreeBSD gets a dummy backend with no system fallback. Fallback is chosen per script and locale, not per character: a probe on this Mac found nothing for `😀` or the Nerd-font codepoint U+F0001. Building its collection took 138 ms. | registry: `fontique-0.9.0/src/backend/`; scratch: `xplat/text_port` | verified |
| Since macOS 15, PingFang keeps its outlines in a nonstandard `hvgl` table that non-Apple rasterizers can't read. On macOS, CJK fallback that renders PingFang needs Core Text. | https://developer.apple.com/forums/thread/759219 ; registry: `fontique-0.9.0/src/backend/coretext.rs:80–89` | likely |
| **Most of die-render is neutral already.** die-core's `Scene` is cells with resolved colors. R1's rule keeps the atlas, row cache and instance building outside `platform`. S1's key mapping uses winit fields that exist on every backend. My split of S1's 4.1–5.0k estimate: about 1.5–2.1k is macOS-only (Metal, Core Text, AppKit glue). | `m0/k8-scene:crates/die-core/src/scene.rs`; [m0-plan.md](../m0-plan.md) R1; [s1-renderer.md](s1-renderer.md) §3 | estimate |

### 2.2 Terminal engine

| Finding | Source | |
|---|---|---|
| **Ghostty's signed tip publishes four lib-vt assets,** each with a `.minisig`: `ghostty-vt.xcframework.zip` (macOS and iOS), `ghostty-vt.wasm`, `ghostty-vt-small.wasm` and `libghostty-vt-source.tar.gz`. There is no Linux, FreeBSD or Windows binary. The tip tag is 765ea37, S2's pin. | GitHub API `releases/tags/tip`; `release-tip.yml@main` lines 253, 327, 412 | verified |
| Ghostty's Test workflow builds lib-vt, in Debug and unpublished, for macOS (both arches), Linux (x86_64, aarch64, x86_64-musl) and x86_64-windows-gnu. `x86_64-windows-msvc` is commented out: "doesn't work yet". There's no FreeBSD and no Windows on ARM. | https://github.com/ghostty-org/ghostty/blob/765ea37ad64f7e3a48f6ac27a70ad7fb6f31b9bb/.github/workflows/test.yml lines 608–649 | verified |
| Upstream added tip artifacts on request twice: #12149 (the xcframework) and #13822 (wasm, "so wasm consumers can use libghostty without a Zig toolkit"). I found no request for Linux archives. | GitHub API, pulls 12149 and 13822 | verified |
| libghostty-rs at `d2036bae` passes CI on macOS, Linux (both arches) and Windows (MSVC and GNU), building lib-vt from source with Zig 0.16.0. One checked-in `bindings.rs` serves every target: the headers force int-sized enums, and the bindings have no `c_long`. | libghostty-rs `ci.yml`, `windows-ci.yml`, `tools/gen_bindings.rs` | verified (one bindings file: likely) |
| S2's archive derivation uses `ditto`, `lipo`, `ld -r -exported_symbols_list` and `libtool`, which handle Mach-O only. A Linux archive needs binutils running on Linux. Whether its bundled `compiler_rt` and `ubsan_rt` need hiding next to Rust's `compiler_builtins` is unmeasured. | `spikes/s2-term-engine/scripts/derive-archive.sh`; Ghostty `src/build/GhosttyLibVt.zig` lines 239–270 | likely |
| ghostel builds its Linux and FreeBSD modules with Zig in its own CI (x86_64 and aarch64 glibc 2.35, x86_64-freebsd) and ships them unsigned. For Windows it ships `conpty.dll` and `OpenConsole.exe`. Third-party Linux archives exist (Paneflow, libghostty-vt-static, ghosttea-vt-sys); Ghostty signs none of them. | https://raw.githubusercontent.com/dakra/ghostel/main/.github/workflows/release.yml ; ghostel v0.59.0 assets; https://paneflow.dev/blog/libghostty-linux | likely |
| wasmtime can't host the signed wasm under D16. Its `runtime` feature brings in `cc`, and its build script compiles `helpers.c` on Unix. | https://github.com/bytecodealliance/wasmtime crates/wasmtime/Cargo.toml and build.rs | verified |

### 2.3 Processes, the holder and the helper

| Finding | Source | |
|---|---|---|
| rustix 1.1.5, already pinned by S3, has safe Linux calls for `openpt(CLOEXEC)`, `recvmsg` with `CMSG_CLOEXEC`, `socket_peercred`, `pidfd_open` and `memfd_create`. `rustix::shm::open` opens `/dev/shm/<name>` with `O_CLOEXEC`, so S2's shared memory works unchanged on Linux. | registry: `rustix-1.1.5/src/pty.rs`, `net/sockopt.rs`, `backend/linux_raw/shm/syscalls.rs:9, 38` | verified |
| glibc's `posix_spawn` runs `setsid` (`POSIX_SPAWN_SETSID`) before the file actions, and has `addclosefrom_np` (glibc 2.34+) and `addchdir_np`. So S3's macOS shape ports, with `addclosefrom_np(3)` in place of `POSIX_SPAWN_CLOEXEC_DEFAULT`. That the slave then becomes the controlling tty on DIE's path is unmeasured. musl has no `closefrom`. | https://raw.githubusercontent.com/bminor/glibc/master/sysdeps/unix/sysv/linux/spawni.c lines 159–160, 187–279; registry: `libc-0.2.191` | likely |
| Linux receives fds close-on-exec atomically (`MSG_CMSG_CLOEXEC`), so S3's 0.6–2.6% inheritance race can't happen there. | registry: `rustix-1.1.5/src/backend/libc/net/send_recv.rs:70–84` | verified |
| **Exit status as a non-parent.** A pidfd works on any pid, is pollable and is close-on-exec. `waitid` on a pidfd works only for children. Since Linux 6.15, `PIDFD_GET_INFO` with `PIDFD_INFO_EXIT` returns the status after reaping, while the pidfd stays open. That a non-parent may read it is inferred from kernel code, not documented. Ubuntu 24.04's GA kernel is 6.8, Debian 13's is 6.12. | https://man7.org/linux/man-pages/man2/pidfd_open.2.html ; https://lkml.rescloud.iu.edu/2503.2/09488.html | likely |
| A Linux PTY master returns buffered output before EIO, and Linux doesn't hold an exiting shell in the kernel the way macOS does. The holder's sweep must keep a hung-up master until its output is read. | kernel commit f8747d4a; [s3-fd-store.md](s3-fd-store.md) §1 item 5 | likely |
| FreeBSD 15 has no `POSIX_SPAWN_SETSID`, and `open` doesn't assign a controlling terminal, so a shell needs a fork path with `TIOCSCTTY`. Its kqueue `NOTE_EXIT` gives any watcher the exit status. | https://man.freebsd.org/cgi/man.cgi?query=posix_spawnattr_setflags&sektion=3&manpath=FreeBSD+15.0-RELEASE ; https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2&manpath=FreeBSD+15.0-RELEASE | verified |
| GNOME and KDE start apps in a transient `app-*.scope` cgroup, and DIE's children inherit it. systemd-oomd, on by default on Fedora and Ubuntu, kills whole cgroups. Whether a desktop stops the scope when DIE exits, and how often oomd picks app scopes, is unverified. | Ghostty README (cgroup isolation) | unverified |
| The spike code's macOS-only calls: `getpeereid` and `LOCAL_PEERPID`, `posix_openpt` through libc, `proc_pidinfo`, kqueue `NOTE_EXITSTATUS`, `POSIX_SPAWN_CLOEXEC_DEFAULT`, `confstr` and `CLOCK_UPTIME_RAW`. The rest (SCM_RIGHTS, poll, flock, socketpair, mmap, TIOCSWINSZ) is POSIX. | `spikes/s3-fd-store/src/`, `spikes/s2-term-engine/src/` | verified |

### 2.4 Platform shell and toolchain

| Finding | Source | |
|---|---|---|
| winit 0.30.13 with `x11`, `wayland` and `wayland-dlopen` loads Wayland, X11, xcb and xkbcommon at run time. Building needs no `-dev` packages. | registry: `winit-0.30.13/Cargo.toml` lines 229–313; `wayland-sys-0.31.11/build.rs` | verified |
| That graph brings in `cc` and `pkg-config`, which xtask's clean-build rejects today. Neither compiles C. wayland-backend compiles C only with its `log` feature, which is off. wayland-sys skips pkg-config under dlopen. x11-dl calls pkg-config to find each library's directory and carries on with `None` if it fails. fontique's yeslogic-fontconfig-sys skips pkg-config under dlopen. | registry: build.rs of `wayland-backend-0.3.17`, `wayland-sys-0.31.11`, `x11-dl-2.21.0`, `yeslogic-fontconfig-sys-6.0.1`; `xtask/src/clean.rs:15–17` | verified |
| **The workspace cross-checks for Linux after an 8-line change.** On a copy of main (d87b3bc), with the Apple crates and winit's Linux features moved under target tables, plus a cfg on `platform` and its test file: `cargo check --target x86_64-unknown-linux-musl --workspace --all-targets` passed in 10.6 s. It ran offline, with failing `cc`, `c++`, `clang`, `clang++`, `gcc`, `g++`, `zig` and `pkg-config` first on PATH. | scratch: `daily-first/ws` (rerun today) | verified |
| **The whole Linux stack cross-checks the same way.** winit with its Linux features, wgpu 30.0.1 (Vulkan and GL), skrifa 0.44, swash 0.2.10, fontique 0.9 with fontconfig loaded at run time, and rustix's PTY calls: 15.7 s, and no stand-in was called. | scratch: `daily-first/xcheck` (rerun today) | verified |
| The neutral crates on main and on the k10a, v1 and k3 branches already cross-check for Linux. | scratch: `plat-audit/` | verified |
| winit reports left and right modifier sides only on macOS, and Caps Lock state on no platform. `key_without_modifiers` and `text_with_all_modifiers` work on every backend, so S1's key mapping is portable. Dead keys arrive as committed text through xkb compose. | registry: `winit-0.30.13/src/platform_impl/`, `src/keyboard.rs:1693–1702` | verified |
| **The Wayland clipboard** works only for the focused client, and setting it needs a recent input serial. GNOME has no data-control protocol. smithay-clipboard 0.7.2 runs on winit's own `wl_display` (an unsafe pointer hand-off), and its `load()` has no timeout; 0.7.3 would bring a second smithay-client-toolkit. arboard reaches X11 through pure-Rust x11rb. Neither has a change count. On X11 and Wayland, the clipboard goes away with the app that set it. | https://wayland.app/protocols/wayland#wl_data_device ; registry: `smithay-clipboard-0.7.3/src/lib.rs`, `arboard-3.6.1/src/platform/linux/x11.rs` | verified (data-control on GNOME: likely) |
| Linux targets link through the system C driver (gcc or clang plus libc development files). | `rustc --print target-spec-json` | verified |
| `std::fs::File::lock` is stable since Rust 1.89, so single instance needs no unsafe code. On Linux, an old process's `current_exe()` gets " (deleted)" once restart renames a new build over it, but `exec /proc/self/exe` still runs its own build. | Rust std docs | likely |
| Hosted runners: ubuntu-24.04 and ubuntu-24.04-arm at $0.006 and $0.005 a minute, windows at $0.010, macOS at $0.062. None has a GPU. Free plans include 2,000 minutes a month, Pro 3,000. | https://docs.github.com/en/billing/concepts/product-billing/github-actions | verified |
| Asahi Linux doesn't support M4 or M5 Macs. | https://phoronix.com/news/apple-m3-asahi-linux-2026 | likely |

### 2.5 macOS assumptions in the docs and code today

| Where | Assumption | What it needs |
|---|---|---|
| `crates/die-render/Cargo.toml` | objc2 crates are unconditional, so the workspace doesn't build on any non-Apple host ("objc2 only works on Apple platforms") | A target table and a cfg now, about 10–20 lines |
| K3 (PR #1): `Mods.cmd`, `ModifierKey::Command`, "Option" docs. main: `ctx.rs:1188, 1193` check `mods.cmd` for the leave key and the Cmd layer | Cmd is the DIE layer | Rename now; resolve the layer from config by M2 |
| xtask | `clean.rs` says "Command Line Tools only", rejects `pkg-config`, writes `/bin/sh` stand-ins. `util.rs` calls `/usr/bin/shasum`; `fonts.rs` calls `/usr/bin/curl` and `/usr/bin/unzip`. The oracle defaults to `/opt/homebrew/bin/emacs` | Small changes now (dev-only) |
| K10a: the `(dev, ino)` fingerprint uses `std::os::unix` | Unix | Nothing, with Windows out |
| Planned: K6's `Clipboard` change count (NSPasteboard), K12's session path, F4a's DIE.app and LaunchServices, R3–R5 | macOS | Seams in their test plans, before they're written |
| scope D1, D5, D10, D12, D13, D16, D18, D20; §1, §5.6, §5.8, §5.9, §5.11, §6, §7, §11, §12. Contract §4, §5, §7, §12 | macOS wording or decisions | §7's edits |

The Unix-only part of the plan is about 1.9–2.1k Rust lines plus about 160 lines of bash, out of about 22–28k product lines (estimate). All of it is M2's survival machinery: the holder, the helper transport and core's `sys` module.

### 2.6 The machines you have

| Machine | Can test | Can't test |
|---|---|---|
| Your Mac | Everything on macOS. `cargo check` and clippy of the whole workspace for Linux targets, once the pinned toolchain has their std. The portable renderer on wgpu-Metal behind a dev feature. Per-target build graphs (`cargo tree --target`). | Anything at Linux run time |
| colima, profile `default`: aarch64 vz VM, 2 CPUs, 16 GB, docker, Rosetta off, binfmt on, Ubuntu 24.04.4, currently stopped | Natively on aarch64: build and test die-core, die-vim, the oracle and die-render's neutral code. The Linux clean-build. Every S3 survival check on Linux PTYs. S2's battery once a Linux archive exists. wgpu on lavapipe or llvmpipe, offscreen. Smoke tests under Xvfb and headless sway or weston (window, xdotool and wtype key matrix, clipboard round trip). The Linux step of `ghostty bump`. | A GPU, real drivers, latency or presentation timing. Real compositor behaviour for hidden windows, scaling, focus and serials. A desktop session (systemd user scopes, oomd, what a `.desktop` launch gets). A real keyboard, or a clipboard shared with real apps. Native x86_64: qemu-user is fine for compiling, but its PTY and pidfd ioctls aren't reliable. Its kernel version is unknown until it boots. |
| Linux with a display | You have none. | |
| Windows | You have none. | |

## 3. Windows verdict

**Out.** By your rule it's a considerable deviation. Rendering and the platform shell would be cheap with the portable backend. The terminal half is not: there it changes D20's design rather than porting it, and needs two more families of binaries that D16 and D17 don't allow.

| # | Deviation | What Windows forces | Cost |
|---|---|---|---|
| 1 | **Terminal engine** | Ghostty signs no Windows lib-vt. Its MSVC build is commented out upstream ("doesn't work yet"). An MSVC shared-library linking fix (#13151) is still open. Static linking was fixed only in July 2026 (#13452), and DLL constructors in September (#14447). A DIE CI build would need Zig plus MSVC on a Windows runner, and one project reports that build isn't reproducible. | Blocked under D16, or a new D16 exception plus a spike. **Deciding.** |
| 2 | **ConPTY instead of PTYs** | In-box conhost strips kitty-graphics APC and sixel DCS (project reports, likely). Parity means bundling Microsoft's `conpty.dll` and `OpenConsole.exe` and bumping them each release, as ghostel does. Kitty keyboard through ConPTY arrived only in Windows Terminal 1.25 (2026-10-02). Password detection (`tcgetattr`) and the running-command check (`tcgetpgrp`) have no direct equivalents. | A third vendored binary family, outside D17. **Deciding.** |
| 3 | **D20's holder and fd passing** | Windows has no SCM_RIGHTS. A pseudoconsole dies when its creator's signal pipe breaks (microsoft/terminal `PtySignalInputThread.cpp:315–325`). So the holder would have to create every console, spawn every shell and serve resizes, or DIE would copy the three handles inside an `HPCON`, which Microsoft calls a shared ABI but doesn't document. Named pipes with a user-only DACL replace `getpeereid`; job objects replace process groups. | About 500–700 Windows-only lines, plus either a second holder design or a holder that spawns shells on every OS, against S3's "keep it tiny". **Deciding.** |
| 4 | **The helper** | Overlapped I/O, `DuplicateHandle` and file mappings instead of poll, fd passing and shm. | About 250–350 lines |
| 5 | **Toolchain under D16** | The MSVC Build Tools and Windows SDK. winit pulls `windows_x86_64_msvc 0.52.6`, which ships a prebuilt import library (seen in the build graph). cc finds `cl.exe` without PATH, so clean-build would need failing `CC`, `CXX` and `AR` variables. clean-build's `env -i`, `/bin/sh` stand-ins and permission code need Windows versions. An icon or manifest may need `rc.exe` (unverified). | A written D16 exception, and moderate xtask work |
| 6 | **Renderer and text** | Cheap only with the portable backend: wgpu's DX12 backend with the built-in FXC compiler, fontations, and fontique's DirectWrite fallback. Text would be grayscale, not ClearType. winit sends no occlusion event, live resize runs in Win32's modal loop, and key-to-display can't be measured. | About 0.2–0.45k lines of glue |
| 7 | **Keyboard** | Windows reserves many Win-key chords, so the DIE layer would be Ctrl+Shift, which is the proposed Linux default anyway. Ctrl+Shift+Esc opens Task Manager, so leaving a terminal needs a third default chord. | Small |
| 8 | **Packaging and restart** | A running `.exe` can't be overwritten or deleted, and D20's helpers outlive DIE. Every build would run from a versioned copy, cleaned up only after its helpers exit. PowerShell shell integration replaces `die.bash`. | About 250–400 lines |
| 9 | **Testing** | You have no Windows machine. Everything would be checked on hosted CI only, and nobody would use it day to day. | Reverses question 10's local checks for that platform |

**Total:** about 1.9–2.95k Windows-only lines, 6–10 weeks, two more binary families (the engine and ConPTY), exceptions to D16 and D17, and a reshaped holder.

**A Windows build without terminals** would be cheaper: rendering, the shell and a D16 exception. But D15 says die-term always ships, and an editor without a terminal doesn't replace Emacs. It isn't proposed. *D15 now lets Linux's preview ship without die-term until a signed archive exists (2026-10-19). Windows stays out for the reasons above.*

**Kept for free:**
- The portable renderer exists for Linux anyway, so the renderer isn't what blocks Windows.
- The DIE layer becomes a config value, and paths go through one function.
- Windows users can try the Linux build under WSL2 and WSLg. That's unofficial; its latency and clipboard are unverified.
- No Windows CI minutes and no Windows compile check. It would only exercise fallback code nobody runs.

**Revisit when all three are true:**
1. Ghostty signs a Windows lib-vt that links into an MSVC Rust build.
2. A pseudoconsole can be handed to another process (documented), or you accept a holder that spawns shells.
3. You have a Windows machine to use it on.

## 4. Options considered

### 4.1 When Linux ships (question 11)

Every option includes the phase-0 seams and guards in §6, so no macOS assumption leaks into shared code. Weeks and dates are estimates; M2 isn't planned yet.

| | A. Mac first, Linux at M2's exit (recommended) | B. Parity from M0 | C. Linux in its own milestone after M2 |
|---|---|---|---|
| What happens | M0 runs as planned on the Mac. The renderer track builds the Linux backend once its M0 work is done (after checkpoint 4). The Linux process layer is written alongside the Mac one in M2. | Every checkpoint and exit test, M0 to M4, runs on both platforms, and none passes until Linux does. | M0 and M2 run Mac-only. A Linux milestone (ML) follows M2, alongside M1. |
| Mac dates | Unchanged. M2's Mac exit tests don't wait on Linux. | M0 becomes 13–20 weeks (best guess 16, +3) | Unchanged |
| Linux arrives | "Preview" at M2's exit, about March to May 2027 | With each Mac checkpoint | "Preview" 6–9 weeks after M2, about mid-April to end of June 2027 |
| Added end to end | About 1.5–3 weeks, mostly your review and M2's tail | About 4.5–9 weeks | About 2–4 weeks |
| Your time | About 6–10 h | About +25–32 h, including a Linux exit day per milestone | About 6–10 h |
| Money | Hosted Linux jobs from M2, inside the included minutes. Hardware optional. | A Linux machine with a GPU by about week 7 (not priced), and hosted Linux CI from M0 | As A |
| Main risk | Linux work overlaps M2's reviews | Slows the Mac daily driver, which is the point of M0. Needs hardware you don't have. | Linux arrives last. The renderer's seams have one implementation for longer, so macOS habits can harden. |

### 4.2 Renderer and text

| Option | What | Cost | Verdict |
|---|---|---|---|
| **A. Neutral core, native macOS backend, one portable backend** | Metal and Core Text on macOS, as S1 measured. wgpu (Vulkan, GL fallback) and fontations on Linux. The portable backend also builds on macOS behind a dev feature. | die-render total 5.35–7.15k; per build: macOS 4.1–5.25k, Linux 3.85–5.05k. 2–4 weeks of AI work plus S6. +4–5 MB on Linux. | **Recommended.** macOS stays as measured, and Linux is mostly safe Rust you can run on your Mac. |
| B. wgpu everywhere | Option A, but macOS draws through wgpu-Metal too. Core Text stays. | Saves 0.55–0.75k lines and most Metal `unsafe`. Total 4.45–6.0k. Loses `presentedTime`, forces `allowsNextDrawableTimeout(false)`, +4.3 MB, wgpu's 12-week release cycle in the daily path. Device init is no longer an argument (§2.1). | **Not now.** S6a measures it. If wgpu-Metal passes S1's whole battery with margins, deleting the Metal backend becomes your call after M2. |
| C. A native backend per platform | Vulkan through ash or GL through glow on Linux, with fontconfig and FreeType through DIE's own FFI. | Linux build 4.6–6.9k, over 5k. Unsafe review grows to 2–4k lines. 5–8 weeks. | Fallback only, if S6 or the hardware run fails |
| D. Vulkan everywhere through MoltenVK | One Vulkan renderer | 1.5–2.5k unsafe lines, a MoltenVK dylib of unverified provenance (D16), S1 redone through a translation layer | Rejected |
| E. fontations on macOS too | Delete Core Text | Saves 0.4–0.6k lines. PingFang's `hvgl` outlines can't be read; text changes from what your Emacs shows. | Dev and test path only |

### 4.3 The Linux engine archive

| Path | What | Cost | Verdict |
|---|---|---|---|
| **A. Ask upstream** | A PR to Ghostty's `release-tip.yml` publishing signed `x86_64-linux-gnu` and `aarch64-linux-gnu` archives, modelled on the xcframework job. Both targets already build in their Test matrix. | About 60–80 lines of their YAML, your PR, their CI minutes. D16 and D17 stay as written. | **Start now.** Upstream accepted this kind of request twice. Acceptance for Linux is unknown. |
| **B. DIE's CI builds it** | A hosted job verifies Ghostty's signed `libghostty-vt-source.tar.gz`, requires a passing Test run for that commit, builds with a pinned Zig, and must produce byte-identical members on two runners. It's die-term-engine.md §1's existing "DIE's own CI build" fallback, with a new trigger. ghostel ships its Linux binaries this way. | About 60–100 lines of YAML, a D16 wording change, DIE's CI as the trust root | **Fallback,** if A hasn't produced an archive by M2's start. Your call. |
| C. The signed wasm in wasmi | One signed artifact for every OS, run by a pure-Rust interpreter in the helper | DIE's own engine layer (1.2–4k lines), unmeasured throughput | Last resort, behind a speed spike |
| Ruled out | wasmtime (brings `cc`); third-party archives (not signed by Ghostty); Arch's 1.3.1 `.so` (too old for the tip API) | | |

*2026-10-19: you declined both A and B. Where a signed Linux archive comes from is open until before M2, when Linux terminals need it.*

If neither A nor B has an archive by M2's exit, Linux ships editor-only and gets terminals at the first `ghostty bump` that has one. *With both declined, the same holds for whatever source is chosen before M2.*

### 4.4 Smaller choices

| Choice | Options | Proposed |
|---|---|---|
| Exit status after a restart on Linux | (1) A pidfd opened right after spawn, while DIE is still the parent, kept in the holder beside the PTY. Status from `waitid` while DIE is the parent, and from `PIDFD_INFO_EXIT` after a restart on kernel 6.15+; older kernels show "exited, status unknown". (2) The helper becomes the shell's parent: about 60 lines that differ from macOS, and the status is still lost if the helper crashes. | (1). It's race-free and immune to pid reuse. |
| The DIE layer on Linux | Super (Linux's equivalent of Cmd, but compositors and GNOME take many Super chords) or Ctrl+Shift (kitty, Ghostty and GNOME Terminal on Linux) | Ctrl+Shift by default, Super in config |
| Clipboard change detection | A per-OS change count (NSPasteboard's `changeCount`; nothing on Linux) or comparing with DIE's last write, as Emacs does on X11 | The trait promises "changes when another app copies". macOS keeps `changeCount`; Linux compares. |
| Windows users | Native (out, §3), or the Linux build under WSL2/WSLg | WSLg unofficial, unpromised |
| FreeBSD | Full support (needs a fork-path spawn and an engine nobody builds) or compile checks | Compile-only from M2 |

## 5. Recommendation

**Answer question 11 with A, and take renderer option A and engine path A now, with path B as the named fallback.** Windows is out. macOS changes only in names and seams. Linux arrives at M2's exit as a preview, built mostly from safe Rust you can run on your Mac.

*Answered 2026-10-19: question 11 A. Engine paths A and B were both declined, so the Linux archive's source is open until before M2.*

### 5.1 Timing

| When | macOS | Linux |
|---|---|---|
| Phase 0, now | As planned | §6: seams, guards, cross-checks on every `xtask ci`, headless tests in colima. "Builds and passes headless." |
| M0, weeks 1–16 | Unchanged: four checkpoints, exit tests (a)–(e) | New exit test (f): the workspace cross-checks for both Linux targets, and die-core, die-vim and the oracle pass on aarch64 Linux |
| After checkpoint 4 (renderer track) | Fixes from use | S6: the portable renderer spike |
| M2 | The terminal, as planned. Exit tests (a)–(e) don't wait on Linux | The process layer, holder and helper with their Linux branches. The Linux window, renderer, clipboard and packaging. Hosted Linux jobs start with M2's hosted CI. die-term on Linux if an archive exists. |
| M2's exit | | New exit test (f), "Linux preview": headless tests, Linux clean-build, ELF check, S3L, smoke tests and lavapipe fidelity on both architectures. If it slips, it runs during M1. |
| When hardware exists | | "Linux supported": SL-desktop and SL-hw pass (§8) |
| M1, M3, M4 | As planned | Their tests run in Linux CI. M3's images go through the wgpu ring as well (+0.1–0.15k). |

Dates, all estimates: Mac editor early January to early February 2027. Mac terminal about March to late April 2027. Linux preview at M2's exit. Linux supported when the hardware run passes.

Until "supported", Linux makes no latency or daily-use claim. "Preview" means it builds under D16, every headless and software-rendered test passes on x86_64 and aarch64, and Linux bugs in shared code get fixed.

### 5.2 Architecture, crate by crate

The rule: neutral code is safe Rust and never branches on `target_os`. It may use `std::os::unix` and `cfg(unix)`, because Unix is the floor. An xtask lint enforces it.

| Crate | Neutral | Per-platform |
|---|---|---|
| die-core | Everything else: the holder, its client and protocol, DIE's side of it, the restart handshake (a Unix socket), the session, single instance (`File::lock`), clipboard change detection by comparison, `KeyEvent` with physical modifiers | `sys/` only. M0: `sys/mod.rs` and a safe `paths` function (macOS `~/Library/Application Support/DIE`; Linux `$XDG_STATE_HOME/die`, defaulting to `~/.local/state/die`). M2: `sys/apple.rs` (S3's 80 lines) and `sys/linux.rs`, behind the same entry points: open PTY, spawn, receive fds, peer uid, exit watch, runtime directory, raise the fd limit. Other targets get a stub returning `Unsupported`. |
| die-vim | All of it | None. A denylist keeps window and GPU crates out of its graph on every target. |
| die-render | Scene diff, row cache, instance building (one `#[repr(C)]` layout, asserted in tests), atlas and glyph cache, sprites, the font policy (per-range chain, squeeze-to-fit, last-resort coverage scan), placements, ring accounting, the winit loop and scheduling policy, key mapping (modifier sides tracked from modifier-key events), the latency log | `platform::macos`: today's `platform::{appkit, metal, text}`, renamed and cfg-gated. Portable backend (safe): `backend::wgpu` and `text::fontations`, on Linux, and on macOS behind a `portable` dev feature. `platform::linux`: clipboard, activation token, app_id, frame-callback hooks, about 10–30 unsafe lines. Neutral code names `platform::backend`: `macos` on macOS, and until the Linux backend lands a stub elsewhere, with only what neutral code names. Only the frame driver calls it. The rest of the neutral code takes data in and gives data out, so its tests run on Linux. Internal `Gpu` and `Rasterizer` traits arrive with the second implementation, not before. |
| die-term | The glue. Keys and mouse arrive as core events. The helper's shared memory uses `rustix::shm` on both OSes. | Link flags per target (`-dead_strip_dylibs` on macOS, `--as-needed` on Linux) |
| die-term-sys | One `bindings.rs` | `vendor/<target>/libghostty-vt.a`, a lock section per target. `build.rs` picks by `TARGET`. On other targets it links nothing, and DIE builds there without die-term (scope D10). |
| die | The facade; `--fd-store` and `--term-helper` on both | Linux: helpers exec `/proc/self/exe`; the holder starts under `systemd-run --user --scope` when systemd is present, if SL-desktop shows it's needed |
| apps/die-dev | Your config | Per-OS defaults for the DIE layer and the leave chord |
| xtask | | §5.5 |
| tools/evil-oracle | Emacs found on PATH | |

### 5.3 Renderer and text

**macOS (unchanged from S1).** Metal through objc2-metal, Core Text with its fallback chain, the AppKit glue (the key monitor for Caps Lock, C-/ and C-Return; press-and-hold off; Core Animation transaction presents; the display link; NSPasteboard). R1–R6 build it as planned.

**Linux.**
- **GPU:** wgpu 30.0.1 with default features off and `std`, `parking_lot`, `vulkan`, `gles` and `wgsl` on. Vulkan, with GL as the fallback. `dx12` and `static-dxc` are never enabled.
- **Shader:** one WGSL port of S1's 15-line MSL, drawn to a non-sRGB surface so blending matches Metal.
- **Pixel buffers and images: producers never call `write_texture`.** They fill `write_buffer_with` views with rows padded to 256 bytes, and the main thread only records `copy_buffer_to_texture`. Glyph uploads stay small `write_texture` calls on the main thread. A lint allows `write_texture` only in the atlas module. Image sizes are clamped to the adapter's limits (8192 px, or 2048 on GL).
- **Presentation:** Mailbox where offered, Fifo otherwise. On Wayland, present only after a frame callback (`pre_present_notify`), and treat no frame callbacks as occlusion. On X11, use winit's `Occluded` event. A `get_current_texture` timeout skips the frame.
- **Text:** skrifa for metrics, cmap and the COLRv1 paint graph. swash for gray outlines, CBDT, sbix and COLRv0. A COLRv1 painter (0.25–0.4k), needed for Fedora 43's emoji, at M2. fontique for system fallback, with fontconfig loaded at run time, behind DIE's per-range chain and a last-resort coverage scan. harfrust only for combining-mark clusters. fontique's collection scan runs off the startup path.
- **What's different from macOS:** text is grayscale and unhinted, fallback faces differ, and only DIE's share of latency can be measured. A `wp_presentation` log is a cut-list item.

**The same on both:** the 19×40 px cell and baseline, box, block, braille and powerline glyphs drawn by neutral code, squeeze-to-fit, and the frame-data boundary.

### 5.4 Terminal, holder and helper per platform

| Concern | macOS (S2, S3) | Linux (M2) | FreeBSD |
|---|---|---|---|
| Engine archive | Signed xcframework, cut to arm64 | Path A, else path B (§4.3). Both declined 2026-10-19; open until before M2 | None |
| Open a PTY | libc `posix_openpt(O_CLOEXEC)` (unsafe) | rustix `openpt(CLOEXEC)` (safe) | Stub |
| Spawn | `posix_spawn` with `SETSID` and `CLOEXEC_DEFAULT`, slave opened as a file action | glibc `posix_spawn` with `SETSID`, `addclosefrom_np(3)`, `addchdir_np`. glibc 2.34+; musl unsupported for terminals | Stub (needs a fork path) |
| Receive fds | Set CLOEXEC after `recvmsg` (the race `posix_spawn` covers) | `MSG_CMSG_CLOEXEC` (no race) | |
| Peer check | `getpeereid` | `socket_peercred` (safe) | |
| Exit watch | kqueue `NOTE_EXIT \| NOTE_EXITSTATUS` | pidfd kept in the holder; `waitid`, then `PIDFD_INFO_EXIT` on 6.15+, else "status unknown" | |
| Socket directory | Private directory under `confstr(_CS_DARWIN_USER_TEMP_DIR)`, 104-byte path limit | `$XDG_RUNTIME_DIR/die`, else a checked 0700 `/tmp/die-<uid>`; 108-byte limit | |
| Holder sweep | Every 5 s and before requests | The same, and it keeps a hung-up master until its output is read | |
| Helper shared memory | `rustix::shm` | `rustix::shm` | |
| Desktop survival | launchd reparenting (S3) | `systemd-run --user --scope` for the holder, pending SL-desktop | |
| Shell | `bash --rcfile die.bash -i` | The same. `die.bash` follows bash's login order on both: `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile`, then `~/.bashrc` behind a guard variable | |
| Environment | The env file fixes PATH for Dock launches | The same file fixes `.desktop` launches. `DISPLAY`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` and `XDG_RUNTIME_DIR` pass through; `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID` are removed | |
| Notifications (OSC 9, 777) | macOS notifications | `notify-send` when present, else a line in `*messages*`. No D-Bus crate | |
| DIE layer, leave chord | Cmd, Cmd-Esc | Ctrl+Shift, Ctrl+Shift+Esc by default (the chord is checked against GNOME and KDE in S6c) | |

Sizes: `sys` grows from about 80 lines to 110–125 with Linux, about 30–40 of them new `unsafe` (the spawn flags and the `PIDFD_GET_INFO` ioctl). The holder and DIE's side gain about 40–70 lines of cfg. No new dependencies: rustix 1.1.5 and libc 0.2.191 are already pinned in the spikes.

### 5.5 Toolchain and clean-build per platform

| | macOS | Linux | FreeBSD |
|---|---|---|---|
| Build needs | cargo and the Command Line Tools | cargo and gcc or clang with the libc development files, as the linker driver only. No `-dev` packages: X11, Wayland, xkbcommon, fontconfig and Vulkan load at run time. | cargo check only |
| Graph check | No C-building crate in the `aarch64-apple-darwin` graph | The `x86_64` and `aarch64` Linux graphs may hold only allowlisted C builders, each with a reason: wayland-backend's `cc` (only with `log`), and the `pkg-config` of wayland-sys, x11-dl and yeslogic-fontconfig-sys (§2.4). Runs from any host with `cargo tree --target`. | Same graph as Linux |
| Stand-in build | `env -i`, `--locked --offline`, linker by full path. Failing `cc`, `c++`, `clang`, `clang++`, `gcc`, `g++`, `zig` and now `pkg-config` on PATH, plus failing `CC`, `CXX` and `AR` variables | The same, on a Linux host. `pkg-config` may be called (x11-dl does) but finds nothing, and the build must still pass. A planted crate enabling `wayland-client/log` must fail it. | None |
| Link check | `otool -L`: libSystem and frameworks (plus libiconv from the libc crate, S2) | ELF `NEEDED`: only libc, libm, libgcc_s, ld-linux and libdl | None |
| Where it runs | Your Mac | colima in M0; the hosted Linux runners from M2 | Hosted Linux runner from M2 |

### 5.6 CI, and what colima covers

| Check | Where | When |
|---|---|---|
| Everything on macOS (`cargo xtask ci`) | Your Mac | Every merge, as now |
| `cargo check` and clippy, whole workspace, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, with `cargo xtask sources` on what each build read | Your Mac, inside `xtask ci` (about 11 s) | Every merge, from phase 0 |
| Per-target graph check and the lints | Your Mac | Every merge |
| `cargo xtask ci --linux`: die-core, die-vim, the oracle, xtask tests, and later die-render's neutral tests, in an aarch64 container | colima | Weekly, and at each checkpoint |
| Linux clean-build, ELF check, all tests, S3L, smoke tests under Xvfb and headless sway, lavapipe fidelity, FreeBSD `cargo check` | Hosted ubuntu-24.04 and ubuntu-24.04-arm | From M2, with the hosted CI question 10 already planned. About $0.11 for a pair of 10-minute runs. |
| S1's hand checks and latency on Linux | Real Linux hardware | When it exists |

colima's limits are in §2.6. The two that matter most: it has no GPU or display, and its kernel version (which decides whether `PIDFD_INFO_EXIT` can be tested) is unknown until it boots.

### 5.7 Budgets

**Guard 7, die-render:** "die-render's neutral modules plus any one platform's modules ≤ about 5k per build, and die-render's total ≤ about 7k". The weekend read becomes the shared code plus your platform. The total ceiling keeps platforms from sprawling: a future platform has to fit inside it. `budget.toml` gains platform tags, and `cargo xtask budget` reports each build and the total.

| Module | M0 ratchet | After M3 |
|---|---|---|
| neutral | 2.3k | 3.0k |
| macOS (`platform::macos`) | 0.9k | 2.0k |
| portable (`backend::wgpu`, `text::fontations` with the COLRv1 painter) | – | 1.6k |
| `platform::linux` | – | 0.4k |
| **macOS build / Linux build / total** | 3.2k / – / 3.2k | 5.0k / 5.0k / 7.0k |

The estimates are macOS 4.1–5.25k, Linux 3.85–5.05k and 5.35–7.15k in total, so both builds are tight. R2's first count, on 2026-11-05, is the first measure of the M0 split: neutral 1,122 lines (the atlas, row cache, font policy, sprites and driver), `platform::macos` 516 (Metal and Core Text, with R1's 104 among them) and the stub 65, whose limit went from 50 to 70. That leaves 1,178 neutral lines and 384 macOS lines for the window, loop, input and clipboard (R3–R5). macOS's M0 ratchet was 1.6k until 2026-11-05. It's now 0.9k, the size your `unsafe` review was planned for ([m0-plan.md](../m0-plan.md) §3), so `platform` going past that fails `cargo xtask budget`. That's below §2.1's estimate of 1.5–2.1k macOS-only lines, which nothing has measured. If R2–R4 do need more, raising the ratchet is when the thin-wrapper question gets asked, with a count in hand, and the review's hours get resized then. The cut list gains, after its current entries: the Linux presentation-time log, X11 occlusion handling (an unhandled occlusion only wastes frames), then the COLRv1 painter (emoji then come from a CBDT or sbix font, or show as tofu).

**Elsewhere:**
- die-core: ≤ 14k, unchanged. A new `sys` ratchet of 0.2k from M2. Linux adds about 0.1–0.15k.
- die-term: 4k of glue plus 1k for the helper, unchanged. Linux adds 10–30 lines.
- die-vim: unchanged.
- xtask (dev, uncapped): +0.5–0.9k through M2.
- `unsafe` to review: +40–70 lines for Linux (10–30 in `platform::linux`, 30–40 in `sys/linux.rs`).
- Binary: +4–5 MB on Linux. Nothing changes on macOS.

### 5.8 Calls within this recommendation (veto any)

- `Mods.cmd` becomes `super_` and `ModifierKey::Command` becomes `Super`, folded into PR #1 while it's open. `<D-…>` stays and names the DIE layer.
- The DIE layer on Linux is Ctrl+Shift by default. PRIMARY selection and middle-click paste are out in v1.
- Metal stays on macOS. Deleting it after S6a is a later decision, not a plan dependency.
- Exit status on Linux uses pidfds kept in the holder.
- Helpers use `rustix::shm` on both OSes; Linux helpers exec `/proc/self/exe`.
- Notifications on Linux go through `notify-send`.
- smithay-clipboard `=0.7.2` (not 0.7.3, which brings a second smithay-client-toolkit) and arboard with default features off, for X11 only.
- glibc 2.34+; musl isn't supported for terminals.
- FreeBSD is compile-only. No Windows CI or compile check.
- colima runs weekly and at checkpoints, not on every merge. If 2-vCPU builds are too slow, one hosted Linux job comes forward from M2.

## 6. What changes in M0 now

None of these moves a checkpoint. Together they're about 2–3 days of AI work and about 1 hour of your time.

| # | Change | Size | Needs you |
|---|---|---|---|
| 1 | **PR #1 (K3):** `Mods.cmd` → `super_`, `ModifierKey::Command` → `Super`, docs say "Alt (Option on macOS)", `<D-…>` documented as the DIE layer. V1 (PR #5) rebases mechanically. | About 10–20 lines | API diff approval (09-B) |
| 2 | **die-render gating (R1 follow-up):** Apple crates under `[target.'cfg(target_os = "macos")'.dependencies]`. winit's `x11`, `wayland` and `wayland-dlopen` features under a Linux target table (without them winit 0.30.13 hits a `compile_error!` on Linux). `platform` becomes `platform::macos`, cfg-gated, as does `tests/platform.rs`. A non-macOS stub `run()` returns "no window backend on this platform yet". About 25 Linux-only crates enter `Cargo.lock`; the macOS graph doesn't change. | About 10–20 lines | Normal merge |
| 3 | **Toolchain:** `targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]` in `rust-toolchain.toml`. `xtask ci` then cross-checks and lints the workspace for both. | 1 line, plus about 10 in xtask | **The std downloads.** Approved 2026-10-19 |
| 4 | **xtask guards:** the platform lint (no `target_os`, `target_vendor`, `windows` or `std::os::{macos,linux,freebsd,…}` in die-core, die-vim or the oracle outside `die-core/src/sys/`); the dependency denylist for die-core and die-vim on every target; D15's lint over every dependency table through the existing `dep_tables()`; clean-build's `pkg-config` and `CC`/`CXX`/`AR` stand-ins, per-target graph check and allowlist, and new wording; platform tags in `budget.toml`; sha2 instead of `/usr/bin/shasum`; curl and unzip found on PATH; the oracle's Emacs found on PATH | About 0.15–0.25k dev lines | Normal merge |
| 5 | **`cargo xtask ci --linux`** in an aarch64 container on colima, with the fonts mounted through `DIE_FONT_DIR`. Records `uname -r` once. | About 30–50 lines | **Starting the VM and pulling `rust:1.96.0` pinned by digest.** Approved 2026-10-19 |
| 6 | **Seams in test plans, before the code exists:** R2 states the frame-data boundary and asserts the instance layout. R3 and R4 compile on Linux, and modifier sides come from modifier-key events. K6's change count "changes when another app copies". K12 puts the session path behind `sys::paths` and uses `File::lock`. F4a and K12 run the first-frame handshake over a Unix socket, not through LaunchServices. | Notes only | Test-plan approvals, as usual |
| 7 | **Path A:** open a Ghostty issue, then a PR, for signed Linux lib-vt archives on tip. The lead time is why it starts now. An AI drafts the YAML. | About 60–80 lines of their YAML | **Your OK, from your account.** Declined 2026-10-19 |
| 8 | **Docs:** §7's edits, after you answer question 11 | | Your answer. Done 2026-10-19, adjusted for your answers (§7 note) |

## 7. Proposed doc edits (not made)

*Folded in on 2026-10-19, adjusted for your answers: D10 and D17 say the Linux archive's source is open rather than path A or B, D16 has no path-B clause, die-term-engine.md §1 records the new trigger as declined, and S7 runs once a signed Linux archive exists. Following from that: D12 calls Linux a preview until SL-desktop and SL-hw pass, D15 lets a target with no archive ship without die-term, and S3L/S2L became S3L, since S2's helper needs the archive and is tested with S7.*

### scope.md

**Decision log.**
- **D1:** "FFI is confined to `die-render`'s platform modules (objc2 on macOS; the Wayland and X11 glue on Linux), `die-term`'s engine boundary, and die-core's `sys` module."
- **D5:** "DIE is a native app on macOS and Linux. There is no TUI frontend." The rationale is unchanged.
- **D10:** "one signed engine archive per supported target". The Linux source is path A, or path B if D16 allows it. "Building DIE needs only cargo and the platform's standard toolchain."
- **D12** (replaces "macOS only"): "macOS (aarch64) and Linux (x86_64 and aarch64; Wayland and X11; glibc 2.34+) are supported. Other Unixes are compile-checked only, with no terminals. Windows is out." Status: "Decided 2026-10-19 (replaces macOS only)". Rationale: your words, the Windows verdict with a link to this record's §3, and "Linux ships per question 11."
- **D13:** "winit 0.30.13 on every platform. macOS: Metal through objc2-metal and Core Text, as S1 confirmed. Linux: wgpu 30 (Vulkan, GL fallback) and fontations (skrifa, swash, fontique), pending S6. Producers never call wgpu's `write_texture`." Correct the rationale: the 5.9 ms stall came from the 2026-10-18 review probe, not S1; it's `write_texture` holding a queue lock during its copy; S1's ring on buffer uploads avoids it (main-thread p99 0.15–0.26 ms).
- **D16:** "Building DIE needs only cargo and the platform's standard C toolchain, used only as the linker driver: the Command Line Tools on macOS; gcc or clang with the libc development files on Linux. No `-dev` packages: X11, Wayland, xkbcommon, fontconfig and Vulkan load at run time. A C-building crate may sit in a build graph only on a named allowlist with a reason, and the stand-in build proves it compiles nothing." If you choose path B, add: "or, for a target upstream doesn't publish, DIE's hosted CI built it reproducibly on two runners from source upstream signed, at a commit whose tests passed."
- **D17:** the archive exception covers one archive per supported target.
- **D18:** "the Cmd layer" becomes "the DIE layer": Cmd on macOS, Ctrl+Shift by default on Linux, set in config. The leave chord is the layer's Esc: Cmd-Esc on macOS, Ctrl+Shift+Esc on Linux.
- **D20:** the design is unchanged. Add the per-OS mechanics: kqueue on macOS; on Linux, pidfds kept in the holder, with status after a restart on kernel 6.15+ only, and a systemd user scope for the holder if SL-desktop shows it's needed.
- **D21, D23:** unchanged. M2's real day stays on the Mac. Linux work lands inside M0's phase 0 and M2 rather than as a new milestone.

**Guards.** Guard 7's die-render line becomes §5.7's per-build rule with the 7k total. §12's `unsafe` paragraph: `sys` is per-OS files, and die-render's platform has `macos` and `linux` modules.

**Sections.**
- **§1:** "a native app for macOS and Linux".
- **§5.6:** "Option acts as Alt" becomes "Alt (Option on macOS) acts as Alt". Leaving passthrough uses the DIE layer's Esc. On Linux, dead keys come through xkb compose.
- **§5.8:** "`posix_spawn` with close-on-exec by default, on macOS and glibc". The env file also covers `.desktop` launches.
- **§5.9:** paths from `sys::paths`. Restart copies and renames into DIE.app on macOS, or `~/.local/lib/die/die` with `die.desktop` on Linux, and the handshake runs over a Unix socket. Exit codes after a restart, per OS. On Linux, the restored DIE takes back clipboard ownership if it owned the clipboard at exit.
- **§5.11:** frame scheduling per platform (Wayland frame callbacks, X11 occlusion, Mailbox or Fifo). The tiling rule by app_id on Linux. Latency figures are macOS's; Linux states DIE's share only. "The system clipboard".
- **§6:** "your Mac's clipboard" becomes "the system clipboard". `die.bash` follows bash's login order. Notifications through `notify-send` on Linux.
- **§7:** die-render's line names both backends.
- **§11:** "Linux and Windows (D12)" becomes "Windows (D12); other Unixes beyond compile checks; the X11 PRIMARY selection".
- **§12:** add S6, S3L/S2L, S7, SL-desktop and SL-hw to the spikes table. Add exit test (f) to M0 and M2 (§5.1). Update the crate list's die-render line. Under "Testing", add the Linux jobs and colima's role.

### extension-contract.md

- **§4:** quit is the DIE layer's Q (Cmd-Q on macOS).
- **§5:** "a macOS notification" becomes "a desktop notification".
- **§7:** "Cmd-Esc" and "the Cmd layer" become "the DIE layer's Esc" and "the DIE layer".
- **§12:** "core watches the pid with kqueue (macOS) or a pidfd held by the holder (Linux); on Linux kernels before 6.15 the status after a restart is unknown". "`posix_spawn` on macOS and glibc."
- **§18:** its rows also run in the Linux CI jobs.

### m0-plan.md

- **F2:** add the platform lint, the denylist, the cross-target check, `ci --linux`, and the Linux mode of clean-build.
- **R1:** `platform::macos`, cfg-gated, and the non-macOS stub.
- **R2–R5, K6, K12, F4a:** the seams in §6 item 6.
- **§1:** exit test (f).
- **§6, Risks:** add "Linux drifts while nobody runs it", with the per-merge cross-check and weekly colima runs as the response.

### Other files

- **.questions/README.md:** a row for question 11.
- **die-term-engine.md §1:** the CI-build fallback also triggers for targets Ghostty doesn't publish. **§5:** archives per target.
- **s1-renderer.md §1 Fallbacks:** a note that the wgpu stall figure in D13 came from the review probe, with a link here.

## 8. New spikes

| Spike | When, where | Question | Pass bar |
|---|---|---|---|
| **S6a** wgpu-Metal | After checkpoint 4, renderer track, your Mac, 4–5 days | Does S1's renderer, ported to wgpu-Metal and presenting to a real surface, meet S1's numbers? It decides whether the portable GPU path is sound, and informs the later Metal question. | Full 159×49 redraw: main-thread CPU p99 ≤ 2 ms, GPU p99 ≤ 1.5 ms. 60 s of a 60 Hz full-window producer: ≥ 99% shown, 0 bytes copied on the main thread, no main-thread block over 1 ms from uploads. Idle: 0 frames. 20 occlusion cycles: no stall over 50 ms and no unbounded `nextDrawable`. Live resize: 0 stretched frames. Exec to first frame: reported. DIE's share of typing p99 ≤ 3 ms. |
| **S6b** fontations | Same time, your Mac plus Debian 13 and Fedora 43 containers, about 2 days | Does fontations give the same grid and acceptable glyphs? | All four Term faces give 19×40 px with the baseline at 30.88. S1's check 9 passes on the fontations path. CJK, emoji and Nerd private-use icons resolve through the chain plus the coverage scan in both containers, including Fedora's COLRv1 emoji. The collection scan is off the startup path. You look at a side-by-side image (1 minute). |
| **S6c** Linux shell, headless | Same time, colima, 2–3 days | Do the window, keys, clipboard, build rules and frame pacing work on Linux without a GPU? | wgpu on lavapipe renders the fidelity page within a set tolerance of the Mac's portable render. winit opens, resizes and changes scale under Xvfb and headless sway. The 50-chord matrix through xdotool and wtype gets every `KeyEvent` field right, including modifier sides and inferred Caps Lock. The clipboard round-trips with wl-copy, wl-paste and xclip, including across a restart. 0 frames when idle or hidden. The Linux clean-build passes, and a planted `wayland-client/log` fails it. The leave chord isn't taken by GNOME or KDE defaults (checked against their keybinding lists). |
| **S3L** Linux processes | Start of M2, colima and the hosted arm runner, 2–4 days. First `uname -r`. | Do S3's survival guarantees hold on Linux? S2's helper runs the engine, so it's tested with S7. | Every S3 check at S3's bars: 20 restarts and 5 `kill -9`s with the same PIDs and no SIGHUP; 100 open-and-kill cycles leaking nothing; builds N, N+1 and 2.0; holder crash and lifetime; the shrink-wait-restore redraw with less, vim, pico and top; output with no host. Plus: 0 inherited fds in at least 1,500 spawns during receives; a controlling tty through `posix_spawn` with `SETSID`; exit status through the holder's pidfd as a non-parent (right on 6.15+, "unknown" before, never wrong); a shell that exits with unread output loses 0 lines. |
| **S7** Linux engine archive | Before M2's exit, once path A or B delivers (both declined 2026-10-19: once a signed Linux archive exists) | Does a Linux archive fit DIE's exact wiring, and does S2's helper hold on Linux? | It builds with no Zig. 81 of 86 checks, the same 5 gaps. The helper gates hold: typing +≤ 1 ms p99, redraw +≤ 1 ms, flood ≤ 1 ms per vsync, 0 idle wakeups. It links next to `compiler_builtins`, unmodified or with a documented prelink. `NEEDED` is only libc, libm, libgcc_s and ld-linux. Path B only: members byte-identical from two runners. |
| **SL-desktop** | Before "supported": a real Linux desktop, or a desktop VM on the Mac (UTM; an install, unverified), half a day | Does a desktop session kill terminals with DIE? | Launched from the GNOME app grid through `die.desktop`, `kill -9` of DIE leaves every shell alive. `systemctl --user kill` of DIE's app scope leaves every shell alive. cargo resolves in a die-term shell. |
| **SL-hw** | Real hardware with a GPU (Mesa; NVIDIA if one can be borrowed), 1–2 days | Does Linux meet S1's budgets? | Under GNOME Wayland, X11 and sway: DIE's share of typing p99 ≤ 3 ms; full redraw ≤ 2 ms main-thread CPU p99; idle 0 frames; no main-thread stall over 50 ms across 20 hide and minimize cycles; no stretched frames in a live resize; exec to first frame ≤ 150 ms warm median; the real-key matrix. On failure: GL instead of Vulkan, then stricter Fifo gating, then renderer option C. |

No spike for Windows, wasmi or a native GL backend unless a trigger fires.

## 9. Risks

| Risk | Early warning | Response |
|---|---|---|
| **No Linux engine archive.** Upstream declines or is slow, and you don't allow path B. *(2026-10-19: you declined A and B; the source is open until before M2.)* | No signed Linux archive from Ghostty as M2 approaches | Revisit where the archive comes from before M2 (scope D10). Without one, Linux ships editor-only. |
| **No Linux hardware,** so "supported" can't be earned. A redraw that costs 0.65 ms on the M5 might cost 2–3 times that on an older laptop. | M2's exit with no hardware lined up | The label stays "preview", and no latency claim is made. |
| **Driver and compositor stalls:** Mesa's hidden-window FIFO stall, NVIDIA's present waits, Zed's reported wgpu hangs | SL-hw check 5 | Mailbox, frame-callback gating, a texture timeout skips the frame, GL fallback, then option C |
| **wgpu presenting to a real surface is unmeasured.** The ring probe didn't present. | S6a | Fix in S6a, or reconsider the Linux GPU path before LR work starts |
| **Linux code nobody uses rots.** | A Linux-only test failure found weeks late | The per-merge cross-check, weekly colima runs, hosted Linux jobs from M2, the portable backend runnable on the Mac |
| **die-render's per-build cap is tight** (macOS 4.1–5.25k, Linux 3.85–5.05k) | A module over its ratchet | The cut list |
| **Phase-0 churn.** The rename touches PRs #1–#5. | A rebase that isn't mechanical | It's a field rename; the guards land on `main` separately |
| **Exit status after a restart is "unknown"** on kernels before 6.15 (Ubuntu 24.04, Debian 13). Non-parent reads of `PIDFD_INFO_EXIT` are inferred, not documented. | S3L | Shown as "unknown", never wrong. The helper-as-parent alternative is ~60 lines if you want it. |
| **systemd-oomd or scope teardown kills DIE, the holder and every shell together.** Unverified per desktop, and colima can't test it. | SL-desktop | The holder's own user scope |
| **Allowlisted C builders become live** through a feature change in a dependency | The Linux stand-in build fails | The stand-in build runs before merges that touch dependencies; review the allowlist on every winit bump |
| **Wayland clipboard:** focused client only, background OSC 52 writes may be refused, a yank leaves with the old DIE on restart | S6c's restart round trip | The restored DIE takes ownership back at the first input event |
| **The Ctrl+Shift layer takes chords from terminal programs,** and the leave chord may clash with a desktop default | S6c | Both are config values; Super is the alternative |
| **Linux text looks different:** grayscale, unhinted, other fallback faces | S6b's side-by-side | Accepted. skrifa's hinting is an option later. |
| **Unusual library layouts** (NixOS) need `LD_LIBRARY_PATH` for libraries DIE loads at run time. A missing libvulkan falls back to GL. | A user report | Document it |
| **colima:** aarch64 only, 2 vCPUs, kernel unknown until boot | `ci --linux` too slow to run weekly | Bring one hosted Linux job forward from M2 |
| **This record's estimates** (line splits, weeks, dates) are unmeasured where marked | | Ratchets reset at the first measurement; dates move with review speed |
