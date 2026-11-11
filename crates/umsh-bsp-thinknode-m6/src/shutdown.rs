//! Controlled power-off for the ThinkNode M6.
//!
//! The board has one user button, PWR (P0.17, active-low). The power
//! *policy*—hold-to-off with a blink acknowledgement—lives in the
//! firmware's button task; this module only performs the System OFF
//! teardown once [`SHUTDOWN_SIGNAL`](crate::power::SHUTDOWN_SIGNAL) fires:
//!
//! - hold the SX1262 in reset (P1.10 low) and drop the peripheral rail
//!   (P0.27 low),
//! - settle the battery divider's enable (P0.11) one way or the other—see
//!   below,
//! - keep GNSS powered down (enable P0.06, reset P0.29, standby P0.30 all
//!   low),
//! - tri-state every remaining peripheral signal pin so embassy's leftover
//!   SENSE bits can't fire DETECT and reverse current can't leak into an
//!   unpowered peripheral,
//! - arm PWR as the GPIO-DETECT wake source (SENSE low).
//!
//! Every pin left driving is driven *low*, so none of them can feed an
//! unpowered peripheral whatever the peripheral rail turns out to supply.
//!
//! ## Two ways down, and the divider decides
//!
//! - **[`ShutdownReason::Requested`]**—somebody turned the node off.
//!   Drive P0.11 LOW to disconnect the divider. Off means off until PWR
//!   is pressed.
//! - **[`ShutdownReason::BatteryCritical`]**—the cell ran down. Drive
//!   P0.11 HIGH instead, keeping the divider connected, and arm LPCOMP on
//!   AIN4 (`P0.28`, the tap) against 5/8 VDD with upward detection.
//!
//! Where the threshold lands: the cell is the tap × 1.75, and 5/8 of a
//! regulated 3.3 V VDD puts the tap threshold at 2.06 V, so the crossing is
//! at **≈3.61 V of cell**—above the 3.3 V boot gate and well clear of
//! Critical (≈3.1 V). If VDD instead tracks the cell, the tap sits at
//! 1/1.75 = 0.57 of VDD, below 5/8, and the comparator never trips: the
//! failure is a node that waits for a press, never a wake loop.
//!
//! PWR stays armed on either path, so the LPCOMP wake is strictly an
//! addition.

use embassy_time::{Duration, Timer};
use umsh_bsp_nrf52840::system_off::{
    LpcompInput, LpcompReference, Port, ShutdownReason, WakePin, WakePull, WakeSense,
    arm_lpcomp_wake_up, configure_wake_input, connect_input, drive_pin_high, drive_pin_low,
    enter_system_off, read_pin, tristate_pin,
};

/// The PWR button: P0.17, active-low with a pull-up.
const PWR_BUTTON: (Port, u8) = (Port::P0, 17);

/// The teardown, for a caller that already knows the reason: the firmware's
/// shutdown task, and its boot gate when the cell is too low to start on.
pub async fn enter_off(reason: ShutdownReason) -> ! {
    // PWR is the wake source. If it is still held (LOW) when WakeSense::Low
    // is armed, DETECT fires immediately and the chip wakes right back
    // up—the power-off hold leaves it down. Connect the input buffer (it
    // may still be at its reset configuration, where IN reads 0 regardless
    // of the pad), then wait for the release.
    let (port, pin) = PWR_BUTTON;
    connect_input(port, pin, WakePull::Up);
    while !read_pin(port, pin) {
        Timer::after(Duration::from_millis(50)).await;
    }
    Timer::after(Duration::from_millis(50)).await;

    // Hold the SX1262 in reset (active-low), then drop the peripheral rail.
    drive_pin_low(Port::P1, 10);
    cortex_m::asm::delay(640); // ~10 µs @ 64 MHz
    drive_pin_low(Port::P0, 27);

    // The divider enable, driven either way—a driven output retains its
    // level through System OFF.
    let battery_recovery = reason == ShutdownReason::BatteryCritical;
    if battery_recovery {
        drive_pin_high(Port::P0, 11);
    } else {
        drive_pin_low(Port::P0, 11);
    }

    // GNSS down: enable, reset, and standby all low.
    drive_pin_low(Port::P0, 6);
    drive_pin_low(Port::P0, 29);
    drive_pin_low(Port::P0, 30);

    // LEDs dark (both active-high) and the unused QSPI flash unpowered.
    drive_pin_low(Port::P0, 12);
    drive_pin_low(Port::P0, 7);
    drive_pin_low(Port::P0, 21);

    // Tri-state the remaining peripheral signal pins. Embassy's async GPIO
    // `wait_for_*` leaves PIN_CNF SENSE bits set on in-flight waits; any pin
    // matching its SENSE level at System OFF entry fires DETECT and the chip
    // wakes immediately.
    for (port, pin) in [
        (Port::P1, 9u8),  // I²C SDA
        (Port::P0, 8u8),  // I²C SCL
        (Port::P1, 13u8), // radio SPI SCK
        (Port::P1, 14u8), // radio SPI MOSI
        (Port::P1, 15u8), // radio SPI MISO
        (Port::P1, 12u8), // radio CS
        (Port::P1, 11u8), // radio BUSY
        (Port::P1, 6u8),  // radio DIO1 ← has SENSE set by async radio wait
        (Port::P0, 2u8),  // GNSS UART, MCU TX
        (Port::P0, 3u8),  // GNSS UART, MCU RX
        (Port::P0, 31u8), // GNSS PPS
        (Port::P0, 15u8), // charger status—its pull-up would draw while charging
        (Port::P0, 28u8), // battery ADC (AIN4)
    ] {
        tristate_pin(port, pin);
    }

    configure_wake_input(
        WakePin {
            port,
            pin,
            sense: WakeSense::Low,
        },
        WakePull::Up,
    );

    if battery_recovery {
        // Let the tap settle: the divider was disconnected until a moment
        // ago, so P0.28 has been floating.
        Timer::after(Duration::from_millis(10)).await;

        // Wake when the cell recovers past ~3.61 V. See the module docs.
        arm_lpcomp_wake_up(LpcompInput::AnalogInput4, LpcompReference::Ref58vdd);
    }

    enter_system_off()
}
