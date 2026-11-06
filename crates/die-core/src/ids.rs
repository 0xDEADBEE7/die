//! Generational ids (C4). A dead id never matches a new object, even after slot reuse.
use slotmap::new_key_type;

new_key_type! {
    /// A buffer, of any kind.
    pub struct BufferId;
    /// A window.
    pub struct WindowId;
    /// A marker in a text buffer.
    pub struct MarkerId;
    /// A decoration in a text buffer.
    pub struct DecorationId;
}

// Ids are Copy + Send + 'static, so 'static callbacks and threads can capture them.
const _: () = {
    const fn ok<T: Copy + Send + 'static>() {}
    ok::<BufferId>();
    ok::<WindowId>();
    ok::<MarkerId>();
    ok::<DecorationId>();
};
