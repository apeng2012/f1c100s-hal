#![no_std]
#![allow(static_mut_refs, unexpected_cfgs)]

pub use arm9; // 确保 arm9 crate 被链接（提供 critical_section 实现）
pub(crate) use embassy_hal_internal::{impl_peripheral, peripherals_definition, peripherals_struct};
pub use embassy_hal_internal::{Peri, PeripheralType};
pub use f1c100s_pac as pac;

// This must go FIRST so that all the other modules see its macros.
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

mod macros;

pub mod time;

/// Operating modes for peripherals.
pub mod mode {
    trait SealedMode {}

    /// Operating mode for a peripheral.
    #[allow(private_bounds)]
    pub trait Mode: SealedMode {}

    macro_rules! impl_mode {
        ($name:ident) => {
            impl SealedMode for $name {}
            impl Mode for $name {}
        };
    }

    /// Blocking mode.
    pub struct Blocking;
    /// Async mode.
    pub struct Async;

    impl_mode!(Blocking);
    impl_mode!(Async);
}

pub mod prelude;

pub mod embassy;

pub mod debug;

pub mod rcc;

pub mod dram;

pub mod intc;

pub mod interrupt;

pub mod exti;

pub use crate::_generated::{peripherals, Peripherals};

pub mod gpio;

pub mod spi;

// This must go last, so that it sees all the impl_foo! macros defined earlier.
pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

pub struct Config {
    pub rcc: rcc::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rcc: rcc::Config::default(),
        }
    }
}

/// Initialize the HAL with the provided configuration.
///
/// This returns the peripheral singletons that can be used for creating drivers.
///
/// This should only be called once at startup, otherwise it panics.
pub fn init(config: Config) -> Peripherals {
    // Initialize clock tree (CCU)
    unsafe {
        rcc::init(config.rcc);
    }

    // Initialize debug UART (must be after clock init for correct baud rate)
    debug::DebugPrint::enable();

    let p = Peripherals::take();

    unsafe {
        crate::_generated::init_gpio();
    }

    // Initialize INTC (must be before any interrupt users)
    unsafe {
        intc::init();
    }

    // Copy vector table to 0x00000000 so ARM9 exception vectors work.
    // The linker places __vector_table after the boot header (0x30+),
    // but ARM9 always fetches exceptions from 0x00000000.
    unsafe {
        extern "C" {
            static __vector_table: u32;
        }
        let src = &__vector_table as *const u32;
        let dst = 0x0000_0000 as *mut u32;
        // Vector table is 8 entries (ldr pc, xxx) + 8 addresses = 64 bytes = 16 words
        for i in 0..16 {
            dst.add(i).write_volatile(src.add(i).read_volatile());
        }
    }

    // Initialize EXTI (GPIO external interrupts)
    unsafe {
        exti::init();
    }

    // Initialize Embassy time driver (AVS counter)
    unsafe {
        crate::embassy::init();
    }

    // Enable ARM9 IRQ (clear I bit in CPSR)
    unsafe {
        arm9::interrupt::enable();
    }

    p
}

// Note: disable_mmu_cache function was removed - it caused Timer issues
// FEL mode behavior differs from Flash boot, use Flash mode for development

#[macro_export]
macro_rules! bind_interrupts {
    ($vis:vis struct $name:ident { $($irq:ident => $($handler:ty),*;)* }) => {
        #[derive(Copy, Clone)]
        $vis struct $name;

        $(
            $(
                unsafe impl $crate::interrupt::typelevel::Binding<$crate::interrupt::typelevel::$irq, $handler> for $name {}
            )*
        )*

        // Register all handlers at link time via a constructor-like init function.
        // The user must call the generated `_bind_interrupts_init` or rely on HAL init.
        impl $name {
            /// Register all bound interrupt handlers into the INTC dispatch table.
            ///
            /// # Safety
            /// Must be called after INTC init and before interrupts are enabled.
            #[allow(unused)]
            pub unsafe fn init() {
                $(
                    $crate::intc::set_irq_handler(
                        $crate::interrupt::Interrupt::$irq.number(),
                        || {
                            $(
                                <$handler as $crate::interrupt::typelevel::Handler<$crate::interrupt::typelevel::$irq>>::on_interrupt();
                            )*
                        },
                    );
                    $crate::intc::enable_irq($crate::interrupt::Interrupt::$irq.number());
                )*
            }
        }
    };
}
