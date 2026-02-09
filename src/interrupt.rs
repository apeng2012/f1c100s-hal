//! Interrupt definitions and type-level interrupt infrastructure for F1C100S.
//!
//! F1C100S INTC supports 64 interrupt sources. This module provides:
//! - `Interrupt` enum with all IRQ sources
//! - `InterruptExt` trait for enable/disable/pending operations
//! - Type-level interrupt types for compile-time binding checks
//! - `Handler` and `Binding` traits for the `bind_interrupts!` pattern

use core::sync::atomic::{compiler_fence, Ordering};

use crate::intc;

/// F1C100S Interrupt sources
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum Interrupt {
    NMI = 0,
    UART0 = 1,
    UART1 = 2,
    UART2 = 3,
    OWA = 5,
    CIR = 6,
    TWI0 = 7,
    TWI1 = 8,
    TWI2 = 9,
    SPI0 = 10,
    SPI1 = 11,
    TIMER0 = 13,
    TIMER1 = 14,
    TIMER2 = 15,
    WATCHDOG = 16,
    RSB = 17,
    DMA = 18,
    TOUCH_PANEL = 20,
    AUDIO_CODEC = 21,
    KEYADC = 22,
    SDC0 = 23,
    SDC1 = 24,
    USB_OTG = 26,
    TVD = 27,
    TVE = 28,
    TCON = 29,
    DEFE = 30,
    DEBE = 31,
    CSI = 32,
    DE_INTERLACER = 33,
    VE = 34,
    DAUDIO = 35,
    PIOD = 38,
    PIOE = 39,
    PIOF = 40,
}

impl Interrupt {
    /// Get the IRQ number
    pub fn number(self) -> u8 {
        self as u8
    }
}

/// Extension trait for interrupt operations via INTC.
pub trait InterruptExt: Copy {
    /// Get the IRQ number.
    fn number(self) -> u8;

    /// Enable the interrupt in INTC.
    ///
    /// # Safety
    /// Enabling interrupts can cause handlers to execute immediately.
    unsafe fn enable(self) {
        compiler_fence(Ordering::SeqCst);
        intc::enable_irq(self.number());
    }

    /// Disable the interrupt in INTC.
    fn disable(self) {
        intc::disable_irq(self.number());
        compiler_fence(Ordering::SeqCst);
    }

    /// Check if interrupt is enabled in INTC.
    fn is_enabled(self) -> bool {
        intc::is_irq_enabled(self.number())
    }

    /// Set interrupt pending (via fast-forcing).
    fn pend(self) {
        intc::force_irq(self.number());
    }

    /// Clear interrupt pending.
    fn unpend(self) {
        intc::clear_pending(self.number());
    }
}

impl InterruptExt for Interrupt {
    fn number(self) -> u8 {
        self as u8
    }
}

/// Priority level (F1C100S INTC supports 4 levels)
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Priority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
}

/// Type-level interrupt infrastructure.
///
/// This module contains one *type* per interrupt. This is used for checking at compile time that
/// the interrupts are correctly bound to HAL drivers.
pub mod typelevel {
    use super::*;

    trait SealedInterrupt {}

    /// Type-level interrupt.
    #[allow(private_bounds)]
    pub trait Interrupt: SealedInterrupt {
        /// Interrupt enum variant.
        const IRQ: super::Interrupt;

        /// Enable the interrupt.
        #[inline]
        unsafe fn enable() {
            Self::IRQ.enable()
        }

        /// Disable the interrupt.
        #[inline]
        fn disable() {
            Self::IRQ.disable()
        }

        /// Check if interrupt is enabled.
        #[inline]
        fn is_enabled() -> bool {
            Self::IRQ.is_enabled()
        }
    }

    /// Interrupt handler trait.
    ///
    /// Drivers that need to handle interrupts implement this trait.
    pub trait Handler<I: Interrupt> {
        /// Interrupt handler function.
        ///
        /// # Safety
        /// Must ONLY be called from the interrupt handler for `I`.
        unsafe fn on_interrupt();
    }

    /// Compile-time assertion that an interrupt has been bound to a handler.
    ///
    /// # Safety
    /// By implementing this trait, you assert that `H::on_interrupt()` will be called
    /// every time the `I` interrupt fires.
    pub unsafe trait Binding<I: Interrupt, H: Handler<I>> {}

    // Generate typelevel types for each interrupt
    macro_rules! impl_irq_typelevel {
        ($name:ident) => {
            #[allow(non_camel_case_types)]
            #[doc = stringify!($name)]
            #[doc = " typelevel interrupt."]
            pub enum $name {}
            impl SealedInterrupt for $name {}
            impl Interrupt for $name {
                const IRQ: super::Interrupt = super::Interrupt::$name;
            }
        };
    }

    impl_irq_typelevel!(NMI);
    impl_irq_typelevel!(UART0);
    impl_irq_typelevel!(UART1);
    impl_irq_typelevel!(UART2);
    impl_irq_typelevel!(OWA);
    impl_irq_typelevel!(CIR);
    impl_irq_typelevel!(TWI0);
    impl_irq_typelevel!(TWI1);
    impl_irq_typelevel!(TWI2);
    impl_irq_typelevel!(SPI0);
    impl_irq_typelevel!(SPI1);
    impl_irq_typelevel!(TIMER0);
    impl_irq_typelevel!(TIMER1);
    impl_irq_typelevel!(TIMER2);
    impl_irq_typelevel!(WATCHDOG);
    impl_irq_typelevel!(RSB);
    impl_irq_typelevel!(DMA);
    impl_irq_typelevel!(TOUCH_PANEL);
    impl_irq_typelevel!(AUDIO_CODEC);
    impl_irq_typelevel!(KEYADC);
    impl_irq_typelevel!(SDC0);
    impl_irq_typelevel!(SDC1);
    impl_irq_typelevel!(USB_OTG);
    impl_irq_typelevel!(TVD);
    impl_irq_typelevel!(TVE);
    impl_irq_typelevel!(TCON);
    impl_irq_typelevel!(DEFE);
    impl_irq_typelevel!(DEBE);
    impl_irq_typelevel!(CSI);
    impl_irq_typelevel!(DE_INTERLACER);
    impl_irq_typelevel!(VE);
    impl_irq_typelevel!(DAUDIO);
    impl_irq_typelevel!(PIOD);
    impl_irq_typelevel!(PIOE);
    impl_irq_typelevel!(PIOF);
}
