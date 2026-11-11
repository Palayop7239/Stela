use super::{fixture::radv, *};
use crate::metal::{
    abi::GpuFamily,
    capabilities::{
        APPLE_SIMD_WIDTH, ApiFeatures, Capabilities, Cause, ShaderFeatures, apple_simd_width_runs,
        graphics_subgroups, required_subgroup_size,
    },
    nvidia::fixture::nvk,
    vulkan::{ShaderStages, backend::requirements, physical::PhysicalDevice},
};
use alloc::vec::Vec;

const NAVI23: GcVersion = GcVersion::new(10, 3, 4);
const NAVI31: GcVersion = GcVersion::new(11, 0, 0);
const NAVI48: GcVersion = GcVersion::new(12, 0, 1);
const PHOENIX: GcVersion = GcVersion::new(11, 0, 1);

/// What the NVIDIA route reaches on every Turing and later chip.
const NVIDIA_FAMILIES: [GpuFamily; 15] = [
    GpuFamily::Apple1,
    GpuFamily::Apple2,
    GpuFamily::Apple3,
    GpuFamily::Apple4,
    GpuFamily::Apple5,
    GpuFamily::Apple6,
    GpuFamily::Apple7,
    GpuFamily::Common1,
    GpuFamily::Common2,
    GpuFamily::Common3,
    GpuFamily::Mac1,
    GpuFamily::Mac2,
    GpuFamily::MacCatalyst1,
    GpuFamily::MacCatalyst2,
    GpuFamily::Metal3,
];

fn chip(gc: GcVersion) -> &'static Chip {
    Chip::find(gc).unwrap()
}

fn attachment() -> Attachment {
    Attachment {
        link: PcieLink {
            speed: 4,
            width: 16,
        },
        external: false,
        location_number: 0,
    }
}

fn derive(device: &PhysicalDevice, chip: &Chip) -> Capabilities {
    Capabilities::derive(
        device,
        &chip.platform(attachment()),
        ShaderFeatures::KNOWN,
        ApiFeatures::IMPLEMENTED,
    )
}

fn cause(capabilities: &Capabilities, name: &str) -> Option<Cause> {
    capabilities
        .gaps()
        .iter()
        .find(|gap| gap.capability == name)
        .map(|gap| gap.cause)
}

fn missing_requirements(device: &PhysicalDevice) -> Vec<&'static str> {
    requirements(&device.features)
        .iter()
        .filter(|requirement| !requirement.present)
        .map(|requirement| requirement.name)
        .collect()
}

#[test]
fn admission_requires_radv_on_an_rdna2_or_later_core() {
    let navi23 = chip(NAVI23);
    let identity = radv(navi23, 8 << 30).identity;
    assert_eq!(identity.device_id, 0x73ef, "an RX 6650 XT");
    assert_eq!(Chip::admit(&identity, NAVI23), Ok(navi23));

    let mut other = identity.clone();
    other.vendor_id = 0x10de;
    assert_eq!(
        Chip::admit(&other, NAVI23),
        Err(Refusal::NotAmd { vendor_id: 0x10de })
    );
    let mut other = identity.clone();
    other.driver_id = DriverId::MesaVenus;
    assert_eq!(
        Chip::admit(&other, NAVI23),
        Err(Refusal::NotRadv(DriverId::MesaVenus))
    );

    // RX 5700 (Navi 10), Vega 64 and an RX 580 (Polaris 10).
    for gc in [
        GcVersion::new(10, 1, 10),
        GcVersion::new(9, 0, 1),
        GcVersion::new(8, 0, 3),
    ] {
        assert_eq!(Chip::admit(&identity, gc), Err(Refusal::BeforeRdna2 { gc }));
    }
    // Instinct MI210 (Aldebaran).
    let mi210 = GcVersion::new(9, 4, 2);
    assert_eq!(
        Chip::admit(&identity, mi210),
        Err(Refusal::ComputeOnly { gc: mi210 })
    );
    for gc in [GcVersion::new(10, 3, 9), GcVersion::new(12, 1, 0)] {
        assert_eq!(Chip::admit(&identity, gc), Err(Refusal::UnknownCore { gc }));
    }

    let mut integrated = identity;
    integrated.device_type = PhysicalDeviceType::IntegratedGpu;
    assert_eq!(
        Chip::admit(&integrated, NAVI23),
        Err(Refusal::Placement {
            reported: PhysicalDeviceType::IntegratedGpu
        })
    );
    let phoenix = chip(PHOENIX);
    assert_eq!(
        Chip::admit(&radv(phoenix, 2 << 30).identity, PHOENIX),
        Ok(phoenix)
    );
}

#[test]
fn platform_reports_placement_and_radv_layouts() {
    let card = chip(NAVI23).platform(attachment());
    assert_eq!(card.architecture, "Navi 23");
    assert_eq!(card.location, DeviceLocation::Slot);
    assert_eq!(card.max_transfer_rate, 31_506_000_000);
    assert!(!card.headless && !card.removable);
    assert_eq!(
        (card.linear_row_alignment, card.linear_offset_alignment),
        (256, 256)
    );
    assert_eq!(card.sparse_page_bytes, 64 << 10);

    let egpu = chip(NAVI31).platform(Attachment {
        external: true,
        location_number: 2,
        ..attachment()
    });
    assert_eq!(
        (egpu.location, egpu.location_number, egpu.removable),
        (DeviceLocation::External, 2, true)
    );

    // GFX12 halves the linear pitch alignment.
    assert_eq!(
        chip(NAVI48).platform(attachment()).linear_row_alignment,
        128
    );

    let apu = chip(PHOENIX).platform(attachment());
    assert_eq!(
        (apu.location, apu.location_number, apu.max_transfer_rate),
        (DeviceLocation::BuiltIn, 0, 0)
    );
    assert!(!apu.removable);
}

#[test]
fn the_backend_refuses_radv_for_two_features_today() {
    for chip in &CHIPS {
        let missing = missing_requirements(&radv(chip, 8 << 30));
        // RADV's NGG shaders fix their primitive type, so it never reports
        // `dynamicPrimitiveTopologyUnrestricted`; it reports
        // `VK_KHR_unified_image_layouts` from GFX11.
        let expected: &[&str] = if chip.generation() == Generation::Rdna2 {
            &[
                "unifiedImageLayouts",
                "dynamicPrimitiveTopologyUnrestricted",
            ]
        } else {
            &["dynamicPrimitiveTopologyUnrestricted"]
        };
        assert_eq!(missing, expected, "{}", chip.name);
    }
    assert!(missing_requirements(&nvk(&crate::metal::nvidia::CHIPS[0], 8 << 30)).is_empty());
}

#[test]
fn rdna2_and_later_stop_at_common2_without_indirect_render_pipeline_switching() {
    for chip in &CHIPS {
        let capabilities = derive(&radv(chip, 8 << 30), chip);
        assert_eq!(
            capabilities.families(),
            [GpuFamily::Common1, GpuFamily::Common2],
            "{}",
            chip.name
        );
        // RADV's execution sets hold only compute pipelines.
        assert_eq!(
            cause(&capabilities, "indirect render pipeline switching"),
            Some(Cause::Driver)
        );
        assert_eq!(
            cause(&capabilities, "GPU family Common3"),
            Some(Cause::Driver)
        );
        assert_eq!(capabilities.limits.thread_execution_width, 32);
        assert_eq!(capabilities.has_unified_memory, chip.integrated);
    }
}

#[test]
fn with_indirect_render_pipelines_every_core_matches_the_nvidia_profile() {
    for chip in &CHIPS {
        let mut device = radv(chip, 8 << 30);
        device
            .limits
            .generated_commands
            .as_mut()
            .unwrap()
            .pipeline_stages = ShaderStages::KNOWN.difference(ShaderStages::GEOMETRY);
        let capabilities = derive(&device, chip);
        assert_eq!(capabilities.families(), NVIDIA_FAMILIES, "{}", chip.name);
        assert_eq!(cause(&capabilities, "GPU family Metal3"), None);
        assert_eq!(cause(&capabilities, "raster order groups"), None);
        assert_eq!(cause(&capabilities, "32-wide SIMD groups"), None);
        // What RADV still lacks that NVK has, none of which a family needs.
        assert_eq!(
            cause(&capabilities, "Depth24Unorm_Stencil8"),
            Some(Cause::Driver)
        );
        assert_eq!(
            cause(&capabilities, "sparse multisample textures"),
            Some(Cause::Driver)
        );
        assert!(!capabilities.depth24_stencil8_supported);
        assert!(capabilities.stencil_export);
        assert!(capabilities.supports_mesh_shaders);
    }
}

#[test]
fn radv_runs_metal_at_apples_simd_width() {
    let device = radv(chip(NAVI23), 8 << 30);
    assert!(apple_simd_width_runs(&device));
    assert_eq!(required_subgroup_size(&device), APPLE_SIMD_WIDTH);
    assert_eq!(
        graphics_subgroups(&device),
        (
            32,
            ShaderStages::COMPUTE
                | ShaderStages::FRAGMENT
                | ShaderStages::TASK
                | ShaderStages::MESH
        )
    );

    // NVK's subgroups are 32 wide already: nothing to require.
    let nvk = nvk(&crate::metal::nvidia::CHIPS[0], 8 << 30);
    assert!(apple_simd_width_runs(&nvk));
    assert_eq!(required_subgroup_size(&nvk), 0);
    assert_eq!(graphics_subgroups(&nvk), (0, ShaderStages::EMPTY));

    // A wave64-only core, as GFX9 is, keeps its own width and says so.
    let mut wave64 = device.clone();
    wave64.limits.min_subgroup_size = 64;
    let capabilities = derive(&wave64, chip(NAVI23));
    assert_eq!(capabilities.limits.thread_execution_width, 64);
    assert_eq!(
        cause(&capabilities, "32-wide SIMD groups"),
        Some(Cause::Driver)
    );
    assert_eq!(required_subgroup_size(&wave64), 0);

    // A range without subgroup size control cannot be pinned.
    let mut uncontrolled = device;
    uncontrolled.features.subgroup_size_control = false;
    assert!(!apple_simd_width_runs(&uncontrolled));
    assert_eq!(
        derive(&uncontrolled, chip(NAVI23))
            .limits
            .thread_execution_width,
        64
    );
}
