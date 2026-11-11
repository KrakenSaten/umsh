//! Power control + battery monitoring for the ThinkNode M6.
//!
//! Two pieces:
//!
//! - [`SHUTDOWN_SIGNAL`], which drives the teardown in [`crate::shutdown`].
//! - The battery monitor ([`run_battery_monitor`], [`sample_battery`],
//!   [`BatterySample`], [`battery_state`])—the SenseCAP Solar's, with this
//!   board's wiring and its charger status line.
//!
//! ## Voltage reading (nominal—uncalibrated)
//!
//! Reads AIN4/P0.28 through the on-board divider, connected only while
//! P0.11 (`ADC_CTRL`) is high. Meshtastic scales the tap by 1.75. Our SAADC
//! is `embassy-nrf`'s default single-ended config: 12-bit, `Gain1_6`,
//! 0.6 V internal reference → **3.6 V full scale** at the pin, which a
//! full cell (4.2 V / 1.75 = 2.4 V at the tap) stays well inside:
//!
//! ```text
//! Vpin_mV = raw * 3600 / 4096            (12-bit, Gain1_6, 0.6 V ref)
//! VBAT_mV = Vpin_mV * 1.75
//!         = raw * 6300 / 4096
//! ```
//!
//! This is the vendor's ratio, not a fitted calibration. Good enough for
//! the protective low-battery cutoff; a bench calibration can still
//! replace `DIVIDER_MICRO` with a fitted slope.
//!
//! ## Charger status
//!
//! P0.15 (`EXT_CHRG_DETECT`) is low while the charger is charging, from
//! the panel or from USB. It is the only charge signal on the board, so a
//! panel that is lit but has nothing left to charge reads the same as no
//! panel at all.

use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::signal::Signal;
use umsh_bsp_nrf52840::system_off::ShutdownReason;

/// Single-consumer power-off trigger. Fired by the PWR-button hold, the
/// low-battery protective cutoff in [`run_battery_monitor`], and any other
/// firmware-local source. The firmware's shutdown task (which calls
/// [`crate::shutdown::enter_off`]) is the only consumer.
///
/// The [`ShutdownReason`] carried here splits that teardown in two: a
/// requested power-off disconnects the divider and waits for a button,
/// while the low-battery cutoff leaves the divider live and arms LPCOMP so
/// the panel can bring the node back unattended. See [`crate::shutdown`].
pub static SHUTDOWN_SIGNAL: Signal<ThreadModeRawMutex, ShutdownReason> = Signal::new();

#[cfg(target_os = "none")]
pub use monitor::*;

#[cfg(target_os = "none")]
mod monitor {
    use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

    use embassy_nrf::gpio::{Input, Level, Output, OutputDrive};
    use embassy_nrf::interrupt::typelevel::{Binding, SAADC as SaadcIrq};
    use embassy_nrf::saadc::{
        ChannelConfig, Config as SaadcConfig, InterruptHandler as SaadcInterruptHandler, Saadc,
    };
    use embassy_nrf::{Peri, pac, peripherals};
    use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
    use embassy_sync::signal::Signal;
    use embassy_sync::watch::Watch;
    use embassy_time::{Duration, Timer};
    use umsh_ux_tracker::battery::{
        BOOT_SAMPLES, BatteryState, BatteryThresholds, ChargeClass, LevelEstimator, LevelSample,
        charge_class, classify, load_recent,
    };
    use umsh_ux_tracker::power::LowBatteryDetector;

    /// VBAT scaling, chosen so integer math stays exact:
    /// `battery_mv = raw * DIVIDER_MICRO / 4096`.
    ///
    ///   Vadc_mV = raw * 3600 / 4096              (12-bit, Gain1_6, 0.6 V ref)
    ///   VBAT_mV = Vadc_mV * 1.75
    ///          = raw * 6300 / 4096
    const DIVIDER_MICRO: u32 = 6_300;

    /// Current mutually exclusive user-facing battery mode.
    static BATTERY_STATE: AtomicU8 = AtomicU8::new(BatteryState::BatteryOnly as u8);

    /// Wakes any LED policy whenever the battery mode changes.
    pub static BATTERY_STATE_CHANGED: Signal<ThreadModeRawMutex, BatteryState> = Signal::new();

    pub fn battery_state() -> BatteryState {
        BatteryState::from_u8(BATTERY_STATE.load(Ordering::Acquire))
    }

    /// Whether the nRF USB regulator currently detects VBUS.
    pub fn usb_power_present() -> bool {
        pac::POWER.usbregstatus().read().vbusdetect()
    }

    fn publish_battery_state(state: BatteryState) {
        let previous = BATTERY_STATE.swap(state as u8, Ordering::AcqRel);
        if previous != state as u8 {
            BATTERY_STATE_CHANGED.signal(state);
        }
    }

    /// One serviced battery measurement.
    #[derive(Clone, Copy, Debug)]
    pub struct BatterySample {
        pub battery_mv: u16,
        pub state: BatteryState,
        /// `Some` from the monitor's first sample onward.
        pub level_percent: Option<u8>,
    }

    /// Battery measurements worth announcing to a remote observer, for
    /// `PROP_BATTERY` asynchronous updates. Multi-receiver, and filtered
    /// on charge class plus level rather than the five-way presentation
    /// classification—see the T1000-E BSP's equivalent for the
    /// reasoning, which is identical.
    pub static BATTERY_ANNOUNCE: Watch<
        ThreadModeRawMutex,
        BatterySample,
        BATTERY_ANNOUNCE_RECEIVERS,
    > = Watch::new();

    /// Receivers of [`BATTERY_ANNOUNCE`]: the ULCP session driver.
    pub const BATTERY_ANNOUNCE_RECEIVERS: usize = 1;

    /// Millisecond timestamp of the most recent externally reported load;
    /// `u32::MAX` sentinel = never.
    static LAST_LOAD_MS: AtomicU32 = AtomicU32::new(u32::MAX);

    /// Tell the battery monitor a significant transient load just ran (a
    /// radio transmission), so nearby voltage samples are treated as
    /// sagged rather than resting OCV by the level estimator.
    pub fn note_external_load() {
        LAST_LOAD_MS.store(
            embassy_time::Instant::now().as_millis() as u32,
            Ordering::Release,
        );
    }

    static BATTERY_SAMPLE_REQUEST: Signal<ThreadModeRawMutex, ()> = Signal::new();
    static BATTERY_SAMPLE_REPLY: Signal<ThreadModeRawMutex, BatterySample> = Signal::new();

    /// Ask [`run_battery_monitor`]—the sole SAADC and divider-enable
    /// owner—for a fresh measurement and wait for it. Single-consumer.
    /// Never completes once the monitor has exited; callers should apply a
    /// timeout.
    pub async fn sample_battery() -> BatterySample {
        BATTERY_SAMPLE_REPLY.reset();
        BATTERY_SAMPLE_REQUEST.signal(());
        BATTERY_SAMPLE_REPLY.wait().await
    }

    /// One reading: connect the divider (enable HIGH), settle ≥10 ms,
    /// sample, then disconnect it (enable LOW) to drop its quiescent draw.
    async fn read_mv(saadc: &mut Saadc<'_, 1>, divider_enable: &mut Output<'_>) -> u16 {
        divider_enable.set_high();
        Timer::after(Duration::from_millis(10)).await;
        let mut buf = [0i16; 1];
        saadc.sample(&mut buf).await;
        divider_enable.set_low();
        let raw = u32::from(buf[0].max(0) as u16);
        ((raw * DIVIDER_MICRO) / 4_096).min(u32::from(u16::MAX)) as u16
    }

    /// Read the cell for the firmware's boot gate
    /// ([`boot_permitted`](umsh_ux_tracker::battery::boot_permitted)),
    /// before anything else draws current. Same path and scaling as
    /// [`run_battery_monitor`], which later takes over the peripherals this
    /// borrows; the divider is left disconnected.
    pub async fn boot_battery_samples<I>(
        saadc: Peri<'_, peripherals::SAADC>,
        irq: I,
        tap: Peri<'_, peripherals::P0_28>,
        divider_enable: Peri<'_, peripherals::P0_11>,
    ) -> [u16; BOOT_SAMPLES]
    where
        I: Binding<SaadcIrq, SaadcInterruptHandler>,
    {
        let mut divider_enable = Output::new(divider_enable, Level::Low, OutputDrive::Standard);
        let mut saadc = Saadc::new(
            saadc,
            irq,
            SaadcConfig::default(),
            [ChannelConfig::single_ended(tap)],
        );
        let mut samples = [0u16; BOOT_SAMPLES];
        for sample in &mut samples {
            *sample = read_mv(&mut saadc, &mut divider_enable).await;
        }
        samples
    }

    /// Monitors battery voltage via the SAADC on AIN4 / P0.28, connecting
    /// the divider with `divider_enable` (P0.11, active-high) for each
    /// reading, and charger activity on `charging_n` (P0.15, low while
    /// charging).
    ///
    /// External power is VBUS or an active charge: on this board the panel
    /// feeds the same charger as USB, and an active charge is the only
    /// sign the panel gives. Five minutes at or below the critical
    /// threshold (≈3.1 V) fire [`SHUTDOWN_SIGNAL`] for a protective System
    /// OFF, whether or not either is present, since neither says whether
    /// its source carries the load. That signal carries
    /// `ShutdownReason::BatteryCritical`, which is what makes the teardown
    /// leave the divider connected and arm LPCOMP.
    ///
    /// Wrap in `#[embassy_executor::task]` in the firmware binary so the
    /// linker sees a concrete monomorphisation.
    pub async fn run_battery_monitor(
        mut saadc: Saadc<'static, 1>,
        mut divider_enable: Output<'static>,
        charging_n: Input<'static>,
    ) {
        /// Normal cadence. The pack discharges over days and the level
        /// estimator quantizes to 5 %. External-power changes do not wait
        /// for it—[`POWER_POLL_INTERVAL`] catches those.
        const SAMPLE_INTERVAL: Duration = Duration::from_secs(300);
        /// Cadence while the cell reads at or below Low, whatever the charge
        /// state: the protective cutoff needs critical readings no further
        /// apart than
        /// [`CUTOFF_MAX_GAP_MS`](umsh_ux_tracker::power::CUTOFF_MAX_GAP_MS).
        const LOW_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
        /// How often VBUS and the charger line are checked between voltage
        /// samples. Polled rather than interrupt-driven: MPSL owns the
        /// shared CLOCK_POWER vector, and a register read plus a pin read
        /// cost nothing next to a divider reading.
        const POWER_POLL_INTERVAL: Duration = Duration::from_secs(5);

        divider_enable.set_low();

        let thresholds = BatteryThresholds::default();
        let mut cutoff = LowBatteryDetector::new(thresholds);
        let mut reply_pending = false;
        let mut estimator = LevelEstimator::new();
        let announce = BATTERY_ANNOUNCE.sender();
        // Last announced (charge class, level); `None` until the first
        // sample, which always announces.
        let mut announced: Option<(ChargeClass, Option<u8>)> = None;

        loop {
            let battery_mv = read_mv(&mut saadc, &mut divider_enable).await;

            let usb = usb_power_present();
            let charging = charging_n.is_low();
            let state = classify(battery_mv, usb || charging, charging, thresholds);
            publish_battery_state(state);

            // Feed the level estimator; a load within the sag window marks
            // this voltage as potentially sagged rather than resting.
            let now_ms = embassy_time::Instant::now().as_millis() as u32;
            let last_load_ms = match LAST_LOAD_MS.load(Ordering::Acquire) {
                u32::MAX => None,
                timestamp => Some(timestamp),
            };
            estimator.sample(LevelSample {
                battery_mv,
                state,
                load_recent: load_recent(now_ms, last_load_ms),
                now_ms,
            });

            let sample = BatterySample {
                battery_mv,
                state,
                level_percent: estimator.level(),
            };

            if reply_pending || BATTERY_SAMPLE_REQUEST.try_take().is_some() {
                reply_pending = false;
                BATTERY_SAMPLE_REPLY.signal(sample);
            }

            // Announce when the fields a remote observer acts on moved.
            let key = (charge_class(state), sample.level_percent);
            if announced != Some(key) {
                announced = Some(key);
                announce.send(sample);
            }

            // Protective cell cutoff: sustained critical voltage drives a
            // System OFF so the pack is not deep-discharged when nobody is
            // present.
            if cutoff
                .observe(embassy_time::Instant::now().as_millis(), battery_mv)
                .is_some()
            {
                super::SHUTDOWN_SIGNAL.signal(super::ShutdownReason::BatteryCritical);
                return;
            }

            let interval = if battery_mv <= thresholds.low_mv {
                LOW_SAMPLE_INTERVAL
            } else {
                SAMPLE_INTERVAL
            };
            // Watch VBUS and the charger between voltage samples so a
            // change is reflected within seconds instead of within
            // `interval`.
            let power_changed = async {
                loop {
                    Timer::after(POWER_POLL_INTERVAL).await;
                    if usb_power_present() != usb || charging_n.is_low() != charging {
                        return;
                    }
                }
            };
            match embassy_futures::select::select3(
                Timer::after(interval),
                BATTERY_SAMPLE_REQUEST.wait(),
                power_changed,
            )
            .await
            {
                embassy_futures::select::Either3::First(()) => {}
                embassy_futures::select::Either3::Second(()) => reply_pending = true,
                embassy_futures::select::Either3::Third(()) => {}
            }
        }
    }
}
