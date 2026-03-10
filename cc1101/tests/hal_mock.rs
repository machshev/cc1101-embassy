//! Integration tests using `embedded-hal-mock` to verify the SPI byte sequences
//! the CC1101 driver produces without needing any hardware.
//!
//! Each test pre-programs a sequence of expected SPI transactions, runs a driver
//! method, then asserts every expectation was consumed — catching both missing
//! transactions and extra unexpected ones.
//!
//! # Transaction encoding
//!
//! The driver uses `SpiDevice::transaction()`, which the mock models as:
//!   `Transaction::transaction_start()`
//!   …one or more `Transaction::write_vec()` / `Transaction::read_vec()`…
//!   `Transaction::transaction_end()`
//!
//! The GDO pin mock uses `PinTransaction::wait_for_edge(Edge::Rising)` / `wait_for_falling_edge()`.

use cc1101_embassy::{
    config::{PacketLength, RadioConfig},
    Cc1101,
};
use embedded_hal_mock::eh1::{
    digital::{Edge, Mock as PinMock, Transaction as PinTransaction},
    spi::{Mock as SpiMock, Transaction as SpiTransaction},
};

// ---- helpers ----------------------------------------------------------------

/// Returns the mock transactions for `Cc1101::new()`:
///   1. SRES strobe
///   2. Read PARTNUM → 0x00
///   3. Read VERSION  → 0x04
fn new_transactions() -> Vec<SpiTransaction<u8>> {
    vec![
        // reset(): strobe SRES = 0x30
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0x30]),
        SpiTransaction::transaction_end(),
        // read_status(STATUS_PARTNUM = 0x30): header = 0x30 | READ(0x80) | BURST(0x40) = 0xF0
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF0]),
        SpiTransaction::read_vec(vec![0x00]),
        SpiTransaction::transaction_end(),
        // read_status(STATUS_VERSION = 0x31): header = 0x31 | 0x80 | 0x40 = 0xF1
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF1]),
        SpiTransaction::read_vec(vec![0x04]),
        SpiTransaction::transaction_end(),
    ]
}

/// A do-nothing GDO pin mock with no expected transactions.
fn idle_pin() -> PinMock {
    PinMock::new(&[])
}

// ---- Cc1101::new() ----------------------------------------------------------

#[tokio::test]
async fn new_resets_and_verifies_chip_id() {
    let mut spi = SpiMock::new(&new_transactions());
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .expect("Cc1101::new should succeed with valid part/version");

    drop(radio);
    spi.done();
    gdo0.done();
    gdo2.done();
}

#[tokio::test]
async fn new_rejects_wrong_part_number() {
    let txns = vec![
        // SRES
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0x30]),
        SpiTransaction::transaction_end(),
        // PARTNUM → wrong value 0x01
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF0]),
        SpiTransaction::read_vec(vec![0x01]),
        SpiTransaction::transaction_end(),
        // VERSION (still read before the check)
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF1]),
        SpiTransaction::read_vec(vec![0x04]),
        SpiTransaction::transaction_end(),
    ];

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let result = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone()).await;
    assert!(
        result.is_err(),
        "Cc1101::new should reject wrong PARTNUM"
    );

    spi.done();
    gdo0.done();
    gdo2.done();
}

#[tokio::test]
async fn new_accepts_version_0x14() {
    // Some CC1101 silicon returns VERSION = 0x14 — both should be accepted
    let txns = vec![
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0x30]),
        SpiTransaction::transaction_end(),
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF0]),
        SpiTransaction::read_vec(vec![0x00]), // PARTNUM ok
        SpiTransaction::transaction_end(),
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![0xF1]),
        SpiTransaction::read_vec(vec![0x14]), // VERSION = 0x14
        SpiTransaction::transaction_end(),
    ];

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .expect("VERSION 0x14 should be accepted");

    spi.done();
    gdo0.done();
    gdo2.done();
}

// ---- Cc1101::configure() ----------------------------------------------------

/// Build expected SPI transactions for `configure()` given a `RadioConfig`.
///
/// The order mirrors `driver.rs::configure()` exactly. Any deviation in the
/// driver (reordered writes, missing writes) will cause the mock to assert.
///
/// Register values are derived from `RadioConfig`'s public fields using the
/// same formulas as the driver, so this helper stays in sync with any config
/// changes without needing access to `pub(crate)` methods.
fn configure_transactions(config: &RadioConfig) -> Vec<SpiTransaction<u8>> {
    // ---- Frequency (formula: FREQ = freq_hz * 2^16 / 26_000_000) ----
    let freq_word = (config.frequency_hz as u64 * (1 << 16)) / 26_000_000u64;
    let freq2 = ((freq_word >> 16) & 0xFF) as u8;
    let freq1 = ((freq_word >> 8) & 0xFF) as u8;
    let freq0 = (freq_word & 0xFF) as u8;

    // ---- Baud rate: R = f_xosc * (256+M) * 2^E / 2^28
    // Solving for M: M = R * 2^28 / (f_xosc * 2^E) - 256 ----
    let (mdmcfg4, mdmcfg3) = {
        let target = config.baud_rate as u64;
        let mut best_e = 0u8;
        let mut best_m = 0u8;
        let mut best_err = u64::MAX;
        for e in 0u8..16 {
            let m_raw = (target * (1u64 << 28)) / (26_000_000u64 * (1u64 << e));
            if m_raw < 256 || m_raw > 511 { continue; }
            let m = (m_raw - 256) as u8;
            let actual = 26_000_000u64 * (256 + m as u64) * (1u64 << e) / (1u64 << 28);
            let err = target.abs_diff(actual);
            if err < best_err { best_err = err; best_e = e; best_m = m; }
        }
        // bw_bits: matches driver's bw_bits() for channel_bandwidth_khz
        let bw_bits: u8 = match config.channel_bandwidth_khz {
            0..=58    => 0b1111,
            59..=67   => 0b1110,
            68..=81   => 0b1101,
            82..=101  => 0b1100,
            102..=116 => 0b1011,
            117..=135 => 0b1010,
            136..=162 => 0b1001,
            163..=203 => 0b1000,
            204..=232 => 0b0111,
            233..=270 => 0b0110,
            271..=325 => 0b0101,
            326..=406 => 0b0100,
            407..=464 => 0b0011,
            465..=541 => 0b0010,
            542..=650 => 0b0001,
            _         => 0b0000,
        };
        ((bw_bits << 4) | (best_e & 0x0F), best_m)
    };

    // ---- Modulation / sync mode (MDMCFG2) ----
    let mod_bits: u8 = match config.modulation {
        cc1101_embassy::config::Modulation::Fsk2 => 0b000,
        cc1101_embassy::config::Modulation::Gfsk => 0b001,
        cc1101_embassy::config::Modulation::Ook  => 0b011,
        cc1101_embassy::config::Modulation::Fsk4 => 0b100,
        cc1101_embassy::config::Modulation::Msk  => 0b111,
    };
    let sync_bits: u8 = match config.sync_mode {
        cc1101_embassy::config::SyncMode::None              => 0b000,
        cc1101_embassy::config::SyncMode::Match15of16       => 0b001,
        cc1101_embassy::config::SyncMode::Match16of16       => 0b010,
        cc1101_embassy::config::SyncMode::Match30of32       => 0b011,
        cc1101_embassy::config::SyncMode::CarrierSense      => 0b100,
        cc1101_embassy::config::SyncMode::Match15of16AndCs  => 0b101,
        cc1101_embassy::config::SyncMode::Match16of16AndCs  => 0b110,
        cc1101_embassy::config::SyncMode::Match30of32AndCs  => 0b111,
    };
    let mdmcfg2 = 0x80 | (mod_bits << 4) | sync_bits;

    // ---- Deviation (f_dev = f_xosc * (8+M) * 2^E / 2^17) ----
    let deviatn = {
        let target = config.deviation_hz as u64;
        let mut best_e = 0u8; let mut best_m = 0u8; let mut best_err = u64::MAX;
        for e in 0u8..8 { for m in 0u8..8 {
            let actual = 26_000_000u64 * (8 + m as u64) * (1u64 << e) / (1u64 << 17);
            let err = target.abs_diff(actual);
            if err < best_err { best_err = err; best_e = e; best_m = m; }
        }}
        (best_e << 4) | best_m
    };

    // ---- Packet format ----
    let (pktlen, len_config) = match config.packet_length {
        PacketLength::Fixed(n) => (n, 0b00u8),
        PacketLength::Variable(n) => (n, 0b01u8),
    };
    let crc_bit = if config.crc_enable { 0b0100 } else { 0b0000 };
    let pktctrl0 = crc_bit | len_config;
    let pktctrl1 = if config.append_status { 0x04 } else { 0x00 };

    // ---- TX power (433 MHz PA table values from datasheet Table 39) ----
    let patable = match config.tx_power {
        cc1101_embassy::config::TxPower::DbmMinus30 => 0x12u8,
        cc1101_embassy::config::TxPower::DbmMinus20 => 0x0E,
        cc1101_embassy::config::TxPower::DbmMinus15 => 0x1D,
        cc1101_embassy::config::TxPower::DbmMinus10 => 0x34,
        cc1101_embassy::config::TxPower::Dbm0       => 0x60,
        cc1101_embassy::config::TxPower::Dbm5       => 0x84,
        cc1101_embassy::config::TxPower::Dbm7       => 0xC8,
        cc1101_embassy::config::TxPower::Dbm10      => 0xC0,
    };

    // Build transaction list using plain push calls — two closures over the
    // same Vec would each hold a mutable borrow, which the borrow checker rejects.
    let mut t = Vec::new();

    // 1. SIDLE strobe
    t.extend(spi_strobe(0x36));
    // 2. FREQ2, FREQ1, FREQ0
    t.extend(spi_wreg(0x0D, freq2));
    t.extend(spi_wreg(0x0E, freq1));
    t.extend(spi_wreg(0x0F, freq0));
    // 3. MDMCFG4, MDMCFG3
    t.extend(spi_wreg(0x10, mdmcfg4));
    t.extend(spi_wreg(0x11, mdmcfg3));
    // 4. MDMCFG2
    t.extend(spi_wreg(0x12, mdmcfg2));
    // 5. SYNC1, SYNC0
    t.extend(spi_wreg(0x04, (config.sync_word >> 8) as u8));
    t.extend(spi_wreg(0x05, (config.sync_word & 0xFF) as u8));
    // 6. DEVIATN
    t.extend(spi_wreg(0x15, deviatn));
    // 7. PKTLEN, PKTCTRL0, PKTCTRL1
    t.extend(spi_wreg(0x06, pktlen));
    t.extend(spi_wreg(0x08, pktctrl0));
    t.extend(spi_wreg(0x07, pktctrl1));
    // 8. CHANNR
    t.extend(spi_wreg(0x0A, config.channel));
    // 9. FREND0, then PATABLE as burst write
    t.extend(spi_wreg(0x22, 0x10));
    t.push(SpiTransaction::transaction_start());
    t.push(SpiTransaction::write_vec(vec![0x3E | 0x40, patable])); // PATABLE | BURST
    t.push(SpiTransaction::transaction_end());
    // 10. IOCFG0, IOCFG2
    t.extend(spi_wreg(0x02, 0x06)); // GDO_SYNC_WORD
    t.extend(spi_wreg(0x00, 0x00)); // GDO_RX_FIFO_THRESHOLD
    // 11. AGC + test registers
    t.extend(spi_wreg(0x1B, 0x43)); // AGCCTRL2
    t.extend(spi_wreg(0x2C, 0x81)); // TEST2
    t.extend(spi_wreg(0x2D, 0x35)); // TEST1
    t.extend(spi_wreg(0x2E, 0x09)); // TEST0
    t.extend(spi_wreg(0x23, 0xE9)); // FSCAL3
    t.extend(spi_wreg(0x24, 0x2A)); // FSCAL2
    t.extend(spi_wreg(0x25, 0x00)); // FSCAL1
    t.extend(spi_wreg(0x26, 0x1F)); // FSCAL0
    // 12. MCSM0, MCSM1
    t.extend(spi_wreg(0x18, 0x18)); // MCSM0
    t.extend(spi_wreg(0x17, 0x00)); // MCSM1

    t
}

/// Single-register write: [addr & 0x3F, value] in one transaction.
fn spi_wreg(addr: u8, val: u8) -> [SpiTransaction<u8>; 3] {
    [
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![addr & 0x3F, val]),
        SpiTransaction::transaction_end(),
    ]
}

/// Strobe command: [cmd] in one transaction.
fn spi_strobe(cmd: u8) -> [SpiTransaction<u8>; 3] {
    [
        SpiTransaction::transaction_start(),
        SpiTransaction::write_vec(vec![cmd]),
        SpiTransaction::transaction_end(),
    ]
}

#[tokio::test]
async fn configure_default_writes_correct_register_sequence() {
    let config = RadioConfig::default();
    let mut all_txns = new_transactions();
    all_txns.extend(configure_transactions(&config));

    let mut spi = SpiMock::new(&all_txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();

    radio.configure(&config).await.expect("configure should succeed");

    spi.done();
    gdo0.done();
    gdo2.done();
}

#[tokio::test]
async fn configure_868mhz_writes_correct_freq_registers() {
    let config = RadioConfig::new().frequency_hz(868_000_000);

    // Verify the freq register values are correct for 868 MHz before testing the full sequence.
    // Formula: FREQ = 868_000_000 * 65536 / 26_000_000 = 0x216276
    let freq_word = (868_000_000u64 * (1 << 16)) / 26_000_000u64;
    assert_eq!((freq_word >> 16) as u8, 0x21);
    assert_eq!(((freq_word >> 8) & 0xFF) as u8, 0x62);
    assert_eq!((freq_word & 0xFF) as u8, 0x76);

    let mut all_txns = new_transactions();
    all_txns.extend(configure_transactions(&config));

    let mut spi = SpiMock::new(&all_txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();
    radio.configure(&config).await.unwrap();

    spi.done();
    gdo0.done();
    gdo2.done();
}

// ---- Cc1101::transmit() -----------------------------------------------------

#[tokio::test]
async fn transmit_writes_fifo_and_waits_for_gdo0() {
    let config = RadioConfig::default();
    let payload: &[u8] = b"PING\x01";

    // SPI: new + configure
    let mut txns = new_transactions();
    txns.extend(configure_transactions(&config));

    // transmit():
    //   1. SIDLE strobe (0x36)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x36]));
    txns.push(SpiTransaction::transaction_end());
    //   2. SFTX strobe (0x3B)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x3B]));
    txns.push(SpiTransaction::transaction_end());
    //   3. Write length byte to TXFIFO (single write to reg 0x3F)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x3F, payload.len() as u8]));
    txns.push(SpiTransaction::transaction_end());
    //   4. Burst write payload to TXFIFO (0x3F | BURST = 0x7F)
    let mut burst = vec![0x7F];
    burst.extend_from_slice(payload);
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(burst));
    txns.push(SpiTransaction::transaction_end());
    //   5. STX strobe (0x35)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x35]));
    txns.push(SpiTransaction::transaction_end());
    // GDO0: high then low (packet sent)

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = PinMock::new(&[
        PinTransaction::wait_for_edge(Edge::Rising),
        PinTransaction::wait_for_edge(Edge::Falling),
    ]);
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();
    radio.configure(&config).await.unwrap();
    radio.transmit(payload).await.expect("transmit should succeed");

    spi.done();
    gdo0.done();
    gdo2.done();
}

#[tokio::test]
async fn transmit_rejects_payload_too_long() {
    let config = RadioConfig::default(); // variable mode, max 61 bytes
    let payload = [0u8; 62]; // one byte too many

    let mut txns = new_transactions();
    txns.extend(configure_transactions(&config));

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();
    radio.configure(&config).await.unwrap();

    let result = radio.transmit(&payload).await;
    assert!(result.is_err(), "62-byte payload should be rejected");

    // No further SPI transactions should have occurred
    spi.done();
    gdo0.done();
    gdo2.done();
}

// ---- Cc1101::receive() ------------------------------------------------------

#[tokio::test]
async fn receive_reads_packet_with_status_bytes() {
    let config = RadioConfig::default(); // append_status=true, crc_enable=true
    let payload = b"PING\x01";
    let rssi_raw: u8 = 0xA0; // -56 dBm raw → ((0xA0 as i16 - 256) / 2) - 74 = -54 dBm
    let status_byte: u8 = 0x80 | 42; // CRC ok, LQI=42

    let mut txns = new_transactions();
    txns.extend(configure_transactions(&config));

    // receive():
    //   1. SRX strobe (0x34)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x34]));
    txns.push(SpiTransaction::transaction_end());
    // GDO0 high + low (handled by pin mock below)
    //   2. Read STATUS_RXBYTES (0x3B | 0x80 | 0x40 = 0xFB)
    //      = payload(5) + length(1) + rssi(1) + status(1) = 8 bytes
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xFB]));
    txns.push(SpiTransaction::read_vec(vec![8]));
    txns.push(SpiTransaction::transaction_end());
    //   3. Read length byte from RXFIFO (single read, addr 0x3F | READ = 0xBF)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![payload.len() as u8]));
    txns.push(SpiTransaction::transaction_end());
    //   4. Burst read payload from RXFIFO (0x3F | READ | BURST = 0xFF)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xFF]));
    txns.push(SpiTransaction::read_vec(payload.to_vec()));
    txns.push(SpiTransaction::transaction_end());
    //   5. Read RSSI byte (single read RXFIFO)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![rssi_raw]));
    txns.push(SpiTransaction::transaction_end());
    //   6. Read status byte (LQI/CRC)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![status_byte]));
    txns.push(SpiTransaction::transaction_end());

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = PinMock::new(&[
        PinTransaction::wait_for_edge(Edge::Rising),
        PinTransaction::wait_for_edge(Edge::Falling),
    ]);
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();
    radio.configure(&config).await.unwrap();

    let mut buf = [0u8; 64];
    let pkt = radio.receive(&mut buf).await.expect("receive should succeed");

    assert_eq!(pkt.len, payload.len());
    assert_eq!(&buf[..pkt.len], payload);
    assert_eq!(pkt.lqi, 42);
    assert!(pkt.crc_ok);

    spi.done();
    gdo0.done();
    gdo2.done();
}

#[tokio::test]
async fn receive_returns_crc_error_when_crc_bit_clear() {
    let config = RadioConfig::default(); // crc_enable=true
    let payload = b"BAD!";

    let mut txns = new_transactions();
    txns.extend(configure_transactions(&config));

    // SRX
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x34]));
    txns.push(SpiTransaction::transaction_end());
    // RXBYTES: 4 payload + 1 length + 2 status = 7
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xFB]));
    txns.push(SpiTransaction::read_vec(vec![7]));
    txns.push(SpiTransaction::transaction_end());
    // length byte = 4
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![4]));
    txns.push(SpiTransaction::transaction_end());
    // payload burst
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xFF]));
    txns.push(SpiTransaction::read_vec(payload.to_vec()));
    txns.push(SpiTransaction::transaction_end());
    // RSSI
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![0x80]));
    txns.push(SpiTransaction::transaction_end());
    // status: CRC bit = 0 (bad CRC), LQI = 30
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xBF]));
    txns.push(SpiTransaction::read_vec(vec![0x1E])); // bit 7 = 0 → CRC fail
    txns.push(SpiTransaction::transaction_end());

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = PinMock::new(&[
        PinTransaction::wait_for_edge(Edge::Rising),
        PinTransaction::wait_for_edge(Edge::Falling),
    ]);
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();
    radio.configure(&config).await.unwrap();

    let mut buf = [0u8; 64];
    let result = radio.receive(&mut buf).await;
    assert!(result.is_err(), "CRC failure should return error");

    spi.done();
    gdo0.done();
    gdo2.done();
}

// ---- Cc1101::read_rssi() ----------------------------------------------------

#[tokio::test]
async fn read_rssi_converts_raw_value_correctly() {
    let mut txns = new_transactions();

    // start_rx: SRX strobe (0x34)
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0x34]));
    txns.push(SpiTransaction::transaction_end());
    // read_rssi: read STATUS_RSSI (0x34 | 0x80 | 0x40 = 0xF4)
    // raw = 0x50 = 80 → (80/2) - 74 = -34 dBm
    txns.push(SpiTransaction::transaction_start());
    txns.push(SpiTransaction::write_vec(vec![0xF4]));
    txns.push(SpiTransaction::read_vec(vec![0x50]));
    txns.push(SpiTransaction::transaction_end());

    let mut spi = SpiMock::new(&txns);
    let mut gdo0 = idle_pin();
    let mut gdo2 = idle_pin();

    let mut radio = Cc1101::new(spi.clone(), gdo0.clone(), gdo2.clone())
        .await
        .unwrap();

    radio.start_rx().await.unwrap();
    let rssi = radio.read_rssi().await.unwrap();
    assert_eq!(rssi, -34);

    spi.done();
    gdo0.done();
    gdo2.done();
}
