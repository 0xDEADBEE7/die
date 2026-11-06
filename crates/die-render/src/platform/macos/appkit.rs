//! AppKit: the event loop's and the window's macOS options, the window's layer, and the
//! key monitor. The clipboard (R5) joins here.

use super::metal::{Pipeline, TARGET_FORMAT};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSRunningApplication,
    NSView, NSWindow, NSWindowOcclusionState,
};
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSDictionary, NSNumber, NSPoint, NSString, NSUserDefaults};
use objc2_metal::{MTLDrawable, MTLDrawablePresentedHandler};
use objc2_quartz_core::{CACurrentMediaTime, CAMetalDrawable, CAMetalLayer};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
/// Holding a key repeats it: that's set for this process only, and nothing goes to disk.
pub fn prepare_loop<T: 'static>(builder: &mut EventLoopBuilder<T>, dock: bool) {
    let key = NSString::from_str(PRESS_AND_HOLD);
    let no = NSNumber::new_bool(false);
    let defaults = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[&**no]);
    // SAFETY: the dictionary's values are NSNumbers, which the defaults system can hold.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
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

/// The moment this host time was, on the clock an event's timestamp and a drawable's
/// presented time both use.
fn instant_at(host_time: f64) -> Instant {
    let age = Duration::try_from_secs_f64(CACurrentMediaTime() - host_time).unwrap_or_default();
    let now = Instant::now();
    now.checked_sub(age).unwrap_or(now)
}

/// Post a key to this app's own event queue, timestamped now, as a key from the OS would be.
/// Nothing leaves the process. A check does this, through `KeyMonitor::deliver_to`.
pub fn post_key(vk: u16, c: char, down: bool) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let kind = [NSEventType::KeyUp, NSEventType::KeyDown][usize::from(down)];
    let text = NSString::from_str(&c.to_string());
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        kind,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags(0),
        CACurrentMediaTime(),
        0,
        None,
        &text,
        &text,
        false,
        vk,
    )
    .ok_or("AppKit made no event")?;
    NSApplication::sharedApplication(mtm).postEvent_atStart(&event, false);
    Ok(())
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
    deliver_to: RefCell<Option<Retained<NSWindow>>>,
    /// When the last key press happened, by the event's own timestamp.
    key_at: Cell<Option<Instant>>,
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
        if kind == NSEventType::KeyDown && e.timestamp() > 0.0 {
            self.key_at.set(Some(instant_at(e.timestamp())));
        }
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
            deliver_to: RefCell::new(None),
            key_at: Cell::new(None),
        });
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

    /// Whether Caps Lock was on at the last event.
    pub fn caps(&self) -> bool {
        self.seen.caps.get()
    }

    /// How many panics it has caught.
    pub fn panics(&self) -> u64 {
        self.seen.panics.get()
    }

    /// When the key press it saw last happened, if it hasn't been asked since.
    pub fn take_key_at(&self) -> Option<Instant> {
        self.seen.key_at.take()
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

/// A present, and when it reached the screen, or None if it never did.
type Shown = (u64, Option<Instant>);

/// A window's view and the layer DIE draws on, which DIE owns, with its scale and size.
pub struct Surface {
    view: Retained<NSView>,
    window: Retained<NSWindow>,
    pub(super) layer: Retained<CAMetalLayer>,
    /// Whether the next frame comes back as an image: `Presented::image`.
    pub(super) read_next: Cell<bool>,
    /// Whether its drawables can be read: they aren't framebuffer-only.
    readable: bool,
    /// How many presents it has had.
    presents: Cell<u64>,
    /// Whether to note when each present reached the screen.
    watching: Cell<bool>,
    /// Each watched present and when it reached the screen, or None if it never did.
    presented: Arc<Mutex<Vec<Shown>>>,
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
            presents: Cell::new(0),
            watching: Cell::new(false),
            presented: Arc::default(),
        })
    }

    /// From now on, note when each present reaches the screen.
    pub fn watch_presents(&self) {
        self.watching.set(true);
    }

    /// The presents that have reached the screen, or been dropped, since the last call.
    pub fn take_presented(&self) -> Vec<Shown> {
        std::mem::take(&mut *self.presented.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Count a present of `drawable`, watching it if asked to. Its id.
    pub(super) fn next_present(&self, drawable: &ProtocolObject<dyn CAMetalDrawable>) -> u64 {
        let id = self.presents.get() + 1;
        self.presents.set(id);
        if self.watching.get() {
            let presented = self.presented.clone();
            let block = RcBlock::new(move |shown: NonNull<ProtocolObject<dyn MTLDrawable>>| {
                // SAFETY: Metal passes the live drawable for the length of this call.
                let time = unsafe { shown.as_ref() }.presentedTime();
                let at = (time > 0.0).then(|| instant_at(time));
                let shown = presented.lock();
                shown.unwrap_or_else(|e| e.into_inner()).push((id, at));
            });
            let handler: MTLDrawablePresentedHandler = RcBlock::as_ptr(&block);
            // SAFETY: the pointer is to a live block of the type the method says, and Metal
            // copies it. Metal may call it on any thread, and all it holds is `Send`.
            unsafe { drawable.addPresentedHandler(handler) };
        }
        id
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
