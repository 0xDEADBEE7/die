//! Metal: the device and its command queue, the one pipeline, its two atlas textures,
//! and drawing a frame's instances to a texture and reading it back. What the
//! instances are is neutral code's business. A frame goes to a texture read back, or to
//! the window's drawable, and either way `render` and `present` wait for the GPU, which
//! the SAFETY comments on `upload` and `fill_instances` rely on. Doing without that wait
//! takes S1's ring of buffers.

use super::appkit::Surface;
use crate::frame::{Image, Plane, Presented, Quad, Upload};
use die_core::Rgb;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBinding, MTLBindingType, MTLBlendFactor, MTLBuffer, MTLBufferBinding, MTLClearColor,
    MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLDrawable, MTLLibrary, MTLLoadAction, MTLOrigin,
    MTLPipelineOption, MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
    MTLResourceOptions, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
    MTLTextureUsage,
};
use objc2_quartz_core::{CAMetalDrawable, CATransaction};
use std::mem::size_of_val;
use std::ptr::NonNull;

/// The pipeline's shader. `Quad` here must match `frame::Quad`, field for field, which
/// `Gpu::instance_layout` lets a test check against the compiled shader.
const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Quad { float4 rect; float4 uv; uint color; uint kind; uint2 pad; };
struct Out { float4 pos [[position]]; float2 uv; float4 color; uint kind [[flat]]; };
vertex Out vs(uint v [[vertex_id]], uint i [[instance_id]],
              constant float2& target [[buffer(0)]], const device Quad* quads [[buffer(1)]]) {
  Quad q = quads[i];
  float2 corner = float2(v & 1, v >> 1);
  float2 px = q.rect.xy + corner * q.rect.zw;
  Out o;
  o.pos = float4(px / target * float2(2, -2) + float2(-1, 1), 0, 1);
  o.uv = q.uv.xy + corner * q.uv.zw;
  uint c = q.color;
  o.color = float4((c >> 24) & 255, (c >> 16) & 255, (c >> 8) & 255, c & 255) / 255.0;
  o.kind = q.kind;
  return o;
}
fragment float4 fs(Out in [[stage_in]],
                   texture2d<float> gray [[texture(0)]], texture2d<float> color [[texture(1)]]) {
  constexpr sampler s(coord::pixel, filter::nearest);
  if (in.kind == 0) { return float4(in.color.rgb * in.color.a, in.color.a); }
  if (in.kind == 1) { float a = gray.sample(s, in.uv).r * in.color.a; return float4(in.color.rgb * a, a); }
  return color.sample(s, in.uv);
}
"#;

/// The format of a frame's target. A window's layer has to match.
pub(super) const TARGET_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

/// The longest side every Apple GPU has for a 2D texture. Some have more.
const MAX_TEXTURE_SIDE: u32 = 16384;

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type State = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;

/// The GPU DIE draws with.
pub struct Gpu {
    /// The device.
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    /// Its command queue.
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

impl Gpu {
    /// The system's default GPU, or None on a Mac without Metal.
    pub fn system_default() -> Option<Gpu> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        Some(Gpu { device, queue })
    }

    /// The device's name.
    pub fn name(&self) -> String {
        self.device.name().to_string()
    }

    fn descriptor(&self) -> Result<Retained<MTLRenderPipelineDescriptor>, String> {
        let source = NSString::from_str(MSL);
        let lib = self
            .device
            .newLibraryWithSource_options_error(&source, None)
            .map_err(|e| format!("the shader doesn't compile: {e:?}"))?;
        let function = |name: &str| {
            lib.newFunctionWithName(&NSString::from_str(name))
                .ok_or_else(|| format!("the shader has no function {name}"))
        };
        let d = MTLRenderPipelineDescriptor::new();
        d.setVertexFunction(Some(&*function("vs")?));
        d.setFragmentFunction(Some(&*function("fs")?));
        // SAFETY: attachment 0 exists: Metal has at least eight, and this is the first.
        let target = unsafe { d.colorAttachments().objectAtIndexedSubscript(0) };
        target.setPixelFormat(TARGET_FORMAT);
        target.setBlendingEnabled(true);
        target.setSourceRGBBlendFactor(MTLBlendFactor::One);
        target.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        target.setSourceAlphaBlendFactor(MTLBlendFactor::One);
        target.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        Ok(d)
    }

    fn state(&self) -> Result<State, String> {
        self.device
            .newRenderPipelineStateWithDescriptor_error(&*self.descriptor()?)
            .map_err(|e| format!("no pipeline: {e:?}"))
    }

    fn texture(
        &self,
        (w, h): (u32, u32),
        format: MTLPixelFormat,
        usage: MTLTextureUsage,
    ) -> Result<Texture, String> {
        // Metal stops the process for a size it can't make, rather than returning none.
        if w == 0 || h == 0 || w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
            return Err(format!("Metal has no {w}x{h} texture"));
        }
        // SAFETY: a valid format, from the constants here, and sizes checked above.
        let d = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                format, w as usize, h as usize, false,
            )
        };
        d.setUsage(usage);
        d.setStorageMode(MTLStorageMode::Shared);
        self.device
            .newTextureWithDescriptor(&d)
            .ok_or_else(|| format!("no {w}x{h} texture"))
    }

    /// The pipeline and its atlases, square textures of these sizes in pixels.
    pub fn pipeline(self, gray: u32, color: u32) -> Result<Pipeline, String> {
        let state = self.state()?;
        let read = MTLTextureUsage::ShaderRead;
        let gray = self.texture((gray, gray), MTLPixelFormat::R8Unorm, read)?;
        let color = self.texture((color, color), TARGET_FORMAT, read)?;
        Ok(Pipeline {
            gpu: self,
            state,
            gray,
            color,
            instances: None,
        })
    }

    /// The instance struct as the compiled shader lays it out: its size, then each
    /// member's name and offset.
    pub fn instance_layout(&self) -> Result<(usize, Vec<(String, usize)>), String> {
        let no = |what: &str| format!("the shader's reflection has no {what}");
        let mut reflection = None;
        self.device
            .newRenderPipelineStateWithDescriptor_options_reflection_error(
                &*self.descriptor()?,
                MTLPipelineOption::BufferTypeInfo | MTLPipelineOption::BindingInfo,
                Some(&mut reflection),
            )
            .map_err(|e| format!("no pipeline: {e:?}"))?;
        let bindings = reflection.ok_or_else(|| no("pipeline"))?.vertexBindings();
        let binding = bindings
            .iter()
            .find(|b| b.r#type() == MTLBindingType::Buffer && b.name().to_string() == "quads")
            .ok_or_else(|| no("quads buffer"))?;
        // SAFETY: a binding whose type is Buffer conforms to MTLBufferBinding.
        let binding: Retained<ProtocolObject<dyn MTLBufferBinding>> =
            unsafe { Retained::cast_unchecked(binding) };
        let pointer = binding.bufferPointerType().ok_or_else(|| no("pointer"))?;
        let element = pointer.elementStructType().ok_or_else(|| no("struct"))?;
        let members = element.members();
        let members = members.iter().map(|m| (m.name().to_string(), m.offset()));
        Ok((pointer.dataSize(), members.collect()))
    }
}

/// The one pipeline, with its atlases and the buffer its instances go in.
pub struct Pipeline {
    gpu: Gpu,
    state: State,
    gray: Texture,
    color: Texture,
    instances: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
}

fn region(x: u32, y: u32, w: u32, h: u32) -> MTLRegion {
    MTLRegion {
        origin: MTLOrigin {
            x: x as usize,
            y: y as usize,
            z: 0,
        },
        size: MTLSize {
            width: w as usize,
            height: h as usize,
            depth: 1,
        },
    }
}

impl Pipeline {
    /// The device, which a window's layer draws with.
    pub(super) fn device(&self) -> &ProtocolObject<dyn MTLDevice> {
        &self.gpu.device
    }

    /// Copy pixels into an atlas. It's an error unless the region has pixels, is
    /// wholly inside the atlas, and `data` is exactly its pixels.
    pub fn upload(&self, u: &Upload) -> Result<(), String> {
        let (texture, bytes) = match u.plane {
            Plane::Gray => (&self.gray, 1),
            Plane::Color => (&self.color, 4),
        };
        // In u64, since these come from outside and a u32 sum can wrap into the texture.
        let inside = |at: u32, len: u32, side: usize| {
            len > 0 && u64::from(at) + u64::from(len) <= side as u64
        };
        let fits = inside(u.x, u.w, texture.width()) && inside(u.y, u.h, texture.height());
        let row = u.w as usize * bytes;
        // Past `fits`, both sides are a texture's at most, so the product can't wrap.
        if !fits || u.data.len() != row * u.h as usize {
            return Err(format!("an upload that doesn't fit its atlas: {u:?}"));
        }
        // SAFETY: the region has pixels and is inside the texture, and `data` holds exactly
        // those pixels, all checked above, and Metal copies them before this returns. No
        // frame is reading the texture, since `render` and `present` wait for each one.
        unsafe {
            texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                region(u.x, u.y, u.w, u.h),
                0,
                NonNull::from(&*u.data).cast(),
                row,
            )
        };
        Ok(())
    }

    /// Put `quads` in the instance buffer, growing it if it's too small.
    fn fill_instances(&mut self, quads: &[Quad]) -> Result<(), String> {
        let bytes = size_of_val(quads);
        let have = self.instances.as_ref().map_or(0, |b| b.length());
        if have < bytes {
            let shared = MTLResourceOptions::StorageModeShared;
            let buffer = self
                .gpu
                .device
                .newBufferWithLength_options(bytes * 2, shared);
            self.instances = Some(buffer.ok_or("no instance buffer")?);
        }
        if let Some(b) = &self.instances {
            let to = b.contents().cast::<u8>();
            // SAFETY: the buffer holds at least `bytes`, made sure of above, and nothing
            // reads it now, since `render` and `present` wait for each frame before the
            // next. A new buffer can't overlap `quads`, which Rust owns.
            unsafe {
                std::ptr::copy_nonoverlapping(quads.as_ptr().cast::<u8>(), to.as_ptr(), bytes)
            };
        }
        Ok(())
    }

    /// Clear a new `w` × `h` texture to `clear`, draw `quads` on it, wait for the GPU and
    /// read the pixels back. A size Metal can't make is an error.
    pub fn render(&mut self, quads: &[Quad], clear: Rgb, w: u32, h: u32) -> Result<Image, String> {
        autoreleasepool(|_| self.render_in_pool(quads, clear, w, h))
    }

    fn render_in_pool(
        &mut self,
        quads: &[Quad],
        clear: Rgb,
        w: u32,
        h: u32,
    ) -> Result<Image, String> {
        self.fill_instances(quads)?;
        let usage = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
        let target = self.gpu.texture((w, h), TARGET_FORMAT, usage)?;
        let commands = self.encode(quads, clear, &target, (w, h))?;
        commands.commit();
        finish(&commands)?;
        Ok(read_back(&target, w, h))
    }

    /// Draw `quads` on the surface's next drawable and show it, waiting for the GPU. With
    /// `sync`, it goes on screen with the Core Animation transaction, which a resize needs so
    /// that nothing stretches, and the transaction is flushed, since a second drawable in one
    /// transaction blocks `nextDrawable` for a second (S1). `Ok(None)` means there was no
    /// drawable to draw on.
    pub fn present(
        &mut self,
        quads: &[Quad],
        clear: Rgb,
        surface: &Surface,
        sync: bool,
    ) -> Result<Option<Presented>, String> {
        autoreleasepool(|_| self.present_in_pool(quads, clear, surface, sync))
    }

    fn present_in_pool(
        &mut self,
        quads: &[Quad],
        clear: Rgb,
        surface: &Surface,
        sync: bool,
    ) -> Result<Option<Presented>, String> {
        self.fill_instances(quads)?;
        let Some(drawable) = surface.layer.nextDrawable() else {
            return Ok(None);
        };
        let target = drawable.texture();
        let (w, h) = (target.width() as u32, target.height() as u32);
        let commands = self.encode(quads, clear, &target, (w, h))?;
        surface.layer.setPresentsWithTransaction(sync);
        let show: &ProtocolObject<dyn MTLDrawable> = ProtocolObject::from_ref(&*drawable);
        if !sync {
            commands.presentDrawable(show);
        }
        commands.commit();
        finish(&commands)?;
        if sync {
            show.present();
            CATransaction::flush();
        }
        let image = surface.read_next.take().then(|| read_back(&target, w, h));
        Ok(Some(Presented { w, h, image }))
    }

    /// Encode the frame's draw on `target`: its commands, not yet committed. The instance
    /// buffer already holds `quads`.
    fn encode(
        &self,
        quads: &[Quad],
        clear: Rgb,
        target: &ProtocolObject<dyn MTLTexture>,
        (w, h): (u32, u32),
    ) -> Result<Retained<ProtocolObject<dyn MTLCommandBuffer>>, String> {
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 exists: Metal has at least eight, and this is the first.
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(target));
        attachment.setLoadAction(MTLLoadAction::Clear);
        attachment.setStoreAction(MTLStoreAction::Store);
        let unit = |v: u8| f64::from(v) / 255.0;
        attachment.setClearColor(MTLClearColor {
            red: unit(clear.0),
            green: unit(clear.1),
            blue: unit(clear.2),
            alpha: 1.0,
        });
        let commands = self.gpu.queue.commandBuffer().ok_or("no command buffer")?;
        let encoder = commands
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or("no render encoder")?;
        encoder.setRenderPipelineState(&self.state);
        let size = [w as f32, h as f32];
        // SAFETY: the pointer covers the 8 bytes given, a float2 as the shader takes at
        // buffer 0, and Metal copies them before this returns.
        unsafe {
            encoder.setVertexBytes_length_atIndex(NonNull::from(&size).cast(), 8, 0);
        }
        // SAFETY: the buffer holds `quads`, and the command buffer retains it until it's
        // done. Index 1 is where the shader reads them.
        unsafe {
            encoder.setVertexBuffer_offset_atIndex(self.instances.as_deref(), 0, 1);
        }
        // SAFETY: texture 0 is the shader's gray atlas, an R8 texture this pipeline made.
        unsafe { encoder.setFragmentTexture_atIndex(Some(&self.gray), 0) };
        // SAFETY: texture 1 is the shader's color atlas, a BGRA texture this pipeline made.
        unsafe { encoder.setFragmentTexture_atIndex(Some(&self.color), 1) };
        if !quads.is_empty() {
            // SAFETY: the shader reads `quads[instance]` for each instance, the buffer
            // holds that many, and 4 vertices make a strip's rectangle.
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    quads.len(),
                )
            };
        }
        encoder.endEncoding();
        Ok(commands)
    }
}

/// Wait for the GPU, and say if it failed.
fn finish(commands: &ProtocolObject<dyn MTLCommandBuffer>) -> Result<(), String> {
    commands.waitUntilCompleted();
    if commands.status() != MTLCommandBufferStatus::Completed {
        return Err(format!("the GPU failed: {:?}", commands.error()));
    }
    Ok(())
}

/// A finished target's pixels: the offscreen target's, or a drawable's when the surface
/// asked, which only a readable one can (`Surface::read_next_frame`).
fn read_back(target: &ProtocolObject<dyn MTLTexture>, w: u32, h: u32) -> Image {
    let mut bgra = vec![0u8; w as usize * h as usize * 4];
    // SAFETY: the buffer holds the region's `w` × `h` BGRA pixels at the stride given, and
    // the GPU has finished with the texture. It isn't framebuffer-only: the offscreen target
    // never is, and `Surface::read_next_frame`, the one way to reach this for a drawable,
    // refuses a surface whose layer is framebuffer-only.
    unsafe {
        target.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            NonNull::from(&mut *bgra).cast(),
            w as usize * 4,
            region(0, 0, w, h),
            0,
        )
    };
    Image { w, h, bgra }
}
