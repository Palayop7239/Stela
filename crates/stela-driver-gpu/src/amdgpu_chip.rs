//! AMD graphics core identity shared by boot and the RADV device path.
//!
//! From GFX10 on, amdgpu identifies a GPU by the IP versions in the
//! discovery table it reads from VRAM, not by PCI device ID, and reports the
//! graphics core's version to user space as `drm_amdgpu_info_hw_ip`'s
//! `ip_discovery_version` for `AMDGPU_HW_IP_GFX`. The table follows the
//! graphics core family and `AMD_IS_APU` switches of
//! `amdgpu_discovery_set_ip_blocks` in
//! `drivers/gpu/drm/amd/amdgpu/amdgpu_discovery.c` (Linux v7.2.12), for the
//! RDNA2 and later cores Mesa's RADV runs Metal 3 on. Firmware prefixes
//! follow `amdgpu_ucode_ip_version_decode` in `amdgpu_ucode.c`: the legacy
//! code name where `amdgpu_ucode_legacy_naming` has one, else `gc_` and the
//! version.

/// A graphics core IP version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GcVersion {
    pub major: u8,
    pub minor: u8,
    pub revision: u8,
}

impl GcVersion {
    pub const fn new(major: u8, minor: u8, revision: u8) -> Self {
        Self {
            major,
            minor,
            revision,
        }
    }

    /// Decode `ip_discovery_version`: bits 23:16 major, 15:8 minor and
    /// 7:0 revision.
    pub const fn from_ip_discovery(version: u32) -> Self {
        Self::new((version >> 16) as u8, (version >> 8) as u8, version as u8)
    }

    /// The generation Mesa's `amd_gfx_level` assigns this core.
    pub const fn generation(self) -> Generation {
        match (self.major, self.minor) {
            (0..=8, _) => Generation::Gcn,
            // GC 9.4.1 and later 9.x are the compute-only CDNA parts.
            (9, 4) if self.revision >= 1 => Generation::Cdna,
            (9, 5..) => Generation::Cdna,
            (9, _) => Generation::Vega,
            (10, 0..=2) => Generation::Rdna1,
            (10, 3) => Generation::Rdna2,
            (11, 0) => Generation::Rdna3,
            (11, 5) => Generation::Rdna3_5,
            (12, 0) => Generation::Rdna4,
            _ => Generation::Unknown,
        }
    }
}

impl core::fmt::Display for GcVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.revision)
    }
}

/// The graphics generation, as RADV gates its features.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Generation {
    /// GFX6 to GFX8.
    Gcn,
    /// GFX9 with graphics: Vega, Raven, Renoir.
    Vega,
    /// GFX9 compute-only accelerators without a graphics pipeline.
    Cdna,
    /// GFX10: RX 5000.
    Rdna1,
    /// GFX10.3: RX 6000, Van Gogh, Rembrandt. RADV's mesh and task shaders
    /// and fragment barycentrics start here.
    Rdna2,
    /// GFX11: RX 7000, Phoenix.
    Rdna3,
    /// GFX11.5: Strix.
    Rdna3_5,
    /// GFX12: RX 9000.
    Rdna4,
    /// A core newer than this table, or one ACO does not compile for.
    Unknown,
}

impl Generation {
    /// RADV exposes `VK_EXT_mesh_shader` and
    /// `VK_KHR_fragment_shader_barycentric` from GFX10.3 on, which Metal 3
    /// requires.
    pub const fn runs_metal3(self) -> bool {
        matches!(
            self,
            Self::Rdna2 | Self::Rdna3 | Self::Rdna3_5 | Self::Rdna4
        )
    }

    /// RADV's `minSubgroupSize` is 32 from GFX10; earlier cores run only
    /// 64-wide waves.
    pub const fn runs_wave32(self) -> bool {
        matches!(
            self,
            Self::Rdna1 | Self::Rdna2 | Self::Rdna3 | Self::Rdna3_5 | Self::Rdna4
        )
    }
}

/// One graphics core from amdgpu's table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Chip {
    pub gc: GcVersion,
    /// The chip's common name, which is also Metal's architecture name.
    pub name: &'static str,
    /// amdgpu sets `AMD_IS_APU` for this core: it shares system memory and
    /// has no PCIe link of its own.
    pub integrated: bool,
    /// The prefix of the graphics core's firmware files in linux-firmware's
    /// `amdgpu/` directory (`<prefix>_pfp.bin`, `<prefix>_mec.bin`, ...).
    /// The PSP, SMU, SDMA and display engines name theirs by their own IP
    /// versions.
    pub firmware: &'static str,
}

const fn chip(
    gc: (u8, u8, u8),
    name: &'static str,
    integrated: bool,
    firmware: &'static str,
) -> Chip {
    Chip {
        gc: GcVersion::new(gc.0, gc.1, gc.2),
        name,
        integrated,
        firmware,
    }
}

/// amdgpu's RDNA2 and later graphics cores with a graphics pipeline that
/// Mesa names. Linux v7.2.12 also lists GC 11.5.3, 11.5.4, 11.5.6, 11.7.x and
/// 12.1.0; they stay out until their chips and Mesa support are known.
pub static CHIPS: [Chip; 18] = [
    chip((10, 3, 0), "Navi 21", false, "sienna_cichlid"),
    chip((10, 3, 1), "Van Gogh", true, "vangogh"),
    chip((10, 3, 2), "Navi 22", false, "navy_flounder"),
    chip((10, 3, 3), "Rembrandt", true, "yellow_carp"),
    chip((10, 3, 4), "Navi 23", false, "dimgrey_cavefish"),
    chip((10, 3, 5), "Navi 24", false, "beige_goby"),
    chip((10, 3, 6), "Raphael", true, "gc_10_3_6"),
    chip((10, 3, 7), "Mendocino", true, "gc_10_3_7"),
    chip((11, 0, 0), "Navi 31", false, "gc_11_0_0"),
    chip((11, 0, 1), "Phoenix", true, "gc_11_0_1"),
    chip((11, 0, 2), "Navi 33", false, "gc_11_0_2"),
    chip((11, 0, 3), "Navi 32", false, "gc_11_0_3"),
    chip((11, 0, 4), "Phoenix 2", true, "gc_11_0_4"),
    chip((11, 5, 0), "Strix Point", true, "gc_11_5_0"),
    chip((11, 5, 1), "Strix Halo", true, "gc_11_5_1"),
    chip((11, 5, 2), "Krackan Point", true, "gc_11_5_2"),
    chip((12, 0, 0), "Navi 44", false, "gc_12_0_0"),
    chip((12, 0, 1), "Navi 48", false, "gc_12_0_1"),
];

impl Chip {
    pub fn find(gc: GcVersion) -> Option<&'static Chip> {
        CHIPS.iter().find(|chip| chip.gc == gc)
    }

    pub const fn generation(&self) -> Generation {
        self.gc.generation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn ip_discovery_versions_decode_as_amdgpu_packs_them() {
        // An RX 6650 XT (Navi 23) reports GC 10.3.4.
        let navi23 = GcVersion::from_ip_discovery(0x000a_0304);
        assert_eq!(navi23, GcVersion::new(10, 3, 4));
        assert_eq!(Chip::find(navi23).map(|chip| chip.name), Some("Navi 23"));
        assert_eq!(navi23.generation(), Generation::Rdna2);
        assert_eq!(GcVersion::new(12, 0, 1).to_string(), "12.0.1");
    }

    #[test]
    fn generations_follow_mesas_gfx_levels() {
        let generation =
            |major, minor, revision| GcVersion::new(major, minor, revision).generation();
        assert_eq!(generation(8, 0, 1), Generation::Gcn);
        assert_eq!(generation(9, 0, 1), Generation::Vega);
        assert_eq!(generation(9, 4, 0), Generation::Vega);
        assert_eq!(generation(9, 3, 0), Generation::Vega);
        assert_eq!(generation(9, 4, 1), Generation::Cdna);
        assert_eq!(generation(9, 4, 3), Generation::Cdna);
        assert_eq!(generation(9, 5, 0), Generation::Cdna);
        assert_eq!(generation(10, 1, 10), Generation::Rdna1);
        assert_eq!(generation(10, 3, 7), Generation::Rdna2);
        assert_eq!(generation(11, 0, 3), Generation::Rdna3);
        assert_eq!(generation(11, 5, 1), Generation::Rdna3_5);
        assert_eq!(generation(12, 0, 0), Generation::Rdna4);
        assert_eq!(generation(12, 1, 0), Generation::Unknown);
    }

    #[test]
    fn the_table_lists_metal3_cores_once_in_order() {
        assert!(CHIPS.windows(2).all(|pair| pair[0].gc < pair[1].gc));
        assert!(CHIPS.iter().all(|chip| chip.generation().runs_metal3()));
        assert!(CHIPS.iter().all(|chip| chip.generation().runs_wave32()));
        assert!(!Generation::Rdna1.runs_metal3() && Generation::Rdna1.runs_wave32());
        assert!(!Generation::Vega.runs_wave32());
        let integrated: alloc::vec::Vec<_> = CHIPS
            .iter()
            .filter(|chip| chip.integrated)
            .map(|chip| chip.name)
            .collect();
        assert_eq!(
            integrated,
            [
                "Van Gogh",
                "Rembrandt",
                "Raphael",
                "Mendocino",
                "Phoenix",
                "Phoenix 2",
                "Strix Point",
                "Strix Halo",
                "Krackan Point"
            ]
        );
    }

    #[test]
    fn firmware_prefixes_follow_amdgpu_naming() {
        let firmware = |major, minor, revision| {
            Chip::find(GcVersion::new(major, minor, revision)).map(|chip| chip.firmware)
        };
        // The cores before GC 10.3.6 keep their legacy code names.
        assert_eq!(firmware(10, 3, 0), Some("sienna_cichlid"));
        assert_eq!(firmware(10, 3, 4), Some("dimgrey_cavefish"));
        assert_eq!(firmware(10, 3, 6), Some("gc_10_3_6"));
        assert_eq!(firmware(11, 0, 0), Some("gc_11_0_0"));
        assert_eq!(firmware(12, 0, 1), Some("gc_12_0_1"));
        assert_eq!(firmware(10, 1, 10), None);
    }
}
