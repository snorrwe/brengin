//! Performs instanced rendering of spritesheets.
//!
//! Entities are grouped together by their meshes and spritesheets.
//!
//! For arbitrary meshes, it's assumed that the mesh fills a 1by1 AABB.
//! This fact is used by the visibility calculation
//!
//! TODO: support arbitrary sized meshes in visibility
//!
use image::DynamicImage;
use std::collections::{BTreeMap, HashMap};
use tracing::trace;

use cecs::prelude::*;
use glam::Vec2;
use wgpu::{include_wgsl, util::DeviceExt};

use crate::{
    assets::{AssetId, Assets, AssetsPlugin, Handle, WeakHandle},
    camera::ViewFrustum,
    transform::GlobalTransform,
    GameWorld, Plugin, Stage,
};

use super::{
    texture::{texture_bind_group_layout, texture_to_bindings, Texture},
    Extract, ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput,
    RenderCommandPlugin, RenderPass, Vertex,
};

pub fn sprite_sheet_bundle(
    handle: Handle<SpriteSheet>,
    instance: impl Into<Option<SpriteInstance>>,
) -> impl Bundle {
    (
        instance.into().unwrap_or(SpriteInstance {
            index: 0,
            flip: false,
        }),
        Visible,
        handle,
        SpriteInstanceRaw::default(),
    )
}

struct CullSize(pub f32);
struct Visible;

fn signed_dist_plane_point(plane: &glam::Vec4, pos: glam::Vec3) -> f32 {
    plane.dot(pos.extend(1.0))
}

fn is_visible(pos: glam::Vec3, planes: &[glam::Vec4], radius: f32) -> bool {
    for plane in planes {
        let d = signed_dist_plane_point(plane, pos);
        if d < -radius {
            return false;
        }
    }
    true
}

fn update_visible(
    mut cmd: Commands,
    cameras: Query<&ViewFrustum>,
    visible: Query<(EntityId, &CullSize, &GlobalTransform), With<Visible>>,
) {
    for (id, size, tr) in visible.iter() {
        if cameras
            .iter()
            .all(|fr| !is_visible(tr.0.pos, &fr.planes, size.0))
        {
            cmd.entity(id).remove::<Visible>();
        }
    }
}

fn update_invisible(
    mut cmd: Commands,
    cameras: Query<&ViewFrustum>,
    invisible: Query<(EntityId, &CullSize, &GlobalTransform), WithOut<Visible>>,
) {
    for (id, size, tr) in invisible.iter() {
        for fr in cameras.iter() {
            if is_visible(tr.0.pos, &fr.planes, size.0) {
                cmd.entity(id).insert(Visible);
                break;
            }
        }
    }
}

fn insert_missing_cull(
    q: Query<(EntityId, &Handle<SpriteSheet>), WithOut<CullSize>>,
    assets: Res<Assets<SpriteSheet>>,
    mut cmd: Commands,
) {
    for (id, handle) in q.iter() {
        let sheet = assets.get(handle);
        cmd.entity(id)
            .insert(CullSize(sheet.box_size.x.max(sheet.box_size.y)));
    }
}

pub struct SpriteSheet {
    /// Padding applied to the box
    pub padding: Vec2,
    /// Size of the entire box
    pub box_size: Vec2,
    /// Number of boxes in a row
    pub num_cols: u32,
    pub image: DynamicImage,
    /// Size of the entire sheet
    pub size: Vec2,
}

impl SpriteSheet {
    pub fn from_image(padding: Vec2, box_size: Vec2, num_cols: u32, image: DynamicImage) -> Self {
        Self {
            padding,
            box_size,
            num_cols,
            size: Vec2::new(image.width() as f32, image.height() as f32),
            image,
        }
    }

    fn extract(&self) -> SpriteSheetGpu {
        SpriteSheetGpu {
            padding: self.padding.to_array(),
            box_size: self.box_size.to_array(),
            num_cols: self.num_cols,
            size: self.size.to_array(),
            _pad: 0xDEADBEEF,
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct SpriteInstance {
    pub index: u32,
    pub flip: bool,
}

pub fn add_missing_sheets(
    mut pipeline: ResMut<SpritePipeline>,
    renderer: Res<GraphicsState>,
    game_world: Res<GameWorld>,
) {
    game_world
        .world()
        .run_view_system(|sheets: Res<crate::assets::Assets<SpriteSheet>>| {
            for (id, sheet) in sheets.iter() {
                if !pipeline.sheets.contains_key(&id) {
                    pipeline.add_sheet(id, sheet, &renderer);
                }
            }
        });
}

fn unload_sheets(
    mut handles: ResMut<RenderSpritesheetHandles>,
    mut pipeline: ResMut<SpritePipeline>,
    mut instances: ResMut<SpritePipelineInstances>,
) {
    let unloaded = handles
        .0
        .iter()
        .filter(|(_, h)| h.upgrade().is_none())
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in unloaded {
        pipeline.unload_sheet(id);
        instances.0.retain(|k, _| k.sprite_sheet != id);
        handles.0.remove(&id);
    }
}

fn compute_sprite_instances(
    mut q: Query<
        (
            &crate::transform::GlobalTransform,
            &SpriteInstance,
            &mut SpriteInstanceRaw,
        ),
        With<Visible>,
    >,
) {
    q.par_for_each_mut(|(tr, i, instance)| {
        let pos = tr.0.pos;
        let scale = tr.0.scale;
        *instance = SpriteInstanceRaw {
            index: i.index,
            pos_scale: [pos.x, pos.y, pos.z, scale.x],
            scale_y: scale.y,
            flip: i.flip as u32,
        };
    });
}

#[derive(Default)]
struct SpritePipelineInstances(BTreeMap<InstanceKey, Vec<SpriteInstanceRaw>>);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct InstanceKey {
    pub sprite_sheet: AssetId,
    pub mesh: MeshKey,
}

fn clear_pipeline_instances(mut instances: ResMut<SpritePipelineInstances>) {
    for i in instances.0.values_mut() {
        i.clear();
    }
}

impl Extract for SpriteInstanceRaw {
    type QueryItem = (
        &'static Handle<SpriteSheet>,
        &'static SpriteInstanceRaw,
        Option<&'static Handle<SpriteMesh>>,
    );

    type Filter = With<Visible>;

    type Out = (
        WeakHandle<SpriteSheet>,
        SpriteInstanceRaw,
        Visible,
        MeshHandle,
    );

    fn extract<'a>(
        (handle, instance, mesh): <Self::QueryItem as cecs::query::QueryFragment>::Item<'a>,
    ) -> Option<Self::Out> {
        Some((
            handle.downgrade(),
            *instance,
            Visible,
            mesh.map(|h| MeshHandle::Mesh(h.downgrade()))
                .unwrap_or_default(),
        ))
    }
}

fn update_sprite_pipelines(
    renderer: Res<GraphicsState>,
    q: Query<(&WeakHandle<SpriteSheet>, &SpriteInstanceRaw, &MeshHandle)>,
    mut pipeline: ResMut<SpritePipeline>,
    mut instances: ResMut<SpritePipelineInstances>,
) {
    for (handle, raw, mesh) in q.iter() {
        let k = InstanceKey {
            sprite_sheet: handle.id(),
            mesh: mesh.into(),
        };
        instances.0.entry(k).or_default().push(*raw);
    }

    for (id, cpu) in instances.0.iter() {
        let Some(instances) = pipeline.instances.get_mut(id) else {
            continue;
        };

        let instance_data_bytes = bytemuck::cast_slice::<_, u8>(&cpu);
        let size = instance_data_bytes.len() as u64;
        if instances.instance_gpu.size() < size {
            // resize the buffer
            instances.instance_gpu = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!(
                    "Sprite Instance Buffer - {} {:?}",
                    id.sprite_sheet, id.mesh
                )),
                size: size * 2,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        renderer
            .queue
            .write_buffer(&instances.instance_gpu, 0, bytemuck::cast_slice(&cpu));
        instances.count = cpu.len();
    }
}

#[derive(Default)]
struct RenderSpritesheetHandles(pub HashMap<AssetId, WeakHandle<SpriteSheet>>);

// per spritesheet
pub struct SpriteRenderingData {
    pub spritesheet_gpu: wgpu::BindGroup,
    pub spritesheet_bind_group: wgpu::BindGroup,
    pub texture: Texture,
}

struct SpriteInstances {
    pub count: usize,
    pub instance_gpu: wgpu::Buffer,
}

pub struct SpritePipeline {
    instances: HashMap<InstanceKey, SpriteInstances>,
    sheets: HashMap<AssetId, SpriteRenderingData>,
    meshes: BTreeMap<MeshKey, SpriteMeshGpu>,
    // shared
    render_pipeline: wgpu::RenderPipeline,
    sprite_sheet_layout: wgpu::BindGroupLayout,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteSheetGpu {
    pub padding: [f32; 2],
    pub box_size: [f32; 2],
    pub size: [f32; 2],
    pub num_cols: u32,
    pub _pad: u32,
}

impl SpritePipeline {
    pub fn unload_sheet(&mut self, id: AssetId) {
        self.sheets.remove(&id);
    }

    pub fn add_sheet(&mut self, id: AssetId, sheet: &SpriteSheet, renderer: &GraphicsState) {
        let texture = Texture::from_image(renderer.device(), renderer.queue(), &sheet.image, None)
            .expect("Failed to create texture");

        let (_, spritesheet_bind_group) = texture_to_bindings(&renderer.device, &texture);
        let sheet_gpu = sheet.extract();

        let spritesheet_buffer =
            renderer
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("SpriteSheet Instance Buffer {}", id)),
                    usage: wgpu::BufferUsages::UNIFORM,
                    contents: bytemuck::cast_slice(&[sheet_gpu]),
                });

        let spritesheet_gpu = renderer
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.sprite_sheet_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: spritesheet_buffer.as_entire_binding(),
                }],
                label: Some(&format!("spritesheet_bind_group {}", id)),
            });

        self.sheets.insert(
            id,
            SpriteRenderingData {
                spritesheet_gpu,
                spritesheet_bind_group,
                texture,
            },
        );
    }

    pub fn new(renderer: &GraphicsState) -> Self {
        let sprite_sheet_layout: wgpu::BindGroupLayout =
            renderer
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Sprite Sheet Uniform Layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::all(),
                        count: None,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                    }],
                });
        let shader = renderer
            .device
            .create_shader_module(include_wgsl!("sprite-shader.wgsl"));

        let texture_bind_group_layout =
            texture_bind_group_layout(&renderer.device, "sprite-texture-layout");

        let render_pipeline_layout =
            renderer
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Sprite Render Pipeline Layout"),
                    bind_group_layouts: &[
                        &renderer.camera_bind_group_layout,
                        &texture_bind_group_layout,
                        &sprite_sheet_layout,
                    ],
                    ..Default::default()
                });
        let render_pipeline =
            renderer
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Sprite Render Pipeline"),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[Vertex::desc(), SpriteInstanceRaw::desc()],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: renderer.config.format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                        polygon_mode: wgpu::PolygonMode::Fill,
                        // Requires Features::DEPTH_CLIP_CONTROL
                        unclipped_depth: false,
                        // Requires Features::CONSERVATIVE_RASTERIZATION
                        conservative: false,
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: super::texture::Texture::DEPTH_FORMAT,
                        depth_write_enabled: true,
                        depth_compare: wgpu::CompareFunction::Less,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0,
                        alpha_to_coverage_enabled: true,
                    },
                    multiview_mask: None,
                    cache: None,
                });

        let vertex_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sprite Vertex Buffer"),
                contents: bytemuck::cast_slice(SQUARE_VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let index_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sprite Index Buffer"),
                contents: bytemuck::cast_slice(SQUARE_INDICES),
                usage: wgpu::BufferUsages::INDEX,
            });
        let num_indices = SQUARE_INDICES.len() as u32;

        let mut meshes: BTreeMap<MeshKey, SpriteMeshGpu> = Default::default();
        meshes.insert(
            MeshKey::DefaultSquare,
            SpriteMeshGpu {
                vertex_buffer,
                index_buffer,
                num_indices,
            },
        );

        SpritePipeline {
            sheets: Default::default(),
            meshes,
            sprite_sheet_layout,
            render_pipeline,
            instances: Default::default(),
        }
    }

    pub fn render(
        &self,
        RenderCommandInput {
            render_pass,
            camera,
        }: &mut RenderCommandInput,
    ) {
        render_pass.set_pipeline(&self.render_pipeline);
        for (k, instances) in self.instances.iter().filter(|(_, s)| s.count > 0) {
            let Some(mesh) = self.meshes.get(&k.mesh) else {
                continue;
            };
            let Some(sheet) = self.sheets.get(&k.sprite_sheet) else {
                continue;
            };
            trace!(mesh=?k.mesh, sprite_sheet=k.sprite_sheet, "Rendering {} instances", instances.count);

            render_pass.set_bind_group(0, *camera, &[]);
            render_pass.set_bind_group(1, &sheet.spritesheet_bind_group, &[]);
            render_pass.set_bind_group(2, &sheet.spritesheet_gpu, &[]);
            render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, instances.instance_gpu.slice(..));
            render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.draw_indexed(0..mesh.num_indices, 0, 0..instances.count as u32);
        }
    }
}

struct SpriteRenderCommand;

impl<'a> RenderCommand<'a> for SpriteRenderCommand {
    type Parameters = Res<'a, SpritePipeline>;

    fn render<'r>(input: &'r mut RenderCommandInput<'a, 'r>, pipeline: &'r Self::Parameters) {
        pipeline.render(input)
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteInstanceRaw {
    pos_scale: [f32; 4],
    scale_y: f32,
    index: u32,
    /// bool
    flip: u32,
}

impl SpriteInstanceRaw {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        const ROW_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 4]>() as wgpu::BufferAddress;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<SpriteInstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: ROW_SIZE,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: ROW_SIZE + 4,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: ROW_SIZE + 4 + 4,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

const SQUARE_VERTICES: &[Vertex] = &[
    // A
    Vertex {
        pos: [-0.5, 0.5, 0.0],
        uv: [0.0, 1.0],
    },
    // B
    Vertex {
        pos: [-0.5, -0.5, 0.0],
        uv: [0.0, 0.0],
    },
    // C
    Vertex {
        pos: [0.5, -0.5, 0.0],
        uv: [1.0, 0.0],
    },
    // D
    Vertex {
        pos: [0.5, 0.5, 0.0],
        uv: [1.0, 1.0],
    },
];

const SQUARE_INDICES: &[u16] = &[3, 2, 1, 3, 1, 0];

fn setup(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let sprite_pipeline = SpritePipeline::new(&graphics_state);
    cmd.insert_resource(sprite_pipeline);
}

pub struct SpriteRendererPlugin;

impl Plugin for SpriteRendererPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(AssetsPlugin::<SpriteSheet>::default());
        app.add_plugin(AssetsPlugin::<SpriteMesh>::default());
        app.add_plugin(ExtractionPlugin::<SpriteInstanceRaw>::default());
        app.with_stage(Stage::Update, |s| {
            // putting this system in update means that the last frame's data will be presented
            s.add_system(compute_sprite_instances)
                .add_system(insert_missing_cull)
                .add_system(update_visible)
                .add_system(update_invisible);
        });

        app.add_plugin(RenderCommandPlugin::<SpriteRenderCommand>::new(
            RenderPass::Transparent,
        ));
        app.extract_stage.add_system(add_missing_sheets);
        app.extract_stage.add_system(add_missing_meshes);
        app.extract_stage.add_system(add_missing_instances);

        if let Some(ref mut app) = app.render_app {
            app.add_startup_system(setup);
            app.insert_resource(SpritePipelineInstances::default());
            app.insert_resource(RenderSpritesheetHandles::default());
            app.insert_resource(SpriteMeshHandles::default());
            app.with_stage(Stage::PreUpdate, |s| {
                s.add_system(clear_pipeline_instances);
                s.add_system(add_missing_instances);
            });
            app.with_stage(Stage::Update, |s| {
                s.add_system(unload_sheets)
                    .add_system(update_sprite_pipelines)
                    .add_system(unload_meshes);
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpriteMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

struct SpriteMeshGpu {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
}

#[derive(Default)]
struct SpriteMeshHandles(pub BTreeMap<AssetId, WeakHandle<SpriteMesh>>);

fn unload_meshes(mut handles: ResMut<SpriteMeshHandles>) {
    handles.0.retain(|_, h| h.upgrade().is_some());
}

#[derive(Debug, Default)]
enum MeshHandle {
    Mesh(WeakHandle<SpriteMesh>),
    #[default]
    DefaultSquare,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum MeshKey {
    Mesh(AssetId),
    #[default]
    DefaultSquare,
}

impl From<MeshHandle> for MeshKey {
    fn from(value: MeshHandle) -> Self {
        MeshKey::from(&value)
    }
}

impl<'a> From<&'a MeshHandle> for MeshKey {
    fn from(value: &'a MeshHandle) -> Self {
        match value {
            MeshHandle::Mesh(weak_handle) => MeshKey::Mesh(weak_handle.id()),
            MeshHandle::DefaultSquare => MeshKey::DefaultSquare,
        }
    }
}

fn add_missing_meshes(
    renderer: Res<GraphicsState>,
    game_world: Res<GameWorld>,
    mut pipeline: ResMut<SpritePipeline>,
) {
    game_world
        .world()
        .run_view_system(|meshes: Res<crate::assets::Assets<SpriteMesh>>| {
            for (id, sheet) in meshes.iter() {
                let key = MeshKey::Mesh(id);
                if !pipeline.meshes.contains_key(&key) {
                    let vertex_buffer =
                        renderer
                            .device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Sprite Vertex Buffer"),
                                contents: bytemuck::cast_slice(sheet.vertices.as_slice()),
                                usage: wgpu::BufferUsages::VERTEX,
                            });

                    let index_buffer =
                        renderer
                            .device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("Sprite Index Buffer"),
                                contents: bytemuck::cast_slice(sheet.indices.as_slice()),
                                usage: wgpu::BufferUsages::INDEX,
                            });
                    let num_indices = sheet.indices.len() as u32;

                    pipeline.meshes.insert(
                        key,
                        SpriteMeshGpu {
                            vertex_buffer,
                            index_buffer,
                            num_indices,
                        },
                    );
                }
            }
        });
}

fn add_missing_instances(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<SpritePipeline>,
    q: Query<(&MeshHandle, &WeakHandle<SpriteSheet>), With<Visible>>,
) {
    let mut instances = q
        .iter()
        .map(|(mesh, sheet)| InstanceKey {
            sprite_sheet: sheet.id(),
            mesh: mesh.into(),
        })
        .collect::<Vec<_>>();

    instances.sort_unstable();
    for g in instances.chunk_by_mut(|a, b| a == b) {
        let k = g[0];
        pipeline
            .instances
            .entry(k)
            .or_insert_with(|| SpriteInstances {
                count: g.len(),
                instance_gpu: renderer.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!(
                        "Sprite Instance Buffer - {} {:?}",
                        k.sprite_sheet, k.mesh
                    )),
                    size: 0,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            });
    }
}
