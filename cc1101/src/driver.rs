//! Core CC1101 driver implementation.

use embedded_hal_async::digital::Wait;
use embedded_hal_async::spi::{Operation, SpiDevice};

use crate::config::{PacketLength, RadioConfig};
use crate::error::Error;
use crate::regs;

// ---- Public types -----------------------------------------------------------

/// A received packet, returned by [`Cc1101::receive`].
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ReceivedPacket {
    /// Number of payload bytes written into the caller's buffer.
    pub len: usize,
    /// Received signal strength in dBm.
    /// Only valid when `append_status` is enabled in [`RadioConfig`].
    pub rssi_dbm: i16,
    /// Link quality indicator (0–127; higher is better).
    /// Only valid when `append_status` is enabled in [`RadioConfig`].
    pub lqi: u8,
    /// Whether the hardware CRC was OK.
    /// Always `true` when `crc_enable` is set — packets failing CRC are dropped.
    pub crc_ok: bool,
}

// ---- Driver struct ----------------------------------------------------------

/// Async CC1101 driver.
///
/// `SPI` must implement [`SpiDevice`] (i.e. it manages CS itself).
/// `GDO0` and `GDO2` must implement [`Wait`] (Embassy GPIO inputs do).
///
/// Construct with [`Cc1101::new`], then call [`Cc1101::configure`] before use.
pub struct Cc1101<SPI, GDO0, GDO2> {
    spi:    SPI,
    gdo0:   GDO0,
    /// Reserved for future use: RX FIFO threshold interrupt and sync-word detection.
    /// GDO2 is configured as RX FIFO threshold in [`Cc1101::configure`] but is not
    /// yet awaited directly — polling RXBYTES is used instead for simplicity.
    #[allow(dead_code)]
    gdo2:   GDO2,
    config: Option<RadioConfig>,
}

impl<SPI, GDO0, GDO2> Cc1101<SPI, GDO0, GDO2>
where
    SPI:  SpiDevice,
    GDO0: Wait,
    GDO2: Wait,
{
    // ---- Construction -------------------------------------------------------

    /// Create a new CC1101 driver, reset the chip, and verify its identity.
    ///
    /// # Errors
    /// Returns [`Error::InvalidChip`] if the part number or version register
    /// doesn't match a CC1101, indicating a wiring problem.
    pub async fn new(
        spi:  SPI,
        gdo0: GDO0,
        gdo2: GDO2,
    ) -> Result<Self, Error<SPI::Error>> {
        let mut dev = Self { spi, gdo0, gdo2, config: None };

        dev.reset().await?;

        // Verify chip identity
        let part    = dev.read_status(regs::STATUS_PARTNUM).await?;
        let version = dev.read_status(regs::STATUS_VERSION).await?;

        // CC1101 always returns 0x00 for PARTNUM; VERSION is typically 0x04 or 0x14
        if part != 0x00 || (version != 0x04 && version != 0x14) {
            return Err(Error::InvalidChip { part, version });
        }

        Ok(dev)
    }

    // ---- Configuration ------------------------------------------------------

    /// Apply a [`RadioConfig`] to the hardware.
    ///
    /// Can be called multiple times to reconfigure without re-creating the driver.
    pub async fn configure(&mut self, config: &RadioConfig) -> Result<(), Error<SPI::Error>> {
        // Strobe IDLE first — most registers can only be written in IDLE state
        self.strobe(regs::SIDLE).await?;

        // Frequency
        let (freq2, freq1, freq0) = config.freq_registers();
        self.write_reg(regs::FREQ2, freq2).await?;
        self.write_reg(regs::FREQ1, freq1).await?;
        self.write_reg(regs::FREQ0, freq0).await?;

        // Baud rate
        let (mdmcfg4, mdmcfg3) = config.baud_rate_registers();
        self.write_reg(regs::MDMCFG4, mdmcfg4).await?;
        self.write_reg(regs::MDMCFG3, mdmcfg3).await?;

        // Modulation and sync mode
        self.write_reg(regs::MDMCFG2, config.mdmcfg2()).await?;

        // Sync word
        self.write_reg(regs::SYNC1, (config.sync_word >> 8) as u8).await?;
        self.write_reg(regs::SYNC0, (config.sync_word & 0xFF) as u8).await?;

        // Deviation (FSK/GFSK only — OOK ignores this)
        self.write_reg(regs::DEVIATN, config.deviation_register()).await?;

        // Packet format
        let (pktlen, pktctrl0) = Self::packet_registers(config);
        self.write_reg(regs::PKTLEN, pktlen).await?;
        self.write_reg(regs::PKTCTRL0, pktctrl0).await?;

        // PKTCTRL1: append status bytes, no address check
        let pktctrl1 = if config.append_status { 0x04 } else { 0x00 };
        self.write_reg(regs::PKTCTRL1, pktctrl1).await?;

        // Channel
        self.write_reg(regs::CHANNR, config.channel).await?;

        // TX power — load PA table index 0 only (simple non-ASK use)
        self.write_reg(regs::FREND0, 0x10).await?; // PA table index = 0
        self.write_patable(config.tx_power.patable_byte()).await?;

        // GDO0: assert when sync word received, de-assert at end of packet
        // This gives us a clean falling edge to wait on after TX/RX completes.
        self.write_reg(regs::IOCFG0, regs::GDO_SYNC_WORD).await?;

        // GDO2: assert when RX FIFO at or above threshold
        self.write_reg(regs::IOCFG2, regs::GDO_RX_FIFO_THRESHOLD).await?;

        // Recommended register values from datasheet (Table 41)
        // These improve sensitivity and should always be applied.
        self.write_reg(regs::AGCCTRL2, 0x43).await?;
        self.write_reg(regs::TEST2,    0x81).await?;
        self.write_reg(regs::TEST1,    0x35).await?;
        self.write_reg(regs::TEST0,    0x09).await?;
        self.write_reg(regs::FSCAL3,   0xE9).await?;
        self.write_reg(regs::FSCAL2,   0x2A).await?;
        self.write_reg(regs::FSCAL1,   0x00).await?;
        self.write_reg(regs::FSCAL0,   0x1F).await?;

        // Auto-calibrate when going from IDLE to RX/TX
        self.write_reg(regs::MCSM0, 0x18).await?;

        // After RX: stay in IDLE. After TX: go to IDLE.
        // This keeps state simple — the driver always commands the next action.
        self.write_reg(regs::MCSM1, 0x00).await?;

        self.config = Some(config.clone());
        Ok(())
    }

    // ---- RSSI ---------------------------------------------------------------

    /// Read the current received signal strength in dBm.
    ///
    /// The chip must be in RX mode — call [`Cc1101::start_rx`] first.
    /// This is useful as the very first hardware test: put the chip in RX,
    /// point a 433 MHz transmitter at it, and watch the value change.
    ///
    /// Returns dBm as a signed 16-bit value (typically –110 to –10 dBm).
    pub async fn read_rssi(&mut self) -> Result<i16, Error<SPI::Error>> {
        let raw = self.read_status(regs::STATUS_RSSI).await?;
        Ok(Self::rssi_dbm(raw))
    }

    /// Put the chip into RX mode.
    pub async fn start_rx(&mut self) -> Result<(), Error<SPI::Error>> {
        self.strobe(regs::SRX).await
    }

    /// Put the chip into IDLE mode.
    pub async fn idle(&mut self) -> Result<(), Error<SPI::Error>> {
        self.strobe(regs::SIDLE).await
    }

    // ---- Transmit -----------------------------------------------------------

    /// Transmit a packet asynchronously.
    ///
    /// Loads `data` into the TX FIFO, strobes TX, then waits for GDO0 to
    /// assert (sync word sent) and de-assert (packet complete) before returning.
    ///
    /// Max payload: 61 bytes in variable-length mode, or whatever fixed length
    /// was set in [`RadioConfig`].
    pub async fn transmit(&mut self, data: &[u8]) -> Result<(), Error<SPI::Error>> {
        let max_len = match self.config.as_ref().map(|c| c.packet_length) {
            Some(PacketLength::Variable(n)) => n as usize,
            Some(PacketLength::Fixed(n))    => n as usize,
            None => PacketLength::MAX_VARIABLE as usize,
        };

        if data.len() > max_len {
            return Err(Error::PayloadTooLong);
        }

        // Ensure we're in IDLE before touching the FIFO
        self.strobe(regs::SIDLE).await?;
        self.strobe(regs::SFTX).await?;

        // Write length byte (variable mode) then payload
        if matches!(
            self.config.as_ref().map(|c| c.packet_length),
            Some(PacketLength::Variable(_)) | None
        ) {
            self.write_reg(regs::TXFIFO, data.len() as u8).await?;
        }
        self.write_burst(regs::TXFIFO, data).await?;

        // Strobe TX — chip will calibrate then transmit
        self.strobe(regs::STX).await?;

        // GDO0 is configured as SYNC_WORD: goes high when sync sent,
        // goes low when packet complete. Wait for the falling edge.
        self.gdo0.wait_for_high().await.map_err(|_| Error::UnexpectedState { state: 0 })?;
        self.gdo0.wait_for_low().await.map_err(|_| Error::UnexpectedState { state: 0 })?;

        Ok(())
    }

    // ---- Receive ------------------------------------------------------------

    /// Wait for and receive a single packet asynchronously.
    ///
    /// Puts the chip into RX mode and waits for GDO0 to assert then de-assert
    /// (indicating a complete packet in the FIFO), then reads the FIFO into
    /// `buf`. Returns a [`ReceivedPacket`] with length, RSSI, and LQI.
    ///
    /// If the RX FIFO overflows, flushes it and returns [`Error::RxFifoOverflow`].
    /// If CRC fails (and `crc_enable` is set), returns [`Error::CrcError`].
    pub async fn receive(&mut self, buf: &mut [u8]) -> Result<ReceivedPacket, Error<SPI::Error>> {
        self.strobe(regs::SRX).await?;

        // Wait for a complete packet: GDO0 high (sync received), then low (end of packet)
        self.gdo0.wait_for_high().await.map_err(|_| Error::UnexpectedState { state: 0 })?;
        self.gdo0.wait_for_low().await.map_err(|_| Error::UnexpectedState { state: 0 })?;

        // Check for FIFO overflow
        let rxbytes = self.read_status(regs::STATUS_RXBYTES).await?;
        if rxbytes & regs::RXFIFO_OVERFLOW != 0 {
            self.strobe(regs::SFRX).await?;
            self.strobe(regs::SIDLE).await?;
            return Err(Error::RxFifoOverflow);
        }

        let available = (rxbytes & 0x7F) as usize;
        if available == 0 {
            return Err(Error::UnexpectedState { state: 0 });
        }

        // Read length byte in variable mode
        let payload_len = {
            let is_variable = matches!(
                self.config.as_ref().map(|c| c.packet_length),
                Some(PacketLength::Variable(_)) | None
            );
            if is_variable {
                self.read_reg(regs::RXFIFO).await? as usize
            } else {
                // Fixed length: read from config
                match self.config.as_ref().map(|c| c.packet_length) {
                    Some(PacketLength::Fixed(n)) => n as usize,
                    _ => available,
                }
            }
        };

        let read_len = payload_len.min(buf.len());
        self.read_burst(regs::RXFIFO, &mut buf[..read_len]).await?;

        // Read appended status bytes if configured (RSSI + LQI/CRC)
        let append_status = self.config.as_ref().map(|c| c.append_status).unwrap_or(true);
        let crc_enable    = self.config.as_ref().map(|c| c.crc_enable).unwrap_or(true);

        let (rssi_dbm, lqi, crc_ok) = if append_status {
            let rssi_raw = self.read_reg(regs::RXFIFO).await?;
            let status   = self.read_reg(regs::RXFIFO).await?;
            let crc_ok   = (status & 0x80) != 0;
            let lqi      = status & 0x7F;
            (Self::rssi_dbm(rssi_raw), lqi, crc_ok)
        } else {
            (0i16, 0u8, true)
        };

        if crc_enable && !crc_ok {
            return Err(Error::CrcError);
        }

        Ok(ReceivedPacket { len: read_len, rssi_dbm, lqi, crc_ok })
    }

    // ---- SPI primitives (private) -------------------------------------------

    /// Reset the CC1101 to power-on defaults via the SRES strobe.
    async fn reset(&mut self) -> Result<(), Error<SPI::Error>> {
        // The CC1101 requires CSn to pulse high briefly before SRES on power-up.
        // SpiDevice manages CS, so we issue a dummy transaction then SRES.
        self.strobe(regs::SRES).await
    }

    /// Write a single configuration register.
    pub(crate) async fn write_reg(&mut self, addr: u8, value: u8) -> Result<(), Error<SPI::Error>> {
        // Header byte: write (bit 7 = 0), single (bit 6 = 0), address [5:0]
        self.spi
            .transaction(&mut [
                Operation::Write(&[addr & 0x3F, value]),
            ])
            .await
            .map_err(Error::Spi)
    }

    /// Read a configuration register.
    async fn read_reg(&mut self, addr: u8) -> Result<u8, Error<SPI::Error>> {
        let mut buf = [0u8; 1];
        self.spi
            .transaction(&mut [
                Operation::Write(&[addr | regs::READ]),
                Operation::Read(&mut buf),
            ])
            .await
            .map_err(Error::Spi)?;
        Ok(buf[0])
    }

    /// Read a status register (burst bit set, read bit set).
    async fn read_status(&mut self, addr: u8) -> Result<u8, Error<SPI::Error>> {
        let mut buf = [0u8; 1];
        self.spi
            .transaction(&mut [
                Operation::Write(&[addr | regs::READ | regs::BURST]),
                Operation::Read(&mut buf),
            ])
            .await
            .map_err(Error::Spi)?;
        Ok(buf[0])
    }

    /// Issue a strobe command (single-byte write, no data).
    async fn strobe(&mut self, cmd: u8) -> Result<(), Error<SPI::Error>> {
        self.spi
            .transaction(&mut [
                Operation::Write(&[cmd]),
            ])
            .await
            .map_err(Error::Spi)
    }

    /// Burst write to a register (multiple bytes in one CS assertion).
    async fn write_burst(&mut self, addr: u8, data: &[u8]) -> Result<(), Error<SPI::Error>> {
        // Header: write, burst, address
        let header = [addr | regs::BURST];
        self.spi
            .transaction(&mut [
                Operation::Write(&header),
                Operation::Write(data),
            ])
            .await
            .map_err(Error::Spi)
    }

    /// Burst read from a register.
    async fn read_burst(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), Error<SPI::Error>> {
        let header = [addr | regs::READ | regs::BURST];
        self.spi
            .transaction(&mut [
                Operation::Write(&header),
                Operation::Read(buf),
            ])
            .await
            .map_err(Error::Spi)
    }

    /// Write the PA table (single entry — index 0 only for non-ASK use).
    async fn write_patable(&mut self, value: u8) -> Result<(), Error<SPI::Error>> {
        self.spi
            .transaction(&mut [
                Operation::Write(&[regs::PATABLE | regs::BURST, value]),
            ])
            .await
            .map_err(Error::Spi)
    }

    // ---- RSSI conversion ----------------------------------------------------

    /// Convert raw RSSI byte to dBm per CC1101 datasheet section 17.3.
    fn rssi_dbm(raw: u8) -> i16 {
        rssi_raw_to_dbm(raw)
    }

    // ---- Packet register helpers --------------------------------------------

    fn packet_registers(config: &RadioConfig) -> (u8, u8) {
        // PKTLEN: the length byte
        // PKTCTRL0 bits: [6:5] = packet format, [2] = CRC enable, [1:0] = length config
        let (pktlen, len_config) = match config.packet_length {
            PacketLength::Fixed(n)    => (n, 0b00u8), // fixed length
            PacketLength::Variable(n) => (n, 0b01u8), // variable length
        };

        let crc_bit = if config.crc_enable { 0b0100 } else { 0b0000 };
        // Normal packet format (no serial or random TX): bits [6:5] = 00
        // Data whitening off: bit 6 = 0
        let pktctrl0 = crc_bit | len_config;

        (pktlen, pktctrl0)
    }
}

// ---- Free functions ---------------------------------------------------------

/// Convert raw CC1101 RSSI register byte to dBm.
///
/// Formula from datasheet section 17.3:
///   if RSSI_dec >= 128: RSSI_dBm = (RSSI_dec - 256) / 2 - RSSI_offset
///   else:               RSSI_dBm = RSSI_dec / 2 - RSSI_offset
/// RSSI_offset = 74 dB (typical for CC1101)
pub(crate) fn rssi_raw_to_dbm(raw: u8) -> i16 {
    const RSSI_OFFSET: i16 = 74;
    if raw >= 128 {
        ((raw as i16 - 256) / 2) - RSSI_OFFSET
    } else {
        (raw as i16 / 2) - RSSI_OFFSET
    }
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rssi_positive_raw() {
        // raw=0x72 (114 decimal): 114/2 - 74 = -17 dBm
        assert_eq!(rssi_raw_to_dbm(0x72), -17);
    }

    #[test]
    fn rssi_negative_raw() {
        // raw=0xEC (236 decimal): (236-256)/2 - 74 = -10 - 74 = -84 dBm
        assert_eq!(rssi_raw_to_dbm(0xEC), -84);
    }

    #[test]
    fn rssi_boundary_128() {
        // raw=0x80 (128): (128-256)/2 - 74 = -64 - 74 = -138 dBm (noise floor)
        assert_eq!(rssi_raw_to_dbm(0x80), -138);
    }

    #[test]
    fn rssi_zero() {
        // raw=0x00: 0/2 - 74 = -74 dBm
        assert_eq!(rssi_raw_to_dbm(0x00), -74);
    }
}
