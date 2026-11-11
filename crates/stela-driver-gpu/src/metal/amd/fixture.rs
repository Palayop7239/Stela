//! Test snapshots modeled on what RADV (Mesa main, `radv_physical_device.c`
//! and `ac_gpu_info.c`) reports for GFX10.3 and later graphics cores.
//!
//! These are fixtures for the derivation code, not a substitute for the
//! runtime query. Limits and features follow RADV's own tables and gates.
//! Format support reuses the NVK fixture's simplified table without
//! `D24_UNORM_S8_UINT`, which RADV lacks; detailed format tests stay with
//! the NVIDIA fixture. Memory models a discrete card with Resizable BAR, or
//! an APU's carve-out beside system memory.
use super::{Chip, Generation};
use crate::metal::{
    nvidia::fixture::formats,
    vulkan::{
        DriverId, Format, MemoryHeapFlags, MemoryProperties as M, PhysicalDeviceType, ResolveModes,
        SampleCounts, ShaderStages, SubgroupFeatures,
        physical::{
            Features, FormatSupport, FormatTable, GeneratedCommandsLimits, Identity, Limits,
            Memory, MemoryHeap, MemoryType, MeshLimits, PciBusInfo, PhysicalDevice,
            SampleLocationLimits, SparseLimits,
        },
    },
};
use alloc::{format, vec};

const SAMPLE_COUNTS: SampleCounts = SampleCounts::C1
    .union(SampleCounts::C2)
    .union(SampleCounts::C4)
    .union(SampleCounts::C8);
/// About `radv_max_descriptor_set_size()`: 2 GiB of descriptors over the
/// 224 bytes the largest combination of them takes.
const DESCRIPTORS: u32 = ((1u64 << 31) / 224) as u32;
/// `RADV_MAX_MEMORY_ALLOCATION_SIZE`.
const MAX_ALLOCATION: u64 = 0xffff_fffc;

/// RADV's device for `chip` with `vram` bytes of local memory (an APU's
/// carve-out).
pub(crate) fn radv(chip: &Chip, vram: u64) -> PhysicalDevice {
    PhysicalDevice {
        identity: Identity {
            api_version: (1 << 22) | (4 << 12),
            driver_id: DriverId::MesaRadv,
            driver_version: 0,
            vendor_id: super::VENDOR_ID,
            // An RX 6650 XT for Navi 23; the other cores take a sample ID.
            device_id: if chip.name == "Navi 23" {
                0x73ef
            } else {
                0x7400
            },
            pipeline_cache_uuid: [0xa3; 16],
            device_type: if chip.integrated {
                PhysicalDeviceType::IntegratedGpu
            } else {
                PhysicalDeviceType::DiscreteGpu
            },
            device_name: format!("AMD Radeon test GPU (RADV {})", chip.name),
            conformance_version: [1, 4, 0, 0],
            pci_bus: Some(PciBusInfo {
                domain: 0,
                bus: 3,
                device: 0,
                function: 0,
            }),
        },
        limits: limits(chip.generation()),
        features: features(chip.generation()),
        memory: memory(vram),
        formats: radv_formats(),
    }
}

fn limits(generation: Generation) -> Limits {
    let gfx12 = generation == Generation::Rdna4;
    // `radv_get_image_properties` and `max_viewport_size`.
    let (dims, depth, viewport) = if gfx12 {
        (32768, 16384, 32768)
    } else {
        (16384, 8192, 16384)
    };
    Limits {
        max_image_dimension_1d: dims,
        max_image_dimension_2d: dims,
        max_image_dimension_3d: depth,
        max_image_dimension_cube: dims,
        max_image_array_layers: 8192,
        max_texel_buffer_elements: 512 << 20,
        max_uniform_buffer_range: u32::MAX,
        max_storage_buffer_range: u32::MAX,
        max_push_constants_size: 256,
        max_memory_allocation_count: u32::MAX,
        max_sampler_allocation_count: 64 << 10,
        buffer_image_granularity: 1,
        sparse_address_space_size: 1 << 47,
        max_per_stage_update_after_bind_samplers: DESCRIPTORS,
        max_per_stage_update_after_bind_sampled_images: DESCRIPTORS,
        max_per_stage_update_after_bind_storage_images: DESCRIPTORS,
        max_per_stage_update_after_bind_storage_buffers: DESCRIPTORS,
        max_descriptor_set_update_after_bind_samplers: DESCRIPTORS,
        max_descriptor_set_update_after_bind_sampled_images: DESCRIPTORS,
        max_descriptor_set_update_after_bind_storage_images: DESCRIPTORS,
        max_vertex_input_attributes: 32,
        max_vertex_input_bindings: 32,
        max_vertex_input_attribute_offset: u32::MAX,
        max_vertex_input_binding_stride: 2048,
        max_vertex_output_components: 128,
        max_tessellation_generation_level: 64,
        max_tessellation_patch_size: 32,
        max_fragment_input_components: 128,
        max_fragment_output_attachments: 8,
        max_fragment_dual_src_attachments: 1,
        // `lds_size_per_workgroup` from GFX7.
        max_compute_shared_memory_size: 64 << 10,
        max_compute_work_group_count: [u32::MAX, 65535, 65535],
        max_compute_work_group_invocations: 1024,
        max_compute_work_group_size: [1024, 1024, 1024],
        max_sampler_lod_bias: 16.0,
        max_sampler_anisotropy: 16.0,
        max_viewports: 16,
        max_viewport_dimensions: [viewport, viewport],
        min_texel_buffer_offset_alignment: 4,
        min_uniform_buffer_offset_alignment: 4,
        min_storage_buffer_offset_alignment: 4,
        max_framebuffer_width: 1 << 14,
        max_framebuffer_height: 1 << 14,
        max_framebuffer_layers: 1 << 10,
        framebuffer_color_sample_counts: SAMPLE_COUNTS,
        framebuffer_depth_sample_counts: SAMPLE_COUNTS,
        framebuffer_stencil_sample_counts: SAMPLE_COUNTS,
        framebuffer_no_attachments_sample_counts: SAMPLE_COUNTS,
        max_color_attachments: 8,
        sampled_image_color_sample_counts: SAMPLE_COUNTS,
        sampled_image_integer_sample_counts: SAMPLE_COUNTS,
        sampled_image_depth_sample_counts: SAMPLE_COUNTS,
        storage_image_sample_counts: SAMPLE_COUNTS,
        max_sample_mask_words: 1,
        timestamp_compute_and_graphics: true,
        // 1e6 over a 100 MHz `clock_crystal_freq`.
        timestamp_period: 10.0,
        max_clip_distances: 8,
        max_cull_distances: 8,
        point_size_range: [0.0, 8191.875],
        line_width_range: [0.0, 8.0],
        non_coherent_atom_size: 64,
        // `RADV_SUBGROUP_SIZE`; RDNA also runs 32-wide waves when asked.
        subgroup_size: 64,
        min_subgroup_size: 32,
        max_subgroup_size: 64,
        // Vertex and other geometry stages keep RADV's wave size.
        required_subgroup_size_stages: ShaderStages::COMPUTE
            .union(ShaderStages::FRAGMENT)
            .union(ShaderStages::TASK)
            .union(ShaderStages::MESH),
        subgroup_supported_stages: ShaderStages::KNOWN,
        subgroup_supported_operations: SubgroupFeatures::KNOWN,
        subgroup_quad_operations_in_all_stages: true,
        // `AC_MULTIVIEW_MAX_VIEWS`.
        max_multiview_view_count: 8,
        max_memory_allocation_size: MAX_ALLOCATION,
        max_buffer_size: MAX_ALLOCATION,
        storage_texel_buffer_offset_alignment_bytes: 4,
        storage_texel_buffer_offset_single_texel_alignment: true,
        uniform_texel_buffer_offset_alignment_bytes: 4,
        uniform_texel_buffer_offset_single_texel_alignment: true,
        supported_depth_resolve_modes: ResolveModes::KNOWN,
        supported_stencil_resolve_modes: ResolveModes::SAMPLE_ZERO
            | ResolveModes::MIN
            | ResolveModes::MAX,
        independent_resolve_none: true,
        independent_resolve: true,
        max_vertex_attrib_divisor: u32::MAX,
        sample_locations: Some(SampleLocationLimits {
            sample_counts: SAMPLE_COUNTS,
            max_grid_size: [2, 2],
            coordinate_range: [0.0, 0.9375],
            sub_pixel_bits: 4,
            variable_sample_locations: true,
        }),
        mesh: Some(MeshLimits {
            max_task_work_group_total_count: 1 << 22,
            max_task_work_group_count: [65535, 65535, 65535],
            max_task_work_group_invocations: 1024,
            max_task_payload_size: 1 << 14,
            max_mesh_work_group_total_count: 1 << 22,
            max_mesh_work_group_count: [65535, 65535, 65535],
            // The NGG hardware limit.
            max_mesh_work_group_invocations: 256,
            max_mesh_output_vertices: 256,
            max_mesh_output_primitives: 256,
        }),
        generated_commands: Some(GeneratedCommandsLimits {
            max_pipelines: 4096,
            max_sequences: 1 << 20,
            max_tokens: 128,
            max_token_offset: 2047,
            max_stride: 2048,
            // RADV switches only compute pipelines from an execution set.
            pipeline_stages: ShaderStages::COMPUTE,
        }),
        dma_buf_import: true,
        stencil_export: true,
        sparse: SparseLimits {
            residency_standard_2d_block_shape: true,
            residency_standard_2d_multisample_block_shape: false,
            residency_standard_3d_block_shape: true,
            residency_aligned_mip_size: false,
            residency_non_resident_strict: true,
        },
    }
}

fn features(generation: Generation) -> Features {
    let gfx11 = generation >= Generation::Rdna3;
    Features {
        image_cube_array: true,
        independent_blend: true,
        geometry_shader: true,
        tessellation_shader: true,
        sample_rate_shading: true,
        dual_src_blend: true,
        alpha_to_one: true,
        multi_draw_indirect: true,
        draw_indirect_first_instance: true,
        depth_clamp: true,
        depth_bias_clamp: true,
        fill_mode_non_solid: true,
        wide_lines: true,
        large_points: true,
        multi_viewport: true,
        sampler_anisotropy: true,
        texture_compression_bc: true,
        occlusion_query_precise: true,
        pipeline_statistics_query: true,
        vertex_pipeline_stores_and_atomics: true,
        fragment_stores_and_atomics: true,
        shader_image_gather_extended: true,
        shader_storage_image_multisample: true,
        shader_storage_image_read_without_format: true,
        shader_storage_image_write_without_format: true,
        shader_clip_distance: true,
        shader_cull_distance: true,
        shader_float64: true,
        shader_int64: true,
        shader_int16: true,
        shader_float16: true,
        shader_int8: true,
        shader_resource_residency: true,
        shader_resource_min_lod: true,
        sparse_binding: true,
        sparse_residency_buffer: true,
        sparse_residency_image_2d: true,
        sparse_residency_image_3d: true,
        // RADV sets none of `sparseResidency{2,4,8,16}Samples`.
        sparse_residency_samples: SampleCounts::EMPTY,
        sparse_residency_aliased: true,
        storage_buffer_8bit_access: true,
        storage_buffer_16bit_access: true,
        multiview: true,
        sampler_mirror_clamp_to_edge: true,
        sampler_filter_minmax: true,
        descriptor_indexing: true,
        runtime_descriptor_array: true,
        descriptor_binding_partially_bound: true,
        descriptor_binding_update_unused_while_pending: true,
        descriptor_binding_sampled_image_update_after_bind: true,
        descriptor_binding_storage_image_update_after_bind: true,
        descriptor_binding_uniform_texel_buffer_update_after_bind: true,
        descriptor_binding_storage_texel_buffer_update_after_bind: true,
        shader_sampled_image_array_non_uniform_indexing: true,
        shader_storage_image_array_non_uniform_indexing: true,
        buffer_device_address: true,
        timeline_semaphore: true,
        vulkan_memory_model: true,
        vulkan_memory_model_device_scope: true,
        shader_buffer_int64_atomics: true,
        shader_shared_int64_atomics: true,
        shader_image_int64_atomics: true,
        // Buffer float adds start at GFX11.
        shader_buffer_float32_atomic_add: gfx11,
        shader_shared_float32_atomic_add: true,
        shader_image_float32_atomics: true,
        shader_demote_to_helper_invocation: true,
        subgroup_size_control: true,
        compute_full_subgroups: true,
        synchronization2: true,
        dynamic_rendering: true,
        dynamic_rendering_local_read: true,
        maintenance4: true,
        maintenance5: true,
        maintenance8: true,
        // `radv_host_image_copy_enabled` from GFX10.
        host_image_copy: true,
        extended_dynamic_state: true,
        extended_dynamic_state2: true,
        extended_dynamic_state3_polygon_mode: true,
        extended_dynamic_state3_depth_clamp_enable: true,
        extended_dynamic_state3_depth_clip_enable: true,
        vertex_input_dynamic_state: true,
        vertex_attribute_instance_rate_divisor: true,
        depth_clip_enable: true,
        bresenham_lines: true,
        rectangular_lines: true,
        image_view_min_lod: true,
        // From GFX10.3.
        fragment_shader_barycentric: true,
        // `radv_taskmesh_enabled` from GFX10.3, with MEC firmware 100 or
        // later on GFX10.3.
        task_shader: true,
        mesh_shader: true,
        // Primitive Ordered Pixel Shading from GFX9 under ACO.
        fragment_shader_pixel_interlock: true,
        // `VK_KHR_unified_image_layouts` from GFX11.
        unified_image_layouts: gfx11,
        null_descriptor: true,
        dynamic_rendering_unused_attachments: true,
        dynamic_primitive_topology_unrestricted: false,
        host_query_reset: true,
        conditional_rendering: true,
        image_drm_format_modifier: true,
        // `VK_NV_shader_atomic_float16_vector` is NVK's alone.
        shader_float16_vector_atomics: false,
        memory_budget: true,
        calibrated_timestamps: true,
        // `radv_cooperative_matrix_enabled` from GFX11.
        cooperative_matrix: gfx11,
        // `radv_enable_rt` from GFX10.3.
        acceleration_structure: true,
        ray_query: true,
        ray_tracing_pipeline: true,
        ..Features::default()
    }
}

fn memory(vram: u64) -> Memory {
    Memory {
        heaps: vec![
            MemoryHeap {
                size: vram,
                flags: MemoryHeapFlags::DEVICE_LOCAL,
                budget: Some(vram - (256 << 20)),
            },
            MemoryHeap {
                size: 16 << 30,
                flags: MemoryHeapFlags::EMPTY,
                budget: Some(12 << 30),
            },
        ],
        types: vec![
            MemoryType {
                properties: M::DEVICE_LOCAL,
                heap_index: 0,
            },
            MemoryType {
                properties: M::HOST_VISIBLE | M::HOST_COHERENT,
                heap_index: 1,
            },
            // With Resizable BAR all of a card's VRAM is CPU-visible, and an
            // APU's carve-out always is.
            MemoryType {
                properties: M::DEVICE_LOCAL | M::HOST_VISIBLE | M::HOST_COHERENT,
                heap_index: 0,
            },
            MemoryType {
                properties: M::HOST_VISIBLE | M::HOST_COHERENT | M::HOST_CACHED,
                heap_index: 1,
            },
        ],
    }
}

fn radv_formats() -> FormatTable {
    let mut table = formats();
    // `radv_translate_dbformat` has no 24-bit depth.
    table.insert(Format::D24UnormS8Uint, FormatSupport::default());
    table
}
