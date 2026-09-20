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

# ---- PINNED, 2026-09-19 ---------------------------------------------------
#
# CURRENT, at the u64 default:  x86_64 16,781,246   i686 19,230,865
#
# Down from 18,595,310 and 20,990,785 (-9.75% and -8.38%) across three wins,
# all of them the same move: collapse two ADJACENT lookups of the same `End`
# into one, in `insert_end`, `link_between` and `iter`. See the module doc in
# list.rs for those and for the six things that looked identical and lost.
#
# The width table below was taken at the 18,595,310 baseline. Its RATIOS are
# what it is for, and they have not been re-taken since:
#
#   key   Node size        x86_64        i686      vs the u64 default
#   u16      8 bytes   18,333,152  19,896,655      -1.41%   -5.21%
#   u64     16 bytes   18,595,310  20,990,785       base     base
#   u32     12 bytes   19,395,334  22,176,946      +4.30%   +5.65%
#
# THE NARROW KEY IS NOT MONOTONE IN WIDTH, and the obvious choice for a
# 32-bit kernel is the worst of the three. A `u32` key was supposed to be the
# win -- every Kairos target is 32-bit, where a 64-bit compare costs two
# instructions -- and it is +4.3% and +5.7% against the `u64` it was meant to
# beat, while `u16` is -1.4% and -5.2%.
#
# TWO mechanisms were proposed for that ordering. BOTH were refuted by their
# own predictions, and the ordering outlived both:
#
#   * "It is the power-of-two stride" (16 and 8 index by a shift, 12 by a
#     multiply). Then padding `Node<u32>` to 16 bytes should recover it on
#     both machines. Measured: +3.5% on x86-64 and -3.15% on i686 -- OPPOSITE
#     SIGNS -- and even the arm it helped stayed +0.77% behind u64. Worse,
#     forcing `Node<u16>` to 16 bytes read +14.1% and landed WORSE than
#     `Node<u64>` at the identical 16-byte stride, which stride cannot
#     explain at all.
#   * "It is `Lists::new()`, whose cost scales with the node SIZE, charged
#     2,000 times by a rep loop that rebuilt the structure." That one was a
#     genuine defect in THIS instrument and it is fixed -- construction is
#     hoisted, and it was worth 0.2% to 3.2% depending on the arm. The table
#     above is what survived the fix. Not the explanation either.
#
# So the ordering is measured and its mechanism is NOT known. It is recorded
# that way deliberately. A wrong mechanism in a comment is worse than an
# admitted gap, because the next person optimises against it -- which is
# exactly how the -5.12% that started this thread came to be believed.
echo "--- list-ir: instructions, checksum ---"
run ""       ""                         "x86_64-u64"
run "narrow" ""                         "x86_64-u32"
run "tiny"   ""                         "x86_64-u16"
run ""       "i686-unknown-linux-gnu"   "i686-u64"
run "narrow" "i686-unknown-linux-gnu"   "i686-u32"
run "tiny"   "i686-unknown-linux-gnu"   "i686-u16"
echo "(checksum and the anchors must be identical in ALL SIX; the numbers are"
echo " the price of the width and of the machine.)"
