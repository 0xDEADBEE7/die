//! AppKit: the event loop's and the window's macOS options, the window's layer, the key monitor,
//! and the files you open with DIE from Finder.

use super::catch::guarded;
use super::metal::{Pipeline, TARGET_FORMAT};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, ProtocolObject, Sel};
use objc2::{MainThreadMarker, ffi, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSRunningApplication,
    NSView, NSWindow, NSWindowOcclusionState,
};
use objc2_core_foundation::CGSize;
use objc2_foundation::{
    NSArgumentDomain, NSArray, NSDictionary, NSNotFound, NSNumber, NSPoint, NSRange, NSString,
    NSURL, NSUserDefaults,
};
use objc2_metal::{MTLDrawable, MTLDrawablePresentedHandler};
use objc2_quartz_core::{CACurrentMediaTime, CAMetalDrawable, CAMetalLayer};
use std::cell::{Cell, RefCell};
use std::mem::transmute;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::ptr::{NonNull, null_mut};
use std::rc::{Rc, Weak};
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

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
}

/// How many times the OS has woken this process's threads, which is what costs battery.
pub fn process_wakeups() -> u64 {
    // rusage_info_v0: a 16-byte id, user and system time, the two kinds of wakeup, and 6 more.
    let mut usage = [0u64; 12];
    // SAFETY: flavor 0 fills 96 bytes, which `usage` is, and it lives through the call.
    let _ = unsafe { proc_pid_rusage(std::process::id() as i32, 0, usage.as_mut_ptr()) };
    usage[4] + usage[5]
}

/// No default menu, and never activate the app on launch. With `dock`, it's a regular app
/// with a Dock icon, and otherwise an accessory, which has none: a check's window is one. An error
/// says what didn't take, and the loop can run without it.
pub fn prepare_loop<T: 'static>(
    builder: &mut EventLoopBuilder<T>,
    dock: bool,
) -> Result<(), String> {
    let policy = if dock {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    builder
        .with_default_menu(false)
        .with_activate_ignoring_other_apps(false)
        .with_activation_policy(policy);
    // `setVolatileDomain:forName:` raises if the domain is there.
    guarded("can't make held keys repeat", hold_repeats)
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
    inserted: RefCell<Vec<String>>,
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
        let string = match text.downcast_ref::<NSString>() {
            Some(string) => string.to_string(),
            None => {
                // SAFETY: AppKit's text is an NSString or an NSAttributedString, which answers
                // `string`. Were it anything else that raises, and there's no text to take.
                let string: Result<Option<Retained<NSString>>, String> =
                    guarded("", || unsafe { msg_send![text, string] });
                let string = string.ok().flatten();
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
    // SAFETY: `key_down` takes what `keyDown:` takes (winit's platform_impl/macos/view.rs, at
    // the exact version Cargo.toml pins).
    let new_keys = unsafe { transmute::<KeyDownImp, Imp>(key_down) };
    // SAFETY: as above: `insert_text` takes what `insertText:replacementRange:` takes.
    let new_inserts = unsafe { transmute::<InsertImp, Imp>(insert_text) };
    // SAFETY: it's the implementation just cast, and of this method's type.
    let old_keys = unsafe { keys.set_implementation(new_keys) };
    // SAFETY: as above.
    let old_inserts = unsafe { inserts.set_implementation(new_inserts) };
    // SAFETY: this was the method's implementation, and that's its type.
    let old_keys = unsafe { transmute::<Imp, KeyDownImp>(old_keys) };
    // SAFETY: as above.
    let old_inserts = unsafe { transmute::<Imp, InsertImp>(old_inserts) };
    ORIGINAL.set(Some((old_keys, old_inserts)));
    Ok(())
}

type OpenUrlsImp = unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>);
/// What takes the files.
type OpenFiles = Box<dyn Fn(Vec<PathBuf>)>;

thread_local! {
    /// What `on_open_files` hands the files to. The main thread's, where AppKit calls the delegate.
    static OPEN: RefCell<Option<OpenFiles>> = const { RefCell::new(None) };
}

/// The app delegate's `application:openURLs:`, which AppKit calls with the files Finder, the Dock
/// or `open` asks this app to open.
unsafe extern "C-unwind" fn open_urls(_: &AnyObject, _: Sel, _: &AnyObject, urls: &NSArray<NSURL>) {
    let paths: Vec<PathBuf> = urls.iter().filter_map(|url| file_path(&url)).collect();
    // A panic can't unwind into AppKit.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        OPEN.with_borrow(|open| open.as_ref().map(|f| f(paths)))
    }));
}

/// A file URL's path, composed (NFC) as a name typed in a terminal is. AppKit's file URLs and
/// their file system representation decompose accents (NFD), so a file opened from Finder would
/// have a name that differs from the same one typed, and `:b café` wouldn't find it.
pub fn file_path(url: &NSURL) -> Option<PathBuf> {
    let path = url.isFileURL().then(|| url.path()).flatten()?;
    let composed = path.precomposedStringWithCanonicalMapping();
    Some(composed.to_string().into())
}

/// Hand `f` the files you open with DIE from Finder, the Dock or `open -a`, on the main thread, as
/// they come. winit 0.30 has no event for them and its app delegate no `application:openURLs:`, so
/// this adds one to the delegate's class. Call it once the loop is built and before it runs: a
/// launch to open files gets them as it starts.
pub fn on_open_files(f: OpenFiles) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app.delegate().ok_or("the app has no delegate")?;
    let class: &AnyClass = AsRef::<AnyObject>::as_ref(&*delegate).class();
    let sel = sel!(application:openURLs:);
    OPEN.set(Some(f));
    if class.instance_method(sel).is_some() {
        return Ok(());
    }
    // SAFETY: `open_urls` takes what the method does: the delegate, the selector, the app and the
    // array of URLs, and returns nothing.
    let imp = unsafe { transmute::<OpenUrlsImp, Imp>(open_urls) };
    let class = (class as *const AnyClass).cast_mut();
    // SAFETY: the class is live, the method is new to it, and "v@:@@" is `open_urls`'s type.
    let added = unsafe { ffi::class_addMethod(class, sel, imp, c"v@:@@".as_ptr()) };
    if !added.as_bool() {
        return Err("can't take the files Finder opens with DIE".into());
    }
    Ok(())
}

/// winit's window's view and NSWindow.
fn view_of(window: &Window) -> Result<(Retained<NSView>, Retained<NSWindow>), String> {
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
    Ok((view, ns_window))
}

/// No one sees the window or clicks it, and it casts no shadow, for a test's DIE (`UNSEEN`). The
/// window server still has it, and calls it covered, so it draws its first frame and then waits.
pub fn unseen(window: &Window) -> Result<(), String> {
    let (_, ns_window) = view_of(window)?;
    ns_window.setAlphaValue(0.0);
    ns_window.setIgnoresMouseEvents(true);
    ns_window.setHasShadow(false);
    Ok(())
}

/// A local event monitor: it runs inside `sendEvent:`, before the event is dispatched. It sees
/// Caps Lock, and hands the chords AppKit traps straight to the first responder. A panic or
/// exception in it can't unwind into AppKit, so it's caught and counted, and the event goes on as
/// if the monitor weren't there. It's removed on drop.
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
            key_at: Cell::new(None),
        });
        SEEN.set(Rc::downgrade(&seen));
        let shared = seen.clone();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the length of this call.
            let e = unsafe { event.as_ref() };
            // What it hands on can raise too, and that can't be let reach a `catch_unwind`.
            match catch_unwind(AssertUnwindSafe(|| guarded("", || shared.see(e)))) {
                Ok(Ok(true)) => null_mut(),
                Ok(Ok(false)) => event.as_ptr(),
                _ => {
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

    /// How many panics and exceptions it has caught.
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
        let (view, ns_window) = view_of(window)?;
        hook_view(&view)?;
        let layer = guarded("can't put a layer on the window", || {
            let layer = CAMetalLayer::new();
            layer.setDevice(Some(pipeline.device()));
            layer.setPixelFormat(TARGET_FORMAT);
            layer.setOpaque(true);
            layer.setFramebufferOnly(!readable);
            view.setLayer(Some(&layer));
            view.setWantsLayer(true);
            // A window that isn't key yet hasn't made its view the first responder.
            ns_window.makeFirstResponder(Some(&view));
            layer
        })?;
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

    /// Hand the view text as an input method does, with no key and no marked text.
    pub fn insert_text(&self, text: &str) {
        let text = NSString::from_str(text);
        let nowhere = NSRange::new(NSNotFound as usize, 0);
        // SAFETY: winit's view takes this NSTextInputClient message, on the main thread, where a
        // surface stays.
        let () = unsafe { msg_send![&*self.view, insertText: &*text, replacementRange: nowhere] };
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

    /// Make this the active app, with this window in front and restored from the Dock. The newer
    /// `activate` only takes what the active app yields, and no terminal yields anything.
    pub fn come_to_front(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }
}
