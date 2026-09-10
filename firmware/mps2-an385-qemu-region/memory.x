/* ARM MPS2 AN385. Chosen over the lm3s6965evb the plan first named for one
   measured reason: `MIN_REGION` is 64 KiB and the LM3S6965 has 64 KiB of
   SRAM in total, so the seam's smallest possible region is the whole chip.
   The AN385 has megabytes, and FreeRTOS ships a QEMU demo for it too, so
   the C arm of the A/B still exists. */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 4M
  RAM   : ORIGIN = 0x20000000, LENGTH = 4M
}
