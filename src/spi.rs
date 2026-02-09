//! SPI driver for F1C100S/F1C200S
//!
//! The F1C100S has two SPI controllers (SPI0, SPI1) with:
//! - Full-duplex synchronous serial interface
//! - Master/Slave configurable
//! - 64-byte TX/RX FIFOs
//! - 4 chip selects for SPI0, 1 for SPI1
//! - SPI modes 0-3 (CPOL/CPHA)
//! - Clock: 3KHz ~ 100MHz, AHB_CLK >= 2x SPI_SCLK
//!
//! Pin mapping (from datasheet):
//! - SPI0: PC0=CLK, PC1=CS, PC2=MISO, PC3=MOSI (Func2)
//! - SPI1: PA0=CS, PA1=MOSI, PA2=CLK, PA3=MISO (Func5)

use core::marker::PhantomData;

use embedded_hal::spi::{Mode, Phase, Polarity, MODE_0};

use crate::gpio::{self, PinMode, Pull};
use crate::{pac, rcc, Peri};

const SPI_FIFO_DEPTH: usize = 64;

/// SPI Error
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    RxOverflow,
    TxUnderrun,
    Timeout,
}

#[derive(Copy, Clone)]
pub enum BitOrder {
    MsbFirst,
    LsbFirst,
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum ChipSelect {
    Ss0 = 0,
    Ss1 = 1,
    Ss2 = 2,
    Ss3 = 3,
}

#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    pub mode: Mode,
    pub bit_order: BitOrder,
    pub frequency: crate::time::Hertz,
    pub cs: ChipSelect,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: MODE_0,
            bit_order: BitOrder::MsbFirst,
            frequency: crate::time::Hertz(1_000_000),
            cs: ChipSelect::Ss0,
        }
    }
}

/// SPI driver (blocking, master mode).
pub struct Spi<'d, T: Instance> {
    _peri: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> Spi<'d, T> {
    /// Create SPI master with SCK, MOSI, MISO, CS pins.
    pub fn new(
        _peri: Peri<'d, T>,
        sck: Peri<'d, impl SckPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        cs: Peri<'d, impl CsPin<T>>,
        config: Config,
    ) -> Self {
        // Enable clock and reset BEFORE configuring pins/registers
        T::enable_clock();
        T::assert_reset();
        T::deassert_reset();
        into_af_pin(&*sck);
        into_af_pin(&*mosi);
        into_af_pin(&*miso);
        into_af_pin(&*cs);
        let mut this = Self { _peri: PhantomData };
        this.configure(&config);
        this
    }

    /// Create SPI master without CS pin (use cs_low/cs_high for manual control).
    pub fn new_nocs(
        _peri: Peri<'d, T>,
        sck: Peri<'d, impl SckPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        config: Config,
    ) -> Self {
        T::enable_clock();
        T::assert_reset();
        T::deassert_reset();
        into_af_pin(&*sck);
        into_af_pin(&*mosi);
        into_af_pin(&*miso);
        let mut this = Self { _peri: PhantomData };
        this.configure(&config);
        this
    }

    #[inline]
    fn regs() -> &'static pac::spi0::RegisterBlock {
        unsafe { &*T::regs() }
    }

    fn configure(&mut self, config: &Config) {
        let regs = Self::regs();

        // Soft reset
        regs.spi_gcr().modify(|_, w| w.srst().set_bit());
        while regs.spi_gcr().read().srst().bit_is_set() {}

        // Enable SPI, master mode, transmit pause enable
        regs.spi_gcr().write(|w| {
            w.en().set_bit();
            w.mode().set_bit();
            w.tp_en().set_bit()
        });

        // Transfer control: CPOL, CPHA, bit order, CS, etc.
        // Manual CS mode from the start. In manual mode (SS_OWNER=1),
        // SS_LEVEL directly controls pin output:
        //   ss_level=0 → pin LOW (CS asserted)
        //   ss_level=1 → pin HIGH (CS deasserted)
        regs.spi_tcr().write(|w| unsafe {
            match config.mode.phase {
                Phase::CaptureOnFirstTransition => w.cpha().clear_bit(),
                Phase::CaptureOnSecondTransition => w.cpha().set_bit(),
            };
            match config.mode.polarity {
                Polarity::IdleLow => w.cpol().clear_bit(),
                Polarity::IdleHigh => w.cpol().set_bit(),
            };
            w.spol().set_bit(); // CS active low
            w.ss_sel().bits(config.cs as u8);
            w.ss_owner().set_bit(); // Manual CS control
            w.ss_level().set_bit(); // Deasserted (pin HIGH)
            w.dhb().set_bit(); // discard hash burst
            match config.bit_order {
                BitOrder::MsbFirst => w.fbs().clear_bit(),
                BitOrder::LsbFirst => w.fbs().set_bit(),
            };
            w
        });

        // Reset FIFOs
        regs.spi_fcr().write(|w| unsafe {
            w.tf_rst().set_bit();
            w.rf_rst().set_bit();
            w.rx_trig_level().bits(1);
            w.tx_trig_level().bits(SPI_FIFO_DEPTH as u8 / 4)
        });
        // Wait for FIFO reset to complete
        while regs.spi_fcr().read().tf_rst().bit_is_set() || regs.spi_fcr().read().rf_rst().bit_is_set() {}

        // Disable all interrupts (write 0 to all bits)
        regs.spi_ier().write(|w| w);

        // Clear all pending interrupt flags
        regs.spi_isr().write(|w| {
            w.tc().set_bit();
            w.rx_rdy().set_bit();
            w.tx_ready().set_bit();
            w.rx_ovf().set_bit();
            w.rx_udf().set_bit();
            w.tf_ovf().set_bit();
            w.tf_udf().set_bit();
            w.ssi().set_bit();
            w.rx_emp().set_bit();
            w.rx_full().set_bit();
            w.tx_emp().set_bit();
            w.tx_full().set_bit()
        });

        self.set_clock(config.frequency);
    }

    fn set_clock(&self, freq: crate::time::Hertz) {
        let regs = Self::regs();
        let ahb_clk = rcc::clocks().hclk.0;
        let target = freq.0;
        if target == 0 {
            return;
        }

        // CDR2: SPICLK = AHB_CLK / (2*(CDR2+1)), CDR2 range 0..=255
        let cdr2_val = if target >= ahb_clk / 2 {
            0u32
        } else {
            (ahb_clk / (2 * target)).saturating_sub(1)
        };

        if cdr2_val <= 255 {
            regs.spi_ccr().write(|w| unsafe {
                w.drs().set_bit();
                w.cdr2().bits(cdr2_val as u8)
            });
        } else {
            // CDR1: SPICLK = AHB_CLK / 2^(CDR1+1)
            let mut cdr1 = 0u8;
            while cdr1 < 15 {
                if ahb_clk / (1u32 << (cdr1 as u32 + 1)) <= target {
                    break;
                }
                cdr1 += 1;
            }
            regs.spi_ccr().write(|w| unsafe {
                w.drs().clear_bit();
                w.cdr1().bits(cdr1)
            });
        }
    }

    fn set_dhb(&self, discard: bool) {
        Self::regs().spi_tcr().modify(|_, w| {
            if discard {
                w.dhb().set_bit()
            } else {
                w.dhb().clear_bit()
            }
        });
    }

    /// Write one byte to TX FIFO using 8-bit access.
    /// The F1C100S SPI TX FIFO must be accessed in byte units;
    /// a 32-bit write would push 4 bytes into the FIFO.
    #[inline]
    fn write_txd_byte(byte: u8) {
        let txd_addr = T::regs() as usize + 0x200;
        unsafe {
            core::ptr::write_volatile(txd_addr as *mut u8, byte);
        }
    }

    /// Read one byte from RX FIFO using 8-bit access.
    #[inline]
    fn read_rxd_byte() -> u8 {
        let rxd_addr = T::regs() as usize + 0x300;
        unsafe { core::ptr::read_volatile(rxd_addr as *const u8) }
    }

    fn reset_fifos(&self) {
        let regs = Self::regs();
        regs.spi_fcr().modify(|_, w| {
            w.tf_rst().set_bit();
            w.rf_rst().set_bit()
        });
        while regs.spi_fcr().read().tf_rst().bit_is_set() || regs.spi_fcr().read().rf_rst().bit_is_set() {}
    }

    fn wait_transfer_complete(&self) -> Result<(), Error> {
        let regs = Self::regs();
        let mut timeout = 0xFFFF_FFFFu32;
        while timeout > 0 {
            if regs.spi_isr().read().tc().bit_is_set() {
                regs.spi_isr().write(|w| w.tc().set_bit());
                return Ok(());
            }
            timeout -= 1;
        }
        Err(Error::Timeout)
    }

    /// Full-duplex SPI transfer: send `tx_buf`, then receive `rx_buf.len()` bytes.
    ///
    /// Total burst = tx_buf.len() + rx_buf.len(). During the TX phase, received
    /// bytes are discarded. During the RX phase, dummy bytes (0x00) are sent.
    pub fn transfer(&mut self, tx_buf: &[u8], rx_buf: &mut [u8]) -> Result<(), Error> {
        let regs = Self::regs();
        let total_len = tx_buf.len() + rx_buf.len();
        if total_len == 0 {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(false);

        // Burst counters
        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(total_len as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(tx_buf.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(tx_buf.len() as u32) });

        // Fill TX FIFO (byte access)
        let mut tx_idx = 0usize;
        let initial = tx_buf.len().min(SPI_FIFO_DEPTH);
        while tx_idx < initial {
            Self::write_txd_byte(tx_buf[tx_idx]);
            tx_idx += 1;
        }

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        // Start exchange
        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        // Continue filling TX FIFO
        while tx_idx < tx_buf.len() {
            if (regs.spi_fsr().read().tf_cnt().bits() as usize) < SPI_FIFO_DEPTH {
                Self::write_txd_byte(tx_buf[tx_idx]);
                tx_idx += 1;
            }
        }

        // Read RX: skip tx_buf.len() echo bytes, then collect rx_buf
        let mut rx_skip = tx_buf.len();
        let mut rx_idx = 0usize;
        let mut rx_done = 0usize;
        while rx_done < total_len {
            let cnt = regs.spi_fsr().read().rf_cnt().bits() as usize;
            for _ in 0..cnt {
                let byte = Self::read_rxd_byte();
                rx_done += 1;
                if rx_skip > 0 {
                    rx_skip -= 1;
                } else if rx_idx < rx_buf.len() {
                    rx_buf[rx_idx] = byte;
                    rx_idx += 1;
                }
            }
        }

        self.wait_transfer_complete()
    }

    /// Write-only: send bytes, discard received data.
    pub fn blocking_write(&mut self, data: &[u8]) -> Result<(), Error> {
        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(true);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(data.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(data.len() as u32) });

        let mut idx = 0usize;
        let initial = data.len().min(SPI_FIFO_DEPTH);
        while idx < initial {
            Self::write_txd_byte(data[idx]);
            idx += 1;
        }

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        while idx < data.len() {
            if (regs.spi_fsr().read().tf_cnt().bits() as usize) < SPI_FIFO_DEPTH {
                Self::write_txd_byte(data[idx]);
                idx += 1;
            }
        }

        self.wait_transfer_complete()
    }

    /// Read-only: send dummy 0x00, collect received bytes.
    pub fn blocking_read(&mut self, data: &mut [u8]) -> Result<(), Error> {
        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(false);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(0) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(0) });

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        let mut idx = 0usize;
        while idx < data.len() {
            let cnt = regs.spi_fsr().read().rf_cnt().bits() as usize;
            for _ in 0..cnt {
                if idx < data.len() {
                    data[idx] = Self::read_rxd_byte();
                    idx += 1;
                }
            }
        }

        self.wait_transfer_complete()
    }

    /// Assert CS (drive low) for manual chip-select control.
    /// In manual mode (SS_OWNER=1), SS_LEVEL directly controls the pin output.
    /// ss_level=0 → pin LOW (asserted for active-low CS)
    pub fn cs_low(&self) {
        Self::regs().spi_tcr().modify(|_, w| w.ss_level().clear_bit());
    }

    /// De-assert CS (drive high).
    /// ss_level=1 → pin HIGH (deasserted)
    pub fn cs_high(&self) {
        Self::regs().spi_tcr().modify(|_, w| w.ss_level().set_bit());
    }

    /// Dump all SPI register values (println version, no defmt needed).
    pub fn dump_regs_println(&self) {
        let regs = Self::regs();
        crate::println!("SPI_GCR:  0x{:08X}", regs.spi_gcr().read().bits());
        crate::println!("SPI_TCR:  0x{:08X}", regs.spi_tcr().read().bits());
        crate::println!("SPI_CCR:  0x{:08X}", regs.spi_ccr().read().bits());
        crate::println!("SPI_FCR:  0x{:08X}", regs.spi_fcr().read().bits());
        crate::println!("SPI_FSR:  0x{:08X}", regs.spi_fsr().read().bits());
        crate::println!("SPI_IER:  0x{:08X}", regs.spi_ier().read().bits());
        crate::println!("SPI_ISR:  0x{:08X}", regs.spi_isr().read().bits());
    }
}

impl<'d, T: Instance> Drop for Spi<'d, T> {
    fn drop(&mut self) {
        Self::regs().spi_gcr().modify(|_, w| w.en().clear_bit());
        T::disable_clock();
    }
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance {
    fn regs() -> *const pac::spi0::RegisterBlock;
    fn enable_clock();
    fn disable_clock();
    fn assert_reset();
    fn deassert_reset();
}

/// SPI peripheral instance
#[allow(private_bounds)]
pub trait Instance: SealedInstance + embassy_hal_internal::PeripheralType + 'static {}

impl SealedInstance for crate::peripherals::SPI0 {
    fn regs() -> *const pac::spi0::RegisterBlock {
        pac::Spi0::ptr()
    }
    fn enable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi0_gating().set_bit());
    }
    fn disable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi0_gating().clear_bit());
    }
    fn assert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi0_rst().clear_bit());
    }
    fn deassert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi0_rst().set_bit());
    }
}
impl Instance for crate::peripherals::SPI0 {}

impl SealedInstance for crate::peripherals::SPI1 {
    fn regs() -> *const pac::spi0::RegisterBlock {
        pac::Spi1::ptr()
    }
    fn enable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi1_gating().set_bit());
    }
    fn disable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi1_gating().clear_bit());
    }
    fn assert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi1_rst().clear_bit());
    }
    fn deassert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi1_rst().set_bit());
    }
}
impl Instance for crate::peripherals::SPI1 {}

// ============================================================================
// Pin traits
// ============================================================================

fn into_af_pin<T: gpio::Pin>(pin: &T) {
    let af = pin_af_for_spi(pin.port(), pin.pin());
    pin.set_mode(af);
    pin.set_pull(Pull::None);
}

fn pin_af_for_spi(port: u8, pin: u8) -> PinMode {
    match (port, pin) {
        (2, 0..=3) => PinMode::Func2,  // SPI0 on Port C
        (0, 0..=3) => PinMode::Func5,  // SPI1 on Port A
        (4, 7..=10) => PinMode::Func4, // SPI1 on Port E (alt)
        _ => PinMode::Disabled,
    }
}

mod sealed {
    pub trait SckPin<T> {}
    pub trait MosiPin<T> {}
    pub trait MisoPin<T> {}
    pub trait CsPin<T> {}
}

#[allow(private_bounds)]
pub trait SckPin<T: Instance>: sealed::SckPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait MosiPin<T: Instance>: sealed::MosiPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait MisoPin<T: Instance>: sealed::MisoPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait CsPin<T: Instance>: sealed::CsPin<T> + gpio::Pin {}

// SPI0: PC0=CLK, PC1=CS, PC2=MISO, PC3=MOSI
impl sealed::SckPin<crate::peripherals::SPI0> for crate::peripherals::PC0 {}
impl SckPin<crate::peripherals::SPI0> for crate::peripherals::PC0 {}
impl sealed::CsPin<crate::peripherals::SPI0> for crate::peripherals::PC1 {}
impl CsPin<crate::peripherals::SPI0> for crate::peripherals::PC1 {}
impl sealed::MisoPin<crate::peripherals::SPI0> for crate::peripherals::PC2 {}
impl MisoPin<crate::peripherals::SPI0> for crate::peripherals::PC2 {}
impl sealed::MosiPin<crate::peripherals::SPI0> for crate::peripherals::PC3 {}
impl MosiPin<crate::peripherals::SPI0> for crate::peripherals::PC3 {}

// SPI1: PA2=CLK, PA0=CS, PA3=MISO, PA1=MOSI
impl sealed::SckPin<crate::peripherals::SPI1> for crate::peripherals::PA2 {}
impl SckPin<crate::peripherals::SPI1> for crate::peripherals::PA2 {}
impl sealed::CsPin<crate::peripherals::SPI1> for crate::peripherals::PA0 {}
impl CsPin<crate::peripherals::SPI1> for crate::peripherals::PA0 {}
impl sealed::MisoPin<crate::peripherals::SPI1> for crate::peripherals::PA3 {}
impl MisoPin<crate::peripherals::SPI1> for crate::peripherals::PA3 {}
impl sealed::MosiPin<crate::peripherals::SPI1> for crate::peripherals::PA1 {}
impl MosiPin<crate::peripherals::SPI1> for crate::peripherals::PA1 {}
