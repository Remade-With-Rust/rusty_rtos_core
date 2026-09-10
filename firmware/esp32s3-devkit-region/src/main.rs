//! The `small-metal` seam on silicon: give the allocator a static region,
//! then prove the bytes it hands back came out of it.
//!
//! `docs/HOUSE-STACK.md` has said of this seam, since it was written, that it
//! is "OK to compile ... **no board has run it**". This is the firmware that
//! changes that sentence, and it is deliberately the same shape as the two
//! board rows `docs/plans/build-me-bare.md` already carries for `rusty_zstd`
//! (B1) and `rusty_time-core` (B2): one part, one claim, printed checks, and
//! a caveat about what it does not prove.
//!
//! # What it does NOT prove
//!
//! **This is an ESP32-S3, which is not a Kairos target.** B4 wants a cycle
//! count from the Cortex-M3 QEMU cell, and no Xtensa row can supply it. What
//! this does is make the fixed-region backend real on 32-bit silicon, on the
//! same desk and the same part as B1 and B2 — and it is the first time any
//! board has run `rusty_rtos_alloc` at all.
//!
//! It also claims **no timing**. There is no cycle count here on purpose:
//! that number is K4's and it belongs on a Kairos part, measured against the
//! C `heap_4`.
//!
//! # The check that matters
//!
//! Printing "the allocation succeeded" would prove almost nothing — a global
//! allocator that quietly fell back to something else would print exactly the
//! same thing. So every allocation's address is put through
//! [`region_contains`], which answers against the base and length the region
//! registered when it was given. An allocation that did not come out of
//! `HEAP` fails that, and the run fails with it.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use esp_backtrace as _;
use esp_println::println;

use rusty_rtos_alloc::small_metal::{
    FERR_GEOMETRY, FERR_MISALIGNED, FERR_REGISTERED, FERR_TOO_SMALL, FIXED_PAGE, MIN_REGION,
    PrimError, REGION_ALIGN, Region, good_region_size, region_contains, region_stats,
};

// The image header the second-stage bootloader looks for.
esp_bootloader_esp_idf::esp_app_desc!();

/// The seam's allocator, and the whole of what a deliverable writes.
#[global_allocator]
static ALLOC: rusty_rtos_alloc::Alloc = rusty_rtos_alloc::Alloc;

/// What the crate docs' worked example asks for. `good_region_size` rounds it
/// down to whole segments, so the region is smaller than the budget and the
/// firmware says both numbers rather than the flattering one.
const BUDGET: usize = 220 * 1024;
type Heap = Region<{ good_region_size(BUDGET) }>;

static HEAP: Heap = Heap::new();

/// `PrimError` is a `u32`, so a bare integer would say only that the region
/// was refused. These are the four codes 2.1.0 made public, and re-exporting
/// them is half of what `build-me-bare` B3 was for.
fn why(e: PrimError) -> &'static str {
    match e {
        FERR_TOO_SMALL => "FERR_TOO_SMALL: the region cannot hold one segment",
        FERR_GEOMETRY => "FERR_GEOMETRY: not a whole number of segments",
        FERR_REGISTERED => "FERR_REGISTERED: a region was already given",
        FERR_MISALIGNED => "FERR_MISALIGNED: base is off the REGION_ALIGN grid",
        _ => "unknown PrimError",
    }
}

fn check(passed: &mut u32, failed: &mut u32, ok: bool, what: &str) {
    if ok {
        *passed += 1;
        println!("      ok    {what}");
    } else {
        *failed += 1;
        println!("      FAIL  {what}");
    }
}

#[esp_hal::main]
fn main() -> ! {
    let _p = esp_hal::init(esp_hal::Config::default());

    let (mut passed, mut failed) = (0u32, 0u32);

    println!();
    println!("=== rusty_rtos_alloc small-metal seam on ESP32-S3 (xtensa, no_std + alloc) ===");
    println!("rusty_alloc      {}", rusty_rtos_alloc::VERSION);
    println!("REGION_ALIGN     {REGION_ALIGN} bytes");
    println!("MIN_REGION       {MIN_REGION} bytes");
    println!("FIXED_PAGE       {FIXED_PAGE} bytes");
    println!("budget asked     {BUDGET} bytes");
    println!("region reserved  {} bytes", HEAP.len());
    println!("usable of that   {} bytes", Heap::USABLE);

    println!();
    println!("[1] give the allocator its region");
    let usable = match HEAP.give() {
        Ok(usable) => {
            println!("      give() -> {usable} bytes usable");
            usable
        }
        Err(e) => {
            println!("      give() -> {}", why(e));
            println!();
            println!("RESULT: FAIL -- the region was refused");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    check(&mut passed, &mut failed, usable > 0, "give answered a usable size");
    check(
        &mut passed,
        &mut failed,
        usable <= HEAP.len(),
        "usable does not exceed the region",
    );

    // Giving twice must be refused, and must say which way. This is the one
    // error path a firmware can reach on purpose, so it is worth pinning.
    println!();
    println!("[2] a second give is refused, and says why");
    match HEAP.give() {
        Err(FERR_REGISTERED) => check(&mut passed, &mut failed, true, "second give -> FERR_REGISTERED"),
        Err(e) => check(&mut passed, &mut failed, false, why(e)),
        Ok(_) => check(&mut passed, &mut failed, false, "second give was ACCEPTED"),
    }

    println!();
    println!("[3] allocations come out of THAT region");
    let boxed = Box::new(0xA5A5_5A5Au32);
    let addr = core::ptr::from_ref::<u32>(&boxed).addr();
    check(
        &mut passed,
        &mut failed,
        region_contains(addr),
        "a Box lands inside the region we gave",
    );
    check(&mut passed, &mut failed, *boxed == 0xA5A5_5A5A, "and holds its value");

    // A Vec that grows crosses several size classes and forces reallocation,
    // which is the path a single Box never touches.
    let mut v: Vec<u64> = Vec::new();
    for i in 0..4096u64 {
        v.push(i ^ 0x0F0F_0F0F_0F0F_0F0F);
    }
    let vaddr = v.as_ptr().addr();
    check(
        &mut passed,
        &mut failed,
        region_contains(vaddr),
        "a Vec grown to 32 KiB lands inside it too",
    );
    let sum = v.iter().fold(0u64, |a, b| a.wrapping_add(*b));
    let expect = (0..4096u64).fold(0u64, |a, b| a.wrapping_add(b ^ 0x0F0F_0F0F_0F0F_0F0F));
    check(&mut passed, &mut failed, sum == expect, "and every element survived");

    println!();
    println!("[4] freeing gives the bytes back");
    drop(v);
    drop(boxed);

    // This is the check that has to be able to FAIL, so it is sized to
    // outrun the region rather than to read a counter. Sixty-four rounds of
    // 32 KiB is 2 MiB against a 192 KiB region: a heap that reclaimed
    // nothing would be exhausted on the sixth, and the allocation would
    // fault or leave the region. Reading `region_stats` here instead looked
    // like a check and was not — its `free` counts extents the allocator
    // has not been handed yet, so dropping a `Box` never moves it.
    const ROUNDS: usize = 64;
    const BLOCK: usize = 32 * 1024;
    let mut reused = 0u32;
    let mut inside = 0u32;
    let mut first_addr = 0usize;
    for round in 0..ROUNDS {
        let mut block: Vec<u8> = Vec::new();
        block.resize(BLOCK, round as u8);
        let addr = block.as_ptr().addr();
        if region_contains(addr) {
            inside += 1;
        }
        if round == 0 {
            first_addr = addr;
        } else if addr == first_addr {
            reused += 1;
        }
        // Touch both ends so a lying allocator cannot hand back a short block.
        if block.first() == Some(&(round as u8)) && block.last() == Some(&(round as u8)) {
            // fine
        } else {
            failed += 1;
        }
        drop(block);
    }
    println!("      {ROUNDS} rounds of {BLOCK} bytes = {} total", ROUNDS * BLOCK);
    println!("      served from the region: {inside}/{ROUNDS}");
    println!("      landed on the first block's address again: {reused}/{}", ROUNDS - 1);
    check(
        &mut passed,
        &mut failed,
        inside as usize == ROUNDS,
        "every round was served, so the bytes were reclaimed each time",
    );
    check(
        &mut passed,
        &mut failed,
        reused as usize == ROUNDS - 1,
        "and each round reused the same address, which is reclamation not luck",
    );

    // `region_stats` is still worth printing -- it just is not a reclamation
    // check. `total` is what the allocator has taken from the region so far;
    // `free` is what is still unhanded.
    let (total, unhanded, _) = region_stats();
    println!("      region_stats: taken {total}, still unhanded {unhanded}");

    println!();
    println!("checks passed {passed} / {}", passed + failed);
    if failed == 0 {
        println!("RESULT: PASS -- the seam gave, served and reclaimed on the board");
    } else {
        println!("RESULT: FAIL -- {failed} check(s) failed");
    }
    println!("NOTE: this is an ESP32-S3, NOT a Kairos target. It claims no timing.");
    println!("      build-me-bare B4 still wants the Cortex-M3 QEMU cell.");

    loop {
        core::hint::spin_loop();
    }
}
