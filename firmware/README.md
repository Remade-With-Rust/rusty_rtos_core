# firmware/

Per-chip example projects for `rusty_rtos_core`. Each directory here is a **separate
cargo project**, excluded from the workspace, because every chip needs its own
target triple, linker script and (for Xtensa parts) its own toolchain. n0's
iroh-on-ESP32 work and the Janus family both reached the same conclusion: keep
the firmware projects out of the library workspace so architecture-specific
patches never leak into it.

Naming: `<board>-<demo>/`, for example `lm3s6965-qemu-flash/` or
`esp32c6-devkitc-blink/`.

| Chip class | Runtime | Target |
|---|---|---|
| Cortex-M3 (QEMU `lm3s6965evb`) | `cortex-m-rt` + `rusty_rtos_port-cortex-m` | `thumbv7m-none-eabi` |
| Cortex-M4F / M7 | same | `thumbv7em-none-eabihf` |
| Cortex-M33 | same | `thumbv8m.main-none-eabihf` |
| RISC-V RV32 (QEMU `virt`) | `riscv-rt` + `rusty_rtos_port-riscv` | `riscv32imac-unknown-none-elf` |
| ESP32-C6 / P4 | `esp-hal` + `rusty_rtos_port-riscv` | `riscv32imac-unknown-none-elf` / `riscv32imafc-unknown-none-elf` |
| ESP32 / ESP32-S3 | `esp-hal` (esp toolchain) + `rusty_rtos_port-xtensa` | `xtensa-esp32-none-elf` / `xtensa-esp32s3-none-elf` |

Rules:

- Depend on this repo's crates by **path** (`../../crates/rusty_rtos_core`) inside a
  firmware example; depend on siblings by git URL as usual.
- Release profile for a chip: `opt-level = "s"` (or `"z"`), `lto = true`,
  `codegen-units = 1`, `panic = "abort"`, `overflow-checks = true`.
- A firmware example is not a test. The library's tests run on the host and
  on the sim port.

## The cells

| cell | what it proves | needs |
|---|---|---|
| [`esp32s3-devkit-region`](esp32s3-devkit-region) | the `small-metal` seam on silicon, 9/9 | an **ESP32-S3** on USB |
| [`mps2-an385-qemu-region`](mps2-an385-qemu-region) | the same seam on a Cortex-M3, 9/9, plus the footprint decomposition and the six probes that establish QEMU has no cycle counter | nothing — `kairos check --qemu` |
| [`esp32s3-devkit-alloc-cycles`](esp32s3-devkit-alloc-cycles) | cycles per `alloc`+`free` against `CCOUNT`, with the palindrome control that says the table is a function of size, and the one-byte experiment that shows the cost is **routing** | an **ESP32-S3** on USB |
| [`esp32s3-devkit-alloc-ab`](esp32s3-devkit-alloc-ab) | `rusty_alloc` against FreeRTOS's own `heap_4`, compiled verbatim from the oracle — the A/B `build-me-bare` B4b asks for | an **ESP32-S3** on USB |

`kairos check --qemu` discovers cells by their runner: a `qemu-system-*` one
needs only this box and is run; an `espflash` one wants a board on a serial
port and is never started by a gate. That is why the two S3 cells above are
run by hand and the M3 one gates.
