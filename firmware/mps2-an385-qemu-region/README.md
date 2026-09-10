# mps2-an385-qemu-region — the seam on a Kairos target

The `small-metal` seam running on a **Cortex-M3**, which is what
`build-me-bare` B4 and the mission plan's K3 mean by a Kairos part. The
ESP32-S3 row beside this one proved the fixed-region backend on 32-bit
silicon and carried a caveat in capitals: *that is not a Kairos target*.
This one is.

```sh
cargo run --release      # the runner is qemu-system-arm on mps2-an385
```

No hardware. QEMU 11.1.0, `qemu-system-arm`, and the cell **exits with a
code** — `debug::exit(EXIT_SUCCESS)` from inside the guest — so it can be
a gate rather than something a person watches.

## Why `mps2-an385` and not `lm3s6965evb`

The plan named the LM3S6965 because FreeRTOS ships a QEMU demo for it. It
cannot host this seam, for a reason that is arithmetic rather than taste:

```text
MIN_REGION           65536 bytes   (one segment at this geometry)
LM3S6965 SRAM        65536 bytes   (the whole chip)
```

The smallest region the allocator will accept **is the entire SRAM**,
leaving nothing for the stack, `.data` or `.bss`. The AN385 is the same
core with megabytes, and FreeRTOS ships a QEMU demo for it too
(`CORTEX_MPS2_QEMU_*`), so the C arm of K3's A/B still exists. Same ISA,
same `thumbv7m-none-eabi` — and note that is **not** the
`thumbv7em-none-eabihf` every other bare-metal rung uses: the M3 has no
FPU.

## Measured, 2026-09-10

```text
rusty_alloc      2.1.0
budget asked     225280 bytes
region reserved  196608 bytes      <- three whole 64 KiB segments
give() -> 196608 bytes usable
second give -> FERR_REGISTERED
64 rounds of 32768 bytes = 2097152 total
  served from the region: 64/64
  landed on the first block's address again: 63/63
checks passed 9 / 9
RESULT: PASS       QEMU exit code: 0
```

Same numbers as the S3 row, which is the point: the seam's geometry is a
property of the configuration, not of the part.

## What this cell CANNOT do, measured rather than assumed

**It cannot supply a cycle count, a latency, or any work counter.** That
matters because B4's kill test asked for one and K3 asks for "cycle rows",
so it was worth establishing with evidence instead of a shrug.

| probe | 1000 iters | 2000 | 4000 |
|---|---|---|---|
| DWT `CYCCNT` | 0 | 0 | 0 |
| SysTick, plain, run 1 | 688 | 493 | 492 |
| SysTick, plain, run 2 | 667 | 490 | 317 |
| SysTick, plain, run 3 | 810 | 585 | 395 |
| SysTick, `-icount shift=0` | 1 | 0 | 0 |
| SysTick, `-icount shift=0,sleep=off` | 1 | 0 | 0 |

- **DWT is not implemented** on this machine: `CYCCNT` never moves.
- **SysTick is emulated, and its deltas are host wall time.** They
  *shrink* as the loop grows — the opposite of a work counter — because
  what they measure is the host while TCG's translation cache warms. They
  also differ run to run.
- **Under `-icount` the virtual clock does not advance** for SysTick here,
  so the deterministic-time route is closed too.
- The clock source is pinned to `Core` explicitly, so this is not a
  misconfigured peripheral: a negative result taken from one would not be
  a result.
- TCG **plugin support is compiled in but no plugin library ships** with
  the Windows QEMU build, so `libinsn` instruction counting is not
  available either.

**So the division of labour is:** QEMU cells prove *correctness* and gate
on an exit code; **cycle and latency rows need silicon** — the ESP32-C6 is
a Kairos target (`riscv32imac`) with a real `mcycle`. Flash and RAM
decomposition are unaffected: those are properties of the binary, not of
execution, so a QEMU cell can carry them.

This is the same rule the rest of the repo already runs on — the kernel's
speed work is judged on callgrind instruction counts and the corpus, never
on a clock.
