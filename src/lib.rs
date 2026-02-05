#![no_std]
#![allow(static_mut_refs, unexpected_cfgs)]

pub use arm9; // 确保 arm9 crate 被链接（提供 critical_section 实现）
pub(crate) use embassy_hal_internal::{impl_peripheral, peripherals_definition, peripherals_struct};
pub use embassy_hal_internal::{Peri, PeripheralType};
pub use f1c100s_pac as pac;

// This must go FIRST so that all the other modules see its macros.
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

pub(crate) mod internal;

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

pub mod usart;

pub use crate::_generated::{peripherals, Peripherals};

pub mod gpio;

// This must go last, so that it sees all the impl_foo! macros defined earlier.
pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

pub struct Config {
    // TODO: add CCU config
}

impl Default for Config {
    fn default() -> Self {
        Self {}
    }
}

/// Initialize the HAL with the provided configuration.
///
/// This returns the peripheral singletons that can be used for creating drivers.
///
/// This should only be called once at startup, otherwise it panics.
pub fn init(_config: Config) -> Peripherals {

    let p = Peripherals::take();

    unsafe {
        crate::_generated::init_gpio();
    }

    // Initialize Embassy time driver (AVS counter)
    unsafe {
        crate::embassy::init();
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
    };
}
