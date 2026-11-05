//! AppKit: the event loop's and the window's macOS options, and the window's layer.
//! The key monitor (R4) and the clipboard (R5) join here.

use super::metal::{Pipeline, TARGET_FORMAT};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSView, NSWindow, NSWindowOcclusionState};
use objc2_core_foundation::CGSize;
use objc2_quartz_core::CAMetalLayer;
use std::cell::Cell;
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::{
    ActivationPolicy, EventLoopBuilderExtMacOS, OptionAsAlt, WindowAttributesExtMacOS,
    WindowExtMacOS,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

/// No default menu, and never activate the app on launch. With `dock`, it's a regular app
/// with a Dock icon, and otherwise an accessory, which has none: a check's window is one.
pub fn prepare_loop<T: 'static>(builder: &mut EventLoopBuilder<T>, dock: bool) {
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

/// A window's view and the layer DIE draws on, which DIE owns, with its scale and size.
pub struct Surface {
    view: Retained<NSView>,
    window: Retained<NSWindow>,
    pub(super) layer: Retained<CAMetalLayer>,
    /// Whether the next frame comes back as an image: `Presented::image`.
    pub(super) read_next: Cell<bool>,
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
        Ok(Surface {
            view,
            window: ns_window,
            layer,
            read_next: Cell::new(false),
        })
    }

    /// Read the next frame back, which costs tens of ms. The surface must be readable.
    pub fn read_next_frame(&self) {
        self.read_next.set(true);
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
