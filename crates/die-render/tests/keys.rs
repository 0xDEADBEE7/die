//! S1's 50-chord matrix on a real event loop, with every field of every `KeyEvent` checked, and
//! `a_panic_in_the_key_monitor_is_caught`. The loop has to be on the main thread, so this has
//! no test harness and runs them itself. Its last act is to ask the loop to stop.
//!
//! Each chord is real `CGEvent`s posted with `CGEventPostToPid` to this process and no other,
//! so nothing reaches your desktop, and the window stays hidden and takes no focus. AppKit
//! makes `NSEvent`s of them and the key monitor hands them to the window, which is the one
//! step a real key doesn't take: it can't, in a window that isn't key. S1's hand run covers
//! that step. The characters come from the current layout, so this expects a US or AU one.
//!
//! Two things can't come this way, and so differ from real keys. macOS drops a posted key-down
//! marked as a repeat, so the repeats are `NSEvent`s AppKit is handed directly. And with Cmd
//! held, winit sends a key-up straight to the key window, past the monitor, and there's no key
//! window, so those chords have no key-up here.

use die_test_cap as _;

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    if std::env::args().len() > 1 {
        matrix::run();
    } else {
        matrix::in_a_process_saying_press_and_hold();
    }
}

#[cfg(target_os = "macos")]
mod matrix {
    use die_core::{Ctx, Key, KeyEvent, KeyKind, ModifierKey, Mods};
    use die_render::driver::{self, Config};
    use die_render::window::{self, Remote};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventType};
    use objc2_foundation::{NSPoint, NSString, NSUserDefaults};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    const CAPS: u16 = 57;
    /// What `CGEventFlags` says about Caps Lock.
    const CAPS_FLAG: u64 = 1 << 16;

    /// A modifier key: its virtual key code, W3C name, which modifier, the side, and its
    /// device-independent and device-dependent flags.
    struct Modifier {
        vk: u16,
        code: &'static str,
        key: ModifierKey,
        right: bool,
        flags: u64,
    }

    const SHIFT: u64 = 1 << 17;
    const CTRL: u64 = 1 << 18;
    const OPT: u64 = 1 << 19;
    const CMD: u64 = 1 << 20;

    const fn m(vk: u16, code: &'static str, key: ModifierKey, right: bool, flags: u64) -> Modifier {
        Modifier {
            vk,
            code,
            key,
            right,
            flags,
        }
    }

    const MODIFIERS: [Modifier; 8] = [
        m(56, "ShiftLeft", ModifierKey::Shift, false, SHIFT | 0x2),
        m(60, "ShiftRight", ModifierKey::Shift, true, SHIFT | 0x4),
        m(59, "ControlLeft", ModifierKey::Control, false, CTRL | 0x1),
        m(
            62,
            "ControlRight",
            ModifierKey::Control,
            true,
            CTRL | 0x2000,
        ),
        m(58, "AltLeft", ModifierKey::Alt, false, OPT | 0x20),
        m(61, "AltRight", ModifierKey::Alt, true, OPT | 0x40),
        m(55, "SuperLeft", ModifierKey::Super, false, CMD | 0x8),
        m(54, "SuperRight", ModifierKey::Super, true, CMD | 0x10),
    ];

    fn modifier(vk: u16) -> Option<&'static Modifier> {
        MODIFIERS.iter().find(|m| m.vk == vk)
    }

    fn set(mods: &mut Mods, key: ModifierKey) {
        match key {
            ModifierKey::Shift => mods.shift = true,
            ModifierKey::Control => mods.ctrl = true,
            ModifierKey::Alt => mods.alt = true,
            ModifierKey::Super => mods.super_ = true,
            ModifierKey::CapsLock => {}
        }
    }

    /// One chord: the modifiers held for it, its key, how often it repeats, and what the key
    /// should come out as.
    struct Chord {
        name: &'static str,
        mods: &'static [u16],
        vk: u16,
        repeats: u32,
        key: Key,
        code: &'static str,
        unshifted: Option<char>,
    }

    const fn ch(
        name: &'static str,
        mods: &'static [u16],
        vk: u16,
        key: Key,
        code: &'static str,
        unshifted: Option<char>,
    ) -> Chord {
        Chord {
            name,
            mods,
            vk,
            repeats: 0,
            key,
            code,
            unshifted,
        }
    }

    const fn hold(name: &'static str, vk: u16, c: char, code: &'static str) -> Chord {
        Chord {
            repeats: 6,
            ..ch(name, &[], vk, Key::Char(c), code, Some(c))
        }
    }

    const fn modifier_alone(name: &'static str, vk: u16, key: ModifierKey) -> Chord {
        ch(name, &[], vk, Key::Modifier(key), "", None)
    }

    const fn c(ch: char) -> Key {
        Key::Char(ch)
    }

    /// S1's matrix (spikes/s1-renderer/src/keys.rs).
    const MATRIX: &[Chord] = &[
        ch("a", &[], 0, c('a'), "KeyA", Some('a')),
        ch("S-a", &[56], 0, c('A'), "KeyA", Some('a')),
        ch("RS-a", &[60], 0, c('A'), "KeyA", Some('a')),
        ch("C-a", &[59], 0, c('a'), "KeyA", Some('a')),
        ch("C-[", &[59], 33, c('['), "BracketLeft", Some('[')),
        ch("C-/", &[59], 44, c('/'), "Slash", Some('/')),
        ch("C-Return", &[59], 36, Key::Enter, "Enter", None),
        ch("C-Tab", &[59], 48, Key::Tab, "Tab", None),
        ch("C-Space", &[59], 49, c(' '), "Space", Some(' ')),
        ch("C-g", &[59], 5, c('g'), "KeyG", Some('g')),
        ch("C-S-a", &[59, 56], 0, c('A'), "KeyA", Some('a')),
        ch("M-x (LOpt)", &[58], 7, c('x'), "KeyX", Some('x')),
        ch("M-x (ROpt)", &[61], 7, c('x'), "KeyX", Some('x')),
        ch("M-f", &[58], 3, c('f'), "KeyF", Some('f')),
        ch("M-e", &[58], 14, c('e'), "KeyE", Some('e')),
        ch("M-S-f", &[58, 56], 3, c('F'), "KeyF", Some('f')),
        ch("C-M-a", &[59, 58], 0, c('a'), "KeyA", Some('a')),
        ch("Cmd-c", &[55], 8, c('c'), "KeyC", Some('c')),
        ch("Cmd-Esc", &[55], 53, Key::Esc, "Escape", None),
        ch("RCmd-Esc", &[54], 53, Key::Esc, "Escape", None),
        ch("Esc", &[], 53, Key::Esc, "Escape", None),
        ch("Return", &[], 36, Key::Enter, "Enter", None),
        ch("Tab", &[], 48, Key::Tab, "Tab", None),
        ch("S-Tab", &[56], 48, Key::Tab, "Tab", None),
        ch("Backspace", &[], 51, Key::Backspace, "Backspace", None),
        ch("Fwd-Delete", &[], 117, Key::Delete, "Delete", None),
        ch("Up", &[], 126, Key::Up, "ArrowUp", None),
        ch("S-Up", &[56], 126, Key::Up, "ArrowUp", None),
        ch("C-Left", &[59], 123, Key::Left, "ArrowLeft", None),
        ch("M-Right", &[58], 124, Key::Right, "ArrowRight", None),
        ch("Down", &[], 125, Key::Down, "ArrowDown", None),
        ch("Home", &[], 115, Key::Home, "Home", None),
        ch("PageDown", &[], 121, Key::PageDown, "PageDown", None),
        ch("F1", &[], 122, Key::F(1), "F1", None),
        ch("F5", &[], 96, Key::F(5), "F5", None),
        ch("F12", &[], 111, Key::F(12), "F12", None),
        ch("KP-1", &[], 83, c('1'), "Numpad1", Some('1')),
        ch("KP-Enter", &[], 76, Key::Enter, "NumpadEnter", None),
        ch("S-4 ($)", &[56], 21, c('$'), "Digit4", Some('4')),
        ch("\\ (leader)", &[], 42, c('\\'), "Backslash", Some('\\')),
        hold("hold j", 38, 'j', "KeyJ"),
        hold("hold l", 37, 'l', "KeyL"),
        hold("hold e", 14, 'e', "KeyE"),
        modifier_alone("LShift alone", 56, ModifierKey::Shift),
        modifier_alone("RShift alone", 60, ModifierKey::Shift),
        modifier_alone("LCtrl alone", 59, ModifierKey::Control),
        modifier_alone("LOpt alone", 58, ModifierKey::Alt),
        modifier_alone("ROpt alone", 61, ModifierKey::Alt),
        modifier_alone("LCmd alone", 55, ModifierKey::Super),
        modifier_alone("Caps Lock", CAPS, ModifierKey::CapsLock),
    ];

    /// What's held, as a key event carries it.
    fn fields(ev: KeyEvent, held: &[u16]) -> KeyEvent {
        let (mut mods, mut right_mods) = (Mods::default(), Mods::default());
        for m in held.iter().filter_map(|&vk| modifier(vk)) {
            set(&mut mods, m.key);
            if m.right {
                set(&mut right_mods, m.key);
            }
        }
        KeyEvent {
            mods,
            right_mods,
            ..ev
        }
    }

    fn modifier_event(m: &Modifier, kind: KeyKind, held: &[u16]) -> KeyEvent {
        let ev = KeyEvent {
            code: Some(m.code.into()),
            ..KeyEvent::press(Key::Modifier(m.key)).kind(kind)
        };
        fields(ev, held)
    }

    fn caps_event(on: bool) -> KeyEvent {
        let kind = if on { KeyKind::Press } else { KeyKind::Release };
        KeyEvent {
            code: Some("CapsLock".into()),
            caps_lock: on,
            ..KeyEvent::press(Key::Modifier(ModifierKey::CapsLock)).kind(kind)
        }
    }

    /// Every event a chord should give, whole, in order.
    fn expected(c: &Chord) -> Vec<KeyEvent> {
        let mut out = vec![];
        let mut held = vec![];
        for &vk in c.mods {
            held.push(vk);
            out.push(modifier_event(modifier(vk).unwrap(), KeyKind::Press, &held));
        }
        if c.vk == CAPS {
            out.extend([caps_event(true), caps_event(false)]);
        } else if let Some(alone) = modifier(c.vk) {
            out.push(modifier_event(alone, KeyKind::Press, &[c.vk]));
            out.push(modifier_event(alone, KeyKind::Release, &[]));
        } else {
            let key = |kind| {
                let ev = KeyEvent {
                    code: Some(c.code.into()),
                    unshifted: c.unshifted,
                    ..KeyEvent::press(c.key).kind(kind)
                };
                fields(ev, &held)
            };
            out.push(key(KeyKind::Press));
            out.extend((0..c.repeats).map(|_| key(KeyKind::Repeat)));
            if !c.mods.iter().any(|vk| matches!(vk, 54 | 55)) {
                out.push(key(KeyKind::Release));
            }
        }
        while let Some(vk) = held.pop() {
            out.push(modifier_event(
                modifier(vk).unwrap(),
                KeyKind::Release,
                &held,
            ));
        }
        out
    }

    fn post(vk: u16, kind: Option<bool>, flags: u64) {
        // `None` is a modifier's flags-changed event, whose flags say which way it went.
        let event = CGEvent::new_keyboard_event(None, vk, kind.unwrap_or(true)).expect("an event");
        if kind.is_none() {
            CGEvent::set_type(Some(&event), CGEventType::FlagsChanged);
        }
        CGEvent::set_flags(Some(&event), CGEventFlags(flags));
        CGEvent::post_to_pid(std::process::id() as i32, Some(&event));
    }

    /// A repeat of a key that types `c`.
    fn post_repeat(r: &Remote, vk: u16, c: char) -> Result<(), String> {
        r.call(move |_, _| {
            let text = NSString::from_str(&c.to_string());
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                NSEventType::KeyDown,
                NSPoint::new(0.0, 0.0),
                NSEventModifierFlags(0),
                0.0,
                0,
                None,
                &text,
                &text,
                true,
                vk,
            )
            .expect("an event");
            let mtm = MainThreadMarker::new().expect("the main thread");
            NSApplication::sharedApplication(mtm).postEvent_atStart(&event, false);
        })
    }

    fn post_chord(r: &Remote, c: &Chord) -> Result<(), String> {
        let mut flags = 0;
        for m in c.mods.iter().filter_map(|&vk| modifier(vk)) {
            flags |= m.flags;
            post(m.vk, None, flags);
        }
        if c.vk == CAPS {
            post(CAPS, None, CAPS_FLAG);
            post(CAPS, None, 0);
        } else if let Some(alone) = modifier(c.vk) {
            post(alone.vk, None, alone.flags);
            post(alone.vk, None, 0);
        } else {
            post(c.vk, Some(true), flags);
            if let (true, Key::Char(typed)) = (c.repeats > 0, c.key) {
                // Spaced out, so that each comes after what the window server is still bringing.
                for _ in 0..c.repeats {
                    sleep(Duration::from_millis(50));
                    post_repeat(r, c.vk, typed)?;
                }
                sleep(Duration::from_millis(50));
            }
            post(c.vk, Some(false), flags);
        }
        for m in c.mods.iter().rev().filter_map(|&vk| modifier(vk)) {
            flags &= !m.flags;
            post(m.vk, None, flags);
        }
        Ok(())
    }

    /// The keys that reach core, until there are `want` of them, or a little after.
    fn keys(r: &Remote, want: usize) -> Result<Vec<KeyEvent>, String> {
        let start = Instant::now();
        let mut got = vec![];
        while got.len() < want && start.elapsed() < Duration::from_secs(5) {
            got.extend(r.call(|s, _| s.take_keys())?);
            sleep(Duration::from_millis(2));
        }
        // Anything past what was wanted shows up in here.
        sleep(Duration::from_millis(10));
        got.extend(r.call(|s, _| s.take_keys())?);
        Ok(got)
    }

    fn the_matrix_holds(r: &Remote) -> Result<(), String> {
        let mut wrong = vec![];
        for chord in MATRIX {
            let want = expected(chord);
            post_chord(r, chord)?;
            let got = keys(r, want.len())?;
            if got != want {
                wrong.push(format!("{}\n  got  {got:?}\n  want {want:?}", chord.name));
            }
        }
        if wrong.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} of {} chords:\n{}",
                wrong.len(),
                MATRIX.len(),
                wrong.join("\n")
            ))
        }
    }

    /// A panic in the monitor doesn't reach AppKit, which would end the process. The keys go on,
    /// in order, and a line says so.
    fn a_panic_in_the_key_monitor_is_caught(r: &Remote) -> Result<(), String> {
        r.call(|s, _| {
            let monitor = s.key_monitor().expect("a key monitor");
            monitor.set_on_caps(|| panic!("a panic planted in the key monitor"));
        })?;
        post(CAPS, None, CAPS_FLAG);
        post(CAPS, None, 0);
        post(0, Some(true), 0);
        post(0, Some(false), 0);
        let got = keys(r, 4)?;
        let a = |kind| KeyEvent {
            code: Some("KeyA".into()),
            unshifted: Some('a'),
            ..KeyEvent::press(Key::Char('a')).kind(kind)
        };
        let want = vec![
            caps_event(true),
            caps_event(false),
            a(KeyKind::Press),
            a(KeyKind::Release),
        ];
        if got != want {
            return Err(format!("got {got:?}\nwant {want:?}"));
        }
        let panics = r.call(|s, _| s.key_monitor().map(|m| m.panics()))?;
        if panics != Some(2) {
            return Err(format!("{panics:?} panics counted, not 2"));
        }
        let logged = r.call(|s, _| {
            let line = "the key monitor panicked, and goes on";
            s.messages().iter().any(|m| m.contains(line))
        })?;
        if !logged {
            return Err("no line in *messages* says so".into());
        }
        Ok(())
    }

    /// The Character Viewer inserts text with no key and no marked text, which winit drops.
    fn text_inserted_without_a_key_reaches_core(r: &Remote) -> Result<(), String> {
        r.call(|s, _| s.insert_text("\u{1F600}"))?;
        let got = keys(r, 1)?;
        if got.len() != 1 || got[0].text.as_deref() != Some("\u{1F600}") {
            return Err(format!("core got {got:?}"));
        }
        Ok(())
    }

    /// A test failed. A quit ends the loop with 0, so this keeps that from hiding it.
    static FAILED: AtomicBool = AtomicBool::new(false);

    /// An argument of the process's, which must survive the loop's setup.
    const ARG: &str = "DIE_KEYS_ARG";

    /// Run this binary again with arguments the defaults system reads. That domain outranks your
    /// own defaults, so its yes stands in for one in your NSGlobalDomain, which a test mustn't
    /// write. A regular defaults domain would lose to it and the accent popup would stay.
    pub fn in_a_process_saying_press_and_hold() {
        let me = std::env::current_exe().unwrap();
        let args = ["-ApplePressAndHoldEnabled", "YES"];
        let status = std::process::Command::new(me)
            .args(args)
            .args([format!("-{ARG}"), "7".into()])
            .status()
            .unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }

    type Test = fn(&Remote) -> Result<(), String>;

    fn script(r: &Remote) -> i32 {
        let press_and_hold = driver::press_and_hold();
        let arg = NSUserDefaults::standardUserDefaults().integerForKey(&NSString::from_str(ARG));
        let setup = r.call(|s, _| {
            s.deliver_keys_here();
            s.record_keys();
        });
        if let Err(e) = setup {
            println!("{e}");
            return 1;
        }
        let tests: [(&str, Test); 3] = [
            ("the_matrix_holds", the_matrix_holds),
            (
                "a_panic_in_the_key_monitor_is_caught",
                a_panic_in_the_key_monitor_is_caught,
            ),
            (
                "text_inserted_without_a_key_reaches_core",
                text_inserted_without_a_key_reaches_core,
            ),
        ];
        let mut code = 0;
        if press_and_hold {
            println!("test press_and_hold_is_off ... FAILED");
            code = 1;
        }
        if arg != 7 {
            println!("test the_process_keeps_its_arguments ... FAILED");
            code = 1;
        }
        for (name, test) in tests {
            match test(r) {
                Ok(()) => println!("test {name} ... ok"),
                Err(why) => {
                    println!("test {name} ... FAILED\n{why}");
                    code = 1;
                }
            }
        }
        FAILED.store(code != 0, Ordering::SeqCst);
        if r.call(|s, _| s.ctx().quit()).is_err() {
            return 1;
        }
        sleep(Duration::from_secs(10));
        println!("test a_quit_request_ends_the_loop ... FAILED");
        1
    }

    pub fn run() {
        assert_eq!(MATRIX.len(), 50);
        if driver::ready().is_err() {
            println!("no window here: skipped");
            return;
        }
        let ctx = Ctx::new(vec![]).unwrap();
        let options = window::Options {
            font: Config::term(16.0, 1.0),
            title: "DIE keys".into(),
            script: Some(Box::new(script)),
            hidden: true,
            first_frame: None,
        };
        let code = window::run(ctx, options).expect("a window");
        if code == 0 {
            println!("test a_quit_request_ends_the_loop ... ok");
        }
        let code = if FAILED.load(Ordering::SeqCst) {
            1
        } else {
            code
        };
        std::process::exit(code);
    }
}
