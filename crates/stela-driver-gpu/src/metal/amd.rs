//! AMD GPUs under Stela's Metal device: amdgpu in the kernel and RADV as the
//! Vulkan driver.
//!
//! Neither driver is in Stela's image yet. This module is the Metal side of
//! that route, built like [`super::nvidia`]: hardware access, firmware,
//! memory management, command submission and shader code generation stay
//! with amdgpu, RADV and ACO, while this module admits a device and supplies
//! the facts Vulkan does not report, and [`open`] serves it.
use super::{
    abi::DeviceLocation,
    capabilities::Platform,
    vulkan::{DriverId, PhysicalDeviceType, physical::Identity},
};
use alloc::string::String;

pub mod open;

pub const VENDOR_ID: u32 = 0x1002;

pub use super::attachment::{Attachment, PcieLink};
pub use crate::amdgpu_chip::{CHIPS, Chip, GcVersion, Generation};

/// Why a Vulkan device is not admitted as an AMD Metal device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    NotAmd {
        vendor_id: u32,
    },
    /// Only RADV is planned over Stela's amdgpu port.
    NotRadv(DriverId),
    /// Cores before GFX10.3: RADV exposes no mesh shaders or fragment
    /// barycentrics there, and GFX9 and earlier run only 64-wide waves.
    BeforeRdna2 {
        gc: GcVersion,
    },
    /// Compute-only accelerators have no graphics pipeline.
    ComputeOnly {
        gc: GcVersion,
    },
    /// amdgpu names no graphics core of this version that Stela knows.
    UnknownCore {
        gc: GcVersion,
    },
    /// RADV reports a placement that disagrees with amdgpu's APU flag for
    /// this core.
    Placement {
        reported: PhysicalDeviceType,
    },
}

impl Chip {
    /// Admits the Vulkan device `identity` whose kernel driver reports the
    /// graphics core `gc` (`AMDGPU_INFO_HW_IP_INFO`'s `ip_discovery_version`
    /// for `AMDGPU_HW_IP_GFX`).
    pub fn admit(identity: &Identity, gc: GcVersion) -> Result<&'static Chip, Refusal> {
        if identity.vendor_id != VENDOR_ID {
            return Err(Refusal::NotAmd {
                vendor_id: identity.vendor_id,
            });
        }
        if identity.driver_id != DriverId::MesaRadv {
            return Err(Refusal::NotRadv(identity.driver_id));
        }
        match gc.generation() {
            Generation::Gcn | Generation::Vega | Generation::Rdna1 => {
                return Err(Refusal::BeforeRdna2 { gc });
            }
            Generation::Cdna => return Err(Refusal::ComputeOnly { gc }),
            Generation::Unknown => return Err(Refusal::UnknownCore { gc }),
            Generation::Rdna2 | Generation::Rdna3 | Generation::Rdna3_5 | Generation::Rdna4 => {}
        }
        let chip = Self::find(gc).ok_or(Refusal::UnknownCore { gc })?;
        let expected = if chip.integrated {
            PhysicalDeviceType::IntegratedGpu
        } else {
            PhysicalDeviceType::DiscreteGpu
        };
        if identity.device_type != expected {
            return Err(Refusal::Placement {
                reported: identity.device_type,
            });
        }
        Ok(chip)
    }

    /// The row pitch RADV accepts for an explicitly laid out linear image:
    /// `ac_surface_get_pitch_align` asks 256 bytes from GFX9 and 128 bytes
    /// from GFX12.
    pub const fn linear_row_alignment(&self) -> u64 {
        match self.generation() {
            Generation::Rdna4 => 128,
            _ => 256,
        }
    }

    pub fn platform(&self, attachment: Attachment) -> Platform {
        // An APU's graphics core sits on the SoC fabric: Metal reports it
        // built in, with no transfer link.
        let (location, location_number, max_transfer_rate, removable) = if self.integrated {
            (DeviceLocation::BuiltIn, 0, 0, false)
        } else if attachment.external {
            (
                DeviceLocation::External,
                attachment.location_number,
                attachment.link.bytes_per_second(),
                true,
            )
        } else {
            (
                DeviceLocation::Slot,
                attachment.location_number,
                attachment.link.bytes_per_second(),
                false,
            )
        };
        Platform {
            architecture: String::from(self.name),
            location,
            location_number,
            max_transfer_rate,
            // Every core in the table has a display engine.
            headless: false,
            removable,
            linear_row_alignment: self.linear_row_alignment(),
            // addrlib's base alignment for linear surfaces is 256 bytes on
            // GFX10 and GFX11; GFX12 needs no more.
            linear_offset_alignment: 256,
            // Partially resident textures map 64 KiB tiles, Vulkan's standard
            // sparse block size.
            sparse_page_bytes: 64 << 10,
        }
    }
}

#[cfg(test)]
pub(crate) mod fixture;

#[cfg(test)]
mod tests;
