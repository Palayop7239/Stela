//! Opens the Metal service for one AMD GPU: admits its Vulkan device,
//! derives the platform facts Vulkan does not report, and serves a
//! [`Device`] over the backend and compiler. [`open`] composes the native
//! stack the same way the NVIDIA route does, with RADV's entry point in
//! place of NVK's.
use super::{Attachment, Chip, GcVersion, Refusal};
#[cfg(feature = "metal-vulkan")]
use crate::metal::{
    compiler::{self, AirCompiler},
    native::{Loader, MemoryExporter, NativeBackend, vk},
    nvidia::open::stack,
};
use crate::metal::{
    device::Device,
    error::Error,
    service::{Service, identity::DeviceEpoch},
    shader::Compiler,
    vulkan::{backend::Backend, physical::PciBusInfo},
};
#[cfg(feature = "metal-vulkan")]
use alloc::boxed::Box;

/// The GPU the platform hands the Metal device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gpu {
    pub pci: PciBusInfo,
    /// The graphics core amdgpu reports.
    pub gc: GcVersion,
    /// Ignored for an APU, whose graphics core has no link of its own.
    pub attachment: Attachment,
}

/// Why the Metal service did not open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenError {
    /// The device is not an AMD GPU this route drives.
    Refused(Refusal),
    /// The device lacks a requirement or could not be created.
    Device(Error),
}

impl From<Error> for OpenError {
    fn from(error: Error) -> Self {
        Self::Device(error)
    }
}

/// Serves the Metal device over `backend`, which drives `gpu`, and
/// `compiler`, in the gate's `epoch` for it.
pub fn serve<B: Backend, C: Compiler>(
    backend: B,
    compiler: C,
    gpu: &Gpu,
    epoch: DeviceEpoch,
) -> Result<Service<B, C>, OpenError> {
    Ok(Service::new(device(backend, compiler, gpu)?, epoch))
}

/// The Metal device over `backend`, once it is admitted as `gpu`.
fn device<B: Backend, C: Compiler>(
    backend: B,
    compiler: C,
    gpu: &Gpu,
) -> Result<Device<B, C>, OpenError> {
    let identity = &backend.physical_device().identity;
    if identity.pci_bus != Some(gpu.pci) {
        return Err(Error::Invalid("the backend drives another GPU").into());
    }
    let chip = Chip::admit(identity, gpu.gc).map_err(OpenError::Refused)?;
    let platform = chip.platform(gpu.attachment);
    Ok(Device::new(backend, compiler, &platform)?)
}

/// The Metal service over RADV and the AIR compiler.
#[cfg(feature = "metal-vulkan")]
pub type NativeService = Service<NativeBackend, AirCompiler>;

/// Opens RADV's device for `gpu` and serves it with the AIR compiler service
/// `compiler` answers for that device.
///
/// # Safety
///
/// `loader` is RADV's entry point and `compiler` answers the AIR compiler
/// service with RADV's NIR options for the device it is given, both
/// outliving the service.
#[cfg(feature = "metal-vulkan")]
pub unsafe fn open(
    loader: Loader,
    exporter: Box<dyn MemoryExporter>,
    compiler: impl FnOnce(vk::PhysicalDevice) -> compiler::Service,
    gpu: &Gpu,
    epoch: DeviceEpoch,
) -> Result<NativeService, OpenError> {
    // SAFETY: the caller's guarantees; the stack names no Vulkan driver.
    let (backend, compiler) = unsafe { stack(loader, gpu.pci, exporter, compiler) }?;
    serve(backend, compiler, gpu, epoch)
}

/// Opens RADV's device for `gpu` as [`open`] does, for a service whose device
/// was lost ([`Service::with_reopen`]).
///
/// # Safety
///
/// As for [`open`].
#[cfg(feature = "metal-vulkan")]
pub unsafe fn reopen(
    loader: Loader,
    exporter: Box<dyn MemoryExporter>,
    compiler: impl FnOnce(vk::PhysicalDevice) -> compiler::Service,
    gpu: &Gpu,
) -> Result<Device<NativeBackend, AirCompiler>, Error> {
    // SAFETY: the caller's guarantees.
    let (backend, compiler) = unsafe { stack(loader, gpu.pci, exporter, compiler) }?;
    device(backend, compiler, gpu).map_err(|error| match error {
        OpenError::Device(error) => error,
        OpenError::Refused(_) => Error::Invalid("the GPU was refused as it reopened"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metal::{
        amd::{CHIPS, PcieLink, fixture::radv},
        device::fixture::{MockBackend, MockCompiler},
        vulkan::DriverId,
    };
    use core::num::NonZeroU64;

    const NAVI23: GcVersion = GcVersion::new(10, 3, 4);
    const PHOENIX: GcVersion = GcVersion::new(11, 0, 1);

    fn gpu(gc: GcVersion) -> Gpu {
        Gpu {
            pci: PciBusInfo {
                domain: 0,
                bus: 3,
                device: 0,
                function: 0,
            },
            gc,
            attachment: Attachment {
                link: PcieLink { speed: 4, width: 8 },
                external: false,
                location_number: 1,
            },
        }
    }

    /// RADV's device as it reports itself today.
    fn backend(gc: GcVersion) -> MockBackend {
        let chip = Chip::find(gc).unwrap();
        let mut physical = radv(chip, 8 << 30);
        physical.identity.pci_bus = Some(gpu(gc).pci);
        let mut backend = MockBackend::new();
        backend.physical = Some(physical);
        backend
    }

    /// RADV's device with the two backend requirements it lacks today, as
    /// once Stela no longer needs them.
    fn admissible(gc: GcVersion) -> MockBackend {
        let mut backend = backend(gc);
        let features = &mut backend.physical.as_mut().unwrap().features;
        features.unified_image_layouts = true;
        features.dynamic_primitive_topology_unrestricted = true;
        backend
    }

    fn serve_on(
        backend: MockBackend,
        gpu: &Gpu,
    ) -> Result<Service<MockBackend, MockCompiler>, OpenError> {
        let epoch = DeviceEpoch::from_registered(NonZeroU64::MIN);
        serve(backend, MockCompiler::default(), gpu, epoch)
    }

    #[test]
    fn the_backend_requirements_refuse_radv_today() {
        assert_eq!(
            serve_on(backend(NAVI23), &gpu(NAVI23)).err(),
            Some(OpenError::Device(Error::Unsupported("unifiedImageLayouts")))
        );
        let navi31 = GcVersion::new(11, 0, 0);
        assert_eq!(
            serve_on(backend(navi31), &gpu(navi31)).err(),
            Some(OpenError::Device(Error::Unsupported(
                "dynamicPrimitiveTopologyUnrestricted"
            )))
        );
    }

    #[test]
    fn an_rx_6650_xt_is_served_in_its_slot() {
        let mut service = serve_on(admissible(NAVI23), &gpu(NAVI23)).unwrap();
        let capabilities = service.device().capabilities();
        assert_eq!(capabilities.architecture, "Navi 23");
        assert!(!capabilities.removable && !capabilities.has_unified_memory);
        assert_eq!(capabilities.location_number, 1);
        assert_eq!(capabilities.max_transfer_rate, 15_753_000_000);
        assert_eq!(capabilities.limits.thread_execution_width, 32);
    }

    #[test]
    fn an_apu_is_served_built_in_with_unified_memory() {
        let mut service = serve_on(admissible(PHOENIX), &gpu(PHOENIX)).unwrap();
        let capabilities = service.device().capabilities();
        assert_eq!(capabilities.architecture, "Phoenix");
        assert!(capabilities.has_unified_memory && capabilities.low_power);
        assert_eq!(capabilities.max_transfer_rate, 0);
        assert_eq!(capabilities.location_number, 0);
    }

    #[test]
    fn other_gpus_and_drivers_are_refused() {
        let mut other = gpu(NAVI23);
        other.pci.bus = 4;
        assert_eq!(
            serve_on(backend(NAVI23), &other).err(),
            Some(OpenError::Device(Error::Invalid(
                "the backend drives another GPU"
            )))
        );
        // An RX 5700 (Navi 10) has neither mesh shaders nor barycentrics.
        let navi10 = GcVersion::new(10, 1, 10);
        assert_eq!(
            serve_on(backend(NAVI23), &gpu(navi10)).err(),
            Some(OpenError::Refused(Refusal::BeforeRdna2 { gc: navi10 }))
        );
        let mut nvk = backend(NAVI23);
        nvk.physical.as_mut().unwrap().identity.driver_id = DriverId::MesaNvk;
        assert_eq!(
            serve_on(nvk, &gpu(NAVI23)).err(),
            Some(OpenError::Refused(Refusal::NotRadv(DriverId::MesaNvk)))
        );
        assert!(
            CHIPS
                .iter()
                .all(|chip| { serve_on(admissible(chip.gc), &gpu(chip.gc)).is_ok() })
        );
    }
}
