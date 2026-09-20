#!/bin/sh
# Instruction counts for the intrusive lists and the generational arena, under callgrind. Run inside WSL.
#
# The count is the verdict; a clock on this workload would be measuring the
# box. The checksum and the verdict counts are printed so a compiler that
# removed the work, or a change that altered a verdict, shows up as a changed
# number rather than as a good one.
#
# THE FRESHNESS CHECK IS NOT OPTIONAL. Its sibling in rusty_rtos_heap was
# wrong once -- it profiled a stale binary left by an earlier run and reported
# an identical count across a real source change, which is exactly what a
# stale binary looks like.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

bin=target/release/core-ir
cargo build --release
[ -f "$bin" ] || { echo "no binary at $bin" >&2; exit 1; }

# Every source that can change the count must be OLDER than the binary.
newest=$(find ../../crates/rusty_rtos_core/src src -name '*.rs' -newer "$bin" -print -quit)
if [ -n "$newest" ]; then
    echo "STALE: $newest is newer than $bin -- the build did not take" >&2
    exit 1
fi

rm -f callgrind.out
valgrind --tool=callgrind --callgrind-out-file=callgrind.out \
         --cache-sim=no --branch-sim=no "./$bin" 2>&1 | grep -E "checksum|reps|pairs|refs:"
echo "--- per file ---"
callgrind_annotate callgrind.out 2>/dev/null | sed -n '18,26p'

# ---- and again at 32 bits, because THAT is what the product runs on -------
#
# Measured 2026-09-19: the same workload is 21,229,486 instructions on x86-64
# and 25,535,514 on i686 -- 20% MORE -- because `Node::value` is a `u64` and a
# 32-bit machine needs two instructions where this host needs one. Every
# Kairos target is 32-bit: Cortex-M, RISC-V RV32, Xtensa LX7.
#
# A change measured only on the host can be flat here and a win there, or the
# reverse, so both arms are printed. Needs `rustup target add
# i686-unknown-linux-gnu` plus `gcc-multilib libc6-dev-i386`; skipped with a
# note rather than a failure when they are absent.
if cargo build --release --target i686-unknown-linux-gnu >/dev/null 2>&1; then
    b32=target/i686-unknown-linux-gnu/release/core-ir
    rm -f callgrind32.out
    echo "--- 32-bit ---"
    valgrind --tool=callgrind --callgrind-out-file=callgrind32.out              --cache-sim=no --branch-sim=no "./$b32" 2>&1 | grep -E "checksum|reps"
    grep -m1 '^summary:' callgrind32.out
else
    echo "--- 32-bit arm skipped: no i686 target or no 32-bit libc ---"
fi
