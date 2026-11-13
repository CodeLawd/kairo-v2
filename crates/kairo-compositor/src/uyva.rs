//! A finished frame to NDI's UYVA on the GPU, read back without stalling.
//!
//! The conversion is `uyva.wgsl`, byte for byte the CPU reference below
//! (Electron's `native/decklink/uyvy.h`). Read-back uses a ring of
//! [`SLOTS`] staging buffers: a frame whose slot is still in flight, or
//! whose bytes the sender still holds, is dropped instead of queued, as
//! Electron's `gpu-frames` converter dropped a busy frame.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, ComputePassDescriptor, ComputePipeline, ComputePipelineDescriptor, Device,
    MapMode, Queue, ShaderModuleDescriptor, ShaderSource, Texture,
};

/// Read-back slots: a burst beyond this is dropped.
pub const SLOTS: usize = 3;
/// Frame buffers in circulation: the slots plus what a sender may hold
/// (the frame it repeats and one waiting).
pub const MAX_BUFFERS: usize = SLOTS + 2;

/// Bytes of a UYVA frame: the UYVY plane, then one alpha byte a pixel.
pub fn uyva_len(width: u32, height: u32) -> usize {
    width as usize * height as usize * 3
}

fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

fn luma(r: i32, g: i32, b: i32) -> u8 {
    clamp8(((47 * r + 157 * g + 16 * b + 128) >> 8) + 16)
}

/// The CPU reference: `convertFrame` from `uyvy.h` over premultiplied
/// RGBA (not BGRA), nearest-neighbour scaled, with the alpha plane after
/// the UYVY plane when `alpha` is set.
pub fn reference(rgba: &[u8], src: (u32, u32), dst: (u32, u32), alpha: bool) -> Vec<u8> {
    let (sw, sh) = (src.0 as usize, src.1 as usize);
    let (w, h) = (dst.0 as usize, dst.1 as usize);
    let mut out = vec![0u8; w * h * 2 + if alpha { w * h } else { 0 }];
    for y in 0..h {
        let sy = y * sh / h;
        for x in (0..w).step_by(2) {
            let at = |px: usize| {
                let o = (sy * sw + px * sw / w) * 4;
                [
                    i32::from(rgba[o]),
                    i32::from(rgba[o + 1]),
                    i32::from(rgba[o + 2]),
                    i32::from(rgba[o + 3]),
                ]
            };
            let (a, c) = (at(x), at(x + 1));
            let (r, g, b) = ((a[0] + c[0]) / 2, (a[1] + c[1]) / 2, (a[2] + c[2]) / 2);
            let d = (y * w + x) * 2;
            out[d] = clamp8(((-26 * r - 87 * g + 112 * b + 128) >> 8) + 128);
            out[d + 1] = luma(a[0], a[1], a[2]);
            out[d + 2] = clamp8(((112 * r - 102 * g - 10 * b + 128) >> 8) + 128);
            out[d + 3] = luma(c[0], c[1], c[2]);
            if alpha {
                out[w * h * 2 + y * w + x] = a[3] as u8;
                out[w * h * 2 + y * w + x + 1] = c[3] as u8;
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    alpha: u32,
    bgra: u32,
    pad: [u32; 2],
}

struct Slot {
    storage: Buffer,
    staging: Buffer,
    busy: Arc<AtomicBool>,
}

/// Counts, as `gpu-frames` reported them.
#[derive(Debug, Default)]
pub struct Stats {
    pub converted: AtomicU64,
    pub skipped: AtomicU64,
    pub failed: AtomicU64,
}

/// A converted frame. Its bytes go back to the converter's pool when it
/// is dropped, so a sender that holds frames makes the converter drop
/// new ones rather than allocate.
pub struct UyvaFrame {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    /// Premultiplied BGRA, four bytes a pixel, rows of `width × 4`, instead of UYVA.
    pub bgra: bool,
    data: Option<Vec<u8>>,
    pool: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl UyvaFrame {
    pub fn data(&self) -> &[u8] {
        self.data.as_deref().unwrap_or(&[])
    }
}

impl Drop for UyvaFrame {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            self.pool
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(data);
        }
    }
}

pub type Done = Box<dyn FnOnce(Result<UyvaFrame, String>) + Send>;

pub struct UyvaConverter {
    pipeline: ComputePipeline,
    params: Buffer,
    slots: Vec<Slot>,
    pool: Arc<Mutex<Vec<Vec<u8>>>>,
    /// The source texture last converted and its bind group per slot.
    cache: Mutex<Option<(Texture, Vec<BindGroup>)>>,
    /// Frame buffers made so far; never more than [`MAX_BUFFERS`].
    allocated: std::sync::atomic::AtomicUsize,
    width: u32,
    height: u32,
    alpha: bool,
    bgra: bool,
    pub stats: Arc<Stats>,
}

impl UyvaConverter {
    /// A converter to `width` × `height` (even width, a multiple of 4).
    pub fn new(device: &Device, width: u32, height: u32, alpha: bool) -> Result<Self, String> {
        Self::make(device, width, height, alpha, false)
    }

    /// Premultiplied BGRA instead of UYVA, for the recorder's tap.
    pub fn bgra(device: &Device, width: u32, height: u32) -> Result<Self, String> {
        Self::make(device, width, height, true, true)
    }

    fn len(&self) -> usize {
        if self.bgra {
            self.width as usize * self.height as usize * 4
        } else {
            uyva_len(self.width, self.height)
        }
    }

    fn make(
        device: &Device,
        width: u32,
        height: u32,
        alpha: bool,
        bgra: bool,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 || !width.is_multiple_of(4) || width > 8192 || height > 8192 {
            return Err(format!("UYVA frame {width}×{height} is not supported"));
        }
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("uyva"),
            source: ShaderSource::Wgsl(include_str!("uyva.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("uyva conversion"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let params = device.create_buffer(&BufferDescriptor {
            label: Some("uyva params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes = if bgra {
            width as u64 * height as u64 * 4
        } else {
            uyva_len(width, height) as u64
        };
        let slots = (0..SLOTS)
            .map(|_| Slot {
                storage: device.create_buffer(&BufferDescriptor {
                    label: Some("uyva out"),
                    size: bytes,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                staging: device.create_buffer(&BufferDescriptor {
                    label: Some("uyva read-back"),
                    size: bytes,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                busy: Arc::new(AtomicBool::new(false)),
            })
            .collect();
        Ok(Self {
            pipeline,
            params,
            slots,
            pool: Arc::new(Mutex::new(Vec::new())),
            cache: Mutex::new(None),
            allocated: std::sync::atomic::AtomicUsize::new(0),
            width,
            height,
            alpha,
            bgra,
            stats: Arc::new(Stats::default()),
        })
    }

    /// The bind group for `src` and slot `index`, made once per source
    /// texture: the compositor converts the same target every frame.
    fn bind_group(&self, device: &Device, queue: &Queue, src: &Texture, index: usize) -> BindGroup {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.as_ref().is_none_or(|(t, _)| t != src) {
            let size = src.size();
            let params = Params {
                src_w: size.width,
                src_h: size.height,
                dst_w: self.width,
                dst_h: self.height,
                alpha: u32::from(self.alpha),
                bgra: u32::from(self.bgra),
                pad: [0; 2],
            };
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
            let view = src.create_view(&Default::default());
            let layout = self.pipeline.get_bind_group_layout(0);
            let groups = self
                .slots
                .iter()
                .map(|slot| {
                    device.create_bind_group(&BindGroupDescriptor {
                        label: Some("uyva"),
                        layout: &layout,
                        entries: &[
                            BindGroupEntry {
                                binding: 0,
                                resource: BindingResource::TextureView(&view),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: slot.storage.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 2,
                                resource: self.params.as_entire_binding(),
                            },
                        ],
                    })
                })
                .collect();
            *cache = Some((src.clone(), groups));
        }
        cache
            .as_ref()
            .map(|(_, g)| g[index].clone())
            .expect("cached")
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Starts converting `src` (premultiplied RGBA, any size). `done` runs
    /// when the device is next polled after the GPU finishes. Returns
    /// `false`, dropping the frame, when every slot is busy.
    pub fn convert(&self, device: &Device, queue: &Queue, src: &Texture, done: Done) -> bool {
        let Some(slot) = self
            .slots
            .iter()
            .find(|s| !s.busy.swap(true, Ordering::AcqRel))
        else {
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        // The bytes come from the pool; when the sender holds every one,
        // the frame is dropped too.
        let len = self.len();
        let buffer = {
            let mut pool = self.pool.lock().unwrap_or_else(|e| e.into_inner());
            match pool.pop() {
                Some(b) => Some(b),
                None if self.allocated.load(Ordering::Relaxed) < MAX_BUFFERS => {
                    self.allocated.fetch_add(1, Ordering::Relaxed);
                    Some(Vec::with_capacity(len))
                }
                None => None,
            }
        };
        let Some(mut buffer) = buffer else {
            slot.busy.store(false, Ordering::Release);
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        let index = self
            .slots
            .iter()
            .position(|s| std::ptr::eq(s, slot))
            .expect("slot");
        let bind = self.bind_group(device, queue, src, index);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("uyva conversion"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((self.width / 4).div_ceil(16), self.height.div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&slot.storage, 0, &slot.staging, 0, len as u64);
        queue.submit([encoder.finish()]);
        let staging = slot.staging.clone();
        let busy = slot.busy.clone();
        let stats = self.stats.clone();
        let pool = self.pool.clone();
        let (width, height, alpha, bgra) = (self.width, self.height, self.alpha, self.bgra);
        slot.staging.map_async(MapMode::Read, .., move |result| {
            let out = match result {
                Ok(()) => match staging.get_mapped_range(..) {
                    Ok(view) => {
                        buffer.clear();
                        buffer.extend_from_slice(&view[..len]);
                        drop(view);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            };
            staging.unmap();
            busy.store(false, Ordering::Release);
            match out {
                Ok(()) => {
                    stats.converted.fetch_add(1, Ordering::Relaxed);
                    done(Ok(UyvaFrame {
                        width,
                        height,
                        alpha,
                        bgra,
                        data: Some(buffer),
                        pool,
                    }));
                }
                Err(e) => {
                    stats.failed.fetch_add(1, Ordering::Relaxed);
                    pool.lock().unwrap_or_else(|p| p.into_inner()).push(buffer);
                    done(Err(e));
                }
            }
        });
        true
    }
}
