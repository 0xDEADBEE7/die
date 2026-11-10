# DIE: Extension and surface contract

**Design · drafted 2026-10-09 · revised 2026-10-19 after review · no code yet**

This doc expands §5.4 (surfaces) and §5.10 (extension contract) of [scope.md](scope.md). It describes the shape of the contract and its rules, not the final API. Signatures are outlines, not Rust.

On 2026-10-19 you confirmed C1–C13. C2, C4 and C9 are amended by what the review's Rust prototype found, and C14–C17 are new. Spike S4 checked the details on 2026-10-19 ([records/s4-core-api.md](records/s4-core-api.md)), and you accepted its recommendations. Its clarifications are folded in below.

> **This doc defines what DIE's core has to provide. It doesn't plan any extensions.** seek, scrib, manifold, orchid, termin, delta, `die-seek` and the markdown view appear only as requirements, meaning "the core must make this possible". You'll build them yourself later, separately, but for the ports of your packages that you choose: those are built in this repo under `extensions/`, outside the product, its budget and core, on die-core's public API alone, until each moves to its own repository at M4 (scope D14, amended 2026-11-09, at your word: "not part of die itself as the core product but similar to my emacs-orchid-client"). The first is die-orchid-client. The Vim grammar (`die-vim`) and kitty-level terminals (`die-term`) are part of DIE itself and are built here (scope D15). Internally they're crates that use this same contract.

## 1. What your packages actually need

I counted the Emacs primitives used by your own packages (seek, scrib, manifold and orchid, excluding tests). delta was added to the requirements check later, and is covered in §18:

| Need | Emacs primitive (uses) | Packages |
|---|---|---|
| Per-buffer state | `defvar-local` (orchid 30, scrib 7, manifold 5) | all |
| Act on a specific buffer later | `with-current-buffer` (orchid 31), `buffer-live-p` (orchid 21) | orchid |
| Keymaps and initial modal state | `define-key` (orchid 31, scrib 8, manifold 3), `evil-define-key`, `evil-set-initial-state` | all |
| Styled text | `propertize` (orchid 25, seek 8), `defface` (orchid 15) | all |
| Protected regions | `inhibit-read-only` (orchid 10, seek 1) | orchid, seek |
| Hidden or replaced ranges | `make-overlay`, `overlay-put` (manifold 5, orchid 4) | manifold, orchid |
| Processes | `make-process` (orchid), `call-process` (scrib, blocking) | orchid, scrib |
| File watching | `file-notify-add-watch` (orchid) | orchid |
| Timers | `run-with-timer`, `run-at-time`, `run-with-idle-timer` (orchid 6) | orchid |
| Prompts | `completing-read`, `read-string` | scrib, orchid |
| Options | `defcustom` (orchid 9, scrib 4, seek 3, manifold 2) | all |
| Cleanup | `kill-buffer-hook` | orchid |
| Overriding someone else's code | `fset` on `evil-next-line` (manifold), `advice-add` (your `init.el`) | manifold, config |

Four of these shape the contract:

1. **orchid checks `buffer-live-p` 21 times.** Async callbacks outlive their buffers. The contract should make a dead buffer something you can't forget to handle, instead of something you have to remember to check (C4).
2. **orchid wraps edits in `inhibit-read-only` 10 times.** In Emacs, read-only text blocks the package's own edits. In DIE, read-only blocks only edits that come from you, the user (C14, §9).
3. **manifold replaces `evil-next-line` with `fset`,** because Evil had no extension point for skipping folds. Rust can't patch functions at runtime. Either core provides the behavior (line motions skip hidden ranges natively) or you edit the crate, which is yours (C13).
4. **seek, scrib's browser and orchid's session browser redraw a read-only buffer from their own state.** They erase it, insert styled lines and track the selected row themselves. Their search modes bind every printable character in a loop and read `last-command-event`. orchid even patches `window-width` with `cl-letf` to limit the width of seek's rows. The core should let this pattern be ported as-is, with small primitives replacing the hacks (§9, "Browsers rendered as text").

## 2. Shape

```
your config crate (main)
  └─ extensions (typed values), bindings, theme, buffer defaults, environment, startup
          │  register (once, at startup)
          ▼
  Registry: commands · keymaps · modes · surface kinds · faces · hooks · restorers
          │
          ▼
  Workspace (single-threaded) ──── Ctx ────▶ every call into extension code:
          ▲                                   commands, callbacks, events, hooks
          │                                   (calls that get Ctx are queued, C3)
          │
          └── messages from threads and helpers: jobs, processes, watches,
              pixel producers, terminal engines
```

## 3. Decisions

| ID | Decision | Rationale |
|---|---|---|
| C1 | One context type, `Ctx`, is passed mutably to every command, callback, event handler and hook. Extension code changes the editor only through it. | One thing to learn. Helix's commands work the same way. |
| C2 | *(Amended)* Borrowing uses a **checkout** pattern. `with_surface`, `with_mode` and `with_ext` take an object out of the workspace, hand it to a closure along with `Ctx`, then put it back. They return a `Result`:<br>- `Busy` if the object is already checked out<br>- `Gone` if its buffer was killed (never for `with_ext`)<br>- `Missing` if there's no such object: the extension isn't registered, the buffer has no such mode, or its surface is another type<br><br>Rules:<br>- A panic inside the closure puts the object back before the panic continues.<br>- Killing a buffer while its surface or a mode is checked out marks it dead at once, and the object is dropped at check-in. A killed buffer's modes are dropped, not disabled: `on_disable` doesn't run.<br>- Core-owned data (text, windows, markers, decorations) is never checked out; it's borrowed by id. | The usual way around Rust's rule that you can't mutably borrow the editor and a part of it at the same time. Zed's GPUI uses the same lease pattern. Spike S4 passes re-entry and panic tests for all three, and kill-during-checkout tests for surfaces and modes. `Busy` keeps a re-entrancy bug from looking like a dead buffer. |
| C3 | Async work uses callbacks, not futures. A callback is a one-shot `'static` closure that captures ids, never references, and runs on the main thread. Calls that receive `Ctx` (hooks, callbacks, prompt answers, `on_enable`, `on_disable`, `on_resize`) are queued and run from the top of the event loop. They are never nested inside another extension call, except through an explicit `ctx.run` or `ctx.reveal`. Calls that get no `Ctx` (`render`, `decorate`, key observers) run synchronously. | Holding `&mut Ctx` across `.await` points is painful in Rust. Callbacks keep the single-threaded model simple. Cost: multi-step prompts nest (§12). A small async layer of about 100 lines can be added later without a redesign. |
| C4 | *(Amended)* Buffers, windows, markers and decorations are referred to by generational ids. Looking up a dead id returns an error, and the type makes the caller handle it. Async subscriptions return handles. Dropping a handle cancels the subscription, and `.detach()` ties it to the extension instead. A handle stored in a mode is cancelled when its buffer is killed. If the mode is checked out at the time, the handle is cancelled when the mode drops at check-in. | Replaces orchid's `buffer-live-p` checks with something you can't forget. Handles also cover mode-owned timers and per-request processes, which an owner argument didn't. |
| C5 | After each batch of events, core re-renders every visible surface. Extensions never request redraws. Pixel buffers are the exception: they present their own changes (§11). | No forgotten-redraw bugs. Diffing cells keeps it cheap, and surfaces cache expensive layout themselves. |
| C6 | **Support faithful ports.** The contract has to let you rebuild your existing packages later with the same structure they have in Emacs: text buffers, modes and keymaps. That includes seek-style browsers that redraw a read-only buffer, and orchid's conversation buffer. **Custom surfaces** are for things text can't do: terminals, pixel views, and new graphical extensions. | The graphical work is for things you couldn't build in Emacs. What already works shouldn't be redesigned, and text buffers get Vim motions, search, undo and yank for free. |
| C7 | Extension options are typed fields on the extension's struct, set in your config. There's no `defcustom`-style registry. | Checked at compile time, with no machinery. Cost: no runtime customize; you change config and rebuild. Core buffer settings (wrap, width, line numbers) stay changeable at runtime. |
| C8 | Keymaps bind keys to command names. Extensions provide default keymaps. Your config is applied last, so within any keymap it binds into, it wins; to override a mode's or die-vim's key, bind in that mode's or surface kind's keymap. After config is applied, core checks that every bound name exists and reports all missing names at once. | Names keep `describe-key` working and make every binding inspectable. The startup check catches typos that strings would otherwise hide. |
| C9 | *(Amended)* Every call from core into extension code is wrapped in `catch_unwind`, on a best-effort basis.<br>- A panic becomes an error message naming the extension, and a panic hook logs it to `*messages*`.<br>- Repeated panics disable the object that panicked (a mode, a surface or a subscription), never a whole crate.<br>- DIE refuses to build with `panic = "abort"`.<br><br>*(K5, 2026-11-05: a panic in `render` or `decorate` names only the place, not the extension, and disabling comes from M1. §15 has the details.)* | A bug in one extension shouldn't take your terminals down with it. Some failures abort the process anyway: stack overflows, double panics and panics inside FFI callbacks. Surviving those is the holder's job (scope D20), not C9's. `die-term` never runs extension code inside engine callbacks. |
| C10 | A custom surface has one state, shared by every window that shows it. Text buffers keep a cursor, selection and scroll position per window, as Vim does, and the session saves them per window. | Per-window state for arbitrary surfaces is a lot of machinery for a rare case. |
| C11 | A headless test harness is part of core's public API (§16). | Your packages have ERT suites, and extensions need the same. |
| C12 | No API stability promises. Everything is compiled together, so the compiler is the migration tool. | It's a personal project. Semantic-versioning ceremony would be pure cost. |
| C13 | No advice. Behavior changes go through explicit hooks, or through editing the crate. | Rust can't patch functions at runtime, and the source is yours anyway (scope guard 2). |
| C14 | *(New)* Every edit carries an origin, `User` or `Extension`. die-vim's editing commands mark their edits `User`. Read-only ranges block only `User` edits. Undo and redo are `User` edits too. A `User` insertion is blocked only strictly inside a read-only range. At an edge it's allowed, and the range's stickiness decides whether the new text becomes read-only (§9). | Your editing commands are themselves an extension (die-vim), so "read-only blocks the user, not code" needs an explicit marker. |
| C15 | *(New)* Modes pull changes with `edits_since(rev)` instead of receiving pushed `on_edit` calls. | A mode that's checked out (C2) can't receive pushed edits, including its own. Pulling also gives providers incremental caching. |
| C16 | *(New)* In v1, decoration providers return faces only. Hidden ranges, virtual text and placements are stored decorations, so one core layout function serves both rendering and motions. | Motions need layout outside render. If a provider could hide text at render time, commands and the screen would disagree about where lines are. Spike S4 confirmed this with mid-line and multi-line Hides, so Hide isn't limited to whole lines. |
| C17 | *(New)* Modes apply only to text buffers in v1. | Nothing needs a mode on a custom surface yet, and `render` can't see modes anyway. |

## 4. Lifecycle

1. **Build.** Your config lists extensions as values, with their typed options.
2. **Register.** Core calls each extension's `register` in config order. Registration only declares things: commands, keymaps, modes (with their keymaps, initial state and restorer), surface kinds, faces, hooks, restorers and file-type associations. There's no editor access yet. Registering the same extension type twice is an error, because `with_ext` looks extensions up by type.
3. **Apply config.** Your bindings, theme, buffer defaults and settings by path are applied after all extensions, so they win within each keymap they bind into (C8). Your config can declare commands and faces here too, and a panic here is a startup error. Core then checks that every bound command exists, and reports every problem at once. Your environment joins this step in M2.
4. **Init.** Core calls each extension's `init` in order, with `Ctx`. Extensions start watchers, timers and background state here. An error or a panic is logged, and the next extension still inits.
5. **Restore.** Core restores the session (§14). Extensions are already initialized, so their restorers work. Terminals reattach to their PTYs in the holder.
6. **Startup.** Your config's startup hook runs. An error or a panic is logged, and the editor still starts. From M2 it's told whether this is a cold start or a restore, so it can open a terminal as `user/startup` does today without adding another one on every restart.
7. **Run.** The main loop.
8. **Shutdown or restart.** Core asks every surface and mode to `save()`, writes the session, then calls each extension's `shutdown` in reverse order. What an extension detached is cancelled after its `shutdown`, and what your config detached is cancelled last.
   - On restart, core starts the newly built binary and exits. Terminals stay alive in the holder (scope §5.9).
   - On quit (the DIE layer's Q, Cmd-Q on macOS, after confirming if processes are running), core tells the holder to close the terminals and exit.

```
Extension
  name()             "orchid"; prefix for its commands, faces and surface kinds
  register(reg)      declarations only
  init(ctx)          optional
  shutdown(ctx)      optional

Config               everything optional; the harness can run with none
  register(reg)      your bindings, commands, faces and leader, on the registry the extensions used
  theme
  text defaults      the settings every text buffer starts with
  file settings      settings by path, on top of the defaults, until file-type modes (M1)
  clock              real in DIE, fake in the harness (§12)
  startup(ctx)       step 6
```

An extension's global state is the extension struct itself. A command reaches it with `ctx.with_ext::<Orchid>(|orchid, ctx| …)`.

## 5. What `Ctx` offers

| Area | Operations |
|---|---|
| Buffers | create (name and surface; names made unique as in Emacs, `term<2>`), open file, find by name, list, kill, `with_surface::<S>`. Each buffer has a `directory`, like `default-directory`. |
| Windows | focused window and buffer; the size in cells of each window showing a buffer (`window-width`, `window-body-height`); `show(buffer, ShowIn)`, where `ShowIn` is the current window, a new split, a window already showing it, or background (not shown); split, close, only, and focus by direction (for `C-w`) |
| Text | `text(buffer)` and `text_mut(buffer)` borrow a buffer's text for reading, editing, markers, decorations, per-window cursors and selections, and layout queries (§9) |
| Commands | `run(name, args)`; `feed_keys(keys)`, which queues keys as if typed, ahead of further typed input, and `feed_keys_apart(keys)`, which keeps them apart from what's queued behind them (§6); `keys_pending`, whether the keys so far only start a longer one; a key observer that sees each key before dispatch, marked typed or fed, so die-vim can record macros (`qb@aq` records `@a`, as Vim does) |
| Modes | enable, disable, `with_mode::<M>` |
| Modal state | read and set the state, which is per buffer: of the focused buffer, or of any buffer by id; read and set the prefix argument (count and register) |
| Async | `spawn`, `process`, `pty`, `watch`, `after`, `every`, `idle`, `prompt` (§12) |
| Hooks | subscribe to buffer lifecycle, edits (after the fact), saves, modes, focus and idle, returning a handle (C4). A save runs its write as a queued step after the before-save hooks. |
| Pixels | create a pixel buffer and present into it, from any thread (§11); images come with M3 |
| Registers | read and write typed registers (char, line or block) and the system clipboard |
| Messages | message and error, shown in the command line and logged to a `*messages*` buffer; a note that stays at the command line's right end until it's cleared (`set_note`, die-vim's `recording @a`); a desktop notification |
| Extensions | `with_ext::<E>` |

## 6. Commands

```
command
  name         "orchid-open": kebab-case, prefixed with the extension's name
  doc          a summary line plus details, shown by describe-command
  run(ctx, args) -> result

args
  count        from the prefix argument, if set
  register     from the prefix argument, if set
  text         the rest of the line, when called as ":orchid-send hello"
  key          the key sequence or committed text that invoked the command (Emacs's last-command-event)
  window       the window it was invoked from, so async code doesn't depend on focus
  buffer       the buffer it was invoked on
  deco         for a Hide's reveal command: the decoration being revealed (§9)
```

- **The prefix argument.** Core stores a count and a register, and doesn't parse Vim syntax. die-vim's digit and `"x` commands fill the slot, and `0` is a die-vim command (scope D9). An undefined key clears the slot, as Emacs's `undefined` does, so a typo can't arm the next command.
- **No `interactive`-style argument prompting.** A command that needs input calls `ctx.prompt` and continues in the callback (§12).
- **Errors** returned from `run` are shown in the command line and logged.
- **An error ends what was fed.** When a key's command fails, by an error or a panic, or a key reaches nothing that handles it, the rest of the fed keys are dropped, so a macro stops where it fails, as Emacs's keyboard macros do. A prompt's answer is part of the key that submitted it, so `/nomatch<CR>dd` stops at the search. Typed keys stay. An error from a timer or another callback drops nothing, since it isn't part of the key stream, and neither does one a command handles itself. In passthrough only a panic ends what was fed: a terminal that ignores a key hasn't failed. `on_fed_dropped` runs a call from the top of the loop, before the next key is looked up, whenever a key's failure ends what was fed, so die-vim can leave a macro's selection or session behind first. In it, `take_quit` says whether a typed `C-g` ended them instead, as Emacs's `quit` isn't an `error`. While `hush_key_errors` is on, a failure ends what was fed without saying so, as Emacs's `ignore-errors` round a keyboard macro, for die-vim's `:norm`.
- **Keys fed apart.** `feed_keys_apart` feeds keys as `feed_keys` does, but a failure among them, or among keys they feed in turn, drops only those, and the fed keys behind them run, as a keyboard macro run inside `ignore-errors` does in Emacs. Once they've all run, with no failure, `on_fed_run_out` runs a call from the top of the loop, before the next key, fed or typed, is looked up, so what they left waiting (a key that only starts a longer one, an open prompt) can be ended before anything else reaches it. Feeds kept apart nest, and keys the host feeds meanwhile go behind all of them. A typed `C-g` still drops every fed key. die-vim's `:norm` feeds each line's keys apart, so a line that fails inside a macro, or inside another `:norm`'s line, leaves the rest of that macro or line to run.
- **A command is over.** `on_command_ended` runs a call from the top of the loop, before the next key is looked up, whenever a command has run from a key, whoever's it is, a prompt has given its answer, or a pointer press has moved the cursor, so die-vim's `C-o` can end on a command that isn't its own, as Emacs's `post-command-hook` lets Evil's do. A key that only starts a longer one runs nothing.
- **The order of fed keys.** Fed keys run ahead of typed ones. Keys fed from inside a call also run ahead of what's left of the fed keys, in the order they're fed, so a macro inside a macro runs before the rest of it, as in Emacs, and an error in it ends both. Keys the host feeds outside any call go behind what's left.
- **Introspection.** `describe-command` shows the doc, the owning extension and every key bound to the command. `describe-key` follows a key through the lookup order (§7) and reports where it was found, plus any bindings it shadows.
- **Core commands** include `find-file`, `switch-buffer`, `kill-buffer`, `command-palette` and `restart`, built on the command-line prompt, and `toggle-max-width` (§9). A picker extension can replace them by registering its own commands under the same names and declaring them as overrides.
- **Name collisions.** Overriding a core command is allowed only when declared explicitly. Any other duplicate name is a registration error.
- **Your config can register commands too.** termin doesn't need to be an extension: it becomes a ten-line command in your config, using die-term's API (scope §6).

## 7. Keymaps and modal states

- **States** (scope §5.6): normal, insert, replace, visual, operator-pending, command line, copy and passthrough.
- **Copy state** is normal and visual running over a read-only snapshot (scope D19) of a surface that gives its text (`Surface::text`, §10). Core takes the snapshot when the buffer leaves passthrough, by either Esc or `set_state_of`, or when one starts in normal state and is first drawn or gets a key, and drops it when the buffer goes back to passthrough. While it's there, `Ctx::text` and `text_mut` give it, so die-vim's grammar runs on it as on any text, with no keymap of its own: core looks a key up in the text kind's keymap right after the surface kind's, so the kind's own bindings win. die-term's are `<Esc>` (to the program, once a run of Escs, not to the shell at its prompt and not in line mode, scope D18), `i a I A` (back to passthrough), `p P` (`surface-paste`), `G` (to the terminal's cursor) and `<C-c><C-r>` (where insert's Esc goes). A terminal on the alternate screen gives that screen's rows alone, which have no scrollback. An edit fails with "Buffer is read-only", and nothing can start an insert session there (`TextBuf::is_snapshot`). The pointer is a text window's: a click moves the cursor, a drag selects, the wheel scrolls. A surface that gives no text has no copy state, and its keys in normal state go to `event()` as before; reading its rendered cells instead waits for a surface that wants it.
- **A keymap** maps key sequences to command names, per state. The notation follows Vim: `gg`, `<C-x>`, `<leader>o`, `<S-Tab>`.
  - Key names match without case. A `<` that doesn't start a key name is a literal `<`. A bracketed word that isn't a key name, or modifiers followed by something that isn't a key or by no closing `>`, is a startup error. `feed_keys` and a register `q` recorded (`RegKind::Macro`) are notation too, so a literal `<` there is written `<lt>`. Any other register holds plain text, and `@` types it as it is, one key per character, as Vim and Evil do. A recording that's pasted, edited and yanked back has become plain text, so it can be re-recorded with `q`. Literal characters that form one grapheme cluster (`👍🏽`, `🇦🇺`) are one key, as text committed by the OS is. A keymap can't bind one: that's a startup error, so bind `<char>` and read `args.typed()`.
  - Ctrl with a letter ignores the letter's case (`<C-V>` is `<C-v>`); `<C-S-v>` is Ctrl-Shift-V. Key events match the same way, so Caps Lock doesn't change what Ctrl-v matches.
  - Shift counts on named keys and with Ctrl and a letter. Written with any other character it's a startup error, because the layout already applied it and no key sends what it says: write `<C-@>`, not `<C-S-2>`, and `<Space>`, not `<S-Space>`.
  - `<C-[>` is `<Esc>`, `<C-i>` is `<Tab>` and `<C-m>` is `<CR>`, as in your GUI Emacs and in Vim, so they can't be bound apart. Only keymaps fold these, and passthrough's leaving Esc (below) reads Ctrl-[ as one: otherwise the raw event keeps the difference for terminals that report it.
  - `<A-…>` and `<M-…>` are Alt (Option on macOS). `<D-…>` names the DIE layer, which on macOS is scope D18's Cmd layer: `<D-v>` is Cmd-V there.
  - `<leader>` is the key your config sets, `\` by default. It applies to every keymap once all extensions have registered.
- **`<char>` matches any printable character,** including text committed by dead keys. No key with Ctrl, Alt or Super held is printable. The command reads which character it was with `args.typed()`. This replaces the loops in scrib's and orchid's search modes that bind every character to a self-insert command.
- **`<any>` matches any key** a keymap doesn't otherwise bind, printable or not. die-vim uses it to cancel operator-pending on an unbound key. `<char>` and `<any>` match one key where they're bound: a key that fails under a prefix isn't tried against the root's, so die-vim also binds `i<any>`, `a<any>`, `g<any>` and `f<any>` (and `F`, `t` and `T`) to cancel `di`, `dg` or `df` followed by an unbound key.
- **Specificity applies within one keymap; order applies across keymaps.** An exact key beats `<char>`, which beats `<any>`, inside the same keymap. Across keymaps, lookup order wins, so a seek-style mode's `<char>` beats die-vim's `j`.
- **A binding can't also be a prefix** of a longer binding in the same keymap and state, such as `g` and `gg`. Registration reports it, along with the C8 check.
- **Where keymaps attach:**
  - globally
  - to a surface kind (`text`, `term`). This is the equivalent of a major-mode map. `die-vim` registers its grammar as the keymap for the `text` kind.
  - to a mode (§8)
  - to a buffer (buffer-local)
  - to a range of text, through a keymap decoration (§9). This is how buttons and collapsible toggles work, like Emacs's `keymap` text property.
- **Lookup order** for a key:
  1. the range keymap at the cursor
  2. buffer-local
  3. modes, most recently enabled first
  4. the surface kind, and in copy state the text kind's after it
  5. global
  6. the surface's `event()` (§10)
  7. otherwise, an "unbound" message
- **Passthrough has its own dispatch rule.** It's insert state on a surface, and `describe-key`, the status line and the cursor say insert (scope D18).
  - The DIE layer's Esc is checked first; it leaves passthrough for normal state (a terminal's copy state) and is never sent to the program.
  - A plain Esc, with no modifier, or Ctrl-[, leaves for normal state too, unless the surface's `takes_esc()` says it's its own now: a terminal whose per-buffer toggle sends Esc to the program, always or, under evil-ghostel's `auto`, while it's on the alternate screen (scope D18). By default a terminal never takes it, on the alternate screen either. Then it goes to `event()`. Shift-, Ctrl- and Alt-Esc always go to `event()`.
  - The rest of the DIE layer is looked up in DIE's keymaps, in their passthrough bindings: the `die` facade binds `<D-q>` to `quit` and `<D-v>` to `surface-paste` there. A DIE-layer chord nothing binds goes to `event()`.
  - Every other key goes straight to the surface's `event()`: the leader, `C-c`, `C-z`, `C-g`, and every key die-vim's insert state binds in text.
  - A surface declares which keys it handles there, so `describe-key` can report them.
  - In normal state a surface's buffer looks keys up as any buffer does, its kind's keymap among them, so die-term binds `<Esc>` (sending it to the program, with its release if the kitty flags ask for releases, but not again before another key, which it counts with `Ctx::observe_keys` as a keymap reads keys, nor to its shell at the prompt, nor in line mode, `Pty::line_mode`), `i a I A`, `p P`, and `<C-c><C-c>`, `<C-c><C-z>`, `<C-c><C-\>` and `<C-c><C-d>`, which send the program those bytes, as ghostel's map does, there.
- **Press, repeat and release.** Keymaps match press and repeat. Release events, modifier keys pressed alone and keys DIE has no name for reach only surfaces, in passthrough; a release goes to the surface its press went to there, whatever state or window has the keys by then, so a program that asked for releases (kitty's flag 2) hears a key let go after Esc left passthrough: a letter rolled over into Esc, or the Ctrl of Ctrl-[. Binding a modifier key alone, or making one the leader, is a startup error. Any key DIE takes in passthrough, either Esc that leaves or a DIE-layer binding, swallows its own repeat and release, and so does a key that puts the focused buffer in passthrough from another state (die-term's `i`, or `<CR>` on a command that opens a terminal), so a program never sees a release without its press. die-render delivers every press, repeat and release, as `crates/die-render/tests/keys.rs` checks on a real event loop.
- **Initial state.** A surface kind or mode can declare the state a buffer starts in, as `evil-set-initial-state` does. Text buffers start in normal, `die-term` in passthrough (shown as insert, scope D18), and orchid's conversation mode might choose insert.
- **Conflicts.** Within a keymap, later registration wins, and `describe-key` lists what was shadowed. Your config is applied last, and your bindings go into the keymap they name (global by default). To free a key an extension bound, your config unbinds it. That removes the binding and every longer one that starts with it, and `describe-key` lists them as shadowed. This is how config binds a key alone where an extension uses it as a prefix, or the reverse.

## 8. Modes

A mode is a named bundle attached to a text buffer (C17): keymaps, state, hooks into that buffer's events, and optionally decorations. It covers both Emacs major modes (the file-type mode of a text buffer) and minor modes.

Type-level facts about a mode are declared once in the registry, so `describe-key` sees a mode's keymaps before any instance exists:

```
ModeSpec (registered)
  name                            "manifold"
  initial_state                   optional
  keymaps                         optional, per state
  new()                           creates the per-buffer instance
  restore(data, buffer, ctx)      optional (§14)

Mode (one instance per buffer)
  on_enable(buffer, ctx)          optional; queued (C3)
  on_disable(buffer, ctx)         optional; queued (C3)
  on_resize(window, size, ctx)    optional; a window showing the buffer changed size; queued (C3)
  decorate(text, visible range)   optional; a face provider (§9, C16)
  save() -> data                  optional (§14)
```

- **The mode object is its per-buffer state.** It replaces `defvar-local`. manifold's fold cache lives in the `Fold` mode instance attached to that buffer.
- **Reading changes.** A mode remembers the revision it last processed and calls `text.edits_since(rev)` when it needs to catch up: in `decorate`, in a command, or from an idle hook. The result is every change since that revision, or nothing if the revision is too old for the bounded log, in which case the mode resyncs fully (C15).
- **File-type modes.** Extensions associate extensions or file names with a mode (`.md` → markdown). The first mode on a text buffer is its file-type mode, and it sets defaults such as soft wrap and indentation.
- **Reaching a mode from a command:** `ctx.with_mode::<Fold>(buffer, |fold, ctx| …)`.

## 9. Text buffers: the extension API

The text surface belongs to core. This section covers what extensions can do with it. Each item is required by die-vim or by at least two of the future extensions this contract has to support: a syntax highlighter, folding (manifold), a markdown view, orchid's conversation buffer, scrib and delta.

**Reading.**
- Rope and line access. Reads go through a snapshot that exposes ropey's `RopeSlice` directly, so motions and text objects are plain functions over it.
- Conversions between char offset, byte offset and line/column. Byte offsets are the canonical position type.
- Regex search over a range, the first match that ends by a bound as Emacs's bounded `re-search-forward` finds it (`first_match_in`, for `:g`), and a word pattern for `*` and `#`: a literal that matches only where the characters on either side aren't word characters, by the caller's definition. Rust's `\b` can't say this, since its word characters include `_`.
- A **revision** number that increases with every change. manifold's caches use Emacs's modification tick for the same purpose.
- `edits_since(rev)`: every change since a revision (C15)

**Editing.**
- Edits happen inside a **transaction**: a set of changes in the coordinates of the text before it, applied all or none. It carries an **origin**, `User` or `Extension` (C14). A change with an empty range and empty text is dropped: it makes no revision, no logged change and no undo step, and redo stays available. Text replaced by itself is still a change and an undo step, as `ra` on an `a` is in Evil and Vim.
- **Undo groups** are separate from transactions. Each command or callback is one undo step by default, so `3dd` is one step. Edits made outside any command or callback, by the host or a harness, are a step of their own. An explicit group spans commands, as an insert session does. It takes every recorded edit to its buffer until it ends, so end it on every way out. Undoing inside a group leaves it open: the next edit starts a new step inside it. `undo_boundary` ends a step early. Undoing a step restores the selection from just before its first change (shape and goal included) and returns the range it changed; redo puts a cursor at the start of that range. So a command that moves the cursor and edits in one go, such as die-vim's `p`, `J` or `dd` from mid-line, makes its edit first, or undo restores the moved cursor. Each buffer keeps at most 2,000 steps or 16 MB of undone text, whichever comes first; both limits are buffer settings.
- **Styled insertion** inserts text together with its faces and tags in one call, the equivalent of inserting a `propertize`d string. Replacing the whole buffer is one call too, which is what seek's "erase and redraw" becomes.
- A transaction can be **silent**, meaning it isn't recorded in undo. Streaming output, like orchid's assistant text, shouldn't fill your undo history. Undo adjusts for silent edits: if you type, an extension then appends silently above your text, and you press `u`, your text comes back exactly. If a silent edit changed the text an undo step needs, undo refuses with an error and drops the older history. If that step is still being collected, the history goes at your next edit, and the undo that reaches the gap reports the same error, once.
- **Read-only ranges block `User` edits only.** Edits from extension code always succeed, which replaces orchid's `inhibit-read-only` wrappers.
- Every edit produces a **change**: the old range, the new text, and byte and line/column positions. Incremental parsers like tree-sitter need exactly this. Modes read changes with `edits_since` (§8). They're listed as they were applied, each in the coordinates of the text after the one before it. A transaction applies its changes from the last position to the first, so two inserts at one position are listed last one first. Undo and redo log their changes too, with origin `User` and not silent.

**Markers.** A marker is a position that moves with edits. Its gravity decides whether it stays before text inserted at it or moves after it. orchid's input marker and its streaming insertion point become markers.

**Cursors and selections.** Each window has its own cursor and selection (C10). A selection has a shape (char, line or block) and a goal column for vertical motion. Extensions can read and set them in a given window, or in the focused one. Text inserted at a window's cursor goes after it in the acting window and before it in every other window, as Emacs's window points do. The acting window is the one a command was run for (`args.window`), and otherwise the focused one: in a callback, in a command run with no window, and in code between commands. A command that moves the focus goes on acting for the window it moved to. An extension that wants a window to follow its output sets that window's cursor.

**Anchored decorations.** Decorations are added and removed by id, and their ranges move with edits, the way markers do. Each end has a gravity. By default, text inserted at either edge stays outside the range.

| Kind | Effect | Used by |
|---|---|---|
| Face | Styles a range | Search highlights, orchid message styling, delta's diff lines |
| Hide | Hides a range, showing nothing, replacement text, or a placement (an image or pixel buffer) | manifold folds, orchid collapsibles, markdown markup hiding and inline images |
| Virtual text | Shows text just before a position, without adding it to the buffer. An end-of-line hint anchors at the newline. | Inline hints, token counts |
| Read-only | Blocks `User` edits in a range | orchid's separators and restored line (in Emacs nothing else of its transcript is read-only) |
| Keymap | Keys pressed with the cursor in the range use this keymap first | Buttons, collapsible toggles, markdown links |
| Tag | Attaches an extension value to a range, which can be looked up by position | "Which session, section or note is under the cursor?" |

**Styled text.** A Hide's replacement and virtual text are `Styled`: text, and faces on byte ranges of it, so a plain string stays one call and manifold's fold strings keep their faces. Each cell draws its face over the faces at the position the string stands in for or before, as Emacs bases a display string on the text under it. A cluster takes the last run that holds its first byte. Styled insertion shares the shape.

**Revealing hidden text.** A Hide decoration carries a reveal policy for when search lands inside it: never reveal, remove the decoration, or run a named command with the decoration's id. The command lets the owner update its own state, as Emacs's `isearch-open-invisible` does, so manifold's fold cache stays in sync.

**Decoration providers.** A mode's `decorate` is called for the visible range during render and returns faces without storing them (C16). This is font-lock's model: syntax highlighting shouldn't have to store a decoration per token. Providers cache their results by revision, and use `edits_since` to update incrementally.

**Layout.** One core layout function serves both rendering and motions. It handles soft wrap, centered maximum width, the line-number gutter, tabs, wide characters, hidden ranges and virtual text. Its queries are:
- `viewport`: the lines and columns a window shows
- `move_visual`: move by screen line, for `gj`/`gk`
- `display_col`: the screen column of a position, for block selections
- `next_visible`: the next line that isn't hidden
- `display_line`: the display line holding a position, with its lines and cursor positions
- `pos_at_vcol`: the first cursor position at or after a display column, or else the display line's last, for `j`/`k` goals. A column inside a tab, a wide character or a replacement lands after it, as `move-to-column` does for Evil
- `snap`: the nearest cursor position in a direction, for `h`/`l`
- `screen_pos`: where a position is drawn in a window
- `scroll`, `set_scroll`, `scroll_rows` and `recenter`: which row a window shows first, set outright, moved by rows, or chosen to put the cursor on a given row. A command that scrolls a window also leaves its cursor on a visible row; otherwise core scrolls back to the cursor before the next render (C5)

**How text becomes rows.** The paragraphs from here to "Motions respect hidden ranges" are the whole rule. Someone reading only them can write a layout that agrees with K4's on every row and every cursor position, and K4's property test does exactly that. The rule is Emacs's `visual-line-mode` with `word-wrap` on, checked case by case against tty Emacs (`die-core/tests/emacs_layout.rs`). A Hide that shows a placement is M3's: `Deco::Hide` has no placement yet, and its rule gets written with it.

**Display lines.** A *display line* is a logical line, joined with the lines after it when a Hide covers its newline. It runs from a line's start to its first newline that isn't hidden, or to the end of the text. A newline is the byte U+000A: nothing else ends a line, so `\r` and U+2028 are characters like any other. A display line's *first line* is the line it starts on. Each display line takes one or more rows.

**Hides merge first.** A Hide with an empty range hides nothing and shows nothing. Hides that overlap merge into one hidden range. It takes the replacement of the Hide that starts first; if several start there, it prefers one with a replacement, then the longest, then the one added first. So a Hide with no replacement that starts first hides a later one's replacement too. Hides that only touch don't merge.

**Items.** Layout places a display line's *items* left to right, in the order they come along the text:
- Each grapheme cluster of visible text. Clusters are `unicode-segmentation`'s extended clusters, found separately in each run of visible text. A run stops at a newline and at a hidden range's start, and the next run starts after that newline or at that range's end. So a hidden range's edge inside a cluster splits it, and the text on two sides of a hidden range never forms one cluster.
- Each cluster of a hidden range's replacement, at the range's start.
- Each cluster of virtual text, just before whatever is at its position, in the order the virtual texts were added. Clusters are found in each virtual text on its own.

Newlines and the end of the text aren't items (Line ends, below). Hidden text with no replacement adds no items, so the text on each side of it joins into one word.

Where virtual text shows, by its position:
- before the cluster, newline or end of the text there;
- at a hidden range's start, before the replacement if there is one, and not at all if there isn't;
- at a hidden range's end, as at any visible position, unless another hidden range starts there, when the start rule applies;
- strictly inside a hidden range or strictly inside a cluster, not at all.

**Widths.** Every item takes whole cells:
- A tab in the text reaches the next multiple of `tab_width`, where 0 counts as 1. The count is in *display columns*: the cells of every item before it on the display line, whichever rows they're on and drawn or not. Filler cells (below) don't count, so a display line's columns don't change with the width. A display column is what `display_col` returns. A tab's cells are blank. Replacements and virtual text hold no tabs: `decorate` turns their tabs and newlines into spaces, and an empty replacement is no replacement.
- A control character (`char::is_control`, other than a tab in the text) is drawn as Emacs draws it, a cell per character. U+0000 to U+001F are `^` and the character U+0040 above them, `^@` to `^_`, two cells, so `\r` is `^M`. U+007F is `^?`, two cells. U+0080 to U+009F are `\` and three octal digits, `\200` to `\237`, four cells.
- Any other item is `unicode-width` 0.2's `width()` of the whole cluster, and at least one cell. So `👍🏽` is 2 cells (tty Emacs draws 4), and U+200B and a lone combining mark are 1. It's drawn in its first cell, and its other cells continue it.

**Where soft wrap breaks.** Rows are `text_w` cells wide (Gutter and width, below). Layout places each item at its row's next column.
- *Whitespace* is an item whose cluster starts with U+0020 or U+0009, so a space with a combining mark on it still counts, as in tty Emacs. A no-break space, U+200B, U+3000 and `-` aren't whitespace, and nothing breaks at them.
- A *break point* is the place before an item that isn't whitespace and directly follows a whitespace item, unless the item after the place is the first thing on its row: breaking there would leave the row empty. A whitespace item that's first on its row still makes a break point after it. The rest of a split tab counts as being on the row, so the item after it can be a break point. A control character isn't whitespace, so the item after its rest isn't one. Hidden text in between doesn't stop it: items are what count.
- An item *doesn't fit* if its last cell would be past the row's last cell. Then:
  1. If the row has a break point, at this item or before it, the row ends just before the last one. The whitespace before it stays at this row's end. The next row starts with the item after the break point, and the items after that are placed again.
  2. Otherwise the row ends here. A tab or control character is split: the cells that fit, which may be none, end this row, and the rest start the next, over several rows if it's wider than a row. Any other item moves to the next row whole, and the cells it leaves on this row are *filler*: blank, with no position. Only an item wider than a cell leaves any.
  3. A cluster wider than a whole row, once at a row's start, stays there, since moving it wouldn't help. Its cells past the row's end aren't drawn. The row then counts as full: the next item starts a new row, and a line end there is at column `text_w`.
- With `Wrap::Char` the same rule applies with no break points.
- With `Wrap::Off` no item fails to fit, so a display line is one row, however long. M0 has no horizontal scrolling: cells and cursors past the window's edge aren't drawn.

**Line ends.** The newline and the end of the text take no cell, and a line end never moves to a new row. Each has a cursor position at its row's next column. So an exactly full line gets no blank row after it, as in Emacs. On a full row, that position is just past the row's last cell: `screen_pos` gives column `text_x + text_w`, and in a wrapped window the scene draws the cursor in the right padding when there is one. Otherwise it stays on the row's last cell, where a bar goes at the cell's right edge and other shapes draw on the cell. A selection doesn't draw there: a line end's cell is a place to highlight only on a row with room for it.

**The empty last row.** A final newline ends the last line. The position after it is laid out as a row of its own, as in Emacs: it's where a cursor at the end of the text is drawn, and scrolling and window positions count it. Which commands may move a cursor there is the keymap's business; die-vim's motions never do (scope §10). A Hide that covers that newline joins it to the end of the text, so there's no such row. An empty text has one empty row, and so does an empty line anywhere.

**Examples,** at 10 cells with `tab_width` 8. In the text, `→` is a tab, `^A` is U+0001 and `[ ]` is hidden text. In the rows, `|` ends a row, `·` is a space, `→` is a cell of a tab and `_` is filler.

```
Text                        Rows                        Why
aaaa bbbb cccc              aaaa·bbbb·|cccc             c doesn't fit, and it's the last break point
aaaa bbbbb cccc             aaaa·|bbbbb·cccc            the space doesn't fit; the last break point is before b
aaaa bbbb  cccc             aaaa·|bbbb··cccc            the second space doesn't fit; the same
aaaa + 8 spaces + bbbb      aaaa······|··bbbb           the 7th space doesn't fit, with no break point yet
aaaaaaaaaa bbbb             aaaaaaaaaa|·bbbb            the space doesn't fit, with no break point
aa bbbbbbbbbbbb             aa·|bbbbbbbbbb|bb           a long word starts its own row, then breaks anywhere
    bbbbbbbbbbbb            ····|bbbbbbbbbb|bb          the same after an indent
 bbbbbbbbbbbb               ·|bbbbbbbbbb|bb             and after one space, first on its row
aaaaaaaaaa bbbbbbbbbbbb     aaaaaaaaaa|·|bbbbbbbbbb|bb  the space is first on its row, and still a break point
aaaaaaaaaaaaaa bb ccc       aaaaaaaaaa|aaaa·bb·|ccc     the last break point, not the first
aaaa bbbb cc→dd             aaaa·bbbb·|cc→→→→dd         the tab runs from column 12 to 16, not from 2 to 8
aaaaaaaaa→bb                aaaaaaaaa→|→→→→→→bb         a tab with no break point splits
aa bbbbbb→cc                aa·|bbbbbb→→→→|→→→cc        a break point first, then the split
aaaaaaaaa→bbbbb             aaaaaaaaa→|→→→→→→|bbbbb     b follows the tab's rest, so it's a break point
aaaaaaaaaa→bbbbb            aaaaaaaaaa|→→→→→→|bbbbb     the same, with no cell of the tab on the first row
aaaaaaaaaa→b                aaaaaaaaaa|→→→→→→b          no cell of the tab fits
aaaaaaaaa中b                aaaaaaaaa_|中b              a wide character moves whole, leaving filler
aaaa bbbb中c                aaaa·|bbbb中c               but a break point comes first
aaaaaaaa 中b                aaaaaaaa·|中b               and that row is 9 cells, with no filler
aaaaaaaaa中→b               aaaaaaaaa_|中→→→→→b         the tab runs from column 11 to 16: filler doesn't count
aaaaaaaaa^Abb               aaaaaaaaa^|Abb              a control character splits, as a tab does
aaaaaaaaa^A + 11 b          aaaaaaaaa^|Abbbbbbbbb|bb    it isn't whitespace, so b isn't a break point
aaaaaaaa ^Ab                aaaaaaaa·|^Ab               unless it's a break point
aaaaaaa[ ]bbbb              aaaaaaabbb|b                hidden text joins the words
aaaaaaaa b, with xxxx       aaaaaaaaxx|xx·b             virtual text before the space at 8
aaaa bbbb, with xx yy       aaaa·xx·|yy·bbbb            virtual text "xx yy " at 5 breaks like text
aaaa + space+mark + bbbbbbb aaaa·|bbbbbbb               a space with a combining mark on it is whitespace
aaaa bbbbb cccc, Char       aaaa·bbbbb|·cccc            Wrap::Char: no break points
```

Cursor positions on full rows and last rows, at 10 cells:

```
Text              Rows                     Position → (row, col)
aaaaaaaaaa\nb\n   aaaaaaaaaa|b|(empty)     9 → (0, 9)    10, the newline → (0, 10)    11 → (1, 0)    13, the end → (2, 0)
aaaaaaaaaa        aaaaaaaaaa               10, the end → (0, 10)
a\tb\n            a→→→→→→→b|(empty)        1, the tab → (0, 1)    2, b → (0, 8)    3, the newline → (0, 9)    4, the end → (1, 0)
aaaaaaaaaa\tb     aaaaaaaaaa|→→→→→→b       10, the tab → (1, 0)    11, b → (1, 6)    12, the end → (1, 7)
(empty text)      (empty)                  0, the end → (0, 0)
```

The Evil oracle's tty Emacs keeps its last column for the continuation glyph and otherwise breaks this way, so a fixture W columns wide is checked at W − 1 text columns. Two things differ from it on purpose: tty Emacs counts filler toward tab stops, and draws emoji clusters wider. A third is only how it's asked: `posn-at-point` reports a wide character that moved whole, or a control character with no cell left on its row, at the column it left, but the terminal's own cursor is drawn on its new row, where this rule puts it.

**Cursor positions.** Layout decides where the cursor can be. The cursor positions are:
- the start of each cluster of visible text, drawn on its first cell. A position inside a cluster isn't one;
- each newline that isn't hidden, and the end of the text, drawn as Line ends says;
- the start of a hidden range with a replacement, drawn on the replacement's first cell: that's the fold line.

Nothing else is. A hidden range with no replacement has no cursor position at its start or inside it, and virtual text and a replacement's later clusters have none. A row break adds none, so a row can have none: a tab's rest, or the end of a long replacement. After every edit or decoration change, core moves each cursor out of hidden text: to the fold line if the hidden range it's in has a replacement, otherwise to the next visible position. A cursor left inside a cluster goes to the next cursor position. A whole-line fold hides from the start of its first line up to the newline that ends its last line, and not that newline, so the next line keeps its own row.

`move_visual` counts only rows with a cursor position. Its goal is the column it's given, or else the starting position's column on its row. On the row it lands on, it takes the last cursor position at or left of the goal, or else the row's first. With fewer such rows than asked, it goes as far as there are. With none, it stays where it is. It returns the position and the goal it used.

**Gutter and width.** `max_width` counts text cells only, so you get 80 columns of code. `visual-fill-column` includes the gutter, so this deviates from it (scope §8). In a window `cols` cells wide:
- `gutter` is 0 with line numbers off. Otherwise it's `max(3, digits(n)) + 1`, where `n` is the number of lines, hidden or not, and not counting an empty last line. A text of 999 lines with a final newline has `n` = 999, so a gutter of 4.
- `avail` is `cols − gutter`, and at least 2.
- `text_w` is `min(max_width, avail)`, and at least 2. With no `max_width` it's `avail`.
- `pad` is `(avail − text_w) / 2`, rounded down, when the buffer is centered, and otherwise 0. Gutter and text are centered together.
- A row runs `pad` blank cells, the gutter, then the text, from `text_x = pad + gutter`. The rest of the window is the right padding. For example, `cols` 120 with numbers on, under 1,000 lines, `max_width` 80 and centered gives a gutter of 4, `text_w` 80, `pad` 18 and `text_x` 22.
- The gutter draws a number right-aligned in its first `gutter − 1` cells, and its last cell is blank. A number with more digits than those cells shows its last digits, so the text doesn't move. Only Visual can have one, since its distances count rows. In a window narrower than `text_x + text_w`, the text simply runs past the window's edge.
- `screen_pos` gives a position's row counted from the window's top row, and `text_x` plus its column on that row. For a position that isn't a cursor position, `screen_pos` and `display_col` answer for the last cursor position at or before it on its display line, or else that line's first. `screen_pos` gives nothing if the row is scrolled out of view.

**Line numbers.** A row shows at most one number:
- *Absolute:* a display line's first row shows its first line's number, from 1.
- *Relative:* the first row of the cursor's display line shows its first line's number. Every other display line's first row shows its distance from the cursor's display line, in display lines, so a fold counts once, as `j` does. Emacs's count hidden lines, so this deviates from it (scope §8).
- *Visual:* the cursor's row shows the number of the line the cursor is on. Every other row shows its distance from the cursor's row, in rows, counting rows with no cursor position.
- Other rows show nothing.
- In every style, no row of a display line that starts at the end of the text has a number, as in Emacs, even with the cursor on it: that's the empty row after a final newline, and an empty text's only row. It still counts when other rows measure their distance from it. What counts is where the display line starts, not what shows: a hidden last line's row looks the same, and keeps its number. With the whole text hidden, Emacs draws no number, and here the only row has one.

Each column shows what that style draws for the text `one\na line that wraps\nthree\nfour\n` at 12 cells, with the cursor where the heading says:

```
                     Cursor on three               Cursor on wraps               Cursor on the empty row
                     Absolute  Relative  Visual    Absolute  Relative  Visual    Absolute  Relative  Visual
one                     1         2        3          1         1        2          1         4        5
a line that             2         1        2          2         2        1          2         3        4
wraps                                      1                             2                             3
three                   3         3        3          3         1        1          3         2        2
four                    4         1        1          4         2        2          4         1        1
(empty row)
```

With lines `a` to `e`, `b` to `d` folded, with a replacement or without one, and the cursor on `a`: Absolute shows 1, 2, 5; Relative and Visual show 1, 1, 2 (Emacs's Relative shows 1, 1, 4).

A Hide can join lines. With lines `a` to `d`, from the end of `a` to the start of `c` hidden, `a` and `c` share a row. With the cursor on `c`: Absolute shows 1, 4; Relative shows 1, 1 (Emacs's shows 2, 1); Visual shows 3, 1, as Emacs's does.

**Motions respect hidden ranges.** Line motions skip hidden ranges through `next_visible`, and search can match inside them and reveal them (scope §8). Because core does this, manifold needs no override.

**Settings.** Soft wrap (`Wrap::Off`, `Word` or `Char`), centered maximum width, line-number style (off, absolute, relative or visual), indentation, whole-buffer read-only, and cursor visibility (scrib's browser sets `cursor-type` to nil). These can be changed at runtime per buffer. Their defaults come from the file-type mode and your config. The core command `toggle-max-width` turns a buffer's maximum width off, or back on at the width it had, or at 80 if it never had one, as your `\z` does with `visual-fill-column-mode`. A surface buffer has a maximum width and centering too, which place its area (§10); `Ctx::settings` and `Ctx::change_settings` reach them.

**Browsers rendered as text.** Here's how seek, scrib's browser and orchid's session browser would map across when you port them. `die-seek` is a placeholder name for your seek port.

| In Emacs | In DIE |
|---|---|
| `defvar-local` for the selected row, scroll offset, search string and search mode | Fields on the browser's mode object |
| `seek-render`: `erase-buffer`, then `insert` `propertize`d lines | `die-seek` builds styled lines; one silent call replaces the buffer's text |
| `window-width`, `window-body-height` | Window size from `Ctx`, plus `on_resize` to redraw |
| `cl-letf` overriding `window-width` to cap row width | `die-seek` takes width as a parameter, so no override is needed |
| `highlight` face on the selected row | A face decoration, or a face included in the styled lines |
| `cursor-type` set to nil | The buffer's cursor-visibility setting |
| Every printable character bound to a search self-insert command | One `<char>` binding that reads `args.typed()` |
| `evil-normal-state` when search ends | `ctx.set_state(normal)` |
| `define-key` / `evil-define-key` on the mode map | Mode keymaps, per state |

A seek port would stay a library, as seek is today. It would hold no buffer itself; scrib and orchid ports would each own their browser buffer and call it.

### die-vim's primitives

die-vim uses only this public API (scope D15). These are the pieces it needs that a plain text API wouldn't have. Each has a contract test (§18):

1. Undo groups that span commands, with selection restore
2. Per-window selections (char, line or block) with a goal column
3. Typed registers (char, line or block), and the system clipboard
4. The prefix-argument slot (§6)
5. `feed_keys` and a key observer, for `q` and `@`
6. Layout queries: `viewport`, `move_visual`, `display_col`, `next_visible`, `display_line`, `pos_at_vcol`, `snap`, `screen_pos`, `scroll`, `set_scroll`
7. An `<any>` binding per state (§7)
8. `edits_since`, so dot-repeat can record text inserted by any binding, including a mode's
9. Window commands (split, close, only, focus by direction) and a surface's `scroll_to` and `reveal`
10. Reading and setting the modal state, with the cursor shape following it

Inside die-vim, motions are plain functions over the text snapshot. A Vim command keeps its checkout short: read state, release it, act, then write state back. It never holds `with_ext::<Vim>` across `ctx.run`.

**Copy state** (scope §5.5) reuses all of this. Core takes a read-only text snapshot of a surface, and die-vim's normal and visual grammar runs on it (§7): the snapshot is a `TextBuf` with a view per window, wrap off, and a limit on how far down a window's top can scroll, its live view's. die-vim's one change for it is that an insert session refuses to start on a snapshot.

## 10. Custom surfaces

This is the canonical Surface trait; scope §5.4 points here.

```
Surface
  kind()                          "term"; used for keymaps, restore and describe
  render(frame, info)             write the window's area into a DIE-owned cell frame
                                  info: the window, whether it takes input, modal state, cells in pixels
  cursor(info)                    optional: where the cursor is in the area, and how it looks
  status()                        optional: what the window's status line says, as a terminal's directory and title
  event(event, ctx) -> handled    input that keymaps didn't take (below)
  handles(key)                    optional: which keys it takes in passthrough, for describe-key
  takes_esc()                     optional: whether a plain Esc in passthrough is its own now (§7)
  takes_wheel()                   optional: whether the wheel in copy state is its own now (below)
  initial_state()                 default normal; die-term returns passthrough
  text(update) -> SurfaceText     optional: its content as lines, for copy state (below)
  scroll_to(top)                  optional: show copy state's line `top` on the top row, or the live view
  reveal(range)                   later: make a range visible
  save() -> data                  later (§14)
```

**Rendering.**
- `render` writes into a `Frame`, DIE's cell frame for the window's area (scope D8). **The area** is a text window's text area with no gutter (§9, "Gutter and width"): a surface buffer has settings of its own, which start as a text buffer's defaults (`Config::text_defaults`), and their `max_width` and `center` make it `min(max_width, cols)` columns wide at the same `pad`, on every row; with no maximum width it's the whole window. Its other settings mean nothing. `Ctx::surface_area(w)` gives the area as a `Geometry` (`text_x`, `text_w`, `rows`), and `Settings::place(cols, rows, gutter)` is the rule it shares with `TextBuf::geometry`. Core draws the margins blank in the `default` face, as a text window's, and the status line still spans the window. `Ctx::settings(buf)` reads a buffer's settings, a surface buffer's own and never its snapshot's, and `Ctx::change_settings(buf, f)` changes them, as `toggle-max-width` (`\z`) does in insert and normal state alike: a change of `max_width` or `center` sends each window showing the buffer a `Resize` with its area, and copy state's snapshot takes the width. Core keeps each surface's frame between frames, so a surface can rewrite only the rows that changed. `frame.is_new()` says it holds nothing the surface wrote: its first render, a new size, or another window of another size drew it last. `frame.put(row, col, cell)`, `frame.row_mut(r)` and `frame.text(row, col, text, style)` write it; `text` lays text out as a text window does, a wide character's second cell a continuation.
- A `FrameCell` holds a glyph (a grapheme, or `Glyph::Continuation` for a wide character's second cell), `fg` and `bg` (`Color::Rgb`, or `Color::Default` for the `default` face's), bold, italic, faint, reverse, invisible, strikethrough, blink, and an underline style and color (`Color::Default` is the foreground). So a surface needs no theme.
- Core resolves what the scene's cell doesn't hold. Reverse video swaps the colors, the defaults too. Faint keeps 65 % of the foreground and takes 35 % of the background, as ghostel does, before the swap. Invisible text draws a space with no underline or strikethrough, and blinking text draws steadily.
- Cells are built with `FrameCell::new(glyph, fg, bg)` and then their fields set, since a struct literal doesn't compile outside die-core. So overline and the link id can arrive with die-term without breaking a producer.
- `cursor(info)` gives a `SurfaceCursor`: row and column in the area, a `CursorShape` (block, bar, underline or hollow, as DECSCUSR sets them), visible (DECTCEM) and blinking, which draws steadily for now. Core draws it hollow in a window that doesn't take input: not focused, a prompt open, or DIE's window without the keyboard. It draws no cursor that isn't visible, and none for `None`. `CursorShape::for_state(state)` is a text window's shape in a state, passthrough's as insert's bar, for a terminal that follows the modal state on the main screen as evil-ghostel does.
- A surface window's status line shows the surface's `status()`, or the buffer's status text if it gives none, and at the right the buffer's state as Evil's tag: `<I>` in passthrough and insert, `<N>` in normal. Your status text is made from the file a buffer visits, and a surface knows where it is: a terminal's directory, which its shell can change. A terminal's cursor is the program's, so it can't always say. Text windows show no tag.
- `frame.place(id, z)` shows a pixel buffer over the whole area, under the glyphs at a negative z (§11). Images, source rectangles and sub-cell offsets come with M3.
- `render` can't change the workspace. It gets the surface mutably, so it can update its own caches, but it doesn't get `Ctx`. A panic in it draws `<render panicked>` until the next good render (§15), which finds the frame blank and new, so a surface that writes only its changed rows writes them all.
- **Components are plain Rust types that render into part of an area,** so a new graphical extension can be assembled from reusable pieces. The contract needs nothing extra for this. Your existing browsers don't use custom surfaces at all (§9).

**Copy state's text** (scope §5.5, §7).
- `text(false)` gives the whole text, `text(true)` what changed since the last read, or None if nothing did. A `SurfaceText` holds lines, each ending in a newline, oldest first; an update replaces the last text's lines from `from` to its end after `dropped` lines left its start, as a terminal's scrollback drops its oldest. It also says where the surface's cursor is, as a line and a byte, which line its live view shows on the top row, below which copy state never scrolls, and, if the surface's own view is scrolled back from that, as by the wheel in insert, the line it shows on top (`shown_top`): copy state starts there, with the cursor on the window's last row, rather than at the live view. `renumbered` says its lines are numbered afresh, as die-term says when its program switches screens, so every window goes to the surface's cursor, as `G` does, since its line says nothing now. `Ctx::surface_cursor(buf)` gives the surface's cursor as a snapshot position, for a `G` that goes to it. It's synchronous: die-term's whole read of a 9,600-line scrollback takes about 10 ms.
- The surface says its text changed with `Ctx::surface_text_changed(buf)`, from its output callback. Core reads the update at the next key or frame in normal state with no prompt open. A window follows if its cursor is on the surface's cursor line or below: it stays on the surface's cursor if it was on it, or as far below the cursor's line, and as far above the live view. Other windows stay on their lines, less those dropped, until they come back down, which `G` does; their text is read all the same, so what a window draws is what it yanks, after a `clear` empties the scrollback too. Visual, operator-pending state and a prompt wait until they end.
- A prompt opened over a surface in passthrough, as a terminal's "Paste it?", leaves it out of copy state: it goes on showing its live view, and isn't read. The answer puts back the state of the buffer the prompt was opened over, if it's still there, and touches no other.
- A resize reads the whole text again after the surface hears it, since a terminal reflows. `wrapped` lists the lines that run on into the next one, each with the columns it takes, as die-term lists the rows its program's long lines wrapped onto, at the terminal's width. Through a resize, a window that isn't following keeps its cursor, its selection's other end and its top row at the same place in the same line of output (the lines of output before it, and the columns before it in its own), as a file's window keeps its text through `\z`; a follower goes back to the cursor as before.
- Core calls `scroll_to(top)` before each render: the snapshot line the window shows on its top row, or None outside copy state. The window keeps showing the surface's own cells there, and core draws die-vim's cursor, the selection and the search's matches over them, recoloring the cells with the `selection`, `search` and `search-current` faces. The surface's own `cursor()` isn't drawn then. The snapshot has the surface buffer's `max_width` and `center` and never wraps, so its text area is the surface's area, and what core draws over it lands on the surface's cells at the area's offset; a click maps by the same cells.

**What `event()` receives** (`Event`, which is `#[non_exhaustive]`, so a match needs a `_` arm):
- `Key`: keys no keymap took (§7), so a surface can implement its own `j`/`k` without registering them. In passthrough, every key outside the DIE layer (scope D18), with its repeats and releases, modifier keys alone, and keys DIE has no name for. A repeat or release comes only after its press did, matched by the physical key (`code`), since a release's character is what the modifiers held then make it: not those of `i`, whose press left normal state, nor of a Shift pressed there. A release comes in whatever state the buffer is in by then, and in a window that no longer has the focus.
- `Resize { window, cols, rows, cell }`: the window's area, when the surface goes into a window (`open_window`'s caller already gave the size, so there only an area narrower than it is told), when the window changes size, when the buffer's `max_width` or `center` changes (at the same size too), and when the host says the cells' pixels changed (`Ctx::set_cell_pixels`). `cell` is a `CellPixels`: a cell's width and height in physical pixels and the display's scale, 0 until the host says, as XTWINOPS answers want them. `PtySize::new(cols, rows, cell)` is that as a PTY's size.
- `Focus(bool)`: a window showing it gained or lost the focus.
- `Pointer(SurfacePointer)`: a press, release, motion (`Move`, or `Drag` with a button held) or a precise scroll (`dx`, `dy` in cells), with the cell in the area, the pixel in the area (`x`, `y`, physical), the click count and the modifiers. A press focuses the window, ending a waiting command as a press on text does. A press, motion or release over a margin isn't the surface's: the press focuses the window, as one in a text window's margin does, and in copy state it is that click. The wheel over a margin reaches it, held at the area's edge, as the wheel over a file's margin scrolls its text. Drags and the release go to the window that had the press, held inside its area; those of a press that went elsewhere, as to text or a margin, never reach a surface. While a prompt is open only a surface's own reach it. In copy state the pointer is the snapshot's, except the wheel while `takes_wheel()` says yes, as die-term does while its program tracks the mouse.
- `Paste(text)`: text to paste as one piece, which a terminal brackets. The DIE layer's V in passthrough sends it (the `die` facade binds `<D-v>` there to `surface-paste`), and `surface-paste` with a register does, with the text as many times as its count, so die-term can bind `p` and `P` in normal state to it. It reads a register by the rule a prompt's `<C-r>` uses (`set_prompt_register`, die-vim's), so `"+` is the clipboard, `A` is `a` and `_` pastes nothing.
- Typed text in insert state, for example a search field in a graphical view.

`event()` receives `Ctx`. Core checks the surface out of its buffer for the duration of the call (C2).

**One state per surface (C10).** If a terminal or a pixel view is shown in two windows, both show the same content and scroll position.

## 11. Pixel buffers and images

The first slice landed early as M3a (2026-11-10), with the whiteboard as its driver and die-term's kitty graphics as its second user: a mutable pixel buffer, shown over a custom surface's whole area. Core holds no pixels and has no drawing API (D11): an extension brings its rasterizer, writes a buffer it owns, and the host's renderer puts it on the GPU.

**Creating one.** `ctx.pixel_buffer(width, height)` takes a size in physical pixels (from a `Resize`'s cells and its `CellPixels`: `cols × cell.width` by `rows × cell.height`) and returns a `PixelBufferId` and a **producer**. It fails with no sink connected, which `ctx.pixels_supported()` says first, and for a side of 0 or over `MAX_SIDE` (16,384). The id names the buffer across threads and is never given out twice.

**The producer** (`Producer`) owns one persistent CPU buffer of premultiplied RGBA8, `width × 4` bytes a row, transparent at first: `pixels_mut()` to draw into, `pixels()`, `size()`, `stride()`, `id()`. It's `Send`, so a thread of its own can own it, and it carries nothing of `Ctx` (D7).
- `present(damage: PixelRect)` says the rectangle is drawn. It hands the buffer to the host's sink on the calling thread, which copies the damage out, and wakes the loop so a frame follows. The damage is clipped to the buffer; nothing left means nothing happens.
- `resize(width, height)` makes the buffer that size, transparent, and tells the sink; what was drawn is gone, and the placement shows nothing until the next present.
- Dropping it destroys the buffer: the sink is told, and a placement that still names it draws nothing.

**The sink** (`PixelSink`) is where producers' pixels go: `create`, `present(id, pixels, width, damage)`, `resize` and `destroy`. The host connects one at startup, once, with `ctx.set_pixel_sink`, as it connects the waker; die-render's is its staging ring, and the harness's is a fake. Every call runs on the producer's thread, which is the main thread for a surface that rasterizes in `event()`. The rule the shapes keep (scope §5.2): `present` may block its caller, as when the ring is full, but the main thread never waits on a lock a producer can hold; it tries the hand-off, and a busy one shows on the next frame. A buffer shows nothing until its first present after `create` or `resize`.

**Showing one.** A custom surface calls `frame.place(id, z)` in `render`: the buffer over the whole area, from its top-left corner, 1:1 in physical pixels and clipped at the area's edge, so a buffer sized to the area fills it. Placements aren't kept between renders: a surface places what it shows at every render. Core composes each into `Scene.placements` at the area's offset (a `Placement`: the buffer, its cells and its z), drops a destroyed buffer's, stops each above the prompt's list as it does the hollow cursors, and lists them in drawing order, lower z first. A negative `z` draws between the cells' backgrounds and their glyphs, so text put on the frame shows over the pixels (the whiteboard's labels in the editor's font); 0 and above draws over the glyphs and under the cursors. `Frame::placed()` reads back what a render placed.

**Pointer events** over a surface already carry the pixel inside the area (`SurfacePointer.x`, `y`, physical), which is a full-area placement's own pixel space; the host sends a `Drag` for every pointer sample while a button is held, so ink follows the hand rather than the cell grid.

**Testing.** The harness connects a `FakePixelSink`, so `pixels_supported()` is true there, `Harness::pixels(id)` gives a buffer's size and latest pixels, and `fake_pixels().take_calls()` the `SinkCall`s the sink heard, so a test can assert what an extension drew and when. `Scene::snapshot` lists the placements when there are any.

**What waits for M3:** images (`ctx.image(bytes)`, decoded off the main thread), a placement's source rectangle and sub-cell pixel offset, placements narrower than the area, `Hide` with a placement in text buffers, the display link for animation with no input, and the one-frame-behind rule for a producer thread's resize (a buffer larger than its area shows its top-left part until the producer catches up).

## 12. Async

All callbacks run on the main thread, receive `Ctx`, and are queued (C3). Every subscription returns a handle; dropping it cancels the subscription (C4).

| API | Shape | Replaces |
|---|---|---|
| `spawn(work, done)` | `work` runs on a thread of its own and returns a value; `done(ctx, value)` runs on the main thread | Anything slow, such as scrib's blocking `call-process` or delta's `shell-command-to-string` |
| `process(cmd)` | A builder for args, environment and working directory, with callbacks for stdout (lines or chunks), stderr and exit. Returns a handle for writing to stdin or killing the process. | `make-process` with a filter and sentinel |
| `pty(cmd, size)` | The same, on a PTY: `ctx.pty(PtyCommand, PtySize) -> Pty`. Used by `die-term`. Until D20's helper, its output reaches DIE's main thread; then die-term hands the PTY to the terminal's helper process, so its output never passes through DIE. | |
| `watch(path, f)` | File-system events | `file-notify-add-watch` |
| `after(d, f)`, `every(d, f)`, `idle(d, f)` | Timers | `run-with-timer`, `run-at-time`, `run-with-idle-timer` |
| `prompt(spec, f)` | A command-line prompt with optional completion candidates. `f(ctx, answer)` runs with the answer on submit, and with nothing on cancel, so a multi-step flow can clean up. | `completing-read`, `read-string` |

**The prompt's spec (K11a).** A label will do. `PromptSpec` adds starting text, a named history (`<Up>` and `<Down>`; a cancel, an empty answer and a repeat of the newest add nothing), `<BS>` on an empty line cancelling, as `:` and `/` want, and `on_change(f)`, queued with the text after each key that changes it, for incremental search. An error from `on_change` drops no keys. A prompt takes its keys before any keymap: `prompt.rs` lists them.

**Candidates (K11b).** `candidates(list)` offers a fixed list, and `candidates_from(f)` works one out from the text with each change: `f` returns how many bytes at the start of the text are fixed, and the candidates for the rest. `recent_first(history, items)` puts a list in the owner's Vertico order: what was answered under that history, newest first, then the shortest, then alphabetically. `descends()` makes a candidate ending in `/` go on the line instead of answering; while nothing's typed after the directory no row is chosen, shown as `*/N`, `<CR>` answers with the directory, and `<Down>` chooses the first row. `require_match()` keeps the prompt open with a `[Match required]` note when `<CR>` or `<M-CR>` would answer with anything but a candidate or an empty line. Up to 20 show under the prompt's line, over the windows, which keep their size. The list grows as needed and doesn't shrink while the prompt is open. Matching and ranking are Emacs's flex, except that a query with no capitals ignores case, and a tie goes to the candidate in the query's case. `<CR>` answers with the chosen candidate, or the typed text if none matches, `<M-CR>` answers with the typed text, and `<Tab>` puts the chosen one on the line, or the first if none is chosen. With candidates, `<Up>` and `<Down>` choose a row, `<M-<>` and `<M->>` go to the first and last, and history is on `<M-p>` and `<M-n>`. `find-file` starts from the buffer's directory, with the whole path on the line and `~` for the home directory. It lists the directory the line names, hidden entries last. `<M-CR>` names a new file, and answering with a directory keeps the prompt open there.

**Timers.**
- They fire in deadline order, idle timers among them. Timers due together fire in the order they were set, and a repeat counts as set again each time it fires. An idle timer goes after any other timer due at the same moment.
- A repeat counts from its last deadline, so being late doesn't shift the next one. One that has fallen a whole period behind, as after a laptop's sleep, skips what it missed instead of running it all. The shortest period is a millisecond.
- Timers due together fire as a batch. After a batch, the next key gets a turn, and with none waiting, the host does: it draws, reads its events, and finds the next timer already due. So a repeat that takes longer than its period can't lock out keys or the frame.
- `idle(d, f)` runs `f` when no key has come for `d`, once each time that happens, as `run-with-idle-timer` does with a repeat. It counts from when it's set, however long the editor has already been idle, and after that from when a key's command or prompt answer ended, so a slow command doesn't use up the wait. It doesn't fire while a key is waiting.
- Core can say when the next timer is due. The host sleeps until then, or until it's woken.

**The clock** is real in DIE and fake in the harness (§16). A fake clock moves only when the harness moves it. It stops at each deadline on the way, so a callback reads its own time, and a chain of retries runs inside one move. If a job outlasts its wait, the rest of that move doesn't wait for it again. A real clock can't be moved.

**Waking the loop.** A background thread never touches the editor. It posts to a channel, and a `Waker` tells whatever runs the loop. The waker names no event loop: the host connects its own, once. Any thread can call it, and however many jobs finish between two turns of the loop, the host hears once. A turn of the loop never waits for a job, so `work` can take as long as it needs.

**PTYs** need more than pipes do:
- **The API.** `PtyCommand::new(program)` takes `arg`, `args`, `arg0` (`-zsh` for a login shell), `current_dir`, `env`, `env_remove`, `on_output(|ctx, bytes|)` and `on_exit(|ctx, status|)`. `ctx.pty(cmd, size)` returns a `Pty`, or an error if there's no PTY or no such directory. `Pty` has `write(bytes)`, which never waits, `resize(size)` (TIOCSWINSZ, cells and pixels), `pid()`, `foreground_group()` (None while the terminal has no session), `echoes()`, `line_mode()` (ICANON) and `close()`. The terminal starts with IUTF8, as vterm's `stty iutf8` has it. Dropping it closes it, and no callback runs after (C4). die-term sees nothing of where the PTY lives, so D20's holder can take it with no change there.
- **Starting it.** A child needs code run before `exec` to get a controlling terminal, which std offers only through `unsafe`, and die-core is `forbid(unsafe_code)`. So DIE's own binary does it, in a short mode: `die --pty-exec name program args`, started with the PTY as its stdin, stdout and stderr, calls `setsid`, makes the PTY its controlling terminal (TIOCSCTTY) and `exec`s the program, which keeps its pid. A binary that opens PTYs calls `die::other_mode()` (`die_core::pty::trampoline()`) first in `main`, as your config's does, and a test binary with no harness does too. `die::other_mode()` first marks every fd above 2 close-on-exec, for what frameworks open without it (Metal's shader cache), in die-render's `platform`; a binary with die-render's window up uses it, not die-core's alone. On Linux the trampoline is `/proc/self/exe`, which a rebuild can't remove. An `exec` that fails says why on the terminal and exits 127 or 126, as a shell does.
- **The environment** is DIE's, with the PATH your login shell sets (the one `:make` gets), `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=DIE` and `TERM_PROGRAM_VERSION`, and `LANG` in UTF-8 from your region (`AppleLocale`, else `en_US.UTF-8`) if none of `LC_ALL`, `LC_CTYPE` and `LANG` is set, as in DIE.app from the Dock, less what a terminal DIE was started from would have left that's wrong in DIE's (its size, `TERMINFO`, and Ghostty's, kitty's, iTerm's, WezTerm's, Alacritty's and VTE's variables), then the command's own.
- **Output** is read on a thread of the PTY's own and handed to `on_output` from the top of the loop (C3), 256 KiB a batch per PTY, after which a waiting key goes, as with timers. With no key waiting, batches go on for 4 ms, and then the frame does. So a flood can't hold up a key, and a frame can take about 1 MiB, what the reader keeps. The reader keeps at most 1 MiB waiting; past that the program waits in the kernel. While output waits for its turn, `Ctx::pty_output_waiting` says so, and the host doesn't sleep. Input is written from the same thread, so a program that doesn't read never blocks DIE.
- **The exit** comes to `on_exit` after the last output: the end of output on the PTY marks it, or 200 ms after the program exits if something it left holds the terminal. A failure to start comes there too.
- **Closing** a `Pty` hangs the terminal up, as closing a terminal window does, and kills the program's process group 300 ms later if it's still there. Quitting DIE closes every PTY that way, before it stops its other commands.
- Every fd DIE opens is close-on-exec, PTYs included, and no other terminal's fd reaches a program. A test runs `ls -l /dev/fd/` on one terminal while another is open. Frameworks open fds without close-on-exec, Metal's shader cache among them, so `die::other_mode()` marks every fd past 2 before the `exec`, and a test runs `ls` from a DIE with its window and Metal up. The holder's received fds will arrive without close-on-exec on macOS (S3); the same sweep covers them.
- **With D20 (next round):** a PTY is registered with the holder when `pty()` creates it, so it survives restarts and crashes (scope §5.9). The holder takes labelled fds from any DIE process (`term/<id>/pty`, `term/<id>/engine`), so a terminal's helper process can reach its PTY, and a restarted DIE can reach the helper. After a restart DIE isn't the shell's parent; the exit callback still gets the status: core watches the pid with kqueue (macOS, `NOTE_EXIT | NOTE_EXITSTATUS`) or a pidfd held by the holder (Linux); on Linux kernels before 6.15 the status after a restart is unknown.
- A terminal's output is then parsed in its helper process and reaches the main thread as finished frames: the changed rows as DIE cells, the placements, and images as shared memory (scope §5.8). Other PTY users keep main-thread callbacks.

There are no blocking calls. A command that waited on a process would freeze everything, including your terminals, so the API doesn't offer one. The main thread never waits on a terminal's helper either.

C3 costs you something with multi-step prompts, which nest one callback per step. scrib's `scrib-new-filter` and orchid's session creation are the cases to watch.

## 13. Faces and themes

- **Base faces in core:** `default`, `shadow`, `bold`, `highlight`, `selection`, `search`, `link`, `heading` (inherits from `bold`), `error`, `warning`, `success`, `line-number` (inherits from `shadow`), `status-line` and `cursor`. Core also declares the faces it draws its own screen parts with, each inheriting from a base face: `line-number-current`, `status-line-inactive`, `divider`, `search-current`, `bracket-match`, `prompt`, `prompt-current`, `prompt-match`, `header-line` (the Buffer List's titles) and `buffer-menu-buffer`. An extension can't declare those names. seek uses only base faces (`shadow`, `bold`, `highlight` and `search`).
- **Faces stack** on a text cell from the bottom up: `default`, then stored face decorations (one nested inside another is above it), then mode providers' faces, then search, bracket-match and selection. What a face leaves unset comes from the face below it.
- **A newline has a cell of its own,** so a selected or highlighted newline shows as one cell, as in Vim and in your Emacs, where mono-dark's region has no `:extend`. The cells past it draw `default`. A block selection is the exception: it stops at each line's end, so it shows nothing on a short or empty line's newline, as in Evil.
- **A face no one declared draws as nothing.** A decoration or a provider can name any face, so the first time one no one declared is drawn, `*messages*` reports it, once for each name. A theme's undeclared faces are reported when it's set.
- **Extension faces** are declared with a default style and an optional parent face. For example, `orchid.user-message` inherits from `default`.
- **A theme** is data that maps face names to styles, and it can style any extension's faces. It also sets the 16 ANSI colors that terminals use. mono-dark and dark-coffee become theme values in your config.

## 14. Persistence

- **Core saves:**
  - the buffer list and the window tree
  - for each text buffer, the cursor and scroll position in each window
  - unsaved text, and buffers with no file and no mode restorer, such as scratch buffers
  - registers and recorded macros
  - search and command histories

  Undo history comes later.
- **Format.** JSON with a version field. Each buffer's data is stored separately and read with defaults for missing fields, so data that fails to load drops only that buffer, with a message.
- **When.** Core writes the session after a short idle period following any change, as well as on restart and quit, so a crash loses at most the last few seconds.
- **Extensions** keep what they hold themselves with `save_session` and `restore_session`, as JSON core writes and hands back without reading, whole or by buffer. die-vim keeps `.`, `@@`, `;`, `n` and `gv` this way.
- **Custom surfaces** return data from `save()`, serialized with serde and tagged with the surface kind. The extension registers a **restorer** for that kind, which rebuilds the surface from the data.
- **Buffers restored by their mode** (orchid's chat). The mode saves what it needs, such as a session id, and repopulates the buffer on restore, for example by re-reading the session log. Core doesn't save their text.
- **Missing extensions.** If a saved buffer belongs to an extension that's no longer in your config, the buffer is dropped and a message is logged.
- **Terminals.** A PTY is handed to the holder when it's created (§12), so `save()` returns state only, never file descriptors. A live PTY is never dropped on restore; if its saved state fails to load, it's reattached as a fresh terminal. The holder decides which terminals exist. A saved terminal the holder no longer has ended while DIE was down, and it's restored as exited. A terminal the holder has but the session doesn't (DIE crashed before writing the session) is reattached as a fresh terminal, never dropped.

## 15. Errors and panics

- **Errors.** Commands and callbacks return results. Errors go to the command line and to `*messages*`.
- **Panics** are caught at every call from core into extension code, on a best-effort basis (C9). Each is logged to `*messages*` once, with the extension's name and the place in the source, whoever else sees the error. What the code had checked out goes back, and the editor goes on. A line already among the last panic lines in the log isn't logged again, so panics on every frame can't flood the log, even from several objects taking turns.
  - A panic in `render` replaces that window's content with an error placeholder until the next successful render. A panic in `decorate` draws that frame without the rest of the mode's faces. Neither gets a `Ctx`, so the log line comes at the next turn of the loop, and names only the place.
  - A panic in a job's `work` reaches the main thread as its value would. `done` doesn't run.
  - A panic in a key observer doesn't stop the key.
  - A panic in a key's command or a prompt's answer drops the rest of the fed keys (§6).
  - A panic in the `Drop` of something core holds for an extension, such as a cancelled timer's closure or a cancelled job's value, is logged and names core, with the place. It doesn't escape the turn of the loop.
  - From M1, after repeated panics in a short time, the object that panicked is disabled until restart. That's a mode, a surface or a subscription, never a whole crate such as die-vim.
  - A panic hook sees what no guard catches, such as a panic the host caught itself. It can't reach two kinds of panic, which go only to the standard error stream: one on a thread an extension started itself, and one that aborts, since `*messages*` dies with the process.
- **Half-updated state.** A panic can leave an extension's state half-updated, so the error message suggests restarting.
- **Crashes.** What `catch_unwind` can't catch, the holder covers: terminals survive, and the session restores unsaved text (scope D20).

## 16. Testing

Core provides a headless harness that:

- builds a workspace with no window and no GPU, at a fixed grid size
- loads extensions and your config
- types keys (`\o`, `jjx`) and sends pointer events. `feed_keys` is for macros, since what it feeds drops at the first error (§6)
- runs until idle, using a **fake clock** so timer-driven code (orchid's retries) runs instantly
- exposes buffer text, decorations, and the rendered screen as text for snapshot tests
- can save the session and restore it into a fresh workspace

**Contract tests.** DIE can't prove the contract by building your extensions, because that's not part of this project. Instead, a test-only crate (`die-contract-tests`) contains tiny extensions, each a few dozen lines, one per row of §18. They use only `die`'s public API. For example:

- a fold test hides ranges and checks that `j`/`k` skip them
- a browser test redraws a read-only buffer, binds `<char>`, and reads the window size
- a process test streams output into a read-only transcript with silent edits, then kills the buffer and checks the subscription is cancelled

These tests are throwaway fixtures, not starting points for the real extensions.

**die-vim** is tested against Evil itself. Evil runs in batch Emacs and turns each case (text with a cursor, a key sequence) into the expected text, cursor and registers as JSON fixtures (scope §12, S5).

## 17. Your config, sketched

This is illustrative only. Your config crate, and every extension in it except `die-vim` and `die-term`, are yours to write later. It mirrors your current `init.el`:

```
main
  die::run(config
    leader   "\"
    theme    mono_dark
    font     Ioskeley Mono Term; braille pinned to Ioskeley Mono
    buffer defaults: soft wrap, max width 80 centered, visual line numbers in code
    environment: PATH additions and an env file, as in init.el

    die_term options:    { shell: /bin/bash, scrollback: 10000 }   (die-vim and die-term are always registered)

    extension manifold   { files: [*.el] }
    extension markdown
    extension scrib      { directory: ~/.config/scrib.el, extension: .md }
    extension orchid     { cli: orchid, timeout: 300s }
    extension delta

    command "term-new"   open term<n> at the next free number   (termin, via die-term's API)

    bind normal "<leader>x"  "command-palette"
    bind normal "<leader>]"  "switch-buffer"
    bind normal "<leader>f"  "find-file"
    bind normal "<leader>e"  "term-new"
    bind normal "<leader>o"  "orchid"
    bind normal "<leader>s"  "scrib"
    bind normal "<leader>d"  "delta"

    on startup (cold start only): run "term-new"
  )
```

## 18. Requirements check: could your packages be rebuilt on this contract?

This is a check on the core, not a plan to build anything. Each row becomes a contract test in M1 (§16), and those tests also run in the Linux CI jobs; die-term's do once a signed Linux archive exists (scope D10). die-vim and die-term are built in this project, so their rows are also exercised for real.

| Package | Could be rebuilt as | Uses | Gaps |
|---|---|---|---|
| termin (18 lines) | A command in your config | Find buffer by name, `run`, die-term's API | None |
| seek (165) | The `die-seek` library crate: browser state in, styled lines out | Styled insertion, base faces | None. Width becomes a parameter instead of a call to `window-width`. |
| manifold (754) | An extension with a `Fold` mode | Hide decorations with replacement text and a reveal command, revision, `edits_since`, mode keymaps | None. The `evil-next-line` override disappears (§9). |
| scrib (534) | An extension with the same structure: a browser text buffer with a mode (using `die-seek`), notes as ordinary `.md` text buffers, and the Go indexer | Mode state, `<char>` search binding, window size and `on_resize`, cursor visibility, `spawn` or `process` for the indexer, `prompt` | `scrib-new-filter` becomes a nested prompt callback |
| orchid (~4.5k) | An extension with the same structure: a session-browser text buffer (using `die-seek`), and its conversation buffer (`orchid-chat-*.el`) as a text buffer with a mode | `process` with JSON, `watch` plus `after` for retries, markers, silent edits, read-only and keymap decorations, Hide for collapsibles, subscription handles, faces, mode save and restore | None known. It's the hardest row to cover with contract tests. |
| delta (~1.4k) | An extension with the same structure: a commit browser and a file browser as text buffers with modes, and diffs as read-only text buffers | Styled insertion, face decorations for diff lines, mode keymaps and state, window size, `prompt`, `process` for git | None. Its blocking `shell-command-to-string` calls become `process` callbacks. |
| die-vim (part of DIE, built in this project) | A built-in crate: the keymap for the `text` kind, its commands, and the copy-state grammar | All of §9's die-vim primitives | None |
| die-term (part of DIE, built in this project) | A built-in crate with a custom surface | `pty` with the holder, passthrough, full key events, pointer events, placements, `text()` for scrollback, `scroll_to` | None |
| markdown view (a future extension of yours) | An extension providing the file-type mode for `.md` text buffers | Face providers, stored Hide decorations with placements for images and diagrams, `image` | None. Hidden markup is stored rather than provided (C16), and updated from `edits_since`. |

## 19. Open questions

Settled on 2026-10-19: modes stay on text buffers (C17); providers return faces and catch up with `edits_since` (C15, C16); the session file is JSON with a version field (§14).

1. **Prompt nesting (C3).** If multi-step prompts become painful, a small single-threaded async layer could come later. Revisit if the multi-step prompt contract test in M1 turns out awkward.
2. **Window placement rules.** Callers choose where to show a buffer. Do you also want a config-level override? Your only `display-buffer-alist` entry hides `*Async Shell Command*`, but orchid's "open buffers without splitting" plan suggests you might.
