//! Keys, the pointer, pastes and focus as bytes for the PTY (the record's G4, G5 and G13).
//!
//! Keys go through the engine's encoder, which follows the program's modes: DECCKM, the keypad,
//! modifyOtherKeys and the kitty keyboard flags, release and repeat included. Alt (Option) is
//! Meta. One shim on top: in legacy encoding, Ctrl-[, Ctrl-i and Ctrl-m alone send Esc, Tab and
//! Return, as GUI Emacs and Vim fold them and die-core's keymaps do. Ghostty sends them as CSI u
//! so that they stay apart, which needs the kitty flags to be readable; with the flags on, they
//! do stay apart.

use crate::Term;
use die_core::{Key, KeyEvent, KeyKind, ModifierKey, Mods};
use libghostty_vt::key::{self, Action, Mods as EngineMods, OptionAsAlt};
use libghostty_vt::terminal::{ClipboardMime, Mode};
use libghostty_vt::{Error, focus, mouse, paste};

pub(crate) struct Input {
    keys: key::Encoder<'static>,
    key: key::Event<'static>,
    mice: mouse::Encoder<'static>,
    mouse: mouse::Event<'static>,
}

impl Input {
    pub(crate) fn new() -> Result<Input, Error> {
        Ok(Input {
            keys: key::Encoder::new()?,
            key: key::Event::new()?,
            mice: mouse::Encoder::new()?,
            mouse: mouse::Event::new()?,
        })
    }
}

/// What the pointer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerAction {
    /// A button went down, or the wheel turned one step.
    Press,
    /// A button came up.
    Release,
    /// The pointer moved.
    Motion,
}

/// A pointer button, or a wheel step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    /// The primary button.
    Left,
    /// The middle button.
    Middle,
    /// The secondary button.
    Right,
    /// One step of the wheel away from you.
    WheelUp,
    /// One step toward you.
    WheelDown,
    /// One step left.
    WheelLeft,
    /// One step right.
    WheelRight,
    /// The back button.
    Back,
    /// The forward button.
    Forward,
}

/// A pointer event over the terminal's grid. The integrator maps core's pointer events to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// What happened.
    pub action: PointerAction,
    /// The button, for a press or release; for motion, the button held, if any.
    pub button: Option<PointerButton>,
    /// The modifiers held.
    pub mods: Mods,
    /// Physical pixels from the grid's left edge.
    pub x: f32,
    /// Physical pixels from the grid's top edge.
    pub y: f32,
}

fn engine_mods(m: Mods, right: Mods, caps: bool, num: bool) -> EngineMods {
    let mut out = EngineMods::empty();
    for (on, bit, side) in [
        (m.shift, EngineMods::SHIFT, right.shift),
        (m.ctrl, EngineMods::CTRL, right.ctrl),
        (m.alt, EngineMods::ALT, right.alt),
        (m.super_, EngineMods::SUPER, right.super_),
    ] {
        if on {
            out |= bit;
            if side {
                out |= match bit {
                    EngineMods::SHIFT => EngineMods::SHIFT_SIDE,
                    EngineMods::CTRL => EngineMods::CTRL_SIDE,
                    EngineMods::ALT => EngineMods::ALT_SIDE,
                    _ => EngineMods::SUPER_SIDE,
                };
            }
        }
    }
    if caps {
        out |= EngineMods::CAPS_LOCK;
    }
    if num {
        out |= EngineMods::NUM_LOCK;
    }
    out
}

/// The key's unshifted character, and whether Shift made the character the key typed. The OS
/// gives the unshifted key; for an event from notation, which has none, an uppercase letter is
/// its lowercase one with Shift, as ghostel's are.
fn unshifted(ev: &KeyEvent) -> Option<(char, bool)> {
    let Key::Char(c) = ev.key else { return None };
    match ev.unshifted {
        Some(u) => Some((u, ev.mods.shift && u != c)),
        None if c.is_ascii_uppercase() => Some((c.to_ascii_lowercase(), true)),
        None => Some((c, false)),
    }
}

/// The engine's key for an event: its physical key when the OS named it, otherwise the key a
/// US layout types it with.
fn engine_key(ev: &KeyEvent) -> key::Key {
    if let Some(k) = ev.code.as_deref().and_then(engine_key::from_code) {
        return k;
    }
    use key::Key as K;
    let right = |m: bool, r: K, l: K| if m { r } else { l };
    match ev.key {
        Key::Esc => K::Escape,
        Key::Enter => K::Enter,
        Key::Tab => K::Tab,
        Key::Backspace => K::Backspace,
        Key::Delete => K::Delete,
        Key::Up => K::ArrowUp,
        Key::Down => K::ArrowDown,
        Key::Left => K::ArrowLeft,
        Key::Right => K::ArrowRight,
        Key::Home => K::Home,
        Key::End => K::End,
        Key::PageUp => K::PageUp,
        Key::PageDown => K::PageDown,
        Key::Insert => K::Insert,
        Key::F(n @ 1..=25) => K::try_from(i32::from(n) - 1 + K::F1 as i32).unwrap_or(K::F1),
        Key::Modifier(ModifierKey::Shift) => {
            right(ev.right_mods.shift, K::ShiftRight, K::ShiftLeft)
        }
        Key::Modifier(ModifierKey::Control) => {
            right(ev.right_mods.ctrl, K::ControlRight, K::ControlLeft)
        }
        Key::Modifier(ModifierKey::Alt) => right(ev.right_mods.alt, K::AltRight, K::AltLeft),
        Key::Modifier(ModifierKey::Super) => right(ev.right_mods.super_, K::MetaRight, K::MetaLeft),
        Key::Modifier(ModifierKey::CapsLock) => K::CapsLock,
        Key::Char(_) => unshifted(ev).map_or(K::Unidentified, |(u, _)| engine_key::from_char(u)),
        _ => K::Unidentified,
    }
}

impl Term {
    /// The bytes a key sends to the program, as its modes ask; nothing for a key they don't
    /// report, such as a release without the kitty flags for it. The DIE layer's keys (Cmd on
    /// macOS) are the surface's to keep from here (scope D18).
    pub fn key(&mut self, ev: &KeyEvent) -> Vec<u8> {
        let legacy = self.term.kitty_keyboard_flags().map_or(0, |f| f.bits()) == 0;
        let m = ev.mods;
        let bare_ctrl = m.ctrl && !m.alt && !m.shift && !m.super_;
        let folded = match (ev.key, legacy && bare_ctrl) {
            (Key::Char('['), true) => Some(Key::Esc),
            (Key::Char('i'), true) => Some(Key::Tab),
            (Key::Char('m'), true) => Some(Key::Enter),
            _ => None,
        };
        if let Some(k) = folded {
            let mut plain = KeyEvent::press(k).kind(ev.kind);
            plain.caps_lock = ev.caps_lock;
            return self.key(&plain);
        }
        let shifted = unshifted(ev);
        let mut mods = engine_mods(m, ev.right_mods, ev.caps_lock, ev.num_lock);
        // Notation writes an uppercase letter without Shift; the encoder wants it held.
        if shifted.is_some_and(|(_, s)| s) {
            mods |= EngineMods::SHIFT;
        }
        let text = match (&ev.text, ev.key) {
            (Some(t), _) => Some(t.clone()),
            (None, Key::Char(c)) if !c.is_control() => Some(c.to_string()),
            _ => None,
        };
        let action = match ev.kind {
            KeyKind::Press => Action::Press,
            KeyKind::Repeat => Action::Repeat,
            KeyKind::Release => Action::Release,
        };
        let input = &mut self.input;
        input.keys.set_options_from_terminal(&self.term);
        input.keys.set_macos_option_as_alt(OptionAsAlt::True);
        input
            .key
            .set_key(engine_key(ev))
            .set_mods(mods)
            .set_action(action)
            .set_consumed_mods(if shifted.is_some_and(|(_, s)| s) {
                EngineMods::SHIFT
            } else {
                EngineMods::empty()
            })
            .set_utf8(text)
            .set_unshifted_codepoint(shifted.map_or('\0', |(u, _)| u));
        let mut out = Vec::new();
        let _ = input.keys.encode_to_vec(&input.key, &mut out);
        out
    }

    /// The bytes a pointer event sends, as the program's mouse tracking and format ask (SGR
    /// pixels included); nothing while it doesn't track the mouse, when the pointer is the
    /// surface's.
    pub fn mouse(&mut self, p: &Pointer) -> Vec<u8> {
        let (cols, rows) = self.size();
        let (cw, ch) = self.cell_px;
        let held = p.action == PointerAction::Motion && p.button.is_some();
        let input = &mut self.input;
        input
            .mice
            .set_options_from_terminal(&self.term)
            .set_size(mouse::EncoderSize {
                screen_width: u32::from(cols) * cw,
                screen_height: u32::from(rows) * ch,
                cell_width: cw,
                cell_height: ch,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(held);
        let action = match p.action {
            PointerAction::Press => mouse::Action::Press,
            PointerAction::Release => mouse::Action::Release,
            PointerAction::Motion => mouse::Action::Motion,
        };
        let button = p.button.map(|b| match b {
            PointerButton::Left => mouse::Button::Left,
            PointerButton::Middle => mouse::Button::Middle,
            PointerButton::Right => mouse::Button::Right,
            PointerButton::WheelUp => mouse::Button::Four,
            PointerButton::WheelDown => mouse::Button::Five,
            PointerButton::WheelLeft => mouse::Button::Six,
            PointerButton::WheelRight => mouse::Button::Seven,
            PointerButton::Back => mouse::Button::Eight,
            PointerButton::Forward => mouse::Button::Nine,
        });
        input
            .mouse
            .set_action(action)
            .set_button(button)
            .set_mods(engine_mods(p.mods, Mods::default(), false, false))
            .set_position(mouse::Position { x: p.x, y: p.y });
        let mut out = Vec::new();
        let _ = input.mice.encode_to_vec(&input.mouse, &mut out);
        out
    }

    /// Turn the wheel `steps` steps at pixel (x, y), up when negative. A program that tracks the
    /// mouse gets the steps as wheel buttons. On the alternate screen with mode 1007, it gets
    /// arrow keys (less, man). Otherwise the viewport scrolls by `steps` rows and nothing is
    /// sent.
    pub fn wheel(&mut self, steps: i32, x: f32, y: f32, mods: Mods) -> Vec<u8> {
        let modes = self.modes();
        let mut out = Vec::new();
        let n = steps.unsigned_abs();
        if modes.mouse != crate::MouseTracking::None {
            let button = if steps < 0 {
                PointerButton::WheelUp
            } else {
                PointerButton::WheelDown
            };
            let p = Pointer {
                action: PointerAction::Press,
                button: Some(button),
                mods,
                x,
                y,
            };
            for _ in 0..n {
                out.extend(self.mouse(&p));
            }
        } else if modes.alt_screen && modes.alt_scroll {
            let k = if steps < 0 { Key::Up } else { Key::Down };
            for _ in 0..n {
                out.extend(self.key(&KeyEvent::press(k)));
            }
        } else {
            self.scroll(steps as isize);
        }
        out
    }

    /// The bytes that paste `text`, bracketed if the program asked for it (mode 2004), with
    /// control bytes made spaces. Text that could run a command it wasn't meant to (a newline
    /// outside a bracketed paste, or the bracket's own end) is refused unless `allow_unsafe`:
    /// ask, then paste again with it.
    pub fn paste(&mut self, text: &str, allow_unsafe: bool) -> Result<Vec<u8>, PasteRefused> {
        let earlier = std::mem::take(&mut self.out.borrow_mut().pty);
        let mimes = [ClipboardMime::new("text/plain")];
        // As text, never as a kitty paste event (mode 5522): the program would read the event's
        // contents back through the clipboard, and reads are denied.
        let options = paste::Options::new()
            .with_source(paste::Source::Text)
            .with_allow_unsafe(allow_unsafe);
        let done = self
            .term
            .paste(options, &mimes, |_, w| w.write_all(text.as_bytes()));
        let pasted = std::mem::replace(&mut self.out.borrow_mut().pty, earlier);
        match done {
            Ok(_) => Ok(pasted),
            Err(Error::Rejected) => Err(PasteRefused),
            Err(_) => Ok(Vec::new()),
        }
    }

    /// The bytes that tell the program DIE's window gained or lost the keyboard focus, if it
    /// asked to know (mode 1004).
    pub fn focus(&mut self, gained: bool) -> Vec<u8> {
        if !self.term.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
            return Vec::new();
        }
        let ev = if gained {
            focus::Event::Gained
        } else {
            focus::Event::Lost
        };
        let mut buf = [0u8; 8];
        ev.encode(&mut buf)
            .map_or(Vec::new(), |n| buf[..n].to_vec())
    }
}

/// A paste that could run a command the text didn't mean to: confirm, then paste with
/// `allow_unsafe`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteRefused;

/// The engine's keys by W3C `code` name, and by the character a US layout's key types.
mod engine_key {
    use libghostty_vt::key::Key as K;

    const CODES: &[(&str, K)] = &[
        ("Backquote", K::Backquote),
        ("Backslash", K::Backslash),
        ("BracketLeft", K::BracketLeft),
        ("BracketRight", K::BracketRight),
        ("Comma", K::Comma),
        ("Equal", K::Equal),
        ("IntlBackslash", K::IntlBackslash),
        ("IntlRo", K::IntlRo),
        ("IntlYen", K::IntlYen),
        ("Minus", K::Minus),
        ("Period", K::Period),
        ("Quote", K::Quote),
        ("Semicolon", K::Semicolon),
        ("Slash", K::Slash),
        ("AltLeft", K::AltLeft),
        ("AltRight", K::AltRight),
        ("Backspace", K::Backspace),
        ("CapsLock", K::CapsLock),
        ("ContextMenu", K::ContextMenu),
        ("ControlLeft", K::ControlLeft),
        ("ControlRight", K::ControlRight),
        ("Enter", K::Enter),
        ("MetaLeft", K::MetaLeft),
        ("MetaRight", K::MetaRight),
        ("SuperLeft", K::MetaLeft),
        ("SuperRight", K::MetaRight),
        ("ShiftLeft", K::ShiftLeft),
        ("ShiftRight", K::ShiftRight),
        ("Space", K::Space),
        ("Tab", K::Tab),
        ("Delete", K::Delete),
        ("End", K::End),
        ("Help", K::Help),
        ("Home", K::Home),
        ("Insert", K::Insert),
        ("PageDown", K::PageDown),
        ("PageUp", K::PageUp),
        ("ArrowDown", K::ArrowDown),
        ("ArrowLeft", K::ArrowLeft),
        ("ArrowRight", K::ArrowRight),
        ("ArrowUp", K::ArrowUp),
        ("NumLock", K::NumLock),
        ("NumpadAdd", K::NumpadAdd),
        ("NumpadComma", K::NumpadComma),
        ("NumpadDecimal", K::NumpadDecimal),
        ("NumpadDivide", K::NumpadDivide),
        ("NumpadEnter", K::NumpadEnter),
        ("NumpadEqual", K::NumpadEqual),
        ("NumpadMultiply", K::NumpadMultiply),
        ("NumpadSubtract", K::NumpadSubtract),
        ("Escape", K::Escape),
        ("Fn", K::Fn),
    ];

    /// The engine's key for a W3C `code` name, if it has one.
    pub(super) fn from_code(code: &str) -> Option<K> {
        let at = |base: K, n: u32| K::try_from(base as i32 + n as i32).ok();
        let digit = |s: &str| s.parse::<u32>().ok().filter(|_| s.len() == 1);
        if let Some(l) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
            let c = l.as_bytes()[0];
            return c
                .is_ascii_uppercase()
                .then(|| at(K::A, u32::from(c - b'A')))
                .flatten();
        }
        if let Some(d) = code.strip_prefix("Digit").and_then(digit) {
            return at(K::Digit0, d);
        }
        if let Some(d) = code.strip_prefix("Numpad").and_then(digit) {
            return at(K::Numpad0, d);
        }
        if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
            return (1..=25).contains(&n).then(|| at(K::F1, n - 1)).flatten();
        }
        CODES
            .iter()
            .find(|(name, _)| *name == code)
            .map(|(_, k)| *k)
    }

    /// The engine's key for the unshifted character a US layout's key types.
    pub(super) fn from_char(c: char) -> K {
        let name = match c {
            'a'..='z' => return K::try_from(K::A as i32 + (c as i32 - 'a' as i32)).unwrap_or(K::A),
            '0'..='9' => {
                return K::try_from(K::Digit0 as i32 + (c as i32 - '0' as i32))
                    .unwrap_or(K::Digit0);
            }
            ' ' => "Space",
            '`' => "Backquote",
            '\\' => "Backslash",
            '[' => "BracketLeft",
            ']' => "BracketRight",
            ',' => "Comma",
            '=' => "Equal",
            '-' => "Minus",
            '.' => "Period",
            '\'' => "Quote",
            ';' => "Semicolon",
            '/' => "Slash",
            _ => return K::Unidentified,
        };
        from_code(name).unwrap_or(K::Unidentified)
    }
}
