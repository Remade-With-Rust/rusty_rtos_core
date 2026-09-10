# esp32s3-devkit-alloc-ab — rusty_alloc vs FreeRTOS heap_4, on silicon

**Verified cell.** ESP32-S3 DevKit on USB.

```sh
cargo run --release
```

This is the A/B `build-me-bare` **B4b** asks for. The sibling
[`esp32s3-devkit-alloc-cycles`](../esp32s3-devkit-alloc-cycles) measured one
arm and said so everywhere it could — and the thing that was missing was
never the board. It was the **second allocator**.

`heap_4.c` is portable C over a static byte array with no port dependency at
all, so it compiles for this part with the esp toolchain's
`xtensa-esp32s3-elf-gcc` and runs under the identical harness.

## The result

```text
     size   rust c/op    heap_4 c/op        ratio   heap_4 charged B
       16         99            236        2.38x                24
       32        109            236        2.17x                40
       64        141            236        1.67x                72
      128        193            236        1.22x               136
      256        298            236        0.79x               264
      512        298            236        0.79x               520
     1024        255            236        0.93x              1032
     2048        255            236        0.93x              2056
```

`rusty_alloc` is **1.2×–2.4× faster below 256 bytes** and **1.08×–1.27×
slower from 256 to 2048**.

## …but a flat heap_4 number is its best case, so here is the other half

`heap_4` reads 236 cycles at *every* size, and that is the tell. This
workload keeps exactly **one block live**, so heap_4's free list is one
entry and its first-fit walk answers in one step. Its cost is O(free-list
length) by construction; `rusty_alloc`'s size-class pop is not.

512-byte requests against a free list of 16-byte holes:

```text
    holes   rust c/op    heap_4 c/op        ratio
        0        297            235        0.79x
        8        297            466        1.57x
       32        297           1114        3.75x
      128        297           3706       12.48x
```

`rusty_alloc` is **flat at 297**. `heap_4` degrades **linearly at ~27 cycles
per free-list entry walked** (+28.9, +27.5, +27.1 per hole) and has lost its
advantage by **eight holes**. These are different floor functions, not one
function tuned differently:

```text
heap_4:      cost = c0 + 27 x (entries walked past)
rusty_alloc: cost = f(size), independent of heap state
```

**The first version of this probe was refuted, and that is worth recording.**
It fragmented with holes of the *same* size as the request, and heap_4 got
**faster** — 236 → 218 — because first-fit stops at the first block that
fits, and a same-size hole fits immediately. A long free list only costs
something when the allocator has to walk *past* it. The holes must be
smaller than the request.

## What makes this an A/B and not two numbers

| | |
|---|---|
| **The C arm is FreeRTOS's file, verbatim** | `build.rs` compiles `oracle/FreeRTOS-Kernel/portable/MemMang/heap_4.c` straight out of the oracle checkout — the same kernel the conformance traces come from. Not copied into this tree, not edited. Only its includes are supplied, by three shims in `csrc/` |
| **Same memory** | both allocators get 64 KiB; the cell prints both usable figures so a drift is visible |
| **Same call shape** | both called *directly*, `alloc::alloc::alloc(Layout)` / `dealloc` against `pvPortMalloc` / `vPortFree`, at the same 8-byte `portBYTE_ALIGNMENT`. No `Vec` on one side and raw calls on the other |
| **Symmetric locking, i.e. none** | heap_4 brackets its work in `vTaskSuspendAll`/`xTaskResumeAll`; the shims make those no-ops, because the Rust arm is `--cfg ra_single_threaded` and takes no lock either. A C arm paying for a scheduler lock the Rust arm does not pay for would be measuring the lock |
| **`configASSERT` traps** | a no-op assert would let heap_4 violate its own invariants silently and still produce a cycle number |
| **Interleaved, not blocked** | ABBA, so drift passes through both arms equally |
| **A null A/B** | the Rust arm against *itself*, presented as two allocators: **37,963 vs 37,963, delta 0**. The resolution floor is zero cycles, so every difference above counts |
| **Work parity checked** | both arms compute the same checksum from the same sequence, and the run FAILS if they differ |
| **The null arm is subtracted** | 11 cycles/op of harness, printed |

## What it does not claim

- **`heap_4`'s charge in bytes is measured; `rusty_alloc`'s is not.** heap_4
  charges request + 8 exactly, read from `xPortGetFreeHeapSize()` either side
  of one allocation. There is no comparable number for the Rust arm: the seam
  does not expose a per-allocation usable size, and `region_stats()` answers
  over region *extents* so it does not move for a small allocation. Reaching
  around the seam to `rusty_alloc::alloc::usable_size` is the one thing the
  seam exists to prevent. **Recorded as a seam gap** rather than turned into
  half a comparison.
- **It is not on a Kairos target.** B4b's kill test names the ESP32-C6 —
  `riscv32imac`, real `mcycle`. Xtensa is not one of the family's four
  targets. This closes the *comparison* and leaves the *target* clause open,
  the same way B4a substituted `mps2-an385` for the `lm3s6965evb` the plan
  named, and recorded why.

## Three things the build needed

- **`xtensa-esp32s3-elf-gcc`, not `xtensa-esp-elf-gcc`.** Xtensa is a
  configurable core: the generic driver builds a different configuration and
  the link fails with *"compiled for a big endian system and target is little
  endian"*. The check is to compile the C arm with whatever links the Rust
  one — which is the driver rustc itself picks for this target.
- **`-mlongcalls`**, because S3 code is linked across a 4 MiB flash map.
- **64 KiB heaps, not 192 KiB.** Two 192 KiB static heaps do not fit in the
  S3's DRAM; the link fails with *"stack.x:13 cannot move location counter
  backwards"*. It costs the measurement nothing — the harness holds one
  allocation live at a time, so heap size is not on the measured path — and
  that claim is **checked rather than asserted**: the Rust arm's per-size
  numbers here differ from the 192 KiB cell's published ones by a *constant*
  16 cycles across every size, with the plateau structure identical. A
  constant offset is the harness difference (`Vec` versus a direct call), not
  a heap-size effect.

## Where the loss comes from

The only range `rusty_alloc` loses is 256–2048, and the
[cycles cell](../esp32s3-devkit-alloc-cycles) shows that is a **routing**
boundary, not a gradual effect: the cost steps 43 cycles between a 512-byte
request and a 513-byte one, at `SMALL_OBJ_SIZE_MAX`. Closing that gap would
put 256–512 at roughly 255 cycles and turn the one losing range into a win.
Written up for the allocator's owner in
[`docs/upstream/rusty_alloc-size-class-inversion.md`](../../../docs/upstream/rusty_alloc-size-class-inversion.md).
