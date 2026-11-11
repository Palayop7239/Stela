//! amdgpu: the `amdgpu-y` composite of its own makefiles, compiled unchanged
//! in Stela's Linux environment for headless RDNA bring-up, and the kernel
//! services it imports that the environment does not yet supply.
//!
//! Survey state: the composite is compiled and its imports checked, but it is
//! not linked into the image and nothing registers it. Each source compiles
//! on its own, so the ones that do not build yet are listed together in
//! `amdgpu/compile-errors.txt` under the build's output directory instead of
//! stopping at the first. The services the compiled objects import that
//! nothing supplies, with the objects that import each, are in
//! `amdgpu/open-imports.txt`.
use super::native::{self, Linux};
use super::nouveau::{HARNESS, PLATFORM, RUNTIME, members, traps};
use object::{Object, ObjectSymbol};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    thread,
};
use stela_c_build::linux::Module;

const AMD: &str = "../../third_party/linux/drivers/gpu/drm/amd";
const MODULE: Module<'static> = Module {
    name: "amdgpu",
    file: "drivers/gpu/drm/amd/amdgpu/amdgpu",
};

/// Below `drivers/gpu/drm`: the `ccflags-y` of amdgpu's Makefile, then the
/// `subdir-ccflags-y` its pm and ras makefiles add, in their order.
const INCLUDES: &[&str] = &[
    "amd/include/asic_reg",
    "amd/include",
    "amd/amdgpu",
    "amd/pm/inc",
    "amd/acp/include",
    "amd/display",
    "amd/display/include",
    "amd/display/modules/inc",
    "amd/display/dc",
    "amd/display/amdgpu_dm",
    "amd/amdkfd",
    "amd/ras/ras_mgr",
    "amd/pm/swsmu",
    "amd/pm/swsmu/inc",
    "amd/pm/swsmu/inc/pmfw_if",
    "amd/pm/swsmu/smu11",
    "amd/pm/swsmu/smu12",
    "amd/pm/swsmu/smu13",
    "amd/pm/swsmu/smu14",
    "amd/pm/swsmu/smu15",
    "amd/pm/powerplay/inc",
    "amd/pm/powerplay/smumgr",
    "amd/pm/powerplay/hwmgr",
    "amd/pm/legacy-dpm",
    "amd/ras/rascore",
];

/// `amdgpu-y` as amdgpu's makefiles compose it with `CONFIG_DRM_AMDGPU=y` and
/// no optional symbol: no display core (`DRM_AMD_DC`), compute driver
/// (`HSA_AMD`), SI or CIK parts, ACP or ISP blocks, and none of the objects
/// `ACPI`, `PROC_FS`, `PERF_EVENTS`, `COMPAT`, `VGA_SWITCHEROO` or
/// `HMM_MIRROR` add. Paths below `drivers/gpu/drm/amd`, without `.o`. The
/// makefiles set no per-object flags for these.
const OBJECTS: &[&str] = &[
    "amdgpu/amdgpu_drv",
    "amdgpu/amdgpu_device",
    "amdgpu/amdgpu_reg_access",
    "amdgpu/amdgpu_doorbell_mgr",
    "amdgpu/amdgpu_kms",
    "amdgpu/amdgpu_atombios",
    "amdgpu/atombios_crtc",
    "amdgpu/amdgpu_connectors",
    "amdgpu/atom",
    "amdgpu/amdgpu_fence",
    "amdgpu/amdgpu_ttm",
    "amdgpu/amdgpu_object",
    "amdgpu/amdgpu_gart",
    "amdgpu/amdgpu_encoders",
    "amdgpu/amdgpu_display",
    "amdgpu/amdgpu_i2c",
    "amdgpu/amdgpu_gem",
    "amdgpu/amdgpu_ring",
    "amdgpu/amdgpu_cs",
    "amdgpu/amdgpu_bios",
    "amdgpu/amdgpu_benchmark",
    "amdgpu/atombios_dp",
    "amdgpu/amdgpu_afmt",
    "amdgpu/amdgpu_trace_points",
    "amdgpu/atombios_encoders",
    "amdgpu/amdgpu_sa",
    "amdgpu/atombios_i2c",
    "amdgpu/amdgpu_dma_buf",
    "amdgpu/amdgpu_vm",
    "amdgpu/amdgpu_vm_pt",
    "amdgpu/amdgpu_vm_tlb_fence",
    "amdgpu/amdgpu_ib",
    "amdgpu/amdgpu_pll",
    "amdgpu/amdgpu_ucode",
    "amdgpu/amdgpu_bo_list",
    "amdgpu/amdgpu_ctx",
    "amdgpu/amdgpu_sync",
    "amdgpu/amdgpu_gtt_mgr",
    "amdgpu/amdgpu_preempt_mgr",
    "amdgpu/amdgpu_vram_mgr",
    "amdgpu/amdgpu_virt",
    "amdgpu/amdgpu_atomfirmware",
    "amdgpu/amdgpu_vf_error",
    "amdgpu/amdgpu_sched",
    "amdgpu/amdgpu_debugfs",
    "amdgpu/amdgpu_ids",
    "amdgpu/amdgpu_gmc",
    "amdgpu/amdgpu_mmhub",
    "amdgpu/amdgpu_hdp",
    "amdgpu/amdgpu_xgmi",
    "amdgpu/amdgpu_csa",
    "amdgpu/amdgpu_ras",
    "amdgpu/amdgpu_vm_cpu",
    "amdgpu/amdgpu_vm_sdma",
    "amdgpu/amdgpu_discovery",
    "amdgpu/amdgpu_ras_eeprom",
    "amdgpu/amdgpu_nbio",
    "amdgpu/amdgpu_umc",
    "amdgpu/smu_v11_0_i2c",
    "amdgpu/amdgpu_fru_eeprom",
    "amdgpu/amdgpu_rap",
    "amdgpu/amdgpu_fw_attestation",
    "amdgpu/amdgpu_securedisplay",
    "amdgpu/amdgpu_eeprom",
    "amdgpu/amdgpu_mca",
    "amdgpu/amdgpu_psp_ta",
    "amdgpu/amdgpu_lsdma",
    "amdgpu/amdgpu_lockdep",
    "amdgpu/amdgpu_ring_mux",
    "amdgpu/amdgpu_xcp",
    "amdgpu/amdgpu_seq64",
    "amdgpu/amdgpu_aca",
    "amdgpu/amdgpu_dev_coredump",
    "amdgpu/amdgpu_cper",
    "amdgpu/amdgpu_userq_fence",
    "amdgpu/amdgpu_eviction_fence",
    "amdgpu/amdgpu_ip",
    "amdgpu/vi",
    "amdgpu/mxgpu_vi",
    "amdgpu/nbio_v6_1",
    "amdgpu/soc15",
    "amdgpu/emu_soc",
    "amdgpu/mxgpu_ai",
    "amdgpu/nbio_v7_0",
    "amdgpu/vega10_reg_init",
    "amdgpu/vega20_reg_init",
    "amdgpu/nbio_v7_4",
    "amdgpu/nbio_v2_3",
    "amdgpu/nv",
    "amdgpu/arct_reg_init",
    "amdgpu/mxgpu_nv",
    "amdgpu/nbio_v7_2",
    "amdgpu/hdp_v4_0",
    "amdgpu/hdp_v5_0",
    "amdgpu/aldebaran_reg_init",
    "amdgpu/aldebaran",
    "amdgpu/soc21",
    "amdgpu/soc24",
    "amdgpu/sienna_cichlid",
    "amdgpu/smu_v13_0_10",
    "amdgpu/nbio_v4_3",
    "amdgpu/hdp_v6_0",
    "amdgpu/nbio_v7_7",
    "amdgpu/hdp_v5_2",
    "amdgpu/lsdma_v6_0",
    "amdgpu/nbio_v7_9",
    "amdgpu/aqua_vanjaram",
    "amdgpu/nbio_v7_11",
    "amdgpu/lsdma_v7_0",
    "amdgpu/hdp_v7_0",
    "amdgpu/nbif_v6_3_1",
    "amdgpu/cyan_skillfish_reg_init",
    "amdgpu/soc_v1_0",
    "amdgpu/lsdma_v7_1",
    "amdgpu/nbio_v6_3_2",
    "amdgpu/df_v1_7",
    "amdgpu/df_v3_6",
    "amdgpu/df_v4_3",
    "amdgpu/df_v4_6_2",
    "amdgpu/df_v4_15",
    "amdgpu/gmc_v7_0",
    "amdgpu/gmc_v8_0",
    "amdgpu/gfxhub_v1_0",
    "amdgpu/mmhub_v1_0",
    "amdgpu/gmc_v9_0",
    "amdgpu/gfxhub_v1_1",
    "amdgpu/mmhub_v9_4",
    "amdgpu/gfxhub_v2_0",
    "amdgpu/mmhub_v2_0",
    "amdgpu/gmc_v10_0",
    "amdgpu/gfxhub_v2_1",
    "amdgpu/mmhub_v2_3",
    "amdgpu/mmhub_v1_7",
    "amdgpu/gfxhub_v3_0",
    "amdgpu/mmhub_v3_0",
    "amdgpu/mmhub_v3_0_2",
    "amdgpu/gmc_v11_0",
    "amdgpu/mmhub_v3_0_1",
    "amdgpu/gfxhub_v3_0_3",
    "amdgpu/gfxhub_v1_2",
    "amdgpu/mmhub_v1_8",
    "amdgpu/mmhub_v3_3",
    "amdgpu/gfxhub_v11_5_0",
    "amdgpu/mmhub_v4_1_0",
    "amdgpu/gfxhub_v12_0",
    "amdgpu/gmc_v12_0",
    "amdgpu/mmhub_v4_2_0",
    "amdgpu/gfxhub_v12_1",
    "amdgpu/gmc_v12_1",
    "amdgpu/umc_v6_0",
    "amdgpu/umc_v6_1",
    "amdgpu/umc_v6_7",
    "amdgpu/umc_v8_7",
    "amdgpu/umc_v8_10",
    "amdgpu/umc_v12_0",
    "amdgpu/umc_v8_14",
    "amdgpu/amdgpu_irq",
    "amdgpu/amdgpu_ih",
    "amdgpu/iceland_ih",
    "amdgpu/tonga_ih",
    "amdgpu/cz_ih",
    "amdgpu/vega10_ih",
    "amdgpu/vega20_ih",
    "amdgpu/navi10_ih",
    "amdgpu/ih_v6_0",
    "amdgpu/ih_v6_1",
    "amdgpu/ih_v7_0",
    "amdgpu/amdgpu_psp",
    "amdgpu/psp_v3_1",
    "amdgpu/psp_v10_0",
    "amdgpu/psp_v11_0",
    "amdgpu/psp_v11_0_8",
    "amdgpu/psp_v12_0",
    "amdgpu/psp_v13_0",
    "amdgpu/psp_v13_0_4",
    "amdgpu/psp_v14_0",
    "amdgpu/psp_v15_0",
    "amdgpu/psp_v15_0_8",
    "amdgpu/dce_v10_0",
    "amdgpu/amdgpu_vkms",
    "amdgpu/amdgpu_gfx",
    "amdgpu/amdgpu_rlc",
    "amdgpu/gfx_v8_0",
    "amdgpu/gfx_v9_0",
    "amdgpu/gfx_v9_4",
    "amdgpu/gfx_v9_4_2",
    "amdgpu/gfx_v9_4_3",
    "amdgpu/gfx_v10_0",
    "amdgpu/imu_v11_0",
    "amdgpu/gfx_v11_0",
    "amdgpu/gfx_v11_0_3",
    "amdgpu/imu_v11_0_3",
    "amdgpu/gfx_v12_0",
    "amdgpu/gfx_v12_1",
    "amdgpu/imu_v12_0",
    "amdgpu/imu_v12_1",
    "amdgpu/amdgpu_sdma",
    "amdgpu/sdma_v2_4",
    "amdgpu/sdma_v3_0",
    "amdgpu/sdma_v4_0",
    "amdgpu/sdma_v4_4",
    "amdgpu/sdma_v4_4_2",
    "amdgpu/sdma_v5_0",
    "amdgpu/sdma_v5_2",
    "amdgpu/sdma_v6_0",
    "amdgpu/sdma_v7_0",
    "amdgpu/sdma_v7_1",
    "amdgpu/amdgpu_mes",
    "amdgpu/mes_v11_0",
    "amdgpu/mes_v12_0",
    "amdgpu/mes_v12_1",
    "amdgpu/mes_userqueue",
    "amdgpu/amdgpu_uvd",
    "amdgpu/uvd_v5_0",
    "amdgpu/uvd_v6_0",
    "amdgpu/uvd_v7_0",
    "amdgpu/amdgpu_vce",
    "amdgpu/vce_v3_0",
    "amdgpu/vce_v4_0",
    "amdgpu/amdgpu_vcn",
    "amdgpu/vcn_sw_ring",
    "amdgpu/vcn_v1_0",
    "amdgpu/vcn_v2_0",
    "amdgpu/vcn_v2_5",
    "amdgpu/vcn_v3_0",
    "amdgpu/vcn_v4_0",
    "amdgpu/vcn_v4_0_3",
    "amdgpu/vcn_v4_0_5",
    "amdgpu/vcn_v5_0_0",
    "amdgpu/vcn_v5_0_1",
    "amdgpu/vcn_v5_0_2",
    "amdgpu/amdgpu_jpeg",
    "amdgpu/jpeg_v1_0",
    "amdgpu/jpeg_v2_0",
    "amdgpu/jpeg_v2_5",
    "amdgpu/jpeg_v3_0",
    "amdgpu/jpeg_v4_0",
    "amdgpu/jpeg_v4_0_3",
    "amdgpu/jpeg_v4_0_5",
    "amdgpu/jpeg_v5_0_0",
    "amdgpu/jpeg_v5_0_1",
    "amdgpu/jpeg_v5_0_2",
    "amdgpu/jpeg_v5_3_0",
    "amdgpu/amdgpu_vpe",
    "amdgpu/vpe_v6_1",
    "amdgpu/vpe_v2_0",
    "amdgpu/amdgpu_umsch_mm",
    "amdgpu/umsch_mm_v4_0",
    "amdgpu/athub_v1_0",
    "amdgpu/athub_v2_0",
    "amdgpu/athub_v2_1",
    "amdgpu/athub_v3_0",
    "amdgpu/athub_v4_1_0",
    "amdgpu/smuio_v9_0",
    "amdgpu/smuio_v11_0",
    "amdgpu/smuio_v11_0_6",
    "amdgpu/smuio_v13_0",
    "amdgpu/smuio_v13_0_3",
    "amdgpu/smuio_v13_0_6",
    "amdgpu/smuio_v14_0_2",
    "amdgpu/smuio_v15_0_0",
    "amdgpu/smuio_v15_0_8",
    "amdgpu/amdgpu_reset",
    "amdgpu/mca_v3_0",
    "amdgpu/amdgpu_amdkfd",
    "amdgpu/amdgpu_userq",
    "amdgpu/amdgpu_cgs",
    "amdgpu/amdgpu_job",
    "pm/swsmu/smu11/arcturus_ppt",
    "pm/swsmu/smu11/navi10_ppt",
    "pm/swsmu/smu11/sienna_cichlid_ppt",
    "pm/swsmu/smu11/vangogh_ppt",
    "pm/swsmu/smu11/cyan_skillfish_ppt",
    "pm/swsmu/smu11/smu_v11_0",
    "pm/swsmu/smu12/renoir_ppt",
    "pm/swsmu/smu12/smu_v12_0",
    "pm/swsmu/smu13/smu_v13_0",
    "pm/swsmu/smu13/aldebaran_ppt",
    "pm/swsmu/smu13/yellow_carp_ppt",
    "pm/swsmu/smu13/smu_v13_0_0_ppt",
    "pm/swsmu/smu13/smu_v13_0_4_ppt",
    "pm/swsmu/smu13/smu_v13_0_5_ppt",
    "pm/swsmu/smu13/smu_v13_0_7_ppt",
    "pm/swsmu/smu13/smu_v13_0_6_ppt",
    "pm/swsmu/smu13/smu_v13_0_12_ppt",
    "pm/swsmu/smu14/smu_v14_0",
    "pm/swsmu/smu14/smu_v14_0_0_ppt",
    "pm/swsmu/smu14/smu_v14_0_2_ppt",
    "pm/swsmu/smu15/smu_v15_0",
    "pm/swsmu/smu15/smu_v15_0_0_ppt",
    "pm/swsmu/smu15/smu_v15_0_8_ppt",
    "pm/swsmu/amdgpu_smu",
    "pm/swsmu/smu_cmn",
    "pm/powerplay/smumgr/smumgr",
    "pm/powerplay/smumgr/smu8_smumgr",
    "pm/powerplay/smumgr/tonga_smumgr",
    "pm/powerplay/smumgr/fiji_smumgr",
    "pm/powerplay/smumgr/polaris10_smumgr",
    "pm/powerplay/smumgr/iceland_smumgr",
    "pm/powerplay/smumgr/smu7_smumgr",
    "pm/powerplay/smumgr/vega10_smumgr",
    "pm/powerplay/smumgr/smu10_smumgr",
    "pm/powerplay/smumgr/ci_smumgr",
    "pm/powerplay/smumgr/vega12_smumgr",
    "pm/powerplay/smumgr/vegam_smumgr",
    "pm/powerplay/smumgr/smu9_smumgr",
    "pm/powerplay/smumgr/vega20_smumgr",
    "pm/powerplay/hwmgr/hwmgr",
    "pm/powerplay/hwmgr/processpptables",
    "pm/powerplay/hwmgr/hardwaremanager",
    "pm/powerplay/hwmgr/smu8_hwmgr",
    "pm/powerplay/hwmgr/pppcielanes",
    "pm/powerplay/hwmgr/process_pptables_v1_0",
    "pm/powerplay/hwmgr/ppatomctrl",
    "pm/powerplay/hwmgr/ppatomfwctrl",
    "pm/powerplay/hwmgr/smu7_hwmgr",
    "pm/powerplay/hwmgr/smu7_powertune",
    "pm/powerplay/hwmgr/smu7_thermal",
    "pm/powerplay/hwmgr/smu7_clockpowergating",
    "pm/powerplay/hwmgr/vega10_processpptables",
    "pm/powerplay/hwmgr/vega10_hwmgr",
    "pm/powerplay/hwmgr/vega10_powertune",
    "pm/powerplay/hwmgr/vega10_thermal",
    "pm/powerplay/hwmgr/smu10_hwmgr",
    "pm/powerplay/hwmgr/pp_psm",
    "pm/powerplay/hwmgr/vega12_processpptables",
    "pm/powerplay/hwmgr/vega12_hwmgr",
    "pm/powerplay/hwmgr/vega12_thermal",
    "pm/powerplay/hwmgr/pp_overdriver",
    "pm/powerplay/hwmgr/smu_helper",
    "pm/powerplay/hwmgr/vega20_processpptables",
    "pm/powerplay/hwmgr/vega20_hwmgr",
    "pm/powerplay/hwmgr/vega20_powertune",
    "pm/powerplay/hwmgr/vega20_thermal",
    "pm/powerplay/hwmgr/common_baco",
    "pm/powerplay/hwmgr/vega10_baco",
    "pm/powerplay/hwmgr/vega20_baco",
    "pm/powerplay/hwmgr/vega12_baco",
    "pm/powerplay/hwmgr/smu9_baco",
    "pm/powerplay/hwmgr/tonga_baco",
    "pm/powerplay/hwmgr/polaris_baco",
    "pm/powerplay/hwmgr/fiji_baco",
    "pm/powerplay/hwmgr/ci_baco",
    "pm/powerplay/hwmgr/smu7_baco",
    "pm/powerplay/amd_powerplay",
    "pm/legacy-dpm/legacy_dpm",
    "pm/amdgpu_dpm",
    "pm/amdgpu_pm",
    "pm/amdgpu_dpm_internal",
    "ras/ras_mgr/amdgpu_ras_sys",
    "ras/ras_mgr/amdgpu_ras_mgr",
    "ras/ras_mgr/amdgpu_ras_eeprom_i2c",
    "ras/ras_mgr/amdgpu_ras_mp1_v13_0",
    "ras/ras_mgr/amdgpu_ras_cmd",
    "ras/ras_mgr/amdgpu_virt_ras_cmd",
    "ras/ras_mgr/amdgpu_ras_process",
    "ras/ras_mgr/amdgpu_ras_nbio_v7_9",
    "ras/rascore/ras_core",
    "ras/rascore/ras_mp1",
    "ras/rascore/ras_mp1_v13_0",
    "ras/rascore/ras_aca",
    "ras/rascore/ras_aca_v1_0",
    "ras/rascore/ras_eeprom",
    "ras/rascore/ras_umc",
    "ras/rascore/ras_umc_v12_0",
    "ras/rascore/ras_cmd",
    "ras/rascore/ras_gfx",
    "ras/rascore/ras_gfx_v9_0",
    "ras/rascore/ras_process",
    "ras/rascore/ras_nbio",
    "ras/rascore/ras_nbio_v7_9",
    "ras/rascore/ras_log_ring",
    "ras/rascore/ras_cper",
    "ras/rascore/ras_psp",
    "ras/rascore/ras_psp_v13_0",
    "ras/rascore/ras_eeprom_fw",
];

pub fn port(linux: &Linux) {
    let mut build = stela_c_build::native::configure();
    linux.environment.apply(
        &mut build,
        None,
        &native::drm_includes(INCLUDES),
        Some(&MODULE),
    );
    stela_c_build::linux_build::upstream_warnings(&mut build);
    build
        // Linux's top-level Makefile builds every file as GNU C with clang's
        // GNU-extension warnings off (`CC_FLAGS_DIALECT += -Wno-gnu`): the
        // SR-IOV message header nests a flexible union mid-struct.
        .flag("-Wno-gnu")
        // amdgpu's Makefile turns this W=1 warning off for its own tree.
        .flag("-Wno-override-init")
        // Linux builds without -Werror; amdgpu_vm.c prints u64s with `%Lx`.
        .flag("-Wno-error=format")
        .opt_level(2)
        .cargo_metadata(false);
    let compiler = build.get_compiler();
    let output = stela_c_build::linux_build::out_dir().join("amdgpu");
    let units = OBJECTS
        .iter()
        .map(|object| {
            let source = Path::new(AMD).join(object).with_extension("c");
            println!("cargo:rerun-if-changed={}", source.display());
            (source, output.join(object).with_extension("o"))
        })
        .collect::<Vec<_>>();
    let jobs = env::var("NUM_JOBS")
        .ok()
        .and_then(|jobs| jobs.parse().ok())
        .unwrap_or(1)
        .max(1);
    let results = thread::scope(|scope| {
        units
            .chunks(units.len().div_ceil(jobs))
            .map(|chunk| {
                let compiler = &compiler;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(source, object)| compile(compiler, source, object))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|compiling| compiling.join().expect("a compiling thread"))
            .collect::<Vec<_>>()
    });
    let mut objects = Vec::new();
    let mut errors = String::new();
    for ((source, _), result) in units.iter().zip(results) {
        match result {
            Ok(object) => objects.push(object),
            Err(error) => {
                let _ = writeln!(errors, "== {}\n{error}", source.display());
            }
        }
    }
    let failed = units.len() - objects.len();
    fs::write(output.join("compile-errors.txt"), &errors).expect("write the compile report");
    let archive = build.create_archive("stela-amdgpu", &objects);
    let open = open_imports(&objects, &[&linux.archive, &linux.common_archive]);
    let mut report = String::new();
    for (symbol, importers) in &open {
        let _ = writeln!(
            report,
            "{symbol}: {}",
            importers.iter().cloned().collect::<Vec<_>>().join(" ")
        );
    }
    fs::write(output.join("open-imports.txt"), &report).expect("write the import report");
    println!(
        "cargo:warning=amdgpu survey: {} of {} sources compiled, {failed} not yet ({}); \
         {} imported services unsupplied ({}); archive {}",
        objects.len(),
        units.len(),
        output.join("compile-errors.txt").display(),
        open.len(),
        output.join("open-imports.txt").display(),
        archive.display()
    );
}

/// One source with the configured compiler; the compiler's report when it
/// fails.
fn compile(compiler: &cc::Tool, source: &Path, object: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(object.parent().expect("an object directory"))
        .expect("create an object directory");
    let result = compiler
        .to_command()
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(object)
        .output()
        .expect("run the C compiler");
    if result.status.success() {
        Ok(object.to_owned())
    } else {
        Err(String::from_utf8_lossy(&result.stderr).into_owned())
    }
}

/// The services `objects` reach when linked, directly or through the
/// environment's archive members, that nothing supplies or that the
/// environment only traps, with the objects that import each directly (or
/// `(environment)` when only an environment member pulled in reaches it).
fn open_imports(objects: &[PathBuf], environment: &[&Path]) -> BTreeMap<String, BTreeSet<String>> {
    let mut defined = BTreeSet::new();
    let mut importers = BTreeMap::<String, BTreeSet<String>>::new();
    for object in objects {
        let data = fs::read(object).unwrap_or_else(|error| panic!("{}: {error}", object.display()));
        let file = object::File::parse(&*data).expect("an object file");
        let name = object
            .strip_prefix(stela_c_build::linux_build::out_dir().join("amdgpu"))
            .unwrap_or(object)
            .display()
            .to_string();
        for symbol in file.symbols() {
            let Ok(symbol_name) = symbol.name() else {
                continue;
            };
            if symbol.is_undefined() {
                importers
                    .entry(symbol_name.to_owned())
                    .or_default()
                    .insert(name.clone());
            } else if symbol.is_global() {
                defined.insert(symbol_name.to_owned());
            }
        }
    }
    let environment = environment
        .iter()
        .flat_map(|archive| members(archive))
        .collect::<Vec<_>>();
    let definers = environment
        .iter()
        .flat_map(|member| member.defined.iter().map(move |symbol| (symbol, member)))
        .collect::<BTreeMap<_, _>>();
    let traps = traps();
    let harness = env::var_os("DEP_STELA_LINUX_HOSTED_FIXTURES").is_some();
    let mut reached = importers
        .keys()
        .filter(|symbol| !defined.contains(*symbol))
        .cloned()
        .collect::<Vec<_>>();
    let mut seen = reached.iter().cloned().collect::<BTreeSet<_>>();
    let mut open = BTreeMap::new();
    while let Some(symbol) = reached.pop() {
        let direct = || {
            importers
                .get(&symbol)
                .cloned()
                .unwrap_or_else(|| BTreeSet::from(["(environment)".to_owned()]))
        };
        if traps.contains(&symbol) {
            open.insert(symbol.clone(), direct());
        }
        match definers.get(&symbol) {
            Some(member) => {
                for import in &member.imported {
                    if !defined.contains(import) && seen.insert(import.clone()) {
                        reached.push(import.clone());
                    }
                }
            }
            None if RUNTIME.contains(&symbol.as_str())
                || symbol.starts_with(PLATFORM)
                || (harness && symbol.starts_with(HARNESS)) => {}
            None => {
                open.insert(symbol.clone(), direct());
            }
        }
    }
    open
}
