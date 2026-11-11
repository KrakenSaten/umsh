# Elecrow ThinkNode M6 hardware reference

The ThinkNode M6 is an outdoor solar node: an nRF52840, a Semtech SX1262,
a Quectel L76K GNSS receiver, a 6 W panel, and two 18650 cells in parallel
(one series cell, 7000 mAh), in a weatherproof enclosure with two SP11
aviation connectors (I²C and UART). Elecrow sells it as model LMM14106D.

Elecrow publishes no schematic. The pin map below comes from the
Meshtastic `ELECROW-ThinkNode-M6` variant, and the bootloader facts from a
retail unit. Firmware support lives in `crates/umsh-bsp-thinknode-m6` and
the `board-thinknode-m6` feature of the shared nRF52840 tracker sources.

## Bootloader

Read from `INFO_UF2.TXT` and `CURRENT.UF2` on a retail unit (2026-11-11):

| Field | Value |
|---|---|
| Bootloader | UF2 Bootloader 0.9.2-6-g3a2708b-dirty |
| Model | ELECROW ThinkNode M6 for Meshtastic |
| Board-ID | `nRF52840-ThinkNodeM6-v1` |
| SoftDevice | S140 6.1.1, so the application starts at `0x26000` |
| UF2 family | `0x239A00DA` (the bootloader's own USB ID, not `0xADA52840`) |
| Volume name | `ThinkNodeM6` |
| Bootloader USB ID | `239A:00DA` |
| Stock application USB ID | `239A:4405` |
| UF2-writable window | ends at `0xEA000` (end of `CURRENT.UF2`) |

A double press of RESET within 250 ms enters the bootloader.

## Pin map

| Function | nRF pin | Notes |
|---|---|---|
| Radio CS | P1.12 | |
| Radio DIO1 | P1.06 | |
| Radio BUSY | P1.11 | |
| Radio RESET | P1.10 | |
| Radio SPI SCK / MOSI / MISO | P1.13 / P1.14 / P1.15 | |
| Radio RF switch | DIO2 | no RXEN/TXEN pins |
| Radio TCXO | DIO3, 3.3 V | |
| Peripheral power enable | P0.27 | loads not documented; held high while running |
| GNSS enable | P0.06 | active-high |
| GNSS reset | P0.29 | active-low |
| GNSS standby | P0.30 | high wakes |
| GNSS PPS | P0.31 | unused |
| GNSS UART (MCU TX / MCU RX) | P0.02 / P0.03 | 9600 baud |
| LED, red (power) | P0.12 | active-high; attention LED |
| LED, blue (data) | P0.07 | active-high; heartbeat |
| PWR button | P0.17 | active-low, internal pull-up |
| USB power detect | P0.13 | unused; VBUS comes from `POWER.USBREGSTATUS` |
| Charger status | P0.15 | low while charging |
| Battery ADC | P0.28 / AIN4 | ×1.75 divider |
| Battery divider enable | P0.11 | active-high |
| I²C SDA / SCL | P1.09 / P0.08 | expansion connector; PCF8563 RTC at 0x51 |
| QSPI flash (MX25R1635F) | SCK P1.03, CS P0.23, IO0–IO3 P1.01 / P1.02 / P1.04 / P1.05 | unused |
| QSPI flash power enable | P0.21 | held low |
| Secondary UART RX / TX | P0.22 / P0.24 | expansion connector; unused |

## Behavior

- **PWR** short press: the primary action (beacon, or silence the locate
  alert). Hold for four seconds: power off. Press from off: power on.
  Held for a second through power-on: force the Bluetooth pairing window.
- **Blue LED**: heartbeat and pairing blink. **Red LED**: locate alert,
  confirmations, and the power-off acknowledgement.
- GNSS runs by default, as on the SenseCAP Solar.
- A sustained critical cell (≈3.1 V) powers the node off with the divider
  left connected and LPCOMP armed on AIN4 at 5/8 VDD, so the panel brings
  it back at ≈3.61 V of cell. PWR also wakes it.

## Confirmed on hardware

On a retail unit, 2026-11-11:

- The image boots from the UF2 above and enumerates as `239A:4405`.
- The SX1262 transmits on this pin map with the 3.3 V TCXO: frames
  complete, with no radio resets.
- The I²C bus answers on P1.09/P0.08, with the PCF8563 at 0x51.
- The battery reads 4183 mV on USB with a full pack, and the boot gate
  passes.

## Unconfirmed

Everything below is taken from the vendor's firmware and has not been
measured:

- what P0.27 powers,
- the battery divider's ratio (the vendor's 1.75 is not a fitted
  calibration) and whether P0.11 switches its high or low side,
- that VDD is regulated at 3.3 V, which the LPCOMP wake point assumes,
- the GNSS UART direction (no fix yet: the receiver runs, but a fix needs
  open sky),
- LoRa reception,
- the PWR button, the LEDs, and System OFF with its LPCOMP wake,
- whether P0.15 reports USB charging as well as solar charging.

## Not yet supported

- The PCF8563 RTC. The GNSS rail is cut when the receiver is off, so
  nothing keeps time across a power-off until the RTC is read at boot.
- The QSPI flash.
