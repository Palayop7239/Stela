// SPDX-License-Identifier: GPL-2.0-only
// Copyright (c) 2026 Stela contributors
//! Packet io cases.
use super::*;

/// An RTL8169S over `registers` and `memory`, its rings in `memory`.
fn modelled(registers: &mut [u32], memory: &mut [u64], signalled: bool) -> Realtek {
    use crate::bus::dma::RegionAllocator;
    let start = memory.as_mut_ptr() as u64;
    // SAFETY: the caller owns both allocations for the driver's lifetime.
    let (device, mut dma) = unsafe {
        (
            Mmio::new(registers.as_mut_ptr() as u64, size_of_val(&registers[..])),
            RegionAllocator::new(start..start + (memory.len() * 8) as u64),
        )
    };
    let revision = revision::detect(0x0080_0000).unwrap();
    let mut net = Realtek {
        device,
        clock: crate::timing::model(Clock {
            ticks_per_microsecond: 1,
        }),
        receive: Ring::new(&mut dma, true).unwrap(),
        transmit: Ring::new(&mut dma, true).unwrap(),
        mac: Mac([2, 0, 0, 0, 0, 1]),
        link: RuntimeLink::default(),
        revision,
        warning: None,
        function: 0,
        location: "model".into(),
        signalled,
        watch: ReceiveWatch::default(),
    };
    net.watch = ReceiveWatch::since(Timer::now(&net.clock));
    net
}

/// An empty receive acknowledges every pending event, as Linux's
/// `rtl_ack_events` of the whole status does, the ring's exhaustion among
/// them, signalled or polled; the model's register keeps what was written.
#[test]
fn empty_receive_acknowledges_every_pending_event() {
    for signalled in [false, true] {
        let mut registers = vec![0u32; 64];
        let mut memory = vec![0u64; 262_144];
        let pending = RX_OK | LINK_CHANGE | 1 << 2 | RX_EXHAUSTED;
        registers[INTERRUPT_STATUS / 4] = u32::from(pending) << 16;
        let mut net = modelled(&mut registers, &mut memory, signalled);
        net.receive.initialize(true);
        assert_eq!(net.signals(), signalled);
        assert_eq!(net.receive(), Ok(None));
        let written = (registers[INTERRUPT_STATUS / 4] >> 16) as u16;
        assert_eq!(written, pending);
        assert_eq!(net.watch.exhausted, 1);
        assert!(net.watch.starved);
    }
}

/// A receiver that ran out of descriptors and then stays silent is stopped,
/// offered the whole ring from its base again and started; a frame clears
/// the watch, and a quiet receiver that never ran out is left alone.
#[test]
fn a_starved_silent_receiver_restarts_from_the_ring_base() {
    let mut registers = vec![0u32; 64];
    let mut memory = vec![0u64; 262_144];
    let mut net = modelled(&mut registers, &mut memory, false);
    net.receive.initialize(true);
    // One good frame moves the ring on.
    net.receive.descriptors.write(0, FIRST | LAST | 64u32);
    assert_eq!(net.receive().unwrap().map(|frame| frame.len()), Some(60));
    assert_eq!(net.receive.position().0, 1);
    assert_eq!(net.watch.frames, 1);
    // Quiet, but never exhausted: no restart.
    net.watch.framed_at = Timer::now(&net.clock).saturating_sub(RECEIVE_STALL_US);
    assert_eq!(net.receive(), Ok(None));
    assert_eq!(net.watch.restarts, 0);
    // Exhausted, then silent past the stall: restarted.
    registers[INTERRUPT_STATUS / 4] = u32::from(RX_EXHAUSTED) << 16;
    assert_eq!(net.receive(), Ok(None));
    assert_eq!(net.watch.restarts, 1);
    assert!(!net.watch.starved);
    assert_eq!(net.receive.position(), (0, DESCRIPTORS));
    let byte = |offset: usize| (registers[offset / 4] >> (8 * (offset % 4))) as u8;
    assert_eq!(byte(COMMAND), ENABLE);
    assert_eq!(byte(CONFIG_LOCK), CONFIG_LOCKED);
    assert_eq!(
        registers[RX_LOW / 4],
        net.receive.descriptors.physical() as u32
    );
}

#[test]
fn transmit_without_carrier_is_refused_and_leaves_the_ring_untouched() {
    let mut registers = vec![0u32; 64];
    let mut memory = vec![0u64; 262_144];
    let mut net = modelled(&mut registers, &mut memory, false);
    net.transmit.initialize(false);
    let descriptors = net.transmit.descriptors.bytes().to_vec();
    assert_eq!(net.transmit(&[0; 60]), Err(LinkError::NoCarrier));
    assert_eq!(net.transmit.descriptors.bytes(), descriptors);
    assert_eq!(registers[TX_POLL / 4], 0);
}
