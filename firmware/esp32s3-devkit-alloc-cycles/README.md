# esp32s3-devkit-alloc-cycles — the first cycle numbers this project has had

**Verified cell.** ESP32-S3 DevKit, `no_std`, real silicon.

```
cargo run --release        # with an S3 on USB
```

## What it measures

Cycles for one `alloc` + one `free` through `rusty_rtos_alloc`'s
`small-metal` seam, swept over allocation size, using the Xtensa **`CCOUNT`**
register — one tick per CPU cycle.

It exists because [`mps2-an385-qemu-region`](../mps2-an385-qemu-region)
established, six ways, that **QEMU supplies no cycle, latency or work
counter**: DWT unimplemented, SysTick tracking host wall time and *shrinking*
as work grows, nothing at all under `-icount`. A timing row needs silicon.

## The method

A cycle number without its method is not a number.

| | |
|---|---|
| **A null arm, subtracted** | The loop, the `black_box` and the `CCOUNT` reads cost cycles too. An identical empty round is measured the same way and taken off — 16 cycles/op here, printed so a reader can see how much of a small figure is floor. |
| **Best of 32 rounds** | The floor is what survives an interrupt landing mid-round. A mean would measure whatever else the chip did. |
| **8 warm-up rounds** | The first pass pulls the code from flash into the instruction cache. Measuring that measures the flash controller. |
| **A checksum** | Each round is a fixed count of identical operations and keeps a checksum, so a compiler that removed the allocation shows up as a changed checksum rather than a suspiciously good number. |
| **`region_contains`** | Proves the allocations came from the declared region, and not from some other allocator. |
| **A palindrome** | See below. This is the one that makes the table admissible. |

## The result

```
     size    total      per op   (harness removed)
       16    33570         115
       32    36146         125
       48    41653         146
       64    44370         157
       96    57746         209
      128    57746         209
      160    84498         314
      192    84498         314
      224    84498         314
      256    84498         314
      288    84498         314
      320    84498         314
      384    84498         314
      512    84498         314
      768    73490         271
     1024    73490         271
     2048    73490         271
```

Three flat plateaus, and **the 768–2048 plateau is 43 cycles CHEAPER than the
160–512 plateau below it** — a larger allocation costs 14% less than a smaller
one, monotonically on either side of the boundary.

## Why the palindrome is here

The first run of this cell measured only five sizes and showed 1024 bytes
costing *fewer* cycles than 256. The obvious reading is noise. It was not:
the totals were byte-identical across two flashes. So the sweep was widened
to 18 sizes rather than guessing, and that produced something a size-class
model cannot explain — **totals identical to the cycle across sizes 3× apart**
(160, 192, 224, 256, 288, 320, 384 and 512 all read exactly 84,498), though
those are eight *distinct* bins in the allocator's geometry.

The allocator's own source says why a geometric model will not fit. From
`rusty_alloc-2.1.0/src/alloc.rs`:

> a tight alloc/free loop frees into `local_free`, so the queue front's `free`
> list is ALWAYS dry when the next allocation arrives

This harness *is* that loop. Its fast path can never hit, so its cost is set
by the slow collect — whose frequency is a property of free-list **state**.
Which raises the question the table cannot answer about itself: *is this a
function of size at all, or of history?*

So the cell measures every size **twice, once ascending and once descending**,
and fails if a size does not reproduce its own total. A cost that depends on
size is symmetric; a cost that depends on what ran before it is not.

**All seventeen sizes up to 2048 reproduce their ascending total to the
cycle.** The table is a function of size, and the plateaus are real.

## The row that is NOT quoted

`4096` is `FIXED_PAGE`, so it cannot be served from a page at all and takes
the dedicated path. It measured **789, 861 and 933** cycles/op in three runs
whose only difference was which sizes ran before it. It is therefore *not* a
function of size, and this cell declares it an expected asymmetry — the way
the house gate declares its expected failure — so the check keeps its teeth
for the other seventeen. The number is not quoted anywhere.

Note the cell states where this control is weakest: the descending pass starts
at the largest size, so its 4,096 runs immediately after the ascending one and
sees nearly the same history. A symmetric run here is not a refutation.

## What this is NOT

It is **not** `build-me-bare` B4b and does not close it. B4b wants the
allocation-latency row **against the C `heap_4`**, on a Kairos target — the
ESP32-C6 (`riscv32imac`, real `mcycle`) or the Cortex-M3. This is one arm of
that comparison, on an Xtensa part, and **an arm is not an A/B**. It is here
because the family had no timing number from any silicon at all.

The plateau step-down is a property of a house dependency, not of Kairos. The
reproduction and the measurement are written up for the owner to file at
[`docs/upstream/rusty_alloc-size-class-inversion.md`](../../../docs/upstream/rusty_alloc-size-class-inversion.md);
this session does not touch upstream repositories.

## Hardware

An ESP32-S3 DevKit on USB. Everything else in `kairos check --qemu` needs no
board; this cell and [`esp32s3-devkit-region`](../esp32s3-devkit-region) are
the two that do.
