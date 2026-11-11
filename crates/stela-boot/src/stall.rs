//! What a frozen boot was waiting for. Once the boot PE's run loop has not
//! reached a slice's end for [`STALL_SECONDS`], each processor that then
//! spins for a lock or waits on a driver says once where it waits: the image
//! offsets on its stack, which `stela.pdb` resolves, and where the host
//! drivers' lock was taken if a core holds it. Nothing else draws on a frozen
//! machine and its log may not reach a disk, so the lines go to the screen
//! as well as the log.
//!
//! Only the boot PE stores the log, so a silent one leaves the disk without
//! these lines: the first other processor to report keeps storing it in the
//! boot PE's place while the host drivers are free. After
//! [`INTERRUPT_SECONDS`] that processor stops the boot PE with an NMI, whose
//! handler leaves where the boot PE was here ([`interrupted`]) before its
//! fatal report, and the keeper reports it too, in case the boot PE's own
//! report needs something the boot PE held when it stopped.
use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

/// How long the boot PE's loop may go without a slice's end. Its beats come
/// every 10 to 20 s, so a wait this long is no beat's.
const STALL_SECONDS: u64 = 30;
/// How long the boot PE may be silent before the keeper stops it with an
/// NMI for its place: a stall this long has already lost the guest.
const INTERRUPT_SECONDS: u64 = 40;
/// How long the keeper waits for the stopped boot PE to leave its place.
const ANSWER_MILLIS: u64 = 2_000;
/// Stack words shown per report.
const SHOWN: usize = 24;
/// No processor: the APIC IDs' sentinel.
const NONE: u32 = u32::MAX;

/// The counter when the boot PE's run loop last reached a slice's end; 0
/// before it first runs. It stays set once the loop returns, so the stop
/// after a run is watched as well.
static ALIVE: AtomicU64 = AtomicU64::new(0);
/// The boot PE's APIC ID, recorded by its first [`alive`].
static HOME: AtomicU32 = AtomicU32::new(NONE);
/// The processors that reported, by local APIC ID.
static REPORTED: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
/// Held while a processor draws its report.
static REPORTING: AtomicBool = AtomicBool::new(false);
/// The processor that stores the log while the boot PE is silent.
static KEEPER: AtomicU32 = AtomicU32::new(NONE);
/// The counter at the keeper's last log store.
static KEPT_AT: AtomicU64 = AtomicU64::new(0);
/// The keeper sent the boot PE its NMI.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// Where the NMI found the boot PE: its instruction's image offset and the
/// image offsets on its stack, [`PLACED`] of them, once [`interrupted`] ran.
static PLACE: [AtomicU64; SHOWN + 1] = [const { AtomicU64::new(0) }; SHOWN + 1];
static PLACED: AtomicUsize = AtomicUsize::new(0);
static PLACE_READY: AtomicBool = AtomicBool::new(false);

/// This processor's local APIC ID, as its ICR addresses it.
fn this_apic() -> u32 {
    let base = crate::apic::base_msr();
    crate::apic::Apic::new(base, crate::layout::physical(base & crate::apic::BASE_ADDRESS)).id()
}

/// The boot PE's run loop reached a slice's end at counter `now`.
pub(crate) fn alive(now: u64) {
    let hz = crate::output::clock_hz();
    // Stored every sixteenth of a second at most: every waiting processor
    // reads it.
    if now.wrapping_sub(ALIVE.load(Ordering::Relaxed)) >= hz / 16 {
        if HOME.load(Ordering::Relaxed) == NONE {
            HOME.store(this_apic(), Ordering::Relaxed);
        }
        ALIVE.store(now, Ordering::Relaxed);
    }
}

/// A turn of a lock spin or a driver's wait, `site`: report this processor
/// once if the boot PE stopped, and keep the log stored in its place.
#[inline]
pub(crate) fn check(site: &str) {
    let alive = ALIVE.load(Ordering::Relaxed);
    if alive == 0 {
        return;
    }
    let hz = crate::output::clock_hz();
    // SAFETY: RDTSC has no memory effects.
    let now = unsafe { core::arch::x86_64::_rdtsc() };
    // A counter a little behind the boot PE's is no stall.
    let stalled = now.saturating_sub(alive);
    if hz == 0 || stalled < STALL_SECONDS * hz {
        return;
    }
    stalled_turn(site, stalled / hz, hz);
}

#[cold]
#[inline(never)]
fn stalled_turn(site: &str, seconds: u64, hz: u64) {
    let apic = this_apic();
    report(site, seconds, apic);
    if apic == HOME.load(Ordering::Relaxed) {
        return;
    }
    let keeper = match KEEPER.compare_exchange(NONE, apic, Ordering::Relaxed, Ordering::Relaxed) {
        Ok(_) => apic,
        Err(keeper) => keeper,
    };
    if keeper != apic {
        return;
    }
    // The boot PE inside the host drivers holds their lock, and a store
    // would wait for it for good. A store at most every fiftieth of a
    // second, as the boot PE's service stores.
    let now = crate::output::now();
    if now.wrapping_sub(KEPT_AT.load(Ordering::Relaxed)) >= hz / 50
        && !crate::engine::host_drivers_taken()
    {
        KEPT_AT.store(now, Ordering::Relaxed);
        crate::output::flush_due();
    }
    if seconds >= INTERRUPT_SECONDS && !INTERRUPTED.swap(true, Ordering::Relaxed) {
        interrupt_home(apic, seconds, hz);
    }
}

/// Stop the silent boot PE with an NMI and report where it was.
fn interrupt_home(apic: u32, seconds: u64, hz: u64) {
    let home = HOME.load(Ordering::Relaxed);
    if home == NONE {
        return;
    }
    crate::output::arguments(format_args!(
        "STALL cpu {apic} stops the boot PE, cpu {home}, silent {seconds} s, with an NMI for its place\n"
    ));
    let base = crate::apic::base_msr();
    let local =
        crate::apic::Apic::new(base, crate::layout::physical(base & crate::apic::BASE_ADDRESS));
    // SAFETY: every engine processor's IDT routes the NMI to its terminal
    // handler; a stall check runs with interrupts disabled, outside any
    // other ICR write on this processor.
    if let Err(error) = unsafe { local.nmi(home, hz) } {
        crate::output::arguments(format_args!("STALL the NMI was not sent: {error:?}\n"));
        return;
    }
    let started = crate::output::now();
    while !PLACE_READY.load(Ordering::Acquire) {
        if crate::output::millis_since(started) >= ANSWER_MILLIS {
            crate::output::arguments(format_args!(
                "STALL the boot PE did not answer its NMI in {ANSWER_MILLIS} ms\n"
            ));
            show(format_args!(
                "STALL the boot PE (cpu {home}) did not answer an NMI in {ANSWER_MILLIS} ms"
            ));
            return;
        }
        core::hint::spin_loop();
    }
    let mut line = Line::new();
    let _ = write!(
        line,
        "STALL boot PE cpu {home} was at image+{:#x}; stack",
        PLACE[0].load(Ordering::Relaxed)
    );
    for slot in &PLACE[1..=PLACED.load(Ordering::Relaxed).min(SHOWN)] {
        let _ = write!(line, " {:#x}", slot.load(Ordering::Relaxed));
    }
    crate::output::arguments(format_args!("{}\n", line.as_str()));
    show(format_args!("{}", line.as_str()));
    crate::output::flush_due();
}

/// The NMI the keeper sent stopped this processor at `rip` with stack
/// pointer `rsp`: leave the place for the keeper before anything that may
/// need what this processor held. Called first by the terminal handler.
pub(crate) fn interrupted(rip: u64, rsp: u64) {
    if !INTERRUPTED.load(Ordering::Relaxed) || PLACE_READY.load(Ordering::Relaxed) {
        return;
    }
    unsafe extern "C" {
        static __ImageBase: u8;
    }
    PLACE[0].store(
        rip.wrapping_sub(&raw const __ImageBase as u64),
        Ordering::Relaxed,
    );
    let mut placed = 0;
    // SAFETY: the interrupted stack's own page is mapped.
    unsafe {
        crate::output::stack_offsets(rsp as *const u64, SHOWN, |offset| {
            placed += 1;
            PLACE[placed].store(offset, Ordering::Relaxed);
        })
    };
    PLACED.store(placed, Ordering::Relaxed);
    PLACE_READY.store(true, Ordering::Release);
}

/// Draw one line on the screen, after another core's report.
fn show(args: fmt::Arguments<'_>) {
    let mut line = Line::new();
    let _ = line.write_fmt(args);
    let mut spins = 0u32;
    while REPORTING
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins > 1 << 24 {
            return;
        }
        core::hint::spin_loop();
    }
    crate::console::reclaim();
    crate::status::info(line.as_str());
    REPORTING.store(false, Ordering::Release);
}

fn report(site: &str, seconds: u64, apic: u32) {
    let index = apic as usize;
    let bit = 1u64 << (index % 64);
    if REPORTED[index / 64 % REPORTED.len()].fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    let mut head = Line::new();
    let _ = write!(
        head,
        "STALL cpu {apic}: the boot PE has been silent {seconds} s; this core waits in a {site}"
    );
    match crate::engine::host_drivers_site() {
        Some(at) => {
            let _ = write!(head, "; host drivers held from {}:{}", at.file(), at.line());
        }
        None => {
            let _ = write!(head, "; host drivers free");
        }
    }
    let mut stack = Line::new();
    let _ = write!(stack, "STALL cpu {apic} stack");
    let rsp: *const u64;
    // SAFETY: reads the stack pointer.
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags)) };
    // SAFETY: the stack pointer's own page is mapped.
    unsafe {
        crate::output::stack_offsets(rsp, SHOWN, |offset| {
            let _ = write!(stack, " {offset:#x}");
        })
    };
    crate::output::arguments(format_args!("{}\n{}\n", head.as_str(), stack.as_str()));
    // Another core's report goes first; a long wait leaves this one to the
    // log alone.
    show(format_args!("{}", head.as_str()));
    show(format_args!("{}", stack.as_str()));
}

/// A report line in a fixed buffer, so the log line needs no heap: a stalled
/// core may be waiting for the allocator's holder. Text past its room is cut.
pub(crate) struct Line {
    bytes: [u8; 400],
    len: usize,
}

impl Line {
    pub(crate) fn new() -> Self {
        Self {
            bytes: [0; 400],
            len: 0,
        }
    }

    /// The text, up to the last whole character a cut left.
    pub(crate) fn as_str(&self) -> &str {
        let bytes = &self.bytes[..self.len];
        match core::str::from_utf8(bytes) {
            Ok(text) => text,
            // SAFETY: `valid_up_to` ends a valid UTF-8 prefix.
            Err(error) => unsafe { core::str::from_utf8_unchecked(&bytes[..error.valid_up_to()]) },
        }
    }
}

impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let take = text.len().min(self.bytes.len() - self.len);
        self.bytes[self.len..self.len + take].copy_from_slice(&text.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}
