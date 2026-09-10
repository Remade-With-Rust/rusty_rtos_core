//! The `small-metal` seam on a **Kairos target**: Cortex-M3 under QEMU.
//!
//! The ESP32-S3 row beside this one made the fixed-region backend real on
//! 32-bit silicon, and carried a caveat in capitals: *that is not a Kairos
//! target*. This is. `thumbv7m-none-eabi` on an ARMv7-M, which is the
//! architecture `build-me-bare` B4 and the mission plan's K3 both name.
//!
//! # Why `mps2-an385` and not `lm3s6965evb`
//!
//! The plan named the LM3S6965 because FreeRTOS ships a QEMU demo for it.
//! It cannot host this seam, and the reason is arithmetic rather than
//! taste: `MIN_REGION` is one segment, 64 KiB at this geometry, and the
//! LM3S6965 has **64 KiB of SRAM in total**. The smallest region the
//! allocator will accept is the entire chip, leaving nothing for the
//! stack, `.data` or `.bss`.
//!
//! The AN385 is the same core with megabytes of RAM, and FreeRTOS ships a
//! QEMU demo for it as well (`CORTEX_MPS2_QEMU_*`), so the C arm of K3's
//! A/B still exists. Nothing about the port changes: same ISA, same
//! `thumbv7m-none-eabi`.
//!
//! # What it measures, and what it refuses to call a measurement
//!
//! QEMU is **not cycle-accurate**. It is a translator, not a simulator,
//! so a wall-clock or cycle figure taken from it is a number about the
//! host. This firmware therefore prints DWT's cycle counter only to say
//! *whether it is even implemented here* — never as a performance figure —
//! and everything it actually asserts is a deterministic count.
//!
//! That is not a workaround; it is the same rule the rest of this repo
//! already runs on. The kernel's speed work is judged on callgrind
//! instruction counts and the corpus, not on a clock.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_semihosting as _;

use rusty_rtos_alloc::small_metal::{
    good_region_size, region_contains, region_stats, PrimError, Region, FERR_GEOMETRY,
    FERR_MISALIGNED, FERR_REGISTERED, FERR_TOO_SMALL, FIXED_PAGE, MIN_REGION, REGION_ALIGN,
};

#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;

/// The crate docs' worked example. `good_region_size` rounds down to whole
/// segments, so the firmware prints both the budget and what it got.
const BUDGET: usize = 220 * 1024;
type Heap = Region<{ good_region_size(BUDGET) }>;

static HEAP: Heap = Heap::new();

fn why(e: PrimError) -> &'static str {
    match e {
        FERR_TOO_SMALL => "FERR_TOO_SMALL: the region cannot hold one segment",
        FERR_GEOMETRY => "FERR_GEOMETRY: not a whole number of segments",
        FERR_REGISTERED => "FERR_REGISTERED: a region was already given",
        FERR_MISALIGNED => "FERR_MISALIGNED: base is off the REGION_ALIGN grid",
        _ => "unknown PrimError",
    }
}

struct Tally {
    passed: u32,
    failed: u32,
}

impl Tally {
    fn check(&mut self, ok: bool, what: &str) {
        if ok {
            self.passed += 1;
            hprintln!("      ok    {}", what);
        } else {
            self.failed += 1;
            hprintln!("      FAIL  {}", what);
        }
    }
}

#[entry]
fn main() -> ! {
    let mut t = Tally {
        passed: 0,
        failed: 0,
    };

    hprintln!();
    hprintln!("=== rusty_rtos_alloc small-metal seam on Cortex-M3 (mps2-an385, QEMU) ===");
    hprintln!("target           thumbv7m-none-eabi   <- a KAIROS target");
    hprintln!("rusty_alloc      {}", rusty_rtos_alloc::VERSION);
    hprintln!("REGION_ALIGN     {} bytes", REGION_ALIGN);
    hprintln!("MIN_REGION       {} bytes", MIN_REGION);
    hprintln!("FIXED_PAGE       {} bytes", FIXED_PAGE);
    hprintln!("budget asked     {} bytes", BUDGET);
    hprintln!("region reserved  {} bytes", HEAP.len());
    hprintln!("usable of that   {} bytes", Heap::USABLE);

    hprintln!();
    hprintln!("[1] give the allocator its region");
    let usable = match HEAP.give() {
        Ok(u) => {
            hprintln!("      give() -> {} bytes usable", u);
            u
        }
        Err(e) => {
            hprintln!("      give() -> {}", why(e));
            hprintln!("RESULT: FAIL -- the region was refused");
            debug::exit(debug::EXIT_FAILURE);
            loop {
                cortex_m::asm::wfi();
            }
        }
    };
    t.check(usable > 0, "give answered a usable size");
    t.check(usable <= HEAP.len(), "usable does not exceed the region");

    hprintln!();
    hprintln!("[2] a second give is refused, and says why");
    match HEAP.give() {
        Err(FERR_REGISTERED) => t.check(true, "second give -> FERR_REGISTERED"),
        Err(e) => t.check(false, why(e)),
        Ok(_) => t.check(false, "second give was ACCEPTED"),
    }

    hprintln!();
    hprintln!("[3] allocations come out of THAT region");
    let boxed = Box::new(0xA5A5_5A5Au32);
    t.check(
        region_contains(core::ptr::from_ref::<u32>(&boxed).addr()),
        "a Box lands inside the region we gave",
    );
    t.check(*boxed == 0xA5A5_5A5A, "and holds its value");

    let mut v: Vec<u64> = Vec::new();
    for i in 0..4096u64 {
        v.push(i ^ 0x0F0F_0F0F_0F0F_0F0F);
    }
    t.check(
        region_contains(v.as_ptr().addr()),
        "a Vec grown to 32 KiB lands inside it too",
    );
    let sum = v.iter().fold(0u64, |a, b| a.wrapping_add(*b));
    let expect = (0..4096u64).fold(0u64, |a, b| a.wrapping_add(b ^ 0x0F0F_0F0F_0F0F_0F0F));
    t.check(sum == expect, "and every element survived");
    drop(v);
    drop(boxed);

    hprintln!();
    hprintln!("[4] freeing gives the bytes back");
    // Sized to outrun the region rather than to read a counter: 64 rounds of
    // 32 KiB is 2 MiB against a 192 KiB region, so a heap reclaiming nothing
    // is exhausted on the sixth.
    const ROUNDS: usize = 64;
    const BLOCK: usize = 32 * 1024;
    let mut inside = 0usize;
    let mut reused = 0usize;
    let mut first = 0usize;
    for round in 0..ROUNDS {
        let mut b: Vec<u8> = Vec::new();
        b.resize(BLOCK, round as u8);
        let addr = b.as_ptr().addr();
        if region_contains(addr) {
            inside += 1;
        }
        if round == 0 {
            first = addr;
        } else if addr == first {
            reused += 1;
        }
        if b.first() != Some(&(round as u8)) || b.last() != Some(&(round as u8)) {
            t.failed += 1;
        }
    }
    hprintln!(
        "      {} rounds of {} bytes = {} total",
        ROUNDS,
        BLOCK,
        ROUNDS * BLOCK
    );
    hprintln!("      served from the region: {}/{}", inside, ROUNDS);
    hprintln!(
        "      landed on the first block's address again: {}/{}",
        reused,
        ROUNDS - 1
    );
    t.check(
        inside == ROUNDS,
        "every round was served, so the bytes were reclaimed",
    );
    t.check(
        reused == ROUNDS - 1,
        "and each reused the same address: reclamation, not luck",
    );

    let (taken, unhanded, _) = region_stats();
    hprintln!(
        "      region_stats: taken {}, still unhanded {}",
        taken,
        unhanded
    );

    // ---- Is a cycle number even available here, and is it worth anything?
    //
    // Printed, never asserted. QEMU translates; it does not simulate a
    // pipeline, so any figure below is a fact about this host and this
    // build of QEMU. It is here so the K3 plan can record what the cell
    // CAN and CANNOT measure, with evidence instead of an assumption.
    hprintln!();
    hprintln!("[5] what this cell can measure (printed, NOT asserted)");
    let mut core_p = cortex_m::Peripherals::take();
    match core_p.as_mut() {
        Some(p) => {
            p.DCB.enable_trace();
            p.DWT.enable_cycle_counter();
            let a = cortex_m::peripheral::DWT::cycle_count();
            let mut acc = 0u32;
            for i in 0..1000u32 {
                acc = acc.wrapping_add(i);
            }
            let b = cortex_m::peripheral::DWT::cycle_count();
            core::hint::black_box(acc);
            hprintln!(
                "      DWT CYCCNT across a 1000-iteration loop: {}",
                b.wrapping_sub(a)
            );
            hprintln!("      ^ 0 means QEMU does not implement DWT on this machine.");

            // SysTick IS emulated, and under `-icount` QEMU's virtual clock
            // advances a fixed amount per instruction -- so a SysTick delta
            // is deterministic and proportional to work done. Whether it is
            // deterministic is the question K3 needs answered, so measure
            // the SAME workload at two sizes and print both: if the ratio
            // tracks the work, the counter is real.
            let syst = &mut p.SYST;
            // Drive it from the processor clock explicitly. A SysTick left
            // on the external reference would read nothing on a machine
            // that does not wire one, and a negative result taken from a
            // misconfigured peripheral is not a result.
            syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
            syst.set_reload(0x00FF_FFFF);
            syst.clear_current();
            syst.enable_counter();
            hprintln!("      SYST clock source: Core, reload 0x00FF_FFFF, enabled");
            let span = |iters: u32| -> u32 {
                let start = cortex_m::peripheral::SYST::get_current();
                let mut x = 0u32;
                for i in 0..iters {
                    x = x.wrapping_add(i);
                }
                core::hint::black_box(x);
                // SysTick counts DOWN.
                start.wrapping_sub(cortex_m::peripheral::SYST::get_current()) & 0x00FF_FFFF
            };
            let s1 = span(1000);
            let s2 = span(2000);
            let s4 = span(4000);
            hprintln!("      SysTick delta  1000 iters: {}", s1);
            hprintln!("      SysTick delta  2000 iters: {}", s2);
            hprintln!("      SysTick delta  4000 iters: {}", s4);
            hprintln!("      ^ MEASURED 2026-09-10: those do NOT scale with work.");
            hprintln!("        They SHRINK as the loop grows (688/493/492, and");
            hprintln!("        667/490/317 on a second run) because what they");
            hprintln!("        track is host wall time while TCG's translation");
            hprintln!("        cache warms -- not the guest's work. Under");
            hprintln!("        `-icount shift=0` they read 1/0/0 instead.");
            hprintln!("      CONCLUSION: this cell can prove CORRECTNESS and");
            hprintln!("        exits with a code, so it gates. It cannot supply");
            hprintln!("        a cycle or work number. Those rows need silicon.");
        }
        None => hprintln!("      cortex_m::Peripherals already taken"),
    }

    hprintln!();
    hprintln!("checks passed {} / {}", t.passed, t.passed + t.failed);
    if t.failed == 0 {
        hprintln!("RESULT: PASS -- the seam gave, served and reclaimed on a Cortex-M3");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!("RESULT: FAIL -- {} check(s) failed", t.failed);
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {
        cortex_m::asm::wfi();
    }
}
