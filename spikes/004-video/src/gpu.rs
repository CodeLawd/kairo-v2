use crate::decode::Frame;
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::time::Duration;
use wgpu::*;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ColorUniform {
    kr: f32,
    kb: f32,
    y_offset: f32,
    y_scale: f32,
    c_offset: f32,
    c_scale: f32,
    shift: u32,
    alpha: u32,
}

pub struct Gpu {
    pub device: Device,
    queue: Queue,
    pipeline: ComputePipeline,
}

pub struct Planes {
    dimensions: (u32, u32, u32, bool),
    y: Texture,
    uv: Texture,
    alpha: Texture,
    rgba: Texture,
    uniform: Buffer,
    bindings: BindGroup,
}

impl Gpu {
    pub async fn open() -> Result<Self> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await?;
        eprintln!("GPU: {:?}", adapter.get_info());
        let (device, queue) = adapter.request_device(&DeviceDescriptor::default()).await?;
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("YUV to straight RGBA"),
            source: ShaderSource::Wgsl(include_str!("video.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("video conversion"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: PipelineCompilationOptions::default(),
            cache: None,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }

    fn allocate(&self, dimensions: (u32, u32, u32, bool)) -> Planes {
        let (w, h, bits, has_alpha) = dimensions;
        let upload = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        let y = self.texture(
            w,
            h,
            if bits == 10 {
                TextureFormat::R16Uint
            } else {
                TextureFormat::R8Uint
            },
            upload,
        );
        let uv = self.texture(
            w.div_ceil(2),
            h.div_ceil(2),
            if bits == 10 {
                TextureFormat::Rg16Uint
            } else {
                TextureFormat::Rg8Uint
            },
            upload,
        );
        let alpha = self.texture(
            if has_alpha { w } else { 1 },
            if has_alpha { h } else { 1 },
            TextureFormat::R8Uint,
            upload,
        );
        let rgba = self.texture(
            w,
            h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::STORAGE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::TEXTURE_BINDING,
        );
        let uniform = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: size_of::<ColorUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let views = [&y, &uv, &alpha, &rgba]
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()));
        let bindings = self.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&views[0]),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&views[1]),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&views[2]),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&views[3]),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Planes {
            dimensions,
            y,
            uv,
            alpha,
            rgba,
            uniform,
            bindings,
        }
    }

    fn texture(&self, w: u32, h: u32, format: TextureFormat, usage: TextureUsages) -> Texture {
        self.device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    pub fn render(&self, frame: &Frame, cached: &mut Option<Planes>) -> Result<()> {
        let (w, h) = (frame.pixels.width(), frame.pixels.height());
        let dimensions = (w, h, frame.bits, frame.alpha.is_some());
        let limit = self.device.limits().max_texture_dimension_2d;
        anyhow::ensure!(
            w > 0 && h > 0 && w <= limit && h <= limit,
            "frame exceeds device limits"
        );
        if cached
            .as_ref()
            .is_none_or(|planes| planes.dimensions != dimensions)
        {
            *cached = Some(self.allocate(dimensions));
        }
        let planes = cached.as_ref().context("video textures unavailable")?;
        for (index, texture) in [&planes.y, &planes.uv].into_iter().enumerate() {
            self.write(
                texture,
                frame.pixels.data(index),
                frame.pixels.stride(index) as u32,
            );
        }
        if let Some(alpha) = &frame.alpha {
            self.write(&planes.alpha, alpha, w);
        }
        let steps = if frame.bits == 10 { 4.0 } else { 1.0 };
        let max_code: f32 = if frame.bits == 10 { 1023.0 } else { 255.0 };
        let uniform = ColorUniform {
            kr: frame.color.kr,
            kb: frame.color.kb,
            y_offset: if frame.color.full { 0.0 } else { 16.0 * steps },
            y_scale: if frame.color.full {
                max_code.recip()
            } else {
                1.0 / (219.0 * steps)
            },
            c_offset: 128.0 * steps,
            c_scale: if frame.color.full {
                max_code.recip()
            } else {
                1.0 / (224.0 * steps)
            },
            shift: if frame.bits == 10 { 6 } else { 0 },
            alpha: u32::from(frame.alpha.is_some()),
        };
        self.queue
            .write_buffer(&planes.uniform, 0, bytemuck::bytes_of(&uniform));
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &planes.bindings, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        let index = self.queue.submit([encoder.finish()]);
        self.wait(Some(index))?;
        Ok(())
    }

    fn write(&self, texture: &Texture, bytes: &[u8], stride: u32) {
        self.queue.write_texture(
            texture.as_image_copy(),
            bytes,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
            texture.size(),
        );
    }

    fn wait(&self, submission_index: Option<SubmissionIndex>) -> Result<()> {
        self.device.poll(PollType::Wait {
            submission_index,
            timeout: Some(Duration::from_secs(10)),
        })?;
        Ok(())
    }

    pub fn read_pixel(&self, planes: &Planes, x: u32, y: u32) -> Result<[u8; 4]> {
        let buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: 256,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        let mut source = planes.rgba.as_image_copy();
        source.origin = Origin3d { x, y, z: 0 };
        encoder.copy_texture_to_buffer(
            source,
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: None,
                    rows_per_image: None,
                },
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let index = self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.wait(Some(index))?;
        receiver.recv_timeout(Duration::from_secs(10))??;
        let mapped = buffer.slice(..).get_mapped_range()?;
        Ok(mapped[..4].try_into()?)
    }
}
