/* The minimum FreeRTOS configuration `heap_4.c` reads.
 *
 * heap_4.c is the only C file in this cell. It is compiled VERBATIM from
 * `oracle/FreeRTOS-Kernel/portable/MemMang/heap_4.c` -- not copied, not
 * edited -- so what is measured is FreeRTOS's allocator and not a
 * transcription of it. This header and the two shims beside it exist only
 * to satisfy its includes.
 */
#ifndef FREERTOS_CONFIG_H
#define FREERTOS_CONFIG_H

/* The same number of bytes the Rust arm is given, so the two allocators
 * are working the same memory. The cell PRINTS both usable figures, so a
 * drift between this line and `BUDGET` in main.rs is visible rather than
 * silent.
 *
 * 64 KiB and not the 192 KiB the single-arm cell used, for a boring
 * reason: two 192 KiB static heaps do not fit in the S3's DRAM, and the
 * link fails with "stack.x:13 cannot move location counter backwards".
 * It costs the measurement nothing -- the harness holds exactly ONE
 * allocation live at a time, so heap size is not on the measured path --
 * and the cell checks that claim rather than asserting it, by comparing
 * its Rust arm against the 192 KiB cell's published numbers. */
#define configTOTAL_HEAP_SIZE            ( ( size_t ) 65536 )

/* Xtensa's ABI alignment. Reported by the cell rather than assumed to
 * match the Rust arm's: the two allocators have different geometry, and
 * what each one charges for a request is part of the answer. */
#define portBYTE_ALIGNMENT               8

#define configAPPLICATION_ALLOCATED_HEAP 0
#define configUSE_MALLOC_FAILED_HOOK     0
#define configENABLE_HEAP_PROTECTOR      0
#define configSUPPORT_DYNAMIC_ALLOCATION 1

#endif /* FREERTOS_CONFIG_H */
