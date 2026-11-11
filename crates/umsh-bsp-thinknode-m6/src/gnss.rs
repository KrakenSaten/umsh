//! Power sequencing for the ThinkNode M6's Quectel L76K GNSS receiver.
//!
//! | Signal | Pin | Notes |
//! |---|---|---|
//! | Module power enable | P0.06 | `GPS_EN`; high powers the module |
//! | Reset | P0.29 | `GPS_REINIT`; active-low |
//! | Standby / wake | P0.30 | High wakes the receiver; low lets it sleep |
//! | Module TX → MCU RX | P0.03 | 9600 baud |
//! | Module RX ← MCU TX | P0.02 | Unused today; the receiver needs no commands |
//!
//! Like the SenseCAP Solar, this board can cut the receiver's power
//! outright, and "off" means off: the enable drops, and with it the
//! module's backup domain, its ephemeris, and its clock.
//!
//! Unlike the Solar, the reset line is driven. Meshtastic pulses it on
//! every power-up, and a reset input nobody drives is only as deasserted
//! as whatever pull the board happens to fit. It is released (driven
//! high) once the rail is up and pinned low with the rail on the way
//! down, so no pin drives into an unpowered module.

use embassy_nrf::Peri;
use embassy_nrf::gpio::{Level, Output, OutputDrive, Pin};
use embassy_time::{Duration, Timer};

/// The receiver's UART speed.
pub const BAUD: u32 = 9600;

/// How long the module's rail is given to settle before reset and
/// standby are released.
const RAIL_SETTLE: Duration = Duration::from_millis(50);

/// How long the receiver takes to start emitting sentences after power-up.
///
/// Not waited on for correctness—the parser resynchronizes at the next
/// `$` whatever arrives first—but powering on and immediately reading
/// otherwise spends a wake-up on a UART with nothing behind it yet.
const STARTUP: Duration = Duration::from_millis(150);

/// The ThinkNode M6's GNSS power control.
///
/// Implements [`umsh_gnss::pump::Power`], which is the whole of its
/// public surface: the pump owns when the receiver runs, and this owns
/// how.
pub struct Gnss<'d> {
    enable: Output<'d>,
    reset_n: Output<'d>,
    standby: Output<'d>,
}

impl<'d> Gnss<'d> {
    /// Take the enable, reset, and standby pins, leaving the receiver
    /// unpowered.
    ///
    /// All three are driven, not tri-stated: a floating enable on a load
    /// switch is not an off switch, and driven levels are also what
    /// survive nRF52840 System OFF.
    pub fn new(
        enable: Peri<'d, impl Pin>,
        reset_n: Peri<'d, impl Pin>,
        standby: Peri<'d, impl Pin>,
    ) -> Self {
        Self {
            enable: Output::new(enable, Level::Low, OutputDrive::Standard),
            reset_n: Output::new(reset_n, Level::Low, OutputDrive::Standard),
            standby: Output::new(standby, Level::Low, OutputDrive::Standard),
        }
    }
}

impl umsh_gnss::pump::Power for Gnss<'_> {
    async fn power_on(&mut self) {
        self.enable.set_high();
        Timer::after(RAIL_SETTLE).await;
        self.reset_n.set_high();
        self.standby.set_high();
        Timer::after(STARTUP).await;
    }

    async fn power_off(&mut self) {
        // Standby first, then the rail, with reset pinned low alongside
        // it so neither control line drives into an unpowered module.
        self.standby.set_low();
        self.reset_n.set_low();
        self.enable.set_low();
    }
}
