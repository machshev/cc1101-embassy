//! Dedicated receiver for CC1101 loopback test.
//!
//! Flash this to the RX board. It listens continuously and prints every
//! packet received. Run `tx` on the other board to send packets.
//!
//! ```bash
//! cargo run --bin rx
//! ```

#![no_std]
#![no_main]

use cc1101_embassy::{Modulation, PacketLength, RadioConfig, TxPower};

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

type SpiBus = Spi<'static, embassy_rp::peripherals::SPI0, embassy_rp::spi::Async>;
type SpiDev = SpiDevice<'static, NoopRawMutex, SpiBus, Output<'static>>;
type Radio  = cc1101_embassy::Cc1101<SpiDev, Input<'static>, Input<'static>>;

static SPI_BUS: StaticCell<Mutex<NoopRawMutex, SpiBus>> = StaticCell::new();

fn radio_config() -> RadioConfig {
    RadioConfig::new()
        .frequency_hz(433_920_000)
        .baud_rate(38_400)
        .modulation(Modulation::Gfsk)
        .sync_word(0xD391)
        .packet_length(PacketLength::Fixed(5))
        .crc_enable(true)
        .append_status(true)
        .tx_power(TxPower::Dbm0)
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    info!("CC1101 RX starting");

    let mut spi_config = SpiConfig::default();
    spi_config.frequency = 4_000_000;
    spi_config.phase     = embassy_rp::spi::Phase::CaptureOnFirstTransition;
    spi_config.polarity  = embassy_rp::spi::Polarity::IdleLow;

    let spi_bus = Spi::new(p.SPI0, p.PIN_2, p.PIN_3, p.PIN_4, p.DMA_CH0, p.DMA_CH1, spi_config);
    let spi_bus = SPI_BUS.init(Mutex::new(spi_bus));
    let cs      = Output::new(p.PIN_5, Level::High);
    let spi_dev = SpiDevice::new(spi_bus, cs);
    let gdo0    = Input::new(p.PIN_6, Pull::None);
    let gdo2    = Input::new(p.PIN_7, Pull::None);

    let mut radio: Radio = match cc1101_embassy::Cc1101::new(spi_dev, gdo0, gdo2).await {
        Ok(r)  => { info!("CC1101 OK"); r }
        Err(e) => {
            error!("init failed: {:?}", e);
            loop { Timer::after(Duration::from_secs(1)).await; }
        }
    };

    radio.configure(&radio_config()).await.unwrap();
    info!("Configured. Listening for packets...");

    let mut count: u32 = 0;
    loop {
        let mut buf = [0u8; 5];
        match radio.receive(&mut buf).await {
            Ok(packet) => {
                count += 1;
                if &buf[..4] == b"PING" {
                    info!(
                        "PING #{} received (total {}) | RSSI: {} dBm | LQI: {}",
                        buf[4], count, packet.rssi_dbm, packet.lqi,
                    );
                } else {
                    // Print raw bytes if not a recognised PING
                    info!("packet #{} | {:?} | RSSI: {} dBm | LQI: {}",
                        count, buf, packet.rssi_dbm, packet.lqi,
                    );
                }
            }
            Err(e) => warn!("RX error: {:?}", e),
        }
    }
}
