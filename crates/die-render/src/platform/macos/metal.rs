//! Metal: the device and its command queue. R2 adds buffers, textures and the
//! pipeline.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice};

/// The GPU DIE draws with.
pub struct Gpu {
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

impl Gpu {
    /// The system's default GPU, or None on a Mac without Metal.
    pub fn system_default() -> Option<Gpu> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        Some(Gpu { device, queue })
    }

    pub fn name(&self) -> String {
        self.device.name().to_string()
    }
}
