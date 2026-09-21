//! Compile FreeRTOS's `heap_4.c` for the S3, verbatim.
//!
//! The point of this cell is an A/B, and an A/B needs the real other arm.
//! `heap_4.c` is taken straight from the oracle checkout — the same
//! `FreeRTOS-Kernel` the conformance traces come from — and is neither
//! copied into this tree nor edited. Only its includes are supplied, by
//! the three shims in `csrc/`.

use std::path::PathBuf;

fn main() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Up to the umbrella, then into the oracle's kernel checkout. Named
    // once, here, so a moved oracle is a build error and not a silently
    // stale copy of the file.
    let heap_4 = here
        .join("../../../oracle/FreeRTOS-Kernel/portable/MemMang/heap_4.c")
        .canonicalize()
        .expect("the oracle's heap_4.c: run `kairos oracle` first");

    println!("cargo:rerun-if-changed={}", heap_4.display());
    println!("cargo:rerun-if-changed=csrc");

    cc::Build::new()
        // The **part-specific** driver, not the generic `xtensa-esp-elf-gcc`.
        // Xtensa is a configurable core: the generic driver builds for a
        // different configuration and the link fails with "compiled for a
        // big endian system and target is little endian". This is the same
        // driver rustc itself picks as the linker for this target, which is
        // the check to make — compile the C arm with whatever links the
        // Rust one.
        .compiler("xtensa-esp32s3-elf-gcc")
        // ESP32-S3 code is linked across a 4 MiB flash map, further than a
        // bare `call8` can reach.
        .flag("-mlongcalls")
        .file(&heap_4)
        .include(here.join("csrc"))
        // -O2 is what FreeRTOS ships under and what a firmware would use.
        // The Rust arm is `opt-level = "s"` with LTO; neither arm is given
        // a flag the other lacks beyond its own toolchain's idiom, and the
        // cell prints both so the reader can judge.
        .opt_level(2)
        .flag("-ffunction-sections")
        .flag("-fdata-sections")
        .warnings(false)
        .compile("heap_4");

    // The method line must be DERIVED, not asserted. It used to read
    // `opt-level=s` from a string literal, so building this cell at any
    // other optimisation level produced a report that said otherwise --
    // a benchmark lying about how it was built, which is the one thing a
    // printed method line exists to prevent. Cargo hands both of these to
    // the build script; pass them through and let the cell read them back.
    println!(
        "cargo:rustc-env=AB_OPT_LEVEL={}",
        std::env::var("OPT_LEVEL").unwrap_or_else(|_| "?".into())
    );
    // Cargo does not hand `overflow-checks` to a build script, and
    // `cfg!(overflow_checks)` is still unstable — so read the manifest,
    // which is the setting's own source of truth. Anything derived beats
    // anything asserted here.
    let manifest = std::fs::read_to_string(here.join("Cargo.toml")).unwrap_or_default();
    let overflow = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("overflow-checks").map(|v| v.trim_start_matches([' ', '=']).trim().to_owned()))
        .unwrap_or_else(|| "?".into());
    println!("cargo:rustc-env=AB_OVERFLOW_CHECKS={overflow}");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
