//! Embassy framework support for F1C100S.
//!
//! This module provides the time driver for the Embassy framework.

mod time_driver;

/// Initialize the Embassy time driver.
///
/// System global clocks must be initialized before calling this function.
///
/// # Safety
///
/// This function should be called only once.
pub unsafe fn init() {
    critical_section::with(|cs| time_driver::init(cs));
}
