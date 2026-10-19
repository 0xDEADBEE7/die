//! AppKit glue on top of winit: the CAMetalLayer on winit's NSView, the
//! press-and-hold default, an NSEvent local monitor (event timestamps, the
//! C-/ and C-Return fix, Caps Lock), app-side event injection with
//! NSApplication.postEvent, and a would-beep counter. No permission prompts:
//! nothing here uses CGEventPost or Accessibility.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSView, NSWindow, NSWindowOcclusionState};
use objc2_foundation::{NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString, NSUserDefaults};
use objc2_quartz_core::CAMetalLayer;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub static WOULD_BEEP: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Timestamp of the last keyDown seen by the monitor (NSEvent.timestamp).
    pub static LAST_KEYDOWN_TS: Cell<f64> = const { Cell::new(0.0) };
    /// Raw events the monitor saw, for the keyboard matrix.
    pub static RAW: RefCell<Vec<RawEv>> = const { RefCell::new(Vec::new()) };
    pub static FIX_KEYS: Cell<bool> = const { Cell::new(true) };
    static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct RawEv { pub kind: &'static str, pub code: u16, pub flags: usize, pub repeat: bool, pub chars: String, pub ts: f64, pub rerouted: bool }

pub fn ns_view(w: &winit::window::Window) -> Retained<NSView> {
    let h = w.window_handle().unwrap().as_raw();
    let RawWindowHandle::AppKit(h) = h else { panic!("not AppKit") };
    unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView) }.unwrap()
}

pub fn ns_window(v: &NSView) -> Retained<NSWindow> { v.window().expect("view has a window") }

/// Layer-hosting view: we own the CAMetalLayer and its scale and size.
pub fn attach_layer(view: &NSView, layer: &CAMetalLayer) {
    view.setLayer(Some(layer));
    view.setWantsLayer(true);
}

pub fn set_layer_size(layer: &CAMetalLayer, view: &NSView, scale: f64) -> (u32, u32) {
    let b = view.bounds();
    let (w, h) = ((b.size.width * scale).round(), (b.size.height * scale).round());
    layer.setContentsScale(scale);
    layer.setDrawableSize(objc2_core_foundation::CGSize { width: w, height: h });
    (w as u32, h as u32)
}

pub fn backing_px(view: &NSView, scale: f64) -> (u32, u32) {
    let b = view.bounds();
    ((b.size.width * scale).round() as u32, (b.size.height * scale).round() as u32)
}

/// Registers ApplePressAndHoldEnabled = NO in this process's registration
/// domain only (nothing is written to disk), as Ghostty does.
pub fn disable_press_and_hold() -> bool {
    let d = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("ApplePressAndHoldEnabled");
    let no = NSNumber::new_bool(false);
    let dict = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[&**no]);
    unsafe { d.registerDefaults(&dict) };
    d.boolForKey(&key)
}

pub fn occluded(win: &NSWindow) -> bool { !win.occlusionState().contains(NSWindowOcclusionState::Visible) }

const F_CTRL: usize = 1 << 18;
const F_SHIFT: usize = 1 << 17;
const F_OPT: usize = 1 << 19;
const F_CMD: usize = 1 << 20;
const F_CAPS: usize = 1 << 16;

/// Local monitor: runs inside -[NSApplication sendEvent:] before dispatch.
pub fn install_monitor(mtm: MainThreadMarker) {
    let blk = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { ev.as_ref() };
        let ty = e.r#type();
        let kind = match ty { NSEventType::KeyDown => "down", NSEventType::KeyUp => "up", NSEventType::FlagsChanged => "flags", _ => return ev.as_ptr() };
        let flags = e.modifierFlags().0;
        let code = e.keyCode();
        let chars = if ty == NSEventType::FlagsChanged { String::new() } else { e.characters().map(|s| s.to_string()).unwrap_or_default() };
        let repeat = ty != NSEventType::FlagsChanged && e.isARepeat();
        if ty == NSEventType::KeyDown { LAST_KEYDOWN_TS.with(|c| c.set(e.timestamp())); }
        // C-/ and C-Return: AppKit treats them as key equivalents and beeps
        // (winit #4093). Hand them straight to the first responder's keyDown:/keyUp:.
        let reroute = FIX_KEYS.with(|f| f.get()) && ty != NSEventType::FlagsChanged
            && flags & F_CTRL != 0 && flags & (F_CMD | F_OPT) == 0 && (code == 44 || code == 36);
        RAW.with(|r| r.borrow_mut().push(RawEv { kind, code, flags, repeat, chars, ts: e.timestamp(), rerouted: reroute }));
        if reroute {
            if let Some(w) = NSApplication::sharedApplication(MainThreadMarker::new().unwrap()).keyWindow() {
                if let Some(fr) = w.firstResponder() {
                    let s = if ty == NSEventType::KeyDown { sel!(keyDown:) } else { sel!(keyUp:) };
                    unsafe { let _: () = objc2::msg_send![&*fr, performSelector: s, withObject: e]; }
                    return std::ptr::null_mut();
                }
            }
        }
        ev.as_ptr()
    });
    let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
    let m = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &blk) };
    let _ = mtm;
    MONITOR.with(|c| *c.borrow_mut() = m);
}

extern "C-unwind" fn no_responder_for(_this: &AnyObject, _sel: Sel, _ev_sel: Sel) {
    // -[NSResponder noResponderFor:] is what beeps for an unhandled keyDown:.
    WOULD_BEEP.fetch_add(1, Ordering::Relaxed);
}

/// Counts (and silences) would-be beeps for unhandled key events.
pub fn install_beep_counter() {
    let cls = AnyClass::get(c"NSResponder").unwrap();
    let m = cls.instance_method(sel!(noResponderFor:)).unwrap();
    unsafe { m.set_implementation(std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, Sel), Imp>(no_responder_for)) };
}

/// Device-dependent side bits (NX_DEVICE*KEYMASK) that winit reads for left/right.
pub fn side_bits(code: u16) -> usize {
    match code { 56 => 0x2, 60 => 0x4, 59 => 0x1, 62 => 0x2000, 58 => 0x20, 61 => 0x40, 55 => 0x8, 54 => 0x10, _ => 0 }
}
pub fn mod_flag(code: u16) -> usize {
    match code { 56 | 60 => F_SHIFT, 59 | 62 => F_CTRL, 58 | 61 => F_OPT, 55 | 54 => F_CMD, 57 => F_CAPS, _ => 0 }
}

pub enum Inject<'a> { Key { down: bool, code: u16, chars: &'a str, ign: &'a str, repeat: bool }, Flags { code: u16 }, Mouse { down: bool, x: f64, y: f64 } }

/// Posts an event at the end of the app's queue, as hardware events arrive.
/// `flags` is the modifier state *after* this event. Timestamp = now.
pub fn post(mtm: MainThreadMarker, win: &NSWindow, ev: Inject, flags: usize) -> f64 {
    let ts = crate::stats::now_s();
    let wn = win.windowNumber();
    let f = NSEventModifierFlags(flags);
    let e = match ev {
        Inject::Key { down, code, chars, ign, repeat } => NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            if down { NSEventType::KeyDown } else { NSEventType::KeyUp }, NSPoint::new(0.0, 0.0), f, ts, wn, None, &NSString::from_str(chars), &NSString::from_str(ign), repeat, code),
        Inject::Flags { code } => NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::FlagsChanged, NSPoint::new(0.0, 0.0), f, ts, wn, None, &NSString::from_str(""), &NSString::from_str(""), false, code),
        Inject::Mouse { down, x, y } => NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            if down { NSEventType::LeftMouseDown } else { NSEventType::LeftMouseUp }, NSPoint::new(x, y), f, ts, wn, None, 0, 1, if down { 1.0 } else { 0.0 }),
    };
    NSApplication::sharedApplication(mtm).postEvent_atStart(&e.expect("NSEvent"), false);
    ts
}

pub fn set_frame(win: &NSWindow, x: f64, y: f64, w: f64, h: f64) {
    win.setFrame_display(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), true);
}

pub fn is_key(win: &NSWindow) -> bool { win.isKeyWindow() }
