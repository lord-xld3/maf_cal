use std::io;
use wgpu::util::{DeviceExt, BufferInitDescriptor};
use bytemuck::cast_slice;
use naga;

struct Range {
    min: f32,
    max: f32,
}

const PRECISION: u32 = 4096;
const POPULATION_SIZE: u32 = 100; // example value
const TOURNAMENT_SIZE: u32 = 10;  // example value
const MAX_GENERATIONS: u32 = 1000; // example value
const MUTATION_RATE: f32 = 0.01; // example value
const MUTATION_RANGE: f32 = 0.1; // example value
const RANGE_A: Range = Range { min: 0.0, max: 16.0 };
const RANGE_N: Range = Range { min: 0.0, max: 16.0 };
const MIN_A: f32 = RANGE_A.min;
const MAX_A: f32 = RANGE_A.max;
const MIN_N: f32 = RANGE_N.min;
const MAX_N: f32 = RANGE_N.max;

pub async fn run(x_data: &[f32], y_data: &[f32]) -> io::Result<(f32, f32)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        dx12_shader_compiler: Default::default(),
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .unwrap();
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::default(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                limits: wgpu::Limits::default(),
            },
            None, // Trace path
        )
        .await
        .unwrap();

    device.start_capture();
    // 1. Load the shader
    let cs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Compute Shader"),
        source: wgpu::ShaderSource::Glsl {
            shader: include_str!("shader.comp").into(),
            stage: naga::ShaderStage::Compute,
            defines: naga::FastHashMap::default(),
        },
    });

    // Input buffer
    let xy_data: Vec<[f32; 2]> = x_data.iter().zip(y_data.iter()).map(|(&x, &y)| [x, y]).collect();
    let xy_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("XY Buffer"),
        contents: bytemuck::cast_slice(&xy_data),
        usage: wgpu::BufferUsages::STORAGE,
    });
    // Output buffer
    let results_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Results Buffer"),
        size: (2 * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    // Constants buffer
    let const_data = [
        POPULATION_SIZE as f32,
        TOURNAMENT_SIZE as f32,
        MAX_GENERATIONS as f32,
        MUTATION_RATE,
        MUTATION_RANGE,
        MIN_A,
        MAX_A,
        MIN_N,
        MAX_N,
    ];
    let const_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("Const Buffer"),
        contents: bytemuck::cast_slice(&const_data),
        usage: wgpu::BufferUsages::STORAGE,
    });
    // Fitness buffer
    let fitness_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fitness Buffer"),
        size: (POPULATION_SIZE as usize * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    // Population buffer
    let pop_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Pop Buffer"),
        size: ((POPULATION_SIZE * 2) as usize * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    // Create a buffer to read the results from the GPU
    let read_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Read Buffer"),
        size: (2 * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    // Create a bind group layout and bind group
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new((x_data.len() * 2 * std::mem::size_of::<f32>()) as _), // vec2 xy_data[]; 2 f32 values for each vec2
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new((2 * std::mem::size_of::<f32>()) as _), // float best_a; float best_n;
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new((const_data.len() * std::mem::size_of::<f32>()) as _),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new((POPULATION_SIZE as usize * std::mem::size_of::<f32>()) as _),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(((POPULATION_SIZE * 2) as usize * std::mem::size_of::<f32>()) as _),
                },
                count: None,
            },
        ],
        label: Some("bind_group_layout"),
    });
    
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &xy_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new((x_data.len() * 2 * std::mem::size_of::<f32>()) as _), // vec2 xy_data[]; 2 f32 values for each vec2
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &results_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new((2 * std::mem::size_of::<f32>()) as _), // float best_a; float best_n;
                }),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &const_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new((const_data.len() * std::mem::size_of::<f32>()) as _),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &fitness_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new((POPULATION_SIZE as usize * std::mem::size_of::<f32>()) as _),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &pop_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(((POPULATION_SIZE * 2) as usize * std::mem::size_of::<f32>()) as _),
                }),
            },
        ],
        label: Some("bind_group"),
    });    

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Compute Pipeline Layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Compute Pipeline"),
        layout: Some(&pipeline_layout),
        module: &cs_module,
        entry_point: "main",
    });

    {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Compute Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Pass"),
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(PRECISION, PRECISION, 1);
        }
        // Copy the result from the result buffer to the read buffer
        encoder.copy_buffer_to_buffer(&results_buffer, 0, &read_buffer, 0, read_buffer.size());
        queue.submit(Some(encoder.finish()));
    }

    // Map the read buffer and read the results
    let result_slice = read_buffer.slice(..);

    // Use a channel to wait for the buffer mapping to complete
    let (tx, rx) = futures_intrusive::channel::shared::oneshot_channel();
    result_slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result).unwrap();
    });
    device.poll(wgpu::Maintain::Wait);
    rx.receive().await.unwrap().unwrap();

    // Get the mapped range and read the results
    let mapped_range = result_slice.get_mapped_range();
    let result_data: &[f32] = bytemuck::cast_slice(&mapped_range);

    let best_a = result_data[0];
    let best_n = result_data[1];

    // Drop the mapped view explicitly
    drop(mapped_range);

    println!("Optimized Coefficient (a): {}, Optimized Exponent (n): {}", best_a, best_n);
    device.stop_capture();
    Ok((best_a, best_n))
}