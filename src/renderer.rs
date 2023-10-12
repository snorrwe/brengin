pub mod sprite_renderer;
pub mod texture;

use cecs::prelude::*;
use tracing::debug;
use wgpu::Backends;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::{Camera3d, CameraBuffers, CameraPlugin, CameraUniform, ViewFrustum},
    Plugin,
};

use self::sprite_renderer::SpriteRendererPlugin;

pub fn camera_bundle(camera: Camera3d) -> impl cecs::bundle::Bundle {
    (camera, CameraUniform::default(), ViewFrustum::default())
}

pub struct GraphicsState {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,

    camera_bind_group_layout: wgpu::BindGroupLayout,

    depth_texture: texture::Texture,
}

impl GraphicsState {
    pub async fn new(window: &Window) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: Backends::all(),
            dx12_shader_compiler: Default::default(),
        });
        let surface = unsafe {
            instance
                .create_surface(&window)
                .expect("Failed to create surface")
        };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to create adapter");

        debug!("Choosen adapter: {:?}", adapter);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    // WebGL doesn't support all of wgpu's features, so if
                    // we're building for the web we'll have to disable some.
                    limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                    label: None,
                },
                None, // Trace path
            )
            .await
            .unwrap();

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            view_formats: vec![surface.get_capabilities(&adapter).formats[0]],
            width: size.width.max(1),
            height: size.height.max(1),
            // TODO: configure
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        };
        surface.configure(&device, &config);

        let camera_bind_group_layout = device.create_bind_group_layout(&CameraUniform::desc());

        let depth_texture =
            texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        Self {
            depth_texture,
            size,
            device,
            queue,
            config,
            surface,
            camera_bind_group_layout,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture =
                texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
        }
    }

    pub fn render(
        &mut self,
        cameras: &CameraBuffers,
        sprite_pipeline: &sprite_renderer::SpritePipeline,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        for camera_buffer in cameras.0.values() {
            let camera_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.camera_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                }],
                label: Some("camera_bind_group"),
            });
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Transparent Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.4588,
                                g: 0.031,
                                b: 0.451,
                                a: 1.0,
                            }),
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_texture.view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: true,
                        }),
                        stencil_ops: None,
                    }),
                });
                sprite_pipeline.render(&mut render_pass, &camera_bind_group);
            }
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn surface(&self) -> &wgpu::Surface {
        &self.surface
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn size_mut(&mut self) -> &mut PhysicalSize<u32> {
        &mut self.size
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            // attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2, // NEW!
                },
            ],
        }
    }
}

pub type RenderResult = Result<(), wgpu::SurfaceError>;

pub struct RendererPlugin;

impl Plugin for RendererPlugin {
    fn build(self, app: &mut crate::App) {
        app.stage(crate::Stage::Render).add_system(render_system);
        app.add_plugin(CameraPlugin);
        app.add_plugin(SpriteRendererPlugin);
    }
}

fn render_system(
    mut state: ResMut<GraphicsState>,
    mut cmd: Commands,
    sprite_pipeline: Res<sprite_renderer::SpritePipeline>,
    cameras: Res<CameraBuffers>,
) {
    let result = state.render(&cameras, &sprite_pipeline);
    // Reconfigure the surface if lost
    if let Err(wgpu::SurfaceError::Lost) = result {
        let size = state.size();
        state.resize(size);
    }
    cmd.insert_resource(result);
}

fn texture_bind_group_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                // This should match the filterable field of the
                // corresponding Texture entry above.
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
        label: Some(label),
    })
}

fn texture_to_bindings(
    device: &wgpu::Device,
    texture: &texture::Texture,
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let texture_bind_group_layout = texture_bind_group_layout(device, "texture_bind_group_layout");
    let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&texture.sampler),
            },
        ],
        label: Some("diffuse_bind_group"),
    });
    (texture_bind_group_layout, diffuse_bind_group)
}
