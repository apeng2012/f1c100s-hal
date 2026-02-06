//! Debug output module for F1C100S.
//!
//! Provides `print!` and `println!` macros for debug output via UART.
//!
//! # Features
//! - `debug-uart0` - Use UART0 (PE1=TX, PE0=RX) - default
//! - `debug-uart1` - Use UART1 (PA3=TX, PA2=RX)
//! - `debug-uart2` - Use UART2 (PE7=TX, PE8=RX)
//!
//! If no debug feature is enabled, print!/println! are no-ops.
//!
//! Default baudrate: 115200 @ 6MHz APB clock

use core::fmt::{self, Write};

#[cfg(feature = "debug-uart0")]
use f1c100s_pac::Uart0 as DebugUart;
#[cfg(feature = "debug-uart1")]
use f1c100s_pac::Uart1 as DebugUart;
#[cfg(feature = "debug-uart2")]
use f1c100s_pac::Uart2 as DebugUart;
use f1c100s_pac::{Ccu, Pio};

/// Debug print output using UART
pub struct DebugPrint;

impl DebugPrint {
    /// Initialize UART for debug output (115200 baud, 8N1)
    ///
    /// Must be called after clock init so APB frequency is known.
    #[cfg(feature = "_debug-output")]
    pub fn enable() {
        let ccu = unsafe { Ccu::steal() };
        let pio = unsafe { Pio::steal() };
        let uart = unsafe { DebugUart::steal() };

        // Enable UART clock gate and de-assert reset
        #[cfg(feature = "debug-uart0")]
        {
            ccu.bus_clk_gating2().modify(|_, w| w.uart0_gating().set_bit());
            ccu.bus_soft_rst2().modify(|_, w| w.uart0_rst().set_bit());
        }
        #[cfg(feature = "debug-uart1")]
        {
            ccu.bus_clk_gating2().modify(|_, w| w.uart1_gating().set_bit());
            ccu.bus_soft_rst2().modify(|_, w| w.uart1_rst().set_bit());
        }
        #[cfg(feature = "debug-uart2")]
        {
            ccu.bus_clk_gating2().modify(|_, w| w.uart2_gating().set_bit());
            ccu.bus_soft_rst2().modify(|_, w| w.uart2_rst().set_bit());
        }

        // Configure GPIO pins
        #[cfg(feature = "debug-uart0")]
        {
            // PE0 (RX) and PE1 (TX) as UART0 function (Func 5)
            pio.pe_cfg0()
                .modify(|_, w| unsafe { w.pe0_select().bits(5).pe1_select().bits(5) });
        }
        #[cfg(feature = "debug-uart1")]
        {
            // PA2 (RX) and PA3 (TX) as UART1 function (Func 5)
            pio.pa_cfg0()
                .modify(|_, w| unsafe { w.pa2_select().bits(5).pa3_select().bits(5) });
        }
        #[cfg(feature = "debug-uart2")]
        {
            // PE7 (TX) and PE8 (RX) as UART2 function (Func 3)
            pio.pe_cfg0().modify(|_, w| unsafe { w.pe7_select().bits(3) });
            pio.pe_cfg1().modify(|_, w| unsafe { w.pe8_select().bits(3) });
        }

        // Configure UART: 115200 baud, 8N1
        // Following the C reference sys_uart_init() sequence:

        // Disable all interrupts
        uart.ier().write(|w| unsafe { w.bits(0) });
        // Reset and enable FIFO (RCVR trigger = 14 bytes)
        uart.fcr().write(|w| unsafe { w.bits(0xf7) });
        // Clear MCR
        uart.mcr().write(|w| unsafe { w.bits(0) });

        // Divisor = APB_CLK / (16 * 115200)
        let apb_hz = crate::rcc::clocks().pclk.0;
        let divisor = (apb_hz + 8 * 115200) / (16 * 115200);

        // Set DLAB to access divisor latch
        uart.lcr().modify(|r, w| unsafe { w.bits(r.bits() | (1 << 7)) });

        // Write divisor
        uart.dll().write(|w| unsafe { w.dll().bits(divisor as u8) });
        uart.dlh().write(|w| unsafe { w.dlh().bits((divisor >> 8) as u8) });

        // Clear DLAB
        uart.lcr().modify(|r, w| unsafe { w.bits(r.bits() & !(1 << 7)) });

        // Set 8N1: 8 data bits (DLS=3), no parity, 1 stop bit
        uart.lcr().modify(|r, w| unsafe {
            let v = (r.bits() & !0x1f) | 0x03; // DLS=3, STOP=0, PEN=0
            w.bits(v)
        });
    }

    /// No-op when debug output is disabled
    #[cfg(not(feature = "_debug-output"))]
    pub fn enable() {}

    /// Write a single byte
    #[cfg(feature = "_debug-output")]
    #[inline]
    fn write_byte(byte: u8) {
        let uart = unsafe { DebugUart::steal() };
        // Wait for TX FIFO not full (THRE bit)
        while !uart.lsr().read().thre().bit() {}
        uart.thr().write(|w| unsafe { w.data().bits(byte) });
    }
}

#[cfg(feature = "_debug-output")]
impl Write for DebugPrint {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                DebugPrint::write_byte(b'\r');
            }
            DebugPrint::write_byte(byte);
        }
        Ok(())
    }
}

#[cfg(not(feature = "_debug-output"))]
impl Write for DebugPrint {
    fn write_str(&mut self, _s: &str) -> fmt::Result {
        Ok(())
    }
}

/// Print to UART debug output
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            let _ = write!(&mut $crate::debug::DebugPrint, $($arg)*);
        }
    }
}

/// Print with newline to UART debug output
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            let _ = writeln!(&mut $crate::debug::DebugPrint, $($arg)*);
        }
    }
}
