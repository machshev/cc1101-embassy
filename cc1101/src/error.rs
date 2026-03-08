//! Error types for the CC1101 driver.

/// All errors that can be returned by the CC1101 driver.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<SpiError> {
    /// An error occurred on the SPI bus
    Spi(SpiError),

    /// The RX FIFO overflowed before we could read it.
    /// The driver will automatically flush the FIFO and return to IDLE.
    RxFifoOverflow,

    /// The TX FIFO underflowed during transmission.
    TxFifoUnderflow,

    /// A packet was received but the hardware CRC check failed.
    /// Only returned when `crc_enable` is set in [`RadioConfig`].
    CrcError,

    /// The provided payload exceeds the maximum packet length.
    /// For variable-length mode, max is 61 bytes; fixed-length is set at config time.
    PayloadTooLong,

    /// The chip did not respond correctly during initialisation.
    /// Check your SPI wiring and that the CC1101 is powered.
    InvalidChip {
        /// Raw value of the PARTNUM status register (expected 0x00)
        part: u8,
        /// Raw value of the VERSION status register (expected 0x04 or 0x14)
        version: u8,
    },

    /// The chip was in an unexpected state when an operation was attempted.
    UnexpectedState {
        /// Raw MARCSTATE register value at the time of the error
        state: u8,
    },
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Spi(e)
    }
}
