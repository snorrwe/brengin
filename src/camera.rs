use cecs::prelude::*;
use glam::{IVec2, Mat4, Vec3, Vec4};

use crate::{
    renderer::{ExtractionPlugin, GraphicsState, WindowSize},
    transform::GlobalTransform,
    Plugin, Stage,
};

#[derive(Default)]
pub struct ViewFrustum {
    pub planes: [Vec4; 6],
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CameraSize {
    pub width: u32,
    pub height: u32,
}

pub struct PerspectiveCamera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Default for PerspectiveCamera {
    fn default() -> Self {
        PerspectiveCamera {
            eye: Vec3::new(0.0, 0.0, 100.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy: std::f32::consts::TAU / 6.0,
            znear: 5.0,
            zfar: 5000.0,
        }
    }
}

/// Cameras marked with this component are automatically updated to fit their window
/// Camera entities do not have this component by default
pub struct WindowCamera;

fn update_camera_aspect(
    gs: Res<WindowSize>,
    mut q: Query<(&mut PerspectiveCamera, &mut CameraSize), With<WindowCamera>>,
) {
    let size = *gs;
    let aspect = size.width as f32 / size.height as f32;
    for (cam, size) in q.iter_mut() {
        cam.aspect = aspect;
        size.width = gs.width;
        size.height = gs.height;
    }
}

impl PerspectiveCamera {
    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_lh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_lh(self.fovy, self.aspect, self.znear, self.zfar);

        proj * view
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: Mat4,
    pub view: Mat4,
    pub proj: Mat4,
    pub view_inv: Mat4,
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
            view_inv: Mat4::IDENTITY,
        }
    }
}

impl CameraUniform {
    pub fn desc<'a>() -> wgpu::BindGroupLayoutDescriptor<'a> {
        wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("camera_bind_group_layout"),
        }
    }
}

fn update_view_projections(
    mut q: Query<(&GlobalTransform, &PerspectiveCamera, &mut CameraUniform)>,
) {
    for (tr, cam, uni) in q.iter_mut() {
        uni.view = tr.0.inverse().compute_matrix();
        uni.view_inv = uni.view.inverse();
        uni.proj = cam.view_projection();
        uni.view_proj = uni.proj * uni.view;
    }
}

impl crate::renderer::Extract for CameraUniform {
    type QueryItem = &'static CameraUniform;

    type Filter = ();

    type Out = (Self,);

    fn extract<'a>(
        it: <Self::QueryItem as cecs::query::QueryFragment>::Item<'a>,
    ) -> Option<Self::Out> {
        Some((*it,))
    }
}

fn upload_camera_uniform(queue: &wgpu::Queue, buffer: &wgpu::Buffer, uni: &CameraUniform) {
    queue.write_buffer(
        &buffer,
        0,
        bytemuck::cast_ref::<_, [u8; std::mem::size_of::<CameraUniform>()]>(uni),
    );
}

fn insert_missing_camera_buffers(
    renderer: Res<GraphicsState>,
    q_new: Query<(EntityId, &CameraUniform), WithOut<CameraBuffer>>,
    mut cmd: Commands,
) {
    for (id, uni) in q_new.iter() {
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some(format!("camera3d-{id}").as_str()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            size: std::mem::size_of::<CameraUniform>() as u64,
            mapped_at_creation: false,
        });
        upload_camera_uniform(renderer.queue(), &buffer, uni);
        cmd.entity(id).insert(CameraBuffer(buffer));
    }
}

fn update_camera_buffers(
    renderer: Res<GraphicsState>,
    q: Query<(&CameraUniform, &mut CameraBuffer)>,
) {
    for (uni, CameraBuffer(buffer)) in q.iter() {
        upload_camera_uniform(renderer.queue(), &buffer, uni);
    }
}

fn update_frustum(mut q: Query<(&mut ViewFrustum, &CameraUniform)>) {
    for (fr, cam) in q.iter_mut() {
        let mat = &cam.view_proj;
        // left
        for i in 0..4 {
            fr.planes[0][i] = mat.col(i)[3] + mat.col(i)[0];
        }
        // right
        for i in 0..4 {
            fr.planes[1][i] = mat.col(i)[3] - mat.col(i)[0];
        }
        // bot
        for i in 0..4 {
            fr.planes[2][i] = mat.col(i)[3] + mat.col(i)[1];
        }
        // top
        for i in 0..4 {
            fr.planes[3][i] = mat.col(i)[3] - mat.col(i)[1];
        }
        // near
        for i in 0..4 {
            fr.planes[4][i] = mat.col(i)[3] + mat.col(i)[2];
        }
        // far
        for i in 0..4 {
            fr.planes[5][i] = mat.col(i)[3] - mat.col(i)[2];
        }

        // normalize planes
        for plane in fr.planes.iter_mut() {
            let mag = plane.truncate().length();
            *plane /= mag;
        }
    }
}

pub struct CameraBuffer(pub wgpu::Buffer);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build<'a>(self, app: &mut crate::App) {
        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(update_camera_aspect);
        })
        .with_stage(Stage::Update, |s| {
            s.add_system(update_view_projections)
                .add_system(update_frustum.after(update_view_projections));
        });

        app.add_plugin(ExtractionPlugin::<CameraUniform>::default());

        app.render_app_mut().with_stage(Stage::Update, |s| {
            s.add_system(insert_missing_camera_buffers)
                .add_system(update_camera_buffers);
        });
    }
}

pub fn camera_bundle(camera: PerspectiveCamera) -> impl cecs::bundle::Bundle {
    (
        camera,
        CameraUniform::default(),
        ViewFrustum::default(),
        CameraSize::default(),
    )
}
