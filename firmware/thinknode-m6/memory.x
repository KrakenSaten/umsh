/* Elecrow ThinkNode M6: Adafruit UF2 bootloader 0.9.2 with S140 v6.1.1,
 * so the application starts at 0x26000 like the T-Echo's (confirmed from
 * INFO_UF2.TXT on a retail unit, 2026-11-11). The final 64 KiB of
 * application space (0xE4000..0xF4000) is dedicated to ULCP security
 * persistence, same as the other nRF52840 device boards—see
 * nrf52-tracker's proto_store.rs, which hard-codes those page addresses.
 *
 * The bootloader's CURRENT.UF2 ends at 0xEA000, so its UF2-writable window
 * stops there, as on the XIAO. FLASH below ends at 0xE4000, so a linked
 * image can never reach that ceiling.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00026000, LENGTH = 760K
  RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}
