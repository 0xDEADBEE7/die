//! AppKit: the event loop's and the window's macOS options, the window's layer, and the
//! key monitor. The clipboard (R5) joins here.

use super::metal::{Pipeline, TARGET_FORMAT};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSRunningApplication,
    NSView, NSWindow, NSWindowOcclusionState,
};
use objc2_core_foundation::CGSize;
use objc2_foundation::{
    NSArgumentDomain, NSDictionary, NSNotFound, NSNumber, NSRange, NSString, NSUserDefaults,
};
use objc2_quartz_core::CAMetalLayer;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{NonNull, null_mut};
use std::rc::{Rc, Weak};
use std::time::Duration;
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::{
    ActivationPolicy, EventLoopBuilderExtMacOS, OptionAsAlt, WindowAttributesExtMacOS,
    WindowExtMacOS,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

/// The default that makes holding a key offer accents rather than repeat.
const PRESS_AND_HOLD: &str = "ApplePressAndHoldEnabled";
/// The virtual key codes the monitor cares about.
const CAPS_LOCK: u16 = 57;
const SLASH: u16 = 44;
const RETURN: u16 = 36;

/// No default menu, and never activate the app on launch. With `dock`, it's a regular app
/// with a Dock icon, and otherwise an accessory, which has none: a check's window is one.
pub fn prepare_loop<T: 'static>(builder: &mut EventLoopBuilder<T>, dock: bool) {
    hold_repeats();
    let policy = if dock {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    builder
        .with_default_menu(false)
        .with_activate_ignoring_other_apps(false)
        .with_activation_policy(policy);
}

/// Holding a key repeats it, whatever your own defaults say: the process's argument domain
/// outranks them, and stays in memory, so nothing goes to disk. Its real arguments stay.
fn hold_repeats() {
    let key = NSString::from_str(PRESS_AND_HOLD);
    let no = NSNumber::new_bool(false);
    let defaults = NSUserDefaults::standardUserDefaults();
    // SAFETY: Foundation's constant names the domain, and it's read and replaced whole, with
    // what it held and an NSNumber, which the defaults system can hold.
    unsafe {
        let (mut keys, mut values) = defaults.volatileDomainForName(NSArgumentDomain).to_vecs();
        if let Some(at) = keys.iter().position(|k| **k == *key) {
            keys.remove(at);
            values.remove(at);
        }
        let mut names: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let mut objects: Vec<&AnyObject> = values.iter().map(|v| &**v).collect();
        names.push(&key);
        objects.push(&no);
        let domain = NSDictionary::from_slices(&names, &objects);
        defaults.removeVolatileDomainForName(NSArgumentDomain);
        defaults.setVolatileDomain_forName(&domain, NSArgumentDomain);
    }
}

/// Option acts as Alt on both sides.
pub fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes.with_option_as_alt(OptionAsAlt::Both)
}

/// Whether Option acts as Alt on both sides of this window.
pub fn option_as_alt(window: &Window) -> bool {
    window.option_as_alt() == OptionAsAlt::Both
}

/// Whether holding a key would offer accents.
pub fn press_and_hold() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(PRESS_AND_HOLD))
}

/// How close together two presses are one click run, as you've set it.
pub fn double_click_interval() -> Duration {
    Duration::try_from_secs_f64(NSEvent::doubleClickInterval())
        .unwrap_or(Duration::from_millis(500))
}

/// What AppKit treats as a key equivalent and beeps at, never reaching the view: Ctrl-/ and
/// Ctrl-Return, with no Option or Cmd (winit #4093).
fn trapped(code: u16, flags: NSEventModifierFlags) -> bool {
    let other = NSEventModifierFlags::Command | NSEventModifierFlags::Option;
    matches!(code, SLASH | RETURN)
        && flags.contains(NSEventModifierFlags::Control)
        && !flags.intersects(other)
}

/// What the monitor's block shares with `KeyMonitor`.
struct Seen {
    mtm: MainThreadMarker,
    caps: Cell<bool>,
    toggles: RefCell<Vec<bool>>,
    panics: Cell<u64>,
    on_caps: RefCell<Box<dyn Fn()>>,
    inserted: RefCell<Vec<String>>,
    deliver_to: RefCell<Option<Retained<NSWindow>>>,
}

impl Seen {
    /// One key event, on its way into `sendEvent:`. Whether this delivered it, so that AppKit
    /// shouldn't. A flags-changed event for Caps Lock is all there is of it: winit drops those.
    fn see(&self, e: &NSEvent) -> bool {
        let (kind, code, flags) = (e.r#type(), e.keyCode(), e.modifierFlags());
        let caps = flags.contains(NSEventModifierFlags::CapsLock);
        self.caps.set(caps);
        if kind == NSEventType::FlagsChanged && code == CAPS_LOCK {
            self.toggles.borrow_mut().push(caps);
            (self.on_caps.borrow())();
        }
        let is_key = matches!(kind, NSEventType::KeyDown | NSEventType::KeyUp);
        let trap = is_key && trapped(code, flags);
        let chosen = self.deliver_to.borrow().clone();
        if let Some(window) = &chosen
            && !trap
        {
            window.sendEvent(e);
            return true;
        }
        if !trap {
            return false;
        }
        let app = NSApplication::sharedApplication(self.mtm);
        let window = chosen.or_else(|| app.keyWindow());
        let Some(responder) = window.and_then(|w| w.firstResponder()) else {
            return false;
        };
        if kind == NSEventType::KeyDown {
            responder.keyDown(e);
        } else {
            responder.keyUp(e);
        }
        true
    }
}

type KeyDownImp = unsafe extern "C-unwind" fn(&NSView, Sel, &NSEvent);
type InsertImp = unsafe extern "C-unwind" fn(&NSView, Sel, &AnyObject, NSRange);

thread_local! {
    /// What `hook_view` replaced. All of this is the main thread's, where views live.
    static ORIGINAL: Cell<Option<(KeyDownImp, InsertImp)>> = const { Cell::new(None) };
    /// How many of winit's `keyDown:` are running.
    static IN_KEY_DOWN: Cell<u32> = const { Cell::new(0) };
    /// The installed monitor's state.
    static SEEN: RefCell<Weak<Seen>> = const { RefCell::new(Weak::new()) };
}

/// The view's `keyDown:`, as winit has it, marking that it's running.
unsafe extern "C-unwind" fn key_down(view: &NSView, sel: Sel, event: &NSEvent) {
    let Some((original, _)) = ORIGINAL.get() else {
        return;
    };
    IN_KEY_DOWN.set(IN_KEY_DOWN.get() + 1);
    // SAFETY: this is what the method was, called as it would have been.
    unsafe { original(view, sel, event) };
    IN_KEY_DOWN.set(IN_KEY_DOWN.get() - 1);
}

/// The view's `insertText:replacementRange:`. winit commits only marked text, since within
/// `keyDown:` the key's own event carries the text, so it drops what comes with neither: the
/// Character Viewer's. That text goes to the monitor, and all else to winit.
unsafe extern "C-unwind" fn insert_text(view: &NSView, sel: Sel, text: &AnyObject, range: NSRange) {
    // SAFETY: winit's view answers this, on the main thread.
    let marked: bool = unsafe { msg_send![view, hasMarkedText] };
    let seen = SEEN.with_borrow(Weak::upgrade);
    let taker = seen.filter(|_| IN_KEY_DOWN.get() == 0 && !marked);
    if let Some(seen) = taker {
        // SAFETY: AppKit's text is an NSString or an NSAttributedString, which answers `string`.
        let string = match text.downcast_ref::<NSString>() {
            Some(string) => string.to_string(),
            None => {
                let string: Option<Retained<NSString>> = unsafe { msg_send![text, string] };
                string.map_or_else(String::new, |s| s.to_string())
            }
        };
        // winit drops a control character too.
        if string.chars().next().is_some_and(|c| !c.is_control()) {
            seen.inserted.borrow_mut().push(string);
            // A panic in it can't unwind into AppKit, as in the monitor's block.
            let woke = catch_unwind(AssertUnwindSafe(|| (seen.on_caps.borrow())()));
            if woke.is_err() {
                seen.panics.set(seen.panics.get() + 1);
            }
        }
    } else if let Some((_, original)) = ORIGINAL.get() {
        // SAFETY: this is what the method was, called as it would have been.
        unsafe { original(view, sel, text, range) };
    }
}

/// Put `key_down` and `insert_text` on winit's view class, once.
fn hook_view(view: &NSView) -> Result<(), String> {
    if ORIGINAL.get().is_some() {
        return Ok(());
    }
    let class = view.class();
    let method = |sel| {
        let found = class.instance_method(sel);
        found.ok_or("winit's view hasn't the text input methods it had")
    };
    let keys = method(sel!(keyDown:))?;
    let inserts = method(sel!(insertText:replacementRange:))?;
    // SAFETY: both replacements take what these methods take (winit's platform_impl/macos/
    // view.rs, at the exact version Cargo.toml pins), and the originals, cast back to those
    // same types, are what they call.
    unsafe {
        let new_keys = std::mem::transmute::<KeyDownImp, Imp>(key_down);
        let new_inserts = std::mem::transmute::<InsertImp, Imp>(insert_text);
        let old_keys = keys.set_implementation(new_keys);
        let old_inserts = inserts.set_implementation(new_inserts);
        ORIGINAL.set(Some((
            std::mem::transmute::<Imp, KeyDownImp>(old_keys),
            std::mem::transmute::<Imp, InsertImp>(old_inserts),
        )));
    }
    Ok(())
}

/// A local event monitor: it runs inside `sendEvent:`, before the event is dispatched. It sees
/// Caps Lock, and hands the chords AppKit traps straight to the first responder. A panic
/// in it can't unwind into AppKit, so it's caught and counted, and the event goes on as if
/// the monitor weren't there. It's removed on drop.
pub struct KeyMonitor {
    token: Retained<AnyObject>,
    seen: Rc<Seen>,
}

impl KeyMonitor {
    /// Install it. `on_caps` runs when Caps Lock changes, so the loop can wake for it.
    pub fn install(on_caps: impl Fn() + 'static) -> Result<KeyMonitor, String> {
        let mtm = MainThreadMarker::new().ok_or("the key monitor isn't on the main thread")?;
        let caps = NSEvent::modifierFlags_class().contains(NSEventModifierFlags::CapsLock);
        let seen = Rc::new(Seen {
            mtm,
            caps: Cell::new(caps),
            toggles: RefCell::default(),
            panics: Cell::new(0),
            on_caps: RefCell::new(Box::new(on_caps)),
            inserted: RefCell::default(),
            deliver_to: RefCell::new(None),
        });
        SEEN.set(Rc::downgrade(&seen));
        let shared = seen.clone();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the length of this call.
            let e = unsafe { event.as_ref() };
            match catch_unwind(AssertUnwindSafe(|| shared.see(e))) {
                Ok(true) => null_mut(),
                Ok(false) => event.as_ptr(),
                Err(_) => {
                    shared.panics.set(shared.panics.get() + 1);
                    event.as_ptr()
                }
            }
        });
        let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
        // SAFETY: the block takes and returns what the method says, and AppKit keeps a copy of
        // it until `removeMonitor`.
        let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
        let token = token.ok_or("AppKit gave no key monitor")?;
        Ok(KeyMonitor { token, seen })
    }

    /// What Caps Lock did since the last call, in order: on or off.
    pub fn take_caps(&self) -> Vec<bool> {
        std::mem::take(&mut self.seen.toggles.borrow_mut())
    }

    /// The text inserted with no key since the last call, in order (`hook_view`).
    pub fn take_inserted(&self) -> Vec<String> {
        std::mem::take(&mut self.seen.inserted.borrow_mut())
    }

    /// Whether Caps Lock was on at the last event.
    pub fn caps(&self) -> bool {
        self.seen.caps.get()
    }

    /// How many panics it has caught.
    pub fn panics(&self) -> u64 {
        self.seen.panics.get()
    }

    /// Run this when Caps Lock changes, rather than what it ran before.
    pub fn set_on_caps(&self, on_caps: impl Fn() + 'static) {
        *self.seen.on_caps.borrow_mut() = Box::new(on_caps);
    }

    /// Deliver every key event to this surface's window, key or not. A test does this to take
    /// keys it posts to its own process, since a window that doesn't take the keyboard from
    /// you gets none.
    pub fn deliver_to(&self, surface: &Surface) {
        *self.seen.deliver_to.borrow_mut() = Some(surface.window.clone());
    }
}

impl Drop for KeyMonitor {
    fn drop(&mut self) {
        // SAFETY: the token is what `addLocalMonitorForEventsMatchingMask:handler:` returned,
        // and only this drop removes it.
        unsafe { NSEvent::removeMonitor(&self.token) };
    }
}

/// A window's view and the layer DIE draws on, which DIE owns, with its scale and size.
pub struct Surface {
    view: Retained<NSView>,
    window: Retained<NSWindow>,
    pub(super) layer: Retained<CAMetalLayer>,
    /// Whether the next frame comes back as an image: `Presented::image`.
    pub(super) read_next: Cell<bool>,
    /// Whether its drawables can be read: they aren't framebuffer-only.
    readable: bool,
}

impl Surface {
    /// Put a new layer on `window`'s view, with the pipeline's device and target format, as
    /// the only thing on it. With `readable`, its drawables can be read back, at some cost.
    pub fn attach(window: &Window, pipeline: &Pipeline, readable: bool) -> Result<Surface, String> {
        MainThreadMarker::new().ok_or("the window isn't on the main thread")?;
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("the window isn't an AppKit window".into());
        };
        // SAFETY: AppKit's handle holds the window's live NSView, retaining it keeps it past
        // the handle, and this is the main thread, checked above, where the view lives.
        let view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) };
        let view = view.ok_or("the window has no view")?;
        let ns_window = view.window().ok_or("the view has no window")?;
        hook_view(&view)?;
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(pipeline.device()));
        layer.setPixelFormat(TARGET_FORMAT);
        layer.setOpaque(true);
        layer.setFramebufferOnly(!readable);
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        // A window that isn't key yet hasn't made its view the first responder.
        ns_window.makeFirstResponder(Some(&view));
        Ok(Surface {
            view,
            window: ns_window,
            layer,
            read_next: Cell::new(false),
            readable,
        })
    }

    /// Hand the view text as an input method does, with no key and no marked text.
    pub fn insert_text(&self, text: &str) {
        let text = NSString::from_str(text);
        let nowhere = NSRange::new(NSNotFound as usize, 0);
        // SAFETY: winit's view takes this NSTextInputClient message, on the main thread, where a
        // surface stays.
        let () = unsafe { msg_send![&*self.view, insertText: &*text, replacementRange: nowhere] };
    }

    /// Read the next frame back, which costs tens of ms. On a surface attached without
    /// `readable` this is an error, and nothing will be read.
    pub fn read_next_frame(&self) -> Result<(), String> {
        if !self.readable {
            return Err("this surface's frames can't be read back".into());
        }
        self.read_next.set(true);
        Ok(())
    }

    /// The view's size now, in pixels at `scale`.
    pub fn size(&self, scale: f64) -> (u32, u32) {
        let size = self.view.bounds().size;
        let px = |v: f64| (v * scale).round() as u32;
        (px(size.width), px(size.height))
    }

    /// Make the drawable `w` × `h` pixels, at `scale`.
    pub fn resize(&self, (w, h): (u32, u32), scale: f64) {
        self.layer.setContentsScale(scale);
        self.layer.setDrawableSize(CGSize {
            width: f64::from(w),
            height: f64::from(h),
        });
    }

    /// Whether none of the window can be seen.
    pub fn occluded(&self) -> bool {
        let state = self.window.occlusionState();
        !state.contains(NSWindowOcclusionState::Visible)
    }

    /// Whether you're dragging the window's edge.
    pub fn in_live_resize(&self) -> bool {
        self.window.inLiveResize()
    }

    /// Whether this app is the active one.
    pub fn app_active(&self) -> bool {
        NSRunningApplication::currentApplication().isActive()
    }
}
