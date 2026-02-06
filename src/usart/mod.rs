//! UART - Universal Asynchronous Receiver Transmitter for F1C100S/F1C200S
//!
//! F1C100S has 3 UART controllers (UART0, UART1, UART2), compatible with 16550.
//!
//! Features:
//! - 64-byte TX and RX FIFOs
//! - Programmable baud rate
//! - 5-8 data bits, 1/1.5/2 stop bits
//! - Odd/Even/No parity

use core::marker::PhantomData;

use f1c100s_pac::{uart, Ccu, Pio};

use crate::gpio::PinMode;
use crate::time::Hertz;

/// UART data bits
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum DataBits {
    DataBits5 = 0,
    DataBits6 = 1,
    DataBits7 = 2,
    #[default]
    DataBits8 = 3,
}

/// UART parity
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

/// UART stop bits
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StopBits {
    #[default]
    Stop1,
    Stop1p5, // Only valid when DataBits5
    Stop2,
}

/// UART configuration
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub baudrate: u32,
    pub data_bits: DataBits,
    pub stop_bits: StopBits,
    pub parity: Parity,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            baudrate: 115200,
            data_bits: DataBits::DataBits8,
            stop_bits: StopBits::Stop1,
            parity: Parity::None,
        }
    }
}

impl Config {
    /// Create a new config with specified baudrate
    pub fn with_baudrate(baudrate: u32) -> Self {
        Self {
            baudrate,
            ..Self::default()
        }
    }
}

/// UART error
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Error {
    /// Framing error
    Framing,
    /// Parity error  
    Parity,
    /// RX buffer overrun
    Overrun,
    /// Break detected
    Break,
}

/// Configuration error
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    BaudrateTooLow,
    BaudrateTooHigh,
}

// ============ Blocking UART Driver ============

/// Blocking UART Driver
pub struct Uart<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> Uart<T> {
    /// Create a new UART instance with default pin configuration
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        T::enable_and_reset();
        T::configure_pins();
        configure::<T>(&config)?;

        Ok(Self { _phantom: PhantomData })
    }

    /// Perform a blocking write
    pub fn blocking_write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        let regs = T::regs();
        for &c in buffer {
            // Wait for TX holding register empty
            while !regs.lsr().read().thre().bit() {}
            regs.thr().write(|w| unsafe { w.data().bits(c) });
        }
        Ok(())
    }

    /// Block until transmission complete
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        let regs = T::regs();
        // Wait for transmitter empty
        while !regs.lsr().read().temt().bit() {}
        Ok(())
    }

    /// Check for RX errors and return data ready status
    fn check_rx_flags(&self) -> Result<bool, Error> {
        let regs = T::regs();
        let lsr = regs.lsr().read();

        if lsr.oe().bit() {
            return Err(Error::Overrun);
        }
        if lsr.pe().bit() {
            return Err(Error::Parity);
        }
        if lsr.fe().bit() {
            return Err(Error::Framing);
        }
        if lsr.bi().bit() {
            return Err(Error::Break);
        }

        Ok(lsr.dr().bit())
    }

    /// Try to read a single byte (non-blocking)
    /// Returns Ok(Some(byte)) if data available, Ok(None) if no data
    pub fn try_read(&mut self) -> Result<Option<u8>, Error> {
        let regs = T::regs();
        if self.check_rx_flags()? {
            Ok(Some(regs.rbr().read().data().bits()))
        } else {
            Ok(None)
        }
    }

    /// Perform a blocking read into buffer
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        let regs = T::regs();
        for b in buffer {
            while !self.check_rx_flags()? {}
            *b = regs.rbr().read().data().bits();
        }
        Ok(())
    }

    /// Write a single byte (blocking)
    pub fn write_byte(&mut self, byte: u8) {
        let regs = T::regs();
        while !regs.lsr().read().thre().bit() {}
        regs.thr().write(|w| unsafe { w.data().bits(byte) });
    }

    /// Read a single byte (blocking)
    pub fn read_byte(&mut self) -> Result<u8, Error> {
        let regs = T::regs();
        while !self.check_rx_flags()? {}
        Ok(regs.rbr().read().data().bits())
    }
}

impl<T: Instance> Drop for Uart<T> {
    fn drop(&mut self) {
        T::disable();
    }
}

// ============ Configuration ============

fn configure<T: Instance>(config: &Config) -> Result<(), ConfigError> {
    let regs = T::regs();

    // Wait for UART not busy
    while regs.usr().read().busy().bit() {}

    // Disable all interrupts
    regs.ier().write(|w| unsafe { w.bits(0) });

    // Set DLAB to access divisor registers
    regs.lcr().write(|w| w.dlab().set_bit());

    // Calculate divisor
    // baud_rate = apb_clk / (16 * divisor)
    let apb_clk = T::frequency().0;
    let divisor = apb_clk / (16 * config.baudrate);

    if divisor == 0 {
        return Err(ConfigError::BaudrateTooHigh);
    }
    if divisor > 0xFFFF {
        return Err(ConfigError::BaudrateTooLow);
    }

    // Set divisor
    regs.dll().write(|w| unsafe { w.dll().bits((divisor & 0xFF) as u8) });
    regs.dlh()
        .write(|w| unsafe { w.dlh().bits(((divisor >> 8) & 0xFF) as u8) });

    // Configure line control register
    regs.lcr().write(|w| unsafe {
        let mut lcr = w.dls().bits(config.data_bits as u8);

        // Stop bits
        if config.stop_bits != StopBits::Stop1 {
            lcr = lcr.stop().set_bit();
        }

        // Parity
        match config.parity {
            Parity::None => {}
            Parity::Odd => {
                lcr = lcr.pen().set_bit();
            }
            Parity::Even => {
                lcr = lcr.pen().set_bit().eps().bits(1);
            }
        }

        lcr
    });

    // Enable and reset FIFOs
    regs.fcr()
        .write(|w| w.fifoe().set_bit().rfifor().set_bit().xfifor().set_bit());

    // Clear MCR
    regs.mcr().write(|w| unsafe { w.bits(0) });

    Ok(())
}

// ============ core::fmt::Write implementation ============

impl<T: Instance> core::fmt::Write for Uart<T> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.blocking_write(s.as_bytes()).map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

// ============ Instance trait ============

trait SealedInstance {
    fn regs() -> &'static uart::RegisterBlock;
    fn configure_pins();

    fn frequency() -> Hertz {
        // Default APB clock: 6MHz (24MHz / 2 / 2)
        Hertz(6_000_000)
    }

    fn enable_and_reset();
    fn disable();
}

/// UART instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + 'static {}

// ============ Helper to configure pin ============

fn set_pin_mode(port: usize, pin: usize, mode: PinMode) {
    let pio = unsafe { Pio::steal() };

    match port {
        0 => {
            // Port A
            match pin / 8 {
                0 => {
                    pio.pa_cfg0().modify(|r, w| unsafe {
                        let shift = (pin % 8) * 4;
                        let mask = !(0x07u32 << shift);
                        w.bits((r.bits() & mask) | ((mode as u32) << shift))
                    });
                }
                1 => {
                    pio.pa_cfg1().modify(|r, w| unsafe {
                        let shift = (pin % 8) * 4;
                        let mask = !(0x07u32 << shift);
                        w.bits((r.bits() & mask) | ((mode as u32) << shift))
                    });
                }
                _ => {}
            }
        }
        4 => {
            // Port E
            match pin / 8 {
                0 => {
                    pio.pe_cfg0().modify(|r, w| unsafe {
                        let shift = (pin % 8) * 4;
                        let mask = !(0x07u32 << shift);
                        w.bits((r.bits() & mask) | ((mode as u32) << shift))
                    });
                }
                1 => {
                    pio.pe_cfg1().modify(|r, w| unsafe {
                        let shift = (pin % 8) * 4;
                        let mask = !(0x07u32 << shift);
                        w.bits((r.bits() & mask) | ((mode as u32) << shift))
                    });
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ============ UART0 implementation ============
// UART0: TX=PE1, RX=PE0

/// UART0 peripheral
pub struct UART0;

impl SealedInstance for UART0 {
    fn regs() -> &'static uart::RegisterBlock {
        unsafe { &*f1c100s_pac::Uart0::ptr() }
    }

    fn configure_pins() {
        // PE1 = UART0_TX (Func5)
        set_pin_mode(4, 1, PinMode::Func5);
        // PE0 = UART0_RX (Func5)
        set_pin_mode(4, 0, PinMode::Func5);
    }

    fn enable_and_reset() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart0_gating().set_bit());
        ccu.bus_soft_rst2().modify(|_, w| w.uart0_rst().set_bit());
    }

    fn disable() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart0_gating().clear_bit());
    }
}

impl Instance for UART0 {}

// ============ UART1 implementation ============
// UART1: TX=PA3, RX=PA2 (default)

/// UART1 peripheral
pub struct UART1;

impl SealedInstance for UART1 {
    fn regs() -> &'static uart::RegisterBlock {
        unsafe { &*f1c100s_pac::Uart1::ptr() }
    }

    fn configure_pins() {
        // PA3 = UART1_TX (Func5)
        set_pin_mode(0, 3, PinMode::Func5);
        // PA2 = UART1_RX (Func5)
        set_pin_mode(0, 2, PinMode::Func5);
    }

    fn enable_and_reset() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart1_gating().set_bit());
        ccu.bus_soft_rst2().modify(|_, w| w.uart1_rst().set_bit());
    }

    fn disable() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart1_gating().clear_bit());
    }
}

impl Instance for UART1 {}

// ============ UART2 implementation ============
// UART2: TX=PE7, RX=PE8 (default)

/// UART2 peripheral
pub struct UART2;

impl SealedInstance for UART2 {
    fn regs() -> &'static uart::RegisterBlock {
        unsafe { &*f1c100s_pac::Uart2::ptr() }
    }

    fn configure_pins() {
        // PE7 = UART2_TX (Func3)
        set_pin_mode(4, 7, PinMode::Func3);
        // PE8 = UART2_RX (Func3)
        set_pin_mode(4, 8, PinMode::Func3);
    }

    fn enable_and_reset() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart2_gating().set_bit());
        ccu.bus_soft_rst2().modify(|_, w| w.uart2_rst().set_bit());
    }

    fn disable() {
        let ccu = unsafe { Ccu::steal() };
        ccu.bus_clk_gating2().modify(|_, w| w.uart2_gating().clear_bit());
    }
}

impl Instance for UART2 {}

// ============ Type aliases ============

/// UART0 driver
pub type Uart0 = Uart<UART0>;
/// UART1 driver
pub type Uart1 = Uart<UART1>;
/// UART2 driver
pub type Uart2 = Uart<UART2>;
