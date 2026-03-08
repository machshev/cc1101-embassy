//! CC1101 register addresses, strobe commands, and status bits.
//!
//! All values taken directly from the CC1101 datasheet (SWRS061I), Table 43-45.
//! Registers are split into three groups:
//!   - Configuration registers (0x00–0x2E): read/write, persist across resets
//!   - Status registers (0x30–0x3D): read-only, prefixed with STATUS_
//!   - Strobe commands (0x30–0x3D when written): write-only one-shot commands
//!
//! Not every constant is used by the driver today — the full register map is
//! provided here as a reference for future features and user extension.
#![allow(dead_code)]
//!   - Status registers (0x30–0x3D): read-only, prefixed with STATUS_
//!   - Strobe commands (0x30–0x3D when written): write-only one-shot commands

// ---- Configuration registers ------------------------------------------------

/// GDO2 signal selection and I/O pin configuration
pub const IOCFG2: u8 = 0x00;
/// GDO1 signal selection and I/O pin configuration (shared with MISO)
pub const IOCFG1: u8 = 0x01;
/// GDO0 signal selection and I/O pin configuration
pub const IOCFG0: u8 = 0x02;
/// TX FIFO thresholds and RX FIFO thresholds
pub const FIFOTHR: u8 = 0x03;
/// Sync word, high byte
pub const SYNC1: u8 = 0x04;
/// Sync word, low byte
pub const SYNC0: u8 = 0x05;
/// Packet length (in fixed length mode)
pub const PKTLEN: u8 = 0x06;
/// Packet automation control: address check, CRC autoflush, status append
pub const PKTCTRL1: u8 = 0x07;
/// Packet automation control: data whitening, CRC enable, packet format, length config
pub const PKTCTRL0: u8 = 0x08;
/// Device address for filtering
pub const ADDR: u8 = 0x09;
/// Channel number — added to base frequency
pub const CHANNR: u8 = 0x0A;
/// Frequency synthesiser IF frequency
pub const FSCTRL1: u8 = 0x0B;
/// Frequency synthesiser DC offset compensation
pub const FSCTRL0: u8 = 0x0C;
/// Base frequency, high byte
pub const FREQ2: u8 = 0x0D;
/// Base frequency, mid byte
pub const FREQ1: u8 = 0x0E;
/// Base frequency, low byte
pub const FREQ0: u8 = 0x0F;
/// Modem configuration: channel bandwidth and data rate exponent
pub const MDMCFG4: u8 = 0x10;
/// Modem configuration: data rate mantissa
pub const MDMCFG3: u8 = 0x11;
/// Modem configuration: modulation format and sync word mode
pub const MDMCFG2: u8 = 0x12;
/// Modem configuration: FEC, preamble count
pub const MDMCFG1: u8 = 0x13;
/// Modem configuration: channel spacing
pub const MDMCFG0: u8 = 0x14;
/// Modem deviation (FSK/MSK frequency offset)
pub const DEVIATN: u8 = 0x15;
/// Main radio control state machine: RX timeout and CCA mode
pub const MCSM2: u8 = 0x16;
/// Main radio control state machine: CCA mode, RX/TX off modes
pub const MCSM1: u8 = 0x17;
/// Main radio control state machine: auto-calibration, power-on timeout
pub const MCSM0: u8 = 0x18;
/// Frequency offset compensation
pub const FOCCFG: u8 = 0x19;
/// Bit synchronisation configuration
pub const BSCFG: u8 = 0x1A;
/// AGC control: maximum gain, target amplitude
pub const AGCCTRL2: u8 = 0x1B;
/// AGC control: LNA priority, carrier sense threshold
pub const AGCCTRL1: u8 = 0x1C;
/// AGC control: hysteresis, wait time, freeze
pub const AGCCTRL0: u8 = 0x1D;
/// Wake on radio event 0 timeout (high byte)
pub const WOREVT1: u8 = 0x1E;
/// Wake on radio event 0 timeout (low byte)
pub const WOREVT0: u8 = 0x1F;
/// Wake on radio control
pub const WORCTRL: u8 = 0x20;
/// Front end RX configuration
pub const FREND1: u8 = 0x21;
/// Front end TX configuration (PA power index)
pub const FREND0: u8 = 0x22;
/// Frequency synthesiser calibration (high)
pub const FSCAL3: u8 = 0x23;
/// Frequency synthesiser calibration
pub const FSCAL2: u8 = 0x24;
/// Frequency synthesiser calibration
pub const FSCAL1: u8 = 0x25;
/// Frequency synthesiser calibration (low)
pub const FSCAL0: u8 = 0x26;
/// RC oscillator configuration (reserved)
pub const RCCTRL1: u8 = 0x27;
/// RC oscillator configuration (reserved)
pub const RCCTRL0: u8 = 0x28;
/// Factory test — do not write
pub const FSTEST: u8 = 0x29;
/// Production test — do not write
pub const PTEST: u8 = 0x2A;
/// AGC test — do not write
pub const AGCTEST: u8 = 0x2B;
/// Test setting — do not write
pub const TEST2: u8 = 0x2C;
/// Test setting — do not write
pub const TEST1: u8 = 0x2D;
/// Test setting — do not write
pub const TEST0: u8 = 0x2E;

// ---- Strobe commands --------------------------------------------------------
// Written to the address byte (with header byte 0x00) to trigger a one-shot
// action. These share address space with status registers — the CC1101
// distinguishes reads (status) from writes (strobe).

/// Reset chip to power-on defaults
pub const SRES: u8 = 0x30;
/// Enable and calibrate frequency synthesiser; ready for TX or RX
pub const SFSTXON: u8 = 0x31;
/// Turn off crystal oscillator
pub const SXOFF: u8 = 0x32;
/// Calibrate frequency synthesiser and turn it off
pub const SCAL: u8 = 0x33;
/// Enable RX
pub const SRX: u8 = 0x34;
/// Enable TX (if in RX, switch to TX when packet sent)
pub const STX: u8 = 0x35;
/// Exit RX/TX, turn off frequency synthesiser and exit WOR
pub const SIDLE: u8 = 0x36;
/// Start automatic RX polling sequence (WOR)
pub const SWOR: u8 = 0x38;
/// Enter power down mode when CSn goes high
pub const SPWD: u8 = 0x39;
/// Flush the RX FIFO buffer — only in IDLE or RXFIFO_OVERFLOW state
pub const SFRX: u8 = 0x3A;
/// Flush the TX FIFO buffer — only in IDLE or TXFIFO_UNDERFLOW state
pub const SFTX: u8 = 0x3B;
/// Reset real time clock to event 1 value
pub const SWORRST: u8 = 0x3C;
/// No operation strobe — reads out status byte
pub const SNOP: u8 = 0x3D;

// ---- Multi-byte registers ---------------------------------------------------

/// PA (power amplifier) power table — up to 8 entries, accessed as burst
pub const PATABLE: u8 = 0x3E;
/// TX FIFO: burst write, single read
pub const TXFIFO: u8 = 0x3F;
/// RX FIFO: burst read, single write
pub const RXFIFO: u8 = 0x3F;

// ---- Status registers (read-only) ------------------------------------------
// These are accessed by setting the burst bit (0x40) in the address byte.

/// Part number — always reads 0x00 for CC1101
pub const STATUS_PARTNUM: u8 = 0x30;
/// Chip version — reads 0x04 for typical CC1101 silicon
pub const STATUS_VERSION: u8 = 0x31;
/// Frequency offset estimate (signed)
pub const STATUS_FREQEST: u8 = 0x32;
/// Current LNA gain value
pub const STATUS_LQI: u8 = 0x33;
/// Received signal strength indication
pub const STATUS_RSSI: u8 = 0x34;
/// Main radio control state machine state
pub const STATUS_MARCSTATE: u8 = 0x35;
/// High byte of WOR time
pub const STATUS_WORTIME1: u8 = 0x36;
/// Low byte of WOR time
pub const STATUS_WORTIME0: u8 = 0x37;
/// Packet status: GDO0/GDO2 state, sync, CRC, carrier sense, channel clear
pub const STATUS_PKTSTATUS: u8 = 0x38;
/// Current RX/TX data rate in baud (high byte)
pub const STATUS_VCO_VC_DAC: u8 = 0x39;
/// TX/RX FIFO status
pub const STATUS_TXBYTES: u8 = 0x3A;
/// Number of bytes in RX FIFO and overflow status
pub const STATUS_RXBYTES: u8 = 0x3B;
/// Last RC oscillator calibration result (high)
pub const STATUS_RCCTRL1_STATUS: u8 = 0x3C;
/// Last RC oscillator calibration result (low)
pub const STATUS_RCCTRL0_STATUS: u8 = 0x3D;

// ---- SPI header byte flags --------------------------------------------------

/// Set in the header byte to indicate a read operation
pub const READ: u8 = 0x80;
/// Set in the header byte to indicate a burst (multi-byte) operation
pub const BURST: u8 = 0x40;

// ---- MARCSTATE values -------------------------------------------------------
// Returned by STATUS_MARCSTATE — the main radio state machine state

pub const MARCSTATE_SLEEP: u8       = 0x00;
pub const MARCSTATE_IDLE: u8        = 0x01;
pub const MARCSTATE_XOFF: u8        = 0x02;
pub const MARCSTATE_VCOON_MC: u8    = 0x03;
pub const MARCSTATE_REGON_MC: u8    = 0x04;
pub const MARCSTATE_MANCAL: u8      = 0x05;
pub const MARCSTATE_VCOON: u8       = 0x06;
pub const MARCSTATE_REGON: u8       = 0x07;
pub const MARCSTATE_STARTCAL: u8    = 0x08;
pub const MARCSTATE_BWBOOST: u8     = 0x09;
pub const MARCSTATE_FS_LOCK: u8     = 0x0A;
pub const MARCSTATE_IFADCON: u8     = 0x0B;
pub const MARCSTATE_ENDCAL: u8      = 0x0C;
pub const MARCSTATE_RX: u8          = 0x0D;
pub const MARCSTATE_RX_END: u8      = 0x0E;
pub const MARCSTATE_RX_RST: u8      = 0x0F;
pub const MARCSTATE_TXRX_SWITCH: u8 = 0x10;
pub const MARCSTATE_RXFIFO_OVERFLOW: u8 = 0x11;
pub const MARCSTATE_FSTXON: u8      = 0x12;
pub const MARCSTATE_TX: u8          = 0x13;
pub const MARCSTATE_TX_END: u8      = 0x14;
pub const MARCSTATE_RXTX_SWITCH: u8 = 0x15;
pub const MARCSTATE_TXFIFO_UNDERFLOW: u8 = 0x16;

// ---- RXBYTES / TXBYTES flags ------------------------------------------------

/// Set in RXBYTES if the RX FIFO has overflowed
pub const RXFIFO_OVERFLOW: u8  = 0x80;
/// Set in TXBYTES if the TX FIFO has underflowed
pub const TXFIFO_UNDERFLOW: u8 = 0x80;

// ---- Status byte (returned on every SPI transfer) ---------------------------

/// Mask for the CHIP_RDYn bit — low means chip is ready
pub const STATUS_CHIP_RDY: u8    = 0x80;
/// Mask for the current STATE field in the status byte
pub const STATUS_STATE_MASK: u8  = 0x70;
/// Mask for the FIFO bytes available field in the status byte
pub const STATUS_FIFO_BYTES: u8  = 0x0F;

// ---- GDO signal values (IOCFG0/1/2) ----------------------------------------

/// GDO asserts when RX FIFO at or above threshold, de-asserts when drained
pub const GDO_RX_FIFO_THRESHOLD: u8   = 0x00;
/// GDO asserts when TX FIFO at or above threshold, de-asserts when refilled
pub const GDO_TX_FIFO_THRESHOLD: u8   = 0x02;
/// GDO asserts when packet received with CRC OK; de-asserts on FIFO empty
pub const GDO_PACKET_RECEIVED: u8     = 0x07;
/// GDO asserts when preamble quality is high enough (carrier sense)
pub const GDO_CARRIER_SENSE: u8       = 0x0E;
/// GDO asserts when CRC is OK (sync to end of packet)
pub const GDO_CRC_OK: u8              = 0x07;
/// GDO driven low when chip is in TX state — convenient for TX-done detection
pub const GDO_CLK_XOSC_DIV192: u8    = 0x3F;
/// Hardwired high — useful for testing a GDO pin connection
pub const GDO_HI_Z: u8               = 0x2E;
/// Sync word sent/received: asserts at start, de-asserts at end of packet
pub const GDO_SYNC_WORD: u8          = 0x06;
