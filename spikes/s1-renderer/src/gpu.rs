//! Metal renderer: one MSL pipeline drawing instanced quads (cell backgrounds,
//! atlas glyphs, decorations, the pixel-buffer placement, the cursor), a
//! Core Text glyph atlas, per-row instance caches, and a frame log fed by
//! completion and presented handlers.

use crate::font::{FontSet, Raster};
use crate::scene::{attr, CursorShape, Scene, Underline, Wide, BG};
use crate::sprite;
use crate::stats::{now_s, thread_cpu_s};
use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub type Tex = Retained<ProtocolObject<dyn MTLTexture>>;
pub type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
pub struct SendBuf(pub Buf);
unsafe impl Send for SendBuf {}
unsafe impl Sync for SendBuf {}

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Quad { float4 rect; float4 uv; uint color; uint flags; uint2 pad; };
struct VOut { float4 pos [[position]]; float2 uv; float4 color; uint flags [[flat]]; };
vertex VOut vs(uint vid [[vertex_id]], uint iid [[instance_id]], constant float2& vp [[buffer(0)]], const device Quad* q [[buffer(1)]]) {
  Quad k = q[iid]; float2 c = float2(vid & 1, vid >> 1);
  float2 px = k.rect.xy + c * k.rect.zw; VOut o;
  o.pos = float4(px / vp * float2(2, -2) + float2(-1, 1), 0, 1);
  o.uv = k.uv.xy + c * k.uv.zw;
  uint col = k.color; o.color = float4((col >> 24) & 255, (col >> 16) & 255, (col >> 8) & 255, col & 255) / 255.0;
  o.flags = k.flags; return o; }
fragment float4 fs(VOut in [[stage_in]], texture2d<float> gray [[texture(0)]], texture2d<float> color [[texture(1)]], texture2d<float> img [[texture(2)]]) {
  constexpr sampler s(coord::pixel, filter::nearest);
  if (in.flags == 0) { return float4(in.color.rgb * in.color.a, in.color.a); }
  if (in.flags == 1) { float a = gray.sample(s, in.uv).r * in.color.a; return float4(in.color.rgb * a, a); }
  if (in.flags == 2) { return color.sample(s, in.uv); }
  return float4(img.sample(s, in.uv).rgb, 1.0); }
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Quad { pub rect: [f32; 4], pub uv: [f32; 4], pub color: u32, pub flags: u32, pub pad: [u32; 2] } // 48 bytes, as MSL lays it out

pub struct Atlas { pub tex: Tex, w: u32, h: u32, bpp: u32, x: u32, y: u32, row_h: u32 }

impl Atlas {
    fn new(dev: &ProtocolObject<dyn MTLDevice>, w: u32, h: u32, fmt: MTLPixelFormat, bpp: u32) -> Atlas {
        Atlas { tex: new_texture(dev, w, h, fmt, MTLTextureUsage::ShaderRead, MTLStorageMode::Shared), w, h, bpp, x: 0, y: 0, row_h: 0 }
    }
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.x + w + 1 > self.w { self.x = 0; self.y += self.row_h + 1; self.row_h = 0; }
        if self.y + h + 1 > self.h || w + 1 > self.w { return None; }
        let p = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(p)
    }
    fn upload(&self, x: u32, y: u32, r: &Raster) {
        let region = MTLRegion { origin: MTLOrigin { x: x as usize, y: y as usize, z: 0 }, size: MTLSize { width: r.w as usize, height: r.h as usize, depth: 1 } };
        unsafe { self.tex.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, NonNull::new(r.data.as_ptr() as *mut _).unwrap(), (r.w * self.bpp) as usize) };
    }
    fn reset(&mut self) { self.x = 0; self.y = 0; self.row_h = 0; }
}

pub fn new_texture(dev: &ProtocolObject<dyn MTLDevice>, w: u32, h: u32, fmt: MTLPixelFormat, usage: MTLTextureUsage, storage: MTLStorageMode) -> Tex {
    unsafe {
        let d = MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(fmt, w.max(1) as usize, h.max(1) as usize, false);
        d.setUsage(usage);
        d.setStorageMode(storage);
        dev.newTextureWithDescriptor(&d).expect("texture")
    }
}

#[derive(Clone, Copy)]
struct Slot { u: f32, v: f32, w: f32, h: f32, left: f32, top: f32, color: bool }

/// One rendered frame. `presented` is the drawable's presentedTime once known
/// (0 = not yet, -1 = dropped by the compositor).
#[derive(Clone, Default, Debug)]
pub struct FrameRec {
    pub id: u64, pub t0: f64, pub t_commit: f64, pub cpu_ms: f64, pub wall_ms: f64, pub acquire_ms: f64, pub gpu_ms: f64,
    pub presented: f64, pub completed: bool, pub drawable: (u32, u32), pub expected: (u32, u32),
    pub keys: Vec<(u64, f64, f64)>, pub pix_frame: u64, pub blit_bytes: u64, pub tag: &'static str, pub quads: usize,
}

#[derive(Default)]
pub struct FrameLog { pub frames: Vec<FrameRec> }
impl FrameLog {
    pub fn get(&mut self, id: u64) -> Option<&mut FrameRec> {
        let base = self.frames.first()?.id;
        self.frames.get_mut(id.checked_sub(base)? as usize)
    }
}

#[derive(Clone, Copy)]
pub struct RenderOpts { pub sprites: bool, pub braille_sprite: bool }

pub struct PixBlit { pub buf: Arc<SendBuf>, pub rect: [u32; 4], pub frame: u64, pub release: Box<dyn FnOnce() + Send> }

pub enum Target<'a> { Layer(&'a CAMetalLayer), Texture(&'a Tex) }

pub struct Gpu {
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub font: FontSet,
    pub opts: RenderOpts,
    gray: Atlas,
    color: Atlas,
    glyphs: HashMap<(char, u8, u8), Option<Slot>>,
    row_bg: Vec<Vec<Quad>>,
    row_fg: Vec<Vec<Quad>>,
    ibufs: Vec<Buf>,
    next_id: u64,
    pub in_flight: Arc<AtomicUsize>,
    pub log: Arc<Mutex<FrameLog>>,
    dummy: Tex,
    pub pix_tex: Option<Tex>,
    pub pix_rect: Option<[f32; 4]>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
    pub atlas_gen: u32,
    pub scale: f64,
    pub pt: f64,
    pub startup: Vec<(&'static str, f64)>,
}

pub const MAX_IN_FLIGHT: usize = 3;

impl Gpu {
    pub fn new(files: &[PathBuf; 4], pt: f64, scale: f64, opts: RenderOpts, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Gpu, String> {
        let mut startup = vec![];
        let t = now_s();
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let queue = device.newCommandQueue().ok_or("no queue")?;
        startup.push(("metal_device_ms", (now_s() - t) * 1e3));
        let t = now_s();
        let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).map_err(|e| format!("MSL: {e:?}"))?;
        let d = MTLRenderPipelineDescriptor::new();
        d.setVertexFunction(Some(&lib.newFunctionWithName(&NSString::from_str("vs")).unwrap()));
        d.setFragmentFunction(Some(&lib.newFunctionWithName(&NSString::from_str("fs")).unwrap()));
        let ca = unsafe { d.colorAttachments().objectAtIndexedSubscript(0) };
        ca.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        ca.setBlendingEnabled(true);
        ca.setSourceRGBBlendFactor(MTLBlendFactor::One);
        ca.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        ca.setSourceAlphaBlendFactor(MTLBlendFactor::One);
        ca.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        let pipeline = device.newRenderPipelineStateWithDescriptor_error(&d).map_err(|e| format!("pipeline: {e:?}"))?;
        startup.push(("msl_compile_ms", (now_s() - t) * 1e3));
        let t = now_s();
        let font = FontSet::new(files, pt, scale)?;
        startup.push(("font_load_ms", (now_s() - t) * 1e3));
        let gray = Atlas::new(&device, 2048, 2048, MTLPixelFormat::R8Unorm, 1);
        let color = Atlas::new(&device, 1024, 1024, MTLPixelFormat::BGRA8Unorm, 4);
        let dummy = new_texture(&device, 1, 1, MTLPixelFormat::RGBA8Unorm, MTLTextureUsage::ShaderRead, MTLStorageMode::Private);
        Ok(Gpu { device, queue, pipeline, font, opts, gray, color, glyphs: HashMap::new(), row_bg: vec![], row_fg: vec![], ibufs: vec![],
                 next_id: 1, in_flight: Arc::new(AtomicUsize::new(0)), log: Arc::new(Mutex::new(FrameLog::default())), dummy,
                 pix_tex: None, pix_rect: None, wake, atlas_gen: 1, scale, pt, startup })
    }

    pub fn cell(&self) -> (u32, u32) { (self.font.metrics.cell_w, self.font.metrics.cell_h) }

    /// Backing-scale change: new font size in pixels, new metrics, empty atlas.
    pub fn set_scale(&mut self, scale: f64) -> Result<(), String> {
        let files = self.font.files.clone();
        self.font = FontSet::new(&files, self.pt, scale)?;
        self.scale = scale;
        self.clear_atlas();
        Ok(())
    }

    /// Drops every cached glyph; rows are rebuilt by the next render.
    pub fn clear_atlas(&mut self) {
        self.glyphs.clear();
        self.gray.reset();
        self.color.reset();
        self.atlas_gen += 1;
    }

    fn use_sprite(&self, ch: char) -> bool {
        self.opts.sprites && sprite::is_sprite(ch) && (!(0x2800..=0x28FF).contains(&(ch as u32)) || self.opts.braille_sprite)
    }

    fn glyph(&mut self, ch: char, style: u8, span: u8) -> Option<Slot> {
        let key = (ch, style & 3, span);
        if let Some(s) = self.glyphs.get(&key) { return *s; }
        let m = self.font.metrics;
        let thin = ((m.ul_thick).round() as u32).max(1);
        let cell_raster = |data: Vec<u8>| Raster { w: m.cell_w, h: m.cell_h, left: 0, top: 0, color: false, data };
        let raster = if (sprite::UL_BASE + 1..=sprite::UL_BASE + 5).contains(&(ch as u32)) || ch == sprite::STRIKE {
            let pos = (m.baseline - m.ul_pos - m.ul_thick / 2.0).round() as i32;
            sprite::decoration(ch, m.cell_w, m.cell_h, pos.min(m.cell_h as i32 - 2 * thin as i32), thin).map(cell_raster)
        } else if self.use_sprite(ch) {
            sprite::draw(ch, m.cell_w, m.cell_h, thin).map(cell_raster)
        } else {
            let (face, g) = self.font.lookup(ch, style);
            self.font.rasterize(face, g, span as u32)
        };
        let slot = raster.and_then(|r| {
            let mut pos = if r.color { self.color.alloc(r.w, r.h) } else { self.gray.alloc(r.w, r.h) };
            if pos.is_none() { self.clear_atlas(); pos = if r.color { self.color.alloc(r.w, r.h) } else { self.gray.alloc(r.w, r.h) }; }
            let (x, y) = pos?;
            if r.color { self.color.upload(x, y, &r) } else { self.gray.upload(x, y, &r) }
            Some(Slot { u: x as f32, v: y as f32, w: r.w as f32, h: r.h as f32, left: r.left as f32, top: r.top as f32, color: r.color })
        });
        self.glyphs.insert(key, slot);
        slot
    }

    fn build_row(&mut self, scene: &Scene, r: usize) {
        let (cw, chh) = (self.font.metrics.cell_w as f32, self.font.metrics.cell_h as f32);
        let mut bg = std::mem::take(&mut self.row_bg[r]);
        let mut fg = std::mem::take(&mut self.row_fg[r]);
        bg.clear();
        fg.clear();
        let row = scene.grid.row(r).to_vec();
        let y = r as f32 * chh;
        let mut run: Option<(usize, u32)> = None;
        for (c, cell) in row.iter().enumerate() {
            let (mut fgc, mut bgc) = (cell.fg, cell.bg);
            if cell.attrs & attr::INVERSE != 0 { std::mem::swap(&mut fgc, &mut bgc); }
            // backgrounds: one quad per run of equal colour (default bg is the clear colour)
            match run {
                Some((_, col)) if col == bgc => {}
                _ => {
                    if let Some((s, col)) = run { if col != BG { bg.push(Quad { rect: [s as f32 * cw, y, (c - s) as f32 * cw, chh], uv: [0.; 4], color: col, flags: 0, pad: [0; 2] }); } }
                    run = Some((c, bgc));
                }
            }
            if cell.wide == Wide::Cont { continue; }
            let x = c as f32 * cw;
            let span = if cell.wide == Wide::Wide { 2 } else { 1 };
            let mut push = |gpu: &mut Gpu, ch: char, style: u8, span: u8, color: u32, x: f32| {
                if let Some(s) = gpu.glyph(ch, style, span) {
                    fg.push(Quad { rect: [x + s.left, y + s.top, s.w, s.h], uv: [s.u, s.v, s.w, s.h], color, flags: if s.color { 2 } else { 1 }, pad: [0; 2] });
                }
            };
            if cell.ch != ' ' { push(self, cell.ch, cell.attrs & (attr::BOLD | attr::ITALIC), span, fgc, x); }
            if cell.ul != Underline::None {
                let d = char::from_u32(sprite::UL_BASE + cell.ul as u32).unwrap();
                let col = if cell.ul_color != 0 { cell.ul_color } else { fgc };
                for k in 0..span { push(self, d, 0, 1, col, x + k as f32 * cw); }
            }
            if cell.attrs & attr::STRIKE != 0 { for k in 0..span { push(self, sprite::STRIKE, 0, 1, fgc, x + k as f32 * cw); } }
        }
        if let Some((s, col)) = run { if col != BG { bg.push(Quad { rect: [s as f32 * cw, y, (row.len() - s) as f32 * cw, chh], uv: [0.; 4], color: col, flags: 0, pad: [0; 2] }); } }
        self.row_bg[r] = bg;
        self.row_fg[r] = fg;
    }

    /// Encodes and commits one frame. Returns the frame id, or None if no
    /// drawable was available. Never blocks except inside nextDrawable.
    pub fn render(&mut self, scene: &mut Scene, target: Target, blit: Option<PixBlit>, keys: Vec<(u64, f64, f64)>, tag: &'static str) -> Option<u64> {
        autoreleasepool(|_| self.render_inner(scene, target, blit, keys, tag))
    }

    fn render_inner(&mut self, scene: &mut Scene, target: Target, blit: Option<PixBlit>, keys: Vec<(u64, f64, f64)>, tag: &'static str) -> Option<u64> {
        let (t0, c0) = (now_s(), thread_cpu_s());
        let rows = scene.grid.rows;
        if self.row_bg.len() != rows { self.row_bg = vec![vec![]; rows]; self.row_fg = vec![vec![]; rows]; scene.grid.mark_all(); }
        let gen = self.atlas_gen;
        for r in 0..rows { if scene.grid.dirty[r] { self.build_row(scene, r); scene.grid.dirty[r] = false; } }
        if self.atlas_gen != gen { scene.grid.mark_all(); return self.render_inner(scene, target, blit, keys, tag); }
        let (cw, chh) = (self.font.metrics.cell_w as f32, self.font.metrics.cell_h as f32);
        let mut quads: Vec<Quad> = Vec::with_capacity(self.row_bg.iter().chain(self.row_fg.iter()).map(|v| v.len()).sum::<usize>() + 2);
        self.row_bg.iter().for_each(|v| quads.extend_from_slice(v));
        if let (Some(r), Some(_)) = (self.pix_rect, &self.pix_tex) { quads.push(Quad { rect: r, uv: [0., 0., r[2], r[3]], color: 0, flags: 3, pad: [0; 2] }); }
        self.row_fg.iter().for_each(|v| quads.extend_from_slice(v));
        if let Some((r, c, shape)) = scene.cursor {
            let (x, y) = (c as f32 * cw, r as f32 * chh);
            let rect = match shape { CursorShape::Block => [x, y, cw, chh], CursorShape::Bar => [x, y, (self.scale as f32 * 1.5).max(1.0), chh], CursorShape::Underline => [x, y + chh - 2.0 * self.scale as f32, cw, 2.0 * self.scale as f32] };
            quads.push(Quad { rect, uv: [0.; 4], color: 0xffcc00ff, flags: 0, pad: [0; 2] });
        }
        let id = self.next_id;
        let bytes = std::mem::size_of_val(quads.as_slice()).max(64);
        let slot = (id as usize) % MAX_IN_FLIGHT;
        if self.ibufs.len() < MAX_IN_FLIGHT { while self.ibufs.len() < MAX_IN_FLIGHT { self.ibufs.push(self.device.newBufferWithLength_options(bytes * 2, MTLResourceOptions::StorageModeShared).unwrap()); } }
        if self.ibufs[slot].length() < bytes { self.ibufs[slot] = self.device.newBufferWithLength_options(bytes * 2, MTLResourceOptions::StorageModeShared).unwrap(); }
        unsafe { std::ptr::copy_nonoverlapping(quads.as_ptr() as *const u8, self.ibufs[slot].contents().as_ptr() as *mut u8, std::mem::size_of_val(quads.as_slice())) };

        let ta = now_s();
        let (tex, drawable): (Tex, Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>) = match target {
            Target::Layer(layer) => { let d = layer.nextDrawable()?; (d.texture(), Some(d)) }
            Target::Texture(t) => (t.clone(), None),
        };
        let acquire_ms = (now_s() - ta) * 1e3;
        let (tw, th) = (tex.width() as u32, tex.height() as u32);
        let cb = self.queue.commandBuffer().unwrap();
        let mut release: Option<Box<dyn FnOnce() + Send>> = None;
        let (mut pix_frame, mut blit_bytes) = (0, 0);
        if let (Some(b), Some(pt)) = (blit, &self.pix_tex) {
            let be = cb.blitCommandEncoder().unwrap();
            let [x, y, w, h] = b.rect;
            unsafe { be.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &b.buf.0, 0, (w * 4) as usize, (w * h * 4) as usize, MTLSize { width: w as usize, height: h as usize, depth: 1 }, pt, 0, 0, MTLOrigin { x: x as usize, y: y as usize, z: 0 }) };
            be.endEncoding();
            release = Some(b.release);
            pix_frame = b.frame;
            blit_bytes = (w * h * 4) as u64;
        }
        let rpd = MTLRenderPassDescriptor::new();
        let ca = unsafe { rpd.colorAttachments().objectAtIndexedSubscript(0) };
        ca.setTexture(Some(&tex));
        ca.setLoadAction(MTLLoadAction::Clear);
        ca.setStoreAction(MTLStoreAction::Store);
        let c = |s: u32| ((BG >> s) & 255) as f64 / 255.0;
        ca.setClearColor(MTLClearColor { red: c(24), green: c(16), blue: c(8), alpha: 1.0 });
        let enc = cb.renderCommandEncoderWithDescriptor(&rpd).unwrap();
        enc.setRenderPipelineState(&self.pipeline);
        let vp = [tw as f32, th as f32];
        unsafe {
            enc.setVertexBytes_length_atIndex(NonNull::new(vp.as_ptr() as *mut _).unwrap(), 8, 0);
            enc.setVertexBuffer_offset_atIndex(Some(&self.ibufs[slot]), 0, 1);
            enc.setFragmentTexture_atIndex(Some(&self.gray.tex), 0);
            enc.setFragmentTexture_atIndex(Some(&self.color.tex), 1);
            enc.setFragmentTexture_atIndex(Some(self.pix_tex.as_ref().unwrap_or(&self.dummy)), 2);
            if !quads.is_empty() { enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, quads.len()); }
        }
        enc.endEncoding();

        self.next_id += 1;
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        {
            let (log, inf, wake) = (self.log.clone(), self.in_flight.clone(), self.wake.clone());
            let release = Mutex::new(release);
            let blk = RcBlock::new(move |cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let cb = unsafe { cb.as_ref() };
                let gpu_ms = (cb.GPUEndTime() - cb.GPUStartTime()) * 1e3;
                if let Some(f) = log.lock().unwrap().get(id) { f.gpu_ms = gpu_ms; f.completed = true; }
                if let Some(r) = release.lock().unwrap().take() { r(); }
                inf.fetch_sub(1, Ordering::SeqCst);
                wake();
            });
            unsafe { cb.addCompletedHandler(RcBlock::as_ptr(&blk)) };
        }
        let (dw, dh) = (tw, th);
        if let Some(d) = &drawable {
            let log = self.log.clone();
            let blk = RcBlock::new(move |dr: NonNull<ProtocolObject<dyn MTLDrawable>>| {
                let t = unsafe { dr.as_ref() }.presentedTime();
                if let Some(f) = log.lock().unwrap().get(id) { f.presented = if t > 0.0 { t } else { -1.0 }; }
            });
            unsafe { d.addPresentedHandler(RcBlock::as_ptr(&blk)) };
        }
        let mut rec = FrameRec { id, t0, acquire_ms, drawable: (dw, dh), keys, pix_frame, blit_bytes, tag, quads: quads.len(), ..Default::default() };
        // push before commit so handlers always find the record
        self.log.lock().unwrap().frames.push(rec.clone());
        match (&drawable, target) {
            (Some(d), Target::Layer(layer)) if layer.presentsWithTransaction() => { cb.commit(); cb.waitUntilScheduled(); d.present(); }
            (Some(d), _) => { cb.presentDrawable(ProtocolObject::from_ref(&**d)); cb.commit(); }
            (None, _) => { cb.commit(); cb.waitUntilCompleted(); }
        }
        rec.t_commit = now_s();
        rec.cpu_ms = (thread_cpu_s() - c0) * 1e3;
        rec.wall_ms = (rec.t_commit - t0) * 1e3;
        if let Some(f) = self.log.lock().unwrap().get(id) { f.t_commit = rec.t_commit; f.cpu_ms = rec.cpu_ms; f.wall_ms = rec.wall_ms; }
        Some(id)
    }

    /// Experiment (--pix-layer): present the pixel buffer on its own
    /// CAMetalLayer so producer frames never queue in front of text frames.
    pub fn render_pix_layer(&mut self, layer: &CAMetalLayer, blit: PixBlit) -> Option<u64> {
        autoreleasepool(|_| {
            let (t0, c0) = (now_s(), thread_cpu_s());
            let pt = self.pix_tex.clone()?;
            let d = match layer.nextDrawable() { Some(d) => d, None => { (blit.release)(); return None; } };
            let tex = d.texture();
            let (tw, th) = (tex.width() as u32, tex.height() as u32);
            let q = [Quad { rect: [0.0, 0.0, tw as f32, th as f32], uv: [0.0, 0.0, tw as f32, th as f32], color: 0, flags: 3, pad: [0; 2] }];
            let cb = self.queue.commandBuffer().unwrap();
            let be = cb.blitCommandEncoder().unwrap();
            let [x, y, w, h] = blit.rect;
            unsafe { be.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &blit.buf.0, 0, (w * 4) as usize, (w * h * 4) as usize, MTLSize { width: w as usize, height: h as usize, depth: 1 }, &pt, 0, 0, MTLOrigin { x: x as usize, y: y as usize, z: 0 }) };
            be.endEncoding();
            let rpd = MTLRenderPassDescriptor::new();
            let ca = unsafe { rpd.colorAttachments().objectAtIndexedSubscript(0) };
            ca.setTexture(Some(&tex));
            ca.setLoadAction(MTLLoadAction::DontCare);
            ca.setStoreAction(MTLStoreAction::Store);
            let enc = cb.renderCommandEncoderWithDescriptor(&rpd).unwrap();
            enc.setRenderPipelineState(&self.pipeline);
            let vp = [tw as f32, th as f32];
            unsafe {
                enc.setVertexBytes_length_atIndex(NonNull::new(vp.as_ptr() as *mut _).unwrap(), 8, 0);
                enc.setVertexBytes_length_atIndex(NonNull::new(q.as_ptr() as *mut _).unwrap(), std::mem::size_of_val(&q), 1);
                enc.setFragmentTexture_atIndex(Some(&self.gray.tex), 0);
                enc.setFragmentTexture_atIndex(Some(&self.color.tex), 1);
                enc.setFragmentTexture_atIndex(Some(&pt), 2);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, 1);
            }
            enc.endEncoding();
            let id = self.next_id;
            self.next_id += 1;
            self.in_flight.fetch_add(1, Ordering::SeqCst);
            let (log, inf, wake) = (self.log.clone(), self.in_flight.clone(), self.wake.clone());
            let release = Mutex::new(Some(blit.release));
            let blk = RcBlock::new(move |cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let cb = unsafe { cb.as_ref() };
                if let Some(f) = log.lock().unwrap().get(id) { f.gpu_ms = (cb.GPUEndTime() - cb.GPUStartTime()) * 1e3; f.completed = true; }
                if let Some(r) = release.lock().unwrap().take() { r(); }
                inf.fetch_sub(1, Ordering::SeqCst);
                wake();
            });
            unsafe { cb.addCompletedHandler(RcBlock::as_ptr(&blk)) };
            let log = self.log.clone();
            let pblk = RcBlock::new(move |dr: NonNull<ProtocolObject<dyn MTLDrawable>>| {
                let t = unsafe { dr.as_ref() }.presentedTime();
                if let Some(f) = log.lock().unwrap().get(id) { f.presented = if t > 0.0 { t } else { -1.0 }; }
            });
            unsafe { d.addPresentedHandler(RcBlock::as_ptr(&pblk)) };
            self.log.lock().unwrap().frames.push(FrameRec { id, t0, drawable: (tw, th), expected: (tw, th), pix_frame: blit.frame, blit_bytes: (w * h * 4) as u64, tag: "pix", quads: 1, ..Default::default() });
            cb.presentDrawable(ProtocolObject::from_ref(&*d));
            cb.commit();
            let t1 = now_s();
            if let Some(f) = self.log.lock().unwrap().get(id) { f.t_commit = t1; f.cpu_ms = (thread_cpu_s() - c0) * 1e3; f.wall_ms = (t1 - t0) * 1e3; }
            Some(id)
        })
    }

    pub fn new_pix_tex(&mut self, w: u32, h: u32) {
        self.pix_tex = Some(new_texture(&self.device, w, h, MTLPixelFormat::RGBA8Unorm, MTLTextureUsage::ShaderRead, MTLStorageMode::Private));
    }
}
