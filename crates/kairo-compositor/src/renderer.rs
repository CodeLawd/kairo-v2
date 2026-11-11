//! Drawing a program frame from its layer stack, and showing it in a window.

use crate::camera::{Mode, VideoLayout, place};
use crate::geometry::{FRAME_WIDTH, Rect, Transform, frame_height, letterbox};
use crate::layers::{Content, Fill, Fit, Raster, Slot, Stack};
use std::collections::HashMap;
use std::time::{Duration, Instant};
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
    /// radius, opacity, mode (0 fill, 1 shadow, 2 gradient), shadow sigma.
    params: [f32; 4],
    /// The visible box, in the quad's own (local) pixels.
    clip: [f32; 4],
    /// Centre of the turn, then its cosine and sine.
    turn: [f32; 4],
    /// Colour filter (hue in radians, saturate, brightness, contrast), or
    /// a gradient's end colour.
    filt: [f32; 4],
    /// Flags (1 filter, 2 flip x, 4 flip y), gradient angle in radians.
    extra: [f32; 4],
}

const FLAG_FILTER: f32 = 1.0;
const FLAG_FLIP_X: f32 = 2.0;
const FLAG_FLIP_Y: f32 = 4.0;

impl Quad {
    fn new(rect: Rect, color: [f32; 4], radius: f32, opacity: f32, mode: f32, sigma: f32) -> Self {
        let r = [rect.x, rect.y, rect.width, rect.height];
        Self {
            rect: r,
            color,
            params: [radius, opacity, mode, sigma],
            clip: r,
            turn: [
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0,
                1.0,
                0.0,
            ],
            filt: [0.0; 4],
            extra: [0.0; 4],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Texel {
    White,
    Image(u64),
    Source(u64),
    /// The shared offscreen texture of this size and nesting depth.
    Temp((u32, u32), u8),
    /// A cached blurred picture.
    Blurred(u64),
}

/// Work to finish before a draw: its picture is drawn (and perhaps blurred)
/// into an offscreen texture first.
struct Pre {
    size: (u32, u32),
    depth: u8,
    draws: Vec<Draw>,
    /// Standard deviation of the Gaussian blur, in texture pixels.
    blur: Option<f32>,
    /// Keep the result under this key instead of the shared texture.
    cache: Option<u64>,
}

struct Draw {
    quad: Quad,
    texel: Texel,
    clip: Option<Rect>,
    pre: Option<Box<Pre>>,
}

impl Draw {
    fn new(quad: Quad, texel: Texel, clip: Option<Rect>) -> Self {
        Self {
            quad,
            texel,
            clip,
            pre: None,
        }
    }
}

/// An offscreen colour texture that can be drawn into and sampled.
struct Offscreen {
    view: TextureView,
    sampled: BindGroup,
    globals: BindGroup,
    _texture: Texture,
}

struct TempPair {
    a: Offscreen,
    b: Offscreen,
    used: u64,
}

struct Blurred {
    surface: Offscreen,
    used: u64,
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

/// What a draw's pixels are, for placing and caching.
struct Picture {
    texel: Texel,
    width: f32,
    height: f32,
    /// Identity for the blur cache: a still raster.
    still: Option<u64>,
}

/// The state a node is drawn under.
#[derive(Clone, Copy)]
struct Ctx {
    frame: Rect,
    transform: Transform,
    clip: Option<Rect>,
    size: (u32, u32),
    depth: u8,
}

/// The widest an offscreen texture may be.
const MAX_OFFSCREEN: u32 = 8192;

pub struct Renderer {
    device: Device,
    queue: Queue,
    info: AdapterInfo,
    globals_layout: BindGroupLayout,
    fill_layout: BindGroupLayout,
    blur_layout: BindGroupLayout,
    pipeline_layout: PipelineLayout,
    shader: ShaderModule,
    pipelines: HashMap<TextureFormat, RenderPipeline>,
    blur_pipeline: RenderPipeline,
    sampler: Sampler,
    white: BindGroup,
    images: HashMap<u64, Cached>,
    sources: HashMap<u64, SourceTexture>,
    temps: HashMap<(u32, u32, u8), TempPair>,
    blurred: HashMap<u64, Blurred>,
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
        let texture_entries = |extra: Option<BindGroupLayoutEntry>| {
            let mut entries = vec![
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
            ];
            entries.extend(extra);
            entries
        };
        let fill_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("fill"),
            entries: &texture_entries(None),
        });
        let blur_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &texture_entries(Some(BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("compositor"),
            bind_group_layouts: &[Some(&globals_layout), Some(&fill_layout)],
            immediate_size: 0,
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("blur"),
            bind_group_layouts: &[None, Some(&blur_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("compositor"),
            source: ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let blur_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("blur"),
            layout: Some(&blur_pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("blur_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("blur_fs"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: FRAME_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
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
            blur_layout,
            pipeline_layout,
            shader,
            pipelines: HashMap::new(),
            blur_pipeline,
            sampler,
            images: HashMap::new(),
            sources: HashMap::new(),
            temps: HashMap::new(),
            blurred: HashMap::new(),
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
                        attributes: &vertex_attr_array![
                            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
                            4 => Float32x4, 5 => Float32x4, 6 => Float32x4
                        ],
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

    fn texture_for(&self, size: (u32, u32), usage: TextureUsages) -> Texture {
        self.device.create_texture(&TextureDescriptor {
            label: Some("frame"),
            size: Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FRAME_FORMAT,
            usage,
            view_formats: &[],
        })
    }

    fn offscreen(&self, size: (u32, u32)) -> Offscreen {
        let texture = self.texture_for(
            size,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let view = texture.create_view(&Default::default());
        Offscreen {
            sampled: bind_view(&self.device, &self.fill_layout, &self.sampler, &view),
            globals: self.globals(size),
            view,
            _texture: texture,
        }
    }

    /// A frame for an output `requested` pixels tall (clamped to 360..=4320).
    pub fn target(&self, requested: u32) -> FrameTarget {
        let height = frame_height(requested);
        let texture = self.texture_for(
            (FRAME_WIDTH, height),
            TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
        );
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

    /// The offscreen pair of this size and depth, made when first wanted.
    fn temp(&mut self, size: (u32, u32), depth: u8) {
        let key = (size.0, size.1, depth);
        if !self.temps.contains_key(&key) {
            let pair = TempPair {
                a: self.offscreen(size),
                b: self.offscreen(size),
                used: self.frames,
            };
            self.temps.insert(key, pair);
        }
        if let Some(pair) = self.temps.get_mut(&key) {
            pair.used = self.frames;
        }
    }

    /// The draws for one frame, bottom to top.
    fn collect(&mut self, scene: &Scene, size: (u32, u32), now: Instant) -> Vec<Draw> {
        let height = size.1;
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
        self.slot(
            &mut draws,
            scene,
            Slot::Background,
            frame,
            size,
            content,
            now,
        );
        if placement.mode != Mode::Pip {
            self.slot(&mut draws, scene, Slot::Camera, frame, size, camera, now);
        }
        for slot in Slot::FORE {
            self.slot(&mut draws, scene, slot, frame, size, content, now);
        }
        if placement.mode == Mode::Pip {
            self.slot(&mut draws, scene, Slot::Camera, frame, size, camera, now);
        }
        draws
    }

    #[allow(clippy::too_many_arguments)]
    fn slot(
        &mut self,
        draws: &mut Vec<Draw>,
        scene: &Scene,
        slot: Slot,
        frame: Rect,
        size: (u32, u32),
        (transform, clip): (Transform, Option<Rect>),
        now: Instant,
    ) {
        let ctx = Ctx {
            frame,
            transform,
            clip,
            size,
            depth: 0,
        };
        for node in scene.stack.nodes(slot) {
            let opacity = node.content.opacity * node.fade(now);
            if opacity <= 0.0 {
                continue;
            }
            self.node(draws, &node.content, ctx, opacity, node.age(now));
        }
    }

    fn node(
        &mut self,
        draws: &mut Vec<Draw>,
        content: &Content,
        ctx: Ctx,
        opacity: f32,
        age: Duration,
    ) {
        let Ctx {
            frame,
            transform,
            clip,
            ..
        } = ctx;
        let rect = transform.apply(content.rect.unwrap_or(frame));
        let radius = content.radius * transform.scale;
        if let Some(shadow) = content.shadow {
            let mut quad = Quad::new(
                Rect::new(
                    rect.x + shadow.offset.0,
                    rect.y + shadow.offset.1,
                    rect.width,
                    rect.height,
                ),
                shadow.color,
                radius,
                opacity,
                1.0,
                shadow.blur / 2.0,
            );
            quad.params[3] = shadow.blur / 2.0;
            draws.push(Draw::new(quad, Texel::White, clip));
        }
        match &content.fill {
            Fill::Empty | Fill::Pending => {}
            Fill::Solid(color) => draws.push(Draw::new(
                Quad::new(rect, *color, radius, opacity, 0.0, 0.0),
                Texel::White,
                clip,
            )),
            Fill::Gradient {
                from,
                to,
                angle_deg,
            } => {
                let mut quad = Quad::new(rect, *from, radius, opacity, 2.0, 0.0);
                quad.filt = *to;
                quad.extra[1] = angle_deg.to_radians();
                draws.push(Draw::new(quad, Texel::White, clip));
            }
            Fill::Image(raster) => {
                self.image(raster);
                let picture = Picture {
                    texel: Texel::Image(raster.id()),
                    width: raster.width as f32,
                    height: raster.height as f32,
                    still: Some(raster.id()),
                };
                self.picture(draws, content, ctx, rect, radius, opacity, picture);
            }
            Fill::Frames(frames) => {
                let at = frames.index_at(age);
                let raster = &frames.rasters[at.min(frames.rasters.len() - 1)];
                self.image(raster);
                let picture = Picture {
                    texel: Texel::Image(raster.id()),
                    width: raster.width as f32,
                    height: raster.height as f32,
                    still: Some(raster.id()),
                };
                self.picture(draws, content, ctx, rect, radius, opacity, picture);
            }
            Fill::Source { id, backdrop } => {
                // The node's own background shows until the first frame.
                match self.sources.get(id).map(|s| s.size) {
                    Some((w, h)) => {
                        let picture = Picture {
                            texel: Texel::Source(*id),
                            width: w as f32,
                            height: h as f32,
                            still: None,
                        };
                        self.picture(draws, content, ctx, rect, radius, opacity, picture);
                    }
                    None => draws.push(Draw::new(
                        Quad::new(rect, *backdrop, radius, opacity, 0.0, 0.0),
                        Texel::White,
                        clip,
                    )),
                }
            }
            Fill::Group(children) => self.group(draws, children, ctx, opacity, age),
        }
    }

    /// A layer of contents faded as one: drawn straight when it is fully
    /// opaque (or a single content), else into an offscreen frame first so
    /// overlapping children do not show through each other.
    fn group(
        &mut self,
        draws: &mut Vec<Draw>,
        children: &[Content],
        ctx: Ctx,
        opacity: f32,
        age: Duration,
    ) {
        if opacity >= 1.0 || children.len() < 2 {
            for child in children {
                let o = child.opacity * opacity;
                if o > 0.0 {
                    self.node(draws, child, ctx, o, age);
                }
            }
            return;
        }
        let inner_ctx = Ctx {
            depth: ctx.depth + 1,
            ..ctx
        };
        let mut inner = Vec::new();
        for child in children {
            if child.opacity > 0.0 {
                self.node(&mut inner, child, inner_ctx, child.opacity, age);
            }
        }
        if inner.is_empty() {
            return;
        }
        self.temp(ctx.size, ctx.depth);
        let rect = Rect::new(0.0, 0.0, ctx.size.0 as f32, ctx.size.1 as f32);
        let mut draw = Draw::new(
            Quad::new(rect, [1.0; 4], 0.0, opacity, 0.0, 0.0),
            Texel::Temp(ctx.size, ctx.depth),
            ctx.clip,
        );
        draw.pre = Some(Box::new(Pre {
            size: ctx.size,
            depth: ctx.depth,
            draws: inner,
            blur: None,
            cache: None,
        }));
        draws.push(draw);
    }

    /// Places a picture in `rect` by the node's look: fit, turn, flips,
    /// colour filter and blur.
    #[allow(clippy::too_many_arguments)]
    fn picture(
        &mut self,
        draws: &mut Vec<Draw>,
        content: &Content,
        ctx: Ctx,
        rect: Rect,
        radius: f32,
        opacity: f32,
        picture: Picture,
    ) {
        let look = content.look;
        if picture.width <= 0.0 || picture.height <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0
        {
            return;
        }
        let dest = fit_rect(rect, picture.width, picture.height, look.fit);
        let centre = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let theta = look.rotation_deg.to_radians();
        let mut flags = 0.0;
        if look.flip_x {
            flags += FLAG_FLIP_X;
        }
        if look.flip_y {
            flags += FLAG_FLIP_Y;
        }
        let filter = if look.filter.is_identity() {
            None
        } else {
            Some([
                look.filter.hue_deg.to_radians(),
                look.filter.saturation,
                look.filter.brightness,
                look.filter.contrast,
            ])
        };
        let finish = |mut quad: Quad, with_filter: bool, flags: f32| {
            quad.clip = [rect.x, rect.y, rect.width, rect.height];
            quad.turn = [centre.0, centre.1, theta.cos(), theta.sin()];
            quad.extra[0] = flags;
            if with_filter && let Some(f) = filter {
                quad.filt = f;
                quad.extra[0] += FLAG_FILTER;
            }
            quad
        };
        let sigma = look.blur_px * ctx.transform.scale;
        if sigma <= 0.0 || !sigma.is_finite() {
            let quad = finish(
                Quad::new(dest, [1.0; 4], radius, opacity, 0.0, 0.0),
                true,
                flags,
            );
            let quad = Quad {
                rect: [dest.x, dest.y, dest.width, dest.height],
                ..quad
            };
            let quad = scale_about(quad, centre, look.scale);
            draws.push(Draw::new(quad, picture.texel, ctx.clip));
            return;
        }
        // Blur: draw the picture into a transparent texture padded by three
        // standard deviations, blur it there, then lay that over the box.
        let margin = (3.0 * sigma).ceil() + 2.0;
        let ox = (dest.x.floor() - margin).max(-4.0 * MAX_OFFSCREEN as f32);
        let oy = (dest.y.floor() - margin).max(-4.0 * MAX_OFFSCREEN as f32);
        let w = ((dest.x + dest.width).ceil() + margin - ox).clamp(1.0, MAX_OFFSCREEN as f32);
        let h = ((dest.y + dest.height).ceil() + margin - oy).clamp(1.0, MAX_OFFSCREEN as f32);
        let size = (w as u32, h as u32);
        let inner = Quad {
            rect: [dest.x - ox, dest.y - oy, dest.width, dest.height],
            clip: [dest.x - ox, dest.y - oy, dest.width, dest.height],
            turn: [0.0, 0.0, 1.0, 0.0],
            ..finish(
                Quad::new(dest, [1.0; 4], 0.0, 1.0, 0.0, 0.0),
                !look.blur_first,
                0.0,
            )
        };
        let inner = Quad {
            clip: inner.rect,
            ..inner
        };
        let depth = ctx.depth;
        self.temp(size, depth);
        // Still pictures keep their blurred result across frames.
        let cache = picture.still.filter(|_| ctx.depth == 0).map(|id| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            id.hash(&mut h);
            size.hash(&mut h);
            sigma.to_bits().hash(&mut h);
            (
                dest.x.to_bits(),
                dest.y.to_bits(),
                dest.width.to_bits(),
                dest.height.to_bits(),
            )
                .hash(&mut h);
            if !look.blur_first {
                filter.map(|f| f.map(f32::to_bits)).hash(&mut h);
            }
            h.finish()
        });
        let mut pre = Some(Box::new(Pre {
            size,
            depth,
            draws: vec![Draw::new(inner, picture.texel, None)],
            blur: Some(sigma),
            cache,
        }));
        let texel = match cache {
            Some(key) => {
                if let Some(entry) = self.blurred.get_mut(&key) {
                    entry.used = self.frames;
                    pre = None;
                } else {
                    let surface = self.offscreen(size);
                    self.blurred.insert(
                        key,
                        Blurred {
                            surface,
                            used: self.frames,
                        },
                    );
                }
                Texel::Blurred(key)
            }
            None => Texel::Temp(size, depth),
        };
        let quad = finish(
            Quad::new(Rect::new(ox, oy, w, h), [1.0; 4], radius, opacity, 0.0, 0.0),
            look.blur_first,
            flags,
        );
        let quad = scale_about(quad, centre, look.scale);
        let mut draw = Draw::new(quad, texel, ctx.clip);
        draw.pre = pre;
        draws.push(draw);
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
        let draws = self.collect(scene, (target.width, target.height), now);
        self.pipeline(FRAME_FORMAT);
        self.encode(
            encoder,
            &target.view,
            FRAME_FORMAT,
            &target.globals,
            (target.width, target.height),
            Some(Color::TRANSPARENT),
            &draws,
            None,
        );
        // Rasters no longer drawn by any output for a while are released.
        let frames = self.frames;
        self.images
            .retain(|_, c| frames.saturating_sub(c.used) < 240);
        self.temps
            .retain(|_, t| frames.saturating_sub(t.used) < 240);
        self.blurred
            .retain(|_, b| frames.saturating_sub(b.used) < 240);
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
        let draw = Draw::new(
            Quad::new(rect, [1.0; 4], 0.0, 1.0, 0.0, 0.0),
            Texel::White,
            None,
        );
        self.encode(
            encoder,
            view,
            format,
            &globals,
            size,
            Some(Color::BLACK),
            &[draw],
            Some(&target.sampled),
        );
    }

    /// Records `draws` into `view`. A draw with offscreen work first ends
    /// the pass, does that work, and the pass resumes loading what it drew.
    #[allow(clippy::too_many_arguments)]
    fn encode(
        &self,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        format: TextureFormat,
        globals: &BindGroup,
        size: (u32, u32),
        clear: Option<Color>,
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
        let mut load = clear.map_or(LoadOp::Load, LoadOp::Clear);
        let mut pass: Option<RenderPass<'static>> = None;
        for (index, draw) in draws.iter().enumerate() {
            let Some((x, y, w, h)) = scissor(draw.clip, size) else {
                continue;
            };
            if let Some(pre) = &draw.pre {
                // The pass cannot stay open: its target is not this one.
                if let Some(open) = pass.take() {
                    drop(open);
                    load = LoadOp::Load;
                }
                self.run_pre(encoder, pre);
            }
            if pass.is_none() {
                pass = Some(
                    self.begin_compose(encoder, view, format, globals, buffer.as_ref(), load)
                        .forget_lifetime(),
                );
                load = LoadOp::Load;
            }
            let open = pass.as_mut().expect("opened above");
            let bind = match (override_texel, draw.texel) {
                (Some(bind), _) => bind,
                (None, Texel::White) => &self.white,
                (None, Texel::Image(id)) => &self.images[&id].bind,
                (None, Texel::Source(id)) => &self.sources[&id].bind,
                (None, Texel::Temp(size, depth)) => &self.temps[&(size.0, size.1, depth)].a.sampled,
                (None, Texel::Blurred(key)) => &self.blurred[&key].surface.sampled,
            };
            open.set_scissor_rect(x, y, w, h);
            open.set_bind_group(1, bind, &[]);
            let i = index as u32;
            open.draw(0..6, i..i + 1);
        }
        if pass.is_none() && matches!(load, LoadOp::Clear(_)) {
            // Nothing to draw, but the target must still be cleared.
            drop(self.begin_compose(encoder, view, format, globals, buffer.as_ref(), load));
        }
    }

    fn begin_compose<'e>(
        &self,
        encoder: &'e mut CommandEncoder,
        view: &TextureView,
        format: TextureFormat,
        globals: &BindGroup,
        buffer: Option<&Buffer>,
        load: LoadOp<Color>,
    ) -> RenderPass<'e> {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("compose"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(buffer) = buffer {
            pass.set_pipeline(&self.pipelines[&format]);
            pass.set_bind_group(0, globals, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
        }
        pass
    }

    /// Draws a pre's picture into its offscreen texture and blurs it.
    fn run_pre(&self, encoder: &mut CommandEncoder, pre: &Pre) {
        let pair = &self.temps[&(pre.size.0, pre.size.1, pre.depth)];
        let target = match pre.cache {
            Some(key) => &self.blurred[&key].surface,
            None => &pair.a,
        };
        self.encode(
            encoder,
            &target.view,
            FRAME_FORMAT,
            &target.globals,
            pre.size,
            Some(Color::TRANSPARENT),
            &pre.draws,
            None,
        );
        let Some(sigma) = pre.blur else { return };
        let radius = (3.0 * sigma).ceil().max(1.0);
        // Horizontal into the scratch texture, vertical back.
        self.blur_pass(
            encoder,
            target,
            &pair.b,
            (1.0 / pre.size.0 as f32, 0.0),
            sigma,
            radius,
        );
        self.blur_pass(
            encoder,
            &pair.b,
            target,
            (0.0, 1.0 / pre.size.1 as f32),
            sigma,
            radius,
        );
    }

    fn blur_pass(
        &self,
        encoder: &mut CommandEncoder,
        from: &Offscreen,
        to: &Offscreen,
        step: (f32, f32),
        sigma: f32,
        radius: f32,
    ) {
        let params = [step.0, step.1, sigma, radius];
        let buffer = self.device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("blur"),
            contents: bytemuck::cast_slice(&params),
            usage: BufferUsages::UNIFORM,
        });
        let bind = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("blur"),
            layout: &self.blur_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&from.view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("blur"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &to.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blur_pipeline);
        pass.set_bind_group(1, &bind, &[]);
        pass.draw(0..3, 0..1);
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
    Draw::new(
        Quad::new(rect, color, 0.0, opacity, 0.0, 0.0),
        Texel::White,
        clip,
    )
}

/// Where a picture of `width` × `height` lands in `rect` under `object-fit`,
/// centred (`object-position: 50% 50%`).
fn fit_rect(rect: Rect, width: f32, height: f32, fit: Fit) -> Rect {
    let scale = match fit {
        Fit::Fill => return rect,
        Fit::Contain => (rect.width / width).min(rect.height / height),
        Fit::Cover => (rect.width / width).max(rect.height / height),
    };
    let (w, h) = (width * scale, height * scale);
    Rect::new(
        rect.x + (rect.width - w) / 2.0,
        rect.y + (rect.height - h) / 2.0,
        w,
        h,
    )
}

/// `transform: scale(s)` about `centre` on a quad (its own box, not its clip).
fn scale_about(mut quad: Quad, centre: (f32, f32), s: f32) -> Quad {
    if s == 1.0 || !s.is_finite() || s <= 0.0 {
        return quad;
    }
    quad.rect = [
        centre.0 + (quad.rect[0] - centre.0) * s,
        centre.1 + (quad.rect[1] - centre.1) * s,
        quad.rect[2] * s,
        quad.rect[3] * s,
    ];
    quad
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
