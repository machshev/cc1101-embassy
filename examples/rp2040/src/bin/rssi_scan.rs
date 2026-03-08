//! RSSI scan example for the CC1101 on Raspberry Pi Pico (RP2040).
//!
//! This is milestone 1: put the CC1101 into RX mode and continuously print
//! the received signal strength. No packet reception, no matching protocol.
//! If you see the RSSI value change when you press your 433 MHz doorbell, the
//! hardware is wired correctly and the SPI communication is working.
//!
//! # Wiring
//!
//! | CC1101 pin | Pico GPIO | Notes                              |
//! |------------|-----------|------------------------------------|
//! | VCC        | 3.3V      |                                    |
//! | GND        | GND       |                                    |
//! | CSn        | GP5       | Active low — held high by driver   |
//! | SCLK       | GP2       | SPI0 SCK                           |
//! | MOSI (SI)  | GP3       | SPI0 TX                            |
//! | MISO (SO)  | GP4       | SPI0 RX                            |
//! | GDO0       | GP6       | Packet sync interrupt              |
//! | GDO2       | GP7       | RX FIFO threshold (reserved)       |
//!
//! # Running
//!
//! ```bash
//! cargo run --bin rssi_scan
//! ```

#![no_std]
#![no_main]

use cc1101_embassy::{Modulation, RadioConfig};

use defmt::*;
use embassy_executor::Spawner;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::spi::{Config as SpiConfig, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// ---- SPI bus clock ----------------------------------------------------------
// CC1101 supports up to 10 MHz. 4 MHz is conservative and reliable.
const SPI_FREQ_HZ: u32 = 4_000_000;

// ---- Radio config -----------------------------------------------------------
fn radio_config() -> RadioConfig {
    RadioConfig::new()
        .frequency_hz(433_920_000)
        .baud_rate(38_400)
        .modulation(Modulation::Gfsk)
}

// ---- Static SPI bus ---------------------------------------------------------
// embassy_embedded_hal::SpiDevice requires the bus in a Mutex so it can be
// shared. Even with a single device this is the idiomatic Embassy pattern.
type SpiBus = Spi<'static, embassy_rp::peripherals::SPI0, embassy_rp::spi::Async>;
static SPI_BUS: StaticCell<Mutex<NoopRawMutex, SpiBus>> = StaticCell::new();

// ---- Main -------------------------------------------------------------------

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    info!("CC1101 RSSI scan starting...");

    // Configure SPI0 — CC1101 uses mode 0: CPOL=0, CPHA=0
    let mut spi_config = SpiConfig::default();
    spi_config.frequency = SPI_FREQ_HZ;
    spi_config.phase = embassy_rp::spi::Phase::CaptureOnFirstTransition;
    spi_config.polarity = embassy_rp::spi::Polarity::IdleLow;

    let spi_bus = Spi::new(
        p.SPI0,
        p.PIN_2,   // SCLK
        p.PIN_3,   // MOSI (CC1101 SI)
        p.PIN_4,   // MISO (CC1101 SO / GDO1)
        p.DMA_CH0,
        p.DMA_CH1,
        spi_config,
    );

    // Put the bus in a static Mutex so SpiDevice can borrow it
    let spi_bus = SPI_BUS.init(Mutex::new(spi_bus));

    // CS pin: active low, idle high
    let cs = Output::new(p.PIN_5, Level::High);

    // Wrap bus + CS into an async SpiDevice
    let spi_dev = SpiDevice::new(spi_bus, cs);

    // GDO0 and GDO2: floating inputs driven by the CC1101
    let gdo0 = Input::new(p.PIN_6, Pull::None);
    let gdo2 = Input::new(p.PIN_7, Pull::None);

    // Explicit type annotation so the compiler can resolve the generics
    type Radio = cc1101_embassy::Cc1101<
        SpiDevice<'static, NoopRawMutex, SpiBus, Output<'static>>,
        Input<'static>,
        Input<'static>,
    >;

    // Initialise the CC1101 — resets chip and verifies part number
    let mut radio: Radio = match cc1101_embassy::Cc1101::new(spi_dev, gdo0, gdo2).await {
        Ok(r) => {
            info!("CC1101 found OK");
            r
        }
        Err(e) => {
            // InvalidChip here usually means a wiring problem:
            //   - MISO floating (check SO pin connection)
            //   - CS wrong polarity or wrong pin
            //   - No 3.3V power or missing GND
            error!("CC1101 init failed: {:?}", e);
            loop {
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    };

    // Apply radio configuration
    let config = radio_config();
    if let Err(e) = radio.configure(&config).await {
        error!("CC1101 configure failed: {:?}", e);
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
    info!("CC1101 configured: 433.920 MHz GFSK 38.4 kbps");

    // Enter RX mode — RSSI register is only valid while in RX
    radio.start_rx().await.unwrap();
    info!("RX active. Press your doorbell to see RSSI jump!");
    info!("(noise floor ~-105 dBm; a nearby transmitter should read -70 dBm or better)");

    // ---- Main loop: read and display RSSI every 100 ms ---------------------
    loop {
        match radio.read_rssi().await {
            Ok(rssi) => {
                // Map -110..-70 dBm onto a 0..8 segment bar
                let bar_len = ((rssi + 110).max(0) as usize).min(40);
                info!("RSSI: {} dBm  [{}]", rssi, RssiBar(bar_len));
            }
            Err(e) => {
                warn!("RSSI read error: {:?}", e);
                // Re-enter RX in case of state machine glitch
                let _ = radio.start_rx().await;
            }
        }

        Timer::after(Duration::from_millis(100)).await;
    }
}

// ---- RSSI bar display -------------------------------------------------------
// defmt doesn't support runtime string building, so we use fixed buckets.
// Each '=' represents roughly 5 dBm above the noise floor.

struct RssiBar(usize);

impl defmt::Format for RssiBar {
    fn format(&self, f: defmt::Formatter) {
        match self.0 {
            0..=4   => defmt::write!(f, ""),
            5..=9   => defmt::write!(f, "="),
            10..=14 => defmt::write!(f, "=="),
            15..=19 => defmt::write!(f, "==="),
            20..=24 => defmt::write!(f, "===="),
            25..=29 => defmt::write!(f, "====="),
            30..=34 => defmt::write!(f, "======"),
            35..=39 => defmt::write!(f, "======="),
            _       => defmt::write!(f, "========"),
        }
    }
}
