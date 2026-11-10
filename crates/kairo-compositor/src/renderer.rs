//! Drawing a program frame from its layer stack, and showing it in a window.

use crate::camera::{Mode, VideoLayout, place};
use crate::geometry::{FRAME_WIDTH, Rect, Transform, frame_height, letterbox};
use crate::layers::{Content, Fill, Raster, Slot, Stack};
use std::collections::HashMap;
use std::time::Instant;
use wgpu::util::DeviceExt;
use wgpu::*;

/// Every program frame is stored in this format: premultiplied, and blended
/// as stored rather than linearised, as Chromium does.
pub const FRAME_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// One output's layers and shell state (`#pa-program`).
pub struct Scene {
    pub stack: Stack,
    /// `program.style.background`: opaque black for documents.
    pub program_background: Option<[f32; 4]>,
    /// `config.video`, normalised once.
    pub layout: VideoLayout,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            stack: Stack::default(),
            program_background: None,
            layout: VideoLayout::default_layout(),
        }
    }
}

impl Scene {
    /// `applyCameraNodeStyle` for the current camera node.
    pub fn restyle_camera(&mut self, frame_height: u32) {
        let live = self.stack.current(Slot::Camera).is_some();
        let placement = place(&self.layout, live, frame_height);
        if let Some(node) = self.stack.current_mut(Slot::Camera) {
            node.content.rect = Some(placement.camera);
            node.content.radius = placement.radius;
            node.content.shadow = placement.shadow.then_some(crate::layers::PIP_SHADOW);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Quad {
    rect: [f32; 4],
    color: [f32; 4],
    /// radius, opacity, mode, sigma.
    params: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Texel {
    White,
    Image(u64),
    Source(u64),
}

struct Draw {
    quad: Quad,
    texel: Texel,
    clip: Option<Rect>,
}

/// One output's frame: 1920 × frame height, transparent where nothing draws.
pub struct FrameTarget {
    pub width: u32,
    pub height: u32,
    texture: Texture,
    view: TextureView,
    sampled: BindGroup,
    globals: BindGroup,
}

impl FrameTarget {
    pub fn texture(&self) -> &Texture {
        &self.texture
    }
}

struct Cached {
    _texture: Texture,
    bind: BindGroup,
    used: u64,
}

struct SourceTexture {
    texture: Texture,
    bind: BindGroup,
    size: (u32, u32),
}

pub struct Renderer {
    device: Device,
    queue: Queue,
    info: AdapterInfo,
    globals_layout: BindGroupLayout,
    fill_layout: BindGroupLayout,
    pipeline_layout: PipelineLayout,
    shader: ShaderModule,
    pipelines: HashMap<TextureFormat, RenderPipeline>,
    sampler: Sampler,
    white: BindGroup,
    images: HashMap<u64, Cached>,
    sources: HashMap<u64, SourceTexture>,
    frames: u64,
}

impl Renderer {
    /// A device for tests and tools. `software` asks for the fallback
    /// adapter (WARP, lavapipe) and takes any adapter when there is none.
    pub fn headless(software: bool) -> Result<Self, String> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::LowPower,
            force_fallback_adapter: software,
            ..Default::default()
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
        })
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        Self::new(&adapter)
    }

    pub fn new(adapter: &Adapter) -> Result<Self, String> {
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("kairo-compositor"),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU device: {e}"))?;
        let globals_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let fill_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("fill"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("compositor"),
            bind_group_layouts: &[Some(&globals_layout), Some(&fill_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("compositor"),
            source: ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("fill"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let mut renderer = Self {
            info: adapter.get_info(),
            white: {
                // Placeholder until replaced just below; bind groups need a texture.
                let texture = solid_texture(&device, &queue, [255, 255, 255, 255]);
                bind_texture(&device, &fill_layout, &sampler, &texture)
            },
            device,
            queue,
            globals_layout,
            fill_layout,
            pipeline_layout,
            shader,
            pipelines: HashMap::new(),
            sampler,
            images: HashMap::new(),
            sources: HashMap::new(),
            frames: 0,
        };
        renderer.pipeline(FRAME_FORMAT);
        Ok(renderer)
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn adapter_info(&self) -> &AdapterInfo {
        &self.info
    }

    fn pipeline(&mut self, format: TextureFormat) -> &RenderPipeline {
        let (device, layout, shader) = (&self.device, &self.pipeline_layout, &self.shader);
        self.pipelines.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("compositor"),
                layout: Some(layout),
                vertex: VertexState {
                    module: shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(VertexBufferLayout {
                        array_stride: size_of::<Quad>() as u64,
                        step_mode: VertexStepMode::Instance,
                        attributes: &vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
                    })],
                },
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                fragment: Some(FragmentState {
                    module: shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(ColorTargetState {
                        format,
                        blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    fn globals(&self, size: (u32, u32)) -> BindGroup {
        let data = [size.0 as f32, size.1 as f32, 0.0, 0.0];
        let buffer = self.device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::cast_slice(&data),
            usage: BufferUsages::UNIFORM,
        });
        self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("globals"),
            layout: &self.globals_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    }

    /// A frame for an output `requested` pixels tall (clamped to 360..=4320).
    pub fn target(&self, requested: u32) -> FrameTarget {
        let height = frame_height(requested);
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("frame"),
            size: Extent3d {
                width: FRAME_WIDTH,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampled = bind_view(&self.device, &self.fill_layout, &self.sampler, &view);
        FrameTarget {
            width: FRAME_WIDTH,
            height,
            globals: self.globals((FRAME_WIDTH, height)),
            texture,
            view,
            sampled,
        }
    }

    /// Hands a live source (video, camera) its newest frame.
    pub fn update_source(&mut self, id: u64, frame: &Raster) {
        let size = (frame.width, frame.height);
        let stale = self.sources.get(&id).is_none_or(|s| s.size != size);
        if stale {
            let texture = self.device.create_texture(&TextureDescriptor {
                label: Some("source"),
                size: Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: FRAME_FORMAT,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let bind = bind_texture(&self.device, &self.fill_layout, &self.sampler, &texture);
            self.sources.insert(
                id,
                SourceTexture {
                    texture,
                    bind,
                    size,
                },
            );
        }
        let source = &self.sources[&id];
        write_raster(&self.queue, &source.texture, frame);
    }

    pub fn remove_source(&mut self, id: u64) {
        self.sources.remove(&id);
    }

    fn image(&mut self, raster: &Raster) {
        let frames = self.frames;
        if let Some(cached) = self.images.get_mut(&raster.id()) {
            cached.used = frames;
            return;
        }
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("raster"),
            size: Extent3d {
                width: raster.width.max(1),
                height: raster.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if raster.width > 0 && raster.height > 0 {
            write_raster(&self.queue, &texture, raster);
        }
        let bind = bind_texture(&self.device, &self.fill_layout, &self.sampler, &texture);
        self.images.insert(
            raster.id(),
            Cached {
                _texture: texture,
                bind,
                used: frames,
            },
        );
    }

    /// The draws for one frame, bottom to top.
    fn collect(&mut self, scene: &Scene, height: u32, now: Instant) -> Vec<Draw> {
        let frame = Rect::new(0.0, 0.0, FRAME_WIDTH as f32, height as f32);
        let mut draws = Vec::new();
        if let Some(color) = scene.program_background {
            draws.push(solid(frame, color, 1.0, None));
        }
        let live = scene.stack.current(Slot::Camera).is_some();
        let placement = place(&scene.layout, live, height);
        if placement.backdrop {
            draws.push(solid(frame, [0.0, 0.0, 0.0, 1.0], 1.0, None));
        }
        // `.pa-slot` and `#pa-content-*` clip to their (transformed) box.
        let content = (placement.content, Some(placement.content.apply(frame)));
        let camera = (Transform::IDENTITY, Some(frame));
        self.slot(&mut draws, scene, Slot::Background, frame, content, now);
        if placement.mode != Mode::Pip {
            self.slot(&mut draws, scene, Slot::Camera, frame, camera, now);
        }
        for slot in Slot::FORE {
            self.slot(&mut draws, scene, slot, frame, content, now);
        }
        if placement.mode == Mode::Pip {
            self.slot(&mut draws, scene, Slot::Camera, frame, camera, now);
        }
        draws
    }

    fn slot(
        &mut self,
        draws: &mut Vec<Draw>,
        scene: &Scene,
        slot: Slot,
        frame: Rect,
        (transform, clip): (Transform, Option<Rect>),
        now: Instant,
    ) {
        for node in scene.stack.nodes(slot) {
            let opacity = node.content.opacity * node.fade(now);
            if opacity <= 0.0 {
                continue;
            }
            self.node(draws, &node.content, frame, transform, clip, opacity);
        }
    }

    fn node(
        &mut self,
        draws: &mut Vec<Draw>,
        content: &Content,
        frame: Rect,
        transform: Transform,
        clip: Option<Rect>,
        opacity: f32,
    ) {
        let rect = transform.apply(content.rect.unwrap_or(frame));
        let radius = content.radius * transform.scale;
        if let Some(shadow) = content.shadow {
            draws.push(Draw {
                quad: Quad {
                    rect: [
                        rect.x + shadow.offset.0,
                        rect.y + shadow.offset.1,
                        rect.width,
                        rect.height,
                    ],
                    color: shadow.color,
                    params: [radius, opacity, 1.0, shadow.blur / 2.0],
                },
                texel: Texel::White,
                clip,
            });
        }
        match &content.fill {
            Fill::Empty => {}
            Fill::Solid(color) => draws.push(Draw {
                quad: Quad {
                    rect: [rect.x, rect.y, rect.width, rect.height],
                    color: *color,
                    params: [radius, opacity, 0.0, 0.0],
                },
                texel: Texel::White,
                clip,
            }),
            Fill::Image(raster) => {
                self.image(raster);
                draws.push(Draw {
                    quad: Quad {
                        rect: [rect.x, rect.y, rect.width, rect.height],
                        color: [1.0; 4],
                        params: [radius, opacity, 0.0, 0.0],
                    },
                    texel: Texel::Image(raster.id()),
                    clip,
                });
            }
            Fill::Source { id, backdrop } => {
                // The node's own background shows until the first frame.
                let (color, texel) = if self.sources.contains_key(id) {
                    ([1.0; 4], Texel::Source(*id))
                } else {
                    (*backdrop, Texel::White)
                };
                draws.push(Draw {
                    quad: Quad {
                        rect: [rect.x, rect.y, rect.width, rect.height],
                        color,
                        params: [radius, opacity, 0.0, 0.0],
                    },
                    texel,
                    clip,
                });
            }
        }
    }

    /// Draws `scene` into `target` as it stands at `now`.
    pub fn draw_frame(
        &mut self,
        encoder: &mut CommandEncoder,
        scene: &Scene,
        target: &FrameTarget,
        now: Instant,
    ) {
        self.frames += 1;
        let draws = self.collect(scene, target.height, now);
        self.pipeline(FRAME_FORMAT);
        self.encode(
            encoder,
            &target.view,
            FRAME_FORMAT,
            &target.globals,
            (target.width, target.height),
            Color::TRANSPARENT,
            &draws,
        );
        // Rasters no longer drawn by any output for a while are released.
        let frames = self.frames;
        self.images
            .retain(|_, c| frames.saturating_sub(c.used) < 240);
    }

    /// `fitToWindow`: `target` letterboxed on black into a window's frame.
    pub fn present(
        &mut self,
        encoder: &mut CommandEncoder,
        target: &FrameTarget,
        view: &TextureView,
        format: TextureFormat,
        size: (u32, u32),
    ) {
        let rect = letterbox(size, (target.width, target.height));
        let globals = self.globals(size);
        self.pipeline(format);
        let draw = Draw {
            quad: Quad {
                rect: [rect.x, rect.y, rect.width, rect.height],
                color: [1.0; 4],
                params: [0.0, 1.0, 0.0, 0.0],
            },
            texel: Texel::White,
            clip: None,
        };
        self.encode_with(
            encoder,
            view,
            format,
            &globals,
            size,
            Color::BLACK,
            &[draw],
            Some(&target.sampled),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn encode(
        &self,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        format: TextureFormat,
        globals: &BindGroup,
        size: (u32, u32),
        clear: Color,
        draws: &[Draw],
    ) {
        self.encode_with(encoder, view, format, globals, size, clear, draws, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_with(
        &self,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        format: TextureFormat,
        globals: &BindGroup,
        size: (u32, u32),
        clear: Color,
        draws: &[Draw],
        override_texel: Option<&BindGroup>,
    ) {
        let quads: Vec<Quad> = draws.iter().map(|d| d.quad).collect();
        let buffer = (!quads.is_empty()).then(|| {
            self.device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("instances"),
                contents: bytemuck::cast_slice(&quads),
                usage: BufferUsages::VERTEX,
            })
        });
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("compose"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(clear),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(buffer) = buffer else { return };
        pass.set_pipeline(&self.pipelines[&format]);
        pass.set_bind_group(0, globals, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (index, draw) in draws.iter().enumerate() {
            let bind = match (override_texel, draw.texel) {
                (Some(bind), _) => bind,
                (None, Texel::White) => &self.white,
                (None, Texel::Image(id)) => &self.images[&id].bind,
                (None, Texel::Source(id)) => &self.sources[&id].bind,
            };
            let Some((x, y, w, h)) = scissor(draw.clip, size) else {
                continue;
            };
            pass.set_scissor_rect(x, y, w, h);
            pass.set_bind_group(1, bind, &[]);
            let i = index as u32;
            pass.draw(0..6, i..i + 1);
        }
    }

    /// Copies `target` back as tightly packed premultiplied RGBA rows.
    pub fn read_frame(&self, target: &FrameTarget) -> Vec<u8> {
        self.read_texture(&target.texture)
    }

    /// Copies a 4-byte-per-pixel texture back as tightly packed rows, in
    /// the texture's own channel order.
    pub fn read_texture(&self, texture: &Texture) -> Vec<u8> {
        let (width, height) = (texture.width(), texture.height());
        let row = width * 4;
        let padded = row.div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT) * COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(height),
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(MapMode::Read, |_| {});
        self.device
            .poll(PollType::wait_indefinitely())
            .expect("readback poll");
        let mapped = slice.get_mapped_range().expect("mapped readback");
        let mut out = Vec::with_capacity((row * height) as usize);
        for line in mapped.chunks(padded as usize) {
            out.extend_from_slice(&line[..row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        out
    }
}

fn solid(rect: Rect, color: [f32; 4], opacity: f32, clip: Option<Rect>) -> Draw {
    Draw {
        quad: Quad {
            rect: [rect.x, rect.y, rect.width, rect.height],
            color,
            params: [0.0, opacity, 0.0, 0.0],
        },
        texel: Texel::White,
        clip,
    }
}

/// The clip as a scissor inside the target, `None` when nothing is left.
fn scissor(clip: Option<Rect>, size: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
    let Some(clip) = clip else {
        return Some((0, 0, size.0, size.1));
    };
    let x0 = clip.x.floor().clamp(0.0, size.0 as f32) as u32;
    let y0 = clip.y.floor().clamp(0.0, size.1 as f32) as u32;
    let x1 = (clip.x + clip.width).ceil().clamp(0.0, size.0 as f32) as u32;
    let y1 = (clip.y + clip.height).ceil().clamp(0.0, size.1 as f32) as u32;
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

fn solid_texture(device: &Device, queue: &Queue, rgba: [u8; 4]) -> Texture {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("white"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: FRAME_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &rgba,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn write_raster(queue: &Queue, texture: &Texture, raster: &Raster) {
    queue.write_texture(
        texture.as_image_copy(),
        &raster.pixels,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(raster.width * 4),
            rows_per_image: None,
        },
        Extent3d {
            width: raster.width,
            height: raster.height,
            depth_or_array_layers: 1,
        },
    );
}

fn bind_texture(
    device: &Device,
    layout: &BindGroupLayout,
    sampler: &Sampler,
    texture: &Texture,
) -> BindGroup {
    bind_view(
        device,
        layout,
        sampler,
        &texture.create_view(&Default::default()),
    )
}

fn bind_view(
    device: &Device,
    layout: &BindGroupLayout,
    sampler: &Sampler,
    view: &TextureView,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("fill"),
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
        ],
    })
}
