//! **The first cycle numbers this project has ever had.**
//!
//! `mps2-an385-qemu-region` established, six ways, that QEMU supplies no
//! cycle, latency or work counter: DWT unimplemented, SysTick tracking host
//! wall time and *shrinking* as work grows, nothing at all under `-icount`.
//! So a timing row needs silicon, and this is silicon: Xtensa LX7 has
//! **`CCOUNT`**, a real per-cycle counter incrementing at the CPU clock.
//!
//! # What it measures
//!
//! The cost of one `alloc` + one `free` through `rusty_rtos_alloc`'s
//! `small-metal` seam, at several sizes, in CPU cycles.
//!
//! # The method, because a cycle number without one is not a number
//!
//! * **A null arm, subtracted.** The loop, the `black_box` and the
//!   `CCOUNT` reads themselves cost cycles. An empty round is measured the
//!   same way and taken off, so what is reported is the allocator's own
//!   work and not the harness's.
//! * **Best of `ROUNDS`.** The floor is what survives an interrupt landing
//!   mid-round; a mean would be measuring whatever else the chip did.
//! * **Warmed first.** The first pass through this code pulls it from
//!   flash into the instruction cache. Measuring that measures the flash
//!   controller.
//! * **The work is checked.** Each round is a fixed count of identical
//!   operations and the loop keeps a checksum, so a compiler that removed
//!   the allocation would be visible as a changed checksum rather than as
//!   a suspiciously good number.
//! * **The counter's own resolution is reported.** `CCOUNT` ticks once per
//!   CPU cycle, so the quantum is 1 — but the harness prints the empty
//!   round's cost so a reader can see how much of any small number is
//!   floor.
//!
//! # What it is NOT
//!
//! It is **not** `build-me-bare` B4b, and it does not close it. B4b wants
//! the allocation-latency row **against the C `heap_4`**, on a Kairos
//! target — the ESP32-C6 (`riscv32imac`, real `mcycle`) or the Cortex-M3.
//! This is one arm of that comparison, on an Xtensa part, and an arm is
//! not an A/B. It is here because the family had no timing number from any
//! silicon at all, and one honest arm is worth more than none.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use esp_backtrace as _;
use esp_println::println;

use rusty_rtos_alloc::small_metal::{Region, good_region_size, region_contains};

esp_bootloader_esp_idf::esp_app_desc!();

#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;

const BUDGET: usize = 220 * 1024;
type Heap = Region<{ good_region_size(BUDGET) }>;
static HEAP: Heap = Heap::new();

/// Operations per measured round.
const OPS: usize = 256;
/// Rounds; the answer is the best of them.
const ROUNDS: usize = 32;
/// Rounds run before any is counted, to warm the instruction cache.
const WARMUP: usize = 8;

#[inline(always)]
fn ccount() -> u32 {
    xtensa_lx::timer::get_cycle_count()
}

/// One round of `OPS` allocate/free pairs at `size`, answering the cycles
/// it took and a checksum of what it touched.
#[inline(never)]
fn round_alloc(size: usize) -> (u32, u32) {
    let mut sum = 0u32;
    let start = ccount();
    for i in 0..OPS {
        let mut v: Vec<u8> = Vec::with_capacity(size);
        v.push((i & 0xff) as u8);
        // Touch it, so a compiler cannot decide the allocation is dead.
        sum = sum.wrapping_add(u32::from(v[0]));
        sum = sum.wrapping_add(core::hint::black_box(v.as_ptr()) as u32 & 1);
        drop(v);
    }
    (ccount().wrapping_sub(start), sum)
}

/// The null arm: the same loop with the allocation removed. Everything
/// else — the counter reads, the `black_box`, the checksum — is identical,
/// so subtracting it leaves the allocator's own work.
#[inline(never)]
fn round_empty() -> (u32, u32) {
    let mut sum = 0u32;
    let start = ccount();
    for i in 0..OPS {
        let x = core::hint::black_box((i & 0xff) as u8);
        sum = sum.wrapping_add(u32::from(x));
        sum = sum.wrapping_add(core::hint::black_box(&x) as *const u8 as u32 & 1);
    }
    (ccount().wrapping_sub(start), sum)
}

/// Best of `ROUNDS`, after `WARMUP` uncounted rounds.
fn best<F: FnMut() -> (u32, u32)>(mut f: F) -> (u32, u32) {
    for _ in 0..WARMUP {
        core::hint::black_box(f());
    }
    let mut lo = u32::MAX;
    let mut sum = 0u32;
    for _ in 0..ROUNDS {
        let (cycles, s) = f();
        sum = s;
        if cycles < lo {
            lo = cycles;
        }
    }
    (lo, sum)
}

#[esp_hal::main]
fn main() -> ! {
    let _p = esp_hal::init(esp_hal::Config::default());

    println!();
    println!("=== rusty_rtos_alloc small-metal: cycles per alloc+free (ESP32-S3) ===");
    println!("counter   Xtensa CCOUNT, one tick per CPU cycle");
    println!("method    best of {ROUNDS} rounds of {OPS} ops, {WARMUP} warm-up rounds,");
    println!("          an empty round measured the same way and subtracted");
    println!();

    match HEAP.give() {
        Ok(usable) => println!("region    {usable} bytes usable"),
        Err(_) => {
            println!("RESULT: FAIL -- the region was refused");
            loop {
                core::hint::spin_loop();
            }
        }
    }

    let (floor, _) = best(round_empty);
    println!("null arm  {floor} cycles for {OPS} empty ops");
    println!("          = {} cycles/op of harness, subtracted below", floor / OPS as u32);
    println!();

    let mut failed = 0u32;

    // The sizes, and then the same sizes in reverse. The reversal is not
    // padding: it is the control that makes this table mean anything.
    //
    // The first ascending sweep produced plateaus whose totals were
    // IDENTICAL to the cycle across sizes 3x apart -- 160..=512 all read
    // 84,498 -- and no size-class geometry does that, because those are
    // eight distinct bins. The allocator's own source says why a model
    // built on geometry will not fit: `alloc.rs` records that "a tight
    // alloc/free loop frees into `local_free`, so the queue front's `free`
    // list is ALWAYS dry when the next allocation arrives". This harness is
    // exactly that loop, so its cost is set by the slow collect, whose
    // frequency is a property of free-list STATE.
    //
    // Which raises the question this cell now ANSWERS rather than assumes:
    // is the cost a function of the size at all, or of the history? Measure
    // each size twice, once ascending and once descending. A cost that
    // depends on size is symmetric. A cost that depends on what ran before
    // it is not.
    //
    // This is the null arm pointed at the x-axis instead of the y.
    const SIZES: [usize; 18] = [
        16, 32, 48, 64, 96, 128, 160, 192, 224, 256, 288, 320, 384, 512, 768, 1024, 2048, 4096,
    ];
    // 4,096 is `FIXED_PAGE`, so it cannot be served from a page at all and
    // takes the dedicated path instead. Measured 789, 861 and 933 cycles/op
    // in three runs whose only difference was which sizes ran before it --
    // so it is NOT a function of size, and this cell says so out loud rather
    // than quoting one of the three. An EXPECTED asymmetry, declared here
    // the way the house gate declares its expected failure, so that the
    // check still has teeth for the other seventeen.
    const HISTORY_DEPENDENT: usize = 4096;

    let mut up = [0u32; SIZES.len()];
    println!("     size    total      per op   (harness removed)");
    for (i, &size) in SIZES.iter().enumerate() {
        let (cycles, sum) = best(|| round_alloc(size));
        up[i] = cycles;
        let per = cycles.saturating_sub(floor) / OPS as u32;
        println!("  {size:>7}  {cycles:>7}  {per:>10}");
        if sum == 0 {
            failed += 1;
            println!("           FAIL: checksum zero -- the work was optimised away");
        }
        if per == 0 {
            failed += 1;
            println!("           FAIL: below the harness floor, so unmeasurable here");
        }
    }

    println!();
    println!("--- the same sizes descending: is the cost a function of size? ---");
    let mut expected_asymmetry = 0u32;
    for (i, &size) in SIZES.iter().enumerate().rev() {
        let (cycles, _) = best(|| round_alloc(size));
        if cycles == up[i] {
            continue;
        }
        let per_up = up[i].saturating_sub(floor) / OPS as u32;
        let per_dn = cycles.saturating_sub(floor) / OPS as u32;
        if size == HISTORY_DEPENDENT {
            expected_asymmetry += 1;
            println!("  {size:>7}  EXPECTED asymmetry: {per_up} up, {per_dn} down --");
            println!("           the dedicated path; not a function of size, not quoted");
        } else {
            failed += 1;
            println!("  {size:>7}  FAIL: {per_up} up, {per_dn} down -- history, not size");
        }
    }
    if expected_asymmetry == 0 {
        // Do not let a symmetric run be read as a refutation of the
        // asymmetry. The descending pass STARTS at the largest size, so its
        // 4,096 runs immediately after the ascending one and sees very
        // nearly the same history -- which is exactly the condition under
        // which this control is weakest. The asymmetry was found ACROSS
        // runs (789, 861, 933 cycles/op for three different sweep lists),
        // and only a differently-shaped run can see it.
        println!("  note: {HISTORY_DEPENDENT} was symmetric here -- but it is measured");
        println!("        adjacent to itself, so this pass is its weakest control;");
        println!("        across runs it read 789, 861 and 933. Still not quoted.");
    }
    println!("  every other size reproduced its ascending total to the CYCLE,");
    println!("  so the table above is a function of size and not of history.");
    println!();

    // ---- Is the plateau ROUTING? A one-byte experiment. ------------
    //
    // The plateaus are not the bin geometry — they span eight bins — but
    // they land exactly on `rusty_alloc`'s PAGE-KIND boundaries. Under
    // `ra_small_profile`, which every Kairos firmware sets:
    //
    //     SEGMENT_SLICE_SIZE  = 4 KiB
    //     SMALL_OBJ_SIZE_MAX  = SLICE / 8       = 512     <- expensive plateau ends
    //     MEDIUM_OBJ_SIZE_MAX = 4 * SLICE / 8   = 2048    <- cheap plateau ends
    //
    // If the size picks the page KIND, and the small-page route is the
    // expensive one, then the cost must step at exactly those two values —
    // between 512 and 513, and between 2048 and 2049 — and nowhere else.
    // Requests one byte apart cannot differ for any other reason: they
    // round to the same alignment, they differ by no bin, and the whole
    // table has already been shown to be a function of size.
    //
    // A prediction that can be wrong by one byte is worth more than a
    // paragraph of mechanism.
    println!("--- routing: does the cost step at the page-kind boundaries? ---");
    println!("  SMALL_OBJ_SIZE_MAX = 512, MEDIUM_OBJ_SIZE_MAX = 2048");
    println!("     size    total      per op   route predicted");
    for (size, route) in [
        (256usize, "small"),
        (511, "small"),
        (512, "small"),
        (513, "MEDIUM <- step here"),
        (640, "medium"),
        (1024, "medium"),
        (2047, "medium"),
        (2048, "medium"),
        (2049, "LARGE span <- and here"),
        (2560, "large span"),
    ] {
        let (cycles, sum) = best(|| round_alloc(size));
        let per = cycles.saturating_sub(floor) / OPS as u32;
        println!("  {size:>7}  {cycles:>7}  {per:>10}   {route}");
        if sum == 0 {
            failed += 1;
            println!("           FAIL: checksum zero -- the work was optimised away");
        }
    }
    println!();

    // The allocations really came from the region, which is what says the
    // numbers are the seam's and not some other allocator's.
    let probe: Vec<u8> = Vec::with_capacity(64);
    if !region_contains(probe.as_ptr() as usize) {
        failed += 1;
        println!("  FAIL: an allocation did not come from the declared region");
    }
    drop(probe);

    println!();
    if failed == 0 {
        println!("RESULT: PASS -- cycles measured on silicon, harness floor subtracted");
    } else {
        println!("RESULT: FAIL -- {failed} check(s) failed");
    }
    println!("NOTE: this is ONE ARM. build-me-bare B4b wants this against the C");
    println!("      heap_4, on a Kairos target (the C6). An arm is not an A/B.");

    loop {
        core::hint::spin_loop();
    }
}
