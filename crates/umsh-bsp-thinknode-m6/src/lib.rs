#![no_std]

//! Board support for the Elecrow ThinkNode M6 (nRF52840 + SX1262 + L76K),
//! an outdoor solar node with two 18650 cells in parallel.
//!
//! Composes [`umsh-bsp-nrf52840`](../umsh_bsp_nrf52840/index.html) with
//! the M6's pinout and on-board peripherals. The radio path differs from
//! the SenseCAP Solar's in two places: there is no external RXEN (DIO2
//! alone switches the antenna), and the TCXO runs from DIO3 at **3.3 V**.
//!
//! - USB-CDC handles (over the nRF native USB peripheral, chip BSP)
//! - two LEDs and the PWR button
//! - battery measurement and charger status—see [`power`]
//! - SX1262 LoRa radio
//! - L76K GNSS power sequencing—see [`gnss`]
//! - low-battery System OFF with LPCOMP solar recovery—see [`shutdown`]
//!
//! See `docs/hardware/elecrow-thinknode-m6-hardware.md` for the hardware
//! reference and what remains unconfirmed.
//!
//! # Pin map (nRF52840, from the Meshtastic `ELECROW-ThinkNode-M6` variant)
//!
//! | Function                     | nRF pin      | Notes |
//! |------------------------------|--------------|-------|
//! | Radio CS                     | `P1.12`      | |
//! | Radio DIO1                   | `P1.06`      | |
//! | Radio BUSY                   | `P1.11`      | |
//! | Radio RESET                  | `P1.10`      | |
//! | Radio SPI SCK / MOSI / MISO  | `P1.13` / `P1.14` / `P1.15` | |
//! | Peripheral power enable      | `P0.27`      | driven high for the life of the program |
//! | GNSS enable                  | `P0.06`      | active-high |
//! | GNSS reset                   | `P0.29`      | active-low |
//! | GNSS standby                 | `P0.30`      | high wakes |
//! | GNSS PPS                     | `P0.31`      | unused |
//! | GNSS UART (MCU TX / MCU RX)  | `P0.02` / `P0.03` | NMEA 9600 |
//! | LED_POWER (red)              | `P0.12`      | active-high |
//! | LED_DATA (blue)              | `P0.07`      | active-high; heartbeat |
//! | PWR button                   | `P0.17`      | active-low, internal pull-up |
//! | USB power detect             | `P0.13`      | unused; VBUS comes from `POWER.USBREGSTATUS` |
//! | Charger status               | `P0.15`      | low while charging |
//! | Battery ADC                  | `P0.28`/AIN4 | ×1.75 divider |
//! | Battery divider enable       | `P0.11`      | active-high |
//! | I²C SDA / SCL                | `P1.09` / `P0.08` | expansion connector, PCF8563 RTC at 0x51 |
//! | QSPI flash power enable      | `P0.21`      | held low—the flash is unused |
//! | Secondary UART RX / TX       | `P0.22` / `P0.24` | expansion connector, unused |

#[cfg(target_os = "none")]
pub mod gnss;

#[cfg(target_os = "none")]
pub mod platform;

#[cfg(target_os = "none")]
pub mod power;

#[cfg(target_os = "none")]
pub mod shutdown;

#[cfg(target_os = "none")]
pub use platform::{ThinknodeM6Mac, ThinknodeM6Platform};

#[cfg(all(target_os = "none", feature = "ulcp-i2c"))]
pub mod i2c;

/// What the stock Meshtastic application reports; its bootloader is
/// `239A:00DA`.
pub const USB: umsh_bsp_nrf52840::usb::UsbIdentity = umsh_bsp_nrf52840::usb::UsbIdentity {
    vid: 0x239a,
    pid: 0x4405,
    product: "ThinkNode M6 UMSH Radio",
    serial_number: "thinknode-m6",
};
