//! Generational ids (C4). A dead id never matches a new object, even after slot reuse.
use slotmap::new_key_type;

new_key_type! {
    pub struct BufferId;
    pub struct WindowId;
    pub struct MarkerId;
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
