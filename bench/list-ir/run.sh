#!/bin/sh
# Instruction counts for the intrusive lists alone, under callgrind. Run inside WSL.
#
# FOUR ARMS, because the answer differs along both axes and a change measured
# on one of them is a change measured on a machine nobody ships:
#
#   * x86-64 vs i686 -- every Kairos target (Cortex-M, RV32, Xtensa LX7) is a
#     32-bit machine, where a 64-bit compare is two instructions and a 64-bit
#     load is two loads. The same workload is ~20% dearer at i686.
#   * u64 key vs u32 key -- `TickWidth` offers 16, 32 and 64 bits, and the
#     difference between these two arms IS the price of the width.
#
# THE FRESHNESS CHECK IS NOT OPTIONAL. Its sibling in rusty_rtos_heap was
# wrong once: it profiled a stale binary left by an earlier run and reported an
# identical count across a real source change, which is exactly what a stale
# binary looks like. The `narrow` feature makes this sharper, not softer --
# the two features share one `target/` path, so the LAST build wins and an
# unguarded script will happily profile the other arm's binary and call it a
# result.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

run() {
    feat=$1
    tgt=$2
    label=$3
    # The label names a file, so a `/` in it silently points callgrind at a
    # directory that does not exist. That failure is INVISIBLE: valgrind
    # errors, `set -e` takes the script down inside a command substitution,
    # and the run exits 0 having printed only its header. Refuse it loudly.
    case "$label" in
        */*) echo "label '$label' contains a slash and names a file" >&2; exit 1 ;;
    esac

    if [ -n "$tgt" ]; then
        set -- --target "$tgt"
        bin="target/$tgt/release/list-ir"
    else
        set --
        bin="target/release/list-ir"
    fi
    # `[ ... ] && set -- ...` would be a FAILING and-list when `$feat` is
    # empty, and `set -e` would take the whole script down with it -- silently,
    # after printing the header and nothing else.
    if [ -n "$feat" ]; then
        set -- "$@" --features "$feat"
    fi

    rm -f "$bin"
    if ! cargo build --release "$@" >/dev/null 2>&1; then
        echo "$label  SKIPPED (no toolchain: needs rustup target add $tgt plus gcc-multilib libc6-dev-i386)"
        return 0
    fi
    [ -f "$bin" ] || { echo "$label  SKIPPED (no binary at $bin)"; return 0; }

    # Every source that can change the count must be OLDER than the binary.
    stale=$(find ../../crates/rusty_rtos_core/src src Cargo.toml -newer "$bin" -print -quit)
    if [ -n "$stale" ]; then
        echo "STALE: $stale is newer than $bin -- the build did not take" >&2
        exit 1
    fi

    out="callgrind-$label.out"
    rm -f "$out"
    txt=$(valgrind --tool=callgrind --callgrind-out-file="$out" \
                   --cache-sim=no --branch-sim=no "./$bin" 2>/dev/null)
    n=$(grep -m1 '^summary:' "$out" | awk '{print $2}')
    printf '%-12s %12s   %s\n' "$label" "$n" "$(echo "$txt" | head -1)"
}

# ---- PINNED, 2026-09-19, and the finding that came with it ----------------
#
#   key   Node size        x86_64        i686
#   u16      8 = 2^3   18,365,152  19,994,581
#   u64     16 = 2^4   18,769,212  21,528,560
#   u32     12            20,039,224  22,400,584
#
# THE NARROW KEY IS NOT MONOTONE IN WIDTH, and the obvious choice is the
# worst one. A `u32` key is +6.8% on x86-64 and +4.0% on i686 against the
# `u64` it was supposed to beat, while a `u16` key is -2.2% and -7.1%.
#
# Width is not the lever; the node's SIZE is. `Node<u64>` lays out as 16
# bytes and `Node<u16>` as 8 -- both powers of two, so `items[i]` is a shift.
# `Node<u32>` is 4 + 2 + 2 + 1 = 9, rounded to 12, and 12 is the one size
# that needs a multiply. Every single node access pays it, and on this
# workload that outweighs the narrower compare it was bought for.
#
# A ceiling probe on core-ir had read -5.12% for the same idea. It was
# measuring something else.
echo "--- list-ir: instructions, checksum ---"
run ""       ""                         "x86_64-u64"
run "narrow" ""                         "x86_64-u32"
run "tiny"   ""                         "x86_64-u16"
run ""       "i686-unknown-linux-gnu"   "i686-u64"
run "narrow" "i686-unknown-linux-gnu"   "i686-u32"
run "tiny"   "i686-unknown-linux-gnu"   "i686-u16"
echo "(checksum and the anchors must be identical in ALL SIX; the numbers are"
echo " the price of the width and of the machine.)"
