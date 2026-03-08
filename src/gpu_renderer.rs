use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
// --- UNIFORM DATA ---

/// Per-frame transform data passed to the GPU via a uniform buffer.
/// Maps (lat, lon) aircraft positions into screen clip-space.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Uniforms {
    /// Screen rect: (left, top, width, height) in pixels.
    pub rect: [f32; 4],
    /// Viewport size: (width_px, height_px, zoom, _pad).
    pub viewport: [f32; 4],
    /// Pan offset: (offset_x, offset_y, point_size, _pad).
    pub offset: [f32; 4],
}

// --- GPU RESOURCES (stored in CallbackResources) ---

/// Persistent GPU resources that live across frames.
/// Stored inside `egui_wgpu::Renderer::callback_resources`.
pub struct AircraftGpuResources {
    pub aircraft_pipeline: wgpu::RenderPipeline,
    pub trail_pipeline: wgpu::RenderPipeline,
    pub uniform_buffer: wgpu::Buffer,
    pub uniform_bind_group: wgpu::BindGroup,
    pub aircraft_buffer: wgpu::Buffer,
    pub aircraft_capacity: usize,
    pub trail_buffer: wgpu::Buffer,
    pub trail_capacity: usize,
}

impl AircraftGpuResources {
    const INITIAL_AIRCRAFT_CAP: usize = 8192;
    const INITIAL_TRAIL_CAP: usize = 65536;

    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aircraft Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("aircraft.wgsl").into()),
        });

        // Uniform bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aircraft Uniform BGL"),
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
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Aircraft Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aircraft Uniform BG"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Aircraft Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Vertex buffer layout for instanced positions: vec2<f32> per instance
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };

        // Aircraft pipeline: renders instanced quads (6 verts per aircraft dot)
        let aircraft_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Aircraft Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_aircraft",
                buffers: std::slice::from_ref(&instance_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_aircraft",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Trail line vertex layout: position (vec2<f32>) per vertex
        let trail_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };

        // Trail pipeline: renders line segments
        let trail_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Trail Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_trail",
                buffers: &[trail_vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_trail",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let aircraft_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Aircraft Instance Buffer"),
            size: (Self::INITIAL_AIRCRAFT_CAP * std::mem::size_of::<[f32; 2]>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let trail_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Trail Vertex Buffer"),
            size: (Self::INITIAL_TRAIL_CAP * std::mem::size_of::<[f32; 2]>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            aircraft_pipeline,
            trail_pipeline,
            uniform_buffer,
            uniform_bind_group,
            aircraft_buffer,
            aircraft_capacity: Self::INITIAL_AIRCRAFT_CAP,
            trail_buffer,
            trail_capacity: Self::INITIAL_TRAIL_CAP,
        }
    }
}

// --- PER-FRAME CALLBACK ---

/// Lightweight per-frame callback that carries the data to upload to GPU.
/// Implements `egui_wgpu::CallbackTrait`.
pub struct AircraftCallback {
    /// Aircraft positions as (lat, lon) pairs.
    pub aircraft_positions: Arc<Vec<[f32; 2]>>,
    /// Trail line endpoints: pairs of (lat, lon) forming line segments.
    pub trail_vertices: Arc<Vec<[f32; 2]>>,
    /// The egui rect (in points) where the map is drawn.
    pub rect: [f32; 4],
    /// Viewport size in pixels.
    pub viewport_px: [f32; 2],
    /// Zoom factor.
    pub zoom: f32,
    /// Pan offset in points.
    pub offset: [f32; 2],
    /// Point size for aircraft dots.
    pub point_size: f32,
}

impl egui_wgpu::CallbackTrait for AircraftCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let resources: &mut AircraftGpuResources = callback_resources.get_mut().unwrap();

        // Upload uniforms
        let uniforms = Uniforms {
            rect: self.rect,
            viewport: [self.viewport_px[0], self.viewport_px[1], self.zoom, 0.0],
            offset: [self.offset[0], self.offset[1], self.point_size, 0.0],
        };
        queue.write_buffer(&resources.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Upload aircraft instance data (grow buffer if needed)
        let aircraft_data: &[u8] = bytemuck::cast_slice(&self.aircraft_positions);
        if self.aircraft_positions.len() > resources.aircraft_capacity {
            let new_cap = self.aircraft_positions.len().next_power_of_two();
            resources.aircraft_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Aircraft Instance Buffer"),
                size: (new_cap * std::mem::size_of::<[f32; 2]>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            resources.aircraft_capacity = new_cap;
        }
        if !aircraft_data.is_empty() {
            queue.write_buffer(&resources.aircraft_buffer, 0, aircraft_data);
        }

        // Upload trail vertex data (grow buffer if needed)
        let trail_data: &[u8] = bytemuck::cast_slice(&self.trail_vertices);
        if self.trail_vertices.len() > resources.trail_capacity {
            let new_cap = self.trail_vertices.len().next_power_of_two();
            resources.trail_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Trail Vertex Buffer"),
                size: (new_cap * std::mem::size_of::<[f32; 2]>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            resources.trail_capacity = new_cap;
        }
        if !trail_data.is_empty() {
            queue.write_buffer(&resources.trail_buffer, 0, trail_data);
        }

        Vec::new()
    }

    fn paint<'a>(
        &'a self,
        _info: epaint::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'a>,
        callback_resources: &'a egui_wgpu::CallbackResources,
    ) {
        let resources: &AircraftGpuResources = callback_resources.get().unwrap();

        // Draw trail lines
        let num_trail_verts = self.trail_vertices.len() as u32;
        if num_trail_verts >= 2 {
            render_pass.set_pipeline(&resources.trail_pipeline);
            render_pass.set_bind_group(0, &resources.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, resources.trail_buffer.slice(..));
            render_pass.draw(0..num_trail_verts, 0..1);
        }

        // Draw aircraft dots (instanced: 6 vertices per quad, N instances)
        let num_aircraft = self.aircraft_positions.len() as u32;
        if num_aircraft > 0 {
            render_pass.set_pipeline(&resources.aircraft_pipeline);
            render_pass.set_bind_group(0, &resources.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, resources.aircraft_buffer.slice(..));
            render_pass.draw(0..6, 0..num_aircraft);
        }
    }
}
