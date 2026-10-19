> **Decision record for spike S3 (scope D20).** Spike code is in [spikes/s3-fd-store/](../../spikes/s3-fd-store/). The original question and your answer are in [.questions/06](../../.questions/06-restart-guarantees.md). Evidence paths under `/private/tmp` are scratch files and may not last; `scripts/run-all.sh` reproduces all of them.

# S3 decision record: terminal survival (`die --fd-store`)

Date: 2026-10-19. Status: **accepted 2026-10-19.** The proposed edits are folded into scope.md and extension-contract.md. The two questions in §4 were decided as recommended, and you can veto either: die-core gets a small hand-reviewed `sys` module for processes and PTYs, and shells left by a crash with no relaunch live until logout.
Measured on this Mac: macOS 26.5 (25F71), arm64, rustc 1.96.0, rustix 1.1.5, libc 0.2.191. No C was compiled, nothing was installed, and the builds ran with `--offline`.

## 1. Verdict

**Pass. Keep D20's holder as designed.** DIE's own binary in a second mode, `die --fd-store`, holds every PTY. Shells came through 20 planned restarts, 5 × `kill -9` of the host and a `kill -9` of a `.app`, with the same PIDs and no SIGHUP. No output was lost, except a line or two that a host killed with `-9` had already read. 100 open and kill-buffer cycles leaked nothing. A holder from build N served build N+1 and cleanly refused an incompatible build. Every row in §2 was run; none is inferred.

The spike changes eight details of the design:

1. **The redraw needs a pause.** Changing the row count by one and straight back did not repaint pico or top. Probably both handle SIGWINCH later, see an unchanged size, and skip the redraw. Shrinking by one row, waiting until the program starts repainting (at most 50 ms), then restoring repainted all four programs tested: less, vim, pico and top. macOS drops a same-size SIGWINCH, as the earlier notes said Linux does.
2. **Exit codes survive a restart.** kqueue's `NOTE_EXIT | NOTE_EXITSTATUS` gives the exit status even when DIE isn't the shell's parent, because the shell runs as the same user. die-term's exit callback keeps its status.
3. **Spawn every child with `posix_spawn`.** macOS has no way to receive an fd with close-on-exec already set. In a race test, 2–8 of about 307 children (0.6–2.6%, five runs) started with std's `Command` inherited a PTY that another thread was receiving at that moment. Such a child keeps that terminal open after you kill its buffer. `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` leaked none in about 307 tries per run. Shells can be spawned that way too, using `POSIX_SPAWN_SETSID`: they get a controlling terminal and SIGHUP on hangup.
4. **The holder's sweep runs on a timer.** macOS `poll()` reports a hung-up PTY only when it's also asked about input, which would wake the holder on every byte a terminal prints. So the holder checks its PTYs every 5 s and before answering a request. DIE itself notices an exit at once.
5. **A dying shell waits for DIE.** A process that exits with unread output stays in the kernel's exit path, even after `kill -9`, until something reads the PTY. Nothing is lost: the new DIE reads the last output, then sees the end. The holder can't drop such a terminal until then.
6. **Output pauses rather than drops.** While no DIE is reading, the kernel holds about 1 KB per terminal. Then the program blocks in `write()` until DIE is back. After a 3 s gap all 546,447 lines of a flood arrived in order.
7. **A holder crash while DIE is down kills every shell,** within 2 ms. While DIE is running it's harmless: DIE still holds the PTYs, notices, and moves them into a fresh holder (3.1 and 3.6 ms in two runs). So the holder stays tiny. If S2 puts each engine in a helper that also holds its PTY, that helper is a second holder.
8. **The socket needs a short path.** macOS caps socket paths at 104 bytes. This session's scratch directory alone is 104 bytes, so the spike connects by relative path. Production uses a private directory under the per-user temp dir.

It's also bigger than estimated. The holder and its client are about 450 lines, and DIE's side about 200 more, against the "about 250 lines" in .questions/06 (§3).

### Proposed edits (not made)

**scope.md §5.9**, replace the "Terminals survive crashes" bullet with:

> - **Terminals survive crashes.** A holder process, which is DIE's own binary in a second mode (`die --fd-store`), keeps every PTY open. PTYs are handed to it over a Unix socket when they're created. Spike S3 measured it on your machine: shells survived 20 restarts and 5 `kill -9`s of DIE, including DIE launched as a `.app`, with the same PIDs. Nothing was lost but a line or two a killed DIE had already read ([records/s3-fd-store.md](records/s3-fd-store.md)).
>   - A new DIE reattaches in under a millisecond per terminal. die-term then makes full-screen programs redraw: it shrinks the terminal by one row, waits for the program to start repainting (at most 50 ms), and restores it. The kernel drops a same-size resize, and some programs ignore a resize that's immediately undone.
>   - While no DIE is running, the kernel keeps about 1 KB of output per terminal, and then the program pauses until DIE is back. Nothing is lost.
>   - Exit codes survive a restart. DIE watches each shell with kqueue, which works even though the shell's parent is gone.
>   - The holder never closes a live terminal by itself. It exits a few minutes after it holds nothing.
>   - If the holder dies while DIE is down, every shell is hung up. So it stays tiny, and a running DIE moves its terminals into a fresh holder if the old one dies.

**scope.md D20**, Status column: `Decided (2026-10-19); holder confirmed by S3 (2026-10-19)`.

**scope.md §12, S3 row**, Question column:

> Passed 2026-10-19 ([record](records/s3-fd-store.md)). `die --fd-store` kept every shell through 20 planned restarts and 5 × `kill -9`, also as a `.app`, with no output lost in the kernel. 100 open and kill cycles leaked nothing. A holder from build N served build N+1 and refused an incompatible major. Shell exit is seen in under 1 ms without being the parent.

**scope.md §5.8**, add to the Process API bullet (suggested, not requested): "Every child, shells included, is started with `posix_spawn` and close-on-exec by default, so no child can hold another terminal open."

**extension-contract.md §12**, replace the first PTY bullet with:

> - A PTY is registered with the holder when `pty()` creates it, so it survives restarts and crashes (scope §5.9). The holder takes labelled fds from any DIE process (`term/<id>/pty`, `term/<id>/engine`), so a terminal's helper process can reach its PTY, and a restarted DIE can reach the helper.
> - After a restart DIE isn't the shell's parent. The exit callback still gets the status: core watches the pid with kqueue (`NOTE_EXIT | NOTE_EXITSTATUS`). The end of output on the PTY marks the final text.
> - Children are started with `posix_spawn` and close-on-exec by default, so no child holds another terminal open after you kill its buffer.

**extension-contract.md §14**, append to the "Terminals" bullet:

> The holder decides which terminals exist. A saved terminal the holder no longer has ended while DIE was down, and it's restored as exited. A terminal the holder has but the session doesn't (DIE crashed before writing the session) is reattached as a fresh terminal, never dropped.

## 2. Pass criteria, measured

Final run: `scripts/run-all.sh` into `/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s3/final/`. Every check also ran on its own earlier with the same results. The helper check's first run was marked failed only because my pass condition expected the wrong reply text.

| Check | Target | Measured | Result |
|---|---|---|---|
| 20 planned restarts (save, spawn new host, exit) | Shell PIDs unchanged, no SIGHUP, a long-running child keeps running, output not lost beyond the PTY buffer | 21 hosts, 20 restarts. The same 3 shell PIDs throughout. 0 HUP lines. All 3 background writers alive at every restart, and 0 of their numbered lines missing. 63/63 pings (p50 0.05 ms). Gaps between hosts were 303 ms, and 3,009 ms every 5th restart. After a 3 s gap the kernel held 999–1,026 bytes and the writers had paused. Host fds steady at 7, holder at 9. | Pass |
| 5 × `kill -9` of the host | Shells survive; a new host reattaches and can read and write | After each kill: both shells and the holder alive, 0 HUP. 5/5 new hosts reattached (0.26–0.35 ms) with the same PIDs, writers alive, 10/10 pings. 0–2 lines went missing per kill. That fits lines the killed host had read but not yet recorded; the restart and buffer checks lost none. | Pass |
| 100 open/kill-buffer cycles | No leaked processes or fds; kill-buffer delivers HUP | 100 cycles in 0.99 s. HUP delivered 100/100: each shell's trap wrote one line. 100/100 background jobs gone. Host fds 4 → 4, holder fds 6 → 6, 0 held, 0 tombstones. No marked process left, and the holder then exited when idle. Kill-buffer to shell gone: 2.5 ms. | Pass |
| Store from build N serves build N+1 | Handshake works; incompatible major refused cleanly | N+1 host on an N holder: works, with `cwd` defaulted to empty. The N holder kept N+1's `cwd`, N+1 read it back, and N ignored it. A 2.0 host got `ERR code=incompatible-major` and exited 3, with the holder and shells unaffected. An N host was refused by a 2.0 holder. An N host worked with an N+1 holder. Takeovers N → N+1 and N → 2.0: all shells alive, 0 HUP, old holder gone. | Pass |
| Shell exit detected without being the parent | Within 100 ms | 15 trials on shells this host wasn't the parent of (5 SIGKILL, 5 `exit 7`, 5 `echo; exit 3`). EIO after 0.25–0.38 ms, NOTE_EXIT after 0.25–0.39 ms, and the right status every time (signal 9, exit 7, exit 3). The final output was read before EIO in 8/8 trials. With the read delayed 500 ms, both arrived at about 500 ms. | Pass |
| Launched as a `.app` | `open`, `kill -9`, shells survive, relaunch reattaches | Launched with `open -n -g`, so its parent was launchd. It started the holder, whose parent became launchd too. After `kill -9` of the app, the holder and both shells were alive, 0 HUP. A relaunch reattached the same PIDs and pinged both. After that instance exited normally they were still alive, and a third launch reattached again. | Pass |
| Store crash | Report and recommend | With a host attached, the shells lived. The host saw the holder die and moved both PTYs into a new holder in 3.1 ms. A later host `kill -9` and reattach worked. With no host attached, both shells got SIGHUP 1.3 ms after the holder died, and exited. | Reported (§4) |
| Store lifetime and single instance | Defined and implemented | A second holder was refused (exit 3, "already running"). After detaching, its parent was launchd. It stayed up while holding a terminal, and exited 2.2 s after the last DROP (idle exit 2 s), removing its socket. After a `kill -9` the next client started a new holder over the stale socket. A 0755 directory was refused. A wrong uid (simulated) was dropped and logged. 1.5 MB resident, no CPU. | Pass |
| Reattach redraw and same-size SIGWINCH | Check macOS | Same size: 0 SIGWINCH in 3 tries, and 0 bytes from less, vim, pico and top. Rows ±1 with no pause: less and vim repainted, pico and top didn't. With 10, 20 or 50 ms between: all four did. Shrink, wait for output, restore: all four did, after waiting 0.1–0.3 ms. | Pass with a change (§1 item 1) |
| Output beyond the PTY buffer with no host | Measure and report | The writer completed 93 writes (11 bytes each on the master), then slept in `write()` for the rest of 2 s. On attach the first read was 1,024 bytes, then lines 1 to 532,340 arrived in 1 s with no gaps. | Reported |
| API usable by any DIE process (S2's helper) | Deposit and retrieve labelled fds | A helper took `term/1/pty` and deposited `term/1/engine`. Two later hosts reached the helper through the holder. After `kill -9` of the helper, the sweep made `term/1/engine` a tombstone within 1 s while the PTY stayed live, and a new helper took over. | Pass |

Leftover processes after every check: none. Each check kill-buffers what it started, waits for the holder's idle exit, and lists any process still carrying its marker. All lists were empty, and `ps` showed no spike processes after the final run.

## 3. What was built

One cargo project, `spikes/s3-fd-store/`, with an empty `[workspace]` and pinned `rustix =1.1.5` and `libc =0.2.191`. It builds one binary, `die`, with these modes:

- `die --fd-store --dir D [--daemon] [--idle-exit S] [--sweep S] [--takeover]`: the holder.
- `die host MODE`: a fake DIE host. It spawns throwaway `bash --norc --noprofile` shells on PTYs, each with a HUP trap that writes to a file and a background job that prints numbered lines. It deposits the masters, reattaches, pings, and counts lost lines.
- `die check NAME|all`: the drivers that start, kill and restart hosts, and write a summary.
- `die probe`, `die race`, `die ctty`: the kernel facts in §4.

Build N+1 is the same source with `--features proto-n1`: protocol 1.1, and terminals carry an extra `cwd` field. `--features proto-major2` builds protocol 2.0, which a 1.x holder must refuse.

| File | Code lines | What it is |
|---|---|---|
| `src/proto.rs` | 186 | Frames, fd passing (SCM_RIGHTS), messages, peer uid and pid |
| `src/store.rs` | 324 | The holder: private-dir check, lock, socket, loop, verbs, sweep, idle exit, takeover |
| `src/client.rs` | 139 | The client API any DIE process uses |
| `src/pty.rs` | 192 | PTY open and spawn, window size, redraw nudges, fd counting |
| `src/host.rs` | 239 | The fake host's terminals: spawn, attach, ping, lost-line tracking, kill-buffer |
| `src/modes.rs` | 567 | Host modes the checks run |
| `src/checks.rs` | 632 | Check drivers and aggregation |
| `src/probe.rs` | 388 | Kernel probes |
| `src/json.rs`, `src/main.rs` | 144 | Result output and dispatch |
| `scripts/*.sh` | 53 | `run-all.sh`, `replace-binary.sh` |

Code lines exclude blank and comment lines. The production-shaped part is `proto.rs`, `store.rs` and `client.rs`: 649 lines.

### The holder

- **Where.** A private directory: the holder refuses to start unless it's owned by you with mode 0700. The socket is created under umask 077 (mode 0700, owner only). Every connection is checked with `getpeereid`, and a different uid is dropped without a reply.
- **One per user.** An `flock` on `store.lock`. A second holder exits with "already running". A holder killed with `-9` leaves a stale socket, and the next one removes it, since it holds the lock.
- **Started on demand.** The first DIE process that finds no holder starts `die --fd-store` in a new session. The holder binds its socket, then detaches, so its parent becomes launchd. That happened both from a shell and from the `.app`.
- **Lifetime.** It never exits while it holds a live fd, and it never closes one on a timer. It exits once it has held nothing and had no client for `--idle-exit`; the spike's checks use 2–3 s, and I'd ship 5 minutes. Starting a fresh one is cheap: in the store-crash check, starting a new holder and depositing two PTYs took 3–4 ms. Quit (Cmd-Q) closes every terminal, and the holder then exits by itself.
- **Sweep.** Every 5 s, and before answering LIST, GET or STATUS, the holder drops fds whose other side hung up. It keeps a tombstone (label and time), so a restored DIE can say "this terminal ended while DIE was down". The checks swept every second, and dropped shells killed with no host attached 0.9 s later.
- **Cost.** 1.5 MB resident, 0.00 s of CPU over 3.5 s idle while holding a terminal (`ps`). It never reads or writes a terminal.

### The protocol

A Unix stream socket, strictly one request and one reply. Each frame is a 4-byte length and a text body, with at most one fd attached. The body is `VERB k=v k=v` plus one `item` line per list entry.

- **Handshake.** `HELLO proto=1.0 role=host build=…` gets `OK proto=… build=… pid=… held=…`. A different major gets `ERR code=incompatible-major` with the holder's version, and the connection closes. The client reports it and exits; the holder and its shells carry on.
- **Verbs.** `PUT label + fd + metadata`, `GET label` (returns a duplicate), `LIST`, `UPDATE`, `DROP`, `STATUS`, `EXIT`.
- **Compatibility.** Metadata is opaque to the holder: it stores the key-value pairs as they arrive and returns them unchanged. Readers ignore keys they don't know and default the ones that are missing. Build N's holder kept build N+1's `cwd` field, and N+1 read it back. Build N read its own fields and ignored `cwd`.
- **Frozen forever:** `HELLO`, the `proto` and `code` fields, `LIST`, `GET` and `EXIT`.
- **Upgrade.** `die --fd-store --takeover` copies every fd out of the running holder, asks it to exit, and takes its place, so the fds are never without a holder. This worked from N to N+1. It also worked across a major bump, N to 2.0, because the old holder lets another holder use the frozen verbs whatever its major.

### The API for any DIE process

```rust
Store::connect(dir, role) -> Result<Store, Error>          // role: "host", "helper", "store"
Store::connect_or_spawn(dir, role, exe, args) -> Result<Store, Error>
store.put(label, fd, &meta, replace) -> Result<(), Error>  // store keeps a duplicate
store.get(label) -> Result<(OwnedFd, Msg), Error>          // a duplicate plus metadata
store.list() -> Result<Vec<Kv>, Error>                     // label, live or exited, metadata
store.update(label, &meta), store.drop_label(label), store.status(), store.exit(..)
// Error: Io, Refused { code, store_proto, store_build, msg }, Missing, Exited, Exists, Other
```

Labels are free-form and namespaced by convention. The helper check ran S2's likely shape: a helper process took `term/1/pty` from the holder and deposited one end of a socketpair as `term/1/engine`. Two hosts started after it found the helper through that label and got replies. When the helper was killed, the sweep turned `term/1/engine` into a tombstone within a second, the PTY stayed live, and a new helper took over.

### How to rerun

```sh
cd spikes/s3-fd-store
scripts/run-all.sh /private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s3/final
# or one check, from inside a private (0700) directory:
target/release/die check restart --base <dir>     # also kill9, cycles, exit, lifetime,
                                                  # store-crash, winch, buffer, helper
target/release/die check versions --base <dir> --n1 target/n1/release/die --m2 target/m2/release/die
target/release/die check app --base <dir> --app-dir target/app
```

`run-all.sh` builds the three variants (`target/release`, `target/n1`, `target/m2`), runs the probes, then `die check all`. It takes about 3 minutes. The `.app` is built at `target/app/S3Host.app`: an `Info.plist` with `LSUIElement`, and the binary copied into `Contents/MacOS`.

### Production size

| Part | Spike | Production estimate |
|---|---|---|
| Wire protocol | 186 | ~100, with serde_json (already in die-core) instead of the text codec |
| Holder | 324 | ~250, without the spike's logging and fd counting, and `posix_spawn` instead of `fork` |
| Client | 139 | ~110 |
| DIE's side: register in `pty()`, reattach and redraw, move to a fresh holder, exit status, kill-buffer, `posix_spawn` | test-shaped, in `host.rs`/`pty.rs` | ~200 |
| **Total** | | **~650** (holder and client ~450) |

That's about 2.5 times the "about 250 lines" in .questions/06. The core-libs review budgeted 650 lines for the session, the store and its client together. The store side alone now fills that, so the row grows by whatever the session needs.

## 4. Surprises, risks and decisions

### Kernel facts measured here

| Fact | Evidence | Consequence |
|---|---|---|
| Same-size `TIOCSWINSZ` sends no SIGWINCH. A pixel-only change does. | P3, winch check | A redraw needs a real change. A pixel-only nudge repainted less, vim and top, but not pico. |
| Rows +1 then back with no pause: pico and top don't repaint. A 10 ms pause or more: all four do. Shrink, wait for output, restore: all four, after 0.1–0.3 ms. | winch check | die-term shrinks, waits for output (at most 50 ms), restores. Shrinking keeps programs from drawing below the engine's grid. |
| `poll()` with no events never reports POLLHUP on a PTY master or a socket. With POLLIN it reports `IN|HUP`. | P5, P6, P6b | The holder's sweep polls with POLLIN and a zero timeout, on a timer. |
| kqueue `EVFILT_READ` reports EOF on a master, but ignores `NOTE_LOWAT` for ttys, so it also fires on output. | P7 | Not used for the sweep. |
| A process exiting with unread PTY output waits in exit (state `E`), even after SIGKILL, until the master is read. `TIOCSDRAINWAIT` is refused on the master. | P1 | Final output is never lost. The holder can't sweep such a terminal until DIE reads it. |
| When the session leader exits, the kernel revokes the tty. A background job ignoring HUP doesn't delay EIO. | exit check, bg-holder trials | EIO and NOTE_EXIT arrive together. |
| `NOTE_EXITSTATUS` works for a non-child of the same user. | P2, exit check | Exit status survives a restart. |
| PTY output buffer: 1,034 bytes, whatever the baud rate. The writer then sleeps in `write()`. | P4, buffer check | Programs pause during a long gap; nothing is lost. |
| A master whose slave was never opened shows nothing to `poll()`, and `TIOCSWINSZ` on it fails with ENOTTY. | P0, ctty probe | Set the size after opening the slave (the spike does). |
| Received fds arrive without close-on-exec. 0.6–2.6% of children spawned with std `Command` during a receive inherited the PTY (fd type vnode). With `POSIX_SPAWN_CLOEXEC_DEFAULT`: 0. | race probe, five runs | Spawn every child with `posix_spawn`. |
| `posix_spawn` with `SETSID` and the slave opened as a file action gives a controlling tty (`Ss+`) and SIGHUP on hangup. | ctty probe | Shells need no `fork` and no `pre_exec`. |
| Cargo writes a new inode for the binary on every build. Overwriting the running holder's executable in place, or launching a binary overwritten in place, did no harm here. | `replace-binary.sh` | Not reproduced, but the DIE.app script should still copy and rename. |

### Risks

- **Holder crash while DIE is down.** It's the one way to lose shells: measured HUP within 2 ms. Mitigations: keep it small and dependency-free (only libc and rustix); DIE moves its terminals to a fresh holder whenever it sees the old one die (measured, 3–4 ms); S2's helpers, if they hold their PTYs, are a second holder. Logout kills everything anyway.
- **An old holder runs old code** until a takeover. A holder bug stays live until DIE does a takeover or you quit every terminal. The takeover exists and works; when to trigger it is in the decision list below.
- **A pid can be reused** after a shell dies while DIE is down. Store the shell's start time with the pid, and check it before watching the pid. Not built in the spike.
- **The fd limit.** The holder spends one fd per terminal, plus one per helper channel. Both limits were logged in the final run: the holder started from the `.app` had a soft limit of 256 fds, against 1,048,576 when started from a shell. DIE and the holder should raise their soft limits at startup; I didn't test that.
- **The takeover has a small race.** Between the old holder exiting and the new one binding, a client that finds no socket starts its own holder. If that one wins the lock, the new holder gives up holding fds it already took, and shells with no other holder are hung up. Not seen in the spike, where nothing else connects during a takeover. Fix in production: the new holder deposits into whichever holder won.
- **The `.app` relaunch is a new instance.** I launched it with `open -n`. DIE's own `restart` will start the binary directly, which wasn't tested as a `.app` child.
- **The uid check was simulated.** I can't create a second user without sudo, so the holder was told to expect uid 0 instead. It dropped the connection and logged the refusal. The real `getpeereid` comparison is the same code.

### Decisions I made (veto any)

1. The holder never closes a live terminal on a timer. Shells left by a crash you don't follow with a relaunch live until logout, as in tmux.
2. Idle exit: 5 minutes after the holder holds nothing and has no client.
3. The sweep runs every 5 s and before LIST, GET and STATUS.
4. Exit detection: EIO on the master marks the end of the terminal's text; kqueue `NOTE_EXIT | NOTE_EXITSTATUS`, registered at attach, supplies the status. Both arrived within 1 ms. kqueue alone would do, but the reader sees EIO anyway, and EIO comes after the final output.
5. Redraw on reattach: one row fewer, wait for output or 50 ms, restore.
6. Takeover happens only when DIE finds a holder speaking a different protocol (any minor or major). Same protocol, old build: leave it alone.
7. Socket: `fd-store.sock` in a 0700 directory under `confstr(_CS_DARWIN_USER_TEMP_DIR)`, which keeps the path well under 104 bytes.
8. Every child is spawned with `posix_spawn(POSIX_SPAWN_CLOEXEC_DEFAULT)`, and PTY children add `POSIX_SPAWN_SETSID`.
9. Quit (Cmd-Q) drops every terminal; the holder exits on its own when it's empty.

### Questions for you

1. **A third `unsafe` module.** Scope §12 allows `unsafe` only in die-render's platform module and die-term's engine boundary. PTYs and the holder need about 80 lines of libc calls that rustix doesn't wrap on macOS: `posix_spawn`, `getpeereid`, kqueue, and `posix_openpt` with close-on-exec. Std's `Command::pre_exec` is unsafe too, so there's no safe route. Should die-core get a small, hand-reviewed `sys` module for processes and PTYs?
2. **Orphaned shells (decision 1).** Is it fine that a crash you never follow with a relaunch leaves shells running until logout? The alternative is an orphan timeout, such as a day, which could kill a long job.

## 5. Raw data

All under `/private/tmp/claude-501/-Users-user-workspace-die/e7311f20-b331-496c-869a-2cf4760f32af/scratchpad/s0/s3/`:

- `final/summary-all.json` and `final/summary-<check>.json`: the final run, one summary per check.
- `final/<check>/`: each check's files. The holder's log is `store.log`. Per-host results are `chain.jsonl` (restart), `serve-<n>.json` (kill9), `cycles.json`, `exit.json`, `ver-<tag>.json`, `app-<phase>.json`, `flood-attach.json` and `kill-all.json`. Each shell's HUP trap writes `hup-<label>.txt`.
- `final/probe/probe.txt`, `final/race/race.txt`, `final/race/ctty.txt`, `final/replace-binary.txt`: the kernel probes.
- `final-run.log`: the build and run output.
- Earlier single-check runs: `restart/`, `kill9/`, `cycles/`, `exit/`, `versions*/`, `lifetime*/`, `storecrash*/`, `winch/`, `buffer/`, `helper/`, `app/`, `probe/`, `race/`, and `summary-<check>.json`.
