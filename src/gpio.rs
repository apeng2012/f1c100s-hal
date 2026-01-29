//! GPIO driver for F1C100S
//!
//! F1C100S has 6 GPIO ports:
//! - PA: 4 pins (PA0-PA3)
//! - PB: 4 pins (PB0-PB3)
//! - PC: 4 pins (PC0-PC3)
//! - PD: 22 pins (PD0-PD21)
//! - PE: 13 pins (PE0-PE12)
//! - PF: 6 pins (PF0-PF5)
//!
//! Register layout per port (offset = port_num * 0x24):
//! - CFG0: 0x00 (pins 0-7 config, 4 bits each)
//! - CFG1: 0x04 (pins 8-15 config)
//! - CFG2: 0x08 (pins 16-23 config)
//! - CFG3: 0x0C (pins 24-31 config)
//! - DATA: 0x10
//! - DRV0: 0x14 (drive strength pins 0-15, 2 bits each)
//! - DRV1: 0x18 (drive strength pins 16-31)
//! - PUL0: 0x1C (pull config pins 0-15, 2 bits each)
//! - PUL1: 0x20 (pull config pins 16-31)

use core::convert::Infallible;

use embassy_hal_internal::PeripheralType;

use crate::{impl_peripheral, peripherals, Peri};

/// PIO base address
const PIO_BASE: usize = 0x01C2_0800;

/// GPIO pin mode (3 bits in CFG register)
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum PinMode {
    Input = 0,
    Output = 1,
    Func2 = 2,
    Func3 = 3,
    Func4 = 4,
    Func5 = 5,
    Func6 = 6,
    Disabled = 7,
}

/// Pull setting for a pin (2 bits in PUL register)
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
#[repr(u8)]
pub enum Pull {
    #[default]
    None = 0,
    Up = 1,
    Down = 2,
}

/// Drive strength level (2 bits in DRV register)
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
#[repr(u8)]
pub enum DriveStrength {
    Level0 = 0,
    #[default]
    Level1 = 1,
    Level2 = 2,
    Level3 = 3,
}

/// Logic level
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Level {
    Low,
    High,
}

impl From<bool> for Level {
    fn from(val: bool) -> Self {
        if val {
            Self::High
        } else {
            Self::Low
        }
    }
}

impl From<Level> for bool {
    fn from(level: Level) -> bool {
        matches!(level, Level::High)
    }
}

impl Default for Level {
    fn default() -> Self {
        Self::Low
    }
}

/// Flexible GPIO pin that can be configured as input or output
pub struct Flex<'d> {
    pin: Peri<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    #[inline]
    pub fn new<P: Pin + Into<AnyPin>>(pin: Peri<'d, P>) -> Self {
        Self { pin: pin.into() }
    }

    #[inline]
    pub fn set_as_input(&mut self, pull: Pull) {
        self.pin.set_mode(PinMode::Input);
        self.pin.set_pull(pull);
    }

    #[inline]
    pub fn set_as_output(&mut self, drive: DriveStrength) {
        self.pin.set_mode(PinMode::Output);
        self.pin.set_drive(drive);
    }

    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.read_data()
    }

    #[inline]
    pub fn is_low(&self) -> bool {
        !self.is_high()
    }

    #[inline]
    pub fn get_level(&self) -> Level {
        self.is_high().into()
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.write_data(true);
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.write_data(false);
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::Low => self.set_low(),
            Level::High => self.set_high(),
        }
    }

    #[inline]
    pub fn toggle(&mut self) {
        if self.is_low() {
            self.set_high()
        } else {
            self.set_low()
        }
    }
}

/// Input pin
pub struct Input<'d> {
    pin: Flex<'d>,
}

impl<'d> Input<'d> {
    #[inline]
    pub fn new<P: Pin + Into<AnyPin>>(pin: Peri<'d, P>, pull: Pull) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        Self { pin }
    }

    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    #[inline]
    pub fn get_level(&self) -> Level {
        self.pin.get_level()
    }
}

/// Output pin
pub struct Output<'d> {
    pin: Flex<'d>,
}

impl<'d> Output<'d> {
    #[inline]
    pub fn new<P: Pin + Into<AnyPin>>(pin: Peri<'d, P>, initial_output: Level, drive: DriveStrength) -> Self {
        let mut pin = Flex::new(pin);
        match initial_output {
            Level::High => pin.set_high(),
            Level::Low => pin.set_low(),
        }
        pin.set_as_output(drive);
        Self { pin }
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        self.pin.set_level(level)
    }

    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_high()
    }

    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.is_low()
    }

    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.pin.get_level()
    }

    #[inline]
    pub fn toggle(&mut self) {
        self.pin.toggle();
    }
}

// ============ Low-level pin trait ============

pub(crate) trait SealedPin {
    fn pin_port(&self) -> u8;

    #[inline]
    fn _pin(&self) -> u8 {
        self.pin_port() & 0x1F
    }

    #[inline]
    fn _port(&self) -> u8 {
        self.pin_port() >> 5
    }

    /// Get port base address
    #[inline]
    fn port_base(&self) -> usize {
        PIO_BASE + (self._port() as usize) * 0x24
    }

    /// Set pin mode (CFG register, 4 bits per pin)
    fn set_mode(&self, mode: PinMode) {
        let pin = self._pin() as usize;
        let cfg_reg = pin / 8; // CFG0-CFG3
        let cfg_offset = (pin % 8) * 4;
        let cfg_addr = self.port_base() + cfg_reg * 4;

        unsafe {
            let reg = cfg_addr as *mut u32;
            let val = reg.read_volatile();
            let mask = !(0x7 << cfg_offset);
            let new_val = (val & mask) | ((mode as u32) << cfg_offset);
            reg.write_volatile(new_val);
        }
    }

    /// Set pull configuration (PUL register, 2 bits per pin)
    fn set_pull(&self, pull: Pull) {
        let pin = self._pin() as usize;
        let pull_reg = pin / 16; // PUL0 or PUL1
        let pull_offset = (pin % 16) * 2;
        let pull_addr = self.port_base() + 0x1C + pull_reg * 4;

        unsafe {
            let reg = pull_addr as *mut u32;
            let val = reg.read_volatile();
            let mask = !(0x3 << pull_offset);
            let new_val = (val & mask) | ((pull as u32) << pull_offset);
            reg.write_volatile(new_val);
        }
    }

    /// Set drive strength (DRV register, 2 bits per pin)
    fn set_drive(&self, drive: DriveStrength) {
        let pin = self._pin() as usize;
        let drv_reg = pin / 16; // DRV0 or DRV1
        let drv_offset = (pin % 16) * 2;
        let drv_addr = self.port_base() + 0x14 + drv_reg * 4;

        unsafe {
            let reg = drv_addr as *mut u32;
            let val = reg.read_volatile();
            let mask = !(0x3 << drv_offset);
            let new_val = (val & mask) | ((drive as u32) << drv_offset);
            reg.write_volatile(new_val);
        }
    }

    /// Read pin data (DATA register)
    fn read_data(&self) -> bool {
        let pin = self._pin() as usize;
        let data_addr = self.port_base() + 0x10;

        unsafe {
            let reg = data_addr as *const u32;
            (reg.read_volatile() >> pin) & 1 != 0
        }
    }

    /// Write pin data (DATA register)
    fn write_data(&self, high: bool) {
        let pin = self._pin() as usize;
        let data_addr = self.port_base() + 0x10;

        unsafe {
            let reg = data_addr as *mut u32;
            let val = reg.read_volatile();
            let new_val = if high { val | (1 << pin) } else { val & !(1 << pin) };
            reg.write_volatile(new_val);
        }
    }
}

/// GPIO Pin trait
#[allow(private_bounds)]
pub trait Pin: PeripheralType + SealedPin + Sized + 'static {
    #[inline]
    fn pin(&self) -> u8 {
        self._pin()
    }

    #[inline]
    fn port(&self) -> u8 {
        self._port()
    }

    #[inline]
    fn degrade(self) -> AnyPin {
        AnyPin {
            pin_port: self.pin_port(),
        }
    }
}

/// Type-erased GPIO pin
pub struct AnyPin {
    pin_port: u8,
}

impl AnyPin {
    #[inline]
    pub unsafe fn steal(pin_port: u8) -> Self {
        Self { pin_port }
    }
}

impl_peripheral!(AnyPin);

impl Pin for AnyPin {}

impl SealedPin for AnyPin {
    #[inline]
    fn pin_port(&self) -> u8 {
        self.pin_port
    }
}

// ============ Generate pin implementations ============

foreach_pin!(
    ($pin_name:ident, $port_name:ident, $port_num:expr, $pin_num:expr) => {
        impl Pin for peripherals::$pin_name {}

        impl SealedPin for peripherals::$pin_name {
            #[inline]
            fn pin_port(&self) -> u8 {
                ($port_num << 5) | $pin_num
            }
        }

        impl From<peripherals::$pin_name> for AnyPin {
            fn from(x: peripherals::$pin_name) -> Self {
                x.degrade()
            }
        }
    };
);

// ============ embedded-hal implementations ============

impl<'d> embedded_hal::digital::ErrorType for Input<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Input<'d> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Output<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::OutputPin for Output<'d> {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.set_high();
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.set_low();
        Ok(())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Output<'d> {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Flex<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Flex<'d> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::OutputPin for Flex<'d> {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.set_high();
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.set_low();
        Ok(())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Flex<'d> {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.is_high())
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.is_low())
    }
}
