# esp32s3-devkit-region — the `small-metal` seam on silicon

The first firmware to run `rusty_rtos_alloc` on any board. It gives the
allocator a static `Region`, allocates and frees through the Rust global
allocator, and proves the bytes came out of *that* region.

```sh
cargo run --release            # the runner is `espflash flash --monitor`
# or:
cargo build --release
espflash flash --port COM4 --monitor target/xtensa-esp32s3-none-elf/release/esp32s3-devkit-region
```

## What it proves, and what it does not

**It is an ESP32-S3, which is not a Kairos target.** `build-me-bare` B4
wants a cycle count from the Cortex-M3 QEMU cell and no Xtensa row can
supply one. What this row does is make `rusty_alloc`'s fixed-region
backend real on 32-bit silicon — the same caveat, on the same desk and the
same part, that the B1 (`rusty_zstd`) and B2 (`rusty_time-core`) rows carry.

**It claims no timing.** There is no cycle count here on purpose: that
number is K4's, and it belongs on a Kairos part measured against the C
`heap_4`.

## The two checks that carry it

Everything else is arithmetic a host test could do. These two need a board:

- **`region_contains` on every allocation's address.** "The allocation
  succeeded" proves nothing — a global allocator quietly falling back to
  something else prints exactly the same thing. Each address is tested
  against the base and length the region registered when it was given.
- **64 rounds of a 32 KiB block against a 192 KiB region.** Two megabytes
  through a region that holds a tenth of it, so a heap that reclaimed
  nothing would be exhausted on the sixth round. All 64 are served, and 63
  of 63 land on the first block's address, which is reclamation rather
  than luck.

The first version of that second check read `region_stats()` and asserted
`free_after >= free_before`. It passed with `65536 == 65536` and **could
not have failed**: that field counts extents the allocator has not been
handed yet, so dropping a `Box` never moves it. It is printed now, and
labelled, but it is not the check.

## Measured, 2026-09-10

ESP32-S3 revision v0.2, 8 MB flash, 40 MHz crystal, MAC `68:ee:8f:51:74:64`.

```text
rusty_alloc      2.1.0
REGION_ALIGN     16 bytes
MIN_REGION       65536 bytes
budget asked     225280 bytes
region reserved  196608 bytes      <- three whole 64 KiB segments
give() -> 196608 bytes usable
second give -> FERR_REGISTERED
64 rounds of 32768 bytes = 2097152 total
  served from the region: 64/64
  landed on the first block's address again: 63/63
checks passed 9 / 9
RESULT: PASS
```

`good_region_size(220 KiB)` answering 196,608 is the crate docs' own worked
example — "three whole 64 KiB segments" — reproduced on the part.

## The three things that made it build

Worth writing down; each cost a cycle:

1. **`-C link-arg=-Tlinkall.x`.** Without it the GCC default script links a
   binary that builds fine and flashes nowhere: `.flash.appdesc` is never
   placed and espflash refuses the image with *"appdesc segment not
   found"*. esp-hal's own script has the `KEEP` for it — you have to name
   the script.
2. **`-C link-arg=-nostartfiles`** beside it. Removing it to chase (1)
   produced undefined references to `__exception` and the RTC BSS symbols:
   both flags are needed, not either.
3. **`esp_bootloader_esp_idf::esp_app_desc!()`.** esp-hal 1.2 images carry
   an ESP-IDF app descriptor or the second-stage bootloader will not take
   them.

And `esp-println` needs `default-features = false`: its default `auto`
collides with an explicit `jtag-serial`, and the build script rejects both.
