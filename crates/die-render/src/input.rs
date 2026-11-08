//! Input (R4): winit's keys and pointer, as die-core's `KeyEvent` and `PointerEvent`. All of
//! this is data in and data out, so its tests run anywhere. The one thing winit can't give is
//! Caps Lock on macOS, which `driver::KeyMonitor` turns into events.
//!
//! winit reports which side of a modifier is held only on macOS, so `Held` takes the sides
//! from the modifier keys' own events, and winit's modifier state only corrects it, for a key
//! released while another app had the keyboard (docs/records/platforms.md).

use crate::font::CellMetrics;
use die_core::{Button, Key, KeyEvent, KeyKind, ModifierKey, Mods, PointerEvent, PointerKind};
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey, SmolStr};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// Which modifiers are held, on which side, and whether Caps Lock is on.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Held {
    left: Mods,
    right: Mods,
    caps: bool,
}

impl Held {
    /// A key with this physical code went down or up. Anything but a modifier is ignored.
    pub fn key(&mut self, code: KeyCode, down: bool) {
        match code {
            KeyCode::ShiftLeft => self.left.shift = down,
            KeyCode::ShiftRight => self.right.shift = down,
            KeyCode::ControlLeft => self.left.ctrl = down,
            KeyCode::ControlRight => self.right.ctrl = down,
            KeyCode::AltLeft => self.left.alt = down,
            KeyCode::AltRight => self.right.alt = down,
            KeyCode::SuperLeft => self.left.super_ = down,
            KeyCode::SuperRight => self.right.super_ = down,
            KeyCode::CapsLock if down => self.caps = !self.caps,
            _ => {}
        }
    }

    /// What the OS says is held now. A modifier it says is up is up on both sides, and one it
    /// says is down, with no key's event to say which side, counts as the left one.
    pub fn sync(&mut self, state: ModifiersState) {
        let (l, r) = (&mut self.left, &mut self.right);
        for (on, left, right) in [
            (state.shift_key(), &mut l.shift, &mut r.shift),
            (state.control_key(), &mut l.ctrl, &mut r.ctrl),
            (state.alt_key(), &mut l.alt, &mut r.alt),
            (state.super_key(), &mut l.super_, &mut r.super_),
        ] {
            if !on {
                (*left, *right) = (false, false);
            } else if !*left && !*right {
                *left = true;
            }
        }
    }

    /// Caps Lock is on or off, as the key monitor saw it.
    pub fn set_caps(&mut self, on: bool) {
        self.caps = on;
    }

    /// The modifiers held on either side.
    pub fn mods(&self) -> Mods {
        let (l, r) = (self.left, self.right);
        Mods {
            ctrl: l.ctrl || r.ctrl,
            alt: l.alt || r.alt,
            shift: l.shift || r.shift,
            super_: l.super_ || r.super_,
        }
    }

    /// The modifiers held on the right.
    pub fn right_mods(&self) -> Mods {
        self.right
    }

    /// Whether Caps Lock is on.
    pub fn caps(&self) -> bool {
        self.caps
    }
}

/// What a key event from winit says, as values, since a test can't make winit's own.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyFacts {
    /// The key went down, rather than up.
    pub down: bool,
    /// The OS repeated it.
    pub repeat: bool,
    /// The key on the keyboard.
    pub physical: PhysicalKey,
    /// What the layout produced, with Shift applied.
    pub logical: WinitKey,
    /// What it produces with no modifiers.
    pub unshifted: WinitKey,
    /// The text it commits.
    pub text: Option<SmolStr>,
}

impl From<&winit::event::KeyEvent> for KeyFacts {
    fn from(e: &winit::event::KeyEvent) -> KeyFacts {
        KeyFacts {
            down: e.state == ElementState::Pressed,
            repeat: e.repeat,
            physical: e.physical_key,
            logical: e.logical_key.clone(),
            unshifted: e.key_without_modifiers(),
            text: e.text.clone(),
        }
    }
}

/// A named key as die-core has it. The function keys it names stop at F24.
fn named(n: NamedKey) -> Key {
    let modifier = Key::Modifier;
    match n {
        NamedKey::Escape => Key::Esc,
        NamedKey::Enter => Key::Enter,
        NamedKey::Tab => Key::Tab,
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Delete => Key::Delete,
        NamedKey::ArrowUp => Key::Up,
        NamedKey::ArrowDown => Key::Down,
        NamedKey::ArrowLeft => Key::Left,
        NamedKey::ArrowRight => Key::Right,
        NamedKey::Home => Key::Home,
        NamedKey::End => Key::End,
        NamedKey::PageUp => Key::PageUp,
        NamedKey::PageDown => Key::PageDown,
        NamedKey::Insert => Key::Insert,
        NamedKey::Space => Key::Char(' '),
        NamedKey::Shift => modifier(ModifierKey::Shift),
        NamedKey::Control => modifier(ModifierKey::Control),
        NamedKey::Alt => modifier(ModifierKey::Alt),
        NamedKey::Super | NamedKey::Meta => modifier(ModifierKey::Super),
        NamedKey::CapsLock => modifier(ModifierKey::CapsLock),
        other => format!("{other:?}")
            .strip_prefix('F')
            .and_then(|n| n.parse().ok())
            .filter(|n| (1..=24).contains(n))
            .map_or(Key::Unidentified, Key::F),
    }
}

/// `KeyEvent::text`: what the OS committed beyond the character `key` names. It's never the
/// OS's per-key text for a chord, nor a named key's control character.
fn committed(f: &KeyFacts, mods: Mods) -> Option<String> {
    if !f.down || mods.ctrl || mods.alt || mods.super_ {
        return None;
    }
    if matches!(f.logical, WinitKey::Named(_)) {
        return None;
    }
    let text = f.text.as_ref()?;
    let mut chars = text.chars();
    let (first, more) = (chars.next(), chars.next().is_some());
    let in_key = !more && matches!(&f.logical, WinitKey::Character(s) if s.chars().next() == first);
    (!in_key).then(|| text.to_string())
}

/// The event die-core gets for this key, with `held` already updated for it.
pub fn key_event(f: &KeyFacts, held: &Held) -> KeyEvent {
    let key = match &f.logical {
        WinitKey::Named(n) => named(*n),
        WinitKey::Character(s) => s.chars().next().map_or(Key::Unidentified, Key::Char),
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => Key::Unidentified,
    };
    let kind = match (f.down, f.repeat) {
        (false, _) => KeyKind::Release,
        (true, true) => KeyKind::Repeat,
        (true, false) => KeyKind::Press,
    };
    let code = match f.physical {
        PhysicalKey::Code(code) => Some(format!("{code:?}")),
        PhysicalKey::Unidentified(_) => None,
    };
    let unshifted = match &f.unshifted {
        WinitKey::Character(s) => s.chars().next(),
        WinitKey::Named(NamedKey::Space) => Some(' '),
        _ => None,
    };
    KeyEvent {
        key,
        mods: held.mods(),
        kind,
        text: committed(f, held.mods()),
        code,
        unshifted,
        right_mods: held.right_mods(),
        caps_lock: held.caps(),
        num_lock: false,
    }
}

/// Caps Lock's own event, on a platform where it has none from winit: a press when it turns on,
/// and a release when it turns off.
pub fn caps_event(held: &Held) -> KeyEvent {
    let kind = if held.caps() {
        KeyKind::Press
    } else {
        KeyKind::Release
    };
    KeyEvent {
        code: Some("CapsLock".into()),
        right_mods: held.right_mods(),
        caps_lock: held.caps(),
        ..KeyEvent::press(Key::Modifier(ModifierKey::CapsLock))
            .mods(held.mods())
            .kind(kind)
    }
}

/// The pointer: which cell it's on, which button is down, and how many clicks this is.
#[derive(Clone, Debug)]
pub struct Pointer {
    double_click: Duration,
    at: (usize, usize),
    /// Where it is in physical pixels, for a surface that wants more than the cell.
    px: Option<(f32, f32)>,
    /// The buttons held, oldest first, each with the click count of its press.
    held: Vec<(Button, u8)>,
    last: Option<(Button, (usize, usize), Instant)>,
    clicks: u8,
}

impl Pointer {
    /// A pointer whose presses count as one click run if they come within `double_click`.
    pub fn new(double_click: Duration) -> Pointer {
        Pointer {
            double_click,
            at: (0, 0),
            px: None,
            held: vec![],
            last: None,
            clicks: 0,
        }
    }

    /// It moved to `px` in a frame of `grid` (columns, rows). An event only if it's on another
    /// cell, since nothing DIE does depends on where in a cell it is. Each event says the pixel.
    pub fn moved(
        &mut self,
        px: (f64, f64),
        cell: &CellMetrics,
        grid: (usize, usize),
        mods: Mods,
    ) -> Option<PointerEvent> {
        let along = |p: f64, size: u32, cells: usize| {
            ((p.max(0.0) / f64::from(size.max(1))) as usize).min(cells.saturating_sub(1))
        };
        let at = (along(px.1, cell.h, grid.1), along(px.0, cell.w, grid.0));
        self.px = Some((px.0 as f32, px.1 as f32));
        if at == self.at {
            return None;
        }
        self.at = at;
        let dragging = self.held.first().map(|&(button, _)| button);
        let kind = dragging.map_or(PointerKind::Move, PointerKind::Drag);
        Some(self.event(kind, 0, mods))
    }

    /// A button went down or up, on the cell it's on. Back, forward and the rest have no name
    /// in die-core.
    pub fn button(
        &mut self,
        button: MouseButton,
        down: bool,
        now: Instant,
        mods: Mods,
    ) -> Option<PointerEvent> {
        let button = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
            _ => return None,
        };
        let at = self.held.iter().position(|&(b, _)| b == button);
        let pressed = at.map(|i| self.held.remove(i).1);
        if !down {
            let clicks = pressed.unwrap_or(1);
            return Some(self.event(PointerKind::Release(button), clicks, mods));
        }
        let again = self.last.is_some_and(|(b, at, t)| {
            b == button && at == self.at && now.duration_since(t) <= self.double_click
        });
        self.clicks = if again {
            self.clicks.saturating_add(1)
        } else {
            1
        };
        self.last = Some((button, self.at, now));
        self.held.push((button, self.clicks));
        Some(self.event(PointerKind::Press(button), self.clicks, mods))
    }

    /// A wheel or trackpad scroll, which winit gives as how the content moves, and die-core
    /// takes as how the view does. A scroll of nothing, as a gesture's end is, is no event.
    pub fn wheel(
        &self,
        delta: MouseScrollDelta,
        cell: &CellMetrics,
        mods: Mods,
    ) -> Option<PointerEvent> {
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (-x, -y),
            MouseScrollDelta::PixelDelta(p) => (
                (-p.x / f64::from(cell.w.max(1))) as f32,
                (-p.y / f64::from(cell.h.max(1))) as f32,
            ),
        };
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        Some(self.event(PointerKind::Scroll { dx, dy }, 0, mods))
    }

    fn event(&self, kind: PointerKind, clicks: u8, mods: Mods) -> PointerEvent {
        PointerEvent {
            kind,
            row: self.at.0,
            col: self.at.1,
            clicks,
            mods,
            px: self.px,
        }
    }
}
