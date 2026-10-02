#!/bin/sh
# Build FreeRTOS's own list.c (the pinned oracle, unmodified) into the list
# differential driver and write `list.trace`, which
# `crates/rusty_rtos_core/tests/list_differential.rs` replays. Under WSL:
#   wsl -e sh -c 'cd /mnt/f/coding/rusty_RTOS/rusty_rtos_core/oracle/list && sh run.sh'
set -eu
here=$(cd "$(dirname "$0")" && pwd)
kernel="$here/../../../oracle/FreeRTOS-Kernel"
[ -f "$kernel/list.c" ] || { echo "no FreeRTOS-Kernel at $kernel -- run \`kairos oracle fetch\` first" >&2; exit 1; }
cc -O1 -g -Wall -I "$here" -I "$kernel/include" -o "$here/list_driver" "$kernel/list.c" "$here/driver.c"
"$here/list_driver" > "$here/list.trace"
echo "wrote $(wc -l < "$here/list.trace") lines to $here/list.trace"
