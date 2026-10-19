//! DIE's key event, shaped after what libghostty's key encoder needs
//! (action, W3C physical code, logical key, unshifted codepoint, modifiers
//! with sides, committed text), and the ~45-chord keyboard matrix.

use crate::stats::J;
use winit::event::{ElementState, KeyEvent, Modifiers};
use winit::keyboard::{Key, ModifiersKeyState, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action { Press, Repeat, Release }

pub mod m {
    pub const LSHIFT: u16 = 1; pub const RSHIFT: u16 = 2; pub const LCTRL: u16 = 4; pub const RCTRL: u16 = 8;
    pub const LALT: u16 = 16; pub const RALT: u16 = 32; pub const LSUPER: u16 = 64; pub const RSUPER: u16 = 128; pub const CAPS: u16 = 256;
    pub const SHIFT: u16 = LSHIFT | RSHIFT; pub const CTRL: u16 = LCTRL | RCTRL; pub const ALT: u16 = LALT | RALT; pub const SUPER: u16 = LSUPER | RSUPER;
}

#[derive(Clone, Debug)]
pub struct DieKey {
    pub t: f64, pub action: Action, pub code: String, pub logical: String, pub unshifted: Option<char>,
    pub mods: u16, pub text: Option<String>, pub source: &'static str,
}

pub fn mods_of(md: &Modifiers) -> u16 {
    let p = |s: ModifiersKeyState| s == ModifiersKeyState::Pressed;
    let st = md.state();
    let mut b = 0;
    if p(md.lshift_state()) { b |= m::LSHIFT } if p(md.rshift_state()) { b |= m::RSHIFT }
    if p(md.lcontrol_state()) { b |= m::LCTRL } if p(md.rcontrol_state()) { b |= m::RCTRL }
    if p(md.lalt_state()) { b |= m::LALT } if p(md.ralt_state()) { b |= m::RALT }
    if p(md.lsuper_state()) { b |= m::LSUPER } if p(md.rsuper_state()) { b |= m::RSUPER }
    // side unknown but modifier active: report left
    if st.shift_key() && b & m::SHIFT == 0 { b |= m::LSHIFT }
    if st.control_key() && b & m::CTRL == 0 { b |= m::LCTRL }
    if st.alt_key() && b & m::ALT == 0 { b |= m::LALT }
    if st.super_key() && b & m::SUPER == 0 { b |= m::LSUPER }
    b
}

fn key_str(k: &Key) -> String {
    match k { Key::Character(s) => s.to_string(), Key::Named(n) => format!("{n:?}"), Key::Unidentified(_) => "Unidentified".into(), Key::Dead(c) => format!("Dead({c:?})") }
}

pub fn from_winit(ev: &KeyEvent, md: &Modifiers, t: f64) -> DieKey {
    let action = match (ev.state, ev.repeat) { (ElementState::Released, _) => Action::Release, (_, true) => Action::Repeat, _ => Action::Press };
    let code = match ev.physical_key { PhysicalKey::Code(c) => format!("{c:?}"), PhysicalKey::Unidentified(n) => format!("Unidentified({n:?})") };
    // winit names Space instead of giving ' '; the kitty encoder wants codepoint 32
    let unshifted = match ev.key_without_modifiers() { Key::Character(s) => s.chars().next(), Key::Named(winit::keyboard::NamedKey::Space) => Some(' '), _ => None };
    DieKey { t, action, code, logical: key_str(&ev.logical_key), unshifted, mods: mods_of(md), text: ev.text_with_all_modifiers().map(|s| s.to_string()).or(ev.text.as_ref().map(|s| s.to_string())), source: "winit" }
}

impl DieKey {
    pub fn json(&self) -> J {
        J::obj(vec![("t", J::N(self.t)), ("action", J::s(format!("{:?}", self.action))), ("code", J::s(&self.code)), ("logical", J::s(&self.logical)),
            ("unshifted", self.unshifted.map(|c| J::s(c.to_string())).unwrap_or(J::Null)), ("mods", J::N(self.mods as f64)),
            ("text", self.text.as_ref().map(|s| J::s(s)).unwrap_or(J::Null)), ("source", J::s(self.source))])
    }
}

/// One chord: modifier key codes (side-specific), the main key, what AppKit
/// would put in characters / charactersIgnoringModifiers, how many repeats,
/// and the expected W3C code and unshifted character.
pub struct Chord { pub name: &'static str, pub mods: &'static [u16], pub code: u16, pub chars: &'static str, pub ign: &'static str, pub repeats: u32, pub w3c: &'static str, pub base: Option<char> }

const fn ch(name: &'static str, mods: &'static [u16], code: u16, chars: &'static str, ign: &'static str, repeats: u32, w3c: &'static str, base: Option<char>) -> Chord {
    Chord { name, mods, code, chars, ign, repeats, w3c, base }
}
// macOS virtual key codes: LShift 56 RShift 60 LCtrl 59 RCtrl 62 LOpt 58 ROpt 61 LCmd 55 RCmd 54 Caps 57
pub const MATRIX: &[Chord] = &[
    ch("a", &[], 0, "a", "a", 0, "KeyA", Some('a')),
    ch("S-a", &[56], 0, "A", "A", 0, "KeyA", Some('a')),
    ch("RS-a", &[60], 0, "A", "A", 0, "KeyA", Some('a')),
    ch("C-a", &[59], 0, "\u{1}", "a", 0, "KeyA", Some('a')),
    ch("C-[", &[59], 33, "\u{1b}", "[", 0, "BracketLeft", Some('[')),
    ch("C-/", &[59], 44, "\u{1f}", "/", 0, "Slash", Some('/')),
    ch("C-Return", &[59], 36, "\r", "\r", 0, "Enter", None),
    ch("C-Tab", &[59], 48, "\t", "\t", 0, "Tab", None),
    ch("C-Space", &[59], 49, " ", " ", 0, "Space", Some(' ')),
    ch("C-g", &[59], 5, "\u{7}", "g", 0, "KeyG", Some('g')),
    ch("C-S-a", &[59, 56], 0, "\u{1}", "A", 0, "KeyA", Some('a')),
    ch("M-x (LOpt)", &[58], 7, "\u{2248}", "x", 0, "KeyX", Some('x')),
    ch("M-x (ROpt)", &[61], 7, "\u{2248}", "x", 0, "KeyX", Some('x')),
    ch("M-f", &[58], 3, "\u{192}", "f", 0, "KeyF", Some('f')),
    ch("M-e (dead key on US/AU)", &[58], 14, "\u{b4}", "e", 0, "KeyE", Some('e')),
    ch("M-S-f", &[58, 56], 3, "\u{cf}", "F", 0, "KeyF", Some('f')),
    ch("C-M-a", &[59, 58], 0, "\u{1}", "a", 0, "KeyA", Some('a')),
    ch("Cmd-c", &[55], 8, "c", "c", 0, "KeyC", Some('c')),
    ch("Cmd-Esc", &[55], 53, "\u{1b}", "\u{1b}", 0, "Escape", None),
    ch("RCmd-Esc", &[54], 53, "\u{1b}", "\u{1b}", 0, "Escape", None),
    ch("Esc", &[], 53, "\u{1b}", "\u{1b}", 0, "Escape", None),
    ch("Return", &[], 36, "\r", "\r", 0, "Enter", None),
    ch("Tab", &[], 48, "\t", "\t", 0, "Tab", None),
    ch("S-Tab", &[56], 48, "\u{19}", "\t", 0, "Tab", None),
    ch("Backspace", &[], 51, "\u{7f}", "\u{7f}", 0, "Backspace", None),
    ch("Fwd-Delete", &[], 117, "\u{f728}", "\u{f728}", 0, "Delete", None),
    ch("Up", &[], 126, "\u{f700}", "\u{f700}", 0, "ArrowUp", None),
    ch("S-Up", &[56], 126, "\u{f700}", "\u{f700}", 0, "ArrowUp", None),
    ch("C-Left", &[59], 123, "\u{f702}", "\u{f702}", 0, "ArrowLeft", None),
    ch("M-Right", &[58], 124, "\u{f703}", "\u{f703}", 0, "ArrowRight", None),
    ch("Down", &[], 125, "\u{f701}", "\u{f701}", 0, "ArrowDown", None),
    ch("Home", &[], 115, "\u{f729}", "\u{f729}", 0, "Home", None),
    ch("PageDown", &[], 121, "\u{f72d}", "\u{f72d}", 0, "PageDown", None),
    ch("F1", &[], 122, "\u{f704}", "\u{f704}", 0, "F1", None),
    ch("F5", &[], 96, "\u{f708}", "\u{f708}", 0, "F5", None),
    ch("F12", &[], 111, "\u{f70f}", "\u{f70f}", 0, "F12", None),
    ch("KP-1", &[], 83, "1", "1", 0, "Numpad1", Some('1')),
    ch("KP-Enter", &[], 76, "\u{3}", "\u{3}", 0, "NumpadEnter", None),
    ch("S-4 ($)", &[56], 21, "$", "$", 0, "Digit4", Some('4')),
    ch("\\ (leader)", &[], 42, "\\", "\\", 0, "Backslash", Some('\\')),
    ch("hold j", &[], 38, "j", "j", 6, "KeyJ", Some('j')),
    ch("hold l", &[], 37, "l", "l", 6, "KeyL", Some('l')),
    ch("hold e (accent candidate)", &[], 14, "e", "e", 6, "KeyE", Some('e')),
    ch("LShift alone", &[], 56, "", "", 0, "ShiftLeft", None),
    ch("RShift alone", &[], 60, "", "", 0, "ShiftRight", None),
    ch("LCtrl alone", &[], 59, "", "", 0, "ControlLeft", None),
    ch("LOpt alone", &[], 58, "", "", 0, "AltLeft", None),
    ch("ROpt alone", &[], 61, "", "", 0, "AltRight", None),
    ch("LCmd alone", &[], 55, "", "", 0, "SuperLeft", None),
    ch("Caps Lock", &[], 57, "", "", 0, "CapsLock", None),
];

pub fn is_modifier_code(code: u16) -> bool { matches!(code, 54..=63) }

/// W3C code name winit uses for a macOS modifier key code.
pub fn mods_code(code: u16) -> &'static str {
    match code { 56 => "ShiftLeft", 60 => "ShiftRight", 59 => "ControlLeft", 62 => "ControlRight", 58 => "AltLeft", 61 => "AltRight", 55 => "SuperLeft", 54 => "SuperRight", _ => "" }
}
