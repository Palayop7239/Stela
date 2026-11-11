//! Where a GPU sits on the host, as the platform's PCI and ACPI owners
//! report it, for every vendor route.

/// The negotiated PCI Express link, from the Link Status register
/// (`PCI_EXP_LNKSTA`) of the GPU's PCIe capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcieLink {
    /// Current Link Speed: 1 for 2.5 GT/s through 6 for 64 GT/s.
    pub speed: u8,
    /// Negotiated Link Width in lanes.
    pub width: u8,
}

impl PcieLink {
    pub const fn from_link_status(status: u16) -> Self {
        Self {
            speed: (status & 0xf) as u8,
            width: ((status >> 4) & 0x3f) as u8,
        }
    }

    /// Data bandwidth in bytes per second after line encoding, as Linux's
    /// `PCIE_SPEED2MBS_ENC`: 8b/10b through 5 GT/s, 128b/130b from 8 GT/s,
    /// and FLIT mode at 64 GT/s. Unknown speeds report zero.
    pub const fn bytes_per_second(self) -> u64 {
        let megabits_per_lane: u64 = match self.speed {
            1 => 2500 * 8 / 10,
            2 => 5000 * 8 / 10,
            3 => 8000 * 128 / 130,
            4 => 16000 * 128 / 130,
            5 => 32000 * 128 / 130,
            6 => 64000,
            _ => 0,
        };
        megabits_per_lane * self.width as u64 * 1_000_000 / 8
    }
}

/// Where the GPU is attached, as the platform's PCI and ACPI owners report it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attachment {
    pub link: PcieLink,
    /// Behind a hot-pluggable Thunderbolt or USB4 tunnel.
    pub external: bool,
    /// The slot or enclosure index Metal reports as `locationNumber`.
    pub location_number: u64,
}
