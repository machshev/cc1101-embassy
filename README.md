# cc1101-embassy

An async [Embassy](https://embassy.dev) driver for the [CC1101](https://www.ti.com/product/CC1101) sub-1 GHz RF transceiver, built on [`embedded-hal`](https://github.com/rust-embedded/embedded-hal) 1.0 and [`embedded-hal-async`](https://github.com/rust-embedded/embedded-hal) 1.0.

## Status

**Early development** — RX and TX are implemented, RSSI scanning works. Tested on RP2040 (Raspberry Pi Pico) with Ebyte E07-M1101D 433 MHz modules.

## Features

- **Async-first** — uses `embedded-hal-async`'s `Wait` trait for GDO pin interrupts; no polling loops
- **Human-readable config** — `RadioConfig` builder accepts frequency in Hz, baud rate in bps, etc.; no raw register values required
- **Hardware CRC** — enabled by default, with optional RSSI/LQI status byte appending
- **Variable or fixed packet length** — up to 61 bytes payload in variable mode
- **`no_std`, no heap** — works on stable Rust with any Embassy-compatible executor
- **Optional `defmt` logging** — behind the `defmt` feature flag, zero-cost when disabled

## Quick start

```rust
use cc1101_embassy::{Cc1101, Modulation, RadioConfig, TxPower};

let config = RadioConfig::new()
    .frequency_hz(433_920_000)   // 433.920 MHz EU ISM centre
    .baud_rate(38_400)
    .modulation(Modulation::Gfsk)
    .tx_power(TxPower::Dbm0);

let mut radio = Cc1101::new(spi, gdo0, gdo2).await?;
radio.configure(&config).await?;

// Transmit
radio.transmit(b"hello").await?;

// Receive
let mut buf = [0u8; 64];
let packet = radio.receive(&mut buf).await?;
defmt::info!("rssi: {} dBm  lqi: {}", packet.rssi_dbm, packet.lqi);
```

## Wiring

The CC1101 uses SPI mode 0 (CPOL=0, CPHA=0) at up to 10 MHz. The example below uses SPI0 on a Raspberry Pi Pico — adjust pin numbers to match your wiring.

| CC1101 pin | Pico GPIO | Notes                                        |
|------------|-----------|----------------------------------------------|
| VCC        | 3.3 V     | 1.8–3.6 V supply                            |
| GND        | GND       |                                              |
| CSn        | GP5       | Active low — idle high                       |
| SCLK       | GP2       | SPI0 clock                                   |
| MOSI (SI)  | GP3       | SPI0 TX                                      |
| MISO (SO)  | GP4       | SPI0 RX (shared with GDO1)                  |
| GDO0       | GP6       | Packet sync interrupt — required             |
| GDO2       | GP7       | RX FIFO threshold — wire up, reserved for future use |

> **Note:** The E07-M1101D modules from Ebyte have an SMA connector and include an antenna. Fit the antenna before testing — range and sensitivity are significantly worse without it.

## Dependency setup (RP2040)

```toml
[dependencies]
cc1101-embassy       = { version = "0.1", features = ["defmt"] }

embassy-rp           = { version = "0.3", features = ["defmt", "rp2040", "time-driver", "critical-section-impl"] }
embassy-executor     = { version = "0.7", features = ["arch-cortex-m", "executor-thread", "defmt"] }
embassy-time         = { version = "0.4", features = ["defmt", "defmt-timestamp-uptime"] }
embassy-sync         = { version = "0.6", features = ["defmt"] }
embassy-embedded-hal = { version = "0.3", features = ["defmt"] }
static_cell          = "2"
portable-atomic      = { version = "1", features = ["critical-section"] }
defmt                = "0.3"
defmt-rtt            = "0.4"
panic-probe          = { version = "0.3", features = ["print-defmt"] }
cortex-m             = { version = "0.7", features = ["inline-asm"] }
cortex-m-rt          = "0.7"
```

## Examples

The `examples/rp2040/` directory contains working examples for the Raspberry Pi Pico. Flash with `probe-rs`:

```bash
cd examples/rp2040
cargo run --bin rssi_scan
```

### `rssi_scan`

Milestone 1 — puts the CC1101 into RX mode and prints the received signal strength every 100 ms. No protocol matching required. Use this to verify SPI wiring before developing RX/TX.

Expected output:
```
INFO  CC1101 found OK
INFO  CC1101 configured: 433.920 MHz GFSK 38.4 kbps
INFO  RX active. Press your doorbell to see RSSI jump!
INFO  RSSI: -103 dBm  []
INFO  RSSI: -101 dBm  []
INFO  RSSI:  -68 dBm  [=====]    <- doorbell pressed
INFO  RSSI:  -71 dBm  [=====]
INFO  RSSI: -104 dBm  []
```

## Radio configuration

```rust
let config = RadioConfig::new()
    // Carrier frequency — must be in a supported CC1101 band
    .frequency_hz(433_920_000)      // 433.920 MHz (EU ISM)
    // .frequency_hz(868_000_000)   // 868 MHz (EU SRD)

    // Data rate
    .baud_rate(38_400)              // 38.4 kbps (also try 4_800, 9_600, 115_200)

    // Modulation
    .modulation(Modulation::Gfsk)   // GFSK recommended for new designs
    // .modulation(Modulation::Ook) // For receiving OOK doorbells etc.

    // Sync word — both ends must match; avoid 0x0000 and 0xFFFF
    .sync_word(0xD391)

    // Packet length
    .packet_length(PacketLength::Variable(61))  // up to 61 bytes
    // .packet_length(PacketLength::Fixed(16))  // fixed 16 bytes

    // TX power (433 MHz)
    .tx_power(TxPower::Dbm0)        // 1 mW — safe for bench testing
    // .tx_power(TxPower::Dbm10)    // 10 mW — ISM band max for unlicensed UK/EU use
    ;
```

## UK/EU regulatory notes

The 433.050–434.790 MHz band is an ISM (licence-exempt) band in the UK and EU. Key limits for unlicensed use:

- Maximum ERP: **10 mW** (`TxPower::Dbm10`) 
- Duty cycle: **≤ 10%** in most sub-bands

As a licensed radio amateur (e.g. Foundation or Full licence) you may operate on the 70 cm amateur allocation (430–440 MHz) with higher power and without duty cycle restrictions, subject to your licence conditions.

## Architecture

```
cc1101-embassy/          <- library crate (this crate)
├── src/
│   ├── lib.rs           <- public API and re-exports
│   ├── config.rs        <- RadioConfig builder + register maths
│   ├── driver.rs        <- Cc1101<SPI, GDO0, GDO2> implementation
│   ├── error.rs         <- Error enum
│   └── regs.rs          <- CC1101 register map constants
└── examples/
    └── rp2040/          <- Embassy/RP2040 example binary crate
        └── src/bin/
            └── rssi_scan.rs
```

The library is generic over `SpiDevice` and `Wait` (from `embedded-hal-async`), making it portable to any async HAL — not just Embassy or RP2040.

## Running the tests

The library's unit tests run on the host (no hardware needed) and cover the register calculation maths:

```bash
cargo test -p cc1101-embassy
```

## Contributing

Issues and PRs welcome. The most useful next contributions would be:

- Testing on hardware other than RP2040
- STM32 or nRF52 example crates
- OOK receive mode for interoperability with common 433 MHz devices
- Channel hopping support

## Licence

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT licence](LICENSE-MIT) at your option.
