use std::collections::HashMap;

use cecs::prelude::*;
use glam::{Mat4, Vec3, Vec4};

use crate::{renderer::GraphicsState, transform::GlobalTransform, Plugin, Stage};

#[derive(Default)]
pub struct ViewFrustum {
    pub planes: [Vec4; 6],
}

pub struct Camera3d {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera3d {
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
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
        }
    }
}

impl CameraUniform {
    pub fn desc<'a>() -> wgpu::BindGroupLayoutDescriptor<'a> {
        wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
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

fn update_view_projections(mut q: Query<(&GlobalTransform, &Camera3d, &mut CameraUniform)>) {
    for (tr, cam, uni) in q.iter_mut() {
        uni.view_proj = cam.view_projection() * tr.0.inverse().compute_matrix();
    }
}

fn update_camera_buffers(
    renderer: Res<GraphicsState>,
    mut buffers: ResMut<CameraBuffers>,
    q: Query<(EntityId, &CameraUniform)>,
) {
    for (id, uni) in q.iter() {
        let buffer = buffers.0.entry(id).or_insert_with(|| {
            renderer.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some(format!("camera3d-{id}").as_str()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                size: std::mem::size_of::<CameraUniform>() as u64,
                mapped_at_creation: false,
            })
        });

        renderer
            .queue()
            .write_buffer(buffer, 0, bytemuck::cast_slice(&[uni.view_proj]));
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

#[derive(Default)]
pub struct CameraBuffers(pub HashMap<EntityId, wgpu::Buffer>);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build<'a>(self, app: &mut crate::App) {
        app.insert_resource(CameraBuffers::default());
        app.stage(Stage::Update)
            .add_system(update_view_projections)
            .add_system(update_camera_buffers.after(update_view_projections))
            .add_system(update_frustum.after(update_view_projections));
    }
}
