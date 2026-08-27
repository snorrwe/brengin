//! Performs instanced rendering of spritesheets.
//!
//! Entities are grouped together by their meshes and spritesheets.
//!
//! For arbitrary meshes, it's assumed that the mesh fills a 1by1 AABB.
//! This fact is used by the visibility calculation
//!
//! TODO: support arbitrary sized meshes in visibility
//!
pub mod sprite_sheet;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};

use cecs::prelude::*;
use glam::{FloatExt as _, Vec2};
use wgpu::{include_wgsl, util::DeviceExt};

use crate::{
    Plugin, Stage,
    assets::{AssetId, Assets, AssetsPlugin, Handle, WeakHandle},
    camera::ViewFrustum,
    renderer::texture_atlas::{AtlasRect, TextureAtlas, TextureAtlasRegistry},
    transform::GlobalTransform,
};

use super::{
    GraphicsState, RenderCommand, RenderCommandInput, RenderCommandPlugin, RenderPass, Vertex,
    texture::texture_bind_group_layout,
};

pub use sprite_sheet::{SpriteInstance, SpriteSheet, sprite_sheet_bundle};

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

fn update_cull(
    mut q: Query<(&GlobalTransform, &mut CullSize, &Handle<SpriteSheet>)>,
    assets: Res<Assets<SpriteSheet>>,
) {
    q.par_for_each_mut(|(GlobalTransform(tr), cull, sheet)| {
        let Some(sheet) = assets.get(sheet) else {
            return;
        };
        cull.0 = (sheet.box_size.x * tr.scale.x).max(sheet.box_size.y * tr.scale.y);
    });
}

fn insert_missing_cull(
    q: Query<EntityId, (WithOut<CullSize>, With<Handle<SpriteSheet>>)>,
    mut cmd: Commands,
) {
    for id in q.iter() {
        cmd.entity(id).insert(CullSize(0.0));
    }
}

pub fn add_missing_sheets(
    mut pipeline: ResMut<SpritePipeline>,
    sheets: Res<crate::assets::Assets<SpriteSheet>>,
    mut reg: TextureAtlasRegistry,
) {
    for (id, sheet) in sheets.iter() {
        if !pipeline.sheets.contains_key(&id) {
            pipeline.add_sheet(id, sheet, &mut reg);
        }
    }
}

fn unload_sheets(
    mut handles: ResMut<RenderSpritesheetHandles>,
    mut pipeline: ResMut<SpritePipeline>,
    mut instances: ResMut<SpritePipelineInstances>,
    mut textures: TextureAtlasRegistry,
) {
    let unloaded = handles
        .0
        .iter()
        .filter(|(_, h)| h.upgrade().is_none())
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in unloaded {
        if let Some(data) = pipeline.unload_sheet(id) {
            textures.deallocate(data.rect);
            if let Some(mask_rect) = data.mask_rect {
                textures.deallocate(mask_rect);
            }
        }
        instances.0.retain(|k, _| k.sprite_sheet != id);
        handles.0.remove(&id);
    }
}

fn spritesheet_uv_in_atlas(spritesheet_uv: &[Vec2; 2], atlas_uv: &[f32; 4]) -> [f32; 4] {
    let uv_minx = atlas_uv[0].lerp(atlas_uv[2], spritesheet_uv[0].x);
    let uv_maxx = atlas_uv[0].lerp(atlas_uv[2], spritesheet_uv[1].x);
    let uv_miny = atlas_uv[1].lerp(atlas_uv[3], spritesheet_uv[0].y);
    let uv_maxy = atlas_uv[1].lerp(atlas_uv[3], spritesheet_uv[1].y);
    [uv_minx, uv_miny, uv_maxx, uv_maxy]
}

fn compute_sprite_instances(
    mut q: Query<
        (
            &crate::transform::GlobalTransform,
            &SpriteInstance,
            &mut SpriteInstanceRaw,
            &Handle<SpriteSheet>,
        ),
        With<Visible>,
    >,
    sheets: Res<crate::assets::Assets<SpriteSheet>>,
    pipeline: Res<SpritePipeline>,
) {
    q.par_for_each_mut(|(tr, i, instance, sheet)| {
        let pos = tr.0.pos.to_array();
        let scale = tr.0.scale.truncate().to_array();
        let Some(sprite_rendering_data) = pipeline.sheets.get(&sheet.id()) else {
            return;
        };
        let Some(sheet) = sheets.get(sheet) else {
            return;
        };
        let spritesheet_uv = sheet.get_instance_uv(*i);

        let atlas_uv = sprite_rendering_data.rect.uv();
        let mask_uv = sprite_rendering_data.mask_rect.as_ref().map(|r| r.uv());

        let uv = spritesheet_uv_in_atlas(&spritesheet_uv, &atlas_uv);
        let mask_uv = mask_uv
            .map(|atlas_uv| spritesheet_uv_in_atlas(&spritesheet_uv, &atlas_uv))
            .unwrap_or_else(|| pipeline.default_mask_rect.uv());

        *instance = SpriteInstanceRaw {
            mask_uv,
            uv,
            pos,
            scale,
            color_flip: (i.color.0 & 0xFFFFFF00) | i.flip as u32,
        };
    });
}

#[derive(Default)]
struct SpritePipelineInstances(BTreeMap<InstanceKey, Vec<SpriteInstanceRaw>>);

/// Groups by the texture atlases and the mesh
/// holds sprite_sheet id for identification, but it is ignored in comparision and hash functions
#[derive(Default, Debug, Clone, Copy)]
struct InstanceKey {
    pub sprite_sheet: AssetId<SpriteSheet>,
    pub texture: AssetId<TextureAtlas>,
    pub mask: AssetId<TextureAtlas>,
    pub mesh: MeshKey,
}

impl Eq for InstanceKey {}

impl Ord for InstanceKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl std::hash::Hash for InstanceKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.texture.hash(state);
        self.mask.hash(state);
        self.mesh.hash(state);
    }
}

impl PartialOrd for InstanceKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.texture.partial_cmp(&other.texture) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.mask.partial_cmp(&other.mask) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.mesh.partial_cmp(&other.mesh)
    }
}

impl PartialEq for InstanceKey {
    fn eq(&self, other: &Self) -> bool {
        self.texture == other.texture && self.mask == other.mask && self.mesh == other.mesh
    }
}

fn clear_pipeline_instances(mut instances: ResMut<SpritePipelineInstances>) {
    for i in instances.0.values_mut() {
        i.clear();
    }
}

fn update_sprite_pipelines(
    renderer: Res<GraphicsState>,
    q: Query<(
        &Handle<SpriteSheet>,
        &SpriteInstanceRaw,
        Option<&Handle<SpriteMesh>>,
    )>,
    mut pipeline: ResMut<SpritePipeline>,
    mut instances: ResMut<SpritePipelineInstances>,
) {
    for (sheet, raw, mesh) in q.iter() {
        let mesh = mesh
            .map(|h| MeshHandle::Mesh(h.downgrade()))
            .unwrap_or_default();

        let Some(t) = pipeline.sheets.get(&sheet.id()) else {
            continue;
        };

        let k = InstanceKey {
            sprite_sheet: sheet.id(),
            texture: t.rect.atlas_handle().id(),
            mask: t
                .mask_rect
                .as_ref()
                .map(|r| r.atlas_handle().id())
                .unwrap_or_default(),
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
            instances.instance_gpu.destroy();
            instances.instance_gpu = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!(
                    "Sprite Instance Buffer - {:?} {:?} {:?}",
                    id.texture, id.mask, id.mesh
                )),
                size: size * 3 / 2,
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
struct RenderSpritesheetHandles(pub HashMap<AssetId<SpriteSheet>, WeakHandle<SpriteSheet>>);

// per spritesheet
pub struct SpriteRenderingData {
    pub texture_bind_group: wgpu::BindGroup,
    pub mask_bind_group: Option<wgpu::BindGroup>,
    pub rect: AtlasRect,
    pub mask_rect: Option<AtlasRect>,
}

struct SpriteInstances {
    pub count: usize,
    pub instance_gpu: wgpu::Buffer,
}

pub struct SpritePipeline {
    instances: HashMap<InstanceKey, SpriteInstances>,
    sheets: HashMap<AssetId<SpriteSheet>, SpriteRenderingData>,
    // TODO: unload unused meshes
    meshes: BTreeMap<MeshKey, SpriteMeshGpu>,
    // shared
    render_pipeline: wgpu::RenderPipeline,
    // TODO: this 1x1 allocation is leaked by the renderer but it's
    // not a big deal, the renderer should only be destroyed on program exit anyway
    default_mask_rect: AtlasRect,
    default_mask_bind_group: wgpu::BindGroup,
}

impl SpritePipeline {
    pub fn unload_sheet(&mut self, id: AssetId<SpriteSheet>) -> Option<SpriteRenderingData> {
        self.sheets.remove(&id)
    }

    pub fn add_sheet(
        &mut self,
        id: AssetId<SpriteSheet>,
        sheet: &SpriteSheet,
        atlases: &mut TextureAtlasRegistry,
    ) {
        let Some(texture_rect) = atlases.allocate(sheet.size.x as i32, sheet.size.y as i32) else {
            #[cfg(feature = "tracing")]
            tracing::error!(?id, "Failed to allocate texture for spritesheet");
            return;
        };
        atlases.upload_image(&texture_rect, &sheet.image);
        let texture_bind_group = atlases.get_bind_group(&texture_rect).1;

        let mask_rect = sheet
            .mask
            .as_ref()
            .and_then(|m| atlases.allocate(m.width() as i32, m.height() as i32));
        if let (Some(m), Some(r)) = (sheet.mask.as_ref(), mask_rect.as_ref()) {
            atlases.upload_image(r, m);
        }

        let mask_bind_group = mask_rect.as_ref().map(|m| atlases.get_bind_group(m).1);

        self.sheets.insert(
            id,
            SpriteRenderingData {
                texture_bind_group,
                mask_bind_group,
                rect: texture_rect,
                mask_rect,
            },
        );
    }

    pub fn new(renderer: &GraphicsState, reg: &mut TextureAtlasRegistry) -> Self {
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
                        Some(&renderer.camera_bind_group_layout),
                        Some(&texture_bind_group_layout),
                        Some(&texture_bind_group_layout),
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
                        buffers: &[Some(Vertex::desc()), Some(SpriteInstanceRaw::desc())],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: renderer.config.format,
                            // the fragment shader outputs premultiplied alpha, which avoids
                            // dark fringes around sprite edges when the texture is filtered
                            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0,
                        // alpha-to-coverage only has a single sample to work with here, which
                        // quantizes (and on most drivers dithers) partial alpha
                        alpha_to_coverage_enabled: false,
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

        let default_texture = reg
            .allocate(1, 1)
            .expect("Failed to allocate initial texture");

        reg.upload_rgba(&default_texture, &[0; 4], 1, 1);

        let (_, default_mask_bind_group) = reg.get_bind_group(&default_texture);

        SpritePipeline {
            sheets: Default::default(),
            meshes,
            render_pipeline,
            instances: Default::default(),
            default_mask_rect: default_texture,
            default_mask_bind_group,
        }
    }
}

struct SpriteRenderCommand;

impl<'a> RenderCommand<'a> for SpriteRenderCommand {
    type Parameters = Res<'a, SpritePipeline>;

    fn render<'r>(
        RenderCommandInput {
            render_pass,
            camera,
        }: &'r mut RenderCommandInput<'a, 'r>,
        pipeline: &'r Self::Parameters,
    ) {
        render_pass.set_pipeline(&pipeline.render_pipeline);
        for (k, instances) in pipeline.instances.iter().filter(|(_, s)| s.count > 0) {
            let Some(mesh) = pipeline.meshes.get(&k.mesh) else {
                continue;
            };
            let Some(sheet) = pipeline.sheets.get(&k.sprite_sheet) else {
                continue;
            };

            render_pass.set_bind_group(0, *camera, &[]);
            render_pass.set_bind_group(1, &sheet.texture_bind_group, &[]);
            match sheet.mask_bind_group.as_ref() {
                Some(bg) => {
                    render_pass.set_bind_group(2, bg, &[]);
                }
                None => {
                    render_pass.set_bind_group(2, &pipeline.default_mask_bind_group, &[]);
                }
            }
            render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, instances.instance_gpu.slice(..));
            render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.draw_indexed(0..mesh.num_indices, 0, 0..instances.count as u32);
        }
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteInstanceRaw {
    pos: [f32; 3],
    scale: [f32; 2],
    uv: [f32; 4],
    mask_uv: [f32; 4],
    /// rgb 24 bits, bool 8 bits
    color_flip: u32,
}

impl SpriteInstanceRaw {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        const POS_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 3]>() as wgpu::BufferAddress;
        const SCALE_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 2]>() as wgpu::BufferAddress;
        const UV_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 4]>() as wgpu::BufferAddress;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<SpriteInstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: POS_SIZE,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: POS_SIZE + SCALE_SIZE,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: POS_SIZE + SCALE_SIZE + UV_SIZE,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: POS_SIZE + SCALE_SIZE + UV_SIZE * 2,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

const SQUARE_VERTICES: &[Vertex] = &[
    // A
    Vertex {
        pos: [-0.5, -0.5, 0.0],
        uv: [0.0, 0.0],
    },
    // B
    Vertex {
        pos: [-0.5, 0.5, 0.0],
        uv: [0.0, 1.0],
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

const SQUARE_INDICES: &[u16] = &[0, 1, 2, 2, 1, 3];

fn setup(mut cmd: Commands, graphics_state: Res<GraphicsState>, mut reg: TextureAtlasRegistry) {
    let sprite_pipeline = SpritePipeline::new(&graphics_state, &mut reg);
    cmd.insert_resource(sprite_pipeline);
}

pub struct SpriteRendererPlugin;

impl Plugin for SpriteRendererPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(AssetsPlugin::<SpriteSheet>::default());
        app.add_plugin(AssetsPlugin::<SpriteMesh>::default());
        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(clear_pipeline_instances);
        });
        app.with_stage(Stage::Update, |s| {
            // putting this system in update means that the last frame's data will be presented
            s.add_system(compute_sprite_instances)
                .add_system(insert_missing_cull)
                .add_system(update_cull)
                .add_system(update_visible.after(update_cull))
                .add_system(update_invisible.after(update_cull))
                .add_system(unload_sheets)
                .add_system(update_sprite_pipelines);
        });
        app.with_stage(Stage::PostUpdate, |s| {
            s.add_system(add_missing_sheets)
                .add_system(add_missing_meshes)
                .add_system(add_missing_instance_buffers);
        });

        app.add_plugin(RenderCommandPlugin::<SpriteRenderCommand>::new(
            RenderPass::Opaque,
        ));
        app.add_startup_system(setup);
        app.insert_resource(SpritePipelineInstances::default());
        app.insert_resource(RenderSpritesheetHandles::default());
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

#[derive(Debug, Default)]
enum MeshHandle {
    Mesh(WeakHandle<SpriteMesh>),
    #[default]
    DefaultSquare,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum MeshKey {
    Mesh(AssetId<SpriteMesh>),
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
    mut pipeline: ResMut<SpritePipeline>,
    meshes: Res<crate::assets::Assets<SpriteMesh>>,
) {
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
}

fn add_missing_instance_buffers(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<SpritePipeline>,
    q: Query<(Option<&Handle<SpriteMesh>>, &Handle<SpriteSheet>), With<Visible>>,
) {
    let instances = q
        .iter()
        .filter_map(|(mesh, sheet)| {
            let mesh = mesh
                .map(|h| MeshHandle::Mesh(h.downgrade()))
                .unwrap_or_default();
            let t = pipeline.sheets.get(&sheet.id())?;
            Some(InstanceKey {
                sprite_sheet: sheet.id(),
                texture: t.rect.atlas_handle().id(),
                mask: t
                    .mask_rect
                    .as_ref()
                    .map(|r| r.atlas_handle().id())
                    .unwrap_or_default(),
                mesh: mesh.into(),
            })
        })
        .fold(HashSet::new(), |mut a, b| {
            a.insert(b);
            a
        });

    for k in instances.into_iter() {
        pipeline
            .instances
            .entry(k)
            .or_insert_with(|| SpriteInstances {
                count: 0,
                instance_gpu: renderer.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!(
                        "Sprite Instance Buffer - {:?} {:?} {:?}",
                        k.texture, k.mask, k.mesh
                    )),
                    size: 0,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            });
    }
}
